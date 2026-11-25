//! Plan registry contract for Payflow subscriptions.
//!
//! Stores merchant plans and profiles. Plans are immutable templates defining
//! pricing, billing periods, and keeper incentives.
//!
//! ## Resource costs
//!
//! All storage TTLs are expressed in ledgers (~5s each). Instance storage
//! (configuration) uses a 30-day bump with a 29-day threshold. Persistent
//! storage (plans, indexes) uses a 90-day bump with an 89-day threshold.
//!
//! | Entry point | Instance reads | Instance writes | Persistent reads | Persistent writes | Notes |
//! |---|---|---|---|---|---|
//! | `__constructor` | 0 | 2 (Admin, NextPlanId) | 0 | 0 | Extends instance TTL |
//! | `create_plan` | 1 (NextPlanId) | 1 (NextPlanId) | 0 | 4–6 (Plan, 2 index slots, 2 counts) | Extends instance + persistent TTLs |
//! | `create_plans` | 1 (NextPlanId) | 1 (NextPlanId) | 0 | 4–6 per plan | Batch ≤10; extends instance + persistent TTLs |
//! | `set_plan_name` | 0 | 0 | 1 (Plan) | 1 (Plan) | Extends persistent TTL |
//! | `set_plan_active` | 0 | 0 | 1 (Plan) | 1 (Plan) | Extends persistent TTL |
//! | `set_plan_charges_paused` | 0 | 0 | 1 (Plan) | 1 (PlanChargesPaused) | Extends persistent TTL |
//! | `set_plan_grace_period` | 0 | 0 | 1 (Plan) | 1 (PlanGracePeriod) | Extends persistent TTL |
//! | `set_successor` | 0 | 0 | 2 (Plan ×2) | 1 (Plan) | Extends persistent TTL |
//! | `set_profile` | 0 | 0 | 0 | 1 (Profile) | Extends persistent TTL |
//! | `profile` | 0 | 0 | 1 (Profile) | 0 | |
//! | `deactivate_page` | 0 | 0 | ≤100 (index slots + Plans) | ≤50 (Plans) | Page size ≤50, extends persistent TTLs |
//! | `get_plan` | 0 | 0 | 1 (Plan) | 0 | |
//! | `plan_charges_paused` | 0 | 0 | 2 (Plan, PlanChargesPaused) | 0 | |
//! | `plan_grace_period` | 0 | 0 | 2 (Plan, PlanGracePeriod) | 0 | |
//! | `merchant_plan_count` | 0 | 0 | 1 (count) | 0 | |
//! | `merchant_plans` | 0 | 0 | ≤50 (index slots) | 0 | Page size ≤50 |
//! | `token_plan_count` | 0 | 0 | 1 (count) | 0 | |
//! | `token_plans` | 0 | 0 | ≤50 (index slots) | 0 | Page size ≤50 |
//! | `merchant_plan_listings` | 0 | 0 | ≤100 (index slots + Plans) | 0 | Page size ≤50 |
//! | `token_plan_listings` | 0 | 0 | ≤100 (index slots + Plans) | 0 | Page size ≤50 |
//! | `next_plan_id` | 1 (NextPlanId) | 0 | 0 | 0 | |
//! | `update_admin` | 1 (Admin) | 1 (Admin) | 0 | 0 | Extends instance TTL |
//! | `admin` | 1 (Admin) | 0 | 0 | 0 | |
//! | `upgrade_wasm` | 1 (Admin) | 0 | 0 | 0 | Extends instance TTL |
//! | `propose_plan_owner` | 0 | 0 | 1 (Plan) | 1 (Proposal) | Extends persistent TTL |
//! | `accept_plan_owner` | 0 | 0 | 2 (Proposal, Plan) | 2 (Plan, index slots) | Extends persistent TTLs |

#![no_std]
#![deny(missing_docs)]
// `deny`, not `forbid`: soroban-sdk's #[contractimpl] expansion carries its
// own #[allow(unsafe_code)], which `forbid` cannot be overridden by. `deny`
// still fails the build on any unsafe block written in this crate.
#![deny(unsafe_code)]

#[allow(missing_docs)]
mod error;
#[allow(missing_docs)]
mod events;
#[allow(missing_docs)]
mod types;

#[cfg(test)]
mod test;
#[cfg(test)]
mod test_errors;
#[cfg(test)]
mod test_events;

pub use error::Error;
pub use events::{
    ContractUpgraded, PlanChargesPausedChanged, PlanCreated, PlanGracePeriodSet,
    PlanOwnershipProposed, PlanRenamed, PlanStatusChanged, PlanSuccessorSet, PlanTransferred,
    PlansCreated, ProfileUpdated,
};
pub use types::{DataKey, Plan, PlanListing, PlanParams, Profile, UpgradeRecord};

use payflow_shared::{MAX_KEEPER_BPS, MAX_REFERRAL_BPS};
use soroban_sdk::{
    contract, contractimpl, Address, BytesN, ContractExecutable, Env, String, Symbol, Vec,
};

const DAY_IN_LEDGERS: u32 = 17_280;
const INSTANCE_BUMP: u32 = 30 * DAY_IN_LEDGERS;
const INSTANCE_THRESHOLD: u32 = INSTANCE_BUMP - DAY_IN_LEDGERS;
const PERSIST_BUMP: u32 = 90 * DAY_IN_LEDGERS;
const PERSIST_THRESHOLD: u32 = PERSIST_BUMP - DAY_IN_LEDGERS;

/// Minimum billing period. Guards against a merchant publishing a
/// one-second plan and draining a mandate through rapid repeat charges.
pub const MIN_PERIOD: u64 = 60;

