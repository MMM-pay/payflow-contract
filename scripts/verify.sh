#!/usr/bin/env bash
# Verify deployed contract WASMs match the source in this repository.
# Usage: ./scripts/verify.sh [network]
set -euo pipefail

NETWORK="${1:-testnet}"
ENV_FILE="deployments/$NETWORK.env"
WASM_DIR="target/wasm32v1-none/release"

command -v stellar >/dev/null || { echo "stellar CLI not found" >&2; exit 1; }
command -v sha256sum >/dev/null || { echo "sha256sum not found" >&2; exit 1; }
[[ -f "$ENV_FILE" ]] || { echo "Deployment file not found: $ENV_FILE" >&2; exit 1; }

# shellcheck disable=SC1090
. "$ENV_FILE"

: "${PLAN_REGISTRY_ID:?PLAN_REGISTRY_ID missing from $ENV_FILE}"
: "${VAULT_ID:?VAULT_ID missing from $ENV_FILE}"
: "${SUBSCRIPTION_ID:?SUBSCRIPTION_ID missing from $ENV_FILE}"

echo "==> building source WASMs"
stellar contract build >/dev/null

TEMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TEMP_DIR"' EXIT

matches=0
failed=0
printf '%-16s %-64s %-64s %s\n' "Contract" "Source SHA-256" "Deployed SHA-256" "Result"

verify_contract() {
  local name="$1"
  local wasm="$2"
  local contract_id="$3"
  local local_file="$WASM_DIR/$wasm"
  local fetched_file="$TEMP_DIR/$wasm"
  local source_hash
  local deployed_hash

  if [[ ! -f "$local_file" ]]; then
    echo "Built WASM not found: $local_file" >&2
    return 1
  fi

  echo "==> fetching $name ($contract_id)" >&2
  stellar contract fetch --id "$contract_id" --network "$NETWORK" --out-file "$fetched_file" >/dev/null
  source_hash="$(sha256sum "$local_file" | cut -d ' ' -f 1)"
  deployed_hash="$(sha256sum "$fetched_file" | cut -d ' ' -f 1)"

  if [[ "$source_hash" == "$deployed_hash" ]]; then
    printf '%-16s %-64s %-64s %s\n' "$name" "$source_hash" "$deployed_hash" "MATCH"
    ((matches += 1))
  else
    printf '%-16s %-64s %-64s %s\n' "$name" "$source_hash" "$deployed_hash" "MISMATCH"
    failed=1
  fi
}

verify_contract "plan-registry" "payflow_plan_registry.wasm" "$PLAN_REGISTRY_ID"
verify_contract "vault" "payflow_vault.wasm" "$VAULT_ID"
verify_contract "subscription" "payflow_subscription.wasm" "$SUBSCRIPTION_ID"

if (( failed != 0 )); then
  echo "Verification failed: deployed WASM does not match source." >&2
  exit 1
fi

echo "Verified $matches deployed contracts against source."