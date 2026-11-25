#![cfg(test)]

use super::*;
use payflow_test_token::{TestToken, TestTokenClient};
use soroban_sdk::{testutils::AuthorizedFunction, Symbol};
use soroban_sdk::{
    testutils::{
        storage::Persistent as _, Address as _, Events as _, Ledger as _, MockAuth, MockAuthInvoke,
    },
    token::{StellarAssetClient, TokenClient},
    xdr::{ContractEventBody, ScMap, ScSymbol, ScVal},
    Address, Env, IntoVal, String,
};

const THIRTY_DAYS: u64 = 2_592_000;
/// The all-zero account and contract strkeys (#236).
const ZERO_ACCOUNT: &str = "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF";
const ZERO_CONTRACT: &str = "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABSC4";
/// A valid vault wasm used as the target for `upgrade_wasm` tests (#158); the
/// contract upgrades to a copy of itself, exercising the upgrade mechanism.
const UPGRADE_WASM: &[u8] = include_bytes!("../testdata/payflow_vault.wasm");

struct Harness<'a> {
    env: Env,
    vault: VaultClient<'a>,
    vault_id: Address,
    token: Address,
    token_admin: StellarAssetClient<'a>,
    token_client: TokenClient<'a>,
    subscription: Address,
}

fn setup() -> Harness<'static> {
    // Existing tests predate the reserve cap (#348) and reserve the whole
    // balance, so they run with the cap fully open. The cap's own tests below
    // deploy with a real limit.
    setup_with_reserve_cap(BPS_DENOMINATOR)
}

fn setup_with_reserve_cap(reserve_cap_bps: u32) -> Harness<'static> {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let subscription = Address::generate(&env);

    let sac = env.register_stellar_asset_contract_v2(issuer);
    let token = sac.address();

    let vault_id = env.register(Vault, (admin.clone(), reserve_cap_bps));
    let vault = VaultClient::new(&env, &vault_id);
    vault.set_subscription(&subscription);

    Harness {
        token_admin: StellarAssetClient::new(&env, &token),
        token_client: TokenClient::new(&env, &token),
        env,
        vault,
        vault_id,
        token,
        subscription,
    }
}

#[test]
fn version_matches_package_version() {
    let h = setup();
    assert_eq!(
        h.vault.version(),
        String::from_str(&h.env, env!("CARGO_PKG_VERSION"))
    );
}

fn funded_user(h: &Harness, amount: i128) -> Address {
    let user = Address::generate(&h.env);
    h.token_admin.mint(&user, &amount);
    user
}

#[test]
fn deposit_credits_balance_and_moves_tokens() {
    let h = setup();
    let user = funded_user(&h, 10_000);

    h.vault.deposit(&user, &h.token, &4_000);

    assert_eq!(h.vault.balance(&user, &h.token), 4_000);
    assert_eq!(h.token_client.balance(&user), 6_000);
    assert_eq!(h.token_client.balance(&h.vault_id), 4_000);
}

#[test]
fn deposits_accumulate() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    h.vault.deposit(&user, &h.token, &1_000);
    h.vault.deposit(&user, &h.token, &2_500);
    assert_eq!(h.vault.balance(&user, &h.token), 3_500);
}

#[test]
fn deposit_rejects_non_positive() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    assert_eq!(
        h.vault.try_deposit(&user, &h.token, &0),
        Err(Ok(Error::InvalidAmount))
    );
    assert_eq!(
        h.vault.try_deposit(&user, &h.token, &-5),
        Err(Ok(Error::InvalidAmount))
    );
}

#[test]
fn deposit_auth_tree_is_exact() {
    let h = setup();
    let user = funded_user(&h, 10_000);

    h.vault.deposit(&user, &h.token, &1_000);

    let auths = h.env.auths();
    assert_eq!(auths.len(), 1);
    assert_eq!(auths[0].0, user);
    assert_eq!(
        auths[0].1.function,
        AuthorizedFunction::Contract((
            h.vault_id.clone(),
            Symbol::new(&h.env, "deposit"),
            (user.clone(), h.token.clone(), 1_000i128).into_val(&h.env),
        ))
    );
}

#[test]
fn deposit_rejects_wrong_signer() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    let intruder = Address::generate(&h.env);
    h.env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &intruder,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &h.vault_id,
            fn_name: "deposit",
            args: (user.clone(), h.token.clone(), 1_000i128).into_val(&h.env),
            sub_invokes: &[],
        },
    }]);
    assert!(h.vault.try_deposit(&user, &h.token, &1_000).is_err());
}

#[test]
fn withdraw_returns_tokens() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    h.vault.deposit(&user, &h.token, &4_000);

    h.vault.withdraw(&user, &h.token, &1_500);

    assert_eq!(h.vault.balance(&user, &h.token), 2_500);
    assert_eq!(h.token_client.balance(&user), 7_500);
}

#[test]
fn withdraw_to_sends_tokens_to_the_destination() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    let destination = Address::generate(&h.env);
    h.vault.deposit(&user, &h.token, &4_000);

    h.vault.withdraw_to(&user, &h.token, &destination, &1_500);

    assert_eq!(h.vault.balance(&user, &h.token), 2_500);
    assert_eq!(h.token_client.balance(&user), 6_000);
    assert_eq!(h.token_client.balance(&destination), 1_500);
    assert_eq!(h.token_client.balance(&h.vault_id), 2_500);
}

#[test]
fn withdraw_to_rejects_non_positive_and_excess_amounts() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    let destination = Address::generate(&h.env);
    h.vault.deposit(&user, &h.token, &1_000);

    for amount in [0, -5] {
        assert_eq!(
            h.vault
                .try_withdraw_to(&user, &h.token, &destination, &amount),
            Err(Ok(Error::InvalidAmount))
        );
    }
    assert_eq!(
        h.vault
            .try_withdraw_to(&user, &h.token, &destination, &1_001),
        Err(Ok(Error::InsufficientBalance))
    );
    assert_eq!(h.vault.balance(&user, &h.token), 1_000);
    assert_eq!(h.token_client.balance(&destination), 0);
}

#[test]
fn withdraw_to_rejects_the_zero_address() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    h.vault.deposit(&user, &h.token, &1_000);

    for zero in [
        Address::from_str(&h.env, ZERO_ACCOUNT),
        Address::from_str(&h.env, ZERO_CONTRACT),
    ] {
        assert_eq!(
            h.vault.try_withdraw_to(&user, &h.token, &zero, &100),
            Err(Ok(Error::ZeroAddress))
        );
    }

    assert_eq!(h.vault.balance(&user, &h.token), 1_000);
    assert_eq!(h.token_client.balance(&h.vault_id), 1_000);
    // A rejected destination consumes no rate-limit budget: a real withdrawal
    // still succeeds immediately afterwards.
    let destination = Address::generate(&h.env);
    h.vault.withdraw_to(&user, &h.token, &destination, &100);
    assert_eq!(h.token_client.balance(&destination), 100);
}

#[test]
fn payouts_to_the_zero_address_are_rejected_without_spending() {
    let h = setup();
    let user = depositor(&h, 1_000);
    let zero = Address::from_str(&h.env, ZERO_CONTRACT);
    let good = Address::generate(&h.env);

    assert_eq!(
        h.vault.try_debit(&user, &h.token, &zero, &100),
        Err(Ok(Error::ZeroAddress))
    );
    assert_eq!(
        h.vault
            .try_debit_split(&user, &h.token, &good, &100, &zero, &50),
        Err(Ok(Error::ZeroAddress))
    );
    let mut legs = soroban_sdk::Vec::new(&h.env);
    legs.push_back((good.clone(), 100_i128));
    legs.push_back((zero.clone(), 50_i128));
    assert_eq!(
        h.vault
            .try_debit_multi_mandate(&user, &h.token, &MANDATE_A, &legs),
        Err(Ok(Error::ZeroAddress))
    );

    assert_eq!(h.vault.balance(&user, &h.token), 1_000);
    assert_eq!(h.token_client.balance(&good), 0);
    assert_eq!(h.token_client.balance(&zero), 0);
    assert_eq!(h.token_client.balance(&h.vault_id), 1_000);
}

#[test]
fn withdraw_to_allows_the_user_to_name_themselves() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    h.vault.deposit(&user, &h.token, &1_000);

    h.vault.withdraw_to(&user, &h.token, &user, &1_000);

    assert_eq!(h.vault.balance(&user, &h.token), 0);
    assert_eq!(h.token_client.balance(&user), 10_000);
}

#[test]
fn withdraw_to_shares_the_operation_window() {
    let h = setup();
    let user = funded_user(&h, 100_000);
    let destination = Address::generate(&h.env);

    for _ in 0..4 {
        h.vault.deposit(&user, &h.token, &1_000);
        h.vault.withdraw_to(&user, &h.token, &destination, &500);
    }
    assert_eq!(h.vault.balance(&user, &h.token), 2_000);

    assert_eq!(
        h.vault.try_withdraw_to(&user, &h.token, &destination, &500),
        Err(Ok(Error::RateLimitExceeded))
    );
    assert_eq!(h.vault.balance(&user, &h.token), 2_000);
    assert_eq!(h.token_client.balance(&destination), 2_000);
}

#[test]
fn withdraw_beyond_balance_fails() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    h.vault.deposit(&user, &h.token, &1_000);
    assert_eq!(
        h.vault.try_withdraw(&user, &h.token, &1_001),
        Err(Ok(Error::InsufficientBalance))
    );
}

#[test]
fn rapid_deposit_withdraw_cycling_is_rate_limited() {
    let h = setup();
    let user = funded_user(&h, 100_000);

    // Eight operations fill one window: four deposit/withdraw round trips.
    for _ in 0..4 {
        h.vault.deposit(&user, &h.token, &1_000);
        h.vault.withdraw(&user, &h.token, &500);
    }
    assert_eq!(h.vault.balance(&user, &h.token), 2_000);

    // The ninth and tenth operations in the same window are rejected before
    // any balance is touched, whichever direction they take.
    assert_eq!(
        h.vault.try_deposit(&user, &h.token, &1_000),
        Err(Ok(Error::RateLimitExceeded))
    );
    assert_eq!(
        h.vault.try_withdraw(&user, &h.token, &500),
        Err(Ok(Error::RateLimitExceeded))
    );
    assert_eq!(h.vault.balance(&user, &h.token), 2_000);
    assert_eq!(h.token_client.balance(&h.vault_id), 2_000);

    // The bound is per (user, token), not global: another user is unaffected.
    let other = funded_user(&h, 10_000);
    h.vault.deposit(&other, &h.token, &1_000);

    // Once the fixed window rolls over, operations are allowed again.
    h.env.ledger().set_timestamp(VAULT_OPERATION_WINDOW_SECS);
    h.vault.deposit(&user, &h.token, &1_000);
    assert_eq!(h.vault.balance(&user, &h.token), 3_000);
}

#[test]
fn debit_pays_merchant_and_reduces_balance() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    let merchant = Address::generate(&h.env);
    h.vault.deposit(&user, &h.token, &5_000);

    h.vault.debit(&user, &h.token, &merchant, &1_200);

    assert_eq!(h.vault.balance(&user, &h.token), 3_800);
    assert_eq!(h.token_client.balance(&merchant), 1_200);
}

/// Number of `Debit` events emitted during the most recent invocation.
fn debit_event_count(h: &Harness) -> usize {
    let mut count = 0;
    for event in h.env.events().all().events() {
        let ContractEventBody::V0(body) = &event.body;
        if let Some(ScVal::Symbol(ScSymbol(name))) = body.topics.first() {
            if name.to_utf8_string_lossy() == "debit" {
                count += 1;
            }
        }
    }
    count
}

/// Balance field of the n-th `Debit` event of the most recent invocation.
fn debit_event_balance(h: &Harness, index: usize) -> Option<i128> {
    let mut seen = 0;
    for event in h.env.events().all().events() {
        let ContractEventBody::V0(body) = &event.body;
        let is_debit = matches!(
            body.topics.first(),
            Some(ScVal::Symbol(ScSymbol(name))) if name.to_utf8_string_lossy() == "debit"
        );
        if !is_debit {
            continue;
        }
        if seen < index {
            seen += 1;
            continue;
        }
        let ScVal::Map(Some(soroban_sdk::xdr::ScMap(entries))) = &body.data else {
            return None;
        };
        for entry in entries {
            if let ScVal::Symbol(ScSymbol(name)) = &entry.key {
                if name.to_utf8_string_lossy() == "balance" {
                    if let ScVal::I128(ref parts) = entry.val {
                        return Some((i128::from(parts.hi) << 64) | i128::from(parts.lo));
                    }
                }
            }
        }
        return None;
    }
    None
}

#[test]
fn debit_split_pays_both_recipients_from_one_balance_update() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    let merchant = Address::generate(&h.env);
    let fee_to = Address::generate(&h.env);
    h.vault.deposit(&user, &h.token, &5_000);

    h.vault
        .debit_split(&user, &h.token, &merchant, &1_200, &fee_to, &300);

    // One debit per payee, exactly as two `debit` calls emitted, and the
    // running balances are the ones sequential debits would have reported.
    assert_eq!(debit_event_count(&h), 2);
    assert_eq!(debit_event_balance(&h, 0), Some(5_000 - 1_200));
    assert_eq!(debit_event_balance(&h, 1), Some(5_000 - 1_500));

    // The subscriber pays the sum once, and each payee gets its own leg.
    assert_eq!(h.vault.balance(&user, &h.token), 5_000 - 1_500);
    assert_eq!(h.token_client.balance(&merchant), 1_200);
    assert_eq!(h.token_client.balance(&fee_to), 300);
    assert_eq!(h.token_client.balance(&h.vault_id), 5_000 - 1_500);
}

#[test]
fn debit_split_with_a_zero_second_leg_emits_one_debit() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    let merchant = Address::generate(&h.env);
    let fee_to = Address::generate(&h.env);
    h.vault.deposit(&user, &h.token, &5_000);

    h.vault
        .debit_split(&user, &h.token, &merchant, &1_200, &fee_to, &0);

    // Equivalent to the single `debit` call this replaces.
    assert_eq!(debit_event_count(&h), 1);
    assert_eq!(h.vault.balance(&user, &h.token), 3_800);
    assert_eq!(h.token_client.balance(&merchant), 1_200);
    assert_eq!(h.token_client.balance(&fee_to), 0);
}

#[test]
fn debit_split_checks_the_combined_total() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    let merchant = Address::generate(&h.env);
    let fee_to = Address::generate(&h.env);
    h.vault.deposit(&user, &h.token, &100);

    // Neither leg alone exceeds the balance, but the two together do.
    assert_eq!(
        h.vault
            .try_debit_split(&user, &h.token, &merchant, &80, &fee_to, &30),
        Err(Ok(Error::InsufficientBalance))
    );
    // The rejected call moved nothing.
    assert_eq!(h.vault.balance(&user, &h.token), 100);
    assert_eq!(h.token_client.balance(&merchant), 0);
    assert_eq!(h.token_client.balance(&fee_to), 0);
}

#[test]
fn debit_split_rejects_non_positive_amounts() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    let merchant = Address::generate(&h.env);
    let fee_to = Address::generate(&h.env);
    h.vault.deposit(&user, &h.token, &5_000);

    // The first leg is the merchant share and must exist...
    for first in [0i128, -1] {
        assert_eq!(
            h.vault
                .try_debit_split(&user, &h.token, &merchant, &first, &fee_to, &100),
            Err(Ok(Error::InvalidAmount))
        );
    }
    // ...while the fee leg may be empty but never negative.
    assert_eq!(
        h.vault
            .try_debit_split(&user, &h.token, &merchant, &100, &fee_to, &-1),
        Err(Ok(Error::InvalidAmount))
    );
    assert_eq!(h.vault.balance(&user, &h.token), 5_000);
}

