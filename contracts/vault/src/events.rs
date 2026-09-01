use soroban_sdk::{contractevent, Address};

/// Emitted when a subscriber funds their vault balance.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Deposit {
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
    #[topic]
    pub user: Address,
    #[topic]
    pub token: Address,
    pub amount: i128,
    pub balance: i128,
}

/// Emitted when the subscription contract debits a subscriber to pay a merchant.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Debit {
    #[topic]
    pub user: Address,
    #[topic]
    pub token: Address,
    #[topic]
    pub to: Address,
    pub amount: i128,
    pub balance: i128,
}
