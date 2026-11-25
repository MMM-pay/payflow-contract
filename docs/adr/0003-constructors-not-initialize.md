# ADR 0003: Constructors instead of `initialize`

## Status

Accepted

## Context

The original contracts were deployed and then configured by a separate
`initialize` call. Anyone watching the ledger could call `initialize` between
the deploy transaction and the owner's own call and claim admin rights.

## Decision

Initial configuration (admin and fee) is passed to a `__constructor` that
runs in the same transaction that deploys the contract. Cross-contract
addresses that depend on deployment order are wired by admin-only setters
after all contracts exist. There is no public `initialize`.

## Consequences

- No window for a third party to claim the contract.
- Breaking change: `initialize` was removed in v0.2.0.
- Deploys are ordered: registry, then vault, then subscription (constructed
  with the registry and vault addresses). The vault grants the subscription
  contract debit rights, and the registry records the subscription address for
  plan-token validation (`scripts/deploy.sh`).
