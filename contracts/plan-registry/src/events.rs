use soroban_sdk::{contractevent, Address, String};

/// Emitted when a merchant publishes a new plan.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanCreated {
    #[topic]
    pub merchant: Address,
    #[topic]
    pub plan_id: u64,
    pub token: Address,
    pub amount: i128,
    pub period: u64,
    /// Carried in the event so indexers need no follow-up contract read.
    pub name: String,
    /// Keeper tip frozen into the plan, in basis points of the merchant share.
    pub keeper_bps: u32,
    /// Referral share frozen into the plan, in basis points of the merchant
    /// share. 0 means referrals are disabled for this plan.
    pub referral_bps: u32,
    /// The address charges pay, which may be a treasury rather than the
    /// merchant itself.
    pub payout: Address,
}

/// Emitted when a merchant proposes to hand a plan to a new owner. The
/// transfer only takes effect when the proposed owner accepts.
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
    #[topic]
    pub merchant: Address,
    #[topic]
    pub plan_id: u64,
    pub active: bool,
}

/// Emitted when a merchant sets or updates its self-declared profile. The
/// profile is not verified by the contract.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileUpdated {
    #[topic]
    pub merchant: Address,
    pub name: String,
    pub url: String,
}
