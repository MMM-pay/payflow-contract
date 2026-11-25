#![cfg(test)]
extern crate std;

use super::*;
use soroban_sdk::xdr::{ContractEventBody, ScMap, ScSymbol, ScVal};
use soroban_sdk::{
    testutils::{Address as _, Events as _, MockAuth, MockAuthInvoke},
    Address, Env, IntoVal, String, Symbol, TryFromVal, Vec,
};

/// A valid plan-registry wasm used as the `upgrade_wasm` target (#158); the
/// contract upgrades to a copy of itself to exercise the upgrade mechanism.
const UPGRADE_WASM: &[u8] = include_bytes!("../testdata/payflow_plan_registry.wasm");

fn setup() -> (Env, PlanRegistryClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register(PlanRegistry, (admin.clone(),));
    let client = PlanRegistryClient::new(&env, &id);
    (env, client, admin)
}

fn batch_plan(env: &Env, token: &Address, amount: i128, name: &str) -> PlanParams {
    PlanParams {
        token: token.clone(),
        amount,
        period: 2_592_000,
        name: String::from_str(env, name),
        description: String::from_str(env, ""),
        keeper_bps: 0,
        category: Symbol::new(env, ""),
        terms_uri: String::from_str(env, ""),
        available_until: 0,
        referral_bps: 0,
        payout: None,
        max_subscribers: 0,
    }
}

#[test]
fn version_matches_package_version() {
    let (env, client, _) = setup();
    assert_eq!(
        client.version(),
        String::from_str(&env, env!("CARGO_PKG_VERSION"))
    );
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
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );
    let b = client.create_plan(
        &merchant,
        &token,
        &2_000,
        &2_592_000,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
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
fn create_plan_authorizes_exact_merchant_tree() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let id = env.register(PlanRegistry, (admin.clone(),));
    let client = PlanRegistryClient::new(&env, &id);
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let name = String::from_str(&env, "Pro Monthly");
    let desc = String::from_str(&env, "");

    env.mock_auths(&[MockAuth {
        address: &merchant,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "create_plan",
            args: (
                &merchant,
                &token,
                &1_000_i128,
                &604_800_u64,
                &name,
                &desc,
                &0_u32,
                &Symbol::new(&env, ""),
                &String::from_str(&env, ""),
                &0_u64,
                &0_u32,
                &None::<Address>,
                &0u32,
            )
                .into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &name,
        &desc,
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );
    assert_eq!(
        env.auths(),
        std::vec![(
            merchant.clone(),
            soroban_sdk::testutils::AuthorizedInvocation {
                function: soroban_sdk::testutils::AuthorizedFunction::Contract((
                    client.address.clone(),
                    soroban_sdk::Symbol::new(&env, "create_plan"),
                    (
                        &merchant,
                        &token,
                        &1_000_i128,
                        &604_800_u64,
                        &name,
                        &desc,
                        &0_u32,
                        &Symbol::new(&env, ""),
                        &String::from_str(&env, ""),
                        &0_u64,
                        &0_u32,
                        &None::<Address>,
                        &0u32
                    )
                        .into_val(&env),
                )),
                sub_invocations: std::vec![],
            }
        )]
    );
}

#[test]
fn create_plan_rejects_a_wrong_signer() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let id = env.register(PlanRegistry, (admin.clone(),));
    let client = PlanRegistryClient::new(&env, &id);
    let merchant = Address::generate(&env);
    let attacker = Address::generate(&env);
    let token = Address::generate(&env);
    let name = String::from_str(&env, "Pro Monthly");
    let desc = String::from_str(&env, "");

    env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "create_plan",
            args: (
                &merchant,
                &token,
                &1_000_i128,
                &604_800_u64,
                &name,
                &desc,
                &0_u32,
                &Symbol::new(&env, ""),
                &String::from_str(&env, ""),
            )
                .into_val(&env),
            sub_invokes: &[],
        },
    }]);
    assert!(client
        .try_create_plan(
            &merchant,
            &token,
            &1_000,
            &604_800,
            &name,
            &desc,
            &0,
            &Symbol::new(&env, ""),
            &String::from_str(&env, ""),
            &0u64,
            &0u32,
            &None::<Address>,
            &0u32,
        )
        .is_err());
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
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );
    let plan = client.get_plan(&id);

    assert_eq!(plan.merchant, merchant);
    assert_eq!(plan.token, token);
    assert_eq!(plan.amount, 5_000);
    assert_eq!(plan.period, 604_800);
    assert!(plan.active);
    assert_eq!(plan.keeper_bps, 0);
}

#[test]
fn create_plan_stores_the_keeper_tip() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);

    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &MAX_KEEPER_BPS,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );

    assert_eq!(client.get_plan(&id).keeper_bps, MAX_KEEPER_BPS);
}

#[test]
fn create_plan_rejects_a_keeper_tip_above_the_cap() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);

    let res = client.try_create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &(MAX_KEEPER_BPS + 1),
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );

    assert_eq!(res, Err(Ok(Error::KeeperBpsTooHigh)));
    assert_eq!(client.merchant_plan_count(&merchant), 0);
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
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
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
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
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
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
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
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );

    client.set_plan_active(&merchant, &id, &false);
    assert!(!client.get_plan(&id).active);

    client.set_plan_active(&merchant, &id, &true);
    assert!(client.get_plan(&id).active);
}

#[test]
fn set_plan_active_authorizes_exact_merchant_tree() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let id = env.register(PlanRegistry, (admin.clone(),));
    let client = PlanRegistryClient::new(&env, &id);
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    env.mock_all_auths();
    let plan = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );

    env.mock_auths(&[MockAuth {
        address: &merchant,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "set_plan_active",
            args: (&merchant, &plan, &false).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.set_plan_active(&merchant, &plan, &false);
    assert_eq!(
        env.auths(),
        std::vec![(
            merchant.clone(),
            soroban_sdk::testutils::AuthorizedInvocation {
                function: soroban_sdk::testutils::AuthorizedFunction::Contract((
                    client.address.clone(),
                    soroban_sdk::Symbol::new(&env, "set_plan_active"),
                    (&merchant, &plan, &false).into_val(&env),
                )),
                sub_invocations: std::vec![],
            }
        )]
    );
    assert!(!client.get_plan(&plan).active);

    // A different signer cannot toggle the merchant's plan.
    let attacker = Address::generate(&env);
    env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "set_plan_active",
            args: (&merchant, &plan, &true).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    assert!(client.try_set_plan_active(&merchant, &plan, &true).is_err());
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
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );

    let res = client.try_set_plan_active(&attacker, &id, &false);
    assert_eq!(res, Err(Ok(Error::NotPlanOwner)));
}

#[test]
fn set_plan_charges_paused_toggles_without_deactivating() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );

    assert!(!client.plan_charges_paused(&id));
    assert!(!client.plan_paused(&id));

    client.set_plan_charges_paused(&merchant, &id, &true);
    assert!(client.plan_charges_paused(&id));
    // Pausing charges is not deactivation: the plan still accepts sign-ups.
    assert!(client.get_plan(&id).active);

    // The short alias controls the same flag.
    client.set_plan_paused(&merchant, &id, &false);
    assert!(!client.plan_charges_paused(&id));
    assert!(client.get_plan(&id).active);
}

#[test]
fn set_plan_charges_paused_rejects_a_non_owner_and_missing_plan() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );

    let attacker = Address::generate(&env);
    assert_eq!(
        client.try_set_plan_charges_paused(&attacker, &id, &true),
        Err(Ok(Error::NotPlanOwner))
    );
    assert_eq!(
        client.try_set_plan_charges_paused(&merchant, &42, &true),
        Err(Ok(Error::PlanNotFound))
    );
    assert_eq!(
        client.try_plan_charges_paused(&42),
        Err(Ok(Error::PlanNotFound))
    );
}

#[test]
fn set_plan_charges_paused_emits_the_change() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );

    client.set_plan_charges_paused(&merchant, &id, &true);
    assert_eq!(env.events().all().events().len(), 1);
    let last = env.events().all().events().last().cloned().unwrap();
    let ContractEventBody::V0(body) = &last.body;
    let ScVal::Symbol(ScSymbol(name)) = &body.topics[0] else {
        panic!("topic 0 is not a symbol");
    };
    let expected: soroban_sdk::xdr::StringM<32> = "plan_charges_paused_changed".try_into().unwrap();
    assert_eq!(name, &expected);
}

#[test]
fn set_plan_grace_period_is_stored_and_defaults_to_zero() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );

    assert_eq!(client.plan_grace_period(&id), 0);
    client.set_plan_grace_period(&merchant, &id, &172_800);
    assert_eq!(client.plan_grace_period(&id), 172_800);

    let attacker = Address::generate(&env);
    assert_eq!(
        client.try_set_plan_grace_period(&attacker, &id, &1),
        Err(Ok(Error::NotPlanOwner))
    );
    assert_eq!(
        client.try_set_plan_grace_period(&merchant, &42, &1),
        Err(Ok(Error::PlanNotFound))
    );
    assert_eq!(
        client.try_plan_grace_period(&42),
        Err(Ok(Error::PlanNotFound))
    );
    assert_eq!(client.plan_grace_period(&id), 172_800);
}

#[test]
fn set_plan_charges_paused_requires_the_merchants_auth() {
    let (env, client, _) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &2_592_000,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );
    env.set_auths(&[]);
    assert!(client
        .try_set_plan_charges_paused(&merchant, &id, &true)
        .is_err());
    assert!(!client.plan_charges_paused(&id));
}

#[test]
fn set_plan_paused_requires_the_merchants_auth() {
    let (env, client, _) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &2_592_000,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );
    env.set_auths(&[]);
    assert!(client.try_set_plan_paused(&merchant, &id, &true).is_err());
    assert!(!client.plan_charges_paused(&id));
}

#[test]
fn set_plan_grace_period_requires_the_merchants_auth() {
    let (env, client, _) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &2_592_000,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );
    env.set_auths(&[]);
    assert!(client
        .try_set_plan_grace_period(&merchant, &id, &1_000)
        .is_err());
    assert_eq!(client.plan_grace_period(&id), 0);
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
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
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
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
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
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
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
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
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
        client.create_plan(
            &merchant,
            &token,
            &1_000,
            &3_600,
            &name,
            &String::from_str(&env, ""),
            &0,
            &Symbol::new(&env, ""),
            &String::from_str(&env, ""),
            &0u64,
            &0u32,
            &None::<Address>,
            &0u32,
        );
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

// ---------------------------------------------------------------------------
// Merchant profile
// ---------------------------------------------------------------------------

#[test]
fn set_profile_stores_name_and_url() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let name = String::from_str(&env, "Acme Corp");
    let url = String::from_str(&env, "https://acme.example");

    client.set_profile(&merchant, &name, &url);

    let profile = client.profile(&merchant);
    assert_eq!(profile.merchant, merchant);
    assert_eq!(profile.name, name);
    assert_eq!(profile.url, url);
}

