check: fmt clippy test coverage build

fmt:
    cargo fmt --all -- --check

clippy:
    cargo clippy --all-targets -- -D warnings

test:
    cargo test --all

coverage:
    python3 scripts/check_coverage.py

build:
    stellar contract build

deploy network identity="payflow-deployer":
    ./scripts/deploy.sh {{network}} {{identity}}

demo network="testnet":
    ./scripts/demo.sh {{network}}
