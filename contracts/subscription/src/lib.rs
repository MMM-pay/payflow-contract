#![no_std]

mod error;
mod events;
mod interfaces;
mod types;

#[cfg(test)]
mod test;

pub use error::Error;
pub use events::{
    Cancelled, Charged, FeeChanged, MandateCompleted, MandateEnded, PauseChanged, Subscribed,
};
pub use interfaces::{PlanRegistryClient, VaultClient};
pub use types::{DataKey, Mandate, MandateStatus, Plan};

use soroban_sdk::{contract, contractimpl, Address, Env, Vec};

const DAY_IN_LEDGERS: u32 = 17_280;
const INSTANCE_BUMP: u32 = 30 * DAY_IN_LEDGERS;
const INSTANCE_THRESHOLD: u32 = INSTANCE_BUMP - DAY_IN_LEDGERS;
const PERSIST_BUMP: u32 = 90 * DAY_IN_LEDGERS;
const PERSIST_THRESHOLD: u32 = PERSIST_BUMP - DAY_IN_LEDGERS;

/// Basis-point denominator. All fee math is integer basis points; the contract
/// contains no floating point arithmetic.
pub const BPS_DENOMINATOR: i128 = 10_000;

/// Hard ceiling on the protocol fee: 10%. Prevents an admin from setting a
/// confiscatory fee on mandates that are already open.
pub const MAX_FEE_BPS: u32 = 1_000;

/// Largest page the index getters return. Each index position is its own
/// storage entry, and a transaction may touch at most 100 entries in total,
/// so a page of 50 leaves room for the entries the caller reads alongside it.
pub const MAX_PAGE: u32 = 50;

/// Billing engine for pull-based recurring payments.
///
/// Classic Stellar has no pull-payment primitive: a payment must be pushed by
/// the account that holds the funds. This contract supplies the missing half.
/// A subscriber opens a `Mandate` once, and thereafter any caller may trigger
/// `charge` when the mandate falls due. The mandate is the only thing standing
/// between a merchant and a subscriber's balance, and it is bounded on every
/// axis the subscriber cares about: fixed amount, fixed period, capped count,
/// and cancellable at any time.
#[contract]
pub struct Subscription;

#[contractimpl]
impl Subscription {
    /// Runs once, in the same transaction that deploys the contract. With a
    /// separate `initialize` call, anyone watching the ledger could call it
    /// between the deploy and the owner's own call and make themselves admin.
    pub fn __constructor(
        env: Env,
        admin: Address,
        plan_registry: Address,
        vault: Address,
        fee_bps: u32,
        fee_to: Address,
    ) -> Result<(), Error> {
        if fee_bps > MAX_FEE_BPS {
            return Err(Error::FeeTooHigh);
        }

        let s = env.storage().instance();
        s.set(&DataKey::Admin, &admin);
        s.set(&DataKey::PlanRegistry, &plan_registry);
        s.set(&DataKey::Vault, &vault);
        s.set(&DataKey::FeeBps, &fee_bps);
        s.set(&DataKey::FeeTo, &fee_to);
        s.set(&DataKey::NextMandateId, &1u64);
        s.extend_ttl(INSTANCE_THRESHOLD, INSTANCE_BUMP);
        Ok(())
    }

