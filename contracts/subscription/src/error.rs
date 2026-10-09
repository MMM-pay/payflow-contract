use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    MandateNotFound = 3,
    NotSubscriber = 4,
    MandateNotActive = 5,
    NotDue = 6,
    PlanInactive = 7,
    FeeTooHigh = 8,
    MaxChargesReached = 9,
    InvalidMaxCharges = 10,
    NotMerchant = 11,
}
