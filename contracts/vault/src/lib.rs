//! Custody for subscriber funds.
//!
//! A pull-payment scheme fails the moment a subscriber's wallet is empty at
//! charge time. The vault decouples funding from billing: subscribers top up
//! ahead of time, and the subscription contract debits from a balance that is
//! already committed. Custody is deliberately separated from billing logic so
//! the two can be audited independently.
//!
//! ## Resource costs
//!
//! All storage TTLs are expressed in ledgers (~5s each). Instance storage
//! (configuration) uses a 30-day bump with a 29-day threshold. Persistent
//! storage (balances, allowlist, caps, spend limits) uses a 90-day bump with
//! an 89-day threshold.
//!
//! | Entry point | Instance reads | Instance writes | Persistent reads | Persistent writes | Notes |
//! |---|---|---|---|---|---|
//! | `__constructor` | 0 | 2 (Admin, ReserveCapBps) | 0 | 0 | Extends instance TTL |
//! | `set_subscription` | 1 (Admin) | 1 (Subscription) | 0 | 0 | Admin only, extends instance TTL |
//! | `set_allowlist_enabled` | 1 (Admin) | 1 (AllowlistEnabled) | 0 | 0 | Admin only, extends instance TTL |
//! | `set_token_allowed` | 1 (Admin) | 0 | 0 | 1 (TokenAllowed) | Admin only, extends persistent TTL |
//! | `set_deposit_cap` | 1 (Admin) | 0 | 0 | 1 (DepositCap) | Admin only, extends persistent TTL |
//! | `deposit` | 0 | 0 | 2 (Balance, DepositCap) | 1 (Balance) | Extends persistent TTL; token transfer |
//! | `deposit_for` | 0 | 0 | 2 (Balance, DepositCap) | 1 (Balance) | Extends persistent TTL; token transfer |
//! | `withdraw` | 0 | 0 | 1 (Balance) | 1 (Balance) | Extends persistent TTL; token transfer |
//! | `withdraw_to` | 0 | 0 | 1 (Balance) | 1 (Balance) | Extends persistent TTL; token transfer |
//! | `withdraw_all` | 0 | 0 | 1 (Balance) | 1 (Balance) | Extends persistent TTL; token transfer |
//! | `debit` | 1 (Subscription) | 0 | 2 (Balance, SpendLimit/Window) | 2 (Balance, SpendWindow) | Subscription only, extends persistent TTLs |
//! | `debit_split` | 1 (Subscription) | 0 | 2 (Balance, SpendLimit/Window) | 2 (Balance, SpendWindow) | Subscription only, extends persistent TTLs |
//! | `balance` | 0 | 0 | 1 (Balance) | 0 | |
//! | `allowlist_enabled` | 1 (AllowlistEnabled) | 0 | 0 | 0 | |
//! | `token_allowed` | 0 | 0 | 1 (TokenAllowed) | 0 | |
//! | `can_deposit` | 1 (AllowlistEnabled) | 0 | 1 (TokenAllowed) | 0 | |
//! | `subscription` | 1 (Subscription) | 0 | 0 | 0 | |
//! | `deposit_cap` | 0 | 0 | 1 (DepositCap) | 0 | |
//! | `set_spend_limit` | 0 | 0 | 0 | 1–2 (SpendLimit, SpendWindow) | User auth, extends persistent TTL |
//! | `spend_limit` | 0 | 0 | 1 (SpendLimit) | 0 | |
//! | `emergency_withdraw` | 1 (Admin) | 0 | 1–2 (ReservedTotal, ReservedMandates) | 1+ (clears Reserved keys) | Admin only, releases all reserves |
//! | `propose_admin` | 1 (Admin) | 1 (PendingAdmin) | 0 | 0 | Extends instance TTL |
//! | `accept_admin` | 2 (Admin, PendingAdmin) | 1 (Admin) | 0 | 0 | Removes PendingAdmin; extends instance TTL |
//! | `admin` | 1 (Admin) | 0 | 0 | 0 | |
//! | `upgrade_wasm` | 1 (Admin) | 0 | 0 | 0 | Extends instance TTL |

#![no_std]
#![deny(missing_docs)]
// `deny`, not `forbid`: soroban-sdk's #[contractimpl] expansion carries its
// own #[allow(unsafe_code)], which `forbid` cannot be overridden by. `deny`
// still fails the build on any unsafe block written in this crate.
#![deny(unsafe_code)]

#[allow(missing_docs)]
mod error;
#[allow(missing_docs)]
mod events;
#[allow(missing_docs)]
mod types;

#[cfg(test)]
mod test;
#[cfg(test)]
mod test_errors;
#[cfg(test)]
mod test_events;

pub use error::Error;
pub use events::{
    ContractUpgraded, Debit, Deposit, DepositCapSet, DepositFor, EmergencyWithdraw, ReserveSet,
    Withdraw, WithdrawTo,
};
pub use types::{DataKey, OperationWindow, SpendLimit, SpendWindow, UpgradeRecord};

use soroban_sdk::{
    contract, contractimpl, token, Address, BytesN, ContractExecutable, Env, String,
};

const DAY_IN_LEDGERS: u32 = 17_280;
const INSTANCE_BUMP: u32 = 30 * DAY_IN_LEDGERS;
const INSTANCE_THRESHOLD: u32 = INSTANCE_BUMP - DAY_IN_LEDGERS;
const PERSIST_BUMP: u32 = 90 * DAY_IN_LEDGERS;
const PERSIST_THRESHOLD: u32 = PERSIST_BUMP - DAY_IN_LEDGERS;

/// The all-zero account and contract strkeys (#236). Both are well-formed
/// Stellar addresses, but neither can ever sign or hold a deployed contract,
/// so tokens sent there are irretrievable.
const ZERO_ACCOUNT: &str = "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF";
const ZERO_CONTRACT: &str = "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABSC4";

/// Length of the fixed window that bounds repeated deposit/withdraw cycling
/// (#314). Windows are aligned to the Unix epoch, so every caller sees the
/// same boundaries and a fresh window cannot be opened by waiting less than
/// this long.
pub const VAULT_OPERATION_WINDOW_SECS: u64 = 60;

/// Maximum deposit, `deposit_for`, withdraw and `withdraw_all` calls one
/// (user, token) pair may make in a single operation window (#314). The limit
/// is deliberately generous for normal use: a subscriber can deposit and
/// withdraw several times a minute, but cannot loop the pair indefinitely to
/// rewrite the balance entry and spam events.
pub const MAX_VAULT_OPERATIONS_PER_WINDOW: u32 = 8;

/// Largest share of a subscriber's balance that may be earmarked as reserves,
/// in basis points. A subscriber may reserve for each of their mandates, so
/// without a ceiling many open mandates could lock the whole vault and leave
/// nothing withdrawable (#348). `9_000` keeps 10% free; `10_000` allows the
/// whole balance and `0` forbids reserves.
pub const DEFAULT_RESERVE_CAP_BPS: u32 = 9_000;

/// Basis-point denominator used by the reserve cap.
pub const BPS_DENOMINATOR: u32 = 10_000;

