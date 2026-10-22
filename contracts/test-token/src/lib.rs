#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, panic_with_error, Address, Env, String,
};

const DAY_IN_LEDGERS: u32 = 17_280;
const INSTANCE_BUMP: u32 = 30 * DAY_IN_LEDGERS;
const INSTANCE_THRESHOLD: u32 = INSTANCE_BUMP - DAY_IN_LEDGERS;
const PERSIST_BUMP: u32 = 90 * DAY_IN_LEDGERS;
const PERSIST_THRESHOLD: u32 = PERSIST_BUMP - DAY_IN_LEDGERS;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    InvalidAmount = 2,
    InsufficientBalance = 3,
    AmountOverflow = 4,
    TransferFailed = 5,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Balance(Address),
    FailTransfers,
    FailTransfersTo(Address),
}

/// A test-only token contract that can be configured to fail transfers.
///
/// Implements the subset of the SEP-41 token interface used by Payflow contracts
/// (`transfer`, `balance`, `decimals`, `name`, `symbol`), plus minting and failure injection
/// toggles for testing behaviour with misbehaving tokens.
#[contract]
pub struct TestToken;

#[contractimpl]
impl TestToken {
    pub fn __constructor(env: Env, admin: Address) {
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_BUMP);
    }

    pub fn admin(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)
    }

    /// Toggle global transfer failure. When enabled, any call to `transfer` panics with `TransferFailed`.
    pub fn set_fail_transfers(env: Env, fail: bool) -> Result<(), Error> {
        let admin = Self::admin(env.clone())?;
        admin.require_auth();
        env.storage().instance().set(&DataKey::FailTransfers, &fail);
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_BUMP);
        Ok(())
    }

    /// Toggle transfer failure targeted to a specific recipient address.
    pub fn set_fail_transfers_to(env: Env, to: Address, fail: bool) -> Result<(), Error> {
        let admin = Self::admin(env.clone())?;
        admin.require_auth();
        let key = DataKey::FailTransfersTo(to);
        if fail {
            env.storage().persistent().set(&key, &true);
            env.storage()
                .persistent()
                .extend_ttl(&key, PERSIST_THRESHOLD, PERSIST_BUMP);
        } else {
            env.storage().persistent().remove(&key);
        }
        Ok(())
    }

    pub fn fail_transfers(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::FailTransfers)
            .unwrap_or(false)
    }

    pub fn fail_transfers_to(env: Env, to: Address) -> bool {
        let key = DataKey::FailTransfersTo(to);
        env.storage().persistent().get(&key).unwrap_or(false)
    }

    pub fn mint(env: Env, to: Address, amount: i128) -> Result<(), Error> {
        let admin = Self::admin(env.clone())?;
        admin.require_auth();
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        let balance = Self::read_balance(&env, &to);
        let new_balance = balance.checked_add(amount).ok_or(Error::AmountOverflow)?;
        Self::write_balance(&env, &to, new_balance);
        Ok(())
    }

    pub fn balance(env: Env, id: Address) -> i128 {
        Self::read_balance(&env, &id)
    }

    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        from.require_auth();
        if amount <= 0 {
            panic_with_error!(&env, Error::InvalidAmount);
        }

        if Self::fail_transfers(env.clone()) || Self::fail_transfers_to(env.clone(), to.clone()) {
            panic_with_error!(&env, Error::TransferFailed);
        }

        let from_balance = Self::read_balance(&env, &from);
        if from_balance < amount {
            panic_with_error!(&env, Error::InsufficientBalance);
        }

        let to_balance = Self::read_balance(&env, &to);
        let new_from = from_balance - amount;
        let new_to = match to_balance.checked_add(amount) {
            Some(b) => b,
            None => panic_with_error!(&env, Error::AmountOverflow),
        };

        Self::write_balance(&env, &from, new_from);
        Self::write_balance(&env, &to, new_to);
    }

    pub fn decimals(_env: Env) -> u32 {
        7
    }

    pub fn name(env: Env) -> String {
        String::from_str(&env, "Test Token")
    }

    pub fn symbol(env: Env) -> String {
        String::from_str(&env, "TEST")
    }
}

