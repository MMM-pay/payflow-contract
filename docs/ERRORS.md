# Contract error codes

This page is the integration reference for the errors returned by the contracts in this repository. Error values are exposed as absolute `u32` codes. The ranges are intentionally separated by contract:

- **1000 range** — `plan-registry`
- **2000 range** — `subscription`
- **3000 range** — `vault`

The Rust enums remain the source of truth: [plan-registry errors](../contracts/plan-registry/src/error.rs), [subscription errors](../contracts/subscription/src/error.rs), and [vault errors](../contracts/vault/src/error.rs). Error variants are append-only so existing client mappings remain stable.

## Plan registry (1001–1026)

| Code | Variant | Returned when |
| --- | --- | --- |
| 1001 | `AlreadyInitialized` | Initialization has already been completed. |
| 1002 | `NotInitialized` | A method requires initialization, but it has not happened. |
| 1003 | `PlanNotFound` | The requested plan does not exist. |
| 1004 | `NotPlanOwner` | The caller is not the plan owner. |
| 1005 | `InvalidAmount` | An amount is zero or otherwise invalid for the operation. |
| 1006 | `InvalidPeriod` | The supplied billing period is invalid. |
| 1007 | `NameTooLong` | A profile name exceeds its maximum length. |
| 1008 | `ProfileNotFound` | The requested profile does not exist. |
| 1009 | `ProfileNameTooLong` | A profile name exceeds the profile-name limit. |
| 1010 | `ProfileUrlTooLong` | A profile URL exceeds the profile-URL limit. |
| 1011 | `KeeperBpsTooHigh` | Keeper basis points exceed the configured maximum. |
| 1012 | `SuccessorNotFound` | The proposed successor profile cannot be found. |
| 1013 | `SelfSuccessor` | The owner attempted to set itself as successor. |
| 1014 | `SuccessorTokenMismatch` | The successor uses a different token. |
| 1015 | `DescriptionTooLong` | A plan description exceeds the maximum length. |
| 1016 | `PlanNameTooLong` | A plan name exceeds the maximum length. |
| 1017 | `UriTooLong` | A plan URI exceeds the maximum length. |
| 1018 | `ReferralBpsTooHigh` | Referral basis points exceed the configured maximum. |
| 1019 | `AmountTooLow` | The amount is below the minimum accepted amount. |
| 1020 | `PendingOwnershipNotFound` | No pending ownership transfer exists. |
| 1021 | `NotPendingOwner` | The caller is not the pending owner. |
| 1022 | `EmptyBatch` | A batch operation was submitted without items. |
| 1023 | `BatchTooLarge` | A batch exceeds the contract's maximum size. |
| 1024 | `PlanIdOverflow` | The next plan ID would exceed `u64`; no plan is written. |
| 1025 | `PendingAdminNotFound` | No admin handover is pending, or the proposal was already accepted. |
| 1026 | `NotPendingAdmin` | The accepting address is not the one the current admin proposed. |

## Subscription (2001–2042)

