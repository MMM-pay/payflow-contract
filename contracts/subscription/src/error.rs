//! Error types returned by the subscription contract.

use payflow_shared::ErrorCode;
use soroban_sdk::contracterror;

/// Errors returned by the subscription contract.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    /// The contract has already been initialized.
    AlreadyInitialized = 2001,
    /// The contract has not been initialized with required configuration.
    NotInitialized = 2002,
    /// The requested mandate ID does not exist in storage.
    MandateNotFound = 2003,
    /// The caller is not the subscriber who opened the mandate.
    NotSubscriber = 2004,
    /// The mandate is not active (paused, cancelled, or completed).
    MandateNotActive = 2005,
    /// The current ledger timestamp is earlier than the mandate's next charge time.
    NotDue = 2006,
    /// The plan is inactive and does not accept subscriptions or upgrades.
    PlanInactive = 2007,
    /// The requested protocol fee in basis points exceeds `MAX_FEE_BPS`.
    FeeTooHigh = 2008,
    /// The mandate has reached its maximum charge count.
    MaxChargesReached = 2009,
    /// The maximum charge count parameter is invalid.
    InvalidMaxCharges = 2010,
    /// The caller is not the merchant of the mandate.
    NotMerchant = 2011,
    /// Fee or tip math would overflow `i128` at this amount.
    AmountOverflow = 2012,
    /// `pause_until` was given a date that has already passed. Pausing must
    /// move billing forwards, never backwards.
    InvalidUntil = 2013,
    /// A refund larger than what the merchant has collected for this mandate.
    RefundExceedsReceived = 2014,
    /// A refund amount that is zero or negative.
    InvalidAmount = 2015,
    /// The plan a mandate is being moved to is denominated in another token.
    /// A mandate's token is frozen when it is opened, so an upgrade can never
    /// change which asset the subscriber pays in.
    DifferentToken = 2016,
    /// The plan a mandate is being moved to belongs to another merchant.
    /// Moving between merchants is a cancel and a new subscription, not an
    /// upgrade: the mandate's merchant decides who is paid and who may
    /// refund.
    DifferentMerchant = 2017,
    /// The estimated end timestamp for a mandate's remaining schedule overflows `u64`.
    ScheduleOverflow = 2018,
    /// The seat count is zero, or exceeds the plan's `max_seats`.
    InvalidSeats = 2019,
    /// The seat count was supplied for a plan that is not seat-based, or was
    /// omitted for one that is.
    SeatsNotApplicable = 2020,
    /// `price * seats` overflows `i128`.
    SeatsOverflow = 2021,
    /// A total-spend cap that is negative.
    InvalidMaxTotal = 2022,
    /// The pause has lasted longer than the maximum pause length frozen into
    /// the mandate, so it can be expired.
    PauseNotExpired = 2023,
    /// The pause has exceeded its maximum length and must be expired with
    /// `expire_pause`, not resumed or extended.
    PauseExpired = 2024,
    /// A batch subscription was given more plans than `MAX_BATCH_PLANS`.
    BatchTooLarge = 2025,
    /// A batch subscription was given no plans.
    EmptyBatch = 2026,
    /// `subscribe_at` was given a `start` before the current ledger timestamp
    /// or more than `MAX_START_OFFSET` (one year) after it.
    InvalidStart = 2027,
    /// A cancellation reason outside the documented `CANCEL_REASON_*` code
    /// set was supplied. The set is closed so an indexer can map every
    /// `Cancelled` event it will ever see without a catch-all branch.
    InvalidCancelReason = 2028,
    /// The caller gave a position in `subscriber_mandates` that does not hold
    /// this mandate, so the transfer cannot update the old index in place.
    InvalidSubscriberIndex = 2029,
    /// `accept_mandate_owner` was called for a mandate with no pending
    /// transfer, or after the transfer was already accepted.
    NoPendingTransfer = 2030,
    /// The address accepting the transfer is not the one the current
    /// subscriber proposed in `propose_mandate_owner`.
    NotProposedOwner = 2031,
    /// A transfer was proposed to the mandate's current subscriber; a move to
    /// the same address is a no-op and is rejected.
    SameSubscriber = 2032,
    /// The new subscriber already holds the maximum number of mandates its
    /// index can track.
    MandateIndexFull = 2033,
    /// A new subscription was rejected because the plan is at its
    /// `max_subscribers` seat cap.
    PlanFull = 2034,
    /// `flag_failed_charge` was called for a due mandate whose charge would
    /// succeed right now: the subscriber's vault covers it. Only a charge
    /// that would really fail can be flagged (#296).
    ChargeWouldSucceed = 2035,
    /// The plan registry did not return a plan for the requested ID.
    PlanNotFound = 2036,
    /// The vault rejected a deposit or debit. The subscription call fails
    /// without changing mandate state; the vault's own error code is preserved
    /// in the invocation diagnostics.
    VaultCallFailed = 2037,
    /// The mandate's plan has charging paused by its merchant (#315). The
    /// mandate itself is still open; retry after the merchant resumes.
    PlanChargesPaused = 2038,
    /// The admin has paused the protocol (#285). New subscriptions, charges
    /// and plan upgrades are refused until it is resumed; reads,
    /// cancellations, refunds and vault withdrawals are unaffected.
    ProtocolPaused = 2039,
    /// A refund transfer did not deliver exactly the recorded refund amount,
    /// so the token is unsafe for exact-amount accounting (for example a
    /// fee-on-transfer token). The refund is rolled back.
    UnexpectedTransferAmount = 2040,
    /// `accept_admin` was called with no pending admin handover, or after the
    /// proposal was already accepted (#238).
    PendingAdminNotFound = 2041,
    /// The address accepting the admin role is not the one the current admin
    /// proposed in `propose_admin` (#238).
    NotPendingAdmin = 2042,
    /// The requested mandate ID has been tombstoned after deletion and cannot
    /// be reused (#368).
    MandateIdTombstoned = 2043,
    /// The requested mandate ID is already assigned to an existing mandate (#368).
    MandateAlreadyExists = 2044,
    /// The requested mandate ID is invalid (mandate IDs start at 1) (#368).
    InvalidMandateId = 2045,
    /// An active or paused mandate cannot be deleted until it is cancelled or
    /// completed (#368).
    MandateStillActive = 2046,
    /// A configured address is the contract itself or otherwise invalid for
    /// this role (#282).
    InvalidAddress = 2047,
}

impl From<Error> for ErrorCode {
    fn from(error: Error) -> Self {
        ErrorCode::new(error as u32)
    }
}