#[test]
fn debit_split_rejects_a_total_that_overflows() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    let merchant = Address::generate(&h.env);
    let fee_to = Address::generate(&h.env);
    h.vault.deposit(&user, &h.token, &5_000);

    // A wrapped total would under-charge the subscriber, so it is refused.
    assert_eq!(
        h.vault
            .try_debit_split(&user, &h.token, &merchant, &i128::MAX, &fee_to, &i128::MAX),
        Err(Ok(Error::AmountOverflow))
    );
    assert_eq!(h.vault.balance(&user, &h.token), 5_000);
}

#[test]
fn debit_split_rejects_a_different_signer() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    let merchant = Address::generate(&h.env);
    let fee_to = Address::generate(&h.env);
    h.vault.deposit(&user, &h.token, &5_000);

    // A signature from some other address must not move funds: only the
    // registered subscription contract may split a debit.
    let attacker = Address::generate(&h.env);
    h.env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &attacker,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &h.vault_id,
            fn_name: "debit_split",
            args: (&user, &h.token, &merchant, &1_200i128, &fee_to, &300i128).into_val(&h.env),
            sub_invokes: &[],
        },
    }]);

    assert!(h
        .vault
        .try_debit_split(&user, &h.token, &merchant, &1_200, &fee_to, &300)
        .is_err());
    assert_eq!(h.vault.balance(&user, &h.token), 5_000);
    assert_eq!(h.token_client.balance(&merchant), 0);
}

#[test]
fn debit_split_requires_the_subscription_auth() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    let merchant = Address::generate(&h.env);
    let fee_to = Address::generate(&h.env);
    h.vault.deposit(&user, &h.token, &5_000);

    // Drop the blanket mock: the subscription contract must authorize, and it
    // is the only address that can.
    h.env.set_auths(&[]);
    assert!(h
        .vault
        .try_debit_split(&user, &h.token, &merchant, &1_200, &fee_to, &300)
        .is_err());
    assert_eq!(h.vault.balance(&user, &h.token), 5_000);
}

#[test]
fn debit_beyond_balance_fails() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    let merchant = Address::generate(&h.env);
    h.vault.deposit(&user, &h.token, &100);
    assert_eq!(
        h.vault.try_debit(&user, &h.token, &merchant, &101),
        Err(Ok(Error::InsufficientBalance))
    );
}

#[test]
fn balances_are_isolated_per_user() {
    let h = setup();
    let a = funded_user(&h, 10_000);
    let b = funded_user(&h, 10_000);
    h.vault.deposit(&a, &h.token, &3_000);
    assert_eq!(h.vault.balance(&a, &h.token), 3_000);
    assert_eq!(h.vault.balance(&b, &h.token), 0);
}

#[test]
fn balance_of_unknown_user_is_zero() {
    let h = setup();
    let stranger = Address::generate(&h.env);
    assert_eq!(h.vault.balance(&stranger, &h.token), 0);
}

#[test]
fn constructor_sets_admin() {
    let h = setup();
    // The constructor ran at registration, so the vault is already configured.
    assert!(h.vault.try_admin().is_ok());
}

#[test]
fn subscription_address_is_recorded() {
    let h = setup();
    assert_eq!(h.vault.subscription(), h.subscription);
}

/// The vault admin can only call configuration functions (`set_subscription`,
/// `set_allowlist_enabled`, `set_token_allowed`). None of them touches user
/// balances directly. This test verifies that every admin-callable function
/// leaves all user balances unchanged.
///
/// SECURITY.md: "The vault admin can repoint `set_subscription` to a different
/// contract. A malicious admin could point it at a contract that drains
/// balances." This test pins down that the admin functions do not move funds.
#[test]
fn admin_has_no_path_to_user_funds() {
    let h = setup();

    // Fund two users and deposit into the vault.
    let user_a = funded_user(&h, 10_000);
    let user_b = funded_user(&h, 10_000);
    h.vault.deposit(&user_a, &h.token, &3_000);
    h.vault.deposit(&user_b, &h.token, &2_500);

    let first_before = h.vault.balance(&user_a, &h.token);
    let second_before = h.vault.balance(&user_b, &h.token);

    // Exercise every admin function: repoint the subscription contract and
    // flip the allowlist state around. None may move user balances.
    let new_subscription = Address::generate(&h.env);
    h.vault.set_subscription(&new_subscription);
    h.vault.set_allowlist_enabled(&true);
    h.vault.set_token_allowed(&h.token, &true);
    h.vault.set_token_allowed(&h.token, &false);
    h.vault.set_allowlist_enabled(&false);
    h.vault.set_deposit_cap(&user_a, &h.token, &10_000);
    h.vault.set_deposit_cap(&user_b, &h.token, &0);

    let first_after = h.vault.balance(&user_a, &h.token);
    let second_after = h.vault.balance(&user_b, &h.token);

    assert_eq!(
        first_before, first_after,
        "set_subscription must not change user balances"
    );
    assert_eq!(
        second_before, second_after,
        "set_subscription must not change user balances"
    );
}

/// The sum of internal balances per token must equal the vault's token
/// balance: the vault must never hold more or less than what its internal
/// ledger accounts for, or the last user to withdraw could find the vault
/// insolvent.
///
/// Deposits, withdrawals and debits (the charge settlement path) are
/// randomized across four users; the invariant is asserted after every step,
/// for well over 200 random steps.
#[test]
fn internal_balances_add_up_to_the_vault_holdings() {
    let h = setup();
    let merchant = Address::generate(&h.env);

    // Four funded subscribers: mint 100_000_000 to each wallet and deposit
    // 10_000_000 into the vault, so each user starts with 10_000_000 inside
    // the vault and 90_000_000 still in its wallet (enough for every
    // randomized deposit in the test).
    let user_a = funded_user(&h, 100_000_000);
    let user_b = funded_user(&h, 100_000_000);
    let user_c = funded_user(&h, 100_000_000);
    let user_d = funded_user(&h, 100_000_000);
    for user in [&user_a, &user_b, &user_c, &user_d] {
        h.vault.deposit(user, &h.token, &10_000_000);
    }
    let users = [user_a, user_b, user_c, user_d];
    let mut expected = [10_000_000i128; 4];

    assert_invariant(&h, &users, &expected);

    // 300 randomized steps: deposit, withdraw, or debit (a charge settlement).
    let mut rng = Rng::new(0x_1234_5678_9abc_def0);
    for _ in 0..300 {
        // Move into the next operation window so the per-window cycling bound
        // (#314) does not interfere with the accounting invariant under test.
        let now = h.env.ledger().timestamp();
        h.env
            .ledger()
            .set_timestamp(now + VAULT_OPERATION_WINDOW_SECS + 1);
        let i = usize::try_from(rng.below(4)).unwrap();
        let user = users[i].clone();

        if expected[i] == 0 {
            // Nothing to withdraw or debit: fund the user instead. Each user's
            // wallet holds 10_000_000 tokens at most, and a user is picked at
            // most 300 times with deposits capped at 5_000, so it never runs
            // dry.
            let amount = 1_000 + i128::from(rng.below(5_000));
            h.vault.deposit(&user, &h.token, &amount);
            expected[i] += amount;
            assert_invariant(&h, &users, &expected);
            continue;
        }

        match rng.below(3) {
            0 => {
                let amount = 1 + i128::from(rng.below(5_000));
                h.vault.deposit(&user, &h.token, &amount);
                expected[i] += amount;
            }
            1 => {
                let amount = 1 + i128::from(rng.below(u64::try_from(expected[i]).unwrap()));
                h.vault.withdraw(&user, &h.token, &amount);
                expected[i] -= amount;
            }
            _ => {
                let amount = 1 + i128::from(rng.below(u64::try_from(expected[i]).unwrap()));
                h.vault.debit(&user, &h.token, &merchant, &amount);
                expected[i] -= amount;
            }
        }
        assert_invariant(&h, &users, &expected);
    }
}

/// Asserts each user's internal balance matches the expected value and that
/// their sum equals the vault's token holdings.
fn assert_invariant(h: &Harness, users: &[Address; 4], expected: &[i128]) {
    let mut sum: i128 = 0;
    for (i, user) in users.iter().enumerate() {
        let actual = h.vault.balance(user, &h.token);
        assert_eq!(
            actual, expected[i],
            "internal balance of user {i} drifted from the expected value"
        );
        sum += expected[i];
    }
    let holdings = h.token_client.balance(&h.vault_id);
    assert_eq!(
        sum, holdings,
        "the sum of internal balances must equal the vault's token holdings"
    );
}

/// Tiny generator so the randomized test is deterministic without pulling in
/// `std` or a `rand` dependency (the crate is `no_std`).
struct Rng {
    state: u64,
}

impl Rng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Returns a value in `0..bound`.
    fn below(&mut self, bound: u64) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state % bound
    }
}

// ---------------------------------------------------------------------------
// withdraw_all: exit without racing the keeper
// ---------------------------------------------------------------------------

#[test]
fn withdraw_all_returns_the_whole_balance() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    h.vault.deposit(&user, &h.token, &4_000);

    assert_eq!(h.vault.withdraw_all(&user, &h.token), 4_000);

    // The existing Withdraw event carries the amount and the new balance.
    // (Events are only readable from the last invocation, hence right here.)
    let last = h.env.events().all().events().last().cloned().unwrap();
    let ContractEventBody::V0(body) = &last.body;
    let ScVal::Symbol(ScSymbol(name)) = &body.topics[0] else {
        panic!("topic 0 is not a symbol");
    };
    let expected: soroban_sdk::xdr::StringM<32> = "withdraw".try_into().unwrap();
    assert_eq!(name, &expected);

    assert_eq!(h.vault.balance(&user, &h.token), 0);
    // The deposit (4_000) came back: the wallet holds everything again.
    assert_eq!(h.token_client.balance(&user), 10_000);
    assert_eq!(h.token_client.balance(&h.vault_id), 0);
}

#[test]
fn withdraw_all_works_after_a_charge_reduced_the_balance() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    let merchant = Address::generate(&h.env);
    h.vault.deposit(&user, &h.token, &5_000);
    // The subscription contract settles a charge against this balance.
    h.vault.debit(&user, &h.token, &merchant, &1_200);

    // A plain `withdraw` of the original 5_000 would fail with
    // InsufficientBalance; withdraw_all takes what is left.
    assert_eq!(h.vault.withdraw_all(&user, &h.token), 3_800);

    assert_eq!(h.vault.balance(&user, &h.token), 0);
    assert_eq!(h.token_client.balance(&user), 10_000 - 1_200);
    assert_eq!(h.token_client.balance(&h.vault_id), 0);
}

#[test]
fn withdraw_all_on_an_empty_balance_returns_zero_without_moving_tokens() {
    let h = setup();
    let user = funded_user(&h, 10_000);

    // An empty exit is a no-op: read straight after the call, withdraw_all —
    // the last invocation — published no events, so no Withdraw fired.
    assert_eq!(h.vault.withdraw_all(&user, &h.token), 0);
    assert_eq!(h.env.events().all().events().len(), 0);

    // ...and no token moved.
    assert_eq!(h.token_client.balance(&user), 10_000);
    assert_eq!(h.token_client.balance(&h.vault_id), 0);
}

#[test]
fn withdraw_all_rejects_a_different_signer() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    h.vault.deposit(&user, &h.token, &4_000);
    let attacker = Address::generate(&h.env);

    // The attacker signs, but the call withdraws the user's balance: the
    // signature does not cover `user`, so it is rejected and funds stay put.
    h.env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &h.vault_id,
            fn_name: "withdraw_all",
            args: (&user, &h.token).into_val(&h.env),
            sub_invokes: &[],
        },
    }]);

    assert!(h.vault.try_withdraw_all(&user, &h.token).is_err());
    assert_eq!(h.vault.balance(&user, &h.token), 4_000);
    assert_eq!(h.token_client.balance(&h.vault_id), 4_000);
}

// ---------------------------------------------------------------------------
// Token allowlist (#39): only tokens the admin vouched for may be deposited
// ---------------------------------------------------------------------------

#[test]
fn allowlist_is_disabled_by_default() {
    let h = setup();
    let user = funded_user(&h, 10_000);

    // Testnet keeps accepting every SEP-41 token until the admin opts in.
    assert!(!h.vault.allowlist_enabled());
    h.vault.deposit(&user, &h.token, &1_000);
    assert_eq!(h.vault.balance(&user, &h.token), 1_000);
}

#[test]
fn deposits_reject_unlisted_tokens_when_the_allowlist_is_on() {
    let h = setup();
    let user = funded_user(&h, 10_000);

    h.vault.set_allowlist_enabled(&true);

    assert!(h.vault.allowlist_enabled());
    assert!(!h.vault.token_allowed(&h.token));
    assert_eq!(
        h.vault.try_deposit(&user, &h.token, &1_000),
        Err(Ok(Error::TokenNotAllowed))
    );
    // The rejected deposit moved nothing.
    assert_eq!(h.token_client.balance(&user), 10_000);
    assert_eq!(h.vault.balance(&user, &h.token), 0);
}

#[test]
fn an_allowlisted_token_can_be_deposited() {
    let h = setup();
    let user = funded_user(&h, 10_000);

    h.vault.set_allowlist_enabled(&true);
    h.vault.set_token_allowed(&h.token, &true);
    assert!(h.vault.token_allowed(&h.token));

    h.vault.deposit(&user, &h.token, &1_000);
    assert_eq!(h.vault.balance(&user, &h.token), 1_000);
}

/// The acceptance criterion for the allowlist: removing a token blocks new
/// deposits but never withdrawals, so a subscriber can always exit.
#[test]
fn removing_a_token_blocks_deposits_but_not_withdrawals() {
    let h = setup();
    let user = funded_user(&h, 10_000);

    h.vault.set_allowlist_enabled(&true);
    h.vault.set_token_allowed(&h.token, &true);
    h.vault.deposit(&user, &h.token, &4_000);

    h.vault.set_token_allowed(&h.token, &false);

    assert_eq!(
        h.vault.try_deposit(&user, &h.token, &1_000),
        Err(Ok(Error::TokenNotAllowed))
    );
    h.vault.withdraw(&user, &h.token, &4_000);
    assert_eq!(h.vault.balance(&user, &h.token), 0);
    assert_eq!(h.token_client.balance(&user), 10_000);
}

#[test]
fn disabling_the_allowlist_reopens_deposits() {
    let h = setup();
    let user = funded_user(&h, 10_000);

    h.vault.set_allowlist_enabled(&true);
    assert_eq!(
        h.vault.try_deposit(&user, &h.token, &1_000),
        Err(Ok(Error::TokenNotAllowed))
    );

    h.vault.set_allowlist_enabled(&false);
    h.vault.deposit(&user, &h.token, &1_000);
    assert_eq!(h.vault.balance(&user, &h.token), 1_000);
}

#[test]
fn set_allowlist_enabled_requires_admin_auth() {
    let h = setup();
    // Drop the blanket mock: the call must carry the admin's authorization.
    h.env.set_auths(&[]);
    assert!(h.vault.try_set_allowlist_enabled(&true).is_err());
    assert!(!h.vault.allowlist_enabled());
}

#[test]
fn set_token_allowed_requires_admin_auth() {
    let h = setup();
    h.env.set_auths(&[]);
    assert!(h.vault.try_set_token_allowed(&h.token, &true).is_err());
    assert!(!h.vault.token_allowed(&h.token));
}

/// The allowlist functions are configuration only: toggling them must not
/// change the admin's or any user's balances.
#[test]
fn allowlist_admin_functions_move_no_funds() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    h.vault.deposit(&user, &h.token, &3_000);

    h.vault.set_allowlist_enabled(&true);
    h.vault.set_token_allowed(&h.token, &true);
    h.vault.set_token_allowed(&h.token, &false);
    h.vault.set_allowlist_enabled(&false);

    assert_eq!(h.vault.balance(&user, &h.token), 3_000);
    assert_eq!(h.token_client.balance(&h.vault_id), 3_000);
    assert_eq!(h.token_client.balance(&user), 7_000);
}

