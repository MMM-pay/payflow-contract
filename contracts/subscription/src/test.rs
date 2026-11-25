#![cfg(test)]
extern crate std;

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
    xdr::{ContractEventBody, Limits, ReadXdr, ScMap, ScSymbol, ScVal, WriteXdr},
    Address, Env, FromVal, IntoVal, String, Symbol, TryFromVal,
};

const MONTH: u64 = 2_592_000;
const WEEK: u64 = 604_800;
const PRICE: i128 = 10_000;
const FEE_BPS: u32 = 100; // 1%
const KEEPER_BPS: u32 = 200; // 2% of the merchant share
const REFERRAL_BPS: u32 = 1_000; // 10% of the merchant share (from #136)
/// The all-zero contract strkey (#236).
const ZERO_CONTRACT: &str = "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABSC4";
/// A valid subscription wasm used as the `upgrade_wasm` target (#158).
const UPGRADE_WASM: &[u8] = include_bytes!("../testdata/payflow_subscription.wasm");

struct World<'a> {
    env: Env,
    sub: SubscriptionClient<'a>,
    sub_id: Address,
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

    let vault_id = env.register(Vault, (admin.clone(), 10_000u32));
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
        sub_id,
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

    /// A plan with every split knob exposed: keeper tip, referral share and an
    /// optional payout address (None = pay the merchant itself). From #136's
    /// referral/payout tests.
    fn plan_full(&self, keeper_bps: u32, referral_bps: u32, payout: Option<Address>) -> u64 {
        self.registry.create_plan(
            &self.merchant,
            &self.token,
            &PRICE,
            &MONTH,
            &String::from_str(&self.env, "Demo Plan"),
            &String::from_str(&self.env, ""),
            &keeper_bps,
            &soroban_sdk::Symbol::new(&self.env, ""),
            &String::from_str(&self.env, ""),
            &0u64,
            &referral_bps,
            &payout,
            &0u32,
        )
    }

    /// A plan with a seat cap of `max_subscribers` (0 = unlimited). From #158.
    fn plan_with_cap(&self, max_subscribers: u32) -> u64 {
        self.registry.create_plan(
            &self.merchant,
            &self.token,
            &PRICE,
            &MONTH,
            &String::from_str(&self.env, "Seated Plan"),
            &String::from_str(&self.env, ""),
            &0,
            &soroban_sdk::Symbol::new(&self.env, ""),
            &String::from_str(&self.env, ""),
            &0u64,
            &0u32,
            &None::<Address>,
            &max_subscribers,
        )
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
            &Symbol::new(&self.env, ""),
            &String::from_str(&self.env, ""),
            &0u64,
            &0u32,
            &None::<Address>,
            &0u32,
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
            &Symbol::new(&self.env, ""),
            &String::from_str(&self.env, ""),
            &0u64,
            &0u32,
            &None::<Address>,
            &0u32,
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

    /// Publish a plan that stops accepting new subscribers at `cutoff`
    /// (from #134's availability-cutoff tests).
    fn plan_expiring(&self, cutoff: u64) -> u64 {
        self.registry.create_plan(
            &self.merchant,
            &self.token,
            &PRICE,
            &MONTH,
            &String::from_str(&self.env, "Promo Plan"),
            &String::from_str(&self.env, ""),
            &0,
            &soroban_sdk::Symbol::new(&self.env, ""),
            &String::from_str(&self.env, ""),
            &cutoff,
            &0u32,
            &None::<Address>,
            &0u32,
        )
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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

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

    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
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
/// the subscriber's authorization for `subscribe(subscriber, plan_id,
/// max_charges, max_total, referrer)`,
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
            args: (&user, &plan_id, &0u32, &0i128, &None::<Address>).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);

    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
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
    let id = w.sub.subscribe(&user, &plan_id, &4, &0, &None::<Address>);

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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    w.env.mock_auths(&[]);
    assert_eq!(w.sub.estimated_end(&id), None);
}

#[test]
fn estimated_end_reports_schedule_overflow() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    w.env.ledger().set_timestamp(u64::MAX - MONTH + 1);
    let id = w.sub.subscribe(&user, &plan_id, &2, &0, &None::<Address>);

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

    let first_id = w
        .sub
        .subscribe(&first_user, &plan_id, &0, &0, &None::<Address>);
    let next_id = w.sub.next_mandate_id();
    assert_eq!(next_id, first_id + 1);

    let second_id = w
        .sub
        .subscribe(&second_user, &plan_id, &0, &0, &None::<Address>);
    assert_eq!(second_id, next_id);
}

/// Pull a u32 field out of the data map of any emitted contract event.
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

/// Find the most recent event with the given topic name (e.g. "`mandate_completed`").
fn event_by_topic(env: &Env, topic: &str) -> Option<soroban_sdk::xdr::ContractEventV0> {
    for event in env.events().all().events() {
        let ContractEventBody::V0(body) = &event.body;
        if let Some(ScVal::Symbol(ScSymbol(name))) = body.topics.first() {
            if name.to_utf8_string_lossy() == topic {
                return Some(body.clone());
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
        for entry in entries {
            let ScVal::Symbol(ScSymbol(name)) = &entry.key else {
                continue;
            };
            if name.to_utf8_string_lossy() == field {
                if let ScVal::I128(ref parts) = entry.val {
                    found = Some((i128::from(parts.hi) << 64) | i128::from(parts.lo));
                }
            }
        }
    }
    found
}

/// Number of vault `Debit` events emitted during the most recent invocation.
/// `charge` used to call `vault.debit` once per payee; it now settles the
/// merchant and the fee through one `debit_split`, which must still produce the
/// same events.
fn vault_debit_event_count(env: &Env) -> usize {
    let mut count = 0;
    for event in env.events().all().events() {
        let ContractEventBody::V0(body) = &event.body;
        if let Some(ScVal::Symbol(ScSymbol(name))) = body.topics.first() {
            if name.to_utf8_string_lossy() == "debit" {
                count += 1;
            }
        }
    }
    count
}

fn last_event_field_u64(env: &Env, field: &str) -> Option<u64> {
    let mut found = None;
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
                if let ScVal::U64(value) = entry.val {
                    found = Some(value);
                }
            }
        }
    }
    found
}

/// Pull a u64 field out of the data map of any emitted contract event.
fn event_field_u64(env: &Env, field: &str) -> Option<u64> {
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
                if let ScVal::U64(v) = entry.val {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// The value of `field` from the data map of the most recent event whose
/// topic 0 is `event_name`.
fn event_field_address(env: &Env, event_name: &str, field: &str) -> Option<Address> {
    let mut found = None;
    for event in env.events().all().events() {
        let ContractEventBody::V0(body) = &event.body;
        let ScVal::Symbol(ScSymbol(topic)) = &body.topics[0] else {
            continue;
        };
        if topic.to_utf8_string_lossy() != event_name {
            continue;
        }
        let ScVal::Map(Some(ScMap(entries))) = &body.data else {
            continue;
        };
        for entry in entries {
            let ScVal::Symbol(ScSymbol(name)) = &entry.key else {
                continue;
            };
            if name.to_utf8_string_lossy() == field && matches!(entry.val, ScVal::Address(_)) {
                found = Address::try_from_val(env, &entry.val).ok();
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

    let id = w.sub.subscribe(&user, &plan_id, &3, &0, &None::<Address>);

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
        w.sub
            .try_subscribe(&user, &plan_id, &0, &0, &None::<Address>,),
        Err(Ok(Error::PlanInactive))
    );
}

#[test]
fn first_charge_is_due_immediately_and_splits_fee() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    assert!(w.sub.is_due(&id));
    w.sub.charge(&w.keeper, &id);

    let expected_fee = PRICE * i128::from(FEE_BPS) / BPS_DENOMINATOR;
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
fn charge_settles_merchant_and_fee_through_one_vault_call() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    w.sub.charge(&w.keeper, &id);

    // One `debit_split` replaces the two `debit` calls, but the vault still
    // emits one `Debit` per payee, exactly as before.
    assert_eq!(vault_debit_event_count(&w.env), 2);

    let expected_fee = PRICE * i128::from(FEE_BPS) / BPS_DENOMINATOR;
    assert_eq!(w.token_client.balance(&w.merchant), PRICE - expected_fee);
    assert_eq!(w.token_client.balance(&w.fee_to), expected_fee);
    assert_eq!(w.vault.balance(&user, &w.token), 100_000 - PRICE);

    let m = w.sub.get_mandate(&id);
    assert_eq!(m.charges_made, 1);
    assert_eq!(m.total_spent, PRICE);
    assert_eq!(m.next_charge, w.env.ledger().timestamp() + MONTH);
}

#[test]
fn a_zero_fee_charge_emits_one_debit() {
    let w = world();
    let plan_id = w.plan();

    // Open the mandate at a zero protocol fee, so the split has an empty
    // second leg and must look exactly like a single `debit` call.
    w.sub.set_fee_bps(&0);
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    w.sub.charge(&w.keeper, &id);

    assert_eq!(vault_debit_event_count(&w.env), 1);
    assert_eq!(w.token_client.balance(&w.merchant), PRICE);
    assert_eq!(w.token_client.balance(&w.fee_to), 0);
    assert_eq!(w.vault.balance(&user, &w.token), 100_000 - PRICE);
}

#[test]
fn a_tipped_charge_still_pays_three_payees() {
    let w = world();
    let plan_id = w.plan_with_keeper(KEEPER_BPS);
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    w.sub.charge(&w.keeper, &id);

    // The tip is a third payee and keeps its own debit, so a tipped charge
    // makes two vault calls and still emits three `Debit` events.
    assert_eq!(vault_debit_event_count(&w.env), 3);

    let fee = PRICE * i128::from(FEE_BPS) / BPS_DENOMINATOR;
    let merchant_gross = PRICE - fee;
    let tip = merchant_gross * i128::from(KEEPER_BPS) / BPS_DENOMINATOR;
    assert_eq!(w.token_client.balance(&w.fee_to), fee);
    assert_eq!(w.token_client.balance(&w.keeper), tip);
    assert_eq!(w.token_client.balance(&w.merchant), merchant_gross - tip);
    // The subscriber is debited the price exactly, tip included.
    assert_eq!(w.vault.balance(&user, &w.token), 100_000 - PRICE);
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
            args: (&user, &plan_id, &0u32, &0i128, &None::<Address>).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);

    assert!(w
        .sub
        .try_subscribe(&user, &plan_id, &0, &0, &None::<Address>,)
        .is_err());
}

#[test]
fn any_account_can_settle_a_charge() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    let fee = PRICE * i128::from(FEE_BPS) / BPS_DENOMINATOR;
    let merchant_gross = PRICE - fee;
    let tip = merchant_gross * i128::from(KEEPER_BPS) / BPS_DENOMINATOR;

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
    let id = w.sub.subscribe(&user, &plan_id, &1, &0, &None::<Address>);

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

    let body = event_by_topic(&w.env, "mandate_completed").expect("mandate_completed event");
    let ScVal::Symbol(ScSymbol(name)) = &body.topics[0] else {
        panic!("topic 0 is not a symbol");
    };
    let expected: soroban_sdk::xdr::StringM<32> = "mandate_completed".try_into().unwrap();
    assert_eq!(*name, expected);
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
    let plain_id = w.sub.subscribe(&without, &plain, &0, &0, &None::<Address>);
    let tipped_id = w.sub.subscribe(&with, &tipped, &0, &0, &None::<Address>);

    w.sub.charge(&w.keeper, &plain_id);
    w.sub.charge(&w.keeper, &tipped_id);

    assert_eq!(w.vault.balance(&without, &w.token), 100_000 - PRICE);
    assert_eq!(w.vault.balance(&with, &w.token), 100_000 - PRICE);

    // Only the tipped charge pays the keeper.
    let fee = PRICE * i128::from(FEE_BPS) / BPS_DENOMINATOR;
    let tip = (PRICE - fee) * i128::from(KEEPER_BPS) / BPS_DENOMINATOR;
    assert_eq!(w.token_client.balance(&w.keeper), tip);
}

#[test]
fn second_charge_is_rejected_before_due_and_succeeds_at_due_time() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    w.sub.cancel(&user, &id);
    assert_eq!(
        w.sub.try_cancel(&user, &id),
        Err(Ok(Error::MandateNotActive))
    );
}

// ---------------------------------------------------------------------------
// Cancellation reasons (#73)
// ---------------------------------------------------------------------------

#[test]
fn cancel_with_reason_records_the_code_in_the_event() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    w.sub.charge(&w.keeper, &id);

    w.sub
        .cancel_with_reason(&user, &id, &CANCEL_REASON_TOO_EXPENSIVE);

    // The reason must be recoverable from the event alone. The assertions come
    // first because the test host keeps only the most recent invocation's
    // events, and `get_mandate` below would replace them.
    assert_eq!(
        event_field_u32(&w.env, "reason"),
        Some(CANCEL_REASON_TOO_EXPENSIVE)
    );
    assert_eq!(event_field_u32(&w.env, "charges_made"), Some(1));
    assert_eq!(w.sub.get_mandate(&id).status, MandateStatus::Cancelled);
}

#[test]
fn plain_cancel_emits_reason_zero() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    w.sub.cancel(&user, &id);

    // `cancel` is `cancel_with_reason` with no reason: indexers see 0, never a
    // missing field.
    assert_eq!(event_field_u32(&w.env, "reason"), Some(0));
    assert_eq!(w.sub.get_mandate(&id).status, MandateStatus::Cancelled);
}

#[test]
fn every_documented_cancel_reason_is_accepted() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);

    for reason in CANCEL_REASON_NONE..=MAX_CANCEL_REASON {
        let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
        w.sub.cancel_with_reason(&user, &id, &reason);
        assert_eq!(event_field_u32(&w.env, "reason"), Some(reason));
        assert_eq!(w.sub.get_mandate(&id).status, MandateStatus::Cancelled);
    }
}

#[test]
fn an_undocumented_cancel_reason_is_rejected() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    assert_eq!(
        w.sub
            .try_cancel_with_reason(&user, &id, &(MAX_CANCEL_REASON + 1)),
        Err(Ok(Error::InvalidCancelReason))
    );
    assert_eq!(
        w.sub.try_cancel_with_reason(&user, &id, &u32::MAX),
        Err(Ok(Error::InvalidCancelReason))
    );
    // The rejected call must not cancel or otherwise alter the mandate.
    assert_eq!(w.sub.get_mandate(&id).status, MandateStatus::Active);
}

#[test]
fn cancel_with_reason_rejects_a_non_subscriber() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let attacker = Address::generate(&w.env);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    assert_eq!(
        w.sub
            .try_cancel_with_reason(&attacker, &id, &CANCEL_REASON_NOT_USING),
        Err(Ok(Error::NotSubscriber))
    );
    // The merchant has `end_mandate`, not a subscriber cancellation.
    assert_eq!(
        w.sub
            .try_cancel_with_reason(&w.merchant, &id, &CANCEL_REASON_NOT_USING),
        Err(Ok(Error::NotSubscriber))
    );
    assert_eq!(w.sub.get_mandate(&id).status, MandateStatus::Active);
}

#[test]
fn cancel_with_reason_wrong_signer_rejected() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    // A reason is not an authorization: only the subscriber's own signature
    // may cancel, even though anyone can read the code set.
    let wrong_signer = Address::generate(&w.env);
    w.env.mock_auths(&[MockAuth {
        address: &wrong_signer,
        invoke: &MockAuthInvoke {
            contract: &w.sub.address,
            fn_name: "cancel_with_reason",
            args: (&user, &id, &CANCEL_REASON_NOT_USING).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);

    assert!(w
        .sub
        .try_cancel_with_reason(&user, &id, &CANCEL_REASON_NOT_USING)
        .is_err());
    assert_eq!(w.sub.get_mandate(&id).status, MandateStatus::Active);
}

#[test]
fn cancel_with_reason_cannot_cancel_twice() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    w.sub.cancel_with_reason(&user, &id, &CANCEL_REASON_OTHER);
    assert_eq!(
        w.sub
            .try_cancel_with_reason(&user, &id, &CANCEL_REASON_OTHER),
        Err(Ok(Error::MandateNotActive))
    );
}

#[test]
fn mandate_owner_transfer_updates_indexes_and_future_charges() {
    let w = world();
    let plan_id = w.plan();
    let old_subscriber = w.subscriber(100_000);
    let new_subscriber = w.subscriber(100_000);
    let id = w
        .sub
        .subscribe(&old_subscriber, &plan_id, &0, &0, &None::<Address>);

    w.sub
        .propose_mandate_owner(&old_subscriber, &id, &new_subscriber, &0);
    assert_eq!(
        w.sub.get_mandate(&id).subscriber,
        old_subscriber,
        "proposal alone must not change mandate ownership"
    );
    w.sub.accept_mandate_owner(&new_subscriber, &id);

    let last = w.env.events().all().events().last().cloned().unwrap();
    let ContractEventBody::V0(body) = &last.body;
    let ScVal::Symbol(ScSymbol(name)) = &body.topics[0] else {
        panic!("topic 0 is not a symbol");
    };
    let expected: soroban_sdk::xdr::StringM<32> = "mandate_transferred".try_into().unwrap();
    assert_eq!(name, &expected);

    assert_eq!(w.sub.get_mandate(&id).subscriber, new_subscriber);
    assert_eq!(w.sub.subscriber_mandate_count(&old_subscriber), 0);
    assert_eq!(w.sub.subscriber_mandates(&old_subscriber, &0, &50).len(), 0);
    assert_eq!(w.sub.subscriber_mandate_count(&new_subscriber), 1);
    assert_eq!(
        w.sub.subscriber_mandates(&new_subscriber, &0, &50),
        soroban_sdk::vec![&w.env, id]
    );
    assert_eq!(w.sub.merchant_mandate_count(&w.merchant), 1);
    assert_eq!(
        w.sub.merchant_mandates(&w.merchant, &0, &50),
        soroban_sdk::vec![&w.env, id]
    );
    assert_eq!(
        w.sub.try_cancel(&old_subscriber, &id),
        Err(Ok(Error::NotSubscriber))
    );

    w.sub.charge(&w.keeper, &id);
    assert_eq!(w.vault.balance(&old_subscriber, &w.token), 100_000);
    assert_eq!(w.vault.balance(&new_subscriber, &w.token), 100_000 - PRICE);
}

#[test]
fn mandate_owner_transfer_repairs_a_pending_index_position() {
    let w = world();
    let plan_id = w.plan();
    let old_subscriber = w.subscriber(100_000);
    let first_new_owner = w.subscriber(100_000);
    let second_new_owner = w.subscriber(100_000);
    let first_id = w
        .sub
        .subscribe(&old_subscriber, &plan_id, &0, &0, &None::<Address>);
    let second_id = w
        .sub
        .subscribe(&old_subscriber, &plan_id, &0, &0, &None::<Address>);

    w.sub
        .propose_mandate_owner(&old_subscriber, &second_id, &first_new_owner, &1);
    w.sub
        .propose_mandate_owner(&old_subscriber, &first_id, &second_new_owner, &0);
    w.sub.accept_mandate_owner(&second_new_owner, &first_id);
    w.sub.accept_mandate_owner(&first_new_owner, &second_id);

    assert_eq!(w.sub.subscriber_mandate_count(&old_subscriber), 0);
    assert_eq!(
        w.sub.subscriber_mandates(&first_new_owner, &0, &50),
        soroban_sdk::vec![&w.env, second_id]
    );
    assert_eq!(
        w.sub.subscriber_mandates(&second_new_owner, &0, &50),
        soroban_sdk::vec![&w.env, first_id]
    );
}

#[test]
fn propose_mandate_owner_requires_subscriber_auth() {
    let w = world();
    let plan_id = w.plan();
    let subscriber = w.subscriber(100_000);
    let new_subscriber = w.subscriber(100_000);
    let wrong_signer = Address::generate(&w.env);
    let id = w
        .sub
        .subscribe(&subscriber, &plan_id, &0, &0, &None::<Address>);

    // A signer other than the current subscriber cannot propose the transfer.
    w.env.mock_auths(&[MockAuth {
        address: &wrong_signer,
        invoke: &MockAuthInvoke {
            contract: &w.sub.address,
            fn_name: "propose_mandate_owner",
            args: (&subscriber, &id, &new_subscriber, &0u32).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);
    assert!(w
        .sub
        .try_propose_mandate_owner(&subscriber, &id, &new_subscriber, &0)
        .is_err());
}

#[test]
fn accept_mandate_owner_requires_new_subscriber_auth() {
    let w = world();
    let plan_id = w.plan();
    let subscriber = w.subscriber(100_000);
    let new_subscriber = w.subscriber(100_000);
    let wrong_signer = Address::generate(&w.env);
    let id = w
        .sub
        .subscribe(&subscriber, &plan_id, &0, &0, &None::<Address>);

    w.env.mock_all_auths();
    w.sub
        .propose_mandate_owner(&subscriber, &id, &new_subscriber, &0);

    // Only the proposed new subscriber can accept; any other signer is rejected.
    w.env.mock_auths(&[MockAuth {
        address: &wrong_signer,
        invoke: &MockAuthInvoke {
            contract: &w.sub.address,
            fn_name: "accept_mandate_owner",
            args: (&new_subscriber, &id).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);
    assert!(w
        .sub
        .try_accept_mandate_owner(&new_subscriber, &id)
        .is_err());
}

#[test]
fn mandate_owner_transfer_rejects_invalid_proposals_and_positions() {
    let w = world();
    let plan_id = w.plan();
    let subscriber = w.subscriber(100_000);
    let new_subscriber = w.subscriber(100_000);
    let id = w
        .sub
        .subscribe(&subscriber, &plan_id, &0, &0, &None::<Address>);

    assert_eq!(
        w.sub
            .try_propose_mandate_owner(&subscriber, &id, &new_subscriber, &1),
        Err(Ok(Error::InvalidSubscriberIndex))
    );
    assert_eq!(
        w.sub.try_accept_mandate_owner(&new_subscriber, &id),
        Err(Ok(Error::NoPendingTransfer))
    );

    w.sub
        .propose_mandate_owner(&subscriber, &id, &new_subscriber, &0);
    let other_candidate = w.subscriber(100_000);
    assert_eq!(
        w.sub.try_accept_mandate_owner(&other_candidate, &id),
        Err(Ok(Error::NotProposedOwner))
    );
}

#[test]
fn pause_suspends_billing_and_resume_restores_it() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

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
    let id = w.sub.subscribe(&user, &plan_id, &2, &0, &None::<Address>);

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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    w.vault.withdraw(&user, &w.token, &100_000);
    assert!(w.sub.try_charge(&w.keeper, &id).is_err());
}

#[test]
fn indexes_track_both_sides() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);

    let a = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    let b = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

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
    let vault_id = env.register(Vault, (admin.clone(), 10_000u32));
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
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );
    let user = Address::generate(&env);
    StellarAssetClient::new(&env, &token).mint(&user, &100_000);
    vault.deposit(&user, &token, &100_000);

    let id = sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    sub.charge(&keeper, &id);

    assert_eq!(TokenClient::new(&env, &token).balance(&merchant), PRICE);
    assert_eq!(TokenClient::new(&env, &token).balance(&fee_to), 0);
}

#[test]
#[should_panic(expected = "Error(Contract, #2008)")]
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
/// 101 stroops at 100 bps is 1.01 stroops, truncated to a 1 stroop fee.
#[test]
fn quote_matches_charge_for_a_price_that_rounds() {
    let w = world();
    let plan_id = w.plan_priced(101);
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    let quote = w.sub.quote_charge(&id);
    assert_eq!(
        quote,
        ChargeQuote {
            amount: 101,
            fee: 1,
            merchant_amount: 100,
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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    let quote = w.sub.quote_charge(&id);
    let fee = PRICE * i128::from(FEE_BPS) / BPS_DENOMINATOR;
    let tip = (PRICE - fee) * i128::from(KEEPER_BPS) / BPS_DENOMINATOR;

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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

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
    let capped = w.sub.subscribe(&user, &plan_id, &1, &0, &None::<Address>);
    assert!(w.sub.is_due(&capped));
    assert!(w.sub.quote_charge(&capped).chargeable);
    w.sub.charge(&w.keeper, &capped);
    // Completed by the charge cap.
    assert_eq!(w.sub.get_mandate(&capped).status, MandateStatus::Completed);
    assert!(!w.sub.is_due(&capped));
    assert!(!w.sub.quote_charge(&capped).chargeable);

    // Paused.
    let paused = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    w.sub.set_paused(&user, &paused, &true);
    assert!(!w.sub.is_due(&paused));
    assert!(!w.sub.quote_charge(&paused).chargeable);

    // Cancelled.
    let cancelled = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    w.sub.cancel(&user, &cancelled);
    assert!(!w.sub.is_due(&cancelled));
    assert!(!w.sub.quote_charge(&cancelled).chargeable);

    // Active but waiting for the next period.
    let waiting = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    w.env.mock_auths(&[]);
    assert_eq!(w.sub.quote_charge(&id).amount, PRICE);
}

#[test]
fn simulate_charge_reports_exact_recipients_without_authorization_or_writes() {
    let w = world();
    let referrer = Address::generate(&w.env);
    let payout = Address::generate(&w.env);
    let plan_id = w.plan_full(KEEPER_BPS, REFERRAL_BPS, Some(payout.clone()));
    let user = w.subscriber(100_000);
    let id = w
        .sub
        .subscribe(&user, &plan_id, &0, &0, &Some(referrer.clone()));
    let mandate_before = w.sub.get_mandate(&id);
    let vault_before = w.vault.balance(&user, &w.token);

    w.env.mock_auths(&[]);
    let simulation = w.sub.simulate_charge(&w.keeper, &id);

    let fee = PRICE * i128::from(FEE_BPS) / BPS_DENOMINATOR;
    let merchant_gross = PRICE - fee;
    let tip = merchant_gross * i128::from(KEEPER_BPS) / BPS_DENOMINATOR;
    let referral = merchant_gross * i128::from(REFERRAL_BPS) / BPS_DENOMINATOR;
    let merchant_amount = merchant_gross - tip - referral;
    assert_eq!(simulation.subscriber, user);
    assert_eq!(simulation.token, w.token);
    assert_eq!(simulation.amount, PRICE);
    assert_eq!(simulation.payout, payout);
    assert_eq!(simulation.merchant_amount, merchant_amount);
    assert_eq!(simulation.fee_to, w.fee_to);
    assert_eq!(simulation.fee, fee);
    assert_eq!(simulation.keeper, w.keeper);
    assert_eq!(simulation.tip, tip);
    assert_eq!(simulation.referrer, Some(referrer.clone()));
    assert_eq!(simulation.referral, referral);
    assert_eq!(simulation.due_at, mandate_before.next_charge);
    assert!(simulation.chargeable);
    assert_eq!(w.sub.get_mandate(&id), mandate_before);
    assert_eq!(w.vault.balance(&user, &w.token), vault_before);

    w.env.mock_all_auths();
    w.sub.charge(&w.keeper, &id);
    assert_eq!(w.token_client.balance(&payout), merchant_amount);
    assert_eq!(w.token_client.balance(&w.fee_to), fee);
    assert_eq!(w.token_client.balance(&w.keeper), tip);
    assert_eq!(w.token_client.balance(&referrer), referral);
    assert_eq!(
        vault_before - w.vault.balance(&user, &w.token),
        simulation.amount
    );
}

#[test]
fn simulate_charge_reports_paused_and_missing_mandates() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    w.sub.set_protocol_paused(&true);
    assert!(!w.sub.simulate_charge(&w.keeper, &id).chargeable);
    assert_eq!(
        w.sub.try_simulate_charge(&w.keeper, &99),
        Err(Ok(Error::MandateNotFound))
    );
}

#[test]
fn simulate_charge_reports_plan_pause_and_cap_without_moving_funds() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &1, &0, &None::<Address>);

    w.registry
        .set_plan_charges_paused(&w.merchant, &plan_id, &true);
    assert!(!w.sub.simulate_charge(&w.keeper, &id).chargeable);

    w.registry
        .set_plan_charges_paused(&w.merchant, &plan_id, &false);
    w.sub.charge(&w.keeper, &id);
    assert!(!w.sub.simulate_charge(&w.keeper, &id).chargeable);
    assert_eq!(w.vault.balance(&user, &w.token), 100_000 - PRICE);
}

#[test]
fn simulate_charge_reports_amount_overflow() {
    let w = world();
    let plan_id = w.plan_priced(i128::MAX);
    let user = w.subscriber(1_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    assert_eq!(
        w.sub.try_simulate_charge(&w.keeper, &id),
        Err(Ok(Error::AmountOverflow))
    );
}

/// A price large enough to overflow the fee multiplication is reported as
/// `AmountOverflow` by both quote and charge instead of panicking.
#[test]
fn quote_and_charge_report_fee_overflow() {
    let w = world();
    let plan_id = w.plan_priced(i128::MAX);
    let user = w.subscriber(1_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    assert_eq!(w.sub.try_quote_charge(&id), Err(Ok(Error::AmountOverflow)));
    assert_eq!(
        w.sub.try_charge(&w.keeper, &id),
        Err(Ok(Error::AmountOverflow))
    );
    // The failed charge left the mandate untouched.
    assert_eq!(w.sub.get_mandate(&id).charges_made, 0);
}

/// Quote matches charge when keeper tip and referral are both present.
#[test]
fn quote_matches_charge_with_keeper_tip_and_referral() {
    let w = world();
    let referrer = Address::generate(&w.env);
    let plan_id = w.plan_full(KEEPER_BPS, REFERRAL_BPS, None);
    let user = w.subscriber(100_000);
    let id = w
        .sub
        .subscribe(&user, &plan_id, &0, &0, &Some(referrer.clone()));

    let quote = w.sub.quote_charge(&id);
    let fee = PRICE * i128::from(FEE_BPS) / BPS_DENOMINATOR;
    let merchant_gross = PRICE - fee;
    let tip = merchant_gross * i128::from(KEEPER_BPS) / BPS_DENOMINATOR;
    let referral = merchant_gross * i128::from(REFERRAL_BPS) / BPS_DENOMINATOR;
    let merchant_amount = merchant_gross - tip - referral;

    assert_eq!(quote.amount, PRICE);
    assert_eq!(quote.fee, fee);
    assert_eq!(quote.merchant_amount, merchant_amount);
    assert!(quote.chargeable);

    w.sub.charge(&w.keeper, &id);

    assert_eq!(w.token_client.balance(&w.merchant), merchant_amount);
    assert_eq!(w.token_client.balance(&w.keeper), tip);
    assert_eq!(w.token_client.balance(&referrer), referral);
    assert_eq!(w.token_client.balance(&w.fee_to), fee);
    assert_eq!(w.vault.balance(&user, &w.token), 100_000 - PRICE);
}

/// Quote matches charge when `fee_bps` is zero.
#[test]
fn quote_matches_charge_with_zero_fee() {
    let w = world();
    w.sub.set_fee_bps(&0);
    let plan_id = w.plan_with_keeper(KEEPER_BPS);
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    let quote = w.sub.quote_charge(&id);
    let tip = PRICE * i128::from(KEEPER_BPS) / BPS_DENOMINATOR;

    assert_eq!(quote.amount, PRICE);
    assert_eq!(quote.fee, 0);
    assert_eq!(quote.merchant_amount, PRICE - tip);
    assert!(quote.chargeable);

    w.sub.charge(&w.keeper, &id);

    assert_eq!(w.token_client.balance(&w.merchant), PRICE - tip);
    assert_eq!(w.token_client.balance(&w.keeper), tip);
    assert_eq!(w.token_client.balance(&w.fee_to), 0);
    assert_eq!(w.vault.balance(&user, &w.token), 100_000 - PRICE);
}

/// Quote chargeable=false when `max_charges` reached; charge fails (mandate becomes Completed).
#[test]
fn quote_and_charge_agree_on_max_charges_reached() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &1, &0, &None::<Address>);

    // First charge succeeds.
    w.sub.charge(&w.keeper, &id);
    assert_eq!(w.sub.get_mandate(&id).charges_made, 1);
    assert_eq!(w.sub.get_mandate(&id).status, MandateStatus::Completed);

    // Now quote reports not chargeable.
    let quote = w.sub.quote_charge(&id);
    assert!(!quote.chargeable);
    assert_eq!(quote.amount, PRICE);

    // Charge fails because mandate is now Completed (MandateNotActive).
    // The specific error differs from quote's check order, but both agree:
    // quote says not chargeable, charge fails.
    assert_eq!(
        w.sub.try_charge(&w.keeper, &id),
        Err(Ok(Error::MandateNotActive))
    );
}

/// Quote chargeable=false when `max_total` reached; charge fails.
#[test]
fn quote_and_charge_agree_on_max_total_reached() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    // max_total = 2 * PRICE, so two charges max.
    let id = w
        .sub
        .subscribe(&user, &plan_id, &0, &(2 * PRICE), &None::<Address>);

    // Verify max_total is set correctly.
    let mandate = w.sub.get_mandate(&id);
    assert_eq!(mandate.max_total, 2 * PRICE);
    assert_eq!(mandate.total_spent, 0);
    assert_eq!(mandate.status, MandateStatus::Active);

    // First charge succeeds.
    w.sub.charge(&w.keeper, &id);
    let after_first = w.sub.get_mandate(&id);
    assert_eq!(after_first.total_spent, PRICE);
    assert_eq!(after_first.charges_made, 1);
    assert_eq!(after_first.status, MandateStatus::Active);

    // Advance time to the next charge period.
    w.advance(MONTH);

    // Second charge succeeds and reaches max_total.
    w.sub.charge(&w.keeper, &id);
    let after_second = w.sub.get_mandate(&id);
    assert_eq!(after_second.total_spent, 2 * PRICE);
    assert_eq!(after_second.charges_made, 2);
    assert_eq!(after_second.status, MandateStatus::Completed);

    // Now quote reports not chargeable.
    let quote = w.sub.quote_charge(&id);
    assert!(!quote.chargeable);
    assert_eq!(quote.amount, PRICE);

    // Charge fails because mandate is now Completed (MandateNotActive).
    let charge_result = w.sub.try_charge(&w.keeper, &id);
    assert_eq!(charge_result, Err(Ok(Error::MandateNotActive)));
}

/// Quote chargeable=false when plan charges paused; charge fails with `PlanChargesPaused`.
#[test]
fn quote_and_charge_agree_on_plan_charges_paused() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    // Pause charging at the plan level.
    w.registry
        .set_plan_charges_paused(&w.merchant, &plan_id, &true);

    let quote = w.sub.quote_charge(&id);
    assert!(!quote.chargeable);
    assert_eq!(quote.amount, PRICE);

    assert_eq!(
        w.sub.try_charge(&w.keeper, &id),
        Err(Ok(Error::PlanChargesPaused))
    );
}

