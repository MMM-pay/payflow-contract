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

/// Largest referral share a plan may set, in basis points of the merchant's
/// share: 20%. The referral is paid out of the merchant's share and never out
/// of the protocol fee or the keeper tip; the registry rejects anything higher.
pub const MAX_REFERRAL_BPS: u32 = 2_000;

/// Largest plan description, in bytes. A description is a short, plain
/// summary of what the plan includes ("monthly consults, unlimited
/// follow-ups"). At most `MAX_DESCRIPTION_LEN` bytes; may be empty.
pub const MAX_DESCRIPTION_LEN: u32 = 280;

/// Protocol-level minimum charge amount in the token's smallest unit.
/// Plans with an amount below this are rejected at creation to avoid
/// dust charges that cost more in fees than they transfer.
pub const MIN_CHARGE_AMOUNT: i128 = 100; // 100 stroops = 0.00001 XLM

/// First error code reserved for the plan registry contract.
pub const PLAN_REGISTRY_ERROR_BASE: u32 = 1_000;
/// First error code reserved for the subscription contract.
pub const SUBSCRIPTION_ERROR_BASE: u32 = 2_000;
/// First error code reserved for the vault contract.
pub const VAULT_ERROR_BASE: u32 = 3_000;
/// Number of codes reserved for each contract. Local error numbers must be
/// smaller than this so no two contracts can ever return the same code.
pub const ERROR_RANGE_SIZE: u32 = 1_000;

const _: () = assert!(PLAN_REGISTRY_ERROR_BASE + ERROR_RANGE_SIZE <= SUBSCRIPTION_ERROR_BASE);
const _: () = assert!(SUBSCRIPTION_ERROR_BASE + ERROR_RANGE_SIZE <= VAULT_ERROR_BASE);

/// A Payflow contract family, used to interpret the absolute error codes
/// returned by the contracts.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ErrorDomain {
    /// The plan registry contract.
    PlanRegistry,
    /// The subscription contract.
    Subscription,
    /// The vault contract.
    Vault,
}

impl ErrorDomain {
    /// First code reserved for this contract.
    pub const fn base(self) -> u32 {
        match self {
            Self::PlanRegistry => PLAN_REGISTRY_ERROR_BASE,
            Self::Subscription => SUBSCRIPTION_ERROR_BASE,
            Self::Vault => VAULT_ERROR_BASE,
        }
    }

    /// Absolute code for a contract-local error number.
    ///
    /// Local numbers start at 1, so a contract's first error is
    /// `base() + 1`. Values at or above [`ERROR_RANGE_SIZE`] fall outside the
    /// contract's reserved range; callers constructing codes should keep local
    /// numbers below that bound.
    pub const fn code(self, local: u32) -> u32 {
        self.base() + local
    }

    /// The contract that owns `code`, or `None` when the code is outside every
    /// reserved range.
    pub const fn from_code(code: u32) -> Option<Self> {
        if code >= PLAN_REGISTRY_ERROR_BASE && code < SUBSCRIPTION_ERROR_BASE {
            Some(Self::PlanRegistry)
        } else if code >= SUBSCRIPTION_ERROR_BASE && code < VAULT_ERROR_BASE {
            Some(Self::Subscription)
        } else if code >= VAULT_ERROR_BASE && code < VAULT_ERROR_BASE + ERROR_RANGE_SIZE {
            Some(Self::Vault)
        } else {
            None
        }
    }

    /// The local error number encoded by `code` when this contract owns it.
    pub const fn local(self, code: u32) -> Option<u32> {
        if code >= self.base() && code < self.base() + ERROR_RANGE_SIZE {
            Some(code - self.base())
        } else {
            None
        }
    }
}

/// A contract error code together with the contract family that owns it.
///
/// The contracts return these absolute codes as their `contracterror`
/// discriminants, so a client can decode any code without knowing which
/// contract produced it:
///
/// ```
/// use payflow_shared::{ErrorCode, ErrorDomain};
///
/// let code = ErrorCode::from_local(ErrorDomain::Vault, 4);
/// assert_eq!(code.code(), 3_004);
/// assert_eq!(code.domain(), Some(ErrorDomain::Vault));
/// assert_eq!(code.local(), Some(4));
/// ```
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ErrorCode(u32);

