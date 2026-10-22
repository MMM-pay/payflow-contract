#![cfg(test)]

use super::*;
use payflow_plan_registry::{PlanRegistry, PlanRegistryClient as RegistryClient};
use payflow_vault::{Vault, VaultClient as RealVaultClient};
use soroban_sdk::{
    testutils::{Address as _, Events as _, Ledger as _, MockAuth, MockAuthInvoke},
    token::{StellarAssetClient, TokenClient},
    xdr::{ContractEventBody, ScMap, ScSymbol, ScVal},
    Address, Env, IntoVal, String,
};

const MONTH: u64 = 2_592_000;
const PRICE: i128 = 10_000;
const FEE_BPS: u32 = 100; // 1%
const KEEPER_BPS: u32 = 200; // 2% of the merchant share

struct World<'a> {
    env: Env,
    sub: SubscriptionClient<'a>,
    registry: RegistryClient<'a>,
    vault: RealVaultClient<'a>,
    token: Address,
    token_client: TokenClient<'a>,
    minter: StellarAssetClient<'a>,
    merchant: Address,
    fee_to: Address,
    /// An independent keeper: not the merchant, not the subscriber.
    keeper: Address,
}

fn world() -> World<'static> {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1_000_000);

    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let merchant = Address::generate(&env);
    let fee_to = Address::generate(&env);
    let keeper = Address::generate(&env);

    let sac = env.register_stellar_asset_contract_v2(issuer);
    let token = sac.address();

    let registry_id = env.register(PlanRegistry, (admin.clone(),));
    let registry = RegistryClient::new(&env, &registry_id);

    let vault_id = env.register(Vault, (admin.clone(),));
    let vault = RealVaultClient::new(&env, &vault_id);

    let sub_id = env.register(
        Subscription,
        (
            admin.clone(),
            registry_id.clone(),
            vault_id.clone(),
            FEE_BPS,
            fee_to.clone(),
        ),
    );
    let sub = SubscriptionClient::new(&env, &sub_id);

    vault.set_subscription(&sub_id);

    World {
        token_client: TokenClient::new(&env, &token),
        minter: StellarAssetClient::new(&env, &token),
        env,
        sub,
        registry,
        vault,
        token,
        merchant,
        fee_to,
        keeper,
    }
}

impl World<'_> {
    /// A plan with no keeper tip, which is what most tests want.
    fn plan(&self) -> u64 {
        self.plan_with_keeper(0)
    }

    fn plan_with_keeper(&self, keeper_bps: u32) -> u64 {
        self.registry.create_plan(
            &self.merchant,
            &self.token,
            &PRICE,
            &MONTH,
            &String::from_str(&self.env, "Demo Plan"),
            &keeper_bps,
        )
    }

    /// A subscriber with `funding` deposited into the vault.
    fn subscriber(&self, funding: i128) -> Address {
        let user = Address::generate(&self.env);
        self.minter.mint(&user, &(funding * 2));
        self.vault.deposit(&user, &self.token, &funding);
        user
    }

    fn advance(&self, secs: u64) {
        let now = self.env.ledger().timestamp();
        self.env.ledger().set_timestamp(now + secs);
    }
}

#[test]
fn subscribe_copies_plan_terms_into_mandate() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);

    let id = w.sub.subscribe(&user, &plan_id, &0);
    let m = w.sub.get_mandate(&id);

    assert_eq!(m.subscriber, user);
    assert_eq!(m.merchant, w.merchant);
    assert_eq!(m.amount, PRICE);
    assert_eq!(m.period, MONTH);
    assert_eq!(m.created_at, w.env.ledger().timestamp());
    assert_eq!(m.charges_made, 0);
    assert_eq!(m.status, MandateStatus::Active);
    assert_eq!(m.next_charge, w.env.ledger().timestamp());
}

