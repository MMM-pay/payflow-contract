#!/usr/bin/env bash
# Deploy the Payflow contract suite in dependency order.
#
#   ./scripts/deploy.sh [network] [source-identity]
#
# Dependency order is not optional: the subscription contract is constructed
# with the addresses of the registry and the vault, and the vault must be told
# which subscription contract may debit it.
#
# Writes the resulting ids to deployments/<network>.env.
set -euo pipefail

NETWORK="${1:-testnet}"
SOURCE="${2:-payflow-deployer}"
FEE_BPS="${FEE_BPS:-100}"
RPC_URL="${RPC_URL:-https://soroban-testnet.stellar.org}"
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

# deploy <wasm> [constructor args...]
deploy() {
  local wasm="$1"
  shift
  stellar contract deploy --wasm "$WASM_DIR/$wasm" --source "$SOURCE" --network "$NETWORK" \
    -- "$@" 2>/dev/null | tail -1
}

# Indexers can start from here instead of scanning older ledgers.
START_LEDGER="$(curl -s -X POST -H 'content-type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"getLatestLedger"}' "$RPC_URL" \
  | sed -n 's/.*"sequence":\([0-9]*\).*/\1/p')"

# Each contract takes its configuration as constructor arguments, so it is
# deployed and configured in one transaction and there is no window in which
# someone else could claim it.
echo "==> 1/3 plan-registry"
REGISTRY="$(deploy payflow_plan_registry.wasm --admin "$ADMIN")"

echo "==> 2/3 vault"
VAULT="$(deploy payflow_vault.wasm --admin "$ADMIN")"

echo "==> 3/3 subscription"
SUBSCRIPTION="$(deploy payflow_subscription.wasm --admin "$ADMIN" \
  --plan_registry "$REGISTRY" --vault "$VAULT" --fee_bps "$FEE_BPS" --fee_to "$FEE_TO")"

# The vault and the subscription contract each need the other's address, so
# one side has to be wired after both exist. Only the vault admin can do it.
echo "==> granting debit rights to subscription"
stellar contract invoke --id "$VAULT" --source "$SOURCE" --network "$NETWORK" \
  -- set_subscription --subscription "$SUBSCRIPTION" >/dev/null 2>&1

TOKEN="$(stellar contract id asset --asset native --network "$NETWORK" 2>/dev/null | tail -1)"

mkdir -p deployments
cat > "deployments/$NETWORK.env" <<EOF
# Written by scripts/deploy.sh on $(date -u +%Y-%m-%dT%H:%M:%SZ)
NETWORK=$NETWORK
ADMIN=$ADMIN
FEE_BPS=$FEE_BPS
FEE_TO=$FEE_TO
PLAN_REGISTRY_ID=$REGISTRY
VAULT_ID=$VAULT
SUBSCRIPTION_ID=$SUBSCRIPTION
TOKEN_ID=$TOKEN
START_LEDGER=$START_LEDGER
EOF

echo
echo "Wrote deployments/$NETWORK.env:"
echo
cat "deployments/$NETWORK.env"