/// Maximum plan name length in bytes. Bounded so a merchant cannot bloat
/// persistent storage or break list rendering with an unbounded label.
pub const MAX_NAME_LEN: u32 = 64;

/// Maximum plan description length in bytes. Bounded so a merchant cannot
/// bloat persistent storage or break list rendering with a long blurb.
pub const MAX_DESCRIPTION_LEN: u32 = payflow_shared::MAX_DESCRIPTION_LEN;

/// Maximum profile URL length in bytes. Self-declared and unbounded in theory,
/// capped here so storage and UI rendering stay bounded.
pub const MAX_URL_LEN: u32 = 256;

/// Maximum plan terms URI length in bytes. Like the profile URL it is
/// self-declared, capped so one plan cannot bloat persistent storage.
pub const MAX_TERMS_LEN: u32 = 200;

/// Largest page `merchant_plans` returns in one call. Each id is read from its
/// own storage entry, and a transaction may touch at most 100 entries.
pub const MAX_PAGE: u32 = 50;

/// Largest number of plans `create_plans` accepts in one call. Each plan
/// writes several persistent entries, so the batch is bounded to keep a single
/// transaction inside Soroban's resource limits.
pub const MAX_BATCH_PLANS: u32 = 10;

/// Protocol-level minimum charge amount in the token's smallest unit.
/// Plans with an amount below this are rejected at creation to avoid
/// dust charges that cost more in fees than they transfer.
pub const MIN_CHARGE_AMOUNT: i128 = payflow_shared::MIN_CHARGE_AMOUNT;

#[allow(missing_docs)]
mod contract {
    use super::{
        contract, contractimpl, Address, BytesN, ContractExecutable, ContractUpgraded, DataKey,
        Env, Error, Plan, PlanChargesPausedChanged, PlanCreated, PlanGracePeriodSet, PlanListing,
        PlanOwnershipProposed, PlanParams, PlanRenamed, PlanStatusChanged, PlanSuccessorSet,
        PlanTransferred, PlansCreated, Profile, ProfileUpdated, String, Symbol, UpgradeRecord, Vec,
        INSTANCE_BUMP, INSTANCE_THRESHOLD, MAX_BATCH_PLANS, MAX_DESCRIPTION_LEN, MAX_KEEPER_BPS,
        MAX_NAME_LEN, MAX_PAGE, MAX_REFERRAL_BPS, MAX_TERMS_LEN, MAX_URL_LEN, MIN_CHARGE_AMOUNT,
        MIN_PERIOD, PERSIST_BUMP, PERSIST_THRESHOLD,
    };

    /// Contract implementation for publishing and managing subscription plans.
    #[contract]
    pub struct PlanRegistry;

    #[contractimpl]
    #[allow(clippy::too_many_arguments)]
    impl PlanRegistry {
        /// The version of this contract's crate.
        pub fn version(env: Env) -> String {
            String::from_str(&env, env!("CARGO_PKG_VERSION"))
        }

        /// Runs once, in the same transaction that deploys the contract. With a
        /// separate `initialize` call, anyone watching the ledger could call it
        /// between the deploy and the owner's own call and make themselves admin.
        pub fn __constructor(env: Env, admin: Address) {
            env.storage().instance().set(&DataKey::Admin, &admin);
            env.storage().instance().set(&DataKey::NextPlanId, &1u64);
            env.storage()
                .instance()
                .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_BUMP);
        }

        /// Publish a new plan. Only the merchant may publish plans in its own name.
        ///
        /// `keeper_bps` is the share of each charge paid to whoever settles it,
        /// taken out of the merchant's share, capped at `MAX_KEEPER_BPS`. It is
        /// part of the plan's immutable terms: a mandate opened against this plan
        /// keeps the tip it was opened with, so raising the tip later cannot take
        /// money from an existing subscriber or merchant.
        ///
        /// `terms_uri` links to the plan's description and terms of service, so a
        /// subscriber can read what they are agreeing to before opening a
        /// mandate. It may be empty, is capped at `MAX_TERMS_LEN` bytes, and is
        /// self-declared by the merchant: the contract does not fetch or verify it.
        ///
        /// # Auth
        /// Requires authorization from `merchant`.
        ///
        /// # Events
        /// Emits [`PlanCreated`].
        ///
        /// # Errors
        /// Returns [`Error::InvalidAmount`] if `amount <= 0`, [`Error::AmountTooLow`]
        /// if `amount < MIN_CHARGE_AMOUNT`, [`Error::NameTooLong`]
        /// if `name` exceeds [`MAX_NAME_LEN`], [`Error::DescriptionTooLong`] if `description`
        /// exceeds [`MAX_DESCRIPTION_LEN`], [`Error::InvalidPeriod`] if `period < MIN_PERIOD`,
        /// [`Error::KeeperBpsTooHigh`] if `keeper_bps > MAX_KEEPER_BPS`,
        /// [`Error::UriTooLong`] if `terms_uri` exceeds [`MAX_TERMS_LEN`],
        /// [`Error::NotInitialized`] if the contract has not been initialized, or
        /// [`Error::PlanIdOverflow`] if the next plan ID would exceed `u64`.
        #[allow(clippy::too_many_arguments)]
        pub fn create_plan(
            env: Env,
            merchant: Address,
            token: Address,
            amount: i128,
            period: u64,
            name: String,
            description: String,
            keeper_bps: u32,
            category: Symbol,
            terms_uri: String,
            available_until: u64,
            referral_bps: u32,
            payout: Option<Address>,
            max_subscribers: u32,
        ) -> Result<u64, Error> {
            merchant.require_auth();

            let params = PlanParams {
                token,
                amount,
                period,
                name,
                description,
                keeper_bps,
                category,
                terms_uri,
                available_until,
                referral_bps,
                payout,
                max_subscribers,
            };
            Self::validate_plan(&params)?;

            let id = Self::read_next_plan_id(&env)?;
            let next_id = id.checked_add(1).ok_or(Error::PlanIdOverflow)?;
            Self::store_plan(&env, &merchant, id, &params);
            Self::set_next_plan_id(&env, next_id);

            Ok(id)
        }

