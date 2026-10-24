//! Plan registry contract for Payflow subscriptions.
//!
//! Stores merchant plans and profiles. Plans are immutable templates defining
//! pricing, billing periods, and keeper incentives.

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

pub use error::Error;
pub use events::{PlanCreated, PlanStatusChanged, PlanSuccessorSet, ProfileUpdated};
pub use types::{DataKey, Plan, Profile};

use payflow_shared::MAX_KEEPER_BPS;
use soroban_sdk::{contract, contractimpl, Address, Env, String, Vec};

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

/// Largest page `merchant_plans` returns in one call. Each id is read from its
/// own storage entry, and a transaction may touch at most 100 entries.
pub const MAX_PAGE: u32 = 50;

#[allow(missing_docs)]
mod contract {
    use super::*;

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
        /// # Auth
        /// Requires authorization from `merchant`.
        ///
        /// # Events
        /// Emits [`PlanCreated`].
        ///
        /// # Errors
        /// Returns [`Error::InvalidAmount`] if `amount <= 0`, [`Error::NameTooLong`]
        /// if `name` exceeds [`MAX_NAME_LEN`], [`Error::DescriptionTooLong`] if `description`
        /// exceeds [`MAX_DESCRIPTION_LEN`], [`Error::InvalidPeriod`] if `period < MIN_PERIOD`,
        /// [`Error::KeeperBpsTooHigh`] if `keeper_bps > MAX_KEEPER_BPS`, or [`Error::NotInitialized`]
        /// if the contract has not been initialized.
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
        ) -> Result<u64, Error> {
            merchant.require_auth();

            if amount <= 0 {
                return Err(Error::InvalidAmount);
            }
            if name.len() > MAX_NAME_LEN {
                return Err(Error::NameTooLong);
            }
            if description.len() > MAX_DESCRIPTION_LEN {
                return Err(Error::DescriptionTooLong);
            }
            if period < MIN_PERIOD {
                return Err(Error::InvalidPeriod);
            }
            if keeper_bps > MAX_KEEPER_BPS {
                return Err(Error::KeeperBpsTooHigh);
            }

            let id: u64 = env
                .storage()
                .instance()
                .get(&DataKey::NextPlanId)
                .ok_or(Error::NotInitialized)?;

            let plan = Plan {
                id,
                merchant: merchant.clone(),
                name: name.clone(),
                description: description.clone(),
                token: token.clone(),
                amount,
                period,
                active: true,
                keeper_bps,
                successor_id: 0,
            };

            env.storage().persistent().set(&DataKey::Plan(id), &plan);
            env.storage().persistent().extend_ttl(
                &DataKey::Plan(id),
                PERSIST_THRESHOLD,
                PERSIST_BUMP,
            );

            // One entry per position, so publishing a plan costs the same however
            // many plans the merchant already has.
            let count_key = DataKey::MerchantPlanCount(merchant.clone());
            let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);
            let slot_key = DataKey::MerchantPlan(merchant.clone(), count);
            let p = env.storage().persistent();
            p.set(&slot_key, &id);
            p.extend_ttl(&slot_key, PERSIST_THRESHOLD, PERSIST_BUMP);
            p.set(&count_key, &(count + 1));
            p.extend_ttl(&count_key, PERSIST_THRESHOLD, PERSIST_BUMP);

            // One entry per position in the per-token index, so publishing a plan
            // costs constant time and storage footprint regardless of how many
            // plans already use the token.
            let token_count_key = DataKey::TokenPlanCount(token.clone());
            let token_count: u32 = p.get(&token_count_key).unwrap_or(0);
            let token_slot_key = DataKey::TokenPlan(token.clone(), token_count);
            p.set(&token_slot_key, &id);
            p.extend_ttl(&token_slot_key, PERSIST_THRESHOLD, PERSIST_BUMP);
            p.set(&token_count_key, &(token_count + 1));
            p.extend_ttl(&token_count_key, PERSIST_THRESHOLD, PERSIST_BUMP);

            env.storage()
                .instance()
                .set(&DataKey::NextPlanId, &(id + 1));
            env.storage()
                .instance()
                .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_BUMP);

            PlanCreated {
                merchant,
                plan_id: id,
                token: plan.token.clone(),
                amount: plan.amount,
                period: plan.period,
                name,
                description,
                keeper_bps,
            }
            .publish(&env);

            Ok(id)
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

            let mut plan: Plan = env
                .storage()
                .persistent()
                .get(&DataKey::Plan(plan_id))
                .ok_or(Error::PlanNotFound)?;

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
            let mut plan: Plan = env
                .storage()
                .persistent()
                .get(&plan_key)
                .ok_or(Error::PlanNotFound)?;

            if plan.merchant != merchant {
                return Err(Error::NotPlanOwner);
            }
            if successor_id == plan_id {
                return Err(Error::SelfSuccessor);
            }

            let successor: Plan = env
                .storage()
                .persistent()
                .get(&DataKey::Plan(successor_id))
                .ok_or(Error::SuccessorNotFound)?;

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
                let mut plan: Plan = p.get(&plan_key).ok_or(Error::PlanNotFound)?;
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
        pub fn get_plan(env: Env, plan_id: u64) -> Result<Plan, Error> {
            env.storage()
                .persistent()
                .get(&DataKey::Plan(plan_id))
                .ok_or(Error::PlanNotFound)
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

        /// The id the next published plan will get. Plans are numbered from 1, so
        /// `next_plan_id() - 1` is the number of plans published so far.
        pub fn next_plan_id(env: Env) -> Result<u64, Error> {
            env.storage()
                .instance()
                .get(&DataKey::NextPlanId)
                .ok_or(Error::NotInitialized)
        }

        /// Address of the contract administrator.
        pub fn admin(env: Env) -> Result<Address, Error> {
            env.storage()
                .instance()
                .get(&DataKey::Admin)
                .ok_or(Error::NotInitialized)
        }
    }
}

pub use contract::{PlanRegistry, PlanRegistryClient};
