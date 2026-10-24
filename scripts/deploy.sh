#!/usr/bin/env bash
# Deploy the Payflow contract suite in dependency order.
#
#   ./scripts/deploy.sh [--dry-run] [--confirm-mainnet] [network] [source-identity]
#
# Dependency order is not optional: the subscription contract is constructed
# with the addresses of the registry and the vault, and the vault must be told
# which subscription contract may debit it.
#
# Writes the resulting ids to deployments/<network>.env.
set -euo pipefail

NETWORK="testnet"
SOURCE="payflow-deployer"
DRY_RUN=false
CONFIRM_MAINNET=false
POSITIONAL=()

while (($#)); do
  case "$1" in
    --dry-run)
      DRY_RUN=true
      ;;
    --confirm-mainnet)
      CONFIRM_MAINNET=true
      ;;
    --help|-h)
      sed -n '2,12p' "$0"
      exit 0
      ;;
    -* )
      echo "Unknown option: $1" >&2
      exit 2
      ;;
    *)
      POSITIONAL+=("$1")
      ;;
  esac
  shift
done

if ((${#POSITIONAL[@]} > 2)); then
  echo "Usage: $0 [--dry-run] [--confirm-mainnet] [network] [source-identity]" >&2
  exit 2
fi

NETWORK="${POSITIONAL[0]:-testnet}"
SOURCE="${POSITIONAL[1]:-payflow-deployer}"
FEE_BPS="${FEE_BPS:-100}"
if [[ "$NETWORK" == "mainnet" ]]; then
  RPC_URL="${RPC_URL:-}"
else
  RPC_URL="${RPC_URL:-https://soroban-testnet.stellar.org}"
fi
WASM_DIR="target/wasm32v1-none/release"

command -v stellar >/dev/null || { echo "stellar CLI not found" >&2; exit 1; }

if ! stellar keys address "$SOURCE" >/dev/null 2>&1; then
  echo "Identity '$SOURCE' not found. Create it with:" >&2
  echo "  stellar keys generate $SOURCE --network $NETWORK --fund" >&2
  exit 1
fi

DEPLOYER="$(stellar keys address "$SOURCE")"
ADMIN_OVERRIDE="${ADMIN:-}"
FEE_TO_OVERRIDE="${FEE_TO:-}"
ADMIN="${ADMIN_OVERRIDE:-$DEPLOYER}"
FEE_TO="${FEE_TO_OVERRIDE:-$ADMIN}"

if [[ "$NETWORK" == "mainnet" ]]; then
  if [[ "$CONFIRM_MAINNET" != true ]]; then
    echo "Mainnet deployment requires --confirm-mainnet." >&2
    exit 2
  fi
  if [[ -z "$ADMIN_OVERRIDE" ]]; then
    echo "Mainnet deployment requires ADMIN to be set to the multisig admin address." >&2
    exit 2
  fi
  if [[ -z "$FEE_TO_OVERRIDE" ]]; then
    echo "Mainnet deployment requires FEE_TO to be set explicitly." >&2
    exit 2
  fi
  if [[ "$ADMIN" == "$DEPLOYER" ]]; then
    echo "Mainnet ADMIN must not be the deployer address." >&2
    exit 2
  fi
  if [[ "$FEE_TO" == "$DEPLOYER" ]]; then
    echo "Mainnet FEE_TO must not be the deployer address." >&2
    exit 2
  fi
  if [[ -z "$RPC_URL" ]]; then
    echo "Mainnet deployment requires RPC_URL to be set explicitly." >&2
    exit 2
  fi
fi

echo "network : $NETWORK"
echo "admin   : $ADMIN"
echo "fee     : ${FEE_BPS}bps -> $FEE_TO"
echo

# Print shell-escaped commands in dry-run mode without running any command that
# could contact a network or write deployment output.
print_command() {
  printf '  '
  printf '%q ' "$@"
  printf '\n'
}

if [[ "$DRY_RUN" == true ]]; then
  printf 'DRY RUN: no commands will be executed except local key lookup.\n\n'
  echo '==> building'
  print_command stellar contract build
  echo '==> reading latest ledger'
  print_command curl -s -X POST -H 'content-type: application/json' \
    -d '{"jsonrpc":"2.0","id":1,"method":"getLatestLedger"}' "$RPC_URL"
  echo '==> 1/3 plan-registry'
  print_command stellar contract deploy --wasm "$WASM_DIR/payflow_plan_registry.wasm" \
    --source "$SOURCE" --network "$NETWORK" -- --admin "$ADMIN"
  echo '    result: <REGISTRY_ID>'
  echo '==> 2/3 vault'
  print_command stellar contract deploy --wasm "$WASM_DIR/payflow_vault.wasm" \
    --source "$SOURCE" --network "$NETWORK" -- --admin "$ADMIN"
  echo '    result: <VAULT_ID>'
  echo '==> 3/3 subscription'
  print_command stellar contract deploy --wasm "$WASM_DIR/payflow_subscription.wasm" \
    --source "$SOURCE" --network "$NETWORK" -- --admin "$ADMIN" \
    --plan_registry '<REGISTRY_ID>' --vault '<VAULT_ID>' --fee_bps "$FEE_BPS" --fee_to "$FEE_TO"
  echo '    result: <SUBSCRIPTION_ID>'
  echo '==> granting debit rights to subscription'
  print_command stellar contract invoke --id '<VAULT_ID>' --source "$SOURCE" \
    --network "$NETWORK" -- set_subscription --subscription '<SUBSCRIPTION_ID>'
  echo '==> reading native token id'
  print_command stellar contract id asset --asset native --network "$NETWORK"
  echo "==> writing deployments/$NETWORK.env (after successful deployment)"
  exit 0
fi

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