    /// Open a mandate against a published plan.
    ///
    /// The plan's terms are copied into the mandate here and never re-read, so
    /// a merchant cannot change the price of a subscription after the fact.
    /// The first charge is due immediately.
    pub fn subscribe(
        env: Env,
        subscriber: Address,
        plan_id: u64,
        max_charges: u32,
    ) -> Result<u64, Error> {
        subscriber.require_auth();

        let registry: Address = Self::cfg_address(&env, DataKey::PlanRegistry)?;
        let plan = PlanRegistryClient::new(&env, &registry).get_plan(&plan_id);

        if !plan.active {
            return Err(Error::PlanInactive);
        }

        let id: u64 = env
            .storage()
            .instance()
            .get(&DataKey::NextMandateId)
            .ok_or(Error::NotInitialized)?;

        let fee_bps: u32 = env
            .storage()
            .instance()
            .get(&DataKey::FeeBps)
            .ok_or(Error::NotInitialized)?;

        let now = env.ledger().timestamp();
        let mandate = Mandate {
            id,
            subscriber: subscriber.clone(),
            plan_id,
            merchant: plan.merchant.clone(),
            token: plan.token.clone(),
            amount: plan.amount,
            period: plan.period,
            next_charge: now,
            last_charge: 0,
            charges_made: 0,
            max_charges,
            fee_bps,
            status: MandateStatus::Active,
        };

        Self::write_mandate(&env, &mandate);
        Self::index_push(
            &env,
            DataKey::SubscriberMandateCount(subscriber.clone()),
            |i| DataKey::SubscriberMandate(subscriber.clone(), i),
            id,
        );
        Self::index_push(
            &env,
            DataKey::MerchantMandateCount(plan.merchant.clone()),
            |i| DataKey::MerchantMandate(plan.merchant.clone(), i),
            id,
        );

        env.storage()
            .instance()
            .set(&DataKey::NextMandateId, &(id + 1));
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_BUMP);

        Subscribed {
            mandate_id: id,
            subscriber,
            merchant: plan.merchant,
            plan_id,
            amount: plan.amount,
            period: plan.period,
            next_charge: now,
            max_charges,
            fee_bps,
        }
        .publish(&env);