        /// Publish several plans in one transaction.
        ///
        /// Every plan is validated before any is written, so a batch that
        /// contains one invalid plan creates none of them. IDs are allocated
        /// sequentially in request order, exactly as if each plan had been
        /// published with `create_plan`.
        ///
        /// The batch is capped at [`MAX_BATCH_PLANS`] plans to stay inside
        /// Soroban's per-transaction resource limits.
        ///
        /// # Auth
        /// Requires authorization from `merchant`. The same merchant owns every
        /// plan in the batch.
        ///
        /// # Events
        /// Emits one [`PlanCreated`] per plan, followed by one [`PlansCreated`]
        /// summarising the batch.
        ///
        /// # Errors
        /// Returns [`Error::EmptyBatch`] if `plans` is empty,
        /// [`Error::BatchTooLarge`] if it holds more than [`MAX_BATCH_PLANS`]
        /// plans, [`Error::PlanIdOverflow`] if the batch would carry the plan
        /// ID counter past `u64::MAX`, and otherwise the same validation errors
        /// as `create_plan` for the first invalid plan.
        pub fn create_plans(
            env: Env,
            merchant: Address,
            plans: Vec<PlanParams>,
        ) -> Result<Vec<u64>, Error> {
            merchant.require_auth();

            if plans.is_empty() {
                return Err(Error::EmptyBatch);
            }
            if plans.len() > MAX_BATCH_PLANS {
                return Err(Error::BatchTooLarge);
            }

            for i in 0..plans.len() {
                Self::validate_plan(&plans.get(i).unwrap())?;
            }

            let first_id = Self::read_next_plan_id(&env)?;
            // Reserve the whole range before writing any plan, so a batch that
            // would carry the counter past `u64::MAX` writes nothing at all.
            let next_id = first_id
                .checked_add(u64::from(plans.len()))
                .ok_or(Error::PlanIdOverflow)?;
            let mut plan_ids = Vec::new(&env);
            for i in 0..plans.len() {
                let id = first_id
                    .checked_add(u64::from(i))
                    .ok_or(Error::PlanIdOverflow)?;
                Self::store_plan(&env, &merchant, id, &plans.get(i).unwrap());
                plan_ids.push_back(id);
            }
            Self::set_next_plan_id(&env, next_id);

            PlansCreated {
                merchant,
                plan_ids: plan_ids.clone(),
            }
            .publish(&env);

            Ok(plan_ids)
        }

        /// Rename a plan. Only the plan's own merchant may do this, and only
        /// the label changes: price, period, token and status are frozen once
        /// a plan is published, because subscribers' mandates are priced from
        /// them.
        ///
        /// # Errors
        /// [`Error::PlanNameTooLong`] if `name` exceeds [`MAX_NAME_LEN`],
        /// [`Error::PlanNotFound`] if no such plan exists, and
        /// [`Error::NotPlanOwner`] if `merchant` does not own it.
        ///
        /// # Auth
        /// Requires authorization from `merchant`.
        ///
        /// # Events
        /// Emits [`PlanRenamed`].
        pub fn set_plan_name(
            env: Env,
            merchant: Address,
            plan_id: u64,
            name: String,
        ) -> Result<(), Error> {
            merchant.require_auth();

            if name.len() > MAX_NAME_LEN {
                return Err(Error::PlanNameTooLong);
            }

            let key = DataKey::Plan(plan_id);
            let mut plan: Plan = Self::read_plan(&env, plan_id).ok_or(Error::PlanNotFound)?;

            if plan.merchant != merchant {
                return Err(Error::NotPlanOwner);
            }

            plan.name = name.clone();
            env.storage().persistent().set(&key, &plan);
            env.storage()
                .persistent()
                .extend_ttl(&key, PERSIST_THRESHOLD, PERSIST_BUMP);

            PlanRenamed {
                merchant,
                plan_id,
                name,
            }
            .publish(&env);

            Ok(())
        }

        /// Activate or deactivate a plan. Deactivating stops new subscriptions but
        /// does not cancel mandates already open against the plan.
        ///
        /// # Auth
        /// Requires authorization from `merchant`.
        ///
        /// # Events
        /// Emits [`PlanStatusChanged`].
        ///
        /// # Errors
        /// Returns [`Error::PlanNotFound`] if the plan does not exist, or [`Error::NotPlanOwner`]
        /// if the caller does not own the plan.
        pub fn set_plan_active(
            env: Env,
            merchant: Address,
            plan_id: u64,
            active: bool,
        ) -> Result<(), Error> {
            merchant.require_auth();

            let mut plan: Plan = Self::read_plan(&env, plan_id).ok_or(Error::PlanNotFound)?;

            if plan.merchant != merchant {
                return Err(Error::NotPlanOwner);
            }

            plan.active = active;
            env.storage()
                .persistent()
                .set(&DataKey::Plan(plan_id), &plan);
            env.storage().persistent().extend_ttl(
                &DataKey::Plan(plan_id),
                PERSIST_THRESHOLD,
                PERSIST_BUMP,
            );

            PlanStatusChanged {
                merchant,
                plan_id,
                active,
            }
            .publish(&env);

            Ok(())
        }