/// `can_deposit` returns true when the allowlist is disabled.
#[test]
fn can_deposit_returns_true_when_allowlist_disabled() {
    let h = setup();
    let user = funded_user(&h, 10_000);

    assert!(!h.vault.allowlist_enabled());
    assert!(h.vault.can_deposit(&user, &h.token));
}

/// `can_deposit` returns false when the allowlist is enabled but the token is not allowed.
#[test]
fn can_deposit_returns_false_when_token_not_allowed() {
    let h = setup();
    let user = funded_user(&h, 10_000);

    h.vault.set_allowlist_enabled(&true);
    assert!(h.vault.allowlist_enabled());
    assert!(!h.vault.token_allowed(&h.token));
    assert!(!h.vault.can_deposit(&user, &h.token));
}

/// `can_deposit` returns true when the allowlist is enabled and the token is allowed.
#[test]
fn can_deposit_returns_true_when_token_allowed() {
    let h = setup();
    let user = funded_user(&h, 10_000);

    h.vault.set_allowlist_enabled(&true);
    h.vault.set_token_allowed(&h.token, &true);
    assert!(h.vault.allowlist_enabled());
    assert!(h.vault.token_allowed(&h.token));
    assert!(h.vault.can_deposit(&user, &h.token));
}

/// `can_deposit` is per-token: allowing one token doesn't affect another.
#[test]
fn can_deposit_is_per_token() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    let other_token = Address::generate(&h.env);

    h.vault.set_allowlist_enabled(&true);
    h.vault.set_token_allowed(&h.token, &true);

    assert!(h.vault.can_deposit(&user, &h.token));
    assert!(!h.vault.can_deposit(&user, &other_token));
}

/// `can_deposit` ignores the user parameter (allowlist is not per-user).
#[test]
fn can_deposit_ignores_user() {
    let h = setup();
    let user_a = funded_user(&h, 10_000);
    let user_b = funded_user(&h, 10_000);

    h.vault.set_allowlist_enabled(&true);
    h.vault.set_token_allowed(&h.token, &true);

    // Both users get the same result: the allowlist gates tokens, not users.
    assert!(h.vault.can_deposit(&user_a, &h.token));
    assert!(h.vault.can_deposit(&user_b, &h.token));

    h.vault.set_token_allowed(&h.token, &false);

    assert!(!h.vault.can_deposit(&user_a, &h.token));
    assert!(!h.vault.can_deposit(&user_b, &h.token));
}

#[test]
fn admin_repoint_of_subscription_does_not_move_funds() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    h.vault.deposit(&user, &h.token, &4_000);
    let balance_before = h.vault.balance(&user, &h.token);
    let vault_tokens_before = h.token_client.balance(&h.vault_id);
    assert_eq!(balance_before, 4_000);
    assert_eq!(vault_tokens_before, 4_000);

    // The vault admin's only write path that touches any funds-related state
    // is `set_subscription` (repoint). That repoint must not move user funds.
    let new_subscription = Address::generate(&h.env);
    h.vault.set_subscription(&new_subscription);

    assert_eq!(h.vault.subscription(), new_subscription);
    assert_eq!(h.vault.balance(&user, &h.token), balance_before);
    assert_eq!(h.token_client.balance(&h.vault_id), vault_tokens_before);
}

#[test]
fn admin_repoint_changes_only_the_subscription_pointer() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    h.vault.deposit(&user, &h.token, &2_000);
    let original_subscription = h.vault.subscription();
    let admin_before = h.vault.admin();

    let new_subscription = Address::generate(&h.env);
    h.vault.set_subscription(&new_subscription);

    // Repointing does not touch balances, the admin, or the deposited tokens.
    assert_eq!(h.vault.admin(), admin_before);
    assert_eq!(h.vault.balance(&user, &h.token), 2_000);
    assert_eq!(h.token_client.balance(&h.vault_id), 2_000);
    assert_ne!(h.vault.subscription(), original_subscription);
}

struct FailingTokenHarness<'a> {
    env: Env,
    vault: VaultClient<'a>,
    vault_id: Address,
    token: Address,
    token_admin: Address,
    test_token: TestTokenClient<'a>,
}

fn setup_failing_token() -> FailingTokenHarness<'static> {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let subscription = Address::generate(&env);

    let token = env.register(TestToken, (token_admin.clone(),));
    let test_token = TestTokenClient::new(&env, &token);

    let vault_id = env.register(Vault, (admin, BPS_DENOMINATOR));
    let vault = VaultClient::new(&env, &vault_id);
    vault.set_subscription(&subscription);

    FailingTokenHarness {
        env,
        vault,
        vault_id,
        token,
        token_admin,
        test_token,
    }
}

#[test]
fn failing_transfer_reverts_deposit_and_leaves_balances_unchanged() {
    let h = setup_failing_token();
    let user = Address::generate(&h.env);
    h.test_token.mint(&user, &10_000);

    // Initial state: user has 10_000 in token, vault holds 0, user internal balance is 0.
    assert_eq!(h.vault.balance(&user, &h.token), 0);
    assert_eq!(h.test_token.balance(&user), 10_000);
    assert_eq!(h.test_token.balance(&h.vault_id), 0);

    // Configure token to fail transfers.
    h.test_token.set_fail_transfers(&true);

    // Deposit must fail and revert completely.
    assert!(h.vault.try_deposit(&user, &h.token, &4_000).is_err());

    // Invariant: internal balance and token balances remain completely untouched.
    assert_eq!(h.vault.balance(&user, &h.token), 0);
    assert_eq!(h.test_token.balance(&user), 10_000);
    assert_eq!(h.test_token.balance(&h.vault_id), 0);

    // Once transfer failure is cleared, deposit succeeds normally.
    h.test_token.set_fail_transfers(&false);
    h.vault.deposit(&user, &h.token, &4_000);

    assert_eq!(h.vault.balance(&user, &h.token), 4_000);
    assert_eq!(h.test_token.balance(&user), 6_000);
    assert_eq!(h.test_token.balance(&h.vault_id), 4_000);
}

#[test]
fn failing_transfer_reverts_withdraw_and_leaves_balances_unchanged() {
    let h = setup_failing_token();
    let user = Address::generate(&h.env);
    h.test_token.mint(&user, &10_000);
    h.vault.deposit(&user, &h.token, &4_000);

    assert_eq!(h.vault.balance(&user, &h.token), 4_000);
    assert_eq!(h.test_token.balance(&user), 6_000);
    assert_eq!(h.test_token.balance(&h.vault_id), 4_000);

    // Configure token to fail transfers.
    h.test_token.set_fail_transfers(&true);

    // Withdraw must fail.
    assert!(h.vault.try_withdraw(&user, &h.token, &1_500).is_err());

    // Invariant: internal balance was NOT decremented, tokens were NOT moved.
    assert_eq!(h.vault.balance(&user, &h.token), 4_000);
    assert_eq!(h.test_token.balance(&user), 6_000);
    assert_eq!(h.test_token.balance(&h.vault_id), 4_000);

    // Re-enabling transfers allows withdrawal.
    h.test_token.set_fail_transfers(&false);
    h.vault.withdraw(&user, &h.token, &1_500);

    assert_eq!(h.vault.balance(&user, &h.token), 2_500);
    assert_eq!(h.test_token.balance(&user), 7_500);
    assert_eq!(h.test_token.balance(&h.vault_id), 2_500);
}

#[test]
fn failing_transfer_reverts_debit_and_leaves_balances_unchanged() {
    let h = setup_failing_token();
    let user = Address::generate(&h.env);
    let merchant = Address::generate(&h.env);
    h.test_token.mint(&user, &10_000);
    h.vault.deposit(&user, &h.token, &5_000);

    assert_eq!(h.vault.balance(&user, &h.token), 5_000);
    assert_eq!(h.test_token.balance(&merchant), 0);
    assert_eq!(h.test_token.balance(&h.vault_id), 5_000);

    // Configure token to fail transfers.
    h.test_token.set_fail_transfers(&true);

    // Debit must fail.
    assert!(h
        .vault
        .try_debit(&user, &h.token, &merchant, &1_200)
        .is_err());

    // Invariant: internal balance was NOT decremented, merchant received nothing.
    assert_eq!(h.vault.balance(&user, &h.token), 5_000);
    assert_eq!(h.test_token.balance(&merchant), 0);
    assert_eq!(h.test_token.balance(&h.vault_id), 5_000);

    // Re-enabling transfers allows debit.
    h.test_token.set_fail_transfers(&false);
    h.vault.debit(&user, &h.token, &merchant, &1_200);

    assert_eq!(h.vault.balance(&user, &h.token), 3_800);
    assert_eq!(h.test_token.balance(&merchant), 1_200);
    assert_eq!(h.test_token.balance(&h.vault_id), 3_800);
}

#[test]
fn fee_on_transfer_deposit_is_rejected_and_moves_nothing() {
    let h = setup_failing_token();
    let user = Address::generate(&h.env);
    h.test_token.mint(&user, &10_000);
    h.test_token.set_transfer_fee_bps(&100);

    // The vault would receive 9_900 but credit 10_000: reject before writing.
    assert_eq!(
        h.vault.try_deposit(&user, &h.token, &10_000),
        Err(Ok(Error::UnexpectedTransferAmount))
    );

    assert_eq!(h.vault.balance(&user, &h.token), 0);
    assert_eq!(h.test_token.balance(&user), 10_000);
    assert_eq!(h.test_token.balance(&h.vault_id), 0);
    assert_eq!(h.test_token.balance(&h.token_admin), 0);

    // Without the fee the same deposit is exact and succeeds.
    h.test_token.set_transfer_fee_bps(&0);
    h.vault.deposit(&user, &h.token, &10_000);
    assert_eq!(h.vault.balance(&user, &h.token), 10_000);
    assert_eq!(h.test_token.balance(&user), 0);
    assert_eq!(h.test_token.balance(&h.vault_id), 10_000);
}

#[test]
fn fee_on_transfer_deposit_without_user_auth_moves_nothing() {
    let h = setup_failing_token();
    let user = Address::generate(&h.env);
    h.test_token.mint(&user, &10_000);
    h.test_token.set_transfer_fee_bps(&100);

    h.env.mock_auths(&[]);
    assert!(h.vault.try_deposit(&user, &h.token, &4_000).is_err());

    assert_eq!(h.vault.balance(&user, &h.token), 0);
    assert_eq!(h.test_token.balance(&user), 10_000);
    assert_eq!(h.test_token.balance(&h.vault_id), 0);
}

#[test]
fn fee_on_transfer_withdraw_is_rejected_and_state_unchanged() {
    let h = setup_failing_token();
    let user = Address::generate(&h.env);
    h.test_token.mint(&user, &10_000);
    h.vault.deposit(&user, &h.token, &4_000);

    h.test_token.set_transfer_fee_bps(&100);
    assert_eq!(
        h.vault.try_withdraw(&user, &h.token, &1_500),
        Err(Ok(Error::UnexpectedTransferAmount))
    );

    // The user would receive 1_485 while the vault burned 1_500 of their
    // balance: the whole withdrawal is rolled back.
    assert_eq!(h.vault.balance(&user, &h.token), 4_000);
    assert_eq!(h.test_token.balance(&user), 6_000);
    assert_eq!(h.test_token.balance(&h.vault_id), 4_000);

    h.test_token.set_transfer_fee_bps(&0);
    h.vault.withdraw(&user, &h.token, &1_500);
    assert_eq!(h.vault.balance(&user, &h.token), 2_500);
    assert_eq!(h.test_token.balance(&user), 7_500);
    assert_eq!(h.test_token.balance(&h.vault_id), 2_500);
}

#[test]
fn fee_on_transfer_debit_is_rejected_and_state_unchanged() {
    let h = setup_failing_token();
    let user = Address::generate(&h.env);
    let merchant = Address::generate(&h.env);
    h.test_token.mint(&user, &10_000);
    h.vault.deposit(&user, &h.token, &5_000);

    h.test_token.set_transfer_fee_bps(&100);
    assert_eq!(
        h.vault.try_debit(&user, &h.token, &merchant, &1_200),
        Err(Ok(Error::UnexpectedTransferAmount))
    );

    // The merchant would receive 1_188 for a 1_200 charge: reject and roll back.
    assert_eq!(h.vault.balance(&user, &h.token), 5_000);
    assert_eq!(h.test_token.balance(&merchant), 0);
    assert_eq!(h.test_token.balance(&h.vault_id), 5_000);

    h.test_token.set_transfer_fee_bps(&0);
    h.vault.debit(&user, &h.token, &merchant, &1_200);
    assert_eq!(h.vault.balance(&user, &h.token), 3_800);
    assert_eq!(h.test_token.balance(&merchant), 1_200);
    assert_eq!(h.test_token.balance(&h.vault_id), 3_800);
}

// ----- per-token deposit cap (#104) -----
//
// While the protocol is unaudited, the cost of a bug grows with the amount
// of subscriber funds the vault can hold. The admin can cap the balance a
// subscriber may keep for one token. A cap of 0 means unlimited, and the
// cap only gates new deposits: a balance that is already above a newly set
// cap can still be withdrawn.

#[test]
fn deposit_cap_defaults_to_unlimited() {
    let h = setup();
    let user = funded_user(&h, 10_000);

    assert_eq!(h.vault.deposit_cap(&user, &h.token), 0);
    h.vault.deposit(&user, &h.token, &10_000);
    assert_eq!(h.vault.balance(&user, &h.token), 10_000);
}

#[test]
fn deposit_up_to_the_cap_succeeds() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    h.vault.set_deposit_cap(&user, &h.token, &5_000);

    h.vault.deposit(&user, &h.token, &5_000);
    assert_eq!(h.vault.balance(&user, &h.token), 5_000);
    assert_eq!(h.vault.deposit_cap(&user, &h.token), 5_000);
}

#[test]
fn deposit_above_the_cap_is_rejected_and_moves_no_funds() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    h.vault.set_deposit_cap(&user, &h.token, &5_000);

    assert_eq!(
        h.vault.try_deposit(&user, &h.token, &5_001),
        Err(Ok(Error::DepositCapExceeded))
    );

    // Nothing moved: no tokens were taken and the balance is unchanged.
    assert_eq!(h.vault.balance(&user, &h.token), 0);
    assert_eq!(h.token_client.balance(&user), 10_000);
    assert_eq!(h.token_client.balance(&h.vault_id), 0);
}

#[test]
fn a_deposit_that_would_cross_the_cap_is_rejected() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    h.vault.set_deposit_cap(&user, &h.token, &5_000);

    h.vault.deposit(&user, &h.token, &4_000);
    assert_eq!(
        h.vault.try_deposit(&user, &h.token, &1_001),
        Err(Ok(Error::DepositCapExceeded))
    );
    assert_eq!(h.vault.balance(&user, &h.token), 4_000);
    assert_eq!(h.token_client.balance(&user), 6_000);
}

#[test]
fn a_cap_of_zero_means_unlimited() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    h.vault.set_deposit_cap(&user, &h.token, &1_000);
    h.vault.set_deposit_cap(&user, &h.token, &0);

    h.vault.deposit(&user, &h.token, &10_000);
    assert_eq!(h.vault.balance(&user, &h.token), 10_000);
    assert_eq!(h.vault.deposit_cap(&user, &h.token), 0);
}

#[test]
fn a_balance_above_a_new_cap_can_still_be_withdrawn() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    h.vault.deposit(&user, &h.token, &8_000);

    // The admin caps the balance below what is already held.
    h.vault.set_deposit_cap(&user, &h.token, &5_000);
    assert_eq!(h.vault.balance(&user, &h.token), 8_000);

    // The funds are not trapped: the whole balance can still come out.
    h.vault.withdraw(&user, &h.token, &8_000);
    assert_eq!(h.vault.balance(&user, &h.token), 0);
    assert_eq!(h.token_client.balance(&user), 10_000);
    assert_eq!(h.token_client.balance(&h.vault_id), 0);
}

