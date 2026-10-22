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
    /// The plan a mandate is being moved to is denominated in another token.
    /// A mandate's token is frozen when it is opened, so an upgrade can never
    /// change which asset the subscriber pays in.
    DifferentToken = 16,
    /// The plan a mandate is being moved to belongs to another merchant.
    /// Moving between merchants is a cancel and a new subscription, not an
    /// upgrade: the mandate's merchant decides who is paid and who may
    /// refund.
    DifferentMerchant = 17,
}
