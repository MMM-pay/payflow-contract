#![cfg(test)]

use super::*;
use payflow_plan_registry::{PlanRegistry, PlanRegistryClient as RegistryClient};
use payflow_vault::{Vault, VaultClient as RealVaultClient};
use soroban_sdk::{
    testutils::{Address as _, Events as _, Ledger as _},
    token::{StellarAssetClient, TokenClient},
    xdr::{ContractEventBody, ScMap, ScSymbol, ScVal},
    Address, Env,
};

const MONTH: u64 = 2_592_000;
const PRICE: i128 = 10_000;
const FEE_BPS: u32 = 100; // 1%

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
}

fn world() -> World<'static> {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1_000_000);

    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let merchant = Address::generate(&env);
    let fee_to = Address::generate(&env);

    let sac = env.register_stellar_asset_contract_v2(issuer);
    let token = sac.address();

    let registry_id = env.register(PlanRegistry, ());
    let registry = RegistryClient::new(&env, &registry_id);
    registry.initialize(&admin);

    let vault_id = env.register(Vault, ());
    let vault = RealVaultClient::new(&env, &vault_id);
    vault.initialize(&admin);

    let sub_id = env.register(Subscription, ());
    let sub = SubscriptionClient::new(&env, &sub_id);
    sub.initialize(&admin, &registry_id, &vault_id, &FEE_BPS, &fee_to);

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
    }
}

impl World<'_> {
    fn plan(&self) -> u64 {
        self.registry
            .create_plan(&self.merchant, &self.token, &PRICE, &MONTH)
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
    w.sub.charge(&id);

    let expected_fee = PRICE * FEE_BPS as i128 / BPS_DENOMINATOR;
    assert_eq!(expected_fee, 100);
    assert_eq!(w.token_client.balance(&w.merchant), PRICE - expected_fee);
    assert_eq!(w.token_client.balance(&w.fee_to), expected_fee);
    assert_eq!(w.vault.balance(&user, &w.token), 100_000 - PRICE);

    let m = w.sub.get_mandate(&id);
    assert_eq!(m.charges_made, 1);
    assert_eq!(m.next_charge, w.env.ledger().timestamp() + MONTH);
}

#[test]
fn charge_is_permissionless() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0);

    // Drop every mocked authorization. `charge` must still settle, proving it
    // needs no subscriber signature and that the vault accepts the debit purely
    // because the subscription contract is the direct caller.
    w.env.mock_auths(&[]);
    w.sub.charge(&id);

    assert_eq!(w.sub.get_mandate(&id).charges_made, 1);
    assert_eq!(w.token_client.balance(&w.merchant), PRICE - 100);
}

#[test]
fn second_charge_before_period_elapses_is_rejected() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0);

    w.sub.charge(&id);
    assert!(!w.sub.is_due(&id));
    assert_eq!(w.sub.try_charge(&id), Err(Ok(Error::NotDue)));

    w.advance(MONTH - 1);
    assert_eq!(w.sub.try_charge(&id), Err(Ok(Error::NotDue)));
}

#[test]
fn charge_succeeds_once_period_elapses() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0);

    w.sub.charge(&id);
    w.advance(MONTH);
    w.sub.charge(&id);

    assert_eq!(w.sub.get_mandate(&id).charges_made, 2);
    assert_eq!(w.vault.balance(&user, &w.token), 100_000 - 2 * PRICE);
}

#[test]
fn keeper_outage_does_not_create_a_chargeable_backlog() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0);

    w.sub.charge(&id);
    // Nobody calls charge for five periods.
    w.advance(MONTH * 5);
    w.sub.charge(&id);

    // Exactly one extra charge is collected, not five.
    assert_eq!(w.sub.get_mandate(&id).charges_made, 2);
    assert_eq!(w.sub.try_charge(&id), Err(Ok(Error::NotDue)));
}