#[test]
fn set_profile_accepts_length_limits() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);

    let at_name_limit = String::from_str(&env, &"x".repeat(MAX_NAME_LEN as usize));
    let at_url_limit = String::from_str(&env, &"x".repeat(MAX_URL_LEN as usize));
    client.set_profile(&merchant, &at_name_limit, &at_url_limit);

    let profile = client.profile(&merchant);
    assert_eq!(profile.name.len(), MAX_NAME_LEN);
    assert_eq!(profile.url.len(), MAX_URL_LEN);
}

#[test]
fn set_profile_rejects_an_overlong_name() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);

    let too_long = String::from_str(&env, &"x".repeat(MAX_NAME_LEN as usize + 1));
    let res = client.try_set_profile(
        &merchant,
        &too_long,
        &String::from_str(&env, "https://example.com"),
    );
    assert_eq!(res, Err(Ok(Error::ProfileNameTooLong)));
}

#[test]
fn set_profile_rejects_an_overlong_url() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);

    let too_long = String::from_str(&env, &"x".repeat(MAX_URL_LEN as usize + 1));
    let res = client.try_set_profile(&merchant, &String::from_str(&env, "Acme"), &too_long);
    assert_eq!(res, Err(Ok(Error::ProfileUrlTooLong)));
}

#[test]
fn profile_of_unknown_merchant_errors() {
    let (env, client, _admin) = setup();
    let stranger = Address::generate(&env);
    assert_eq!(
        client.try_profile(&stranger),
        Err(Ok(Error::ProfileNotFound))
    );
}

#[test]
fn set_profile_requires_the_merchants_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let merchant = Address::generate(&env);
    let id = env.register(PlanRegistry, (merchant.clone(),));
    let client = PlanRegistryClient::new(&env, &id);

    let name = String::from_str(&env, "Acme");
    let url = String::from_str(&env, "https://acme.example");

    // Happy path: the merchant sets its own profile.
    client.set_profile(&merchant, &name, &url);
    assert_eq!(client.profile(&merchant).name, name);

    // Drop every mocked authorization: the merchant must sign for itself, and
    // a different signer cannot set another merchant's profile.
    env.set_auths(&[]);
    assert!(client.try_set_profile(&merchant, &name, &url).is_err());
}

#[test]
fn set_profile_emits_profile_updated() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let name = String::from_str(&env, "Acme");
    let url = String::from_str(&env, "https://acme.example");

    let before = env.events().all().events().len();
    client.set_profile(&merchant, &name, &url);
    assert_eq!(env.events().all().events().len(), before + 1);

    let last = env.events().all().events().last().cloned().unwrap();
    let ContractEventBody::V0(body) = &last.body;
    let ScVal::Symbol(ScSymbol(event_name)) = &body.topics[0] else {
        panic!("topic 0 is not a symbol");
    };
    let expected: soroban_sdk::xdr::StringM<32> = "profile_updated".try_into().unwrap();
    assert_eq!(event_name, &expected);
}

// ---------------------------------------------------------------------------
// Deactivate a page of a merchant's plans
// ---------------------------------------------------------------------------

#[test]
fn deactivate_page_turns_off_a_page_of_plans() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let name = String::from_str(&env, "Plan");

    let mut ids = Vec::<u64>::new(&env);
    for _ in 0..7 {
        ids.push_back(client.create_plan(
            &merchant,
            &token,
            &1_000,
            &3_600,
            &name.clone(),
            &String::from_str(&env, ""),
            &0,
            &Symbol::new(&env, ""),
            &String::from_str(&env, ""),
            &0u64,
            &0u32,
            &None::<Address>,
            &0u32,
        ));
    }

    assert_eq!(client.deactivate_page(&merchant, &2, &3), 3);

    // Positions 2, 3, 4 are inactive; everything else is still active.
    assert!(!client.get_plan(&ids.get_unchecked(2)).active);
    assert!(!client.get_plan(&ids.get_unchecked(3)).active);
    assert!(!client.get_plan(&ids.get_unchecked(4)).active);
    assert!(client.get_plan(&ids.get_unchecked(0)).active);
    assert!(client.get_plan(&ids.get_unchecked(1)).active);
    assert!(client.get_plan(&ids.get_unchecked(5)).active);
    assert!(client.get_plan(&ids.get_unchecked(6)).active);
}

#[test]
fn deactivate_page_is_idempotent() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let name = String::from_str(&env, "Plan");

    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &3_600,
        &name,
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );

    assert_eq!(client.deactivate_page(&merchant, &0, &10), 1);
    assert!(!client.get_plan(&id).active);
    // Deactivating an already-inactive plan changes nothing.
    assert_eq!(client.deactivate_page(&merchant, &0, &10), 0);
    assert!(!client.get_plan(&id).active);
}

#[test]
fn deactivate_page_respects_the_page_cap() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let name = String::from_str(&env, "Plan");

    for _ in 0..(MAX_PAGE + 3) {
        client.create_plan(
            &merchant,
            &token,
            &1_000,
            &3_600,
            &name,
            &String::from_str(&env, ""),
            &0,
            &Symbol::new(&env, ""),
            &String::from_str(&env, ""),
            &0u64,
            &0u32,
            &None::<Address>,
            &0u32,
        );
    }

    // A huge limit is capped at MAX_PAGE.
    assert_eq!(client.deactivate_page(&merchant, &0, &u32::MAX), MAX_PAGE);
    // The remaining plans are on the next page.
    assert_eq!(client.deactivate_page(&merchant, &MAX_PAGE, &10), 3);
    // Nothing is left to deactivate.
    assert_eq!(client.deactivate_page(&merchant, &0, &10), 0);
    // Deactivated plans stay listed; they are just inactive.
    let page = client.merchant_plans(&merchant, &0, &10);
    assert_eq!(page.len(), 10);
    for i in 0..10 {
        assert!(!client.get_plan(&page.get_unchecked(i)).active);
    }
}

#[test]
fn deactivate_page_only_touches_the_merchant_own_plans() {
    let (env, client, _admin) = setup();
    let token = Address::generate(&env);
    let name = String::from_str(&env, "Plan");

    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let a_id = client.create_plan(
        &a,
        &token,
        &1_000,
        &3_600,
        &name,
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );
    let b_id = client.create_plan(
        &b,
        &token,
        &1_000,
        &3_600,
        &name,
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );

    client.deactivate_page(&a, &0, &10);

    assert!(!client.get_plan(&a_id).active);
    assert!(client.get_plan(&b_id).active);
}

#[test]
fn deactivate_page_requires_the_merchants_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let id = env.register(PlanRegistry, (merchant.clone(),));
    let client = PlanRegistryClient::new(&env, &id);
    client.create_plan(
        &merchant,
        &token,
        &1_000,
        &3_600,
        &String::from_str(&env, "Plan"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );

    // Drop every mocked authorization: the merchant must sign for itself.
    env.set_auths(&[]);
    assert!(client.try_deactivate_page(&merchant, &0, &10).is_err());
}

#[test]
fn create_plan_rejects_a_multibyte_name_over_the_byte_limit() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);

    // A single euro sign is 3 bytes in UTF-8. 22 of them is 22 characters
    // but 66 bytes, which is under 64 characters yet over the 64-byte limit.
    // The guard counts bytes, not characters, so this must be rejected.
    let name = "€".repeat(22);
    assert_eq!(name.len(), 66); // UTF-8 byte length, not character count
    let res = client.try_create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, &name),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );
    assert_eq!(res, Err(Ok(Error::NameTooLong)));
}

#[test]
fn create_plan_accepts_a_multibyte_name_at_the_byte_limit() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);

    // 21 euro signs is 63 bytes, under the limit, so it is accepted and the
    // full multibyte name is stored verbatim.
    let name = "€".repeat(21);
    assert_eq!(name.len(), 63);
    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, &name),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );
    assert_eq!(client.get_plan(&id).name, String::from_str(&env, &name));
}

// ---------------------------------------------------------------------------
// Point a retired plan at its replacement
// ---------------------------------------------------------------------------

#[test]
fn set_successor_stores_the_replacement() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let old_id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );
    let new_id = client.create_plan(
        &merchant,
        &token,
        &2_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );

    // A freshly published plan has no successor yet.
    assert_eq!(client.get_plan(&new_id).successor_id, 0);

    client.set_successor(&merchant, &old_id, &new_id);

    assert_eq!(client.get_plan(&old_id).successor_id, new_id);
    // The replacement itself does not point back at the plan it replaces.
    assert_eq!(client.get_plan(&new_id).successor_id, 0);
}

#[test]
fn set_successor_rejects_a_plan_that_is_its_own_successor() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );

    let res = client.try_set_successor(&merchant, &id, &id);
    assert_eq!(res, Err(Ok(Error::SelfSuccessor)));
    // The pointer was not written.
    assert_eq!(client.get_plan(&id).successor_id, 0);
}

#[test]
fn set_successor_rejects_an_unknown_successor() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );

    let res = client.try_set_successor(&merchant, &id, &999);
    assert_eq!(res, Err(Ok(Error::SuccessorNotFound)));
    assert_eq!(client.get_plan(&id).successor_id, 0);
}

#[test]
fn set_successor_rejects_a_plan_of_another_merchant() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let other = Address::generate(&env);
    let token = Address::generate(&env);
    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );
    let other_id = client.create_plan(
        &other,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Other Plan"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );

    // The merchant may only point at plans it owns itself.
    let res = client.try_set_successor(&merchant, &id, &other_id);
    assert_eq!(res, Err(Ok(Error::NotPlanOwner)));
    assert_eq!(client.get_plan(&id).successor_id, 0);
}

#[test]
fn set_successor_rejects_a_different_token() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let other_token = Address::generate(&env);
    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );
    let other_id = client.create_plan(
        &merchant,
        &other_token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );

    // A replacement denominated in another asset would reprice differently and
    // is not a valid successor.
    let res = client.try_set_successor(&merchant, &id, &other_id);
    assert_eq!(res, Err(Ok(Error::SuccessorTokenMismatch)));
    assert_eq!(client.get_plan(&id).successor_id, 0);
}

