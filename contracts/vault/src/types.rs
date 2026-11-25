//! Storage keys for the vault contract.

use soroban_sdk::{contracttype, Address, String};

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
    /// Largest share of a subscriber's balance that may be reserved, in basis
    /// points; fixed when the contract is deployed (#348).
    ReserveCapBps,
    /// Fixed-window counter for deposit/withdraw operations (#314): where the
    /// current window starts and how many operations it already contains.
    OperationWindow(Address, Address),
    /// Ordered list of contract upgrades: (version, ledger, `wasm_hash`).
    /// Append-only; the initial deployment is not recorded.
    UpgradeHistory,
    /// Proposed successor admin awaiting their acceptance (#238). Missing
    /// means no handover is pending.
    PendingAdmin,
    /// List of mandate IDs with non-zero reserves for (user, token), used by
    /// `emergency_withdraw` (#341) to release every reserve in one call.
    ReservedMandates(Address, Address),
}

/// A subscriber's optional ceiling on total debits per fixed time window,
/// across all of their mandates for one token. Set by the subscriber
/// themselves; it can only ever make charges fail, never move funds.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpendLimit {
    /// Maximum total debit within one window, in the token's smallest unit.
    /// No limit is stored (and the getter reports 0) when this is 0.
    pub amount: i128,
    /// Window length in seconds. Meaningful only when `amount > 0`.
    pub window_secs: u64,
}

/// How much of a `SpendLimit` has been used in the current window. Aligned to
/// the epoch (`start = now - now % window_secs`) so every window boundary is
/// deterministic and the reset does not depend on when the first debit
/// happened.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpendWindow {
    pub start: u64,
    pub spent: i128,
}

/// How many deposit/withdraw operations (user, token) has performed in the
/// current fixed window (#314). The window resets at deterministic,
/// epoch-aligned boundaries so repeated cycling cannot keep a counter alive.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationWindow {
    pub start: u64,
    pub operations: u32,
}

/// A single contract upgrade record.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpgradeRecord {
    /// Semantic version of the new code (from `CARGO_PKG_VERSION`).
    pub version: String,
    /// Ledger sequence number when the upgrade was applied.
    pub ledger: u32,
    /// Hash of the new wasm code.
    pub wasm_hash: soroban_sdk::BytesN<32>,
}
