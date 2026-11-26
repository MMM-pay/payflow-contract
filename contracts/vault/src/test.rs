#![cfg(test)]
use super::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

#[test]
fn test_balance_detail_matches_manual_reserves() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let subscriber = Address::generate(&env);
    let token_contract = env.register_contract_none(());

    let contract_id = env.register(VaultContract, (&admin, 60_u64, 8_u32));
    let client = VaultContractClient::new(&env, &contract_id);

    client.deposit(&subscriber, &token_contract, &1000);

    let detail_initial = client.balance_detail(&subscriber, &token_contract);
    assert_eq!(detail_initial.total, 1000);
    assert_eq!(detail_initial.locked, 0);
    assert_eq!(detail_initial.free, 1000);

    client.lock_reserve(&subscriber, &token_contract, &1_u64, &250);
    client.lock_reserve(&subscriber, &token_contract, &2_u64, &350);

    let detail_locked = client.balance_detail(&subscriber, &token_contract);
    assert_eq!(detail_locked.total, 1000);
    assert_eq!(detail_locked.locked, 600);
    assert_eq!(detail_locked.free, 400);

    let manual_sum = 250 + 350;
    assert_eq!(detail_locked.locked, manual_sum);
    assert_eq!(detail_locked.free, detail_locked.total - manual_sum);
}