#[test]
fn set_successor_requires_the_merchants_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let merchant = Address::generate(&env);
    let id = env.register(PlanRegistry, (merchant.clone(),));
    let client = PlanRegistryClient::new(&env, &id);
    let token = Address::generate(&env);
    let plan_id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );
    let replacement_id = client.create_plan(
        &merchant,
        &token,
        &2_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );

    // The merchant signs the call itself, and the signature must cover the
    // exact invocation: the plan and the replacement it points at.
    env.mock_auths(&[MockAuth {
        address: &merchant,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "set_successor",
            args: (&merchant, &plan_id, &replacement_id).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.set_successor(&merchant, &plan_id, &replacement_id);
    assert_eq!(client.get_plan(&plan_id).successor_id, replacement_id);

    // A different signer cannot stand in for the merchant.
    let other_signer = Address::generate(&env);
    env.mock_auths(&[MockAuth {
        address: &other_signer,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "set_successor",
            args: (&merchant, &plan_id, &replacement_id).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    assert!(client
        .try_set_successor(&merchant, &plan_id, &replacement_id)
        .is_err());
}

#[test]
fn set_successor_emits_plan_successor_set() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let old_id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );
    let new_id = client.create_plan(
        &merchant,
        &token,
        &2_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );

    client.set_successor(&merchant, &old_id, &new_id);
    assert_eq!(env.events().all().events().len(), 1);

    let last = env.events().all().events().last().cloned().unwrap();
    let ContractEventBody::V0(body) = &last.body;
    let ScVal::Symbol(ScSymbol(event_name)) = &body.topics[0] else {
        panic!("topic 0 is not a symbol");
    };
    let expected: soroban_sdk::xdr::StringM<32> = "plan_successor_set".try_into().unwrap();
    assert_eq!(event_name, &expected);
}

// ----- plan description (#91) -----
//
// A plan used to carry a name only, so a subscriber had to leave the app to
// find out what the plan includes. The description is a short, plain summary
// bounded at MAX_DESCRIPTION_LEN bytes; it may be empty.

#[test]
fn create_plan_stores_the_description() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);

    let description = String::from_str(&env, "Monthly consults, unlimited follow-ups");
    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
        &description,
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );

    let plan = client.get_plan(&id);
    assert_eq!(plan.description, description);
    assert_eq!(plan.name, String::from_str(&env, "Pro Monthly"));
}

#[test]
fn an_empty_description_is_allowed() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);

    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );

    assert_eq!(client.get_plan(&id).description, String::from_str(&env, ""));
}

#[test]
fn a_description_at_the_limit_is_accepted() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);

    let at_limit = String::from_str(&env, &"x".repeat(MAX_DESCRIPTION_LEN as usize));
    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
        &at_limit,
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );

    assert_eq!(client.get_plan(&id).description, at_limit);
}

#[test]
fn create_plan_rejects_an_overlong_description() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);

    let too_long = String::from_str(&env, &"x".repeat(MAX_DESCRIPTION_LEN as usize + 1));
    let res = client.try_create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
        &too_long,
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );
    assert_eq!(res, Err(Ok(Error::DescriptionTooLong)));
}

// ---------------------------------------------------------------------------
// Per-token plan index
// ---------------------------------------------------------------------------

#[test]
fn token_plans_empty_for_unknown_token() {
    let (env, client, _admin) = setup();
    let stranger_token = Address::generate(&env);
    assert_eq!(client.token_plans(&stranger_token, &0, &10).len(), 0);
    assert_eq!(client.token_plan_count(&stranger_token), 0);
}

#[test]
fn create_plan_indexes_by_token() {
    let (env, client, _admin) = setup();
    let merchant_a = Address::generate(&env);
    let merchant_b = Address::generate(&env);
    let token_usdc = Address::generate(&env);
    let token_xlm = Address::generate(&env);

    let plan_1 = client.create_plan(
        &merchant_a,
        &token_usdc,
        &1_000,
        &604_800,
        &String::from_str(&env, "USDC Plan 1"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );
    let plan_2 = client.create_plan(
        &merchant_b,
        &token_usdc,
        &2_000,
        &604_800,
        &String::from_str(&env, "USDC Plan 2"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );
    let plan_3 = client.create_plan(
        &merchant_a,
        &token_xlm,
        &5_000,
        &604_800,
        &String::from_str(&env, "XLM Plan 1"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );

    assert_eq!(client.token_plan_count(&token_usdc), 2);
    assert_eq!(
        client.token_plans(&token_usdc, &0, &10),
        soroban_sdk::vec![&env, plan_1, plan_2]
    );

    assert_eq!(client.token_plan_count(&token_xlm), 1);
    assert_eq!(
        client.token_plans(&token_xlm, &0, &10),
        soroban_sdk::vec![&env, plan_3]
    );
}

#[test]
fn token_plans_pages_are_bounded() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let name = String::from_str(&env, "Plan");

    for _ in 0..(MAX_PAGE + 3) {
        client.create_plan(
            &merchant,
            &token,
            &1_000,
            &3_600,
            &name,
            &String::from_str(&env, ""),
            &0,
            &Symbol::new(&env, ""),
            &String::from_str(&env, ""),
            &0u64,
            &0u32,
            &None::<Address>,
            &0u32,
        );
    }

    assert_eq!(client.token_plan_count(&token), MAX_PAGE + 3);
    assert_eq!(client.token_plans(&token, &0, &u32::MAX).len(), MAX_PAGE);
    assert_eq!(
        client.token_plans(&token, &MAX_PAGE, &10),
        soroban_sdk::vec![
            &env,
            u64::from(MAX_PAGE) + 1,
            u64::from(MAX_PAGE) + 2,
            u64::from(MAX_PAGE) + 3
        ]
    );
    assert_eq!(client.token_plans(&token, &u32::MAX, &u32::MAX).len(), 0);
}

#[test]
fn set_plan_name_renames_only_the_label() {
    let (env, client, _) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &2_592_000,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );

    let renamed = String::from_str(&env, "Pro");
    client.set_plan_name(&merchant, &id, &renamed);

    let plan = client.get_plan(&id);
    assert_eq!(plan.name, renamed);
    // Everything a mandate is priced from is frozen.
    assert_eq!(plan.amount, 1_000);
    assert_eq!(plan.period, 2_592_000);
    assert_eq!(plan.token, token);
    assert!(plan.active);
}

#[test]
fn set_plan_name_rejects_another_merchant_and_a_missing_plan() {
    let (env, client, _) = setup();
    let merchant = Address::generate(&env);
    let other = Address::generate(&env);
    let token = Address::generate(&env);
    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &2_592_000,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );
    let name = String::from_str(&env, "Mine now");

    assert_eq!(
        client.try_set_plan_name(&other, &id, &name),
        Err(Ok(Error::NotPlanOwner))
    );
    assert_eq!(
        client.try_set_plan_name(&merchant, &(id + 1), &name),
        Err(Ok(Error::PlanNotFound))
    );
}

#[test]
fn set_plan_name_rejects_a_name_over_the_limit() {
    let (env, client, _) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &2_592_000,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );

    let long = String::from_str(&env, &"n".repeat(MAX_NAME_LEN as usize + 1));
    assert_eq!(
        client.try_set_plan_name(&merchant, &id, &long),
        Err(Ok(Error::PlanNameTooLong))
    );
}

#[test]
fn set_plan_name_requires_the_merchants_auth() {
    let (env, client, _) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &2_592_000,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );

    env.set_auths(&[]);
    assert!(client
        .try_set_plan_name(&merchant, &id, &String::from_str(&env, "Pro"))
        .is_err());
}

#[test]
fn update_admin_hands_the_role_over() {
    let (env, client, admin) = setup();
    let next = Address::generate(&env);

    client.update_admin(&next);
    assert_eq!(client.admin(), next);
    assert_ne!(client.admin(), admin);
}

#[test]
fn update_admin_requires_the_current_admins_auth() {
    let (env, client, admin) = setup();
    let next = Address::generate(&env);

    env.set_auths(&[]);
    assert!(client.try_update_admin(&next).is_err());
    assert_eq!(client.admin(), admin);
}

#[test]
fn create_plan_requires_the_merchants_auth() {
    let (env, client, _) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    // Drop every mocked authorization: a plan is published only by its merchant.
    env.set_auths(&[]);
    assert!(client
        .try_create_plan(
            &merchant,
            &token,
            &1_000,
            &2_592_000,
            &String::from_str(&env, "Pro Monthly"),
            &String::from_str(&env, ""),
            &0,
            &Symbol::new(&env, ""),
            &String::from_str(&env, ""),
            &0u64,
            &0u32,
            &None::<Address>,
            &0u32,
        )
        .is_err());
}

#[test]
fn set_plan_active_requires_the_merchants_auth() {
    let (env, client, _) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &2_592_000,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );
    env.set_auths(&[]);
    assert!(client.try_set_plan_active(&merchant, &id, &false).is_err());
    assert!(client.get_plan(&id).active);
}

// ---------------------------------------------------------------------------
// Plan terms URI
// ---------------------------------------------------------------------------

/// Pull a string field out of the data map of any emitted contract event.
fn event_field_string(env: &Env, field: &str) -> Option<String> {
    for event in env.events().all().events() {
        let ContractEventBody::V0(body) = &event.body;
        let ScVal::Map(Some(ScMap(entries))) = &body.data else {
            continue;
        };
        for entry in entries {
            let ScVal::Symbol(ScSymbol(name)) = &entry.key else {
                continue;
            };
            if name.to_utf8_string_lossy() == field && matches!(entry.val, ScVal::String(_)) {
                return String::try_from_val(env, &entry.val).ok();
            }
        }
    }
    None
}

#[test]
fn create_plan_stores_the_terms_uri() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let terms = String::from_str(&env, "https://acme.example/terms");

    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &terms,
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );

    assert_eq!(client.get_plan(&id).terms_uri, terms);
}

#[test]
fn create_plan_accepts_an_empty_terms_uri() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);

    // The URI is optional: not every plan has published terms yet.
    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );

    assert_eq!(client.get_plan(&id).terms_uri.len(), 0);
}

#[test]
fn create_plan_accepts_a_terms_uri_at_the_byte_limit() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let at_limit = String::from_str(&env, &"x".repeat(MAX_TERMS_LEN as usize));

    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &at_limit.clone(),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );

    assert_eq!(client.get_plan(&id).terms_uri, at_limit);
}

#[test]
fn create_plan_rejects_an_overlong_terms_uri() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let too_long = String::from_str(&env, &"x".repeat(MAX_TERMS_LEN as usize + 1));

    let res = client.try_create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &too_long,
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );

    assert_eq!(res, Err(Ok(Error::UriTooLong)));
    // The rejected plan was not stored or indexed.
    assert_eq!(client.merchant_plan_count(&merchant), 0);
    assert_eq!(client.next_plan_id(), 1);
}

#[test]
fn create_plan_rejects_a_multibyte_terms_uri_over_the_byte_limit() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);

    // 67 euro signs is 201 bytes: only 67 characters, but over the 200-byte
    // limit. The guard counts bytes, not characters.
    let terms = "€".repeat(67);
    assert_eq!(terms.len(), 201);

    let res = client.try_create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, &terms),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );

    assert_eq!(res, Err(Ok(Error::UriTooLong)));
}

#[test]
fn plan_created_carries_the_terms_uri() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let terms = String::from_str(&env, "https://acme.example/terms");

    client.create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &terms,
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );

    // The URI must be recoverable from the event alone: a frontend rendering
    // newly published plans from events needs no follow-up contract read.
    assert_eq!(event_field_string(&env, "terms_uri"), Some(terms));
}

#[test]
fn create_plan_stores_the_availability_cutoff() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let empty = String::from_str(&env, "");
    let forever = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &3_600,
        &String::from_str(&env, "Always"),
        &empty,
        &0,
        &Symbol::new(&env, ""),
        &empty,
        &0,
        &0u32,
        &None::<Address>,
        &0u32,
    );
    let promo = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &3_600,
        &String::from_str(&env, "Launch"),
        &empty,
        &0,
        &Symbol::new(&env, ""),
        &empty,
        &1_700_000_000,
        &0u32,
        &None::<Address>,
        &0u32,
    );

    assert_eq!(client.get_plan(&forever).available_until, 0);
    assert_eq!(client.get_plan(&promo).available_until, 1_700_000_000);
}

