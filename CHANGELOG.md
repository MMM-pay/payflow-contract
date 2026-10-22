# Changelog

All notable changes to the Payflow contracts. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[semantic versioning](https://semver.org/). Before 1.0, a minor version may
change the contract interface.

## [0.2.0] - 2026-10-09

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
- **Test: admin has no path to user funds** — verifies that every admin-
  callable function leaves all user balances unchanged.
- **Test: fees paid to a contract address** — verifies that the protocol fee
  is correctly transferred to the `fee_to` address when a mandate is charged.
- **Test: `internal_balances_add_up_to_the_vault_holdings`** — 300 randomized
  deposit/withdraw/debit steps across four users, asserting after every step
  that the sum of internal balances equals the vault's token holdings.
- **Test: `charges_keep_vault_balances_in_sync`** — the same invariant driven
  through the real `charge` path in the subscription contract.
- `docs/adr/`: five architecture decision records (vault/billing split,
  `now + period` scheduling, constructors, per-position indexes,
  permissionless `charge`), linked from README.
- `plan-registry:create_plan` takes `available_until`, a unix timestamp after
  which the plan stops accepting new subscribers (0 = forever), carried in
  `Plan.available_until` and in `PlanCreated`. A promotion ends itself without
  anyone remembering to call `set_plan_active(false)`. Enforced by
  `subscription:subscribe`, which rejects an expired plan with the existing
  `PlanInactive` (7); mandates opened before the cutoff keep billing.
- `subscription:plan_mandate_count(plan_id)` and
  `subscription:plan_mandates(plan_id, start, limit)`: a paged per-plan index
  of mandate ids, the same one-entry-per-position layout as the merchant and
  subscriber indexes, so merchants can see per-plan adoption without an
  indexer. Pages are capped at `MAX_PAGE` (50).
- `vault:set_spend_limit(user, token, amount, window_secs)` and
  `vault:spend_limit(user, token)`: a subscriber-set ceiling on total debits
  per fixed window across all of their mandates in one token ("never debit
  more than 50 XLM per 30 days in total"). Enforced in `debit`; over the
  ceiling a charge fails atomically with the new error `SpendLimitExceeded`
  (6), the mandate stays due, and the keeper's backoff simply retries. Only
  the user can set their own limit; `amount = 0` disables it; the new error
  `InvalidWindow` (7) rejects a positive amount with a zero window. The limit
  can only make charges fail — it never moves funds and never blocks
  `withdraw`.
- `subscription:subscribe_with_deposit(subscriber, plan_id, max_charges,
  deposit)`: funds the vault and opens a mandate in one transaction, so a new
  subscriber signs once instead of signing a deposit and then a subscribe.
  The subscriber's single authorization covers the nested `vault:deposit` as
  a sub-invocation (asserted in an auth-tree test), and the flow is atomic:
  a failed deposit subscribes nothing, a failed subscribe rolls the deposit
  back.

Interface changes (`set_profile`, `profile`, `deactivate_page`,
`ProfileUpdated`, `create_plan`'s `keeper_bps` and `available_until`,
`charge`'s `keeper` argument, the new `tip`/`keeper`/`keeper_bps` and
`available_until` event fields, `Plan.available_until`, the new
`subscribe_with_deposit` and vault spend-limit entry points, and the new
error codes 6 and 7 in the vault) ripple into `payflow-backend`,
`payflow-frontend` and `payflow-docs`: follow-up issues to consume the new
fields, the one-transaction sign-up and the spend-limit UI are tracked in the
pull request.

### Changed

- **Breaking:** `charge` takes the settling `keeper: Address` and requires its
  authorization; it no longer settles without a signature. Any address may
  still act as keeper.
- **Breaking:** `plan-registry:create_plan` takes `keeper_bps` as its last
  argument, and `Plan`, `Mandate`, `PlanCreated`, `Subscribed` and `Charged`
  gain fields. Existing testnet deployments must be redeployed: stored `Plan`
  and `Mandate` values do not decode into the new shape.
- **Breaking:** `plan-registry:create_plan` takes `available_until` after
  `keeper_bps`, and `Plan` and `PlanCreated` gain the field. Existing testnet
  deployments must be redeployed for the same decode reason.
- **Breaking:** `merchant_plans`, `merchant_mandates` and `subscriber_mandates`
  take `start` and `limit` and return at most 50 ids per call.
- **Breaking:** `initialize` is replaced by `__constructor` in all three
  contracts.
- `rust-toolchain.toml` names the `wasm32v1-none` target that
  `stellar contract build` uses.

### Tests

- 42 → 63 tests, covering paging limits, entry footprint limits, merchant
  ending and constructor configuration.
- **Keeper tip tests:** the tip is paid from the merchant's share, the
  subscriber pays the same total with or without it, a missing or wrong signer
  cannot settle or collect, and the registry rejects a tip above the cap.
- **New test: `admin_has_no_path_to_user_funds`** — asserts that `set_subscription`
  does not change any user balances.
- **New test: `fee_paid_to_contract_address`** — asserts that the protocol fee
  is transferred to the `fee_to` address when a mandate is charged.
- **Plan cutoff tests:** a subscriber gets in one second before the cutoff and
  is rejected at it, existing mandates keep billing past it, and the cutoff
  rides in `PlanCreated` for indexers.
- **Per-plan index tests:** per-plan isolation, oldest-first order, paging
  bounds (`MAX_PAGE`, start past the end), cancelled mandates staying listed,
  and the subscribe cost staying flat with 301 mandates on one plan.
- **Spend limit tests:** over-the-ceiling debits fail atomically (through the
  real `charge` path), the next window resets, limit 0 disables it, replacing
  a limit resets the window, limits are scoped per (user, token), a wrong
  signer cannot set one, and the unlimited debit path writes no window state.
- **One-signature sign-up tests:** the auth tree a wallet builds from one
  subscriber signature (root invoke plus nested `deposit` plus the token
  `transfer` it makes) is sufficient; dropping the nested entries or using a
  different signer fails; a failed deposit subscribes nothing and burns no
  mandate id; an expired plan rolls the deposit back.
- 90 → 108 tests with these four features (18 new: 2 plan-registry, 9
  subscription, 7 vault — plus an existing cost test extended in place).

## [0.1.0] - 2026-09-02

First testnet deployment: plan registry, vault and subscription contracts with
permissionless `charge`, spend caps, pause and cancel, and a capped protocol
fee.

[0.2.0]: https://github.com/MMM-pay/payflow-contract/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/MMM-pay/payflow-contract/releases/tag/v0.1.0
