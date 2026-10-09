#!/usr/bin/env bash
# End-to-end smoke test against a deployed Payflow suite.
# Usage: ./scripts/demo.sh [network]
# Reads contract ids from deployments/<network>.env, or from REGISTRY, VAULT
# and SUBSCRIPTION in the environment.
set -euo pipefail
NETWORK="${1:-testnet}"
if [ -f "deployments/$NETWORK.env" ]; then
  # shellcheck disable=SC1090
  . "deployments/$NETWORK.env"
  REGISTRY="${REGISTRY:-$PLAN_REGISTRY_ID}"
  VAULT="${VAULT:-$VAULT_ID}"
  SUBSCRIPTION="${SUBSCRIPTION:-$SUBSCRIPTION_ID}"
fi
: "${REGISTRY:?set REGISTRY}"; : "${VAULT:?set VAULT}"; : "${SUBSCRIPTION:?set SUBSCRIPTION}"

TOKEN="$(stellar contract id asset --asset native --network "$NETWORK" | tail -1)"
for id in pf-demo-merchant pf-demo-subscriber; do
  stellar keys generate "$id" --network "$NETWORK" --fund --overwrite >/dev/null 2>&1 || true
done
MERCHANT="$(stellar keys address pf-demo-merchant)"
SUBSCRIBER="$(stellar keys address pf-demo-subscriber)"

echo "==> merchant publishes a named 1 XLM / 60s plan"
PLAN="$(stellar contract invoke --id "$REGISTRY" --source pf-demo-merchant --network "$NETWORK" \
  -- create_plan --merchant "$MERCHANT" --token "$TOKEN" --amount 10000000 --period 60 \
  --name "Demo Monthly" 2>/dev/null | tail -1)"

echo "==> subscriber funds the vault with 5 XLM"
stellar contract invoke --id "$VAULT" --source pf-demo-subscriber --network "$NETWORK" \
  -- deposit --user "$SUBSCRIBER" --token "$TOKEN" --amount 50000000 >/dev/null 2>&1

echo "==> subscriber opens a mandate"
MANDATE="$(stellar contract invoke --id "$SUBSCRIPTION" --source pf-demo-subscriber --network "$NETWORK" \
  -- subscribe --subscriber "$SUBSCRIBER" --plan_id "$PLAN" --max_charges 3 2>/dev/null | tail -1)"

echo "==> charging as an unrelated third party"
stellar contract invoke --id "$SUBSCRIPTION" --source pf-demo-merchant --network "$NETWORK" \
  -- charge --mandate_id "$MANDATE" 2>&1 | grep -E "Charged|Error" | head -2

stellar contract invoke --id "$SUBSCRIPTION" --source pf-demo-merchant --network "$NETWORK" \
  -- get_mandate --mandate_id "$MANDATE" 2>/dev/null | tail -1

echo "==> the merchant's mandates, first page"
stellar contract invoke --id "$SUBSCRIPTION" --source pf-demo-merchant --network "$NETWORK" \
  -- merchant_mandates --merchant "$MERCHANT" --start 0 --limit 10 2>/dev/null | tail -1