        Ok(id)
    }

    /// Execute a due charge. Deliberately permissionless: the mandate itself is
    /// the authorization, so anyone — the merchant, a keeper bot, or the
    /// subscriber — may settle it. This keeps billing alive without trusting a
    /// single operator to stay online.
    ///
    /// `next_charge` advances to `now + period` rather than
    /// `next_charge + period`. A keeper outage therefore delays billing instead
    /// of accumulating a backlog that could drain a vault in one burst. The
    /// tradeoff is slow forward drift, which favours the subscriber.
    pub fn charge(env: Env, mandate_id: u64) -> Result<(), Error> {
        let mut mandate = Self::read_mandate(&env, mandate_id)?;

        if mandate.status != MandateStatus::Active {
            return Err(Error::MandateNotActive);
        }

        let now = env.ledger().timestamp();
        if now < mandate.next_charge {
            return Err(Error::NotDue);
        }
        if mandate.max_charges != 0 && mandate.charges_made >= mandate.max_charges {
            return Err(Error::MaxChargesReached);
        }

        // The mandate's own fee, frozen when the subscriber opened it.
        let fee = mandate.amount * (mandate.fee_bps as i128) / BPS_DENOMINATOR;
        let merchant_amount = mandate.amount - fee;

        let vault_addr: Address = Self::cfg_address(&env, DataKey::Vault)?;
        let vault = VaultClient::new(&env, &vault_addr);

        vault.debit(
            &mandate.subscriber,
            &mandate.token,
            &mandate.merchant,
            &merchant_amount,
        );

        if fee > 0 {
            let fee_to: Address = Self::cfg_address(&env, DataKey::FeeTo)?;
            vault.debit(&mandate.subscriber, &mandate.token, &fee_to, &fee);
        }

        mandate.charges_made += 1;
        mandate.last_charge = now;
        mandate.next_charge = now + mandate.period;

        let completed = mandate.max_charges != 0 && mandate.charges_made >= mandate.max_charges;
        if completed {
            mandate.status = MandateStatus::Completed;
        }

        Self::write_mandate(&env, &mandate);

        Charged {
            mandate_id,
            subscriber: mandate.subscriber.clone(),
            merchant: mandate.merchant.clone(),
            amount: merchant_amount,
            fee,
            charges_made: mandate.charges_made,
            next_charge: mandate.next_charge,
        }
        .publish(&env);

        if completed {
            MandateCompleted {
                mandate_id,
                subscriber: mandate.subscriber,
                charges_made: mandate.charges_made,
            }
            .publish(&env);
        }

        Ok(())
    }

    /// Terminate a mandate permanently. Only the subscriber may do this, and it
    /// can be done at any time without the merchant's involvement.
    pub fn cancel(env: Env, subscriber: Address, mandate_id: u64) -> Result<(), Error> {
        subscriber.require_auth();

        let mut mandate = Self::read_mandate(&env, mandate_id)?;
        if mandate.subscriber != subscriber {
            return Err(Error::NotSubscriber);
        }
        if mandate.status == MandateStatus::Cancelled || mandate.status == MandateStatus::Completed
        {
            return Err(Error::MandateNotActive);
        }

        mandate.status = MandateStatus::Cancelled;
        Self::write_mandate(&env, &mandate);

        Cancelled {
            mandate_id,
            subscriber,
            charges_made: mandate.charges_made,
        }
        .publish(&env);
        Ok(())
    }

    /// End a mandate from the merchant's side, for example when the service is
    /// discontinued. Deactivating a plan only stops new subscriptions; this is
    /// how a merchant stops billing the ones already open. Like `cancel`, it
    /// is permanent, and it can only ever reduce what the subscriber pays.
    pub fn end_mandate(env: Env, merchant: Address, mandate_id: u64) -> Result<(), Error> {
        merchant.require_auth();

        let mut mandate = Self::read_mandate(&env, mandate_id)?;
        if mandate.merchant != merchant {
            return Err(Error::NotMerchant);
        }
        if mandate.status == MandateStatus::Cancelled || mandate.status == MandateStatus::Completed
        {
            return Err(Error::MandateNotActive);
        }

        mandate.status = MandateStatus::Cancelled;
        Self::write_mandate(&env, &mandate);

        MandateEnded {
            mandate_id,
            merchant,
            subscriber: mandate.subscriber,
            charges_made: mandate.charges_made,
        }
        .publish(&env);
        Ok(())
    }

    /// Pause or resume billing without losing the mandate's history.
    pub fn set_paused(
        env: Env,
        subscriber: Address,
        mandate_id: u64,
        paused: bool,
    ) -> Result<(), Error> {
        subscriber.require_auth();

        let mut mandate = Self::read_mandate(&env, mandate_id)?;
        if mandate.subscriber != subscriber {
            return Err(Error::NotSubscriber);
        }

        mandate.status = match (mandate.status, paused) {
            (MandateStatus::Active, true) => MandateStatus::Paused,
            (MandateStatus::Paused, false) => MandateStatus::Active,
            _ => return Err(Error::MandateNotActive),
        };

        Self::write_mandate(&env, &mandate);

        PauseChanged {
            mandate_id,
            subscriber,
            paused,
        }
        .publish(&env);
        Ok(())
    }

    /// Change the protocol fee for mandates opened from now on.
    ///
    /// Safe to call at any time: every open mandate carries the fee it was
    /// created with, so this cannot reprice an existing subscriber. The
    /// `MAX_FEE_BPS` ceiling still applies.
    pub fn set_fee_bps(env: Env, new_fee_bps: u32) -> Result<(), Error> {
        let admin: Address = Self::cfg_address(&env, DataKey::Admin)?;
        admin.require_auth();

        if new_fee_bps > MAX_FEE_BPS {
            return Err(Error::FeeTooHigh);
        }

        let old_fee_bps: u32 = env
            .storage()
            .instance()
            .get(&DataKey::FeeBps)
            .ok_or(Error::NotInitialized)?;

        env.storage().instance().set(&DataKey::FeeBps, &new_fee_bps);
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_BUMP);

        FeeChanged {
            admin,
            old_fee_bps,
            new_fee_bps,
        }
        .publish(&env);

        Ok(())
    }

    pub fn get_mandate(env: Env, mandate_id: u64) -> Result<Mandate, Error> {
        Self::read_mandate(&env, mandate_id)
    }

    /// True when `charge` would succeed on timing and status grounds. Does not
    /// check vault balance; a funded-ness check is the caller's job.
    pub fn is_due(env: Env, mandate_id: u64) -> Result<bool, Error> {
        let m = Self::read_mandate(&env, mandate_id)?;
        if m.status != MandateStatus::Active {
            return Ok(false);
        }
        if m.max_charges != 0 && m.charges_made >= m.max_charges {
            return Ok(false);
        }
        Ok(env.ledger().timestamp() >= m.next_charge)
    }

    /// How many mandates `subscriber` has opened, in any state.
    pub fn subscriber_mandate_count(env: Env, subscriber: Address) -> u32 {
        Self::index_len(&env, DataKey::SubscriberMandateCount(subscriber))
    }

    /// Ids of the mandates `subscriber` opened, oldest first, starting at
    /// position `start`. Returns at most `min(limit, MAX_PAGE)` ids.
    pub fn subscriber_mandates(env: Env, subscriber: Address, start: u32, limit: u32) -> Vec<u64> {
        let len = Self::index_len(&env, DataKey::SubscriberMandateCount(subscriber.clone()));
        Self::index_page(&env, len, start, limit, |i| {
            DataKey::SubscriberMandate(subscriber.clone(), i)
        })
    }

    /// How many mandates have been opened against `merchant`'s plans.
    pub fn merchant_mandate_count(env: Env, merchant: Address) -> u32 {
        Self::index_len(&env, DataKey::MerchantMandateCount(merchant))
    }

    /// Ids of the mandates opened against `merchant`'s plans, oldest first,
    /// starting at position `start`. Returns at most `min(limit, MAX_PAGE)`.
    pub fn merchant_mandates(env: Env, merchant: Address, start: u32, limit: u32) -> Vec<u64> {
        let len = Self::index_len(&env, DataKey::MerchantMandateCount(merchant.clone()));
        Self::index_page(&env, len, start, limit, |i| {
            DataKey::MerchantMandate(merchant.clone(), i)
        })
    }

    pub fn fee_bps(env: Env) -> Result<u32, Error> {
        env.storage()
            .instance()
            .get(&DataKey::FeeBps)
            .ok_or(Error::NotInitialized)
    }

    pub fn admin(env: Env) -> Result<Address, Error> {
        Self::cfg_address(&env, DataKey::Admin)
    }

    pub fn plan_registry(env: Env) -> Result<Address, Error> {
        Self::cfg_address(&env, DataKey::PlanRegistry)
    }

    pub fn vault(env: Env) -> Result<Address, Error> {
        Self::cfg_address(&env, DataKey::Vault)
    }
}

