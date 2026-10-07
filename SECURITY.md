# Security Policy

## Audit status

**These contracts are unaudited.** They are deployed to Stellar testnet only.
Do not use them with real funds.

## Scope

In scope:

- `contracts/plan-registry` — plan ownership and mutation
- `contracts/vault` — custody, deposit/withdraw/debit accounting
- `contracts/subscription` — mandate state machine, scheduling, fee math
- The deploy and wiring scripts in `scripts/`

Out of scope:

- The `payflow-backend` keeper and indexer (report in that repository)
- The `payflow-frontend` application (report in that repository)
- Testnet availability, RPC provider behaviour, faucet issues

## Reporting a vulnerability

Do **not** open a public issue for a security bug.

Use GitHub's private vulnerability reporting on this repository
(Security → Report a vulnerability), or email the maintainer listed in the
README.

Please include: affected contract, the invariant broken, a reproduction (a
failing `cargo test` case is ideal), and the impact.

We aim to acknowledge within 72 hours.

## Known limitations

These are understood tradeoffs, not undiscovered bugs:

- **Admin trust.** The subscription admin can change the protocol fee within
  the 10% `MAX_FEE_BPS` ceiling, but a change only applies to mandates opened
  after it. Every mandate stores the `fee_bps` it was created with and settles
  at that rate for life, so an admin cannot reprice an existing subscriber and
  no timelock is needed to make that safe.
- **Vault admin.** The vault admin can repoint `set_subscription` to a different
  contract. A malicious admin could point it at a contract that drains balances.
  Production deployment must place both admin keys behind a multisig.
- **No reentrancy guard.** The contracts call out to a SEP-41 token during
  `debit`. State is written before the transfer in every path, so a malicious
  token cannot observe stale balances, but a hostile token contract can still
  fail or grief a charge.
- **Fee rounding.** Integer basis-point math rounds the fee down, favouring the
  merchant. For a plan priced below 100 stroops with a 1% fee, the fee rounds to
  zero.
- **Timestamp dependence.** Scheduling uses ledger timestamps, which validators
  may skew by a small margin. Periods are bounded below at 60 seconds
  (`MIN_PERIOD`) so skew cannot meaningfully accelerate billing.