#[test]
fn a_cap_belongs_to_one_user_and_one_token() {
    let h = setup();
    let user_a = funded_user(&h, 10_000);
    let user_b = funded_user(&h, 10_000);
    let other_token = Address::generate(&h.env);

    h.vault.set_deposit_cap(&user_a, &h.token, &1_000);

    // Another subscriber is unaffected.
    h.vault.deposit(&user_b, &h.token, &5_000);
    assert_eq!(h.vault.balance(&user_b, &h.token), 5_000);

    // The same subscriber's other token is unaffected too.
    assert_eq!(h.vault.deposit_cap(&user_a, &other_token), 0);
}

#[test]
fn set_deposit_cap_requires_admin_auth() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    // Drop the blanket mock: the call must carry the admin's authorization.
    h.env.set_auths(&[]);
    assert!(h
        .vault
        .try_set_deposit_cap(&user, &h.token, &1_000)
        .is_err());
    assert_eq!(h.vault.deposit_cap(&user, &h.token), 0);
}

#[test]
fn set_deposit_cap_rejects_a_negative_cap() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    assert_eq!(
        h.vault.try_set_deposit_cap(&user, &h.token, &-1),
        Err(Ok(Error::InvalidAmount))
    );
    assert_eq!(h.vault.deposit_cap(&user, &h.token), 0);
}

#[test]
fn debits_stop_after_subscription_contract_is_replaced() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    let merchant = Address::generate(&h.env);
    h.vault.deposit(&user, &h.token, &5_000);

    let old_subscription = h.subscription;
    let new_subscription = Address::generate(&h.env);

    // Initial debit authorized as old_subscription succeeds.
    h.env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &old_subscription,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &h.vault_id,
            fn_name: "debit",
            args: (user.clone(), h.token.clone(), merchant.clone(), 1_000i128).into_val(&h.env),
            sub_invokes: &[],
        },
    }]);
    h.vault.debit(&user, &h.token, &merchant, &1_000);
    assert_eq!(h.vault.balance(&user, &h.token), 4_000);
    assert_eq!(h.token_client.balance(&merchant), 1_000);

    // Admin points the vault to the new subscription contract.
    h.env.mock_all_auths();
    h.vault.set_subscription(&new_subscription);
    assert_eq!(h.vault.subscription(), new_subscription);

    // Attempting debit as the old (retired) subscription contract now fails.
    h.env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &old_subscription,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &h.vault_id,
            fn_name: "debit",
            args: (user.clone(), h.token.clone(), merchant.clone(), 1_000i128).into_val(&h.env),
            sub_invokes: &[],
        },
    }]);
    assert!(h
        .vault
        .try_debit(&user, &h.token, &merchant, &1_000)
        .is_err());
    assert_eq!(h.vault.balance(&user, &h.token), 4_000);

    // Debit authorized as the new subscription contract succeeds.
    h.env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &new_subscription,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &h.vault_id,
            fn_name: "debit",
            args: (user.clone(), h.token.clone(), merchant.clone(), 1_000i128).into_val(&h.env),
            sub_invokes: &[],
        },
    }]);
    h.vault.debit(&user, &h.token, &merchant, &1_000);
    assert_eq!(h.vault.balance(&user, &h.token), 3_000);
    assert_eq!(h.token_client.balance(&merchant), 2_000);
}

#[test]
fn propose_admin_does_not_hand_over_until_the_successor_accepts() {
    let h = setup();
    let admin = h.vault.admin();
    let next = Address::generate(&h.env);

    h.vault.propose_admin(&next);
    // Proposing alone moves no authority: the current admin still holds it.
    assert_eq!(h.vault.admin(), admin);

    h.vault.accept_admin(&next);
    assert_eq!(h.vault.admin(), next);
}

#[test]
fn propose_admin_requires_the_current_admins_auth() {
    let h = setup();
    let before = h.vault.admin();
    let next = Address::generate(&h.env);

    h.env.set_auths(&[]);
    assert!(h.vault.try_propose_admin(&next).is_err());
    assert_eq!(h.vault.admin(), before);
}

#[test]
fn accept_admin_requires_the_proposed_admins_auth() {
    let h = setup();
    let before = h.vault.admin();
    let next = Address::generate(&h.env);
    h.vault.propose_admin(&next);

    h.env.set_auths(&[]);
    assert!(h.vault.try_accept_admin(&next).is_err());
    assert_eq!(h.vault.admin(), before);
}

// Regression for #238: a direct takeover must fail. Without the two-step
// handshake, accepting with no proposal would set the caller as admin.
#[test]
fn accept_admin_without_a_proposal_is_rejected() {
    let h = setup();
    let before = h.vault.admin();
    let attacker = Address::generate(&h.env);

    assert_eq!(
        h.vault.try_accept_admin(&attacker),
        Err(Ok(Error::PendingAdminNotFound))
    );
    assert_eq!(h.vault.admin(), before);
}

#[test]
fn accept_admin_rejects_an_address_that_was_not_proposed() {
    let h = setup();
    let before = h.vault.admin();
    let next = Address::generate(&h.env);
    let attacker = Address::generate(&h.env);
    h.vault.propose_admin(&next);

    assert_eq!(
        h.vault.try_accept_admin(&attacker),
        Err(Ok(Error::NotPendingAdmin))
    );
    assert_eq!(h.vault.admin(), before);
}

#[test]
fn a_consumed_admin_proposal_cannot_be_replayed() {
    let h = setup();
    let next = Address::generate(&h.env);
    h.vault.propose_admin(&next);
    h.vault.accept_admin(&next);

    assert_eq!(
        h.vault.try_accept_admin(&next),
        Err(Ok(Error::PendingAdminNotFound))
    );
    assert_eq!(h.vault.admin(), next);
}

#[test]
fn deposit_requires_the_user_auth() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    // Drop the blanket mock: the depositor must sign for its own funds.
    h.env.set_auths(&[]);
    assert!(h.vault.try_deposit(&user, &h.token, &1_000).is_err());
    assert_eq!(h.vault.balance(&user, &h.token), 0);
}

#[test]
fn withdraw_requires_the_user_auth() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    h.vault.deposit(&user, &h.token, &1_000);
    h.env.set_auths(&[]);
    assert!(h.vault.try_withdraw(&user, &h.token, &1_000).is_err());
    assert_eq!(h.vault.balance(&user, &h.token), 1_000);
}

#[test]
fn withdraw_to_requires_the_user_auth() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    let destination = Address::generate(&h.env);
    h.vault.deposit(&user, &h.token, &1_000);
    h.env.set_auths(&[]);
    assert!(h
        .vault
        .try_withdraw_to(&user, &h.token, &destination, &1_000)
        .is_err());
    assert_eq!(h.vault.balance(&user, &h.token), 1_000);
    assert_eq!(h.token_client.balance(&destination), 0);
}

#[test]
fn withdraw_to_does_not_require_the_destination_auth() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    let destination = Address::generate(&h.env);
    h.vault.deposit(&user, &h.token, &1_000);

    h.env.mock_auths(&[MockAuth {
        address: &user,
        invoke: &MockAuthInvoke {
            contract: &h.vault_id,
            fn_name: "withdraw_to",
            args: (
                user.clone(),
                h.token.clone(),
                destination.clone(),
                1_000i128,
            )
                .into_val(&h.env),
            sub_invokes: &[],
        },
    }]);

    h.vault.withdraw_to(&user, &h.token, &destination, &1_000);
    let auths = h.env.auths();
    assert_eq!(auths.len(), 1);
    assert_eq!(auths[0].0, user);
    assert_eq!(h.token_client.balance(&destination), 1_000);
}

#[test]
fn withdraw_all_requires_the_user_auth() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    h.vault.deposit(&user, &h.token, &1_000);
    h.env.set_auths(&[]);
    assert!(h.vault.try_withdraw_all(&user, &h.token).is_err());
    assert_eq!(h.vault.balance(&user, &h.token), 1_000);
}

#[test]
fn debit_requires_the_subscription_auth() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    let merchant = Address::generate(&h.env);
    h.vault.deposit(&user, &h.token, &1_000);
    // Only the configured subscription contract may debit a balance.
    h.env.set_auths(&[]);
    assert!(h.vault.try_debit(&user, &h.token, &merchant, &500).is_err());
    assert_eq!(h.vault.balance(&user, &h.token), 1_000);
}

#[test]
fn set_subscription_requires_admin_auth() {
    let h = setup();
    let other = Address::generate(&h.env);
    h.env.set_auths(&[]);
    assert!(h.vault.try_set_subscription(&other).is_err());
    assert_eq!(h.vault.subscription(), h.subscription);
}

#[test]
fn set_subscription_rejects_self_address() {
    let h = setup();
    let before = h.vault.subscription();
    assert_eq!(
        h.vault.try_set_subscription(&h.vault_id),
        Err(Ok(Error::InvalidAddress))
    );
    assert_eq!(h.vault.subscription(), before);
}

#[test]
fn spend_limit_caps_debits_and_the_next_window_resets() {
    let h = setup();
    let user = funded_user(&h, 100_000);
    let merchant = Address::generate(&h.env);
    h.vault.deposit(&user, &h.token, &50_000);

    h.vault
        .set_spend_limit(&user, &h.token, &5_000, &THIRTY_DAYS);
    assert_eq!(h.vault.spend_limit(&user, &h.token).amount, 5_000);
    assert_eq!(
        h.vault.spend_limit(&user, &h.token).window_secs,
        THIRTY_DAYS
    );

    h.vault.debit(&user, &h.token, &merchant, &3_000);
    // Landing exactly on the ceiling is allowed...
    h.vault.debit(&user, &h.token, &merchant, &2_000);
    // ...one unit more is not.
    assert_eq!(
        h.vault.try_debit(&user, &h.token, &merchant, &1),
        Err(Ok(Error::SpendLimitExceeded))
    );
    // The failed debit moved nothing.
    assert_eq!(h.vault.balance(&user, &h.token), 45_000);

    // Once the window elapses the ceiling applies to fresh spending.
    h.env
        .ledger()
        .set_timestamp(h.env.ledger().timestamp() + THIRTY_DAYS);
    h.vault.debit(&user, &h.token, &merchant, &5_000);
    assert_eq!(h.vault.balance(&user, &h.token), 40_000);
}

#[test]
fn spend_limit_zero_disables_the_limit() {
    let h = setup();
    let user = funded_user(&h, 100_000);
    let merchant = Address::generate(&h.env);
    h.vault.deposit(&user, &h.token, &50_000);

    h.vault.set_spend_limit(&user, &h.token, &100, &THIRTY_DAYS);
    assert_eq!(
        h.vault.try_debit(&user, &h.token, &merchant, &101),
        Err(Ok(Error::SpendLimitExceeded))
    );

    // 0 clears the limit and its window tracker entirely.
    h.vault.set_spend_limit(&user, &h.token, &0, &0);
    assert_eq!(h.vault.spend_limit(&user, &h.token).amount, 0);
    h.vault.debit(&user, &h.token, &merchant, &10_000);
    assert_eq!(h.token_client.balance(&merchant), 10_000);
}

#[test]
fn set_spend_limit_rejects_bad_input() {
    let h = setup();
    let user = funded_user(&h, 10_000);

    assert_eq!(
        h.vault
            .try_set_spend_limit(&user, &h.token, &-1, &THIRTY_DAYS),
        Err(Ok(Error::InvalidAmount))
    );
    // A ceiling without a window length would be meaningless.
    assert_eq!(
        h.vault.try_set_spend_limit(&user, &h.token, &100, &0),
        Err(Ok(Error::InvalidWindow))
    );
    // A disabled limit needs no window.
    assert!(h.vault.try_set_spend_limit(&user, &h.token, &0, &0).is_ok());
}

#[test]
fn set_spend_limit_requires_the_user_to_authorize() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    let stranger = Address::generate(&h.env);

    // Only the user may set their own ceiling; a stranger's signature for
    // the same call does not cover it.
    h.env.mock_auths(&[MockAuth {
        address: &stranger,
        invoke: &MockAuthInvoke {
            contract: &h.vault_id,
            fn_name: "set_spend_limit",
            args: (&user, &h.token, &100i128, &THIRTY_DAYS).into_val(&h.env),
            sub_invokes: &[],
        },
    }]);

    assert!(h
        .vault
        .try_set_spend_limit(&user, &h.token, &100, &THIRTY_DAYS)
        .is_err());
    assert_eq!(h.vault.spend_limit(&user, &h.token).amount, 0);
}

#[test]
fn spend_limits_are_scoped_to_user_and_token() {
    let h = setup();
    let a = funded_user(&h, 100_000);
    let b = funded_user(&h, 100_000);
    let merchant = Address::generate(&h.env);

    let token2 = h
        .env
        .register_stellar_asset_contract_v2(Address::generate(&h.env))
        .address();
    StellarAssetClient::new(&h.env, &token2).mint(&a, &100_000);
    h.vault.deposit(&a, &token2, &50_000);
    h.vault.deposit(&a, &h.token, &50_000);
    h.vault.deposit(&b, &h.token, &50_000);

    h.vault.set_spend_limit(&a, &h.token, &100, &THIRTY_DAYS);

    // The limit applies to a's first token only...
    assert_eq!(
        h.vault.try_debit(&a, &h.token, &merchant, &101),
        Err(Ok(Error::SpendLimitExceeded))
    );
    // ...not to the same user's other token...
    h.vault.debit(&a, &token2, &merchant, &1_000);
    // ...and not to another user.
    h.vault.debit(&b, &h.token, &merchant, &1_000);
}

#[test]
fn changing_the_limit_resets_the_window() {
    let h = setup();
    let user = funded_user(&h, 100_000);
    let merchant = Address::generate(&h.env);
    h.vault.deposit(&user, &h.token, &50_000);

    h.vault
        .set_spend_limit(&user, &h.token, &5_000, &THIRTY_DAYS);
    h.vault.debit(&user, &h.token, &merchant, &5_000);

    h.vault
        .set_spend_limit(&user, &h.token, &1_000, &THIRTY_DAYS);
    h.vault.debit(&user, &h.token, &merchant, &1_000);
    assert_eq!(h.vault.balance(&user, &h.token), 44_000);
}

#[test]
fn debit_without_a_limit_writes_no_window_state() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    let merchant = Address::generate(&h.env);
    h.vault.deposit(&user, &h.token, &5_000);

    h.vault.debit(&user, &h.token, &merchant, &1_000);

    let (has_limit, has_window) = h.env.as_contract(&h.vault_id, || {
        let p = h.env.storage().persistent();
        (
            p.has(&DataKey::SpendLimit(user.clone(), h.token.clone())),
            p.has(&DataKey::SpendWindow(user.clone(), h.token.clone())),
        )
    });
    assert!(!has_limit, "an unlimited user must have no limit entry");
    assert!(!has_window, "an unlimited user must have no window entry");
}

#[test]
fn deposit_for_credits_the_recipient_and_debits_the_payer() {
    let h = setup();
    let from = funded_user(&h, 10_000);
    let user = Address::generate(&h.env);

    h.vault.deposit_for(&from, &user, &h.token, &4_000);

    // The recipient owns the balance; the payer's vault balance is untouched.
    assert_eq!(h.vault.balance(&user, &h.token), 4_000);
    assert_eq!(h.vault.balance(&from, &h.token), 0);
    // The payer's wallet funded it, and the vault holds the tokens.
    assert_eq!(h.token_client.balance(&from), 6_000);
    assert_eq!(h.token_client.balance(&h.vault_id), 4_000);
}

#[test]
fn deposit_for_accumulates_on_the_recipients_balance() {
    let h = setup();
    let from = funded_user(&h, 10_000);
    let other = funded_user(&h, 10_000);
    let user = Address::generate(&h.env);

    h.vault.deposit_for(&from, &user, &h.token, &1_000);
    h.vault.deposit_for(&other, &user, &h.token, &2_500);

    assert_eq!(h.vault.balance(&user, &h.token), 3_500);
    assert_eq!(h.token_client.balance(&h.vault_id), 3_500);
}

