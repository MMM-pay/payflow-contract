//! Data types and storage keys for the plan registry contract.

use soroban_sdk::{contracttype, Address, String};

/// `Plan` is defined once in `payflow-shared` and used by both this contract
/// and the subscription contract that decodes it, so the two cannot drift.
///
/// Seat-based plans: `per_seat` marks a plan whose price is multiplied by the
/// number of seats chosen at subscribe time, bounded by `max_seats`. Non-seat
/// plans keep `per_seat == false` and `max_seats == 0`.
pub use payflow_shared::Plan;

/// A merchant's self-declared profile, shown to subscribers alongside the
/// merchant's plans so they can tell who they are paying. The profile is
/// self-declared: the contract does not verify the name or the URL.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Profile {
    /// Account address of the merchant.
    pub merchant: Address,
    /// Display name, e.g. "Acme Corp". At most MAX_NAME_LEN bytes.
    pub name: String,
    /// Website or terms URL. At most MAX_URL_LEN bytes.
    pub url: String,
}

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
}
