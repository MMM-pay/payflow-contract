use soroban_sdk::{contracttype, Address};

/// Values from a settled charge. `merchant_amount` matches `Charged.amount`:
/// the merchant's share after the protocol fee and keeper tip.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChargeResult {
    pub merchant_amount: i128,
    pub fee: i128,
    pub charges_made: u32,
    pub next_charge: u64,
    pub completed: bool,
}

/// The registry's `Plan`, imported from the shared crate rather than copied.
/// A field added to or removed from the registry's plan therefore changes the
/// type this contract decodes, and the subscription build fails instead of
/// silently failing to decode `get_plan` at runtime.
pub use payflow_shared::Plan;

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MandateStatus {
    /// Chargeable when due.
    Active,
    /// Temporarily not chargeable. Subscriber can resume.
    Paused,
    /// Terminated by the subscriber. Irreversible.
    Cancelled,
    /// Reached `max_charges`. Irreversible.
    Completed,
}

/// A standing authorization from a subscriber to a merchant, bounded in
/// amount, frequency, and total number of charges.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Mandate {
    pub id: u64,
    pub subscriber: Address,
    pub plan_id: u64,
    pub merchant: Address,
    pub token: Address,
    /// Frozen at subscribe time. A later plan edit cannot reprice this mandate.
    pub amount: i128,
    pub period: u64,
    /// Unix seconds when the mandate was opened.
    pub created_at: u64,
    /// Unix seconds. Chargeable once the ledger timestamp reaches this.
    pub next_charge: u64,
    pub last_charge: u64,
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
    pub status: MandateStatus,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    PlanRegistry,
    Vault,
    FeeBps,
    FeeTo,
    NextMandateId,
    Mandate(u64),
    /// Number of mandates a subscriber has opened.
    SubscriberMandateCount(Address),
    /// The n-th mandate a subscriber opened (0-based).
    SubscriberMandate(Address, u32),
    /// Number of mandates opened against a merchant's plans.
    MerchantMandateCount(Address),
    /// The n-th mandate opened against a merchant's plans (0-based).
    MerchantMandate(Address, u32),
}
