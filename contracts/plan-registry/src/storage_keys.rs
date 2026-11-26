//! Canonical typed storage key namespace for the plan-registry contract.
//!
//! The variant order and payload types are intentionally unchanged: Soroban
//! serializes these keys into ledger storage, so reordering or renaming them
//! would make existing entries inaccessible after a contract upgrade.

use soroban_sdk::{contracttype, Address};

/// Storage keys used to partition instance and persistent storage for the plan registry.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Contract administrator address.
    Admin,
    /// Counter storing the next sequential plan ID to be allocated.
    NextPlanId,
    /// A published plan record stored by its unique ID.
    Plan(u64),
    /// Number of plans a merchant has published.
    MerchantPlanCount(Address),
    /// The n-th plan a merchant published (0-based).
    MerchantPlan(Address, u32),
    /// A merchant's self-declared profile.
    Profile(Address),
    /// Number of plans priced in a given token.
    TokenPlanCount(Address),
    /// The n-th plan priced in a given token (0-based).
    TokenPlan(Address, u32),
    /// Position of a plan within its merchant's `MerchantPlan` index, so an
    /// ownership transfer can remove it in constant time.
    PlanIndex(u64),
    /// Pending ownership proposal for a plan: the address the current owner
    /// proposed as the new owner, awaiting their acceptance.
    PlanOwnerProposal(u64),
    /// Whether charging is paused for a plan (#315). Missing means not paused.
    PlanChargesPaused(u64),
    /// Grace window for plans opened after the setting was changed (#312).
    /// Missing means zero.
    PlanGracePeriod(u64),
    /// Ordered list of contract upgrades: (version, ledger, `wasm_hash`).
    /// Append-only; the initial deployment is not recorded.
    UpgradeHistory,
    /// Proposed successor admin awaiting their acceptance (#238). Missing
    /// means no handover is pending.
    PendingAdmin,
}
