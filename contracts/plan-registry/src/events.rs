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
    /// Where the merchant's share of each charge is paid. Equal to `merchant`
    /// unless the plan was published with a separate treasury address.
    pub payout: Address,
    /// Referral share frozen into the plan, in basis points of the merchant
    /// share. 0 disables referrals.
    pub referral_bps: u32,
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
/// Emitted when a plan's current owner proposes a new owner. The transfer only
/// completes when the proposed owner calls `accept_plan_owner`.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanOwnerProposed {
    #[topic]
    pub merchant: Address,
    #[topic]
    pub plan_id: u64,
    pub new_owner: Address,
}

/// Emitted when a plan changes hands. Mandates already open against the plan
/// are unaffected: they keep paying the address frozen at subscribe time.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanTransferred {
    #[topic]
    pub plan_id: u64,
    pub from: Address,
    pub to: Address,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileUpdated {
    #[topic]
    pub merchant: Address,
    pub name: String,
    pub url: String,
}
