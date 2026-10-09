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
    /// 0 means open-ended. Carried in the event so indexers do not have to
    /// make a follow-up contract read to learn a mandate's spend cap.
    pub max_charges: u32,
    /// The fee rate frozen into this mandate, for the same reason.
    pub fee_bps: u32,
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

/// Emitted when the merchant ends a mandate, for example because it stopped
/// offering the service. The mandate is cancelled exactly as if the
/// subscriber had cancelled it; the event records who did it.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MandateEnded {
    #[topic]
    pub mandate_id: u64,
    #[topic]
    pub merchant: Address,
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

/// Emitted when the admin changes the protocol fee. Applies to mandates opened
/// after this point only; existing mandates keep the fee they were opened with.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeeChanged {
    #[topic]
    pub admin: Address,
    pub old_fee_bps: u32,
    pub new_fee_bps: u32,
}
