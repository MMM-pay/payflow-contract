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

## Externally visible behaviour changes

### Due-date overflow now reports `ScheduleOverflow` (#290)

The period and due-date arithmetic in `subscription` was reviewed for overflow
at large periods and ledger timestamps. All schedule math was already checked
or saturating except where noted here:

- `charge` computes the next due date as `now + period` with checked math. When
  that would exceed `u64::MAX` it now fails with `ScheduleOverflow` (2018)
  instead of `AmountOverflow` (2012). No state is written either way; only the
  error code a client sees changes. Keepers matching on `AmountOverflow` for
  this case should match `ScheduleOverflow` too.
- The `NextMandateId` increment uses checked math and the per-plan open-seat
  and index counters saturate rather than panic.

Regression tests: `charge_reports_schedule_overflow_when_next_due_date_exceeds_u64`,
`charge_succeeds_at_the_last_representable_due_date` and
`huge_period_plan_overflows_on_charge_not_on_subscribe`.

### Vault bounds deposit/withdraw cycling (#314)

`vault:deposit`, `deposit_for`, `withdraw`, `withdraw_to` and `withdraw_all` now share a
fixed 60-second window per `(user, token)`. At most 8 operations are accepted
inside one window; a ninth fails with `RateLimitExceeded` before any balance is
written or any token moves. Windows are aligned to the Unix epoch and reset
deterministically, so a caller cannot keep extending one. The limit is per user
and token: other users, and the same user's other tokens, are unaffected.
`debit` and the mandate-aware debit paths used by charges are not counted, so
billing is unaffected.

This is a hardening change: an address that cycled tiny deposits and
withdrawals could otherwise rewrite the balance entry and emit events
indefinitely. Normal subscribers can still deposit and withdraw several times
a minute. A wallet that hits the limit should wait for the next window.

### Withdrawals and payouts reject the zero address (#236)

`vault:withdraw_to` now rejects `to` when it is the all-zero account or
contract address, with the new typed error `ZeroAddress` (3017). `withdraw` and
`withdraw_all` apply the same check to `user`, and every debit path (`debit`,
`debit_split`, `debit_multi_mandate`) rejects zero-recipient legs, so a plan's
`payout`, the fee recipient, a keeper tip or a referral share cannot be sent to
an unset address. The check runs before any balance write, token transfer or
event, so a rejected call leaves the ledger untouched and consumes no
deposit/withdraw window budget.

This is a behaviour change for clients: a withdrawal to the zero strkey that
used to succeed (and burn the tokens) now fails with 3017. A charge whose
mandate carries a zero payout fails at settlement with
`subscription:VaultCallFailed` (2037), because the vault rejects the leg; no
funds move and the mandate stays chargeable once the payout is corrected.

### Refund cap now accounts for referral fees and keeper tips (PR #301)

Previously, the refund cap (`merchant_received`) only subtracted the protocol fee
and keeper tip from the per-charge amount. It did not account for referral fees,
which are also paid out of the merchant's share. This meant a merchant with a
referral-enabled mandate could refund more than it actually received — up to the
(pre-referral) merchant gross per charge.

The fix updates `merchant_received` to also subtract the referral share (when a
referrer is set) before multiplying by `charges_made`. The refund cap is now:
`(amount - fee - tip - referral) * charges_made`

This is a hardening change: it reduces the maximum refundable amount for
mandates with referrers or keeper tips. No mandate can now refund more than the
merchant actually collected. Existing mandates without referrers or tips are
unaffected.