/// Pull a u32 field out of the data map of any emitted contract event.
fn event_field_u32(env: &Env, field: &str) -> Option<u32> {
    for event in env.events().all().events() {
        let ContractEventBody::V0(body) = &event.body;
        let ScVal::Map(Some(ScMap(entries))) = &body.data else {
            continue;
        };
        for entry in entries.iter() {
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
fn subscribe_event_carries_the_spend_cap() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);

    let id = w.sub.subscribe(&user, &plan_id, &3);

    // The cap must be recoverable from the event alone. An indexer that reads
    // only events must not have to guess it, or it will report a capped
    // mandate as open-ended.
    assert_eq!(
        event_field_u32(&w.env, "max_charges"),
        Some(3),
        "Subscribed event must carry max_charges on the wire"
    );
    assert_eq!(w.sub.get_mandate(&id).max_charges, 3);
}

#[test]
fn subscribe_rejects_inactive_plan() {
    let w = world();
    let plan_id = w.plan();
    w.registry.set_plan_active(&w.merchant, &plan_id, &false);
    let user = w.subscriber(100_000);

    assert_eq!(
        w.sub.try_subscribe(&user, &plan_id, &0),
        Err(Ok(Error::PlanInactive))
    );
}

#[test]
fn first_charge_is_due_immediately_and_splits_fee() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0);

    assert!(w.sub.is_due(&id));
    w.sub.charge(&w.keeper, &id);

    let expected_fee = PRICE * FEE_BPS as i128 / BPS_DENOMINATOR;
    assert_eq!(expected_fee, 100);
    assert_eq!(w.token_client.balance(&w.merchant), PRICE - expected_fee);
    assert_eq!(w.token_client.balance(&w.fee_to), expected_fee);
    // No tip on the plan: the keeper that settled is paid nothing.
    assert_eq!(w.token_client.balance(&w.keeper), 0);
    assert_eq!(w.vault.balance(&user, &w.token), 100_000 - PRICE);

    let m = w.sub.get_mandate(&id);
    assert_eq!(m.charges_made, 1);
    assert_eq!(m.created_at, 1_000_000);
    assert_eq!(m.next_charge, w.env.ledger().timestamp() + MONTH);
}

#[test]
fn subscribe_requires_the_subscriber_to_authorize() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let other_signer = Address::generate(&w.env);

    w.env.mock_auths(&[MockAuth {
        address: &other_signer,
        invoke: &MockAuthInvoke {
            contract: &w.sub.address,
            fn_name: "subscribe",
            args: (&user, &plan_id, &0).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);

    assert!(w.sub.try_subscribe(&user, &plan_id, &0).is_err());
}

#[test]
fn any_account_can_settle_a_charge() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0);

    // An unrelated account may settle the mandate: `charge` needs no
    // subscriber or merchant signature, and the vault accepts the debit purely
    // because the subscription contract is the direct caller. The vault debit
    // to the merchant therefore succeeds even though the keeper never holds
    // the token.
    w.sub.charge(&w.keeper, &id);

    assert_eq!(w.sub.get_mandate(&id).charges_made, 1);
    assert_eq!(w.token_client.balance(&w.merchant), PRICE - 100);
}

#[test]
fn charge_requires_the_keeper_to_authorize() {
    let w = world();
    let plan_id = w.plan_with_keeper(KEEPER_BPS);
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0);

    // The tip is paid to the caller, so the caller must prove it controls
    // that address. With every authorization dropped, nothing settles.
    w.env.mock_auths(&[]);
    assert!(w.sub.try_charge(&w.keeper, &id).is_err());
    assert_eq!(w.sub.get_mandate(&id).charges_made, 0);
    assert_eq!(w.token_client.balance(&w.keeper), 0);
}

