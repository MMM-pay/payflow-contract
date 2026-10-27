#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WASM_DIR="$ROOT/target/wasm32v1-none/release"
OUT_DIR="$ROOT/bindings/src"
SDK_VERSION="17.3.0"

command -v node >/dev/null || { echo "node is required" >&2; exit 1; }
command -v npm >/dev/null || { echo "npm is required" >&2; exit 1; }

for contract in plan_registry vault subscription; do
  wasm="$WASM_DIR/payflow_${contract}.wasm"
  if [[ ! -f "$wasm" ]]; then
    echo "Missing $wasm; run 'stellar contract build' first" >&2
    exit 1
  fi
done

# stellar-cli v27 deprecated its TypeScript generator in favor of the SDK CLI.
# Keep this SDK version in sync with bindings/package.json for reproducible output.
rm -rf "$ROOT/bindings/.generated"
mkdir -p "$ROOT/bindings/.generated"

for contract in plan_registry vault subscription; do
  package_name="${contract//_/-}"
  case "$contract" in
    plan_registry) client_name="PlanRegistryClient" ;;
    vault) client_name="VaultClient" ;;
    subscription) client_name="SubscriptionClient" ;;
  esac
  npx --yes "@stellar/stellar-sdk@$SDK_VERSION" generate \
    --wasm "$WASM_DIR/payflow_${contract}.wasm" \
    --output-dir "$ROOT/bindings/.generated/$package_name" \
    --contract-name "$client_name" \
    --overwrite
  mkdir -p "$OUT_DIR/$package_name"
  cp "$ROOT/bindings/.generated/$package_name/src/client.ts" "$OUT_DIR/$package_name/client.ts"
  cp "$ROOT/bindings/.generated/$package_name/src/types.ts" "$OUT_DIR/$package_name/types.ts"
done