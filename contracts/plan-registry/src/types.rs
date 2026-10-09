use soroban_sdk::{contracttype, Address, String};

/// A merchant-published charge template. A `Plan` is immutable except for its
/// `active` flag: changing price or period requires publishing a new plan so
/// existing mandates can never be repriced under a subscriber.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Plan {
    pub id: u64,
    pub merchant: Address,
    /// Human-readable label shown to subscribers, e.g. "Pro Monthly".
    /// At most MAX_NAME_LEN bytes; may be empty.
    pub name: String,
    /// SEP-41 token this plan is denominated in.
    pub token: Address,
    /// Amount charged per period, in the token's smallest unit.
    pub amount: i128,
    /// Billing period in seconds.
    pub period: u64,
    pub active: bool,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    NextPlanId,
    Plan(u64),
    /// Number of plans a merchant has published.
    MerchantPlanCount(Address),
    /// The n-th plan a merchant published (0-based).
    MerchantPlan(Address, u32),
}
