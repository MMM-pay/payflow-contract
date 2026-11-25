# ADR 0007: TTL refresh on access for hot keys

## Context

Persistent entries live for 90 days (`PERSIST_BUMP`) and are extended on write.
A plan is read on every `subscribe` and `charge` but may never be edited again;
a mandate can be polled, quoted or flagged as failed without being rewritten.
Those keys are hot by traffic but cold by writes, so they could expire while
they are still in active use. The permissionless batch endpoint
(`extend_merchant_mandates_ttl`) helps, but it relies on somebody remembering
to call it.

## Decision

The central read helpers for hot keys extend the TTL as part of the read:

- `plan-registry::read_plan` for `DataKey::Plan`
- `subscription::read_mandate_opt` for `DataKey::Mandate`
- `vault::read_reserved` / `read_reserved_total` for mandate reserves

The bump uses `PERSIST_THRESHOLD` / `PERSIST_BUMP`, so it is a no-op until an
entry is within one day of expiry. Every read path (views and mutators) goes
through the helper, so the behaviour cannot be skipped by a caller or a new
entry point.

## Consequences

- Actively used mandates, plans and reserves stay alive without an operator
  running a TTL keeper. A single access before expiry buys another 90 days.
- Views are still side-effect free for entries with healthy rent: the bump is
  conditional, so their ledger snapshot is unchanged. For an aged entry the
  TTL is refreshed; this is rent housekeeping, not a change to contract data,
  and simulation never commits it.
- Reads now cost a few thousand more CPU instructions, so the committed
  resource snapshots were regenerated. The batch TTL endpoint remains useful
  for cold-but-open mandates that nobody reads at all.
- No contract interface, storage layout or event changes are involved.
