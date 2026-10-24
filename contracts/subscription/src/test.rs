#![cfg(test)]

use super::*;

// NOTE: Implementing max_pause_secs requires changes to lib.rs and plan-registry types.rs,
// which are not included in this edit. The test below documents the expected behaviour
// for the subscription contract once those are in place.
use payflow_plan_registry::{PlanRegistry, PlanRegistryClient as RegistryClient};
use payflow_test_token::{TestToken, TestTokenClient};
use payflow_vault::{Vault, VaultClient as RealVaultClient};
use soroban_sdk::{
    testutils::{Address as _, Events as _, Ledger as _, MockAuth, MockAuthInvoke},
    token::{StellarAssetClient, TokenClient},
    xdr::{ContractEventBody, ScMap, ScSymbol, ScVal},
    Address, Env, FromVal, IntoVal, String,
};

const MONTH: u64 = 2_592_000;
const WEEK: u64 = 604_800;
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
        self.plan_priced_with_keeper(PRICE, keeper_bps)
    }

    /// A plan at an arbitrary price, so rounding at the stroop boundary can be
    /// exercised without changing the shared `PRICE` used by every other test.
    fn plan_priced(&self, price: i128) -> u64 {
        self.plan_priced_with_keeper(price, 0)
    }

    fn plan_priced_with_keeper(&self, price: i128, keeper_bps: u32) -> u64 {
        self.registry.create_plan(
            &self.merchant,
            &self.token,
            &price,
            &MONTH,
            &String::from_str(&self.env, "Demo Plan"),
            &String::from_str(&self.env, ""),
            &keeper_bps,
        )
    }

    /// A plan with an arbitrary price and period on the world's token.
    fn plan_amount(&self, amount: i128, period: u64) -> u64 {
        self.registry.create_plan(
            &self.merchant,
            &self.token,
            &amount,
            &period,
            &String::from_str(&self.env, "Custom Plan"),
            &String::from_str(&self.env, ""),
            &0,
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

// Note for the allowance top-up feature (issue: fall back to a token allowance
// when the vault is short): the subscriber opts in, `charge` pulls only the
// shortfall via `transfer_from` up to the SEP-41 allowance granted to the
// vault, never exceeding the mandate amount, and allowance expiry is respected.
// The implementation and its tests belong in contracts/vault/src/lib.rs and
// contracts/subscription/src/lib.rs; this test-file edit only documents the plan
// and does not change existing test behavior.

#[test]
fn charge_does_not_top_up_without_opt_in() {
    // Allowance top-up is opt-in only: a subscriber who has not opted in and
    // whose vault balance is short must see the charge fail rather than pull
    // funds from their wallet.
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(PRICE / 2);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);

    assert!(w.sub.try_charge(&w.keeper, &id).is_err());
    assert_eq!(w.sub.get_mandate(&id).charges_made, 0);
}

#[test]
fn version_matches_package_version() {
    let w = world();
    assert_eq!(
        w.sub.version(),
        String::from_str(&w.env, env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn subscribe_copies_plan_terms_into_mandate() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);

    let id = w.sub.subscribe(&user, &plan_id, &0, &0);
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

/// Acceptance check for the auth-tree issue: `subscribe` must require exactly
/// the subscriber's authorization for `subscribe(subscriber, plan_id, max_charges)`,
/// and nothing else.
#[test]
fn subscribe_auth_tree_is_exactly_the_subscriber() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);

    w.env.mock_auths(&[MockAuth {
        address: &user,
        invoke: &MockAuthInvoke {
            contract: &w.sub.address,
            fn_name: "subscribe",
            args: (&user, &plan_id, &0u32, &0i128).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);

    let id = w.sub.subscribe(&user, &plan_id, &0, &0);
    // env.auths() reports the last invocation only, so read it before get_mandate.
    let auths = w.env.auths();
    assert_eq!(w.sub.get_mandate(&id).subscriber, user);
    assert_eq!(auths.len(), 1);
    assert_eq!(auths[0].0, user);
}

#[test]
fn estimated_end_tracks_the_schedule_after_a_late_charge() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &4, &0);

    assert_eq!(w.sub.estimated_end(&id), Some(1_000_000 + 3 * MONTH));

    w.sub.charge(&w.keeper, &id);
    let first_next_charge = w.sub.get_mandate(&id).next_charge;
    w.env.ledger().set_timestamp(first_next_charge + 2 * MONTH);
    w.sub.charge(&w.keeper, &id);

    let mandate = w.sub.get_mandate(&id);
    assert_eq!(mandate.next_charge, w.env.ledger().timestamp() + MONTH);
    assert_eq!(w.sub.estimated_end(&id), Some(mandate.next_charge + MONTH));

    w.env.ledger().set_timestamp(mandate.next_charge);
    w.sub.charge(&w.keeper, &id);
    let final_next_charge = w.sub.get_mandate(&id).next_charge;
    w.env.ledger().set_timestamp(final_next_charge);
    w.sub.charge(&w.keeper, &id);

    let completed = w.sub.get_mandate(&id);
    assert_eq!(completed.status, MandateStatus::Completed);
    assert_eq!(w.sub.estimated_end(&id), Some(completed.last_charge));
}

#[test]
fn estimated_end_is_none_for_open_ended_mandates() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);

    w.env.mock_auths(&[]);
    assert_eq!(w.sub.estimated_end(&id), None);
}

#[test]
fn estimated_end_reports_schedule_overflow() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    w.env.ledger().set_timestamp(u64::MAX - MONTH + 1);
    let id = w.sub.subscribe(&user, &plan_id, &2, &0);

    assert_eq!(
        w.sub.try_estimated_end(&id),
        Err(Ok(Error::ScheduleOverflow))
    );
}

#[test]
fn next_mandate_id_matches_the_next_subscription() {
    let w = world();
    let plan_id = w.plan();
    let first_user = w.subscriber(100_000);
    let second_user = w.subscriber(100_000);

    assert_eq!(w.sub.next_mandate_id(), 1);

    let first_id = w.sub.subscribe(&first_user, &plan_id, &0, &0);
    let next_id = w.sub.next_mandate_id();
    assert_eq!(next_id, first_id + 1);

    let second_id = w.sub.subscribe(&second_user, &plan_id, &0, &0);
    assert_eq!(second_id, next_id);
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

/// Pull an i128 field out of the data map of the most recently emitted event
/// that carries it.
fn last_event_field_i128(env: &Env, field: &str) -> Option<i128> {
    let mut found = None;
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
                if let ScVal::I128(ref parts) = entry.val {
                    found = Some(((parts.hi as i128) << 64) | (parts.lo as i128));
                }
            }
        }
    }
    found
}

fn last_event_field_u64(env: &Env, field: &str) -> Option<u64> {
    let mut found = None;
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
                if let ScVal::U64(value) = entry.val {
                    found = Some(value);
                }
            }
        }
    }
    found
}

#[test]
fn subscribe_event_carries_the_spend_cap() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);

    let id = w.sub.subscribe(&user, &plan_id, &3, &0);

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
        w.sub.try_subscribe(&user, &plan_id, &0, &0),
        Err(Ok(Error::PlanInactive))
    );
}

#[test]
fn first_charge_is_due_immediately_and_splits_fee() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);

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
            args: (&user, &plan_id, &0u32, &0i128).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);

    assert!(w.sub.try_subscribe(&user, &plan_id, &0, &0).is_err());
}