| Code | Variant | Returned when |
| --- | --- | --- |
| 2001 | `AlreadyInitialized` | Initialization has already been completed. |
| 2002 | `NotInitialized` | A method requires initialization, but it has not happened. |
| 2003 | `MandateNotFound` | The requested subscription mandate does not exist. |
| 2004 | `NotSubscriber` | The caller is not the mandate subscriber. |
| 2005 | `MandateNotActive` | The mandate is not active. |
| 2006 | `NotDue` | A charge was attempted before its due time. |
| 2007 | `PlanInactive` | The referenced plan is inactive. |
| 2008 | `FeeTooHigh` | The configured fee exceeds the allowed maximum. |
| 2009 | `MaxChargesReached` | The mandate has reached its maximum number of charges. |
| 2010 | `InvalidMaxCharges` | The maximum-charge value is invalid. |
| 2011 | `NotMerchant` | The caller is not the merchant for the mandate. |
| 2012 | `AmountOverflow` | Arithmetic overflow occurred while calculating an amount. |
| 2013 | `InvalidUntil` | The supplied expiry/`until` value is invalid. |
| 2014 | `RefundExceedsReceived` | The requested refund exceeds the amount received. |
| 2015 | `InvalidAmount` | An amount is zero or otherwise invalid for the operation. |
| 2016 | `DifferentToken` | A token differs from the token recorded for the mandate or plan. |
| 2017 | `DifferentMerchant` | A merchant differs from the merchant recorded for the mandate. |
| 2018 | `ScheduleOverflow` | The next scheduled charge cannot be represented safely. |
| 2019 | `InvalidSeats` | The supplied seat count is invalid. |
| 2020 | `SeatsNotApplicable` | Seats were supplied for a plan that does not use seats. |
| 2021 | `SeatsOverflow` | Seat arithmetic overflowed. |
| 2022 | `InvalidMaxTotal` | The maximum total amount is invalid. |
| 2023 | `PauseNotExpired` | A paused mandate cannot be resumed yet. |
| 2024 | `PauseExpired` | An operation requires a currently valid pause, but it has expired. |
| 2025 | `BatchTooLarge` | A batch exceeds the contract's maximum size. |
| 2026 | `EmptyBatch` | A batch operation was submitted without items. |
| 2027 | `InvalidStart` | The supplied start time is invalid. |
| 2028 | `InvalidCancelReason` | The cancellation reason is not recognized or allowed. |
| 2029 | `InvalidSubscriberIndex` | The subscriber index is outside the mandate's subscriber list. |
| 2030 | `NoPendingTransfer` | No subscriber transfer is pending. |
| 2031 | `NotProposedOwner` | The caller is not the proposed owner. |
| 2032 | `SameSubscriber` | The proposed subscriber is already the current subscriber. |
| 2033 | `MandateIndexFull` | No more mandates can be stored for the subscriber. |
| 2034 | `PlanFull` | The plan has reached its mandate capacity. |
| 2035 | `ChargeWouldSucceed` | A dry-run/check expected the charge to succeed, so no failure was produced. |
| 2036 | `PlanNotFound` | The referenced plan does not exist. |
| 2037 | `VaultCallFailed` | A call to the vault contract failed. |
| 2038 | `PlanChargesPaused` | Charges are paused for the referenced plan. |
| 2039 | `ProtocolPaused` | Protocol-wide operations are paused. |
| 2040 | `UnexpectedTransferAmount` | A refund transfer did not deliver the exact amount (for example a fee-on-transfer token); the refund is rolled back. |
| 2041 | `PendingAdminNotFound` | No admin handover is pending, or the proposal was already accepted. |
| 2042 | `NotPendingAdmin` | The accepting address is not the one the current admin proposed. |

## Vault (3001–3021)

| Code | Variant | Returned when |
| --- | --- | --- |
| 3001 | `AlreadyInitialized` | Initialization has already been completed. |
| 3002 | `NotInitialized` | A method requires initialization, but it has not happened. |
| 3003 | `InvalidAmount` | An amount is zero or otherwise invalid for the operation. |
| 3004 | `InsufficientBalance` | The account or reserve lacks enough balance. |
| 3005 | `SubscriptionNotSet` | The required subscription configuration is missing. |
| 3006 | `TokenNotAllowed` | The token is not enabled by the vault. |
| 3007 | `DepositCapExceeded` | A deposit would exceed the configured cap. |
| 3008 | `AllowanceNotEnabled` | Allowance-based spending is disabled. |
| 3009 | `AllowanceInsufficient` | The allowance is smaller than the requested spend. |
| 3010 | `AllowanceExpired` | The allowance is no longer valid. |
| 3011 | `AmountOverflow` | Arithmetic overflow occurred while calculating an amount. |
| 3012 | `SpendLimitExceeded` | The requested spend exceeds the configured limit. |
| 3013 | `InvalidWindow` | The supplied rate-limit window is invalid. |
| 3014 | `ReserveExceedsBalance` | The requested reserve is greater than the available balance. |
| 3015 | `RateLimitExceeded` | The operation exceeds the configured rate limit. |
| 3016 | `UnexpectedTransferAmount` | A token transfer did not move the exact amount (for example a fee-on-transfer token); the operation is rolled back. |
| 3017 | `ZeroAddress` | A withdrawal or payout destination is the zero/unset account or contract address. |
| 3018 | `ReserveCapExceeded` | A reserve would take the subscriber's total reserve above the vault's configured reserve cap. |
| 3019 | `InvalidReserveCap` | The constructor was given a reserve cap above 10,000 basis points. |
| 3020 | `PendingAdminNotFound` | No admin handover is pending, or the proposal was already accepted. |
| 3021 | `NotPendingAdmin` | The accepting address is not the one the current admin proposed. |

When adding a new error, append it to the relevant enum and update this catalog in the same change. Clients should match on the numeric code and handle unknown future codes gracefully.
