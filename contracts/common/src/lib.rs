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

/// Integer basis-point denominator used by mandate fees.
pub const BPS_DENOMINATOR: i128 = 10_000;

/// Split a mandate charge into protocol fee, keeper tip, referral share and
/// merchant payout. Returns `None` on multiplication overflow. Tip and
/// referral rates are clamped to their configured ceilings before calculation.
pub fn split_mandate_fee(
    amount: i128,
    fee_bps: u32,
    keeper_bps: u32,
    referral_bps: u32,
    has_referrer: bool,
    max_keeper_bps: u32,
    max_referral_bps: u32,
) -> Option<(i128, i128, i128, i128)> {
    let fee = amount.checked_mul(i128::from(fee_bps))? / BPS_DENOMINATOR;
    let merchant_gross = amount - fee;
    let tip = merchant_gross.checked_mul(i128::from(keeper_bps.min(max_keeper_bps)))?
        / BPS_DENOMINATOR;
    let referral = if has_referrer {
        merchant_gross.checked_mul(i128::from(referral_bps.min(max_referral_bps)))?
            / BPS_DENOMINATOR
    } else {
        0
    };
    Some((fee, tip, referral, merchant_gross - tip - referral))
}

/// Prorate only a positive price increase for the unelapsed portion of a
/// billing period. A downgrade, expired period, or zero period costs zero.
/// Returns `None` if the multiplication overflows.
pub fn prorated_upgrade(
    old_amount: i128,
    new_amount: i128,
    old_period: u64,
    next_charge: u64,
    now: u64,
) -> Option<i128> {
    let remaining = next_charge.saturating_sub(now).min(old_period);
    let difference = new_amount - old_amount;
    if difference > 0 && old_period > 0 {
        Some(difference.checked_mul(i128::from(remaining))? / i128::from(old_period))
    } else {
        Some(0)
    }
}

/// Calculate the next due timestamp from the settlement time, not the
/// previously scheduled timestamp, so delayed charges do not accumulate.
pub fn next_due_at(now: u64, period: u64) -> Option<u64> {
    now.checked_add(period)
}

/// Calculate the estimated last due timestamp for remaining charges.
/// The caller handles zero remaining charges (which uses last_charge).
pub fn estimated_last_due(next_charge: u64, period: u64, remaining_charges: u32) -> Option<u64> {
    let offset = u64::from(remaining_charges.saturating_sub(1)).checked_mul(period)?;
    next_charge.checked_add(offset)
}

#[cfg(test)]
mod mandate_math_tests {
    use super::*;

    #[test]
    fn fee_split_preserves_integer_rounding_and_caps() {
        assert_eq!(split_mandate_fee(10_001, 100, 100, 100, true, 50, 50),
            Some((100, 49, 49, 9_803)));
        assert_eq!(split_mandate_fee(100, 0, 0, 0, false, 50, 50),
            Some((0, 0, 0, 100)));
        assert_eq!(split_mandate_fee(i128::MAX, 2, 0, 0, false, 50, 50), None);
    }

    #[test]
    fn proration_truncates_and_downgrades_do_not_refund() {
        assert_eq!(prorated_upgrade(100, 200, 10, 10, 7), Some(30));
        assert_eq!(prorated_upgrade(200, 100, 10, 10, 7), Some(0));
        assert_eq!(prorated_upgrade(100, 200, 10, 4, 7), Some(0));
        assert_eq!(prorated_upgrade(100, 200, 0, 10, 7), Some(0));
        assert_eq!(prorated_upgrade(0, i128::MAX, 10, 10, 0), None);
    }

    #[test]
    fn schedule_uses_checked_arithmetic() {
        assert_eq!(next_due_at(100, 10), Some(110));
        assert_eq!(next_due_at(u64::MAX, 1), None);
        assert_eq!(estimated_last_due(100, 10, 3), Some(120));
        assert_eq!(estimated_last_due(u64::MAX, 10, 2), None);
    }
}
