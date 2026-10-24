# Contributing to Payflow Contracts

# # Before you start

Comment on the issue you want to take so two people do not build the same thing.
Issues carry a complexity label that maps to the Drips Wave point value.

## Setup

```bash
rustup toolchain install stable
rustup target add wasm32v1-none
cargo test --all
```

The Minimum Supported Rust Version (MSRV) for these contracts is **1.91.0**, pinned in `Cargo.toml` (`rust-version = "1.91.0"`) and verified in CI.

You will also want the Stellar CLI (27+) to build wasm and deploy:

```bash
stellar contract build
```

## What CI enforces

A PR cannot merge unless all CI jobs pass:

| Job | Command |
|---|---|
| `fmt` | `cargo fmt --all -- --check` |
| `clippy` | `cargo clippy --all-targets -- -D warnings` |
| `test` | `cargo test --all` |
| `msrv` | `cargo check --all --all-targets` (on Rust 1.91.0) |
| `build-wasm` | `stellar contract build` |

Install [just](https://just.systems/) to run the same four checks locally:

```bash
just check
```

The `check` recipe runs formatting, clippy, tests, and the wasm build. You can
also run `just build` by itself. For deployment and the end-to-end smoke test,
use `just deploy <network> <identity>` and `just demo <network>` respectively.
Deployment defaults to the `testnet` network and the `payflow-deployer`
identity; the demo defaults to `testnet`.

## Code standards

- **No `unwrap()` or `expect()` outside `#[cfg(test)]`.** Return a
  `contracterror` variant instead.
- **No floating point.** All proportional math is integer basis points against
  `BPS_DENOMINATOR` (10 000).
- **No panics for control flow.** Use `Result<T, Error>` and add a variant.
- **Storage.** Config goes in `instance` storage; per-entity records go in
  `persistent`. Every persistent write must extend TTL.
- **Events.** Use the `#[contractevent]` macro, never the deprecated
  `env.events().publish` tuple form. Indexers depend on the typed shape.
- **Auth.** State exactly whose `require_auth()` is required in the doc comment
  of any function that mutates state.
- **File layout.** Keep the `lib.rs` / `types.rs` / `error.rs` / `events.rs` /
  `test.rs` split.

## Tests

Every behavioural change needs a test. Follow the existing naming: describe the
behaviour, not the function — `keeper_outage_does_not_create_a_chargeable_backlog`,
not `test_charge_3`.

Test both directions: the happy path and the specific error variant returned on
the unhappy path, using `try_*` client methods.

### Event snapshots

Each contract event has a committed XDR snapshot under
`contracts/<contract>/test_snapshots/events/`. To update snapshots after an
intentional event layout change, run:

```bash
UPDATE_SNAPSHOTS=1 cargo test --all
```

Review every changed snapshot in the diff before committing. Each snapshot
contains the emitted event XDR and generated event-spec XDR; together they pin
topic symbols, field names, types, order and topic/data locations. Intended
event changes also ripple into `payflow-backend`, `payflow-frontend` and
`payflow-docs`, whose indexers and clients may need coordinated updates.

## Commits

Conventional commits, one logical change per commit:

```
feat(subscription): add mandate pause and resume
fix(vault): reject zero-amount deposits
test(subscription): cover keeper outage backlog
docs(readme): document permissionless charge
```

## Pull requests

- Branch from `main`.
- Link the issue you are closing (`Closes #12`).
- Describe what invariant your change preserves or adds.
- Keep the diff scoped to the issue.
