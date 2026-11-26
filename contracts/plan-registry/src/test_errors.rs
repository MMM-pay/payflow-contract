#![cfg(test)]
extern crate std;

use super::*;
use payflow_shared::{ErrorCode, ErrorDomain};

#[test]
fn every_error_code_is_in_the_plan_registry_range() {
    let errors = [
        (Error::AlreadyInitialized, 1),
        (Error::NotInitialized, 2),
        (Error::PlanNotFound, 3),
        (Error::NotPlanOwner, 4),
        (Error::InvalidAmount, 5),
        (Error::InvalidPeriod, 6),
        (Error::NameTooLong, 7),
        (Error::ProfileNotFound, 8),
        (Error::ProfileNameTooLong, 9),
        (Error::ProfileUrlTooLong, 10),
        (Error::KeeperBpsTooHigh, 11),
        (Error::SuccessorNotFound, 12),
        (Error::SelfSuccessor, 13),
        (Error::SuccessorTokenMismatch, 14),
        (Error::DescriptionTooLong, 15),
        (Error::PlanNameTooLong, 16),
        (Error::UriTooLong, 17),
        (Error::ReferralBpsTooHigh, 18),
        (Error::AmountTooLow, 19),
        (Error::PendingOwnershipNotFound, 20),
        (Error::NotPendingOwner, 21),
        (Error::EmptyBatch, 22),
        (Error::BatchTooLarge, 23),
        (Error::PlanIdOverflow, 24),
        (Error::PendingAdminNotFound, 25),
        (Error::NotPendingAdmin, 26),
    ];

    let mut seen = std::vec::Vec::new();
    for (error, local) in errors {
        let code = ErrorCode::from(error);
        assert_eq!(code.code(), ErrorDomain::PlanRegistry.code(local));
        assert_eq!(code.domain(), Some(ErrorDomain::PlanRegistry));
        assert_eq!(code.local(), Some(local));
        assert!(!seen.contains(&code.code()));
        seen.push(code.code());
    }
}
