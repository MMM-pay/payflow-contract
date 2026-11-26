//! Data types and storage keys for the plan registry contract.

use soroban_sdk::{contracttype, Address, String, Symbol};

/// `Plan` is defined once in `payflow-shared` and used by both this contract
/// and the subscription contract that decodes it, so the two cannot drift.
///
/// Seat-based plans: `per_seat` marks a plan whose price is multiplied by the
/// number of seats chosen at subscribe time, bounded by `max_seats`. Non-seat
/// plans keep `per_seat == false` and `max_seats == 0`.
///
/// Note: the `max_pause_secs` field (0 = unlimited) is part of `Plan` in
/// `payflow-shared`; this crate re-exports that definition, so no local
/// change to the struct is required here. The subscription contract's mirror
/// of `Plan` must be updated in lockstep with `payflow-shared`.
pub use payflow_shared::{Plan, PlanChargeTerms};

/// The mutable inputs shared by `create_plan` and `create_plans`.
///
/// The merchant is supplied separately to the batch call and is the same for
/// every plan in it; everything else that defines a plan is carried here.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanParams {
    /// Token contract address this plan is denominated in.
    pub token: Address,
    /// Amount charged per billing period, in the token's smallest unit.
    pub amount: i128,
    /// Billing period in seconds. At least `MIN_PERIOD`.
    pub period: u64,
    /// Display name, at most `MAX_NAME_LEN` bytes.
    pub name: String,
    /// Short summary, at most `MAX_DESCRIPTION_LEN` bytes.
    pub description: String,
    /// Keeper tip in basis points of the merchant share. At most
    /// `MAX_KEEPER_BPS`.
    pub keeper_bps: u32,
    /// Discovery category; may be empty.
    pub category: Symbol,
    /// Link to the plan's terms, at most `MAX_TERMS_LEN` bytes.
    pub terms_uri: String,
    /// Unix seconds after which the plan stops accepting subscribers;
    /// 0 means forever.
    pub available_until: u64,
    /// Referral share in basis points of the merchant's share; 0 disables
    /// referrals for this plan.
    pub referral_bps: u32,
    /// Account that receives the merchant's share of each charge. `None`
    /// defaults to the merchant.
    pub payout: Option<Address>,
    /// Maximum number of simultaneously-open mandates; 0 means unlimited.
    pub max_subscribers: u32,
}

/// A merchant's self-declared profile, shown to subscribers alongside the
/// merchant's plans so they can tell who they are paying. The profile is
/// self-declared: the contract does not verify the name or the URL.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Profile {
    /// Account address of the merchant.
    pub merchant: Address,
    /// Display name, e.g. "Acme Corp". At most `MAX_NAME_LEN` bytes.
    pub name: String,
    /// Website or terms URL. At most `MAX_URL_LEN` bytes.
    pub url: String,
}

// Keep the historical types::DataKey path available to existing consumers.
pub use crate::storage_keys::DataKey;

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

/// One row of a plan listing (#288). Carries the settlement `token` alongside
/// the price so a catalogue mixing tokens is unambiguous without a follow-up
/// `get_plan` per row.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanListing {
    /// Plan id.
    pub id: u64,
    /// Merchant that published the plan.
    pub merchant: Address,
    /// Display name.
    pub name: String,
    /// Settlement token contract the plan is priced and charged in.
    pub token: Address,
    /// Price per period in the token's smallest unit.
    pub amount: i128,
    /// Billing period in seconds.
    pub period: u64,
    /// Whether the plan accepts new subscribers.
    pub active: bool,
}
