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

- `vault:deposit_for(from, user, token, amount)`: fund another account's vault
  balance. `from` signs and pays; `user` owns the credited balance and is the
  only account that can withdraw or spend it. Emits `DepositFor` with both
  accounts, so indexers can tell a gift from a self-deposit.
- `subscription:subscribe` takes an optional `referrer`. A plan's
  `referral_bps` (capped at `MAX_REFERRAL_BPS`, 1000 = 10%, `0` disables
  referrals) is paid to that referrer out of the merchant's share on every
  charge. The referrer and its rate are frozen into the mandate. `Charged`
  gains `referral`, `referrer` and `payout`; `Subscribed` gains `payout`,
  `referral_bps` and `referrer`. The subscriber's total debit and the protocol
  fee are unchanged.
- `plan-registry:create_plan` takes `referral_bps` and an optional `payout`.
  `payout` is where the merchant's share of each charge is paid and defaults to
  `merchant`, so a treasury or multisig can be paid while `merchant` keeps
  managing plans. Plans carry both in `Plan.payout` / `Plan.referral_bps` and
  in `PlanCreated`. A referral share above the cap fails with
  `ReferralBpsTooHigh` (12).
- `plan-registry:propose_plan_owner(merchant, plan_id, new_owner)` and
  `accept_plan_owner(new_owner, plan_id)`: a two-step handover of a plan to
  another merchant address, emitting `PlanOwnerProposed` and `PlanTransferred`.
  The plan moves between both merchants' plan indexes in constant time, and
  `pending_plan_owner(plan_id)` exposes a pending proposal. A payout that
  defaulted to the old owner follows the plan; an explicitly set payout is
  kept. Mandates already open are untouched and keep paying the addresses
  frozen at subscribe time.
- New plan-registry errors `NotPendingOwner` (13) and `OwnerUnchanged` (14),
  appended after the existing codes.
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

Interface changes (`set_profile`, `profile`, `deactivate_page`,
`ProfileUpdated`, `create_plan`'s `keeper_bps`, `charge`'s `keeper` argument and
the new `tip`/`keeper`/`keeper_bps` event fields) ripple into
`payflow-backend`, `payflow-frontend` and `payflow-docs`: follow-up issues to
consume the new fields and pass the keeper argument are tracked in the pull
request.

### Changed

- **Breaking:** `subscription:subscribe` takes `referrer: Option<Address>` as
  its last argument and the merchant share is paid to `Mandate.payout` rather
  than to `Mandate.merchant`.
- **Breaking:** `plan-registry:create_plan` takes `referral_bps` and
  `payout: Option<Address>` after `keeper_bps`, and the plan type gains a
  `PlanSlot` bookkeeping entry per plan.
- **Breaking:** `Plan` gains `payout` and `referral_bps`; `Mandate` gains
  `payout`, `referral_bps` and `referrer`. Both are shared with the
  subscription contract, so a stored plan or mandate from an earlier
  deployment does not decode and the suite must be redeployed.
- **Breaking:** `charge` takes the settling `keeper: Address` and requires its
  authorization; it no longer settles without a signature. Any address may
  still act as keeper.
- **Breaking:** `plan-registry:create_plan` takes `keeper_bps` as an argument,
  and `Plan`, `Mandate`, `PlanCreated`, `Subscribed` and `Charged` gain
  fields. Existing testnet deployments must be redeployed: stored `Plan` and
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
- **Referral, payout and transfer tests** — 90 → 115 tests. The referral share
  is paid out of the merchant share and never changes what the subscriber
  pays; a plan's `referral_bps` of 0 pays nobody; 99 stroops at 5% loses no
  stroop to rounding; `Charged` reports the referral; the merchant share goes
  to the plan's `payout`; an open mandate keeps its frozen payout after the
  plan is sold and keeps billing the old merchant's payout. In the registry, a
  transfer updates both merchants' indexes (including a swap-with-last slot),
  a stranger cannot accept or propose one, a plan cannot be transferred to its
  current owner, and an explicit payout survives the handover. In the vault,
  `deposit_for` credits the recipient only, and only the recipient can
  withdraw the gift.

The four new interface changes (`deposit_for`/`DepositFor`, `subscribe`'s
`referrer`, `create_plan`'s `referral_bps`/`payout`, and
`propose_plan_owner`/`accept_plan_owner`/`PlanTransferred`) ripple into
`payflow-backend`, `payflow-frontend` and `payflow-docs`: follow-up issues to
render referral earnings, pass the referrer on subscribe, show a plan's payout
address and handle plan transfers are tracked in the pull request.

## [0.1.0] - 2026-09-02

First testnet deployment: plan registry, vault and subscription contracts with
permissionless `charge`, spend caps, pause and cancel, and a capped protocol
fee.

[0.2.0]: https://github.com/MMM-pay/payflow-contract/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/MMM-pay/payflow-contract/releases/tag/v0.1.0
