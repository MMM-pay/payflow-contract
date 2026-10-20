# ADR 0001: Vault separate from billing

## Status

Accepted

## Context

Payflow is a pull-payment system: a subscriber authorizes a merchant to debit
their funds on a schedule. If the billing contract also holds subscriber
funds, custody and billing are mixed:

- The billing contract becomes a honeypot: every mandate settlement touches
  custody logic.
- Custody cannot be audited independently of billing, and vice versa.
- A bug in scheduling could freeze or leak funds; a bug in custody could
  break every merchant's billing at once.

## Decision

Subscriber funds live in a separate `vault` contract, held per
(user, token). The `subscription` contract may only `debit` vault balances,
and only after the vault admin registers it with `set_subscription`. Users
deposit and withdraw at any time; withdrawing starves any mandate, so funds
are never locked.

## Consequences

- Custody logic can be reviewed on its own. The vault admin has no path to
  move user funds except through the registered subscription contract
  (pinned by `admin_has_no_path_to_user_funds`).
- Billing cannot settle against an empty wallet: the subscriber must fund
  ahead, and a mandate fails safe when the vault is underfunded.
- Charge atomicity: if either the merchant share or the protocol fee debit
  fails, the whole transaction reverts, so internal balances and token
  transfers never diverge (pinned by
  `internal_balances_add_up_to_the_vault_holdings`).
- Cross-contract calls add latency and ledger entries to every charge; this
  is the accepted price of auditable custody.
