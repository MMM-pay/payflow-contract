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
    /// A getter found no profile for the merchant.
    ProfileNotFound = 8,
    /// `set_profile` was given a name longer than MAX_NAME_LEN bytes.
    ProfileNameTooLong = 9,
    /// `set_profile` was given a URL longer than MAX_URL_LEN bytes.
    ProfileUrlTooLong = 10,
}
