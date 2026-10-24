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

pub use error::Error;
pub use events::{Debit, Deposit, DepositCapSet, Withdraw};
pub use types::DataKey;

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

        /// Admin address configured for this vault.
        pub fn admin(env: Env) -> Result<Address, Error> {
            env.storage()
                .instance()
                .get(&DataKey::Admin)
                .ok_or(Error::NotInitialized)
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