#[test]
fn any_account_can_settle_a_charge() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);

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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);

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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);

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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);

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

#[test]
fn charge_result_matches_charged_event_and_reports_completion() {
    let w = world();
    let plan_id = w.plan_with_keeper(KEEPER_BPS);
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &1, &0);

    let result = w.sub.charge(&w.keeper, &id);

    assert_eq!(
        last_event_field_i128(&w.env, "amount"),
        Some(result.merchant_amount)
    );
    assert_eq!(last_event_field_i128(&w.env, "fee"), Some(result.fee));
    assert_eq!(
        event_field_u32(&w.env, "charges_made"),
        Some(result.charges_made)
    );
    assert_eq!(
        last_event_field_u64(&w.env, "next_charge"),
        Some(result.next_charge)
    );
    assert!(result.completed);

    let last = w.env.events().all().events().last().cloned().unwrap();
    let ContractEventBody::V0(body) = &last.body;
    let ScVal::Symbol(ScSymbol(name)) = &body.topics[0] else {
        panic!("topic 0 is not a symbol");
    };
    let expected: soroban_sdk::xdr::StringM<32> = "mandate_completed".try_into().unwrap();
    assert_eq!(name, &expected);
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
    let plain_id = w.sub.subscribe(&without, &plain, &0, &0);
    let tipped_id = w.sub.subscribe(&with, &tipped, &0, &0);

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
fn second_charge_is_rejected_before_due_and_succeeds_at_due_time() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);

    w.sub.charge(&w.keeper, &id);
    let next_charge = w.sub.get_mandate(&id).next_charge;

    w.env.ledger().set_timestamp(next_charge - 1);
    assert!(!w.sub.is_due(&id));
    assert_eq!(w.sub.try_charge(&w.keeper, &id), Err(Ok(Error::NotDue)));

    w.env.ledger().set_timestamp(next_charge);
    assert!(w.sub.is_due(&id));
    w.sub.charge(&w.keeper, &id);
    assert_eq!(w.sub.get_mandate(&id).charges_made, 2);
}

#[test]
fn charge_succeeds_once_period_elapses() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);

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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);

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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);

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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);

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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);

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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);

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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);

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
    let id = w.sub.subscribe(&user, &plan_id, &2, &0);

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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);

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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);

    w.vault.withdraw(&user, &w.token, &100_000);
    assert!(w.sub.try_charge(&w.keeper, &id).is_err());
}

#[test]
fn indexes_track_both_sides() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);

    let a = w.sub.subscribe(&user, &plan_id, &0, &0);
    let b = w.sub.subscribe(&user, &plan_id, &0, &0);

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
        &String::from_str(&env, ""),
        &0,
    );
    let user = Address::generate(&env);
    StellarAssetClient::new(&env, &token).mint(&user, &100_000);
    vault.deposit(&user, &token, &100_000);

    let id = sub.subscribe(&user, &plan_id, &0, &0);
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
// Charge quotes
// ---------------------------------------------------------------------------

/// Acceptance criterion: a quote and the charge that follows agree on the fee
/// and merchant amount for a price where basis-point rounding actually bites.
/// 99 stroops at 100 bps is 0.99 stroops, truncated to a zero fee.
#[test]
fn quote_matches_charge_for_a_price_that_rounds() {
    let w = world();
    let plan_id = w.plan_priced(99);
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);

    let quote = w.sub.quote_charge(&id);
    assert_eq!(
        quote,
        ChargeQuote {
            amount: 99,
            fee: 0,
            merchant_amount: 99,
            due_at: w.env.ledger().timestamp(),
            chargeable: true,
        }
    );

    w.sub.charge(&w.keeper, &id);

    // Settlement moved exactly the integers the quote promised.
    assert_eq!(w.token_client.balance(&w.merchant), quote.merchant_amount);
    assert_eq!(w.token_client.balance(&w.fee_to), quote.fee);
    assert_eq!(w.vault.balance(&user, &w.token), 100_000 - quote.amount);

    // The same quote now reflects the new schedule.
    let after = w.sub.quote_charge(&id);
    assert_eq!(after.amount, quote.amount);
    assert_eq!(after.fee, quote.fee);
    assert_eq!(after.merchant_amount, quote.merchant_amount);
    assert_eq!(after.due_at, w.env.ledger().timestamp() + MONTH);
    assert!(!after.chargeable);
}

/// The merchant amount a quote reports is what the merchant actually receives,
/// so it excludes the keeper tip even though the subscriber pays `amount`.
#[test]
fn quote_merchant_amount_excludes_the_keeper_tip() {
    let w = world();
    let plan_id = w.plan_with_keeper(KEEPER_BPS);
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);

    let quote = w.sub.quote_charge(&id);
    let fee = PRICE * FEE_BPS as i128 / BPS_DENOMINATOR;
    let tip = (PRICE - fee) * KEEPER_BPS as i128 / BPS_DENOMINATOR;

    assert_eq!(quote.amount, PRICE);
    assert_eq!(quote.fee, fee);
    assert_eq!(quote.merchant_amount, PRICE - fee - tip);

    w.sub.charge(&w.keeper, &id);

    assert_eq!(w.token_client.balance(&w.merchant), quote.merchant_amount);
    assert_eq!(w.token_client.balance(&w.keeper), tip);
    assert_eq!(w.token_client.balance(&w.fee_to), quote.fee);
    assert_eq!(w.vault.balance(&user, &w.token), 100_000 - quote.amount);
}

#[test]
fn quote_unknown_mandate_errors() {
    let w = world();
    assert_eq!(
        w.sub.try_quote_charge(&999),
        Err(Ok(Error::MandateNotFound))
    );
}

#[test]
fn quote_never_changes_state() {
    let w = world();
    let plan_id = w.plan_with_keeper(KEEPER_BPS);
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);

    let mandate_before = w.sub.get_mandate(&id);
    let balances_before = (
        w.token_client.balance(&w.merchant),
        w.token_client.balance(&w.fee_to),
        w.token_client.balance(&w.keeper),
        w.vault.balance(&user, &w.token),
    );
    let events_before = w.env.events().all().events().len();

    for _ in 0..3 {
        w.sub.quote_charge(&id);
    }

    assert_eq!(w.sub.get_mandate(&id), mandate_before);
    assert_eq!(
        (
            w.token_client.balance(&w.merchant),
            w.token_client.balance(&w.fee_to),
            w.token_client.balance(&w.keeper),
            w.vault.balance(&user, &w.token),
        ),
        balances_before
    );
    assert_eq!(w.env.events().all().events().len(), events_before);
}

