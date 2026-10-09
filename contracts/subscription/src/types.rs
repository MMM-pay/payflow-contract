use soroban_sdk::{contracttype, Address, String};

/// Mirror of `payflow-plan-registry`'s `Plan`. Declared locally so the
/// subscription contract can decode registry responses without linking the
/// registry crate into its own wasm.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Plan {
    pub id: u64,
    pub merchant: Address,
    pub name: String,
    pub token: Address,
    pub amount: i128,
    pub period: u64,
    pub active: bool,
}

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
