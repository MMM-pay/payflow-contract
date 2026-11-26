//! Canonical typed storage key namespace for the subscription contract.
//!
//! The variant order and payload types are intentionally unchanged: Soroban
//! serializes these keys into ledger storage, so reordering or renaming them
//! would make existing entries inaccessible after a contract upgrade.

use soroban_sdk::{contracttype, Address};

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
    /// Proposed new admin address awaiting acceptance (#238).
    PendingAdmin,
    /// Marks a mandate as deleted. `true` means the mandate has been
    /// tombstoned and must not be re-opened. Used to prevent ID reuse.
    MandateTombstone(u64),
    /// Unix seconds when a mandate was first flagged as past due (#34).
    /// Present only while the mandate is in the `PastDue` state.
    PastDueSince(u64),
    /// Metered usage amount reported by the merchant for the current billing
    /// period of a mandate (#35). Absent means no usage has been reported yet.
    PendingUsage(u64),
}
