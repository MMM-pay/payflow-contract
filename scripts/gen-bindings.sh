#!/usr/bin/env bash
# Generate TypeScript bindings for all three Payflow contracts.
#
#   ./scripts/gen-bindings.sh [network] [out-dir]
#
# Doc comments written on each contract function (/// …) are embedded in the
# contract spec by soroban-sdk and carried through to the generated TypeScript
# as JSDoc comments by the Stellar CLI.
#
# Prerequisites: Stellar CLI 27+, a built wasm (run `stellar contract build`
# or `./scripts/deploy.sh` first to also obtain live contract IDs).
set -euo pipefail

NETWORK="${1:-testnet}"
OUT="${2:-../payflow-frontend/src/contracts}"

WASM_DIR="target/wasm32v1-none/release"

command -v stellar >/dev/null || { echo "stellar CLI not found" >&2; exit 1; }

for contract in plan-registry vault subscription; do
  safe="${contract//-/_}"
  wasm="$WASM_DIR/payflow_${safe}.wasm"

  if [[ ! -f "$wasm" ]]; then
    echo "WASM not found: $wasm — run 'stellar contract build' first" >&2
    exit 1
  fi

  echo "==> generating bindings for $contract"
  stellar contract bindings typescript \
    --wasm "$wasm" \
    --network "$NETWORK" \
    --output-dir "$OUT/$contract" \
    --overwrite
done

echo
echo "Bindings written to $OUT"
echo "JSDoc comments sourced from /// doc comments in each contract's lib.rs."