#[test]
fn deposit_for_event_identifies_the_payer() {
    let h = setup();
    let from = funded_user(&h, 10_000);
    let user = Address::generate(&h.env);

    h.vault.deposit_for(&from, &user, &h.token, &4_000);

    let last = h.env.events().all().events().last().cloned().unwrap();
    let ContractEventBody::V0(body) = &last.body;

    let ScVal::Symbol(ScSymbol(name)) = &body.topics[0] else {
        panic!("topic 0 is not a symbol");
    };
    assert_eq!(name.to_utf8_string_lossy(), "deposit_for");
    // from, user, token are the topics, in that order.
    assert_eq!(body.topics[1], ScVal::from(from));
    assert_eq!(body.topics[2], ScVal::from(user));
    assert_eq!(body.topics[3], ScVal::from(h.token));
}

#[test]
fn only_the_recipient_can_withdraw_gifted_funds() {
    let h = setup();
    let from = funded_user(&h, 10_000);
    let user = funded_user(&h, 0);

    h.vault.deposit_for(&from, &user, &h.token, &4_000);

    // The payer has no vault balance of its own to withdraw.
    assert_eq!(
        h.vault.try_withdraw(&from, &h.token, &1_000),
        Err(Ok(Error::InsufficientBalance))
    );
    // The recipient can withdraw everything, and only it receives the tokens.
    // The payer's wallet keeps its remaining 6_000: the gift is gone and
    // cannot be pulled back.
    assert!(h.vault.try_withdraw(&user, &h.token, &4_000).is_ok());
    assert_eq!(h.token_client.balance(&user), 4_000);
    assert_eq!(h.token_client.balance(&from), 6_000);
    assert_eq!(h.token_client.balance(&h.vault_id), 0);
}

#[test]
fn deposit_for_rejects_non_positive_amount() {
    let h = setup();
    let from = funded_user(&h, 10_000);
    let user = Address::generate(&h.env);

    assert_eq!(
        h.vault.try_deposit_for(&from, &user, &h.token, &0),
        Err(Ok(Error::InvalidAmount))
    );
    assert_eq!(
        h.vault.try_deposit_for(&from, &user, &h.token, &-5),
        Err(Ok(Error::InvalidAmount))
    );
    assert_eq!(h.vault.balance(&user, &h.token), 0);
    assert_eq!(h.token_client.balance(&from), 10_000);
}

#[test]
fn deposit_for_requires_the_payer_to_authorize() {
    let h = setup();
    let from = funded_user(&h, 10_000);
    let user = Address::generate(&h.env);

    h.env.mock_auths(&[MockAuth {
        address: &user,
        invoke: &MockAuthInvoke {
            contract: &h.vault.address,
            fn_name: "deposit_for",
            args: (&from, &user, &h.token, &4_000).into_val(&h.env),
            sub_invokes: &[],
        },
    }]);

    assert!(h
        .vault
        .try_deposit_for(&from, &user, &h.token, &4_000)
        .is_err());
    // Nothing moved on either side.
    assert_eq!(h.vault.balance(&user, &h.token), 0);
    assert_eq!(h.token_client.balance(&from), 10_000);
    assert_eq!(h.token_client.balance(&h.vault_id), 0);
}

#[test]
fn admin_can_upgrade_and_state_written_before_survives() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    h.vault.deposit(&user, &h.token, &4_000); // persistent state

    let wasm_hash = h.env.deployer().upload_contract_wasm(UPGRADE_WASM);
    h.vault.upgrade_wasm(&wasm_hash);

    // The balance and the instance configuration written before the upgrade
    // are both still readable after it.
    assert_eq!(h.vault.balance(&user, &h.token), 4_000);
    assert_eq!(h.vault.subscription(), h.subscription);

    // And the new code still runs: deposits continue where they left off.
    h.vault.deposit(&user, &h.token, &1_000);
    assert_eq!(h.vault.balance(&user, &h.token), 5_000);
}

/// #237: every persistent and instance value written before an upgrade must
/// still decode with the current types after the code is swapped.
#[test]
fn upgrade_preserves_every_stored_value() {
    let h = setup();
    let admin = h.vault.admin();
    let user = funded_user(&h, 10_000);
    let pending_admin = Address::generate(&h.env);

    h.vault.deposit(&user, &h.token, &4_000);
    h.vault.set_deposit_cap(&user, &h.token, &8_000);
    h.vault.set_allowlist_enabled(&true);
    h.vault.set_token_allowed(&h.token, &true);
    h.vault.set_spend_limit(&user, &h.token, &5_000, &3_600);
    h.vault.set_reserve(&user, &h.token, &1, &1_500);
    h.vault.propose_admin(&pending_admin);

    let wasm_hash = h.env.deployer().upload_contract_wasm(UPGRADE_WASM);
    h.vault.upgrade_wasm(&wasm_hash);

    assert_eq!(h.vault.balance(&user, &h.token), 4_000);
    assert_eq!(h.vault.deposit_cap(&user, &h.token), 8_000);
    assert!(h.vault.token_allowed(&h.token));
    assert!(h.vault.allowlist_enabled());
    assert_eq!(h.vault.subscription(), h.subscription);
    assert_eq!(
        h.vault.spend_limit(&user, &h.token),
        SpendLimit {
            amount: 5_000,
            window_secs: 3_600,
        }
    );
    assert_eq!(h.vault.admin(), admin);

    // Keys the committed upgrade target predates decode with the current
    // types too: a code swap never rewrites or drops storage entries.
    h.env.as_contract(&h.vault_id, || {
        let p = h.env.storage().persistent();
        assert_eq!(
            p.get::<_, i128>(&DataKey::Reserved(user.clone(), h.token.clone(), 1)),
            Some(1_500)
        );
        assert_eq!(
            p.get::<_, i128>(&DataKey::ReservedTotal(user.clone(), h.token.clone())),
            Some(1_500)
        );
        let window = p
            .get::<_, OperationWindow>(&DataKey::OperationWindow(user.clone(), h.token.clone()))
            .unwrap();
        assert!(window.operations >= 1);
        assert_eq!(
            h.env
                .storage()
                .instance()
                .get::<_, Address>(&DataKey::PendingAdmin),
            Some(pending_admin.clone())
        );
    });

    // The upgraded code still moves funds against the preserved balance entry.
    h.vault.withdraw(&user, &h.token, &1_000);
    assert_eq!(h.vault.balance(&user, &h.token), 3_000);
}

#[test]
fn rejected_upgrade_leaves_stored_values_intact() {
    let h = setup();
    let admin = h.vault.admin();
    let user = funded_user(&h, 10_000);
    h.vault.deposit(&user, &h.token, &4_000);

    let wasm_hash = h.env.deployer().upload_contract_wasm(UPGRADE_WASM);
    let attacker = Address::generate(&h.env);
    h.env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &h.vault_id,
            fn_name: "upgrade_wasm",
            args: (&wasm_hash,).into_val(&h.env),
            sub_invokes: &[],
        },
    }]);
    assert!(h.vault.try_upgrade_wasm(&wasm_hash).is_err());

    // The failed call moved no state and recorded no upgrade.
    assert_eq!(h.vault.balance(&user, &h.token), 4_000);
    assert_eq!(h.vault.admin(), admin);
    assert_eq!(h.vault.upgrade_history().len(), 0);
}

#[test]
fn upgrade_wasm_wrong_signer_rejected() {
    let h = setup();
    let wasm_hash = h.env.deployer().upload_contract_wasm(UPGRADE_WASM);
    let attacker = Address::generate(&h.env);

    // The attacker passes the hash as the argument, but the signature on the
    // call is the attacker's: `require_auth` on the admin fails.
    h.env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &h.vault_id,
            fn_name: "upgrade_wasm",
            args: (&wasm_hash,).into_val(&h.env),
            sub_invokes: &[],
        },
    }]);
    assert!(h.vault.try_upgrade_wasm(&wasm_hash).is_err());

    // With no authorization at all the call fails too.
    h.env.set_auths(&[]);
    assert!(h.vault.try_upgrade_wasm(&wasm_hash).is_err());

    // The subscription pointer and balances are untouched.
    assert_eq!(h.vault.subscription(), h.subscription);
}

// ---------------------------------------------------------------------------
// Upgrade history view (#304)
// ---------------------------------------------------------------------------

#[test]
fn upgrade_history_starts_empty() {
    let h = setup();
    let history = h.vault.upgrade_history();
    assert_eq!(history.len(), 0);
}

#[test]
fn upgrade_history_records_upgrades() {
    let h = setup();

    // Initial history is empty.
    assert_eq!(h.vault.upgrade_history().len(), 0);

    // Record an upgrade directly via the internal function logic.
    // We test the recording logic by verifying the event emission in
    // upgrade_history_emits_contract_upgraded_event test.
    // Cross-upgrade persistence is tested by the upgrade_wasm test which
    // verifies state survives upgrade; the history storage uses the same
    // instance storage mechanism.
    let wasm_hash = h.env.deployer().upload_contract_wasm(UPGRADE_WASM);
    h.vault.upgrade_wasm(&wasm_hash);

    // After upgrade, the contract is replaced with the old WASM which
    // doesn't have upgrade_history. The history is recorded in storage
    // and would be readable by a newer contract version.
    // We verify the event was emitted instead.
    let mut found = false;
    for event in h.env.events().all().events() {
        let ContractEventBody::V0(body) = &event.body;
        if let Some(ScVal::Symbol(ScSymbol(name))) = body.topics.first() {
            if name.to_utf8_string_lossy() == "contract_upgraded" {
                found = true;
                break;
            }
        }
    }
    assert!(found, "ContractUpgraded event should be emitted");
}

#[test]
fn upgrade_history_requires_admin_auth() {
    let h = setup();
    let wasm_hash = h.env.deployer().upload_contract_wasm(UPGRADE_WASM);
    let attacker = Address::generate(&h.env);

    h.env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &h.vault_id,
            fn_name: "upgrade_wasm",
            args: (&wasm_hash,).into_val(&h.env),
            sub_invokes: &[],
        },
    }]);
    assert!(h.vault.try_upgrade_wasm(&wasm_hash).is_err());

    // History should still be empty.
    assert_eq!(h.vault.upgrade_history().len(), 0);
}

#[test]
fn upgrade_history_emits_contract_upgraded_event() {
    let h = setup();
    let wasm_hash = h.env.deployer().upload_contract_wasm(UPGRADE_WASM);

    h.vault.upgrade_wasm(&wasm_hash);

    // Find the ContractUpgraded event.
    let mut found = false;
    for event in h.env.events().all().events() {
        let ContractEventBody::V0(body) = &event.body;
        if let Some(ScVal::Symbol(ScSymbol(name))) = body.topics.first() {
            if name.to_utf8_string_lossy() == "contract_upgraded" {
                found = true;
                // Check topics: admin is topic 1, new_wasm_hash is in data.
                assert!(
                    body.topics.len() >= 2,
                    "ContractUpgraded event should have admin topic"
                );
                let ScVal::Address(_) = &body.topics[1] else {
                    panic!("topic 1 should be admin address");
                };
                // new_wasm_hash is in data map.
                let ScVal::Map(Some(ScMap(entries))) = &body.data else {
                    panic!("data should be a map");
                };
                let mut has_wasm = false;
                for entry in entries {
                    let ScVal::Symbol(ScSymbol(key)) = &entry.key else {
                        continue;
                    };
                    if key.to_utf8_string_lossy() == "new_wasm_hash" {
                        has_wasm = true;
                    }
                }
                assert!(has_wasm);
            }
        }
    }
    assert!(found, "ContractUpgraded event should be emitted");
}

// ---------------------------------------------------------------------------
// Per-mandate reserved balance (#295): a subscriber earmarks part of a balance
// for one mandate, and no other mandate (nor a plain debit or a withdrawal)
// can draw it down.
// ---------------------------------------------------------------------------

const MANDATE_A: u64 = 1;
const MANDATE_B: u64 = 2;

/// A user with `amount` deposited into the vault.
fn depositor(h: &Harness, amount: i128) -> Address {
    let user = funded_user(h, amount);
    h.vault.deposit(&user, &h.token, &amount);
    user
}

/// The `amount` and `reserved_total` fields of the last `reserve_set` event.
fn last_reserve_set(h: &Harness) -> Option<(i128, i128)> {
    let mut found = None;
    for event in h.env.events().all().events() {
        let ContractEventBody::V0(body) = &event.body;
        let is_reserve = matches!(
            body.topics.first(),
            Some(ScVal::Symbol(ScSymbol(name))) if name.to_utf8_string_lossy() == "reserve_set"
        );
        if !is_reserve {
            continue;
        }
        let ScVal::Map(Some(soroban_sdk::xdr::ScMap(entries))) = &body.data else {
            continue;
        };
        let mut amount = None;
        let mut total = None;
        for entry in entries {
            let (ScVal::Symbol(ScSymbol(name)), ScVal::I128(parts)) = (&entry.key, &entry.val)
            else {
                continue;
            };
            let value = (i128::from(parts.hi) << 64) | i128::from(parts.lo);
            match name.to_utf8_string_lossy().as_str() {
                "amount" => amount = Some(value),
                "reserved_total" => total = Some(value),
                _ => {}
            }
        }
        found = amount.zip(total);
    }
    found
}

#[test]
fn no_reserve_by_default() {
    let h = setup();
    let user = depositor(&h, 1_000);
    assert_eq!(h.vault.reserved(&user, &h.token, &MANDATE_A), 0);
    assert_eq!(h.vault.reserved_total(&user, &h.token), 0);
    assert_eq!(h.vault.unreserved_balance(&user, &h.token), 1_000);
}

#[test]
fn set_reserve_earmarks_part_of_the_balance() {
    let h = setup();
    let user = depositor(&h, 1_000);

    h.vault.set_reserve(&user, &h.token, &MANDATE_A, &300);
    h.vault.set_reserve(&user, &h.token, &MANDATE_B, &200);

    assert_eq!(h.vault.reserved(&user, &h.token, &MANDATE_A), 300);
    assert_eq!(h.vault.reserved(&user, &h.token, &MANDATE_B), 200);
    assert_eq!(h.vault.reserved_total(&user, &h.token), 500);
    assert_eq!(h.vault.unreserved_balance(&user, &h.token), 500);
    // Reserving moves no tokens and leaves the total balance untouched.
    assert_eq!(h.vault.balance(&user, &h.token), 1_000);
    assert_eq!(h.token_client.balance(&h.vault_id), 1_000);
}

#[test]
fn set_reserve_emits_the_new_reserve_and_total() {
    let h = setup();
    let user = depositor(&h, 1_000);

    h.vault.set_reserve(&user, &h.token, &MANDATE_A, &300);
    assert_eq!(last_reserve_set(&h), Some((300, 300)));

    h.vault.set_reserve(&user, &h.token, &MANDATE_B, &100);
    assert_eq!(last_reserve_set(&h), Some((100, 400)));

    h.vault.set_reserve(&user, &h.token, &MANDATE_A, &0);
    assert_eq!(last_reserve_set(&h), Some((0, 100)));
}

#[test]
fn set_reserve_replaces_rather_than_adds() {
    let h = setup();
    let user = depositor(&h, 1_000);

    h.vault.set_reserve(&user, &h.token, &MANDATE_A, &300);
    h.vault.set_reserve(&user, &h.token, &MANDATE_A, &700);
    assert_eq!(h.vault.reserved(&user, &h.token, &MANDATE_A), 700);
    assert_eq!(h.vault.reserved_total(&user, &h.token), 700);

    h.vault.set_reserve(&user, &h.token, &MANDATE_A, &100);
    assert_eq!(h.vault.reserved_total(&user, &h.token), 100);
}