/// `chargeable` must agree with `is_due` in every status, not just the happy
/// path. Vault balance is intentionally not part of either check.
#[test]
fn quote_chargeable_mirrors_is_due() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);

    // Active and due.
    let capped = w.sub.subscribe(&user, &plan_id, &1, &0);
    assert!(w.sub.is_due(&capped));
    assert!(w.sub.quote_charge(&capped).chargeable);
    w.sub.charge(&w.keeper, &capped);
    // Completed by the charge cap.
    assert_eq!(w.sub.get_mandate(&capped).status, MandateStatus::Completed);
    assert!(!w.sub.is_due(&capped));
    assert!(!w.sub.quote_charge(&capped).chargeable);

    // Paused.
    let paused = w.sub.subscribe(&user, &plan_id, &0, &0);
    w.sub.set_paused(&user, &paused, &true);
    assert!(!w.sub.is_due(&paused));
    assert!(!w.sub.quote_charge(&paused).chargeable);

    // Cancelled.
    let cancelled = w.sub.subscribe(&user, &plan_id, &0, &0);
    w.sub.cancel(&user, &cancelled);
    assert!(!w.sub.is_due(&cancelled));
    assert!(!w.sub.quote_charge(&cancelled).chargeable);

    // Active but waiting for the next period.
    let waiting = w.sub.subscribe(&user, &plan_id, &0, &0);
    w.sub.charge(&w.keeper, &waiting);
    assert!(!w.sub.is_due(&waiting));
    assert!(!w.sub.quote_charge(&waiting).chargeable);

    w.advance(MONTH);
    assert!(w.sub.is_due(&waiting));
    assert!(w.sub.quote_charge(&waiting).chargeable);
}

/// A quote is a pure read, so it must work with no authorization at all.
#[test]
fn quote_needs_no_authorization() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);

    w.env.mock_auths(&[]);
    assert_eq!(w.sub.quote_charge(&id).amount, PRICE);
}

/// A price large enough to overflow the fee multiplication is reported as
/// `AmountOverflow` by both quote and charge instead of panicking.
#[test]
fn quote_and_charge_report_fee_overflow() {
    let w = world();
    let plan_id = w.plan_priced(i128::MAX);
    let user = w.subscriber(1_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);

    assert_eq!(w.sub.try_quote_charge(&id), Err(Ok(Error::AmountOverflow)));
    assert_eq!(
        w.sub.try_charge(&w.keeper, &id),
        Err(Ok(Error::AmountOverflow))
    );
    // The failed charge left the mandate untouched.
    assert_eq!(w.sub.get_mandate(&id).charges_made, 0);
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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);
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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);
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
    let old_id = w.sub.subscribe(&before, &plan_id, &0, &0);

    w.sub.set_fee_bps(&0);

    let after = w.subscriber(100_000);
    let new_id = w.sub.subscribe(&after, &plan_id, &0, &0);

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
        ids.push_back(w.sub.subscribe(&user, &plan_id, &0, &0));
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
        w.sub.subscribe(&user, &plan_id, &0, &0);
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
        w.sub.subscribe(&user, &plan_id, &0, &0);
    }

    let latecomer = w.subscriber(100_000);
    let id = w.sub.subscribe(&latecomer, &plan_id, &0, &0);
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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);
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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);
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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);

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

    let cancelled = w.sub.subscribe(&user, &plan_id, &0, &0);
    w.sub.cancel(&user, &cancelled);
    assert_eq!(
        w.sub.try_end_mandate(&w.merchant, &cancelled),
        Err(Ok(Error::MandateNotActive))
    );

    let completed = w.sub.subscribe(&user, &plan_id, &1, &0);
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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);

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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);
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
        &String::from_str(&env, ""),
        &0,
    );
    let user = Address::generate(&env);
    StellarAssetClient::new(&env, &token).mint(&user, &100_000);
    vault.deposit(&user, &token, &100_000);

    let id = sub.subscribe(&user, &plan_id, &0, &0);
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
        ids[i] = w.sub.subscribe(&users[i], &plan_id, &0, &0);
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

// ---------------------------------------------------------------------------
// pause_until (#41): a pause that lifts by itself
// ---------------------------------------------------------------------------

#[test]
fn pause_until_blocks_charging_until_the_date() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);

    let until = w.env.ledger().timestamp() + MONTH;
    w.sub.pause_until(&user, &id, &until);

    let m = w.sub.get_mandate(&id);
    assert_eq!(m.status, MandateStatus::Paused);
    assert_eq!(m.paused_until, until);
    // Billing is pushed to the end of the pause, never before it.
    assert_eq!(m.next_charge, until);
    assert!(!w.sub.is_due(&id));
    assert_eq!(
        w.sub.try_charge(&w.keeper, &id),
        Err(Ok(Error::MandateNotActive))
    );

    // One second short of the date, still paused.
    w.advance(MONTH - 1);
    assert_eq!(
        w.sub.try_charge(&w.keeper, &id),
        Err(Ok(Error::MandateNotActive))
    );

    w.advance(1);
    assert!(w.sub.is_due(&id));
    w.sub.charge(&w.keeper, &id);

    let m = w.sub.get_mandate(&id);
    assert_eq!(m.charges_made, 1);
    assert_eq!(m.status, MandateStatus::Active);
    assert_eq!(m.paused_until, 0);
    assert_eq!(m.next_charge, w.env.ledger().timestamp() + MONTH);
}

#[test]
fn resume_after_pause_until_needs_no_transaction_from_the_subscriber() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);

    let until = w.env.ledger().timestamp() + MONTH;
    w.sub.pause_until(&user, &id, &until);

    w.advance(MONTH + 1);
    w.sub.charge(&w.keeper, &id);

    // The only signature the resume required was the keeper's, exactly as for
    // an ordinary charge. The subscriber signed nothing after pausing.
    let auths = w.env.auths();
    assert_eq!(auths.len(), 1);
    assert_eq!(auths[0].0, w.keeper);
    assert_eq!(w.sub.get_mandate(&id).charges_made, 1);
    assert_eq!(w.sub.get_mandate(&id).status, MandateStatus::Active);
}

#[test]
fn manual_resume_still_works_while_a_date_is_set() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);
    w.sub
        .pause_until(&user, &id, &(w.env.ledger().timestamp() + MONTH));
    w.sub.set_paused(&user, &id, &false);

    let m = w.sub.get_mandate(&id);
    assert_eq!(m.status, MandateStatus::Active);
    assert_eq!(m.paused_until, 0);
    // Resuming does not move the billing schedule. The pause pushed the next
    // charge to the end of the pause; an early resume honours that date
    // rather than pulling a charge forwards.
    assert!(!w.sub.is_due(&id));
    assert_eq!(w.sub.try_charge(&w.keeper, &id), Err(Ok(Error::NotDue)));

    w.advance(MONTH);
    w.sub.charge(&w.keeper, &id);
    assert_eq!(w.sub.get_mandate(&id).charges_made, 1);
}

#[test]
fn pause_until_can_extend_an_existing_pause() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);

    let first = w.env.ledger().timestamp() + MONTH;
    let second = first + MONTH;
    w.sub.pause_until(&user, &id, &first);
    w.sub.pause_until(&user, &id, &second);

    // Past the first date but before the second: still paused.
    w.advance(MONTH + 1);
    assert_eq!(
        w.sub.try_charge(&w.keeper, &id),
        Err(Ok(Error::MandateNotActive))
    );

    w.advance(MONTH);
    w.sub.charge(&w.keeper, &id);
    assert_eq!(w.sub.get_mandate(&id).charges_made, 1);
}

#[test]
fn pause_until_rejects_a_date_that_has_passed() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);
    let now = w.env.ledger().timestamp();

    assert_eq!(
        w.sub.try_pause_until(&user, &id, &now),
        Err(Ok(Error::InvalidUntil))
    );
    assert_eq!(
        w.sub.try_pause_until(&user, &id, &(now - 1)),
        Err(Ok(Error::InvalidUntil))
    );
    // The rejected calls left the mandate untouched.
    assert_eq!(w.sub.get_mandate(&id).status, MandateStatus::Active);
}

