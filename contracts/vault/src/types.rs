//! Storage keys for the vault contract.

use soroban_sdk::{contracttype, String};

// Keep the historical types::DataKey path available to existing consumers.
pub use crate::storage_keys::DataKey;

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