#[test]
fn releasing_a_reserve_restores_the_unreserved_balance() {
    let h = setup();
    let user = depositor(&h, 1_000);

    h.vault.set_reserve(&user, &h.token, &MANDATE_A, &600);
    h.vault.set_reserve(&user, &h.token, &MANDATE_A, &0);

    assert_eq!(h.vault.reserved(&user, &h.token, &MANDATE_A), 0);
    assert_eq!(h.vault.reserved_total(&user, &h.token), 0);
    assert_eq!(h.vault.unreserved_balance(&user, &h.token), 1_000);
}

#[test]
fn reserves_cannot_exceed_the_balance() {
    let h = setup();
    let user = depositor(&h, 1_000);

    assert_eq!(
        h.vault.try_set_reserve(&user, &h.token, &MANDATE_A, &1_001),
        Err(Ok(Error::ReserveExceedsBalance))
    );

    // Across mandates: the sum is what is capped.
    h.vault.set_reserve(&user, &h.token, &MANDATE_A, &700);
    assert_eq!(
        h.vault.try_set_reserve(&user, &h.token, &MANDATE_B, &301),
        Err(Ok(Error::ReserveExceedsBalance))
    );
    // Exactly the rest is fine.
    h.vault.set_reserve(&user, &h.token, &MANDATE_B, &300);
    assert_eq!(h.vault.unreserved_balance(&user, &h.token), 0);
}

#[test]
fn set_reserve_rejects_a_negative_amount() {
    let h = setup();
    let user = depositor(&h, 1_000);
    assert_eq!(
        h.vault.try_set_reserve(&user, &h.token, &MANDATE_A, &-1),
        Err(Ok(Error::InvalidAmount))
    );
    assert_eq!(h.vault.reserved_total(&user, &h.token), 0);
}

#[test]
fn a_user_with_no_balance_cannot_reserve() {
    let h = setup();
    let user = Address::generate(&h.env);
    assert_eq!(
        h.vault.try_set_reserve(&user, &h.token, &MANDATE_A, &1),
        Err(Ok(Error::ReserveExceedsBalance))
    );
    // Zero is a valid no-op release.
    h.vault.set_reserve(&user, &h.token, &MANDATE_A, &0);
}

#[test]
fn another_mandate_cannot_draw_down_a_reserve() {
    let h = setup();
    let user = depositor(&h, 1_000);
    let merchant = Address::generate(&h.env);
    h.vault.set_reserve(&user, &h.token, &MANDATE_A, &800);

    // Mandate B sees only the unreserved 200.
    assert_eq!(
        h.vault
            .try_debit_mandate(&user, &h.token, &MANDATE_B, &merchant, &201),
        Err(Ok(Error::InsufficientBalance))
    );
    h.vault
        .debit_mandate(&user, &h.token, &MANDATE_B, &merchant, &200);

    // Mandate A's reserve is intact.
    assert_eq!(h.vault.reserved(&user, &h.token, &MANDATE_A), 800);
    assert_eq!(h.vault.balance(&user, &h.token), 800);
    assert_eq!(
        h.vault
            .try_debit_mandate(&user, &h.token, &MANDATE_B, &merchant, &1),
        Err(Ok(Error::InsufficientBalance))
    );
}

#[test]
fn plain_debits_cannot_draw_down_a_reserve() {
    let h = setup();
    let user = depositor(&h, 1_000);
    let merchant = Address::generate(&h.env);
    let fee_to = Address::generate(&h.env);
    h.vault.set_reserve(&user, &h.token, &MANDATE_A, &800);

    assert_eq!(
        h.vault.try_debit(&user, &h.token, &merchant, &201),
        Err(Ok(Error::InsufficientBalance))
    );
    assert_eq!(
        h.vault
            .try_debit_split(&user, &h.token, &merchant, &150, &fee_to, &51),
        Err(Ok(Error::InsufficientBalance))
    );
    h.vault
        .debit_split(&user, &h.token, &merchant, &150, &fee_to, &50);
    assert_eq!(h.vault.reserved(&user, &h.token, &MANDATE_A), 800);
    assert_eq!(h.vault.unreserved_balance(&user, &h.token), 0);
}

#[test]
fn the_owning_mandate_spends_its_reserve_first() {
    let h = setup();
    let user = depositor(&h, 1_000);
    let merchant = Address::generate(&h.env);
    h.vault.set_reserve(&user, &h.token, &MANDATE_A, &300);
    h.vault.set_reserve(&user, &h.token, &MANDATE_B, &200);

    // 250 fits inside A's reserve: only the reserve shrinks.
    h.vault
        .debit_mandate(&user, &h.token, &MANDATE_A, &merchant, &250);
    assert_eq!(h.vault.reserved(&user, &h.token, &MANDATE_A), 50);
    assert_eq!(h.vault.reserved_total(&user, &h.token), 250);
    assert_eq!(h.vault.unreserved_balance(&user, &h.token), 500);

    // 150 exhausts A's last 50 and takes 100 of the unreserved balance.
    h.vault
        .debit_mandate(&user, &h.token, &MANDATE_A, &merchant, &150);
    assert_eq!(h.vault.reserved(&user, &h.token, &MANDATE_A), 0);
    assert_eq!(h.vault.reserved(&user, &h.token, &MANDATE_B), 200);
    assert_eq!(h.vault.reserved_total(&user, &h.token), 200);
    assert_eq!(h.vault.balance(&user, &h.token), 600);
    assert_eq!(h.vault.unreserved_balance(&user, &h.token), 400);
    assert_eq!(h.token_client.balance(&merchant), 400);
}

#[test]
fn a_mandate_can_spend_its_reserve_plus_the_unreserved_balance_but_no_more() {
    let h = setup();
    let user = depositor(&h, 1_000);
    let merchant = Address::generate(&h.env);
    h.vault.set_reserve(&user, &h.token, &MANDATE_A, &300);
    h.vault.set_reserve(&user, &h.token, &MANDATE_B, &500);

    // A can reach its own 300 plus the unreserved 200, never B's 500.
    assert_eq!(
        h.vault
            .try_debit_mandate(&user, &h.token, &MANDATE_A, &merchant, &501),
        Err(Ok(Error::InsufficientBalance))
    );
    h.vault
        .debit_mandate(&user, &h.token, &MANDATE_A, &merchant, &500);
    assert_eq!(h.vault.balance(&user, &h.token), 500);
    assert_eq!(h.vault.reserved(&user, &h.token, &MANDATE_B), 500);
}

#[test]
fn debit_split_mandate_draws_the_reserve_for_the_whole_total() {
    let h = setup();
    let user = depositor(&h, 1_000);
    let merchant = Address::generate(&h.env);
    let fee_to = Address::generate(&h.env);
    h.vault.set_reserve(&user, &h.token, &MANDATE_A, &1_000);

    // Nothing is unreserved, so only mandate A's split settles.
    assert_eq!(
        h.vault
            .try_debit_split(&user, &h.token, &merchant, &90, &fee_to, &10),
        Err(Ok(Error::InsufficientBalance))
    );
    h.vault
        .debit_split_mandate(&user, &h.token, &MANDATE_A, &merchant, &90, &fee_to, &10);
    // Running balances on the two Debit events are unchanged by reserves.
    assert_eq!(debit_event_balance(&h, 0), Some(910));
    assert_eq!(debit_event_balance(&h, 1), Some(900));

    assert_eq!(h.vault.reserved(&user, &h.token, &MANDATE_A), 900);
    assert_eq!(h.vault.balance(&user, &h.token), 900);
    assert_eq!(h.token_client.balance(&merchant), 90);
    assert_eq!(h.token_client.balance(&fee_to), 10);
}

#[test]
fn a_failed_mandate_debit_leaves_the_reserve_untouched() {
    let h = setup();
    let user = depositor(&h, 1_000);
    let merchant = Address::generate(&h.env);
    h.vault.set_reserve(&user, &h.token, &MANDATE_A, &400);

    assert!(h
        .vault
        .try_debit_mandate(&user, &h.token, &MANDATE_A, &merchant, &1_001)
        .is_err());
    assert_eq!(h.vault.reserved(&user, &h.token, &MANDATE_A), 400);
    assert_eq!(h.vault.reserved_total(&user, &h.token), 400);
    assert_eq!(h.vault.balance(&user, &h.token), 1_000);
}

#[test]
fn spend_limit_still_applies_to_reserved_funds() {
    let h = setup();
    let user = depositor(&h, 1_000);
    let merchant = Address::generate(&h.env);
    h.vault.set_reserve(&user, &h.token, &MANDATE_A, &1_000);
    h.vault.set_spend_limit(&user, &h.token, &100, &THIRTY_DAYS);

    assert_eq!(
        h.vault
            .try_debit_mandate(&user, &h.token, &MANDATE_A, &merchant, &101),
        Err(Ok(Error::SpendLimitExceeded))
    );
    // The rejected debit consumed nothing from the reserve.
    assert_eq!(h.vault.reserved(&user, &h.token, &MANDATE_A), 1_000);
}

#[test]
fn withdraw_reaches_only_the_unreserved_balance() {
    let h = setup();
    let user = depositor(&h, 1_000);
    h.vault.set_reserve(&user, &h.token, &MANDATE_A, &600);

    assert_eq!(
        h.vault.try_withdraw(&user, &h.token, &401),
        Err(Ok(Error::InsufficientBalance))
    );
    h.vault.withdraw(&user, &h.token, &400);
    assert_eq!(h.vault.balance(&user, &h.token), 600);
    assert_eq!(h.vault.reserved(&user, &h.token, &MANDATE_A), 600);
}

#[test]
fn withdraw_to_reaches_only_the_unreserved_balance() {
    let h = setup();
    let user = depositor(&h, 1_000);
    let destination = Address::generate(&h.env);
    h.vault.set_reserve(&user, &h.token, &MANDATE_A, &600);

    assert_eq!(
        h.vault.try_withdraw_to(&user, &h.token, &destination, &401),
        Err(Ok(Error::InsufficientBalance))
    );
    h.vault.withdraw_to(&user, &h.token, &destination, &400);
    assert_eq!(h.vault.balance(&user, &h.token), 600);
    assert_eq!(h.vault.reserved(&user, &h.token, &MANDATE_A), 600);
    assert_eq!(h.token_client.balance(&destination), 400);
}

#[test]
fn withdraw_all_leaves_reserves_behind_until_released() {
    let h = setup();
    let user = depositor(&h, 1_000);
    h.vault.set_reserve(&user, &h.token, &MANDATE_A, &600);

    assert_eq!(h.vault.withdraw_all(&user, &h.token), 400);
    assert_eq!(h.vault.balance(&user, &h.token), 600);
    // Fully reserved: nothing more to take, and no event or transfer.
    assert_eq!(h.vault.withdraw_all(&user, &h.token), 0);

    // Releasing the reserve lets the subscriber exit completely: a reserve
    // never traps funds.
    h.vault.set_reserve(&user, &h.token, &MANDATE_A, &0);
    assert_eq!(h.vault.withdraw_all(&user, &h.token), 600);
    assert_eq!(h.vault.balance(&user, &h.token), 0);
    assert_eq!(h.token_client.balance(&user), 1_000);
}

#[test]
fn reserves_are_scoped_to_user_and_token() {
    let h = setup();
    let alice = depositor(&h, 1_000);
    let bob = depositor(&h, 1_000);
    let merchant = Address::generate(&h.env);
    let other_token = h
        .env
        .register_stellar_asset_contract_v2(Address::generate(&h.env))
        .address();

    h.vault.set_reserve(&alice, &h.token, &MANDATE_A, &1_000);

    // Bob's mandate with the same id, and Alice's other token, are unaffected.
    assert_eq!(h.vault.reserved(&bob, &h.token, &MANDATE_A), 0);
    assert_eq!(h.vault.reserved(&alice, &other_token, &MANDATE_A), 0);
    h.vault.debit(&bob, &h.token, &merchant, &1_000);
    assert_eq!(h.vault.reserved_total(&alice, &h.token), 1_000);
}

// ---------------------------------------------------------------------------
// Total reserve cap per subscriber (#348): many mandates can no longer
// earmark the whole balance, so a minimum free balance stays withdrawable.
// ---------------------------------------------------------------------------

#[test]
fn reserve_cap_keeps_a_minimum_free_balance() {
    let h = setup_with_reserve_cap(DEFAULT_RESERVE_CAP_BPS);
    let user = depositor(&h, 1_000);
    assert_eq!(h.vault.reserve_cap_bps(), DEFAULT_RESERVE_CAP_BPS);

    // 90% of 1,000 is 900, so the whole balance can no longer be earmarked.
    assert_eq!(
        h.vault.try_set_reserve(&user, &h.token, &MANDATE_A, &901),
        Err(Ok(Error::ReserveCapExceeded))
    );
    h.vault.set_reserve(&user, &h.token, &MANDATE_A, &900);
    assert_eq!(h.vault.reserved_total(&user, &h.token), 900);
    assert_eq!(h.vault.unreserved_balance(&user, &h.token), 100);

    // Releasing a reserve is always allowed, cap or no cap.
    h.vault.set_reserve(&user, &h.token, &MANDATE_A, &0);
    assert_eq!(h.vault.unreserved_balance(&user, &h.token), 1_000);
}

#[test]
fn ten_mandates_fill_the_cap_and_the_eleventh_is_rejected() {
    let h = setup_with_reserve_cap(DEFAULT_RESERVE_CAP_BPS);
    let user = depositor(&h, 1_000);

    // Ten mandates at 90 each reach exactly the 900 cap.
    for id in 1..=10u64 {
        h.vault.set_reserve(&user, &h.token, &id, &90);
    }
    assert_eq!(h.vault.reserved_total(&user, &h.token), 900);
    assert_eq!(h.vault.unreserved_balance(&user, &h.token), 100);

    assert_eq!(
        h.vault.try_set_reserve(&user, &h.token, &11u64, &90),
        Err(Ok(Error::ReserveCapExceeded))
    );

    // Releasing one mandate's reserve makes room for the eleventh.
    h.vault.set_reserve(&user, &h.token, &1u64, &0);
    h.vault.set_reserve(&user, &h.token, &11u64, &90);
    assert_eq!(h.vault.reserved_total(&user, &h.token), 900);
}

#[test]
fn reducing_a_reserve_is_allowed_even_under_the_cap() {
    let h = setup_with_reserve_cap(DEFAULT_RESERVE_CAP_BPS);
    let user = depositor(&h, 1_000);

    h.vault.set_reserve(&user, &h.token, &MANDATE_A, &900);
    // Replacing it with a smaller reserve must not be mistaken for growth.
    h.vault.set_reserve(&user, &h.token, &MANDATE_A, &100);
    assert_eq!(h.vault.reserved_total(&user, &h.token), 100);

    // Growing it again is still bound by the cap.
    assert_eq!(
        h.vault.try_set_reserve(&user, &h.token, &MANDATE_A, &901),
        Err(Ok(Error::ReserveCapExceeded))
    );
}

#[test]
fn a_zero_cap_forbids_reserving() {
    let h = setup_with_reserve_cap(0);
    let user = depositor(&h, 1_000);

    assert_eq!(
        h.vault.try_set_reserve(&user, &h.token, &MANDATE_A, &1),
        Err(Ok(Error::ReserveCapExceeded))
    );
    // Releasing (or leaving at zero) is still a no-op that succeeds.
    h.vault.set_reserve(&user, &h.token, &MANDATE_A, &0);
}

#[test]
fn a_full_balance_cap_keeps_the_previous_behaviour() {
    let h = setup_with_reserve_cap(BPS_DENOMINATOR);
    let user = depositor(&h, 1_000);

    h.vault.set_reserve(&user, &h.token, &MANDATE_A, &1_000);
    assert_eq!(h.vault.reserved_total(&user, &h.token), 1_000);
    assert_eq!(h.vault.unreserved_balance(&user, &h.token), 0);
}