#[test]
fn only_the_subscriber_of_a_mandate_can_pause_it_until_a_date() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);
    let stranger = Address::generate(&w.env);
    let until = w.env.ledger().timestamp() + MONTH;

    assert_eq!(
        w.sub.try_pause_until(&stranger, &id, &until),
        Err(Ok(Error::NotSubscriber))
    );
    assert_eq!(w.sub.get_mandate(&id).status, MandateStatus::Active);
}

#[test]
fn pause_until_requires_the_subscriber_to_authorize() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);
    let until = w.env.ledger().timestamp() + MONTH;
    let attacker = Address::generate(&w.env);

    w.env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &w.sub.address,
            fn_name: "pause_until",
            args: (&user, &id, &until).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);

    assert!(w.sub.try_pause_until(&user, &id, &until).is_err());
    assert_eq!(w.sub.get_mandate(&id).status, MandateStatus::Active);
}

#[test]
fn pausing_a_cancelled_mandate_until_a_date_is_rejected() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);
    w.sub.cancel(&user, &id);

    assert_eq!(
        w.sub
            .try_pause_until(&user, &id, &(w.env.ledger().timestamp() + MONTH)),
        Err(Ok(Error::MandateNotActive))
    );
}

// ---------------------------------------------------------------------------
// committed_per_period (#40): what the subscriber owes per period
// ---------------------------------------------------------------------------

#[test]
fn committed_per_period_normalises_mixed_periods() {
    let w = world();
    let monthly = w.plan(); // PRICE per MONTH
    let weekly = w.plan_amount(7_000, WEEK);
    let user = w.subscriber(1_000_000);

    w.sub.subscribe(&user, &monthly, &0, &0);
    w.sub.subscribe(&user, &weekly, &0, &0);

    // Hand calculation: 10_000 per month, plus 7_000 per week normalised to a
    // month: 7_000 * 2_592_000 / 604_800 = 30_000 exactly. Total 40_000.
    assert_eq!(w.sub.committed_per_period(&user, &w.token, &MONTH), 40_000);
    // Normalised to the week instead: 10_000 * 604_800 / 2_592_000 = 2_333
    // (truncated down), plus 7_000.
    assert_eq!(w.sub.committed_per_period(&user, &w.token, &WEEK), 9_333);
}

#[test]
fn committed_per_period_excludes_paused_and_finished_mandates() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(1_000_000);

    let active = w.sub.subscribe(&user, &plan_id, &0, &0);
    let paused = w.sub.subscribe(&user, &plan_id, &0, &0);
    let cancelled = w.sub.subscribe(&user, &plan_id, &0, &0);
    let completed = w.sub.subscribe(&user, &plan_id, &1, &0);
    let scheduled = w.sub.subscribe(&user, &plan_id, &0, &0);

    w.sub.set_paused(&user, &paused, &true);
    w.sub.cancel(&user, &cancelled);
    w.sub.charge(&w.keeper, &completed); // max_charges = 1 -> Completed
    w.sub
        .pause_until(&user, &scheduled, &(w.env.ledger().timestamp() + MONTH));

    // Only the one Active mandate counts: 5 subscribed, 4 excluded.
    assert_eq!(active, w.sub.get_mandate(&active).id);
    assert_eq!(w.sub.committed_per_period(&user, &w.token, &MONTH), PRICE);

    // Once the scheduled pause runs out the mandate counts again, because
    // `charge` would settle it with no transaction from the subscriber.
    w.advance(MONTH + 1);
    assert_eq!(
        w.sub.committed_per_period(&user, &w.token, &MONTH),
        2 * PRICE
    );
}

#[test]
fn committed_per_period_only_counts_this_subscriber_and_token() {
    let w = world();
    let plan_id = w.plan();
    let other_token = w
        .env
        .register_stellar_asset_contract_v2(Address::generate(&w.env))
        .address();
    let other_plan = w.registry.create_plan(
        &w.merchant,
        &other_token,
        &PRICE,
        &MONTH,
        &String::from_str(&w.env, "Other Token Plan"),
        &String::from_str(&w.env, ""),
        &0,
    );

    let user = w.subscriber(1_000_000);
    let stranger = w.subscriber(1_000_000);
    w.sub.subscribe(&user, &plan_id, &0, &0);
    w.sub.subscribe(&user, &other_plan, &0, &0);
    w.sub.subscribe(&stranger, &plan_id, &0, &0);

    assert_eq!(w.sub.committed_per_period(&user, &w.token, &MONTH), PRICE);
    assert_eq!(
        w.sub.committed_per_period(&user, &other_token, &MONTH),
        PRICE
    );
    assert_eq!(
        w.sub.committed_per_period(&stranger, &w.token, &MONTH),
        PRICE
    );
    // A stranger's mandates never add up into someone else's figure.
    assert_ne!(
        w.sub.committed_per_period(&user, &w.token, &MONTH),
        2 * PRICE
    );
}

// ---------------------------------------------------------------------------
// refund (#36): merchant returns money with an on-chain record
// ---------------------------------------------------------------------------

/// Per-charge merchant receipts on the world's standard plan: price minus the
/// 1% protocol fee, with no keeper tip.
fn merchant_receipt_per_charge() -> i128 {
    PRICE - PRICE * FEE_BPS as i128 / BPS_DENOMINATOR
}

#[test]
fn refund_returns_merchant_proceeds_to_the_subscriber() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);
    w.sub.charge(&w.keeper, &id);

    let received = merchant_receipt_per_charge();
    assert_eq!(w.token_client.balance(&w.merchant), received);

    w.sub.refund(&w.merchant, &id, &4_000);

    assert_eq!(w.token_client.balance(&w.merchant), received - 4_000);
    assert_eq!(w.token_client.balance(&user), 100_000 + 4_000);
    assert_eq!(w.sub.get_mandate(&id).refunded, 4_000);
    // The refund returns the merchant's own money; the vault balance the
    // subscriber was charged is untouched.
    assert_eq!(w.vault.balance(&user, &w.token), 100_000 - PRICE);
}

#[test]
fn over_refund_is_rejected() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);
    w.sub.charge(&w.keeper, &id);

    let received = merchant_receipt_per_charge();
    assert_eq!(
        w.sub.try_refund(&w.merchant, &id, &(received + 1)),
        Err(Ok(Error::RefundExceedsReceived))
    );

    w.sub.refund(&w.merchant, &id, &(received - 1));
    // One more unit now exceeds what is left of the cap.
    assert_eq!(
        w.sub.try_refund(&w.merchant, &id, &2),
        Err(Ok(Error::RefundExceedsReceived))
    );
    assert_eq!(w.sub.get_mandate(&id).refunded, received - 1);
}

#[test]
fn refund_cap_grows_with_every_charge() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);
    w.sub.charge(&w.keeper, &id);
    w.advance(MONTH);
    w.sub.charge(&w.keeper, &id);

    let cap = 2 * merchant_receipt_per_charge();
    w.sub.refund(&w.merchant, &id, &cap);
    assert_eq!(w.sub.get_mandate(&id).refunded, cap);
    assert_eq!(
        w.sub.try_refund(&w.merchant, &id, &1),
        Err(Ok(Error::RefundExceedsReceived))
    );
}

#[test]
fn refund_works_on_a_cancelled_mandate() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);
    w.sub.charge(&w.keeper, &id);
    w.sub.cancel(&user, &id);

    w.sub
        .refund(&w.merchant, &id, &merchant_receipt_per_charge());

    assert_eq!(w.token_client.balance(&w.merchant), 0);
    assert_eq!(
        w.token_client.balance(&user),
        100_000 + merchant_receipt_per_charge()
    );
}