impl TestToken {
    fn read_balance(env: &Env, user: &Address) -> i128 {
        let key = DataKey::Balance(user.clone());
        env.storage().persistent().get(&key).unwrap_or(0)
    }

    fn write_balance(env: &Env, user: &Address, amount: i128) {
        let key = DataKey::Balance(user.clone());
        env.storage().persistent().set(&key, &amount);
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSIST_THRESHOLD, PERSIST_BUMP);
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::Address as _;

    #[test]
    fn mint_and_transfer_happy_path() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let user_a = Address::generate(&env);
        let user_b = Address::generate(&env);

        let contract_id = env.register(TestToken, (admin.clone(),));
        let client = TestTokenClient::new(&env, &contract_id);

        assert_eq!(client.admin(), admin);
        assert_eq!(client.decimals(), 7);
        assert_eq!(client.balance(&user_a), 0);

        client.mint(&user_a, &1_000);
        assert_eq!(client.balance(&user_a), 1_000);

        client.transfer(&user_a, &user_b, &400);
        assert_eq!(client.balance(&user_a), 600);
        assert_eq!(client.balance(&user_b), 400);
    }

    #[test]
    fn fail_transfers_flag_reverts_transfer() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let user_a = Address::generate(&env);
        let user_b = Address::generate(&env);

        let contract_id = env.register(TestToken, (admin,));
        let client = TestTokenClient::new(&env, &contract_id);

        client.mint(&user_a, &1_000);
        client.set_fail_transfers(&true);
        assert!(client.fail_transfers());

        assert!(client.try_transfer(&user_a, &user_b, &100).is_err());
        assert_eq!(client.balance(&user_a), 1_000);
        assert_eq!(client.balance(&user_b), 0);

        // Turn off failure: transfers should work again
        client.set_fail_transfers(&false);
        assert!(!client.fail_transfers());
        client.transfer(&user_a, &user_b, &100);
        assert_eq!(client.balance(&user_a), 900);
        assert_eq!(client.balance(&user_b), 100);
    }

    #[test]
    fn fail_transfers_to_specific_address() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let user_a = Address::generate(&env);
        let user_b = Address::generate(&env);
        let user_c = Address::generate(&env);

        let contract_id = env.register(TestToken, (admin,));
        let client = TestTokenClient::new(&env, &contract_id);

        client.mint(&user_a, &1_000);
        client.set_fail_transfers_to(&user_b, &true);
        assert!(client.fail_transfers_to(&user_b));
        assert!(!client.fail_transfers_to(&user_c));

        // Transfer to user_b should fail
        assert!(client.try_transfer(&user_a, &user_b, &100).is_err());
        assert_eq!(client.balance(&user_a), 1_000);
        assert_eq!(client.balance(&user_b), 0);

        // Transfer to user_c should succeed
        client.transfer(&user_a, &user_c, &100);
        assert_eq!(client.balance(&user_a), 900);
        assert_eq!(client.balance(&user_c), 100);

        // Unset failure for user_b
        client.set_fail_transfers_to(&user_b, &false);
        assert!(!client.fail_transfers_to(&user_b));
        client.transfer(&user_a, &user_b, &200);
        assert_eq!(client.balance(&user_a), 700);
        assert_eq!(client.balance(&user_b), 200);
    }

    #[test]
    fn auth_failures() {
        let env = Env::default();
        let admin = Address::generate(&env);
        let attacker = Address::generate(&env);
        let user = Address::generate(&env);

        let contract_id = env.register(TestToken, (admin.clone(),));
        let client = TestTokenClient::new(&env, &contract_id);

        // Attacker cannot mint
        env.mock_auths(&[]);
        assert!(client.try_mint(&user, &1_000).is_err());

        // Attacker cannot toggle fail_transfers
        assert!(client.try_set_fail_transfers(&true).is_err());

        // Attacker cannot toggle fail_transfers_to
        assert!(client.try_set_fail_transfers_to(&user, &true).is_err());

        // Attacker cannot transfer user's tokens
        assert!(client.try_transfer(&user, &attacker, &100).is_err());
    }
}
