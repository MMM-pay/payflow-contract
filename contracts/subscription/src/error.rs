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
    /// The admin has been updated to a new address.
    AdminUpdated = 13,
}