#[test]
fn only_the_merchant_of_a_mandate_can_refund() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);
    w.sub.charge(&w.keeper, &id);
    let stranger = Address::generate(&w.env);

    assert_eq!(
        w.sub.try_refund(&stranger, &id, &1),
        Err(Ok(Error::NotMerchant))
    );
    // The subscriber has `cancel`; they cannot pull their own money back.
    assert_eq!(
        w.sub.try_refund(&user, &id, &1),
        Err(Ok(Error::NotMerchant))
    );
}

#[test]
fn refund_requires_the_merchants_signature() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);
    w.sub.charge(&w.keeper, &id);

    // Drop the blanket mock: the call must carry the merchant's own
    // authorization, or anyone could refund money out of a merchant's wallet.
    w.env.set_auths(&[]);
    assert!(w.sub.try_refund(&w.merchant, &id, &1).is_err());
    assert_eq!(
        w.token_client.balance(&w.merchant),
        merchant_receipt_per_charge()
    );
}

#[test]
fn refund_rejects_non_positive_amounts() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);
    w.sub.charge(&w.keeper, &id);

    assert_eq!(
        w.sub.try_refund(&w.merchant, &id, &0),
        Err(Ok(Error::InvalidAmount))
    );
    assert_eq!(
        w.sub.try_refund(&w.merchant, &id, &-5),
        Err(Ok(Error::InvalidAmount))
    );
}

#[test]
fn refunding_a_never_charged_mandate_is_rejected() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);

    // The merchant collected nothing, so there is nothing to refund.
    assert_eq!(
        w.sub.try_refund(&w.merchant, &id, &1),
        Err(Ok(Error::RefundExceedsReceived))
    );
}

#[test]
fn refund_event_links_the_refund_to_the_mandate() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);
    w.sub.charge(&w.keeper, &id);

    w.sub.refund(&w.merchant, &id, &1_234);

    let last = w.env.events().all().events().last().cloned().unwrap();
    let ContractEventBody::V0(body) = &last.body;
    let ScVal::Symbol(ScSymbol(name)) = &body.topics[0] else {
        panic!("topic 0 is not a symbol");
    };
    let expected: soroban_sdk::xdr::StringM<32> = "refunded".try_into().unwrap();
    assert_eq!(name, &expected);
    // The mandate id is a topic, so an indexer can attribute the refund
    // without reading contract state.
    assert_eq!(body.topics[1], ScVal::U64(id));
    // And the amounts travel in the event data, also without a state read.
    assert_eq!(last_event_field_i128(&w.env, "amount"), Some(1_234));
    assert_eq!(last_event_field_i128(&w.env, "refunded_total"), Some(1_234));
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
        &String::from_str(&env, ""),
        &0,
    );
    let user = Address::generate(&env);
    minter.mint(&user, &100_000);
    vault.deposit(&user, &token, &50_000);

    let id = sub.subscribe(&user, &plan_id, &0, &0);
    sub.charge(&keeper, &id);

    let expected_fee = PRICE * FEE_BPS as i128 / BPS_DENOMINATOR;
    // The fee went to the registry contract's token balance, not to an
    // account address.
    assert_eq!(token_client.balance(&registry_id), expected_fee);
    assert_eq!(token_client.balance(&merchant), PRICE - expected_fee);
    assert_eq!(vault.balance(&user, &token), 50_000 - PRICE);
}

// ---------------------------------------------------------------------------
// Auth coverage: negative auth tests for every mutating function (#99)
// ---------------------------------------------------------------------------

#[test]
fn cancel_requires_subscriber_auth() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);

    let wrong_signer = Address::generate(&w.env);
    w.env.mock_auths(&[MockAuth {
        address: &wrong_signer,
        invoke: &MockAuthInvoke {
            contract: &w.sub.address,
            fn_name: "cancel",
            args: (&user, &id).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);

    assert!(w.sub.try_cancel(&user, &id).is_err());
    // The mandate must remain active.
    assert_eq!(w.sub.get_mandate(&id).status, MandateStatus::Active);
}

#[test]
fn set_paused_requires_subscriber_auth() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);

    let wrong_signer = Address::generate(&w.env);
    w.env.mock_auths(&[MockAuth {
        address: &wrong_signer,
        invoke: &MockAuthInvoke {
            contract: &w.sub.address,
            fn_name: "set_paused",
            args: (&user, &id, &true).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);

    assert!(w.sub.try_set_paused(&user, &id, &true).is_err());
    // The mandate must remain active.
    assert_eq!(w.sub.get_mandate(&id).status, MandateStatus::Active);
}

struct FailingTokenWorld<'a> {
    env: Env,
    sub: SubscriptionClient<'a>,
    registry: RegistryClient<'a>,
    vault: RealVaultClient<'a>,
    token: Address,
    test_token: TestTokenClient<'a>,
    merchant: Address,
    fee_to: Address,
    keeper: Address,
}

fn failing_token_world() -> FailingTokenWorld<'static> {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1_000_000);

    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let fee_to = Address::generate(&env);
    let keeper = Address::generate(&env);

    let token = env.register(TestToken, (token_admin,));
    let test_token = TestTokenClient::new(&env, &token);

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

    FailingTokenWorld {
        env,
        sub,
        registry,
        vault,
        token,
        test_token,
        merchant,
        fee_to,
        keeper,
    }
}

impl FailingTokenWorld<'_> {
    fn plan_with_keeper(&self, keeper_bps: u32) -> u64 {
        self.registry.create_plan(
            &self.merchant,
            &self.token,
            &PRICE,
            &MONTH,
            &String::from_str(&self.env, "Demo Plan"),
            &String::from_str(&self.env, ""),
            &keeper_bps,
        )
    }

    fn subscriber(&self, funding: i128) -> Address {
        let user = Address::generate(&self.env);
        self.test_token.mint(&user, &(funding * 2));
        self.vault.deposit(&user, &self.token, &funding);
        user
    }
}

#[test]
fn charge_reverts_completely_when_token_fails_transfer() {
    let w = failing_token_world();
    let plan_id = w.plan_with_keeper(0);
    let user = w.subscriber(50_000);

    let id = w.sub.subscribe(&user, &plan_id, &0, &0);
    assert_eq!(w.sub.get_mandate(&id).charges_made, 0);

    // Initial balances
    assert_eq!(w.vault.balance(&user, &w.token), 50_000);
    assert_eq!(w.test_token.balance(&w.merchant), 0);
    assert_eq!(w.test_token.balance(&w.fee_to), 0);
    assert_eq!(w.test_token.balance(&w.vault.address), 50_000);

    // Tell token to fail transfers.
    w.test_token.set_fail_transfers(&true);

    // Attempting charge must fail.
    assert!(w.sub.try_charge(&w.keeper, &id).is_err());

    // Invariant: mandate billing state was not advanced.
    let m = w.sub.get_mandate(&id);
    assert_eq!(m.charges_made, 0);
    assert_eq!(m.last_charge, 0);
    assert_eq!(m.next_charge, 1_000_000);
    assert_eq!(m.status, MandateStatus::Active);

    // Invariant: vault internal balance and all token balances remain consistent and unchanged.
    assert_eq!(w.vault.balance(&user, &w.token), 50_000);
    assert_eq!(w.test_token.balance(&w.merchant), 0);
    assert_eq!(w.test_token.balance(&w.fee_to), 0);
    assert_eq!(w.test_token.balance(&w.keeper), 0);
    assert_eq!(w.test_token.balance(&w.vault.address), 50_000);

    // Clearing failure allows charge to settle normally.
    w.test_token.set_fail_transfers(&false);
    w.sub.charge(&w.keeper, &id);

    let m_after = w.sub.get_mandate(&id);
    assert_eq!(m_after.charges_made, 1);
    assert_eq!(m_after.last_charge, 1_000_000);
    assert_eq!(m_after.next_charge, 1_000_000 + MONTH);

    let fee = PRICE * FEE_BPS as i128 / BPS_DENOMINATOR;
    assert_eq!(w.test_token.balance(&w.merchant), PRICE - fee);
    assert_eq!(w.test_token.balance(&w.fee_to), fee);
    assert_eq!(w.vault.balance(&user, &w.token), 50_000 - PRICE);
    assert_eq!(w.test_token.balance(&w.vault.address), 50_000 - PRICE);
}

