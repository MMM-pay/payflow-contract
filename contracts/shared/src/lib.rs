#![no_std]
#![deny(missing_docs)]

//! Types that cross a contract boundary.
//!
//! `payflow-plan-registry` stores `Plan` and the subscription contract decodes
//! it from `get_plan`. Both use this one definition, so the wire layout cannot
//! drift: there is no second copy to fall out of date. A field added or removed
//! here changes the type both contracts build against.
//!
//! This crate holds data types only. It has no contract, no entry points, and
//! no storage keys of its own.

use soroban_sdk::{contracttype, Address, String, Symbol};

/// Largest keeper tip a plan may set, in basis points of the merchant's share:
/// 5%. The tip is paid out of the merchant's share and never out of the
/// subscriber's, so this cap bounds what a merchant can spend on keepers, not
/// what a subscriber pays.
pub const MAX_KEEPER_BPS: u32 = 500;

/// Largest plan description, in bytes. A description is a short, plain
/// summary of what the plan includes ("monthly consults, unlimited
/// follow-ups"). At most MAX_DESCRIPTION_LEN bytes; may be empty.
pub const MAX_DESCRIPTION_LEN: u32 = 280;

#[allow(missing_docs)]
mod plan {
    use super::*;

    /// A merchant-published charge template. A `Plan` is immutable except for its
    /// `active` flag and its `successor_id` pointer: changing price, period or
    /// keeper tip requires publishing a new plan so existing mandates can never be
    /// repriced under a subscriber.
    #[contracttype]
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct Plan {
        /// Unique identifier for the plan, allocated sequentially starting from 1.
        pub id: u64,
        /// Account that publishes the plan and receives subscription payments.
        pub merchant: Address,
        /// Human-readable label shown to subscribers, e.g. "Pro Monthly".
        /// At most MAX_NAME_LEN bytes; may be empty.
        pub name: String,
        /// Short, plain summary of what the plan includes, shown to
        /// subscribers next to the name. At most MAX_DESCRIPTION_LEN bytes;
        /// may be empty.
        pub description: String,
        /// SEP-41 token this plan is denominated in.
        pub token: Address,
        /// Amount charged per period, in the token's smallest unit. This is what
        /// the subscriber pays in total; the protocol fee and the keeper tip are
        /// both taken out of it before the merchant is paid.
        pub amount: i128,
        /// Billing period in seconds.
        pub period: u64,
        /// Whether the plan accepts new subscriptions. Inactive plans cannot be subscribed to.
        pub active: bool,
        /// Merchant-selected discovery category, so subscribers can browse
        /// plans by what they are for. Soroban symbols are short and limited to
        /// the symbol alphabet, which is what makes them cheap to index; this
        /// may be empty, and it is frozen with the rest of the plan's terms.
        pub category: Symbol,
        /// Share of each charge paid to whichever account settles it, in basis
        /// points of the merchant's share. 0 means the merchant keeps it all.
        /// At most `MAX_KEEPER_BPS`; the registry rejects anything higher.
        pub keeper_bps: u32,
        /// The plan that replaces this one, if the merchant retired it. Set by
        /// `set_successor` and read through `get_plan`, so apps can point a
        /// subscriber at the replacement without a second lookup. 0 means the plan
        /// has no successor. The pointer never re-prices an open mandate: a mandate
        /// keeps the terms of the plan it was opened against.
        pub successor_id: u64,
    }
}
pub use plan::Plan;
