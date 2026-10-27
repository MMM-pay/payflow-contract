# ADR 0006: Prorated upgrade charging

## Status

Accepted

## Context

A subscriber who opens a mandate against a plan is sold a fixed price for a
fixed period. Plans change: a merchant publishes a cheaper plan, a more
expensive one, or one on a different billing period, and the subscriber wants
to move.

Two things are wrong with simply swapping the mandate's price:

1. **The subscriber would pay for a period they never got.** Moving to a more
   expensive plan mid-period and charging the new price at the next charge
   bills the whole new price for a period that is half gone.
2. **The subscriber would be overcharged for the part already paid.** Charging
   the full new price from the next period on is right for a downgrade, but
   for an upgrade the subscriber has already paid the old price for the part
   of the period that is still ahead, and that money is not un-paid.

The protocol also has no floats: every amount in the contracts is an integer
in the token's smallest unit, so the fraction of a period has to be an
integer.

## Decision

`upgrade(subscriber, mandate_id, new_plan_id)` moves a mandate onto another
plan of the same merchant, denominated in the same token.

- **The amount collected now is the price difference for the part of the
  current period that is still ahead:**

  ```text
  remaining = min(next_charge - now, period)      // seconds left in this cycle
  charged  = (new_amount - old_amount) * remaining / period
  ```

  Integer division truncates towards zero, so the subscriber never pays more
  than the exact difference for the time that is left: the rounding is in the
  subscriber's favour, and the contract holds no fractional value. A
  difference that prorates to less than one unit collects nothing.

- **A downgrade collects nothing and refunds nothing.** The difference is
  negative, so no amount is charged, and the subscriber is never refunded for
  the part of the period already paid for. The cheaper price applies from the
  next charge.

- **The billing date does not move.** `next_charge` is left alone, so the
  next charge still falls due when the current period ends, and it carries
  the new price from then on. The new plan's period applies to the interval
  that starts there.

- **The mandate's frozen guarantees stay frozen.** The protocol fee
  (`fee_bps`) is the one the mandate was opened with, and the keeper tip of
  the new plan is clamped to `MAX_KEEPER_BPS`. The tip is not paid on the
  upgrade itself: no charge was settled, so nobody earned it.

- **The plan must be active, in the same token, and from the same merchant.**
  A mandate's token is frozen when it is opened, and its merchant decides who
  is paid and who may refund; moving between merchants is a cancel and a new
  subscription, not an upgrade.

## Consequences

- The subscriber is never charged for a period they have not had, and never
  charged twice for the part of a period they have already paid for.
- Rounding always favours the subscriber, and the contract never holds a
  fraction of a unit.
- The old price survives in the mandate's history only through the charge
  events; the mandate itself carries the new terms.
- An upgrade does not create a backlog: the next charge still happens at
  `next_charge`, and `charge` then schedules the following one
  `now + period` as it always does.
- The prorated amount is not part of the refund ceiling
  (`merchant_received`), which covers settled charges. A merchant that wants
  to refund an upgrade has to transfer the money directly.