        /// Pause or resume charging for every mandate against one plan (#315).
        ///
        /// Pausing does not deactivate the plan: it keeps accepting new
        /// subscribers, and its open mandates are not cancelled or moved. Both
        /// `charge` and `flag_failed_charge` reject a mandate whose plan is
        /// paused, so no charge settles until the merchant resumes. This lets a
        /// merchant stop billing during a dispute without forcing subscribers
        /// to re-subscribe afterwards.
        ///
        /// # Auth
        /// Requires authorization from the plan's merchant.
        ///
        /// # Events
        /// Emits [`PlanChargesPausedChanged`].
        ///
        /// # Errors
        /// Returns [`Error::PlanNotFound`] if the plan does not exist, or
        /// [`Error::NotPlanOwner`] if `merchant` does not own it.
        pub fn set_plan_charges_paused(
            env: Env,
            merchant: Address,
            plan_id: u64,
            paused: bool,
        ) -> Result<(), Error> {
            merchant.require_auth();

            let plan = Self::get_plan(env.clone(), plan_id)?;
            if plan.merchant != merchant {
                return Err(Error::NotPlanOwner);
            }

            env.storage().persistent().extend_ttl(
                &DataKey::Plan(plan_id),
                PERSIST_THRESHOLD,
                PERSIST_BUMP,
            );
            let key = DataKey::PlanChargesPaused(plan_id);
            env.storage().persistent().set(&key, &paused);
            env.storage()
                .persistent()
                .extend_ttl(&key, PERSIST_THRESHOLD, PERSIST_BUMP);

            PlanChargesPausedChanged {
                merchant,
                plan_id,
                paused,
            }
            .publish(&env);

            Ok(())
        }

        /// Alias for [`PlanRegistry::set_plan_charges_paused`].
        pub fn set_plan_paused(
            env: Env,
            merchant: Address,
            plan_id: u64,
            paused: bool,
        ) -> Result<(), Error> {
            Self::set_plan_charges_paused(env, merchant, plan_id, paused)
        }

        /// Set a plan's grace window in seconds (#312). The window is copied
        /// into each new mandate at sign-up, so a later change affects only
        /// subscriptions opened after it. `0` disables the grace view.
        ///
        /// The window is only a warning period: subscription's
        /// `is_in_grace_period` reports a mandate as recoverable between its
        /// due time and the end of the frozen window. It does not cancel or
        /// otherwise change the mandate.
        ///
        /// # Auth
        /// Requires authorization from the plan's merchant.
        ///
        /// # Events
        /// Emits [`PlanGracePeriodSet`].
        ///
        /// # Errors
        /// Returns [`Error::PlanNotFound`] if the plan does not exist, or
        /// [`Error::NotPlanOwner`] if `merchant` does not own it.
        pub fn set_plan_grace_period(
            env: Env,
            merchant: Address,
            plan_id: u64,
            grace_period_secs: u64,
        ) -> Result<(), Error> {
            merchant.require_auth();

            let plan = Self::get_plan(env.clone(), plan_id)?;
            if plan.merchant != merchant {
                return Err(Error::NotPlanOwner);
            }

            env.storage().persistent().extend_ttl(
                &DataKey::Plan(plan_id),
                PERSIST_THRESHOLD,
                PERSIST_BUMP,
            );
            let key = DataKey::PlanGracePeriod(plan_id);
            env.storage().persistent().set(&key, &grace_period_secs);
            env.storage()
                .persistent()
                .extend_ttl(&key, PERSIST_THRESHOLD, PERSIST_BUMP);

            PlanGracePeriodSet {
                merchant,
                plan_id,
                grace_period_secs,
            }
            .publish(&env);

            Ok(())
        }

        /// Whether charging is paused for `plan_id` (#315).
        ///
        /// # Errors
        /// Returns [`Error::PlanNotFound`] if the plan does not exist.
        pub fn plan_charges_paused(env: Env, plan_id: u64) -> Result<bool, Error> {
            let _ = Self::get_plan(env.clone(), plan_id)?;
            Ok(env
                .storage()
                .persistent()
                .get(&DataKey::PlanChargesPaused(plan_id))
                .unwrap_or(false))
        }

        /// Alias for [`PlanRegistry::plan_charges_paused`].
        pub fn plan_paused(env: Env, plan_id: u64) -> Result<bool, Error> {
            Self::plan_charges_paused(env, plan_id)
        }

        /// Grace window in seconds for new mandates against `plan_id` (#312).
        /// Existing mandates keep the window frozen into them at sign-up.
        /// Defaults to `0`, which disables the grace view.
        ///
        /// # Errors
        /// Returns [`Error::PlanNotFound`] if the plan does not exist.
        pub fn plan_grace_period(env: Env, plan_id: u64) -> Result<u64, Error> {
            let _ = Self::get_plan(env.clone(), plan_id)?;
            Ok(env
                .storage()
                .persistent()
                .get(&DataKey::PlanGracePeriod(plan_id))
                .unwrap_or(0))
        }

