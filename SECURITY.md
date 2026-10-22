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
- **Keeper tips and front-running.** A plan may pay whoever settles a due
  charge a tip of at most 5% of the merchant's share (`MAX_KEEPER_BPS`), funded
  by the merchant. Settlement is open to any caller, so the first keeper to land
  a valid `charge` is paid, and a copycat that sees a pending settlement can
  submit an identical call and take the tip by paying a higher inclusion fee.
  That competition is the point of the feature, and the exposure is bounded:

  - The tip is a share of what the merchant was going to receive anyway. The
    subscriber is debited `mandate.amount` in total, tip or no tip, and the plan
    price, protocol fee and tip rate are frozen into the mandate at subscribe
    time, so no keeper can reprice a charge it settles.
  - A front-run can only collect the tip once. The first successful charge
    advances `charges_made` and pushes `next_charge` a full period ahead, so the
    copycat's second attempt fails with `NotDue` rather than charging twice.
  - The tip can only go to the caller, and the caller must authorize the call,
    so nobody can redirect a settlement's tip to a third party or collect on
    another keeper's behalf.
  - A losing keeper's transaction reverts at the due-time check; the only loss
    is its own network fee, and the subscriber, merchant and fee recipient are
    unaffected either way.

  In short, the extractable value is at most one capped tip per mandate per
  period, paid by the merchant, and it cannot be repeated, redirected or used
  to change any mandate's terms.
- **Fee rounding.** Integer basis-point math rounds the fee down, favouring the
  merchant. For a plan priced below 100 stroops with a 1% fee, the fee rounds to
  zero. The keeper tip rounds down too, this time in the merchant's favour.
- **Index entries can be archived.** Each position in a merchant's or
  subscriber's index is its own persistent entry with a 90-day TTL that is
  extended when written, not when read. An index position untouched for 90
  days is archived and skipped by the paged getters until it is restored. The
  mandates and plans themselves are unaffected, and the indexer keeps the full
  history.
- **Timestamp dependence.** Scheduling uses ledger timestamps, which validators
  may skew by a small margin. Periods are bounded below at 60 seconds
  (`MIN_PERIOD`) so skew cannot meaningfully accelerate billing.
