#![cfg(test)]

use super::*;
use payflow_test_token::{TestToken, TestTokenClient};
use soroban_sdk::{testutils::AuthorizedFunction, Symbol};
use soroban_sdk::{
    testutils::{Address as _, Events as _, Ledger as _, MockAuth, MockAuthInvoke},
    token::{StellarAssetClient, TokenClient},
    xdr::{ContractEventBody, ScSymbol, ScVal},
    Address, Env, IntoVal, String,
};

const THIRTY_DAYS: u64 = 2_592_000;

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
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let subscription = Address::generate(&env);

    let sac = env.register_stellar_asset_contract_v2(issuer);
    let token = sac.address();

    let vault_id = env.register(Vault, (admin.clone(),));
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
        for entry in entries.iter() {
            if let ScVal::Symbol(ScSymbol(name)) = &entry.key {
                if name.to_utf8_string_lossy() == "balance" {
                    if let ScVal::I128(ref parts) = entry.val {
                        return Some(((parts.hi as i128) << 64) | (parts.lo as i128));
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
/// SECURITY.md: "The vault admin can repoint set_subscription to a different
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

    let bal_a_before = h.vault.balance(&user_a, &h.token);
    let bal_b_before = h.vault.balance(&user_b, &h.token);

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

    let bal_a_after = h.vault.balance(&user_a, &h.token);
    let bal_b_after = h.vault.balance(&user_b, &h.token);

    assert_eq!(
        bal_a_before, bal_a_after,
        "set_subscription must not change user balances"
    );
    assert_eq!(
        bal_b_before, bal_b_after,
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
        let i = rng.below(4) as usize;
        let user = users[i].clone();

        if expected[i] == 0 {
            // Nothing to withdraw or debit: fund the user instead. Each user's
            // wallet holds 10_000_000 tokens at most, and a user is picked at
            // most 300 times with deposits capped at 5_000, so it never runs
            // dry.
            let amount = 1_000 + (rng.below(5_000) as i128);
            h.vault.deposit(&user, &h.token, &amount);
            expected[i] += amount;
            assert_invariant(&h, &users, &expected);
            continue;
        }

        match rng.below(3) {
            0 => {
                let amount = 1 + (rng.below(5_000) as i128);
                h.vault.deposit(&user, &h.token, &amount);
                expected[i] += amount;
            }
            1 => {
                let amount = 1 + (rng.below(expected[i] as u64) as i128);
                h.vault.withdraw(&user, &h.token, &amount);
                expected[i] -= amount;
            }
            _ => {
                let amount = 1 + (rng.below(expected[i] as u64) as i128);
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
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
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
    test_token: TestTokenClient<'a>,
}

fn setup_failing_token() -> FailingTokenHarness<'static> {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let subscription = Address::generate(&env);

    let token = env.register(TestToken, (token_admin,));
    let test_token = TestTokenClient::new(&env, &token);

    let vault_id = env.register(Vault, (admin,));
    let vault = VaultClient::new(&env, &vault_id);
    vault.set_subscription(&subscription);

    FailingTokenHarness {
        env,
        vault,
        vault_id,
        token,
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
fn update_admin_hands_the_role_over() {
    let h = setup();
    let next = Address::generate(&h.env);

    h.vault.update_admin(&next);
    assert_eq!(h.vault.admin(), next);
}

#[test]
fn update_admin_requires_the_current_admins_auth() {
    let h = setup();
    let before = h.vault.admin();
    let next = Address::generate(&h.env);

    h.env.set_auths(&[]);
    assert!(h.vault.try_update_admin(&next).is_err());
    assert_eq!(h.vault.admin(), before);
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
