# Run the core CI checks locally, in order. Just stops on the first failure.
# Requires Rust/rustfmt/clippy, the wasm32v1-none target and Stellar CLI.
# CI also runs separate coverage, dependency-audit, MSRV and reproducibility jobs.
check: fmt clippy test build

# Discoverable alias for contributors familiar with the CI workflow.
ci: check

fmt:
    cargo fmt --all -- --check

clippy:
    cargo clippy --all-targets -- -D warnings

test:
    cargo test --all
    ./scripts/check_auth_tests.sh

build:
    stellar contract build

deploy network identity="payflow-deployer":
    ./scripts/deploy.sh {{network}} {{identity}}

demo network="testnet":
    ./scripts/demo.sh {{network}}
