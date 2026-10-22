#![no_std]

mod error;
mod events;
mod types;

#[cfg(test)]
mod test;

pub use error::Error;
pub use events::{PlanCreated, PlanStatusChanged, ProfileUpdated};
pub use types::{DataKey, Plan, Profile};

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

/// Maximum profile URL length in bytes. Self-declared and unbounded in theory,
/// capped here so storage and UI rendering stay bounded.
pub const MAX_URL_LEN: u32 = 256;

/// Largest page `merchant_plans` returns in one call. Each id is read from its
/// own storage entry, and a transaction may touch at most 100 entries.
pub const MAX_PAGE: u32 = 50;

#[contract]
pub struct PlanRegistry;

#[contractimpl]
impl PlanRegistry {
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
    pub fn create_plan(
        env: Env,
        merchant: Address,
        token: Address,
        amount: i128,
        period: u64,
        name: String,
    ) -> Result<u64, Error> {
        merchant.require_auth();

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        if name.len() > MAX_NAME_LEN {
            return Err(Error::NameTooLong);
        }
        if period < MIN_PERIOD {
            return Err(Error::InvalidPeriod);
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
            token,
            amount,
            period,
            active: true,
        };

        env.storage().persistent().set(&DataKey::Plan(id), &plan);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Plan(id), PERSIST_THRESHOLD, PERSIST_BUMP);

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
        }
        .publish(&env);

        Ok(id)
    }

    /// Activate or deactivate a plan. Deactivating stops new subscriptions but
    /// does not cancel mandates already open against the plan.
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

    /// Set or replace `merchant`'s self-declared profile: a display name and a
    /// URL (website or terms). The profile is not verified by the contract;
    /// subscribers should treat it as a claim the merchant makes about itself.
    /// Only the merchant may set its own profile.
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

    /// The id the next published plan will get. Plans are numbered from 1, so
    /// `next_plan_id() - 1` is the number of plans published so far.
    pub fn next_plan_id(env: Env) -> Result<u64, Error> {
        env.storage()
            .instance()
            .get(&DataKey::NextPlanId)
            .ok_or(Error::NotInitialized)
    }

    pub fn admin(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)
    }
}
