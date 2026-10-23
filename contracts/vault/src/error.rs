//! Error types returned by vault contract operations.

use soroban_sdk::contracterror;

/// Errors returned by the vault contract.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    /// The contract has already been initialized.
    AlreadyInitialized = 1,
    /// The contract has not been initialized with an admin.
    NotInitialized = 2,
    /// An amount was zero or negative, or an invalid parameter was given.
    InvalidAmount = 3,
    /// The user's spendable vault balance is insufficient for the requested amount.
    InsufficientBalance = 4,
    /// No subscription contract address has been configured.
    SubscriptionNotSet = 5,
    /// The admin has been updated to a new address.
    AdminUpdated = 6,
    /// Deposit cap exceeded.
    DepositCapExceeded = 7,
    /// Token not allowed.
    TokenNotAllowed = 8,
}
