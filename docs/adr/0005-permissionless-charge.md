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

## Keeper responsibilities

Operators running keeper infrastructure should understand the following:

### When to call `charge`

A mandate is chargeable when:
1. The mandate status is `Active` (not `Paused`, `Cancelled`, or `Completed`).
2. The current ledger timestamp has reached or passed `next_charge`.
3. The mandate has not reached its `max_charges` cap (if set).
4. The mandate has not reached its `max_total` spend cap (if set).
5. The plan's merchant has not paused charging via `set_plan_charges_paused`.

Keepers can discover due mandates by calling `due_mandates` (paged by merchant)
or `is_due` / `quote_charge` on specific mandate IDs. `quote_charge` also
returns the exact fee split so a keeper can display the expected payout before
submitting.

### Safety guarantees for keepers

- **No mandate control**: Calling `charge` grants no authority over the mandate.
  The keeper cannot change the amount, redirect the payout, or alter any terms.
- **Fixed debit**: The subscriber is always debited exactly `mandate.amount`.
  The keeper tip and referral are paid from the merchant's share only.
- **Frozen terms**: All rates (fee, keeper tip, referral) are frozen into the
  mandate at subscribe time. A keeper cannot influence them.
- **Single settlement per period**: The first successful `charge` advances
  `next_charge` by a full period. Competing keepers cannot double-charge; the
  second attempt fails with `NotDue`.
- **Tip security**: The keeper must authorize the call, so the tip is only paid
  to an address that proves it controls the receiving account. A front-runner
  can only steal the tip by paying a higher inclusion fee; they cannot redirect
  it to a third party.
- **No replay**: A settled charge updates `charges_made` and `last_charge`. The
  same mandate cannot be charged again until the next period.

### Operational recommendations

- **Monitor `due_mandates`**: Poll the merchant's due mandates regularly
  (e.g., every few ledgers) and submit `charge` for any that are due.
- **Use `quote_charge` first**: Call `quote_charge` to verify the mandate is
  chargeable and to display the expected fee split before submitting a
  transaction.
- **Handle failures gracefully**: If `charge` fails with `VaultCallFailed`,
  the subscriber's vault has insufficient balance. Consider calling
  `flag_failed_charge` to notify the merchant (emits `ChargeFailed` event).
- **Respect pauses**: If `PlanChargesPaused` or `MandateNotActive` is returned,
  do not retry until the pause is lifted.
- **Gas estimation**: The `charge` call makes multiple cross-contract calls
  (vault debits, token transfers). Ensure adequate gas limits.

### Why this is safe

The design follows the principle that the mandate is the sole authorization.
The subscriber signs once at subscribe time, agreeing to the fixed amount,
period, and caps. Thereafter, the mandate executes autonomously. The keeper is
merely the trigger; the contract enforces all bounds. This is analogous to a
standing order in traditional banking: the bank executes the transfer when due,
and the account holder's signature on the mandate is the only authorization
needed.

See also: [`Subscription::charge`](../contracts/subscription/src/lib.rs#L495)
and [`SECURITY.md`](../SECURITY.md#keeper-tips-and-front-running).
