# ADR 0005: Permissionless `charge`

## Context

A mandate must settle on schedule even if the project's keeper goes offline.

## Decision

`charge` takes no authorization: the mandate itself is the authorization.
Anyone — the merchant, the subscriber, a keeper, or a stranger — may settle
a due charge.

## Consequences

- Billing does not depend on any single operator staying online.
- The charge amount is bounded by the mandate: price, period, fee and
  `max_charges` are frozen when the subscriber opens it, so no caller can
  take more than authorized.
- There is no access control to audit on this path; safety rests on the
  mandate limits and the vault's balance checks.