impl Subscription {
    fn cfg_address(env: &Env, key: DataKey) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&key)
            .ok_or(Error::NotInitialized)
    }

    fn read_mandate(env: &Env, id: u64) -> Result<Mandate, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Mandate(id))
            .ok_or(Error::MandateNotFound)
    }

    fn write_mandate(env: &Env, mandate: &Mandate) {
        let key = DataKey::Mandate(mandate.id);
        env.storage().persistent().set(&key, mandate);
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSIST_THRESHOLD, PERSIST_BUMP);
    }

    fn index_len(env: &Env, count_key: DataKey) -> u32 {
        env.storage().persistent().get(&count_key).unwrap_or(0)
    }

    /// Append `id` to an index stored as one entry per position.
    ///
    /// Each subscribe writes two small entries rather than rewriting a list
    /// that keeps growing. With a single list, anyone could open throwaway
    /// mandates against a merchant's plan until the list outgrew Soroban's
    /// entry size limit, after which every new subscription to that merchant
    /// would fail.
    fn index_push(env: &Env, count_key: DataKey, slot: impl Fn(u32) -> DataKey, id: u64) {
        let len = Self::index_len(env, count_key.clone());
        let slot_key = slot(len);
        let p = env.storage().persistent();
        p.set(&slot_key, &id);
        p.extend_ttl(&slot_key, PERSIST_THRESHOLD, PERSIST_BUMP);
        p.set(&count_key, &(len + 1));
        p.extend_ttl(&count_key, PERSIST_THRESHOLD, PERSIST_BUMP);
    }

    fn index_page(
        env: &Env,
        len: u32,
        start: u32,
        limit: u32,
        slot: impl Fn(u32) -> DataKey,
    ) -> Vec<u64> {
        let mut out = Vec::new(env);
        let end = start.saturating_add(limit.min(MAX_PAGE)).min(len);
        for i in start..end {
            if let Some(id) = env.storage().persistent().get::<_, u64>(&slot(i)) {
                out.push_back(id);
            }
        }
        out
    }
}
