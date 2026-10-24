#!/usr/bin/env bash
# Create sample plans and mandates in an existing Payflow deployment.
# Usage: ./scripts/seed.sh [network]
set -euo pipefail

NETWORK="${1:-testnet}"
DEPLOYMENT_FILE="deployments/$NETWORK.env"
if [[ -f "$DEPLOYMENT_FILE" ]]; then
  # shellcheck disable=SC1090
  . "$DEPLOYMENT_FILE"
fi
REGISTRY="${REGISTRY:-${PLAN_REGISTRY_ID:-}}"
VAULT="${VAULT:-${VAULT_ID:-}}"
SUBSCRIPTION="${SUBSCRIPTION:-${SUBSCRIPTION_ID:-}}"
TOKEN="${TOKEN:-${TOKEN_ID:-}}"
: "${REGISTRY:?set REGISTRY or PLAN_REGISTRY_ID}"
: "${VAULT:?set VAULT or VAULT_ID}"
: "${SUBSCRIPTION:?set SUBSCRIPTION or SUBSCRIPTION_ID}"
: "${TOKEN:?set TOKEN or TOKEN_ID}"

command -v stellar >/dev/null || { echo "stellar CLI not found" >&2; exit 1; }

ensure_identity() {
  local identity="$1"
  if ! stellar keys address "$identity" >/dev/null 2>&1; then
    stellar keys generate "$identity" --network "$NETWORK" --fund >/dev/null
  fi
}

ensure_identity pf-demo-merchant
ensure_identity pf-demo-subscriber
MERCHANT="$(stellar keys address pf-demo-merchant)"
SUBSCRIBER="$(stellar keys address pf-demo-subscriber)"

echo "==> publishing weekly, monthly, and yearly plans" >&2
WEEKLY_PLAN="$(stellar contract invoke --id "$REGISTRY" --source pf-demo-merchant --network "$NETWORK" \
  -- create_plan --merchant "$MERCHANT" --token "$TOKEN" --amount 10000000 --period 604800 \
  --name "Demo Weekly" --keeper_bps 100 | tail -1)"
MONTHLY_PLAN="$(stellar contract invoke --id "$REGISTRY" --source pf-demo-merchant --network "$NETWORK" \
  -- create_plan --merchant "$MERCHANT" --token "$TOKEN" --amount 50000000 --period 2592000 \
  --name "Demo Monthly" --keeper_bps 100 | tail -1)"
YEARLY_PLAN="$(stellar contract invoke --id "$REGISTRY" --source pf-demo-merchant --network "$NETWORK" \
  -- create_plan --merchant "$MERCHANT" --token "$TOKEN" --amount 100000000 --period 31536000 \
  --name "Demo Yearly" --keeper_bps 100 | tail -1)"

echo "==> funding the subscriber's vault balance" >&2
stellar contract invoke --id "$VAULT" --source pf-demo-subscriber --network "$NETWORK" \
  -- deposit --user "$SUBSCRIBER" --token "$TOKEN" --amount 500000000 >/dev/null

echo "==> opening two active mandates and one mandate to end" >&2
WEEKLY_MANDATE="$(stellar contract invoke --id "$SUBSCRIPTION" --source pf-demo-subscriber --network "$NETWORK" \
  -- subscribe --subscriber "$SUBSCRIBER" --plan_id "$WEEKLY_PLAN" --max_charges 12 | tail -1)"
stellar contract invoke --id "$SUBSCRIPTION" --source pf-demo-subscriber --network "$NETWORK" \
  -- subscribe --subscriber "$SUBSCRIBER" --plan_id "$MONTHLY_PLAN" --max_charges 12 >/dev/null
ENDED_MANDATE="$(stellar contract invoke --id "$SUBSCRIPTION" --source pf-demo-subscriber --network "$NETWORK" \
  -- subscribe --subscriber "$SUBSCRIBER" --plan_id "$YEARLY_PLAN" --max_charges 12 | tail -1)"
stellar contract invoke --id "$SUBSCRIPTION" --source pf-demo-merchant --network "$NETWORK" \
  -- end_mandate --merchant "$MERCHANT" --mandate_id "$ENDED_MANDATE" >/dev/null

echo "Weekly plan: $WEEKLY_PLAN; monthly plan: $MONTHLY_PLAN; yearly plan: $YEARLY_PLAN" >&2
echo "Weekly mandate: $WEEKLY_MANDATE; ended mandate: $ENDED_MANDATE" >&2
printf '%s\n' "$WEEKLY_MANDATE"