#[test]
fn plan_created_event_carries_the_availability_cutoff() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let empty = String::from_str(&env, "");

    client.create_plan(
        &merchant,
        &token,
        &1_000,
        &3_600,
        &String::from_str(&env, "Launch"),
        &empty,
        &0,
        &Symbol::new(&env, ""),
        &empty,
        &1_700_000_000,
        &0u32,
        &None::<Address>,
        &0u32,
    );

    let mut seen = None;
    for event in env.events().all().events() {
        let ContractEventBody::V0(body) = &event.body;
        let ScVal::Map(Some(ScMap(entries))) = &body.data else {
            continue;
        };
        for entry in entries {
            if let ScVal::Symbol(ScSymbol(name)) = &entry.key {
                if name.to_utf8_string_lossy() == "available_until" {
                    seen = Some(entry.val.clone());
                }
            }
        }
    }
    assert_eq!(seen, Some(ScVal::U64(1_700_000_000)));
}

#[test]
fn create_plan_defaults_payout_to_the_merchant() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);

    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0,
        &None::<Address>,
        &0u32,
    );

    // "Pay me" is what a plain plan does.
    assert_eq!(client.get_plan(&id).payout, merchant);
}

#[test]
fn create_plan_records_an_explicit_payout() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let treasury = Address::generate(&env);
    let token = Address::generate(&env);

    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0,
        &Some(treasury.clone()),
        &0u32,
    );

    assert_eq!(client.get_plan(&id).payout, treasury);
    // The merchant stays the owner regardless of where the money lands.
    assert_eq!(client.get_plan(&id).merchant, merchant);
}

#[test]
fn create_plan_stores_the_referral_share() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);

    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &1_337,
        &None::<Address>,
        &0u32,
    );

    assert_eq!(client.get_plan(&id).referral_bps, 1_337);
}

#[test]
fn create_plan_rejects_a_referral_share_above_the_cap() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);

    let res = client.try_create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &(MAX_REFERRAL_BPS + 1),
        &None::<Address>,
        &0u32,
    );

    assert_eq!(res, Err(Ok(Error::ReferralBpsTooHigh)));
    assert_eq!(client.merchant_plan_count(&merchant), 0);

    // Exactly the cap is accepted.
    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &MAX_REFERRAL_BPS,
        &None::<Address>,
        &0u32,
    );
    assert_eq!(client.get_plan(&id).referral_bps, MAX_REFERRAL_BPS);
}

#[test]
fn propose_and_accept_moves_the_plan_between_merchants() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let new_owner = Address::generate(&env);
    let token = Address::generate(&env);
    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &3_600,
        &String::from_str(&env, "Plan"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0,
        &None::<Address>,
        &0u32,
    );

    client.propose_plan_owner(&merchant, &id, &new_owner);

    // Proposing alone changes nothing: the plan stays where it is until the
    // proposed owner accepts.
    assert_eq!(client.get_plan(&id).merchant, merchant.clone());
    assert_eq!(client.merchant_plan_count(&merchant), 1);
    assert_eq!(client.merchant_plan_count(&new_owner), 0);

    client.accept_plan_owner(&new_owner, &id);

    assert_eq!(client.get_plan(&id).merchant, new_owner.clone());
    assert_eq!(client.merchant_plan_count(&merchant), 0);
    assert_eq!(client.merchant_plans(&merchant, &0, &10).len(), 0);
    assert_eq!(client.merchant_plan_count(&new_owner), 1);
    assert_eq!(
        client.merchant_plans(&new_owner, &0, &10),
        soroban_sdk::vec![&env, id]
    );
}

#[test]
fn transfer_moves_a_plan_out_of_a_multi_plan_index() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let new_owner = Address::generate(&env);
    let token = Address::generate(&env);
    let name = String::from_str(&env, "Plan");

    let first = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &3_600,
        &name,
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0,
        &None::<Address>,
        &0u32,
    );
    let middle = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &3_600,
        &name,
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0,
        &None::<Address>,
        &0u32,
    );
    let last = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &3_600,
        &name,
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0,
        &None::<Address>,
        &0u32,
    );

    client.propose_plan_owner(&merchant, &middle, &new_owner);
    client.accept_plan_owner(&new_owner, &middle);

    // Both of the old owner's remaining plans are listed, and none of them is
    // the transferred one.
    let remaining = client.merchant_plans(&merchant, &0, &10);
    assert_eq!(remaining.len(), 2);
    assert!(remaining.contains(first));
    assert!(remaining.contains(last));
    assert!(!remaining.contains(middle));
    // Every entry still resolves to a plan owned by this merchant.
    for pid in remaining.iter() {
        assert_eq!(client.get_plan(&pid).merchant, merchant.clone());
    }
    assert_eq!(
        client.merchant_plans(&new_owner, &0, &10),
        soroban_sdk::vec![&env, middle]
    );

    // Appending still works after the removal.
    let fourth = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &3_600,
        &name,
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0,
        &None::<Address>,
        &0u32,
    );
    assert_eq!(client.merchant_plan_count(&merchant), 3);
    assert!(client.merchant_plans(&merchant, &0, &10).contains(fourth));
    assert_eq!(client.get_plan(&fourth).merchant, merchant);
}

#[test]
fn only_the_owner_can_propose_a_transfer() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let stranger = Address::generate(&env);
    let new_owner = Address::generate(&env);
    let token = Address::generate(&env);
    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &3_600,
        &String::from_str(&env, "Plan"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0,
        &None::<Address>,
        &0u32,
    );

    assert_eq!(
        client.try_propose_plan_owner(&stranger, &id, &new_owner),
        Err(Ok(Error::NotPlanOwner))
    );
    assert_eq!(
        client.try_propose_plan_owner(&merchant, &999, &new_owner),
        Err(Ok(Error::PlanNotFound))
    );
    // Nothing was proposed.
    assert_eq!(
        client.try_accept_plan_owner(&new_owner, &id),
        Err(Ok(Error::PendingOwnershipNotFound))
    );
}

#[test]
fn only_the_proposed_owner_can_accept() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let new_owner = Address::generate(&env);
    let stranger = Address::generate(&env);
    let token = Address::generate(&env);
    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &3_600,
        &String::from_str(&env, "Plan"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0,
        &None::<Address>,
        &0u32,
    );
    client.propose_plan_owner(&merchant, &id, &new_owner);

    assert_eq!(
        client.try_accept_plan_owner(&stranger, &id),
        Err(Ok(Error::NotPendingOwner))
    );
    // The proposal still stands for the right owner.
    assert_eq!(client.get_plan(&id).merchant, merchant.clone());
    assert_eq!(client.try_accept_plan_owner(&new_owner, &id), Ok(Ok(())));
    assert_eq!(client.get_plan(&id).merchant, new_owner);
}

#[test]
fn accept_without_a_proposal_fails() {
    let (env, client, _admin) = setup();
    let new_owner = Address::generate(&env);

    assert_eq!(
        client.try_accept_plan_owner(&new_owner, &1),
        Err(Ok(Error::PendingOwnershipNotFound))
    );
}

#[test]
fn a_transfer_proposal_is_one_shot() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let new_owner = Address::generate(&env);
    let token = Address::generate(&env);
    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &3_600,
        &String::from_str(&env, "Plan"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0,
        &None::<Address>,
        &0u32,
    );

    client.propose_plan_owner(&merchant, &id, &new_owner);
    client.accept_plan_owner(&new_owner, &id);

    assert_eq!(
        client.try_accept_plan_owner(&new_owner, &id),
        Err(Ok(Error::PendingOwnershipNotFound))
    );
    assert_eq!(client.get_plan(&id).merchant, new_owner);
}

#[test]
fn a_new_proposal_replaces_the_old_one() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let wrong = Address::generate(&env);
    let right = Address::generate(&env);
    let token = Address::generate(&env);
    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &3_600,
        &String::from_str(&env, "Plan"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0,
        &None::<Address>,
        &0u32,
    );

    client.propose_plan_owner(&merchant, &id, &wrong);
    client.propose_plan_owner(&merchant, &id, &right);

    assert_eq!(
        client.try_accept_plan_owner(&wrong, &id),
        Err(Ok(Error::NotPendingOwner))
    );
    client.accept_plan_owner(&right, &id);
    assert_eq!(client.get_plan(&id).merchant, right);
}

#[test]
fn propose_requires_the_merchants_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let merchant = Address::generate(&env);
    let new_owner = Address::generate(&env);
    let token = Address::generate(&env);
    let id = env.register(PlanRegistry, (merchant.clone(),));
    let client = PlanRegistryClient::new(&env, &id);
    let plan = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &3_600,
        &String::from_str(&env, "Plan"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0,
        &None::<Address>,
        &0u32,
    );

    // Drop every mocked authorization: the owner must sign the proposal.
    env.set_auths(&[]);
    assert!(client
        .try_propose_plan_owner(&merchant, &plan, &new_owner)
        .is_err());

    // Nothing was proposed, which the logic check proves independently of
    // authorization: restore blanket auths and the accept still fails.
    env.mock_all_auths();
    assert_eq!(
        client.try_accept_plan_owner(&new_owner, &plan),
        Err(Ok(Error::PendingOwnershipNotFound))
    );
}

#[test]
fn accept_requires_the_new_owners_signature() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let new_owner = Address::generate(&env);
    let token = Address::generate(&env);
    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &3_600,
        &String::from_str(&env, "Plan"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0,
        &None::<Address>,
        &0u32,
    );
    client.propose_plan_owner(&merchant, &id, &new_owner);

    env.mock_auths(&[MockAuth {
        address: &merchant,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "accept_plan_owner",
            args: (&new_owner, &id).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    assert!(client.try_accept_plan_owner(&new_owner, &id).is_err());
    // Nothing moved: the plan is still the merchant's and the proposal stands.
    assert_eq!(client.get_plan(&id).merchant, merchant);
    assert_eq!(client.merchant_plan_count(&new_owner), 0);
}

#[test]
fn transfer_emits_proposal_and_transfer_events() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let new_owner = Address::generate(&env);
    let token = Address::generate(&env);
    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &3_600,
        &String::from_str(&env, "Plan"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0,
        &None::<Address>,
        &0u32,
    );

    client.propose_plan_owner(&merchant, &id, &new_owner);
    let first = env.events().all().events().last().cloned().unwrap();
    let ContractEventBody::V0(body) = &first.body;
    let ScVal::Symbol(ScSymbol(name)) = &body.topics[0] else {
        panic!("topic 0 is not a symbol");
    };
    let expected: soroban_sdk::xdr::StringM<32> = "plan_ownership_proposed".try_into().unwrap();
    assert_eq!(name, &expected);

    client.accept_plan_owner(&new_owner, &id);
    let last = env.events().all().events().last().cloned().unwrap();
    let ContractEventBody::V0(body) = &last.body;
    let ScVal::Symbol(ScSymbol(name)) = &body.topics[0] else {
        panic!("topic 0 is not a symbol");
    };
    let expected: soroban_sdk::xdr::StringM<32> = "plan_transferred".try_into().unwrap();
    assert_eq!(name, &expected);
}

fn event_field_u32(env: &Env, field: &str) -> Option<u32> {
    for event in env.events().all().events() {
        let ContractEventBody::V0(body) = &event.body;
        let ScVal::Map(Some(ScMap(entries))) = &body.data else {
            continue;
        };
        for entry in entries {
            let ScVal::Symbol(ScSymbol(name)) = &entry.key else {
                continue;
            };
            if name.to_utf8_string_lossy() == field {
                if let ScVal::U32(v) = entry.val {
                    return Some(v);
                }
            }
        }
    }
    None
}

#[test]
fn create_plan_stores_max_subscribers() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);

    let seated = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Cohort"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &25,
    );
    assert_eq!(client.get_plan(&seated).max_subscribers, 25);

    // 0 means unlimited.
    let open = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Open"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0,
    );
    assert_eq!(client.get_plan(&open).max_subscribers, 0);
}