#[allow(missing_docs)]
mod contract {
    use super::{
        contract, contractimpl, token, Address, BytesN, ContractExecutable, ContractUpgraded,
        DataKey, Debit, Deposit, DepositCapSet, DepositFor, EmergencyWithdraw, Env, Error,
        OperationWindow, ReserveSet, SpendLimit, SpendWindow, String, UpgradeRecord, Withdraw,
        WithdrawTo, BPS_DENOMINATOR, DEFAULT_RESERVE_CAP_BPS, INSTANCE_BUMP, INSTANCE_THRESHOLD,
        MAX_VAULT_OPERATIONS_PER_WINDOW, PERSIST_BUMP, PERSIST_THRESHOLD,
        VAULT_OPERATION_WINDOW_SECS, ZERO_ACCOUNT, ZERO_CONTRACT,
    };
    use payflow_common::load_instance_or_err;

    /// Custody for subscriber funds.
    ///
    /// A pull-payment scheme fails the moment a subscriber's wallet is empty at
    /// charge time. The vault decouples funding from billing: subscribers top up
    /// ahead of time, and the subscription contract debits from a balance that is
    /// already committed. Custody is deliberately separated from billing logic so
    /// the two can be audited independently.
    #[contract]
    pub struct Vault;

    #[contractimpl]
    impl Vault {
        /// The version of this contract's crate.
        pub fn version(env: Env) -> String {
            String::from_str(&env, env!("CARGO_PKG_VERSION"))
        }

        /// Runs once, in the same transaction that deploys the contract. With a
        /// separate `initialize` call, anyone watching the ledger could call it
        /// between the deploy and the owner's own call and make themselves admin.
        ///
        /// `reserve_cap_bps` is fixed here, in basis points of a subscriber's
        /// balance: the largest share that may be earmarked as reserves across
        /// all of their mandates (#348). `9_000` keeps 10% free, `0` forbids
        /// reserves entirely and `10_000` allows reserving the whole balance.
        ///
        /// # Errors
        /// Returns [`Error::InvalidReserveCap`] if `reserve_cap_bps > 10_000`.
        pub fn __constructor(env: Env, admin: Address, reserve_cap_bps: u32) -> Result<(), Error> {
            if reserve_cap_bps > BPS_DENOMINATOR {
                return Err(Error::InvalidReserveCap);
            }

            env.storage().instance().set(&DataKey::Admin, &admin);
            env.storage()
                .instance()
                .set(&DataKey::ReserveCapBps, &reserve_cap_bps);
            env.storage()
                .instance()
                .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_BUMP);
            Ok(())
        }