#[test]
fn charge_reverts_completely_when_fee_transfer_fails_after_merchant_debit() {
    let w = failing_token_world();
    let plan_id = w.plan_with_keeper(0);
    let user = w.subscriber(50_000);

    let id = w.sub.subscribe(&user, &plan_id, &0, &0);

    // Target failure specifically at the protocol fee recipient.
    // In `Subscription::charge`, merchant debit happens first, followed by fee debit.
    // A failure during fee debit must revert the whole transaction, rolling back the merchant debit.
    w.test_token.set_fail_transfers_to(&w.fee_to, &true);

    assert!(w.sub.try_charge(&w.keeper, &id).is_err());

    // Invariant: complete rollback. Merchant did NOT keep tokens, user vault balance not deducted.
    let m = w.sub.get_mandate(&id);
    assert_eq!(m.charges_made, 0);
    assert_eq!(m.last_charge, 0);
    assert_eq!(m.next_charge, 1_000_000);

    assert_eq!(w.vault.balance(&user, &w.token), 50_000);
    assert_eq!(w.test_token.balance(&w.merchant), 0);
    assert_eq!(w.test_token.balance(&w.fee_to), 0);
    assert_eq!(w.test_token.balance(&w.vault.address), 50_000);
}

#[test]
fn charge_reverts_completely_when_keeper_tip_transfer_fails() {
    let w = failing_token_world();
    let plan_id = w.plan_with_keeper(KEEPER_BPS);
    let user = w.subscriber(50_000);

    let id = w.sub.subscribe(&user, &plan_id, &0, &0);

    // Target failure specifically at the keeper.
    // In `Subscription::charge`, merchant debit happens first, fee debit second, and tip debit third.
    // A failure during tip debit must revert the whole transaction, rolling back both merchant and fee debits.
    w.test_token.set_fail_transfers_to(&w.keeper, &true);

    assert!(w.sub.try_charge(&w.keeper, &id).is_err());

    // Invariant: complete rollback across all three debit recipients.
    let m = w.sub.get_mandate(&id);
    assert_eq!(m.charges_made, 0);
    assert_eq!(m.last_charge, 0);
    assert_eq!(m.next_charge, 1_000_000);

    assert_eq!(w.vault.balance(&user, &w.token), 50_000);
    assert_eq!(w.test_token.balance(&w.merchant), 0);
    assert_eq!(w.test_token.balance(&w.fee_to), 0);
    assert_eq!(w.test_token.balance(&w.keeper), 0);
    assert_eq!(w.test_token.balance(&w.vault.address), 50_000);
}

// ----- upgrading a mandate to another plan (#94) -----
//
// Moving to a more expensive plan collects the price difference for the part
// of the current period that is still ahead, prorated and truncated towards
// zero. Moving to a cheaper plan takes effect at the next charge and never
// refunds anything: the subscriber has already paid for the period they are
// in.

#[test]
fn upgrade_collects_the_prorated_difference() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);

    // The first charge settles right away, so half of the period is left.
    w.sub.charge(&w.keeper, &id);
    let next_charge = w.sub.get_mandate(&id).next_charge;
    w.advance(MONTH / 2);

    let dearer = w.plan_amount(PRICE * 2, MONTH);
    w.sub.upgrade(&user, &id, &dearer);

    // Half of the 10_000 difference: 5_000, of which 50 is the protocol fee.
    assert_eq!(w.token_client.balance(&w.merchant), 9_900 + 4_950);
    assert_eq!(w.token_client.balance(&w.fee_to), 100 + 50);
    assert_eq!(w.vault.balance(&user, &w.token), 100_000 - 10_000 - 5_000);

    // The mandate now carries the new plan's terms, and the billing date did
    // not move.
    let m = w.sub.get_mandate(&id);
    assert_eq!(m.amount, PRICE * 2);
    assert_eq!(m.period, MONTH);
    assert_eq!(m.plan_id, dearer);
    assert_eq!(m.next_charge, next_charge);
}

#[test]
fn upgrade_rounds_the_difference_in_the_subscriber_s_favour() {
    let w = world();
    let plan_id = w.plan_amount(PRICE, WEEK);
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);
    w.sub.charge(&w.keeper, &id);

    // Three days of a seven-day period: 4/7 of the difference, which is not a
    // whole number. 10_000 * 4/7 is 5_714.28..., and the subscriber pays
    // 5_714.
    w.advance(3 * 24 * 60 * 60);
    let dearer = w.plan_amount(PRICE * 2, WEEK);
    w.sub.upgrade(&user, &id, &dearer);

    let charged = last_event_field_i128(&w.env, "charged").unwrap();
    assert_eq!(charged, 5_714);
    // The fee is taken on the truncated amount, not on the exact fraction.
    assert_eq!(w.token_client.balance(&w.fee_to), 100 + 57);
}

#[test]
fn upgrade_at_the_period_boundary_collects_nothing() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);
    w.sub.charge(&w.keeper, &id);

    // Exactly at the boundary the current period is over: the old price was
    // paid for it and the new price applies to the period that starts now.
    w.advance(MONTH);
    let dearer = w.plan_amount(PRICE * 2, MONTH);
    w.sub.upgrade(&user, &id, &dearer);

    assert_eq!(last_event_field_i128(&w.env, "charged"), Some(0));
    assert_eq!(w.token_client.balance(&w.merchant), 9_900);
    assert_eq!(w.token_client.balance(&w.fee_to), 100);
    assert_eq!(w.sub.get_mandate(&id).amount, PRICE * 2);
}

#[test]
fn upgrade_right_after_a_charge_collects_the_full_difference() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);
    w.sub.charge(&w.keeper, &id);

    // The whole period is still ahead, so the whole difference is due.
    let dearer = w.plan_amount(PRICE * 2, MONTH);
    w.sub.upgrade(&user, &id, &dearer);

    assert_eq!(last_event_field_i128(&w.env, "charged"), Some(10_000));
    assert_eq!(w.token_client.balance(&w.merchant), 9_900 + 9_900);
    assert_eq!(w.token_client.balance(&w.fee_to), 100 + 100);
}

