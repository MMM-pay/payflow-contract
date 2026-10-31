//! Events emitted by the plan registry contract.

use soroban_sdk::{contractevent, Address, String, Symbol};

/// Emitted when a merchant publishes a new plan.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanCreated {
    /// Account that published the plan.
    #[topic]
    pub merchant: Address,
    /// Unique ID allocated for the newly published plan.
    #[topic]
    pub plan_id: u64,
    /// Token contract address this plan is denominated in.
    pub token: Address,
    /// Amount charged per billing period.
    pub amount: i128,
    /// Billing period in seconds.
    pub period: u64,
    /// Carried in the event so indexers need no follow-up contract read.
    pub name: String,
    /// Short summary of what the plan includes. May be empty.
    pub description: String,
    /// Keeper tip frozen into the plan, in basis points of the merchant share.
    pub keeper_bps: u32,
    /// Discovery category the merchant chose, empty when it chose none.
    pub category: Symbol,
    /// Link to the plan's terms, carried in the event so frontends need no
    /// follow-up contract read. At most 200 bytes; may be empty.
    pub terms_uri: String,
    /// Unix seconds after which the plan stops accepting new subscribers,
    /// 0 = forever. Carried in the event so indexers need no follow-up read.
    pub available_until: u64,
    /// Referral share in basis points of the merchant's share; 0 disables
    /// referrals for this plan.
    pub referral_bps: u32,
    /// Account that receives the merchant's share of each charge; defaults to
    /// the merchant itself.
    pub payout: Address,
}

/// Emitted when a plan's current owner proposes transferring it to a new owner.
/// The transfer takes effect only once the new owner accepts.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanOwnershipProposed {
    #[topic]
    pub merchant: Address,
    #[topic]
    pub plan_id: u64,
    pub new_owner: Address,
}

/// Emitted when the proposed owner accepts and the plan becomes theirs. The
/// plan moves between the two merchants' plan indexes.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanTransferred {
    #[topic]
    pub merchant: Address,
    #[topic]
    pub plan_id: u64,
    pub new_owner: Address,
}

/// Emitted when a merchant activates or deactivates a plan.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanStatusChanged {
    /// Merchant owning the plan.
    #[topic]
    pub merchant: Address,
    /// ID of the plan whose status changed.
    #[topic]
    pub plan_id: u64,
    /// Whether the plan is active and accepting new subscriptions.
    pub active: bool,
}

/// Emitted when a merchant points a plan at the plan that replaces it, if any.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanSuccessorSet {
    /// Merchant owning the plan.
    #[topic]
    pub merchant: Address,
    /// ID of the retiring plan.
    #[topic]
    pub plan_id: u64,
    /// ID of the successor replacement plan.
    pub successor_id: u64,
}

/// Emitted when a merchant sets or updates its self-declared profile. The
/// profile is not verified by the contract.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileUpdated {
    /// Merchant whose profile was updated.
    #[topic]
    pub merchant: Address,
    /// Display name of the merchant.
    pub name: String,
    /// Website or terms URL of the merchant.
    pub url: String,
}

/// Emitted when a merchant renames a plan.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanRenamed {
    #[topic]
    pub merchant: Address,
    #[topic]
    pub plan_id: u64,
    pub name: String,
}
