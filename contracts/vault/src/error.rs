//! Error types returned by vault contract operations.

use payflow_shared::ErrorCode;
use soroban_sdk::contracterror;

/// Errors returned by the vault contract.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    /// The contract has already been initialized.
    AlreadyInitialized = 3001,
    /// The contract has not been initialized with an admin.
    NotInitialized = 3002,
    /// An amount was zero or negative, or an invalid parameter was given.
    InvalidAmount = 3003,
    /// The user's spendable vault balance is insufficient for the requested amount.
    InsufficientBalance = 3004,
    /// No subscription contract address has been configured.
    SubscriptionNotSet = 3005,
    /// The allowlist is enabled and this token is not on it. Deposits of
    /// unlisted tokens are rejected; withdrawals never are.
    TokenNotAllowed = 3006,
    /// The deposit would take the subscriber's balance for this token above
    /// the cap the admin set. A cap of 0 means unlimited, so this error only
    /// fires while a cap is in force. Withdrawals are never blocked.
    DepositCapExceeded = 3007,
    /// The subscriber has not opted in to allowance top-up for this mandate.
    /// Allowance fallback is disabled by default and must be enabled per subscriber.
    AllowanceNotEnabled = 3008,
    /// The shortfall could not be covered by the SEP-41 allowance granted to the vault,
    /// either because the allowance is too small or the subscriber never granted one.
    AllowanceInsufficient = 3009,
    /// The SEP-41 allowance granted to the vault has expired (its expiration ledger has passed).
    AllowanceExpired = 3010,
    /// `amount_a + amount_b` passed to `debit_split` would overflow `i128`.
    /// The split is rejected rather than wrapped, because a wrapped total
    /// would under-charge the subscriber.
    AmountOverflow = 3011,
    /// The debit would take the user over their per-window spend limit.
    /// New variants go at the end so existing codes keep their numbers.
    SpendLimitExceeded = 3012,
    /// `set_spend_limit` was given a positive amount with a zero window.
    InvalidWindow = 3013,
    /// `set_reserve` would earmark more than the subscriber's balance for
    /// this token across all of their mandates (#295).
    ReserveExceedsBalance = 3014,
    /// The (user, token) pair has reached the per-window limit on
    /// deposit/withdraw operations (#314). The limit bounds rapid cycling that
    /// repeatedly rewrites balances and emits events; it resets when the
    /// window rolls over.
    RateLimitExceeded = 3015,
    /// A token transfer did not move exactly the requested amount, so the
    /// token is not safe for the vault's exact-amount accounting (for example
    /// a fee-on-transfer token). The whole operation is rolled back.
    UnexpectedTransferAmount = 3016,
    /// A transfer destination is the zero/unset address: the all-zero contract
    /// or account strkey. Sending funds there would burn them (#236).
    ZeroAddress = 3017,
    /// The reserve would take the subscriber's total reserve above the cap the
    /// vault was deployed with (#348). A minimum free balance therefore stays
    /// withdrawable no matter how many mandates the subscriber reserves for.
    ReserveCapExceeded = 3018,
    /// `__constructor` was given a reserve cap above 10,000 basis points
    /// (100% of the balance), which is not a meaningful cap (#348).
    InvalidReserveCap = 3019,
    /// `accept_admin` was called with no pending admin handover, or after the
    /// proposal was already accepted (#238).
    PendingAdminNotFound = 3020,
    /// The address accepting the admin role is not the one the current admin
    /// proposed in `propose_admin` (#238).
    NotPendingAdmin = 3021,
}

impl From<Error> for ErrorCode {
    fn from(error: Error) -> Self {
        ErrorCode::new(error as u32)
    }
}
