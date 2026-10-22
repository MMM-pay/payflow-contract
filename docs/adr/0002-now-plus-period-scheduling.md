# ADR 0002: `now + period` scheduling

## Status

Accepted

## Context

When a keeper is offline for several periods, a mandate misses its due time.
There are two ways to reschedule:

1. Catch-up: `next_charge += period` — every missed period is billed in rapid
   succession once the keeper returns, a backlog that can drain a vault in one
   burst.
2. Forward: `next_charge = now + period` — only one charge is collected, at
   the current time plus one period. Billing is delayed, never accumulated.

## Decision

`charge` sets `next_charge = now + mandate.period`. A keeper outage delays
billing; it never creates a backlog. The trade-off is slow forward drift,
which favours the subscriber.

## Consequences

- Subscribers are protected from burst drains; merchants may collect slightly
  less during outages.
- A late settlement lands exactly once (`keeper_outage_does_not_create_a_chargeable_backlog`).
- `is_due` and `charge` agree on timing, so callers can preview the next
  charge reliably.