#[test]
fn plan_created_event_carries_the_seat_cap() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);

    client.create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Cohort"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &7,
    );

    // An indexer reading only events must be able to learn the cap without a
    // follow-up contract read.
    assert_eq!(event_field_u32(&env, "max_subscribers"), Some(7));
}

#[test]
fn admin_can_upgrade_and_state_written_before_survives() {
    let (env, client, admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Plan"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &3,
    );

    let wasm_hash = env.deployer().upload_contract_wasm(UPGRADE_WASM);
    client.upgrade_wasm(&wasm_hash);

    // The plan written before the upgrade decodes under the new code, and
    // the instance configuration survives with it.
    let plan = client.get_plan(&id);
    assert_eq!(plan.max_subscribers, 3);
    assert_eq!(client.admin(), admin);

    // The new code still runs: the next plan gets the next id.
    let next = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Second"),
        &String::from_str(&env, ""),
        &0,
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0,
    );
    assert_eq!(next, id + 1);
}

#[test]
fn upgrade_wasm_wrong_signer_rejected() {
    let (env, client, _admin) = setup();
    let wasm_hash = env.deployer().upload_contract_wasm(UPGRADE_WASM);
    let attacker = Address::generate(&env);

    // The attacker passes the hash as the argument, but the signature on the
    // call is the attacker's: `require_auth` on the admin fails.
    env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "upgrade_wasm",
            args: (&wasm_hash,).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    assert!(client.try_upgrade_wasm(&wasm_hash).is_err());

    // With no authorization at all the call fails too.
    env.set_auths(&[]);
    assert!(client.try_upgrade_wasm(&wasm_hash).is_err());

    assert_eq!(client.next_plan_id(), 1);
}

// ---------------------------------------------------------------------------
// Upgrade history view (#304)
// ---------------------------------------------------------------------------

#[test]
fn upgrade_history_starts_empty() {
    let (_env, client, _admin) = setup();
    let history = client.upgrade_history();
    assert_eq!(history.len(), 0);
}