impl ErrorCode {
    /// Wrap an absolute code already returned by a contract.
    pub const fn new(code: u32) -> Self {
        Self(code)
    }

    /// Build an absolute code from a contract family and a local error number.
    pub const fn from_local(domain: ErrorDomain, local: u32) -> Self {
        Self(domain.code(local))
    }

    /// The absolute numeric code.
    pub const fn code(self) -> u32 {
        self.0
    }

    /// The contract family that owns this code, if it is in a reserved range.
    pub const fn domain(self) -> Option<ErrorDomain> {
        ErrorDomain::from_code(self.0)
    }

    /// The local error number within the owning contract's range, if any.
    pub const fn local(self) -> Option<u32> {
        match self.domain() {
            Some(domain) => domain.local(self.0),
            None => None,
        }
    }
}

impl From<u32> for ErrorCode {
    fn from(code: u32) -> Self {
        Self::new(code)
    }
}

impl From<ErrorCode> for u32 {
    fn from(code: ErrorCode) -> Self {
        code.code()
    }
}

#[allow(missing_docs)]
mod plan {
    use super::{contracttype, Address, String, Symbol};

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
        /// At most `MAX_NAME_LEN` bytes; may be empty.
        pub name: String,
        /// Short, plain summary of what the plan includes, shown to
        /// subscribers next to the name. At most `MAX_DESCRIPTION_LEN` bytes;
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
        /// Link to the plan's description and terms of service, shown to
        /// subscribers before they open a mandate. At most `MAX_TERMS_LEN`
        /// (200) bytes; may be empty. Self-declared by the merchant, like the
        /// profile URL: the contract does not verify it.
        pub terms_uri: String,
        /// Account that actually receives the merchant's share of each charge.
        /// Frozen into every mandate at sign-up, so a later payout change or a
        /// plan-ownership transfer never redirects an open mandate's funds.
        pub payout: Address,
        /// Referral share paid to a mandate's referrer, in basis points of the
        /// merchant's share (out of the merchant's own money, never the fee or
        /// tip). 0 disables referrals. At most `MAX_REFERRAL_BPS`.
        pub referral_bps: u32,
        /// Unix seconds after which the plan stops accepting new subscribers;
        /// 0 means it is available forever. Only new subscriptions are
        /// affected: mandates opened before the cutoff keep billing. The
        /// subscription contract enforces the cutoff in `subscribe`.
        pub available_until: u64,
        /// Maximum number of simultaneously-open mandates the plan accepts; 0
        /// means unlimited. Frozen into the plan, checked on each new sign-up.
        pub max_subscribers: u32,
        /// Number of ledgers after a charge falls due during which a failed
        /// charge may be retried before the mandate is marked failed (#343).
        /// Frozen into each mandate at sign-up. `None` keeps the historical
        /// behaviour: a failure can be flagged as soon as the charge is due.
        pub retry_window_ledgers: Option<u32>,
    }
}
pub use plan::Plan;

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn error_ranges_are_disjoint_and_cover_every_local_code() {
        for domain in [
            ErrorDomain::PlanRegistry,
            ErrorDomain::Subscription,
            ErrorDomain::Vault,
        ] {
            for local in 0..ERROR_RANGE_SIZE {
                let code = ErrorCode::from_local(domain, local);
                assert_eq!(code.code(), domain.base() + local);
                assert_eq!(code.domain(), Some(domain));
                assert_eq!(code.local(), Some(local));
                assert_eq!(ErrorDomain::from_code(code.code()), Some(domain));
            }
        }
    }

    #[test]
    fn codes_outside_the_reserved_ranges_have_no_domain() {
        for code in [
            0,
            1,
            PLAN_REGISTRY_ERROR_BASE - 1,
            VAULT_ERROR_BASE + ERROR_RANGE_SIZE,
        ] {
            let error = ErrorCode::new(code);
            assert_eq!(error.domain(), None);
            assert_eq!(error.local(), None);
        }
    }

    #[test]
    fn error_code_converts_to_and_from_the_raw_u32() {
        let code = ErrorCode::from(SUBSCRIPTION_ERROR_BASE + 7);
        assert_eq!(u32::from(code), SUBSCRIPTION_ERROR_BASE + 7);
        assert_eq!(code.local(), Some(7));
    }
}
