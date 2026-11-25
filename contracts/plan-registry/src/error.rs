//! Error types returned by the plan registry contract.

use payflow_shared::ErrorCode;
use soroban_sdk::contracterror;

/// Errors returned by the plan registry contract.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    /// The contract has already been initialized.
    AlreadyInitialized = 1001,
    /// The contract has not been initialized with an admin and counter.
    NotInitialized = 1002,
    /// The requested plan ID does not exist in storage.
    PlanNotFound = 1003,
    /// The caller is not the merchant that owns the plan.
    NotPlanOwner = 1004,
    /// The plan amount must be positive.
    InvalidAmount = 1005,
    /// The billing period must be at least `MIN_PERIOD`.
    InvalidPeriod = 1006,
    /// The plan name exceeds `MAX_NAME_LEN` bytes.
    NameTooLong = 1007,
    /// A getter found no profile for the merchant.
    ProfileNotFound = 1008,
    /// `set_profile` was given a name longer than `MAX_NAME_LEN` bytes.
    ProfileNameTooLong = 1009,
    /// `set_profile` was given a URL longer than `MAX_URL_LEN` bytes.
    ProfileUrlTooLong = 1010,
    /// `create_plan` was given a keeper tip above `MAX_KEEPER_BPS`.
    KeeperBpsTooHigh = 1011,
    /// `set_successor` was given a plan that does not exist.
    SuccessorNotFound = 1012,
    /// A plan cannot be its own successor.
    SelfSuccessor = 1013,
    /// The replacement plan is denominated in a different token than the plan
    /// it replaces, so the pointer would be meaningless.
    SuccessorTokenMismatch = 1014,
    /// `create_plan` was given a description longer than
    /// `MAX_DESCRIPTION_LEN` bytes.
    DescriptionTooLong = 1015,
    /// `set_plan_name` was given a name longer than `MAX_NAME_LEN` bytes.
    PlanNameTooLong = 1016,
    /// `create_plan` was given a terms URI longer than `MAX_TERMS_LEN` bytes.
    UriTooLong = 1017,
    /// `create_plan` was given a referral rate above `MAX_REFERRAL_BPS`.
    ReferralBpsTooHigh = 1018,
    /// `create_plan` was given an amount below `MIN_CHARGE_AMOUNT`.
    AmountTooLow = 1019,
    /// `accept_plan_owner` was called for a plan with no pending ownership
    /// proposal, or after the proposal was already accepted.
    PendingOwnershipNotFound = 1020,
    /// The address accepting ownership is not the one the current owner
    /// proposed in `propose_plan_owner`.
    NotPendingOwner = 1021,
    /// `create_plans` was given no plans.
    EmptyBatch = 1022,
    /// `create_plans` was given more plans than `MAX_BATCH_PLANS`.
    BatchTooLarge = 1023,
    /// The next plan ID cannot be represented: the counter is at `u64::MAX`,
    /// or a batch would carry it past `u64::MAX`. No plan is written.
    PlanIdOverflow = 1024,
}

impl From<Error> for ErrorCode {
    fn from(error: Error) -> Self {
        ErrorCode::new(error as u32)
    }
}
