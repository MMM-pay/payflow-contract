#Changelog

All notable changes to the Payflow contracts. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[semantic versioning](https://semver.org/). Before 1.0, a minor version may
change the contract interface.

## Unreleased

### Added

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
  for targeted recipient addresses. Verifies in `payflow-vault` and
  `payflow-subscription` that failing transfers revert transactions completely
  (deposits, withdrawals, debits, and charges across merchant, fee, and tip
  debits) and keep vault internal balances and token holdings consistent.
- `docs/adr/`: five architecture decision records (vault/billing split,
  `now + period` scheduling, constructors, per-position indexes,
  permissionless `charge`), linked from README.

Interface changes (`set_profile`, `profile`, `deactivate_page`,
`PlanSuccessorSet`, `set_successor`, `Plan.successor_id`, `create_plan`'s
`keeper_bps`, `charge`'s `keeper` argument and the new `tip`/`keeper`/
`keeper_bps` event fields) ripple into `payflow-backend`, `payflow-frontend` and
`payflow-docs`: follow-up issues to consume the new fields and pass the keeper
argument are tracked in the pull request.

### Changed

- **Breaking:** `Mandate` gains `paused_until` (the self-lifting pause
  schedule, 0 for none) and `refunded` (the running total the merchant has
  refunded). Existing deployments must be redeployed because stored mandates
  do not contain the new fields. This also requires downstream updates in
  `payflow-backend`, `payflow-frontend` and `payflow-docs`.
- **Breaking:** `Mandate` gains `created_at`, set when `subscribe` opens it and
  returned by `get_mandate`. Existing deployments must be redeployed because
  stored mandates do not contain the new field. This also requires downstream
  updates in `payflow-backend`, `payflow-frontend` and `payflow-docs`; follow-up
  issues should be linked in the pull request.
- **Breaking:** `charge` takes the settling `keeper: Address` and requires its
  authorization; it no longer settles without a signature. Any address may
  still act as keeper.
- **Breaking:** `plan-registry:create_plan` takes `keeper_bps` as its last
  argument, and `Plan`, `Mandate`, `PlanCreated`, `Subscribed` and `Charged`
  gain fields. Existing testnet deployments must be redeployed: stored `Plan`
  and `Mandate` values do not decode into the new shape.
- **Breaking:** `Plan` gains `successor_id` (0 = the plan has no successor),
  returned by `get_plan` and written by `set_successor`. Existing testnet
  deployments must be redeployed: stored `Plan` values do not decode into the
  new shape. This also requires downstream updates in `payflow-backend`,
  `payflow-frontend` and `payflow-docs`; follow-up issues should be linked in the
  pull request.
- **Breaking:** `merchant_plans`, `merchant_mandates` and `subscriber_mandates`
  take `start` and `limit` and return at most 50 ids per call.
- **Breaking:** `initialize` is replaced by `__constructor` in all three
  contracts.
- `rust-toolchain.toml` names the `wasm32v1-none` target that
  `stellar contract build` uses.

### Tests

- 42 → 63 tests, covering paging limits, entry footprint limits, merchant
  ending and constructor configuration.
- **New tests for this unreleased set:** scheduled pauses (no charge before
  `until`, self-resume with only the keeper's signature, manual resume,
  extension, rejected past dates and foreign signers), `committed_per_period`
  (mixed-period hand calculations, exclusion of paused/cancelled/completed
  mandates, per-subscriber and per-token isolation), refunds (over-refund
  rejected, cap grows with charges, works on cancelled mandates, merchant
  signature required, event links the refund to the mandate) and the vault
  allowlist (off by default, unlisted deposits rejected, removal blocks
  deposits but not withdrawals, admin auth required, no funds moved).
- **Keeper tip tests:** the tip is paid from the merchant's share, the
  subscriber pays the same total with or without it, a missing or wrong signer
  cannot settle or collect, and the registry rejects a tip above the cap.
- **New test: `admin_has_no_path_to_user_funds`** — asserts that `set_subscription`
  does not change any user balances.
- **New test: `fee_paid_to_contract_address`** — asserts that the protocol fee
  is transferred to the `fee_to` address when a mandate is charged.
- **New test: `set_successor`** — the replacement is stored on the retired plan
  through `get_plan`, self-successor, unknown-successor and cross-merchant
  pointers are rejected, a different token is rejected, and the merchant must
  sign.

## [0.1.0] - 2026-09-02

First testnet deployment: plan registry, vault and subscription contracts with
permissionless `charge`, spend caps, pause and cancel, and a capped protocol
fee.

[0.2.0]: https://github.com/MMM-pay/payflow-contract/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/MMM-pay/payflow-contract/releases/tag/v0.1.0