#[test]
fn a_different_signer_cannot_collect_the_tip() {
    let w = world();
    let plan_id = w.plan_with_keeper(KEEPER_BPS);
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0);

    // The subscriber signs, but the caller is the keeper: the signature does
    // not cover the keeper address, so the charge is rejected.
    w.env.mock_auths(&[MockAuth {
        address: &user,
        invoke: &MockAuthInvoke {
            contract: &w.sub.address,
            fn_name: "charge",
            args: (&w.keeper, &id).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);
    assert!(w.sub.try_charge(&w.keeper, &id).is_err());
    assert_eq!(w.sub.get_mandate(&id).charges_made, 0);
}

#[test]
fn keeper_tip_is_paid_from_the_merchant_share() {
    let w = world();
    let plan_id = w.plan_with_keeper(KEEPER_BPS);
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0);

    let fee = PRICE * FEE_BPS as i128 / BPS_DENOMINATOR;
    let merchant_gross = PRICE - fee;
    let tip = merchant_gross * KEEPER_BPS as i128 / BPS_DENOMINATOR;

    w.sub.charge(&w.keeper, &id);

    assert_eq!(w.token_client.balance(&w.keeper), tip);
    assert_eq!(w.token_client.balance(&w.merchant), merchant_gross - tip);
    assert_eq!(w.token_client.balance(&w.fee_to), fee);
    // The subscriber pays the plan price, no more.
    assert_eq!(w.vault.balance(&user, &w.token), 100_000 - PRICE);
    // The tip rate is frozen into the mandate, like the fee.
    assert_eq!(w.sub.get_mandate(&id).keeper_bps, KEEPER_BPS);
}

/// The acceptance criterion for the tip: a tip changes how the plan's price is
/// split, never what the subscriber pays.
#[test]
fn subscriber_pays_the_same_total_with_or_without_a_tip() {
    let w = world();
    let plain = w.plan();
    let tipped = w.plan_with_keeper(KEEPER_BPS);

    let without = w.subscriber(100_000);
    let with = w.subscriber(100_000);
    let plain_id = w.sub.subscribe(&without, &plain, &0);
    let tipped_id = w.sub.subscribe(&with, &tipped, &0);

    w.sub.charge(&w.keeper, &plain_id);
    w.sub.charge(&w.keeper, &tipped_id);

    assert_eq!(w.vault.balance(&without, &w.token), 100_000 - PRICE);
    assert_eq!(w.vault.balance(&with, &w.token), 100_000 - PRICE);

    // Only the tipped charge pays the keeper.
    let fee = PRICE * FEE_BPS as i128 / BPS_DENOMINATOR;
    let tip = (PRICE - fee) * KEEPER_BPS as i128 / BPS_DENOMINATOR;
    assert_eq!(w.token_client.balance(&w.keeper), tip);
}

#[test]
fn second_charge_before_period_elapses_is_rejected() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0);

    w.sub.charge(&w.keeper, &id);
    assert!(!w.sub.is_due(&id));
    assert_eq!(w.sub.try_charge(&w.keeper, &id), Err(Ok(Error::NotDue)));

    w.advance(MONTH - 1);
    assert_eq!(w.sub.try_charge(&w.keeper, &id), Err(Ok(Error::NotDue)));
}

#[test]
fn charge_succeeds_once_period_elapses() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0);

    w.sub.charge(&w.keeper, &id);
    w.advance(MONTH);
    w.sub.charge(&w.keeper, &id);

    assert_eq!(w.sub.get_mandate(&id).charges_made, 2);
    assert_eq!(w.vault.balance(&user, &w.token), 100_000 - 2 * PRICE);
}

#[test]
fn keeper_outage_does_not_create_a_chargeable_backlog() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0);

    w.sub.charge(&w.keeper, &id);
    // Nobody calls charge for five periods.
    w.advance(MONTH * 5);
    w.sub.charge(&w.keeper, &id);

    // Exactly one extra charge is collected, not five.
    assert_eq!(w.sub.get_mandate(&id).charges_made, 2);
    assert_eq!(w.sub.try_charge(&w.keeper, &id), Err(Ok(Error::NotDue)));
}

#[test]
fn cancel_stops_billing_permanently() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0);

    w.sub.charge(&w.keeper, &id);
    w.sub.cancel(&user, &id);
    w.advance(MONTH * 2);

    assert_eq!(w.sub.get_mandate(&id).status, MandateStatus::Cancelled);
    assert!(!w.sub.is_due(&id));
    assert_eq!(
        w.sub.try_charge(&w.keeper, &id),
        Err(Ok(Error::MandateNotActive))
    );
}

#[test]
fn cancel_rejects_non_subscriber() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let attacker = Address::generate(&w.env);
    let id = w.sub.subscribe(&user, &plan_id, &0);

    assert_eq!(
        w.sub.try_cancel(&attacker, &id),
        Err(Ok(Error::NotSubscriber))
    );
}

#[test]
fn cancel_twice_is_rejected() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0);

    w.sub.cancel(&user, &id);
    assert_eq!(
        w.sub.try_cancel(&user, &id),
        Err(Ok(Error::MandateNotActive))
    );
}

