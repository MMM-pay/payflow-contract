#![no_std]

mod error;
mod events;
mod types;

#[cfg(test)]
mod test;

pub use error::Error;
pub use events::{Debit, Deposit, Withdraw};
pub use types::DataKey;

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
    pub fn initialize(env: Env, admin: Address) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_BUMP);
        Ok(())
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
