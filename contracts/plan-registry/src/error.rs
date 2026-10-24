//! Error types returned by the plan registry contract.

use soroban_sdk::contracterror;

/// Errors returned by the plan registry contract.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    /// The contract has already been initialized.
    AlreadyInitialized = 1,
    /// The contract has not been initialized with an admin and counter.
    NotInitialized = 2,
    /// The requested plan ID does not exist in storage.
    PlanNotFound = 3,
    /// The caller is not the merchant that owns the plan.
    NotPlanOwner = 4,
    /// The plan amount must be positive.
    InvalidAmount = 5,
    /// The billing period must be at least MIN_PERIOD.
    InvalidPeriod = 6,
    /// The plan name exceeds MAX_NAME_LEN bytes.
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
    /// `set_plan_name` was given a name longer than MAX_NAME_LEN bytes.
    PlanNameTooLong = 16,
}
