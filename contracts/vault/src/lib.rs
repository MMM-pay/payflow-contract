#![no_std]

mod error;
mod events;
mod storage;
#[cfg(test)]
mod test;

use soroban_sdk::{contract, contractimpl, Address, Env};

pub use crate::error::VaultError;
pub use crate::storage::BalanceDetail;

#[contract]
pub struct VaultContract;

#[contractimpl]
impl VaultContract {
    pub fn __constructor(env: Env, admin: Address, window_duration: u64, max_ops_per_window: u32) {
        storage::set_admin(&env, &admin);
        storage::set_rate_limit_params(&env, window_duration, max_ops_per_window);
    }

    pub fn deposit(env: Env, subscriber: Address, token: Address, amount: i128) -> Result<(), VaultError> {
        subscriber.require_auth();
        storage::check_rate_limit(&env, &subscriber, &token);
        storage::add_balance(&env, &subscriber, &token, amount);
        let client = soroban_sdk::token::Client::new(&env, &token);
        client.transfer(&subscriber, &env.current_contract_address(), &amount);
        events::emit_deposit(&env, subscriber, token, amount);
        Ok(())
    }

    pub fn withdraw(env: Env, subscriber: Address, token: Address, amount: i128) -> Result<(), VaultError> {
        subscriber.require_auth();
        storage::check_rate_limit(&env, &subscriber, &token);
        let free = storage::get_free_balance(&env, &subscriber, &token);
        if amount > free {
            return Err(VaultError::InsufficientFreeBalance);
        }
        storage::sub_balance(&env, &subscriber, &token, amount);
        let client = soroban_sdk::token::Client::new(&env, &token);
        client.transfer(&env.current_contract_address(), &subscriber, &amount);
        events::emit_withdraw(&env, subscriber, token, amount);
        Ok(())
    }

    pub fn debit(env: Env, subscriber: Address, token: Address, amount: i128) -> Result<(), VaultError> {
        let authorized_caller = storage::get_authorized_caller(&env);
        if let Some(caller) = authorized_caller {
            caller.require_auth();
        } else {
            let admin = storage::get_admin(&env);
            admin.require_auth();
        }
        let free = storage::get_free_balance(&env, &subscriber, &token);
        if amount > free {
            return Err(VaultError::InsufficientFreeBalance);
        }
        storage::sub_balance(&env, &subscriber, &token, amount);
        events::emit_debit(&env, subscriber, token, amount);
        Ok(())
    }

    pub fn lock_reserve(env: Env, subscriber: Address, token: Address, mandate_id: u64, amount: i128) -> Result<(), VaultError> {
        let admin = storage::get_admin(&env);
        admin.require_auth();
        storage::add_reserve(&env, &subscriber, &token, mandate_id, amount);
        Ok(())
    }

    pub fn unlock_reserve(env: Env, subscriber: Address, token: Address, mandate_id: u64, amount: i128) -> Result<(), VaultError> {
        let admin = storage::get_admin(&env);
        admin.require_auth();
        storage::sub_reserve(&env, &subscriber, &token, mandate_id, amount);
        Ok(())
    }

    pub fn balance_detail(env: Env, subscriber: Address, token: Address) -> BalanceDetail {
        let total = storage::get_total_balance(&env, &subscriber, &token);
        let locked = storage::get_total_reserves(&env, &subscriber, &token);
        let free = total.checked_sub(locked).unwrap_or(0);
        BalanceDetail { total, locked, free }
    }
}
