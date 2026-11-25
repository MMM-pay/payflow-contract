//! Test-only SEP-41 token implementation for Payflow testing.
//!
//! Provides a configurable token contract with minting and failure injection
//! to verify contract behavior under error conditions.
//!
//! This is a test fixture, not a production asset. It is excluded from the
//! deploy script, release artifacts and TypeScript bindings. Its admin can
//! mint without limit and force transfers to fail, so it must never be
//! deployed as a real token. See `docs/test-token.md` for its full intended
//! use and limitations.

#![no_std]
#![deny(missing_docs)]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, panic_with_error, Address, Env, String,
};

const DAY_IN_LEDGERS: u32 = 17_280;
const INSTANCE_BUMP: u32 = 30 * DAY_IN_LEDGERS;
const INSTANCE_THRESHOLD: u32 = INSTANCE_BUMP - DAY_IN_LEDGERS;
const PERSIST_BUMP: u32 = 90 * DAY_IN_LEDGERS;
const PERSIST_THRESHOLD: u32 = PERSIST_BUMP - DAY_IN_LEDGERS;

#[allow(missing_docs)]
mod types {
    use super::{contracterror, contracttype, Address};

    /// Errors returned by the test token contract.
    #[contracterror]
    #[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
    #[repr(u32)]
    pub enum Error {
        /// The contract has not been initialized with an admin.
        NotInitialized = 1,
        /// The transfer or mint amount is zero or negative.
        InvalidAmount = 2,
        /// Account balance is insufficient to complete the transfer.
        InsufficientBalance = 3,
        /// Addition would overflow i128.
        AmountOverflow = 4,
        /// Transfer failed because failure injection is enabled.
        TransferFailed = 5,
        /// The transfer fee in basis points exceeds `10_000` (100%).
        InvalidFee = 6,
    }

    /// Storage keys used by the test token contract.
    #[contracttype]
    #[derive(Clone)]
    pub enum DataKey {
        /// Contract administrator address.
        Admin,
        /// Token balance stored for an address.
        Balance(Address),
        /// Flag controlling global transfer failure injection.
        FailTransfers,
        /// Flag controlling failure injection for a specific recipient address.
        FailTransfersTo(Address),
        /// Transfer fee in basis points, deducted from the received amount.
        TransferFeeBps,
    }
}

pub use types::{DataKey, Error};

#[allow(missing_docs)]
mod contract {
    use super::{
        contract, contractimpl, panic_with_error, Address, DataKey, Env, Error, String,
        INSTANCE_BUMP, INSTANCE_THRESHOLD, PERSIST_BUMP, PERSIST_THRESHOLD,
    };

    /// A test-only token contract that can be configured to fail transfers.
    ///
    /// Implements the subset of the SEP-41 token interface used by Payflow
    /// contracts (`transfer`, `balance`, `decimals`, `name`, `symbol`), plus
    /// minting and failure injection toggles for testing behaviour with
    /// misbehaving tokens. It is never deployed by the protocol and must never
    /// be used to hold real funds; see `docs/test-token.md`.
    #[contract]
    pub struct TestToken;

    #[contractimpl]
    impl TestToken {
        /// Runs once on deploy to record the contract admin.
        ///
        /// # Auth
        /// None (invoked during deployment).
        ///
        /// # Events
        /// Emits no events.
        ///
        /// # Errors
        /// Emits no contract errors.
        pub fn __constructor(env: Env, admin: Address) {
            env.storage().instance().set(&DataKey::Admin, &admin);
            env.storage()
                .instance()
                .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_BUMP);
        }

        /// Address of the contract administrator.
        pub fn admin(env: Env) -> Result<Address, Error> {
            env.storage()
                .instance()
                .get(&DataKey::Admin)
                .ok_or(Error::NotInitialized)
        }

        /// Toggle global transfer failure. When enabled, any call to `transfer` panics with `TransferFailed`.
        ///
        /// # Auth
        /// Requires authorization from the contract admin.
        ///
        /// # Events
        /// Emits no events.
        ///
        /// # Errors
        /// Returns [`Error::NotInitialized`] if no admin is configured.
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
        ///
        /// # Auth
        /// Requires authorization from the contract admin.
        ///
        /// # Events
        /// Emits no events.
        ///
        /// # Errors
        /// Returns [`Error::NotInitialized`] if no admin is configured.
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

        /// Whether global transfer failure is currently enabled.
        pub fn fail_transfers(env: Env) -> bool {
            env.storage()
                .instance()
                .get(&DataKey::FailTransfers)
                .unwrap_or(false)
        }

        /// Whether transfer failure to a specific recipient is enabled.
        pub fn fail_transfers_to(env: Env, to: Address) -> bool {
            let key = DataKey::FailTransfersTo(to);
            env.storage().persistent().get(&key).unwrap_or(false)
        }