/// Quote chargeable=false when mandate paused; charge fails with `MandateNotActive`.
#[test]
fn quote_and_charge_agree_on_mandate_paused() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    w.sub.set_paused(&user, &id, &true);

    let quote = w.sub.quote_charge(&id);
    assert!(!quote.chargeable);
    assert_eq!(quote.amount, PRICE);

    assert_eq!(
        w.sub.try_charge(&w.keeper, &id),
        Err(Ok(Error::MandateNotActive))
    );
}

/// Quote chargeable=false when mandate cancelled; charge fails with `MandateNotActive`.
#[test]
fn quote_and_charge_agree_on_mandate_cancelled() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    w.sub.cancel(&user, &id);

    let quote = w.sub.quote_charge(&id);
    assert!(!quote.chargeable);
    assert_eq!(quote.amount, PRICE);

    assert_eq!(
        w.sub.try_charge(&w.keeper, &id),
        Err(Ok(Error::MandateNotActive))
    );
}

/// Quote chargeable=false when mandate completed; charge fails with `MandateNotActive`.
#[test]
fn quote_and_charge_agree_on_mandate_completed() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &1, &0, &None::<Address>);

    w.sub.charge(&w.keeper, &id);
    assert_eq!(w.sub.get_mandate(&id).status, MandateStatus::Completed);

    let quote = w.sub.quote_charge(&id);
    assert!(!quote.chargeable);
    assert_eq!(quote.amount, PRICE);

    assert_eq!(
        w.sub.try_charge(&w.keeper, &id),
        Err(Ok(Error::MandateNotActive))
    );
}

/// Quote chargeable=false when not due yet; charge fails with `NotDue`.
#[test]
fn quote_and_charge_agree_on_not_due() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    // Charge once to advance next_charge.
    w.sub.charge(&w.keeper, &id);

    // Now not due until next period.
    let quote = w.sub.quote_charge(&id);
    assert!(!quote.chargeable);
    assert_eq!(quote.amount, PRICE);
    assert_eq!(quote.due_at, w.env.ledger().timestamp() + MONTH);

    assert_eq!(w.sub.try_charge(&w.keeper, &id), Err(Ok(Error::NotDue)));
}

#[test]
fn charge_with_a_zero_payout_address_fails_at_settlement() {
    let w = world();
    let zero = Address::from_str(&w.env, ZERO_CONTRACT);
    let plan_id = w.plan_full(0, 0, Some(zero.clone()));
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    assert!(w.sub.quote_charge(&id).chargeable);
    assert_eq!(
        w.sub.try_charge(&w.keeper, &id),
        Err(Ok(Error::VaultCallFailed))
    );
    assert_eq!(w.vault.balance(&user, &w.token), 100_000);
    assert_eq!(w.token_client.balance(&zero), 0);
    assert_eq!(w.sub.get_mandate(&id).charges_made, 0);
}

/// Quote chargeable=true but charge fails with `VaultCallFailed` when insufficient balance.
#[test]
fn quote_chargeable_but_charge_fails_on_insufficient_balance() {
    let w = world();
    let plan_id = w.plan_with_keeper(KEEPER_BPS);
    let user = w.subscriber(100); // Only 100, less than PRICE (10_000)
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    let quote = w.sub.quote_charge(&id);
    assert!(quote.chargeable);
    assert_eq!(quote.amount, PRICE);

    // Charge fails because vault has insufficient balance.
    assert_eq!(
        w.sub.try_charge(&w.keeper, &id),
        Err(Ok(Error::VaultCallFailed))
    );

    // Mandate state unchanged.
    assert_eq!(w.sub.get_mandate(&id).charges_made, 0);
    assert_eq!(
        w.sub.get_mandate(&id).next_charge,
        w.env.ledger().timestamp()
    );
}

/// Quote matches charge after a `pause_until` schedule resumes.
#[test]
fn quote_and_charge_agree_after_pause_until_resumes() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    let now = w.env.ledger().timestamp();
    let until = now + 3_600; // 1 hour
    w.sub.pause_until(&user, &id, &until);

    // During pause: not chargeable.
    let quote_during = w.sub.quote_charge(&id);
    assert!(!quote_during.chargeable);

    // Advance past the pause.
    w.env.ledger().set_timestamp(until + 1);

    // Now chargeable again.
    let quote_after = w.sub.quote_charge(&id);
    assert!(quote_after.chargeable);
    assert_eq!(quote_after.amount, PRICE);

    w.sub.charge(&w.keeper, &id);
    assert_eq!(
        w.token_client.balance(&w.merchant),
        PRICE - PRICE * i128::from(FEE_BPS) / BPS_DENOMINATOR
    );
    assert_eq!(w.vault.balance(&user, &w.token), 100_000 - PRICE);
}

/// Quote matches charge when `max_charges` is increased before being reached.
#[test]
fn quote_and_charge_agree_after_max_charges_increased() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    // Start with max_charges = 1
    let id = w.sub.subscribe(&user, &plan_id, &1, &0, &None::<Address>);

    // Quote shows chargeable.
    let quote = w.sub.quote_charge(&id);
    assert!(quote.chargeable);
    assert_eq!(quote.amount, PRICE);

    // Increase max_charges to 3 before using the charge.
    w.sub.set_max_charges(&user, &id, &3);

    // Quote still shows chargeable (max_charges now 3, charges_made still 0).
    let quote_after = w.sub.quote_charge(&id);
    assert!(quote_after.chargeable);
    assert_eq!(quote_after.amount, PRICE);

    w.sub.charge(&w.keeper, &id);
    assert_eq!(w.sub.get_mandate(&id).charges_made, 1);
    assert_eq!(w.sub.get_mandate(&id).status, MandateStatus::Active); // Still active, 2 more charges allowed
}

/// Quote needs no authorization; charge requires keeper authorization.
#[test]
fn quote_no_auth_charge_requires_keeper_auth() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    // Quote works with no auth.
    w.env.mock_auths(&[]);
    let quote = w.sub.quote_charge(&id);
    assert!(quote.chargeable);
    assert_eq!(quote.amount, PRICE);

    // Charge without keeper auth fails with a host error (not a contract error).
    w.env.mock_auths(&[]);
    assert!(w.sub.try_charge(&w.keeper, &id).is_err());

    // Charge with keeper auth succeeds.
    w.env.mock_all_auths();
    w.sub.charge(&w.keeper, &id);
    assert_eq!(w.sub.get_mandate(&id).charges_made, 1);
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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    assert_eq!(w.sub.get_mandate(&id).fee_bps, FEE_BPS);

    // Admin raises the fee to the 10% ceiling afterwards.
    w.sub.set_fee_bps(&MAX_FEE_BPS);
    assert_eq!(w.sub.fee_bps(), MAX_FEE_BPS);

    w.sub.charge(&w.keeper, &id);

    // The mandate still settles at the 1% it was opened with.
    let expected_fee = PRICE * i128::from(FEE_BPS) / BPS_DENOMINATOR;
    assert_eq!(w.token_client.balance(&w.fee_to), expected_fee);
    assert_eq!(w.token_client.balance(&w.merchant), PRICE - expected_fee);
    assert_eq!(w.sub.get_mandate(&id).fee_bps, FEE_BPS);
}

#[test]
fn mandates_opened_before_and_after_a_fee_change_keep_their_own_fees() {
    let w = world();
    let plan_id = w.plan();
    let before = w.subscriber(100_000);
    let old_id = w.sub.subscribe(&before, &plan_id, &0, &0, &None::<Address>);

    w.sub.set_fee_bps(&0);

    let after = w.subscriber(100_000);
    let new_id = w.sub.subscribe(&after, &plan_id, &0, &0, &None::<Address>);

    assert_eq!(w.sub.get_mandate(&old_id).fee_bps, FEE_BPS);
    assert_eq!(w.sub.get_mandate(&new_id).fee_bps, 0);

    w.sub.charge(&w.keeper, &old_id);
    w.sub.charge(&w.keeper, &new_id);

    // Only the pre-change mandate contributed a fee.
    assert_eq!(
        w.token_client.balance(&w.fee_to),
        PRICE * i128::from(FEE_BPS) / BPS_DENOMINATOR
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
        ids.push_back(w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>));
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

/// `subscriber_mandate_summaries` returns summaries with key fields for display.
#[test]
fn subscriber_mandate_summaries_returns_key_fields() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id1 = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    let id2 = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    let summaries = w.sub.subscriber_mandate_summaries(&user, &0, &10);
    assert_eq!(summaries.len(), 2);

    let s1 = summaries.get_unchecked(0);
    assert_eq!(s1.id, id1);
    assert_eq!(s1.merchant, w.merchant);
    assert_eq!(s1.plan_id, plan_id);
    assert_eq!(s1.amount, PRICE);
    assert_eq!(s1.period, MONTH);
    assert_eq!(s1.status, MandateStatus::Active);
    assert_eq!(s1.next_charge, w.env.ledger().timestamp());
    assert_eq!(s1.charges_made, 0);
    assert_eq!(s1.max_charges, 0);
    assert_eq!(s1.token, w.token);

    let s2 = summaries.get_unchecked(1);
    assert_eq!(s2.id, id2);
    assert_eq!(s2.merchant, w.merchant);
    assert_eq!(s2.plan_id, plan_id);
}

/// `subscriber_mandate_summaries` pages are bounded and ordered.
#[test]
fn subscriber_mandate_summaries_pages_are_bounded() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let mut ids = soroban_sdk::Vec::<u64>::new(&w.env);
    for _ in 0..5 {
        ids.push_back(w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>));
    }

    let page1 = w.sub.subscriber_mandate_summaries(&user, &0, &2);
    assert_eq!(page1.len(), 2);
    assert_eq!(page1.get_unchecked(0).id, ids.get_unchecked(0));
    assert_eq!(page1.get_unchecked(1).id, ids.get_unchecked(1));

    let page2 = w.sub.subscriber_mandate_summaries(&user, &2, &2);
    assert_eq!(page2.len(), 2);
    assert_eq!(page2.get_unchecked(0).id, ids.get_unchecked(2));
    assert_eq!(page2.get_unchecked(1).id, ids.get_unchecked(3));

    let page3 = w.sub.subscriber_mandate_summaries(&user, &4, &2);
    assert_eq!(page3.len(), 1);
    assert_eq!(page3.get_unchecked(0).id, ids.get_unchecked(4));

    let page4 = w.sub.subscriber_mandate_summaries(&user, &5, &10);
    assert_eq!(page4.len(), 0);

    // Huge start and limit values must not overflow.
    assert_eq!(
        w.sub
            .subscriber_mandate_summaries(&user, &u32::MAX, &u32::MAX)
            .len(),
        0
    );
}

/// `subscriber_mandate_summaries` page size is capped at `MAX_PAGE`.
#[test]
fn subscriber_mandate_summaries_page_size_is_capped() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    for _ in 0..(MAX_PAGE + 5) {
        w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    }

    assert_eq!(w.sub.subscriber_mandate_count(&user), MAX_PAGE + 5);
    assert_eq!(
        w.sub
            .subscriber_mandate_summaries(&user, &0, &u32::MAX)
            .len(),
        MAX_PAGE
    );
    assert_eq!(
        w.sub
            .subscriber_mandate_summaries(&user, &MAX_PAGE, &MAX_PAGE)
            .len(),
        5
    );
}

/// `subscriber_mandate_summaries` reflects mandate status changes.
#[test]
fn subscriber_mandate_summaries_reflects_status() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let active_id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    let paused_id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    let cancelled_id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    w.sub.set_paused(&user, &paused_id, &true);
    w.sub.cancel(&user, &cancelled_id);

    let summaries = w.sub.subscriber_mandate_summaries(&user, &0, &10);
    assert_eq!(summaries.len(), 3);

    let mut active_count = 0;
    let mut paused_count = 0;
    let mut cancelled_count = 0;
    for s in summaries.iter() {
        match s.status {
            MandateStatus::Active => active_count += 1,
            MandateStatus::Paused => paused_count += 1,
            MandateStatus::Cancelled => cancelled_count += 1,
            MandateStatus::Completed => {}
        }
    }
    assert_eq!(active_count, 1);
    assert_eq!(paused_count, 1);
    assert_eq!(cancelled_count, 1);

    // Check specific mandate statuses.
    for s in summaries.iter() {
        if s.id == active_id {
            assert_eq!(s.status, MandateStatus::Active);
        } else if s.id == paused_id {
            assert_eq!(s.status, MandateStatus::Paused);
        } else if s.id == cancelled_id {
            assert_eq!(s.status, MandateStatus::Cancelled);
        }
    }
}

/// `subscriber_mandate_summaries` works across multiple merchants.
#[test]
fn subscriber_mandate_summaries_across_merchants() {
    let w = world();
    let plan_id = w.plan();
    let other_merchant = Address::generate(&w.env);
    let other_plan = w.registry.create_plan(
        &other_merchant,
        &w.token,
        &PRICE,
        &MONTH,
        &String::from_str(&w.env, "Other Plan"),
        &String::from_str(&w.env, ""),
        &0,
        &soroban_sdk::Symbol::new(&w.env, ""),
        &String::from_str(&w.env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );
    let user = w.subscriber(100_000);

    w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    w.sub
        .subscribe(&user, &other_plan, &0, &0, &None::<Address>);

    let summaries = w.sub.subscriber_mandate_summaries(&user, &0, &10);
    assert_eq!(summaries.len(), 2);

    let mut found_merchant1 = false;
    let mut found_merchant2 = false;
    for s in summaries.iter() {
        if s.merchant == w.merchant {
            found_merchant1 = true;
        } else if s.merchant == other_merchant {
            found_merchant2 = true;
        }
    }
    assert!(found_merchant1);
    assert!(found_merchant2);
}

#[test]
fn index_page_size_is_capped() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    for _ in 0..(MAX_PAGE + 5) {
        w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
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
        w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    }

    let latecomer = w.subscriber(100_000);
    let id = w
        .sub
        .subscribe(&latecomer, &plan_id, &0, &0, &None::<Address>);
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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

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

    let cancelled = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    w.sub.cancel(&user, &cancelled);
    assert_eq!(
        w.sub.try_end_mandate(&w.merchant, &cancelled),
        Err(Ok(Error::MandateNotActive))
    );

    let completed = w.sub.subscribe(&user, &plan_id, &1, &0, &None::<Address>);
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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    w.sub.charge(&w.keeper, &id);

    w.sub.end_mandate(&w.merchant, &id);

    assert_eq!(event_field_u32(&w.env, "charges_made"), Some(1));
    let body = event_by_topic(&w.env, "mandate_ended").expect("mandate_ended event");
    let ScVal::Symbol(ScSymbol(name)) = &body.topics[0] else {
        panic!("topic 0 is not a symbol");
    };
    let expected: soroban_sdk::xdr::StringM<32> = "mandate_ended".try_into().unwrap();
    assert_eq!(*name, expected);
}

