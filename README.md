<h1 align="center">Payflow — Contracts</h1>

<p align="center">
  <strong>Pull-based recurring payments for Stellar.</strong><br/>
  Soroban contracts that let a merchant charge a subscriber on a schedule,
  without the subscriber signing every payment.
</p>

<p align="center">
  <a href="https://github.com/MMM-pay/payflow-contract/actions/workflows/ci.yml">
    <img alt="CI" src="https://github.com/MMM-pay/payflow-contract/actions/workflows/ci.yml/badge.svg"/>
  </a>
  <img alt="Rust" src="https://img.shields.io/badge/rust-1.96-orange"/>
  <img alt="soroban-sdk" src="https://img.shields.io/badge/soroban--sdk-26-blue"/>
  <img alt="License" src="https://img.shields.io/badge/license-Apache--2.0-green"/>
  <img alt="Network" src="https://img.shields.io/badge/network-testnet-yellow"/>
</p>

> **Live demo:** https://mmm-pay.github.io/payflow-frontend/ ·
> **Docs:** https://mmm-pay.github.io/payflow-docs/

---

## The problem

Stellar payments are push-only. The account holding the funds must sign every
transfer. That is correct for a one-off payment and useless for a subscription:
a merchant cannot bill a customer monthly without the customer signing a
transaction every month, and no consumer does that.

Every subscription business on Stellar today solves this by holding customer
funds off-chain or by asking for a custodial key. Both re-introduce exactly the
trust the chain was supposed to remove.

Payflow supplies the missing primitive: a **mandate**. A subscriber authorizes
a bounded, revocable standing charge once. After that, anyone can settle the
payment when it comes due — and nobody can take more than the mandate allows.

## How it works

```
  merchant                subscriber                 keeper (anyone)
     │                        │                            │
     │ create_plan            │ deposit                    │
     ▼                        ▼                            │
┌──────────────┐        ┌──────────────┐                   │
│ plan-registry│        │    vault     │                   │
│              │        │ holds funds  │                   │
│ price/period │        └──────┬───────┘                   │
└──────┬───────┘               │ debit (only)              │
       │ get_plan              │                           │
       ▼                       │                           │
┌──────────────────────────────┴───────┐                   │
│           subscription               │◄──── charge ──────┘
│  mandates · schedule · fee split     │
└──────────────────────────────────────┘
```

Three contracts, one direction of dependency:

| Contract | Owns | Depends on |
|---|---|---|
| `plan-registry` | Merchant plans: price, period, token | — |
| `vault` | Custody of subscriber funds | token (SEP-41) |
| `subscription` | Mandates, scheduling, fee split | `plan-registry`, `vault` |

**Why the vault exists.** A pull payment fails the moment a subscriber's wallet
is empty at charge time. Separating custody from billing means subscribers fund
ahead, and the biller debits a balance that is already committed. It also means
custody logic can be audited on its own.

**Why `charge` is permissionless.** The mandate *is* the authorization, so it
does not matter who triggers settlement. If the project's keeper goes offline,
the merchant — or the subscriber, or a stranger — can still settle. Billing does
not depend on us staying online.

## Guarantees for the subscriber

- The price is copied into the mandate at subscribe time. A merchant editing a
  plan later **cannot** reprice an open mandate.
- `max_charges` caps total collections. `0` means open-ended.
- `cancel` is unilateral and immediate; the merchant is not consulted.
- A merchant can end a mandate (`end_mandate`) when it stops offering the
  service. Ending can only stop future charges; it never takes anything.
- Withdrawing from the vault starves any mandate. Funds are never locked.
- A keeper outage delays billing; it never accumulates a backlog that drains a
  vault in one burst. `next_charge` advances to `now + period`, not
  `next_charge + period`.
- The protocol fee is capped at 10% (`MAX_FEE_BPS = 1000`) and is integer basis
  points, frozen into each mandate when it is opened. An admin fee change only
  applies to new mandates. There is no floating point anywhere in the contracts.

## Design notes

**Constructors, not `initialize`.** Every contract receives its admin and
wiring as constructor arguments, so it is configured in the same transaction
that deploys it. A separate `initialize` call leaves a window in which anyone
can call it first and become admin.

