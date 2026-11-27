# Sequence diagrams: subscribe, charge and cancel

These diagrams show which contract calls which for Payflow's three core flows,
in the order the calls happen. They are drawn from the v0.2.0 sources plus the
protocol pause (#285) and are kept short on purpose: storage writes are grouped
into one step, and only cross-contract calls and events are spelled out. Where a
flow can stop early, the error is named on the arrow so it can be found in
`contracts/subscription/src/error.rs`.

The three contracts:

- **Subscription** (`contracts/subscription`): the billing engine. Owns
  mandates, the per-subscriber, per-merchant and per-plan indexes, and the
  protocol pause.
- **PlanRegistry** (`contracts/plan-registry`): merchants' plans. Subscription
  only ever reads from it, through `PlanRegistryClient` in
  [`interfaces.rs`](../contracts/subscription/src/interfaces.rs).
- **Vault** (`contracts/vault`): subscribers' balances. Only the configured
  Subscription contract may debit it (`require_subscription`), and it is the
  only contract that moves tokens on a charge. See
  [ADR 0001](adr/0001-vault-separate-from-billing.md).

GitHub renders the `mermaid` blocks below. Elsewhere, paste a block into
<https://mermaid.live>.

## Subscribe

`subscribe` is `subscribe_at(..., now)`. `subscribe_with_deposit` adds a vault
deposit before the same core (`do_subscribe_at`), under the subscriber's single
authorization.

```mermaid
sequenceDiagram
    autonumber
    actor S as Subscriber
    participant Sub as Subscription
    participant Reg as PlanRegistry
    participant V as Vault
    participant T as Token

    S->>Sub: subscribe / subscribe_at(plan_id, max_charges, max_total, start, referrer)
    Note over Sub: subscriber.require_auth()
    Sub--xS: ProtocolPaused, if the admin paused the protocol (#285)
    opt subscribe_with_deposit only
        Sub->>Reg: get_plan(plan_id), for the plan's token
        Sub->>V: deposit(subscriber, plan.token, deposit)
        V->>T: transfer(subscriber → vault, deposit)
        Note over Sub,V: a rejected deposit fails the call with VaultCallFailed
    end
    Note over Sub: check max_total ≥ 0 and start in [now, now + 1 year]<br/>(InvalidMaxTotal, InvalidStart)
    Sub->>Reg: get_plan(plan_id)
    Reg-->>Sub: Plan (price, period, token, merchant, keeper/referral bps, seat cap)
    Note over Sub: plan must be active and not full<br/>(PlanNotFound, PlanInactive, PlanFull)
    Sub->>Reg: plan_grace_period(plan_id)
    Reg-->>Sub: grace window, frozen into the mandate
    Note over Sub: write Mandate, MandateGracePeriod (if non-zero),<br/>subscriber / merchant / plan index slots,<br/>open-seat count and committed revenue
    Sub-->>S: mandate id
    Note over Sub: events: Subscribed, MandateUpdated,<br/>CommittedRevenueChanged (when the total moves)
```

No money moves on subscribe (unless `subscribe_with_deposit` deposits). The
first charge is due at `start`.

## Charge

`charge` is permissionless ([ADR 0005](adr/0005-permissionless-charge.md)): any
keeper may call it once a mandate is due. The vault, not the keeper, is the
one that pays every leg.

```mermaid
sequenceDiagram
    autonumber
    actor K as Keeper (anyone)
    participant Sub as Subscription
    participant Reg as PlanRegistry
    participant V as Vault
    participant T as Token

    K->>Sub: charge(keeper, mandate_id)
    Note over Sub: keeper.require_auth()
    Sub--xK: ProtocolPaused, if the admin paused the protocol (#285)
    Note over Sub: read Mandate: must be active, due, under max_charges<br/>and max_total (MandateNotFound, MandateNotActive,<br/>NotDue, MaxChargesReached, AmountOverflow)
    Sub->>Reg: plan_charges_paused(plan_id)
    Reg-->>Sub: bool
    Sub--xK: PlanChargesPaused, if the merchant paused this plan
    Note over Sub: fee_split(mandate) from the rates frozen at subscribe:<br/>fee, keeper tip, referral, merchant net
    Sub->>V: debit_multi_mandate(subscriber, token, mandate_id, legs)
    Note over V: require_subscription(): only this Subscription may debit
    V->>T: transfer(vault → merchant or payout, net)
    V->>T: transfer(vault → fee_to, fee)
    V->>T: transfer(vault → keeper, tip), if non-zero
    V->>T: transfer(vault → referrer, referral), if non-zero
    Note over Sub,V: a vault rejection (insufficient or reserved balance)<br/>fails the call with VaultCallFailed and no state changes
    Note over Sub: advance next_charge by one period, count the charge,<br/>complete the mandate at max_charges / max_total,<br/>clear any flagged failed charge
    Sub-->>K: ChargeResult
    Note over Sub: events: Charged, MandateUpdated,<br/>MandateCompleted (on the last charge), CommittedRevenueChanged
```

If the vault cannot cover a due charge, anyone may call `flag_failed_charge`,
which asks the vault `can_debit` and records the failure without moving money.

## Cancel

Cancelling stays within the Subscription contract: it makes no calls to the
registry or the vault, so it keeps working whatever state those contracts are
in, and while the protocol is paused.

```mermaid
sequenceDiagram
    autonumber
    actor S as Subscriber
    participant Sub as Subscription

    S->>Sub: cancel(subscriber, mandate_id) / cancel_with_reason(..., reason)
    Note over Sub: subscriber.require_auth()<br/>(InvalidCancelReason, MandateNotFound,<br/>NotSubscriber, MandateNotActive)
    Note over Sub: status = Cancelled (irreversible), free the plan seat,<br/>recompute committed revenue
    Sub-->>S: ()
    Note over Sub: events: MandateUpdated, Cancelled (with reason),<br/>CommittedRevenueChanged
```

The subscriber's vault balance is untouched by a cancel. They take it out with
the vault's `withdraw`, `withdraw_to` or `withdraw_all`, which never consult the
Subscription contract or its pause switch.

## During a protocol pause

When the admin calls `set_protocol_paused(true)`:

| Flow | Behaviour |
|---|---|
| `subscribe`, `subscribe_with_id`, `subscribe_at`, `subscribe_with_deposit` | Fail with `ProtocolPaused` before any call to the registry or the vault |
| `charge` | Fails with `ProtocolPaused`; the mandate's `next_charge` is unchanged, so it is chargeable as soon as the protocol resumes |
| `upgrade` | Fails with `ProtocolPaused` |
| `cancel`, `cancel_page`, `end_mandate`, `delete_mandate`, `set_paused`, `pause_until`, `refund` | Unaffected |
| Every view, `extend_merchant_mandates_ttl` | Unaffected |
| Vault `withdraw`, `withdraw_to`, `withdraw_all` | Unaffected: the vault never reads the switch |