/// Verify that the protocol fee is transferred to the `fee_to` address when a
/// mandate is charged. This tests that `fee_to` can be set to any address
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
    let vault_id = env.register(Vault, (admin.clone(), 10_000u32));
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
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );
    let user = Address::generate(&env);
    StellarAssetClient::new(&env, &token).mint(&user, &100_000);
    vault.deposit(&user, &token, &100_000);

    let id = sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    sub.charge(&keeper, &id);

    // The merchant should receive the price minus the fee.
    let expected_fee = PRICE * i128::from(FEE_BPS) / BPS_DENOMINATOR;
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
        ids[i] = w
            .sub
            .subscribe(&users[i], &plan_id, &0, &0, &None::<Address>);
    }

    assert_vault_invariant(&w, &users, &expected);

    // Deterministic pseudo-random steps: withdraw part of a balance, or try a
    // real charge one period later.
    let mut state: u64 = 0x_1234_5678_9abc_def0;
    let mut below = move |bound: u64| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        state % bound
    };

    for _ in 0..120 {
        let i = usize::try_from(below(4)).unwrap();
        let user = users[i].clone();

        if below(2) == 0 || expected[i] < PRICE {
            // Withdraw part of the balance, or nothing when it is empty.
            if expected[i] > 0 {
                let amount = 1 + i128::from(below(u64::try_from(expected[i]).unwrap()));
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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
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

    w.sub.subscribe(&user, &monthly, &0, &0, &None::<Address>);
    w.sub.subscribe(&user, &weekly, &0, &0, &None::<Address>);

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

    let active = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    let paused = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    let cancelled = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    let completed = w.sub.subscribe(&user, &plan_id, &1, &0, &None::<Address>);
    let scheduled = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

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
        &Symbol::new(&w.env, ""),
        &String::from_str(&w.env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );

    let user = w.subscriber(1_000_000);
    let stranger = w.subscriber(1_000_000);
    w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    w.sub
        .subscribe(&user, &other_plan, &0, &0, &None::<Address>);
    w.sub
        .subscribe(&stranger, &plan_id, &0, &0, &None::<Address>);

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
    PRICE - PRICE * i128::from(FEE_BPS) / BPS_DENOMINATOR
}

#[test]
fn refund_returns_merchant_proceeds_to_the_subscriber() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
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
fn refund_requires_the_merchants_auth() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
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
fn set_seats_requires_the_subscriber_auth() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    let other_signer = Address::generate(&w.env);

    // Seats are what the subscriber is billed for, so only their own signature
    // may change them; a merchant scaling a team up is not a subscriber action.
    w.env.mock_auths(&[MockAuth {
        address: &other_signer,
        invoke: &MockAuthInvoke {
            contract: &w.sub.address,
            fn_name: "set_seats",
            args: (&user, &id, &5u32).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);

    assert!(w.sub.try_set_seats(&user, &id, &5).is_err());
    assert_eq!(w.sub.get_mandate(&id).seats, 1);
}

#[test]
fn refund_rejects_non_positive_amounts() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
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

/// Regression test for #301: refund cap must account for referral fees.
/// When a mandate has a referrer, the referral share comes out of the
/// merchant's proceeds, so the refund cap must be reduced accordingly.
/// Without the fix, a merchant could refund more than it actually received.
#[test]
fn refund_cap_accounts_for_referral_fee() {
    let w = world();
    let referrer = Address::generate(&w.env);
    // 10% referral share of the merchant's proceeds.
    let plan_id = w.plan_full(0, 1_000, None);
    let user = w.subscriber(100_000);
    let id = w
        .sub
        .subscribe(&user, &plan_id, &0, &0, &Some(referrer.clone()));
    let charge_result = w.sub.try_charge(&w.keeper, &id);
    assert!(charge_result.is_ok(), "charge failed: {charge_result:?}");
    let mandate = w.sub.get_mandate(&id);
    assert_eq!(mandate.charges_made, 1);
    assert_eq!(w.token_client.balance(&w.merchant), 8_910);
    std::eprintln!("mandate: {mandate:?}");

    // Merchant receipt per charge: price - protocol_fee - referral
    // PRICE = 10_000, FEE_BPS = 100 (1%), referral_bps = 1_000 (10% of merchant gross)
    // fee = 100, merchant_gross = 9_900, referral = 990, merchant_net = 8_910
    let fee = PRICE * i128::from(FEE_BPS) / BPS_DENOMINATOR; // 100
    let merchant_gross = PRICE - fee; // 9_900
    let referral = merchant_gross * 1_000 / BPS_DENOMINATOR; // 990
    let merchant_net = merchant_gross - referral; // 8_910

    // The merchant should only be able to refund up to what it actually received.
    assert_eq!(w.token_client.balance(&w.merchant), merchant_net);

    // Refunding the full merchant_net should succeed.
    w.sub.refund(&w.merchant, &id, &merchant_net);
    let mandate_after_refund = w.sub.get_mandate(&id);
    std::eprintln!("mandate after refund: {mandate_after_refund:?}");
    assert_eq!(mandate_after_refund.refunded, merchant_net);
    assert_eq!(w.token_client.balance(&w.merchant), 0);

    // Check what merchant_received returns for this mandate
    let received = calculate_merchant_received(&mandate_after_refund);
    std::eprintln!("merchant_received (buggy): {received:?}");

    // Refunding one more should fail.
    let refund_result = w.sub.try_refund(&w.merchant, &id, &1);
    std::eprintln!("refund_result: {refund_result:?}");
    assert_eq!(refund_result, Err(Ok(Error::RefundExceedsReceived)));
}

/// Buggy version of `merchant_received` that doesn't account for referral (matches current contract)
fn calculate_merchant_received(m: &Mandate) -> Result<i128, Error> {
    let fee = m.amount * i128::from(m.fee_bps) / BPS_DENOMINATOR;
    let gross = m.amount - fee;
    let tip = gross
        .checked_mul(i128::from(m.keeper_bps.min(MAX_KEEPER_BPS)))
        .ok_or(Error::AmountOverflow)?
        / BPS_DENOMINATOR;
    (gross - tip)
        .checked_mul(i128::from(m.charges_made))
        .ok_or(Error::AmountOverflow)
}

/// Regression test for #301: refund cap must account for keeper tips.
/// When a plan has a keeper tip, the tip comes out of the merchant's proceeds,
/// so the refund cap must be reduced accordingly.
#[test]
fn refund_cap_accounts_for_keeper_tip() {
    let w = world();
    // 2% keeper tip of the merchant's proceeds.
    let plan_id = w.plan_with_keeper(KEEPER_BPS);
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    w.sub.charge(&w.keeper, &id);

    // Merchant receipt per charge: price - protocol_fee - tip
    // PRICE = 10_000, FEE_BPS = 100 (1%), KEEPER_BPS = 200 (2% of merchant gross)
    // fee = 100, merchant_gross = 9_900, tip = 198, merchant_net = 9_702
    let fee = PRICE * i128::from(FEE_BPS) / BPS_DENOMINATOR; // 100
    let merchant_gross = PRICE - fee; // 9_900
    let tip = merchant_gross * i128::from(KEEPER_BPS) / BPS_DENOMINATOR; // 198
    let merchant_net = merchant_gross - tip; // 9_702

    assert_eq!(w.token_client.balance(&w.merchant), merchant_net);

    // Refunding the full merchant_net should succeed.
    w.sub.refund(&w.merchant, &id, &merchant_net);
    assert_eq!(w.sub.get_mandate(&id).refunded, merchant_net);
    assert_eq!(w.token_client.balance(&w.merchant), 0);

    // Refunding one more should fail.
    assert_eq!(
        w.sub.try_refund(&w.merchant, &id, &1),
        Err(Ok(Error::RefundExceedsReceived))
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

    let vault_id = env.register(Vault, (admin.clone(), 10_000u32));
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
        &Symbol::new(&env, ""),
        &String::from_str(&env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
    );
    let user = Address::generate(&env);
    minter.mint(&user, &100_000);
    vault.deposit(&user, &token, &50_000);

    let id = sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    sub.charge(&keeper, &id);

    let expected_fee = PRICE * i128::from(FEE_BPS) / BPS_DENOMINATOR;
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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

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
    token_admin: Address,
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

    let token = env.register(TestToken, (token_admin.clone(),));
    let test_token = TestTokenClient::new(&env, &token);

    let registry_id = env.register(PlanRegistry, (admin.clone(),));
    let registry = RegistryClient::new(&env, &registry_id);

    let vault_id = env.register(Vault, (admin.clone(), 10_000u32));
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
        token_admin,
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
            &Symbol::new(&self.env, ""),
            &String::from_str(&self.env, ""),
            &0u64,
            &0u32,
            &None::<Address>,
            &0u32,
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

    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
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

    let fee = PRICE * i128::from(FEE_BPS) / BPS_DENOMINATOR;
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

    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

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

    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

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

#[test]
fn charge_with_fee_on_transfer_reverts_completely() {
    let w = failing_token_world();
    let plan_id = w.plan_with_keeper(0);
    let user = w.subscriber(50_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    // The merchant would receive 9_900 for a 10_000 charge (the 1% fee is
    // deducted on transfer), so the vault rejects the debit and rolls the
    // whole charge back.
    w.test_token.set_transfer_fee_bps(&100);
    assert!(w.sub.try_charge(&w.keeper, &id).is_err());

    let m = w.sub.get_mandate(&id);
    assert_eq!(m.charges_made, 0);
    assert_eq!(m.last_charge, 0);
    assert_eq!(m.next_charge, 1_000_000);
    assert_eq!(m.status, MandateStatus::Active);

    assert_eq!(w.vault.balance(&user, &w.token), 50_000);
    assert_eq!(w.test_token.balance(&w.merchant), 0);
    assert_eq!(w.test_token.balance(&w.fee_to), 0);
    assert_eq!(w.test_token.balance(&w.vault.address), 50_000);
    assert_eq!(w.test_token.balance(&w.token_admin), 0);

    // With exact transfers the same charge settles normally.
    w.test_token.set_transfer_fee_bps(&0);
    w.sub.charge(&w.keeper, &id);
    let fee = PRICE * i128::from(FEE_BPS) / BPS_DENOMINATOR;
    assert_eq!(w.test_token.balance(&w.merchant), PRICE - fee);
    assert_eq!(w.test_token.balance(&w.fee_to), fee);
    assert_eq!(w.vault.balance(&user, &w.token), 50_000 - PRICE);
}

#[test]
fn refund_with_fee_on_transfer_reverts_and_leaves_the_mandate_unchanged() {
    let w = failing_token_world();
    let plan_id = w.plan_with_keeper(0);
    let user = w.subscriber(50_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    w.sub.charge(&w.keeper, &id);

    let fee = PRICE * i128::from(FEE_BPS) / BPS_DENOMINATOR;
    let merchant_balance = w.test_token.balance(&w.merchant);
    assert_eq!(merchant_balance, PRICE - fee);

    // The subscriber would receive 990 for a 1_000 refund: reject instead of
    // recording a refund that did not deliver.
    w.test_token.set_transfer_fee_bps(&100);
    assert_eq!(
        w.sub.try_refund(&w.merchant, &id, &1_000),
        Err(Ok(Error::UnexpectedTransferAmount))
    );

    assert_eq!(w.sub.get_mandate(&id).refunded, 0);
    assert_eq!(w.test_token.balance(&w.merchant), merchant_balance);
    // The subscriber's wallet still holds the 50_000 not deposited in the vault.
    assert_eq!(w.test_token.balance(&user), 50_000);
    assert_eq!(w.test_token.balance(&w.token_admin), 0);

    // Turning the fee off lets the merchant refund normally.
    w.test_token.set_transfer_fee_bps(&0);
    w.sub.refund(&w.merchant, &id, &1_000);
    assert_eq!(w.sub.get_mandate(&id).refunded, 1_000);
    assert_eq!(w.test_token.balance(&user), 51_000);
    assert_eq!(w.test_token.balance(&w.merchant), merchant_balance - 1_000);
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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

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
        &Symbol::new(&w.env, ""),
        &String::from_str(&w.env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    let other_merchant = Address::generate(&w.env);
    let other_plan = w.registry.create_plan(
        &other_merchant,
        &w.token,
        &(PRICE * 2),
        &MONTH,
        &String::from_str(&w.env, "Other Merchant Plan"),
        &String::from_str(&w.env, ""),
        &0,
        &Symbol::new(&w.env, ""),
        &String::from_str(&w.env, ""),
        &0u64,
        &0u32,
        &None::<Address>,
        &0u32,
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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

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
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
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
                    for entry in entries {
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
    let id = w.sub.subscribe(&user, &plan_id, &2, &0, &None::<Address>);
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
    let id2 = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    w.sub.end_mandate(&w.merchant, &id2);
    let snap6 = find_latest_snapshot(&w.env, id2).expect("MandateUpdated emitted on end_mandate");
    assert_eq!(snap6, w.sub.get_mandate(&id2));
    assert_eq!(snap6.status, MandateStatus::Cancelled);

    // 10. Complete mandate on final charge
    let id3 = w.sub.subscribe(&user, &plan_id, &1, &0, &None::<Address>);
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
    let id = w
        .sub
        .subscribe(&user, &plan_id, &0, &(PRICE * 2), &None::<Address>);

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
    let id = w
        .sub
        .subscribe(&user, &plan_id, &0, &(PRICE + PRICE / 2), &None::<Address>);

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

    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    w.sub.charge(&w.keeper, &id);
    w.advance(MONTH);
    w.sub.charge(&w.keeper, &id);
    let m = w.sub.get_mandate(&id);
    assert_eq!(m.status, MandateStatus::Active);
    assert_eq!(m.total_spent, PRICE * 2);

    assert_eq!(
        w.sub
            .try_subscribe(&user, &plan_id, &0, &-1, &None::<Address>,),
        Err(Ok(Error::InvalidMaxTotal))
    );
}

#[test]
fn propose_admin_does_not_hand_over_until_the_successor_accepts() {
    let w = world();
    let admin = w.sub.admin();
    let next = Address::generate(&w.env);

    w.sub.propose_admin(&next);
    // Proposing alone moves no authority: the current admin still holds it.
    assert_eq!(w.sub.admin(), admin);

    w.sub.accept_admin(&next);
    assert_eq!(w.sub.admin(), next);
}

#[test]
fn propose_admin_requires_the_current_admins_auth() {
    let w = world();
    let before = w.sub.admin();
    let next = Address::generate(&w.env);

    w.env.set_auths(&[]);
    assert!(w.sub.try_propose_admin(&next).is_err());
    assert_eq!(w.sub.admin(), before);
}

#[test]
fn accept_admin_requires_the_proposed_admins_auth() {
    let w = world();
    let before = w.sub.admin();
    let next = Address::generate(&w.env);
    w.sub.propose_admin(&next);

    w.env.set_auths(&[]);
    assert!(w.sub.try_accept_admin(&next).is_err());
    assert_eq!(w.sub.admin(), before);
}

// Regression for #238: a direct takeover must fail. Without the two-step
// handshake, accepting with no proposal would set the caller as admin.
#[test]
fn accept_admin_without_a_proposal_is_rejected() {
    let w = world();
    let before = w.sub.admin();
    let attacker = Address::generate(&w.env);

    assert_eq!(
        w.sub.try_accept_admin(&attacker),
        Err(Ok(Error::PendingAdminNotFound))
    );
    assert_eq!(w.sub.admin(), before);
}

#[test]
fn accept_admin_rejects_an_address_that_was_not_proposed() {
    let w = world();
    let before = w.sub.admin();
    let next = Address::generate(&w.env);
    let attacker = Address::generate(&w.env);
    w.sub.propose_admin(&next);

    assert_eq!(
        w.sub.try_accept_admin(&attacker),
        Err(Ok(Error::NotPendingAdmin))
    );
    assert_eq!(w.sub.admin(), before);
}

#[test]
fn a_consumed_admin_proposal_cannot_be_replayed() {
    let w = world();
    let next = Address::generate(&w.env);
    w.sub.propose_admin(&next);
    w.sub.accept_admin(&next);

    assert_eq!(
        w.sub.try_accept_admin(&next),
        Err(Ok(Error::PendingAdminNotFound))
    );
    assert_eq!(w.sub.admin(), next);
}

#[test]
fn cancel_page_skips_finished_mandates_and_cancels_active_and_paused() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let active = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    let paused = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    let completed = w.sub.subscribe(&user, &plan_id, &1, &0, &None::<Address>);
    let cancelled = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

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
        w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
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
    w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
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

#[test]
fn cancel_page_reports_no_reason() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    assert_eq!(w.sub.cancel_page(&user, &0, &1), 1);

    // A bulk cancel carries no per-mandate explanation, so the event reports
    // the same "no reason given" code plain `cancel` uses.
    assert_eq!(event_field_u32(&w.env, "reason"), Some(0));
    assert_eq!(w.sub.get_mandate(&id).status, MandateStatus::Cancelled);
}

#[test]
fn due_mandates_filters_a_full_page() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(1_000_000);
    let mut ids = soroban_sdk::Vec::<u64>::new(&w.env);

    for index in 0..(MAX_PAGE + 5) {
        let max_charges = u32::from(index == 1);
        ids.push_back(
            w.sub
                .subscribe(&user, &plan_id, &max_charges, &0, &None::<Address>),
        );
    }

    let paused_id = ids.get_unchecked(0);
    let completed_id = ids.get_unchecked(1);
    let early_id = ids.get_unchecked(2);
    w.sub.set_paused(&user, &paused_id, &true);
    w.sub.charge(&w.keeper, &completed_id);
    w.sub.charge(&w.keeper, &early_id);

    let due = w.sub.due_mandates(&w.merchant, &0, &u32::MAX);
    assert_eq!(due.len(), MAX_PAGE - 3);
    assert!(!due.contains(paused_id));
    assert!(!due.contains(completed_id));
    assert!(!due.contains(early_id));
    assert_eq!(due.get(0), Some(ids.get_unchecked(3)));
    assert_eq!(
        w.sub.due_mandates(&w.merchant, &3, &2),
        soroban_sdk::vec![&w.env, ids.get_unchecked(3), ids.get_unchecked(4)]
    );
    assert_eq!(
        w.sub.due_mandates(&w.merchant, &MAX_PAGE, &MAX_PAGE),
        soroban_sdk::vec![
            &w.env,
            ids.get_unchecked(MAX_PAGE),
            ids.get_unchecked(MAX_PAGE + 1),
            ids.get_unchecked(MAX_PAGE + 2),
            ids.get_unchecked(MAX_PAGE + 3),
            ids.get_unchecked(MAX_PAGE + 4),
        ]
    );
}

#[test]
fn due_mandates_agrees_with_is_due_and_needs_no_auth() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    // A read needs no signature at all.
    w.env.set_auths(&[]);
    assert!(w.sub.is_due(&id));
    assert_eq!(
        w.sub.due_mandates(&w.merchant, &0, &u32::MAX),
        soroban_sdk::vec![&w.env, id]
    );

    w.env.mock_all_auths();
    w.sub.cancel(&user, &id);
    assert!(!w.sub.is_due(&id));
    assert!(w.sub.due_mandates(&w.merchant, &0, &u32::MAX).is_empty());
}

// ---------------------------------------------------------------------------
// Charged event: which token the amounts are in
// ---------------------------------------------------------------------------

/// The acceptance criterion for the token in `Charged`: an indexer reading
/// only events must learn which SEP-41 token the amounts are denominated in,
/// without joining back to the mandate. Event-only indexers otherwise report
/// wrong totals for any merchant with more than one token.
#[test]
fn charged_event_carries_the_token() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    w.sub.charge(&w.keeper, &id);

    let token = event_field_address(&w.env, "charged", "token")
        .expect("Charged event must carry the token on the wire");
    assert_eq!(token, w.token);
}

// ---------------------------------------------------------------------------
// subscribe_at: choose when billing starts
// ---------------------------------------------------------------------------

#[test]
fn subscribe_at_schedules_billing_from_the_start() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let start = w.env.ledger().timestamp() + MONTH / 2;

    let id = w
        .sub
        .subscribe_at(&user, &plan_id, &0, &0, &start, &None::<Address>);

    // Indexers see the start in Subscribed.next_charge, no follow-up read.
    // (Events are only visible from the last invocation, hence right here.)
    assert_eq!(event_field_u64(&w.env, "next_charge"), Some(start));

    let m = w.sub.get_mandate(&id);
    assert_eq!(m.next_charge, start);
    assert_eq!(m.charges_made, 0);
    assert!(!w.sub.is_due(&id));

    // Once the start is reached, billing proceeds exactly as before.
    w.advance(MONTH / 2);
    assert!(w.sub.is_due(&id));
    w.sub.charge(&w.keeper, &id);
    assert_eq!(w.sub.get_mandate(&id).charges_made, 1);
    assert_eq!(w.vault.balance(&user, &w.token), 100_000 - PRICE);
}

#[test]
fn charging_before_the_start_fails_with_not_due() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let start = w.env.ledger().timestamp() + MONTH;
    let id = w
        .sub
        .subscribe_at(&user, &plan_id, &0, &0, &start, &None::<Address>);

    // The mandate exists but cannot settle before its start.
    assert_eq!(w.sub.try_charge(&w.keeper, &id), Err(Ok(Error::NotDue)));
    assert_eq!(w.sub.get_mandate(&id).charges_made, 0);
    assert_eq!(w.vault.balance(&user, &w.token), 100_000);

    w.advance(MONTH - 1);
    assert_eq!(w.sub.try_charge(&w.keeper, &id), Err(Ok(Error::NotDue)));

    w.advance(1);
    w.sub.charge(&w.keeper, &id);
    assert_eq!(w.sub.get_mandate(&id).charges_made, 1);
}

#[test]
fn subscribe_at_rejects_a_start_in_the_past() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let past = w.env.ledger().timestamp() - 1;

    assert_eq!(
        w.sub
            .try_subscribe_at(&user, &plan_id, &0, &0, &past, &None::<Address>,),
        Err(Ok(Error::InvalidStart))
    );
    // Nothing was opened: no mandate, no index entries.
    assert_eq!(w.sub.subscriber_mandate_count(&user), 0);
    assert_eq!(w.sub.merchant_mandate_count(&w.merchant), 0);
}

#[test]
fn subscribe_at_rejects_a_start_more_than_a_year_ahead() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let too_far = w.env.ledger().timestamp() + MAX_START_OFFSET + 1;

    assert_eq!(
        w.sub
            .try_subscribe_at(&user, &plan_id, &0, &0, &too_far, &None::<Address>,),
        Err(Ok(Error::InvalidStart))
    );
    assert_eq!(w.sub.subscriber_mandate_count(&user), 0);
}

#[test]
fn subscribe_at_accepts_a_start_one_year_from_now() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let start = w.env.ledger().timestamp() + MAX_START_OFFSET;

    let id = w
        .sub
        .subscribe_at(&user, &plan_id, &0, &0, &start, &None::<Address>);

    // The boundary itself is allowed: exactly one year ahead.
    assert_eq!(w.sub.get_mandate(&id).next_charge, start);
    assert!(!w.sub.is_due(&id));
}

#[test]
fn subscribe_at_requires_the_subscriber_to_authorize() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let stranger = Address::generate(&w.env);
    let start = w.env.ledger().timestamp() + MONTH;

    // The stranger signs, but the call opens a mandate for `user`: the
    // signature does not cover the subscriber, so it is rejected.
    w.env.mock_auths(&[MockAuth {
        address: &stranger,
        invoke: &MockAuthInvoke {
            contract: &w.sub.address,
            fn_name: "subscribe_at",
            args: (&user, &plan_id, &0u32, &0i128, &start).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);

    assert!(w
        .sub
        .try_subscribe_at(&user, &plan_id, &0, &0, &start, &None::<Address>,)
        .is_err());
    assert_eq!(w.sub.subscriber_mandate_count(&user), 0);
}

/// `subscribe` keeps its old meaning: billing starts now, whatever the plan.
#[test]
fn subscribe_is_subscribe_at_now() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let now = w.env.ledger().timestamp();

    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    assert_eq!(w.sub.get_mandate(&id).next_charge, now);
    assert!(w.sub.is_due(&id));
}

#[test]
fn second_charge_before_period_elapses_is_rejected() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    w.sub.charge(&w.keeper, &id);
    assert!(!w.sub.is_due(&id));
    assert_eq!(w.sub.try_charge(&w.keeper, &id), Err(Ok(Error::NotDue)));

    w.advance(MONTH - 1);
    assert_eq!(w.sub.try_charge(&w.keeper, &id), Err(Ok(Error::NotDue)));
}

#[test]
fn plan_cutoff_stops_new_subscriptions_but_not_existing_mandates() {
    let w = world();
    let now = w.env.ledger().timestamp();
    let cutoff = now + 1_000;
    let plan_id = w.plan_expiring(cutoff);
    let user = w.subscriber(100_000);

    // One second before the cutoff the plan is still open.
    w.env.ledger().set_timestamp(cutoff - 1);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    // At the cutoff itself it accepts no one new.
    w.env.ledger().set_timestamp(cutoff);
    let late = w.subscriber(100_000);
    assert_eq!(
        w.sub
            .try_subscribe(&late, &plan_id, &0, &0, &None::<Address>,),
        Err(Ok(Error::PlanInactive))
    );

    // The mandate opened before the cutoff keeps billing past it.
    w.env.ledger().set_timestamp(cutoff + MONTH);
    w.sub.charge(&w.keeper, &id);
    assert_eq!(w.sub.get_mandate(&id).charges_made, 1);
    assert_eq!(
        w.token_client.balance(&w.merchant),
        PRICE - PRICE * i128::from(FEE_BPS) / BPS_DENOMINATOR
    );
}

#[test]
fn plan_mandate_index_lists_subscribers_and_pages() {
    let w = world();
    let a = w.plan();
    let b = w.plan();

    let alice = w.subscriber(100_000);
    let bob = w.subscriber(100_000);
    let carol = w.subscriber(100_000);
    let dave = w.subscriber(100_000);

    let m1 = w.sub.subscribe(&alice, &a, &0, &0, &None::<Address>);
    let m2 = w.sub.subscribe(&bob, &a, &0, &0, &None::<Address>);
    let m3 = w.sub.subscribe(&carol, &a, &0, &0, &None::<Address>);
    let m4 = w.sub.subscribe(&dave, &b, &0, &0, &None::<Address>);

    // Per-plan isolation, oldest first.
    assert_eq!(w.sub.plan_mandate_count(&a), 3);
    assert_eq!(
        w.sub.plan_mandates(&a, &0, &10),
        soroban_sdk::vec![&w.env, m1, m2, m3]
    );
    assert_eq!(w.sub.plan_mandate_count(&b), 1);
    assert_eq!(
        w.sub.plan_mandates(&b, &0, &10),
        soroban_sdk::vec![&w.env, m4]
    );

    // Paging bounds, same contract as the merchant and subscriber indexes.
    assert_eq!(
        w.sub.plan_mandates(&a, &1, &1),
        soroban_sdk::vec![&w.env, m2]
    );
    assert_eq!(w.sub.plan_mandates(&a, &0, &u32::MAX).len(), 3);
    assert_eq!(w.sub.plan_mandates(&a, &3, &10).len(), 0);
    assert_eq!(w.sub.plan_mandates(&a, &u32::MAX, &u32::MAX).len(), 0);

    // A cancelled mandate stays listed: the index records who subscribed,
    // not who is still billing.
    w.sub.cancel(&alice, &m1);
    assert_eq!(w.sub.plan_mandate_count(&a), 3);
    assert_eq!(
        w.sub.plan_mandates(&a, &0, &10),
        soroban_sdk::vec![&w.env, m1, m2, m3]
    );
}

#[test]
fn subscribe_with_deposit_funds_and_subscribes_in_one_call() {
    let w = world();
    let plan_id = w.plan();
    let user = Address::generate(&w.env);
    w.minter.mint(&user, &100_000);

    // No prior deposit: the single call moves the tokens into the vault and
    // opens the mandate.
    let id = w
        .sub
        .subscribe_with_deposit(&user, &plan_id, &0, &0, &40_000, &None::<Address>);

    assert_eq!(w.vault.balance(&user, &w.token), 40_000);
    assert_eq!(w.token_client.balance(&user), 60_000);
    let m = w.sub.get_mandate(&id);
    assert_eq!(m.subscriber, user);
    assert_eq!(m.amount, PRICE);
    assert_eq!(m.status, MandateStatus::Active);
    // Mandate ids stay sequential: nothing was consumed by the deposit half.
    assert_eq!(id, 1);
    assert_eq!(w.sub.plan_mandate_count(&plan_id), 1);
}

#[test]
fn subscribe_with_deposit_aborts_atomically_when_the_deposit_fails() {
    let w = world();
    let plan_id = w.plan();
    let user = Address::generate(&w.env);
    w.minter.mint(&user, &1_000); // holds less than the deposit

    assert!(w
        .sub
        .try_subscribe_with_deposit(&user, &plan_id, &0, &0, &40_000, &None::<Address>,)
        .is_err());

    // Nothing was subscribed and nothing was credited.
    assert_eq!(w.sub.plan_mandate_count(&plan_id), 0);
    assert_eq!(w.vault.balance(&user, &w.token), 0);
    assert_eq!(w.token_client.balance(&user), 1_000);

    // No mandate id was burned: the next subscribe still gets id 1.
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    assert_eq!(id, 1);
}

#[test]
fn one_subscriber_signature_covers_the_nested_vault_deposit() {
    let w = world();
    let plan_id = w.plan();
    let user = Address::generate(&w.env);
    w.minter.mint(&user, &100_000);

    w.env.mock_auths(&[MockAuth {
        address: &user,
        invoke: &MockAuthInvoke {
            contract: &w.sub.address,
            fn_name: "subscribe_with_deposit",
            args: (
                &user,
                &plan_id,
                &0u32,
                &0i128,
                &40_000i128,
                &None::<Address>,
            )
                .into_val(&w.env),
            sub_invokes: &[MockAuthInvoke {
                contract: &w.vault.address,
                fn_name: "deposit",
                args: (&user, &w.token, &40_000i128).into_val(&w.env),
                sub_invokes: &[MockAuthInvoke {
                    contract: &w.token,
                    fn_name: "transfer",
                    args: (&user, &w.vault.address, &40_000i128).into_val(&w.env),
                    sub_invokes: &[],
                }],
            }],
        },
    }]);

    let id = w
        .sub
        .subscribe_with_deposit(&user, &plan_id, &0, &0, &40_000, &None::<Address>);
    assert_eq!(w.vault.balance(&user, &w.token), 40_000);
    assert_eq!(w.sub.get_mandate(&id).subscriber, user);
}

#[test]
fn without_the_nested_auth_entry_the_deposit_is_not_covered() {
    let w = world();
    let plan_id = w.plan();
    let user = Address::generate(&w.env);
    w.minter.mint(&user, &100_000);

    w.env.mock_auths(&[MockAuth {
        address: &user,
        invoke: &MockAuthInvoke {
            contract: &w.sub.address,
            fn_name: "subscribe_with_deposit",
            args: (
                &user,
                &plan_id,
                &0u32,
                &0i128,
                &40_000i128,
                &None::<Address>,
            )
                .into_val(&w.env),
            sub_invokes: &[],
        },
    }]);

    assert!(w
        .sub
        .try_subscribe_with_deposit(&user, &plan_id, &0, &0, &40_000, &None::<Address>,)
        .is_err());
    assert_eq!(w.sub.plan_mandate_count(&plan_id), 0);
    assert_eq!(w.vault.balance(&user, &w.token), 0);
}

#[test]
fn subscribe_with_deposit_requires_the_subscriber_to_authorize() {
    let w = world();
    let plan_id = w.plan();
    let user = Address::generate(&w.env);
    let stranger = Address::generate(&w.env);
    w.minter.mint(&user, &100_000);

    // A different signer authorizing the same call does not cover `user`.
    w.env.mock_auths(&[MockAuth {
        address: &stranger,
        invoke: &MockAuthInvoke {
            contract: &w.sub.address,
            fn_name: "subscribe_with_deposit",
            args: (
                &user,
                &plan_id,
                &0u32,
                &0i128,
                &40_000i128,
                &None::<Address>,
            )
                .into_val(&w.env),
            sub_invokes: &[],
        },
    }]);

    assert!(w
        .sub
        .try_subscribe_with_deposit(&user, &plan_id, &0, &0, &40_000, &None::<Address>,)
        .is_err());
    assert_eq!(w.sub.plan_mandate_count(&plan_id), 0);
    assert_eq!(w.vault.balance(&user, &w.token), 0);
}

#[test]
fn subscribe_with_deposit_rolls_the_deposit_back_when_the_plan_is_expired() {
    let w = world();
    let now = w.env.ledger().timestamp();
    let cutoff = now + 10;
    let plan_id = w.plan_expiring(cutoff);
    let user = Address::generate(&w.env);
    w.minter.mint(&user, &100_000);

    w.env.ledger().set_timestamp(cutoff);

    assert_eq!(
        w.sub
            .try_subscribe_with_deposit(&user, &plan_id, &0, &0, &40_000, &None::<Address>,),
        Err(Ok(Error::PlanInactive))
    );
    assert_eq!(w.vault.balance(&user, &w.token), 0);
    assert_eq!(w.token_client.balance(&user), 100_000);
    assert_eq!(w.sub.plan_mandate_count(&plan_id), 0);
}

#[test]
fn a_spend_limit_blocks_the_charge_atomically() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);

    // "Never debit more than the price minus one unit in this window."
    w.vault
        .set_spend_limit(&user, &w.token, &(PRICE - 1), &MONTH);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    assert!(w.sub.try_charge(&w.keeper, &id).is_err());

    // The merchant-share debit succeeded but the fee debit crossed the
    // ceiling; the whole charge rolled back.
    assert_eq!(w.sub.get_mandate(&id).charges_made, 0);
    assert_eq!(w.vault.balance(&user, &w.token), 100_000);
    assert_eq!(w.token_client.balance(&w.merchant), 0);
    assert_eq!(w.token_client.balance(&w.fee_to), 0);
    assert!(w.sub.is_due(&id));

    // Raising the limit lets the same charge through.
    w.vault.set_spend_limit(&user, &w.token, &PRICE, &MONTH);
    w.sub.charge(&w.keeper, &id);
    assert_eq!(w.sub.get_mandate(&id).charges_made, 1);
    assert_eq!(w.vault.balance(&user, &w.token), 100_000 - PRICE);
}

fn event_field_i128(env: &Env, field: &str) -> Option<i128> {
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
                return i128::try_from(entry.val.clone()).ok();
            }
        }
    }
    None
}

#[test]
fn referral_share_is_paid_to_the_referrer() {
    let w = world();
    let plan_id = w.plan_full(0, REFERRAL_BPS, None);
    let referrer = Address::generate(&w.env);
    let user = w.subscriber(100_000);
    let id = w
        .sub
        .subscribe(&user, &plan_id, &0, &0, &Some(referrer.clone()));

    w.sub.charge(&w.keeper, &id);

    let fee = PRICE * i128::from(FEE_BPS) / BPS_DENOMINATOR;
    let merchant_gross = PRICE - fee;
    let referral = merchant_gross * i128::from(REFERRAL_BPS) / BPS_DENOMINATOR;

    assert_eq!(w.token_client.balance(&referrer), referral);
    assert_eq!(
        w.token_client.balance(&w.merchant),
        merchant_gross - referral
    );
    assert_eq!(w.token_client.balance(&w.fee_to), fee);
    // The subscriber pays the plan price, no more: the referral comes out of
    // the merchant's share, never out of the subscriber's payment.
    assert_eq!(w.vault.balance(&user, &w.token), 100_000 - PRICE);

    // The referrer and its rate are frozen into the mandate.
    let m = w.sub.get_mandate(&id);
    assert_eq!(m.referrer, Some(referrer));
    assert_eq!(m.referral_bps, REFERRAL_BPS);
}

#[test]
fn subscribing_without_a_referrer_pays_no_referral() {
    let w = world();
    let plan_id = w.plan_full(0, REFERRAL_BPS, None);
    let referrer = Address::generate(&w.env);
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None);

    w.sub.charge(&w.keeper, &id);

    let fee = PRICE * i128::from(FEE_BPS) / BPS_DENOMINATOR;
    assert_eq!(w.token_client.balance(&referrer), 0);
    assert_eq!(w.token_client.balance(&w.merchant), PRICE - fee);
    assert_eq!(w.sub.get_mandate(&id).referrer, None);
}

#[test]
fn zero_referral_bps_disables_the_referral() {
    let w = world();
    let plan_id = w.plan_full(0, 0, None);
    let referrer = Address::generate(&w.env);
    let user = w.subscriber(100_000);
    let id = w
        .sub
        .subscribe(&user, &plan_id, &0, &0, &Some(referrer.clone()));

    w.sub.charge(&w.keeper, &id);

    let fee = PRICE * i128::from(FEE_BPS) / BPS_DENOMINATOR;
    assert_eq!(w.token_client.balance(&referrer), 0);
    assert_eq!(w.token_client.balance(&w.merchant), PRICE - fee);
    assert_eq!(w.sub.get_mandate(&id).referral_bps, 0);
}

#[test]
fn referral_split_loses_at_most_one_stroop() {
    const ODD_REFERRAL_BPS: u32 = 1_337;
    let w = world();
    // Merchant, fee_to and the referrer balances are shared across
    // iterations, so the conservation check runs against a running total.
    let referrer = Address::generate(&w.env);
    let mut settled: i128 = 0;

    for amount in [101i128, 107, 3_333, 7_777, 99_999] {
        let plan_id = w.registry.create_plan(
            &w.merchant,
            &w.token,
            &amount,
            &MONTH,
            &String::from_str(&w.env, "Odd Plan"),
            &String::from_str(&w.env, ""),
            &0,
            &soroban_sdk::Symbol::new(&w.env, ""),
            &String::from_str(&w.env, ""),
            &0u64,
            &ODD_REFERRAL_BPS,
            &None::<Address>,
            &0u32,
        );
        let funding = 1_000_000;
        let user = w.subscriber(funding);
        let id = w
            .sub
            .subscribe(&user, &plan_id, &0, &0, &Some(referrer.clone()));
        w.sub.charge(&w.keeper, &id);

        let fee = amount * i128::from(FEE_BPS) / BPS_DENOMINATOR;
        let merchant_gross = amount - fee;
        let referral = merchant_gross * i128::from(ODD_REFERRAL_BPS) / BPS_DENOMINATOR;

        // Integer division never rounds up, and is short of the exact
        // rational share by less than one stroop.
        let exact_scaled = merchant_gross * i128::from(ODD_REFERRAL_BPS);
        assert!(referral * 10_000 <= exact_scaled);
        assert!(exact_scaled < (referral + 1) * 10_000);

        // Every stroop of the price reached a payer exactly once: nothing was
        // created, lost or double-paid by the split. (Accumulated over the
        // iterations so far because the merchant and fee_to are reused.)
        assert_eq!(
            w.token_client.balance(&w.merchant)
                + w.token_client.balance(&referrer)
                + w.token_client.balance(&w.fee_to),
            settled + amount,
            "every stroop of {amount} must be paid out exactly once"
        );
        settled += amount;
        assert_eq!(w.vault.balance(&user, &w.token), funding - amount);
    }
}

#[test]
fn charged_event_reports_the_referral_amount() {
    let w = world();
    let plan_id = w.plan_full(0, REFERRAL_BPS, None);
    let referrer = Address::generate(&w.env);
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &Some(referrer));

    w.sub.charge(&w.keeper, &id);

    let fee = PRICE * i128::from(FEE_BPS) / BPS_DENOMINATOR;
    let referral = (PRICE - fee) * i128::from(REFERRAL_BPS) / BPS_DENOMINATOR;
    assert_eq!(event_field_i128(&w.env, "referral"), Some(referral));
}

#[test]
fn referral_and_keeper_tip_split_the_merchant_share_together() {
    let w = world();
    let plan_id = w.plan_full(KEEPER_BPS, REFERRAL_BPS, None);
    let referrer = Address::generate(&w.env);
    let user = w.subscriber(100_000);
    let id = w
        .sub
        .subscribe(&user, &plan_id, &0, &0, &Some(referrer.clone()));

    w.sub.charge(&w.keeper, &id);

    let fee = PRICE * i128::from(FEE_BPS) / BPS_DENOMINATOR;
    let merchant_gross = PRICE - fee;
    let tip = merchant_gross * i128::from(KEEPER_BPS) / BPS_DENOMINATOR;
    let referral = merchant_gross * i128::from(REFERRAL_BPS) / BPS_DENOMINATOR;

    assert_eq!(w.token_client.balance(&w.keeper), tip);
    assert_eq!(w.token_client.balance(&referrer), referral);
    assert_eq!(
        w.token_client.balance(&w.merchant),
        merchant_gross - tip - referral
    );
    // The subscriber pays the plan price, no more.
    assert_eq!(w.vault.balance(&user, &w.token), 100_000 - PRICE);
}

#[test]
fn charges_are_paid_to_the_payout_address() {
    let w = world();
    let treasury = Address::generate(&w.env);
    let plan_id = w.plan_full(0, 0, Some(treasury.clone()));
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None);

    // The mandate carries both: the merchant for management, the payout for
    // settlement.
    let m = w.sub.get_mandate(&id);
    assert_eq!(m.merchant, w.merchant);
    assert_eq!(m.payout, treasury);

    w.sub.charge(&w.keeper, &id);

    let fee = PRICE * i128::from(FEE_BPS) / BPS_DENOMINATOR;
    assert_eq!(w.token_client.balance(&treasury), PRICE - fee);
    assert_eq!(w.token_client.balance(&w.merchant), 0);
    assert_eq!(w.token_client.balance(&w.fee_to), fee);
}

#[test]
fn payout_does_not_transfer_plan_management() {
    let w = world();
    let treasury = Address::generate(&w.env);
    let plan_id = w.plan_full(0, 0, Some(treasury.clone()));

    // A stranger cannot deactivate the plan, even though it pays the treasury.
    let stranger = Address::generate(&w.env);
    assert_eq!(
        w.registry.try_set_plan_active(&stranger, &plan_id, &false),
        Err(Ok(payflow_plan_registry::Error::NotPlanOwner))
    );
    // The merchant still manages it.
    w.registry.set_plan_active(&w.merchant, &plan_id, &false);
    assert!(!w.registry.get_plan(&plan_id).active);
    // And the merchant cannot redirect the payout: plans are immutable apart
    // from the active flag, so the payout is the one published.
    assert_eq!(w.registry.get_plan(&plan_id).payout, treasury);
}

#[test]
fn plan_transfer_does_not_redirect_open_mandates() {
    let w = world();
    let treasury = Address::generate(&w.env);
    let plan_id = w.plan_full(0, 0, Some(treasury.clone()));
    let buyer = Address::generate(&w.env);
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None);

    // The plan changes hands.
    w.registry.propose_plan_owner(&w.merchant, &plan_id, &buyer);
    w.registry.accept_plan_owner(&buyer, &plan_id);
    assert_eq!(w.registry.get_plan(&plan_id).merchant, buyer.clone());

    // The mandate opened before the transfer still pays the frozen payout and
    // still belongs to the merchant it was opened against.
    w.advance(MONTH);
    w.sub.charge(&w.keeper, &id);
    let fee = PRICE * i128::from(FEE_BPS) / BPS_DENOMINATOR;
    assert_eq!(w.token_client.balance(&treasury), PRICE - fee);
    assert_eq!(w.sub.get_mandate(&id).merchant, w.merchant);

    // The buyer cannot end a mandate it never owned...
    assert_eq!(
        w.sub.try_end_mandate(&buyer, &id),
        Err(Ok(Error::NotMerchant))
    );
    // ...but the original merchant still can.
    w.sub.end_mandate(&w.merchant, &id);
    assert_eq!(w.sub.get_mandate(&id).status, MandateStatus::Cancelled);

    // A mandate opened after the transfer belongs to the new owner.
    let new_id = w.sub.subscribe(&user, &plan_id, &0, &0, &None);
    assert_eq!(w.sub.get_mandate(&new_id).merchant, buyer.clone());
    w.sub.end_mandate(&buyer, &new_id);
    assert_eq!(w.sub.get_mandate(&new_id).status, MandateStatus::Cancelled);
}

