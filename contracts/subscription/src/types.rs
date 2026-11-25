//! Data types and storage keys for the subscription contract.

use soroban_sdk::{contracttype, Address, String};

/// Values from a settled charge. `merchant_amount` matches `Charged.amount`:
/// the merchant's share after the protocol fee and keeper tip.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChargeResult {
    /// Amount transferred to the merchant after fee and keeper tip deductions.
    pub merchant_amount: i128,
    /// Protocol fee collected on the charge.
    pub fee: i128,
    /// Total number of charges completed under this mandate including this one.
    pub charges_made: u32,
    /// Timestamp in Unix seconds when the next charge falls due.
    pub next_charge: u64,
    /// Whether the mandate has reached its charge cap and is now completed.
    pub completed: bool,
}

/// The registry's `Plan`, imported from the shared crate rather than copied.
/// A field added to or removed from the registry's plan therefore changes the
/// type this contract decodes, and the subscription build fails instead of
/// silently failing to decode `get_plan` at runtime.
pub use payflow_shared::Plan;

/// Lifecycle status of a subscription mandate.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MandateStatus {
    /// Chargeable when due.
    Active,
    /// Temporarily not chargeable. Subscriber can resume.
    Paused,
    /// Terminated by the subscriber. Irreversible.
    Cancelled,
    /// Reached `max_charges` or `max_total`. Irreversible.
    Completed,
}

/// A standing authorization from a subscriber to a merchant, bounded in
/// amount, frequency, total number of charges and total spend.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Mandate {
    /// Unique identifier of the mandate.
    pub id: u64,
    /// Account of the subscriber paying for the mandate.
    pub subscriber: Address,
    /// ID of the plan against which this mandate was opened or upgraded.
    pub plan_id: u64,
    /// Merchant receiving payment under this mandate.
    pub merchant: Address,
    /// Token contract address in which payments are denominated.
    pub token: Address,
    /// Frozen at subscribe time. A later plan edit cannot reprice this mandate.
    pub amount: i128,
    /// Billing period in seconds between charges.
    pub period: u64,
    /// Unix seconds when the mandate was opened.
    pub created_at: u64,
    /// Unix seconds. Chargeable once the ledger timestamp reaches this.
    pub next_charge: u64,
    /// Unix seconds when the most recent charge was executed, or 0 if none.
    pub last_charge: u64,
    /// Number of charges successfully executed so far.
    pub charges_made: u32,
    /// 0 means open-ended.
    pub max_charges: u32,
    /// Protocol fee in basis points, frozen at subscribe time. An admin fee
    /// change therefore cannot alter the economics of a mandate that is
    /// already open — the same guarantee the plan's price already has.
    pub fee_bps: u32,
    /// Keeper tip in basis points of the merchant share, frozen at subscribe
    /// time for the same reason as `fee_bps`: a later plan cannot raise what an
    /// open mandate pays whoever settles it.
    pub keeper_bps: u32,
    /// Unix seconds when a scheduled pause (`pause_until`) lifts by itself.
    /// 0 means no schedule: a paused mandate stays paused until `set_paused`
    /// resumes it. Only meaningful while `status` is `Paused`.
    pub paused_until: u64,
    /// Total amount the merchant has refunded on this mandate so far. The
    /// merchant can never refund more than it has collected, so this counter
    /// is the hard cap `refund` enforces.
    pub refunded: i128,
    /// Number of seats the subscriber is paying for. `1` for non-seat plans.
    /// `amount` already equals the per-seat price multiplied by this value.
    pub seats: u32,
    /// Ceiling on everything this mandate may ever debit, frozen when the
    /// subscriber opened it. 0 means open-ended. Mandates opened before this
    /// field existed decode to 0 and are therefore uncapped.
    pub max_total: i128,
    /// Total debited by settled charges so far, the figure `max_total` caps.
    pub total_spent: i128,

    /// Maximum continuous pause length in seconds, frozen at subscribe time
    /// from the plan's `max_pause_secs`. 0 means unlimited. Mandates opened
    /// before this field existed decode to 0 and are therefore unlimited, so
    /// they are unaffected.
    pub max_pause_secs: u64,
    /// Account that receives the merchant's share of each charge, frozen from
    /// the plan's `payout` at sign-up. A later payout change or a plan-owner
    /// transfer never redirects this open mandate's funds.
    pub payout: Address,
    /// Optional referrer paid a share of the merchant's money on every charge.
    /// `None` disables the referral for this mandate.
    pub referrer: Option<Address>,
    /// Referral share in basis points of the merchant's share, frozen from the
    /// plan at sign-up. Capped at `MAX_REFERRAL_BPS`.
    pub referral_bps: u32,
    /// Current lifecycle state of the mandate.
    pub status: MandateStatus,
}

/// Read-only preview of what `charge` would pay right now.
///
/// Clients use this instead of recomputing the merchant/fee split, so a quote
/// can never disagree with settlement because of a different rounding rule.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChargeQuote {
    /// Total the subscriber would be debited: the mandate's frozen price.
    pub amount: i128,
    /// Protocol fee, truncated to whole stroops exactly as `charge` does.
    pub fee: i128,
    /// The merchant's share after both the protocol fee and the keeper tip.
    pub merchant_amount: i128,
    /// Unix seconds at which the mandate next becomes chargeable.
    pub due_at: u64,
    /// True when `charge` would pass its status, cap and timing checks right
    /// now. Vault balance is not considered, matching `is_due`.
    pub chargeable: bool,
}

