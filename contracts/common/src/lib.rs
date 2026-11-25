//! Shared TTL constants and storage helpers for the Payflow contracts.
//!
//! Configuration lives in instance storage; mandates, plans, balances and
//! index entries live in persistent storage with a 90-day TTL. Every write
//! extends it, and reads of hot mandate/plan keys extend it too (#247) so an
//! actively used entry does not expire mid-life. An entry untouched for longer
//! is archived and must be restored before use.

#![no_std]
#![deny(missing_docs)]

use soroban_sdk::{Env, IntoVal, TryFromVal, Val};

/// Number of ledgers in a day, assuming ~5 second close times.
pub const DAY_IN_LEDGERS: u32 = 17_280;

/// Bump amount for instance storage (configuration).
pub const INSTANCE_BUMP: u32 = 30 * DAY_IN_LEDGERS;

/// Bump amount for persistent storage (mandates, plans, balances, indexes).
pub const PERSIST_BUMP: u32 = 90 * DAY_IN_LEDGERS;

/// Threshold below which instance storage is extended on access.
pub const INSTANCE_THRESHOLD: u32 = INSTANCE_BUMP - DAY_IN_LEDGERS;

/// Threshold below which persistent storage is extended on write and on
/// access to a hot key.
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

/// Read `key` from persistent storage, or return `error` when it is absent.
///
/// Contract code used to repeat
/// `env.storage().persistent().get(&key).ok_or(ProjectError::NotFound)?` at
/// every entry point. This keeps the error mapping in one place while still
/// naming the storage location at the call site, so a reader can tell a
/// persistent read from an instance one.
#[inline]
pub fn load_or_err<K, V, E>(env: &Env, key: &K, error: E) -> Result<V, E>
where
    K: IntoVal<Env, Val> + Clone,
    V: TryFromVal<Env, Val>,
{
    env.storage().persistent().get::<K, V>(key).ok_or(error)
}

/// Read `key` from instance storage (contract configuration), or return
/// `error` when it is absent. See [`load_or_err`].
#[inline]
pub fn load_instance_or_err<K, V, E>(env: &Env, key: &K, error: E) -> Result<V, E>
where
    K: IntoVal<Env, Val> + Clone,
    V: TryFromVal<Env, Val>,
{
    env.storage().instance().get::<K, V>(key).ok_or(error)
}

/// Returns `true` if `token` is not an obviously invalid, zero/burn, or
/// self-referential address.
pub fn is_valid_token_address(env: &soroban_sdk::Env, token: &soroban_sdk::Address) -> bool {
    let zero_contract = soroban_sdk::Address::from_str(
        env,
        "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABSC4",
    );
    let zero_account = soroban_sdk::Address::from_str(
        env,
        "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF",
    );
    *token != zero_contract
        && *token != zero_account
        && *token != env.current_contract_address()
}
