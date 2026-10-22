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
    /// The debit would take the user over their per-window spend limit.
    /// New variants go at the end so existing codes keep their numbers.
    SpendLimitExceeded = 6,
    /// `set_spend_limit` was given a positive amount with a zero window.
    InvalidWindow = 7,
}
