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
    /// The allowlist is enabled and this token is not on it. Deposits of
    /// unlisted tokens are rejected; withdrawals never are.
    TokenNotAllowed = 6,
    /// The deposit would take the subscriber's balance for this token above
    /// the cap the admin set. A cap of 0 means unlimited, so this error only
    /// fires while a cap is in force. Withdrawals are never blocked.
    DepositCapExceeded = 7,
    /// The subscriber has not opted in to allowance top-up for this mandate.
    /// Allowance fallback is disabled by default and must be enabled per subscriber.
    AllowanceNotEnabled = 8,
    /// The shortfall could not be covered by the SEP-41 allowance granted to the vault,
    /// either because the allowance is too small or the subscriber never granted one.
    AllowanceInsufficient = 9,
    /// The SEP-41 allowance granted to the vault has expired (its expiration ledger has passed).
    AllowanceExpired = 10,
}
