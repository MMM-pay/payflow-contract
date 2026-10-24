//! Events emitted by the subscription contract.

use soroban_sdk::{contractevent, Address};

/// Emitted when a subscriber opens a new mandate against a plan.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Subscribed {
    /// Unique identifier of the newly created mandate.
    #[topic]
    pub mandate_id: u64,
    /// Account of the subscriber.
    #[topic]
    pub subscriber: Address,
    /// Account of the merchant.
    #[topic]
    pub merchant: Address,
    /// ID of the plan subscribed to.
    pub plan_id: u64,
    /// Amount charged per billing period.
    pub amount: i128,
    /// Billing period in seconds.
    pub period: u64,
    /// Unix timestamp when the first charge is due.
    pub next_charge: u64,
    /// 0 means open-ended. Carried in the event so indexers do not have to
    /// make a follow-up contract read to learn a mandate's spend cap.
    pub max_charges: u32,
    /// The fee rate frozen into this mandate, for the same reason.
    pub fee_bps: u32,
    /// The keeper tip frozen into this mandate, for the same reason.
    pub keeper_bps: u32,
}

/// Emitted when a mandate charge is successfully executed.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Charged {
    /// ID of the charged mandate.
    #[topic]
    pub mandate_id: u64,
    /// Account of the subscriber debited.
    #[topic]
    pub subscriber: Address,
    /// Account of the merchant credited.
    #[topic]
    pub merchant: Address,
    /// Merchant share after the protocol fee and the keeper tip.
    pub amount: i128,
    /// Protocol fee deducted.
    pub fee: i128,
    /// Paid to `keeper` out of the merchant share. The subscriber's total debit
    /// is `amount + fee + tip` exactly as it was before tips existed.
    pub tip: i128,
    /// Account that settled this charge and received the tip.
    pub keeper: Address,
    /// Total number of charges completed under this mandate.
    pub charges_made: u32,
    /// Unix timestamp when the next charge falls due.
    pub next_charge: u64,
}

/// Emitted when a subscriber cancels a mandate.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Cancelled {
    /// ID of the cancelled mandate.
    #[topic]
    pub mandate_id: u64,
    /// Account of the subscriber who cancelled.
    #[topic]
    pub subscriber: Address,
    /// Total number of charges completed before cancellation.
    pub charges_made: u32,
}

/// Emitted when the merchant ends a mandate, for example because it stopped
/// offering the service. The mandate is cancelled exactly as if the
/// subscriber had cancelled it; the event records who did it.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MandateEnded {
    /// ID of the ended mandate.
    #[topic]
    pub mandate_id: u64,
    /// Merchant who ended the mandate.
    #[topic]
    pub merchant: Address,
    /// Subscriber whose mandate was ended.
    pub subscriber: Address,
    /// Total number of charges completed before ending.
    pub charges_made: u32,
}

/// Emitted when a mandate is paused or resumed.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PauseChanged {
    /// ID of the mandate whose pause status changed.
    #[topic]
    pub mandate_id: u64,
    /// Account of the subscriber.
    #[topic]
    pub subscriber: Address,
    /// Whether the mandate is now paused (`true`) or resumed (`false`).
    pub paused: bool,
}

/// Emitted when a mandate reaches its maximum allowed charges and completes.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MandateCompleted {
    /// ID of the completed mandate.
    #[topic]
    pub mandate_id: u64,
    /// Account of the subscriber.
    #[topic]
    pub subscriber: Address,
    /// Total number of charges executed.
    pub charges_made: u32,
}

/// A merchant refunded part of what it collected for a mandate. The event
/// links the refund to the mandate, so both sides can prove a refund from
/// chain data alone. `refunded_total` is the running total refunded on this
/// mandate, which is what the contract caps at the merchant's receipts.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Refunded {
    /// ID of the refunded mandate.
    #[topic]
    pub mandate_id: u64,
    /// Subscriber who received the refund.
    #[topic]
    pub subscriber: Address,
    /// Merchant who issued the refund.
    #[topic]
    pub merchant: Address,
    /// Refund amount transferred in this operation.
    pub amount: i128,
    /// Cumulative amount refunded on this mandate so far.
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
    /// ID of the upgraded mandate.
    #[topic]
    pub mandate_id: u64,
    /// Subscriber who upgraded.
    #[topic]
    pub subscriber: Address,
    /// Merchant receiving payment under the mandate.
    #[topic]
    pub merchant: Address,
    /// Previous plan ID.
    pub old_plan_id: u64,
    /// New plan ID.
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
    /// Admin account that changed the fee.
    #[topic]
    pub admin: Address,
    /// Previous protocol fee in basis points.
    pub old_fee_bps: u32,
    /// New protocol fee in basis points.
    pub new_fee_bps: u32,
}

/// Emitted after every state change to a mandate, carrying a complete snapshot
/// of the mandate so indexers do not have to reconstruct state or risk drift
/// from missed intermediate events.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MandateUpdated {
    /// Id of the updated mandate.
    #[topic]
    pub mandate_id: u64,
    /// Complete snapshot of the mandate after update.
    pub mandate: crate::types::Mandate,
}

/// Emitted when a subscriber changes the seat count on their mandate.
///
/// The new seat count takes effect from the next charge; no money moves at the
/// time this event is emitted.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SeatsChanged {
    /// ID of the mandate whose seat count changed.
    #[topic]
    pub mandate_id: u64,
    /// Subscriber who made the change.
    #[topic]
    pub subscriber: Address,
    /// Seat count before the change.
    pub old_seats: u32,
    /// Seat count after the change, effective from the next charge.
    pub new_seats: u32,
}
