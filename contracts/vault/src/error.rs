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
}
