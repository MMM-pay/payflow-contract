//! Custody for subscriber funds.
//!
//! A pull-payment scheme fails the moment a subscriber's wallet is empty at
//! charge time. The vault decouples funding from billing: subscribers top up
//! ahead of time, and the subscription contract debits from a balance that is
//! already committed. Custody is deliberately separated from billing logic so
//! the two can be audited independently.

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
mod test_events;

pub use error::Error;
pub use events::{Debit, Deposit, DepositCapSet, DepositFor, Withdraw};
pub use types::{DataKey, SpendLimit, SpendWindow};

use soroban_sdk::{contract, contractimpl, token, Address, Env, String};

const DAY_IN_LEDGERS: u32 = 17_280;
const INSTANCE_BUMP: u32 = 30 * DAY_IN_LEDGERS;
const INSTANCE_THRESHOLD: u32 = INSTANCE_BUMP - DAY_IN_LEDGERS;
const PERSIST_BUMP: u32 = 90 * DAY_IN_LEDGERS;
const PERSIST_THRESHOLD: u32 = PERSIST_BUMP - DAY_IN_LEDGERS;

#[allow(missing_docs)]
mod contract {
    use super::*;

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
        pub fn __constructor(env: Env, admin: Address) {
            env.storage().instance().set(&DataKey::Admin, &admin);
            env.storage()
                .instance()
                .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_BUMP);
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
        /// if the allowlist is enabled and `token` is not allowed, or
        /// [`Error::DepositCapExceeded`] if the deposit would exceed the subscriber's cap.
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

            let new_balance = Self::read_balance(&env, &user, &token) + amount;
            // The cap is checked before the transfer, so a rejected deposit moves
            // no tokens at all.
            let cap = Self::read_deposit_cap(&env, &user, &token);
            if cap > 0 && new_balance > cap {
                return Err(Error::DepositCapExceeded);
            }

            let vault = env.current_contract_address();
            token::TokenClient::new(&env, &token).transfer(&user, &vault, &amount);

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
        /// Applies the same allowlist and deposit-cap checks as `deposit`, so
        /// funding on someone's behalf cannot bypass either. (#136 predated both
        /// features; enforcing them here keeps third-party funding consistent
        /// with a direct deposit.)
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

            let new_balance = Self::read_balance(&env, &user, &token) + amount;
            let cap = Self::read_deposit_cap(&env, &user, &token);
            if cap > 0 && new_balance > cap {
                return Err(Error::DepositCapExceeded);
            }

