#![no_std]

mod error;
mod events;
mod types;

#[cfg(test)]
mod test;

pub use error::Error;
pub use events::{Debit, Deposit, Withdraw};
pub use types::{DataKey, SpendLimit, SpendWindow};

use soroban_sdk::{contract, contractimpl, token, Address, Env};

const DAY_IN_LEDGERS: u32 = 17_280;
const INSTANCE_BUMP: u32 = 30 * DAY_IN_LEDGERS;
const INSTANCE_THRESHOLD: u32 = INSTANCE_BUMP - DAY_IN_LEDGERS;
const PERSIST_BUMP: u32 = 90 * DAY_IN_LEDGERS;
const PERSIST_THRESHOLD: u32 = PERSIST_BUMP - DAY_IN_LEDGERS;

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
    pub fn set_subscription(env: Env, subscription: Address) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();

        env.storage()
            .instance()
            .set(&DataKey::Subscription, &subscription);
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_BUMP);
        Ok(())
    }

    /// Move `amount` of `token` from the subscriber's wallet into the vault.
    pub fn deposit(env: Env, user: Address, token: Address, amount: i128) -> Result<(), Error> {
        user.require_auth();
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let vault = env.current_contract_address();
        token::TokenClient::new(&env, &token).transfer(&user, &vault, &amount);

        let new_balance = Self::read_balance(&env, &user, &token) + amount;
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
    pub fn withdraw(env: Env, user: Address, token: Address, amount: i128) -> Result<(), Error> {
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
    /// If the subscriber has set a spend limit for this token, every debit
    /// counts against it — merchant share, protocol fee and keeper tip are
    /// separate debits, so all of them together must fit under the ceiling.
    /// A charge that would cross the limit fails atomically: no partial
    /// debits land, the mandate's `next_charge` does not advance, and the
    /// mandate simply stays due. The keeper's retry-with-backoff then tries
    /// the same charge again on its next attempt, and either succeeds after
    /// the window rolls over or keeps failing until the subscriber raises the
    /// limit or cancels the mandate. The limit can therefore delay billing
    /// but never change a mandate's amount or schedule.
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

    /// Set or replace a per-period ceiling on total debits for
    /// `(user, token)`, across all of the user's mandates in that token. Only
    /// the user themself can set it, and it can only make charges fail — it
    /// never moves funds and never blocks `withdraw`.
    ///
    /// `amount == 0` disables the limit and clears the window tracker. When a
    /// new limit is set, the window tracker is cleared too, so the new ceiling
    /// is judged on usage from this point on, not on what the old limit
    /// allowed.
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
            // Disabled: drop both entries so `debit`'s unlimited path stays a
            // single miss on an absent key.
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

    pub fn balance(env: Env, user: Address, token: Address) -> i128 {
        Self::read_balance(&env, &user, &token)
    }

    pub fn subscription(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Subscription)
            .ok_or(Error::SubscriptionNotSet)
    }

    pub fn admin(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)
    }
}

impl Vault {
    /// Count `amount` against `(user, token)`'s spend limit, rolling the
    /// fixed window forward when the current one has elapsed. Fails with
    /// `SpendLimitExceeded` before anything is written when the ceiling would
    /// be crossed. When no limit is set this is a single absent-key lookup,
    /// so the unlimited path costs one extra storage read.
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
