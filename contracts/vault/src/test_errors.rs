#![cfg(test)]
extern crate std;

use super::*;
use payflow_shared::{ErrorCode, ErrorDomain};

#[test]
fn every_error_code_is_in_the_vault_range() {
    let errors = [
        (Error::AlreadyInitialized, 1),
        (Error::NotInitialized, 2),
        (Error::InvalidAmount, 3),
        (Error::InsufficientBalance, 4),
        (Error::SubscriptionNotSet, 5),
        (Error::TokenNotAllowed, 6),
        (Error::DepositCapExceeded, 7),
        (Error::AllowanceNotEnabled, 8),
        (Error::AllowanceInsufficient, 9),
        (Error::AllowanceExpired, 10),
        (Error::AmountOverflow, 11),
        (Error::SpendLimitExceeded, 12),
        (Error::InvalidWindow, 13),
        (Error::ReserveExceedsBalance, 14),
        (Error::RateLimitExceeded, 15),
        (Error::UnexpectedTransferAmount, 16),
        (Error::ZeroAddress, 17),
        (Error::ReserveCapExceeded, 18),
        (Error::InvalidReserveCap, 19),
        (Error::PendingAdminNotFound, 20),
        (Error::NotPendingAdmin, 21),
        (Error::InvalidAddress, 22),
    ];

    let mut seen = std::vec::Vec::new();
    for (error, local) in errors {
        let code = ErrorCode::from(error);
        assert_eq!(code.code(), ErrorDomain::Vault.code(local));
        assert_eq!(code.domain(), Some(ErrorDomain::Vault));
        assert_eq!(code.local(), Some(local));
        assert!(!seen.contains(&code.code()));
        seen.push(code.code());
    }
}