#[test]
fn pause_suspends_billing_and_resume_restores_it() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0);

    w.sub.set_paused(&user, &id, &true);
    assert_eq!(w.sub.get_mandate(&id).status, MandateStatus::Paused);
    assert!(!w.sub.is_due(&id));
    assert_eq!(
        w.sub.try_charge(&w.keeper, &id),
        Err(Ok(Error::MandateNotActive))
    );

    w.sub.set_paused(&user, &id, &false);
    assert_eq!(w.sub.get_mandate(&id).status, MandateStatus::Active);
    w.sub.charge(&w.keeper, &id);
    assert_eq!(w.sub.get_mandate(&id).charges_made, 1);
}

#[test]
fn pausing_a_cancelled_mandate_is_rejected() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0);

    w.sub.cancel(&user, &id);
    assert_eq!(
        w.sub.try_set_paused(&user, &id, &true),
        Err(Ok(Error::MandateNotActive))
    );
}

#[test]
fn max_charges_completes_the_mandate() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &2);

    w.sub.charge(&w.keeper, &id);
    w.advance(MONTH);
    w.sub.charge(&w.keeper, &id);

    let m = w.sub.get_mandate(&id);
    assert_eq!(m.charges_made, 2);
    assert_eq!(m.status, MandateStatus::Completed);
    assert!(!w.sub.is_due(&id));

    w.advance(MONTH);
    assert_eq!(
        w.sub.try_charge(&w.keeper, &id),
        Err(Ok(Error::MandateNotActive))
    );
}

#[test]
fn charge_fails_when_vault_is_underfunded() {
    let w = world();
    let plan_id = w.plan();
    // Funded for one charge only.
    let user = w.subscriber(PRICE);
    let id = w.sub.subscribe(&user, &plan_id, &0);

    w.sub.charge(&w.keeper, &id);
    assert_eq!(w.vault.balance(&user, &w.token), 0);

    w.advance(MONTH);
    assert!(w.sub.try_charge(&w.keeper, &id).is_err());
    // The failed charge must not advance billing state.
    assert_eq!(w.sub.get_mandate(&id).charges_made, 1);
}

#[test]
fn subscriber_can_withdraw_and_starve_a_mandate() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0);

    w.vault.withdraw(&user, &w.token, &100_000);
    assert!(w.sub.try_charge(&w.keeper, &id).is_err());
}

#[test]
fn indexes_track_both_sides() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);

    let a = w.sub.subscribe(&user, &plan_id, &0);
    let b = w.sub.subscribe(&user, &plan_id, &0);

    assert_eq!(
        w.sub.subscriber_mandates(&user, &0, &10),
        soroban_sdk::vec![&w.env, a, b]
    );
    assert_eq!(
        w.sub.merchant_mandates(&w.merchant, &0, &10),
        soroban_sdk::vec![&w.env, a, b]
    );
    assert_eq!(w.sub.subscriber_mandate_count(&user), 2);
    assert_eq!(w.sub.merchant_mandate_count(&w.merchant), 2);
}

#[test]
fn zero_fee_pays_merchant_in_full() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1_000_000);

    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let merchant = Address::generate(&env);
    let fee_to = Address::generate(&env);
    let keeper = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(issuer).address();

    let registry_id = env.register(PlanRegistry, (admin.clone(),));
    let registry = RegistryClient::new(&env, &registry_id);
    let vault_id = env.register(Vault, (admin.clone(),));
    let vault = RealVaultClient::new(&env, &vault_id);
    let sub_id = env.register(
        Subscription,
        (
            admin.clone(),
            registry_id.clone(),
            vault_id.clone(),
            0u32,
            fee_to.clone(),
        ),
    );
    let sub = SubscriptionClient::new(&env, &sub_id);
    vault.set_subscription(&sub_id);

    let plan_id = registry.create_plan(
        &merchant,
        &token,
        &PRICE,
        &MONTH,
        &String::from_str(&env, "Demo Plan"),
        &0,
    );
    let user = Address::generate(&env);
    StellarAssetClient::new(&env, &token).mint(&user, &100_000);
    vault.deposit(&user, &token, &100_000);

    let id = sub.subscribe(&user, &plan_id, &0);
    sub.charge(&keeper, &id);

    assert_eq!(TokenClient::new(&env, &token).balance(&merchant), PRICE);
    assert_eq!(TokenClient::new(&env, &token).balance(&fee_to), 0);
}

