//! Events emitted by the vault contract.

use soroban_sdk::{contractevent, Address, BytesN};

/// Emitted when a subscriber funds their vault balance.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Deposit {
    /// Account that deposited the funds.
    #[topic]
    pub user: Address,
    /// Token contract address of the deposited asset.
    #[topic]
    pub token: Address,
    /// Amount of tokens deposited.
    pub amount: i128,
    /// Updated spendable balance of the subscriber for this token.
    pub balance: i128,
}

/// Emitted when one account pays into another account's vault balance, for
/// example an employer funding an employee's subscriptions. `from` is the
/// payer: the account whose tokens moved and whose authorization was required.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DepositFor {
    #[topic]
    pub from: Address,
    #[topic]
    pub user: Address,
    #[topic]
    pub token: Address,
    pub amount: i128,
    pub balance: i128,
}

/// Emitted when a subscriber pulls funds back out.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Withdraw {
    /// Account that withdrew the funds.
    #[topic]
    pub user: Address,
    /// Token contract address of the withdrawn asset.
    #[topic]
    pub token: Address,
    /// Amount of tokens withdrawn.
    pub amount: i128,
    /// Remaining spendable balance of the subscriber for this token.
    pub balance: i128,
}

/// Emitted when a subscriber pulls funds out to a destination other than
/// themselves. The subscriber authorizes the call; the destination does not
/// need to.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WithdrawTo {
    /// Account whose balance was debited.
    #[topic]
    pub user: Address,
    /// Account that received the tokens.
    #[topic]
    pub to: Address,
    /// Token contract address of the withdrawn asset.
    #[topic]
    pub token: Address,
    /// Amount of tokens withdrawn.
    pub amount: i128,
    /// Remaining spendable balance of the subscriber for this token.
    pub balance: i128,
}

/// Emitted when the admin sets the per-token deposit cap of a subscriber.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DepositCapSet {
    /// Account whose deposit cap was updated.
    #[topic]
    pub user: Address,
    /// Token contract address subject to the cap.
    #[topic]
    pub token: Address,
    /// The new cap. Zero means unlimited.
    pub cap: i128,
}

/// Emitted when the subscription contract debits a subscriber to pay a merchant.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Debit {
    /// Account debited.
    #[topic]
    pub user: Address,
    /// Token contract address of the debited asset.
    #[topic]
    pub token: Address,
    /// Recipient address receiving the tokens.
    #[topic]
    pub to: Address,
    /// Amount transferred to the recipient.
    pub amount: i128,
    /// Remaining spendable balance of the subscriber for this token.
    pub balance: i128,
}

/// Emitted when a subscriber sets, changes or releases the reserve earmarked
/// for one mandate (#295). `amount` is the mandate's new reserve (0 means
/// released) and `reserved_total` the subscriber's total reserve for the
/// token afterwards, so an indexer can track both without a contract read.
///
/// A debit that draws a reserve down does not emit this event; its `Debit`
/// already reports the movement, and `reserved` returns the remainder.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReserveSet {
    /// Subscriber whose balance is earmarked.
    #[topic]
    pub user: Address,
    /// Token the reserve is denominated in.
    #[topic]
    pub token: Address,
    /// Mandate the reserve is earmarked for.
    #[topic]
    pub mandate_id: u64,
    /// New reserve for this mandate. 0 means the reserve was released.
    pub amount: i128,
    /// Total reserved by `user` in `token` across all mandates afterwards.
    pub reserved_total: i128,
}

/// Emitted when the admin replaces this contract's wasm. Persistent state is
/// untouched by an upgrade; the new code reads the keys the old code wrote.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContractUpgraded {
    #[topic]
    pub admin: Address,
    pub new_wasm_hash: BytesN<32>,
}

/// Emitted when the vault admin releases all per-mandate reserves for a
/// subscriber via `emergency_withdraw` (#341).
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmergencyWithdraw {
    /// Subscriber whose per-mandate reserves were released.
    #[topic]
    pub subscriber: Address,
    /// Sum of released reserves across all mandates for the token.
    pub amount: i128,
}
