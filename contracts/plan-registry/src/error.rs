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
    /// `create_plan` was given a keeper tip above MAX_KEEPER_BPS.
    KeeperBpsTooHigh = 11,
    /// `set_successor` was given a plan that does not exist.
    SuccessorNotFound = 12,
    /// A plan cannot be its own successor.
    SelfSuccessor = 13,
    /// The replacement plan is denominated in a different token than the plan
    /// it replaces, so the pointer would be meaningless.
    SuccessorTokenMismatch = 14,
    /// `create_plan` was given a description longer than
    /// MAX_DESCRIPTION_LEN bytes.
    DescriptionTooLong = 15,
}
