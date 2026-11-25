#![cfg(test)]
extern crate std;

use super::*;
use payflow_shared::{ErrorCode, ErrorDomain};

#[test]
fn every_error_code_is_in_the_subscription_range() {
    let errors = [
        (Error::AlreadyInitialized, 1),
        (Error::NotInitialized, 2),
        (Error::MandateNotFound, 3),
        (Error::NotSubscriber, 4),
        (Error::MandateNotActive, 5),
        (Error::NotDue, 6),
        (Error::PlanInactive, 7),
        (Error::FeeTooHigh, 8),
        (Error::MaxChargesReached, 9),
        (Error::InvalidMaxCharges, 10),
        (Error::NotMerchant, 11),
        (Error::AmountOverflow, 12),
        (Error::InvalidUntil, 13),
        (Error::RefundExceedsReceived, 14),
        (Error::InvalidAmount, 15),
        (Error::DifferentToken, 16),
        (Error::DifferentMerchant, 17),
        (Error::ScheduleOverflow, 18),
        (Error::InvalidSeats, 19),
        (Error::SeatsNotApplicable, 20),
        (Error::SeatsOverflow, 21),
        (Error::InvalidMaxTotal, 22),
        (Error::PauseNotExpired, 23),
        (Error::PauseExpired, 24),
        (Error::BatchTooLarge, 25),
        (Error::EmptyBatch, 26),
        (Error::InvalidStart, 27),
        (Error::InvalidCancelReason, 28),
        (Error::InvalidSubscriberIndex, 29),
        (Error::NoPendingTransfer, 30),
        (Error::NotProposedOwner, 31),
        (Error::SameSubscriber, 32),
        (Error::MandateIndexFull, 33),
        (Error::PlanFull, 34),
        (Error::ChargeWouldSucceed, 35),
        (Error::PlanNotFound, 36),
        (Error::VaultCallFailed, 37),
        (Error::PlanChargesPaused, 38),
        (Error::ProtocolPaused, 39),
        (Error::UnexpectedTransferAmount, 40),
        (Error::PendingAdminNotFound, 41),
        (Error::NotPendingAdmin, 42),
        (Error::MandateIdTombstoned, 43),
        (Error::MandateAlreadyExists, 44),
        (Error::InvalidMandateId, 45),
        (Error::MandateStillActive, 46),
    ];

    let mut seen = std::vec::Vec::new();
    for (error, local) in errors {
        let code = ErrorCode::from(error);
        assert_eq!(code.code(), ErrorDomain::Subscription.code(local));
        assert_eq!(code.domain(), Some(ErrorDomain::Subscription));
        assert_eq!(code.local(), Some(local));
        assert!(!seen.contains(&code.code()));
        seen.push(code.code());
    }
}