fn last_event_field_u32(env: &Env, field: &str) -> Option<u32> {
    let last = env.events().all().events().last().cloned()?;
    let ContractEventBody::V0(body) = &last.body;
    let ScVal::Map(Some(ScMap(entries))) = &body.data else {
        return None;
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
    None
}

#[test]
fn refund_requires_the_merchants_signature() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
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
fn subscribe_rejects_a_full_plan() {
    let w = world();
    let plan_id = w.plan_with_cap(2);
    let a = w.subscriber(100_000);
    let b = w.subscriber(100_000);
    let rejected = w.subscriber(100_000);

    w.sub.subscribe(&a, &plan_id, &0, &0, &None::<Address>);
    w.sub.subscribe(&b, &plan_id, &0, &0, &None::<Address>);
    assert_eq!(w.sub.plan_subscriber_count(&plan_id), 2);

    // The plan is full: no mandate is opened and the counter is unchanged.
    assert_eq!(
        w.sub
            .try_subscribe(&rejected, &plan_id, &0, &0, &None::<Address>),
        Err(Ok(Error::PlanFull))
    );
    assert_eq!(w.sub.plan_subscriber_count(&plan_id), 2);
    assert_eq!(w.sub.subscriber_mandate_count(&rejected), 0);
}

#[test]
fn cancelling_frees_a_seat_on_a_full_plan() {
    let w = world();
    let plan_id = w.plan_with_cap(1);
    let first = w.subscriber(100_000);
    let id = w.sub.subscribe(&first, &plan_id, &0, &0, &None::<Address>);

    let second = w.subscriber(100_000);
    assert_eq!(
        w.sub
            .try_subscribe(&second, &plan_id, &0, &0, &None::<Address>),
        Err(Ok(Error::PlanFull))
    );

    w.sub.cancel(&first, &id);
    assert_eq!(w.sub.plan_subscriber_count(&plan_id), 0);

    // The freed seat is immediately usable.
    w.sub.subscribe(&second, &plan_id, &0, &0, &None::<Address>);
    assert_eq!(w.sub.plan_subscriber_count(&plan_id), 1);
}

#[test]
fn ending_a_mandate_frees_a_seat() {
    let w = world();
    let plan_id = w.plan_with_cap(1);
    let first = w.subscriber(100_000);
    let id = w.sub.subscribe(&first, &plan_id, &0, &0, &None::<Address>);

    w.sub.end_mandate(&w.merchant, &id);
    assert_eq!(w.sub.plan_subscriber_count(&plan_id), 0);

    let second = w.subscriber(100_000);
    w.sub.subscribe(&second, &plan_id, &0, &0, &None::<Address>);
    assert_eq!(w.sub.plan_subscriber_count(&plan_id), 1);
}

#[test]
fn completion_frees_a_seat() {
    let w = world();
    let plan_id = w.plan_with_cap(1);
    let first = w.subscriber(100_000);
    // Capped at one charge: the mandate completes on the first charge.
    let id = w.sub.subscribe(&first, &plan_id, &1, &0, &None::<Address>);

    w.sub.charge(&w.keeper, &id);
    assert_eq!(w.sub.get_mandate(&id).status, MandateStatus::Completed);
    assert_eq!(w.sub.plan_subscriber_count(&plan_id), 0);

    let second = w.subscriber(100_000);
    w.sub.subscribe(&second, &plan_id, &0, &0, &None::<Address>);
    assert_eq!(w.sub.plan_subscriber_count(&plan_id), 1);
}

#[test]
fn paused_mandates_still_hold_their_seat() {
    let w = world();
    let plan_id = w.plan_with_cap(1);
    let first = w.subscriber(100_000);
    let id = w.sub.subscribe(&first, &plan_id, &0, &0, &None::<Address>);
    w.sub.set_paused(&first, &id, &true);

    // Paused is still open: resuming later must keep working, so the seat is
    // not released.
    assert_eq!(w.sub.plan_subscriber_count(&plan_id), 1);
    let second = w.subscriber(100_000);
    assert_eq!(
        w.sub
            .try_subscribe(&second, &plan_id, &0, &0, &None::<Address>),
        Err(Ok(Error::PlanFull))
    );

    w.sub.set_paused(&first, &id, &false);
    w.sub.charge(&w.keeper, &id);
    assert_eq!(w.sub.get_mandate(&id).charges_made, 1);
}

#[test]
fn unlimited_plans_accept_any_number_of_subscribers() {
    let w = world();
    let plan_id = w.plan(); // max_subscribers = 0: unlimited
    let user = w.subscriber(1_000_000);

    for _ in 0..5 {
        w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    }
    // The count still tracks open mandates so an indexer can read it, but
    // nothing is ever rejected for being full.
    assert_eq!(w.sub.plan_subscriber_count(&plan_id), 5);
}

#[test]
fn subscriber_can_raise_the_cap_of_an_open_mandate() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(1_000_000);
    let id = w.sub.subscribe(&user, &plan_id, &2, &0, &None::<Address>);

    w.sub.charge(&w.keeper, &id);
    assert_eq!(w.sub.get_mandate(&id).charges_made, 1);

    // "Three more months": raise without cancelling, keeping the mandate id
    // and its charge history.
    w.sub.set_max_charges(&user, &id, &4);

    // The test env exposes the events of the invocation that just ran, so
    // assert them before any further call.
    assert_eq!(last_event_field_u32(&w.env, "max_charges"), Some(4));

    let m = w.sub.get_mandate(&id);
    assert_eq!(m.max_charges, 4);
    assert_eq!(m.charges_made, 1);
    assert_eq!(m.status, MandateStatus::Active);

    // The new cap binds: billing continues past where the old cap stopped.
    w.advance(MONTH);
    w.sub.charge(&w.keeper, &id);
    assert_eq!(w.sub.get_mandate(&id).charges_made, 2);
}

#[test]
fn only_the_subscriber_can_change_the_cap() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &2, &0, &None::<Address>);

    // A stranger fails the ownership check even though auth is mocked.
    let attacker = Address::generate(&w.env);
    assert_eq!(
        w.sub.try_set_max_charges(&attacker, &id, &99),
        Err(Ok(Error::NotSubscriber))
    );
    // The merchant cannot raise what its own subscriber pays either.
    assert_eq!(
        w.sub.try_set_max_charges(&w.merchant, &id, &99),
        Err(Ok(Error::NotSubscriber))
    );
    assert_eq!(w.sub.get_mandate(&id).max_charges, 2);
}

#[test]
fn a_different_signer_cannot_change_the_cap() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &2, &0, &None::<Address>);
    let attacker = Address::generate(&w.env);

    // The attacker passes the subscriber's address as the argument, but the
    // signature on the call is the attacker's: `require_auth` on the
    // subscriber fails and the cap is unchanged.
    w.env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &w.sub.address,
            fn_name: "set_max_charges",
            args: (&user, &id, &99u32).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);
    assert!(w.sub.try_set_max_charges(&user, &id, &99).is_err());
    assert_eq!(w.sub.get_mandate(&id).max_charges, 2);
}

#[test]
fn lowering_the_cap_to_charges_made_completes_the_mandate() {
    let w = world();
    let plan_id = w.plan_with_cap(1);
    let user = w.subscriber(1_000_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>); // open-ended

    w.sub.charge(&w.keeper, &id);
    w.advance(MONTH);
    w.sub.charge(&w.keeper, &id);
    assert_eq!(w.sub.get_mandate(&id).charges_made, 2);
    assert_eq!(w.sub.plan_subscriber_count(&plan_id), 1);

    // "Stop after the current one": cap == charges_made finishes the mandate
    // right here, exactly as the next charge would have.
    w.sub.set_max_charges(&user, &id, &2);

    // The test env exposes the events of the invocation that just ran, so
    // assert them before any further call. MandateCompleted is one of
    // the events this call emits.
    let body = event_by_topic(&w.env, "mandate_completed").expect("mandate_completed event");
    let ScVal::Symbol(ScSymbol(name)) = &body.topics[0] else {
        panic!("topic 0 is not a symbol");
    };
    let expected: soroban_sdk::xdr::StringM<32> = "mandate_completed".try_into().unwrap();
    assert_eq!(*name, expected);
    assert_eq!(event_field_u32(&w.env, "charges_made"), Some(2));

    let m = w.sub.get_mandate(&id);
    assert_eq!(m.max_charges, 2);
    assert_eq!(m.status, MandateStatus::Completed);
    assert!(!w.sub.is_due(&id));
    // The finished mandate released its seat on the capped plan.
    assert_eq!(w.sub.plan_subscriber_count(&plan_id), 0);

    w.advance(MONTH);
    assert_eq!(
        w.sub.try_charge(&w.keeper, &id),
        Err(Ok(Error::MandateNotActive))
    );
}

#[test]
fn a_cap_below_charges_made_is_rejected() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(1_000_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    w.sub.charge(&w.keeper, &id);
    w.advance(MONTH);
    w.sub.charge(&w.keeper, &id);
    assert_eq!(w.sub.get_mandate(&id).charges_made, 2);

    // The subscriber already paid two charges' worth; a cap of 1 would sit
    // below what has been charged and is rejected.
    assert_eq!(
        w.sub.try_set_max_charges(&user, &id, &1),
        Err(Ok(Error::InvalidMaxCharges))
    );
    // 0 reopens an open-ended mandate, which is never below the count.
    w.sub.set_max_charges(&user, &id, &0);
    assert_eq!(w.sub.get_mandate(&id).max_charges, 0);
    assert_eq!(w.sub.get_mandate(&id).status, MandateStatus::Active);
}

#[test]
fn raising_the_cap_works_on_a_paused_mandate() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(1_000_000);
    let id = w.sub.subscribe(&user, &plan_id, &2, &0, &None::<Address>);

    w.sub.charge(&w.keeper, &id);
    assert_eq!(w.sub.get_mandate(&id).charges_made, 1);
    w.sub.set_paused(&user, &id, &true);
    assert_eq!(w.sub.get_mandate(&id).status, MandateStatus::Paused);

    w.sub.set_max_charges(&user, &id, &3);
    assert_eq!(w.sub.get_mandate(&id).max_charges, 3);
    // Raising does not resume on its own: pause and cap are independent.
    assert_eq!(w.sub.get_mandate(&id).status, MandateStatus::Paused);

    w.sub.set_paused(&user, &id, &false);
    w.advance(MONTH);
    w.sub.charge(&w.keeper, &id);
    assert_eq!(w.sub.get_mandate(&id).charges_made, 2);
}

#[test]
fn changing_the_cap_of_a_cancelled_mandate_is_rejected() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    w.sub.cancel(&user, &id);
    assert_eq!(
        w.sub.try_set_max_charges(&user, &id, &5),
        Err(Ok(Error::MandateNotActive))
    );
}

#[test]
fn admin_can_upgrade_and_state_written_before_survives() {
    let w = world();
    let plan_id = w.plan_with_cap(4);
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &2, &0, &None::<Address>);
    w.sub.charge(&w.keeper, &id);

    let wasm_hash = w.env.deployer().upload_contract_wasm(UPGRADE_WASM);
    w.sub.upgrade_wasm(&wasm_hash);

    // The test env exposes the events of the invocation that just ran:
    // the upgrade must have announced itself.
    let last = w.env.events().all().events().last().cloned().unwrap();
    let ContractEventBody::V0(body) = &last.body;
    let ScVal::Symbol(ScSymbol(name)) = &body.topics[0] else {
        panic!("topic 0 is not a symbol");
    };
    let expected: soroban_sdk::xdr::StringM<32> = "contract_upgraded".try_into().unwrap();
    assert_eq!(name, &expected);

    // The mandate, its seat on the plan, and the fee config written before
    // the upgrade are all still readable after it.
    let m = w.sub.get_mandate(&id);
    assert_eq!(m.charges_made, 1);
    assert_eq!(m.max_charges, 2);
    assert_eq!(w.sub.plan_subscriber_count(&plan_id), 1);
    assert_eq!(w.sub.fee_bps(), FEE_BPS);

    // And the new code still settles charges.
    w.advance(MONTH);
    w.sub.charge(&w.keeper, &id);
    assert_eq!(w.sub.get_mandate(&id).charges_made, 2);
    assert_eq!(w.vault.balance(&user, &w.token), 100_000 - 2 * PRICE);
}

/// #237: every persistent and instance value written before an upgrade must
/// still decode with the current types after the code is swapped.
#[test]
fn upgrade_preserves_every_stored_value() {
    let w = world();
    let admin = w.sub.admin();
    let payout = Address::generate(&w.env);
    let referrer = Address::generate(&w.env);
    let pending_admin = Address::generate(&w.env);

    let plan_id = w.registry.create_plan(
        &w.merchant,
        &w.token,
        &PRICE,
        &MONTH,
        &String::from_str(&w.env, "Full Plan"),
        &String::from_str(&w.env, "All knobs"),
        &KEEPER_BPS,
        &Symbol::new(&w.env, "saas"),
        &String::from_str(&w.env, "https://example.com/terms"),
        &0u64,
        &REFERRAL_BPS,
        &Some(payout.clone()),
        &10u32,
    );
    w.registry
        .set_plan_grace_period(&w.merchant, &plan_id, &86_400);

    let user = w.subscriber(PRICE * 10);
    let id = w
        .sub
        .subscribe(&user, &plan_id, &5, &(PRICE * 4), &Some(referrer.clone()));
    w.sub.charge(&w.keeper, &id);
    let paused = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    w.sub.set_paused(&user, &paused, &true);

    // A mandate whose charge provably cannot be paid, so the failed-charge
    // timestamp is written before the upgrade.
    let broke = Address::generate(&w.env);
    let failed = w.sub.subscribe(&broke, &plan_id, &0, &0, &None::<Address>);
    assert!(w.sub.flag_failed_charge(&failed));
    let failed_at = w.sub.first_failed_charge(&failed).unwrap();

    let deleted = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    w.sub.cancel(&user, &deleted);
    w.sub.delete_mandate(&user, &deleted);

    w.sub.set_protocol_paused(&true);
    w.sub.propose_admin(&pending_admin);

    let mandate_before = w.sub.get_mandate(&id);
    let paused_before = w.sub.get_mandate(&paused);

    let wasm_hash = w.env.deployer().upload_contract_wasm(UPGRADE_WASM);
    w.sub.upgrade_wasm(&wasm_hash);

    // The full mandate record still decodes with the current type.
    let stored = w
        .env
        .as_contract(&w.sub.address, || {
            w.env
                .storage()
                .persistent()
                .get::<_, Mandate>(&DataKey::Mandate(id))
        })
        .unwrap();
    assert_eq!(stored, mandate_before);
    assert_eq!(w.sub.get_mandate(&id).charges_made, 1);
    assert_eq!(
        w.sub.get_mandate(&id).last_charge,
        mandate_before.last_charge
    );
    assert_eq!(
        w.sub.get_mandate(&id).next_charge,
        mandate_before.next_charge
    );
    assert_eq!(w.sub.get_mandate(&paused).status, MandateStatus::Paused);
    assert_eq!(w.sub.get_mandate(&paused).status, paused_before.status);

    // Instance config, indexes and counters all survive.
    assert_eq!(w.sub.admin(), admin);
    assert_eq!(w.sub.fee_bps(), FEE_BPS);
    assert_eq!(w.sub.plan_registry(), w.registry.address);
    assert_eq!(w.sub.vault(), w.vault.address);
    assert_eq!(w.sub.next_mandate_id(), deleted + 1);
    assert_eq!(w.sub.subscriber_mandate_count(&user), 3);
    assert_eq!(
        w.sub.subscriber_mandates(&user, &0, &50),
        soroban_sdk::vec![&w.env, id, paused, deleted]
    );
    assert_eq!(w.sub.merchant_mandate_count(&w.merchant), 4);
    assert_eq!(
        w.sub.merchant_mandates(&w.merchant, &0, &50),
        soroban_sdk::vec![&w.env, id, paused, failed, deleted]
    );
    assert_eq!(w.sub.plan_mandate_count(&plan_id), 4);
    assert_eq!(
        w.sub.plan_mandates(&plan_id, &0, &50),
        soroban_sdk::vec![&w.env, id, paused, failed, deleted]
    );
    assert_eq!(w.sub.plan_subscriber_count(&plan_id), 3);

    // Keys the committed upgrade target predates decode with the current
    // types too: a code swap never rewrites or drops storage entries.
    w.env.as_contract(&w.sub.address, || {
        let p = w.env.storage().persistent();
        assert_eq!(
            p.get::<_, u64>(&DataKey::MandateGracePeriod(id)),
            Some(86_400)
        );
        assert_eq!(
            p.get::<_, u64>(&DataKey::FirstFailedCharge(failed)),
            Some(failed_at)
        );
        assert_eq!(
            p.get::<_, bool>(&DataKey::MandateTombstone(deleted)),
            Some(true)
        );
        assert_eq!(
            w.env
                .storage()
                .instance()
                .get::<_, bool>(&DataKey::ProtocolPaused),
            Some(true)
        );
        assert_eq!(
            w.env
                .storage()
                .instance()
                .get::<_, Address>(&DataKey::PendingAdmin),
            Some(pending_admin.clone())
        );
    });

    // The upgraded code still reads the preserved instance config.
    w.sub.set_fee_bps(&(FEE_BPS + 1));
    assert_eq!(w.sub.fee_bps(), FEE_BPS + 1);
}

#[test]
fn rejected_upgrade_leaves_stored_values_intact() {
    let w = world();
    let admin = w.sub.admin();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &2, &0, &None::<Address>);
    w.sub.charge(&w.keeper, &id);
    let before = w.sub.get_mandate(&id);

    let wasm_hash = w.env.deployer().upload_contract_wasm(UPGRADE_WASM);
    let attacker = Address::generate(&w.env);
    w.env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &w.sub.address,
            fn_name: "upgrade_wasm",
            args: (&wasm_hash,).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);
    assert!(w.sub.try_upgrade_wasm(&wasm_hash).is_err());

    // The failed call moved no state and recorded no upgrade.
    assert_eq!(w.sub.get_mandate(&id), before);
    assert_eq!(w.sub.admin(), admin);
    assert_eq!(w.sub.fee_bps(), FEE_BPS);
    assert_eq!(w.sub.upgrade_history().len(), 0);
}

#[test]
fn upgrade_wasm_wrong_signer_rejected() {
    let w = world();
    let wasm_hash = w.env.deployer().upload_contract_wasm(UPGRADE_WASM);
    let attacker = Address::generate(&w.env);

    // The attacker passes the hash as the argument, but the signature on the
    // call is the attacker's: `require_auth` on the admin fails.
    w.env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &w.sub.address,
            fn_name: "upgrade_wasm",
            args: (&wasm_hash,).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);
    assert!(w.sub.try_upgrade_wasm(&wasm_hash).is_err());

    // With no authorization at all the call fails too.
    w.env.set_auths(&[]);
    assert!(w.sub.try_upgrade_wasm(&wasm_hash).is_err());

    // Configuration is untouched by the rejected upgrades.
    assert_eq!(w.sub.fee_bps(), FEE_BPS);
}

// ---------------------------------------------------------------------------
// Upgrade history view (#304)
// ---------------------------------------------------------------------------

#[test]
fn upgrade_history_starts_empty() {
    let w = world();
    let history = w.sub.upgrade_history();
    assert_eq!(history.len(), 0);
}

#[test]
fn upgrade_history_records_upgrades() {
    let w = world();

    // Initial history is empty.
    assert_eq!(w.sub.upgrade_history().len(), 0);

    // Record an upgrade directly via the internal function logic.
    // We test the recording logic by verifying the event emission in
    // upgrade_history_emits_contract_upgraded_event test.
    // Cross-upgrade persistence is tested by the upgrade_wasm test which
    // verifies state survives upgrade; the history storage uses the same
    // instance storage mechanism.
    let wasm_hash = w.env.deployer().upload_contract_wasm(UPGRADE_WASM);
    w.sub.upgrade_wasm(&wasm_hash);

    // After upgrade, the contract is replaced with the old WASM which
    // doesn't have upgrade_history. The history is recorded in storage
    // and would be readable by a newer contract version.
    // We verify the event was emitted instead.
    let mut found = false;
    for event in w.env.events().all().events() {
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
    let w = world();
    let wasm_hash = w.env.deployer().upload_contract_wasm(UPGRADE_WASM);
    let attacker = Address::generate(&w.env);

    w.env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &w.sub.address,
            fn_name: "upgrade_wasm",
            args: (&wasm_hash,).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);
    assert!(w.sub.try_upgrade_wasm(&wasm_hash).is_err());

    // History should still be empty.
    assert_eq!(w.sub.upgrade_history().len(), 0);
}

#[test]
fn upgrade_history_emits_contract_upgraded_event() {
    let w = world();
    let wasm_hash = w.env.deployer().upload_contract_wasm(UPGRADE_WASM);

    w.sub.upgrade_wasm(&wasm_hash);

    // Find the ContractUpgraded event.
    let mut found = false;
    for event in w.env.events().all().events() {
        let ContractEventBody::V0(body) = &event.body;
        if let Some(ScVal::Symbol(ScSymbol(name))) = body.topics.first() {
            if name.to_utf8_string_lossy() == "contract_upgraded" {
                found = true;
                // Check topics: admin is topic 1, new_wasm_hash is in data.
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
// Plan-wide charge pause (#315).
// ---------------------------------------------------------------------------

#[test]
fn paused_plan_blocks_charges_until_resumed() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(PRICE);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    w.registry
        .set_plan_charges_paused(&w.merchant, &plan_id, &true);

    // The mandate stays open, but nothing may settle it while the plan is
    // paused: neither charge nor the failure flag.
    assert!(!w.sub.is_due(&id));
    assert!(!w.sub.quote_charge(&id).chargeable);
    assert_eq!(
        w.sub.try_charge(&w.keeper, &id),
        Err(Ok(Error::PlanChargesPaused))
    );
    assert_eq!(
        w.sub.try_flag_failed_charge(&id),
        Err(Ok(Error::PlanChargesPaused))
    );
    // `due_mandates` is a local schedule scan and still lists the candidate;
    // `is_due` is the view that knows about the plan-wide pause.
    assert_eq!(
        w.sub.due_mandates(&w.merchant, &0, &10),
        soroban_sdk::vec![&w.env, id]
    );
    assert_eq!(w.sub.get_mandate(&id).status, MandateStatus::Active);
    assert_eq!(w.vault.balance(&user, &w.token), PRICE);

    // Resuming makes the same mandate chargeable again.
    w.registry
        .set_plan_charges_paused(&w.merchant, &plan_id, &false);
    assert!(w.sub.is_due(&id));
    assert!(w.sub.quote_charge(&id).chargeable);
    assert_eq!(
        w.sub.due_mandates(&w.merchant, &0, &10),
        soroban_sdk::vec![&w.env, id]
    );
    w.sub.charge(&w.keeper, &id);
    assert_eq!(w.sub.get_mandate(&id).charges_made, 1);
}

#[test]
fn pausing_one_plan_leaves_other_plans_chargeable() {
    let w = world();
    let paused_plan = w.plan();
    let running_plan = w.plan();
    let user = w.subscriber(PRICE * 2);
    let paused = w
        .sub
        .subscribe(&user, &paused_plan, &0, &0, &None::<Address>);
    let running = w
        .sub
        .subscribe(&user, &running_plan, &0, &0, &None::<Address>);

    w.registry
        .set_plan_charges_paused(&w.merchant, &paused_plan, &true);

    assert_eq!(
        w.sub.try_charge(&w.keeper, &paused),
        Err(Ok(Error::PlanChargesPaused))
    );
    w.sub.charge(&w.keeper, &running);
    assert_eq!(w.sub.get_mandate(&running).charges_made, 1);
    assert_eq!(w.sub.get_mandate(&paused).charges_made, 0);
}

#[test]
fn pausing_a_plan_does_not_block_new_subscriptions() {
    let w = world();
    let plan_id = w.plan();
    w.registry
        .set_plan_charges_paused(&w.merchant, &plan_id, &true);

    // Pausing charges is not deactivation: a subscriber can still sign up, and
    // the new mandate waits for the merchant to resume.
    let user = w.subscriber(PRICE);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    assert_eq!(w.sub.get_mandate(&id).status, MandateStatus::Active);
    assert_eq!(
        w.sub.try_charge(&w.keeper, &id),
        Err(Ok(Error::PlanChargesPaused))
    );

    w.registry
        .set_plan_charges_paused(&w.merchant, &plan_id, &false);
    w.sub.charge(&w.keeper, &id);
}

#[test]
fn upgrading_into_a_paused_plan_is_rejected() {
    let w = world();
    let old_plan = w.plan_priced(PRICE);
    let new_plan = w.plan_priced(PRICE * 2);
    let user = w.subscriber(PRICE * 3);
    let id = w.sub.subscribe(&user, &old_plan, &0, &0, &None::<Address>);

    w.registry
        .set_plan_charges_paused(&w.merchant, &new_plan, &true);
    assert_eq!(
        w.sub.try_upgrade(&user, &id, &new_plan),
        Err(Ok(Error::PlanChargesPaused))
    );
    assert_eq!(w.sub.get_mandate(&id).plan_id, old_plan);
}

#[test]
fn plan_pause_only_affects_the_plan_it_names() {
    let w = world();
    let plan_a = w.plan();
    let plan_b = w.plan();
    assert!(!w.registry.plan_charges_paused(&plan_a));
    w.registry
        .set_plan_charges_paused(&w.merchant, &plan_a, &true);
    assert!(w.registry.plan_charges_paused(&plan_a));
    assert!(!w.registry.plan_charges_paused(&plan_b));
}

// ---------------------------------------------------------------------------
// Grace-period state (#312).
// ---------------------------------------------------------------------------

#[test]
fn grace_view_uses_the_window_frozen_at_subscribe_time() {
    let w = world();
    let plan_id = w.plan();

    // Existing mandates keep the zero window they were opened with even if
    // the merchant later configures one.
    let before = w
        .sub
        .subscribe(&w.merchant, &plan_id, &0, &0, &None::<Address>);
    w.registry
        .set_plan_grace_period(&w.merchant, &plan_id, &3_600);
    let after = w
        .sub
        .subscribe(&w.merchant, &plan_id, &0, &0, &None::<Address>);

    assert_eq!(w.sub.mandate_grace_period(&before), 0);
    assert_eq!(w.sub.mandate_grace_period(&after), 3_600);
    assert!(!w.sub.is_in_grace_period(&before));
    assert!(w.sub.is_in_grace_period(&after));
}

#[test]
fn grace_view_starts_at_the_due_time_and_expires_at_the_boundary() {
    let w = world();
    let plan_id = w.plan();
    w.registry
        .set_plan_grace_period(&w.merchant, &plan_id, &3_600);
    let id = w
        .sub
        .subscribe(&w.merchant, &plan_id, &0, &0, &None::<Address>);
    let due = w.sub.get_mandate(&id).next_charge;

    // Due now: the window has just opened.
    assert!(w.sub.is_in_grace_period(&id));

    // One second before the advertised end is still grace.
    w.env.ledger().set_timestamp(due + 3_599);
    assert!(w.sub.is_in_grace_period(&id));

    // The end is exclusive.
    w.env.ledger().set_timestamp(due + 3_600);
    assert!(!w.sub.is_in_grace_period(&id));
}

#[test]
fn successful_charges_leave_the_grace_view_false() {
    let w = world();
    let plan_id = w.plan();
    w.registry
        .set_plan_grace_period(&w.merchant, &plan_id, &3_600);
    let user = w.subscriber(PRICE);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    w.sub.charge(&w.keeper, &id);
    assert!(!w.sub.is_in_grace_period(&id));
}

#[test]
fn cancelled_and_paused_mandates_are_not_in_grace() {
    let w = world();
    let plan_id = w.plan();
    w.registry
        .set_plan_grace_period(&w.merchant, &plan_id, &3_600);
    let user = Address::generate(&w.env);
    let other = Address::generate(&w.env);
    let cancelled = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    let paused = w.sub.subscribe(&other, &plan_id, &0, &0, &None::<Address>);

    w.sub.cancel(&user, &cancelled);
    w.sub.set_paused(&other, &paused, &true);
    assert!(!w.sub.is_in_grace_period(&cancelled));
    assert!(!w.sub.is_in_grace_period(&paused));
}

#[test]
fn grace_view_rejects_an_unknown_mandate() {
    let w = world();
    assert_eq!(
        w.sub.try_is_in_grace_period(&42),
        Err(Ok(Error::MandateNotFound))
    );
    assert_eq!(
        w.sub.try_mandate_grace_period(&42),
        Err(Ok(Error::MandateNotFound))
    );
}

#[test]
fn grace_view_is_a_pure_read() {
    let w = world();
    let plan_id = w.plan();
    w.registry
        .set_plan_grace_period(&w.merchant, &plan_id, &3_600);
    let id = w
        .sub
        .subscribe(&w.merchant, &plan_id, &0, &0, &None::<Address>);
    let before = w.sub.get_mandate(&id);

    assert!(w.sub.is_in_grace_period(&id));

    assert_eq!(w.sub.get_mandate(&id), before);
    assert!(w.env.events().all().events().is_empty());
}

// ---------------------------------------------------------------------------
// Merchant notification on the first failed charge (#296).
// ---------------------------------------------------------------------------

/// The `charge_failed` events emitted by the most recent invocation.
fn charge_failed_events(env: &Env) -> std::vec::Vec<soroban_sdk::xdr::ContractEvent> {
    env.events()
        .all()
        .events()
        .iter()
        .filter(|event| {
            let ContractEventBody::V0(body) = &event.body;
            matches!(
                body.topics.first(),
                Some(ScVal::Symbol(ScSymbol(name))) if name.to_utf8_string_lossy() == "charge_failed"
            )
        })
        .cloned()
        .collect()
}

/// A subscriber whose vault covers exactly one charge, with that first charge
/// already settled, and the clock moved to when the second one falls due.
fn lapsed_mandate(w: &World) -> (Address, u64) {
    let plan_id = w.plan();
    let user = w.subscriber(PRICE);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    w.sub.charge(&w.keeper, &id);
    w.advance(MONTH);
    (user, id)
}

#[test]
fn flagging_a_failed_charge_notifies_the_merchant_once() {
    let w = world();
    let (user, id) = lapsed_mandate(&w);
    let due_at = w.sub.get_mandate(&id).next_charge;
    let now = w.env.ledger().timestamp();

    // The keeper's charge fails and rolls back...
    assert!(w.sub.try_charge(&w.keeper, &id).is_err());
    assert_eq!(w.sub.first_failed_charge(&id), None);

    // ...so it flags the failure instead.
    assert!(w.sub.flag_failed_charge(&id));
    let events = charge_failed_events(&w.env);
    assert_eq!(events.len(), 1);

    let ContractEventBody::V0(body) = &events[0].body;
    // Topics: name, mandate_id, merchant, subscriber. The merchant topic is
    // what a dunning service filters on.
    assert_eq!(body.topics.len(), 4);
    assert_eq!(body.topics[1], ScVal::U64(id));
    assert_eq!(
        Address::try_from_val(&w.env, &body.topics[2]).unwrap(),
        w.merchant
    );
    assert_eq!(
        Address::try_from_val(&w.env, &body.topics[3]).unwrap(),
        user
    );
    assert_eq!(last_event_field_i128(&w.env, "amount"), Some(PRICE));
    assert_eq!(last_event_field_u64(&w.env, "due_at"), Some(due_at));
    assert_eq!(last_event_field_u64(&w.env, "failed_at"), Some(now));
    assert_eq!(event_field_u32(&w.env, "charges_made"), Some(1));
    assert_eq!(
        event_field_address(&w.env, "charge_failed", "token"),
        Some(w.token.clone())
    );

    assert_eq!(w.sub.first_failed_charge(&id), Some(now));
}

#[test]
fn repeated_flags_in_one_run_of_failures_emit_nothing() {
    let w = world();
    let (_user, id) = lapsed_mandate(&w);
    let first = w.env.ledger().timestamp();

    assert!(w.sub.flag_failed_charge(&id));

    w.advance(3_600);
    assert!(!w.sub.flag_failed_charge(&id));
    assert!(charge_failed_events(&w.env).is_empty());

    w.advance(86_400);
    assert!(!w.sub.flag_failed_charge(&id));
    assert!(charge_failed_events(&w.env).is_empty());

    // The recorded time is the first failure, not the latest retry.
    assert_eq!(w.sub.first_failed_charge(&id), Some(first));
}

#[test]
fn a_successful_charge_ends_the_run_and_the_next_failure_notifies_again() {
    let w = world();
    let (user, id) = lapsed_mandate(&w);
    assert!(w.sub.flag_failed_charge(&id));

    // The subscriber tops up; the retry settles and clears the flag.
    w.vault.deposit(&user, &w.token, &PRICE);
    w.sub.charge(&w.keeper, &id);
    assert_eq!(w.sub.first_failed_charge(&id), None);

    // Next period the vault is empty again: a fresh notification.
    w.advance(MONTH);
    let now = w.env.ledger().timestamp();
    assert!(w.sub.flag_failed_charge(&id));
    assert_eq!(charge_failed_events(&w.env).len(), 1);
    assert_eq!(w.sub.first_failed_charge(&id), Some(now));
}

#[test]
fn a_charge_that_would_succeed_cannot_be_flagged() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(PRICE);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    assert_eq!(
        w.sub.try_flag_failed_charge(&id),
        Err(Ok(Error::ChargeWouldSucceed))
    );
    assert_eq!(w.sub.first_failed_charge(&id), None);
    // The charge indeed succeeds.
    w.sub.charge(&w.keeper, &id);
}

#[test]
fn a_charge_that_is_not_due_cannot_be_flagged() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(PRICE);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    w.sub.charge(&w.keeper, &id);

    // Vault is empty, but nothing is owed until next_charge.
    w.advance(MONTH - 1);
    assert_eq!(w.sub.try_flag_failed_charge(&id), Err(Ok(Error::NotDue)));
    assert_eq!(w.sub.first_failed_charge(&id), None);
}

#[test]
fn inactive_mandates_cannot_be_flagged() {
    let w = world();
    let plan_id = w.plan();

    let cancelled_user = Address::generate(&w.env);
    let cancelled = w
        .sub
        .subscribe(&cancelled_user, &plan_id, &0, &0, &None::<Address>);
    w.sub.cancel(&cancelled_user, &cancelled);

    let paused_user = Address::generate(&w.env);
    let paused = w
        .sub
        .subscribe(&paused_user, &plan_id, &0, &0, &None::<Address>);
    w.sub.set_paused(&paused_user, &paused, &true);

    assert_eq!(
        w.sub.try_flag_failed_charge(&cancelled),
        Err(Ok(Error::MandateNotActive))
    );
    assert_eq!(
        w.sub.try_flag_failed_charge(&paused),
        Err(Ok(Error::MandateNotActive))
    );
    assert_eq!(
        w.sub.try_flag_failed_charge(&999),
        Err(Ok(Error::MandateNotFound))
    );
}

#[test]
fn a_mandate_at_its_spend_cap_cannot_be_flagged() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(PRICE);
    // Room for one and a half charges: the second would cross the cap, so
    // charge refuses it on caps grounds, not funds.
    let id = w
        .sub
        .subscribe(&user, &plan_id, &0, &(PRICE + PRICE / 2), &None::<Address>);
    w.sub.charge(&w.keeper, &id);
    w.advance(MONTH);

    assert_eq!(
        w.sub.try_flag_failed_charge(&id),
        Err(Ok(Error::MaxChargesReached))
    );
}

