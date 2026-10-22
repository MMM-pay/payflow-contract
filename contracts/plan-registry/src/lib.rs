#![no_std]
// Contract entry points mirror the CLI's flat argument lists, and the
// `contractimpl` macro re-emits them with extra parameters on top, so
// `create_plan` legitimately carries more than seven.
#![allow(clippy::too_many_arguments)]

mod error;
mod events;
mod types;

#[cfg(test)]
mod test;

pub use error::Error;
pub use events::{
    PlanCreated, PlanOwnershipProposed, PlanStatusChanged, PlanTransferred, ProfileUpdated,
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
    /// `payout` is the address each charge pays the merchant's share to; pass
    /// `None` to pay the merchant itself, or a treasury/multisig address to
    /// keep revenue somewhere other than the operational key. Plan management
    /// (deactivation, transfer) always stays with `merchant`.
    ///
    /// `keeper_bps` is the share of each charge paid to whoever settles it,
    /// taken out of the merchant's share, capped at `MAX_KEEPER_BPS`. It is
    /// part of the plan's immutable terms: a mandate opened against this plan
    /// keeps the tip it was opened with, so raising the tip later cannot take
    /// money from an existing subscriber or merchant.
    ///
    /// `referral_bps` is the share of each charge paid to the referrer a
    /// subscriber names at subscribe time, also taken out of the merchant's
    /// share and also immutable per mandate, capped at `MAX_REFERRAL_BPS`.
    /// 0 (the default) disables referrals for this plan.
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
        // A payout of None means "pay me"; anything else is an explicit
        // treasury address, which may be the merchant itself.
        let payout = payout.unwrap_or_else(|| merchant.clone());

        let id: u64 = env
            .storage()
            .instance()
            .get(&DataKey::NextPlanId)
            .ok_or(Error::NotInitialized)?;

        let plan = Plan {
            id,
            merchant: merchant.clone(),
            payout: payout.clone(),
            name: name.clone(),
            token,
            amount,
            period,
            active: true,
            keeper_bps,
            referral_bps,
        };

        env.storage().persistent().set(&DataKey::Plan(id), &plan);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Plan(id), PERSIST_THRESHOLD, PERSIST_BUMP);

        // One entry per position, so publishing a plan costs the same however
        // many plans the merchant already has. The position is also recorded
        // per plan so a later ownership transfer can remove it in constant
        // time.
        Self::index_push(&env, &merchant, id);

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
            referral_bps,
            payout,
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

    /// Propose handing `plan_id` to `new_owner`. Step one of a two-step
    /// transfer: nothing changes until the proposed owner accepts, so a
    /// mistyped address cannot strand a plan.
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

    /// Accept a proposed ownership transfer. Step two: only `new_owner` (the
    /// address the merchant proposed) may accept, and the plan moves between
    /// the two merchants' plan indexes.
    ///
    /// Mandates already open against the plan keep paying the `payout`
    /// address frozen when their subscriber subscribed; a transfer never
    /// reprices or redirects an existing subscriber. It only changes who
    /// manages the plan (deactivation, ending mandates, transferring again)
    /// and which merchant newly opened mandates are attributed to.
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
        let mut plan: Plan = env
            .storage()
            .persistent()
            .get(&plan_key)
            .ok_or(Error::PlanNotFound)?;

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

impl PlanRegistry {
    /// Append `plan_id` to `merchant`'s plan index and record its position,
    /// so a later transfer can find it in constant time.
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
    /// plan, and the loop bounds of `merchant_plans` and `deactivate_page`
    /// are unchanged.
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
            if let Some(last_id) = p.get::<_, u64>(&DataKey::MerchantPlan(merchant.clone(), last)) {
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
