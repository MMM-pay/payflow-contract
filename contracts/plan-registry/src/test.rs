#![cfg(test)]
extern crate std;

use super::*;
use soroban_sdk::xdr::{ContractEventBody, ScSymbol, ScVal};
use soroban_sdk::{
    testutils::{Address as _, Events as _, MockAuth, MockAuthInvoke},
    Address, Env, IntoVal, String, Vec,
};

fn setup() -> (Env, PlanRegistryClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register(PlanRegistry, (admin.clone(),));
    let client = PlanRegistryClient::new(&env, &id);
    (env, client, admin)
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
    );
    let b = client.create_plan(
        &merchant,
        &token,
        &2_000,
        &2_592_000,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
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
            )
                .into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.create_plan(&merchant, &token, &1_000, &604_800, &name, &desc, &0);
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
                        &0_u32
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
            )
                .into_val(&env),
            sub_invokes: &[],
        },
    }]);
    assert!(client
        .try_create_plan(&merchant, &token, &1_000, &604_800, &name, &desc, &0)
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
        &String::from_str(&env, ""),
        &0,
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
    );
    let b_id = client.create_plan(
        &b,
        &token,
        &1_000,
        &3_600,
        &name,
        &String::from_str(&env, ""),
        &0,
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
    );
    let new_id = client.create_plan(
        &merchant,
        &token,
        &2_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
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
    );
    let other_id = client.create_plan(
        &other,
        &token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Other Plan"),
        &String::from_str(&env, ""),
        &0,
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
    );
    let other_id = client.create_plan(
        &merchant,
        &other_token,
        &1_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
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
    );
    let replacement_id = client.create_plan(
        &merchant,
        &token,
        &2_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
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
    );
    let new_id = client.create_plan(
        &merchant,
        &token,
        &2_000,
        &604_800,
        &String::from_str(&env, "Pro Monthly"),
        &String::from_str(&env, ""),
        &0,
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
    );
    let plan_2 = client.create_plan(
        &merchant_b,
        &token_usdc,
        &2_000,
        &604_800,
        &String::from_str(&env, "USDC Plan 2"),
        &String::from_str(&env, ""),
        &0,
    );
    let plan_3 = client.create_plan(
        &merchant_a,
        &token_xlm,
        &5_000,
        &604_800,
        &String::from_str(&env, "XLM Plan 1"),
        &String::from_str(&env, ""),
        &0,
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
