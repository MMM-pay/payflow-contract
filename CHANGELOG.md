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
  `now + period` scheduling, constructors, per-position indexes, permissionless
  `charge`), linked from README.
- `vault:deposit_for(from, user, token, amount)`: one account pays into another
  account's vault balance — employers, parents and sponsors funding someone
  else's subscriptions. `from` authorizes and pays; only `user` can later
  withdraw or be debited the credited funds. Emits `DepositFor` with `from` as
  a topic so the payer is recoverable from events alone. Closes #29.
- **Referral share for affiliates (#31).** Plans take `referral_bps`, a share
  of each charge paid to the referrer a subscriber names, capped at
  `MAX_REFERRAL_BPS` (2000 = 20%) with the new error `ReferralBpsTooHigh`
  (12). `subscribe` takes `referrer: Option<Address>`; the referrer and its
  rate are frozen into the mandate like the fee and the keeper tip. In
  `charge`, the referral comes out of the merchant's share after the protocol
  fee, so the subscriber still pays exactly the plan price; the share stays
  with the merchant when no referrer was named. `Charged` gains `referral` and
  `referrer`, `Subscribed` gains `referrer` and `referral_bps`, and `Plan`
  gains `referral_bps`.
- **Separate payout address (#32).** `create_plan` takes
  `payout: Option<Address>` (defaults to the merchant): charges pay the payout
  while plan management stays with the merchant. The payout is frozen into
  each mandate and `PlanCreated` carries it.
- **Two-step plan ownership transfer (#33).**
  `propose_plan_owner(merchant, plan_id, new_owner)` /
  `accept_plan_owner(new_owner, plan_id)`: a merchant proposes, only the
  proposed address can accept, and the plan moves between both merchants'
  plan indexes (new errors `PendingOwnershipNotFound` (13) and
  `NotPendingOwner` (14)). Emits `PlanOwnershipProposed` and `PlanTransferred`.
  Mandates already open keep paying the payout address frozen at subscribe
  time; a transfer only moves who manages the plan and which merchant new
  mandates are attributed to.

Interface changes (`set_profile`, `profile`, `deactivate_page`,
`ProfileUpdated`, `create_plan`'s `keeper_bps`, `charge`'s `keeper` argument and
the new `tip`/`keeper`/`keeper_bps` event fields) ripple into
`payflow-backend`, `payflow-frontend` and `payflow-docs`: follow-up issues to
consume the new fields and pass the keeper argument are tracked in the pull
request.

The same ripple applies to this release's new interface: `create_plan`'s
`referral_bps`/`payout` arguments, `subscribe`'s `referrer` argument, the new
`deposit_for` entry point, the `payout`/`referrer`/`referral_bps` fields on
`Plan`/`Mandate`, the `referral`/`referrer`/`payout` event fields and the
`DepositFor`, `PlanOwnershipProposed` and `PlanTransferred` events (issues #29,
#31, #32, #33).

### Changed

- **Breaking:** `charge` takes the settling `keeper: Address` and requires its
  authorization; it no longer settles without a signature. Any address may
  still act as keeper.
- **Breaking:** `plan-registry:create_plan` takes `keeper_bps` as its last
  argument, and `Plan`, `Mandate`, `PlanCreated`, `Subscribed` and `Charged`
  gain fields. Existing testnet deployments must be redeployed: stored `Plan`
  and `Mandate` values do not decode into the new shape.
- **Breaking:** `plan-registry:create_plan` additionally takes `referral_bps`
  and `payout: Option<Address>` (after `keeper_bps`), and
  `subscription:subscribe` takes `referrer: Option<Address>` as its last
  argument. `Plan` gains `payout` and `referral_bps`; `Mandate` gains `payout`,
  `referrer` and `referral_bps`. `charge` now pays `mandate.payout` instead of
  `mandate.merchant` and may pay a third debit to the referrer. Deployments
  built before these fields existed must be redeployed: stored `Plan` and
  `Mandate` values do not decode into the new shape.
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
- 90 → 119 tests. New coverage: `deposit_for` credits, payer-only auth,
  recipient-only withdrawal and its event (#29); the referral split's integer
  bps rounding bound of one stroop, the zero-bps disable, the no-referrer case
  and the `Charged.referral` event field (#31); charges landing on `payout`
  with management still under merchant auth (#32); and both merchants' plan
  indexes updating on transfer, stranger-cannot-accept, one-shot proposals and
  open mandates continuing to pay their frozen payout (#33).

## [0.1.0] - 2026-09-02

First testnet deployment: plan registry, vault and subscription contracts with
permissionless `charge`, spend caps, pause and cancel, and a capped protocol
fee.

[0.2.0]: https://github.com/MMM-pay/payflow-contract/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/MMM-pay/payflow-contract/releases/tag/v0.1.0
