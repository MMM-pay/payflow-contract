//! Subscription billing engine for Payflow recurring payments.
//!
//! Subscribers open mandates against merchant plans. Charges are triggered
//! permissionlessly when due, debiting the subscriber's vault balance and
//! splitting proceeds between merchant, protocol fees, and optional keeper tips.
//!
//! ## Resource costs
//!
//! All storage TTLs are expressed in ledgers (~5s each). Instance storage
//! (configuration) uses a 30-day bump with a 29-day threshold. Persistent
//! storage (mandates, indexes) uses a 90-day bump with an 89-day threshold.
//!
//! | Entry point | Instance reads | Instance writes | Persistent reads | Persistent writes | Notes |
//! |---|---|---|---|---|---|
//! | `__constructor` | 0 | 6 (config keys) | 0 | 0 | Extends instance TTL |
//! | `subscribe` | 4 (config) | 1 (NextMandateId) | 1 (Plan) | 4–6 (Mandate, 3 index slots, 1 count, optional grace) | Plus registry grace read; extends TTLs |
//! | `subscribe_with_id` | 4 (config) | 1 (NextMandateId) | 1 (Plan) | 4–6 (Mandate, 3 index slots, 1 count, optional grace) | Explicit ID checked against tombstones; extends TTLs |
//! | `subscribe_at` | 4 (config) | 1 (NextMandateId) | 1 (Plan) | 4–6 (Mandate, 3 index slots, 1 count, optional grace) | Plus registry grace read; extends TTLs |
//! | `subscribe_with_deposit` | 4 (config) | 1 (NextMandateId) | 1 (Plan) | 4–6 (Mandate, 3 index slots, 1 count, optional grace) | Plus registry grace read and vault deposit; extends TTLs |
//! | `delete_mandate` | 0 | 0 | 1 (Mandate) | 1 (MandateTombstone, removes Mandate + side keys) | Terminal mandates only; extends tombstone TTL |
//! | `charge` | 4 (config) | 0 | 2–4 (Mandate, Plan, Vault balance) | 2–3 (Mandate, Vault balance) | Vault debit; extends persistent TTLs |
//! | `upgrade` | 4 (config) | 0 | 2–4 (Mandate, Plan, pause view) | 2–3 (Mandate, index slots) | Extends persistent TTLs; vault debit if prorated |
//! | `cancel` / `cancel_with_reason` | 0 | 0 | 1 (Mandate) | 1–2 (Mandate, index) | Extends persistent TTL |
//! | `cancel_page` | 0 | 0 | ≤100 (index + Mandates) | ≤50 (Mandates) | Page ≤50, extends persistent TTLs |
//! | `propose_mandate_owner` | 0 | 0 | 1 (Mandate) | 2 (PendingOwner, Position) | Extends persistent TTLs |
//! | `accept_mandate_owner` | 0 | 0 | 3 (PendingOwner, Position, Mandate) | 3 (Mandate, index slots) | Extends persistent TTLs |
//! | `end_mandate` | 0 | 0 | 1 (Mandate) | 1–2 (Mandate, index) | Extends persistent TTL |
//! | `set_paused` | 0 | 0 | 1 (Mandate) | 1 (Mandate) | Extends persistent TTL |
//! | `pause_until` | 0 | 0 | 1 (Mandate) | 1 (Mandate) | Extends persistent TTL |
//! | `refund` | 0 | 0 | 1 (Mandate) | 1 (Mandate) | Token transfer merchant→subscriber |
//! | `set_seats` | 0 | 0 | 1 (Mandate) | 1 (Mandate) | Extends persistent TTL |
//! | `set_fee_bps` | 1 (Admin) | 1 (FeeBps) | 0 | 0 | Admin only, extends instance TTL |
//! | `set_protocol_paused` | 2 (Admin, ProtocolPaused) | 0–1 (ProtocolPaused) | 0 | 0 | Admin only, extends instance TTL |
//! | `protocol_paused` | 1 (ProtocolPaused) | 0 | 0 | 0 | |
//! | `extend_merchant_mandates_ttl` | 0 | 0 | ≤101 (count, index slots, Mandates) | 0 | TTL bumps only; page ≤50 |
//! | `get_mandate` | 0 | 0 | 1 (Mandate) | 0 | |
//! | `estimated_end` | 0 | 0 | 1 (Mandate) | 0 | |
//! | `is_due` | 0 | 0 | 1–2 (Mandate, Plan) | 0 | |
//! | `quote_charge` | 0 | 0 | 1–2 (Mandate, Plan) | 0 | |
//! | `is_in_grace_period` | 0 | 0 | 1–2 (Mandate, MandateGracePeriod) | 0 | |
//! | `mandate_grace_period` | 0 | 0 | 2 (Mandate, MandateGracePeriod) | 0 | |
//! | `due_mandates` | 0 | 0 | ≤100 (index + Mandates) | 0 | Page ≤50 |
//! | `committed_per_period` | 0 | 0 | ≤50 (index + Mandates) | 0 | Page ≤50 |
//! | `subscriber_mandate_count` | 0 | 0 | 1 (count) | 0 | |
//! | `subscriber_mandates` | 0 | 0 | ≤50 (index) | 0 | Page ≤50 |
//! | `subscriber_mandate_summaries` | 0 | 0 | ≤100 (index + Mandates) | 0 | Page ≤50 |
//! | `merchant_mandate_count` | 0 | 0 | 1 (count) | 0 | |
//! | `merchant_mandates` | 0 | 0 | ≤50 (index) | 0 | Page ≤50 |
//! | `plan_mandate_count` | 0 | 0 | 1 (count) | 0 | |
//! | `plan_mandates` | 0 | 0 | ≤50 (index) | 0 | Page ≤50 |
//! | `plan_subscriber_count` | 0 | 0 | 1 (count) | 0 | |
//! | `set_max_charges` | 0 | 0 | 1 (Mandate) | 1–2 (Mandate, index) | Extends persistent TTL |
//! | `upgrade_wasm` | 1 (Admin) | 0 | 0 | 0 | Extends instance TTL |
//! | `next_mandate_id` | 1 (NextMandateId) | 0 | 0–N (tombstone/mandate check) | 0 | |
//! | `is_mandate_tombstoned` | 0 | 0 | 1 (MandateTombstone) | 0 | |
//! | `fee_bps` | 1 (FeeBps) | 0 | 0 | 0 | |
//! | `propose_admin` | 1 (Admin) | 1 (PendingAdmin) | 0 | 0 | Extends instance TTL |
//! | `accept_admin` | 2 (Admin, PendingAdmin) | 1 (Admin) | 0 | 0 | Removes PendingAdmin; extends instance TTL |
//! | `admin` | 1 (Admin) | 0 | 0 | 0 | |
//! | `plan_registry` | 1 (PlanRegistry) | 0 | 0 | 0 | |
//! | `vault` | 1 (Vault) | 0 | 0 | 0 | |

#![no_std]
#![deny(missing_docs)]
// `deny`, not `forbid`: soroban-sdk's #[contractimpl] expansion carries its
// own #[allow(unsafe_code)], which `forbid` cannot be overridden by. `deny`
// still fails the build on any unsafe block written in this crate.
#![deny(unsafe_code)]

mod charge;
#[allow(missing_docs)]
mod error;
#[allow(missing_docs)]
mod events;
#[allow(missing_docs)]
mod interfaces;
mod lifecycle;
#[allow(missing_docs)]
mod types;
mod views;

#[cfg(test)]
mod test;
#[cfg(test)]
mod test_errors;
#[cfg(test)]
mod test_events;

pub use error::Error;
pub use events::{
    Cancelled, ChargeFailed, Charged, CommittedRevenueChanged, ContractUpgraded, FeeChanged,
    MandateCompleted, MandateDeleted, MandateEnded, MandateTransferred, MandateUpdated,
    MaxChargesChanged, PauseChanged, ProtocolPauseChanged, Refunded, SeatsChanged, Subscribed,
    Upgraded,
};
pub use interfaces::{PlanRegistryClient, VaultClient};
pub use types::{
    ChargeQuote, ChargeResult, ChargeSimulation, DataKey, EstimatedEnd, Mandate, MandateStatus,
    MandateSummary, Plan, UpgradeRecord,
};

use payflow_shared::{MAX_KEEPER_BPS, MAX_REFERRAL_BPS};
use soroban_sdk::{
    contract, contractimpl, token, Address, BytesN, ContractExecutable, Env, String, Vec,
};

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

/// Cancellation code reported by [`Subscription::cancel`] and
/// [`Subscription::cancel_page`]: the subscriber gave no reason. Also accepted
/// by [`Subscription::cancel_with_reason`], so a client that always sends a
/// code can still let the subscriber skip the question.
pub const CANCEL_REASON_NONE: u32 = 0;
/// Cancellation code: the subscription cost more than the subscriber is
/// willing to pay.
pub const CANCEL_REASON_TOO_EXPENSIVE: u32 = 1;
/// Cancellation code: the subscriber stopped using the service.
pub const CANCEL_REASON_NOT_USING: u32 = 2;
/// Cancellation code: the service is missing something the subscriber needs.
pub const CANCEL_REASON_MISSING_FEATURE: u32 = 3;
/// Cancellation code: the subscriber was unhappy with the quality of the
/// service.
pub const CANCEL_REASON_SERVICE_QUALITY: u32 = 4;
/// Cancellation code: anything the codes above do not describe.
pub const CANCEL_REASON_OTHER: u32 = 5;
/// Highest documented cancellation code. The set is closed: a code above it is
/// rejected with [`Error::InvalidCancelReason`] and leaves the mandate
/// untouched, so an indexer can map every [`Cancelled`] event without a
/// catch-all branch.
pub const MAX_CANCEL_REASON: u32 = CANCEL_REASON_OTHER;

