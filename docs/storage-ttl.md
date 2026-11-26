# Storage TTL and Archival

Every ledger entry a Soroban contract writes has a time to live (TTL), measured
in ledgers. Payflow keeps all of its state in **instance** and **persistent**
storage; no entry is `temporary`. When a TTL reaches zero the entry is
**archived**, not lost: the data stays in the network's archive, is no longer
part of live state, and is restored — usually automatically — the next time a
transaction touches it. Archival never cancels a mandate, moves funds, changes a
status or emits an event.

This page explains which entries can expire, how the contracts extend them, and
what a merchant, subscriber or integration must do to keep a mandate alive. The
decision behind the read-side bumps is recorded in [ADR 0007](adr/0007-ttl-refresh-on-access.md);
the reserve-specific case is in [Vault Reserve Lifecycle](vault-reserves.md#on-chain-ttl-expiry-of-a-mandate).

## TTL budgets

All three contracts ([`plan-registry`](../contracts/plan-registry/src/lib.rs),
[`subscription`](../contracts/subscription/src/lib.rs),
[`vault`](../contracts/vault/src/lib.rs)) use the same constants:

| Constant | Ledgers | Wall-clock | Used by |
|---|---|---|---|
| `DAY_IN_LEDGERS` | `17_280` | 1 day (assumes ~5s ledgers) | Unit for the four constants below |
| `INSTANCE_BUMP` | `518_400` | 30 days | Instance entries |
| `INSTANCE_THRESHOLD` | `501_120` | 29 days | Instance entries |
| `PERSIST_BUMP` | `1_555_200` | 90 days | Persistent entries |
| `PERSIST_THRESHOLD` | `1_537_920` | 89 days | Persistent entries |

Every extension is `extend_ttl(&key, THRESHOLD, BUMP)`, which is conditional:
while the entry has at least `THRESHOLD` ledgers left, the call is a no-op;
once it drops below that, the TTL is reset to `BUMP` ledgers from the current
ledger. In practice an entry touched at least once every 89 days (persistent)
or 29 days (instance) stays live, and repeated calls cannot stack TTLs beyond
90 or 30 days from the most recent bump.

The day of difference between each threshold and its bump is deliberate: it
gives a transaction room to read an entry, spend the ledgers, and still write it
back without racing expiry.

## What lives where

### plan-registry

| Storage | Keys | TTL |
|---|---|---|
| Instance | `Admin`, `PendingAdmin`, `NextPlanId` | 30 days |
| Persistent | `Plan(id)`, `MerchantPlanCount(merchant)`, `MerchantPlan(merchant, i)`, `PlanIndex(id)`, `TokenPlanCount(token)`, `TokenPlan(token, i)`, `Profile(merchant)`, `PlanChargesPaused(id)`, `PlanGracePeriod(id)`, `PlanOwnerProposal(id)`, `UpgradeHistory` | 90 days |

### subscription

| Storage | Keys | TTL |
|---|---|---|
| Instance | `Admin`, `PendingAdmin`, `PlanRegistry`, `Vault`, `FeeBps`, `FeeTo`, `NextMandateId`, `ProtocolPaused` | 30 days |
| Persistent | `Mandate(id)`, `MandateTombstone(id)`, `SubscriberMandateCount(subscriber)` / `SubscriberMandate(subscriber, i)`, `MerchantMandateCount(merchant)` / `MerchantMandate(merchant, i)`, `PlanMandateCount(plan)` / `PlanMandate(plan, i)`, `PlanSubscriberCount(plan)`, `PlanCommittedRevenue(plan)`, `FirstFailedCharge(id)`, `MandateGracePeriod(id)`, `MandateRetryWindow(id)`, `PendingMandateOwner(id)`, `PendingSubscriberPosition(id)`, `UpgradeHistory` | 90 days |

### vault

| Storage | Keys | TTL |
|---|---|---|
| Instance | `Admin`, `ReserveCapBps`, `Subscription`, `AllowlistEnabled` | 30 days |
| Persistent | `Balance(user, token)`, `TokenAllowed(token)`, `DepositCap(user, token)`, `SpendLimit(user, token)`, `SpendWindow(user, token)`, `Reserved(user, token, mandate_id)`, `ReservedTotal(user, token)`, `OperationWindow(user, token)`, `UpgradeHistory` | 90 days |

`MandateTombstone` is what keeps a deleted mandate's id from being reused, so
its TTL matters even after the mandate itself is gone: `delete_mandate` writes
it with a full 90-day bump.

## How the TTL gets extended

**1. On every write.** Each `set` is followed by an extension of the same key,
so merchants editing a plan, subscribers pausing a mandate, deposits,
withdrawals, charges and admin changes all refresh what they touch. Writes that
update the instance configuration (`NextPlanId`, `FeeBps`, `ProtocolPaused`,
admin handovers, wiring) also bump the instance TTL.

**2. On reads of hot keys.** A plan is read on every `subscribe` and `charge`
but may never be edited again; a mandate can be polled, quoted or flagged as
failed without being rewritten. Those keys are hot by traffic and cold by
writes, so the shared read helpers extend them as part of the read:

- `plan-registry::read_plan` for a plan — used by `get_plan`,
  `plan_charge_terms`, and every mutator.
- `subscription::read_mandate_opt` for a mandate — used by `get_mandate`,
  `quote_charge`, `is_due`, `due_mandates` and every mutator.
- `vault::read_reserved` / `read_reserved_total` for reserve entries, on every
  deposit, withdrawal, reserve update and debit path.

Because the bump honours the threshold, a healthy entry keeps its ledger
snapshot unchanged; only an entry within a day of expiry is refreshed. Views
are still side-effect free for a client: the extension is host bookkeeping,
and a simulation that only reads the result discards it. **A refresh only
becomes part of live state when the transaction carrying it is submitted and
applied.**

The read helpers bump the entry they return, not indexes or side keys. A
mandate's grace period, retry window or failed-charge marker is refreshed by
whichever path writes it.

**3. The permissionless keeper endpoint.**
`subscription::extend_merchant_mandates_ttl(merchant, start, limit)` bumps the
merchant's mandate count and, for up to `MAX_PAGE` = 50 positions per call, the
index slot and the mandate itself. It skips any mandate that is not `Active` or
`Paused` and returns how many mandates it extended. It needs no auth, is safe to
call repeatedly, and is the tool of choice for **cold-but-open** mandates that
nothing reads: a merchant pages through its index with `merchant_mandates` and
calls this once per page.

There is no equivalent batch endpoint for vault reserves or plan-registry
entries. Plans are refreshed by being used; reserves by being read or debited.

## What callers must do to keep a mandate alive

- **Keepers and merchants settling charges:** nothing extra. `charge` loads the
  mandate through `read_mandate_opt`, so every settlement refreshes it. A
  mandate that is charged at least once every 89 days never needs attention.
- **Open mandates that nobody charges** (indefinitely paused, waiting on a
  merchant-side pause, or scheduled far in the future): the merchant should call
  `extend_merchant_mandates_ttl` for the relevant page at least once every 89
  days. There is no charge traffic to keep them warm and no event warns that
  they are close to expiry.
- **Wallet and dashboard reads:** calling `get_mandate` / `get_plan` from an RPC
  simulation does not keep anything alive. If a view is the only activity on an
  entry, submit the refresh (for example via the keeper endpoint) rather than
  relying on the view.
- **Subscribers:** no action is needed while charges settle. Reserves are
  refreshed by the subscriber's own reserve reads and withdrawals and by the
  mandate's debits. Releasing a reserve that is no longer needed
  (`vault.set_reserve(..., 0)`) is still the right cleanup: it removes the entry
  instead of keeping it on rent.
- **Integrations that build transactions by hand:** simulate first and submit
  the resulting footprint. Simulation is what adds an archived entry (or an
  archived contract instance) to the transaction's restore list.

## What happens when an entry expires

| Entry | TTL reaches 0 | After restoration |
|---|---|---|
| Persistent (mandates, plans, balances, reserves, indexes, history) | Archived. Not in live state; a footprint that does not include its restore entry fails to load it. | Data is intact and the entry works exactly as before. It comes back at the minimum persistent TTL, so call an extending entry point to get a full 90 days. |
| Instance (contract configuration) | The contract instance is archived, so the contract cannot run until it is restored. | Same configuration; the next invocation restores it, and the next admin/config write bumps it back to 30 days. |
| Temporary | Deleted permanently. | Not applicable: these contracts use no temporary entries. |

Restoration is automatic since Protocol 23: when a transaction touches an
archived persistent or instance entry, the network restores it before the
contract runs, charging rent and resource fees. Clients get this for free when
they simulate and submit the simulated transaction; a hand-built transaction
that omits the restore list fails instead. `RestoreFootprintOp` / `stellar
contract restore` remains available for the rare case where nothing can be
invoked first.

Archiving is not deletion, and Payflow never deletes live state implicitly. An
entry only leaves storage when a contract call removes it on purpose —
`delete_mandate` (replaced by a tombstone), `set_reserve(..., 0)`, or a counter
that is no longer needed.

To check an entry's remaining life, read its `liveUntilLedgerSeq` from the RPC
`getLedgerEntries` response and compare it with the current ledger; the
`extend_ttl` calls above run against that same value.

## Related reading

- [ADR 0007: TTL refresh on access for hot keys](adr/0007-ttl-refresh-on-access.md)
- [Vault Reserve Lifecycle: on-chain TTL expiry of a mandate](vault-reserves.md#on-chain-ttl-expiry-of-a-mandate)
- [Stellar docs: State Archival](https://developers.stellar.org/docs/learn/fundamentals/contract-development/storage/state-archival)
