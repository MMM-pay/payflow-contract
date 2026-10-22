use soroban_sdk::{contracttype, Address, String};

/// `Plan` is defined once in `payflow-shared` and used by both this contract
/// and the subscription contract that decodes it, so the two cannot drift.
pub use payflow_shared::Plan;

/// A merchant's self-declared profile, shown to subscribers alongside the
/// merchant's plans so they can tell who they are paying. The profile is
/// self-declared: the contract does not verify the name or the URL.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Profile {
    pub merchant: Address,
    /// Display name, e.g. "Acme Corp". At most MAX_NAME_LEN bytes.
    pub name: String,
    /// Website or terms URL. At most MAX_URL_LEN bytes.
    pub url: String,
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
    /// A merchant's self-declared profile.
    Profile(Address),
}