/// How far ahead of the current ledger timestamp billing may be scheduled
/// with `subscribe_at`: one year, in seconds. Far ahead enough to set up an
/// annual subscription in advance, close enough that a mandate cannot be
/// scheduled so far out that the subscriber forgets it exists.
pub const MAX_START_OFFSET: u64 = 365 * 24 * 60 * 60;

/// Nominal Stellar ledger close time, used only to translate a timestamp into
/// an expected ledger sequence for display (#287).
pub const SECONDS_PER_LEDGER: u64 = 5;

#[allow(missing_docs)]
mod contract {
    use super::{
        contract, contractimpl, token, Address, BytesN, Cancelled, ChargeFailed, ChargeQuote,
        ChargeResult, ChargeSimulation, Charged, CommittedRevenueChanged, ContractExecutable,
        ContractUpgraded, DataKey, Env, Error, EstimatedEnd, FeeChanged, Mandate, MandateCompleted,
        MandateDeleted, MandateEnded, MandateStatus, MandateSummary, MandateTransferred,
        MandateUpdated, MaxChargesChanged, PauseChanged, Plan, PlanRegistryClient,
        ProtocolPauseChanged, Refunded, SeatsChanged, String, Subscribed, UpgradeRecord, Upgraded,
        VaultClient, Vec, BPS_DENOMINATOR, CANCEL_REASON_NONE, INSTANCE_BUMP, INSTANCE_THRESHOLD,
        MAX_CANCEL_REASON, MAX_FEE_BPS, MAX_KEEPER_BPS, MAX_PAGE, MAX_REFERRAL_BPS,
        MAX_START_OFFSET, PERSIST_BUMP, PERSIST_THRESHOLD, SECONDS_PER_LEDGER,
    };
    use payflow_common::{load_instance_or_err, load_or_err};

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
        /// Returns [`Error::FeeTooHigh`] if `fee_bps > MAX_FEE_BPS`, or
        /// [`Error::InvalidAddress`] if `plan_registry` or `vault` is this
        /// contract, or if `vault` equals `plan_registry`.
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
            let self_addr = env.current_contract_address();
            if plan_registry == self_addr || vault == self_addr || vault == plan_registry {
                return Err(Error::InvalidAddress);
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
        /// The first charge is due immediately: this is `subscribe_at(..., now)`.
        ///
        /// # Auth
        /// Requires authorization from `subscriber`.
        ///
        /// # Events
        /// Emits [`MandateUpdated`] and [`Subscribed`].
        ///
        /// # Errors
        /// Returns [`Error::InvalidMaxTotal`] if `max_total < 0`,
        /// [`Error::PlanInactive`] if the target plan is inactive, or
        /// [`Error::NotInitialized`] if configuration keys are missing.
        pub fn subscribe(
            env: Env,
            subscriber: Address,
            plan_id: u64,
            max_charges: u32,
            max_total: i128,
            referrer: Option<Address>,
        ) -> Result<u64, Error> {
            let now = env.ledger().timestamp();
            Self::subscribe_at(
                env,
                subscriber,
                plan_id,
                max_charges,
                max_total,
                now,
                referrer,
            )
        }

        /// Open a mandate against a published plan with an explicitly requested
        /// `mandate_id` (#368).
        ///
        /// Behaves like [`Subscription::subscribe`], except the caller chooses
        /// the mandate ID instead of using `NextMandateId`. Rejects `0`
        /// ([`Error::InvalidMandateId`]), any tombstoned ID from a previously
        /// deleted mandate ([`Error::MandateIdTombstoned`]), and any ID
        /// currently held by an existing mandate
        /// ([`Error::MandateAlreadyExists`]).
        ///
        /// # Auth
        /// Requires authorization from `subscriber`.
        ///
        /// # Events
        /// Emits [`MandateUpdated`] and [`Subscribed`].
        ///
        /// # Errors
        /// Returns [`Error::InvalidMandateId`] if `mandate_id == 0`,
        /// [`Error::MandateIdTombstoned`] if `mandate_id` has been tombstoned,
        /// [`Error::MandateAlreadyExists`] if `mandate_id` already exists,
        /// [`Error::InvalidMaxTotal`] if `max_total < 0`,
        /// [`Error::PlanInactive`] if the target plan is inactive, or
        /// [`Error::NotInitialized`] if configuration keys are missing.
        pub fn subscribe_with_id(
            env: Env,
            subscriber: Address,
            plan_id: u64,
            mandate_id: u64,
            max_charges: u32,
            max_total: i128,
            referrer: Option<Address>,
        ) -> Result<u64, Error> {
            subscriber.require_auth();
            Self::require_not_protocol_paused(&env)?;
            let now = env.ledger().timestamp();
            Self::do_subscribe_at(
                env,
                subscriber,
                plan_id,
                max_charges,
                max_total,
                now,
                referrer,
                Some(mandate_id),
            )
        }

        /// Fund the vault and open a mandate in one transaction, so a new
        /// subscriber signs once instead of signing a deposit and then a
        /// subscribe — the gap between the two is where sign-ups were lost.
        ///
        /// The subscriber's one `require_auth` at the top is the only signature
        /// needed: the nested `vault.deposit` (and the token transfer it makes)
        /// run as sub-invocations of this same authorization, and the mandate
        /// is then created through the normal `subscribe_at` path (start =
        /// now), so it carries the full, current mandate terms.
        ///
        /// Atomic by construction: if the deposit fails the whole invocation
        /// aborts and no mandate is created; if mandate creation then fails,
        /// the deposit is rolled back with it.
        ///
        /// (Merged from #134, extended with `max_total` for parity with the
        /// current `subscribe`, which #134 predated.)
        ///
        /// # Auth
        /// Requires authorization from `subscriber`.
        ///
        /// # Errors
        /// As `subscribe`, plus any error the vault's `deposit` can return.
        pub fn subscribe_with_deposit(
            env: Env,
            subscriber: Address,
            plan_id: u64,
            max_charges: u32,
            max_total: i128,
            deposit: i128,
            referrer: Option<Address>,
        ) -> Result<u64, Error> {
            subscriber.require_auth();
            Self::require_not_protocol_paused(&env)?;

            let registry: Address = Self::cfg_address(&env, DataKey::PlanRegistry)?;
            let plan = Self::read_plan(&env, &registry, plan_id)?;

            let vault_addr: Address = Self::cfg_address(&env, DataKey::Vault)?;
            if VaultClient::new(&env, &vault_addr)
                .try_deposit(&subscriber, &plan.token, &deposit)
                .is_err()
            {
                return Err(Error::VaultCallFailed);
            }

            let now = env.ledger().timestamp();
            Self::do_subscribe_at(
                env,
                subscriber,
                plan_id,
                max_charges,
                max_total,
                now,
                referrer,
                None,
            )
        }

        /// Open a mandate whose first charge falls due at `start` rather than
        /// now, so a subscriber can set up billing ahead of time.
        ///
        /// `start` must be the current ledger timestamp or later, and no more
        /// than `MAX_START_OFFSET` (one year) after it: a mandate may be
        /// scheduled in advance, but neither backdated nor locked away in the
        /// distant future. Everything else is identical to `subscribe` — the
        /// plan's terms are copied into the mandate and never re-read — and
        /// `Subscribed` carries `start` as `next_charge`, so indexers learn when
        /// billing begins from the event alone.
        ///
        /// # Auth
        /// Requires authorization from `subscriber`.
        ///
        /// # Events
        /// Emits [`MandateUpdated`] and [`Subscribed`].
        ///
        /// # Errors
        /// Returns [`Error::InvalidStart`] if `start` is before the current
        /// ledger timestamp or more than `MAX_START_OFFSET` after it,
        /// [`Error::InvalidMaxTotal`] if `max_total < 0`, [`Error::PlanInactive`]
        /// if the target plan is inactive, or [`Error::NotInitialized`] if
        /// configuration keys are missing.
        pub fn subscribe_at(
            env: Env,
            subscriber: Address,
            plan_id: u64,
            max_charges: u32,
            max_total: i128,
            start: u64,
            referrer: Option<Address>,
        ) -> Result<u64, Error> {
            subscriber.require_auth();
            Self::require_not_protocol_paused(&env)?;
            Self::do_subscribe_at(
                env,
                subscriber,
                plan_id,
                max_charges,
                max_total,
                start,
                referrer,
                None,
            )
        }

        /// Core mandate creation shared by `subscribe_at` (which authorizes the
        /// subscriber first), `subscribe_with_id` (which authorizes and passes an
        /// explicit `requested_id`), and `subscribe_with_deposit` (whose single
        /// top-level `require_auth` already covers it — re-authorizing here
        /// would add a second, uncovered auth entry). Not a contract entry
        /// point: it performs no authorization of its own.
        #[allow(clippy::too_many_arguments)]
        fn do_subscribe_at(
            env: Env,
            subscriber: Address,
            plan_id: u64,
            max_charges: u32,
            max_total: i128,
            start: u64,
            referrer: Option<Address>,
            requested_id: Option<u64>,
        ) -> Result<u64, Error> {
            if max_total < 0 {
                return Err(Error::InvalidMaxTotal);
            }

            // `start - now` only after the past check, so it cannot underflow.
            let now = env.ledger().timestamp();
            if start < now || start - now > MAX_START_OFFSET {
                return Err(Error::InvalidStart);
            }

            if let Some(req_id) = requested_id {
                if req_id == 0 {
                    return Err(Error::InvalidMandateId);
                }
                if Self::is_tombstoned(&env, req_id) {
                    return Err(Error::MandateIdTombstoned);
                }
                if Self::has_mandate(&env, req_id) {
                    return Err(Error::MandateAlreadyExists);
                }
            }

            let registry: Address = Self::cfg_address(&env, DataKey::PlanRegistry)?;
            let plan = Self::read_plan(&env, &registry, plan_id)?;

            // A plan that is switched off, or whose availability window has
            // passed, accepts no new subscribers. Existing mandates never
            // re-read the plan, so both checks only ever stop new sign-ups.
            if !plan.active || (plan.available_until != 0 && now >= plan.available_until) {
                return Err(Error::PlanInactive);
            }

            // Seat cap (from #158): a plan with a non-zero `max_subscribers`
            // refuses a new mandate once that many are already open. The count
            // is maintained incrementally (inc here, dec when a mandate closes).
            let open = Self::plan_open_count(&env, plan_id);
            if plan.max_subscribers != 0 && open >= plan.max_subscribers {
                return Err(Error::PlanFull);
            }

            let next_id: u64 =
                load_instance_or_err(&env, &DataKey::NextMandateId, Error::NotInitialized)?;
            let (id, next_mandate_id) = if let Some(req_id) = requested_id {
                let updated = if req_id == next_id {
                    next_id.checked_add(1).ok_or(Error::AmountOverflow)?
                } else {
                    next_id
                };
                (req_id, updated)
            } else {
                let mut candidate = next_id;
                while Self::is_tombstoned(&env, candidate) || Self::has_mandate(&env, candidate) {
                    candidate = candidate.checked_add(1).ok_or(Error::AmountOverflow)?;
                }
                let updated = candidate.checked_add(1).ok_or(Error::AmountOverflow)?;
                (candidate, updated)
            };

            // Freeze the plan's current grace window into this mandate. The
            // registry is the source of truth, and existing mandates never
            // re-read it, so later plan edits cannot shorten a window an open
            // mandate was promised (#312).
            let grace_period_secs = Self::read_plan_grace_period(&env, &registry, plan_id)?;

            let fee_bps: u32 = load_instance_or_err(&env, &DataKey::FeeBps, Error::NotInitialized)?;

            // The registry already caps this, but clamp again here: the mandate's
            // tip must never exceed MAX_KEEPER_BPS even if the registry contract
            // passed to this deployment does not enforce the cap.
            let keeper_bps = plan.keeper_bps.min(MAX_KEEPER_BPS);
            // Defensive clamp, same as keeper_bps: the referral share is frozen
            // into the mandate and can never exceed MAX_REFERRAL_BPS even if a
            // registry passed to this deployment failed to enforce the cap.
            let referral_bps = plan.referral_bps.min(MAX_REFERRAL_BPS);

            let mandate = Mandate {
                id,
                subscriber: subscriber.clone(),
                plan_id,
                merchant: plan.merchant.clone(),
                token: plan.token.clone(),
                amount: plan.amount,
                period: plan.period,
                created_at: now,
                next_charge: start,
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
                payout: plan.payout.clone(),
                referrer: referrer.clone(),
                referral_bps,
                status: MandateStatus::Active,
            };

            Self::write_mandate(&env, &mandate);
            if grace_period_secs != 0 {
                Self::write_grace_period(&env, id, grace_period_secs);
            }
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
            // Per-plan index (from #134): hooked into subscribe_at — the single
            // mandate-creation path — so every sign-up route keeps it in sync.
            Self::index_push(
                &env,
                DataKey::PlanMandateCount(plan_id),
                |i| DataKey::PlanMandate(plan_id, i),
                id,
            );
            // Take a seat against the plan's cap (from #158); released when the
            // mandate is cancelled, ended or completed.
            Self::plan_open_inc(&env, plan_id);

            env.storage()
                .instance()
                .set(&DataKey::NextMandateId, &next_mandate_id);
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
                next_charge: start,
                max_charges,
                fee_bps,
                keeper_bps,
                referrer,
                referral_bps,
            }
            .publish(&env);

            Self::recompute_committed_revenue(&env, plan_id);

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
        /// See [ADR 0005](../../docs/adr/0005-permissionless-charge.md) for a
        /// detailed explanation of why permissionless charging is safe and
        /// keeper operational guidance.
        ///
        /// # Auth
        /// Requires authorization from `keeper`.
        ///
        /// # Events
        /// Emits [`MandateUpdated`] and [`Charged`]. If this charge completes the mandate, also emits [`MandateCompleted`].
        ///
        /// # Errors
        /// Returns [`Error::MandateNotFound`] if the mandate does not exist,
        /// [`Error::MandateNotActive`] if the mandate is cancelled, completed,
        /// or otherwise not active,
        /// [`Error::NotDue`] if the ledger timestamp is before `next_charge`,
        /// [`Error::MaxChargesReached`] if the mandate reached its cap,
        /// [`Error::PlanChargesPaused`] if the plan's merchant paused charging,
        /// or [`Error::AmountOverflow`] if math overflows.
        pub fn charge(env: Env, keeper: Address, mandate_id: u64) -> Result<ChargeResult, Error> {
            keeper.require_auth();
            Self::require_not_protocol_paused(&env)?;

            let mut mandate = Self::read_mandate(&env, mandate_id)?;
            if matches!(
                mandate.status,
                MandateStatus::Cancelled | MandateStatus::Completed
            ) {
                return Err(Error::MandateNotActive);
            }

            let now = env.ledger().timestamp();
            if !super::lifecycle::active_at(&mandate, now) {
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
            if Self::plan_charges_paused(&env, mandate.plan_id)? {
                return Err(Error::PlanChargesPaused);
            }

            let (fee, tip, referral, merchant_amount) = super::charge::split(&mandate)?;
            let charges_made = mandate
                .charges_made
                .checked_add(1)
                .ok_or(Error::AmountOverflow)?;
            // A next due date past u64::MAX is a schedule overflow, not an
            // amount overflow (#290).
            let next_charge = now
                .checked_add(mandate.period)
                .ok_or(Error::ScheduleOverflow)?;
            let completed = (mandate.max_charges != 0 && charges_made >= mandate.max_charges)
                || (mandate.max_total != 0 && next_total >= mandate.max_total);

            let vault_addr: Address = Self::cfg_address(&env, DataKey::Vault)?;
            let fee_to: Address = Self::cfg_address(&env, DataKey::FeeTo)?;
            let vault = VaultClient::new(&env, &vault_addr);

            // Build all payment legs for a single vault call. This replaces the
            // three separate calls (merchant+fee split, tip, referral) with one
            // call that does a single authorization check and balance update.
            // Order matches the original sequential execution so Debit event
            // balances are identical: merchant, fee, tip, referral.
            let mut legs = soroban_sdk::Vec::new(&env);
            legs.push_back((mandate.payout.clone(), merchant_amount));
            if fee > 0 {
                legs.push_back((fee_to.clone(), fee));
            }
            if tip > 0 {
                legs.push_back((keeper.clone(), tip));
            }
            if referral > 0 {
                if let Some(referrer) = &mandate.referrer {
                    legs.push_back((referrer.clone(), referral));
                }
            }

            if vault
                .try_debit_multi_mandate(&mandate.subscriber, &mandate.token, &mandate_id, &legs)
                .is_err()
            {
                return Err(Error::VaultCallFailed);
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
                // A completed mandate releases its seat on the plan (#158).
                Self::plan_open_dec(&env, mandate.plan_id);
            }

            Self::write_mandate(&env, &mandate);
            // A settled charge ends any run of failures (#296), so the next
            // failure notifies the merchant again.
            Self::clear_failed_charge(&env, mandate_id);

            MandateUpdated {
                mandate_id,
                mandate: mandate.clone(),
            }
            .publish(&env);

            Charged {
                mandate_id,
                subscriber: mandate.subscriber.clone(),
                merchant: mandate.merchant.clone(),
                token: mandate.token.clone(),
                amount: merchant_amount,
                fee,
                tip,
                keeper,
                charges_made: mandate.charges_made,
                next_charge: mandate.next_charge,
                referral,
                referrer: mandate.referrer.clone(),
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

            Self::recompute_committed_revenue(&env, mandate.plan_id);

            Ok(ChargeResult {
                merchant_amount,
                fee,
                charges_made,
                next_charge,
                completed,
            })
        }

        /// Record that a due charge fails, and notify the merchant the first
        /// time it does (#296).
        ///
        /// A failed `charge` is rolled back together with any event it would
        /// emit, so a keeper whose charge fails calls this instead. It proves
        /// the failure on chain before recording anything: the mandate must be
        /// due exactly as `charge` requires, and the vault must be unable to
        /// cover the charge right now (balance, this mandate's reserve and the
        /// subscriber's spend limit all considered). A flag can therefore
        /// never be raised against a mandate that would have paid.
        ///
        /// The first flag in a run of failures stores the timestamp and emits
        /// [`ChargeFailed`] with the merchant as a topic, so off-chain dunning
        /// can react promptly. Further flags return `false` and emit nothing,
        /// so retries cannot spam the merchant. The next successful `charge`
        /// ends the run; the failure after that notifies again.
        ///
        /// Permissionless, like `charge`: it moves no money and only records
        /// a fact anyone can check.
        ///
        /// # Auth
        /// None.
        ///
        /// # Events
        /// Emits [`ChargeFailed`] on the first flag of a run of failures, and
        /// nothing otherwise.
        ///
        /// # Errors
        /// Returns [`Error::MandateNotFound`] if the mandate does not exist,
        /// [`Error::MandateNotActive`] if it is cancelled, completed, or not
        /// active,
        /// [`Error::NotDue`] if the ledger timestamp is before `next_charge`,
        /// [`Error::MaxChargesReached`] if it reached a cap,
        /// [`Error::PlanChargesPaused`] if the plan's merchant paused charging,
        /// [`Error::ChargeWouldSucceed`] if the vault covers the charge, or
        /// [`Error::AmountOverflow`] if math overflows.
        pub fn flag_failed_charge(env: Env, mandate_id: u64) -> Result<bool, Error> {
            let mandate = Self::read_mandate(&env, mandate_id)?;
            if matches!(
                mandate.status,
                MandateStatus::Cancelled | MandateStatus::Completed
            ) {
                return Err(Error::MandateNotActive);
            }

            // The same gates `charge` applies, in the same order, so a mandate
            // that `charge` would reject for timing, status or caps is
            // rejected here for the same reason, not flagged as unpaid.
            let now = env.ledger().timestamp();
            if !super::lifecycle::active_at(&mandate, now) {
                return Err(Error::MandateNotActive);
            }
            if now < mandate.next_charge {
                return Err(Error::NotDue);
            }
            if mandate.max_charges != 0 && mandate.charges_made >= mandate.max_charges {
                return Err(Error::MaxChargesReached);
            }
            let next_total = mandate
                .total_spent
                .checked_add(mandate.amount)
                .ok_or(Error::AmountOverflow)?;
            if mandate.max_total != 0 && next_total > mandate.max_total {
                return Err(Error::MaxChargesReached);
            }
            if Self::plan_charges_paused(&env, mandate.plan_id)? {
                return Err(Error::PlanChargesPaused);
            }

            // The subscriber is debited exactly `amount` across all legs, so
            // one check for the full price covers merchant, fee, tip and
            // referral together.
            let vault_addr: Address = Self::cfg_address(&env, DataKey::Vault)?;
            let Ok(Ok(can_debit)) = VaultClient::new(&env, &vault_addr).try_can_debit(
                &mandate.subscriber,
                &mandate.token,
                &mandate_id,
                &mandate.amount,
            ) else {
                return Err(Error::VaultCallFailed);
            };
            if can_debit {
                return Err(Error::ChargeWouldSucceed);
            }

            let key = DataKey::FirstFailedCharge(mandate_id);
            if env.storage().persistent().has(&key) {
                // Already notified for this run of failures.
                return Ok(false);
            }
            env.storage().persistent().set(&key, &now);
            env.storage()
                .persistent()
                .extend_ttl(&key, PERSIST_THRESHOLD, PERSIST_BUMP);

            ChargeFailed {
                mandate_id,
                merchant: mandate.merchant,
                subscriber: mandate.subscriber,
                plan_id: mandate.plan_id,
                token: mandate.token,
                amount: mandate.amount,
                due_at: mandate.next_charge,
                failed_at: now,
                charges_made: mandate.charges_made,
            }
            .publish(&env);

            Ok(true)
        }

        /// When the mandate's current run of failed charges was first flagged
        /// by `flag_failed_charge`, or `None` if its last charge attempt has
        /// not been flagged as failed (#296).
        pub fn first_failed_charge(env: Env, mandate_id: u64) -> Option<u64> {
            env.storage()
                .persistent()
                .get(&DataKey::FirstFailedCharge(mandate_id))
        }

        /// Grace window in seconds frozen into `mandate_id` when it was opened
        /// (#312). `0` means the plan had no grace window at sign-up.
        ///
        /// # Errors
        /// Returns [`Error::MandateNotFound`] if the mandate does not exist.
        pub fn mandate_grace_period(env: Env, mandate_id: u64) -> Result<u64, Error> {
            let _ = Self::read_mandate(&env, mandate_id)?;
            Ok(Self::read_grace_period(&env, mandate_id))
        }

        /// Whether `mandate_id` is inside the grace window it was opened with
        /// (#312), so a wallet can warn the subscriber before the mandate
        /// lapses.
        ///
        /// The window starts when the charge falls due (`next_charge`), not
        /// when a keeper gets around to flagging the failure, and lasts for the
        /// mandate's frozen grace window. A mandate with a zero window is never
        /// in grace. Indefinitely paused, cancelled and completed mandates are
        /// not in grace either; a scheduled pause that has run its course is
        /// treated as active, matching `charge`.
        ///
        /// The window is inclusive of its start and exclusive of its end: at
        /// exactly `next_charge + grace_period_secs` the grace period has
        /// elapsed. This is a pure read: no storage writes and no events.
        ///
        /// # Errors
        /// Returns [`Error::MandateNotFound`] if the mandate does not exist.
        pub fn is_in_grace_period(env: Env, mandate_id: u64) -> Result<bool, Error> {
            let mandate = Self::read_mandate(&env, mandate_id)?;
            let grace_period_secs = Self::read_grace_period(&env, mandate_id);
            let now = env.ledger().timestamp();
            Ok(super::lifecycle::in_grace_period_at(
                &mandate,
                grace_period_secs,
                now,
            ))
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
        /// [`Error::PlanChargesPaused`] if the new plan has charging paused,
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
            Self::require_not_protocol_paused(&env)?;

            let mut mandate = Self::read_mandate(&env, mandate_id)?;
            if mandate.subscriber != subscriber {
                return Err(Error::NotSubscriber);
            }

            let now = env.ledger().timestamp();
            if !super::lifecycle::active_at(&mandate, now) {
                return Err(Error::MandateNotActive);
            }

            let registry: Address = Self::cfg_address(&env, DataKey::PlanRegistry)?;
            let plan = Self::read_plan(&env, &registry, new_plan_id)?;
            if !plan.active {
                return Err(Error::PlanInactive);
            }
            if plan.token != mandate.token {
                return Err(Error::DifferentToken);
            }
            if plan.merchant != mandate.merchant {
                return Err(Error::DifferentMerchant);
            }
            if Self::plan_charges_paused(&env, new_plan_id)? {
                return Err(Error::PlanChargesPaused);
            }

            let old_plan_id = mandate.plan_id;
            let old_amount = mandate.amount;
            let old_period = mandate.period;

            // Moving to a different plan takes a seat there; refuse if that plan
            // is at its cap (#158). Same-plan "upgrades" keep their seat.
            if new_plan_id != old_plan_id {
                let open = Self::plan_open_count(&env, new_plan_id);
                if plan.max_subscribers != 0 && open >= plan.max_subscribers {
                    return Err(Error::PlanFull);
                }
            }

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
                    .checked_mul(i128::from(remaining))
                    .ok_or(Error::AmountOverflow)?
                    / i128::from(old_period)
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
                    .checked_mul(i128::from(mandate.fee_bps))
                    .ok_or(Error::AmountOverflow)?
                    / BPS_DENOMINATOR;
                let merchant_gross = charged - fee;

                let vault_addr: Address = Self::cfg_address(&env, DataKey::Vault)?;
                let fee_to: Address = Self::cfg_address(&env, DataKey::FeeTo)?;
                let vault = VaultClient::new(&env, &vault_addr);
                // The prorated difference is settled the same way a charge is:
                // merchant share and protocol fee from one debit, drawing on
                // this mandate's reserve (#295) first.
                if vault
                    .try_debit_split_mandate(
                        &subscriber,
                        &mandate.token,
                        &mandate_id,
                        &mandate.merchant,
                        &merchant_gross,
                        &fee_to,
                        &fee,
                    )
                    .is_err()
                {
                    return Err(Error::VaultCallFailed);
                }
            }

            // The mandate carries the new plan's terms from now on. A downgrade
            // therefore bills the cheaper price at the next charge, and an
            // upgrade has already collected the difference for the rest of the
            // current period.
            // Move the seat between plans (#158): release the old plan's seat
            // and take one on the new plan. Guarded above so the counts stay
            // right and a full target plan was already rejected.
            if new_plan_id != old_plan_id {
                Self::plan_open_dec(&env, old_plan_id);
                Self::plan_open_inc(&env, new_plan_id);
            }

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

            Self::recompute_committed_revenue(&env, old_plan_id);
            if new_plan_id != old_plan_id {
                Self::recompute_committed_revenue(&env, new_plan_id);
            }

            Ok(())
        }

        /// Terminate a mandate permanently. Only the subscriber may do this, and it
        /// can be done at any time without the merchant's involvement.
        ///
        /// This is [`Subscription::cancel_with_reason`] with
        /// [`CANCEL_REASON_NONE`]: the mandate is cancelled exactly as before and
        /// the [`Cancelled`] event reports reason `0`.
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
            Self::cancel_with_reason(env, subscriber, mandate_id, CANCEL_REASON_NONE)
        }

        /// Terminate a mandate permanently and record why.
        ///
        /// Behave exactly like [`Subscription::cancel`] — same authorization,
        /// same state change, same events — and additionally report the
        /// subscriber's reason in the [`Cancelled`] event, so a merchant can
        /// measure churn from chain data alone instead of asking its users.
        ///
        /// The reason cannot change what the call does: the mandate is
        /// cancelled either way, so a code can never keep a mandate alive, move
        /// money, or make a charge cost more. The set is closed:
        ///
        /// | Code | Constant | Meaning |
        /// |---|---|---|
        /// | 0 | [`CANCEL_REASON_NONE`] | No reason given; what `cancel` emits |
        /// | 1 | [`CANCEL_REASON_TOO_EXPENSIVE`] | Too expensive |
        /// | 2 | [`CANCEL_REASON_NOT_USING`] | No longer using the service |
        /// | 3 | [`CANCEL_REASON_MISSING_FEATURE`] | Missing a needed feature |
        /// | 4 | [`CANCEL_REASON_SERVICE_QUALITY`] | Unhappy with the service |
        /// | 5 | [`CANCEL_REASON_OTHER`] | Something else |
        ///
        /// # Auth
        /// Requires authorization from `subscriber`.
        ///
        /// # Events
        /// Emits [`MandateUpdated`] and [`Cancelled`], whose `reason` is the
        /// code passed in.
        ///
        /// # Errors
        /// Returns [`Error::InvalidCancelReason`] if `reason > MAX_CANCEL_REASON`,
        /// [`Error::MandateNotFound`] if the mandate does not exist,
        /// [`Error::NotSubscriber`] if the caller is not the subscriber, or
        /// [`Error::MandateNotActive`] if the mandate is already cancelled or completed.
        pub fn cancel_with_reason(
            env: Env,
            subscriber: Address,
            mandate_id: u64,
            reason: u32,
        ) -> Result<(), Error> {
            subscriber.require_auth();

            // Validate before touching storage: a rejected call must not change
            // the mandate, and a closed code set keeps the event space small.
            if reason > MAX_CANCEL_REASON {
                return Err(Error::InvalidCancelReason);
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

            mandate.status = MandateStatus::Cancelled;
            // Cancelling releases the mandate's seat on the plan (#158); the
            // guard above means an already-closed mandate never dec's twice.
            Self::plan_open_dec(&env, mandate.plan_id);
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
                reason,
            }
            .publish(&env);

            Self::recompute_committed_revenue(&env, mandate.plan_id);
            Ok(())
        }

        /// Propose moving a mandate to `new_subscriber`. The current subscriber
        /// supplies the position from `subscriber_mandates`; this lets transfers
        /// update old indexes in constant time, including mandates created before
        /// reverse-position metadata existed.
        pub fn propose_mandate_owner(
            env: Env,
            subscriber: Address,
            mandate_id: u64,
            new_subscriber: Address,
            subscriber_position: u32,
        ) -> Result<(), Error> {
            subscriber.require_auth();

            let mandate = Self::read_mandate(&env, mandate_id)?;
            if mandate.subscriber != subscriber {
                return Err(Error::NotSubscriber);
            }
            if mandate.status != MandateStatus::Active && mandate.status != MandateStatus::Paused {
                return Err(Error::MandateNotActive);
            }
            if subscriber == new_subscriber {
                return Err(Error::SameSubscriber);
            }
            Self::validate_subscriber_index(&env, &subscriber, subscriber_position, mandate_id)?;

            Self::write_pending_owner(&env, mandate_id, &new_subscriber);
            Self::write_pending_position(&env, mandate_id, subscriber_position);
            Ok(())
        }

        /// Accept a pending mandate ownership transfer. Future charges are debited
        /// from the new subscriber's vault balance; the merchant index and all
        /// frozen mandate terms remain unchanged.
        pub fn accept_mandate_owner(
            env: Env,
            new_subscriber: Address,
            mandate_id: u64,
        ) -> Result<(), Error> {
            new_subscriber.require_auth();

            let mut mandate = Self::read_mandate(&env, mandate_id)?;
            if mandate.status != MandateStatus::Active && mandate.status != MandateStatus::Paused {
                return Err(Error::MandateNotActive);
            }

            let pending_owner: Address = load_or_err(
                &env,
                &DataKey::PendingMandateOwner(mandate_id),
                Error::NoPendingTransfer,
            )?;
            if pending_owner != new_subscriber {
                return Err(Error::NotProposedOwner);
            }
            let subscriber_position: u32 = load_or_err(
                &env,
                &DataKey::PendingSubscriberPosition(mandate_id),
                Error::NoPendingTransfer,
            )?;

            let old_subscriber = mandate.subscriber.clone();
            Self::index_len(
                &env,
                DataKey::SubscriberMandateCount(new_subscriber.clone()),
            )
            .checked_add(1)
            .ok_or(Error::MandateIndexFull)?;

            Self::remove_subscriber_index(&env, &old_subscriber, mandate_id, subscriber_position)?;
            Self::index_push(
                &env,
                DataKey::SubscriberMandateCount(new_subscriber.clone()),
                |i| DataKey::SubscriberMandate(new_subscriber.clone(), i),
                mandate_id,
            );

            mandate.subscriber = new_subscriber.clone();
            Self::write_mandate(&env, &mandate);
            Self::remove_pending_owner(&env, mandate_id);

            MandateTransferred {
                mandate_id,
                old_subscriber,
                new_subscriber,
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
            // Ending a mandate releases its seat on the plan (#158); the guard
            // above means an already-closed mandate never dec's twice.
            Self::plan_open_dec(&env, mandate.plan_id);
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

            Self::recompute_committed_revenue(&env, mandate.plan_id);
            Ok(())
        }

        /// Permanently delete a cancelled or completed mandate and record its ID
        /// in the tombstone set so it can never be recycled (#368).
        ///
        /// Removes the mandate record and its auxiliary per-mandate keys from
        /// storage while marking `mandate_id` as tombstoned in persistent
        /// storage. Only terminal mandates (`Cancelled` or `Completed`) may be
        /// deleted; open mandates must be cancelled or completed first.
        ///
        /// # Auth
        /// Requires authorization from `subscriber`.
        ///
        /// # Events
        /// Emits [`MandateDeleted`].
        ///
        /// # Errors
        /// Returns [`Error::MandateNotFound`] if the mandate does not exist,
        /// [`Error::NotSubscriber`] if the caller is not the subscriber, or
        /// [`Error::MandateStillActive`] if the mandate is still active or paused.
        pub fn delete_mandate(env: Env, subscriber: Address, mandate_id: u64) -> Result<(), Error> {
            subscriber.require_auth();

            let mandate = Self::read_mandate(&env, mandate_id)?;
            if mandate.subscriber != subscriber {
                return Err(Error::NotSubscriber);
            }
            if mandate.status != MandateStatus::Cancelled
                && mandate.status != MandateStatus::Completed
            {
                return Err(Error::MandateStillActive);
            }

            let p = env.storage().persistent();
            p.remove(&DataKey::Mandate(mandate_id));
            p.remove(&DataKey::MandateGracePeriod(mandate_id));
            p.remove(&DataKey::FirstFailedCharge(mandate_id));
            Self::remove_pending_owner(&env, mandate_id);
            Self::write_tombstone(&env, mandate_id);

            MandateDeleted {
                mandate_id,
                subscriber,
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
            let plan_id = mandate.plan_id;

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

            Self::recompute_committed_revenue(&env, plan_id);
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
            let plan_id = mandate.plan_id;

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

            Self::recompute_committed_revenue(&env, plan_id);
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

            let remaining = super::charge::merchant_received(&mandate)? - mandate.refunded;
            if amount > remaining {
                return Err(Error::RefundExceedsReceived);
            }

            let token_client = token::TokenClient::new(&env, &mandate.token);
            let received_before = token_client.balance(&mandate.subscriber);
            token_client.transfer(&merchant, &mandate.subscriber, &amount);
            let received = token_client
                .balance(&mandate.subscriber)
                .checked_sub(received_before)
                .ok_or(Error::UnexpectedTransferAmount)?;
            if received != amount {
                return Err(Error::UnexpectedTransferAmount);
            }

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

            Self::recompute_committed_revenue(&env, mandate.plan_id);
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

            let old_fee_bps: u32 =
                load_instance_or_err(&env, &DataKey::FeeBps, Error::NotInitialized)?;

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

        /// Pause or resume the whole protocol during an incident (#285).
        /// Admin only.
        ///
        /// While paused, `subscribe`, `subscribe_with_id`, `subscribe_at`,
        /// `subscribe_with_deposit`, `charge` and `upgrade` fail with
        /// [`Error::ProtocolPaused`] before touching any state. Everything that
        /// lets a subscriber or merchant get out keeps working: every view,
        /// `cancel`, `cancel_page`, `end_mandate`, `delete_mandate`,
        /// per-mandate pausing, `refund`, and the vault's withdrawals (the vault
        /// is a separate contract and never reads this switch). A mandate that
        /// falls due during the pause is not lost: its `next_charge` is
        /// unchanged, so it is chargeable as soon as the protocol resumes.
        ///
        /// Setting the switch to its current value is a no-op and emits no
        /// event; otherwise [`ProtocolPauseChanged`] is published.
        pub fn set_protocol_paused(env: Env, paused: bool) -> Result<(), Error> {
            let admin: Address = Self::cfg_address(&env, DataKey::Admin)?;
            admin.require_auth();

            let current = Self::protocol_paused(env.clone());
            if current != paused {
                env.storage()
                    .instance()
                    .set(&DataKey::ProtocolPaused, &paused);
                ProtocolPauseChanged { admin, paused }.publish(&env);
            }
            env.storage()
                .instance()
                .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_BUMP);
            Ok(())
        }

        /// Whether the admin has paused the protocol (#285).
        pub fn protocol_paused(env: Env) -> bool {
            env.storage()
                .instance()
                .get(&DataKey::ProtocolPaused)
                .unwrap_or(false)
        }

        /// Extend the storage TTL of a page of `merchant`'s open mandates in
        /// one call (#284), instead of one transaction per mandate.
        ///
        /// Walks positions `start .. start + min(limit, MAX_PAGE)` of the
        /// merchant index and, for every mandate that is still `Active` or
        /// `Paused`, bumps the mandate entry and its index slot to the
        /// standard persistent TTL. The index count is bumped too, so the
        /// index stays readable. Cancelled and completed mandates are skipped:
        /// they can no longer be charged, so keeping them alive only costs rent.
        ///
        /// Permissionless: extending a TTL changes no contract state that any
        /// read can observe, and Soroban lets anyone extend any entry anyway.
        /// Works while the protocol is paused, so a long incident cannot let
        /// mandates expire. Returns the number of mandates extended.
        ///
        /// Cost: one call reads at most 1 count + 50 slots + 50 mandates
        /// (101 entries, the transaction footprint limit) and writes none, so a
        /// keeper covering N open mandates needs `ceil(N / 50)` transactions
        /// instead of N.
        pub fn extend_merchant_mandates_ttl(
            env: Env,
            merchant: Address,
            start: u32,
            limit: u32,
        ) -> u32 {
            let count_key = DataKey::MerchantMandateCount(merchant.clone());
            let len = Self::index_len(&env, count_key.clone());
            let end = start.saturating_add(limit.min(MAX_PAGE)).min(len);
            if start >= end {
                return 0;
            }
            let p = env.storage().persistent();
            p.extend_ttl(&count_key, PERSIST_THRESHOLD, PERSIST_BUMP);

            let mut extended = 0u32;
            for i in start..end {
                let slot_key = DataKey::MerchantMandate(merchant.clone(), i);
                let Some(id) = p.get::<_, u64>(&slot_key) else {
                    continue;
                };
                let mandate_key = DataKey::Mandate(id);
                let Some(m) = p.get::<_, Mandate>(&mandate_key) else {
                    continue;
                };
                if !matches!(m.status, MandateStatus::Active | MandateStatus::Paused) {
                    continue;
                }
                p.extend_ttl(&mandate_key, PERSIST_THRESHOLD, PERSIST_BUMP);
                p.extend_ttl(&slot_key, PERSIST_THRESHOLD, PERSIST_BUMP);
                extended = extended.saturating_add(1);
            }
            extended
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

        /// [`estimated_end`](Self::estimated_end) expressed as a timestamp,
        /// an expected ledger sequence and a period count (#287), so UIs need
        /// no extra math. Returns `None` for an open-ended mandate.
        ///
        /// The ledger figure assumes [`SECONDS_PER_LEDGER`]-second ledgers and
        /// saturates at `u32::MAX`; it is a hint, not a guarantee.
        ///
        /// # Errors
        /// Same as [`estimated_end`](Self::estimated_end).
        pub fn estimated_end_detail(
            env: Env,
            mandate_id: u64,
        ) -> Result<Option<EstimatedEnd>, Error> {
            let mandate = Self::read_mandate(&env, mandate_id)?;
            let Some(timestamp) = Self::estimated_end(env.clone(), mandate_id)? else {
                return Ok(None);
            };
            let remaining_charges = mandate.max_charges.saturating_sub(mandate.charges_made);
            let now = env.ledger().timestamp();
            let ahead = timestamp.saturating_sub(now);
            let ledgers_ahead = ahead.div_ceil(SECONDS_PER_LEDGER);
            // Clamped to `u32::MAX` first, so the conversion cannot fail.
            let ledger = u32::try_from(
                u64::from(env.ledger().sequence())
                    .saturating_add(ledgers_ahead)
                    .min(u64::from(u32::MAX)),
            )
            .unwrap_or(u32::MAX);
            let periods_remaining = if mandate.period == 0 {
                0
            } else {
                ahead.div_ceil(mandate.period)
            };
            Ok(Some(EstimatedEnd {
                timestamp,
                ledger,
                remaining_charges,
                periods_remaining,
            }))
        }

        /// True when `charge` would succeed on timing and status grounds. Does not
        /// check vault balance; a funded-ness check is the caller's job.
        pub fn is_due(env: Env, mandate_id: u64) -> Result<bool, Error> {
            let m = Self::read_mandate(&env, mandate_id)?;
            if !super::views::mandate_is_due(&m, env.ledger().timestamp()) {
                return Ok(false);
            }
            Ok(!Self::plan_charges_paused(&env, m.plan_id)?)
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
            let (fee, _tip, _referral, merchant_amount) = super::charge::split(&m)?;
            let now = env.ledger().timestamp();
            let chargeable = super::lifecycle::active_at(&m, now)
                && (m.max_charges == 0 || m.charges_made < m.max_charges)
                && (m.max_total == 0 || m.total_spent.saturating_add(m.amount) <= m.max_total)
                && now >= m.next_charge
                && !Self::plan_charges_paused(&env, m.plan_id)?;

            Ok(ChargeQuote {
                amount: m.amount,
                fee,
                merchant_amount,
                due_at: m.next_charge,
                chargeable,
            })
        }

        /// Preview the exact recipients and amounts the next `charge` would
        /// transfer, without requiring authorization or changing state.
        ///
        /// `chargeable` covers protocol and plan pauses, timing, mandate status
        /// and spend caps. Like [`quote_charge`](Self::quote_charge), it does
        /// not check authorization or vault funds. `keeper` is the proposed
        /// charge caller and receives the tip if one is due.
        ///
        /// # Errors
        /// Returns [`Error::MandateNotFound`] if the mandate does not exist,
        /// [`Error::PlanNotFound`] if the registry cannot provide its pause
        /// state,
        /// [`Error::NotInitialized`] if configuration is missing, or
        /// [`Error::AmountOverflow`] if the split calculation overflows.
        pub fn simulate_charge(
            env: Env,
            keeper: Address,
            mandate_id: u64,
        ) -> Result<ChargeSimulation, Error> {
            let mandate = Self::read_mandate(&env, mandate_id)?;
            let (fee, tip, referral, merchant_amount) = super::charge::split(&mandate)?;
            let fee_to = Self::cfg_address(&env, DataKey::FeeTo)?;
            let now = env.ledger().timestamp();
            let next_total = mandate
                .total_spent
                .checked_add(mandate.amount)
                .ok_or(Error::AmountOverflow)?;
            let within_cap = mandate.max_total == 0 || next_total <= mandate.max_total;
            let locally_chargeable = super::lifecycle::active_at(&mandate, now)
                && (mandate.max_charges == 0 || mandate.charges_made < mandate.max_charges)
                && within_cap
                && now >= mandate.next_charge;
            let chargeable = if locally_chargeable && !Self::protocol_paused(env.clone()) {
                !Self::plan_charges_paused(&env, mandate.plan_id)?
            } else {
                false
            };

            Ok(ChargeSimulation {
                subscriber: mandate.subscriber,
                token: mandate.token,
                amount: mandate.amount,
                payout: mandate.payout,
                merchant_amount,
                fee_to,
                fee,
                keeper,
                tip,
                referrer: mandate.referrer,
                referral,
                due_at: mandate.next_charge,
                chargeable,
            })
        }

        /// Permanently cancel every cancellable mandate in one page of the
        /// subscriber's index. The scan is capped at `MAX_PAGE`; reading index
        /// slots directly avoids touching the separate count entry, keeping a
        /// full page within the 100-entry transaction limit.
        ///
        /// Mandates already cancelled or completed are skipped, so calling it
        /// twice is harmless. Returns how many this call cancelled. Each
        /// [`Cancelled`] event reports reason [`CANCEL_REASON_NONE`]: a
        /// subscriber who clears out everything is not explaining one by one.
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
            let mut affected_plans: soroban_sdk::Vec<u64> = soroban_sdk::Vec::new(&env);
            for position in start..end {
                let slot = DataKey::SubscriberMandate(subscriber.clone(), position);
                let Some(mandate_id) = env.storage().persistent().get::<_, u64>(&slot) else {
                    continue;
                };
                let Some(mut mandate) = Self::read_mandate_opt(&env, mandate_id) else {
                    continue;
                };
                if mandate.subscriber != subscriber {
                    return Err(Error::NotSubscriber);
                }
                if mandate.status == MandateStatus::Cancelled
                    || mandate.status == MandateStatus::Completed
                {
                    continue;
                }

                let plan_id = mandate.plan_id;
                mandate.status = MandateStatus::Cancelled;
                // Each cancelled mandate releases its seat (#158); the guard
                // above skips already-closed ones so no seat dec's twice.
                Self::plan_open_dec(&env, plan_id);
                Self::write_mandate(&env, &mandate);
                Cancelled {
                    mandate_id,
                    subscriber: subscriber.clone(),
                    charges_made: mandate.charges_made,
                    // A bulk cancel is not an explained cancellation: the
                    // subscriber chose to stop everything, not to say why.
                    reason: CANCEL_REASON_NONE,
                }
                .publish(&env);
                cancelled += 1;

                // Track affected plans for committed revenue recomputation
                let mut already_tracked = false;
                for p in affected_plans.iter() {
                    if p == plan_id {
                        already_tracked = true;
                        break;
                    }
                }
                if !already_tracked {
                    affected_plans.push_back(plan_id);
                }
            }

            // Recompute committed revenue for all affected plans
            for plan_id in affected_plans.iter() {
                Self::recompute_committed_revenue(&env, plan_id);
            }

            Ok(cancelled)
        }

        /// Ids of due mandates opened against `merchant`'s plans, in index
        /// order. A keeper settling one merchant otherwise has to read every
        /// mandate the merchant has ever had and test each one itself.
        ///
        /// Reads at most one page of index slots and their mandate entries.
        /// It cannot afford a plan-registry read per candidate within the
        /// ledger's 100-entry budget, so it reports the mandates whose own
        /// schedule is due; `is_due` and `charge` add the plan-pause check and
        /// are the final authority before settlement (#315).
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
                let Some(mandate) = Self::read_mandate_opt(&env, id) else {
                    continue;
                };
                if super::views::mandate_is_due(&mandate, now) {
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
                let Some(m) = Self::read_mandate_opt(&env, id) else {
                    continue;
                };
                if m.token != token || !super::lifecycle::active_at(&m, now) || m.period == 0 {
                    continue;
                }
                // Integer math only, truncated downwards: a subscriber who plans
                // against this figure is never told they have more committed than
                // they do.
                let contribution = m
                    .amount
                    .checked_mul(i128::from(period_secs))
                    .ok_or(Error::AmountOverflow)?
                    / i128::from(m.period);
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

        /// Summaries of the mandates `subscriber` opened, oldest first, starting at
        /// position `start`. Returns at most `min(limit, MAX_PAGE)` summaries.
        /// Each summary contains the key fields a wallet needs to display a
        /// subscription without fetching the full mandate.
        pub fn subscriber_mandate_summaries(
            env: Env,
            subscriber: Address,
            start: u32,
            limit: u32,
        ) -> Vec<MandateSummary> {
            let len = Self::index_len(&env, DataKey::SubscriberMandateCount(subscriber.clone()));
            let end = start.saturating_add(limit.min(MAX_PAGE)).min(len);
            let mut out = Vec::new(&env);
            let storage = env.storage().persistent();
            for i in start..end {
                let slot_key = DataKey::SubscriberMandate(subscriber.clone(), i);
                if let Some(mandate_id) = storage.get::<_, u64>(&slot_key) {
                    if let Some(mandate) = Self::read_mandate_opt(&env, mandate_id) {
                        out.push_back(MandateSummary {
                            id: mandate.id,
                            merchant: mandate.merchant.clone(),
                            plan_id: mandate.plan_id,
                            amount: mandate.amount,
                            period: mandate.period,
                            status: mandate.status,
                            next_charge: mandate.next_charge,
                            charges_made: mandate.charges_made,
                            max_charges: mandate.max_charges,
                            token: mandate.token.clone(),
                        });
                    }
                }
            }
            out
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

        /// Number of mandates ever opened against plan `plan_id` (from #134).
        pub fn plan_mandate_count(env: Env, plan_id: u64) -> u32 {
            Self::index_len(&env, DataKey::PlanMandateCount(plan_id))
        }

        /// Ids of the mandates opened against plan `plan_id`, oldest first,
        /// starting at position `start`. Returns at most `min(limit, MAX_PAGE)`.
        pub fn plan_mandates(env: Env, plan_id: u64, start: u32, limit: u32) -> Vec<u64> {
            let len = Self::index_len(&env, DataKey::PlanMandateCount(plan_id));
            Self::index_page(&env, len, start, limit, |i| {
                DataKey::PlanMandate(plan_id, i)
            })
        }

        /// Number of currently-open mandates against `plan_id`; the count the
        /// plan's `max_subscribers` cap is checked against (from #158).
        pub fn plan_subscriber_count(env: Env, plan_id: u64) -> u32 {
            Self::plan_open_count(&env, plan_id)
        }

        /// Committed revenue per period for `plan_id`. This is the sum of all
        /// active mandates' per-period amounts, normalized to the plan's billing
        /// period. Returns 0 if the plan has no active mandates or does not exist.
        pub fn plan_committed_revenue(env: Env, plan_id: u64) -> i128 {
            Self::read_committed_revenue(&env, plan_id)
        }

        /// Raise or lower a mandate's `max_charges` (0 = unlimited). Only the
        /// subscriber may change it, and not below the charges already made.
        /// Reaching the new cap completes the mandate and releases its seat.
        /// (From #158.)
        pub fn set_max_charges(
            env: Env,
            subscriber: Address,
            mandate_id: u64,
            max_charges: u32,
        ) -> Result<(), Error> {
            subscriber.require_auth();

            let mut mandate = Self::read_mandate(&env, mandate_id)?;
            if mandate.subscriber != subscriber {
                return Err(Error::NotSubscriber);
            }
            if mandate.status != MandateStatus::Active && mandate.status != MandateStatus::Paused {
                return Err(Error::MandateNotActive);
            }
            if max_charges != 0 && max_charges < mandate.charges_made {
                return Err(Error::InvalidMaxCharges);
            }

            mandate.max_charges = max_charges;

            let completed = max_charges != 0 && mandate.charges_made >= max_charges;
            if completed {
                mandate.status = MandateStatus::Completed;
                // The finished mandate releases its seat on the plan.
                Self::plan_open_dec(&env, mandate.plan_id);
            }

            Self::write_mandate(&env, &mandate);

            MandateUpdated {
                mandate_id,
                mandate: mandate.clone(),
            }
            .publish(&env);

            MaxChargesChanged {
                mandate_id,
                subscriber,
                max_charges,
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

            Self::recompute_committed_revenue(&env, mandate.plan_id);
            Ok(())
        }

        /// Replace this contract's wasm with code at `new_wasm_hash`. Only the
        /// admin may upgrade; persistent state is untouched. (From #158.)
        ///
        /// Records the upgrade in the contract's upgrade history, which can be
        /// queried via `upgrade_history` for transparency.
        pub fn upgrade_wasm(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), Error> {
            let admin: Address = Self::cfg_address(&env, DataKey::Admin)?;
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

        /// The id the next opened mandate will get (skipping any tombstoned or
        /// already-occupied ids). Mandates are numbered from 1.
        pub fn next_mandate_id(env: Env) -> Result<u64, Error> {
            let mut candidate: u64 =
                load_instance_or_err(&env, &DataKey::NextMandateId, Error::NotInitialized)?;
            while Self::is_tombstoned(&env, candidate) || Self::has_mandate(&env, candidate) {
                candidate = candidate.checked_add(1).ok_or(Error::AmountOverflow)?;
            }
            Ok(candidate)
        }

        /// Whether `mandate_id` has been deleted and permanently tombstoned
        /// (#368).
        pub fn is_mandate_tombstoned(env: Env, mandate_id: u64) -> bool {
            Self::is_tombstoned(&env, mandate_id)
        }

        /// Current protocol fee rate in basis points.
        pub fn fee_bps(env: Env) -> Result<u32, Error> {
            load_instance_or_err(&env, &DataKey::FeeBps, Error::NotInitialized)
        }

        /// Propose handing the admin role to `new_admin` (#238).
        ///
        /// This is step one of a two-step handover. The current admin keeps
        /// the role until `new_admin` accepts with `accept_admin`; proposing
        /// alone changes no authority, so a mistyped or hostile proposal can
        /// be replaced before anyone accepts it. A later proposal to another
        /// address overwrites the pending one.
        ///
        /// # Auth
        /// Requires authorization from the current admin.
        ///
        /// # Errors
        /// Returns [`Error::NotInitialized`] if no admin is configured.
        pub fn propose_admin(env: Env, new_admin: Address) -> Result<(), Error> {
            let current: Address = Self::admin(env.clone())?;
            current.require_auth();

            env.storage()
                .instance()
                .set(&DataKey::PendingAdmin, &new_admin);
            env.storage()
                .instance()
                .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_BUMP);

            Ok(())
        }

        /// Accept a pending admin handover (#238). Step two of the handshake:
        /// only the address the current admin proposed may accept, and the
        /// role moves only when it does. The pending proposal is consumed, so
        /// a second accept by the same address fails.
        ///
        /// # Auth
        /// Requires authorization from `new_admin`.
        ///
        /// # Errors
        /// Returns [`Error::PendingAdminNotFound`] if no handover is pending,
        /// or [`Error::NotPendingAdmin`] if `new_admin` is not the proposed
        /// successor.
        pub fn accept_admin(env: Env, new_admin: Address) -> Result<(), Error> {
            new_admin.require_auth();

            let pending: Address = env
                .storage()
                .instance()
                .get(&DataKey::PendingAdmin)
                .ok_or(Error::PendingAdminNotFound)?;
            if pending != new_admin {
                return Err(Error::NotPendingAdmin);
            }

            env.storage().instance().set(&DataKey::Admin, &new_admin);
            env.storage().instance().remove(&DataKey::PendingAdmin);
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
        /// Whether the plan's merchant has paused charging for it (#315).
        fn plan_charges_paused(env: &Env, plan_id: u64) -> Result<bool, Error> {
            let registry: Address = Self::cfg_address(env, DataKey::PlanRegistry)?;
            match PlanRegistryClient::new(env, &registry).try_plan_charges_paused(&plan_id) {
                Ok(Ok(paused)) => Ok(paused),
                _ => Err(Error::PlanNotFound),
            }
        }

        fn read_plan_grace_period(
            env: &Env,
            registry: &Address,
            plan_id: u64,
        ) -> Result<u64, Error> {
            match PlanRegistryClient::new(env, registry).try_plan_grace_period(&plan_id) {
                Ok(Ok(seconds)) => Ok(seconds),
                _ => Err(Error::PlanNotFound),
            }
        }

        /// Read a plan from the registry, turning any cross-contract failure
        /// into a typed error instead of letting the client panic.
        fn read_plan(env: &Env, registry: &Address, plan_id: u64) -> Result<Plan, Error> {
            match PlanRegistryClient::new(env, registry).try_get_plan(&plan_id) {
                Ok(Ok(plan)) => Ok(plan),
                _ => Err(Error::PlanNotFound),
            }
        }

        /// Fails with [`Error::ProtocolPaused`] while the admin's global pause
        /// is on (#285). Called first, after authorization, by every entry point
        /// that opens a mandate or moves money into the billing flow.
        fn require_not_protocol_paused(env: &Env) -> Result<(), Error> {
            if env
                .storage()
                .instance()
                .get::<_, bool>(&DataKey::ProtocolPaused)
                .unwrap_or(false)
            {
                return Err(Error::ProtocolPaused);
            }
            Ok(())
        }

        fn cfg_address(env: &Env, key: DataKey) -> Result<Address, Error> {
            load_instance_or_err(env, &key, Error::NotInitialized)
        }

        /// Read a mandate and refresh its TTL when it is close to expiry (#247).
        /// A mandate can be accessed many times without being rewritten: a
        /// keeper polling a not-yet-due charge, a failed debit, or a wallet
        /// reading its status. Those accesses must keep the entry alive. The
        /// bump is conditional, so an entry with plenty of rent left is
        /// untouched and views remain side-effect free in the common case.
        fn read_mandate(env: &Env, id: u64) -> Result<Mandate, Error> {
            Self::read_mandate_opt(env, id).ok_or(Error::MandateNotFound)
        }

        fn read_mandate_opt(env: &Env, id: u64) -> Option<Mandate> {
            let key = DataKey::Mandate(id);
            let mandate: Mandate = env.storage().persistent().get(&key)?;
            env.storage()
                .persistent()
                .extend_ttl(&key, PERSIST_THRESHOLD, PERSIST_BUMP);
            Some(mandate)
        }

        fn read_grace_period(env: &Env, id: u64) -> u64 {
            env.storage()
                .persistent()
                .get(&DataKey::MandateGracePeriod(id))
                .unwrap_or(0)
        }

        fn write_grace_period(env: &Env, id: u64, grace_period_secs: u64) {
            let key = DataKey::MandateGracePeriod(id);
            env.storage().persistent().set(&key, &grace_period_secs);
            env.storage()
                .persistent()
                .extend_ttl(&key, PERSIST_THRESHOLD, PERSIST_BUMP);
        }

        fn write_mandate(env: &Env, mandate: &Mandate) {
            let key = DataKey::Mandate(mandate.id);
            env.storage().persistent().set(&key, mandate);
            env.storage()
                .persistent()
                .extend_ttl(&key, PERSIST_THRESHOLD, PERSIST_BUMP);
        }

        fn has_mandate(env: &Env, id: u64) -> bool {
            env.storage().persistent().has(&DataKey::Mandate(id))
        }

        fn is_tombstoned(env: &Env, id: u64) -> bool {
            env.storage()
                .persistent()
                .get::<_, bool>(&DataKey::MandateTombstone(id))
                .unwrap_or(false)
        }

        fn write_tombstone(env: &Env, id: u64) {
            let key = DataKey::MandateTombstone(id);
            let p = env.storage().persistent();
            p.set(&key, &true);
            p.extend_ttl(&key, PERSIST_THRESHOLD, PERSIST_BUMP);
        }

        fn validate_subscriber_index(
            env: &Env,
            subscriber: &Address,
            position: u32,
            mandate_id: u64,
        ) -> Result<(), Error> {
            let count = Self::index_len(env, DataKey::SubscriberMandateCount(subscriber.clone()));
            let key = DataKey::SubscriberMandate(subscriber.clone(), position);
            if position >= count
                || env.storage().persistent().get::<_, u64>(&key) != Some(mandate_id)
            {
                return Err(Error::InvalidSubscriberIndex);
            }
            Ok(())
        }

        fn remove_subscriber_index(
            env: &Env,
            subscriber: &Address,
            mandate_id: u64,
            position: u32,
        ) -> Result<(), Error> {
            Self::validate_subscriber_index(env, subscriber, position, mandate_id)?;

            let count_key = DataKey::SubscriberMandateCount(subscriber.clone());
            let len = Self::index_len(env, count_key.clone());
            let last_position = len - 1;
            let last_key = DataKey::SubscriberMandate(subscriber.clone(), last_position);
            let storage = env.storage().persistent();

            if position != last_position {
                let moved_id: u64 = load_or_err(env, &last_key, Error::InvalidSubscriberIndex)?;
                if moved_id == mandate_id {
                    return Err(Error::InvalidSubscriberIndex);
                }
                let moved_key = DataKey::SubscriberMandate(subscriber.clone(), position);
                storage.set(&moved_key, &moved_id);
                storage.extend_ttl(&moved_key, PERSIST_THRESHOLD, PERSIST_BUMP);
                if storage
                    .get::<_, Address>(&DataKey::PendingMandateOwner(moved_id))
                    .is_some()
                {
                    Self::write_pending_position(env, moved_id, position);
                }
            }

            storage.remove(&last_key);
            let new_len = len - 1;
            storage.set(&count_key, &new_len);
            storage.extend_ttl(&count_key, PERSIST_THRESHOLD, PERSIST_BUMP);
            Ok(())
        }

        /// End a mandate's run of failed charges (#296). Removing an absent
        /// key is a no-op, so this is safe on every settled charge.
        fn clear_failed_charge(env: &Env, mandate_id: u64) {
            env.storage()
                .persistent()
                .remove(&DataKey::FirstFailedCharge(mandate_id));
        }

        fn write_pending_owner(env: &Env, mandate_id: u64, owner: &Address) {
            let key = DataKey::PendingMandateOwner(mandate_id);
            env.storage().persistent().set(&key, owner);
            env.storage()
                .persistent()
                .extend_ttl(&key, PERSIST_THRESHOLD, PERSIST_BUMP);
        }

        fn write_pending_position(env: &Env, mandate_id: u64, position: u32) {
            let key = DataKey::PendingSubscriberPosition(mandate_id);
            env.storage().persistent().set(&key, &position);
            env.storage()
                .persistent()
                .extend_ttl(&key, PERSIST_THRESHOLD, PERSIST_BUMP);
        }

        fn remove_pending_owner(env: &Env, mandate_id: u64) {
            env.storage()
                .persistent()
                .remove(&DataKey::PendingMandateOwner(mandate_id));
            env.storage()
                .persistent()
                .remove(&DataKey::PendingSubscriberPosition(mandate_id));
        }

        /// Count of currently-open mandates against a plan, checked each sign-up
        /// against the plan's `max_subscribers` (0 = unlimited). (From #158.)
        fn plan_open_count(env: &Env, plan_id: u64) -> u32 {
            env.storage()
                .persistent()
                .get(&DataKey::PlanSubscriberCount(plan_id))
                .unwrap_or(0)
        }

        fn plan_open_inc(env: &Env, plan_id: u64) {
            let key = DataKey::PlanSubscriberCount(plan_id);
            let count = Self::plan_open_count(env, plan_id).saturating_add(1);
            let p = env.storage().persistent();
            p.set(&key, &count);
            p.extend_ttl(&key, PERSIST_THRESHOLD, PERSIST_BUMP);
        }

        fn plan_open_dec(env: &Env, plan_id: u64) {
            let key = DataKey::PlanSubscriberCount(plan_id);
            let count = Self::plan_open_count(env, plan_id).saturating_sub(1);
            let p = env.storage().persistent();
            p.set(&key, &count);
            p.extend_ttl(&key, PERSIST_THRESHOLD, PERSIST_BUMP);
        }

        /// Read the stored committed revenue per period for a plan.
        fn read_committed_revenue(env: &Env, plan_id: u64) -> i128 {
            env.storage()
                .persistent()
                .get(&DataKey::PlanCommittedRevenue(plan_id))
                .unwrap_or(0)
        }

        /// Write the committed revenue per period for a plan and emit a change event
        /// if the value actually changed.
        fn write_committed_revenue(
            env: &Env,
            plan_id: u64,
            merchant: &Address,
            token: &Address,
            period: u64,
            new_committed: i128,
        ) {
            let old_committed = Self::read_committed_revenue(env, plan_id);
            if old_committed == new_committed {
                return;
            }

            let key = DataKey::PlanCommittedRevenue(plan_id);
            let p = env.storage().persistent();
            p.set(&key, &new_committed);
            p.extend_ttl(&key, PERSIST_THRESHOLD, PERSIST_BUMP);

            CommittedRevenueChanged {
                plan_id,
                merchant: merchant.clone(),
                token: token.clone(),
                old_committed,
                new_committed,
                period,
            }
            .publish(env);
        }

        /// Recalculate the committed revenue for a plan by summing all active mandates'
        /// per-period amounts and update storage, emitting an event if changed.
        fn recompute_committed_revenue(env: &Env, plan_id: u64) {
            let registry: Address = match Self::cfg_address(env, DataKey::PlanRegistry) {
                Ok(r) => r,
                Err(_) => return,
            };
            let plan_registry = PlanRegistryClient::new(env, &registry);
            let Ok(Ok(plan)) = plan_registry.try_get_plan(&plan_id) else {
                // The revenue cache is advisory; never fail the state change
                // that triggered the recomputation because a plan read failed.
                return;
            };

            let len = Self::index_len(env, DataKey::PlanMandateCount(plan_id));
            let mut total: i128 = 0;
            let now = env.ledger().timestamp();
            for i in 0..len.min(MAX_PAGE) {
                let slot_key = DataKey::PlanMandate(plan_id, i);
                let Some(mandate_id) = env.storage().persistent().get::<_, u64>(&slot_key) else {
                    continue;
                };
                let Some(mandate) = Self::read_mandate_opt(env, mandate_id) else {
                    continue;
                };
                if mandate.token != plan.token
                    || !super::lifecycle::active_at(&mandate, now)
                    || mandate.period == 0
                {
                    continue;
                }
                // Normalize to the plan's period. Equal periods need no
                // multiplication: doing it anyway overflows for an amount near
                // i128::MAX and silently reports zero committed revenue.
                let contribution = if mandate.period == plan.period {
                    mandate.amount
                } else {
                    mandate
                        .amount
                        .checked_mul(i128::from(plan.period))
                        .ok_or(Error::AmountOverflow)
                        .unwrap_or(0)
                        / i128::from(mandate.period)
                };
                total = total.checked_add(contribution).unwrap_or(total);
            }

            Self::write_committed_revenue(
                env,
                plan_id,
                &plan.merchant,
                &plan.token,
                plan.period,
                total,
            );
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
            p.set(&count_key, &len.saturating_add(1));
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
