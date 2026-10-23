//! Events emitted by the vault contract.

use soroban_sdk::{contractevent, Address};

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
