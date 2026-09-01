use soroban_sdk::{contracttype, Address};

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    /// The single subscription contract permitted to call `debit`.
    Subscription,
    /// Spendable balance held for (user, token).
    Balance(Address, Address),
}