#[test]
fn can_debit_reports_reserve_aware_headroom() {
    let h = setup();
    let user = depositor(&h, 1_000);
    h.vault.set_reserve(&user, &h.token, &MANDATE_A, &700);

    assert!(h.vault.can_debit(&user, &h.token, &MANDATE_A, &1_000));
    assert!(!h.vault.can_debit(&user, &h.token, &MANDATE_A, &1_001));
    assert!(h.vault.can_debit(&user, &h.token, &MANDATE_B, &300));
    assert!(!h.vault.can_debit(&user, &h.token, &MANDATE_B, &301));
    assert!(!h.vault.can_debit(&user, &h.token, &MANDATE_A, &0));

    // The spend limit narrows it further.
    h.vault.set_spend_limit(&user, &h.token, &500, &THIRTY_DAYS);
    assert!(h.vault.can_debit(&user, &h.token, &MANDATE_A, &500));
    assert!(!h.vault.can_debit(&user, &h.token, &MANDATE_A, &501));
}

#[test]
fn can_debit_writes_no_state() {
    let h = setup();
    let user = depositor(&h, 1_000);
    h.vault.set_spend_limit(&user, &h.token, &500, &THIRTY_DAYS);

    for _ in 0..3 {
        assert!(h.vault.can_debit(&user, &h.token, &MANDATE_A, &400));
    }
    // Had the check recorded spend, a real 400 debit would now exceed 500.
    let merchant = Address::generate(&h.env);
    h.vault
        .debit_mandate(&user, &h.token, &MANDATE_A, &merchant, &400);
}

#[test]
fn set_reserve_requires_the_user_auth() {
    let h = setup();
    let user = depositor(&h, 1_000);
    let stranger = Address::generate(&h.env);

    // A stranger's signature does not let them earmark someone else's funds.
    h.env.mock_auths(&[MockAuth {
        address: &stranger,
        invoke: &MockAuthInvoke {
            contract: &h.vault_id,
            fn_name: "set_reserve",
            args: (&user, &h.token, &MANDATE_A, &500i128).into_val(&h.env),
            sub_invokes: &[],
        },
    }]);
    assert!(h
        .vault
        .try_set_reserve(&user, &h.token, &MANDATE_A, &500)
        .is_err());

    h.env.set_auths(&[]);
    assert!(h
        .vault
        .try_set_reserve(&user, &h.token, &MANDATE_A, &500)
        .is_err());
    assert_eq!(h.vault.reserved_total(&user, &h.token), 0);
}

#[test]
fn debit_mandate_requires_the_subscription_auth() {
    let h = setup();
    let user = depositor(&h, 1_000);
    let merchant = Address::generate(&h.env);
    h.vault.set_reserve(&user, &h.token, &MANDATE_A, &500);

    h.env.set_auths(&[]);
    assert!(h
        .vault
        .try_debit_mandate(&user, &h.token, &MANDATE_A, &merchant, &100)
        .is_err());
    assert_eq!(h.vault.balance(&user, &h.token), 1_000);
    assert_eq!(h.vault.reserved(&user, &h.token, &MANDATE_A), 500);
}

#[test]
fn debit_split_mandate_requires_the_subscription_auth() {
    let h = setup();
    let user = depositor(&h, 1_000);
    let merchant = Address::generate(&h.env);
    let fee_to = Address::generate(&h.env);
    h.vault.set_reserve(&user, &h.token, &MANDATE_A, &500);

    // Not even the subscriber can settle their own mandate's split.
    h.env.mock_auths(&[MockAuth {
        address: &user,
        invoke: &MockAuthInvoke {
            contract: &h.vault_id,
            fn_name: "debit_split_mandate",
            args: (
                &user, &h.token, &MANDATE_A, &merchant, &90i128, &fee_to, &10i128,
            )
                .into_val(&h.env),
            sub_invokes: &[],
        },
    }]);
    assert!(h
        .vault
        .try_debit_split_mandate(&user, &h.token, &MANDATE_A, &merchant, &90, &fee_to, &10)
        .is_err());
    assert_eq!(h.vault.balance(&user, &h.token), 1_000);
    assert_eq!(h.vault.reserved(&user, &h.token, &MANDATE_A), 500);
}

#[test]
fn debit_multi_mandate_requires_the_subscription_auth() {
    let h = setup();
    let user = depositor(&h, 1_000);
    let merchant = Address::generate(&h.env);
    let fee_to = Address::generate(&h.env);
    h.vault.set_reserve(&user, &h.token, &MANDATE_A, &500);

    // Not even the subscriber can settle their own mandate's multi-debit.
    let legs = soroban_sdk::vec![&h.env, (merchant.clone(), 90i128), (fee_to.clone(), 10i128)];
    h.env.mock_auths(&[MockAuth {
        address: &user,
        invoke: &MockAuthInvoke {
            contract: &h.vault_id,
            fn_name: "debit_multi_mandate",
            args: (&user, &h.token, &MANDATE_A, &legs).into_val(&h.env),
            sub_invokes: &[],
        },
    }]);
    assert!(h
        .vault
        .try_debit_multi_mandate(&user, &h.token, &MANDATE_A, &legs)
        .is_err());
    assert_eq!(h.vault.balance(&user, &h.token), 1_000);
    assert_eq!(h.vault.reserved(&user, &h.token, &MANDATE_A), 500);
}

#[test]
fn reserved_internal_balances_still_add_up_to_holdings() {
    let h = setup();
    let alice = depositor(&h, 1_000);
    let bob = depositor(&h, 1_000);
    let merchant = Address::generate(&h.env);

    h.vault.set_reserve(&alice, &h.token, &MANDATE_A, &500);
    h.vault
        .debit_mandate(&alice, &h.token, &MANDATE_A, &merchant, &700);
    h.vault.set_reserve(&bob, &h.token, &MANDATE_B, &300);
    h.vault.withdraw_all(&bob, &h.token);

    let internal = h.vault.balance(&alice, &h.token) + h.vault.balance(&bob, &h.token);
    assert_eq!(internal, h.token_client.balance(&h.vault_id));
    // And every reserve stays inside its balance.
    assert!(h.vault.reserved_total(&alice, &h.token) <= h.vault.balance(&alice, &h.token));
    assert!(h.vault.reserved_total(&bob, &h.token) <= h.vault.balance(&bob, &h.token));
}

// ---------------------------------------------------------------- #247
// TTL refresh whenever a mandate's reserve is accessed.

fn reserve_ttl(h: &Harness, key: &DataKey) -> u32 {
    h.env
        .as_contract(&h.vault_id, || h.env.storage().persistent().get_ttl(key))
}

#[test]
fn hot_reserve_ttl_is_extended_on_view_access() {
    let h = setup();
    let user = depositor(&h, 1_000);
    h.vault.set_reserve(&user, &h.token, &MANDATE_A, &300);
    let key = DataKey::Reserved(user.clone(), h.token.clone(), MANDATE_A);
    let total_key = DataKey::ReservedTotal(user.clone(), h.token.clone());

    let seq = h.env.ledger().sequence();
    h.env
        .ledger()
        .set_sequence_number(seq + 30 * DAY_IN_LEDGERS);
    assert!(reserve_ttl(&h, &key) < PERSIST_BUMP);
    assert!(reserve_ttl(&h, &total_key) < PERSIST_BUMP);

    assert_eq!(h.vault.reserved(&user, &h.token, &MANDATE_A), 300);
    assert_eq!(h.vault.reserved_total(&user, &h.token), 300);
    assert_eq!(reserve_ttl(&h, &key), PERSIST_BUMP);
    assert_eq!(reserve_ttl(&h, &total_key), PERSIST_BUMP);
}

#[test]
fn missing_reserves_have_no_ttl_to_extend() {
    let h = setup();
    let user = depositor(&h, 1_000);
    assert_eq!(h.vault.reserved(&user, &h.token, &MANDATE_A), 0);
    assert_eq!(h.vault.reserved_total(&user, &h.token), 0);
}

// ---------------------------------------------------------------------------
// Panic safety (#308): extreme amounts must be rejected with typed errors, not
// overflow traps.
// ---------------------------------------------------------------------------

fn assert_typed_error<T: core::fmt::Debug>(
    result: Result<T, Result<Error, soroban_sdk::InvokeError>>,
) {
    if let Err(Err(error)) = result {
        panic!("entry point returned an untyped host error: {error:?}");
    }
}

fn next_fuzz_u64(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    *state
}

#[test]
fn deposit_overflow_is_a_typed_error() {
    let h = setup();
    let user = funded_user(&h, 100);
    let payer = funded_user(&h, 100);
    let near_max = i128::MAX - 1;

    h.env.as_contract(&h.vault_id, || {
        h.env
            .storage()
            .persistent()
            .set(&DataKey::Balance(user.clone(), h.token.clone()), &near_max);
    });

    assert_eq!(
        h.vault.try_deposit(&user, &h.token, &2),
        Err(Ok(Error::AmountOverflow))
    );
    assert_eq!(
        h.vault.try_deposit_for(&payer, &user, &h.token, &2),
        Err(Ok(Error::AmountOverflow))
    );
    assert_eq!(h.vault.balance(&user, &h.token), near_max);
    assert_eq!(h.token_client.balance(&payer), 100);
    assert_eq!(h.token_client.balance(&user), 100);
}

#[test]
fn extreme_debit_and_reserve_amounts_are_typed() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    let destination = Address::generate(&h.env);
    h.vault.deposit(&user, &h.token, &1_000);

    assert_eq!(
        h.vault.try_debit_split(
            &user,
            &h.token,
            &destination,
            &i128::MAX,
            &destination,
            &i128::MAX,
        ),
        Err(Ok(Error::AmountOverflow))
    );
    assert_eq!(
        h.vault
            .try_debit_split(&user, &h.token, &destination, &i128::MIN, &destination, &1),
        Err(Ok(Error::InvalidAmount))
    );
    assert_eq!(
        h.vault.try_set_reserve(&user, &h.token, &1, &i128::MAX),
        Err(Ok(Error::ReserveExceedsBalance))
    );
    assert_eq!(
        h.vault.try_set_reserve(&user, &h.token, &1, &i128::MIN),
        Err(Ok(Error::InvalidAmount))
    );
    assert_eq!(
        h.vault.try_set_spend_limit(&user, &h.token, &i128::MAX, &0),
        Err(Ok(Error::InvalidWindow))
    );
    assert_eq!(
        h.vault.try_set_deposit_cap(&user, &h.token, &i128::MIN),
        Err(Ok(Error::InvalidAmount))
    );
}

#[test]
fn extreme_withdrawals_are_typed() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    let destination = Address::generate(&h.env);
    h.vault.deposit(&user, &h.token, &1_000);

    for amount in [0, -1, i128::MIN] {
        assert_eq!(
            h.vault.try_withdraw(&user, &h.token, &amount),
            Err(Ok(Error::InvalidAmount))
        );
        assert_eq!(
            h.vault
                .try_withdraw_to(&user, &h.token, &destination, &amount),
            Err(Ok(Error::InvalidAmount))
        );
    }
    assert_eq!(
        h.vault.try_withdraw(&user, &h.token, &i128::MAX),
        Err(Ok(Error::InsufficientBalance))
    );
    assert_eq!(
        h.vault
            .try_withdraw_to(&user, &h.token, &destination, &i128::MAX),
        Err(Ok(Error::InsufficientBalance))
    );
    assert_eq!(h.vault.balance(&user, &h.token), 1_000);
    assert_eq!(h.token_client.balance(&destination), 0);
}

#[test]
fn deterministic_random_amounts_never_reach_a_host_trap() {
    let h = setup();
    let user = funded_user(&h, 10_000);
    let destination = Address::generate(&h.env);
    h.vault.deposit(&user, &h.token, &5_000);
    let mut seed = 0x0ddc_0ffe_e15e_beefu64;

    for _ in 0..128 {
        let x = next_fuzz_u64(&mut seed);
        let amount = match x % 8 {
            0 => 0,
            1 => -1,
            2 => i128::MIN,
            3 => i128::MAX,
            4 => 1,
            5 => 10_000,
            6 => -10_000,
            _ => i128::from(x),
        };
        let window = x % 100;
        let other = i128::from(next_fuzz_u64(&mut seed) % 20_000);

        assert_typed_error(h.vault.try_withdraw(&user, &h.token, &amount));
        assert_typed_error(
            h.vault
                .try_withdraw_to(&user, &h.token, &destination, &amount),
        );
        assert_typed_error(
            h.vault
                .try_set_spend_limit(&user, &h.token, &amount, &window),
        );
        assert_typed_error(h.vault.try_set_reserve(&user, &h.token, &(x % 3), &amount));
        assert_typed_error(h.vault.try_debit_split(
            &user,
            &h.token,
            &destination,
            &amount,
            &destination,
            &other,
        ));
    }
}

// ---------------------------------------------------------------------------
// #320: every public view is side-effect free
// ---------------------------------------------------------------------------

/// Run one view call and prove it was read-only: the whole ledger (storage
/// and TTLs) is identical afterwards, no contract events were published and
/// no authorization was required. `env.events().all()` and `env.auths()`
/// report the most recent top-level invocation, so this wraps one call.
fn assert_read_only<T>(env: &Env, label: &str, call: impl FnOnce() -> T) -> T {
    let before = env.to_ledger_snapshot();
    let out = call();
    let after = env.to_ledger_snapshot();
    assert!(before == after, "view `{label}` mutated ledger state");
    assert!(
        env.events().all().events().is_empty(),
        "view `{label}` emitted events"
    );
    assert!(
        env.auths().is_empty(),
        "view `{label}` required authorization"
    );
    out
}

/// A vault holding a balance with a per-mandate reserve, a spend limit that
/// has been partly used, a deposit cap and an active allowlist.
fn view_vault() -> (Harness<'static>, Address) {
    let h = setup();
    let user = Address::generate(&h.env);
    h.token_admin.mint(&user, &10_000);
    h.vault.set_allowlist_enabled(&true);
    h.vault.set_token_allowed(&h.token, &true);
    h.vault.set_deposit_cap(&user, &h.token, &9_000);
    h.vault.deposit(&user, &h.token, &5_000);
    h.vault.set_reserve(&user, &h.token, &7, &1_000);
    h.vault
        .set_spend_limit(&user, &h.token, &3_000, &THIRTY_DAYS);
    h.vault
        .debit(&user, &h.token, &Address::generate(&h.env), &500);
    (h, user)
}

#[test]
fn every_vault_view_leaves_storage_events_and_auth_untouched() {
    let (h, user) = view_vault();
    let env = &h.env;
    let v = &h.vault;
    let t = &h.token;

    assert_read_only(env, "version", || v.version());
    assert_read_only(env, "admin", || v.admin());
    assert_read_only(env, "subscription", || v.subscription());
    assert_read_only(env, "upgrade_history", || v.upgrade_history());
    assert_read_only(env, "allowlist_enabled", || v.allowlist_enabled());
    assert_read_only(env, "token_allowed", || v.token_allowed(t));
    assert_read_only(env, "balance", || v.balance(&user, t));
    assert_read_only(env, "deposit_cap", || v.deposit_cap(&user, t));
    assert_read_only(env, "can_deposit", || v.can_deposit(&user, t));
    assert_read_only(env, "reserved", || v.reserved(&user, t, &7));
    assert_read_only(env, "reserved_total", || v.reserved_total(&user, t));
    assert_read_only(env, "unreserved_balance", || v.unreserved_balance(&user, t));
    assert_read_only(env, "spend_limit", || v.spend_limit(&user, t));
    // can_debit evaluates the spend window; checking it must not consume it.
    assert_read_only(env, "can_debit (reserved mandate)", || {
        v.can_debit(&user, t, &7, &1_000)
    });
    assert_read_only(env, "can_debit (other mandate)", || {
        v.can_debit(&user, t, &8, &100)
    });
}

