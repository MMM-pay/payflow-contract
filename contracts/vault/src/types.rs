//! Storage keys for the vault contract.

use soroban_sdk::{contracttype, Address};

/// Keys used to partition the vault's instance and persistent storage.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// The admin address authorized to update configuration.
    Admin,
    /// The single subscription contract permitted to call `debit`.
    Subscription,
    /// Spendable balance held for (user, token).
    Balance(Address, Address),
    /// Whether the token allowlist is enforced at all. Off by default.
    AllowlistEnabled,
    /// Whether one token may be deposited. Only read while the allowlist is
    /// enabled; absent means not allowed.
    TokenAllowed(Address),
    /// Maximum balance a subscriber may hold for (user, token). Absent or
    /// zero means unlimited.
    DepositCap(Address, Address),
}
