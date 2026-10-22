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

## Update: keeper tip (issue #44)

A plan may set `keeper_bps`, a share of the merchant's own payment paid to
whoever settles the charge. That makes the tip an incentive for independent
keepers rather than a reason to gate the path, so `charge` stays open to any
caller but now takes the keeper `Address` and requires its authorization, so
the tip is only ever sent to an address that proved it controls the account it
is collecting into. The signature covers the transfer to the caller only; it
grants no control over the mandate, and the subscriber's total debit is
unchanged.
See the front-running analysis in `SECURITY.md`.
