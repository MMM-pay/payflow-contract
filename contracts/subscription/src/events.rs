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
    /// The keeper tip frozen into this mandate, for the same reason.
    pub keeper_bps: u32,
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
    /// Merchant share after the protocol fee and the keeper tip.
    pub amount: i128,
    pub fee: i128,
    /// Paid to `keeper` out of the merchant share. The subscriber's total debit
    /// is `amount + fee + tip` exactly as it was before tips existed.
    pub tip: i128,
    /// Account that settled this charge and received the tip.
    pub keeper: Address,
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

/// A merchant refunded part of what it collected for a mandate. The event
/// links the refund to the mandate, so both sides can prove a refund from
/// chain data alone. `refunded_total` is the running total refunded on this
/// mandate, which is what the contract caps at the merchant's receipts.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Refunded {
    #[topic]
    pub mandate_id: u64,
    #[topic]
    pub subscriber: Address,
    #[topic]
    pub merchant: Address,
    pub amount: i128,
    pub refunded_total: i128,
}

/// Emitted when a subscriber moves a mandate onto a different plan.
///
/// `charged` is the prorated difference collected for the part of the
/// current period that was still ahead; it is zero for a downgrade, which
/// takes effect at the next charge and never refunds anything. The event
/// carries the old and the new terms so an indexer can show the change
/// without a follow-up contract read.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Upgraded {
    #[topic]
    pub mandate_id: u64,
    #[topic]
    pub subscriber: Address,
    #[topic]
    pub merchant: Address,
    pub old_plan_id: u64,
    pub new_plan_id: u64,
    /// The price the mandate paid per period before the upgrade.
    pub old_amount: i128,
    /// The price the mandate pays per period from the upgrade on.
    pub new_amount: i128,
    /// Difference collected now, prorated and rounded down. Zero for a
    /// downgrade.
    pub charged: i128,
    /// The new plan's period, in seconds.
    pub period: u64,
    /// When the next charge falls due. The upgrade does not move it.
    pub next_charge: u64,
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

/// Emitted after every state change to a mandate, carrying a complete snapshot
/// of the mandate so indexers do not have to reconstruct state or risk drift
/// from missed intermediate events.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MandateUpdated {
    #[topic]
    pub mandate_id: u64,
    pub mandate: crate::types::Mandate,
}
