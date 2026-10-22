#![cfg(test)]

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
        &0,
        &0,
        &None,
    );
    let b = client.create_plan(
        &merchant,
        &token,
        &2_000,
        &2_592_000,
        &String::from_str(&env, "Pro Monthly"),
        &0,
        &0,
        &None,
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
        &0,
        &0,
        &None,
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
        &MAX_KEEPER_BPS,
        &0,
        &None,
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
        &(MAX_KEEPER_BPS + 1),
        &0,
        &None,
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
        &0,
        &0,
        &None,
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
        &0,
        &0,
        &None,
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
        &0,
        &0,
        &None,
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
        &0,
        &0,
        &None,
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
        &0,
        &0,
        &None,
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
        &0,
        &0,
        &None,
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
        &0,
        &0,
        &None,
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
        &0,
        &0,
        &None,
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
        &0,
        &0,
        &None,
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
        client.create_plan(&merchant, &token, &1_000, &3_600, &name, &0, &0, &None);
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
            &0,
            &0,
            &None,
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

    let id = client.create_plan(&merchant, &token, &1_000, &3_600, &name, &0, &0, &None);

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
        client.create_plan(&merchant, &token, &1_000, &3_600, &name, &0, &0, &None);
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
    let a_id = client.create_plan(&a, &token, &1_000, &3_600, &name, &0, &0, &None);
    let b_id = client.create_plan(&b, &token, &1_000, &3_600, &name, &0, &0, &None);

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
        &0,
        &0,
        &None,
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
        &0,
        &0,
        &None,
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
        &0,
        &0,
        &None,
    );
    assert_eq!(client.get_plan(&id).name, String::from_str(&env, &name));
}

// ---------------------------------------------------------------------------
// Payout address and referral share (#31, #32)
// ---------------------------------------------------------------------------

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
        &0,
        &0,
        &None,
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
        &0,
        &0,
        &Some(treasury.clone()),
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
        &0,
        &1_337,
        &None,
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
        &0,
        &(MAX_REFERRAL_BPS + 1),
        &None,
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
        &0,
        &MAX_REFERRAL_BPS,
        &None,
    );
    assert_eq!(client.get_plan(&id).referral_bps, MAX_REFERRAL_BPS);
}

// ---------------------------------------------------------------------------
// Two-step plan ownership transfer (#33)
// ---------------------------------------------------------------------------

/// The acceptance criterion for #33: the index of both merchants is updated.
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
        &0,
        &0,
        &None,
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

/// Moving a plan out of the middle of an index must leave every other entry
/// intact: swap the last entry into the vacated slot, keep both merchants'
/// plans resolvable, and keep appending correctly afterwards.
#[test]
fn transfer_moves_a_plan_out_of_a_multi_plan_index() {
    let (env, client, _admin) = setup();
    let merchant = Address::generate(&env);
    let new_owner = Address::generate(&env);
    let token = Address::generate(&env);
    let name = String::from_str(&env, "Plan");

    let first = client.create_plan(&merchant, &token, &1_000, &3_600, &name, &0, &0, &None);
    let middle = client.create_plan(&merchant, &token, &1_000, &3_600, &name, &0, &0, &None);
    let last = client.create_plan(&merchant, &token, &1_000, &3_600, &name, &0, &0, &None);

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
    let fourth = client.create_plan(&merchant, &token, &1_000, &3_600, &name, &0, &0, &None);
    assert_eq!(client.merchant_plan_count(&merchant), 3);
    assert!(client.merchant_plans(&merchant, &0, &10).contains(fourth));
    assert_eq!(client.get_plan(&fourth).merchant, merchant);
}

/// The acceptance criterion for #33: only the current owner can propose.
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
        &0,
        &0,
        &None,
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

/// The acceptance criterion for #33: a stranger cannot accept.
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
        &0,
        &0,
        &None,
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

/// A proposal is one-shot: after it is accepted, nobody can "accept" it a
/// second time, so a stale proposal can never move a plan later.
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
        &0,
        &0,
        &None,
    );

    client.propose_plan_owner(&merchant, &id, &new_owner);
    client.accept_plan_owner(&new_owner, &id);

    assert_eq!(
        client.try_accept_plan_owner(&new_owner, &id),
        Err(Ok(Error::PendingOwnershipNotFound))
    );
    assert_eq!(client.get_plan(&id).merchant, new_owner);
}

/// A later proposal replaces an earlier one: only the most recent proposal
/// can be accepted, so correcting a mistyped address is possible.
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
        &0,
        &0,
        &None,
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
        &0,
        &0,
        &None,
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

/// The accept is the new owner's signature to give: even the current owner's
/// signature does not cover it, so a different signer fails.
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
        &0,
        &0,
        &None,
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
        &0,
        &0,
        &None,
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
