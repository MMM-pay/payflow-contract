# Contributing to Payflow Contracts

## Before you start

Comment on the issue you want to take so two people do not build the same thing.
Issues carry a complexity label that maps to the Drips Wave point value.

## Setup

```bash
rustup toolchain install stable
rustup target add wasm32v1-none
cargo test --all
```

You will also want the Stellar CLI (27+) to build wasm and deploy:

```bash
stellar contract build
```

## What CI enforces

A PR cannot merge unless all four jobs pass:

| Job | Command |
|---|---|
| `fmt` | `cargo fmt --all -- --check` |
| `clippy` | `cargo clippy --all-targets -- -D warnings` |
| `test` | `cargo test --all` |
| `build-wasm` | `stellar contract build` |

Run all four locally before pushing.

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
