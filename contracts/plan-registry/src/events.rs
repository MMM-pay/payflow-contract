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
