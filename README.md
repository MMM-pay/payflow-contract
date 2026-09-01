<h1 align="center">Payflow — Contracts</h1>

<p align="center">
  <strong>Pull-based recurring payments for Stellar.</strong><br/>
  Soroban contracts that let a merchant charge a subscriber on a schedule,
  without the subscriber signing every payment.
</p>

<p align="center">
  <a href="https://github.com/payflow-protocol/payflow-contract/actions/workflows/ci.yml">
    <img alt="CI" src="https://github.com/payflow-protocol/payflow-contract/actions/workflows/ci.yml/badge.svg"/>
  </a>
  <img alt="Rust" src="https://img.shields.io/badge/rust-1.96-orange"/>
  <img alt="soroban-sdk" src="https://img.shields.io/badge/soroban--sdk-26-blue"/>
  <img alt="License" src="https://img.shields.io/badge/license-Apache--2.0-green"/>
  <img alt="Network" src="https://img.shields.io/badge/network-testnet-yellow"/>
</p>

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
- Withdrawing from the vault starves any mandate. Funds are never locked.
- A keeper outage delays billing; it never accumulates a backlog that drains a
  vault in one burst. `next_charge` advances to `now + period`, not
  `next_charge + period`.
- The protocol fee is capped at 10% (`MAX_FEE_BPS = 1000`) and is integer basis
  points. There is no floating point anywhere in the contracts.

## Deployed — Stellar Testnet

| Contract | ID |
|---|---|
| `plan-registry` | [`CABL5QWEQKMAIDXTQCHEJ2TAAPYUHSQDY73Z4LIK6BOBGYUYO7RG6HJA`](https://stellar.expert/explorer/testnet/contract/CABL5QWEQKMAIDXTQCHEJ2TAAPYUHSQDY73Z4LIK6BOBGYUYO7RG6HJA) |
| `vault` | [`CBWDD6KVMLEH2D5IVPHZJW4TLLL2RVWGNJRN2US6INNBFTH35K2KYSF7`](https://stellar.expert/explorer/testnet/contract/CBWDD6KVMLEH2D5IVPHZJW4TLLL2RVWGNJRN2US6INNBFTH35K2KYSF7) |
| `subscription` | [`CBKQVZGZJMY47JFRCOF2RFKEMTTYKUF4LI54AOB6EGDFODNZU7MSUW6K`](https://stellar.expert/explorer/testnet/contract/CBKQVZGZJMY47JFRCOF2RFKEMTTYKUF4LI54AOB6EGDFODNZU7MSUW6K) |

## Quick start

```bash
# prerequisites: Rust 1.96+, stellar-cli 27+
rustup target add wasm32v1-none

cargo test --all          # 42 tests
stellar contract build    # wasm -> target/wasm32v1-none/release
```

Deploy your own suite:

```bash
stellar keys generate payflow-deployer --network testnet --fund
./scripts/deploy.sh testnet payflow-deployer
```

The script deploys in dependency order, initializes each contract, grants the
subscription contract debit rights on the vault, and prints a ready-to-paste
env block.

Smoke-test a live deployment end to end:

```bash
REGISTRY=C... VAULT=C... SUBSCRIPTION=C... ./scripts/demo.sh testnet
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
```

Each contract follows the same file split: `lib.rs` (entry points), `types.rs`
(storage types), `error.rs` (`contracterror`), `events.rs` (`contractevent`),
`test.rs`.

## Related repositories

| Repo | Role |
|---|---|
| [payflow-contract](https://github.com/payflow-protocol/payflow-contract) | Soroban contracts (this repo) |
| [payflow-backend](https://github.com/payflow-protocol/payflow-backend) | Event indexer + keeper that settles due mandates |
| [payflow-frontend](https://github.com/payflow-protocol/payflow-frontend) | Merchant dashboard and subscriber portal |

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md). Good first issues are labelled
`good-first-issue`. Every PR must pass `cargo fmt --check`, `cargo clippy -D
warnings`, and `cargo test --all`.

## Security

Unaudited. Testnet only. Do not use with real funds. See [SECURITY.md](SECURITY.md).

## Maintainers

| Name | Role | Contact |
|---|---|---|
| _add your name_ | Lead maintainer | _add your Telegram_ |

## Contributors

<a href="https://github.com/payflow-protocol/payflow-contract/graphs/contributors">
  <img src="https://contrib.rocks/image?repo=payflow-protocol/payflow-contract"/>
</a>

## License

Apache-2.0. See [LICENSE](LICENSE).
