#![no_std]

mod error;
mod events;
mod interfaces;
mod types;

#[cfg(test)]
mod test;

pub use error::Error;
pub use events::{Cancelled, Charged, MandateCompleted, PauseChanged, Subscribed};
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
    pub fn initialize(
        env: Env,
        admin: Address,
        plan_registry: Address,
        vault: Address,
        fee_bps: u32,
        fee_to: Address,
    ) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
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
            status: MandateStatus::Active,
        };

        Self::write_mandate(&env, &mandate);
        Self::index_push(&env, DataKey::SubscriberMandates(subscriber.clone()), id);
        Self::index_push(&env, DataKey::MerchantMandates(plan.merchant.clone()), id);

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

        let fee_bps: u32 = env
            .storage()
            .instance()
            .get(&DataKey::FeeBps)
            .ok_or(Error::NotInitialized)?;
        let fee = mandate.amount * (fee_bps as i128) / BPS_DENOMINATOR;
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

    pub fn subscriber_mandates(env: Env, subscriber: Address) -> Vec<u64> {
        env.storage()
            .persistent()
            .get(&DataKey::SubscriberMandates(subscriber))
            .unwrap_or_else(|| Vec::new(&env))
    }

    pub fn merchant_mandates(env: Env, merchant: Address) -> Vec<u64> {
        env.storage()
            .persistent()
            .get(&DataKey::MerchantMandates(merchant))
            .unwrap_or_else(|| Vec::new(&env))
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

    fn index_push(env: &Env, key: DataKey, id: u64) {
        let mut list: Vec<u64> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(env));
        list.push_back(id);
        env.storage().persistent().set(&key, &list);
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSIST_THRESHOLD, PERSIST_BUMP);
    }
}