#[test]
fn cancel_stops_billing_permanently() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0);

    w.sub.charge(&id);
    w.sub.cancel(&user, &id);
    w.advance(MONTH * 2);

    assert_eq!(w.sub.get_mandate(&id).status, MandateStatus::Cancelled);
    assert!(!w.sub.is_due(&id));
    assert_eq!(w.sub.try_charge(&id), Err(Ok(Error::MandateNotActive)));
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
    assert_eq!(w.sub.try_charge(&id), Err(Ok(Error::MandateNotActive)));

    w.sub.set_paused(&user, &id, &false);
    assert_eq!(w.sub.get_mandate(&id).status, MandateStatus::Active);
    w.sub.charge(&id);
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

    w.sub.charge(&id);
    w.advance(MONTH);
    w.sub.charge(&id);

    let m = w.sub.get_mandate(&id);
    assert_eq!(m.charges_made, 2);
    assert_eq!(m.status, MandateStatus::Completed);
    assert!(!w.sub.is_due(&id));

    w.advance(MONTH);
    assert_eq!(w.sub.try_charge(&id), Err(Ok(Error::MandateNotActive)));
}

#[test]
fn charge_fails_when_vault_is_underfunded() {
    let w = world();
    let plan_id = w.plan();
    // Funded for one charge only.
    let user = w.subscriber(PRICE);
    let id = w.sub.subscribe(&user, &plan_id, &0);

    w.sub.charge(&id);
    assert_eq!(w.vault.balance(&user, &w.token), 0);

    w.advance(MONTH);
    assert!(w.sub.try_charge(&id).is_err());
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
    assert!(w.sub.try_charge(&id).is_err());
}

#[test]
fn indexes_track_both_sides() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);

    let a = w.sub.subscribe(&user, &plan_id, &0);
    let b = w.sub.subscribe(&user, &plan_id, &0);

    assert_eq!(
        w.sub.subscriber_mandates(&user),
        soroban_sdk::vec![&w.env, a, b]
    );
    assert_eq!(
        w.sub.merchant_mandates(&w.merchant),
        soroban_sdk::vec![&w.env, a, b]
    );
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
    let token = env.register_stellar_asset_contract_v2(issuer).address();

    let registry_id = env.register(PlanRegistry, ());
    let registry = RegistryClient::new(&env, &registry_id);
    registry.initialize(&admin);
    let vault_id = env.register(Vault, ());
    let vault = RealVaultClient::new(&env, &vault_id);
    vault.initialize(&admin);
    let sub_id = env.register(Subscription, ());
    let sub = SubscriptionClient::new(&env, &sub_id);
    sub.initialize(&admin, &registry_id, &vault_id, &0u32, &fee_to);
    vault.set_subscription(&sub_id);

    let plan_id = registry.create_plan(&merchant, &token, &PRICE, &MONTH);
    let user = Address::generate(&env);
    StellarAssetClient::new(&env, &token).mint(&user, &100_000);
    vault.deposit(&user, &token, &100_000);

    let id = sub.subscribe(&user, &plan_id, &0);
    sub.charge(&id);

    assert_eq!(TokenClient::new(&env, &token).balance(&merchant), PRICE);
    assert_eq!(TokenClient::new(&env, &token).balance(&fee_to), 0);
}

#[test]
fn initialize_rejects_excessive_fee() {
    let env = Env::default();
    env.mock_all_auths();
    let a = Address::generate(&env);
    let sub_id = env.register(Subscription, ());
    let sub = SubscriptionClient::new(&env, &sub_id);
    assert_eq!(
        sub.try_initialize(&a, &a, &a, &(MAX_FEE_BPS + 1), &a),
        Err(Ok(Error::FeeTooHigh))
    );
}

#[test]
fn initialize_is_single_use() {
    let w = world();
    let a = Address::generate(&w.env);
    assert_eq!(
        w.sub.try_initialize(&a, &a, &a, &0u32, &a),
        Err(Ok(Error::AlreadyInitialized))
    );
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
    w.sub.charge(&id);

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

    w.sub.charge(&id);

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

    w.sub.charge(&old_id);
    w.sub.charge(&new_id);

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
