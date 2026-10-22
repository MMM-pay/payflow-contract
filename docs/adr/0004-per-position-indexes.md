# ADR 0004: Per-position indexes

## Context

Merchant plans, merchant mandates and subscriber mandates were each kept as
one growing list that every write rewrote. Anyone could open throwaway
subscriptions against a merchant's plan until the list outgrew Soroban's
ledger entry size limit, after which every new subscription to that merchant
failed.

## Decision

Each list is stored as one ledger entry per position plus a count, and reads
are paged (`start`, `limit`, capped at `MAX_PAGE = 50`). A write touches the
same small entries however many positions exist; indexes are append-only and
positions are never reused.

## Consequences

- A merchant cannot be blocked by throwaway mandates
  (`subscribe_cost_does_not_grow_with_the_merchant_index`).
- Writes are small and bounded; reads touch at most `MAX_PAGE` entries, and a
  transaction may touch at most 100 entries in total.
- Clients need the matching `*_count` (and `next_plan_id`) to enumerate
  lists.