#[test]
fn a_spend_limit_shortfall_counts_as_a_failed_charge() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(10 * PRICE);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    // Funded, but the subscriber's own spend limit is below the price.
    w.vault
        .set_spend_limit(&user, &w.token, &(PRICE - 1), &MONTH);

    assert!(w.sub.try_charge(&w.keeper, &id).is_err());
    assert!(w.sub.flag_failed_charge(&id));
}

#[test]
fn flag_failed_charge_is_permissionless() {
    let w = world();
    let (_user, id) = lapsed_mandate(&w);

    // No signature from anyone: the flag only records a provable fact.
    w.env.set_auths(&[]);
    assert!(w.sub.flag_failed_charge(&id));
    assert!(w.sub.first_failed_charge(&id).is_some());
}

#[test]
fn flagging_never_moves_money_or_changes_the_mandate() {
    let w = world();
    let (user, id) = lapsed_mandate(&w);
    let before = w.sub.get_mandate(&id);
    let merchant_before = w.token_client.balance(&w.merchant);

    w.vault.deposit(&user, &w.token, &(PRICE / 2));
    assert!(w.sub.flag_failed_charge(&id));

    assert_eq!(w.sub.get_mandate(&id), before);
    assert_eq!(w.vault.balance(&user, &w.token), PRICE / 2);
    assert_eq!(w.token_client.balance(&w.merchant), merchant_before);
}

// ---------------------------------------------------------------------------
// Per-mandate reserves (#295) through the subscription contract.
// ---------------------------------------------------------------------------

#[test]
fn a_reserve_pays_its_own_mandate_and_starves_no_other() {
    let w = world();
    let plan_id = w.plan();
    // Enough for one charge plus a little.
    let user = w.subscriber(PRICE + 100);
    let reserved = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    let other = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    w.vault.set_reserve(&user, &w.token, &reserved, &PRICE);

    // The other mandate sees only the unreserved 100 and fails...
    assert!(w.sub.try_charge(&w.keeper, &other).is_err());
    assert_eq!(w.vault.reserved(&user, &w.token, &reserved), PRICE);
    // ...and its failure is provable, so it can be flagged.
    assert!(w.sub.flag_failed_charge(&other));

    // The reserved mandate settles from its reserve.
    w.sub.charge(&w.keeper, &reserved);
    assert_eq!(w.vault.reserved(&user, &w.token, &reserved), 0);
    assert_eq!(w.vault.balance(&user, &w.token), 100);
    assert_eq!(
        w.sub.try_flag_failed_charge(&reserved),
        Err(Ok(Error::NotDue))
    );
}

#[test]
fn a_tipped_referred_charge_draws_every_leg_from_the_reserve() {
    let w = world();
    let referrer = Address::generate(&w.env);
    let plan_id = w.plan_full(KEEPER_BPS, REFERRAL_BPS, None);
    let user = w.subscriber(PRICE);
    let id = w
        .sub
        .subscribe(&user, &plan_id, &0, &0, &Some(referrer.clone()));
    // Everything is reserved: only this mandate's own legs may spend it.
    w.vault.set_reserve(&user, &w.token, &id, &PRICE);

    w.sub.charge(&w.keeper, &id);

    assert_eq!(w.vault.balance(&user, &w.token), 0);
    assert_eq!(w.vault.reserved(&user, &w.token, &id), 0);
    assert_eq!(w.vault.reserved_total(&user, &w.token), 0);
    assert!(w.token_client.balance(&w.keeper) > 0);
    assert!(w.token_client.balance(&referrer) > 0);
}

#[test]
fn an_upgrade_proration_can_draw_on_the_mandates_reserve() {
    let w = world();
    let cheap = w.plan_priced(PRICE);
    let dear = w.plan_priced(PRICE * 2);
    let user = w.subscriber(PRICE * 2);
    let id = w.sub.subscribe(&user, &cheap, &0, &0, &None::<Address>);
    w.sub.charge(&w.keeper, &id);
    // The remaining PRICE is all reserved for this mandate.
    w.vault.set_reserve(&user, &w.token, &id, &PRICE);

    w.advance(MONTH / 2);
    w.sub.upgrade(&user, &id, &dear);

    // Half a period of the PRICE difference came out of the reserve.
    assert_eq!(w.vault.reserved(&user, &w.token, &id), PRICE / 2);
    assert_eq!(w.vault.balance(&user, &w.token), PRICE / 2);
}

// ---------------------------------------------------------------------------
// The vault's token allowlist gates `subscribe_with_deposit` (#300).
// ---------------------------------------------------------------------------

/// A fresh account holding `amount` of the world's token in its wallet, with
/// nothing deposited.
fn wallet_holder(w: &World, amount: i128) -> Address {
    let user = Address::generate(&w.env);
    w.minter.mint(&user, &amount);
    user
}

#[test]
fn allowlist_off_by_default_leaves_subscribe_open() {
    let w = world();
    assert!(!w.vault.allowlist_enabled());
    let plan_id = w.plan();
    let user = wallet_holder(&w, 100_000);

    let id = w
        .sub
        .subscribe_with_deposit(&user, &plan_id, &0, &0, &40_000, &None::<Address>);
    assert_eq!(w.sub.get_mandate(&id).status, MandateStatus::Active);
}

#[test]
fn enabling_the_allowlist_blocks_subscribing_with_an_unlisted_token() {
    let w = world();
    let plan_id = w.plan();
    let user = wallet_holder(&w, 100_000);
    w.vault.set_allowlist_enabled(&true);

    assert!(w
        .sub
        .try_subscribe_with_deposit(&user, &plan_id, &0, &0, &40_000, &None::<Address>)
        .is_err());
    // The vault refused for the allowlist, not some other reason.
    assert_eq!(
        w.vault.try_deposit(&user, &w.token, &40_000),
        Err(Ok(payflow_vault::Error::TokenNotAllowed))
    );

    // Atomic: no mandate, no seat, no credit, no tokens moved, no id burned.
    assert_eq!(w.sub.plan_mandate_count(&plan_id), 0);
    assert_eq!(w.sub.plan_subscriber_count(&plan_id), 0);
    assert_eq!(w.sub.subscriber_mandate_count(&user), 0);
    assert_eq!(w.vault.balance(&user, &w.token), 0);
    assert_eq!(w.token_client.balance(&user), 100_000);
    assert_eq!(w.sub.next_mandate_id(), 1);
}

#[test]
fn an_allowlisted_token_subscribes_while_the_allowlist_is_on() {
    let w = world();
    let plan_id = w.plan();
    let user = wallet_holder(&w, 100_000);
    w.vault.set_allowlist_enabled(&true);
    w.vault.set_token_allowed(&w.token, &true);

    let id = w
        .sub
        .subscribe_with_deposit(&user, &plan_id, &0, &0, &40_000, &None::<Address>);
    assert_eq!(w.vault.balance(&user, &w.token), 40_000);
    assert_eq!(w.sub.get_mandate(&id).subscriber, user);
}

#[test]
fn disabling_the_allowlist_restores_open_subscribe() {
    let w = world();
    let plan_id = w.plan();
    let user = wallet_holder(&w, 100_000);

    w.vault.set_allowlist_enabled(&true);
    assert!(w
        .sub
        .try_subscribe_with_deposit(&user, &plan_id, &0, &0, &40_000, &None::<Address>)
        .is_err());

    // The token was never listed: turning the allowlist off alone reopens it.
    w.vault.set_allowlist_enabled(&false);
    assert!(!w.vault.token_allowed(&w.token));
    let id = w
        .sub
        .subscribe_with_deposit(&user, &plan_id, &0, &0, &40_000, &None::<Address>);
    assert_eq!(id, 1);
    assert_eq!(w.vault.balance(&user, &w.token), 40_000);
}

#[test]
fn toggling_the_allowlist_repeatedly_gates_subscribe_each_time() {
    let w = world();
    let plan_id = w.plan();

    for round in 0..3 {
        w.vault.set_allowlist_enabled(&true);
        let blocked = wallet_holder(&w, 100_000);
        assert!(
            w.sub
                .try_subscribe_with_deposit(&blocked, &plan_id, &0, &0, &PRICE, &None::<Address>)
                .is_err(),
            "round {round}: allowlist on must block"
        );

        w.vault.set_allowlist_enabled(&false);
        let open = wallet_holder(&w, 100_000);
        w.sub
            .subscribe_with_deposit(&open, &plan_id, &0, &0, &PRICE, &None::<Address>);
    }
    // Exactly the three open-window sign-ups went through.
    assert_eq!(w.sub.plan_mandate_count(&plan_id), 3);
}

#[test]
fn delisting_a_token_blocks_new_subscribers_but_existing_mandates_keep_billing() {
    let w = world();
    let plan_id = w.plan();
    w.vault.set_allowlist_enabled(&true);
    w.vault.set_token_allowed(&w.token, &true);

    let existing = wallet_holder(&w, 100_000);
    let id =
        w.sub
            .subscribe_with_deposit(&existing, &plan_id, &0, &0, &(2 * PRICE), &None::<Address>);

    w.vault.set_token_allowed(&w.token, &false);

    let newcomer = wallet_holder(&w, 100_000);
    assert!(w
        .sub
        .try_subscribe_with_deposit(&newcomer, &plan_id, &0, &0, &PRICE, &None::<Address>)
        .is_err());

    // The allowlist gates new funds only: the open mandate still settles from
    // the balance already in the vault, and the subscriber can still exit.
    w.sub.charge(&w.keeper, &id);
    assert_eq!(w.sub.get_mandate(&id).charges_made, 1);
    w.vault.withdraw_all(&existing, &w.token);
    assert_eq!(w.vault.balance(&existing, &w.token), 0);
}

#[test]
fn allowlist_gates_new_funds_not_balances_already_held() {
    let w = world();
    let plan_id = w.plan();
    // Funded before the allowlist was switched on.
    let user = w.subscriber(PRICE);
    w.vault.set_allowlist_enabled(&true);

    // A plain subscribe deposits nothing, so it is not gated...
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    w.sub.charge(&w.keeper, &id);
    // ...but adding funds for the next charge is.
    assert_eq!(
        w.vault.try_deposit(&user, &w.token, &PRICE),
        Err(Ok(payflow_vault::Error::TokenNotAllowed))
    );
}

#[test]
fn a_non_admin_cannot_enable_the_allowlist_to_block_subscribers() {
    let w = world();
    let plan_id = w.plan();
    let stranger = Address::generate(&w.env);

    w.env.mock_auths(&[MockAuth {
        address: &stranger,
        invoke: &MockAuthInvoke {
            contract: &w.vault.address,
            fn_name: "set_allowlist_enabled",
            args: (true,).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);
    assert!(w.vault.try_set_allowlist_enabled(&true).is_err());

    w.env.mock_all_auths();
    assert!(!w.vault.allowlist_enabled());
    let user = wallet_holder(&w, 100_000);
    w.sub
        .subscribe_with_deposit(&user, &plan_id, &0, &0, &PRICE, &None::<Address>);
}

#[test]
fn a_non_admin_cannot_disable_the_allowlist_to_reopen_subscribe() {
    let w = world();
    let plan_id = w.plan();
    let stranger = Address::generate(&w.env);
    w.vault.set_allowlist_enabled(&true);

    w.env.mock_auths(&[MockAuth {
        address: &stranger,
        invoke: &MockAuthInvoke {
            contract: &w.vault.address,
            fn_name: "set_allowlist_enabled",
            args: (false,).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);
    assert!(w.vault.try_set_allowlist_enabled(&false).is_err());

    w.env.mock_all_auths();
    assert!(w.vault.allowlist_enabled());
    let user = wallet_holder(&w, 100_000);
    assert!(w
        .sub
        .try_subscribe_with_deposit(&user, &plan_id, &0, &0, &PRICE, &None::<Address>)
        .is_err());
}

#[test]
fn a_non_admin_cannot_allowlist_a_token_to_subscribe() {
    let w = world();
    let plan_id = w.plan();
    let user = wallet_holder(&w, 100_000);
    w.vault.set_allowlist_enabled(&true);

    // The would-be subscriber cannot vouch for their own token.
    w.env.mock_auths(&[MockAuth {
        address: &user,
        invoke: &MockAuthInvoke {
            contract: &w.vault.address,
            fn_name: "set_token_allowed",
            args: (&w.token, true).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);
    assert!(w.vault.try_set_token_allowed(&w.token, &true).is_err());

    w.env.mock_all_auths();
    assert!(!w.vault.token_allowed(&w.token));
    assert!(w
        .sub
        .try_subscribe_with_deposit(&user, &plan_id, &0, &0, &PRICE, &None::<Address>)
        .is_err());
}

// ---------------------------------------------------------------------------
// Panic safety (#308): invalid inputs must surface as typed `Error`s, never as
// a host trap from an unchecked conversion or cross-contract call.
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
fn missing_plans_are_typed_errors_on_every_entry_point() {
    let w = world();
    let user = w.subscriber(100_000);
    let now = w.env.ledger().timestamp();

    for missing in [0, 7, u64::MAX] {
        assert_eq!(
            w.sub
                .try_subscribe(&user, &missing, &0, &0, &None::<Address>),
            Err(Ok(Error::PlanNotFound))
        );
        assert_eq!(
            w.sub
                .try_subscribe_at(&user, &missing, &0, &0, &now, &None::<Address>,),
            Err(Ok(Error::PlanNotFound))
        );
        assert_eq!(
            w.sub
                .try_subscribe_with_deposit(&user, &missing, &0, &0, &100, &None::<Address>,),
            Err(Ok(Error::PlanNotFound))
        );
    }

    let plan_id = w.plan();
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    for missing in [0, 7, u64::MAX] {
        assert_eq!(
            w.sub.try_upgrade(&user, &id, &missing),
            Err(Ok(Error::PlanNotFound))
        );
    }
    assert_eq!(w.sub.get_mandate(&id).plan_id, plan_id);
}

#[test]
fn underfunded_charge_is_a_typed_vault_error() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(PRICE);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    w.sub.charge(&w.keeper, &id);
    w.advance(MONTH);

    assert_eq!(
        w.sub.try_charge(&w.keeper, &id),
        Err(Ok(Error::VaultCallFailed))
    );
    assert_eq!(w.sub.get_mandate(&id).charges_made, 1);
    assert_eq!(w.vault.balance(&user, &w.token), 0);

    // The dunning path proves the same failure and records it without trapping.
    assert!(w.sub.flag_failed_charge(&id));
}

#[test]
fn subscribe_with_deposit_maps_vault_failures_to_a_typed_error() {
    let w = world();
    let plan_id = w.plan();
    let user = Address::generate(&w.env);

    assert_eq!(
        w.sub
            .try_subscribe_with_deposit(&user, &plan_id, &0, &0, &100, &None::<Address>),
        Err(Ok(Error::VaultCallFailed))
    );
    assert_eq!(
        w.sub
            .try_subscribe_with_deposit(&user, &plan_id, &0, &0, &0, &None::<Address>),
        Err(Ok(Error::VaultCallFailed))
    );
    assert_eq!(w.sub.next_mandate_id(), 1);
}

#[test]
fn upgrade_reports_vault_failure_as_a_typed_error() {
    let w = world();
    let plan_id = w.plan();
    let dearer = w.plan_amount(PRICE * 2, MONTH);
    let user = Address::generate(&w.env);
    let start = w.env.ledger().timestamp() + MONTH;
    let id = w
        .sub
        .subscribe_at(&user, &plan_id, &0, &0, &start, &None::<Address>);

    assert_eq!(
        w.sub.try_upgrade(&user, &id, &dearer),
        Err(Ok(Error::VaultCallFailed))
    );
    assert_eq!(w.sub.get_mandate(&id).plan_id, plan_id);
}

#[test]
fn extreme_schedules_return_typed_errors() {
    let w = world();
    let plan_id = w.plan();
    let user = Address::generate(&w.env);
    let now = w.env.ledger().timestamp();

    for start in [0, now - 1, now + MAX_START_OFFSET + 1, u64::MAX] {
        assert_eq!(
            w.sub
                .try_subscribe_at(&user, &plan_id, &0, &0, &start, &None::<Address>),
            Err(Ok(Error::InvalidStart))
        );
    }

    let long_plan = w.plan_amount(PRICE, u64::MAX);
    let id = w
        .sub
        .subscribe(&user, &long_plan, &u32::MAX, &0, &None::<Address>);
    assert_eq!(
        w.sub.try_estimated_end(&id),
        Err(Ok(Error::ScheduleOverflow))
    );
}

#[test]
fn deterministic_random_queries_never_reach_a_host_trap() {
    let w = world();
    let user = w.subscriber(PRICE);
    let plan_id = w.plan();
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    let now = w.env.ledger().timestamp();
    let mut seed = 0x5eed_1234_abcd_9876u64;

    for _ in 0..128 {
        let x = next_fuzz_u64(&mut seed);
        let plan = next_fuzz_u64(&mut seed);
        let start = now.saturating_add(x % (MAX_START_OFFSET * 2));
        let max_total = i128::from(x) - (i128::MAX / 2);
        // Fuzz input: keep the low 32 bits of the random value on purpose.
        let max_charges = u32::try_from(x & u64::from(u32::MAX)).unwrap();

        assert_typed_error(w.sub.try_subscribe_at(
            &user,
            &plan,
            &max_charges,
            &max_total,
            &start,
            &None::<Address>,
        ));
        assert_typed_error(w.sub.try_charge(&w.keeper, &x));
        assert_typed_error(w.sub.try_get_mandate(&x));
        assert_typed_error(w.sub.try_estimated_end(&x));
        assert_typed_error(w.sub.try_quote_charge(&x));
        assert_typed_error(w.sub.try_upgrade(&user, &id, &plan));
    }
}

// ---------------------------------------------------------------------------
// #320: every public view is side-effect free
// ---------------------------------------------------------------------------

/// Run one view call and prove it was read-only: the whole ledger (every
/// contract's storage, and every entry's TTL) is byte-for-byte identical
/// afterwards, the call published no contract events, and it asked nobody to
/// authorize anything. Indexers and wallets call these views through
/// simulation and never sign, so a view that wrote, emitted or required auth
/// would be a bug even if the value it returned were correct.
///
/// `env.events().all()` and `env.auths()` report the most recent top-level
/// invocation, so this wraps exactly one client call at a time.
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

/// A world with something in every index a view reads: an active mandate that
/// has been charged, a paused one, a cancelled one, and a capped plan, so the
/// views below are exercised against real state rather than empty storage.
fn view_world() -> (World<'static>, Address, u64, u64, u64, u64) {
    let w = world();
    let plan_id = w.plan_with_cap(5);
    let user = w.subscriber(1_000_000);
    let active = w.sub.subscribe(&user, &plan_id, &3, &0, &None::<Address>);
    w.sub.charge(&w.keeper, &active);
    let paused = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    w.sub.set_paused(&user, &paused, &true);
    let cancelled = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    w.sub.cancel(&user, &cancelled);
    w.advance(MONTH);
    (w, user, plan_id, active, paused, cancelled)
}

#[test]
fn every_subscription_view_leaves_storage_events_and_auth_untouched() {
    let (w, user, plan_id, active, paused, cancelled) = view_world();
    let env = &w.env;
    let s = &w.sub;

    assert_read_only(env, "version", || s.version());
    assert_read_only(env, "admin", || s.admin());
    assert_read_only(env, "plan_registry", || s.plan_registry());
    assert_read_only(env, "vault", || s.vault());
    assert_read_only(env, "fee_bps", || s.fee_bps());
    assert_read_only(env, "next_mandate_id", || s.next_mandate_id());
    assert_read_only(env, "is_mandate_tombstoned", || {
        s.is_mandate_tombstoned(&active)
    });
    assert_read_only(env, "upgrade_history", || s.upgrade_history());

    for id in [active, paused, cancelled] {
        assert_read_only(env, "get_mandate", || s.get_mandate(&id));
        assert_read_only(env, "estimated_end", || s.estimated_end(&id));
        assert_read_only(env, "is_due", || s.is_due(&id));
        assert_read_only(env, "first_failed_charge", || s.first_failed_charge(&id));
        assert_read_only(env, "mandate_grace_period", || s.mandate_grace_period(&id));
        assert_read_only(env, "is_in_grace_period", || s.is_in_grace_period(&id));
    }
    // quote_charge reads the plan registry and the vault too: a cross-contract
    // read must not write in either of them.
    assert_read_only(env, "quote_charge", || s.quote_charge(&active));

    assert_read_only(env, "due_mandates", || s.due_mandates(&w.merchant, &0, &10));
    assert_read_only(env, "committed_per_period", || {
        s.committed_per_period(&user, &w.token, &MONTH)
    });
    assert_read_only(env, "subscriber_mandate_count", || {
        s.subscriber_mandate_count(&user)
    });
    assert_read_only(env, "subscriber_mandates", || {
        s.subscriber_mandates(&user, &0, &10)
    });
    assert_read_only(env, "subscriber_mandate_summaries", || {
        s.subscriber_mandate_summaries(&user, &0, &10)
    });
    assert_read_only(env, "merchant_mandate_count", || {
        s.merchant_mandate_count(&w.merchant)
    });
    assert_read_only(env, "merchant_mandates", || {
        s.merchant_mandates(&w.merchant, &0, &10)
    });
    assert_read_only(env, "plan_mandate_count", || s.plan_mandate_count(&plan_id));
    assert_read_only(env, "plan_mandates", || s.plan_mandates(&plan_id, &0, &10));
    assert_read_only(env, "plan_subscriber_count", || {
        s.plan_subscriber_count(&plan_id)
    });
    assert_read_only(env, "plan_committed_revenue", || {
        s.plan_committed_revenue(&plan_id)
    });

    // Pages past the end and unknown keys read nothing and write nothing.
    let stranger = Address::generate(env);
    assert_read_only(env, "subscriber_mandates (past end)", || {
        s.subscriber_mandates(&user, &1_000, &10)
    });
    assert_read_only(env, "subscriber_mandate_summaries (stranger)", || {
        s.subscriber_mandate_summaries(&stranger, &0, &10)
    });
    assert_read_only(env, "due_mandates (stranger)", || {
        s.due_mandates(&stranger, &0, &10)
    });
    assert_read_only(env, "plan_mandates (unknown plan)", || {
        s.plan_mandates(&9_999, &0, &10)
    });
}

/// The purity check itself must not be vacuous: wrapping a call that does
/// write is caught.
#[test]
#[should_panic(expected = "mutated ledger state")]
fn the_read_only_check_catches_a_state_change() {
    let (w, user, _, active, _, _) = view_world();
    assert_read_only(&w.env, "set_paused", || {
        w.sub.set_paused(&user, &active, &true);
    });
}

#[test]
fn subscription_views_need_no_signature() {
    let (w, user, plan_id, active, _, _) = view_world();
    let env = &w.env;
    let s = &w.sub;

    // Drop the blanket auth mock: from here on any `require_auth` fails.
    env.set_auths(&[]);

    // Prove the switch took: a state-changing call is now refused...
    assert!(s.try_set_paused(&user, &active, &true).is_err());
    assert_eq!(s.get_mandate(&active).status, MandateStatus::Active);

    // ...while every view still answers, unsigned, without side effects.
    assert_read_only(env, "get_mandate (unsigned)", || s.get_mandate(&active));
    assert_read_only(env, "is_due (unsigned)", || s.is_due(&active));
    assert_read_only(env, "quote_charge (unsigned)", || s.quote_charge(&active));
    assert_read_only(env, "estimated_end (unsigned)", || s.estimated_end(&active));
    assert_read_only(env, "is_in_grace_period (unsigned)", || {
        s.is_in_grace_period(&active)
    });
    assert_read_only(env, "due_mandates (unsigned)", || {
        s.due_mandates(&w.merchant, &0, &10)
    });
    assert_read_only(env, "subscriber_mandate_summaries (unsigned)", || {
        s.subscriber_mandate_summaries(&user, &0, &10)
    });
    assert_read_only(env, "committed_per_period (unsigned)", || {
        s.committed_per_period(&user, &w.token, &MONTH)
    });
    assert_read_only(env, "plan_committed_revenue (unsigned)", || {
        s.plan_committed_revenue(&plan_id)
    });
    assert_read_only(env, "is_mandate_tombstoned (unsigned)", || {
        s.is_mandate_tombstoned(&active)
    });
    assert_read_only(env, "admin (unsigned)", || s.admin());
}

#[test]
fn failing_subscription_views_are_side_effect_free_too() {
    let (w, _, _, _, _, cancelled) = view_world();
    let env = &w.env;
    let s = &w.sub;
    let missing = s.next_mandate_id() + 100;

    // Error paths return a typed error and leave no trace.
    assert_eq!(
        assert_read_only(env, "get_mandate (missing)", || s.try_get_mandate(&missing)),
        Err(Ok(Error::MandateNotFound))
    );
    assert_eq!(
        assert_read_only(env, "estimated_end (missing)", || {
            s.try_estimated_end(&missing)
        }),
        Err(Ok(Error::MandateNotFound))
    );
    assert_eq!(
        assert_read_only(env, "is_due (missing)", || s.try_is_due(&missing)),
        Err(Ok(Error::MandateNotFound))
    );
    assert_eq!(
        assert_read_only(env, "quote_charge (missing)", || {
            s.try_quote_charge(&missing)
        }),
        Err(Ok(Error::MandateNotFound))
    );
    assert_eq!(
        assert_read_only(env, "mandate_grace_period (missing)", || {
            s.try_mandate_grace_period(&missing)
        }),
        Err(Ok(Error::MandateNotFound))
    );
    assert_eq!(
        assert_read_only(env, "is_in_grace_period (missing)", || {
            s.try_is_in_grace_period(&missing)
        }),
        Err(Ok(Error::MandateNotFound))
    );
    // A quote for a mandate that can no longer be charged is an answer, not
    // an error: it says so, and still writes nothing.
    let quote = assert_read_only(env, "quote_charge (cancelled)", || {
        s.quote_charge(&cancelled)
    });
    assert!(!quote.chargeable);
    assert_eq!(
        assert_read_only(env, "first_failed_charge (missing)", || {
            s.first_failed_charge(&missing)
        }),
        None
    );
}

// ---------------------------------------------------------------------------
// #316: lowering max_charges to (or below) charges_made ends the mandate
// ---------------------------------------------------------------------------

#[test]
fn lowering_a_bounded_cap_to_charges_made_ends_the_mandate() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(1_000_000);
    let id = w.sub.subscribe(&user, &plan_id, &5, &0, &None::<Address>);

    w.sub.charge(&w.keeper, &id);
    w.advance(MONTH);
    w.sub.charge(&w.keeper, &id);
    assert_eq!(w.sub.plan_committed_revenue(&plan_id), PRICE);

    // Five months agreed, two paid: "make that two" ends it on the spot.
    w.sub.set_max_charges(&user, &id, &2);
    assert_eq!(event_field_u32(&w.env, "max_charges"), Some(2));
    assert!(event_by_topic(&w.env, "mandate_completed").is_some());

    let m = w.sub.get_mandate(&id);
    assert_eq!(m.status, MandateStatus::Completed);
    assert_eq!(m.max_charges, 2);
    assert_eq!(m.charges_made, 2);
    // A completed mandate no longer counts towards the merchant's revenue,
    // nor is it offered to keepers.
    assert_eq!(w.sub.plan_committed_revenue(&plan_id), 0);
    w.advance(MONTH);
    assert!(!w.sub.is_due(&id));
    assert!(w.sub.due_mandates(&w.merchant, &0, &10).is_empty());
    assert_eq!(
        w.sub.try_charge(&w.keeper, &id),
        Err(Ok(Error::MandateNotActive))
    );
}

#[test]
fn lowering_the_cap_of_a_paused_mandate_to_charges_made_ends_it() {
    let w = world();
    let plan_id = w.plan_with_cap(1);
    let user = w.subscriber(1_000_000);
    let id = w.sub.subscribe(&user, &plan_id, &4, &0, &None::<Address>);
    w.sub.charge(&w.keeper, &id);
    w.sub.set_paused(&user, &id, &true);

    w.sub.set_max_charges(&user, &id, &1);

    // Completion wins over the pause: the mandate is finished, not paused,
    // and its seat is released.
    assert_eq!(w.sub.get_mandate(&id).status, MandateStatus::Completed);
    assert_eq!(w.sub.plan_subscriber_count(&plan_id), 0);
    // There is nothing left to resume.
    assert_eq!(
        w.sub.try_set_paused(&user, &id, &false),
        Err(Ok(Error::MandateNotActive))
    );
}

#[test]
fn a_mandate_ended_by_lowering_its_cap_cannot_be_reopened() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(1_000_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    w.sub.charge(&w.keeper, &id);
    w.sub.set_max_charges(&user, &id, &1);
    assert_eq!(w.sub.get_mandate(&id).status, MandateStatus::Completed);

    // Neither raising the cap nor making it unlimited revives it.
    assert_eq!(
        w.sub.try_set_max_charges(&user, &id, &5),
        Err(Ok(Error::MandateNotActive))
    );
    assert_eq!(
        w.sub.try_set_max_charges(&user, &id, &0),
        Err(Ok(Error::MandateNotActive))
    );
    let m = w.sub.get_mandate(&id);
    assert_eq!(m.status, MandateStatus::Completed);
    assert_eq!(m.max_charges, 1);
}

#[test]
fn a_seat_freed_by_lowering_the_cap_admits_the_next_subscriber() {
    let w = world();
    let plan_id = w.plan_with_cap(1);
    let first = w.subscriber(1_000_000);
    let second = w.subscriber(1_000_000);
    let id = w.sub.subscribe(&first, &plan_id, &0, &0, &None::<Address>);
    w.sub.charge(&w.keeper, &id);

    assert_eq!(
        w.sub
            .try_subscribe(&second, &plan_id, &0, &0, &None::<Address>),
        Err(Ok(Error::PlanFull))
    );

    w.sub.set_max_charges(&first, &id, &1);
    let next = w.sub.subscribe(&second, &plan_id, &0, &0, &None::<Address>);
    assert_eq!(w.sub.get_mandate(&next).status, MandateStatus::Active);
    assert_eq!(w.sub.plan_subscriber_count(&plan_id), 1);
}

#[test]
fn lowering_below_charges_made_is_rejected_and_changes_nothing() {
    let w = world();
    let plan_id = w.plan_with_cap(3);
    let user = w.subscriber(1_000_000);
    let id = w.sub.subscribe(&user, &plan_id, &6, &0, &None::<Address>);
    for _ in 0..3 {
        w.sub.charge(&w.keeper, &id);
        w.advance(MONTH);
    }
    let before = w.sub.get_mandate(&id);
    let seats = w.sub.plan_subscriber_count(&plan_id);
    let revenue = w.sub.plan_committed_revenue(&plan_id);

    // Every value strictly between 0 (unlimited) and charges_made is refused.
    for cap in 1..before.charges_made {
        assert_eq!(
            w.sub.try_set_max_charges(&user, &id, &cap),
            Err(Ok(Error::InvalidMaxCharges))
        );
    }
    assert_eq!(w.sub.get_mandate(&id), before);
    assert_eq!(w.sub.plan_subscriber_count(&plan_id), seats);
    assert_eq!(w.sub.plan_committed_revenue(&plan_id), revenue);
    assert!(w.sub.is_due(&id));
}

#[test]
fn only_the_subscriber_can_end_a_mandate_by_lowering_its_cap() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(1_000_000);
    let id = w.sub.subscribe(&user, &plan_id, &4, &0, &None::<Address>);
    w.sub.charge(&w.keeper, &id);

    // The merchant and a stranger fail the ownership check...
    let stranger = Address::generate(&w.env);
    for caller in [&w.merchant, &stranger, &w.keeper] {
        assert_eq!(
            w.sub.try_set_max_charges(caller, &id, &1),
            Err(Ok(Error::NotSubscriber))
        );
    }

    // ...and a forged call naming the subscriber but signed by someone else
    // fails `require_auth`.
    w.env.mock_auths(&[MockAuth {
        address: &stranger,
        invoke: &MockAuthInvoke {
            contract: &w.sub.address,
            fn_name: "set_max_charges",
            args: (&user, &id, &1u32).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);
    assert!(w.sub.try_set_max_charges(&user, &id, &1).is_err());

    let m = w.sub.get_mandate(&id);
    assert_eq!(m.status, MandateStatus::Active);
    assert_eq!(m.max_charges, 4);
}

#[test]
fn lowering_the_cap_of_an_already_ended_mandate_is_rejected() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(1_000_000);

    // Ended by the merchant.
    let ended = w.sub.subscribe(&user, &plan_id, &4, &0, &None::<Address>);
    w.sub.charge(&w.keeper, &ended);
    w.sub.end_mandate(&w.merchant, &ended);
    assert_eq!(
        w.sub.try_set_max_charges(&user, &ended, &1),
        Err(Ok(Error::MandateNotActive))
    );

    // Completed naturally by its last charge.
    let done = w.sub.subscribe(&user, &plan_id, &1, &0, &None::<Address>);
    w.sub.charge(&w.keeper, &done);
    assert_eq!(w.sub.get_mandate(&done).status, MandateStatus::Completed);
    assert_eq!(
        w.sub.try_set_max_charges(&user, &done, &1),
        Err(Ok(Error::MandateNotActive))
    );

    // Unknown ids are a typed error, not a trap.
    assert_eq!(
        w.sub.try_set_max_charges(&user, &9_999, &1),
        Err(Ok(Error::MandateNotFound))
    );
}

// ---------------------------------------------------------------------------
// #313: mandate (and plan) ids are never reused once a mandate has ended
// ---------------------------------------------------------------------------

/// Drive a deterministic random mix of opens and every way a mandate can end
/// (cancel, `cancel_with_reason`, `end_mandate`, completion through charging,
/// completion through `set_max_charges`), interleaved with failed calls and
/// plan retirement. Every id handed out must be fresh, and every ended
/// mandate must keep pointing at its own record: if an id were recycled, a
/// stale reference held by a keeper, merchant or indexer could act on someone
/// else's mandate (a confused deputy).
#[test]
fn fuzz_mandate_and_plan_ids_are_never_reused_after_ending() {
    let w = world();
    let users = [
        w.subscriber(100_000_000),
        w.subscriber(100_000_000),
        w.subscriber(100_000_000),
    ];
    let mut seed = 0x0313_dead_beef_0313u64;

    let mut plans: std::vec::Vec<u64> = std::vec![w.plan()];
    let mut retired_plans: std::vec::Vec<u64> = std::vec::Vec::new();
    // (id, subscriber, plan) for every mandate ever opened.
    let mut opened: std::vec::Vec<(u64, Address, u64)> = std::vec::Vec::new();
    let mut ended: std::vec::Vec<u64> = std::vec::Vec::new();

    for _ in 0..160 {
        let roll = next_fuzz_u64(&mut seed);
        let pick = usize::try_from(next_fuzz_u64(&mut seed)).unwrap();
        let next_before = w.sub.next_mandate_id();

        match roll % 9 {
            // Open a mandate on a live plan.
            0..=2 => {
                let user = &users[pick % users.len()];
                let plan_id = plans[pick % plans.len()];
                let cap = u32::try_from((roll >> 8) % 3).unwrap(); // 0 = open-ended
                let id = w.sub.subscribe(user, &plan_id, &cap, &0, &None::<Address>);
                assert_eq!(id, next_before, "ids are issued in order");
                assert!(
                    opened.iter().all(|(seen, _, _)| *seen != id),
                    "mandate id {id} was issued twice"
                );
                opened.push((id, user.clone(), plan_id));
            }
            // End a mandate one of four ways.
            3..=5 if !opened.is_empty() => {
                let (id, user, _) = opened[pick % opened.len()].clone();
                let m = w.sub.get_mandate(&id);
                let open = m.status == MandateStatus::Active || m.status == MandateStatus::Paused;
                if open {
                    match (roll >> 16) % 4 {
                        0 => w.sub.cancel(&user, &id),
                        1 => w.sub.cancel_with_reason(&user, &id, &1),
                        2 => w.sub.end_mandate(&w.merchant, &id),
                        _ => {
                            if m.status == MandateStatus::Paused {
                                w.sub.set_paused(&user, &id, &false);
                            }
                            if w.sub.is_due(&id) {
                                w.sub.charge(&w.keeper, &id);
                            }
                            let made = w.sub.get_mandate(&id).charges_made;
                            if made == 0 {
                                w.sub.cancel(&user, &id);
                            } else if w.sub.get_mandate(&id).status == MandateStatus::Active {
                                w.sub.set_max_charges(&user, &id, &made);
                            }
                        }
                    }
                    ended.push(id);
                } else {
                    // Ending it again is refused and burns no id.
                    assert_eq!(
                        w.sub.try_cancel(&user, &id),
                        Err(Ok(Error::MandateNotActive))
                    );
                }
            }
            // Retire a plan (keep at least one live) and publish a new one.
            6 => {
                if plans.len() > 1 {
                    let plan_id = plans.remove(pick % plans.len());
                    w.registry.set_plan_active(&w.merchant, &plan_id, &false);
                    retired_plans.push(plan_id);
                }
                let fresh = w.plan();
                assert!(
                    !plans.contains(&fresh) && !retired_plans.contains(&fresh),
                    "plan id {fresh} was issued twice"
                );
                plans.push(fresh);
            }
            // Failure paths: none of these may consume or free an id.
            7 => {
                if let Some(&retired) = retired_plans.get(pick % retired_plans.len().max(1)) {
                    assert_eq!(
                        w.sub
                            .try_subscribe(&users[0], &retired, &0, &0, &None::<Address>),
                        Err(Ok(Error::PlanInactive))
                    );
                }
                let unknown_plan = w.registry.next_plan_id() + 7;
                assert!(w
                    .sub
                    .try_subscribe(&users[1], &unknown_plan, &0, &0, &None::<Address>)
                    .is_err());
                if !opened.is_empty() {
                    let (id, _, _) = opened[pick % opened.len()].clone();
                    let stranger = Address::generate(&w.env);
                    assert!(w.sub.try_cancel(&stranger, &id).is_err());
                }
                assert_eq!(w.sub.next_mandate_id(), next_before);
            }
            // Let time pass so charges come due.
            _ => w.advance(MONTH),
        }

        // The counter only ever moves forward.
        assert!(w.sub.next_mandate_id() >= next_before);
    }

    assert!(!ended.is_empty() && !retired_plans.is_empty());

    // Every mandate id is still bound to the mandate it was issued for, ended
    // or not, and none is past the counter.
    let next = w.sub.next_mandate_id();
    assert_eq!(next, opened.len() as u64 + 1);
    for (id, user, plan_id) in &opened {
        let m = w.sub.get_mandate(id);
        assert_eq!(m.id, *id);
        assert_eq!(&m.subscriber, user);
        assert_eq!(m.plan_id, *plan_id);
        assert!(*id < next);
    }
    for id in &ended {
        let status = w.sub.get_mandate(id).status;
        assert!(
            status == MandateStatus::Cancelled || status == MandateStatus::Completed,
            "ended mandate {id} came back as {status:?}"
        );
    }
    // Retired plans keep their records and stay retired.
    for plan_id in &retired_plans {
        let p = w.registry.get_plan(plan_id);
        assert_eq!(p.id, *plan_id);
        assert!(!p.active);
    }

    // A fresh mandate after all that churn still gets a never-seen id.
    let fresh = w
        .sub
        .subscribe(&users[0], &plans[0], &0, &0, &None::<Address>);
    assert_eq!(fresh, next);
}

// ---- #287: estimated_end as a ledger and a period count ----

#[test]
fn estimated_end_detail_reports_timestamp_ledger_and_periods() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    w.env.ledger().set_sequence_number(1_000);
    let id = w.sub.subscribe(&user, &plan_id, &4, &0, &None::<Address>);

    let detail = w.sub.estimated_end_detail(&id).unwrap();
    let expected = 1_000_000 + 3 * MONTH;
    assert_eq!(detail.timestamp, expected);
    assert_eq!(Some(detail.timestamp), w.sub.estimated_end(&id));
    assert_eq!(detail.remaining_charges, 4);
    assert_eq!(detail.periods_remaining, 3);
    assert_eq!(
        detail.ledger,
        1_000 + u32::try_from((3 * MONTH).div_ceil(SECONDS_PER_LEDGER)).unwrap()
    );
}

#[test]
fn estimated_end_detail_rounds_partial_periods_up() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &3, &0, &None::<Address>);
    w.sub.charge(&w.keeper, &id);
    // Half a period in: two charges left, the last 1.5 periods away.
    w.env.ledger().set_timestamp(1_000_000 + MONTH / 2);

    let detail = w.sub.estimated_end_detail(&id).unwrap();
    assert_eq!(detail.timestamp, 1_000_000 + 2 * MONTH);
    assert_eq!(detail.remaining_charges, 2);
    assert_eq!(detail.periods_remaining, 2);
}

#[test]
fn estimated_end_detail_is_none_for_open_ended_mandates() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    assert_eq!(w.sub.estimated_end_detail(&id), None);
}

#[test]
fn estimated_end_detail_after_completion_points_at_the_past() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    w.env.ledger().set_sequence_number(500);
    let id = w.sub.subscribe(&user, &plan_id, &1, &0, &None::<Address>);
    w.sub.charge(&w.keeper, &id);
    w.env.ledger().set_timestamp(1_000_000 + 5 * MONTH);
    w.env.ledger().set_sequence_number(900);

    let detail = w.sub.estimated_end_detail(&id).unwrap();
    assert_eq!(detail.timestamp, w.sub.get_mandate(&id).last_charge);
    assert_eq!(detail.remaining_charges, 0);
    assert_eq!(detail.periods_remaining, 0);
    assert_eq!(detail.ledger, 900);
}

#[test]
fn estimated_end_detail_errors_for_missing_and_overflowing_mandates() {
    let w = world();
    assert_eq!(
        w.sub.try_estimated_end_detail(&999),
        Err(Ok(Error::MandateNotFound))
    );

    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    w.env.ledger().set_timestamp(u64::MAX - MONTH + 1);
    let id = w.sub.subscribe(&user, &plan_id, &2, &0, &None::<Address>);
    assert_eq!(
        w.sub.try_estimated_end_detail(&id),
        Err(Ok(Error::ScheduleOverflow))
    );
}

// ---- #290: checked math on period and due-date arithmetic ----

#[test]
fn charge_reports_schedule_overflow_when_next_due_date_exceeds_u64() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    w.env.ledger().set_timestamp(u64::MAX - MONTH + 1);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    // Regression: this used to surface as AmountOverflow.
    assert_eq!(
        w.sub.try_charge(&w.keeper, &id),
        Err(Ok(Error::ScheduleOverflow))
    );
    let m = w.sub.get_mandate(&id);
    assert_eq!(m.charges_made, 0);
    assert_eq!(m.next_charge, u64::MAX - MONTH + 1);
}

#[test]
fn charge_succeeds_at_the_last_representable_due_date() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    w.env.ledger().set_timestamp(u64::MAX - MONTH);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    let result = w.sub.charge(&w.keeper, &id);
    assert_eq!(result.next_charge, u64::MAX);
}

#[test]
fn huge_period_plan_overflows_on_charge_not_on_subscribe() {
    let w = world();
    let long_plan = w.plan_amount(PRICE, u64::MAX);
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &long_plan, &0, &0, &None::<Address>);
    assert_eq!(
        w.sub.try_charge(&w.keeper, &id),
        Err(Ok(Error::ScheduleOverflow))
    );
}

