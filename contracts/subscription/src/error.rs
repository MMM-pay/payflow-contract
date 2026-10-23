//! Error types returned by the subscription contract.

use soroban_sdk::contracterror;

/// Errors returned by the subscription contract.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    /// The contract has already been initialized.
    AlreadyInitialized = 1,
    /// The contract has not been initialized with required configuration.
    NotInitialized = 2,
    /// The requested mandate ID does not exist in storage.
    MandateNotFound = 3,
    /// The caller is not the subscriber who opened the mandate.
    NotSubscriber = 4,
    /// The mandate is not active (paused, cancelled, or completed).
    MandateNotActive = 5,
    /// The current ledger timestamp is earlier than the mandate's next charge time.
    NotDue = 6,
    /// The plan is inactive and does not accept subscriptions or upgrades.
    PlanInactive = 7,
    /// The requested protocol fee in basis points exceeds MAX_FEE_BPS.
    FeeTooHigh = 8,
    /// The mandate has reached its maximum charge count.
    MaxChargesReached = 9,
    /// The maximum charge count parameter is invalid.
    InvalidMaxCharges = 10,
    /// The caller is not the merchant of the mandate.
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
    /// The estimated end timestamp for a mandate's remaining schedule overflows `u64`.
    ScheduleOverflow = 18,
    /// The seat count is zero, or exceeds the plan's `max_seats`.
    InvalidSeats = 19,
    /// The seat count was supplied for a plan that is not seat-based, or was
    /// omitted for one that is.
    SeatsNotApplicable = 20,
    /// `price * seats` overflows `i128`.
    SeatsOverflow = 21,
}
