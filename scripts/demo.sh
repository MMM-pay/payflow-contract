#!/usr/bin/env bash
# Seed and smoke-test a deployed Payflow suite.
# Usage: ./scripts/demo.sh [network]
# Reads contract ids from deployments/<network>.env, or from REGISTRY, VAULT
# and SUBSCRIPTION in the environment.
set -euo pipefail
NETWORK="${1:-testnet}"
if [[ -f "deployments/$NETWORK.env" ]]; then
  # shellcheck disable=SC1090
  . "deployments/$NETWORK.env"
  REGISTRY="${REGISTRY:-$PLAN_REGISTRY_ID}"
  VAULT="${VAULT:-$VAULT_ID}"
  SUBSCRIPTION="${SUBSCRIPTION:-$SUBSCRIPTION_ID}"
fi
: "${REGISTRY:?set REGISTRY}"; : "${VAULT:?set VAULT}"; : "${SUBSCRIPTION:?set SUBSCRIPTION}"

echo "==> seeding sample plans and mandates"
# seed.sh generates and funds the demo identities itself; a funding failure
# there stops this run too, because every invoke below needs those keys.
MANDATE="$(./scripts/seed.sh "$NETWORK")"
MERCHANT="$(stellar keys address pf-demo-merchant)"

echo "==> charging as an unrelated third party"
# Run the charge unfiltered: grepping the output for "Charged|Error" would
# let a failed charge exit zero, so CI could go green without billing.
stellar contract invoke --id "$SUBSCRIPTION" --source pf-demo-merchant --network "$NETWORK" \
  -- charge --keeper "$MERCHANT" --mandate_id "$MANDATE"

stellar contract invoke --id "$SUBSCRIPTION" --source pf-demo-merchant --network "$NETWORK" \
  -- get_mandate --mandate_id "$MANDATE" | tail -1

echo "==> the merchant's mandates, first page"
stellar contract invoke --id "$SUBSCRIPTION" --source pf-demo-merchant --network "$NETWORK" \
  -- merchant_mandates --merchant "$MERCHANT" --start 0 --limit 10 | tail -1
