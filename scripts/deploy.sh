#!/usr/bin/env bash
# Deploy the Payflow contract suite in dependency order.
#
#   ./scripts/deploy.sh [network] [source-identity]
#
# Dependency order is not optional: the subscription contract is initialized
# with the addresses of the registry and the vault, and the vault must be told
# which subscription contract may debit it.
set -euo pipefail

NETWORK="${1:-testnet}"
SOURCE="${2:-payflow-deployer}"
FEE_BPS="${FEE_BPS:-100}"
WASM_DIR="target/wasm32v1-none/release"

command -v stellar >/dev/null || { echo "stellar CLI not found" >&2; exit 1; }

if ! stellar keys address "$SOURCE" >/dev/null 2>&1; then
  echo "Identity '$SOURCE' not found. Create it with:" >&2
  echo "  stellar keys generate $SOURCE --network $NETWORK --fund" >&2
  exit 1
fi

ADMIN="$(stellar keys address "$SOURCE")"
FEE_TO="${FEE_TO:-$ADMIN}"

echo "network : $NETWORK"
echo "admin   : $ADMIN"
echo "fee     : ${FEE_BPS}bps -> $FEE_TO"
echo

echo "==> building"
stellar contract build >/dev/null

deploy() {
  stellar contract deploy --wasm "$WASM_DIR/$1" --source "$SOURCE" --network "$NETWORK" 2>/dev/null | tail -1
}

echo "==> 1/3 plan-registry"
REGISTRY="$(deploy payflow_plan_registry.wasm)"
stellar contract invoke --id "$REGISTRY" --source "$SOURCE" --network "$NETWORK" \
  -- initialize --admin "$ADMIN" >/dev/null 2>&1

echo "==> 2/3 vault"
VAULT="$(deploy payflow_vault.wasm)"
stellar contract invoke --id "$VAULT" --source "$SOURCE" --network "$NETWORK" \
  -- initialize --admin "$ADMIN" >/dev/null 2>&1

echo "==> 3/3 subscription"
SUBSCRIPTION="$(deploy payflow_subscription.wasm)"
stellar contract invoke --id "$SUBSCRIPTION" --source "$SOURCE" --network "$NETWORK" \
  -- initialize --admin "$ADMIN" --plan_registry "$REGISTRY" --vault "$VAULT" \
     --fee_bps "$FEE_BPS" --fee_to "$FEE_TO" >/dev/null 2>&1

echo "==> granting debit rights to subscription"
stellar contract invoke --id "$VAULT" --source "$SOURCE" --network "$NETWORK" \
  -- set_subscription --subscription "$SUBSCRIPTION" >/dev/null 2>&1

TOKEN="$(stellar contract id asset --asset native --network "$NETWORK" 2>/dev/null | tail -1)"

cat <<EOF

=========================================================
 Payflow deployed to $NETWORK
=========================================================

NEXT_PUBLIC_STELLAR_NETWORK=$NETWORK
NEXT_PUBLIC_PLAN_REGISTRY_ID=$REGISTRY
NEXT_PUBLIC_VAULT_ID=$VAULT
NEXT_PUBLIC_SUBSCRIPTION_ID=$SUBSCRIPTION
NEXT_PUBLIC_TOKEN_ID=$TOKEN

=========================================================
EOF
