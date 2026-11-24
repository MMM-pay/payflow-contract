#!/usr/bin/env bash
# Upgrade one deployed Payflow contract to a new wasm.
#
#   ./scripts/upgrade.sh <contract> <network> [source-identity]
#
#   contract : plan-registry | vault | subscription
#   network  : the network name known to the Stellar CLI (default: testnet)
#
# The contract's admin must sign, so `source-identity` (default:
# payflow-deployer) has to be the key that deployed the suite. The wasm is
# uploaded first; its hash is then passed to the contract's `upgrade`
# function, which swaps the code without touching any storage.
#
# Reads the contract ids from deployments/<network>.env.
set -euo pipefail

CONTRACT="${1:?usage: upgrade.sh <plan-registry|vault|subscription> <network> [source]}"
NETWORK="${2:-testnet}"
SOURCE="${3:-payflow-deployer}"
WASM_DIR="target/wasm32v1-none/release"

case "$CONTRACT" in
  plan-registry) WASM="payflow_plan_registry.wasm";  ID_VAR="PLAN_REGISTRY_ID" ;;
  vault)         WASM="payflow_vault.wasm";          ID_VAR="VAULT_ID" ;;
  subscription)  WASM="payflow_subscription.wasm";   ID_VAR="SUBSCRIPTION_ID" ;;
  *) echo "unknown contract '$CONTRACT' (want plan-registry, vault or subscription)" >&2; exit 1 ;;
esac

command -v stellar >/dev/null || { echo "stellar CLI not found" >&2; exit 1; }

ENV_FILE="deployments/$NETWORK.env"
[ -f "$ENV_FILE" ] || { echo "$ENV_FILE not found; run scripts/deploy.sh first" >&2; exit 1; }
# shellcheck disable=SC1090
. "$ENV_FILE"
ID="${!ID_VAR:?missing $ID_VAR in $ENV_FILE}"

echo "==> building"
stellar contract build >/dev/null

echo "==> uploading $WASM"
HASH="$(stellar contract upload --wasm "$WASM_DIR/$WASM" --source "$SOURCE" --network "$NETWORK" | tail -1)"

echo "==> upgrading $CONTRACT ($ID)"
echo "    new wasm hash: $HASH"
stellar contract invoke --id "$ID" --source "$SOURCE" --network "$NETWORK" \
  -- upgrade --new_wasm_hash "$HASH"

echo "==> done. Verify with: stellar contract invoke --id $ID --network $NETWORK -- admin"
