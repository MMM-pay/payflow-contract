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
plan of the same merchant that accepts the mandate's frozen token.

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

- **The plan must be active, accept the mandate's frozen token, and belong to
  the same merchant.** The token is selected by the subscriber when the mandate
  is opened; a merchant cannot change it later. The merchant decides who is
  paid and who may refund, so moving between merchants is a cancel and a new
  subscription, not an upgrade.

## Worked Examples

All amounts are in the token's smallest unit (e.g. stroops for XLM). The
protocol fee and keeper tip are omitted for clarity; they are applied on top
of the `charged` amount exactly as they would be for a regular charge.

### Example 1: Mid-period upgrade, monthly plans

- Old plan: 10,000 units / 30 days (2,592,000 seconds)
- New plan: 20,000 units / 30 days (same period)
- Subscriber upgrades 15 days into the period
- `now` = 1,000,000
- `next_charge` = 1,000,000 + 2,592,000 = 3,592,000
- `period` = 2,592,000
- `remaining` = min(3,592,000 - 1,000,000, 2,592,000) = 2,592,000 - wait, that's the full period

Actually, let's use concrete timestamps:
- `created_at` = 1,000,000
- `period` = 2,592,000 (30 days)
- `next_charge` = 3,592,000 (created_at + period)
- `now` = 2,296,000 (15 days after created_at, halfway through the period)
- `remaining` = min(3,592,000 - 2,296,000, 2,592,000) = 1,296,000 (15 days left)
- `difference` = 20,000 - 10,000 = 10,000
- `charged` = 10,000 * 1,296,000 / 2,592,000 = 5,000

The subscriber pays 5,000 units now (half the price difference for half the period
remaining). The next charge at 3,592,000 will be 20,000 units.

### Example 2: Upgrade near the end of the period

- Same plans as Example 1
- Subscriber upgrades 29 days into the 30-day period
- `now` = 3,592,000 - 86,400 = 3,505,600 (1 day left)
- `remaining` = min(3,592,000 - 3,505,600, 2,592,000) = 86,400
- `charged` = 10,000 * 86,400 / 2,592,000 = 333 (truncated)

The subscriber pays only 333 units for the last day at the higher rate.

### Example 3: Upgrade to a plan with a different period

- Old plan: 10,000 units / 7 days (604,800 seconds, weekly)
- New plan: 50,000 units / 30 days (2,592,000 seconds, monthly)
- Subscriber upgrades 3 days into the weekly period
- `now` = 1,000,000 + 259,200 = 1,259,200
- `next_charge` = 1,000,000 + 604,800 = 1,604,800
- `old_period` = 604,800
- `remaining` = min(1,604,800 - 1,259,200, 604,800) = 345,600 (4 days left in weekly cycle)
- `difference` = 50,000 - 10,000 = 40,000
- `charged` = 40,000 * 345,600 / 604,800 = 22,857 (truncated)

The subscriber pays 22,857 units now for the 4 days remaining in the weekly cycle
at the monthly-equivalent rate. The next charge at 1,604,800 will be 50,000 units
and will start the new monthly billing cycle.

### Example 4: Downgrade mid-period

- Old plan: 20,000 units / 30 days
- New plan: 10,000 units / 30 days
- Subscriber downgrades 15 days into the period
- `remaining` = 1,296,000 (same as Example 1)
- `difference` = 10,000 - 20,000 = -10,000 (negative)
- `charged` = 0 (negative difference collects nothing)

The subscriber pays nothing now. The next charge will be 10,000 units. No refund
is issued for the 10,000 units already paid for the first half of the period at
the higher rate.

### Example 5: Upgrade that prorates to less than one unit

- Old plan: 10,000 units / 30 days
- New plan: 10,001 units / 30 days (1 unit more)
- Subscriber upgrades with 1 second left in the period
- `remaining` = 1
- `difference` = 1
- `charged` = 1 * 1 / 2,592,000 = 0 (truncated to zero)

No amount is collected. The 1 unit difference only applies from the next charge.

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