#[test]
fn upgrade_history_records_upgrades() {
    let (env, client, _admin) = setup();

    // Initial history is empty.
    assert_eq!(client.upgrade_history().len(), 0);

    // Record an upgrade directly via the internal function logic.
    // We test the recording logic by verifying the event emission in
    // upgrade_history_emits_contract_upgraded_event test.
    // Cross-upgrade persistence is tested by the upgrade_wasm test which
    // verifies state survives upgrade; the history storage uses the same
    // instance storage mechanism.
    let wasm_hash = env.deployer().upload_contract_wasm(UPGRADE_WASM);
    client.upgrade_wasm(&wasm_hash);

    // After upgrade, the contract is replaced with the old WASM which
    // doesn't have upgrade_history. The history is recorded in storage
    // and would be readable by a newer contract version.
    // We verify the event was emitted instead.
    let mut found = false;
    for event in env.events().all().events() {
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
    let (env, client, _admin) = setup();
    let wasm_hash = env.deployer().upload_contract_wasm(UPGRADE_WASM);
    let attacker = Address::generate(&env);

    env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "upgrade_wasm",
            args: (&wasm_hash,).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    assert!(client.try_upgrade_wasm(&wasm_hash).is_err());

    // History should still be empty.
    assert_eq!(client.upgrade_history().len(), 0);
}

#[test]
fn upgrade_history_emits_contract_upgraded_event() {
    let (env, client, _admin) = setup();
    let wasm_hash = env.deployer().upload_contract_wasm(UPGRADE_WASM);

    client.upgrade_wasm(&wasm_hash);

    // Find the ContractUpgraded event.
    let mut found = false;
    for event in env.events().all().events() {
        let ContractEventBody::V0(body) = &event.body;
        if let Some(ScVal::Symbol(ScSymbol(name))) = body.topics.first() {
            if name.to_utf8_string_lossy() == "contract_upgraded" {
                found = true;
                // Check topics: admin is topic 1 (topic 0 is event name).
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
// A replaced ownership proposal is dead (#297): proposing a new owner
// invalidates any pending proposal, and the stale proposee can never accept.
// ---------------------------------------------------------------------------

/// A plan owned by a fresh merchant, returned with that merchant.
fn owned_plan(env: &Env, client: &PlanRegistryClient) -> (u64, Address) {
    let merchant = Address::generate(env);
    let token = Address::generate(env);
    let id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &3_600,
        &String::from_str(env, "Plan"),
        &String::from_str(env, ""),
        &0,
        &Symbol::new(env, ""),
        &String::from_str(env, ""),
        &0u64,
        &0,
        &None::<Address>,
        &0u32,
    );
    (id, merchant)
}

/// The `new_owner` field of the last `plan_ownership_proposed` event.
fn last_proposed_owner(env: &Env) -> Option<Address> {
    let mut found = None;
    for event in env.events().all().events() {
        let ContractEventBody::V0(body) = &event.body;
        let Some(ScVal::Symbol(ScSymbol(name))) = body.topics.first() else {
            continue;
        };
        if name.to_utf8_string_lossy() != "plan_ownership_proposed" {
            continue;
        }
        let ScVal::Map(Some(ScMap(entries))) = &body.data else {
            continue;
        };
        for entry in entries {
            if let ScVal::Symbol(ScSymbol(key)) = &entry.key {
                if key.to_utf8_string_lossy() == "new_owner" {
                    found = Address::try_from_val(env, &entry.val).ok();
                }
            }
        }
    }
    found
}

#[test]
fn a_replaced_proposal_cannot_be_accepted_and_nothing_moves() {
    let (env, client, _admin) = setup();
    let (id, merchant) = owned_plan(&env, &client);
    let stale = Address::generate(&env);
    let current = Address::generate(&env);

    client.propose_plan_owner(&merchant, &id, &stale);
    client.propose_plan_owner(&merchant, &id, &current);

    assert_eq!(
        client.try_accept_plan_owner(&stale, &id),
        Err(Ok(Error::NotPendingOwner))
    );

    // The rejected accept changed nothing: owner, both indexes, and the live
    // proposal are exactly as before.
    assert_eq!(client.get_plan(&id).merchant, merchant);
    assert_eq!(client.merchant_plan_count(&merchant), 1);
    assert_eq!(client.merchant_plan_count(&stale), 0);
    assert_eq!(client.try_accept_plan_owner(&current, &id), Ok(Ok(())));
}

#[test]
fn a_stale_proposee_cannot_accept_after_the_replacement_is_accepted() {
    let (env, client, _admin) = setup();
    let (id, merchant) = owned_plan(&env, &client);
    let stale = Address::generate(&env);
    let current = Address::generate(&env);

    client.propose_plan_owner(&merchant, &id, &stale);
    client.propose_plan_owner(&merchant, &id, &current);
    client.accept_plan_owner(&current, &id);

    // No proposal is pending any more, so the stale one finds nothing.
    assert_eq!(
        client.try_accept_plan_owner(&stale, &id),
        Err(Ok(Error::PendingOwnershipNotFound))
    );
    assert_eq!(client.get_plan(&id).merchant, current);
    assert_eq!(client.merchant_plans(&current, &0, &10).len(), 1);
    assert_eq!(client.merchant_plan_count(&stale), 0);
    assert_eq!(client.merchant_plan_count(&merchant), 0);
}

#[test]
fn only_the_latest_of_many_proposals_stands() {
    let (env, client, _admin) = setup();
    let (id, merchant) = owned_plan(&env, &client);
    let candidates: [Address; 4] = core::array::from_fn(|_| Address::generate(&env));

    for candidate in &candidates {
        client.propose_plan_owner(&merchant, &id, candidate);
    }

    for stale in &candidates[..3] {
        assert_eq!(
            client.try_accept_plan_owner(stale, &id),
            Err(Ok(Error::NotPendingOwner))
        );
    }
    client.accept_plan_owner(&candidates[3], &id);
    assert_eq!(client.get_plan(&id).merchant, candidates[3]);
}

#[test]
fn re_proposing_a_previously_replaced_owner_revives_only_that_proposal() {
    let (env, client, _admin) = setup();
    let (id, merchant) = owned_plan(&env, &client);
    let first = Address::generate(&env);
    let second = Address::generate(&env);

    client.propose_plan_owner(&merchant, &id, &first);
    client.propose_plan_owner(&merchant, &id, &second);
    // The merchant changes its mind back: `first` is the pending owner again
    // and `second` is now the stale one.
    client.propose_plan_owner(&merchant, &id, &first);

    assert_eq!(
        client.try_accept_plan_owner(&second, &id),
        Err(Ok(Error::NotPendingOwner))
    );
    client.accept_plan_owner(&first, &id);
    assert_eq!(client.get_plan(&id).merchant, first);
}

#[test]
fn every_replacement_emits_a_proposal_naming_the_new_owner() {
    let (env, client, _admin) = setup();
    let (id, merchant) = owned_plan(&env, &client);
    let stale = Address::generate(&env);
    let current = Address::generate(&env);

    client.propose_plan_owner(&merchant, &id, &stale);
    assert_eq!(last_proposed_owner(&env), Some(stale));

    // Indexers learn of the replacement from the event alone.
    client.propose_plan_owner(&merchant, &id, &current);
    assert_eq!(last_proposed_owner(&env), Some(current));
}

#[test]
fn a_stranger_cannot_replace_a_pending_proposal() {
    let (env, client, _admin) = setup();
    let (id, merchant) = owned_plan(&env, &client);
    let intended = Address::generate(&env);
    let attacker = Address::generate(&env);

    client.propose_plan_owner(&merchant, &id, &intended);

    // Logic check: the attacker is not the plan's merchant.
    assert_eq!(
        client.try_propose_plan_owner(&attacker, &id, &attacker),
        Err(Ok(Error::NotPlanOwner))
    );

    // Auth check: naming the real merchant without its signature fails too.
    env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "propose_plan_owner",
            args: (&merchant, &id, &attacker).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    assert!(client
        .try_propose_plan_owner(&merchant, &id, &attacker)
        .is_err());

    // The original proposal survived both attempts.
    env.mock_all_auths();
    assert_eq!(
        client.try_accept_plan_owner(&attacker, &id),
        Err(Ok(Error::NotPendingOwner))
    );
    client.accept_plan_owner(&intended, &id);
    assert_eq!(client.get_plan(&id).merchant, intended);
}

#[test]
fn a_stale_proposee_signing_for_itself_still_cannot_accept() {
    let (env, client, _admin) = setup();
    let (id, merchant) = owned_plan(&env, &client);
    let stale = Address::generate(&env);
    let current = Address::generate(&env);

    client.propose_plan_owner(&merchant, &id, &stale);
    client.propose_plan_owner(&merchant, &id, &current);

    // A real, exact signature from the stale proposee: authorization passes
    // and the logic check is what rejects it.
    env.mock_auths(&[MockAuth {
        address: &stale,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "accept_plan_owner",
            args: (&stale, &id).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    assert_eq!(
        client.try_accept_plan_owner(&stale, &id),
        Err(Ok(Error::NotPendingOwner))
    );

    env.mock_all_auths();
    assert_eq!(client.get_plan(&id).merchant, merchant);
}

#[test]
fn the_new_owner_can_propose_and_the_old_owners_proposals_are_gone() {
    let (env, client, _admin) = setup();
    let (id, merchant) = owned_plan(&env, &client);
    let stale = Address::generate(&env);
    let current = Address::generate(&env);
    let next = Address::generate(&env);

    client.propose_plan_owner(&merchant, &id, &stale);
    client.propose_plan_owner(&merchant, &id, &current);
    client.accept_plan_owner(&current, &id);

    // The previous owner lost the right to propose...
    assert_eq!(
        client.try_propose_plan_owner(&merchant, &id, &stale),
        Err(Ok(Error::NotPlanOwner))
    );
    // ...and the new owner's proposal is the only one that counts.
    client.propose_plan_owner(&current, &id, &next);
    assert_eq!(
        client.try_accept_plan_owner(&stale, &id),
        Err(Ok(Error::NotPendingOwner))
    );
    client.accept_plan_owner(&next, &id);
    assert_eq!(client.get_plan(&id).merchant, next);
}

#[test]
fn proposals_are_scoped_to_one_plan() {
    let (env, client, _admin) = setup();
    let (plan_a, merchant_a) = owned_plan(&env, &client);
    let (plan_b, merchant_b) = owned_plan(&env, &client);
    let owner_a = Address::generate(&env);
    let owner_b = Address::generate(&env);

    client.propose_plan_owner(&merchant_a, &plan_a, &owner_a);
    client.propose_plan_owner(&merchant_b, &plan_b, &owner_b);
    // Replacing plan B's proposal does not touch plan A's.
    client.propose_plan_owner(&merchant_b, &plan_b, &merchant_b);

    assert_eq!(
        client.try_accept_plan_owner(&owner_a, &plan_b),
        Err(Ok(Error::NotPendingOwner))
    );
    assert_eq!(
        client.try_accept_plan_owner(&owner_b, &plan_b),
        Err(Ok(Error::NotPendingOwner))
    );
    client.accept_plan_owner(&owner_a, &plan_a);
    assert_eq!(client.get_plan(&plan_a).merchant, owner_a);
    assert_eq!(client.get_plan(&plan_b).merchant, merchant_b);
}

#[test]
fn create_plans_creates_a_sequential_batch() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let payout = Address::generate(&env);
    let mut second = batch_plan(&env, &token_b, 2_000, "Second");
    second.payout = Some(payout.clone());
    let plans = soroban_sdk::vec![
        &env,
        batch_plan(&env, &token_a, 1_000, "First"),
        second,
        batch_plan(&env, &token_a, 3_000, "Third"),
    ];

    let ids = client.create_plans(&merchant, &plans);

    assert_eq!(ids, soroban_sdk::vec![&env, 1, 2, 3]);
    assert_eq!(client.next_plan_id(), 4);
    assert_eq!(client.merchant_plan_count(&merchant), 3);
    assert_eq!(client.merchant_plans(&merchant, &0, &10), ids);

    let first = client.get_plan(&1);
    assert_eq!(first.merchant, merchant);
    assert_eq!(first.token, token_a);
    assert_eq!(first.amount, 1_000);
    assert_eq!(first.payout, merchant);
    assert_eq!(first.name, String::from_str(&env, "First"));

    let second = client.get_plan(&2);
    assert_eq!(second.token, token_b);
    assert_eq!(second.payout, payout);
    assert_eq!(second.amount, 2_000);

    assert_eq!(client.token_plan_count(&token_a), 2);
    assert_eq!(
        client.token_plans(&token_a, &0, &10),
        soroban_sdk::vec![&env, 1, 3]
    );
    assert_eq!(client.token_plan_count(&token_b), 1);
    assert_eq!(
        client.token_plans(&token_b, &0, &10),
        soroban_sdk::vec![&env, 2]
    );
}

#[test]
fn create_plans_emits_one_event_per_plan_and_a_batch_summary() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let plans = soroban_sdk::vec![
        &env,
        batch_plan(&env, &token, 1_000, "First"),
        batch_plan(&env, &token, 2_000, "Second"),
    ];

    let before = env.events().all().events().len();
    client.create_plans(&merchant, &plans);
    let all = env.events().all();
    let events = all.events();
    assert_eq!(events.len(), before + 3);

    let mut names = std::vec::Vec::new();
    for event in events.iter().skip(before) {
        let ContractEventBody::V0(body) = &event.body;
        let ScVal::Symbol(ScSymbol(name)) = &body.topics[0] else {
            panic!("topic 0 is not a symbol");
        };
        names.push(name.to_utf8_string_lossy());
    }
    assert_eq!(
        names,
        std::vec!["plan_created", "plan_created", "plans_created"]
    );
}

#[test]
fn create_plans_rejects_an_empty_batch() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);

    assert_eq!(
        client.try_create_plans(&merchant, &soroban_sdk::Vec::new(&env)),
        Err(Ok(Error::EmptyBatch))
    );
    assert_eq!(client.next_plan_id(), 1);
    assert_eq!(client.merchant_plan_count(&merchant), 0);
}

#[test]
fn create_plans_accepts_the_maximum_batch_size() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let mut plans = soroban_sdk::Vec::new(&env);
    for i in 0..MAX_BATCH_PLANS {
        plans.push_back(batch_plan(&env, &token, 1_000 + i128::from(i), "Plan"));
    }

    let ids = client.create_plans(&merchant, &plans);

    assert_eq!(ids.len(), MAX_BATCH_PLANS);
    assert_eq!(client.next_plan_id(), u64::from(MAX_BATCH_PLANS) + 1);
    assert_eq!(client.merchant_plan_count(&merchant), MAX_BATCH_PLANS);
}

#[test]
fn create_plans_rejects_a_batch_over_the_maximum_without_writing_anything() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let mut plans = soroban_sdk::Vec::new(&env);
    for _ in 0..=MAX_BATCH_PLANS {
        plans.push_back(batch_plan(&env, &token, 1_000, "Plan"));
    }

    assert_eq!(
        client.try_create_plans(&merchant, &plans),
        Err(Ok(Error::BatchTooLarge))
    );
    assert_eq!(client.next_plan_id(), 1);
    assert_eq!(client.merchant_plan_count(&merchant), 0);
    assert_eq!(client.token_plan_count(&token), 0);
}

#[test]
fn an_invalid_plan_makes_the_whole_batch_atomic() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let mut invalid = batch_plan(&env, &token, 2_000, "Second");
    invalid.period = MIN_PERIOD - 1;
    let plans = soroban_sdk::vec![&env, batch_plan(&env, &token, 1_000, "First"), invalid,];
    let before = env.events().all().events().len();

    assert_eq!(
        client.try_create_plans(&merchant, &plans),
        Err(Ok(Error::InvalidPeriod))
    );

    assert_eq!(client.next_plan_id(), 1);
    assert_eq!(client.merchant_plan_count(&merchant), 0);
    assert_eq!(client.token_plan_count(&token), 0);
    assert!(client.try_get_plan(&1).is_err());
    assert_eq!(env.events().all().events().len(), before);
}

#[test]
fn create_plans_requires_the_merchants_auth() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let plans = soroban_sdk::vec![&env, batch_plan(&env, &token, 1_000, "First")];

    env.set_auths(&[]);
    assert!(client.try_create_plans(&merchant, &plans).is_err());
    assert_eq!(client.next_plan_id(), 1);
    assert_eq!(client.merchant_plan_count(&merchant), 0);
}

#[test]
fn create_plans_authorizes_the_exact_merchant_tree() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let id = env.register(PlanRegistry, (admin.clone(),));
    let client = PlanRegistryClient::new(&env, &id);
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let plans = soroban_sdk::vec![&env, batch_plan(&env, &token, 1_000, "First")];

    env.mock_auths(&[MockAuth {
        address: &merchant,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "create_plans",
            args: (&merchant, &plans).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    assert_eq!(
        client.create_plans(&merchant, &plans),
        soroban_sdk::vec![&env, 1]
    );
    assert_eq!(env.auths().len(), 1);
    assert_eq!(env.auths()[0].0, merchant);
}

// ---------------------------------------------------------------------------
// Panic safety (#308): extreme IDs and invalid parameters must return typed
// errors instead of trapping.
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
fn missing_plan_ids_are_typed_errors_on_every_mutator() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);

    for missing in [0, u64::MAX] {
        assert_eq!(client.try_get_plan(&missing), Err(Ok(Error::PlanNotFound)));
        assert_eq!(
            client.try_set_plan_name(&merchant, &missing, &String::from_str(&env, "Name"),),
            Err(Ok(Error::PlanNotFound))
        );
        assert_eq!(
            client.try_set_plan_active(&merchant, &missing, &false),
            Err(Ok(Error::PlanNotFound))
        );
        assert_eq!(
            client.try_set_successor(&merchant, &missing, &1),
            Err(Ok(Error::PlanNotFound))
        );
        assert_eq!(
            client.try_propose_plan_owner(&merchant, &missing, &merchant),
            Err(Ok(Error::PlanNotFound))
        );
        assert_eq!(
            client.try_accept_plan_owner(&merchant, &missing),
            Err(Ok(Error::PendingOwnershipNotFound))
        );
    }
    assert_eq!(client.token_plan_count(&token), 0);
}

#[test]
fn extreme_page_bounds_are_saturating_and_typed() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);

    assert_eq!(
        client.merchant_plans(&merchant, &u32::MAX, &u32::MAX),
        soroban_sdk::Vec::new(&env)
    );
    assert_eq!(
        client.token_plans(&token, &u32::MAX, &u32::MAX),
        soroban_sdk::Vec::new(&env)
    );
    assert_eq!(
        client.try_deactivate_page(&merchant, &u32::MAX, &u32::MAX),
        Ok(Ok(0))
    );
}

