# ADR 0003: Constructors instead of `initialize`

## Status

Accepted

## Context

The original contracts were deployed and then configured by a separate
`initialize` call. Anyone watching the ledger could call `initialize` between
the deploy transaction and the owner's own call and claim admin rights.

## Decision

Configuration (admin, wiring, fee) is passed to a `__constructor` that runs in
the same transaction that deploys the contract. There is no separate
`initialize`.

## Consequences

- No window for a third party to claim the contract.
- Breaking change: `initialize` was removed in v0.2.0.
- Deploys are ordered: registry, then vault, then subscription (constructed
  with the registry and vault addresses), and finally the vault admin grants
  the subscription contract debit rights (`scripts/deploy.sh`).
