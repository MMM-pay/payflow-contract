//! Shared TTL constants and storage helpers for the Payflow contracts.
//!
//! Configuration lives in instance storage; mandates, plans, balances and
//! index entries live in persistent storage with a 90-day TTL that is
//! extended on write, not on read. An entry untouched for longer is archived
//! and must be restored before use.

#![no_std]
#![deny(missing_docs)]

use soroban_sdk::{Env, IntoVal, Val};

/// Number of ledgers in a day, assuming ~5 second close times.
pub const DAY_IN_LEDGERS: u32 = 17_280;

/// Bump amount for instance storage (configuration).
pub const INSTANCE_BUMP: u32 = 30 * DAY_IN_LEDGERS;

/// Bump amount for persistent storage (mandates, plans, balances, indexes).
pub const PERSIST_BUMP: u32 = 90 * DAY_IN_LEDGERS;

/// Threshold below which instance storage is extended on access.
pub const INSTANCE_THRESHOLD: u32 = INSTANCE_BUMP - DAY_IN_LEDGERS;

/// Threshold below which persistent storage is extended on write.
pub const PERSIST_THRESHOLD: u32 = PERSIST_BUMP - DAY_IN_LEDGERS;

/// Extend the TTL of instance storage, keeping configuration alive.
pub fn extend_instance_ttl(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_BUMP);
}

/// Extend the TTL of a persistent entry after a write.
pub fn extend_persistent_ttl<K>(env: &Env, key: &K)
where
    K: IntoVal<Env, Val> + Clone,
{
    env.storage()
        .persistent()
        .extend_ttl(key, PERSIST_THRESHOLD, PERSIST_BUMP);
}