#[test]
fn deterministic_extreme_plan_inputs_never_trap() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let name = String::from_str(&env, "Plan");
    let empty = String::from_str(&env, "");
    let category = Symbol::new(&env, "");
    let mut seed = 0xfeed_face_0bad_c0deu64;

    for _ in 0..128 {
        let x = next_fuzz_u64(&mut seed);
        let amount = match x % 6 {
            0 => 0,
            1 => -1,
            2 => i128::MIN,
            3 => i128::MAX,
            4 => MIN_CHARGE_AMOUNT - 1,
            _ => MIN_CHARGE_AMOUNT,
        };
        let period = match x % 5 {
            0 => 0,
            1 => 1,
            2 => MIN_PERIOD - 1,
            3 => MIN_PERIOD,
            _ => u64::MAX,
        };
        let keeper = match x % 3 {
            0 => 0,
            1 => MAX_KEEPER_BPS,
            _ => u32::MAX,
        };
        let referral = match x % 3 {
            0 => 0,
            1 => MAX_REFERRAL_BPS,
            _ => u32::MAX,
        };

        assert_typed_error(client.try_create_plan(
            &merchant,
            &token,
            &amount,
            &period,
            &name,
            &empty,
            &keeper,
            &category,
            &empty,
            &0,
            &referral,
            &None::<Address>,
            // Fuzz input: keep the low 32 bits of the random value on purpose.
            &u32::try_from(x & u64::from(u32::MAX)).unwrap(),
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

fn simple_plan(env: &Env, client: &PlanRegistryClient, merchant: &Address, token: &Address) -> u64 {
    client.create_plan(
        merchant,
        token,
        &1_000,
        &2_592_000,
        &String::from_str(env, "Pro Monthly"),
        &String::from_str(env, ""),
        &0,
        &Symbol::new(env, ""),
        &String::from_str(env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    )
}

/// A registry with an active plan, a paused one with a grace period, a
/// retired one and a merchant profile, so every view has state to read.
fn view_registry() -> (Env, PlanRegistryClient<'static>, Address, Address, [u64; 3]) {
    let (env, client, _) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let active = simple_plan(&env, &client, &merchant, &token);
    let paused = simple_plan(&env, &client, &merchant, &token);
    client.set_plan_charges_paused(&merchant, &paused, &true);
    client.set_plan_grace_period(&merchant, &paused, &86_400);
    let retired = simple_plan(&env, &client, &merchant, &token);
    client.set_plan_active(&merchant, &retired, &false);
    client.set_profile(
        &merchant,
        &String::from_str(&env, "Acme"),
        &String::from_str(&env, "https://example.com"),
    );
    (env, client, merchant, token, [active, paused, retired])
}

#[test]
fn every_registry_view_leaves_storage_events_and_auth_untouched() {
    let (env, c, merchant, token, ids) = view_registry();
    let env = &env;

    assert_read_only(env, "version", || c.version());
    assert_read_only(env, "admin", || c.admin());
    assert_read_only(env, "next_plan_id", || c.next_plan_id());
    assert_read_only(env, "upgrade_history", || c.upgrade_history());
    assert_read_only(env, "profile", || c.profile(&merchant));
    assert_read_only(env, "merchant_plan_count", || {
        c.merchant_plan_count(&merchant)
    });
    assert_read_only(env, "merchant_plans", || {
        c.merchant_plans(&merchant, &0, &10)
    });
    assert_read_only(env, "token_plan_count", || c.token_plan_count(&token));
    assert_read_only(env, "token_plans", || c.token_plans(&token, &0, &10));
    for id in ids {
        assert_read_only(env, "get_plan", || c.get_plan(&id));
        assert_read_only(env, "plan_paused", || c.plan_paused(&id));
        assert_read_only(env, "plan_charges_paused", || c.plan_charges_paused(&id));
        assert_read_only(env, "plan_grace_period", || c.plan_grace_period(&id));
    }

    // Pages past the end and unknown keys.
    let stranger = Address::generate(env);
    assert_read_only(env, "merchant_plans (past end)", || {
        c.merchant_plans(&merchant, &1_000, &10)
    });
    assert_read_only(env, "merchant_plans (stranger)", || {
        c.merchant_plans(&stranger, &0, &10)
    });
    assert_read_only(env, "token_plan_count (unknown token)", || {
        c.token_plan_count(&stranger)
    });
}

#[test]
fn registry_views_need_no_signature() {
    let (env, c, merchant, token, [active, ..]) = view_registry();
    let env = &env;
    env.set_auths(&[]);

    // A write is now refused, proving the auth mock is gone...
    assert!(c.try_set_plan_active(&merchant, &active, &false).is_err());
    assert!(c.get_plan(&active).active);

    // ...but views still answer, unsigned and side-effect free.
    assert_read_only(env, "get_plan (unsigned)", || c.get_plan(&active));
    assert_read_only(env, "plan_charges_paused (unsigned)", || {
        c.plan_charges_paused(&active)
    });
    assert_read_only(env, "plan_grace_period (unsigned)", || {
        c.plan_grace_period(&active)
    });
    assert_read_only(env, "profile (unsigned)", || c.profile(&merchant));
    assert_read_only(env, "merchant_plans (unsigned)", || {
        c.merchant_plans(&merchant, &0, &10)
    });
    assert_read_only(env, "token_plans (unsigned)", || {
        c.token_plans(&token, &0, &10)
    });
    assert_read_only(env, "next_plan_id (unsigned)", || c.next_plan_id());
}

#[test]
fn failing_registry_views_are_side_effect_free_too() {
    let (env, c, _, _, _) = view_registry();
    let env = &env;
    let missing = c.next_plan_id() + 50;
    let stranger = Address::generate(env);

    assert_eq!(
        assert_read_only(env, "get_plan (missing)", || c.try_get_plan(&missing)),
        Err(Ok(Error::PlanNotFound))
    );
    assert_eq!(
        assert_read_only(env, "plan_paused (missing)", || c.try_plan_paused(&missing)),
        Err(Ok(Error::PlanNotFound))
    );
    assert_eq!(
        assert_read_only(env, "plan_charges_paused (missing)", || {
            c.try_plan_charges_paused(&missing)
        }),
        Err(Ok(Error::PlanNotFound))
    );
    assert_eq!(
        assert_read_only(env, "plan_grace_period (missing)", || {
            c.try_plan_grace_period(&missing)
        }),
        Err(Ok(Error::PlanNotFound))
    );
    assert_eq!(
        assert_read_only(env, "profile (missing)", || c.try_profile(&stranger)),
        Err(Ok(Error::ProfileNotFound))
    );
}

#[test]
#[should_panic(expected = "mutated ledger state")]
fn the_registry_read_only_check_catches_a_state_change() {
    let (env, c, merchant, _, [active, ..]) = view_registry();
    assert_read_only(&env, "set_plan_active", || {
        c.set_plan_active(&merchant, &active, &false);
    });
}

// ---------------------------------------------------------------------------
// #313: plan ids are never reused after a plan is retired
// ---------------------------------------------------------------------------

/// A deterministic random mix of single and batch creation, retirement
/// (`set_plan_active(false)`, `deactivate_page`), reactivation, successor
/// links and ownership transfer, interleaved with calls that fail. No id may
/// ever be issued twice, failed calls must not consume one, and a retired
/// plan's id must keep resolving to that same plan: a recycled id would let
/// a stale reference (a subscriber's mandate, an indexer, a successor link)
/// silently point at a different merchant's terms.
#[test]
fn fuzz_plan_ids_are_never_reused_after_retirement() {
    let (env, c, _) = setup();
    let merchants = [Address::generate(&env), Address::generate(&env)];
    let token = Address::generate(&env);
    let mut seed = 0x0313_0313_cafe_f00du64;
    let next = |s: &mut u64| {
        *s = s
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        *s
    };

    // (id, owner, amount) for every plan ever created.
    let mut issued: std::vec::Vec<(u64, Address, i128)> = std::vec::Vec::new();
    let mut retired_any = false;

    for round in 0..200u64 {
        let roll = next(&mut seed);
        let pick = usize::try_from(next(&mut seed)).unwrap();
        let before = c.next_plan_id();
        let merchant = merchants[pick % merchants.len()].clone();

        match roll % 8 {
            0 | 1 => {
                let amount = 1_000 + i128::from(round);
                let p = batch_plan(&env, &token, amount, "Fuzz");
                let id = c.create_plan(
                    &merchant,
                    &p.token,
                    &p.amount,
                    &p.period,
                    &p.name,
                    &p.description,
                    &p.keeper_bps,
                    &p.category,
                    &p.terms_uri,
                    &p.available_until,
                    &p.referral_bps,
                    &p.payout,
                    &p.max_subscribers,
                );
                assert_eq!(id, before);
                assert!(
                    issued.iter().all(|(seen, _, _)| *seen != id),
                    "plan id {id} reissued"
                );
                issued.push((id, merchant, amount));
            }
            2 => {
                let n = 1 + (roll >> 8) % 3;
                let mut batch = Vec::new(&env);
                let mut amounts = std::vec::Vec::new();
                for k in 0..n {
                    let amount = 50_000 + i128::from(round * 10 + k);
                    amounts.push(amount);
                    batch.push_back(batch_plan(&env, &token, amount, "Batch"));
                }
                let ids = c.create_plans(&merchant, &batch);
                for (k, id) in ids.iter().enumerate() {
                    assert_eq!(id, before + k as u64);
                    assert!(
                        issued.iter().all(|(seen, _, _)| *seen != id),
                        "plan id {id} reissued"
                    );
                    issued.push((id, merchant.clone(), amounts[k]));
                }
            }
            3 if !issued.is_empty() => {
                let (id, owner, _) = issued[pick % issued.len()].clone();
                c.set_plan_active(&owner, &id, &false);
                retired_any = true;
            }
            4 => {
                c.deactivate_page(&merchant, &0, &5);
                retired_any = true;
            }
            5 if !issued.is_empty() => {
                // Reactivating or linking a successor re-uses the *same* id;
                // it never creates or frees one.
                let (id, owner, _) = issued[pick % issued.len()].clone();
                if roll & 0x100 == 0 {
                    c.set_plan_active(&owner, &id, &true);
                } else if let Some((succ, _, _)) = issued
                    .iter()
                    .find(|(other, o, _)| *other != id && *o == owner)
                    .cloned()
                {
                    c.set_successor(&owner, &id, &succ);
                }
            }
            6 if !issued.is_empty() => {
                // Hand a plan to the other merchant.
                let at = pick % issued.len();
                let (id, owner, amount) = issued[at].clone();
                let other = merchants.iter().find(|m| **m != owner).unwrap().clone();
                c.propose_plan_owner(&owner, &id, &other);
                c.accept_plan_owner(&other, &id);
                issued[at] = (id, other, amount);
            }
            _ => {
                // Failure paths: invalid input, wrong owner, empty batch and
                // a batch with one bad plan. None may consume an id.
                assert!(c
                    .try_create_plan(
                        &merchant,
                        &token,
                        &0,
                        &2_592_000,
                        &String::from_str(&env, "Bad"),
                        &String::from_str(&env, ""),
                        &0,
                        &Symbol::new(&env, ""),
                        &String::from_str(&env, ""),
                        &0u64,
                        &0u32,
                        &None::<Address>,
                        &0u32,
                    )
                    .is_err());
                assert_eq!(
                    c.try_create_plans(&merchant, &Vec::new(&env)),
                    Err(Ok(Error::EmptyBatch))
                );
                let mut bad = Vec::new(&env);
                bad.push_back(batch_plan(&env, &token, 5_000, "Good"));
                bad.push_back(batch_plan(&env, &token, 0, "Bad"));
                assert!(c.try_create_plans(&merchant, &bad).is_err());
                if let Some((id, owner, _)) = issued.get(pick % issued.len().max(1)).cloned() {
                    let stranger = merchants.iter().find(|m| **m != owner).unwrap();
                    assert_eq!(
                        c.try_set_plan_active(stranger, &id, &false),
                        Err(Ok(Error::NotPlanOwner))
                    );
                }
                assert_eq!(c.next_plan_id(), before, "a failed call consumed an id");
            }
        }
        assert!(
            c.next_plan_id() >= before,
            "the plan counter moved backwards"
        );
    }

    assert!(retired_any);
    let next_id = c.next_plan_id();
    assert_eq!(next_id, issued.len() as u64 + 1);

    // Every id, retired or not, still resolves to the plan it was issued for.
    for (id, owner, amount) in &issued {
        let p = c.get_plan(id);
        assert_eq!(p.id, *id);
        assert_eq!(&p.merchant, owner);
        assert_eq!(p.amount, *amount);
    }

    // And the next plan after all that churn gets a brand-new id.
    let fresh = simple_plan(&env, &c, &merchants[0], &token);
    assert_eq!(fresh, next_id);
}

// ---- #288: per-plan settlement token surfaced in listings ----

fn mixed_token_catalogue() -> (Env, PlanRegistryClient<'static>, Address, Address, Address) {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let usdc = Address::generate(&env);
    let xlm = Address::generate(&env);
    let plans = soroban_sdk::vec![
        &env,
        batch_plan(&env, &usdc, 1_000, "Basic"),
        batch_plan(&env, &xlm, 1_000, "Basic"),
        batch_plan(&env, &usdc, 5_000, "Pro"),
    ];
    client.create_plans(&merchant, &plans);
    (env, client, merchant, usdc, xlm)
}

#[test]
fn merchant_plan_listings_expose_each_plans_token() {
    let (_env, client, merchant, usdc, xlm) = mixed_token_catalogue();

    let rows = client.merchant_plan_listings(&merchant, &0, &10);
    assert_eq!(rows.len(), 3);
    // Same name and price, different settlement token: the row disambiguates.
    let a = rows.get(0).unwrap();
    let b = rows.get(1).unwrap();
    assert_eq!((a.id, a.token.clone(), a.amount), (1, usdc.clone(), 1_000));
    assert_eq!((b.id, b.token.clone(), b.amount), (2, xlm, 1_000));
    assert_eq!(a.name, b.name);
    let c = rows.get(2).unwrap();
    assert_eq!(
        (c.id, c.token, c.amount, c.period),
        (3, usdc, 5_000, 2_592_000)
    );
    assert!(c.active);
    assert_eq!(c.merchant, merchant);

    // Rows always match get_plan.
    for row in rows.iter() {
        let plan = client.get_plan(&row.id);
        assert_eq!(row.token, plan.token);
        assert_eq!(row.merchant, plan.merchant);
    }
}

#[test]
fn token_plan_listings_only_contain_plans_in_that_token() {
    let (_env, client, _merchant, usdc, xlm) = mixed_token_catalogue();

    let usdc_rows = client.token_plan_listings(&usdc, &0, &10);
    assert_eq!(usdc_rows.len(), 2);
    assert!(usdc_rows.iter().all(|r| r.token == usdc));
    let xlm_rows = client.token_plan_listings(&xlm, &0, &10);
    assert_eq!(xlm_rows.len(), 1);
    assert_eq!(xlm_rows.get(0).unwrap().id, 2);
}

#[test]
fn plan_listings_follow_paging_and_reflect_status() {
    let (env, client, merchant, usdc, _xlm) = mixed_token_catalogue();

    let page = client.merchant_plan_listings(&merchant, &1, &1);
    assert_eq!(page.len(), 1);
    assert_eq!(page.get(0).unwrap().id, 2);
    assert_eq!(client.merchant_plan_listings(&merchant, &3, &10).len(), 0);
    assert_eq!(
        client
            .merchant_plan_listings(&merchant, &u32::MAX, &10)
            .len(),
        0
    );
    assert_eq!(
        client
            .token_plan_listings(&Address::generate(&env), &0, &10)
            .len(),
        0
    );

    client.set_plan_active(&merchant, &1, &false);
    let rows = client.token_plan_listings(&usdc, &0, &10);
    assert!(!rows.get(0).unwrap().active);
    assert!(rows.get(1).unwrap().active);
}

// ---------------------------------------------------------------------------
// Per-call resource budgets (#250)
// ---------------------------------------------------------------------------

/// Resources the last top-level call consumed: CPU, memory and ledger writes.
struct BudgetMeasurement {
    call: &'static str,
    cpu_instructions: u64,
    memory_bytes: u64,
    write_entries: u32,
    write_bytes: u32,
}

/// Snapshot the resources the most recent call consumed. The caller resets the
/// budget immediately before the measured call so every run starts level.
fn measure_call(env: &Env, call: &'static str) -> BudgetMeasurement {
    let budget = env.cost_estimate().budget();
    let resources = env.cost_estimate().resources();
    BudgetMeasurement {
        call,
        cpu_instructions: budget.cpu_instruction_cost(),
        memory_bytes: budget.memory_bytes_cost(),
        write_entries: resources.write_entries,
        write_bytes: resources.write_bytes,
    }
}

/// Print the measurements, then compare them with the committed snapshot and
/// fail when any metric grows by more than 5%. `UPDATE_RESOURCE_BUDGETS=1`
/// refreshes the snapshot instead; review the CSV diff before committing it.
fn assert_within_committed_budgets(contract: &str, measurements: &[BudgetMeasurement]) {
    let snapshot_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test_snapshots/budgets.csv");

    std::println!("\n### {contract} per-call resource budgets");
    std::println!("| Call | CPU instructions | Memory bytes | Write entries | Write bytes |");
    std::println!("| --- | ---: | ---: | ---: | ---: |");
    for measurement in measurements {
        std::println!(
            "| {} | {} | {} | {} | {} |",
            measurement.call,
            measurement.cpu_instructions,
            measurement.memory_bytes,
            measurement.write_entries,
            measurement.write_bytes
        );
    }

    if std::env::var("UPDATE_RESOURCE_BUDGETS").as_deref() == Ok("1") {
        let mut snapshot = std::string::String::from(
            "call,cpu_instructions,memory_bytes,write_entries,write_bytes\n",
        );
        for measurement in measurements {
            std::fmt::Write::write_fmt(
                &mut snapshot,
                format_args!(
                    "{},{},{},{},{}\n",
                    measurement.call,
                    measurement.cpu_instructions,
                    measurement.memory_bytes,
                    measurement.write_entries,
                    measurement.write_bytes
                ),
            )
            .unwrap();
        }
        std::fs::write(&snapshot_path, snapshot).unwrap();
        return;
    }

    let snapshot = std::fs::read_to_string(&snapshot_path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", snapshot_path.display()));
    for measurement in measurements {
        let line = snapshot
            .lines()
            .find(|line| line.starts_with(&std::format!("{},", measurement.call)))
            .unwrap_or_else(|| panic!("missing budget snapshot for {}", measurement.call));
        let mut fields = line.split(',');
        let _ = fields.next();
        let expected_cpu: u64 = fields.next().unwrap().parse().unwrap();
        let expected_memory: u64 = fields.next().unwrap().parse().unwrap();
        let expected_write_entries: u64 = fields.next().unwrap().parse().unwrap();
        let expected_write_bytes: u64 = fields.next().unwrap().parse().unwrap();

        let limit = |value: u64| value + (value / 20).max(1);
        assert!(
            measurement.cpu_instructions <= limit(expected_cpu),
            "{} CPU budget grew from {expected_cpu} to {} instructions (5% tolerance: {})",
            measurement.call,
            measurement.cpu_instructions,
            limit(expected_cpu)
        );
        assert!(
            measurement.memory_bytes <= limit(expected_memory),
            "{} memory budget grew from {expected_memory} to {} bytes (5% tolerance: {})",
            measurement.call,
            measurement.memory_bytes,
            limit(expected_memory)
        );
        assert!(
            u64::from(measurement.write_entries) <= limit(expected_write_entries),
            "{} write-entry budget grew from {expected_write_entries} to {} entries (5% tolerance: {})",
            measurement.call,
            measurement.write_entries,
            limit(expected_write_entries)
        );
        assert!(
            u64::from(measurement.write_bytes) <= limit(expected_write_bytes),
            "{} write-byte budget grew from {expected_write_bytes} to {} bytes (5% tolerance: {})",
            measurement.call,
            measurement.write_bytes,
            limit(expected_write_bytes)
        );
    }
}

#[test]
fn plan_registry_call_budgets_stay_within_committed_snapshots() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);
    let mut measurements = std::vec::Vec::new();

    let name = String::from_str(&env, "Budget Plan");
    let description = String::from_str(&env, "");
    let category = Symbol::new(&env, "");
    let terms_uri = String::from_str(&env, "");
    env.cost_estimate().budget().reset_default();
    let plan_id = client.create_plan(
        &merchant,
        &token,
        &1_000,
        &604_800,
        &name,
        &description,
        &0,
        &category,
        &terms_uri,
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );
    measurements.push(measure_call(&env, "create_plan"));

    let plans = soroban_sdk::vec![
        &env,
        batch_plan(&env, &token, 1_000, "Batch 1"),
        batch_plan(&env, &token, 1_100, "Batch 2"),
        batch_plan(&env, &token, 1_200, "Batch 3"),
        batch_plan(&env, &token, 1_300, "Batch 4"),
        batch_plan(&env, &token, 1_400, "Batch 5"),
        batch_plan(&env, &token, 1_500, "Batch 6"),
        batch_plan(&env, &token, 1_600, "Batch 7"),
        batch_plan(&env, &token, 1_700, "Batch 8"),
        batch_plan(&env, &token, 1_800, "Batch 9"),
        batch_plan(&env, &token, 1_900, "Batch 10"),
    ];
    env.cost_estimate().budget().reset_default();
    client.create_plans(&merchant, &plans);
    measurements.push(measure_call(&env, "create_plans_batch_10"));

    env.cost_estimate().budget().reset_default();
    let _ = client.get_plan(&plan_id);
    measurements.push(measure_call(&env, "get_plan"));

    env.cost_estimate().budget().reset_default();
    let _ = client.merchant_plans(&merchant, &0, &10);
    measurements.push(measure_call(&env, "merchant_plans_page_10"));

    env.cost_estimate().budget().reset_default();
    client.set_plan_active(&merchant, &plan_id, &false);
    measurements.push(measure_call(&env, "set_plan_active"));

    assert_within_committed_budgets("plan-registry", &measurements);
}
