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
    /// `create_plan` was given a referral share above MAX_REFERRAL_BPS.
    ReferralBpsTooHigh = 12,
    /// `accept_plan_owner` was called with no ownership transfer on record.
    PendingOwnershipNotFound = 13,
    /// `accept_plan_owner` was called by an address other than the one the
    /// owner proposed.
    NotPendingOwner = 14,
}
