# Glossary: contract terms and the docs site

The [docs site glossary](https://mmm-pay.github.io/payflow-docs/glossary.html)
defines Payflow's vocabulary for merchants, subscribers and integrators. This
page maps each of those terms to what it actually is on chain: the contract
that owns it, the type, field or function that implements it, and the storage
key it lives under. Use it to check that a doc, a client or a comment means the
same thing the contracts do.

Everything here is grounded in the v0.2.0 sources. Where the docs site and the
contracts have drifted apart, the [drift section](#where-the-docs-site-and-the-contracts-differ)
says so. The contracts are the source of truth; fix the docs to match.

Docs-site pages referenced below:
[lifecycle](https://mmm-pay.github.io/payflow-docs/protocol/lifecycle.html) ·
[economics](https://mmm-pay.github.io/payflow-docs/protocol/economics.html) ·
[trust model](https://mmm-pay.github.io/payflow-docs/protocol/trust.html) ·
[contract reference](https://mmm-pay.github.io/payflow-docs/reference/contracts.html) ·
[errors](https://mmm-pay.github.io/payflow-docs/reference/errors.html)

## Core terms

These are the terms the docs glossary defines, in the same order.

| Docs term | Contract | On chain | Notes |
|-----------|----------|----------|-------|
| **Mandate** | subscription | `Mandate` struct, stored at `DataKey::Mandate(id)`; opened by `subscribe`, `subscribe_at`, `subscribe_with_deposit` | Ids come from `NextMandateId`, start at 1 and are never reused, even after the mandate ends. |
| **Charge** | subscription | `charge(keeper, mandate_id)` returning `ChargeResult`; previewed by `quote_charge` returning `ChargeQuote` | At most one per period. Refused with `PlanChargesPaused` while the plan's charging is paused. |
| **Plan** | plan-registry | `Plan` struct (defined once in `payflow-shared`), stored at `DataKey::Plan(id)`; created by `create_plan` / `create_plans` | Price, period and token are copied into the mandate at subscribe time. Ids come from `NextPlanId` and are never reused, even after the plan is retired. |
| **Plan registry** | plan-registry | The `PlanRegistry` contract | See [drift](#where-the-docs-site-and-the-contracts-differ): it is no longer "immutable except for an on/off switch". |
| **Keeper** | subscription | The `keeper` argument to `charge`; anyone may pass their own address | Paid a tip of `Mandate.keeper_bps` (at most `MAX_KEEPER_BPS` = 500) out of the merchant's share. See [ADR 0005](adr/0005-permissionless-charge.md). |
| **Vault** | vault | The `Vault` contract; balance at `DataKey::Balance(user, token)` | Only the contract named by `set_subscription` may `debit`. See [ADR 0001](adr/0001-vault-separate-from-billing.md). |
| **Token** | all | `Plan.token`, `Mandate.token` (a SEP-41 contract address) | Frozen into the mandate. `upgrade` refuses a plan in a different token (`DifferentToken`). |
| **Period** | plan-registry, subscription | `Plan.period`, `Mandate.period`, in seconds | At least `MIN_PERIOD`. The next charge is `now + period`; see [ADR 0002](adr/0002-now-plus-period-scheduling.md). |
| **Mandate state** | subscription | `MandateStatus`: `Active`, `Paused`, `Cancelled`, `Completed` | `Cancelled` and `Completed` are terminal. No call moves a mandate out of either. |
| **`next_charge` / `last_charge`** | subscription | `Mandate.next_charge`, `Mandate.last_charge` (Unix seconds) | `last_charge` is 0 until the first charge. `ChargeQuote.due_at` and `MandateSummary.next_charge` mirror `next_charge`. |
| **`fee_bps`** | subscription | `Mandate.fee_bps` (frozen); the current rate at `DataKey::FeeBps`, read by `fee_bps()` and set by `set_fee_bps` | Capped at `MAX_FEE_BPS` = 1000 (10%). A change applies only to mandates opened afterwards. |
| **Subscription contract** | subscription | The `Subscription` contract | Holds the addresses of the registry (`plan_registry()`) and vault (`vault()`). |
| **Subscription admin** | subscription | `DataKey::Admin`; `admin()`, `propose_admin`, `accept_admin`, `set_fee_bps`, `upgrade_wasm` | Handover is two-step and recorded in `PendingAdmin` until the successor accepts. Also the only key that can upgrade the contract's code. |
| **Vault admin** | vault | `DataKey::Admin`; `admin()`, `propose_admin`, `accept_admin`, `set_subscription`, `set_allowlist_enabled`, `set_token_allowed`, `set_deposit_cap`, `upgrade_wasm` | `set_subscription` is the custody risk the trust model describes; handover is two-step. |
| **`MIN_PERIOD`** | plan-registry | `pub const MIN_PERIOD: u64 = 60` | `create_plan` rejects shorter periods with `InvalidPeriod`. |
| **SEP-41** | vault | `soroban_sdk::token::TokenClient` calls in `deposit`, `withdraw*`, `debit*` | State is written before the transfer on every path. |

## Stellar primitives

| Docs term | On chain | Notes |
|-----------|----------|-------|
| **Stroop** | Every `amount` field is an `i128` in the token's smallest unit | For XLM, 1 XLM = 10 000 000 stroops. |
| **Base fee** | Not a contract value | Paid by whoever submits the transaction, e.g. the keeper. |
| **Basis points (bps)** | `BPS_DENOMINATOR` = 10 000 in subscription | Fee is `amount × fee_bps ÷ 10 000`, truncated. Keeper and referral shares are taken from the merchant's share, not the subscriber's total. |

## Contract terms the docs glossary does not define yet

These appear in the contracts, their events or their errors, and integrators
will meet them. They are candidates for the docs glossary.

| Term | Contract | On chain | Meaning |
|------|----------|----------|---------|
| **Merchant** | plan-registry, subscription | `Plan.merchant`, `Mandate.merchant` | The plan's owner. Can transfer a plan with `propose_plan_owner` / `accept_plan_owner`; an open mandate keeps its frozen `payout`. |
| **Subscriber** | subscription | `Mandate.subscriber` | The account that pays. Can transfer a mandate with `propose_mandate_owner` / `accept_mandate_owner`. |
| **Payout** | plan-registry, subscription | `Plan.payout` (`None` = merchant), `Mandate.payout` (frozen) | Where the merchant's share is sent. |
| **Keeper tip** | subscription | `Mandate.keeper_bps` | See *Keeper* above. |
| **Referral** | subscription | `Mandate.referrer`, `Mandate.referral_bps` (at most `MAX_REFERRAL_BPS` = 2000) | Share of the merchant's money paid to a referrer on each charge. |
| **Charge cap** | subscription | `Mandate.max_charges` (0 = open-ended); changed by `set_max_charges` | Lowering it to `charges_made` completes the mandate at once. Lowering it below `charges_made` is rejected with `InvalidMaxCharges`. |
| **Spend cap** | subscription | `Mandate.max_total` (0 = open-ended), `Mandate.total_spent` | Lifetime ceiling on debits. Reaching it completes the mandate. |
| **Seats** | subscription | `Mandate.seats`; changed by `set_seats` | `amount` already equals the per-seat price times `seats`. |
| **Seat cap** | plan-registry, subscription | `Plan.max_subscribers` (0 = unlimited); `plan_subscriber_count` | Counts open mandates only. A cancelled or completed mandate frees its seat. |
| **Pause** | subscription | `set_paused`, `pause_until`, `Mandate.paused_until`, `Mandate.max_pause_secs` | Subscriber-side. Not the same as a plan's charges being paused. |
| **Charges paused** | plan-registry | `set_plan_charges_paused` (alias `set_plan_paused`), `plan_charges_paused`, `DataKey::PlanChargesPaused` | Merchant-side switch that makes `charge` fail for every mandate on the plan without cancelling any. |
| **Grace period** | plan-registry, subscription | `Plan` grace at `DataKey::PlanGracePeriod`; frozen per mandate at `DataKey::MandateGracePeriod`; `mandate_grace_period`, `is_in_grace_period` | Window after `next_charge` during which a due, unpaid mandate is still considered in good standing. |
| **Failed-charge flag** | subscription | `flag_failed_charge`, `first_failed_charge`, `DataKey::FirstFailedCharge` | Permissionless. Only succeeds when the charge is due and the vault provably cannot cover it. |
| **Cancel reason** | subscription | `cancel_with_reason`, `CANCEL_REASON_*` constants 0–5 | Recorded in the `Cancelled` event only; not stored on the mandate. |
| **Ended (by merchant)** | subscription | `end_mandate` | Sets `MandateStatus::Cancelled`, the same terminal state as a subscriber cancel. |
| **Upgrade (plan change)** | subscription | `upgrade(subscriber, mandate_id, new_plan_id)` | Moves a mandate to another plan from the same merchant and token. See [ADR 0006](adr/0006-prorated-upgrade-charging.md). Not the same as `upgrade_wasm`. |
| **Successor** | plan-registry | `set_successor` | A plan's pointer to the plan that replaces it. Does not move any mandate. |
| **Retired plan** | plan-registry | `set_plan_active(false)`, `deactivate_page`; `Plan.active` | Stops new subscriptions (`PlanInactive`). The record and its id stay forever. |
| **Availability cutoff** | plan-registry | `Plan.available_until` (0 = forever) | After this time the plan accepts no new subscribers. |
| **Reserve** | vault | `set_reserve`, `reserved`, `reserved_total`, `unreserved_balance`; `DataKey::Reserved`, `DataKey::ReservedTotal` | Funds earmarked for one mandate. Only that mandate's debits can draw them, and `withdraw` cannot. |
| **Spend limit** | vault | `set_spend_limit`, `spend_limit`; `SpendLimit`, `SpendWindow` | Subscriber's own ceiling on debits per fixed window, across all mandates in one token. |
| **Deposit cap** | vault | `set_deposit_cap`, `deposit_cap`, `can_deposit` | Admin-set maximum balance per (user, token). 0 = unlimited. |
| **Token allowlist** | vault | `set_allowlist_enabled`, `set_token_allowed`, `allowlist_enabled`, `token_allowed` | Off by default. When on, only listed tokens can be deposited. |
| **Operation rate limit** | vault | `VAULT_OPERATION_WINDOW_SECS` = 60, `MAX_VAULT_OPERATIONS_PER_WINDOW` = 8 | Caps deposit and withdraw calls per (user, token) per window. |
| **Upgrade history** | all three | `upgrade_history()`, `UpgradeRecord`, `DataKey::UpgradeHistory` | Append-only log of `upgrade_wasm` calls. The initial deployment is not recorded. |
| **`MIN_CHARGE_AMOUNT`** | shared | `100` (stroops) | `create_plan` rejects smaller amounts with `AmountTooLow`. |
| **`MAX_PAGE`** | plan-registry, subscription | `50` | Upper bound on every paged read. See [ADR 0004](adr/0004-per-position-indexes.md). |
| **Error ranges** | shared | `PLAN_REGISTRY_ERROR_BASE` = 1000, `SUBSCRIPTION_ERROR_BASE` = 2000, `VAULT_ERROR_BASE` = 3000 | Every error code is unique across the three contracts. |

## Where the docs site and the contracts differ

Checked against the docs glossary as of v0.2.0. Each item is a docs fix, not a
contract change.

1. **"Plan registry — immutable except for an on/off switch."** A plan's price,
   period and token are still immutable. But a merchant can now also rename a
   plan (`set_plan_name`), pause its charges (`set_plan_charges_paused`), set
   its grace period (`set_plan_grace_period`), link a successor
   (`set_successor`) and transfer it (`propose_plan_owner` /
   `accept_plan_owner`).
2. **"The terms are copied into the mandate at `subscribe` time and never
   re-read."** The price is never re-read. But `charge` does read the plan's
   charges-paused flag, and `upgrade` moves a mandate onto a different plan's
   terms.
3. **"Mandate state … `Cancelled`."** The docs and the `MandateStatus` comment
   describe `Cancelled` as the subscriber's doing. A merchant's `end_mandate`
   produces the same state.
4. **"`Completed`."** Besides reaching `max_charges` or `max_total` through a
   charge, a mandate also completes when the subscriber lowers `max_charges` to
   the number of charges already made.
5. **"The subscriber can withdraw (drain) the vault at any time."** Only the
   unreserved balance can be withdrawn, and deposits and withdrawals are rate
   limited per window.
6. **"Keeper."** The docs do not mention the keeper tip, which is the reason a
   third party would run a keeper at all.