**Indexes that cannot be griefed.** The lists of a merchant's plans, a
merchant's mandates and a subscriber's mandates are stored one ledger entry per
position plus a count, not as a single growing `Vec`. Opening a subscription
costs the same whether the merchant has one subscriber or a million, and nobody
can block a merchant by opening throwaway mandates until a list outgrows the
ledger's entry size limit.

**Paged reads.** `merchant_plans`, `merchant_mandates` and `subscriber_mandates`
take `start` and `limit` and return at most 50 ids (`MAX_PAGE`), because one
transaction may touch at most 100 ledger entries. Each has a matching
`*_count` function, and `next_plan_id` tells a client how many plans exist.

## Deployed — Stellar Testnet

| Contract | ID |
|---|---|
| `plan-registry` | [`CDQ5EJTVXXX2CBMHWP3BGGUDHSK6D4I25IF5XXH3QGL2TNTBMJUOFKDB`](https://stellar.expert/explorer/testnet/contract/CDQ5EJTVXXX2CBMHWP3BGGUDHSK6D4I25IF5XXH3QGL2TNTBMJUOFKDB) |
| `vault` | [`CBNQEWSYYQOKNX5KB62OFT6ID5TCVSWN4PS4AXAJK6IWUEXOLVN6DXII`](https://stellar.expert/explorer/testnet/contract/CBNQEWSYYQOKNX5KB62OFT6ID5TCVSWN4PS4AXAJK6IWUEXOLVN6DXII) |
| `subscription` | [`CD5YZUJU6WLFIHGAV4J2BV42GCVZE5ZGDUGSW5CBRIA47TQHSZZW3XMM`](https://stellar.expert/explorer/testnet/contract/CD5YZUJU6WLFIHGAV4J2BV42GCVZE5ZGDUGSW5CBRIA47TQHSZZW3XMM) |

Settlement token: the native XLM Stellar Asset Contract
(`CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC`). The same ids, the admin and the ledger the suite was
deployed at are in [`deployments/testnet.env`](deployments/testnet.env).

## Quick start

```bash
# prerequisites: Rust 1.96+, stellar-cli 27+
rustup target add wasm32v1-none

cargo test --all          # 63 tests
stellar contract build    # wasm -> target/wasm32v1-none/release
```

Deploy your own suite:

```bash
stellar keys generate payflow-deployer --network testnet --fund
./scripts/deploy.sh testnet payflow-deployer
```

The script deploys in dependency order, passing each contract its configuration
as constructor arguments, grants the subscription contract debit rights on the
vault, and writes the ids to `deployments/<network>.env`.

Smoke-test a live deployment end to end:

```bash
./scripts/demo.sh testnet   # reads deployments/testnet.env
```

## Repository layout

```
contracts/
  plan-registry/   merchant plan catalogue
  vault/           subscriber custody
  subscription/    mandates, scheduling, fee split
scripts/
  deploy.sh        ordered deploy + wiring
  demo.sh          end-to-end smoke test
deployments/
  testnet.env      ids of the live testnet suite
```

Each contract follows the same file split: `lib.rs` (entry points), `types.rs`
(storage types), `error.rs` (`contracterror`), `events.rs` (`contractevent`),
`test.rs`.

## Related repositories

| Repo | Role |
|---|---|
| [payflow-contract](https://github.com/MMM-pay/payflow-contract) | Soroban contracts (this repo) |
| [payflow-backend](https://github.com/MMM-pay/payflow-backend) | Event indexer + keeper that settles due mandates |
| [payflow-frontend](https://github.com/MMM-pay/payflow-frontend) | Merchant dashboard and subscriber portal |
| [payflow-docs](https://github.com/MMM-pay/payflow-docs) | Protocol documentation |

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md). Good first issues are labelled
`good-first-issue`. Changes are recorded in [CHANGELOG.md](CHANGELOG.md). Every PR must pass `cargo fmt --check`, `cargo clippy -D
warnings`, and `cargo test --all`.

## Security

Unaudited. Testnet only. Do not use with real funds. See [SECURITY.md](SECURITY.md).

## Maintainers

| Name | Role | Contact |
|---|---|---|
| Victor Adeleke | Lead maintainer | [@titilope12](https://github.com/titilope12) |

## Contributors

<a href="https://github.com/MMM-pay/payflow-contract/graphs/contributors">
  <img src="https://contrib.rocks/image?repo=MMM-pay/payflow-contract"/>
</a>

## License

Apache-2.0. See [LICENSE](LICENSE).