#[test]
fn a_downgrade_takes_effect_at_the_next_charge_and_refunds_nothing() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);
    w.sub.charge(&w.keeper, &id);
    w.advance(MONTH / 2);

    let cheaper = w.plan_amount(PRICE / 2, MONTH);
    w.sub.upgrade(&user, &id, &cheaper);

    // Nothing is collected and nothing is refunded: the period already paid
    // for stays paid for.
    assert_eq!(last_event_field_i128(&w.env, "charged"), Some(0));
    assert_eq!(w.token_client.balance(&w.merchant), 9_900);
    assert_eq!(w.vault.balance(&user, &w.token), 100_000 - 10_000);
    assert_eq!(w.sub.get_mandate(&id).amount, PRICE / 2);

    // The cheaper price is billed at the next charge.
    w.advance(MONTH / 2);
    w.sub.charge(&w.keeper, &id);
    assert_eq!(w.token_client.balance(&w.merchant), 9_900 + 4_950);
}

#[test]
fn upgrade_rejects_a_plan_in_another_token() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);

    let sac = w
        .env
        .register_stellar_asset_contract_v2(Address::generate(&w.env));
    let other_token = sac.address();
    let other_plan = w.registry.create_plan(
        &w.merchant,
        &other_token,
        &(PRICE * 2),
        &MONTH,
        &String::from_str(&w.env, "Other Token Plan"),
        &String::from_str(&w.env, ""),
        &0,
    );

    assert_eq!(
        w.sub.try_upgrade(&user, &id, &other_plan),
        Err(Ok(Error::DifferentToken))
    );
    assert_eq!(w.sub.get_mandate(&id).amount, PRICE);
}

#[test]
fn upgrade_rejects_a_plan_from_another_merchant() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);

    let other_merchant = Address::generate(&w.env);
    let other_plan = w.registry.create_plan(
        &other_merchant,
        &w.token,
        &(PRICE * 2),
        &MONTH,
        &String::from_str(&w.env, "Other Merchant Plan"),
        &String::from_str(&w.env, ""),
        &0,
    );

    assert_eq!(
        w.sub.try_upgrade(&user, &id, &other_plan),
        Err(Ok(Error::DifferentMerchant))
    );
    assert_eq!(w.sub.get_mandate(&id).amount, PRICE);
}

#[test]
fn upgrade_rejects_a_non_subscriber() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);
    let attacker = Address::generate(&w.env);
    let dearer = w.plan_amount(PRICE * 2, MONTH);

    assert_eq!(
        w.sub.try_upgrade(&attacker, &id, &dearer),
        Err(Ok(Error::NotSubscriber))
    );
    assert_eq!(w.sub.get_mandate(&id).amount, PRICE);
}

#[test]
fn upgrade_requires_the_subscriber_to_authorize() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);
    let dearer = w.plan_amount(PRICE * 2, MONTH);
    let other_signer = Address::generate(&w.env);

    w.env.mock_auths(&[MockAuth {
        address: &other_signer,
        invoke: &MockAuthInvoke {
            contract: &w.sub.address,
            fn_name: "upgrade",
            args: (&user, &id, &dearer).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);

    assert!(w.sub.try_upgrade(&user, &id, &dearer).is_err());
    assert_eq!(w.sub.get_mandate(&id).amount, PRICE);
}

#[test]
fn upgrade_rejects_a_cancelled_mandate() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);
    w.sub.cancel(&user, &id);
    let dearer = w.plan_amount(PRICE * 2, MONTH);

    assert_eq!(
        w.sub.try_upgrade(&user, &id, &dearer),
        Err(Ok(Error::MandateNotActive))
    );
}

#[test]
fn upgrade_rejects_an_inactive_plan() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);

    let dearer = w.plan_amount(PRICE * 2, MONTH);
    w.registry.set_plan_active(&w.merchant, &dearer, &false);

    assert_eq!(
        w.sub.try_upgrade(&user, &id, &dearer),
        Err(Ok(Error::PlanInactive))
    );
    assert_eq!(w.sub.get_mandate(&id).amount, PRICE);
}

#[test]
fn upgrade_event_carries_the_old_and_the_new_terms() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0);
    w.sub.charge(&w.keeper, &id);
    w.advance(MONTH / 2);

    let dearer = w.plan_amount(PRICE * 2, MONTH);
    w.sub.upgrade(&user, &id, &dearer);

    // An indexer must be able to describe the change from the event alone.
    assert_eq!(last_event_field_i128(&w.env, "old_amount"), Some(PRICE));
    assert_eq!(last_event_field_i128(&w.env, "new_amount"), Some(PRICE * 2));
    assert_eq!(last_event_field_i128(&w.env, "charged"), Some(5_000));
}

#[test]
fn mandate_updated_snapshot_event_emitted_on_all_state_changes() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);

    // Helper to find the latest MandateUpdated snapshot event for a given mandate_id
    let find_latest_snapshot = |env: &Env, target_id: u64| -> Option<Mandate> {
        let mut snapshot = None;
        for event in env.events().all().events() {
            let ContractEventBody::V0(body) = &event.body;
            if body.topics.len() < 2 {
                continue;
            }
            let ScVal::Symbol(ScSymbol(name)) = &body.topics[0] else {
                continue;
            };
            if name.to_utf8_string_lossy() != "mandate_updated" {
                continue;
            }
            if let ScVal::U64(id) = body.topics[1] {
                if id == target_id {
                    let ScVal::Map(Some(ScMap(entries))) = &body.data else {
                        continue;
                    };
                    for entry in entries.iter() {
                        let ScVal::Symbol(ScSymbol(field)) = &entry.key else {
                            continue;
                        };
                        if field.to_utf8_string_lossy() == "mandate" {
                            snapshot = Some(Mandate::from_val(env, &entry.val));
                        }
                    }
                }
            }
        }
        snapshot
    };

    // 1. Subscribe
    let id = w.sub.subscribe(&user, &plan_id, &2, &0);
    let snap1 = find_latest_snapshot(&w.env, id).expect("MandateUpdated emitted on subscribe");
    assert_eq!(snap1, w.sub.get_mandate(&id));
    assert_eq!(snap1.status, MandateStatus::Active);
    assert_eq!(snap1.charges_made, 0);

    // 2. Pause
    w.sub.set_paused(&user, &id, &true);
    let snap2 = find_latest_snapshot(&w.env, id).expect("MandateUpdated emitted on pause");
    assert_eq!(snap2, w.sub.get_mandate(&id));
    assert_eq!(snap2.status, MandateStatus::Paused);

    // 3. Resume
    w.sub.set_paused(&user, &id, &false);
    let snap3 = find_latest_snapshot(&w.env, id).expect("MandateUpdated emitted on resume");
    assert_eq!(snap3, w.sub.get_mandate(&id));
    assert_eq!(snap3.status, MandateStatus::Active);

    // 4. Pause until
    let future_time = 1_000_000 + 10_000;
    w.sub.pause_until(&user, &id, &future_time);
    let snap_pu = find_latest_snapshot(&w.env, id).expect("MandateUpdated emitted on pause_until");
    assert_eq!(snap_pu, w.sub.get_mandate(&id));
    assert_eq!(snap_pu.status, MandateStatus::Paused);
    assert_eq!(snap_pu.paused_until, future_time);

    // Resume from pause_until and advance time so charge is due
    w.sub.set_paused(&user, &id, &false);
    w.env.ledger().set_timestamp(future_time);

    // 5. Charge
    w.sub.charge(&w.keeper, &id);
    let snap4 = find_latest_snapshot(&w.env, id).expect("MandateUpdated emitted on charge");
    assert_eq!(snap4, w.sub.get_mandate(&id));
    assert_eq!(snap4.charges_made, 1);
    assert_eq!(snap4.status, MandateStatus::Active);

    // 6. Upgrade
    let dearer_plan = w.plan_amount(PRICE * 2, MONTH);
    w.sub.upgrade(&user, &id, &dearer_plan);
    let snap_up = find_latest_snapshot(&w.env, id).expect("MandateUpdated emitted on upgrade");
    assert_eq!(snap_up, w.sub.get_mandate(&id));
    assert_eq!(snap_up.plan_id, dearer_plan);
    assert_eq!(snap_up.amount, PRICE * 2);

    // 7. Refund
    w.minter.mint(&w.merchant, &10_000);
    w.sub.refund(&w.merchant, &id, &500);
    let snap_ref = find_latest_snapshot(&w.env, id).expect("MandateUpdated emitted on refund");
    assert_eq!(snap_ref, w.sub.get_mandate(&id));
    assert_eq!(snap_ref.refunded, 500);

    // 8. Cancel (subscriber)
    w.sub.cancel(&user, &id);
    let snap5 = find_latest_snapshot(&w.env, id).expect("MandateUpdated emitted on cancel");
    assert_eq!(snap5, w.sub.get_mandate(&id));
    assert_eq!(snap5.status, MandateStatus::Cancelled);

    // 9. End mandate (merchant) - test on a separate mandate
    let id2 = w.sub.subscribe(&user, &plan_id, &0, &0);
    w.sub.end_mandate(&w.merchant, &id2);
    let snap6 = find_latest_snapshot(&w.env, id2).expect("MandateUpdated emitted on end_mandate");
    assert_eq!(snap6, w.sub.get_mandate(&id2));
    assert_eq!(snap6.status, MandateStatus::Cancelled);

    // 10. Complete mandate on final charge
    let id3 = w.sub.subscribe(&user, &plan_id, &1, &0);
    w.sub.charge(&w.keeper, &id3);
    let snap7 =
        find_latest_snapshot(&w.env, id3).expect("MandateUpdated emitted on mandate completion");
    assert_eq!(snap7, w.sub.get_mandate(&id3));
    assert_eq!(snap7.charges_made, 1);
    assert_eq!(snap7.status, MandateStatus::Completed);
}

