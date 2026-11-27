# Event payload audit — #271

The plan-registry, subscription and vault event definitions were reviewed for values that can be reconstructed without changing contract storage, authorization, transfers, or return values.

## Change: subscription committed revenue

`CommittedRevenueChanged` previously carried both `old_committed` and `new_committed`. The previous value is already held by an event-sourcing indexer's last committed-revenue projection for `plan_id`; the first observed transition should be initialized from the indexer's existing projection or a historical replay. The event now emits `plan_id`, `merchant`, `token`, `new_committed` and `period`. The on-chain equality check and persistent storage update are unchanged.

**Footprint note:** Every emitted committed-revenue update now omits one named i128 field from the Soroban event data map. This saves one map entry, its `old_committed` key and its encoded integer value per event. The precise XDR-byte and instruction savings depend on SDK serialization and should be measured in CI; no unverified CPU or byte-count claim is made here.

**Consumer compatibility:** This is an intentional event-schema change, not a change to contract method behavior. Indexers that decode `old_committed` must instead keep the preceding `new_committed` per plan (or initialize from their persisted projection), and regenerate bindings. Historical events retain their original schema. This should be coordinated before deployment.

## Retained fields

- **Plan registry:** `PlanCreated` carries plan metadata for event-only discovery; `PlansCreated.plan_ids` is needed to identify the batch boundary and ordered IDs.
- **Subscription:** `Subscribed` carries frozen mandate terms; `Charged` carries token, settlement and referral amounts; `MandateUpdated` deliberately carries a full snapshot for indexer recovery.
- **Vault:** balance and reserve totals are emitted to avoid extra reads and to preserve the existing event-only projection behavior.

## Verification

Run `cargo fmt --all --check` and `cargo test --workspace` with the repository's supported Rust/Soroban toolchain. The new schema regression test is in `contracts/subscription/src/test_events.rs`. These commands have not been executed in the current environment.
