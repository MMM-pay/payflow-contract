#![no_std]

mod error;
mod events;
mod types;

#[cfg(test)]
mod test;

pub use error::Error;
pub use events::{PlanCreated, PlanStatusChanged};
pub use types::{DataKey, Plan};

use soroban_sdk::{contract, contractimpl, Address, Env, Vec};

const DAY_IN_LEDGERS: u32 = 17_280;
const INSTANCE_BUMP: u32 = 30 * DAY_IN_LEDGERS;
const INSTANCE_THRESHOLD: u32 = INSTANCE_BUMP - DAY_IN_LEDGERS;
const PERSIST_BUMP: u32 = 90 * DAY_IN_LEDGERS;
const PERSIST_THRESHOLD: u32 = PERSIST_BUMP - DAY_IN_LEDGERS;

/// Minimum billing period. Guards against a merchant publishing a
/// one-second plan and draining a mandate through rapid repeat charges.
pub const MIN_PERIOD: u64 = 60;

#[contract]
pub struct PlanRegistry;

#[contractimpl]
impl PlanRegistry {
    /// Set the admin. Callable once.
    pub fn initialize(env: Env, admin: Address) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::NextPlanId, &1u64);
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_BUMP);
        Ok(())
    }

    /// Publish a new plan. Only the merchant may publish plans in its own name.
    pub fn create_plan(
        env: Env,
        merchant: Address,
        token: Address,
        amount: i128,
        period: u64,
    ) -> Result<u64, Error> {
        merchant.require_auth();

        if amount <= 0 {
            return Err(Error::InvalidAmount);
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
            token,
            amount,
            period,
            active: true,
        };

        env.storage().persistent().set(&DataKey::Plan(id), &plan);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Plan(id), PERSIST_THRESHOLD, PERSIST_BUMP);

        let mkey = DataKey::MerchantPlans(merchant.clone());
        let mut owned: Vec<u64> = env
            .storage()
            .persistent()
            .get(&mkey)
            .unwrap_or_else(|| Vec::new(&env));
        owned.push_back(id);
        env.storage().persistent().set(&mkey, &owned);
        env.storage()
            .persistent()
            .extend_ttl(&mkey, PERSIST_THRESHOLD, PERSIST_BUMP);

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

    pub fn get_plan(env: Env, plan_id: u64) -> Result<Plan, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Plan(plan_id))
            .ok_or(Error::PlanNotFound)
    }

    pub fn merchant_plans(env: Env, merchant: Address) -> Vec<u64> {
        env.storage()
            .persistent()
            .get(&DataKey::MerchantPlans(merchant))
            .unwrap_or_else(|| Vec::new(&env))
    }

    pub fn admin(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)
    }
}
