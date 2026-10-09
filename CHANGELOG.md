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

## [0.1.0] - 2026-09-02

First testnet deployment: plan registry, vault and subscription contracts with
permissionless `charge`, spend caps, pause and cancel, and a capped protocol
fee.

[0.2.0]: https://github.com/MMM-pay/payflow-contract/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/MMM-pay/payflow-contract/releases/tag/v0.1.0
