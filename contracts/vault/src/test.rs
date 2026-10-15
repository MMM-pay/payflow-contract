#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::Address as _,
    token::{StellarAssetClient, TokenClient},
    Address, Env,
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
