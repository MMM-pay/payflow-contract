use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    MandateNotFound = 3,
    NotSubscriber = 4,
    MandateNotActive = 5,
    NotDue = 6,
    PlanInactive = 7,
    FeeTooHigh = 8,
    MaxChargesReached = 9,
    InvalidMaxCharges = 10,
    NotMerchant = 11,
    /// Fee or tip math would overflow `i128` at this amount.
    AmountOverflow = 12,
    /// `pause_until` was given a date that has already passed. Pausing must
    /// move billing forwards, never backwards.
    InvalidUntil = 13,
    /// A refund larger than what the merchant has collected for this mandate.
    RefundExceedsReceived = 14,
    /// A refund amount that is zero or negative.
    InvalidAmount = 15,
}