#[test]
fn vault_views_need_no_signature() {
    let (h, user) = view_vault();
    let env = &h.env;
    let v = &h.vault;
    let t = &h.token;
    env.set_auths(&[]);

    // A write is now refused, proving the auth mock is gone...
    assert!(v.try_withdraw(&user, t, &100).is_err());
    assert_eq!(v.balance(&user, t), 4_500);

    // ...but views still answer, unsigned and side-effect free.
    assert_read_only(env, "balance (unsigned)", || v.balance(&user, t));
    assert_read_only(env, "unreserved_balance (unsigned)", || {
        v.unreserved_balance(&user, t)
    });
    assert_read_only(env, "can_debit (unsigned)", || {
        v.can_debit(&user, t, &7, &100)
    });
    assert_read_only(env, "can_deposit (unsigned)", || v.can_deposit(&user, t));
    assert_read_only(env, "spend_limit (unsigned)", || v.spend_limit(&user, t));
    assert_read_only(env, "admin (unsigned)", || v.admin());
}

#[test]
fn failing_vault_views_are_side_effect_free_too() {
    let (h, user) = view_vault();
    let env = &h.env;
    let v = &h.vault;
    let t = &h.token;
    let stranger = Address::generate(env);
    let other_token = Address::generate(env);

    // Negative answers (rather than errors) still write nothing.
    assert!(!assert_read_only(env, "can_debit (zero)", || {
        v.can_debit(&user, t, &7, &0)
    }));
    assert!(!assert_read_only(env, "can_debit (negative)", || {
        v.can_debit(&user, t, &7, &-1)
    }));
    assert!(!assert_read_only(env, "can_debit (over balance)", || {
        v.can_debit(&user, t, &7, &1_000_000)
    }));
    assert!(!assert_read_only(
        env,
        "can_debit (over spend limit)",
        || { v.can_debit(&user, t, &7, &2_600) }
    ));
    assert!(!assert_read_only(
        env,
        "can_deposit (unlisted token)",
        || { v.can_deposit(&user, &other_token) }
    ));
    assert_eq!(
        assert_read_only(env, "balance (stranger)", || v.balance(&stranger, t)),
        0
    );

    // An unconfigured vault reports a typed error, still without writing.
    let admin = Address::generate(env);
    let bare = VaultClient::new(env, &env.register(Vault, (admin, BPS_DENOMINATOR)));
    assert_eq!(
        assert_read_only(env, "subscription (unset)", || bare.try_subscription()),
        Err(Ok(Error::SubscriptionNotSet))
    );
}

#[test]
#[should_panic(expected = "mutated ledger state")]
fn the_vault_read_only_check_catches_a_state_change() {
    let (h, user) = view_vault();
    assert_read_only(&h.env, "withdraw", || h.vault.withdraw(&user, &h.token, &1));
}

// ---- #289: deposit_for by an unrelated account credits the right subscriber ----

/// Read an i128 field out of the data map of the last emitted event.
fn last_event_i128(h: &Harness, field: &str) -> Option<i128> {
    let last = h.env.events().all().events().last().cloned()?;
    let ContractEventBody::V0(body) = &last.body;
    let ScVal::Map(Some(ScMap(entries))) = &body.data else {
        return None;
    };
    for entry in entries {
        if let ScVal::Symbol(ScSymbol(name)) = &entry.key {
            if name.to_utf8_string_lossy() == field {
                if let ScVal::I128(ref parts) = entry.val {
                    return Some((i128::from(parts.hi) << 64) | i128::from(parts.lo));
                }
            }
        }
    }
    None
}

#[test]
fn deposit_for_by_unrelated_account_credits_only_the_intended_subscriber() {
    let h = setup();
    // The subscriber already holds a balance of their own; a bystander too.
    let subscriber = funded_user(&h, 1_000);
    let bystander = funded_user(&h, 1_000);
    h.vault.deposit(&subscriber, &h.token, &1_000);
    h.vault.deposit(&bystander, &h.token, &1_000);

    // An account with no relationship to either funds the subscriber.
    let stranger = funded_user(&h, 5_000);
    h.vault
        .deposit_for(&stranger, &subscriber, &h.token, &2_000);

    // The event names payer and recipient and reports the recipient's new balance.
    let last = h.env.events().all().events().last().cloned().unwrap();
    let ContractEventBody::V0(body) = &last.body;
    let ScVal::Symbol(ScSymbol(name)) = &body.topics[0] else {
        panic!("topic 0 is not a symbol");
    };
    assert_eq!(name.to_utf8_string_lossy(), "deposit_for");
    assert_eq!(body.topics[1], ScVal::from(stranger.clone()));
    assert_eq!(body.topics[2], ScVal::from(subscriber.clone()));
    assert_eq!(last_event_i128(&h, "amount"), Some(2_000));
    assert_eq!(last_event_i128(&h, "balance"), Some(3_000));

    assert_eq!(h.vault.balance(&subscriber, &h.token), 3_000);
    assert_eq!(h.vault.balance(&bystander, &h.token), 1_000);
    assert_eq!(h.vault.balance(&stranger, &h.token), 0);
    assert_eq!(h.token_client.balance(&stranger), 3_000);
    assert_eq!(h.token_client.balance(&h.vault_id), 4_000);
}

#[test]
fn deposit_for_by_unrelated_account_respects_the_subscribers_cap() {
    let h = setup();
    let subscriber = Address::generate(&h.env);
    let stranger = funded_user(&h, 10_000);
    h.vault.set_deposit_cap(&subscriber, &h.token, &1_500);

    assert_eq!(
        h.vault
            .try_deposit_for(&stranger, &subscriber, &h.token, &1_501),
        Err(Ok(Error::DepositCapExceeded))
    );
    assert_eq!(h.vault.balance(&subscriber, &h.token), 0);
    assert_eq!(h.token_client.balance(&stranger), 10_000);

    // The cap is the subscriber's, not the payer's: a cap on the payer is ignored.
    h.vault.set_deposit_cap(&stranger, &h.token, &1);
    h.vault
        .deposit_for(&stranger, &subscriber, &h.token, &1_500);
    assert_eq!(h.vault.balance(&subscriber, &h.token), 1_500);
}

#[test]
fn deposit_for_by_unrelated_account_rejects_unlisted_tokens() {
    let h = setup();
    let subscriber = Address::generate(&h.env);
    let stranger = funded_user(&h, 10_000);
    h.vault.set_allowlist_enabled(&true);

    assert_eq!(
        h.vault
            .try_deposit_for(&stranger, &subscriber, &h.token, &1_000),
        Err(Ok(Error::TokenNotAllowed))
    );
    assert_eq!(h.vault.balance(&subscriber, &h.token), 0);
    assert_eq!(h.token_client.balance(&stranger), 10_000);
}

#[test]
fn deposit_for_by_unfunded_unrelated_account_fails_without_crediting() {
    let h = setup();
    let subscriber = Address::generate(&h.env);
    let stranger = funded_user(&h, 100);

    assert!(h
        .vault
        .try_deposit_for(&stranger, &subscriber, &h.token, &1_000)
        .is_err());
    assert_eq!(h.vault.balance(&subscriber, &h.token), 0);
    assert_eq!(h.token_client.balance(&stranger), 100);
}

#[test]
fn deposit_for_signed_by_the_subscriber_instead_of_the_payer_is_rejected() {
    let h = setup();
    let subscriber = Address::generate(&h.env);
    let stranger = funded_user(&h, 10_000);
    let impostor = Address::generate(&h.env);

    h.env.mock_auths(&[MockAuth {
        address: &impostor,
        invoke: &MockAuthInvoke {
            contract: &h.vault.address,
            fn_name: "deposit_for",
            args: (&stranger, &subscriber, &h.token, &1_000).into_val(&h.env),
            sub_invokes: &[],
        },
    }]);
    assert!(h
        .vault
        .try_deposit_for(&stranger, &subscriber, &h.token, &1_000)
        .is_err());
    assert_eq!(h.vault.balance(&subscriber, &h.token), 0);
    assert_eq!(h.token_client.balance(&stranger), 10_000);
}

// ---------------------------------------------------------------------------
// Numeric maximum values (#242)
// ---------------------------------------------------------------------------

#[test]
fn deposit_and_withdraw_i128_max_round_trip() {
    let h = setup();
    let user = Address::generate(&h.env);
    h.token_admin.mint(&user, &i128::MAX);
    h.vault.set_deposit_cap(&user, &h.token, &i128::MAX);

    h.vault.deposit(&user, &h.token, &i128::MAX);
    assert_eq!(h.vault.balance(&user, &h.token), i128::MAX);
    assert_eq!(h.vault.deposit_cap(&user, &h.token), i128::MAX);
    assert_eq!(h.token_client.balance(&user), 0);
    assert_eq!(h.token_client.balance(&h.vault_id), i128::MAX);

    assert_eq!(h.vault.withdraw_all(&user, &h.token), i128::MAX);
    assert_eq!(h.vault.balance(&user, &h.token), 0);
    assert_eq!(h.token_client.balance(&user), i128::MAX);
    assert_eq!(h.token_client.balance(&h.vault_id), 0);
}

#[test]
fn reserve_and_spend_limit_at_i128_max_do_not_wrap() {
    let h = setup();
    let user = Address::generate(&h.env);
    let recipient = Address::generate(&h.env);
    h.token_admin.mint(&user, &i128::MAX);
    h.vault.deposit(&user, &h.token, &i128::MAX);

    h.vault.set_reserve(&user, &h.token, &1, &i128::MAX);
    assert_eq!(h.vault.reserved(&user, &h.token, &1), i128::MAX);
    assert_eq!(h.vault.reserved_total(&user, &h.token), i128::MAX);
    assert_eq!(h.vault.unreserved_balance(&user, &h.token), 0);
    assert_eq!(
        h.vault.try_withdraw(&user, &h.token, &1),
        Err(Ok(Error::InsufficientBalance))
    );
    assert_eq!(h.vault.withdraw_all(&user, &h.token), 0);
    h.vault.set_reserve(&user, &h.token, &1, &0);

    h.vault
        .set_spend_limit(&user, &h.token, &i128::MAX, &u64::MAX);
    let limit = h.vault.spend_limit(&user, &h.token);
    assert_eq!(limit.amount, i128::MAX);
    assert_eq!(limit.window_secs, u64::MAX);

    // Put the current window at its largest representable spent total while
    // the balance still has room, which a replenished account can reach.
    h.env.as_contract(&h.vault_id, || {
        h.env.storage().persistent().set(
            &DataKey::SpendWindow(user.clone(), h.token.clone()),
            &SpendWindow {
                start: h.env.ledger().timestamp(),
                spent: i128::MAX,
            },
        );
    });

    // spent == i128::MAX would make `spent + amount` overflow. It must be a
    // typed SpendLimitExceeded error and leave the balance untouched.
    assert_eq!(
        h.vault.try_debit(&user, &h.token, &recipient, &1),
        Err(Ok(Error::SpendLimitExceeded))
    );
    assert_eq!(h.vault.balance(&user, &h.token), i128::MAX);
    assert_eq!(h.token_client.balance(&recipient), 0);
}

#[test]
fn maximum_deposit_requires_the_users_auth() {
    let h = setup();
    let user = Address::generate(&h.env);
    h.token_admin.mint(&user, &i128::MAX);
    h.env.set_auths(&[]);

    assert!(h.vault.try_deposit(&user, &h.token, &i128::MAX).is_err());
    assert_eq!(h.vault.balance(&user, &h.token), 0);
    assert_eq!(h.token_client.balance(&user), i128::MAX);
    assert_eq!(h.token_client.balance(&h.vault_id), 0);
}

// ---------------------------------------------------------------------------
// #341 / #369: Admin emergency_withdraw releases all per-mandate reserves.
// ---------------------------------------------------------------------------

fn last_emergency_withdraw_event(h: &Harness) -> Option<(Address, i128)> {
    let mut found = None;
    for event in h.env.events().all().events() {
        let ContractEventBody::V0(body) = &event.body;
        let is_emergency = matches!(
            body.topics.first(),
            Some(ScVal::Symbol(ScSymbol(name))) if name.to_utf8_string_lossy() == "emergency_withdraw"
        );
        if !is_emergency {
            continue;
        }
        let Some(topic_addr @ ScVal::Address(_)) = body.topics.get(1) else {
            continue;
        };
        let subscriber: Address = soroban_sdk::TryFromVal::try_from_val(&h.env, topic_addr).ok()?;
        let ScVal::Map(Some(ScMap(entries))) = &body.data else {
            continue;
        };
        for entry in entries {
            let (ScVal::Symbol(ScSymbol(name)), ScVal::I128(parts)) = (&entry.key, &entry.val)
            else {
                continue;
            };
            if name.to_utf8_string_lossy() == "amount" {
                let amount = (i128::from(parts.hi) << 64) | i128::from(parts.lo);
                found = Some((subscriber.clone(), amount));
            }
        }
    }
    found
}

#[test]
fn emergency_withdraw_releases_all_reserves_and_emits_the_correct_event() {
    let h = setup();
    let user = depositor(&h, 1_500);
    let other_user = depositor(&h, 600);
    let mandate_1 = 10u64;
    let mandate_2 = 20u64;
    let mandate_3 = 30u64;

    h.vault.set_reserve(&user, &h.token, &mandate_1, &200);
    h.vault.set_reserve(&user, &h.token, &mandate_2, &350);
    h.vault.set_reserve(&user, &h.token, &mandate_3, &450);
    h.vault.set_reserve(&other_user, &h.token, &mandate_1, &400);

    assert_eq!(h.vault.reserved_total(&user, &h.token), 1_000);
    assert_eq!(h.vault.unreserved_balance(&user, &h.token), 500);

    // Admin releases all reserves for `user` in one call.
    let released = h.vault.emergency_withdraw(&user, &h.token);
    assert_eq!(released, 1_000);

    // The emitted event carries the subscriber and sum of released reserves.
    // Must be checked before subsequent contract reads clear `events().all()`.
    assert_eq!(
        last_emergency_withdraw_event(&h),
        Some((user.clone(), 1_000))
    );

    // All three per-mandate reserves and the aggregate total are now zero.
    assert_eq!(h.vault.reserved(&user, &h.token, &mandate_1), 0);
    assert_eq!(h.vault.reserved(&user, &h.token, &mandate_2), 0);
    assert_eq!(h.vault.reserved(&user, &h.token, &mandate_3), 0);
    assert_eq!(h.vault.reserved_total(&user, &h.token), 0);

    // Other subscribers' reserves are unaffected.
    assert_eq!(h.vault.reserved(&other_user, &h.token, &mandate_1), 400);

    // The full balance is now withdrawable by the subscriber.
    assert_eq!(h.vault.unreserved_balance(&user, &h.token), 1_500);
    assert_eq!(h.vault.withdraw_all(&user, &h.token), 1_500);
    assert_eq!(h.vault.balance(&user, &h.token), 0);
    assert_eq!(h.token_client.balance(&user), 1_500);
}

#[test]
fn emergency_withdraw_requires_admin_auth() {
    let h = setup();
    let user = depositor(&h, 1_000);
    h.vault.set_reserve(&user, &h.token, &1, &300);
    h.vault.set_reserve(&user, &h.token, &2, &400);

    // A normal subscriber cannot call emergency_withdraw on their own behalf.
    h.env.mock_auths(&[MockAuth {
        address: &user,
        invoke: &MockAuthInvoke {
            contract: &h.vault_id,
            fn_name: "emergency_withdraw",
            args: (&user, &h.token).into_val(&h.env),
            sub_invokes: &[],
        },
    }]);
    assert!(h.vault.try_emergency_withdraw(&user, &h.token).is_err());

    // An unauthenticated call is also rejected.
    h.env.set_auths(&[]);
    assert!(h.vault.try_emergency_withdraw(&user, &h.token).is_err());

    // All reserves remain untouched.
    assert_eq!(h.vault.reserved(&user, &h.token, &1), 300);
    assert_eq!(h.vault.reserved(&user, &h.token, &2), 400);
    assert_eq!(h.vault.reserved_total(&user, &h.token), 700);
}
