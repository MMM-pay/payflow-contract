#Changelog

All notable changes to the Payflow contracts. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[semantic versioning](https://semver.org/). Before 1.0, a minor version may
change the contract interface.

## Unreleased

### Added

- `subscription:cancel_page(subscriber, start, limit)` lets a subscriber cancel
  up to 50 mandates at once, skipping mandates already cancelled or completed.
  `payflow-backend`, `payflow-frontend` and `payflow-docs` need follow-up issues
  to expose and document the new operation.

- `subscription:subscribe` accepts `max_total` (0 = uncapped). Mandates track
  settled spend and complete without a debit when the next full charge would
  exceed the cap; the existing `max_charges` limit continues to apply, and
  either limit may complete a mandate first. `Subscribed` includes `max_total`.
The `subscribe` argument and `Mandate`/`Subscribed` fields also change for
`max_total` and `total_spent`. These interface changes ripple into
`payflow-backend`, `payflow-frontend` and `payflow-docs`; create and link
follow-up issues in the pull request.

- **Breaking:** `subscribe` gains `max_total`, `Mandate` gains `max_total` and
  `total_spent`, and `Subscribed` gains `max_total`. Existing deployments must
  be redeployed because stored mandates do not contain the new fields. Follow-up
  issues for `payflow-backend`, `payflow-frontend` and `payflow-docs` should be
  linked in the pull request.

- `plan-registry:set_plan_name(merchant, plan_id, name)`: a merchant can rename
  any of its plans. The name is bounded by `MAX_NAME_LEN` bytes (64); longer
  names are rejected with `PlanNameTooLong`. Only the plan's merchant may rename
  it. Emits `PlanRenamed { merchant, plan_id, name }` so indexers and the UI can
  update immediately.

- `subscription:quote_charge(mandate_id)` returns a read-only
  `ChargeQuote { amount, fee, merchant_amount, due_at, chargeable }`, so clients
  no longer have to reimplement the contract's integer fee split. `charge` and
  `quote_charge` share one private split helper, so the quote cannot drift from
  settlement; `chargeable` shares the `is_due` check. Quoting an unknown mandate
  returns `MandateNotFound`, and quoting never writes storage or emits an
  event. `merchant_amount` is net of both the protocol fee and the keeper tip,
  and `chargeable` does not consider the vault balance.
Interface changes (`quote_charge`/`ChargeQuote`, `set_profile`, `profile`,
`deactivate_page`, `ProfileUpdated`, `create_plan`'s `keeper_bps`, `charge`'s
`keeper` argument and the new `tip`/`keeper`/`keeper_bps` event fields) ripple
into `payflow-backend`, `payflow-frontend` and `payflow-docs`: follow-up issues
to consume the new fields, call `quote_charge` instead of recomputing the split,
and pass the keeper argument are tracked in the pull request.
- **`quote_charge` tests** — the quote matches `charge` for a price that
  rounds (99 stroops at 100 bps), excludes the keeper tip from
  `merchant_amount`, returns `MandateNotFound` for an unknown mandate, never
  changes storage/balances/events, mirrors `is_due` across active, paused,
  cancelled and completed mandates, needs no authorization, and reports
  `AmountOverflow` instead of panicking on an extreme price.

### Security

- **contracts:** add `#![forbid(unsafe_code)]` to every contract crate
  (`plan-registry`, `vault`, `subscription`). Any attempt to introduce an
  `unsafe` block now fails the build immediately. No interface or event changes.
  Closes #97.

- **subscription:** `set_seats(subscriber, mandate_id, seats)` lets the
  subscriber change the seat count on an active mandate. The new count is stored
  in `Mandate.seats` and takes effect from the next charge; no proration is
  applied. Only the subscriber may call it, the mandate must be active (not
  cancelled or completed), and `seats` must be at least 1, otherwise the call
  fails with the new `InvalidSeats` error (code 18). Emits the new `SeatsChanged`
  event (topics `mandate_id`, `subscriber`; data `old_seats`, `new_seats`) and a
  `MandateUpdated` snapshot. `Mandate` gains the `seats` field (0 for flat-rate
  mandates that were opened before this change or that do not use seat-based
  pricing). This is an interface change: `payflow-backend`, `payflow-frontend`
  and `payflow-docs` need to read the new `Mandate.seats` field, handle the new
  `seats_changed` event, and map the new error code. Closes #93.

### Changed

- **subscription:** `Mandate` gains a `seats: u32` field, defaulting to `0` for
  mandates that are not seat-based or opened before this change. This is a
  breaking storage change: existing deployed mandates do not contain the new
  field and will fail to decode on deployments that have not been redeployed.
  `payflow-backend`, `payflow-frontend` and `payflow-docs` need downstream
  updates.

- **tests:** the subscription, vault and plan-registry suites will assert the
  exact `env.auths()` tree for every state-changing function, and each mutating
  function gains a negative test using `mock_auths` with the wrong signer. No
  interface, event or error changes. Closes the auth-tree test issue.

- **contracts:** expose `version()` on the subscription, plan-registry and vault
  contracts so clients can identify the deployed crate version. This interface
  addition needs handling in payflow-backend, payflow-frontend and payflow-docs.
  Closes #66.

- **vault:** an optional per-token deposit cap while the protocol is unaudited.
  The admin can call `set_deposit_cap(user, token, amount)` to cap the balance
  one subscriber may hold for one token; a cap of `0` means unlimited, which is
  the default and what an existing deployment runs with. A deposit that would
  take the balance over the cap is rejected with the new `DepositCapExceeded`
  (#7) before any token moves. The cap only gates new deposits: a balance
  that is already above a newly set cap can still be withdrawn and can still be
  debited by the subscription contract, so funds are never trapped. The new
  `deposit_cap_set` event (topics `user`, `token`; data `cap`) needs handling
  in payflow-backend and payflow-frontend, and the new error code needs a
  message in payflow-docs. Closes #104.

- **plan-registry:** a short plan description. `Plan` gains a `description`
  field (at most 280 bytes, may be empty) and `create_plan` takes it as a new
  argument after `name`, so a subscriber can see what a plan includes without
  leaving the app. The `plan_created` event carries the description too, so
  indexers and apps need no follow-up contract read. A description longer than
  the limit is rejected with the new `DescriptionTooLong` error (#15), and a
  description at the limit is accepted. This is an interface change:
  payflow-backend, payflow-frontend and payflow-docs need to read the new
  field and to map the new error code. Closes #91.

- **plan-registry:** list plans by token. A per-token index is maintained
  with the same per-position layout as the per-merchant index, exposing
  `token_plans(token, start, limit)` and `token_plan_count(token)` so apps
  and storefronts can list all plans priced in a given asset without an
  off-chain indexer. Writing to the index adds constant cost to `create_plan`.
  This is an interface addition: payflow-backend, payflow-frontend and
  payflow-docs can query the new read endpoints. Closes #106.

- **subscription:** `subscribe_many(subscriber, plan_ids, max_charges)` opens
  one mandate per plan id in a single transaction, for bundles such as an app
  plus a support plan. The subscriber authorizes the batch once. At most 5 plans
  may be passed per call, and an empty batch is rejected. The call is all or
  nothing: if any plan is inactive or missing, no mandate is created and no
  state changes. Each resulting mandate is subscribed exactly as `subscribe`
  would, with its own price, period and frozen fee. This is an interface
  addition: payflow-backend and payflow-frontend need the new entry point and
  the `BatchTooLarge` error, and payflow-docs need the new error code.
  Closes #95.

- **subscription:** `upgrade(subscriber, mandate_id, new_plan_id)` moves a
  mandate onto another plan of the same merchant, denominated in the same
  token. Moving to a more expensive plan collects the price difference for
  the part of the current period that is still ahead, prorated and truncated
  towards zero, so the subscriber never pays more than the exact difference
  for the time that is left and the contract holds no fractional value.
  Moving to a cheaper plan collects nothing and refunds nothing: it takes
  effect at the next charge, because the period already paid for stays paid
  for. The billing date does not move — `next_charge` is left alone, and the
  new price applies from that charge. The protocol fee stays frozen at the
  rate the mandate was opened with, and the keeper tip of the new plan is
  clamped to `MAX_KEEPER_BPS`; no tip is paid on the upgrade itself, because
  no charge was settled. A plan that is inactive, in another token or from
  another merchant is rejected with the new `DifferentToken` (#16) and
  `DifferentMerchant` (#17) errors. The rounding rule and the reasoning
  behind it are written up in [ADR 0006](docs/adr/0006-prorated-upgrade-charging.md).
  payflow-backend, payflow-frontend and payflow-docs need the new
  `subscription_upgraded` event and the new error codes. Closes #94.

- **subscription:** `estimated_end(mandate_id)` returns the projected final
  charge timestamp for capped mandates using the current `next_charge` schedule,
  including drift after late charges. It returns `None` for open-ended mandates
  and the actual final charge timestamp after completion. This read interface
  addition needs handling in payflow-backend, payflow-frontend and payflow-docs;
  payflow-docs also needs a message for the new `ScheduleOverflow` error. Closes
  #105.

### Changed

- **contracts:** document every public item across all crates and enforce
  `#![deny(missing_docs)]` so new undocumented public items fail `cargo doc`.
  No interface or event changes. Closes #64.

- **subscription:** `charge(keeper, mandate_id)` now returns a `ChargeResult`
  containing the merchant amount, protocol fee, charge count, next charge time
  and completion flag. The return values match the corresponding `Charged`
  event fields, so callers do not need an extra contract read. This contract
  interface change needs handling in payflow-backend, payflow-frontend and
  payflow-docs. Closes #89.

# # [0.2.0] - 2026-10-09

Redeployed to testnet; the ids are in [`deployments/testnet.env`](deployments/testnet.env).

### Security

- **A merchant could be blocked from taking new subscribers.** Each merchant's
  mandate ids were kept in one growing list that every `subscribe` rewrote.
  Anyone could open throwaway mandates against a merchant's plan until the list
  passed the ledger entry size limit, after which every new subscription to
  that merchant failed. Indexes are now stored one entry per position, so a
  subscribe writes the same small entries however many mandates exist. The same
  layout is used for subscriber mandates and merchant plans.
- **Contracts could be claimed between deploy and initialize.** Each contract
  was deployed and then configured by a separate `initialize` call, which
  anyone could have called first. Configuration is now passed to a constructor
  that runs in the deploy transaction.

### Added

- `end_mandate(merchant, mandate_id)`: a merchant can permanently stop billing
  an open or paused mandate, for example when it discontinues a service.
  Deactivating a plan only stops new subscriptions. Emits `MandateEnded`.
- `merchant_mandate_count`, `subscriber_mandate_count`, `merchant_plan_count`
  and `next_plan_id`, so clients can page through lists and enumerate plans.
- `deploy.sh` writes the deployment to `deployments/<network>.env`, including
  the ledger it was deployed at so indexers can start there. `demo.sh` reads it.
- Plans have a merchant-set `name` of up to 64 bytes.
- Each mandate stores the protocol fee it was opened with. An admin fee change
  applies only to mandates opened afterwards.

### Changed

- **Breaking:** `merchant_plans`, `merchant_mandates` and `subscriber_mandates`
  take `start` and `limit` and return at most 50 ids per call.
- **Breaking:** `initialize` is replaced by `__constructor` in all three
  contracts.
- `rust-toolchain.toml` names the `wasm32v1-none` target that
  `stellar contract build` uses.

### Tests

- 42 → 63 tests, covering paging limits, entry footprint limits, merchant
  ending and constructor configuration.

## Unreleased

### Added

- Generated TypeScript clients for the plan registry, vault and subscription
  contracts are published together as `@mmm-pay/contracts` on tagged releases.
  CI verifies the generated package, and a manual testnet smoke workflow calls
  the registry's read-only `next_plan_id` method. Follow-up adoption issues in
  `payflow-frontend`, `payflow-backend` and `payflow-docs` should replace
  handwritten wrappers and document the package as appropriate.

### Security

- **Settlement no longer depends on one operator.** A plan may set
  `keeper_bps`, a tip of at most 5% of the merchant's share paid to whoever
  settles a charge. Competing keepers can now be paid for settling a due
  mandate. The tip comes out of the merchant's share, so the subscriber's total
  debit is unchanged, and it is frozen into each mandate at subscribe time. The
  front-running analysis is in [`SECURITY.md`](SECURITY.md).

- **Vault admin has no path to user funds.** The vault admin can only call
  `set_subscription`, which repoints the subscription contract. No admin
  function (including `set_subscription`) touches user balances directly.
  This is pinned down by a new test `admin_has_no_path_to_user_funds`.

### Added

- `subscription:pause_until(subscriber, mandate_id, until)`: a pause that
  lifts by itself. The mandate is paused like `set_paused` does, but once the
  ledger clock reaches `until`, `charge` and `is_due` treat it as Active again
  without any transaction from the subscriber. `next_charge` is pushed to at
  least `until`. New error `InvalidUntil` (13). Manual resume via
  `set_paused(false)` still works while a date is set.
- `subscription:committed_per_period(subscriber, token, period_secs)`: the
  subscriber's total committed spend per period, normalising mandates with
  different billing periods to the period the caller asks about. Cancelled,
  completed and still-paused mandates are excluded; at most `MAX_PAGE`
  mandates are read so the call stays inside the ledger footprint limit.
- `subscription:refund(merchant, mandate_id, amount)`: a merchant returns
  money from its own wallet straight to the subscriber's wallet, with the
  refund recorded on the mandate and emitted as `Refunded`, so disputes can be
  settled from chain data. Total refunds are capped at what the merchant
  actually collected for that mandate (price minus the frozen fee and tip, per
  settled charge), and refunds keep working on cancelled mandates. New errors
  `RefundExceedsReceived` (14) and `InvalidAmount` (15).
- `vault:set_allowlist_enabled(bool)`, `vault:set_token_allowed(token, bool)`
  and the views `allowlist_enabled` / `token_allowed`: an admin token
  allowlist. When enabled, `deposit` rejects tokens the admin has not vouched
  for with the new error `TokenNotAllowed` (6), so a malicious token contract
  cannot emit misleading `Deposit` events. Off by default (testnet keeps
  accepting every SEP-41 token), and withdrawals are never checked, so
  allowlisting can never trap funds.
- Snapshot tests pin the XDR layouts of every plan-registry, vault and
  subscription event. No interface changed.

- `plan-registry:create_plan` takes `keeper_bps`, the share of each charge paid
  to whoever settles it, capped at `MAX_KEEPER_BPS` (500 = 5%) with the new
  error `KeeperBpsTooHigh` (11). Plans carry it in `Plan.keeper_bps` and in
  `PlanCreated`.
- `subscription:charge(keeper, mandate_id)` pays the plan's tip to `keeper` out
  of the merchant's share. Mandates store `keeper_bps` alongside `fee_bps`;
  `Charged` gains `tip` and `keeper`, and `Subscribed` gains `keeper_bps`.
- `contracts/shared`: the `Plan` type and `MAX_KEEPER_BPS` are defined once and
  used by both the registry and the subscription contract, so the interface
  between them cannot drift.
- `plan-registry:set_profile(merchant, name, url)` and
  `plan-registry:profile(merchant)`: a merchant-set, self-declared profile
  (display name and URL, both length-limited) shown alongside plans so
  subscribers can tell who they are paying. Emits `ProfileUpdated`; the
  profile is not verified by the contract.
- `plan-registry:deactivate_page(merchant, start, limit)`: deactivates up to
  `MAX_PAGE` of a merchant's plans in one call; only the merchant's own plans
  are touched.
- `plan-registry:set_successor(merchant, plan_id, successor_id)`: points a
  retired plan at the plan that replaces it, so subscribers of the old plan have
  an on-chain pointer to the new one and apps can suggest the replacement. Both
  plans must be owned by the merchant and be denominated in the same token; a
  plan cannot point at itself. Emits `PlanSuccessorSet`.
- **Test: admin has no path to user funds** — verifies that every admin-
  callable function leaves all user balances unchanged.
- **Test: fees paid to a contract address** — verifies that the protocol fee
  is correctly transferred to the `fee_to` address when a mandate is charged.
- **Test: `internal_balances_add_up_to_the_vault_holdings`** — 300 randomized
  deposit/withdraw/debit steps across four users, asserting after every step
  that the sum of internal balances equals the vault's token holdings.
- **Test: `charges_keep_vault_balances_in_sync`** — the same invariant driven
  through the real `charge` path in the subscription contract.
- **Test: token transfer failure behaviour** — adds `payflow-test-token`, a
  test-only token contract that can be configured to fail transfers globally or
  for targeted 