#[test]
#[should_panic]
fn constructor_rejects_excessive_fee() {
    let env = Env::default();
    env.mock_all_auths();
    let a = Address::generate(&env);
    env.register(
        Subscription,
        (a.clone(), a.clone(), a.clone(), MAX_FEE_BPS + 1, a.clone()),
    );
}

#[test]
fn constructor_records_its_configuration() {
    let w = world();
    assert_eq!(w.sub.fee_bps(), FEE_BPS);
    assert_eq!(w.sub.plan_registry(), w.registry.address);
    assert_eq!(w.sub.vault(), w.vault.address);
}

#[test]
fn unknown_mandate_errors() {
    let w = world();
    assert_eq!(w.sub.try_get_mandate(&999), Err(Ok(Error::MandateNotFound)));
}

// ---------------------------------------------------------------------------
// Protocol fee administration
// ---------------------------------------------------------------------------

#[test]
fn set_fee_bps_changes_the_fee_for_new_mandates() {
    let w = world();
    let plan_id = w.plan();

    w.sub.set_fee_bps(&0);
    assert_eq!(w.sub.fee_bps(), 0);

    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0);
    w.sub.charge(&w.keeper, &id);

    // Zero fee: the merchant receives the whole charge.
    assert_eq!(w.token_client.balance(&w.merchant), PRICE);
    assert_eq!(w.token_client.balance(&w.fee_to), 0);
}

#[test]
fn an_admin_fee_change_cannot_reprice_an_open_mandate() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);

    // Subscriber opens at 1%.
    let id = w.sub.subscribe(&user, &plan_id, &0);
    assert_eq!(w.sub.get_mandate(&id).fee_bps, FEE_BPS);

    // Admin raises the fee to the 10% ceiling afterwards.
    w.sub.set_fee_bps(&MAX_FEE_BPS);
    assert_eq!(w.sub.fee_bps(), MAX_FEE_BPS);

    w.sub.charge(&w.keeper, &id);

    // The mandate still settles at the 1% it was opened with.
    let expected_fee = PRICE * FEE_BPS as i128 / BPS_DENOMINATOR;
    assert_eq!(w.token_client.balance(&w.fee_to), expected_fee);
    assert_eq!(w.token_client.balance(&w.merchant), PRICE - expected_fee);
    assert_eq!(w.sub.get_mandate(&id).fee_bps, FEE_BPS);
}

#[test]
fn mandates_opened_before_and_after_a_fee_change_keep_their_own_fees() {
    let w = world();
    let plan_id = w.plan();
    let before = w.subscriber(100_000);
    let old_id = w.sub.subscribe(&before, &plan_id, &0);

    w.sub.set_fee_bps(&0);

    let after = w.subscriber(100_000);
    let new_id = w.sub.subscribe(&after, &plan_id, &0);

    assert_eq!(w.sub.get_mandate(&old_id).fee_bps, FEE_BPS);
    assert_eq!(w.sub.get_mandate(&new_id).fee_bps, 0);

    w.sub.charge(&w.keeper, &old_id);
    w.sub.charge(&w.keeper, &new_id);

    // Only the pre-change mandate contributed a fee.
    assert_eq!(
        w.token_client.balance(&w.fee_to),
        PRICE * FEE_BPS as i128 / BPS_DENOMINATOR
    );
}

#[test]
fn set_fee_bps_rejects_a_fee_above_the_ceiling() {
    let w = world();
    assert_eq!(
        w.sub.try_set_fee_bps(&(MAX_FEE_BPS + 1)),
        Err(Ok(Error::FeeTooHigh))
    );
    // The stored fee is unchanged by the rejected call.
    assert_eq!(w.sub.fee_bps(), FEE_BPS);
}

#[test]
fn set_fee_bps_accepts_exactly_the_ceiling() {
    let w = world();
    w.sub.set_fee_bps(&MAX_FEE_BPS);
    assert_eq!(w.sub.fee_bps(), MAX_FEE_BPS);
}

#[test]
fn set_fee_bps_requires_admin_auth() {
    let w = world();
    // Drop the blanket mock: the call must carry the admin's authorization.
    w.env.set_auths(&[]);
    assert!(w.sub.try_set_fee_bps(&0).is_err());
    assert_eq!(w.sub.fee_bps(), FEE_BPS);
}

