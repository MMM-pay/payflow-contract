//! Cross-contract interfaces for plan registry and vault clients.

use crate::types::Plan;
use soroban_sdk::{contractclient, Address, Env};

/// Read side of `payflow-plan-registry`.
#[allow(dead_code)]
#[contractclient(name = "PlanRegistryClient")]
pub trait PlanRegistryInterface {
    /// Retrieve a plan by ID from the plan registry.
    fn get_plan(env: Env, plan_id: u64) -> Plan;
}

/// Debit side of `payflow-vault`.
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

    /// Credit side of `payflow-vault`, used by `subscribe_with_deposit` so a
    /// subscriber can fund and subscribe in one transaction. The nested call
    /// requires the subscriber's auth, which is covered by the outer
    /// `subscribe_with_deposit` authorization.
    fn deposit(env: Env, user: Address, token: Address, amount: i128);
}
