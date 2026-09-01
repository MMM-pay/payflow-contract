use soroban_sdk::{contractevent, Address};

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Subscribed {
    #[topic]
    pub mandate_id: u64,
    #[topic]
    pub subscriber: Address,
    #[topic]
    pub merchant: Address,
    pub plan_id: u64,
    pub amount: i128,
    pub period: u64,
    pub next_charge: u64,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Charged {
    #[topic]
    pub mandate_id: u64,
    #[topic]
    pub subscriber: Address,
    #[topic]
    pub merchant: Address,
    pub amount: i128,
    pub fee: i128,
    pub charges_made: u32,
    pub next_charge: u64,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Cancelled {
    #[topic]
    pub mandate_id: u64,
    #[topic]
    pub subscriber: Address,
    pub charges_made: u32,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PauseChanged {
    #[topic]
    pub mandate_id: u64,
    #[topic]
    pub subscriber: Address,
    pub paused: bool,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MandateCompleted {
    #[topic]
    pub mandate_id: u64,
    #[topic]
    pub subscriber: Address,
    pub charges_made: u32,
}