        /// Point the vault at the subscription contract allowed to debit it.
        ///
        /// # Auth
        /// Requires authorization from the contract admin.
        ///
        /// # Events
        /// Emits no events.
        ///
        /// # Errors
        /// Returns [`Error::NotInitialized`] if no admin is configured.
        pub fn set_subscription(env: Env, subscription: Address) -> Result<(), Error> {
            Self::require_admin(&env)?;

            env.storage()
                .instance()
                .set(&DataKey::Subscription, &subscription);
            env.storage()
                .instance()
                .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_BUMP);
            Ok(())
        }

        /// Turn the token allowlist on or off. Off by default, so a deployment
        /// (testnet included) keeps accepting every SEP-41 token until the admin
        /// opts in. Toggling it moves no funds: it only gates future deposits.
        ///
        /// # Auth
        /// Requires authorization from the contract admin.
        ///
        /// # Events
        /// Emits no events.
        ///
        /// # Errors
        /// Returns [`Error::NotInitialized`] if no admin is configured.
        pub fn set_allowlist_enabled(env: Env, enabled: bool) -> Result<(), Error> {
            Self::require_admin(&env)?;

            env.storage()
                .instance()
                .set(&DataKey::AllowlistEnabled, &enabled);
            env.storage()
                .instance()
                .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_BUMP);
            Ok(())
        }

        /// Add or remove one token from the allowlist. Removing a token blocks
        /// new deposits of it; balances already held can still be withdrawn, so
        /// allowlisting can never trap funds.
        ///
        /// # Auth
        /// Requires authorization from the contract admin.
        ///
        /// # Events
        /// Emits no events.
        ///
        /// # Errors
        /// Returns [`Error::NotInitialized`] if no admin is configured.
        pub fn set_token_allowed(env: Env, token: Address, allowed: bool) -> Result<(), Error> {
            Self::require_admin(&env)?;

            let key = DataKey::TokenAllowed(token);
            env.storage().persistent().set(&key, &allowed);
            env.storage()
                .persistent()
                .extend_ttl(&key, PERSIST_THRESHOLD, PERSIST_BUMP);
            Ok(())
        }

        /// Cap how much a subscriber may hold for one token.
        ///
        /// While the protocol is unaudited, the cost of a bug grows with the
        /// amount of subscriber funds the vault can hold, so the admin can cap
        /// the balance a subscriber may keep per token. The cap is per
        /// (user, token): one subscriber's cap never affects another.
        ///
        /// A cap of 0 means unlimited, which is the default and what an
        /// existing deployment runs with until an admin sets a cap.
        ///
        /// The cap only gates new deposits. A balance that is already above a
        /// newly set cap can still be withdrawn, and can still be debited by the
        /// subscription contract: the cap limits how much can be added, and
        /// never traps funds.
        ///
        /// # Auth
        /// Requires authorization from the contract admin.
        ///
        /// # Events
        /// Emits [`DepositCapSet`].
        ///
        /// # Errors
        /// Returns [`Error::NotInitialized`] if no admin is configured, or
        /// [`Error::InvalidAmount`] if `amount < 0`.
        pub fn set_deposit_cap(
            env: Env,
            user: Address,
            token: Address,
            amount: i128,
        ) -> Result<(), Error> {
            Self::require_admin(&env)?;

            if amount < 0 {
                return Err(Error::InvalidAmount);
            }

            let key = DataKey::DepositCap(user.clone(), token.clone());
            env.storage().persistent().set(&key, &amount);
            env.storage()
                .persistent()
                .extend_ttl(&key, PERSIST_THRESHOLD, PERSIST_BUMP);

            DepositCapSet {
                user,
                token,
                cap: amount,
            }
            .publish(&env);
            Ok(())
        }

        /// The deposit cap for (user, token). Zero means unlimited.
        pub fn deposit_cap(env: Env, user: Address, token: Address) -> i128 {
            Self::read_deposit_cap(&env, &user, &token)
        }

        /// Move `amount` of `token` from the subscriber's wallet into the vault.
        ///
        /// # Auth
        /// Requires authorization from `user`.
        ///
        /// # Events
        /// Emits [`Deposit`].
        ///
        /// # Errors
        /// Returns [`Error::InvalidAmount`] if `amount <= 0`, [`Error::TokenNotAllowed`]
        /// if the allowlist is enabled and `token` is not allowed,
        /// [`Error::DepositCapExceeded`] if the deposit would exceed the
        /// subscriber's cap, or [`Error::RateLimitExceeded`] if the
        /// (user, token) pair has used its deposit/withdraw budget for this
        /// window (#314).
        pub fn deposit(env: Env, user: Address, token: Address, amount: i128) -> Result<(), Error> {
            user.require_auth();
            if amount <= 0 {
                return Err(Error::InvalidAmount);
            }

            // A malicious token contract can emit misleading Deposit events that
            // indexers and UIs show as real balances. When the allowlist is on,
            // only tokens the admin vouched for may add such events. Withdrawals
            // are deliberately not checked: allowlisting must not trap funds.
            if Self::allowlist_on(&env) && !Self::token_on(&env, &token) {
                return Err(Error::TokenNotAllowed);
            }

            let new_balance = Self::read_balance(&env, &user, &token)
                .checked_add(amount)
                .ok_or(Error::AmountOverflow)?;
            // The cap is checked before the transfer, so a rejected deposit moves
            // no tokens at all.
            let cap = Self::read_deposit_cap(&env, &user, &token);
            if cap > 0 && new_balance > cap {
                return Err(Error::DepositCapExceeded);
            }

            Self::record_operation(&env, &user, &token)?;

            Self::transfer_in(&env, &token, &user, amount)?;

            Self::write_balance(&env, &user, &token, new_balance);

            Deposit {
                user,
                token,
                amount,
                balance: new_balance,
            }
            .publish(&env);
            Ok(())
        }

        /// Credit `user`'s balance with tokens paid by a different account
        /// `from` — third-party funding (merged from #136). `from` authorizes
        /// and pays; `user` receives the spendable balance.
        ///
        /// Applies the same allowlist, deposit-cap and per-window operation
        /// checks as `deposit`, so funding on someone's behalf cannot bypass
        /// them. (#136 predated those features; enforcing them here keeps
        /// third-party funding consistent with a direct deposit.)
        pub fn deposit_for(
            env: Env,
            from: Address,
            user: Address,
            token: Address,
            amount: i128,
        ) -> Result<(), Error> {
            from.require_auth();
            if amount <= 0 {
                return Err(Error::InvalidAmount);
            }

            if Self::allowlist_on(&env) && !Self::token_on(&env, &token) {
                return Err(Error::TokenNotAllowed);
            }

            let new_balance = Self::read_balance(&env, &user, &token)
                .checked_add(amount)
                .ok_or(Error::AmountOverflow)?;
            let cap = Self::read_deposit_cap(&env, &user, &token);
            if cap > 0 && new_balance > cap {
                return Err(Error::DepositCapExceeded);
            }

            Self::record_operation(&env, &user, &token)?;

            Self::transfer_in(&env, &token, &from, amount)?;

            Self::write_balance(&env, &user, &token, new_balance);

            DepositFor {
                from,
                user,
                token,
                amount,
                balance: new_balance,
            }
            .publish(&env);
            Ok(())
        }

        /// Return uncommitted funds to the subscriber. Always available: the vault
        /// never locks a balance, so a subscriber can exit at any time.
        ///
        /// Only the unreserved part of the balance can be withdrawn. A reserve
        /// is the subscriber's own earmark (#295) and `set_reserve(.., 0)`
        /// releases it at any time, so reserving never traps funds: it only
        /// stops them leaving by accident while a mandate is counting on them.
        ///
        /// # Auth
        /// Requires authorization from `user`.
        ///
        /// # Events
        /// Emits [`Withdraw`].
        ///
        /// # Errors
        /// Returns [`Error::InvalidAmount`] if `amount <= 0`,
        /// [`Error::InsufficientBalance`] if `amount` exceeds the user's
        /// unreserved balance, [`Error::ZeroAddress`] if `user` is the zero
        /// address (#236), or [`Error::RateLimitExceeded`] if the
        /// (user, token) pair has used its deposit/withdraw budget for this
        /// window (#314).
        pub fn withdraw(
            env: Env,
            user: Address,
            token: Address,
            amount: i128,
        ) -> Result<(), Error> {
            user.require_auth();
            Self::require_recipient(&env, &user)?;
            let new_balance = Self::do_withdraw(&env, &user, &token, amount)?;

            Self::transfer_out(&env, &token, &user, amount)?;

            Withdraw {
                user,
                token,
                amount,
                balance: new_balance,
            }
            .publish(&env);
            Ok(())
        }

        /// Return uncommitted funds to a destination chosen by the subscriber.
        ///
        /// Identical to [`Vault::withdraw`] except that the tokens are sent to
        /// `to` instead of the caller. Only `user` authorizes: `to` is a
        /// destination, not a second signer, so funds can be swept to a
        /// custody, hardware or treasury address without giving that address
        /// authority over the vault.
        ///
        /// Only the unreserved part of the balance can be withdrawn, exactly as
        /// with `withdraw`. A destination may be `user` itself, in which case
        /// this behaves like `withdraw` but reports [`WithdrawTo`].
        ///
        /// # Auth
        /// Requires authorization from `user`. `to` needs no authorization.
        ///
        /// # Events
        /// Emits [`WithdrawTo`].
        ///
        /// # Errors
        /// Returns [`Error::InvalidAmount`] if `amount <= 0`,
        /// [`Error::InsufficientBalance`] if `amount` exceeds the user's
        /// unreserved balance, [`Error::ZeroAddress`] if `to` is the zero
        /// address (#236), or [`Error::RateLimitExceeded`] if the
        /// (user, token) pair has used its deposit/withdraw budget for this
        /// window (#314).
        pub fn withdraw_to(
            env: Env,
            user: Address,
            token: Address,
            to: Address,
            amount: i128,
        ) -> Result<(), Error> {
            user.require_auth();
            Self::require_recipient(&env, &to)?;
            let new_balance = Self::do_withdraw(&env, &user, &token, amount)?;

            Self::transfer_out(&env, &token, &to, amount)?;

            WithdrawTo {
                user,
                to,
                token,
                amount,
                balance: new_balance,
            }
            .publish(&env);
            Ok(())
        }

        /// Withdraw the entire balance in one call.
        ///
        /// Exiting must not be a race: with `withdraw`, a charge that settles
        /// between reading the balance and withdrawing makes the withdraw fail
        /// with [`Error::InsufficientBalance`]. This takes whatever is there
        /// instead, so a subscriber leaving the protocol cannot be thwarted by
        /// a keeper.
        ///
        /// "Entire" means everything not reserved for a mandate (#295): a
        /// reserve stays behind until the subscriber releases it with
        /// `set_reserve(.., 0)`, after which this takes the rest.
        ///
        /// # Auth
        /// Requires authorization from `user`.
        ///
        /// # Events
        /// Emits [`Withdraw`] for a non-empty balance, and nothing at all for an
        /// empty one.
        ///
        /// # Errors
        /// Never fails on the balance itself: an empty balance returns `0`
        /// without a token transfer. Returns [`Error::ZeroAddress`] if `user`
        /// is the zero address (#236), and [`Error::RateLimitExceeded`] if the
        /// (user, token) pair has used its deposit/withdraw budget for this
        /// window (#314).
        pub fn withdraw_all(env: Env, user: Address, token: Address) -> Result<i128, Error> {
            user.require_auth();
            Self::require_recipient(&env, &user)?;

            let balance = Self::read_balance(&env, &user, &token);
            let reserved = Self::read_reserved_total(&env, &user, &token);
            let amount = balance - reserved;
            if amount <= 0 {
                // Nothing to move: no transfer, no event, no state change.
                return Ok(0);
            }

            Self::record_operation(&env, &user, &token)?;

            Self::write_balance(&env, &user, &token, reserved);

            Self::transfer_out(&env, &token, &user, amount)?;

            Withdraw {
                user,
                token,
                amount,
                balance: reserved,
            }
            .publish(&env);
            Ok(amount)
        }

        /// Debit a subscriber and pay `to`. Restricted to the subscription
        /// contract, which is the direct caller and is therefore implicitly
        /// authorized by the host when it invokes this function.
        ///
        /// # Auth
        /// Requires authorization from the registered `subscription` contract.
        ///
        /// # Events
        /// Emits [`Debit`].
        ///
        /// # Errors
        /// Returns [`Error::SubscriptionNotSet`] if no subscription contract is set,
        /// [`Error::InvalidAmount`] if `amount <= 0`, or [`Error::InsufficientBalance`]
        /// if `amount` exceeds `user`'s unreserved balance. This entry point
        /// is not tied to a mandate, so it can never draw on a reserve; use
        /// [`Vault::debit_mandate`] for a mandate's own charge.
        pub fn debit(
            env: Env,
            user: Address,
            token: Address,
            to: Address,
            amount: i128,
        ) -> Result<(), Error> {
            Self::do_debit(&env, &user, &token, None, &to, amount)
        }

        /// Debit a subscriber on behalf of one mandate (#295).
        ///
        /// Identical to [`Vault::debit`], except that `mandate_id`'s own
        /// reserve is spendable too and is drawn down first: the mandate can
        /// spend its reserve plus the unreserved balance, while every other
        /// mandate's reserve stays out of reach.
        ///
        /// # Auth
        /// Requires authorization from the registered `subscription` contract.
        ///
        /// # Events
        /// Emits [`Debit`].
        ///
        /// # Errors
        /// As [`Vault::debit`], with [`Error::InsufficientBalance`] meaning
        /// `amount` exceeds the unreserved balance plus this mandate's reserve.
        pub fn debit_mandate(
            env: Env,
            user: Address,
            token: Address,
            mandate_id: u64,
            to: Address,
            amount: i128,
        ) -> Result<(), Error> {
            Self::do_debit(&env, &user, &token, Some(mandate_id), &to, amount)
        }

        /// Debit a subscriber once and pay two recipients from that debit.
        ///
        /// `charge` splits every settlement into a merchant share and a protocol
        /// fee. Settling that with two `debit` calls repeated the authorization
        /// check, the `Subscription` read and the balance read and write for the
        /// same total. `debit_split` performs them once, then makes the same two
        /// token transfers and emits one [`Debit`] per non-empty leg, with the
        /// running balances two sequential `debit` calls would have reported:
        /// the first event carries the balance after `amount_a`, the second the
        /// balance after both legs.
        ///
        /// `amount_a` must be positive, like `debit`. `amount_b` may be zero: a
        /// settlement with no protocol fee then has a single leg and emits a
        /// single [`Debit`], exactly as one `debit` call would have.
        ///
        /// # Auth
        /// Requires authorization from the registered `subscription` contract.
        ///
        /// # Events
        /// Emits one [`Debit`] per non-empty leg.
        ///
        /// # Errors
        /// Returns [`Error::SubscriptionNotSet`] if no subscription contract is
        /// set, [`Error::InvalidAmount`] if `amount_a <= 0` or `amount_b < 0`,
        /// [`Error::AmountOverflow`] if the two amounts overflow `i128`, or
        /// [`Error::InsufficientBalance`] if the total exceeds `user`'s
        /// unreserved balance. Like `debit`, it never draws on a reserve; use
        /// [`Vault::debit_split_mandate`] for a mandate's own charge.
        pub fn debit_split(
            env: Env,
            user: Address,
            token: Address,
            to_a: Address,
            amount_a: i128,
            to_b: Address,
            amount_b: i128,
        ) -> Result<(), Error> {
            Self::do_debit_split(&env, &user, &token, None, &to_a, amount_a, &to_b, amount_b)
        }

        /// [`Vault::debit_split`] on behalf of one mandate (#295): the
        /// mandate's own reserve is spendable too and is drawn down first.
        ///
        /// # Auth
        /// Requires authorization from the registered `subscription` contract.
        ///
        /// # Events
        /// Emits one [`Debit`] per non-empty leg.
        ///
        /// # Errors
        /// As [`Vault::debit_split`], with [`Error::InsufficientBalance`]
        /// meaning the total exceeds the unreserved balance plus this
        /// mandate's reserve.
        #[allow(clippy::too_many_arguments)]
        pub fn debit_split_mandate(
            env: Env,
            user: Address,
            token: Address,
            mandate_id: u64,
            to_a: Address,
            amount_a: i128,
            to_b: Address,
            amount_b: i128,
        ) -> Result<(), Error> {
            Self::do_debit_split(
                &env,
                &user,
                &token,
                Some(mandate_id),
                &to_a,
                amount_a,
                &to_b,
                amount_b,
            )
        }

        /// Debit a subscriber once and pay multiple recipients from that debit.
        ///
        /// This generalizes [`Vault::debit_split_mandate`] to an arbitrary
        /// number of recipients, so a single charge can settle the merchant
        /// share, the protocol fee, a keeper tip and a referral share in one
        /// cross-contract call. The subscription contract uses this to avoid
        /// repeating the authorization check and balance read/write for each
        /// leg.
        ///
        /// `legs` is a vector of `(to, amount)` pairs. The first amount must
        /// be positive; subsequent amounts may be zero (they are skipped). The
        /// sum of all amounts is checked against the subscriber's spendable
        /// balance and spend limit exactly once.
        ///
        /// # Auth
        /// Requires authorization from the registered `subscription` contract.
        ///
        /// # Events
        /// Emits one [`Debit`] per non-zero leg, with balances reflecting the
        /// running total after each leg (matching what sequential `debit`
        /// calls would produce).
        ///
        /// # Errors
        /// Returns [`Error::SubscriptionNotSet`] if no subscription contract is
        /// set, [`Error::InvalidAmount`] if `legs` is empty, the first amount
        /// is not positive, or any amount is negative, [`Error::AmountOverflow`]
        /// if the sum overflows `i128`, or [`Error::InsufficientBalance`] if
        /// the total exceeds the unreserved balance plus this mandate's reserve.
        #[allow(clippy::too_many_arguments)]
        pub fn debit_multi_mandate(
            env: Env,
            user: Address,
            token: Address,
            mandate_id: u64,
            legs: soroban_sdk::Vec<(Address, i128)>,
        ) -> Result<(), Error> {
            Self::do_debit_multi(&env, &user, &token, Some(mandate_id), legs)
        }

        /// Earmark `amount` of the subscriber's `token` balance for one
        /// mandate (#295), replacing any reserve it already had. `0` releases
        /// the reserve.
        ///
        /// While reserved, those funds can be spent only by `mandate_id`'s
        /// own charges (through `debit_mandate` / `debit_split_mandate`, which
        /// draw the reserve down first). Other mandates' debits, plain
        /// `debit`/`debit_split`, `withdraw` and `withdraw_all` see only the
        /// unreserved balance. The subscriber can release a reserve at any
        /// time, so a reserve never traps funds.
        ///
        /// The vault does not know mandates: `mandate_id` is the subscription
        /// contract's id, which that contract passes on every mandate debit.
        /// Reserving for an id that is not the subscriber's own mandate only
        /// ever withholds the subscriber's money from themselves.
        ///
        /// # Auth
        /// Requires authorization from `user`.
        ///
        /// # Events
        /// Emits [`ReserveSet`].
        ///
        /// # Errors
        /// Returns [`Error::InvalidAmount`] if `amount < 0`,
        /// [`Error::ReserveExceedsBalance`] if the subscriber's total reserve
        /// for `token` would exceed their balance, or
        /// [`Error::ReserveCapExceeded`] if it would exceed the deploy-time cap
        /// (#348). Reducing or releasing a reserve is always allowed.
        pub fn set_reserve(
            env: Env,
            user: Address,
            token: Address,
            mandate_id: u64,
            amount: i128,
        ) -> Result<(), Error> {
            user.require_auth();
            if amount < 0 {
                return Err(Error::InvalidAmount);
            }

            let old = Self::read_reserved(&env, &user, &token, mandate_id);
            let total = Self::read_reserved_total(&env, &user, &token);
            let new_total = (total - old)
                .checked_add(amount)
                .ok_or(Error::ReserveExceedsBalance)?;
            let balance = Self::read_balance(&env, &user, &token);
            if new_total > balance {
                return Err(Error::ReserveExceedsBalance);
            }
            // The cap only blocks a reservation that grows the total: a
            // subscriber must always be able to reduce or release a reserve,
            // even when the balance has since fallen below an existing total.
            if new_total > total && new_total > Self::reserve_cap(&env, balance) {
                return Err(Error::ReserveCapExceeded);
            }

            Self::write_reserved(&env, &user, &token, mandate_id, amount);
            Self::write_reserved_total(&env, &user, &token, new_total);

            ReserveSet {
                user,
                token,
                mandate_id,
                amount,
                reserved_total: new_total,
            }
            .publish(&env);
            Ok(())
        }

        /// The reserve `user` earmarked for `mandate_id` in `token`. 0 when
        /// none is set.
        pub fn reserved(env: Env, user: Address, token: Address, mandate_id: u64) -> i128 {
            Self::read_reserved(&env, &user, &token, mandate_id)
        }

        /// Total reserved by `user` in `token` across every mandate.
        pub fn reserved_total(env: Env, user: Address, token: Address) -> i128 {
            Self::read_reserved_total(&env, &user, &token)
        }

        /// The largest share of a subscriber's balance that may be reserved,
        /// in basis points, as fixed when this vault was deployed (#348).
        pub fn reserve_cap_bps(env: Env) -> u32 {
            Self::read_reserve_cap_bps(&env)
        }

        /// The part of `user`'s `token` balance not reserved for any mandate:
        /// what `withdraw` and non-mandate debits can reach.
        pub fn unreserved_balance(env: Env, user: Address, token: Address) -> i128 {
            Self::read_balance(&env, &user, &token) - Self::read_reserved_total(&env, &user, &token)
        }

        /// Whether `debit_mandate(user, token, mandate_id, _, amount)` would
        /// succeed right now on balance and spend-limit grounds: the
        /// unreserved balance plus the mandate's reserve covers `amount`, and
        /// the subscriber's spend limit has room for it. Reads storage only.
        ///
        /// The subscription contract uses this to prove on chain that a due
        /// charge would fail before flagging it (#296).
        pub fn can_debit(
            env: Env,
            user: Address,
            token: Address,
            mandate_id: u64,
            amount: i128,
        ) -> bool {
            if amount <= 0 {
                return false;
            }
            let spendable = Self::spendable(&env, &user, &token, Some(mandate_id));
            spendable >= amount && Self::spend_window(&env, &user, &token, amount).is_ok()
        }

        /// Total balance of `user` in `token` held in the vault, reserved or
        /// not. See `unreserved_balance` for the part outside every reserve.
        pub fn balance(env: Env, user: Address, token: Address) -> i128 {
            Self::read_balance(&env, &user, &token)
        }

        /// Whether the token allowlist is enforced. Off unless the admin turned it
        /// on.
        pub fn allowlist_enabled(env: Env) -> bool {
            Self::allowlist_on(&env)
        }

        /// Whether `token` is on the allowlist. Only meaningful while the
        /// allowlist is enabled; absent means not allowed.
        pub fn token_allowed(env: Env, token: Address) -> bool {
            Self::token_on(&env, &token)
        }

        /// Whether `user` may deposit `token` into the vault. Returns `true`
        /// when the allowlist is disabled, or when it is enabled and `token`
        /// is explicitly allowed. This is a convenience view so callers do not
        /// need to read both `allowlist_enabled` and `token_allowed`.
        pub fn can_deposit(env: Env, user: Address, token: Address) -> bool {
            // The user parameter is accepted for API symmetry with `deposit`
            // and `deposit_cap`; the allowlist does not gate by user.
            let _ = user;
            !Self::allowlist_on(&env) || Self::token_on(&env, &token)
        }

        /// Address of the subscription contract currently allowed to debit funds.
        pub fn subscription(env: Env) -> Result<Address, Error> {
            load_instance_or_err(&env, &DataKey::Subscription, Error::SubscriptionNotSet)
        }

        /// Propose handing the admin role to `new_admin` (#238).
        ///
        /// This is step one of a two-step handover. The current admin keeps
        /// the role until `new_admin` accepts with `accept_admin`; proposing
        /// alone changes no authority, so a mistyped or hostile proposal can
        /// be replaced before anyone accepts it. A later proposal to another
        /// address overwrites the pending one.
        ///
        /// # Auth
        /// Requires authorization from the current admin.
        ///
        /// # Errors
        /// Returns [`Error::NotInitialized`] if no admin is configured.
        pub fn propose_admin(env: Env, new_admin: Address) -> Result<(), Error> {
            let current: Address = Self::admin(env.clone())?;
            current.require_auth();

            env.storage()
                .instance()
                .set(&DataKey::PendingAdmin, &new_admin);
            env.storage()
                .instance()
                .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_BUMP);

            Ok(())
        }

        /// Accept a pending admin handover (#238). Step two of the handshake:
        /// only the address the current admin proposed may accept, and the
        /// role moves only when it does. The pending proposal is consumed, so
        /// a second accept by the same address fails.
        ///
        /// # Auth
        /// Requires authorization from `new_admin`.
        ///
        /// # Errors
        /// Returns [`Error::PendingAdminNotFound`] if no handover is pending,
        /// or [`Error::NotPendingAdmin`] if `new_admin` is not the proposed
        /// successor.
        pub fn accept_admin(env: Env, new_admin: Address) -> Result<(), Error> {
            new_admin.require_auth();

            let pending: Address = env
                .storage()
                .instance()
                .get(&DataKey::PendingAdmin)
                .ok_or(Error::PendingAdminNotFound)?;
            if pending != new_admin {
                return Err(Error::NotPendingAdmin);
            }

            env.storage().instance().set(&DataKey::Admin, &new_admin);
            env.storage().instance().remove(&DataKey::PendingAdmin);
            env.storage()
                .instance()
                .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_BUMP);

            Ok(())
        }

        /// Admin address configured for this vault.
        pub fn admin(env: Env) -> Result<Address, Error> {
            load_instance_or_err(&env, &DataKey::Admin, Error::NotInitialized)
        }

        /// Replace this contract's wasm with code at `new_wasm_hash`. Only the
        /// admin may upgrade; persistent state is untouched. (From #158.)
        ///
        /// Records the upgrade in the contract's upgrade history, which can be
        /// queried via `upgrade_history` for transparency.
        pub fn upgrade_wasm(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), Error> {
            let admin: Address =
                load_instance_or_err(&env, &DataKey::Admin, Error::NotInitialized)?;
            admin.require_auth();

            env.deployer()
                .update_current_contract(ContractExecutable::Wasm(new_wasm_hash.clone()));

            ContractUpgraded {
                admin,
                new_wasm_hash: new_wasm_hash.clone(),
            }
            .publish(&env);

            // Record the upgrade in history.
            Self::record_upgrade(&env, new_wasm_hash);

            Ok(())
        }

        /// Record an upgrade in the contract's upgrade history.
        fn record_upgrade(env: &Env, wasm_hash: BytesN<32>) {
            let version = String::from_str(env, env!("CARGO_PKG_VERSION"));
            let ledger = env.ledger().sequence();
            let record = UpgradeRecord {
                version,
                ledger,
                wasm_hash,
            };
            let mut history: soroban_sdk::Vec<UpgradeRecord> = env
                .storage()
                .instance()
                .get(&DataKey::UpgradeHistory)
                .unwrap_or_else(|| soroban_sdk::Vec::new(env));
            history.push_back(record);
            env.storage()
                .instance()
                .set(&DataKey::UpgradeHistory, &history);
            env.storage()
                .instance()
                .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_BUMP);
        }

        /// Get the contract's upgrade history.
        ///
        /// Returns a vector of upgrade records, oldest first. Each record
        /// contains the version, ledger sequence, and wasm hash. The initial
        /// deployment is not recorded; only explicit upgrades via `upgrade_wasm`
        /// appear here.
        pub fn upgrade_history(env: Env) -> soroban_sdk::Vec<UpgradeRecord> {
            env.storage()
                .instance()
                .get(&DataKey::UpgradeHistory)
                .unwrap_or_else(|| soroban_sdk::Vec::new(&env))
        }

        /// Set or replace a per-period ceiling on total debits for
        /// `(user, token)`, across all of the user's mandates in that token.
        /// Only the user themself can set it, and it can only make charges
        /// fail — it never moves funds and never blocks `withdraw`.
        ///
        /// `amount == 0` disables the limit and clears the window tracker.
        /// When a new limit is set, the window tracker is cleared too, so the
        /// new ceiling is judged on usage from this point on.
        pub fn set_spend_limit(
            env: Env,
            user: Address,
            token: Address,
            amount: i128,
            window_secs: u64,
        ) -> Result<(), Error> {
            user.require_auth();

            if amount < 0 {
                return Err(Error::InvalidAmount);
            }
            if amount > 0 && window_secs == 0 {
                return Err(Error::InvalidWindow);
            }

            let limit_key = DataKey::SpendLimit(user.clone(), token.clone());
            let window_key = DataKey::SpendWindow(user.clone(), token.clone());
            let p = env.storage().persistent();

            if amount == 0 {
                // Disabled: drop both entries so `debit`'s unlimited path stays
                // a single miss on an absent key.
                p.remove(&limit_key);
                p.remove(&window_key);
                return Ok(());
            }

            let limit = SpendLimit {
                amount,
                window_secs,
            };
            p.set(&limit_key, &limit);
            p.extend_ttl(&limit_key, PERSIST_THRESHOLD, PERSIST_BUMP);
            // Clear the tracker: the new limit starts counting fresh.
            p.remove(&window_key);
            Ok(())
        }

        /// The user's stored spending limit. `amount == 0` means no limit.
        pub fn spend_limit(env: Env, user: Address, token: Address) -> SpendLimit {
            env.storage()
                .persistent()
                .get(&DataKey::SpendLimit(user, token))
                .unwrap_or(SpendLimit {
                    amount: 0,
                    window_secs: 0,
                })
        }

        /// Release all per-mandate reserves held for `subscriber` in `token`
        /// (#341), making their full vault balance withdrawable when a merchant
        /// becomes unresponsive.
        ///
        /// Moves no tokens out of the vault — the admin still has no path to
        /// user funds — and emits [`EmergencyWithdraw`] with the total released
        /// reserve amount.
        ///
        /// # Auth
        /// Requires authorization from the contract admin.
        ///
        /// # Events
        /// Emits [`EmergencyWithdraw`].
        ///
        /// # Errors
        /// Returns [`Error::NotInitialized`] if no admin is configured.
        pub fn emergency_withdraw(
            env: Env,
            subscriber: Address,
            token: Address,
        ) -> Result<i128, Error> {
            Self::require_admin(&env)?;

            let released = Self::read_reserved_total(&env, &subscriber, &token);
            let list_key = DataKey::ReservedMandates(subscriber.clone(), token.clone());
            let p = env.storage().persistent();
            if let Some(mandate_ids) = p.get::<_, soroban_sdk::Vec<u64>>(&list_key) {
                for mandate_id in mandate_ids.iter() {
                    p.remove(&DataKey::Reserved(
                        subscriber.clone(),
                        token.clone(),
                        mandate_id,
                    ));
                }
                p.remove(&list_key);
            }
            Self::write_reserved_total(&env, &subscriber, &token, 0);

            EmergencyWithdraw {
                subscriber,
                amount: released,
            }
            .publish(&env);

            Ok(released)
        }
    }

    impl Vault {
        /// Reject an unset transfer destination before any balance is written
        /// or any token is moved (#236).
        fn require_recipient(env: &Env, to: &Address) -> Result<(), Error> {
            if to == &Address::from_str(env, ZERO_ACCOUNT)
                || to == &Address::from_str(env, ZERO_CONTRACT)
            {
                return Err(Error::ZeroAddress);
            }
            Ok(())
        }

        /// Move `amount` from `from` into the vault only if the vault's token
        /// balance grows by exactly `amount`. Returns
        /// [`Error::UnexpectedTransferAmount`] otherwise (a fee-on-transfer
        /// token, for example), which rolls the invocation back.
        fn transfer_in(
            env: &Env,
            token: &Address,
            from: &Address,
            amount: i128,
        ) -> Result<(), Error> {
            let client = token::TokenClient::new(env, token);
            let vault = env.current_contract_address();
            let before = client.balance(&vault);
            client.transfer(from, &vault, &amount);
            let received = client
                .balance(&vault)
                .checked_sub(before)
                .ok_or(Error::UnexpectedTransferAmount)?;
            if received != amount {
                return Err(Error::UnexpectedTransferAmount);
            }
            Ok(())
        }

        /// Pay `amount` to `to` only if `to`'s token balance grows by exactly
        /// `amount`. A fee-on-transfer token shortchanges the recipient, so the
        /// transfer is rolled back rather than settling a charge or withdrawal
        /// that delivered less than the contract recorded.
        fn transfer_out(
            env: &Env,
            token: &Address,
            to: &Address,
            amount: i128,
        ) -> Result<(), Error> {
            let client = token::TokenClient::new(env, token);
            let vault = env.current_contract_address();
            let before = client.balance(to);
            client.transfer(&vault, to, &amount);
            let received = client
                .balance(to)
                .checked_sub(before)
                .ok_or(Error::UnexpectedTransferAmount)?;
            if received != amount {
                return Err(Error::UnexpectedTransferAmount);
            }
            Ok(())
        }

        fn do_debit(
            env: &Env,
            user: &Address,
            token: &Address,
            mandate_id: Option<u64>,
            to: &Address,
            amount: i128,
        ) -> Result<(), Error> {
            Self::require_subscription(env)?;
            Self::require_recipient(env, to)?;

            if amount <= 0 {
                return Err(Error::InvalidAmount);
            }

            let new_balance = Self::spend(env, user, token, mandate_id, amount)?;

            Self::transfer_out(env, token, to, amount)?;

            Debit {
                user: user.clone(),
                token: token.clone(),
                to: to.clone(),
                amount,
                balance: new_balance,
            }
            .publish(env);
            Ok(())
        }

        #[allow(clippy::too_many_arguments)]
        fn do_debit_split(
            env: &Env,
            user: &Address,
            token: &Address,
            mandate_id: Option<u64>,
            to_a: &Address,
            amount_a: i128,
            to_b: &Address,
            amount_b: i128,
        ) -> Result<(), Error> {
            Self::require_subscription(env)?;
            Self::require_recipient(env, to_a)?;
            Self::require_recipient(env, to_b)?;

            if amount_a <= 0 || amount_b < 0 {
                return Err(Error::InvalidAmount);
            }

            // The total is what the subscriber can afford; both legs are
            // settled from it or neither is. Soroban rolls the invocation back
            // on error, so a failed transfer cannot leave half a charge.
            let total = amount_a
                .checked_add(amount_b)
                .ok_or(Error::AmountOverflow)?;

            // Both legs are one logical charge, so the whole total counts
            // against the spend limit once (merged from #134, which predates
            // debit_split; counting the total here keeps the limit from being
            // bypassed via split settlement).
            let after_both = Self::spend(env, user, token, mandate_id, total)?;
            let after_a = after_both + amount_b;

            Self::transfer_out(env, token, to_a, amount_a)?;
            Debit {
                user: user.clone(),
                token: token.clone(),
                to: to_a.clone(),
                amount: amount_a,
                balance: after_a,
            }
            .publish(env);

            if amount_b > 0 {
                Self::transfer_out(env, token, to_b, amount_b)?;
                Debit {
                    user: user.clone(),
                    token: token.clone(),
                    to: to_b.clone(),
                    amount: amount_b,
                    balance: after_both,
                }
                .publish(env);
            }

            Ok(())
        }

        /// Internal implementation of [`Vault::debit_multi_mandate`].
        fn do_debit_multi(
            env: &Env,
            user: &Address,
            token: &Address,
            mandate_id: Option<u64>,
            legs: soroban_sdk::Vec<(Address, i128)>,
        ) -> Result<(), Error> {
            Self::require_subscription(env)?;

            if legs.is_empty() {
                return Err(Error::InvalidAmount);
            }

            let first = legs.get_unchecked(0);
            if first.1 <= 0 {
                return Err(Error::InvalidAmount);
            }

            for i in 0..legs.len() {
                let leg = legs.get_unchecked(i);
                if leg.1 < 0 {
                    return Err(Error::InvalidAmount);
                }
                Self::require_recipient(env, &leg.0)?;
            }

            // Sum all amounts and check against spendable balance and spend limit once.
            let mut total: i128 = 0;
            for i in 0..legs.len() {
                let leg = legs.get_unchecked(i);
                total = total.checked_add(leg.1).ok_or(Error::AmountOverflow)?;
            }

            // The whole total counts against the spend limit once.
            let after_all = Self::spend(env, user, token, mandate_id, total)?;

            // Track running balance for events: start from after_all and add back
            // each leg's amount as we go backwards, or track forward.
            // We want the first event to show balance after first leg, etc.
            // after_all is the balance after ALL legs.
            // after_first = after_all + sum(legs[1..])
            let mut running_balance = after_all;
            for i in (1..legs.len()).rev() {
                let leg = legs.get_unchecked(i);
                running_balance += leg.1;
            }

            for i in 0..legs.len() {
                let leg = legs.get_unchecked(i);
                let to = &leg.0;
                let amount = leg.1;

                if amount > 0 {
                    Self::transfer_out(env, token, to, amount)?;

                    Debit {
                        user: user.clone(),
                        token: token.clone(),
                        to: to.clone(),
                        amount,
                        balance: running_balance,
                    }
                    .publish(env);

                    running_balance -= amount;
                }
            }

            Ok(())
        }

        fn require_subscription(env: &Env) -> Result<(), Error> {
            let subscription: Address =
                load_instance_or_err(env, &DataKey::Subscription, Error::SubscriptionNotSet)?;
            subscription.require_auth();
            Ok(())
        }

        /// What a debit for `mandate_id` (or for no mandate) can reach: the
        /// unreserved balance plus that mandate's own reserve.
        fn spendable(env: &Env, user: &Address, token: &Address, mandate_id: Option<u64>) -> i128 {
            let balance = Self::read_balance(env, user, token);
            let reserved_total = Self::read_reserved_total(env, user, token);
            let own = mandate_id.map_or(0, |id| Self::read_reserved(env, user, token, id));
            balance - reserved_total + own
        }

        /// Take `total` from `user`'s balance for one debit and return the new
        /// balance. Checks the spendable amount and the spend limit before
        /// writing anything, then draws `mandate_id`'s reserve down first so
        /// the invariant `reserved_total <= balance` keeps holding.
        fn spend(
            env: &Env,
            user: &Address,
            token: &Address,
            mandate_id: Option<u64>,
            total: i128,
        ) -> Result<i128, Error> {
            if Self::spendable(env, user, token, mandate_id) < total {
                return Err(Error::InsufficientBalance);
            }

            Self::apply_spend_limit(env, user, token, total)?;

            if let Some(id) = mandate_id {
                let own = Self::read_reserved(env, user, token, id);
                let from_reserve = own.min(total);
                if from_reserve > 0 {
                    Self::write_reserved(env, user, token, id, own - from_reserve);
                    let reserved_total = Self::read_reserved_total(env, user, token);
                    Self::write_reserved_total(env, user, token, reserved_total - from_reserve);
                }
            }

            let new_balance = Self::read_balance(env, user, token) - total;
            Self::write_balance(env, user, token, new_balance);
            Ok(new_balance)
        }

        /// Validate and debit a withdrawal from `user`'s balance, returning the
        /// new balance. The token transfer is the caller's responsibility so
        /// each entry point can emit the event that matches its destination.
        fn do_withdraw(
            env: &Env,
            user: &Address,
            token: &Address,
            amount: i128,
        ) -> Result<i128, Error> {
            if amount <= 0 {
                return Err(Error::InvalidAmount);
            }

            let balance = Self::read_balance(env, user, token);
            let reserved = Self::read_reserved_total(env, user, token);
            if balance - reserved < amount {
                return Err(Error::InsufficientBalance);
            }

            Self::record_operation(env, user, token)?;

            let new_balance = balance - amount;
            Self::write_balance(env, user, token, new_balance);
            Ok(new_balance)
        }

        fn read_reserved(env: &Env, user: &Address, token: &Address, mandate_id: u64) -> i128 {
            let key = DataKey::Reserved(user.clone(), token.clone(), mandate_id);
            let p = env.storage().persistent();
            let Some(amount) = p.get::<_, i128>(&key) else {
                return 0;
            };
            // A reserve belongs to a live mandate (#247): refreshing it on
            // access keeps the reservation from expiring while the mandate is
            // still being charged. A missing reserve is a no-op.
            p.extend_ttl(&key, PERSIST_THRESHOLD, PERSIST_BUMP);
            amount
        }

        /// Store a mandate's reserve; 0 removes the entry so an unreserved
        /// mandate costs nothing to keep.
        fn write_reserved(
            env: &Env,
            user: &Address,
            token: &Address,
            mandate_id: u64,
            amount: i128,
        ) {
            let key = DataKey::Reserved(user.clone(), token.clone(), mandate_id);
            let list_key = DataKey::ReservedMandates(user.clone(), token.clone());
            let p = env.storage().persistent();
            let mut mandate_ids: soroban_sdk::Vec<u64> = p
                .get(&list_key)
                .unwrap_or_else(|| soroban_sdk::Vec::new(env));

            if amount == 0 {
                p.remove(&key);
                let mut idx: Option<u32> = None;
                for i in 0..mandate_ids.len() {
                    if mandate_ids.get_unchecked(i) == mandate_id {
                        idx = Some(i);
                        break;
                    }
                }
                if let Some(pos) = idx {
                    mandate_ids.remove(pos);
                    if mandate_ids.is_empty() {
                        p.remove(&list_key);
                    } else {
                        p.set(&list_key, &mandate_ids);
                        p.extend_ttl(&list_key, PERSIST_THRESHOLD, PERSIST_BUMP);
                    }
                }
            } else {
                p.set(&key, &amount);
                p.extend_ttl(&key, PERSIST_THRESHOLD, PERSIST_BUMP);
                let mut found = false;
                for i in 0..mandate_ids.len() {
                    if mandate_ids.get_unchecked(i) == mandate_id {
                        found = true;
                        break;
                    }
                }
                if !found {
                    mandate_ids.push_back(mandate_id);
                    p.set(&list_key, &mandate_ids);
                }
                p.extend_ttl(&list_key, PERSIST_THRESHOLD, PERSIST_BUMP);
            }
        }

        fn read_reserved_total(env: &Env, user: &Address, token: &Address) -> i128 {
            let key = DataKey::ReservedTotal(user.clone(), token.clone());
            let p = env.storage().persistent();
            let Some(amount) = p.get::<_, i128>(&key) else {
                return 0;
            };
            p.extend_ttl(&key, PERSIST_THRESHOLD, PERSIST_BUMP);
            amount
        }

        fn write_reserved_total(env: &Env, user: &Address, token: &Address, amount: i128) {
            let key = DataKey::ReservedTotal(user.clone(), token.clone());
            let p = env.storage().persistent();
            if amount == 0 {
                p.remove(&key);
            } else {
                p.set(&key, &amount);
                p.extend_ttl(&key, PERSIST_THRESHOLD, PERSIST_BUMP);
            }
        }

        fn read_reserve_cap_bps(env: &Env) -> u32 {
            env.storage()
                .instance()
                .get(&DataKey::ReserveCapBps)
                .unwrap_or(DEFAULT_RESERVE_CAP_BPS)
        }

        /// Largest total reserve allowed for `balance` under the deploy-time
        /// cap. The balance is split before multiplying so the product cannot
        /// overflow `i128` for a very large balance.
        fn reserve_cap(env: &Env, balance: i128) -> i128 {
            let bps = i128::from(Self::read_reserve_cap_bps(env));
            let denominator = i128::from(BPS_DENOMINATOR);
            (balance / denominator) * bps + (balance % denominator) * bps / denominator
        }

        fn require_admin(env: &Env) -> Result<Address, Error> {
            let admin: Address = load_instance_or_err(env, &DataKey::Admin, Error::NotInitialized)?;
            admin.require_auth();
            Ok(admin)
        }

        fn allowlist_on(env: &Env) -> bool {
            env.storage()
                .instance()
                .get(&DataKey::AllowlistEnabled)
                .unwrap_or(false)
        }

        fn token_on(env: &Env, token: &Address) -> bool {
            env.storage()
                .persistent()
                .get(&DataKey::TokenAllowed(token.clone()))
                .unwrap_or(false)
        }

        /// The deposit cap for (user, token). Absent means unlimited.
        fn read_deposit_cap(env: &Env, user: &Address, token: &Address) -> i128 {
            let key = DataKey::DepositCap(user.clone(), token.clone());
            env.storage().persistent().get(&key).unwrap_or(0)
        }

        /// Count `amount` against `(user, token)`'s spend limit, rolling the
        /// fixed window forward when the current one has elapsed. Fails with
        /// `SpendLimitExceeded` before anything is written when the ceiling
        /// would be crossed. When no limit is set this is a single absent-key
        /// lookup, so the unlimited path costs one extra storage read.
        fn apply_spend_limit(
            env: &Env,
            user: &Address,
            token: &Address,
            amount: i128,
        ) -> Result<(), Error> {
            let Some(window) = Self::spend_window(env, user, token, amount)? else {
                return Ok(());
            };
            let window_key = DataKey::SpendWindow(user.clone(), token.clone());
            let p = env.storage().persistent();
            p.set(&window_key, &window);
            p.extend_ttl(&window_key, PERSIST_THRESHOLD, PERSIST_BUMP);
            Ok(())
        }

        /// The spend window `(user, token)` would have after debiting
        /// `amount`, or `None` when no limit is set. Fails with
        /// `SpendLimitExceeded` when the ceiling would be crossed. Reads only:
        /// `apply_spend_limit` writes the result, `can_debit` just checks it.
        fn spend_window(
            env: &Env,
            user: &Address,
            token: &Address,
            amount: i128,
        ) -> Result<Option<SpendWindow>, Error> {
            let limit_key = DataKey::SpendLimit(user.clone(), token.clone());
            let Some(limit) = env.storage().persistent().get::<_, SpendLimit>(&limit_key) else {
                return Ok(None);
            };
            if limit.amount <= 0 {
                return Ok(None);
            }

            let now = env.ledger().timestamp();
            let window_key = DataKey::SpendWindow(user.clone(), token.clone());
            let (start, spent) = match env
                .storage()
                .persistent()
                .get::<_, SpendWindow>(&window_key)
            {
                Some(w) if now < w.start.saturating_add(limit.window_secs) => (w.start, w.spent),
                // No window yet, or the stored one has elapsed: start a fresh
                // one, aligned to the epoch so the reset is deterministic.
                _ => (now - (now % limit.window_secs), 0),
            };

            // `checked_add`, not `saturating_add`: a spent total at i128::MAX
            // must fail the limit instead of passing the check and then
            // overflowing on the write below.
            let new_spent = spent.checked_add(amount).ok_or(Error::SpendLimitExceeded)?;
            if new_spent > limit.amount {
                return Err(Error::SpendLimitExceeded);
            }

            Ok(Some(SpendWindow {
                start,
                spent: new_spent,
            }))
        }

        /// Charge one deposit/withdraw operation against `(user, token)`'s
        /// current fixed window (#314), failing before any balance is written
        /// when the budget is exhausted. The window is epoch-aligned and rolls
        /// over deterministically, so cycling cannot keep extending it.
        fn record_operation(env: &Env, user: &Address, token: &Address) -> Result<(), Error> {
            let now = env.ledger().timestamp();
            let start = now - (now % VAULT_OPERATION_WINDOW_SECS);
            let key = DataKey::OperationWindow(user.clone(), token.clone());
            let p = env.storage().persistent();
            let operations = match p.get::<_, OperationWindow>(&key) {
                Some(window) if window.start == start => window.operations,
                _ => 0,
            };
            if operations >= MAX_VAULT_OPERATIONS_PER_WINDOW {
                return Err(Error::RateLimitExceeded);
            }
            p.set(
                &key,
                &OperationWindow {
                    start,
                    operations: operations + 1,
                },
            );
            p.extend_ttl(&key, PERSIST_THRESHOLD, PERSIST_BUMP);
            Ok(())
        }

        fn read_balance(env: &Env, user: &Address, token: &Address) -> i128 {
            let key = DataKey::Balance(user.clone(), token.clone());
            env.storage().persistent().get(&key).unwrap_or(0)
        }

        fn write_balance(env: &Env, user: &Address, token: &Address, amount: i128) {
            let key = DataKey::Balance(user.clone(), token.clone());
            env.storage().persistent().set(&key, &amount);
            env.storage()
                .persistent()
                .extend_ttl(&key, PERSIST_THRESHOLD, PERSIST_BUMP);
        }
    }
}

pub use contract::{Vault, VaultClient};