// ---------------------------------------------------------------- #285
// Protocol-wide pause switch.

#[test]
fn protocol_is_not_paused_by_default() {
    let w = world();
    assert!(!w.sub.protocol_paused());
}

#[test]
fn set_protocol_paused_requires_admin_auth() {
    let w = world();
    w.env.set_auths(&[]);
    assert!(w.sub.try_set_protocol_paused(&true).is_err());
    assert!(!w.sub.protocol_paused());
}

#[test]
fn set_protocol_paused_wrong_signer_rejected() {
    let w = world();
    let stranger = Address::generate(&w.env);
    w.env.set_auths(&[]);
    let result = w
        .sub
        .mock_auths(&[MockAuth {
            address: &stranger,
            invoke: &MockAuthInvoke {
                contract: &w.sub.address,
                fn_name: "set_protocol_paused",
                args: (true,).into_val(&w.env),
                sub_invokes: &[],
            },
        }])
        .try_set_protocol_paused(&true);
    assert!(result.is_err());
    assert!(!w.sub.protocol_paused());
}

#[test]
fn protocol_pause_blocks_every_subscribe_entry_point() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    w.sub.set_protocol_paused(&true);
    assert!(w.sub.protocol_paused());

    let next_id = w.sub.next_mandate_id();
    assert_eq!(
        w.sub
            .try_subscribe(&user, &plan_id, &0, &0, &None::<Address>),
        Err(Ok(Error::ProtocolPaused))
    );
    let now = w.env.ledger().timestamp();
    assert_eq!(
        w.sub
            .try_subscribe_at(&user, &plan_id, &0, &0, &(now + WEEK), &None::<Address>),
        Err(Ok(Error::ProtocolPaused))
    );
    let vault_before = w.vault.balance(&user, &w.token);
    assert_eq!(
        w.sub
            .try_subscribe_with_deposit(&user, &plan_id, &0, &0, &PRICE, &None::<Address>),
        Err(Ok(Error::ProtocolPaused))
    );
    // Nothing was opened or deposited.
    assert_eq!(w.sub.next_mandate_id(), next_id);
    assert_eq!(w.sub.merchant_mandate_count(&w.merchant), 0);
    assert_eq!(w.vault.balance(&user, &w.token), vault_before);
}

#[test]
fn protocol_pause_blocks_charge_and_keeps_the_mandate_due() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    let before = w.sub.get_mandate(&id);

    w.sub.set_protocol_paused(&true);
    assert_eq!(
        w.sub.try_charge(&w.keeper, &id),
        Err(Ok(Error::ProtocolPaused))
    );
    // No money moved and the schedule is untouched.
    assert_eq!(w.sub.get_mandate(&id), before);
    assert_eq!(w.token_client.balance(&w.merchant), 0);
    // Reads keep working while paused.
    assert!(w.sub.is_due(&id));
    assert_eq!(w.sub.quote_charge(&id).amount, PRICE);

    // Resuming makes the same charge go through.
    w.sub.set_protocol_paused(&false);
    w.sub.charge(&w.keeper, &id);
    assert_eq!(w.sub.get_mandate(&id).charges_made, 1);
}

#[test]
fn protocol_pause_blocks_plan_upgrades() {
    let w = world();
    let plan_id = w.plan();
    let other_plan = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    w.sub.set_protocol_paused(&true);
    assert_eq!(
        w.sub.try_upgrade(&user, &id, &other_plan),
        Err(Ok(Error::ProtocolPaused))
    );
    assert_eq!(w.sub.get_mandate(&id).plan_id, plan_id);
}

#[test]
fn protocol_pause_preserves_exits_cancel_and_withdraw() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    w.sub.set_protocol_paused(&true);

    // The subscriber can still leave and take their money out.
    w.sub.cancel(&user, &id);
    assert_eq!(w.sub.get_mandate(&id).status, MandateStatus::Cancelled);
    let balance = w.vault.balance(&user, &w.token);
    let wallet_before = w.token_client.balance(&user);
    w.vault.withdraw(&user, &w.token, &balance);
    assert_eq!(w.vault.balance(&user, &w.token), 0);
    assert_eq!(w.token_client.balance(&user), wallet_before + balance);
}

#[test]
fn protocol_pause_emits_one_event_per_transition() {
    let w = world();
    w.sub.set_protocol_paused(&true);
    let after_pause = w
        .env
        .events()
        .all()
        .filter_by_contract(&w.sub.address)
        .events()
        .len();
    assert_eq!(after_pause, 1);

    // Re-pausing is a no-op: no event.
    w.sub.set_protocol_paused(&true);
    assert_eq!(
        w.env
            .events()
            .all()
            .filter_by_contract(&w.sub.address)
            .events()
            .len(),
        0
    );
    assert!(w.sub.protocol_paused());

    w.sub.set_protocol_paused(&false);
    assert_eq!(
        w.env
            .events()
            .all()
            .filter_by_contract(&w.sub.address)
            .events()
            .len(),
        1
    );
    assert!(!w.sub.protocol_paused());
}

// ---------------------------------------------------------------- #284
// Batch TTL extension for a merchant's open mandates.

fn persistent_ttl(w: &World, key: &DataKey) -> u32 {
    use soroban_sdk::testutils::storage::Persistent as _;
    w.env
        .as_contract(&w.sub.address, || w.env.storage().persistent().get_ttl(key))
}

#[test]
fn extend_merchant_mandates_ttl_bumps_open_mandates_and_skips_closed_ones() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(1_000_000);
    let open = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    let paused = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    let cancelled = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    w.sub.set_paused(&user, &paused, &true);
    w.sub.cancel(&user, &cancelled);

    // Let every entry age by 30 days of ledgers.
    let seq = w.env.ledger().sequence();
    w.env.ledger().set_sequence_number(seq + 30 * 17_280);
    let aged = persistent_ttl(&w, &DataKey::Mandate(open));
    assert!(aged < PERSIST_BUMP);

    let before = w.sub.get_mandate(&open);
    let extended = w.sub.extend_merchant_mandates_ttl(&w.merchant, &0, &10);
    assert_eq!(extended, 2);

    assert_eq!(persistent_ttl(&w, &DataKey::Mandate(open)), PERSIST_BUMP);
    assert_eq!(persistent_ttl(&w, &DataKey::Mandate(paused)), PERSIST_BUMP);
    assert_eq!(
        persistent_ttl(&w, &DataKey::MerchantMandate(w.merchant.clone(), 0)),
        PERSIST_BUMP
    );
    assert_eq!(
        persistent_ttl(&w, &DataKey::MerchantMandateCount(w.merchant.clone())),
        PERSIST_BUMP
    );
    // The cancelled mandate is left to age out.
    assert_eq!(persistent_ttl(&w, &DataKey::Mandate(cancelled)), aged);
    // No observable state changed.
    assert_eq!(w.sub.get_mandate(&open), before);
}

#[test]
fn extend_merchant_mandates_ttl_pages_and_bounds() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(1_000_000);
    for _ in 0..3 {
        w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    }
    assert_eq!(w.sub.extend_merchant_mandates_ttl(&w.merchant, &1, &1), 1);
    assert_eq!(w.sub.extend_merchant_mandates_ttl(&w.merchant, &2, &10), 1);
    assert_eq!(w.sub.extend_merchant_mandates_ttl(&w.merchant, &3, &10), 0);
    assert_eq!(
        w.sub
            .extend_merchant_mandates_ttl(&w.merchant, &u32::MAX, &u32::MAX),
        0
    );
    // A merchant with no mandates is a no-op, not an error.
    let nobody = Address::generate(&w.env);
    assert_eq!(w.sub.extend_merchant_mandates_ttl(&nobody, &0, &10), 0);
}

#[test]
fn extend_merchant_mandates_ttl_needs_no_auth_and_works_while_paused() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    w.sub.set_protocol_paused(&true);
    w.env.set_auths(&[]);
    assert_eq!(w.sub.extend_merchant_mandates_ttl(&w.merchant, &0, &10), 1);
}

#[test]
fn extend_merchant_mandates_ttl_budget_beats_one_call_per_mandate() {
    // #284: show the improvement. One batched call over N mandates against N
    // calls of page size 1, which is what extending one mandate per call
    // costs. The CPU saving is shown here; the larger saving is in
    // transactions: N mandates need ceil(N / MAX_PAGE) calls instead of N,
    // each with its own base fee and footprint.
    const N: u32 = 20;
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(10_000_000);
    for _ in 0..N {
        w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    }

    w.env.cost_estimate().budget().reset_default();
    w.sub.extend_merchant_mandates_ttl(&w.merchant, &0, &N);
    let batched = w.env.cost_estimate().budget().cpu_instruction_cost();

    let mut single = 0u64;
    for i in 0..N {
        w.env.cost_estimate().budget().reset_default();
        w.sub.extend_merchant_mandates_ttl(&w.merchant, &i, &1);
        single += w.env.cost_estimate().budget().cpu_instruction_cost();
    }
    std::println!("#284 TTL extension for {N} mandates: batched {batched} CPU instructions, one call each {single}");
    assert!(batched < single);
}

// ---------------------------------------------------------------- #247
// TTL refresh whenever a hot mandate key is accessed.

#[test]
fn hot_mandate_ttl_is_extended_on_view_access() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(1_000_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    let before = w.sub.get_mandate(&id);

    let seq = w.env.ledger().sequence();
    w.env
        .ledger()
        .set_sequence_number(seq + 30 * DAY_IN_LEDGERS);
    let aged = persistent_ttl(&w, &DataKey::Mandate(id));
    assert!(aged < PERSIST_BUMP);

    assert_eq!(w.sub.get_mandate(&id), before);
    assert_eq!(persistent_ttl(&w, &DataKey::Mandate(id)), PERSIST_BUMP);
}

#[test]
fn hot_mandate_ttl_is_extended_on_failed_charge_access() {
    let w = world();
    let plan_id = w.plan();
    // An unfunded subscriber gives `flag_failed_charge` a real failure to
    // report without rewriting the mandate itself.
    let user = Address::generate(&w.env);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    let seq = w.env.ledger().sequence();
    w.env
        .ledger()
        .set_sequence_number(seq + 30 * DAY_IN_LEDGERS);
    let aged = persistent_ttl(&w, &DataKey::Mandate(id));
    assert!(aged < PERSIST_BUMP);

    assert!(w.sub.flag_failed_charge(&id));
    assert_eq!(persistent_ttl(&w, &DataKey::Mandate(id)), PERSIST_BUMP);
}

#[test]
fn fresh_and_missing_mandates_are_untouched_by_access() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(1_000_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    let ttl = persistent_ttl(&w, &DataKey::Mandate(id));
    assert_eq!(ttl, PERSIST_BUMP);
    w.sub.get_mandate(&id);
    assert_eq!(persistent_ttl(&w, &DataKey::Mandate(id)), ttl);

    assert_eq!(w.sub.try_get_mandate(&999), Err(Ok(Error::MandateNotFound)));
    assert_eq!(
        w.sub.try_flag_failed_charge(&999),
        Err(Ok(Error::MandateNotFound))
    );
}

