use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    InvalidAmount = 3,
    InsufficientBalance = 4,
    SubscriptionNotSet = 5,
    /// The allowlist is enabled and this token is not on it. Deposits of
    /// unlisted tokens are rejected; withdrawals never are.
    TokenNotAllowed = 6,
    /// The deposit would take the subscriber's balance for this token above
    /// the cap the admin set. A cap of 0 means unlimited, so this error only
    /// fires while a cap is in force. Withdrawals are never blocked.
    DepositCapExceeded = 7,
}
