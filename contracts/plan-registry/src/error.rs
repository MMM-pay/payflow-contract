use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    PlanNotFound = 3,
    NotPlanOwner = 4,
    InvalidAmount = 5,
    InvalidPeriod = 6,
    NameTooLong = 7,
}
