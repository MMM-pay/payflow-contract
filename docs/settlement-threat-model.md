# Settlement threat model

This note describes the trust assumptions and principal attack surface for
subscription settlement: scheduled and one-off charges, vault debits, and
merchant refunds. It is an engineering guide, not an audit or a guarantee that
the contracts are safe for real funds. The contracts are unaudited and
testnet-only; see [SECURITY.md](../SECURITY.md).

## Assets and authority

- Subscriber tokens are held by `vault`, internally accounted per
  `(subscriber, token)`. `subscription` stores mandates and requests debits;
  it does not custody balances.
- A mandate freezes its subscriber, merchant, payout, token, recurring amount,
  period, fee rates and spend limits. The registry's plan-pause flag and the
  protocol-wide pause can stop settlement, but do not redirect or reprice an
  existing mandate.
- The vault trusts the address configured by its admin as `subscription`.
  That address can spend user balances subject to the balances, reserves and
  spend limits in the vault. The vault admin can replace that trusted address,
  making the admin key a custody-critical authority. Production deployments
  should secure admin keys with a multisig.
- The SEP-41 token implementation is part of the trust boundary. The contracts
  check exact recipient balance changes for transfers that must be exact, but
  cannot make a hostile or unavailable token behave honestly.

## Charge and debit

For a recurring charge, `subscription::charge` checks the mandate's lifecycle,
due time, charge count and lifetime spend cap, then reads the plan's pause
state. The total debit is the mandate's frozen recurring amount. Fees, keeper
tips and referrals are split from that amount; the tip and referral reduce the
merchant's share and do not increase the subscriber's debit. `next_charge`
advances to `now + period`, so a delayed keeper does not produce a burst of
missed charges.

The subscriber can authorize one separate extra charge with
`set_extra_charge_cap`. `charge_extra` is permissionless after that
authorization: any keeper can choose an amount up to the cap and trigger it at
any time the mandate and plan are chargeable. The debit goes only to the
mandate's frozen payout and configured fee/tip/referral recipients. A
successful extra charge consumes the authorization, counts toward
`max_charges` and `max_total`, and does not advance the recurring due date.
Because settlement is permissionless, a subscriber who authorizes a cap trusts
that any caller may use it; the contract does not verify metered usage or wait
for a merchant's separate approval.

Both charge paths make one mandate-aware call to
`vault::debit_multi_mandate`. The vault:

1. requires authorization from the configured subscription contract;
2. validates and sums all payment legs, then checks the subscriber's spendable
   balance, the mandate's own reserve and the subscriber's spend limit;
3. updates internal balances and transfers each non-zero leg to its fixed
   recipient; and
4. verifies each token recipient received exactly the amount recorded.

Other mandates cannot spend this mandate's reserve, and ordinary withdrawals
cannot spend reserved funds. A charge fails rather than partially settling if
the vault rejects the debit or a token transfer fails. Soroban transaction
atomicity rolls back the mandate, vault, token and event changes together.
Underfunding therefore delays settlement; it does not authorize the keeper to
take funds from the subscriber's wallet.

### Adversaries and mitigations

| Actor or failure | Boundary and remaining exposure |
| --- | --- |
| Keeper or front-runner | Cannot change a recurring amount or payout. A competing authorized keeper can win the bounded merchant-funded tip. For an authorized one-off cap, any caller may choose an amount up to the cap and consume it first. |
| Merchant | Cannot raise a recurring price or redirect an existing payout. It may refund only from its own wallet, and total recorded refunds cannot exceed merchant proceeds actually received. A merchant cannot reverse a subscriber's mandate cancellation. |
| Underfunded subscriber | A debit fails atomically. The keeper may flag a failed recurring charge, but cannot pull a wallet balance into the vault. |
| Malicious or failing token | A token can reject transfers and block settlement. Exact-balance checks reject fee-on-transfer behavior on guarded paths; token code remains an external dependency. |
| Compromised vault admin | Can change the trusted subscription address and thereby bypass the intended billing boundary. Protect this authority with a multisig and verify deployed configuration. |

## Refund

`subscription::refund` requires the mandate's merchant to authorize the call.
It transfers tokens directly from the merchant's wallet to the subscriber's
wallet; it does not credit the vault or restore vault funds. Refund accounting
is capped at the merchant's accumulated net receipts after protocol fee,
keeper tip and referral shares, including any settled one-off charge. Exact
recipient balance checks reject a refund token transfer that delivers a
different amount. The state update and `Refunded` event are in the same atomic
transaction as the transfer.

The merchant is trusted to decide whether and when to refund. The contract
does not adjudicate service disputes, and a refund does not reopen, resume or
otherwise change a mandate's billing authorization.

## Review checklist

When changing settlement code, verify that:

- every debit is authorized by the configured subscription contract and uses
  the mandate's stored subscriber, token and payout;
- every amount is bounded by the mandate's authorization and lifetime caps;
- split recipients sum to the subscriber's debit without allowing tips or
  referrals to increase it;
- all transfer and state changes roll back together on failure;
- refund limits use actual merchant proceeds, not the gross subscriber debit;
- new events, errors, tests, snapshots and generated bindings stay in sync.