#[test]
fn index_pages_are_bounded_and_ordered() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);

    let mut ids = soroban_sdk::Vec::<u64>::new(&w.env);
    for _ in 0..5 {
        ids.push_back(w.sub.subscribe(&user, &plan_id, &0));
    }

    assert_eq!(
        w.sub.merchant_mandates(&w.merchant, &1, &2),
        soroban_sdk::vec![&w.env, ids.get_unchecked(1), ids.get_unchecked(2)]
    );
    // A page that runs past the end is cut short, not an error.
    assert_eq!(
        w.sub.merchant_mandates(&w.merchant, &4, &10),
        soroban_sdk::vec![&w.env, ids.get_unchecked(4)]
    );
    assert_eq!(w.sub.merchant_mandates(&w.merchant, &9, &10).len(), 0);
    // Huge start and limit values must not overflow.
    assert_eq!(
        w.sub
            .merchant_mandates(&w.merchant, &u32::MAX, &u32::MAX)
            .len(),
        0
    );
    assert_eq!(w.sub.subscriber_mandates(&user, &0, &0).len(), 0);
}

#[test]
fn index_page_size_is_capped() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    for _ in 0..(MAX_PAGE + 5) {
        w.sub.subscribe(&user, &plan_id, &0);
    }

    assert_eq!(w.sub.merchant_mandate_count(&w.merchant), MAX_PAGE + 5);
    assert_eq!(
        w.sub.merchant_mandates(&w.merchant, &0, &u32::MAX).len(),
        MAX_PAGE
    );
    assert_eq!(
        w.sub
            .merchant_mandates(&w.merchant, &MAX_PAGE, &MAX_PAGE)
            .len(),
        5
    );
}

/// Opening a mandate against a merchant with many existing mandates writes
/// the same fixed-size entries as the first one did. Before, every subscribe
/// rewrote one growing list, so throwaway mandates could push it past the
/// ledger entry size limit and block the merchant's real subscribers.
#[test]
fn subscribe_cost_does_not_grow_with_the_merchant_index() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);

    for _ in 0..300 {
        w.sub.subscribe(&user, &plan_id, &0);
    }

    let latecomer = w.subscriber(100_000);
    let id = w.sub.subscribe(&latecomer, &plan_id, &0);
    assert_eq!(w.sub.merchant_mandate_count(&w.merchant), 301);
    assert_eq!(
        w.sub.merchant_mandates(&w.merchant, &300, &1),
        soroban_sdk::vec![&w.env, id]
    );
    assert_eq!(
        w.sub.subscriber_mandates(&latecomer, &0, &10),
        soroban_sdk::vec![&w.env, id]
    );
}

#[test]
fn merchant_can_end_a_mandate() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0);
    w.sub.charge(&w.keeper, &id);

    w.sub.end_mandate(&w.merchant, &id);

    let m = w.sub.get_mandate(&id);
    assert_eq!(m.status, MandateStatus::Cancelled);
    assert_eq!(m.charges_made, 1);

    w.advance(MONTH);
    assert!(!w.sub.is_due(&id));
    assert_eq!(
        w.sub.try_charge(&w.keeper, &id),
        Err(Ok(Error::MandateNotActive))
    );
}

#[test]
fn merchant_can_end_a_paused_mandate() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0);
    w.sub.set_paused(&user, &id, &true);

    w.sub.end_mandate(&w.merchant, &id);

    assert_eq!(w.sub.get_mandate(&id).status, MandateStatus::Cancelled);
    // Ending is permanent: the subscriber cannot resume it.
    assert_eq!(
        w.sub.try_set_paused(&user, &id, &false),
        Err(Ok(Error::MandateNotActive))
    );
}

#[test]
fn only_the_mandates_merchant_can_end_it() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0);

    let stranger = Address::generate(&w.env);
    assert_eq!(
        w.sub.try_end_mandate(&stranger, &id),
        Err(Ok(Error::NotMerchant))
    );
    // The subscriber is not the merchant either; they have `cancel`.
    assert_eq!(
        w.sub.try_end_mandate(&user, &id),
        Err(Ok(Error::NotMerchant))
    );
    assert_eq!(w.sub.get_mandate(&id).status, MandateStatus::Active);
}