            let vault = env.current_contract_address();
            token::TokenClient::new(&env, &token).transfer(&from, &vault, &amount);

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
        /// # Auth
        /// Requires authorization from `user`.
        ///
        /// # Events
        /// Emits [`Withdraw`].
        ///
        /// # Errors
        /// Returns [`Error::InvalidAmount`] if `amount <= 0`, or
        /// [`Error::InsufficientBalance`] if `amount` exceeds the user's spendable balance.
        pub fn withdraw(
            env: Env,
            user: Address,
            token: Address,
            amount: i128,
        ) -> Result<(), Error> {
            user.require_auth();
            if amount <= 0 {
                return Err(Error::InvalidAmount);
            }

            let balance = Self::read_balance(&env, &user, &token);
            if balance < amount {
                return Err(Error::InsufficientBalance);
            }

            let new_balance = balance - amount;
            Self::write_balance(&env, &user, &token, new_balance);

            let vault = env.current_contract_address();
            token::TokenClient::new(&env, &token).transfer(&vault, &user, &amount);

            Withdraw {
                user,
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
        /// # Auth
        /// Requires authorization from `user`.
        ///
        /// # Events
        /// Emits [`Withdraw`] for a non-empty balance, and nothing at all for an
        /// empty one.
        ///
        /// # Errors
        /// Never fails on the balance itself: an empty balance returns `0`
        /// without a token transfer.
        pub fn withdraw_all(env: Env, user: Address, token: Address) -> Result<i128, Error> {
            user.require_auth();

            let balance = Self::read_balance(&env, &user, &token);
            if balance <= 0 {
                // Nothing to move: no transfer, no event, no state change.
                return Ok(0);
            }

            Self::write_balance(&env, &user, &token, 0);

            let vault = env.current_contract_address();
            token::TokenClient::new(&env, &token).transfer(&vault, &user, &balance);

            Withdraw {
                user,
                token,
                amount: balance,
                balance: 0,
            }
            .publish(&env);
            Ok(balance)
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
        /// if `amount` exceeds `user`'s spendable balance.
        pub fn debit(
            env: Env,
            user: Address,
            token: Address,
            to: Address,
            amount: i128,
        ) -> Result<(), Error> {
            let subscription: Address = env
                .storage()
                .instance()
                .get(&DataKey::Subscription)
                .ok_or(Error::SubscriptionNotSet)?;
            subscription.require_auth();

            if amount <= 0 {
                return Err(Error::InvalidAmount);
            }

            let balance = Self::read_balance(&env, &user, &token);
            if balance < amount {
                return Err(Error::InsufficientBalance);
            }

            Self::apply_spend_limit(&env, &user, &token, amount)?;

            let new_balance = balance - amount;
            Self::write_balance(&env, &user, &token, new_balance);

            let vault = env.current_contract_address();
            token::TokenClient::new(&env, &token).transfer(&vault, &to, &amount);

            Debit {
                user,
                token,
                to,
                amount,
                balance: new_balance,
            }
            .publish(&env);
            Ok(())
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
        /// spendable balance.
        pub fn debit_split(
            env: Env,
            user: Address,
            token: Address,
            to_a: Address,
            amount_a: i128,
            to_b: Address,
            amount_b: i128,
        ) -> Result<(), Error> {
            let subscription: Address = env
                .storage()
                .instance()
                .get(&DataKey::Subscription)
                .ok_or(Error::SubscriptionNotSet)?;
            subscription.require_auth();

            if amount_a <= 0 || amount_b < 0 {
                return Err(Error::InvalidAmount);
            }

            // The total is what the subscriber can afford; both legs are
            // settled from it or neither is. Soroban rolls the invocation back
            // on error, so a failed transfer cannot leave half a charge.
            let total = amount_a
                .checked_add(amount_b)
                .ok_or(Error::AmountOverflow)?;

            let balance = Self::read_balance(&env, &user, &token);
            if balance < total {
                return Err(Error::InsufficientBalance);
            }

            // Both legs are one logical charge, so the whole total counts
            // against the spend limit once (merged from #134, which predates
            // debit_split; counting the total here keeps the limit from being
            // bypassed via split settlement).
            Self::apply_spend_limit(&env, &user, &token, total)?;

            let after_a = balance - amount_a;
            let after_both = after_a - amount_b;
            Self::write_balance(&env, &user, &token, after_both);

            let vault = env.current_contract_address();
            let token_client = token::TokenClient::new(&env, &token);

            token_client.transfer(&vault, &to_a, &amount_a);
            Debit {
                user: user.clone(),
                token: token.clone(),
                to: to_a,
                amount: amount_a,
                balance: after_a,
            }
            .publish(&env);

            if amount_b > 0 {
                token_client.transfer(&vault, &to_b, &amount_b);
                Debit {
                    user,
                    token,
                    to: to_b,
                    amount: amount_b,
                    balance: after_both,
                }
                .publish(&env);
            }

            Ok(())
        }

        /// Spendable balance of `user` in `token` held in the vault.
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

        /// Address of the subscription contract currently allowed to debit funds.
        pub fn subscription(env: Env) -> Result<Address, Error> {
            env.storage()
                .instance()
                .get(&DataKey::Subscription)
                .ok_or(Error::SubscriptionNotSet)
        }

        /// Hand the admin role to another address. Only the current admin may
        /// call this, and the new admin takes effect immediately: the old one
        /// keeps no authority. A deployment whose admin key is lost or
        /// rotated is otherwise stuck with it for the life of the contract.
        ///
        /// # Auth
        /// Requires authorization from the current admin.
        pub fn update_admin(env: Env, new_admin: Address) -> Result<(), Error> {
            let current: Address = Self::admin(env.clone())?;
            current.require_auth();

            env.storage().instance().set(&DataKey::Admin, &new_admin);
            env.storage()
                .instance()
                .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_BUMP);

            Ok(())
        }

        /// Admin address configured for this vault.
        pub fn admin(env: Env) -> Result<Address, Error> {
            env.storage()
                .instance()
                .get(&DataKey::Admin)
                .ok_or(Error::NotInitialized)
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
    }

    impl Vault {
        fn require_admin(env: &Env) -> Result<Address, Error> {
            let admin: Address = env
                .storage()
                .instance()
                .get(&DataKey::Admin)
                .ok_or(Error::NotInitialized)?;
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
            let limit_key = DataKey::SpendLimit(user.clone(), token.clone());
            let Some(limit) = env.storage().persistent().get::<_, SpendLimit>(&limit_key) else {
                return Ok(());
            };
            if limit.amount <= 0 {
                return Ok(());
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

            if spent.saturating_add(amount) > limit.amount {
                return Err(Error::SpendLimitExceeded);
            }

            let window = SpendWindow {
                start,
                spent: spent + amount,
            };
            let p = env.storage().persistent();
            p.set(&window_key, &window);
            p.extend_ttl(&window_key, PERSIST_THRESHOLD, PERSIST_BUMP);
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