        /// Point a plan at the plan that replaces it, if any.
        ///
        /// A merchant that replaces a plan retires the old one and opens a new one
        /// carrying the new terms, so subscribers of the old plan need an on-chain
        /// pointer to the new one to be able to be offered the replacement. The
        /// pointer is stored on the retiring plan itself (`Plan.successor_id`, 0
        /// when there is none) rather than in a side key, so `get_plan` hands back
        /// the answer in the same read that returns the plan, and apps never need a
        /// second lookup. Because the pointer lives on the old plan it never
        /// touches an open mandate's frozen terms.
        ///
        /// The retiring plan may be re-pointed, which is how a merchant that
        /// replaces a replacement retires the middle plan. Both plans must belong
        /// to `merchant` and be denominated in the same token; a replacement from
        /// another merchant's catalogue or another asset would be meaningless, and
        /// a plan cannot be its own successor.
        ///
        /// # Auth
        /// Requires authorization from `merchant`.
        ///
        /// # Events
        /// Emits [`PlanSuccessorSet`].
        ///
        /// # Errors
        /// Returns [`Error::PlanNotFound`] if the retiring plan does not exist,
        /// [`Error::NotPlanOwner`] if `merchant` does not own either plan,
        /// [`Error::SelfSuccessor`] if `successor_id == plan_id`,
        /// [`Error::SuccessorNotFound`] if the replacement plan does not exist, or
        /// [`Error::SuccessorTokenMismatch`] if the plans use different tokens.
        pub fn set_successor(
            env: Env,
            merchant: Address,
            plan_id: u64,
            successor_id: u64,
        ) -> Result<(), Error> {
            merchant.require_auth();

            let plan_key = DataKey::Plan(plan_id);
            let mut plan: Plan = Self::read_plan(&env, plan_id).ok_or(Error::PlanNotFound)?;

            if plan.merchant != merchant {
                return Err(Error::NotPlanOwner);
            }
            if successor_id == plan_id {
                return Err(Error::SelfSuccessor);
            }

            let successor: Plan =
                Self::read_plan(&env, successor_id).ok_or(Error::SuccessorNotFound)?;

            if successor.merchant != merchant {
                return Err(Error::NotPlanOwner);
            }
            if successor.token != plan.token {
                return Err(Error::SuccessorTokenMismatch);
            }

            plan.successor_id = successor_id;
            env.storage().persistent().set(&plan_key, &plan);
            env.storage()
                .persistent()
                .extend_ttl(&plan_key, PERSIST_THRESHOLD, PERSIST_BUMP);

            PlanSuccessorSet {
                merchant,
                plan_id,
                successor_id,
            }
            .publish(&env);

            Ok(())
        }

        /// Set or replace `merchant`'s self-declared profile: a display name and a
        /// URL (website or terms). The profile is not verified by the contract;
        /// subscribers should treat it as a claim the merchant makes about itself.
        /// Only the merchant may set its own profile.
        ///
        /// # Auth
        /// Requires authorization from `merchant`.
        ///
        /// # Events
        /// Emits [`ProfileUpdated`].
        ///
        /// # Errors
        /// Returns [`Error::ProfileNameTooLong`] if `name` exceeds [`MAX_NAME_LEN`], or
        /// [`Error::ProfileUrlTooLong`] if `url` exceeds [`MAX_URL_LEN`].
        pub fn set_profile(
            env: Env,
            merchant: Address,
            name: String,
            url: String,
        ) -> Result<(), Error> {
            merchant.require_auth();

            if name.len() > MAX_NAME_LEN {
                return Err(Error::ProfileNameTooLong);
            }
            if url.len() > MAX_URL_LEN {
                return Err(Error::ProfileUrlTooLong);
            }

            let profile = Profile {
                merchant: merchant.clone(),
                name: name.clone(),
                url: url.clone(),
            };
            env.storage()
                .persistent()
                .set(&DataKey::Profile(merchant.clone()), &profile);
            env.storage().persistent().extend_ttl(
                &DataKey::Profile(merchant.clone()),
                PERSIST_THRESHOLD,
                PERSIST_BUMP,
            );

            ProfileUpdated {
                merchant,
                name,
                url,
            }
            .publish(&env);

            Ok(())
        }

        /// `merchant`'s self-declared profile, if one has been set.
        pub fn profile(env: Env, merchant: Address) -> Result<Profile, Error> {
            env.storage()
                .persistent()
                .get(&DataKey::Profile(merchant))
                .ok_or(Error::ProfileNotFound)
        }

        /// Deactivate up to `limit` of `merchant`'s plans, starting at position
        /// `start` of its plan index. Only plans in `merchant`'s own index are
        /// touched, so a merchant can never deactivate another merchant's plans.
        /// Already-inactive plans are skipped. Returns how many plans were
        /// deactivated in this page. At most `min(limit, MAX_PAGE)` plans are
        /// touched, so the call stays within the ledger entry footprint budget.
        ///
        /// # Auth
        /// Requires authorization from `merchant`.
        ///
        /// # Events
        /// Emits [`PlanStatusChanged`] for each plan deactivated.
        ///
        /// # Errors
        /// Returns [`Error::PlanNotFound`] if an index slot points to a missing plan.
        pub fn deactivate_page(
            env: Env,
            merchant: Address,
            start: u32,
            limit: u32,
        ) -> Result<u32, Error> {
            merchant.require_auth();

            let len = Self::merchant_plan_count(env.clone(), merchant.clone());
            let end = start.saturating_add(limit.min(MAX_PAGE)).min(len);
            let mut deactivated: u32 = 0;
            let p = env.storage().persistent();
            for i in start..end {
                let slot_key = DataKey::MerchantPlan(merchant.clone(), i);
                let Some(plan_id) = p.get::<_, u64>(&slot_key) else {
                    continue;
                };

                let plan_key = DataKey::Plan(plan_id);
                let mut plan: Plan = Self::read_plan(&env, plan_id).ok_or(Error::PlanNotFound)?;
                if plan.merchant != merchant {
                    // Defensive: the per-merchant index only lists its own plans.
                    continue;
                }
                if !plan.active {
                    continue;
                }

                plan.active = false;
                p.set(&plan_key, &plan);
                p.extend_ttl(&plan_key, PERSIST_THRESHOLD, PERSIST_BUMP);

                PlanStatusChanged {
                    merchant: merchant.clone(),
                    plan_id,
                    active: false,
                }
                .publish(&env);

                deactivated += 1;
            }
            Ok(deactivated)
        }