#[test]
fn ending_a_finished_mandate_is_rejected() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);

    let cancelled = w.sub.subscribe(&user, &plan_id, &0);
    w.sub.cancel(&user, &cancelled);
    assert_eq!(
        w.sub.try_end_mandate(&w.merchant, &cancelled),
        Err(Ok(Error::MandateNotActive))
    );

    let completed = w.sub.subscribe(&user, &plan_id, &1);
    w.sub.charge(&w.keeper, &completed);
    assert_eq!(
        w.sub.get_mandate(&completed).status,
        MandateStatus::Completed
    );
    assert_eq!(
        w.sub.try_end_mandate(&w.merchant, &completed),
        Err(Ok(Error::MandateNotActive))
    );
}

#[test]
fn end_mandate_requires_the_merchants_auth() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0);

    w.sub.end_mandate(&w.merchant, &id);

    let auths = w.env.auths();
    assert_eq!(auths.len(), 1);
    assert_eq!(auths[0].0, w.merchant);
}

#[test]
fn end_mandate_emits_an_event_naming_the_merchant() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0);
    w.sub.charge(&w.keeper, &id);

    w.sub.end_mandate(&w.merchant, &id);

    assert_eq!(event_field_u32(&w.env, "charges_made"), Some(1));
    let last = w.env.events().all().events().last().cloned().unwrap();
    let ContractEventBody::V0(body) = &last.body;
    let ScVal::Symbol(ScSymbol(name)) = &body.topics[0] else {
        panic!("topic 0 is not a symbol");
    };
    let expected: soroban_sdk::xdr::StringM<32> = "mandate_ended".try_into().unwrap();
    assert_eq!(name, &expected);
}

/// Verify that the protocol fee is transferred to the fee_to address when a
/// mandate is charged. This tests that fee_to can be set to any address
/// (account or contract) and the fee is correctly transferred.
#[test]
fn fee_paid_to_address() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1_000_000);

    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let merchant = Address::generate(&env);
    let fee_to = Address::generate(&env);
    let keeper = Address::generate(&env);

    let token = env.register_stellar_asset_contract_v2(issuer).address();

    let registry_id = env.register(PlanRegistry, (admin.clone(),));
    let registry = RegistryClient::new(&env, &registry_id);
    let vault_id = env.register(Vault, (admin.clone(),));
    let vault = RealVaultClient::new(&env, &vault_id);
    vault.set_subscription(&Address::generate(&env));

    let sub_id = env.register(
        Subscription,
        (
            admin.clone(),
            registry_id.clone(),
            vault_id.clone(),
            FEE_BPS,
            fee_to.clone(),
        ),
    );
    let sub = SubscriptionClient::new(&env, &sub_id);

    vault.set_subscription(&sub_id);

    let plan_id = registry.create_plan(
        &merchant,
        &token,
        &PRICE,
        &MONTH,
        &String::from_str(&env, "Demo Plan"),
        &0,
    );
    let user = Address::generate(&env);
    StellarAssetClient::new(&env, &token).mint(&user, &100_000);
    vault.deposit(&user, &token, &100_000);

    let id = sub.subscribe(&user, &plan_id, &0);
    sub.charge(&keeper, &id);

    // The merchant should receive the price minus the fee.
    let expected_fee = PRICE * (FEE_BPS as i128) / BPS_DENOMINATOR;
    assert_eq!(
        TokenClient::new(&env, &token).balance(&merchant),
        PRICE - expected_fee,
        "merchant should receive price minus fee"
    );
    // The fee should be transferred to fee_to.
    assert_eq!(
        TokenClient::new(&env, &token).balance(&fee_to),
        expected_fee,
        "fee should be transferred to fee_to address"
    );
}

