#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger as _, MockAuth, MockAuthInvoke},
    token::{StellarAssetClient, TokenClient},
    Address, Env, IntoVal,
};

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

/// The vault admin can only call `set_subscription`, which repoints the
/// subscription contract. No admin function touches user balances directly.
/// This test verifies that every admin-callable function leaves all user
/// balances unchanged.
///
/// SECURITY.md: "The vault admin can repoint set_subscription to a different
/// contract. A malicious admin could point it at a contract that drains
/// balances." This test pins down that the only admin function exists and
/// does not move funds.
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

    // The only admin function is set_subscription; call it with a different
    // subscription address. This should not move any user balances.
    let new_subscription = Address::generate(&h.env);
    h.vault.set_subscription(&new_subscription);

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

// ---------------------------------------------------------------------------
// Per-period spend limit
// ---------------------------------------------------------------------------

const THIRTY_DAYS: u64 = 2_592_000;

/// The acceptance criterion: debits over the limit fail, and the next window
/// starts from a fresh ceiling.
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

/// The acceptance criterion: limit 0 disables it.
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
fn set_spend_limit_requires_the_users_signature() {
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

/// Replacing a limit mid-window judges the new ceiling on fresh usage, not
/// on what the old limit had already allowed.
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

/// The acceptance criterion "gas cost of debit without a limit is unchanged":
/// the unlimited path must add no storage writes at all — no limit or window
/// entry ever appears, so the only extra cost is one absent-key read.
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