        /// Look up a published plan by its unique ID.
        ///
        /// Reading a plan also refreshes its TTL (#247). A plan that is used by
        /// every charge but never edited is still a hot key; leaving its rent
        /// unchanged until the next write would let it expire while mandates
        /// are open against it. The bump is conditional: an entry already past
        /// the threshold keeps its TTL exactly as it was.
        pub fn get_plan(env: Env, plan_id: u64) -> Result<Plan, Error> {
            Self::read_plan(&env, plan_id).ok_or(Error::PlanNotFound)
        }

        /// How many plans `merchant` has published, active or not.
        pub fn merchant_plan_count(env: Env, merchant: Address) -> u32 {
            env.storage()
                .persistent()
                .get(&DataKey::MerchantPlanCount(merchant))
                .unwrap_or(0)
        }

        /// Ids of the plans `merchant` published, oldest first, starting at
        /// position `start`. Returns at most `min(limit, MAX_PAGE)` ids.
        ///
        /// This uses a per-merchant index (`MerchantPlanCount` + `MerchantPlan`)
        /// where each plan occupies a fixed position. Reading a page touches
        /// exactly `min(limit, MAX_PAGE)` storage entries — one per plan — so
        /// listing a merchant's plans is O(1) per entry and does not scan
        /// unrelated merchants' plans.
        pub fn merchant_plans(env: Env, merchant: Address, start: u32, limit: u32) -> Vec<u64> {
            let len = Self::merchant_plan_count(env.clone(), merchant.clone());
            let end = start.saturating_add(limit.min(MAX_PAGE)).min(len);
            let mut out = Vec::new(&env);
            for i in start..end {
                if let Some(id) = env
                    .storage()
                    .persistent()
                    .get::<_, u64>(&DataKey::MerchantPlan(merchant.clone(), i))
                {
                    out.push_back(id);
                }
            }
            out
        }

        /// How many plans are priced in `token`, active or not.
        pub fn token_plan_count(env: Env, token: Address) -> u32 {
            env.storage()
                .persistent()
                .get(&DataKey::TokenPlanCount(token))
                .unwrap_or(0)
        }

        /// Ids of the plans priced in `token`, oldest first, starting at
        /// position `start`. Returns at most `min(limit, MAX_PAGE)` ids.
        pub fn token_plans(env: Env, token: Address, start: u32, limit: u32) -> Vec<u64> {
            let len = Self::token_plan_count(env.clone(), token.clone());
            let end = start.saturating_add(limit.min(MAX_PAGE)).min(len);
            let mut out = Vec::new(&env);
            for i in start..end {
                if let Some(id) = env
                    .storage()
                    .persistent()
                    .get::<_, u64>(&DataKey::TokenPlan(token.clone(), i))
                {
                    out.push_back(id);
                }
            }
            out
        }

        /// Listing rows for the plans `merchant` published, in the same order
        /// and paging as [`merchant_plans`](Self::merchant_plans). Each row
        /// names the plan's settlement token (#288).
        pub fn merchant_plan_listings(
            env: Env,
            merchant: Address,
            start: u32,
            limit: u32,
        ) -> Vec<PlanListing> {
            let ids = Self::merchant_plans(env.clone(), merchant, start, limit);
            Self::listings(&env, ids)
        }

        /// Listing rows for the plans priced in `token`, in the same order and
        /// paging as [`token_plans`](Self::token_plans) (#288).
        pub fn token_plan_listings(
            env: Env,
            token: Address,
            start: u32,
            limit: u32,
        ) -> Vec<PlanListing> {
            let ids = Self::token_plans(env.clone(), token, start, limit);
            Self::listings(&env, ids)
        }

        /// The id the next published plan will get. Plans are numbered from 1, so
        /// `next_plan_id() - 1` is the number of plans published so far.
        pub fn next_plan_id(env: Env) -> Result<u64, Error> {
            env.storage()
                .instance()
                .get(&DataKey::NextPlanId)
                .ok_or(Error::NotInitialized)
        }

        /// Hand the admin role to another address. Only the current admin may
        /// call this, and the new admin takes effect immediately: the old one
        /// keeps no authority. A deployment whose admin key is lost or
        /// rotated is otherwise stuck with it for the life of the contract.
        ///
        /// # Auth
        /// Requires authorization from the current admin.
        pub fn update_admin(env: Env, new_admin: Address) -> Result<(), Error> {
            let current: Address = Self::admin(env.clone())?;
            current.require_auth();

            env.storage().instance().set(&DataKey::Admin, &new_admin);
            env.storage()
                .instance()
                .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_BUMP);

