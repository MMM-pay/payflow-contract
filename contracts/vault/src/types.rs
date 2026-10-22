use soroban_sdk::{contracttype, Address};

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    /// The single subscription contract permitted to call `debit`.
    Subscription,
    /// Spendable balance held for (user, token).
    Balance(Address, Address),
    /// The (user, token)-scoped spending limit, when one has been set.
    SpendLimit(Address, Address),
    /// Fixed-window spend tracker for (user, token): where the current window
    /// starts and how much has been debited inside it.
    SpendWindow(Address, Address),
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