#[test]
fn a_total_spend_cap_completes_the_mandate_instead_of_overcharging() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(PRICE * 10);
    // Room for two charges exactly.
    let id = w.sub.subscribe(&user, &plan_id, &0, &(PRICE * 2));

    w.sub.charge(&w.keeper, &id);
    assert_eq!(w.sub.get_mandate(&id).total_spent, PRICE);

    w.advance(MONTH);
    w.sub.charge(&w.keeper, &id);
    let m = w.sub.get_mandate(&id);
    assert_eq!(m.total_spent, PRICE * 2);
    assert_eq!(m.status, MandateStatus::Completed);

    // A third charge is refused, and the mandate is no longer due.
    w.advance(MONTH);
    assert!(w.sub.try_charge(&w.keeper, &id).is_err());
    assert!(!w.sub.is_due(&id));
}

#[test]
fn a_cap_the_next_charge_would_cross_stops_billing_short_of_it() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(PRICE * 10);
    // One and a half charges' worth: the second must not be settled at all.
    let id = w.sub.subscribe(&user, &plan_id, &0, &(PRICE + PRICE / 2));

    w.sub.charge(&w.keeper, &id);
    w.advance(MONTH);
    assert!(w.sub.try_charge(&w.keeper, &id).is_err());
    assert_eq!(w.sub.get_mandate(&id).total_spent, PRICE);
    assert!(!w.sub.is_due(&id));
}

#[test]
fn zero_means_no_total_spend_cap_and_a_negative_cap_is_rejected() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(PRICE * 10);

    let id = w.sub.subscribe(&user, &plan_id, &0, &0);
    w.sub.charge(&w.keeper, &id);
    w.advance(MONTH);
    w.sub.charge(&w.keeper, &id);
    let m = w.sub.get_mandate(&id);
    assert_eq!(m.status, MandateStatus::Active);
    assert_eq!(m.total_spent, PRICE * 2);

    assert_eq!(
        w.sub.try_subscribe(&user, &plan_id, &0, &-1),
        Err(Ok(Error::InvalidMaxTotal))
    );
}

#[test]
fn update_admin_hands_the_role_over() {
    let w = world();
    let next = Address::generate(&w.env);

    w.sub.update_admin(&next);
    assert_eq!(w.sub.admin(), next);
}

#[test]
fn update_admin_requires_the_current_admins_auth() {
    let w = world();
    let before = w.sub.admin();
    let next = Address::generate(&w.env);

    w.env.set_auths(&[]);
    assert!(w.sub.try_update_admin(&next).is_err());
    assert_eq!(w.sub.admin(), before);
}

#[test]
fn cancel_page_skips_finished_mandates_and_cancels_active_and_paused() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let active = w.sub.subscribe(&user, &plan_id, &0, &0);
    let paused = w.sub.subscribe(&user, &plan_id, &0, &0);
    let completed = w.sub.subscribe(&user, &plan_id, &1, &0);
    let cancelled = w.sub.subscribe(&user, &plan_id, &0, &0);

    w.sub.set_paused(&user, &paused, &true);
    w.sub.charge(&w.keeper, &completed);
    w.sub.cancel(&user, &cancelled);
    assert_eq!(
        w.sub.get_mandate(&completed).status,
        MandateStatus::Completed
    );

    // Only the active and the paused mandate are left to cancel.
    assert_eq!(w.sub.cancel_page(&user, &0, &u32::MAX), 2);
    assert_eq!(w.sub.get_mandate(&active).status, MandateStatus::Cancelled);
    assert_eq!(w.sub.get_mandate(&paused).status, MandateStatus::Cancelled);
    assert_eq!(
        w.sub.get_mandate(&completed).status,
        MandateStatus::Completed
    );

    // A second pass finds nothing to do.
    assert_eq!(w.sub.cancel_page(&user, &0, &u32::MAX), 0);
}

#[test]
fn cancel_page_respects_the_page_cap_and_offset() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    for _ in 0..(MAX_PAGE + 2) {
        w.sub.subscribe(&user, &plan_id, &0, &0);
    }

    assert_eq!(w.sub.cancel_page(&user, &0, &u32::MAX), MAX_PAGE);
    assert_eq!(w.sub.cancel_page(&user, &MAX_PAGE, &u32::MAX), 2);
    for mandate_id in w.sub.subscriber_mandates(&user, &0, &u32::MAX) {
        assert_eq!(
            w.sub.get_mandate(&mandate_id).status,
            MandateStatus::Cancelled
        );
    }
}

#[test]
fn cancel_page_requires_the_subscriber_to_authorize() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    w.sub.subscribe(&user, &plan_id, &0, &0);
    let other_signer = Address::generate(&w.env);

    w.env.mock_auths(&[MockAuth {
        address: &other_signer,
        invoke: &MockAuthInvoke {
            contract: &w.sub.address,
            fn_name: "cancel_page",
            args: (&user, &0u32, &1u32).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);

    assert!(w.sub.try_cancel_page(&user, &0, &1).is_err());
    assert_eq!(w.sub.subscriber_mandates(&user, &0, &1).len(), 1);
}