            Ok(())
        }

        /// Address of the contract administrator.
        pub fn admin(env: Env) -> Result<Address, Error> {
            env.storage()
                .instance()
                .get(&DataKey::Admin)
                .ok_or(Error::NotInitialized)
        }

        /// Replace this contract's wasm with code at `new_wasm_hash`. Only the
        /// admin may upgrade; persistent state is untouched. (From #158.)
        ///
        /// Records the upgrade in the contract's upgrade history, which can be
        /// queried via `upgrade_history` for transparency.
        pub fn upgrade_wasm(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), Error> {
            let admin: Address = Self::admin(env.clone())?;
            admin.require_auth();

            env.deployer()
                .update_current_contract(ContractExecutable::Wasm(new_wasm_hash.clone()));

            ContractUpgraded {
                admin,
                new_wasm_hash: new_wasm_hash.clone(),
            }
            .publish(&env);

            // Record the upgrade in history.
            Self::record_upgrade(&env, new_wasm_hash);

            Ok(())
        }

        /// Record an upgrade in the contract's upgrade history.
        fn record_upgrade(env: &Env, wasm_hash: BytesN<32>) {
            let version = String::from_str(env, env!("CARGO_PKG_VERSION"));
            let ledger = env.ledger().sequence();
            let record = UpgradeRecord {
                version,
                ledger,
                wasm_hash,
            };
            let mut history: soroban_sdk::Vec<UpgradeRecord> = env
                .storage()
                .instance()
                .get(&DataKey::UpgradeHistory)
                .unwrap_or_else(|| soroban_sdk::Vec::new(env));
            history.push_back(record);
            env.storage()
                .instance()
                .set(&DataKey::UpgradeHistory, &history);
            env.storage()
                .instance()
                .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_BUMP);
        }

        /// Get the contract's upgrade history.
        ///
        /// Returns a vector of upgrade records, oldest first. Each record
        /// contains the version, ledger sequence, and wasm hash. The initial
        /// deployment is not recorded; only explicit upgrades via `upgrade_wasm`
        /// appear here.
        pub fn upgrade_history(env: Env) -> soroban_sdk::Vec<UpgradeRecord> {
            env.storage()
                .instance()
                .get(&DataKey::UpgradeHistory)
                .unwrap_or_else(|| soroban_sdk::Vec::new(&env))
        }

        /// Propose transferring a plan to `new_owner`. Step one of a two-step
        /// handshake: only the plan's current `merchant` may propose, and the
        /// transfer takes effect only once `new_owner` accepts. (Merged from
        /// #136.)
        pub fn propose_plan_owner(
            env: Env,
            merchant: Address,
            plan_id: u64,
            new_owner: Address,
        ) -> Result<(), Error> {
            merchant.require_auth();

            let plan: Plan = Self::read_plan(&env, plan_id).ok_or(Error::PlanNotFound)?;
            if plan.merchant != merchant {
                return Err(Error::NotPlanOwner);
            }

            let key = DataKey::PlanOwnerProposal(plan_id);
            env.storage().persistent().set(&key, &new_owner);
            env.storage()
                .persistent()
                .extend_ttl(&key, PERSIST_THRESHOLD, PERSIST_BUMP);

            PlanOwnershipProposed {
                merchant,
                plan_id,
                new_owner,
            }
            .publish(&env);
            Ok(())
        }

        /// Accept a proposed ownership transfer. Only the proposed `new_owner`
        /// may accept, and the plan moves between the two merchants' plan
        /// indexes. Mandates already open against the plan keep paying the
        /// `payout` address frozen at their sign-up; a transfer never reprices
        /// or redirects an existing subscriber, it only changes who manages the
        /// plan and which merchant newly opened mandates are attributed to.
        pub fn accept_plan_owner(env: Env, new_owner: Address, plan_id: u64) -> Result<(), Error> {
            new_owner.require_auth();

            let proposal_key = DataKey::PlanOwnerProposal(plan_id);
            let proposed: Address = env
                .storage()
                .persistent()
                .get(&proposal_key)
                .ok_or(Error::PendingOwnershipNotFound)?;
            if proposed != new_owner {
                return Err(Error::NotPendingOwner);
            }

            let plan_key = DataKey::Plan(plan_id);
            let mut plan: Plan = Self::read_plan(&env, plan_id).ok_or(Error::PlanNotFound)?;

            let old_owner = plan.merchant.clone();
            Self::index_remove(&env, &old_owner, plan_id);
            Self::index_push(&env, &new_owner, plan_id);

            plan.merchant = new_owner.clone();
            env.storage().persistent().set(&plan_key, &plan);
            env.storage()
                .persistent()
                .extend_ttl(&plan_key, PERSIST_THRESHOLD, PERSIST_BUMP);

            // The proposal is one-shot: a second accept must fail.
            env.storage().persistent().remove(&proposal_key);

            PlanTransferred {
                merchant: old_owner,
                plan_id,
                new_owner,
            }
            .publish(&env);
            Ok(())
        }

        fn listings(env: &Env, ids: Vec<u64>) -> Vec<PlanListing> {
            let mut out = Vec::new(env);
            for id in ids.iter() {
                if let Some(plan) = Self::read_plan(env, id) {
                    out.push_back(PlanListing {
                        id: plan.id,
                        merchant: plan.merchant,
                        name: plan.name,
                        token: plan.token,
                        amount: plan.amount,
                        period: plan.period,
                        active: plan.active,
                    });
                }
            }
            out
        }

        /// Read a plan and refresh its TTL when it is close to expiry (#247).
        /// The bump is bounded by `PERSIST_THRESHOLD`, so an entry with plenty
        /// of rent left is untouched. Returns `None` for an unknown plan.
        fn read_plan(env: &Env, plan_id: u64) -> Option<Plan> {
            let key = DataKey::Plan(plan_id);
            let plan: Plan = env.storage().persistent().get(&key)?;
            env.storage()
                .persistent()
                .extend_ttl(&key, PERSIST_THRESHOLD, PERSIST_BUMP);
            Some(plan)
        }

        fn read_next_plan_id(env: &Env) -> Result<u64, Error> {
            env.storage()
                .instance()
                .get(&DataKey::NextPlanId)
                .ok_or(Error::NotInitialized)
        }

        fn set_next_plan_id(env: &Env, next: u64) {
            env.storage().instance().set(&DataKey::NextPlanId, &next);
            env.storage()
                .instance()
                .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_BUMP);
        }

        fn validate_plan(params: &PlanParams) -> Result<(), Error> {
            if params.amount <= 0 {
                return Err(Error::InvalidAmount);
            }
            if params.amount < MIN_CHARGE_AMOUNT {
                return Err(Error::AmountTooLow);
            }
            if params.name.len() > MAX_NAME_LEN {
                return Err(Error::NameTooLong);
            }
            if params.description.len() > MAX_DESCRIPTION_LEN {
                return Err(Error::DescriptionTooLong);
            }
            if params.period < MIN_PERIOD {
                return Err(Error::InvalidPeriod);
            }
            if params.keeper_bps > MAX_KEEPER_BPS {
                return Err(Error::KeeperBpsTooHigh);
            }
            if params.terms_uri.len() > MAX_TERMS_LEN {
                return Err(Error::UriTooLong);
            }
            if params.referral_bps > MAX_REFERRAL_BPS {
                return Err(Error::ReferralBpsTooHigh);
            }
            Ok(())
        }

        /// Persist one validated plan and emit its `PlanCreated` event.
        fn store_plan(env: &Env, merchant: &Address, id: u64, params: &PlanParams) {
            // A payout of None means "pay me"; anything else is an explicit
            // treasury address, which may be the merchant itself.
            let payout = params.payout.clone().unwrap_or_else(|| merchant.clone());

            let plan = Plan {
                id,
                merchant: merchant.clone(),
                payout: payout.clone(),
                name: params.name.clone(),
                description: params.description.clone(),
                token: params.token.clone(),
                amount: params.amount,
                period: params.period,
                active: true,
                keeper_bps: params.keeper_bps,
                referral_bps: params.referral_bps,
                category: params.category.clone(),
                successor_id: 0,
                terms_uri: params.terms_uri.clone(),
                available_until: params.available_until,
                max_subscribers: params.max_subscribers,
            };

            env.storage().persistent().set(&DataKey::Plan(id), &plan);
            env.storage().persistent().extend_ttl(
                &DataKey::Plan(id),
                PERSIST_THRESHOLD,
                PERSIST_BUMP,
            );

            // One entry per position, so publishing a plan costs the same however
            // many plans the merchant already has. The position is also recorded
            // per plan (via `index_push`) so a later ownership transfer can
            // remove it in constant time.
            Self::index_push(env, merchant, id);

            let p = env.storage().persistent();

            // One entry per position in the per-token index, so publishing a plan
            // costs constant time and storage footprint regardless of how many
            // plans already use the token.
            let token_count_key = DataKey::TokenPlanCount(params.token.clone());
            let token_count: u32 = p.get(&token_count_key).unwrap_or(0);
            let token_slot_key = DataKey::TokenPlan(params.token.clone(), token_count);
            p.set(&token_slot_key, &id);
            p.extend_ttl(&token_slot_key, PERSIST_THRESHOLD, PERSIST_BUMP);
            p.set(&token_count_key, &(token_count + 1));
            p.extend_ttl(&token_count_key, PERSIST_THRESHOLD, PERSIST_BUMP);

            PlanCreated {
                merchant: merchant.clone(),
                plan_id: id,
                token: params.token.clone(),
                amount: params.amount,
                period: params.period,
                name: params.name.clone(),
                description: params.description.clone(),
                keeper_bps: params.keeper_bps,
                category: params.category.clone(),
                terms_uri: params.terms_uri.clone(),
                available_until: params.available_until,
                referral_bps: params.referral_bps,
                payout,
                max_subscribers: params.max_subscribers,
            }
            .publish(env);
        }

        /// Append `plan_id` to `merchant`'s plan index, recording the plan's
        /// position (`PlanIndex`) so an ownership transfer can remove it in
        /// constant time.
        fn index_push(env: &Env, merchant: &Address, plan_id: u64) {
            let p = env.storage().persistent();
            let count_key = DataKey::MerchantPlanCount(merchant.clone());
            let count: u32 = p.get(&count_key).unwrap_or(0);

            let slot_key = DataKey::MerchantPlan(merchant.clone(), count);
            p.set(&slot_key, &plan_id);
            p.extend_ttl(&slot_key, PERSIST_THRESHOLD, PERSIST_BUMP);
            p.set(&count_key, &(count + 1));
            p.extend_ttl(&count_key, PERSIST_THRESHOLD, PERSIST_BUMP);

            let pos_key = DataKey::PlanIndex(plan_id);
            p.set(&pos_key, &count);
            p.extend_ttl(&pos_key, PERSIST_THRESHOLD, PERSIST_BUMP);
        }

        /// Remove `plan_id` from `merchant`'s plan index in constant time by
        /// moving the last entry into the vacated slot. The index is for
        /// enumeration only, so one position losing chronological order is
        /// acceptable; every remaining position still lists exactly one live
        /// plan.
        fn index_remove(env: &Env, merchant: &Address, plan_id: u64) {
            let p = env.storage().persistent();
            let count_key = DataKey::MerchantPlanCount(merchant.clone());
            let count: u32 = p.get(&count_key).unwrap_or(0);
            let pos_key = DataKey::PlanIndex(plan_id);
            let position: u32 = p.get(&pos_key).unwrap_or(count);
            if position >= count {
                // Defensive: the plan is not in this merchant's index.
                return;
            }

            let last = count - 1;
            let slot_key = DataKey::MerchantPlan(merchant.clone(), position);
            if position < last {
                if let Some(last_id) =
                    p.get::<_, u64>(&DataKey::MerchantPlan(merchant.clone(), last))
                {
                    p.set(&slot_key, &last_id);
                    p.extend_ttl(&slot_key, PERSIST_THRESHOLD, PERSIST_BUMP);
                    let last_pos_key = DataKey::PlanIndex(last_id);
                    p.set(&last_pos_key, &position);
                    p.extend_ttl(&last_pos_key, PERSIST_THRESHOLD, PERSIST_BUMP);
                }
            } else {
                p.remove(&slot_key);
            }
            p.set(&count_key, &last);
            p.extend_ttl(&count_key, PERSIST_THRESHOLD, PERSIST_BUMP);
            p.remove(&pos_key);
        }
    }
}

pub use contract::{PlanRegistry, PlanRegistryClient};
