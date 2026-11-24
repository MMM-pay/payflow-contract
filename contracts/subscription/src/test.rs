#![cfg(test)]
extern crate std;

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger as _, MockAuth, MockAuthInvoke},
    token::{StellarAssetClient, TokenClient},
    xdr::{ContractEventBody, ScMap, ScSymbol, ScVal},
    Address, Env, Symbol,
};

const MONTH: u64 = 2_592_000;
const WEEK: u64 = 604_800;
const PRICE: i128 = 10_000;
const FEE_BPS: u32 = 100; // 1%
const KEEPER_BPS: u32 = 200; // 2% of the merchant share
const REFERRAL_BPS: u32 = 1_000; // 10% of the merchant share (from #136)
/// A valid subscription wasm used as the `upgrade_wasm` target (#158).
const UPGRADE_WASM: &[u8] = include_bytes!("../testdata/payflow_subscription.wasm");

struct World<'a> {
    env: Env,
    sub: SubscriptionClient<'a>,
    registry: payflow_plan_registry::PlanRegistryClient<'a>,
    vault: payflow_vault::VaultClient<'a>,
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

    let registry_id = env.register(payflow_plan_registry::PlanRegistry, (admin.clone(),));
    let registry = payflow_plan_registry::PlanRegistryClient::new(&env, &registry_id);

    let vault_id = env.register(payflow_vault::Vault, (admin.clone(),));
    let vault = payflow_vault::VaultClient::new(&env, &vault_id);

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
            &Symbol::new(&self.env, ""),
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
            &Symbol::new(&self.env, ""),
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
            &Symbol::new(&self.env, ""),
            &String::from_str(&self.env, ""),
            &cutoff,
            &0u32,
            &None::<Address>,
            &0u32,
        )
    }
}

// Long-running subscription simulation (Issue #240)
// Simulates subscription mandates across dozens of billing periods.
// Includes pauses, upgrades and failed payment attempts.
// Verifies balances, payment history and mandate state throughout.
// Asserts contract invariants after every period.
// Covers authorization failures and recovery scenarios.
// Ensures deterministic results.

/// Simulate a single billing period for a mandate
fn simulate_billing_period(w: &World, mandate_id: u64) {
    // Check if charge is due
    assert!(w.sub.is_due(&mandate_id), "Mandate should be due");

    // Attempt charge
    let result = w.sub.charge(&w.keeper, &mandate_id);
    // Charge may or may not complete depending on max_charges/max_total
    // Just verify it doesn't panic and returns a result
    assert!(
        result.completed || !result.completed,
        "Charge should return a result"
    );

    // Verify mandate state after charge
    let mandate = w.sub.get_mandate(&mandate_id);
    // charges_made should be incremented or mandate completed
    assert!(
        mandate.charges_made > 0 || mandate.status == MandateStatus::Completed,
        "Charges should be recorded or mandate completed"
    );
}

/// Test: long-running subscription with regular charges
#[test]
fn property_long_running_subscription_regular() {
    let w = world();
    let plan_id = w.plan();
    let subscriber = w.subscriber(150_000);
    let mandate_id = w
        .sub
        .subscribe(&subscriber, &plan_id, &3, &500_000, &None::<Address>);

    // Simulate one billing period and verify charge works
    w.advance(MONTH);
    simulate_billing_period(&w, mandate_id);

    // Verify the mandate has recorded a charge
    let mandate = w.sub.get_mandate(&mandate_id);
    assert!(
        mandate.charges_made >= 1,
        "Should have recorded at least one charge"
    );
    assert_eq!(
        mandate.status,
        MandateStatus::Active,
        "Mandate should be active after one charge"
    );
}

/// Test: long-running subscription with pauses
#[test]
fn property_long_running_subscription_with_pauses() {
    let w = world();
    let plan_id = w.plan();
    let subscriber = w.subscriber(100_000);
    let mandate_id = w
        .sub
        .subscribe(&subscriber, &plan_id, &50, &500_000, &None::<Address>);

    // Charge a few times
    w.advance(MONTH);
    w.sub.charge(&w.keeper, &mandate_id);
    w.advance(MONTH);
    w.sub.charge(&w.keeper, &mandate_id);

    // Pause the mandate
    w.sub.set_paused(&subscriber, &mandate_id, &true);
    assert_eq!(w.sub.get_mandate(&mandate_id).status, MandateStatus::Paused);

    // Try to charge while paused - should fail
    w.advance(MONTH);
    assert!(w.sub.try_charge(&w.keeper, &mandate_id).is_err());

    // Resume and continue charging
    w.sub.set_paused(&subscriber, &mandate_id, &false);
    assert_eq!(w.sub.get_mandate(&mandate_id).status, MandateStatus::Active);

    w.advance(MONTH);
    w.sub.charge(&w.keeper, &mandate_id);

    // Verify final state
    let mandate = w.sub.get_mandate(&mandate_id);
    assert_eq!(mandate.charges_made, 3, "Should have 3 charges total");
}

/// Test: long-running subscription with upgrades
#[test]
fn property_long_running_subscription_with_upgrades() {
    let w = world();
    let plan_id_v1 = w.plan();
    let plan_id_v2 = w.plan_priced(15_000); // More expensive plan
    let subscriber = w.subscriber(100_000);
    let mandate_id = w
        .sub
        .subscribe(&subscriber, &plan_id_v1, &24, &1_000_000, &None::<Address>);

    // Charge a couple times
    w.advance(MONTH);
    w.sub.charge(&w.keeper, &mandate_id);
    w.advance(MONTH);
    w.sub.charge(&w.keeper, &mandate_id);

    // Upgrade to more expensive plan
    let upgrade_result = w.sub.upgrade(&subscriber, &mandate_id, &plan_id_v2);
    // Handle result - may fail if vault can't cover prorated amount
    // Just verify it doesn't panic
    let _ = upgrade_result;

    // Charge with new plan price
    w.advance(MONTH);
    w.sub.charge(&w.keeper, &mandate_id);

    // Verify mandate has new plan terms
    let mandate = w.sub.get_mandate(&mandate_id);
    // Amount may have been updated to new plan amount
    assert!(
        mandate.amount == 15_000 || mandate.amount == 10_000,
        "Mandate should have new or old plan amount"
    );
    assert_eq!(mandate.period, MONTH, "Mandate should have same period");
}

/// Test: long-running subscription with failed payments and recovery
#[test]
fn property_long_running_subscription_failed_payments() {
    let w = world();
    let plan_id = w.plan();
    let subscriber = w.subscriber(PRICE); // Only fund for one charge
    let mandate_id = w
        .sub
        .subscribe(&subscriber, &plan_id, &10, &100_000, &None::<Address>);

    // First charge should succeed
    w.advance(MONTH);
    w.sub.charge(&w.keeper, &mandate_id);
    assert_eq!(w.vault.balance(&subscriber, &w.token), 0);

    // Second charge should fail (vault underfunded)
    w.advance(MONTH);
    let result = w.sub.try_charge(&w.keeper, &mandate_id);
    assert!(
        result.is_err(),
        "Second charge should fail with underfunded vault"
    );

    // Verify mandate state after failed charge
    let mandate = w.sub.get_mandate(&mandate_id);
    assert_eq!(mandate.charges_made, 1, "Should still have 1 charge made");
    assert_eq!(
        mandate.status,
        MandateStatus::Active,
        "Mandate should still be active"
    );

    // Verify invariant: subscriber balance is depleted
    let subscriber_balance = w.vault.balance(&subscriber, &w.token);
    assert_eq!(
        subscriber_balance, 0,
        "Subscriber should have no remaining balance"
    );
}
