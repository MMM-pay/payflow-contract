# Testnet quickstart

Deploy the full Payflow contract suite (plan-registry → vault → subscription)
to Stellar testnet and run a live end-to-end smoke test.

## Prerequisites

| Tool | Minimum version |
|---|---|
| Rust (stable) | 1.78 |
| `wasm32v1-none` target | — |
| Stellar CLI | 27 |

```bash
rustup target add wasm32v1-none
# install the CLI if not present
cargo install --locked stellar-cli --features opt
```

## 1. Create a funded deployer identity

```bash
stellar keys generate payflow-deployer --network testnet --fund
```

The faucet tops up the account with enough XLM to cover deployment fees.
Check your balance:

```bash
stellar balance --network testnet payflow-deployer
```

## 2. Build the contracts

```bash
stellar contract build
```

This produces three WASM files under `target/wasm32v1-none/release/`:

- `payflow_plan_registry.wasm`
- `payflow_vault.wasm`
- `payflow_subscription.wasm`

## 3. Deploy

```bash
./scripts/deploy.sh testnet payflow-deployer
```

The script deploys the contracts in dependency order:

1. **plan-registry** — initialised with the deployer as admin.
2. **vault** — initialised with the deployer as admin and a reserve cap of
   9000 bps (90 %): at most that share of a subscriber's balance may be
   earmarked as reserves, so a free balance always stays withdrawable.
   Override it with `RESERVE_CAP_BPS=<0-10000>`.
3. **subscription** — initialised with references to the registry and vault, a
   default fee of 100 bps (1 %), and the deployer as fee recipient.
4. **Debit rights** — the vault is told that the subscription contract may
   debit it.
5. **Plan-token validation** — the registry is told the subscription address
   so merchants cannot publish plans using the subscription contract as a token.

On success the script prints a block of environment variables:

```
NEXT_PUBLIC_STELLAR_NETWORK=testnet
NEXT_PUBLIC_PLAN_REGISTRY_ID=C...
NEXT_PUBLIC_VAULT_ID=C...
NEXT_PUBLIC_SUBSCRIPTION_ID=C...
NEXT_PUBLIC_TOKEN_ID=C...
```

Copy these into your frontend `.env.local` and into the Render dashboard for
the backend indexer. See the top-level README for the full list of env vars.

## 4. Smoke test

Export the three contract IDs the deploy script printed, then run:

```bash
export REGISTRY=CXXXX...
export VAULT=CXXXX...
export SUBSCRIPTION=CXXXX...

./scripts/demo.sh testnet
```

The demo script:

1. Creates two funded identities (`pf-demo-merchant` and `pf-demo-subscriber`).
2. Publishes a 1 XLM / 60-second plan.
3. Deposits 5 XLM into the vault for the subscriber.
4. Opens a mandate limited to three charges.
5. Charges the mandate once and prints the result.

A successful run ends with a JSON mandate object showing `charges_made: 1`.

## 5. Optionally set a custom fee

```bash
stellar contract invoke --id "$SUBSCRIPTION" \
  --source payflow-deployer --network testnet \
  -- set_fee_bps --new_fee_bps 50
```

## Troubleshooting

| Symptom | Likely cause |
|---|---|
| `Identity not found` during deploy | Run step 1 again; the CLI does not persist identities across machines. |
| `InsufficientBalance` on charge | The subscriber has not deposited into the vault, or the vault balance is exhausted. |
| `SubscriptionNotSet` on debit | The vault was not told which subscription contract may call it. The deploy script handles this automatically; a manual deploy must call `set_subscription`. |
| `NotInitialized` on `create_plan` | The registry has not been wired to the subscription address. Call `set_subscription_contract` as the registry admin after deployment. |
| `AlreadyInitialized` | The deploy script deployed to an existing contract ID. Use `stellar contract deploy` without `--wasm-hash` to get a fresh ID. |

## Re-deploying

Redeploy the suite by running `./scripts/deploy.sh` again (it always uploads new
instances). After redeploy, update the four `NEXT_PUBLIC_*` variables in the
frontend and the Render backend environment before the new contracts go live.
