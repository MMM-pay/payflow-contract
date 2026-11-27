//! Cross-contract interfaces for plan registry and vault clients.

use crate::types::{Plan, PlanChargeTerms};
use soroban_sdk::{contractclient, Address, Env};

/// Read side of `payflow-plan-registry`.
///
/// Interface changes to this trait must be listed under `Unreleased` in
/// CHANGELOG.md and flagged in the PR (`interface-change` label); CI publishes
/// the generated spec under `spec/` so downstream breakage is caught early.
#[allow(dead_code)]
#[contractclient(name = "PlanRegistryClient")]
pub trait PlanRegistryInterface {
    /// Retrieve a plan by ID from the plan registry.
    fn get_plan(env: Env, plan_id: u64) -> Plan;

    /// Whether the merchant paused charging for this plan (#315).
    fn plan_charges_paused(env: Env, plan_id: u64) -> bool;

    /// Grace window for new mandates against this plan (#312).
    fn plan_grace_period(env: Env, plan_id: u64) -> u64;

    /// Merchant, token, period and charges-paused flag in one call, so a
    /// settlement reads the plan once.
    fn plan_charge_terms(env: Env, plan_id: u64) -> PlanChargeTerms;
}

/// Debit side of `payflow-vault`.
///
/// Interface changes to this trait must be listed under `Unreleased` in
/// CHANGELOG.md and flagged in the PR (`interface-change` label); CI publishes
/// the generated spec under `spec/` so downstream breakage is caught early.
#[allow(dead_code)]
#[contractclient(name = "VaultClient")]
pub trait VaultInterface {
    /// Debit tokens from a subscriber's vault balance and transfer to `to`.
    fn debit(env: Env, user: Address, token: Address, to: Address, amount: i128);

    /// Debit a subscriber once and pay two recipients from that debit: the
    /// merchant share and the protocol fee of one charge.
    fn debit_split(
        env: Env,
        user: Address,
        token: Address,
        to_a: Address,
        amount_a: i128,
        to_b: Address,
        amount_b: i128,
    );

    /// `debit` on behalf of one mandate: that mandate's own reserve (#295)
    /// is spendable too and is drawn down first.
    fn debit_mandate(
        env: Env,
        user: Address,
        token: Address,
        mandate_id: u64,
        to: Address,
        amount: i128,
    );

    /// `debit_split` on behalf of one mandate: that mandate's own reserve
    /// (#295) is spendable too and is drawn down first.
    #[allow(clippy::too_many_arguments)]
    fn debit_split_mandate(
        env: Env,
        user: Address,
        token: Address,
        mandate_id: u64,
        to_a: Address,
        amount_a: i128,
        to_b: Address,
        amount_b: i128,
    );

    /// `debit_multi_mandate` on behalf of one mandate: that mandate's own
    /// reserve (#295) is spendable too and is drawn down first. Settles
    /// multiple recipients in one call (merchant, fee, tip, referral).
    fn debit_multi_mandate(
        env: Env,
        user: Address,
        token: Address,
        mandate_id: u64,
        legs: soroban_sdk::Vec<(Address, i128)>,
    );

    /// Whether `debit_mandate` for `amount` would succeed right now on
    /// balance and spend-limit grounds. Used by `flag_failed_charge` (#296)
    /// to prove a due charge would fail before flagging it.
    fn can_debit(env: Env, user: Address, token: Address, mandate_id: u64, amount: i128) -> bool;

    /// Credit side of `payflow-vault`, used by `subscribe_with_deposit` so a
    /// subscriber can fund and subscribe in one transaction. The nested call
    /// requires the subscriber's auth, which is covered by the outer
    /// `subscribe_with_deposit` authorization.
    fn deposit(env: Env, user: Address, token: Address, amount: i128);

    /// Balance available to a new mandate after existing reserves.
    fn unreserved_balance(env: Env, user: Address, token: Address) -> i128;
}