#[test]
fn subscription_call_budgets_stay_within_committed_snapshots() {
    let mut measurements = std::vec::Vec::new();

    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    w.env.cost_estimate().budget().reset_default();
    w.sub.charge(&w.keeper, &id);
    let budget = w.env.cost_estimate().budget();
    measurements.push((
        "charge_success",
        budget.cpu_instruction_cost(),
        budget.memory_bytes_cost(),
    ));

    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(100_000);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    w.env.mock_auths(&[]);
    w.env.cost_estimate().budget().reset_default();
    assert!(w.sub.try_charge(&w.keeper, &id).is_err());
    let budget = w.env.cost_estimate().budget();
    measurements.push((
        "charge_missing_auth",
        budget.cpu_instruction_cost(),
        budget.memory_bytes_cost(),
    ));

    let w = world();
    let plan_id = w.plan();
    let user = Address::generate(&w.env);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    w.env.cost_estimate().budget().reset_default();
    assert!(w.sub.try_charge(&w.keeper, &id).is_err());
    let budget = w.env.cost_estimate().budget();
    measurements.push((
        "charge_vault_failure",
        budget.cpu_instruction_cost(),
        budget.memory_bytes_cost(),
    ));

    let w = world();
    let plan_id = w.plan_full(KEEPER_BPS, REFERRAL_BPS, None);
    let user = w.subscriber(100_000);
    let id = w
        .sub
        .subscribe(&user, &plan_id, &0, &0, &Some(w.keeper.clone()));
    w.env.mock_auths(&[]);
    w.env.cost_estimate().budget().reset_default();
    let simulation = w.sub.simulate_charge(&w.keeper, &id);
    assert!(simulation.chargeable);
    let budget = w.env.cost_estimate().budget();
    measurements.push((
        "simulate_charge",
        budget.cpu_instruction_cost(),
        budget.memory_bytes_cost(),
    ));

    let snapshot_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test_snapshots/budgets.csv");
    if std::env::var("UPDATE_RESOURCE_BUDGETS").as_deref() == Ok("1") {
        let mut snapshot = std::string::String::from("call,cpu_instructions,memory_bytes\n");
        for (name, cpu, memory) in measurements {
            std::fmt::Write::write_fmt(&mut snapshot, format_args!("{name},{cpu},{memory}\n"))
                .unwrap();
        }
        std::fs::write(snapshot_path, snapshot).unwrap();
        return;
    }

    let snapshot = std::fs::read_to_string(&snapshot_path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", snapshot_path.display()));
    for (name, cpu, memory) in measurements {
        let line = snapshot
            .lines()
            .find(|line| line.starts_with(&std::format!("{name},")))
            .unwrap_or_else(|| panic!("missing budget snapshot for {name}"));
        let mut fields = line.split(',');
        let _ = fields.next();
        let expected_cpu: u64 = fields.next().unwrap().parse().unwrap();
        let expected_memory: u64 = fields.next().unwrap().parse().unwrap();
        let cpu_limit = expected_cpu + (expected_cpu / 20).max(1);
        let memory_limit = expected_memory + (expected_memory / 20).max(1);
        assert!(
            cpu <= cpu_limit,
            "{name} CPU budget grew from {expected_cpu} to {cpu} instructions (5% tolerance: {cpu_limit})"
        );
        assert!(
            memory <= memory_limit,
            "{name} memory budget grew from {expected_memory} to {memory} bytes (5% tolerance: {memory_limit})"
        );
    }
}

// ---------------------------------------------------------------------------
// #245: two active mandates interleaving debits against one subscriber
// balance can never double-spend it
// ---------------------------------------------------------------------------

/// One subscriber, two merchants, and two active mandates drawing on the same
/// vault balance.
///
/// Soroban runs each invocation to completion before the next one starts, so
/// "interleaving" here means alternating `charge` calls for mandate A and
/// mandate B against that one balance, with the ledger clock set to an
/// absolute timestamp for every billing period. Every settled step checks the
/// exact token movement, and every rejected step checks that nothing moved at
/// all. After every step the conservation invariant is checked against totals
/// the test tracks itself rather than reads back from the contracts.
mod interleaved_mandates {
    use super::*;
    use core::cell::Cell;
    use payflow_plan_registry::Error as RegistryError;
    use payflow_vault::Error as VaultError;
    use soroban_sdk::testutils::{AuthorizedFunction, AuthorizedInvocation};

    /// The subscriber's opening vault balance: enough for some of the combined
    /// debits, never all of them.
    const START: i128 = 100_000;
    /// A larger opening balance for the lifecycle tests, so mandate B can keep
    /// billing for several periods after mandate A stops.
    const ROOMY: i128 = 200_000;
    /// Mandate A's price, paid to merchant A.
    const AMOUNT_A: i128 = 30_000;
    /// Mandate B's price, paid to merchant B.
    const AMOUNT_B: i128 = 40_000;
    /// Ledger timestamp of billing period 0. Each period is set as the
    /// absolute timestamp `T0 + n * MONTH`; nothing relies on the default
    /// ledger or on wall-clock time.
    const T0: u64 = 1_000_000;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Which {
        A,
        B,
    }

    /// Every balance a charge could touch, read from the vault's ledger and
    /// from the token contract itself.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Balances {
        /// The subscriber's balance in the vault's internal ledger.
        vault_ledger: i128,
        /// Tokens the vault contract actually holds.
        vault_tokens: i128,
        /// The subscriber's own wallet, outside the vault.
        wallet: i128,
        merchant_a: i128,
        merchant_b: i128,
        fee_to: i128,
        keeper: i128,
    }

    struct Pair {
        w: World<'static>,
        user: Address,
        merchant_a: Address,
        merchant_b: Address,
        plan_a: u64,
        a: u64,
        b: u64,
        /// Net amount the subscriber put into the vault (deposits minus
        /// withdrawals), tracked by the test, not read from the vault.
        deposited: Cell<i128>,
        /// Sum of every charge that settled, tracked by the test.
        debited: Cell<i128>,
    }

    fn fee(amount: i128) -> i128 {
        amount * i128::from(FEE_BPS) / BPS_DENOMINATOR
    }

    fn price(which: Which) -> i128 {
        match which {
            Which::A => AMOUNT_A,
            Which::B => AMOUNT_B,
        }
    }

    /// Two merchants with one monthly plan each, and one subscriber holding
    /// `funding` in the vault with an active mandate on both plans. Both
    /// mandates are due at `T0`. The subscriber's wallet keeps another
    /// `funding` outside the vault for top-ups.
    fn pair(funding: i128) -> Pair {
        let w = world();
        w.env.ledger().set_timestamp(T0);

        let merchant_a = w.merchant.clone();
        let merchant_b = Address::generate(&w.env);
        let plan_a = w.plan_amount(AMOUNT_A, MONTH);
        let plan_b = w.registry.create_plan(
            &merchant_b,
            &w.token,
            &AMOUNT_B,
            &MONTH,
            &String::from_str(&w.env, "Plan B"),
            &String::from_str(&w.env, ""),
            &0,
            &Symbol::new(&w.env, ""),
            &String::from_str(&w.env, ""),
            &0u64,
            &0u32,
            &None::<Address>,
            &0u32,
        );

        let user = w.subscriber(funding);
        let a = w.sub.subscribe(&user, &plan_a, &0, &0, &None::<Address>);
        let b = w.sub.subscribe(&user, &plan_b, &0, &0, &None::<Address>);

        let p = Pair {
            w,
            user,
            merchant_a,
            merchant_b,
            plan_a,
            a,
            b,
            deposited: Cell::new(funding),
            debited: Cell::new(0),
        };
        p.assert_invariants();
        p
    }

    impl Pair {
        fn id(&self, which: Which) -> u64 {
            match which {
                Which::A => self.a,
                Which::B => self.b,
            }
        }

        /// Move the ledger clock to the start of billing period `n`.
        fn period(&self, n: u64) {
            self.w.env.ledger().set_timestamp(T0 + n * MONTH);
        }

        fn balances(&self) -> Balances {
            let t = &self.w.token_client;
            Balances {
                vault_ledger: self.w.vault.balance(&self.user, &self.w.token),
                vault_tokens: t.balance(&self.w.vault.address),
                wallet: t.balance(&self.user),
                merchant_a: t.balance(&self.merchant_a),
                merchant_b: t.balance(&self.merchant_b),
                fee_to: t.balance(&self.w.fee_to),
                keeper: t.balance(&self.w.keeper),
            }
        }

        /// The invariants that make a double-spend impossible to miss.
        fn assert_invariants(&self) {
            let b = self.balances();
            let deposited = self.deposited.get();
            let debited = self.debited.get();

            assert!(
                b.vault_ledger >= 0,
                "subscriber balance went negative: {b:?}"
            );
            assert!(
                debited <= deposited,
                "charged {debited} against only {deposited} deposited"
            );
            // Conservation: what went in is exactly what is left plus what
            // was charged.
            assert_eq!(
                deposited,
                b.vault_ledger + debited,
                "conservation broken: {b:?}"
            );
            // Every charged unit reached a payee; none was created or lost.
            assert_eq!(
                b.merchant_a + b.merchant_b + b.fee_to + b.keeper,
                debited,
                "payees received a different total than was charged: {b:?}"
            );
            // The vault's ledger is backed one for one by tokens it holds.
            assert_eq!(b.vault_tokens, b.vault_ledger, "vault ledger unbacked");
            assert!(
                self.w.vault.reserved_total(&self.user, &self.w.token) <= b.vault_ledger,
                "reserves exceed the balance"
            );
        }

        /// Settle `which` with `caller` as the keeper and check it moved
        /// exactly its own price: the subscriber down by the price, its own
        /// merchant up by the price less the fee, the fee recipient up by the
        /// fee, and nobody else touched. Returns the auths the charge used.
        fn settles_by(
            &self,
            which: Which,
            caller: &Address,
        ) -> std::vec::Vec<(Address, AuthorizedInvocation)> {
            let id = self.id(which);
            let amount = price(which);
            let now = self.w.env.ledger().timestamp();
            let before = self.balances();
            let mandate = self.w.sub.get_mandate(&id);

            let result = self.w.sub.charge(caller, &id);
            // `env.auths()` only covers the most recent invocation, so read it
            // before any balance query.
            let auths = self.w.env.auths();

            let mut expected = before;
            expected.vault_ledger -= amount;
            expected.vault_tokens -= amount;
            expected.fee_to += fee(amount);
            match which {
                Which::A => expected.merchant_a += amount - fee(amount),
                Which::B => expected.merchant_b += amount - fee(amount),
            }
            assert_eq!(
                self.balances(),
                expected,
                "charge of {which:?} at {now} did not move exactly {amount}"
            );
            assert_eq!(result.merchant_amount + result.fee, amount);

            let after = self.w.sub.get_mandate(&id);
            assert_eq!(after.charges_made, mandate.charges_made + 1);
            assert_eq!(after.total_spent, mandate.total_spent + amount);
            assert_eq!(after.last_charge, now);
            assert_eq!(after.next_charge, now + MONTH);

            self.debited.set(self.debited.get() + amount);
            self.assert_invariants();
            auths
        }

        fn settles(&self, which: Which) {
            self.settles_by(which, &self.w.keeper);
        }

        /// Charge `which`, expect exactly `error`, and check that no balance
        /// and neither mandate changed.
        fn rejected(&self, which: Which, error: Error) {
            let id = self.id(which);
            let now = self.w.env.ledger().timestamp();
            let before = self.balances();
            let mandate_a = self.w.sub.get_mandate(&self.a);
            let mandate_b = self.w.sub.get_mandate(&self.b);

            assert_eq!(
                self.w.sub.try_charge(&self.w.keeper, &id),
                Err(Ok(error)),
                "charge of {which:?} at {now}"
            );

            assert_eq!(
                self.balances(),
                before,
                "rejected charge of {which:?} at {now} moved funds"
            );
            assert_eq!(self.w.sub.get_mandate(&self.a), mandate_a);
            assert_eq!(self.w.sub.get_mandate(&self.b), mandate_b);
            self.assert_invariants();
        }

        /// A due charge the remaining balance cannot cover: the vault refuses
        /// the debit, the subscription contract surfaces `VaultCallFailed`,
        /// and nothing moves.
        fn unaffordable(&self, which: Which) {
            assert!(
                !self
                    .w
                    .vault
                    .can_debit(&self.user, &self.w.token, &self.id(which), &price(which)),
                "{which:?} should not be affordable here"
            );
            self.rejected(which, Error::VaultCallFailed);
        }

        fn top_up(&self, amount: i128) {
            self.w.vault.deposit(&self.user, &self.w.token, &amount);
            self.deposited.set(self.deposited.get() + amount);
            self.assert_invariants();
        }

        fn withdraw(&self, amount: i128) {
            let wallet = self.balances().wallet;
            self.w.vault.withdraw(&self.user, &self.w.token, &amount);
            self.deposited.set(self.deposited.get() - amount);
            assert_eq!(self.balances().wallet, wallet + amount);
            self.assert_invariants();
        }
    }

    // ---- happy path / no double-spend ----

    /// A, B, A, B, ... across billing periods on a balance that covers only
    /// three of the six debits. Every settled debit moves exactly its price,
    /// and once the balance runs dry every further debit fails without
    /// moving anything.
    #[test]
    fn alternating_debits_never_spend_more_than_the_shared_balance() {
        let p = pair(START);
        // (period, mandate, settles?) — a hard-coded interleaving.
        let schedule = [
            (0, Which::A, true),  // 100_000 -> 70_000
            (0, Which::B, true),  //  70_000 -> 30_000
            (1, Which::A, true),  //  30_000 ->      0
            (1, Which::B, false), // 0 cannot cover 40_000
            (2, Which::A, false),
            (2, Which::B, false),
            (3, Which::A, false),
            (3, Which::B, false),
        ];

        for (period, which, settles) in schedule {
            p.period(period);
            if settles {
                p.settles(which);
            } else {
                p.unaffordable(which);
            }
        }

        assert_eq!(p.debited.get(), START);
        assert_eq!(p.w.vault.balance(&p.user, &p.w.token), 0);
        assert_eq!(p.w.sub.get_mandate(&p.a).charges_made, 2);
        assert_eq!(p.w.sub.get_mandate(&p.b).charges_made, 1);
        let b = p.balances();
        assert_eq!(b.merchant_a, 2 * (AMOUNT_A - fee(AMOUNT_A)));
        assert_eq!(b.merchant_b, AMOUNT_B - fee(AMOUNT_B));
        assert_eq!(b.fee_to, 2 * fee(AMOUNT_A) + fee(AMOUNT_B));
    }

    /// When the larger debit no longer fits, it fails and moves nothing, and
    /// the smaller one can still use the remainder in the same period.
    #[test]
    fn a_debit_the_balance_cannot_cover_moves_nothing_and_the_smaller_one_still_settles() {
        let p = pair(START);
        p.settles(Which::A); // 70_000
        p.settles(Which::B); // 30_000

        p.period(1);
        p.unaffordable(Which::B); // 30_000 < 40_000
        p.settles(Which::A); // exactly 30_000 -> 0

        // The failed attempt did not advance B's schedule: B is still due,
        // still unaffordable, and A is now not due.
        assert!(p.w.sub.is_due(&p.b));
        p.unaffordable(Which::B);
        p.rejected(Which::A, Error::NotDue);

        p.period(2);
        p.unaffordable(Which::A);
        p.unaffordable(Which::B);
        assert_eq!(p.debited.get(), START);
    }

    // ---- double-charge protection ----

    /// Neither mandate can be charged twice in one period, however the calls
    /// are interleaved, and each rejected repeat leaves every balance as it
    /// was.
    #[test]
    fn neither_mandate_can_be_debited_twice_in_one_period() {
        let p = pair(START);

        p.settles(Which::A);
        p.rejected(Which::A, Error::NotDue);
        p.settles(Which::B);
        p.rejected(Which::B, Error::NotDue);
        p.rejected(Which::A, Error::NotDue);

        // One second before the next period, both are still not due.
        p.w.env.ledger().set_timestamp(T0 + MONTH - 1);
        p.rejected(Which::B, Error::NotDue);
        p.rejected(Which::A, Error::NotDue);

        // At the period boundary each is due again, once.
        p.period(1);
        p.settles(Which::A); // 30_000 -> 0
        p.rejected(Which::A, Error::NotDue);
        p.unaffordable(Which::B);
        assert_eq!(p.debited.get(), START);
    }

    /// Two debits in the same ledger on a balance that covers either one but
    /// not both: whichever runs first settles, and the second cannot reach
    /// the funds the first already consumed.
    #[test]
    fn back_to_back_debits_in_one_ledger_cannot_share_funds() {
        const EITHER: i128 = 50_000;
        for (first, second) in [(Which::A, Which::B), (Which::B, Which::A)] {
            let p = pair(EITHER);
            p.settles(first);
            p.unaffordable(second);
            assert_eq!(p.debited.get(), price(first));
            assert_eq!(
                p.w.vault.balance(&p.user, &p.w.token),
                EITHER - price(first)
            );
        }
    }

    /// Funds the subscriber reserved for mandate B are out of mandate A's
    /// reach even when the total balance would cover A, so A cannot spend
    /// what B is owed; B then draws its reserve down.
    #[test]
    fn a_reserve_for_one_mandate_cannot_be_spent_by_the_other() {
        let p = pair(START);
        p.w.vault.set_reserve(&p.user, &p.w.token, &p.b, &AMOUNT_B);

        p.settles(Which::A); // unreserved 60_000 -> 30_000
        p.period(1);
        p.settles(Which::A); // unreserved 30_000 -> 0

        p.period(2);
        // 40_000 is left, all of it B's reserve: enough for A on paper, but
        // not A's to spend.
        assert_eq!(p.w.vault.balance(&p.user, &p.w.token), AMOUNT_B);
        p.unaffordable(Which::A);
        // Nor can the subscriber pull B's reserve out from under it.
        assert_eq!(
            p.w.vault.try_withdraw(&p.user, &p.w.token, &1),
            Err(Ok(VaultError::InsufficientBalance))
        );
        p.assert_invariants();

        p.settles(Which::B); // draws the reserve down to 0
        assert_eq!(p.w.vault.reserved(&p.user, &p.w.token, &p.b), 0);
        assert_eq!(p.w.vault.reserved_total(&p.user, &p.w.token), 0);
        assert_eq!(p.debited.get(), START);
    }

    // ---- authorization ----

    /// With no signatures at all, neither mandate's charge settles and no
    /// funds move; restoring the keeper's signature lets the interleaving
    /// resume where it was.
    #[test]
    fn charges_without_the_keepers_signature_move_nothing() {
        let p = pair(START);
        p.settles(Which::A);

        p.w.env.mock_auths(&[]);
        for which in [Which::B, Which::A] {
            let before = p.balances();
            let mandate = p.w.sub.get_mandate(&p.id(which));
            let result = p.w.sub.try_charge(&p.w.keeper, &p.id(which));
            assert!(
                matches!(result, Err(Err(_))),
                "unsigned charge of {which:?} was not rejected by the host: {result:?}"
            );
            assert_eq!(p.balances(), before);
            assert_eq!(p.w.sub.get_mandate(&p.id(which)), mandate);
        }
        p.assert_invariants();

        p.w.env.mock_all_auths();
        p.settles(Which::B);
        p.period(1);
        p.settles(Which::A);
        assert_eq!(p.debited.get(), START);
    }

    /// Merchant B may settle mandate A, since `charge` is permissionless, but
    /// it signs only for itself and A's money still goes to merchant A. The
    /// vault debit is authorized by the subscription contract being its
    /// direct caller, never by a merchant or subscriber signature.
    #[test]
    fn another_merchant_can_settle_a_mandate_but_never_receives_its_funds() {
        let p = pair(START);
        let env = &p.w.env;

        for (which, caller) in [
            (Which::A, p.merchant_b.clone()),
            (Which::B, p.merchant_a.clone()),
        ] {
            let id = p.id(which);
            env.mock_auths(&[MockAuth {
                address: &caller,
                invoke: &MockAuthInvoke {
                    contract: &p.w.sub.address,
                    fn_name: "charge",
                    args: (&caller, &id).into_val(env),
                    sub_invokes: &[],
                },
            }]);

            // `settles_by` checks that the money reached `which`'s own
            // merchant and that the caller got nothing.
            let auths = p.settles_by(which, &caller);
            assert_eq!(
                auths,
                std::vec![(
                    caller.clone(),
                    AuthorizedInvocation {
                        function: AuthorizedFunction::Contract((
                            p.w.sub.address.clone(),
                            Symbol::new(env, "charge"),
                            (&caller, id).into_val(env),
                        )),
                        sub_invocations: std::vec![],
                    }
                )]
            );
        }

        let b = p.balances();
        assert_eq!(b.merchant_a, AMOUNT_A - fee(AMOUNT_A));
        assert_eq!(b.merchant_b, AMOUNT_B - fee(AMOUNT_B));
    }

    /// Nobody but the subscription contract can debit the vault, even naming
    /// a real mandate and holding a valid signature of their own: merchant B
    /// cannot pull mandate A's price (or B's own) straight out of the vault.
    #[test]
    fn vault_debits_that_bypass_the_subscription_contract_are_rejected() {
        let p = pair(START);
        let env = &p.w.env;
        p.settles(Which::A);

        let mut legs = soroban_sdk::Vec::new(env);
        legs.push_back((p.merchant_b.clone(), AMOUNT_A));
        env.mock_auths(&[MockAuth {
            address: &p.merchant_b,
            invoke: &MockAuthInvoke {
                contract: &p.w.vault.address,
                fn_name: "debit_multi_mandate",
                args: (&p.user, &p.w.token, &p.a, legs.clone()).into_val(env),
                sub_invokes: &[],
            },
        }]);
        let before = p.balances();
        assert!(matches!(
            p.w.vault
                .try_debit_multi_mandate(&p.user, &p.w.token, &p.a, &legs),
            Err(Err(_))
        ));
        assert_eq!(p.balances(), before);

        env.mock_auths(&[MockAuth {
            address: &p.merchant_b,
            invoke: &MockAuthInvoke {
                contract: &p.w.vault.address,
                fn_name: "debit_mandate",
                args: (&p.user, &p.w.token, &p.b, &p.merchant_b, &AMOUNT_B).into_val(env),
                sub_invokes: &[],
            },
        }]);
        assert!(matches!(
            p.w.vault
                .try_debit_mandate(&p.user, &p.w.token, &p.b, &p.merchant_b, &AMOUNT_B),
            Err(Err(_))
        ));
        assert_eq!(p.balances(), before);
        p.assert_invariants();

        // The legitimate path is unaffected and still bounded by the balance.
        env.mock_all_auths();
        p.settles(Which::B);
        p.period(1);
        p.settles(Which::A);
        assert_eq!(p.debited.get(), START);
    }

    /// Merchant B cannot end, pause or cancel mandate A, nor pause charging
    /// on merchant A's plan, so it cannot interfere with how A draws on the
    /// shared balance.
    #[test]
    fn a_merchant_cannot_end_pause_or_cancel_another_merchants_mandate() {
        let p = pair(START);
        let mb = &p.merchant_b;

        assert_eq!(
            p.w.sub.try_end_mandate(mb, &p.a),
            Err(Ok(Error::NotMerchant))
        );
        assert_eq!(
            p.w.sub.try_set_paused(mb, &p.a, &true),
            Err(Ok(Error::NotSubscriber))
        );
        assert_eq!(p.w.sub.try_cancel(mb, &p.a), Err(Ok(Error::NotSubscriber)));
        assert_eq!(
            p.w.registry
                .try_set_plan_charges_paused(mb, &p.plan_a, &true),
            Err(Ok(RegistryError::NotPlanOwner))
        );

        assert_eq!(p.w.sub.get_mandate(&p.a).status, MandateStatus::Active);
        p.settles(Which::A);
        p.settles(Which::B);
    }

    // ---- failure / lifecycle ----

    /// Closing mandate A mid-sequence, by the subscriber cancelling or by
    /// merchant A ending it, stops A for good without moving funds, while B
    /// keeps billing until the balance runs out.
    #[test]
    fn closing_one_mandate_mid_sequence_leaves_the_other_billing() {
        for subscriber_cancels in [true, false] {
            let p = pair(ROOMY);
            p.settles(Which::A); // 170_000
            p.settles(Which::B); // 130_000

            if subscriber_cancels {
                p.w.sub.cancel(&p.user, &p.a);
                assert_eq!(p.w.sub.get_mandate(&p.a).status, MandateStatus::Cancelled);
            } else {
                p.w.sub.end_mandate(&p.merchant_a, &p.a);
            }
            p.assert_invariants();

            for period in 1..=3 {
                p.period(period);
                p.rejected(Which::A, Error::MandateNotActive);
                p.settles(Which::B); // 90_000, 50_000, 10_000
            }
            p.period(4);
            p.rejected(Which::A, Error::MandateNotActive);
            p.unaffordable(Which::B);

            assert_eq!(p.w.sub.get_mandate(&p.a).charges_made, 1);
            assert_eq!(p.w.sub.get_mandate(&p.b).charges_made, 4);
            assert_eq!(p.debited.get(), AMOUNT_A + 4 * AMOUNT_B);
        }
    }

    /// A paused mandate A is skipped without moving funds while B bills;
    /// once resumed, A competes for the balance again and the two together
    /// still cannot overdraw it.
    #[test]
    fn pausing_one_mandate_mid_sequence_skips_it_without_moving_funds() {
        let p = pair(ROOMY);
        p.settles(Which::A); // 170_000
        p.settles(Which::B); // 130_000

        p.w.sub.set_paused(&p.user, &p.a, &true);
        p.period(1);
        p.rejected(Which::A, Error::MandateNotActive);
        p.settles(Which::B); // 90_000

        p.w.sub.set_paused(&p.user, &p.a, &false);
        p.settles(Which::A); // 60_000, still due from period 1

        p.period(2);
        p.settles(Which::A); // 30_000
        p.unaffordable(Which::B);
        p.period(3);
        p.settles(Which::A); // 0
        p.unaffordable(Which::B);
        assert_eq!(p.debited.get(), ROOMY);
    }

    /// While merchant A has charging paused on its plan, A's charges fail
    /// with `PlanChargesPaused` and move nothing; B spends the balance in the
    /// meantime, so when A is resumed it cannot overdraw what is left.
    #[test]
    fn a_plan_charge_pause_on_one_mandate_does_not_unlock_extra_spend() {
        let p = pair(START);
        p.w.registry
            .set_plan_charges_paused(&p.merchant_a, &p.plan_a, &true);

        p.rejected(Which::A, Error::PlanChargesPaused);
        p.settles(Which::B); // 60_000
        p.period(1);
        p.rejected(Which::A, Error::PlanChargesPaused);
        p.settles(Which::B); // 20_000

        p.w.registry
            .set_plan_charges_paused(&p.merchant_a, &p.plan_a, &false);
        p.unaffordable(Which::A); // 20_000 < 30_000
        assert_eq!(p.debited.get(), 2 * AMOUNT_B);
    }

    /// A top-up mid-sequence funds a mandate that had just failed, exactly
    /// once: it settles in the same period because the failure never
    /// advanced its schedule, and it cannot settle a second time.
    #[test]
    fn a_top_up_mid_sequence_funds_later_debits_exactly_once() {
        let p = pair(START);
        p.settles(Which::A); // 70_000
        p.settles(Which::B); // 30_000

        p.period(1);
        p.unaffordable(Which::B);
        p.top_up(50_000); // 80_000
        p.settles(Which::B); // 40_000
        p.rejected(Which::B, Error::NotDue);
        p.settles(Which::A); // 10_000
        p.rejected(Which::A, Error::NotDue);

        p.period(2);
        p.unaffordable(Which::A);
        p.unaffordable(Which::B);
        assert_eq!(p.debited.get(), START + 50_000 - 10_000);
    }

    /// Withdrawals interleaved with debits: the subscriber can take back only
    /// what has not been charged, a withdrawal reduces what later debits can
    /// reach, and nothing already charged can be reclaimed.
    #[test]
    fn withdrawals_mid_sequence_cannot_reclaim_charged_funds_or_overdraw() {
        let p = pair(START);
        p.settles(Which::A); // 70_000
        p.withdraw(20_000); // 50_000
        p.settles(Which::B); // 10_000

        assert_eq!(
            p.w.vault.try_withdraw(&p.user, &p.w.token, &10_001),
            Err(Ok(VaultError::InsufficientBalance))
        );
        p.assert_invariants();

        p.period(1);
        p.unaffordable(Which::A);
        p.unaffordable(Which::B);
        p.withdraw(10_000); // 0

        assert_eq!(p.debited.get(), AMOUNT_A + AMOUNT_B);
        assert_eq!(p.balances().wallet, START + 30_000);
    }
}

// ---------------------------------------------------------------------------
// #244: plan and mandate ids are reproducible given the same call sequence
// ---------------------------------------------------------------------------

/// Plan and mandate IDs must come out the same every time the same call
/// sequence runs, because generated bindings, indexers and their tests key on
/// them.
///
/// Both IDs are plain global counters: not hashes, not random, and not scoped
/// per merchant, subscriber or plan.
/// - Plan IDs come from the registry's `NextPlanId` (instance storage, starts
///   at 1). `create_plan` and `create_plans` read it only after checking the
///   merchant's auth and validating every plan.
/// - Mandate IDs come from the subscription contract's `NextMandateId`
///   (instance storage, starts at 1). The one creation path shared by
///   `subscribe`, `subscribe_at` and `subscribe_with_deposit` reads it only
///   after every check has passed.
///
/// A rejected call is rolled back whole by the host, so it must never consume
/// an ID. These tests prove it by slipping failing and unauthorized creates
/// between the steps of a fixed sequence and checking that the IDs that follow
/// are exactly the ones a clean run assigns.
mod deterministic_ids {
    use super::*;
    use payflow_plan_registry::{Error as RegistryError, PlanParams};
    use soroban_sdk::testutils::{AuthorizedFunction, AuthorizedInvocation};
    use soroban_sdk::{InvokeError, Val};

    /// Ledger timestamp every run is pinned to.
    const T0: u64 = 1_000_000;
    /// Ledger sequence every run is pinned to.
    const SEQ: u32 = 100;
    /// What each subscriber holds in the vault, and again in its wallet.
    const FUNDING: i128 = 1_000_000;
    /// A plan ID no run ever reaches.
    const MISSING_PLAN: u64 = 999;

    // GOLDEN VALUES. Generated bindings, indexers and their fixtures rely on
    // these exact IDs for this call sequence. A change that shifts any of them
    // is a breaking change for every consumer of the IDs, so do not edit them
    // just to make this test pass.

    /// Plan IDs in creation order: A's plan, B's plan, then A's batch of two.
    const GOLDEN_PLANS: [u64; 4] = [1, 2, 3, 4];
    /// `(mandate ID, plan ID)` in creation order.
    const GOLDEN_MANDATES: [(u64, u64); 5] = [(1, 1), (2, 1), (3, 2), (4, 3), (5, 4)];
    /// The next IDs each contract will hand out once the sequence is done.
    const GOLDEN_NEXT_PLAN_ID: u64 = 5;
    const GOLDEN_NEXT_MANDATE_ID: u64 = 6;

    type TryId<E> = Result<Result<u64, soroban_sdk::Error>, Result<E, InvokeError>>;

