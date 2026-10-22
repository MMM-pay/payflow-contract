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