        /// Set the fee-on-transfer rate in basis points (`0`–`10_000`).
        ///
        /// The fee is deducted from what the recipient receives and credited
        /// to the token admin, modelling a SEP-41 token that does not deliver
        /// the full transfer amount. `0` disables the fee.
        ///
        /// # Auth
        /// Requires authorization from the contract admin.
        ///
        /// # Events
        /// Emits no events.
        ///
        /// # Errors
        /// Returns [`Error::NotInitialized`] if no admin is configured, or
        /// [`Error::InvalidFee`] if `bps > 10_000`.
        pub fn set_transfer_fee_bps(env: Env, bps: u32) -> Result<(), Error> {
            let admin = Self::admin(env.clone())?;
            admin.require_auth();
            if bps > 10_000 {
                return Err(Error::InvalidFee);
            }
            env.storage().instance().set(&DataKey::TransferFeeBps, &bps);
            env.storage()
                .instance()
                .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_BUMP);
            Ok(())
        }

        /// Current fee-on-transfer rate in basis points.
        pub fn transfer_fee_bps(env: Env) -> u32 {
            env.storage()
                .instance()
                .get(&DataKey::TransferFeeBps)
                .unwrap_or(0)
        }

        /// Mint tokens to an account.
        ///
        /// # Auth
        /// Requires authorization from the contract admin.
        ///
        /// # Events
        /// Emits no events.
        ///
        /// # Errors
        /// Returns [`Error::NotInitialized`] if no admin is configured,
        /// [`Error::InvalidAmount`] if `amount <= 0`, or
        /// [`Error::AmountOverflow`] if addition overflows.
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

        /// Spendable balance of an address.
        pub fn balance(env: Env, id: Address) -> i128 {
            Self::read_balance(&env, &id)
        }

        /// Transfer tokens from one account to another.
        ///
        /// # Auth
        /// Requires authorization from `from`.
        ///
        /// # Events
        /// Emits no events.
        ///
        /// # Errors
        /// Panics with [`Error::InvalidAmount`], [`Error::TransferFailed`],
        /// [`Error::InsufficientBalance`], or [`Error::AmountOverflow`].
        pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
            from.require_auth();
            if amount <= 0 {
                panic_with_error!(&env, Error::InvalidAmount);
            }

            if Self::fail_transfers(env.clone()) || Self::fail_transfers_to(env.clone(), to.clone())
            {
                panic_with_error!(&env, Error::TransferFailed);
            }

            let from_balance = Self::read_balance(&env, &from);
            if from_balance < amount {
                panic_with_error!(&env, Error::InsufficientBalance);
            }

            let fee_bps = Self::transfer_fee_bps(env.clone());
            let fee = if fee_bps == 0 {
                0
            } else {
                let Some(product) = amount.checked_mul(i128::from(fee_bps)) else {
                    panic_with_error!(&env, Error::AmountOverflow)
                };
                product / 10_000
            };
            let net = amount - fee;

            let to_balance = Self::read_balance(&env, &to);
            let new_from = from_balance - amount;
            let Some(new_to) = to_balance.checked_add(net) else {
                panic_with_error!(&env, Error::AmountOverflow)
            };

            Self::write_balance(&env, &from, new_from);
            Self::write_balance(&env, &to, new_to);

            // The fee accrues to the admin, modelling a fee collector. A
            // transfer where the admin is the sender burns the fee instead.
            if fee > 0 {
                let admin = match Self::admin(env.clone()) {
                    Ok(admin) => admin,
                    Err(error) => panic_with_error!(&env, error),
                };
                if admin != from {
                    let admin_balance = Self::read_balance(&env, &admin);
                    let Some(new_admin) = admin_balance.checked_add(fee) else {
                        panic_with_error!(&env, Error::AmountOverflow)
                    };
                    Self::write_balance(&env, &admin, new_admin);
                }
            }
        }

        /// Number of decimals for the token (7).
        pub fn decimals(_env: Env) -> u32 {
            7
        }

        /// Human-readable token name.
        pub fn name(env: Env) -> String {
            String::from_str(&env, "Test Token")
        }

        /// Token symbol.
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
}

pub use contract::{TestToken, TestTokenClient};

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

        let contract_id = env.register(TestToken, (admin,));
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

    #[test]
    fn fee_on_transfer_deducts_from_recipient_and_accrues_to_admin() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let user_a = Address::generate(&env);
        let user_b = Address::generate(&env);

        let contract_id = env.register(TestToken, (admin.clone(),));
        let client = TestTokenClient::new(&env, &contract_id);
        client.mint(&user_a, &10_000);

        client.set_transfer_fee_bps(&100);
        assert_eq!(client.transfer_fee_bps(), 100);
        client.transfer(&user_a, &user_b, &1_000);

        assert_eq!(client.balance(&user_a), 9_000);
        assert_eq!(client.balance(&user_b), 990);
        assert_eq!(client.balance(&admin), 10);

        // Turning the fee off restores exact transfers.
        client.set_transfer_fee_bps(&0);
        client.transfer(&user_a, &user_b, &1_000);
        assert_eq!(client.balance(&user_a), 8_000);
        assert_eq!(client.balance(&user_b), 1_990);
        assert_eq!(client.balance(&admin), 10);
    }

    #[test]
    fn transfer_fee_requires_admin_and_is_capped_at_one_hundred_percent() {
        let env = Env::default();

        let admin = Address::generate(&env);
        let user_a = Address::generate(&env);
        let user_b = Address::generate(&env);

        let contract_id = env.register(TestToken, (admin,));
        let client = TestTokenClient::new(&env, &contract_id);

        env.mock_auths(&[]);
        assert!(client.try_set_transfer_fee_bps(&100).is_err());
        assert_eq!(client.transfer_fee_bps(), 0);

        env.mock_all_auths();
        assert!(client.try_set_transfer_fee_bps(&10_001).is_err());
        assert_eq!(client.transfer_fee_bps(), 0);

        // A 100% fee delivers nothing to the recipient and everything to the
        // admin; it is the largest rate the fixture accepts.
        client.set_transfer_fee_bps(&10_000);
        client.mint(&user_a, &1_000);
        client.transfer(&user_a, &user_b, &1_000);
        assert_eq!(client.balance(&user_a), 0);
        assert_eq!(client.balance(&user_b), 0);
        assert_eq!(client.transfer_fee_bps(), 10_000);
    }
}