/// The sum of internal vault balances per token must equal the vault's token
/// balance, after every deposit, withdrawal and charge. This test drives the
/// real charge path through the subscription contract for several subscribers
/// and asserts the invariant after every step.
#[test]
fn charges_keep_vault_balances_in_sync() {
    let w = world();
    let plan_id = w.plan();

    let users: [Address; 4] = [
        Address::generate(&w.env),
        Address::generate(&w.env),
        Address::generate(&w.env),
        Address::generate(&w.env),
    ];
    let mut ids: [u64; 4] = [0; 4];
    let mut expected: [i128; 4] = [0; 4];
    for i in 0..4 {
        w.minter.mint(&users[i], &200_000);
        w.vault.deposit(&users[i], &w.token, &100_000);
        expected[i] = 100_000;
        ids[i] = w.sub.subscribe(&users[i], &plan_id, &0);
    }

    assert_vault_invariant(&w, &users, &expected);

    // Deterministic pseudo-random steps: withdraw part of a balance, or try a
    // real charge one period later.
    let mut state: u64 = 0x_1234_5678_9abc_def0;
    let mut below = move |bound: u64| {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        state % bound
    };

    for _ in 0..120 {
        let i = below(4) as usize;
        let user = users[i].clone();

        if below(2) == 0 || expected[i] < PRICE {
            // Withdraw part of the balance, or nothing when it is empty.
            if expected[i] > 0 {
                let amount = 1 + (below(expected[i] as u64) as i128);
                w.vault.withdraw(&user, &w.token, &amount);
                expected[i] -= amount;
            }
        } else {
            // Settle the mandate one period after the previous state change.
            w.advance(MONTH);
            let before = w.sub.get_mandate(&ids[i]).charges_made;
            let res = w.sub.try_charge(&w.keeper, &ids[i]);
            if res.is_ok() {
                assert_eq!(
                    w.sub.get_mandate(&ids[i]).charges_made,
                    before + 1,
                    "a successful charge must record the charge"
                );
                // The whole price leaves the user's vault balance: the
                // merchant share goes to the merchant and the fee to fee_to.
                expected[i] -= PRICE;
            }
            // A failed charge (underfunded) changes nothing.
        }

        assert_vault_invariant(&w, &users, &expected);
    }
}

/// Asserts each subscriber's internal vault balance matches the expected
/// value and that their sum equals the vault's token holdings.
fn assert_vault_invariant(w: &World, users: &[Address; 4], expected: &[i128]) {
    let mut sum: i128 = 0;
    for (i, user) in users.iter().enumerate() {
        assert_eq!(
            w.vault.balance(user, &w.token),
            expected[i],
            "internal balance of user {i} drifted from the expected value"
        );
        sum += expected[i];
    }
    assert_eq!(
        sum,
        w.token_client.balance(&w.vault.address),
        "the sum of internal balances must equal the vault's token holdings"
    );
}

#[test]
fn fee_to_contract_address_receives_the_fee() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1_000_000);

    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let merchant = Address::generate(&env);
    let keeper = Address::generate(&env);

    // Use the plan-registry contract itself as the fee destination. A contract
    // address (starts with C) proves the fee path works for a non-account
    // recipient such as a multisig or treasury contract.
    let registry_id = env.register(PlanRegistry, (admin.clone(),));
    let registry = RegistryClient::new(&env, &registry_id);

    let sac = env.register_stellar_asset_contract_v2(issuer);
    let token = sac.address();
    let token_client = TokenClient::new(&env, &token);
    let minter = StellarAssetClient::new(&env, &token);

    let vault_id = env.register(Vault, (admin.clone(),));
    let vault = RealVaultClient::new(&env, &vault_id);

    // fee_to = the registry contract address (a contract, not an account).
    let sub_id = env.register(
        Subscription,
        (
            admin.clone(),
            registry_id.clone(),
            vault_id.clone(),
            FEE_BPS,
            registry_id.clone(),
        ),
    );
    let sub = SubscriptionClient::new(&env, &sub_id);
    vault.set_subscription(&sub_id);

    let plan_id = registry.create_plan(
        &merchant,
        &token,
        &PRICE,
        &MONTH,
        &String::from_str(&env, "Demo Plan"),
        &0,
    );
    let user = Address::generate(&env);
    minter.mint(&user, &100_000);
    vault.deposit(&user, &token, &50_000);

    let id = sub.subscribe(&user, &plan_id, &0);
    sub.charge(&keeper, &id);

    let expected_fee = PRICE * FEE_BPS as i128 / BPS_DENOMINATOR;
    // The fee went to the registry contract's token balance, not to an
    // account address.
    assert_eq!(token_client.balance(&registry_id), expected_fee);
    assert_eq!(token_client.balance(&merchant), PRICE - expected_fee);
    assert_eq!(vault.balance(&user, &token), 50_000 - PRICE);
}
