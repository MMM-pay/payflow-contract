#![no_std]

mod error;
mod events;
mod types;

#[cfg(test)]
mod test;

pub use error::Error;
pub use events::{
    PlanCreated, PlanOwnerProposed, PlanStatusChanged, PlanTransferred, ProfileUpdated,
};
pub use types::{DataKey, Plan, Profile};

use payflow_shared::{MAX_KEEPER_BPS, MAX_REFERRAL_BPS};
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
    ///
    /// `keeper_bps` is the share of each charge paid to whoever settles it,
    /// taken out of the merchant's share, capped at `MAX_KEEPER_BPS`.
    /// `referral_bps` is the share paid to a subscriber's referrer, also out
    /// of the merchant's share, capped at `MAX_REFERRAL_BPS`; 0 disables
    /// referrals. `payout` receives the merchant's share of every charge and
    /// defaults to `merchant`, so a treasury or multisig can be paid while
    /// `merchant` keeps publishing and managing plans.
    ///
    /// All of these are part of the plan's immutable terms: a mandate opened
    /// against this plan keeps the split it was opened with, so raising a rate
    /// later cannot take money from an existing subscriber or merchant.
    ///
    // Contract entry points take flat positional arguments on the wire, so
    // this one is necessarily wider than clippy's default threshold.
    #[allow(clippy::too_many_arguments)]
    pub fn create_plan(
        env: Env,
        merchant: Address,
        token: Address,
        amount: i128,
        period: u64,
        name: String,
        keeper_bps: u32,
        referral_bps: u32,
        payout: Option<Address>,
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
        if keeper_bps > MAX_KEEPER_BPS {
            return Err(Error::KeeperBpsTooHigh);
        }
        if referral_bps > MAX_REFERRAL_BPS {
            return Err(Error::ReferralBpsTooHigh);
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
            keeper_bps,
            payout: payout.unwrap_or_else(|| merchant.clone()),
            referral_bps,
        };

        Self::write_plan(&env, &plan);

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
        Self::write_slot(&env, id, count);

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
            keeper_bps,
            payout: plan.payout,
            referral_bps,
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

    /// Propose handing a plan to `new_owner`. The current owner must sign;
    /// the transfer completes only when `new_owner` calls
    /// `accept_plan_owner`, so a plan can never be moved to an address that
    /// cannot sign for itself. Proposing again replaces any earlier proposal.
    pub fn propose_plan_owner(
        env: Env,
        merchant: Address,
        plan_id: u64,
        new_owner: Address,
    ) -> Result<(), Error> {
        merchant.require_auth();

        let plan: Plan = env
            .storage()
            .persistent()
            .get(&DataKey::Plan(plan_id))
            .ok_or(Error::PlanNotFound)?;
        if plan.merchant != merchant {
            return Err(Error::NotPlanOwner);
        }
        if plan.merchant == new_owner {
            return Err(Error::OwnerUnchanged);
        }

        let key = DataKey::PendingOwner(plan_id);
        env.storage().persistent().set(&key, &new_owner);
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSIST_THRESHOLD, PERSIST_BUMP);

        PlanOwnerProposed {
            merchant,
            plan_id,
            new_owner,
        }
        .publish(&env);
        Ok(())
    }

    /// Accepts a pending ownership transfer. Only the proposed owner can do
    /// this. The plan moves between the two merchants' indexes in constant
    /// time: the old owner's index swaps in its last entry, so no loop over
    /// the index is needed however many plans it holds.
    ///
    /// If the plan paid out to the old owner's address, the payout follows the
    /// plan to the new owner; an explicitly set payout (a treasury address) is
    /// left untouched. Mandates that are already open are **not** changed:
    /// they keep paying the merchant, payout and referral addresses frozen at
    /// subscribe time, exactly as a plan edit cannot reprice them.
    pub fn accept_plan_owner(env: Env, new_owner: Address, plan_id: u64) -> Result<(), Error> {
        new_owner.require_auth();

        let pending_key = DataKey::PendingOwner(plan_id);
        let pending: Address = env
            .storage()
            .persistent()
            .get(&pending_key)
            .ok_or(Error::NotPendingOwner)?;
        if pending != new_owner {
            return Err(Error::NotPendingOwner);
        }

        let plan_key = DataKey::Plan(plan_id);
        let mut plan: Plan = env
            .storage()
            .persistent()
            .get(&plan_key)
            .ok_or(Error::PlanNotFound)?;
        let old_owner = plan.merchant.clone();

        Self::index_remove(&env, &old_owner, plan_id);
        Self::index_append(&env, &new_owner, plan_id);

        if plan.payout == old_owner {
            plan.payout = new_owner.clone();
        }
        plan.merchant = new_owner.clone();
        Self::write_plan(&env, &plan);

        env.storage().persistent().remove(&pending_key);

        PlanTransferred {
            plan_id,
            from: old_owner,
            to: new_owner,
        }
        .publish(&env);
        Ok(())
    }

    /// The address currently proposed to take over `plan_id`, if any.
    pub fn pending_plan_owner(env: Env, plan_id: u64) -> Option<Address> {
        env.storage()
            .persistent()
            .get(&DataKey::PendingOwner(plan_id))
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

impl PlanRegistry {
    fn write_plan(env: &Env, plan: &Plan) {
        let key = DataKey::Plan(plan.id);
        env.storage().persistent().set(&key, plan);
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSIST_THRESHOLD, PERSIST_BUMP);
    }

    fn write_slot(env: &Env, plan_id: u64, slot: u32) {
        let key = DataKey::PlanSlot(plan_id);
        env.storage().persistent().set(&key, &slot);
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSIST_THRESHOLD, PERSIST_BUMP);
    }

    /// Append a plan to a merchant's index.
    fn index_append(env: &Env, merchant: &Address, plan_id: u64) {
        let count_key = DataKey::MerchantPlanCount(merchant.clone());
        let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);
        let slot_key = DataKey::MerchantPlan(merchant.clone(), count);
        let p = env.storage().persistent();
        p.set(&slot_key, &plan_id);
        p.extend_ttl(&slot_key, PERSIST_THRESHOLD, PERSIST_BUMP);
        p.set(&count_key, &(count + 1));
        p.extend_ttl(&count_key, PERSIST_THRESHOLD, PERSIST_BUMP);
        Self::write_slot(env, plan_id, count);
    }

    /// Remove a plan from a merchant's index by swapping the last entry into
    /// its slot, so the index stays dense and the call touches a constant
    /// number of ledger entries.
    fn index_remove(env: &Env, merchant: &Address, plan_id: u64) {
        let count_key = DataKey::MerchantPlanCount(merchant.clone());
        let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);
        let slot: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::PlanSlot(plan_id))
            .unwrap_or(0);
        if count == 0 || slot >= count {
            return;
        }

        let p = env.storage().persistent();
        let last = count - 1;
        let last_key = DataKey::MerchantPlan(merchant.clone(), last);
        if slot != last {
            let moved: u64 = p.get(&last_key).unwrap_or(plan_id);
            let slot_key = DataKey::MerchantPlan(merchant.clone(), slot);
            p.set(&slot_key, &moved);
            p.extend_ttl(&slot_key, PERSIST_THRESHOLD, PERSIST_BUMP);
            Self::write_slot(env, moved, slot);
        }
        p.remove(&last_key);
        p.set(&count_key, &last);
        p.extend_ttl(&count_key, PERSIST_THRESHOLD, PERSIST_BUMP);
    }
}
