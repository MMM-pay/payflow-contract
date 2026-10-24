//! Subscription billing engine for Payflow recurring payments.
//!
//! Subscribers open mandates against merchant plans. Charges are triggered
//! permissionlessly when due, debiting the subscriber's vault balance and
//! splitting proceeds between merchant, protocol fees, and optional keeper tips.

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
mod interfaces;
#[allow(missing_docs)]
mod types;

#[cfg(test)]
mod test;
#[cfg(test)]
mod test_events;

pub use error::Error;
pub use events::{
    Cancelled, Charged, FeeChanged, MandateCompleted, MandateEnded, MandateUpdated, PauseChanged,
    Refunded, SeatsChanged, Subscribed, Upgraded,
};
pub use interfaces::{PlanRegistryClient, VaultClient};
pub use types::{ChargeQuote, ChargeResult, DataKey, Mandate, MandateStatus, Plan};

use payflow_shared::MAX_KEEPER_BPS;
use soroban_sdk::{contract, contractimpl, token, Address, Env, String, Vec};

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

#[allow(missing_docs)]
mod contract {
    use super::*;

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
        /// The version of this contract's crate.
        pub fn version(env: Env) -> String {
            String::from_str(&env, env!("CARGO_PKG_VERSION"))
        }

        /// Runs once, in the same transaction that deploys the contract. With a
        /// separate `initialize` call, anyone watching the ledger could call it
        /// between the deploy and the owner's own call and make themselves admin.
        ///
        /// # Auth
        /// None (invoked during deployment).
        ///
        /// # Events
        /// Emits no events.
        ///
        /// # Errors
        /// Returns [`Error::FeeTooHigh`] if `fee_bps > MAX_FEE_BPS`.
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
        ///
        /// # Auth
        /// Requires authorization from `subscriber`.
        ///
        /// # Events
        /// Emits [`MandateUpdated`] and [`Subscribed`].
        ///
        /// # Errors
        /// Returns [`Error::PlanInactive`] if the target plan is inactive, or
        /// [`Error::NotInitialized`] if configuration keys are missing.
        pub fn subscribe(
            env: Env,
            subscriber: Address,
            plan_id: u64,
            max_charges: u32,
            max_total: i128,
        ) -> Result<u64, Error> {
            subscriber.require_auth();

            if max_total < 0 {
                return Err(Error::InvalidMaxTotal);
            }

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

            // The registry already caps this, but clamp again here: the mandate's
            // tip must never exceed MAX_KEEPER_BPS even if the registry contract
            // passed to this deployment does not enforce the cap.
            let keeper_bps = plan.keeper_bps.min(MAX_KEEPER_BPS);

            let now = env.ledger().timestamp();
            let mandate = Mandate {
                id,
                subscriber: subscriber.clone(),
                plan_id,
                merchant: plan.merchant.clone(),
                token: plan.token.clone(),
                amount: plan.amount,
                period: plan.period,
                created_at: now,
                next_charge: now,
                last_charge: 0,
                charges_made: 0,
                max_charges,
                fee_bps,
                keeper_bps,
                paused_until: 0,
                refunded: 0,
                seats: 1,
                max_total,
                total_spent: 0,

                max_pause_secs: 0,
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

            MandateUpdated {
                mandate_id: id,
                mandate,
            }
            .publish(&env);

            Subscribed {
                mandate_id: id,
                subscriber,
                merchant: plan.merchant,
                plan_id,
                max_total,
                amount: plan.amount,
                period: plan.period,
                next_charge: now,
                max_charges,
                fee_bps,
                keeper_bps,
            }
            .publish(&env);

            Ok(id)
        }

        /// Execute a due charge.
        ///
        /// Open to any caller: the mandate itself is the authorization, so the
        /// merchant, a keeper bot, or the subscriber may all settle it. `keeper`
        /// must authorize this call, because the plan's tip is paid to it; that is
        /// the only signature required, and it grants no power over the mandate.
        /// Competing keepers are what keeps billing alive without trusting one
        /// operator to stay online.
        ///
        /// The tip is paid out of the merchant's share (see the plan's
        /// `keeper_bps`), so the subscriber is debited exactly `mandate.amount`
        /// whether or not a tip is paid, and the protocol fee is unaffected.
        ///
        /// `next_charge` advances to `now + period` rather than
        /// `next_charge + period`. A keeper outage therefore delays billing instead
        /// of accumulating a backlog that could drain a vault in one burst. The
        /// tradeoff is slow forward drift, which favours the subscriber.
        ///
        /// # Auth
        /// Requires authorization from `keeper`.
        ///
        /// # Events
        /// Emits [`MandateUpdated`] and [`Charged`]. If this charge completes the mandate, also emits [`MandateCompleted`].
        ///
        /// # Errors
        /// Returns [`Error::MandateNotFound`] if the mandate does not exist,
        /// [`Error::MandateNotActive`] if the mandate is not active,
        /// [`Error::NotDue`] if the ledger timestamp is before `next_charge`,
        /// [`Error::MaxChargesReached`] if the mandate reached its cap, or
        /// [`Error::AmountOverflow`] if math overflows.
        pub fn charge(env: Env, keeper: Address, mandate_id: u64) -> Result<ChargeResult, Error> {
            keeper.require_auth();

            let mut mandate = Self::read_mandate(&env, mandate_id)?;

            let now = env.ledger().timestamp();
            if !Self::active_at(&mandate, now) {
                return Err(Error::MandateNotActive);
            }

            if now < mandate.next_charge {
                return Err(Error::NotDue);
            }
            if mandate.max_charges != 0 && mandate.charges_made >= mandate.max_charges {
                return Err(Error::MaxChargesReached);
            }
            // A charge that would cross the total-spend cap is not settled at
            // all: a partial debit would bill the subscriber for something the
            // merchant never agreed to deliver.
            let next_total = mandate
                .total_spent
                .checked_add(mandate.amount)
                .ok_or(Error::AmountOverflow)?;
            if mandate.max_total != 0 && next_total > mandate.max_total {
                return Err(Error::MaxChargesReached);
            }

            let (fee, tip, merchant_amount) = Self::fee_split(&mandate)?;
            let charges_made = mandate
                .charges_made
                .checked_add(1)
                .ok_or(Error::AmountOverflow)?;
            let next_charge = now
                .checked_add(mandate.period)
                .ok_or(Error::AmountOverflow)?;
            let completed = (mandate.max_charges != 0 && charges_made >= mandate.max_charges)
                || (mandate.max_total != 0 && next_total >= mandate.max_total);

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

            if tip > 0 {
                vault.debit(&mandate.subscriber, &mandate.token, &keeper, &tip);
            }

            // Settling through a scheduled pause that has run its course resumes
            // the mandate: the subscriber never has to send a transaction just to
            // come back from a pause they themselves timed.
            if mandate.status == MandateStatus::Paused {
                mandate.status = MandateStatus::Active;
                mandate.paused_until = 0;
            }

            mandate.charges_made = charges_made;
            mandate.total_spent = next_total;
            mandate.last_charge = now;
            mandate.next_charge = next_charge;

            if completed {
                mandate.status = MandateStatus::Completed;
            }

            Self::write_mandate(&env, &mandate);

            MandateUpdated {
                mandate_id,
                mandate: mandate.clone(),
            }
            .publish(&env);

            Charged {
                mandate_id,
                subscriber: mandate.subscriber.clone(),
                merchant: mandate.merchant.clone(),
                amount: merchant_amount,
                fee,
                tip,
                keeper,
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

            Ok(ChargeResult {
                merchant_amount,
                fee,
                charges_made,
                next_charge,
                completed,
            })
        }

        /// Move a mandate onto a different plan.
        ///
        /// Moving to a more expensive plan collects the price difference for the
        /// part of the current period that is still ahead, prorated and rounded
        /// in the subscriber's favour. Moving to a cheaper plan takes effect at
        /// the next charge and never refunds anything: the subscriber has
        /// already paid for the period they are in.
        ///
        /// The rules that keep this safe:
        ///
        /// - Only the subscriber may call it, and only for their own mandate.
        /// - The new plan must be active, denominated in the same token (a
        ///   mandate's token is frozen when it is opened) and belong to the same
        ///   merchant (the mandate's merchant decides who is paid and who may
        ///   refund).
        /// - The amount charged now is the price difference multiplied by the
        ///   fraction of the current period that is left, truncated towards zero.
        ///   The subscriber therefore never pays more than the exact difference,
        ///   and never less than zero.
        /// - The billing date does not move: the next charge still falls due at
        ///   `next_charge`, and carries the new plan's price from then on.
        ///
        /// See [ADR 0006](../../docs/adr/0006-prorated-upgrade-charging.md) for
        /// the rounding rule and why it is written this way.
        ///
        /// # Auth
        /// Requires authorization from `subscriber`.
        ///
        /// # Events
        /// Emits [`MandateUpdated`] and [`Upgraded`].
        ///
        /// # Errors
        /// Returns [`Error::MandateNotFound`] if the mandate does not exist,
        /// [`Error::NotSubscriber`] if the caller is not the subscriber,
        /// [`Error::MandateNotActive`] if the mandate is not active,
        /// [`Error::PlanInactive`] if the new plan is inactive,
        /// [`Error::DifferentToken`] if the new plan is denominated in a different token,
        /// [`Error::DifferentMerchant`] if the new plan belongs to a different merchant, or
        /// [`Error::AmountOverflow`] if math overflows.
        pub fn upgrade(
            env: Env,
            subscriber: Address,
            mandate_id: u64,
            new_plan_id: u64,
        ) -> Result<(), Error> {
            subscriber.require_auth();

            let mut mandate = Self::read_mandate(&env, mandate_id)?;
            if mandate.subscriber != subscriber {
                return Err(Error::NotSubscriber);
            }

            let now = env.ledger().timestamp();
            if !Self::active_at(&mandate, now) {
                return Err(Error::MandateNotActive);
            }

            let registry: Address = Self::cfg_address(&env, DataKey::PlanRegistry)?;
            let plan = PlanRegistryClient::new(&env, &registry).get_plan(&new_plan_id);
            if !plan.active {
                return Err(Error::PlanInactive);
            }
            if plan.token != mandate.token {
                return Err(Error::DifferentToken);
            }
            if plan.merchant != mandate.merchant {
                return Err(Error::DifferentMerchant);
            }

            let old_plan_id = mandate.plan_id;
            let old_amount = mandate.amount;
            let old_period = mandate.period;

            // The part of the current period that is still ahead of us. A mandate
            // that was never charged has `next_charge` in the past, so nothing is
            // left of the current period and nothing is prorated: the old price
            // was never collected.
            let remaining = mandate.next_charge.saturating_sub(now).min(old_period);

            // The prorated price difference, truncated towards zero. Truncation
            // is what rounds in the subscriber's favour: the subscriber pays at
            // most the exact difference for the time that is left.
            let difference = plan.amount - old_amount;
            let charged: i128 = if difference > 0 && old_period > 0 {
                difference
                    .checked_mul(remaining as i128)
                    .ok_or(Error::AmountOverflow)?
                    / old_period as i128
            } else {
                // A downgrade collects nothing now: it takes effect at the next
                // charge and never refunds the difference already paid.
                0
            };

            if charged > 0 {
                // The protocol fee on the difference, at the rate frozen into the
                // mandate. The keeper tip is not paid here: no charge was
                // settled, so nobody earned it.
                let fee = charged
                    .checked_mul(mandate.fee_bps as i128)
                    .ok_or(Error::AmountOverflow)?
                    / BPS_DENOMINATOR;
                let merchant_gross = charged - fee;

                let vault_addr: Address = Self::cfg_address(&env, DataKey::Vault)?;
                let vault = VaultClient::new(&env, &vault_addr);
                vault.debit(
                    &subscriber,
                    &mandate.token,
                    &mandate.merchant,
                    &merchant_gross,
                );
                if fee > 0 {
                    let fee_to: Address = Self::cfg_address(&env, DataKey::FeeTo)?;
                    vault.debit(&subscriber, &mandate.token, &fee_to, &fee);
                }
            }

            // The mandate carries the new plan's terms from now on. A downgrade
            // therefore bills the cheaper price at the next charge, and an
            // upgrade has already collected the difference for the rest of the
            // current period.
            mandate.plan_id = new_plan_id;
            mandate.amount = plan.amount;
            mandate.period = plan.period;
            mandate.keeper_bps = plan.keeper_bps.min(MAX_KEEPER_BPS);
            Self::write_mandate(&env, &mandate);

            MandateUpdated {
                mandate_id,
                mandate: mandate.clone(),
            }
            .publish(&env);

            Upgraded {
                mandate_id,
                subscriber,
                merchant: mandate.merchant,
                old_plan_id,
                new_plan_id,
                old_amount,
                new_amount: plan.amount,
                charged,
                period: plan.period,
                next_charge: mandate.next_charge,
            }
            .publish(&env);

            Ok(())
        }

        /// Terminate a mandate permanently. Only the subscriber may do this, and it
        /// can be done at any time without the merchant's involvement.
        ///
        /// # Auth
        /// Requires authorization from `subscriber`.
        ///
        /// # Events
        /// Emits [`MandateUpdated`] and [`Cancelled`].
        ///
        /// # Errors
        /// Returns [`Error::MandateNotFound`] if the mandate does not exist,
        /// [`Error::NotSubscriber`] if the caller is not the subscriber, or
        /// [`Error::MandateNotActive`] if the mandate is already cancelled or completed.
        pub fn cancel(env: Env, subscriber: Address, mandate_id: u64) -> Result<(), Error> {
            subscriber.require_auth();

            let mut mandate = Self::read_mandate(&env, mandate_id)?;
            if mandate.subscriber != subscriber {
                return Err(Error::NotSubscriber);
            }
            if mandate.status == MandateStatus::Cancelled
                || mandate.status == MandateStatus::Completed
            {
                return Err(Error::MandateNotActive);
            }

            mandate.status = MandateStatus::Cancelled;
            Self::write_mandate(&env, &mandate);

            MandateUpdated {
                mandate_id,
                mandate: mandate.clone(),
            }
            .publish(&env);

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
        ///
        /// # Auth
        /// Requires authorization from `merchant`.
        ///
        /// # Events
        /// Emits [`MandateUpdated`] and [`MandateEnded`].
        ///
        /// # Errors
        /// Returns [`Error::MandateNotFound`] if the mandate does not exist,
        /// [`Error::NotMerchant`] if the caller is not the merchant, or
        /// [`Error::MandateNotActive`] if the mandate is already cancelled or completed.
        pub fn end_mandate(env: Env, merchant: Address, mandate_id: u64) -> Result<(), Error> {
            merchant.require_auth();

            let mut mandate = Self::read_mandate(&env, mandate_id)?;
            if mandate.merchant != merchant {
                return Err(Error::NotMerchant);
            }
            if mandate.status == MandateStatus::Cancelled
                || mandate.status == MandateStatus::Completed
            {
                return Err(Error::MandateNotActive);
            }

            mandate.status = MandateStatus::Cancelled;
            Self::write_mandate(&env, &mandate);

            MandateUpdated {
                mandate_id,
                mandate: mandate.clone(),
            }
            .publish(&env);

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
        ///
        /// An open-ended pause: a mandate paused this way stays paused until it is
        /// resumed here. `pause_until` is the alternative for a pause that should
        /// lift by itself.
        ///
        /// # Auth
        /// Requires authorization from `subscriber`.
        ///
        /// # Events
        /// Emits [`MandateUpdated`] and [`PauseChanged`].
        ///
        /// # Errors
        /// Returns [`Error::MandateNotFound`] if the mandate does not exist,
        /// [`Error::NotSubscriber`] if the caller is not the subscriber, or
        /// [`Error::MandateNotActive`] if the mandate cannot be transitioned (e.g. cancelled/completed).
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
            // This call owns the pause schedule: pausing opens an indefinite
            // pause, resuming cancels whatever schedule was set before.
            mandate.paused_until = 0;

            Self::write_mandate(&env, &mandate);

            MandateUpdated {
                mandate_id,
                mandate,
            }
            .publish(&env);

            PauseChanged {
                mandate_id,
                subscriber,
                paused,
            }
            .publish(&env);
            Ok(())
        }

        /// Pause billing until a given date, without needing a transaction from
        /// the subscriber afterwards.
        ///
        /// The mandate is paused like `set_paused` does, but it carries a
        /// schedule: once the ledger clock reaches `until`, `charge` and `is_due`
        /// treat it as Active again on their own. The resume is therefore free —
        /// no keeper window is missed because the subscriber forgot to resume,
        /// which is what loses merchants their returning customers.
        ///
        /// `next_charge` is pushed to at least `until`, so billing continues from
        /// the end of the pause rather than from a date that has already passed.
        ///
        /// # Auth
        /// Requires authorization from `subscriber`.
        ///
        /// # Events
        /// Emits [`MandateUpdated`] and [`PauseChanged`].
        ///
        /// # Errors
        /// Returns [`Error::MandateNotFound`] if the mandate does not exist,
        /// [`Error::NotSubscriber`] if the caller is not the subscriber,
        /// [`Error::MandateNotActive`] if the mandate is cancelled or completed, or
        /// [`Error::InvalidUntil`] if `until` is earlier than or equal to current timestamp.
        pub fn pause_until(
            env: Env,
            subscriber: Address,
            mandate_id: u64,
            until: u64,
        ) -> Result<(), Error> {
            subscriber.require_auth();

            let mut mandate = Self::read_mandate(&env, mandate_id)?;
            if mandate.subscriber != subscriber {
                return Err(Error::NotSubscriber);
            }
            if mandate.status == MandateStatus::Cancelled
                || mandate.status == MandateStatus::Completed
            {
                return Err(Error::MandateNotActive);
            }

            let now = env.ledger().timestamp();
            if until <= now {
                return Err(Error::InvalidUntil);
            }

            mandate.status = MandateStatus::Paused;
            mandate.paused_until = until;
            mandate.next_charge = mandate.next_charge.max(until);
            Self::write_mandate(&env, &mandate);

            MandateUpdated {
                mandate_id,
                mandate,
            }
            .publish(&env);

            PauseChanged {
                mandate_id,
                subscriber,
                paused: true,
            }
            .publish(&env);
            Ok(())
        }

        /// Refund part of what this mandate paid the merchant.
        ///
        /// The merchant returns money from its own wallet, straight to the
        /// subscriber's wallet, and the contract records the refund on the
        /// mandate and emits `Refunded`, so the dispute trail lives on chain
        /// instead of in ad-hoc transfers nobody can attribute. It deliberately
        /// does not touch the vault: crediting internal balances without moving
        /// tokens would break the vault's invariant that internal balances sum to
        /// its holdings.
        ///
        /// Total refunds are capped at what the merchant actually collected for
        /// this mandate (price minus the frozen fee and tip, per settled charge),
        /// so a merchant can never "refund" more than it ever received. The cap
        /// holds regardless of mandate status, which is why a cancelled mandate
        /// remains refundable.
        ///
        /// # Auth
        /// Requires authorization from `merchant`.
        ///
        /// # Events
        /// Emits [`MandateUpdated`] and [`Refunded`].
        ///
        /// # Errors
        /// Returns [`Error::MandateNotFound`] if the mandate does not exist,
        /// [`Error::NotMerchant`] if the caller is not the merchant,
        /// [`Error::InvalidAmount`] if `amount <= 0`,
        /// [`Error::RefundExceedsReceived`] if refund exceeds collected revenue, or
        /// [`Error::AmountOverflow`] if math overflows.
        pub fn refund(
            env: Env,
            merchant: Address,
            mandate_id: u64,
            amount: i128,
        ) -> Result<(), Error> {
            merchant.require_auth();

            let mut mandate = Self::read_mandate(&env, mandate_id)?;
            if mandate.merchant != merchant {
                return Err(Error::NotMerchant);
            }
            if amount <= 0 {
                return Err(Error::InvalidAmount);
            }

            let remaining = Self::merchant_received(&mandate)? - mandate.refunded;
            if amount > remaining {
                return Err(Error::RefundExceedsReceived);
            }

            token::TokenClient::new(&env, &mandate.token).transfer(
                &merchant,
                &mandate.subscriber,
                &amount,
            );

            mandate.refunded += amount;
            Self::write_mandate(&env, &mandate);

            MandateUpdated {
                mandate_id,
                mandate: mandate.clone(),
            }
            .publish(&env);

            Refunded {
                mandate_id,
                subscriber: mandate.subscriber.clone(),
                merchant,
                amount,
                refunded_total: mandate.refunded,
            }
            .publish(&env);
            Ok(())
        }

        /// Change the seat count on an active mandate.
        ///
        /// Seat-based plans price by the number of users on a team. This function
        /// lets the subscriber grow or shrink the seat count; the new count takes
        /// effect from the next charge so no proration is needed. Only the
        /// subscriber may call it, the mandate must be active (not cancelled or
        /// completed), and `seats` must be at least 1.
        ///
        /// # Auth
        /// Requires authorization from `subscriber`.
        ///
        /// # Events
        /// Emits [`MandateUpdated`] and [`SeatsChanged`].
        ///
        /// # Errors
        /// Returns [`Error::MandateNotFound`] if the mandate does not exist,
        /// [`Error::NotSubscriber`] if the caller is not the subscriber,
        /// [`Error::MandateNotActive`] if the mandate is cancelled or completed, or
        /// [`Error::InvalidSeats`] if `seats` is zero.
        pub fn set_seats(
            env: Env,
            subscriber: Address,
            mandate_id: u64,
            seats: u32,
        ) -> Result<(), Error> {
            subscriber.require_auth();

            if seats == 0 {
                return Err(Error::InvalidSeats);
            }

            let mut mandate = Self::read_mandate(&env, mandate_id)?;
            if mandate.subscriber != subscriber {
                return Err(Error::NotSubscriber);
            }
            if mandate.status == MandateStatus::Cancelled
                || mandate.status == MandateStatus::Completed
            {
                return Err(Error::MandateNotActive);
            }

            let old_seats = mandate.seats;
            mandate.seats = seats;
            Self::write_mandate(&env, &mandate);

            MandateUpdated {
                mandate_id,
                mandate: mandate.clone(),
            }
            .publish(&env);

            SeatsChanged {
                mandate_id,
                subscriber,
                old_seats,
                new_seats: seats,
            }
            .publish(&env);

            Ok(())
        }

        /// Change the protocol fee for mandates opened from now on.
        ///
        /// Safe to call at any time: every open mandate carries the fee it was
        /// created with, so this cannot reprice an existing subscriber. The
        /// `MAX_FEE_BPS` ceiling still applies.
        ///
        /// # Auth
        /// Requires authorization from the contract admin.
        ///
        /// # Events
        /// Emits [`FeeChanged`].
        ///
        /// # Errors
        /// Returns [`Error::NotInitialized`] if no admin is set, or
        /// [`Error::FeeTooHigh`] if `new_fee_bps > MAX_FEE_BPS`.
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

        /// Look up a mandate by its unique ID.
        pub fn get_mandate(env: Env, mandate_id: u64) -> Result<Mandate, Error> {
            Self::read_mandate(&env, mandate_id)
        }

        /// Estimate when the mandate's final capped charge is scheduled.
        ///
        /// The estimate follows the current schedule, including any drift from
        /// late charges. Returns `None` for an open-ended mandate and its actual
        /// final charge time once the cap has been reached.
        ///
        /// # Errors
        /// Returns [`Error::MandateNotFound`] if the mandate does not exist, or
        /// [`Error::ScheduleOverflow`] if the remaining schedule overflows `u64`.
        pub fn estimated_end(env: Env, mandate_id: u64) -> Result<Option<u64>, Error> {
            let mandate = Self::read_mandate(&env, mandate_id)?;
            if mandate.max_charges == 0 {
                return Ok(None);
            }

            let remaining_charges = mandate.max_charges.saturating_sub(mandate.charges_made);
            if remaining_charges == 0 {
                return Ok(Some(mandate.last_charge));
            }

            let remaining_periods = u64::from(remaining_charges - 1);
            let offset = remaining_periods
                .checked_mul(mandate.period)
                .ok_or(Error::ScheduleOverflow)?;
            let estimated_end = mandate
                .next_charge
                .checked_add(offset)
                .ok_or(Error::ScheduleOverflow)?;

            Ok(Some(estimated_end))
        }

        /// True when `charge` would succeed on timing and status grounds. Does not
        /// check vault balance; a funded-ness check is the caller's job.
        pub fn is_due(env: Env, mandate_id: u64) -> Result<bool, Error> {
            let m = Self::read_mandate(&env, mandate_id)?;
            Ok(Self::mandate_is_due(&m, env.ledger().timestamp()))
        }

        /// Read-only preview of what `charge` would pay right now.
        ///
        /// `amount` is the subscriber's total debit, `fee` the protocol share
        /// and `merchant_amount` the merchant's net share after both the fee
        /// and the keeper tip. `due_at` is the mandate's next charge
        /// timestamp, and `chargeable` mirrors `is_due`: active status, cap
        /// not reached, and the ledger time has reached `due_at`. Like
        /// `is_due`, it does not check the vault balance. Quoting reads
        /// storage but never writes it.
        ///
        /// The split comes from the same private helper `charge` uses, so a
        /// client can display the exact integers settlement will move rather
        /// than reimplementing the rounding.
        pub fn quote_charge(env: Env, mandate_id: u64) -> Result<ChargeQuote, Error> {
            let m = Self::read_mandate(&env, mandate_id)?;
            let (fee, _tip, merchant_amount) = Self::fee_split(&m)?;
            let now = env.ledger().timestamp();
            let chargeable = Self::active_at(&m, now)
                && (m.max_charges == 0 || m.charges_made < m.max_charges)
                && (m.max_total == 0 || m.total_spent.saturating_add(m.amount) <= m.max_total)
                && now >= m.next_charge;

            Ok(ChargeQuote {
                amount: m.amount,
                fee,
                merchant_amount,
                due_at: m.next_charge,
                chargeable,
            })
        }

        /// Permanently cancel every cancellable mandate in one page of the
        /// subscriber's index. The scan is capped at `MAX_PAGE`; reading index
        /// slots directly avoids touching the separate count entry, keeping a
        /// full page within the 100-entry transaction limit.
        ///
        /// Mandates already cancelled or completed are skipped, so calling it
        /// twice is harmless. Returns how many this call cancelled.
        ///
        /// # Auth
        /// Requires authorization from `subscriber`.
        ///
        /// # Events
        /// Emits one [`Cancelled`] per mandate it cancels.
        pub fn cancel_page(
            env: Env,
            subscriber: Address,
            start: u32,
            limit: u32,
        ) -> Result<u32, Error> {
            subscriber.require_auth();

            let end = start.saturating_add(limit.min(MAX_PAGE));
            let mut cancelled = 0;
            for position in start..end {
                let slot = DataKey::SubscriberMandate(subscriber.clone(), position);
                let Some(mandate_id) = env.storage().persistent().get::<_, u64>(&slot) else {
                    continue;
                };
                let mut mandate = Self::read_mandate(&env, mandate_id)?;
                if mandate.subscriber != subscriber {
                    return Err(Error::NotSubscriber);
                }
                if mandate.status == MandateStatus::Cancelled
                    || mandate.status == MandateStatus::Completed
                {
                    continue;
                }

                mandate.status = MandateStatus::Cancelled;
                Self::write_mandate(&env, &mandate);
                Cancelled {
                    mandate_id,
                    subscriber: subscriber.clone(),
                    charges_made: mandate.charges_made,
                }
                .publish(&env);
                cancelled += 1;
            }

            Ok(cancelled)
        }

        /// Ids of due mandates opened against `merchant`'s plans, in index
        /// order. A keeper settling one merchant otherwise has to read every
        /// mandate the merchant has ever had and test each one itself.
        ///
        /// Reads at most one page of index slots and their mandate entries,
        /// and answers exactly what `is_due` would for each.
        pub fn due_mandates(env: Env, merchant: Address, start: u32, limit: u32) -> Vec<u64> {
            let end = start.saturating_add(limit.min(MAX_PAGE));
            let now = env.ledger().timestamp();
            let mut due = Vec::new(&env);
            let storage = env.storage().persistent();

            // Avoid a separate count read: a full page reads at most 50 index
            // slots and 50 mandates, staying within the 100-entry footprint.
            for position in start..end {
                let Some(id) =
                    storage.get::<_, u64>(&DataKey::MerchantMandate(merchant.clone(), position))
                else {
                    continue;
                };
                let Some(mandate) = storage.get::<_, Mandate>(&DataKey::Mandate(id)) else {
                    continue;
                };
                if Self::mandate_is_due(&mandate, now) {
                    due.push_back(id);
                }
            }
            due
        }

        /// Total committed spend of `subscriber` in `token` per `period_secs`,
        /// normalising mandates with different billing periods to the period the
        /// caller asks about. Monthly and weekly mandates therefore add up to one
        /// figure the subscriber can fund their vault against.
        ///
        /// Only mandates that `charge` would treat as active are counted:
        /// cancelled, completed, indefinitely paused and not-yet-due-scheduled
        /// mandates contribute nothing. At most `MAX_PAGE` mandates (the first
        /// page of the subscriber's index) are read, so the call stays well
        /// inside the ledger's 100-entry footprint limit.
        pub fn committed_per_period(
            env: Env,
            subscriber: Address,
            token: Address,
            period_secs: u64,
        ) -> Result<i128, Error> {
            let now = env.ledger().timestamp();
            let len = Self::index_len(&env, DataKey::SubscriberMandateCount(subscriber.clone()));
            let ids = Self::index_page(&env, len, 0, MAX_PAGE, |i| {
                DataKey::SubscriberMandate(subscriber.clone(), i)
            });

            let mut total: i128 = 0;
            for id in ids.iter() {
                let Some(m) = env
                    .storage()
                    .persistent()
                    .get::<_, Mandate>(&DataKey::Mandate(id))
                else {
                    continue;
                };
                if m.token != token || !Self::active_at(&m, now) || m.period == 0 {
                    continue;
                }
                // Integer math only, truncated downwards: a subscriber who plans
                // against this figure is never told they have more committed than
                // they do.
                let contribution = m
                    .amount
                    .checked_mul(period_secs as i128)
                    .ok_or(Error::AmountOverflow)?
                    / m.period as i128;
                total = total
                    .checked_add(contribution)
                    .ok_or(Error::AmountOverflow)?;
            }
            Ok(total)
        }

        /// How many mandates `subscriber` has opened, in any state.
        pub fn subscriber_mandate_count(env: Env, subscriber: Address) -> u32 {
            Self::index_len(&env, DataKey::SubscriberMandateCount(subscriber))
        }

        /// Ids of the mandates `subscriber` opened, oldest first, starting at
        /// position `start`. Returns at most `min(limit, MAX_PAGE)` ids.
        pub fn subscriber_mandates(
            env: Env,
            subscriber: Address,
            start: u32,
            limit: u32,
        ) -> Vec<u64> {
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

        /// The id the next opened mandate will get. Mandates are numbered from 1,
        /// so `next_mandate_id() - 1` is the number of mandates opened so far.
        pub fn next_mandate_id(env: Env) -> Result<u64, Error> {
            env.storage()
                .instance()
                .get(&DataKey::NextMandateId)
                .ok_or(Error::NotInitialized)
        }

        /// Current protocol fee rate in basis points.
        pub fn fee_bps(env: Env) -> Result<u32, Error> {
            env.storage()
                .instance()
                .get(&DataKey::FeeBps)
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
            Self::cfg_address(&env, DataKey::Admin)
        }

        /// Address of the configured plan registry contract.
        pub fn plan_registry(env: Env) -> Result<Address, Error> {
            Self::cfg_address(&env, DataKey::PlanRegistry)
        }

        /// Address of the configured vault contract.
        pub fn vault(env: Env) -> Result<Address, Error> {
            Self::cfg_address(&env, DataKey::Vault)
        }
    }

    impl Subscription {
        /// Whether `charge` may treat this mandate as active at `now`. A mandate
        /// paused until a date resumes by itself once that date passes; a pause
        /// without a date (`paused_until == 0`) stays in force until `set_paused`
        /// resumes it manually.
        /// The protocol fee, the keeper tip and the merchant's net share for
        /// one charge of `m`, all from the rates frozen into the mandate when
        /// the subscriber opened it. `charge` and `quote_charge` both call
        /// this, so a quote can never round differently from settlement.
        fn fee_split(m: &Mandate) -> Result<(i128, i128, i128), Error> {
            let fee = m
                .amount
                .checked_mul(m.fee_bps as i128)
                .ok_or(Error::AmountOverflow)?
                / BPS_DENOMINATOR;
            let merchant_gross = m.amount - fee;

            // Tip is a share of the merchant's own money, capped again here so
            // a mandate can never pay out more than MAX_KEEPER_BPS of it.
            let tip_bps = m.keeper_bps.min(MAX_KEEPER_BPS);
            let tip = merchant_gross
                .checked_mul(tip_bps as i128)
                .ok_or(Error::AmountOverflow)?
                / BPS_DENOMINATOR;
            Ok((fee, tip, merchant_gross - tip))
        }

        /// Whether `charge` would settle this mandate at `now`: active,
        /// inside both caps and past its next charge date. `is_due` and
        /// `due_mandates` both answer from this, so they cannot disagree.
        fn mandate_is_due(m: &Mandate, now: u64) -> bool {
            Self::active_at(m, now)
                && (m.max_charges == 0 || m.charges_made < m.max_charges)
                && (m.max_total == 0 || m.total_spent.saturating_add(m.amount) <= m.max_total)
                && now >= m.next_charge
        }

        fn active_at(m: &Mandate, now: u64) -> bool {
            match m.status {
                MandateStatus::Active => true,
                MandateStatus::Paused => m.paused_until != 0 && now >= m.paused_until,
                _ => false,
            }
        }

        /// What the merchant has actually collected for this mandate so far: the
        /// per-charge share (price minus the protocol fee minus the keeper tip,
        /// all frozen into the mandate at subscribe time) times the number of
        /// settled charges. This is the hard ceiling on `refund`.
        fn merchant_received(m: &Mandate) -> Result<i128, Error> {
            let fee = m.amount * (m.fee_bps as i128) / BPS_DENOMINATOR;
            let gross = m.amount - fee;
            let tip = gross
                .checked_mul(m.keeper_bps.min(MAX_KEEPER_BPS) as i128)
                .ok_or(Error::AmountOverflow)?
                / BPS_DENOMINATOR;
            (gross - tip)
                .checked_mul(m.charges_made as i128)
                .ok_or(Error::AmountOverflow)
        }

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
}

pub use contract::{Subscription, SubscriptionClient};
