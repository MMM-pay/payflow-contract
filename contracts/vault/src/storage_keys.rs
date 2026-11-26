//! Canonical typed storage key namespace for the vault contract.
//!
//! The variant order and payload types are intentionally unchanged: Soroban
//! serializes these keys into ledger storage, so reordering or renaming them
//! would make existing entries inaccessible after a contract upgrade.

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
    /// The (user, token)-scoped spending limit, when one has been set.
    SpendLimit(Address, Address),
    /// Fixed-window spend tracker for (user, token): where the current window
    /// starts and how much has been debited inside it.
    SpendWindow(Address, Address),
    /// Amount of (user, token) the subscriber earmarked for one mandate
    /// (#295). Only that mandate's debits may draw it down. Absent means 0.
    Reserved(Address, Address, u64),
    /// Sum of every `Reserved` entry for (user, token). Always at most the
    /// (user, token) balance, so the unreserved part never goes negative.
    ReservedTotal(Address, Address),
    /// Fixed-window counter for deposit/withdraw operations (#314): where the
    /// current window starts and how many operations it already contains.
    OperationWindow(Address, Address),
    /// Ordered list of contract upgrades: (version, ledger, `wasm_hash`).
    /// Append-only; the initial deployment is not recorded.
    UpgradeHistory,
    /// Proposed new admin address awaiting acceptance (#238).
    PendingAdmin,
    /// Ordered list of mandate IDs that have a reserve for (user, token).
    /// Used by `emergency_withdraw` to clear all reserves in one pass.
    ReservedMandates(Address, Address),
    /// Reserve cap in basis points, set at construction (#348). Absent means
    /// the contract was deployed before reserve caps were introduced.
    ReserveCapBps,
}
