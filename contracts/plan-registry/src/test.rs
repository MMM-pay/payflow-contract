#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, Address, Env, String};

fn setup() -> (Env, PlanRegistryClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register(PlanRegistry, (admin.clone(),));
    let client = PlanRegistryClient::new(&env, &id);
    (env, client, admin)
}

#[test]
fn constructor_sets_admin() {
    let (_env, client, admin) = setup();
    assert_eq!(client.admin(), admin);
}

#[test]
fn create_plan_assigns_sequential_ids() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);

    let a = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &2_592_000,
        &String::from_str(&env, "Pro Monthly"),
    );
    let b = client.create_plan(
        &merchant,
        &token,
        &2_000,
        &2_592_000,
        &String::from_str(&env, "Pro Monthly"),
    );

    assert_eq!(a, 1);
    assert_eq!(b, 2);
    assert_eq!(
        client.merchant_plans(&merchant, &0, &10),
        soroban_sdk::vec![&env, 1, 2]
    );
    assert_eq!(client.merchant_plan_count(&merchant), 2);
    assert_eq!(client.next_plan_id(), 3);
}

#[test]
fn create_plan_stores_fields() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);

    let id = client.create_plan(
        &merchant,
        &token,
        &5_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
    );
    let plan = client.get_plan(&id);

    assert_eq!(plan.merchant, merchant);
    assert_eq!(plan.token, token);
    assert_eq!(plan.amount, 5_000);
    assert_eq!(plan.period, 604_800);
    assert!(plan.active);
}

#[test]
fn create_plan_rejects_zero_amount() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let res = client.try_create_plan(
        &merchant,
        &token,
        &0,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
    );
    assert_eq!(res, Err(Ok(Error::InvalidAmount)));
}

#[test]
fn create_plan_rejects_negative_amount() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let res = client.try_create_plan(
        &merchant,
        &token,
        &-1,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
    );
    assert_eq!(res, Err(Ok(Error::InvalidAmount)));
}

#[test]
fn create_plan_rejects_sub_minimum_period() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let res = client.try_create_plan(
        &merchant,
        &token,
        &1_000,
        &(MIN_PERIOD - 1),
        &String::from_str(&env, "Pro Monthly"),
    );
    assert_eq!(res, Err(Ok(Error::InvalidPeriod)));
}

#[test]
fn get_plan_unknown_id_errors() {
    let (_env, client, _admin) = setup();
    assert_eq!(client.try_get_plan(&42), Err(Ok(Error::PlanNotFound)));
}

#[test]
fn set_plan_active_toggles() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
    );

    client.set_plan_active(&merchant, &id, &false);
    assert!(!client.get_plan(&id).active);

    client.set_plan_active(&merchant, &id, &true);
    assert!(client.get_plan(&id).active);
}

#[test]
fn set_plan_active_rejects_non_owner() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let attacker = Address::generate(&env);
    let token = Address::generate(&env);
    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
    );

    let res = client.try_set_plan_active(&attacker, &id, &false);
    assert_eq!(res, Err(Ok(Error::NotPlanOwner)));
}

#[test]
fn merchant_plans_empty_for_unknown_merchant() {
    let (env, client, _admin) = setup();
    let stranger = Address::generate(&env);
    assert_eq!(client.merchant_plans(&stranger, &0, &10).len(), 0);
    assert_eq!(client.merchant_plan_count(&stranger), 0);
}

#[test]
fn create_plan_stores_the_name() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);

    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
    );
    assert_eq!(
        client.get_plan(&id).name,
        String::from_str(&env, "Pro Monthly")
    );
}

#[test]
fn create_plan_accepts_an_empty_name() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);

    // A name is optional; the UI falls back to the plan number.
    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, ""),
    );
    assert_eq!(client.get_plan(&id).name.len(), 0);
}

#[test]
fn create_plan_accepts_a_name_at_the_length_limit() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);

    let at_limit = "x".repeat(MAX_NAME_LEN as usize);
    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, &at_limit),
    );
    assert_eq!(client.get_plan(&id).name.len(), MAX_NAME_LEN);
}

#[test]
fn create_plan_rejects_an_overlong_name() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);

    let too_long = "x".repeat(MAX_NAME_LEN as usize + 1);
    let res = client.try_create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, &too_long),
    );
    assert_eq!(res, Err(Ok(Error::NameTooLong)));
}

#[test]
fn merchant_plans_pages_are_bounded() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let name = String::from_str(&env, "Plan");

    for _ in 0..(MAX_PAGE + 3) {
        client.create_plan(&merchant, &token, &1_000, &3_600, &name);
    }

    assert_eq!(client.merchant_plan_count(&merchant), MAX_PAGE + 3);
    assert_eq!(
        client.merchant_plans(&merchant, &0, &u32::MAX).len(),
        MAX_PAGE
    );
    assert_eq!(
        client.merchant_plans(&merchant, &MAX_PAGE, &10),
        soroban_sdk::vec![
            &env,
            u64::from(MAX_PAGE) + 1,
            u64::from(MAX_PAGE) + 2,
            u64::from(MAX_PAGE) + 3
        ]
    );
    assert_eq!(
        client.merchant_plans(&merchant, &u32::MAX, &u32::MAX).len(),
        0
    );
}

#[test]
fn next_plan_id_starts_at_one() {
    let (_env, client, _admin) = setup();
    assert_eq!(client.next_plan_id(), 1);
}