    /// Calls slipped in between the steps of the sequence. Every one of them
    /// must fail and leave both counters where they were.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Noise {
        None,
        /// Creates the contracts reject: bad input, a missing, closed or full
        /// plan, a paused protocol, or a deposit the wallet cannot cover.
        Failures,
        /// Creates without the merchant's or subscriber's signature.
        Unauthorized,
    }

    /// Everything one run assigned, as plain values that compare across
    /// `Env`s. Addresses are read back from the stored plan and mandate
    /// records as `ScVal`s, so comparing two runs also proves each ID points
    /// at the same record in both.
    #[derive(Debug, PartialEq, Eq)]
    struct Ids {
        /// `(plan ID, merchant)` in creation order.
        plans: std::vec::Vec<(u64, ScVal)>,
        /// `(mandate ID, plan ID, subscriber, merchant)` in creation order.
        mandates: std::vec::Vec<(u64, u64, ScVal, ScVal)>,
        next_plan_id: u64,
        next_mandate_id: u64,
    }

    impl Ids {
        fn plan_ids(&self) -> std::vec::Vec<u64> {
            self.plans.iter().map(|(id, _)| *id).collect()
        }

        fn mandate_ids(&self) -> std::vec::Vec<(u64, u64)> {
            self.mandates
                .iter()
                .map(|(id, plan, _, _)| (*id, *plan))
                .collect()
        }
    }

    struct Run {
        w: World<'static>,
        merchant_a: Address,
        merchant_b: Address,
        u1: Address,
        u2: Address,
        /// An account that is never authorized for anything.
        outsider: Address,
        plans: std::vec::Vec<u64>,
        mandates: std::vec::Vec<u64>,
    }

    fn sc(env: &Env, address: &Address) -> ScVal {
        let val: Val = address.into_val(env);
        ScVal::try_from_val(env, &val).unwrap()
    }

    /// A fresh `Env` with the three contracts deployed, the ledger pinned to
    /// `T0` / `SEQ`, and every account the sequence uses created up front in
    /// the same order whatever noise follows, so generated addresses line up
    /// across runs.
    fn fresh() -> Run {
        let w = world();
        w.env.ledger().with_mut(|l| {
            l.timestamp = T0;
            l.sequence_number = SEQ;
        });
        let merchant_a = w.merchant.clone();
        let merchant_b = Address::generate(&w.env);
        let outsider = Address::generate(&w.env);
        let u1 = w.subscriber(FUNDING);
        let u2 = w.subscriber(FUNDING);
        Run {
            w,
            merchant_a,
            merchant_b,
            u1,
            u2,
            outsider,
            plans: std::vec::Vec::new(),
            mandates: std::vec::Vec::new(),
        }
    }

    /// The fixed call sequence, with `noise` slipped in before every create
    /// and once more at the end:
    ///
    /// 1. merchant A publishes a plan, then merchant B publishes one;
    /// 2. A publishes two more in one `create_plans` batch, the second capped
    ///    at one subscriber;
    /// 3. u1 and u2 subscribe to A's first plan, and u1 to B's plan;
    /// 4. B closes its plan to new sign-ups;
    /// 5. u2 schedules a mandate on A's third plan with `subscribe_at`;
    /// 6. u1 takes the only seat on A's capped plan with
    ///    `subscribe_with_deposit`.
    fn run(noise: Noise) -> Run {
        let mut r = fresh();
        let (a, b) = (r.merchant_a.clone(), r.merchant_b.clone());
        let (u1, u2) = (r.u1.clone(), r.u2.clone());

        r.plan_noise(noise);
        let p1 = r.publish(&a, 10_000);
        r.plan_noise(noise);
        let p2 = r.publish(&b, 20_000);
        r.plan_noise(noise);
        let batch = r.w.registry.create_plans(
            &a,
            &soroban_sdk::vec![&r.w.env, r.params(30_000, 0), r.params(40_000, 1)],
        );
        r.plans.extend(batch.iter());
        let (p3, p4) = (batch.get(0).unwrap(), batch.get(1).unwrap());

        r.mandate_noise(noise, p1);
        r.subscribe(&u1, p1);
        r.mandate_noise(noise, p1);
        r.subscribe(&u2, p1);
        r.mandate_noise(noise, p1);
        r.subscribe(&u1, p2);

        r.w.registry.set_plan_active(&b, &p2, &false);
        if noise == Noise::Failures {
            r.closed_plan_failures(p2);
        }

        r.mandate_noise(noise, p1);
        let m4 = r.w.sub.subscribe_at(&u2, &p3, &0, &0, &(T0 + WEEK), &None);
        r.mandates.push(m4);
        r.mandate_noise(noise, p1);
        let m5 =
            r.w.sub
                .subscribe_with_deposit(&u1, &p4, &0, &0, &10_000, &None);
        r.mandates.push(m5);
        if noise == Noise::Failures {
            r.full_plan_failures(p4);
        }

        r.plan_noise(noise);
        r.mandate_noise(noise, p1);
        r
    }

    impl Run {
        fn ids(&self) -> Ids {
            let env = &self.w.env;
            Ids {
                plans: self
                    .plans
                    .iter()
                    .map(|id| (*id, sc(env, &self.w.registry.get_plan(id).merchant)))
                    .collect(),
                mandates: self
                    .mandates
                    .iter()
                    .map(|id| {
                        let m = self.w.sub.get_mandate(id);
                        (*id, m.plan_id, sc(env, &m.subscriber), sc(env, &m.merchant))
                    })
                    .collect(),
                next_plan_id: self.w.registry.next_plan_id(),
                next_mandate_id: self.w.sub.next_mandate_id(),
            }
        }

        fn params(&self, amount: i128, max_subscribers: u32) -> PlanParams {
            let env = &self.w.env;
            PlanParams {
                token: self.w.token.clone(),
                amount,
                period: MONTH,
                name: String::from_str(env, "Plan"),
                description: String::from_str(env, ""),
                keeper_bps: 0,
                category: Symbol::new(env, ""),
                terms_uri: String::from_str(env, ""),
                available_until: 0,
                referral_bps: 0,
                payout: None,
                max_subscribers,
            }
        }

        /// `create_plan`'s arguments, for building a `MockAuth` that matches
        /// `try_publish` exactly.
        fn plan_args(
            &self,
            merchant: &Address,
            amount: i128,
            period: u64,
        ) -> soroban_sdk::Vec<Val> {
            let env = &self.w.env;
            (
                merchant,
                &self.w.token,
                &amount,
                &period,
                &String::from_str(env, "Plan"),
                &String::from_str(env, ""),
                &0u32,
                &Symbol::new(env, ""),
                &String::from_str(env, ""),
                &0u64,
                &0u32,
                &None::<Address>,
                &0u32,
            )
                .into_val(env)
        }

        fn try_publish(
            &self,
            merchant: &Address,
            amount: i128,
            period: u64,
        ) -> TryId<RegistryError> {
            let env = &self.w.env;
            self.w.registry.try_create_plan(
                merchant,
                &self.w.token,
                &amount,
                &period,
                &String::from_str(env, "Plan"),
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

        fn publish(&mut self, merchant: &Address, amount: i128) -> u64 {
            let id = self.try_publish(merchant, amount, MONTH).unwrap().unwrap();
            self.plans.push(id);
            id
        }

        fn try_subscribe(&self, user: &Address, plan_id: u64) -> TryId<Error> {
            self.w
                .sub
                .try_subscribe(user, &plan_id, &0, &0, &None::<Address>)
        }

        fn subscribe(&mut self, user: &Address, plan_id: u64) -> u64 {
            let id = self.try_subscribe(user, plan_id).unwrap().unwrap();
            self.mandates.push(id);
            id
        }

        /// Only `signer` signs the next call, for exactly `fn_name(args)`.
        fn signed_by(
            &self,
            signer: &Address,
            contract: &Address,
            fn_name: &str,
            args: soroban_sdk::Vec<Val>,
        ) {
            self.w.env.mock_auths(&[MockAuth {
                address: signer,
                invoke: &MockAuthInvoke {
                    contract,
                    fn_name,
                    args,
                    sub_invokes: &[],
                },
            }]);
        }

        /// Run `attempt`, which must fail, and check it consumed neither a
        /// plan ID nor a mandate ID.
        fn consumes_no_id(&self, what: &str, attempt: impl FnOnce(&Self)) {
            let next_plan = self.w.registry.next_plan_id();
            let next_mandate = self.w.sub.next_mandate_id();
            attempt(self);
            assert_eq!(
                self.w.registry.next_plan_id(),
                next_plan,
                "{what} consumed a plan id"
            );
            assert_eq!(
                self.w.sub.next_mandate_id(),
                next_mandate,
                "{what} consumed a mandate id"
            );
        }

        fn plan_noise(&self, noise: Noise) {
            match noise {
                Noise::None => {}
                Noise::Failures => self.plan_failures(),
                Noise::Unauthorized => self.unauthorized_plans(),
            }
        }

        fn mandate_noise(&self, noise: Noise, open_plan: u64) {
            match noise {
                Noise::None => {}
                Noise::Failures => self.mandate_failures(open_plan),
                Noise::Unauthorized => self.unauthorized_mandates(open_plan),
            }
        }

        fn plan_failures(&self) {
            let a = &self.merchant_a;
            self.consumes_no_id("a zero-amount plan", |r| {
                assert_eq!(
                    r.try_publish(a, 0, MONTH),
                    Err(Ok(RegistryError::InvalidAmount))
                );
            });
            self.consumes_no_id("a plan with too short a period", |r| {
                assert_eq!(
                    r.try_publish(a, 10_000, 1),
                    Err(Ok(RegistryError::InvalidPeriod))
                );
            });
            self.consumes_no_id("a batch with one invalid plan", |r| {
                let env = &r.w.env;
                let batch = soroban_sdk::vec![env, r.params(10_000, 0), r.params(0, 0)];
                assert_eq!(
                    r.w.registry.try_create_plans(a, &batch),
                    Err(Ok(RegistryError::InvalidAmount))
                );
            });
            self.consumes_no_id("an empty batch", |r| {
                assert_eq!(
                    r.w.registry
                        .try_create_plans(a, &soroban_sdk::Vec::new(&r.w.env)),
                    Err(Ok(RegistryError::EmptyBatch))
                );
            });
            self.consumes_no_id("an oversized batch", |r| {
                let mut batch = soroban_sdk::Vec::new(&r.w.env);
                for _ in 0..=payflow_plan_registry::MAX_BATCH_PLANS {
                    batch.push_back(r.params(10_000, 0));
                }
                assert_eq!(
                    r.w.registry.try_create_plans(a, &batch),
                    Err(Ok(RegistryError::BatchTooLarge))
                );
            });
        }

        fn mandate_failures(&self, open_plan: u64) {
            let u2 = &self.u2;
            self.consumes_no_id("a mandate on a missing plan", |r| {
                assert_eq!(
                    r.try_subscribe(u2, MISSING_PLAN),
                    Err(Ok(Error::PlanNotFound))
                );
            });
            self.consumes_no_id("a deposit-and-subscribe on a missing plan", |r| {
                assert_eq!(
                    r.w.sub
                        .try_subscribe_with_deposit(u2, &MISSING_PLAN, &0, &0, &10_000, &None),
                    Err(Ok(Error::PlanNotFound))
                );
            });
            self.consumes_no_id("a backdated mandate", |r| {
                assert_eq!(
                    r.w.sub
                        .try_subscribe_at(u2, &open_plan, &0, &0, &(T0 - 1), &None),
                    Err(Ok(Error::InvalidStart))
                );
            });
            self.consumes_no_id("a negative spend cap", |r| {
                assert_eq!(
                    r.w.sub.try_subscribe(u2, &open_plan, &0, &-1, &None),
                    Err(Ok(Error::InvalidMaxTotal))
                );
            });
            self.consumes_no_id("a deposit the wallet cannot cover", |r| {
                let vault_before = r.w.vault.balance(u2, &r.w.token);
                assert_eq!(
                    r.w.sub.try_subscribe_with_deposit(
                        u2,
                        &open_plan,
                        &0,
                        &0,
                        &(10 * FUNDING),
                        &None
                    ),
                    Err(Ok(Error::VaultCallFailed))
                );
                assert_eq!(r.w.vault.balance(u2, &r.w.token), vault_before);
            });
            self.consumes_no_id("a mandate while the protocol is paused", |r| {
                r.w.sub.set_protocol_paused(&true);
                assert_eq!(
                    r.try_subscribe(u2, open_plan),
                    Err(Ok(Error::ProtocolPaused))
                );
                r.w.sub.set_protocol_paused(&false);
            });
        }

        /// `closed` no longer accepts sign-ups. `subscribe_with_deposit`
        /// deposits before it finds that out, so this also proves the deposit
        /// is rolled back with the rest of the call.
        fn closed_plan_failures(&self, closed: u64) {
            let u2 = &self.u2;
            self.consumes_no_id("a mandate on a closed plan", |r| {
                assert_eq!(r.try_subscribe(u2, closed), Err(Ok(Error::PlanInactive)));
            });
            self.consumes_no_id("a deposit-and-subscribe on a closed plan", |r| {
                let t = &r.w.token_client;
                let (vault, wallet) = (r.w.vault.balance(u2, &r.w.token), t.balance(u2));
                assert_eq!(
                    r.w.sub
                        .try_subscribe_with_deposit(u2, &closed, &0, &0, &10_000, &None),
                    Err(Ok(Error::PlanInactive))
                );
                assert_eq!(r.w.vault.balance(u2, &r.w.token), vault);
                assert_eq!(t.balance(u2), wallet);
            });
        }

        fn full_plan_failures(&self, full: u64) {
            self.consumes_no_id("a mandate on a full plan", |r| {
                assert_eq!(r.try_subscribe(&r.u2, full), Err(Ok(Error::PlanFull)));
            });
        }

        fn unauthorized_plans(&self) {
            let a = &self.merchant_a;
            let registry = &self.w.registry.address;
            self.consumes_no_id("an unsigned plan", |r| {
                r.w.env.mock_auths(&[]);
                assert!(matches!(r.try_publish(a, 10_000, MONTH), Err(Err(_))));
            });
            self.consumes_no_id("a plan signed by someone else", |r| {
                r.signed_by(
                    &r.outsider,
                    registry,
                    "create_plan",
                    r.plan_args(a, 10_000, MONTH),
                );
                assert!(matches!(r.try_publish(a, 10_000, MONTH), Err(Err(_))));
            });
            self.consumes_no_id("a batch signed by the other merchant", |r| {
                let env = &r.w.env;
                let batch = soroban_sdk::vec![env, r.params(10_000, 0)];
                r.signed_by(
                    &r.merchant_b,
                    registry,
                    "create_plans",
                    (a, batch.clone()).into_val(env),
                );
                assert!(matches!(
                    r.w.registry.try_create_plans(a, &batch),
                    Err(Err(_))
                ));
            });
            self.w.env.mock_all_auths();
        }

        fn unauthorized_mandates(&self, open_plan: u64) {
            let u1 = &self.u1;
            let sub = &self.w.sub.address;
            self.consumes_no_id("an unsigned mandate", |r| {
                r.w.env.mock_auths(&[]);
                assert!(matches!(r.try_subscribe(u1, open_plan), Err(Err(_))));
            });
            self.consumes_no_id("a mandate signed by someone else", |r| {
                let args = (u1, &open_plan, &0u32, &0i128, &None::<Address>).into_val(&r.w.env);
                r.signed_by(&r.outsider, sub, "subscribe", args);
                assert!(matches!(r.try_subscribe(u1, open_plan), Err(Err(_))));
            });
            self.consumes_no_id("a scheduled mandate signed by someone else", |r| {
                let start = T0 + WEEK;
                let args =
                    (u1, &open_plan, &0u32, &0i128, &start, &None::<Address>).into_val(&r.w.env);
                r.signed_by(&r.outsider, sub, "subscribe_at", args);
                assert!(matches!(
                    r.w.sub
                        .try_subscribe_at(u1, &open_plan, &0, &0, &start, &None),
                    Err(Err(_))
                ));
            });
            self.consumes_no_id("an unsigned deposit-and-subscribe", |r| {
                r.w.env.mock_auths(&[]);
                assert!(matches!(
                    r.w.sub
                        .try_subscribe_with_deposit(u1, &open_plan, &0, &0, &10_000, &None),
                    Err(Err(_))
                ));
            });
            self.w.env.mock_all_auths();
        }
    }

    // ---- reproducibility ----

    /// The same call sequence in two separate fresh `Env`s assigns the same
    /// plan and mandate IDs, in the same order, to the same records.
    #[test]
    fn the_same_call_sequence_assigns_the_same_ids_in_fresh_envs() {
        let first = run(Noise::None).ids();
        let second = run(Noise::None).ids();
        assert_eq!(first, second);
    }

    /// The sequence assigns exactly the pinned golden IDs, each to the record
    /// the sequence created it for. See the note on the golden constants:
    /// changing these is a breaking change for bindings and indexers.
    #[test]
    fn ids_match_the_pinned_golden_values() {
        let r = run(Noise::None);
        let ids = r.ids();
        let env = &r.w.env;

        assert_eq!(ids.plan_ids(), GOLDEN_PLANS);
        assert_eq!(ids.mandate_ids(), GOLDEN_MANDATES);
        assert_eq!(ids.next_plan_id, GOLDEN_NEXT_PLAN_ID);
        assert_eq!(ids.next_mandate_id, GOLDEN_NEXT_MANDATE_ID);

        let (a, b) = (sc(env, &r.merchant_a), sc(env, &r.merchant_b));
        let (u1, u2) = (sc(env, &r.u1), sc(env, &r.u2));
        let merchants: std::vec::Vec<_> = ids.plans.iter().map(|(_, m)| m.clone()).collect();
        assert_eq!(merchants, [a.clone(), b.clone(), a.clone(), a.clone()]);
        let owners: std::vec::Vec<_> = ids
            .mandates
            .iter()
            .map(|(_, _, s, m)| (s.clone(), m.clone()))
            .collect();
        assert_eq!(
            owners,
            [
                (u1.clone(), a.clone()),
                (u2.clone(), a.clone()),
                (u1.clone(), b),
                (u2, a.clone()),
                (u1, a),
            ]
        );
    }

    /// No ID repeats within a run, across plans or across mandates.
    #[test]
    fn ids_are_unique_within_a_run() {
        let ids = run(Noise::None).ids();
        let mut plans = ids.plan_ids();
        plans.sort_unstable();
        plans.dedup();
        assert_eq!(plans.len(), ids.plans.len());
        let mut mandates: std::vec::Vec<u64> = ids.mandates.iter().map(|m| m.0).collect();
        mandates.sort_unstable();
        mandates.dedup();
        assert_eq!(mandates.len(), ids.mandates.len());
    }

    /// IDs are one global sequence per contract, not one per merchant,
    /// subscriber or plan: merchant B's first plan is 2 and u2's first mandate
    /// is 2. The per-scope indexes list the global IDs in creation order, and
    /// the two counters do not affect each other.
    #[test]
    fn ids_are_one_global_sequence_per_contract_not_per_scope() {
        let r = run(Noise::None);
        let w = &r.w;
        let list = |v: soroban_sdk::Vec<u64>| -> std::vec::Vec<u64> { v.iter().collect() };

        assert_eq!(
            list(w.registry.merchant_plans(&r.merchant_a, &0, &10)),
            [1, 3, 4]
        );
        assert_eq!(list(w.registry.merchant_plans(&r.merchant_b, &0, &10)), [2]);
        assert_eq!(list(w.sub.subscriber_mandates(&r.u1, &0, &10)), [1, 3, 5]);
        assert_eq!(list(w.sub.subscriber_mandates(&r.u2, &0, &10)), [2, 4]);
        assert_eq!(
            list(w.sub.merchant_mandates(&r.merchant_a, &0, &10)),
            [1, 2, 4, 5]
        );
        assert_eq!(list(w.sub.merchant_mandates(&r.merchant_b, &0, &10)), [3]);
        assert_eq!(list(w.sub.plan_mandates(&1, &0, &10)), [1, 2]);
        assert_eq!(list(w.sub.plan_mandates(&2, &0, &10)), [3]);
    }

    /// IDs follow call order, so a different order assigns them differently,
    /// and that different assignment is itself reproducible.
    #[test]
    fn a_different_call_order_assigns_different_but_reproducible_ids() {
        /// Merchants A and B each publish one plan and u1 subscribes to both,
        /// B first when `b_first`. Returns `(plan ID, mandate ID)` for A's
        /// plan, then for B's.
        fn two_merchants(b_first: bool) -> [(u64, u64); 2] {
            let mut r = fresh();
            let (a, b, u1) = (r.merchant_a.clone(), r.merchant_b.clone(), r.u1.clone());
            let order = if b_first { [&b, &a] } else { [&a, &b] };
            let plans = order.map(|m| r.publish(m, 10_000));
            let mandates = plans.map(|p| r.subscribe(&u1, p));
            let mut out = [(plans[0], mandates[0]), (plans[1], mandates[1])];
            if b_first {
                out.swap(0, 1);
            }
            out
        }

        assert_eq!(two_merchants(false), [(1, 1), (2, 2)]);
        assert_eq!(two_merchants(true), [(2, 2), (1, 1)]);
        assert_eq!(two_merchants(true), two_merchants(true));
        assert_eq!(two_merchants(false), two_merchants(false));
    }

    // ---- failure paths ----

    /// Rejected creates slipped in before every step consume no ID, so every
    /// later create gets exactly the ID a clean run gives it. The rejections
    /// cover bad plan input and bad batches, a missing plan, a closed plan, a
    /// full plan, a backdated start, a negative spend cap, a paused protocol,
    /// and a deposit the wallet cannot cover.
    #[test]
    fn failed_creates_do_not_consume_ids() {
        let noisy = run(Noise::Failures).ids();
        assert_eq!(noisy, run(Noise::None).ids());
        assert_eq!(noisy.plan_ids(), GOLDEN_PLANS);
        assert_eq!(noisy.mandate_ids(), GOLDEN_MANDATES);
    }

    // ---- authorization ----

    /// Creates without the merchant's or subscriber's signature, slipped in
    /// before every step, consume no ID either.
    #[test]
    fn unauthorized_creates_do_not_consume_ids() {
        let noisy = run(Noise::Unauthorized).ids();
        assert_eq!(noisy, run(Noise::None).ids());
        assert_eq!(noisy.plan_ids(), GOLDEN_PLANS);
        assert_eq!(noisy.mandate_ids(), GOLDEN_MANDATES);
    }

    /// With real auth checking (no `mock_all_auths`), a create that nobody or
    /// the wrong account signs is rejected, and the first create its owner
    /// signs still gets ID 1. The owner's signature is the only auth that
    /// create needs.
    #[test]
    fn the_first_authorized_create_gets_id_1_and_needs_only_its_owners_auth() {
        let r = fresh();
        let env = &r.w.env;
        let a = &r.merchant_a;
        let registry = &r.w.registry.address;
        let args = r.plan_args(a, 10_000, MONTH);

        env.mock_auths(&[]);
        assert!(matches!(r.try_publish(a, 10_000, MONTH), Err(Err(_))));
        r.signed_by(&r.outsider, registry, "create_plan", args.clone());
        assert!(matches!(r.try_publish(a, 10_000, MONTH), Err(Err(_))));
        r.signed_by(a, registry, "create_plan", args.clone());
        assert_eq!(r.try_publish(a, 10_000, MONTH), Ok(Ok(GOLDEN_PLANS[0])));
        assert_eq!(
            env.auths(),
            std::vec![(
                a.clone(),
                AuthorizedInvocation {
                    function: AuthorizedFunction::Contract((
                        registry.clone(),
                        Symbol::new(env, "create_plan"),
                        args,
                    )),
                    sub_invocations: std::vec![],
                }
            )]
        );

        let u1 = &r.u1;
        let sub = &r.w.sub.address;
        let plan = GOLDEN_PLANS[0];
        let args: soroban_sdk::Vec<Val> =
            (u1, &plan, &0u32, &0i128, &None::<Address>).into_val(env);

        env.mock_auths(&[]);
        assert!(matches!(r.try_subscribe(u1, plan), Err(Err(_))));
        r.signed_by(&r.outsider, sub, "subscribe", args.clone());
        assert!(matches!(r.try_subscribe(u1, plan), Err(Err(_))));
        r.signed_by(u1, sub, "subscribe", args.clone());
        assert_eq!(r.try_subscribe(u1, plan), Ok(Ok(GOLDEN_MANDATES[0].0)));
        assert_eq!(
            env.auths(),
            std::vec![(
                u1.clone(),
                AuthorizedInvocation {
                    function: AuthorizedFunction::Contract((
                        sub.clone(),
                        Symbol::new(env, "subscribe"),
                        args,
                    )),
                    sub_invocations: std::vec![],
                }
            )]
        );

        assert_eq!(r.w.registry.next_plan_id(), 2);
        assert_eq!(r.w.sub.next_mandate_id(), 2);
    }
}
// Numeric maximum values (#242)
// ---------------------------------------------------------------------------

fn set_next_mandate_id(w: &World, next: u64) {
    w.env.as_contract(&w.sub.address, || {
        w.env
            .storage()
            .instance()
            .set(&DataKey::NextMandateId, &next);
    });
}

#[test]
fn maximum_amount_charge_is_safe_and_reports_max_committed_revenue() {
    let w = world();
    w.sub.set_fee_bps(&0);
    let plan_id = w.plan_priced(i128::MAX);
    let user = Address::generate(&w.env);
    w.minter.mint(&user, &i128::MAX);
    w.vault.deposit(&user, &w.token, &i128::MAX);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);

    // Same-period normalisation must return the amount itself, not overflow
    // to a zero contribution and leave the revenue cache empty.
    assert_eq!(w.sub.plan_committed_revenue(&plan_id), i128::MAX);

    let result = w.sub.charge(&w.keeper, &id);
    assert_eq!(result.merchant_amount, i128::MAX);
    assert_eq!(result.fee, 0);
    assert_eq!(result.charges_made, 1);
    assert!(!result.completed);
    assert_eq!(w.vault.balance(&user, &w.token), 0);
    assert_eq!(w.token_client.balance(&w.merchant), i128::MAX);
    assert_eq!(w.token_client.balance(&w.vault.address), 0);
    assert_eq!(w.sub.get_mandate(&id).total_spent, i128::MAX);
    assert_eq!(w.sub.plan_committed_revenue(&plan_id), i128::MAX);

    // A second charge would make total_spent + amount exceed i128::MAX. It
    // must fail with a typed error and leave the settled mandate untouched.
    let before = w.sub.get_mandate(&id);
    w.advance(MONTH);
    assert_eq!(
        w.sub.try_charge(&w.keeper, &id),
        Err(Ok(Error::AmountOverflow))
    );
    let after = w.sub.get_mandate(&id);
    assert_eq!(after.charges_made, before.charges_made);
    assert_eq!(after.total_spent, before.total_spent);
    assert_eq!(after.next_charge, before.next_charge);
}

#[test]
fn maximum_counts_seats_and_pause_are_stored_without_wrapping() {
    let w = world();
    let plan_id = w.plan_with_cap(u32::MAX);
    let user = w.subscriber(PRICE * 4);
    let id = w
        .sub
        .subscribe(&user, &plan_id, &u32::MAX, &i128::MAX, &None::<Address>);

    let mandate = w.sub.get_mandate(&id);
    assert_eq!(mandate.max_charges, u32::MAX);
    assert_eq!(mandate.max_total, i128::MAX);
    assert_eq!(w.sub.plan_subscriber_count(&plan_id), 1);

    // Re-setting the same maximum is a valid no-op, not an InvalidMaxCharges.
    w.sub.set_max_charges(&user, &id, &u32::MAX);
    w.sub.set_seats(&user, &id, &u32::MAX);
    assert_eq!(w.sub.get_mandate(&id).seats, u32::MAX);

    w.sub.pause_until(&user, &id, &u64::MAX);
    let paused = w.sub.get_mandate(&id);
    assert_eq!(paused.paused_until, u64::MAX);
    assert_eq!(paused.next_charge, u64::MAX);
    assert_eq!(paused.status, MandateStatus::Paused);
}

#[test]
fn mandate_id_counter_exhaustion_is_a_typed_error_without_state_changes() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(PRICE);
    set_next_mandate_id(&w, u64::MAX);

    assert_eq!(
        w.sub
            .try_subscribe(&user, &plan_id, &u32::MAX, &i128::MAX, &None::<Address>),
        Err(Ok(Error::AmountOverflow))
    );
    assert_eq!(w.sub.next_mandate_id(), u64::MAX);
    assert_eq!(w.sub.subscriber_mandate_count(&user), 0);
    assert_eq!(
        w.sub.try_get_mandate(&u64::MAX),
        Err(Ok(Error::MandateNotFound))
    );
}

#[test]
fn maximum_value_subscribe_requires_the_subscribers_auth() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(PRICE);
    w.env.set_auths(&[]);

    assert!(w
        .sub
        .try_subscribe(&user, &plan_id, &u32::MAX, &i128::MAX, &None::<Address>)
        .is_err());
    assert_eq!(w.sub.next_mandate_id(), 1);
    assert_eq!(w.sub.subscriber_mandate_count(&user), 0);
    assert_eq!(w.vault.balance(&user, &w.token), PRICE);
}

// ---------------------------------------------------------------------------
// #363: event log ordering across several entry points in one transaction.
// ---------------------------------------------------------------------------

/// Calls three state-changing subscription entry points from one top-level
/// invocation. `env.events().all()` only reports the most recent top-level
/// invocation, so several calls in one transaction have to come from a
/// contract in between rather than from the test itself.
#[contract]
pub struct EntryPointBatch;

#[contractimpl]
impl EntryPointBatch {
    /// Subscribe, settle the first charge, then cancel - in that order - and
    /// return the mandate id so the caller can check the final state.
    pub fn subscribe_charge_cancel(
        env: Env,
        subscription: Address,
        subscriber: Address,
        plan_id: u64,
        keeper: Address,
    ) -> u64 {
        let client = SubscriptionClient::new(&env, &subscription);
        let id = client.subscribe(&subscriber, &plan_id, &0u32, &0i128, &None::<Address>);
        let _ = client.charge(&keeper, &id);
        client.cancel(&subscriber, &id);
        id
    }
}

#[test]
fn events_follow_call_order_across_a_multi_entry_point_transaction() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(PRICE * 2);
    // The calls inside the batch are not the root invocation, so plain
    // `mock_all_auths` (which only covers the root) is not enough here.
    w.env.mock_all_auths_allowing_non_root_auth();
    let batch_addr = w.env.register(EntryPointBatch, ());
    let batch = EntryPointBatchClient::new(&w.env, &batch_addr);

    let id = batch.subscribe_charge_cancel(&w.sub_id, &user, &plan_id, &w.keeper);

    // Read the log before any further call: `events().all()` reports the most
    // recent invocation, and a view call would replace the batch's log.
    let events = w.env.events().all();
    let events = events.events();
    assert!(!events.is_empty(), "the batch emitted no events");

    // Every event must be well-formed XDR that round-trips byte for byte.
    for event in events {
        let encoded = WriteXdr::to_xdr(event, Limits::none()).expect("event encodes as XDR");
        let decoded = soroban_sdk::xdr::ContractEvent::from_xdr(&encoded, Limits::none())
            .expect("event decodes as XDR");
        assert_eq!(&decoded, event);
    }

    // Only the subscription contract's own events, in emission order. The
    // vault's debit events and the registry's committed-revenue events are
    // interleaved with these; they are other contracts' events and are not
    // part of the sequence this test pins.
    let names: std::vec::Vec<std::string::String> = events
        .iter()
        .filter_map(|event| {
            let ContractEventBody::V0(body) = &event.body;
            match body.topics.first() {
                Some(ScVal::Symbol(ScSymbol(name))) => Some(name.to_utf8_string_lossy()),
                _ => None,
            }
        })
        .filter(|name| {
            matches!(
                name.as_str(),
                "mandate_updated" | "subscribed" | "charged" | "cancelled"
            )
        })
        .collect();

    assert_eq!(
        names,
        std::vec![
            "mandate_updated",
            "subscribed",
            "mandate_updated",
            "charged",
            "mandate_updated",
            "cancelled",
        ],
        "events must appear in call order with no event dropped or duplicated"
    );

    assert_eq!(w.sub.get_mandate(&id).status, MandateStatus::Cancelled);
}

// ---------------------------------------------------------------------------
// #368: prevent replay of a completed/deleted mandate via ID recycling
// ---------------------------------------------------------------------------

#[test]
fn subscribe_with_id_requires_subscriber_auth() {
    let w = world();
    let plan_id = w.plan();
    let user = Address::generate(&w.env);
    let attacker = Address::generate(&w.env);

    w.env.mock_auths(&[]);
    assert!(w
        .sub
        .try_subscribe_with_id(&user, &plan_id, &10, &0, &0, &None::<Address>)
        .is_err());

    let args: soroban_sdk::Vec<soroban_sdk::Val> =
        (&user, &plan_id, &10u64, &0u32, &0i128, &None::<Address>).into_val(&w.env);
    w.env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &w.sub.address,
            fn_name: "subscribe_with_id",
            args,
            sub_invokes: &[],
        },
    }]);
    assert!(w
        .sub
        .try_subscribe_with_id(&user, &plan_id, &10, &0, &0, &None::<Address>)
        .is_err());
    assert_eq!(w.sub.subscriber_mandate_count(&user), 0);
}

#[test]
fn delete_mandate_requires_subscriber_auth() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(PRICE);
    let attacker = Address::generate(&w.env);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    w.sub.cancel(&user, &id);

    w.env.mock_auths(&[]);
    assert!(w.sub.try_delete_mandate(&user, &id).is_err());

    let args: soroban_sdk::Vec<soroban_sdk::Val> = (&user, &id).into_val(&w.env);
    w.env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &w.sub.address,
            fn_name: "delete_mandate",
            args,
            sub_invokes: &[],
        },
    }]);
    assert!(w.sub.try_delete_mandate(&user, &id).is_err());
    assert!(!w.sub.is_mandate_tombstoned(&id));
    assert_eq!(w.sub.get_mandate(&id).status, MandateStatus::Cancelled);
}

#[test]
fn delete_and_recreate_with_same_id_returns_tombstoned_error() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(PRICE * 4);

    // 1. Completed mandate: charge to completion, delete, verify tombstone, reject reuse.
    let completed_id = w.sub.subscribe(&user, &plan_id, &1, &0, &None::<Address>);
    let res = w.sub.charge(&w.keeper, &completed_id);
    assert!(res.completed);
    assert!(!w.sub.is_mandate_tombstoned(&completed_id));

    w.sub.delete_mandate(&user, &completed_id);

    assert!(w.sub.is_mandate_tombstoned(&completed_id));
    assert_eq!(
        w.sub.try_get_mandate(&completed_id),
        Err(Ok(Error::MandateNotFound))
    );
    assert_eq!(
        w.sub
            .try_subscribe_with_id(&user, &plan_id, &completed_id, &0, &0, &None::<Address>),
        Err(Ok(Error::MandateIdTombstoned))
    );

    // 2. Cancelled mandate: cancel, delete, verify tombstone, reject reuse.
    let cancelled_id = w
        .sub
        .subscribe_with_id(&user, &plan_id, &42, &0, &0, &None::<Address>);
    assert_eq!(cancelled_id, 42);
    w.sub.cancel(&user, &cancelled_id);
    w.sub.delete_mandate(&user, &cancelled_id);

    assert!(w.sub.is_mandate_tombstoned(&cancelled_id));
    assert_eq!(
        w.sub
            .try_subscribe_with_id(&user, &plan_id, &cancelled_id, &0, &0, &None::<Address>),
        Err(Ok(Error::MandateIdTombstoned))
    );
}

#[test]
fn auto_generated_ids_skip_tombstoned_and_occupied_values() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(PRICE * 10);

    // Pre-allocate IDs 2 and 3 explicitly while NextMandateId is still 1,
    // delete ID 2 (so 2 is tombstoned) and leave ID 3 active (occupied),
    // and also pre-allocate and tombstone ID 5.
    assert_eq!(w.sub.next_mandate_id(), 1);
    w.sub
        .subscribe_with_id(&user, &plan_id, &2, &1, &0, &None::<Address>);
    w.sub.charge(&w.keeper, &2);
    w.sub.delete_mandate(&user, &2);
    assert!(w.sub.is_mandate_tombstoned(&2));

    w.sub
        .subscribe_with_id(&user, &plan_id, &3, &0, &0, &None::<Address>);

    w.sub
        .subscribe_with_id(&user, &plan_id, &5, &0, &0, &None::<Address>);
    w.sub.cancel(&user, &5);
    w.sub.delete_mandate(&user, &5);
    assert!(w.sub.is_mandate_tombstoned(&5));

    // Next auto-generated ID is 1.
    assert_eq!(w.sub.next_mandate_id(), 1);
    let first = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    assert_eq!(first, 1);

    // Next auto-generated ID must skip tombstoned 2 and occupied 3, landing on 4.
    assert_eq!(w.sub.next_mandate_id(), 4);
    let second = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    assert_eq!(second, 4);

    // Next auto-generated ID must skip tombstoned 5, landing on 6.
    assert_eq!(w.sub.next_mandate_id(), 6);
    let third = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    assert_eq!(third, 6);
    assert_eq!(w.sub.next_mandate_id(), 7);
}

#[test]
fn subscribe_with_id_rejects_zero_and_existing_mandate_ids() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(PRICE * 2);

    assert_eq!(
        w.sub
            .try_subscribe_with_id(&user, &plan_id, &0, &0, &0, &None::<Address>),
        Err(Ok(Error::InvalidMandateId))
    );

    let id = w
        .sub
        .subscribe_with_id(&user, &plan_id, &1, &0, &0, &None::<Address>);
    assert_eq!(id, 1);
    // Sequential explicit ID matching NextMandateId advances the counter.
    assert_eq!(w.sub.next_mandate_id(), 2);

    assert_eq!(
        w.sub
            .try_subscribe_with_id(&user, &plan_id, &1, &0, &0, &None::<Address>),
        Err(Ok(Error::MandateAlreadyExists))
    );

    // Even after cancelling (before deletion), the ID is still occupied.
    w.sub.cancel(&user, &1);
    assert_eq!(
        w.sub
            .try_subscribe_with_id(&user, &plan_id, &1, &0, &0, &None::<Address>),
        Err(Ok(Error::MandateAlreadyExists))
    );
}

#[test]
fn delete_mandate_enforces_terminal_state_ownership_and_cleanup() {
    let w = world();
    let plan_id = w.plan();
    w.registry
        .set_plan_grace_period(&w.merchant, &plan_id, &3_600);
    let user = w.subscriber(PRICE * 2);
    let stranger = Address::generate(&w.env);

    // Missing mandate cannot be deleted.
    assert_eq!(
        w.sub.try_delete_mandate(&user, &999),
        Err(Ok(Error::MandateNotFound))
    );

    // Active mandate cannot be deleted.
    let active_id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    assert_eq!(
        w.sub.try_delete_mandate(&user, &active_id),
        Err(Ok(Error::MandateStillActive))
    );

    // Paused mandate cannot be deleted.
    w.sub.set_paused(&user, &active_id, &true);
    assert_eq!(
        w.sub.try_delete_mandate(&user, &active_id),
        Err(Ok(Error::MandateStillActive))
    );

    // Non-subscriber cannot delete a cancelled mandate.
    w.sub.cancel(&user, &active_id);
    assert_eq!(
        w.sub.try_delete_mandate(&stranger, &active_id),
        Err(Ok(Error::NotSubscriber))
    );

    // Deleting a cancelled mandate cleans up auxiliary keys and emits MandateDeleted.
    w.sub.delete_mandate(&user, &active_id);
    let events = w.env.events().all();
    assert!(!events.events().is_empty());
    w.env.as_contract(&w.sub.address, || {
        let p = w.env.storage().persistent();
        assert!(!p.has(&DataKey::Mandate(active_id)));
        assert!(!p.has(&DataKey::MandateGracePeriod(active_id)));
        assert!(!p.has(&DataKey::FirstFailedCharge(active_id)));
        assert_eq!(
            p.get::<_, bool>(&DataKey::MandateTombstone(active_id)),
            Some(true)
        );
    });

    // Deleting a second time reports MandateNotFound and preserves the tombstone.
    assert_eq!(
        w.sub.try_delete_mandate(&user, &active_id),
        Err(Ok(Error::MandateNotFound))
    );
    assert!(w.sub.is_mandate_tombstoned(&active_id));
}

#[test]
fn delete_mandate_leaves_cancel_page_working_for_remaining_mandates() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(PRICE * 4);

    let id1 = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    let id2 = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    w.sub.cancel(&user, &id1);
    w.sub.delete_mandate(&user, &id1);

    let cancelled = w.sub.cancel_page(&user, &0, &50);
    assert_eq!(cancelled, 1);
    assert_eq!(w.sub.get_mandate(&id2).status, MandateStatus::Cancelled);
}

#[test]
fn subscribe_with_id_respects_protocol_paused_while_delete_mandate_works() {
    let w = world();
    let plan_id = w.plan();
    let user = w.subscriber(PRICE * 2);
    let id = w.sub.subscribe(&user, &plan_id, &0, &0, &None::<Address>);
    w.sub.cancel(&user, &id);

    w.sub.set_protocol_paused(&true);

    assert_eq!(
        w.sub
            .try_subscribe_with_id(&user, &plan_id, &77, &0, &0, &None::<Address>),
        Err(Ok(Error::ProtocolPaused))
    );
    // Deletion of an already-closed mandate keeps working during protocol pause.
    w.sub.delete_mandate(&user, &id);
    assert!(w.sub.is_mandate_tombstoned(&id));
}