/// Exact settlement recipients and amounts for a prospective charge.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChargeSimulation {
    /// Subscriber whose vault balance would be debited.
    pub subscriber: Address,
    /// Token used for the charge.
    pub token: Address,
    /// Total amount debited from the subscriber.
    pub amount: i128,
    /// Address receiving the merchant's net share.
    pub payout: Address,
    /// Merchant's net share after fees, keeper tip and referral.
    pub merchant_amount: i128,
    /// Address receiving the protocol fee.
    pub fee_to: Address,
    /// Protocol fee.
    pub fee: i128,
    /// Proposed keeper address and recipient of any tip.
    pub keeper: Address,
    /// Keeper tip.
    pub tip: i128,
    /// Referral recipient, if any.
    pub referrer: Option<Address>,
    /// Referral payment (zero when there is no referrer).
    pub referral: i128,
    /// Unix seconds at which the charge is due.
    pub due_at: u64,
    /// Whether timing, status, caps and configured pauses permit charging now.
    pub chargeable: bool,
}

/// Estimated end of a capped mandate, in the units a UI needs without extra
/// math (#287). `timestamp` is Unix seconds (format it as ISO-8601 client
/// side), `ledger` the ledger sequence that time is expected to land in, and
/// `periods_remaining` how many billing periods separate now from that end.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EstimatedEnd {
    /// Unix seconds of the final scheduled (or, once completed, actual) charge.
    pub timestamp: u64,
    /// Ledger sequence expected at `timestamp`, assuming
    /// `SECONDS_PER_LEDGER`-second ledgers. Never earlier than the current one.
    pub ledger: u32,
    /// Charges still to be made before the cap is reached.
    pub remaining_charges: u32,
    /// Whole billing periods from now until `timestamp`, rounded up. Zero once
    /// the end is in the past.
    pub periods_remaining: u64,
}

/// Summary of a mandate for list views. Contains the fields a wallet needs to
/// display a subscription without fetching the full mandate.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MandateSummary {
    /// Unique identifier of the mandate.
    pub id: u64,
    /// Merchant receiving payment under this mandate.
    pub merchant: Address,
    /// Plan ID this mandate was opened against.
    pub plan_id: u64,
    /// Amount charged per period.
    pub amount: i128,
    /// Billing period in seconds.
    pub period: u64,
    /// Current lifecycle state of the mandate.
    pub status: MandateStatus,
    /// Unix seconds when the next charge falls due.
    pub next_charge: u64,
    /// Number of charges successfully executed so far.
    pub charges_made: u32,
    /// Maximum number of charges (0 = unlimited).
    pub max_charges: u32,
    /// Token contract address in which payments are denominated.
    pub token: Address,
}

/// A single contract upgrade record.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpgradeRecord {
    /// Semantic version of the new code (from `CARGO_PKG_VERSION`).
    pub version: String,
    /// Ledger sequence number when the upgrade was applied.
    pub ledger: u32,
    /// Hash of the new wasm code.
    pub wasm_hash: soroban_sdk::BytesN<32>,
}

/// Storage keys used by the subscription contract.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Admin address authorized to change configuration.
    Admin,
    /// Address of the plan registry contract.
    PlanRegistry,
    /// Address of the vault contract.
    Vault,
    /// Current protocol fee rate in basis points.
    FeeBps,
    /// Recipient address for protocol fees.
    FeeTo,
    /// Counter for generating sequential mandate IDs.
    NextMandateId,
    /// Mandate stored by its unique mandate ID.
    Mandate(u64),
    /// Number of mandates a subscriber has opened.
    SubscriberMandateCount(Address),
    /// The mandate at this subscriber's current index position (0-based).
    SubscriberMandate(Address, u32),
    /// Proposed new owner for a mandate awaiting acceptance.
    PendingMandateOwner(u64),
    /// Subscriber index position captured with the pending ownership proposal.
    PendingSubscriberPosition(u64),
    /// Number of mandates opened against a merchant's plans.
    MerchantMandateCount(Address),
    /// The n-th mandate opened against a merchant's plans (0-based).
    MerchantMandate(Address, u32),
    /// Number of mandates opened against a plan.
    PlanMandateCount(u64),
    /// The n-th mandate opened against a plan (0-based).
    PlanMandate(u64, u32),
    /// Count of simultaneously-open mandates against a plan, checked against
    /// the plan's `max_subscribers` seat cap.
    PlanSubscriberCount(u64),
    /// Committed revenue per period for a plan (sum of active mandates' per-period amounts).
    PlanCommittedRevenue(u64),
    /// Ledger timestamp at which the mandate's current run of failed charges
    /// was first flagged by `flag_failed_charge` (#296). Cleared by the next
    /// successful `charge`, so the following failure notifies again.
    FirstFailedCharge(u64),
    /// Grace window in seconds frozen into a mandate at sign-up (#312).
    /// Missing means zero.
    MandateGracePeriod(u64),
    /// Ordered list of contract upgrades: (version, ledger, `wasm_hash`).
    /// Append-only; the initial deployment is not recorded.
    UpgradeHistory,
    /// Protocol-wide pause switch set by the admin (#285). While `true`, new
    /// subscriptions, charges and plan upgrades are refused; reads,
    /// cancellations, refunds and vault withdrawals keep working. Missing
    /// means `false`.
    ProtocolPaused,
    /// Proposed successor admin awaiting their acceptance (#238). Missing
    /// means no handover is pending.
    PendingAdmin,
}
