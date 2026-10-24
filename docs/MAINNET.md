# Mainnet deployment checklist

A mainnet deployment is irreversible in practice: contract constructors set the initial admin and wiring, and the admin address controls protocol configuration. Use a multisig account for `ADMIN`, keep the deployer separate, and review every value before broadcasting transactions.

## Before deployment

- [ ] Review the source, release tag, and all three WASM artifacts. Build from the reviewed commit with `stellar contract build`.
- [ ] Deploy and smoke-test the same artifacts on testnet first.
- [ ] Create or select a funded deployer identity. Keep its secret key in the local Stellar CLI key store; do not put secrets in environment variables or this repository.
- [ ] Set `ADMIN` to the public address of the intended multisig account. Verify its signer set and threshold on the target network using an independent account/configuration review. The deploy script cannot determine whether an account is a multisig.
- [ ] Set `FEE_TO` to the approved fee recipient. It must not equal the deployer address.
- [ ] Set `RPC_URL` to the organization's trusted mainnet Soroban RPC endpoint. Mainnet does not inherit the testnet RPC default.
- [ ] Review `FEE_BPS` and confirm that the fee recipient and fee policy have been approved.
- [ ] Confirm the deployment signing setup can authorize `ADMIN` for the vault's `set_subscription` call. That call requires admin authorization and runs after contract creation. The deploy script does not collect multisig approvals; if the installed CLI signing setup cannot satisfy the multisig threshold, stop and arrange the approved multisig transaction flow before deploying.
- [ ] Run a dry run and inspect every printed command and address:

```bash
ADMIN=G... FEE_TO=G... RPC_URL=https://your-mainnet-rpc \
  ./scripts/deploy.sh --dry-run --confirm-mainnet mainnet payflow-deployer
```

Dry run performs only a local Stellar identity lookup. It does not build artifacts, query the RPC, submit transactions, or write `deployments/mainnet.env`. Contract IDs and the ledger number are shown as placeholders because those values only exist after a live deployment.

## Live deployment

After two-person review of the dry-run output, deploy explicitly:

```bash
ADMIN=G... FEE_TO=G... RPC_URL=https://your-mainnet-rpc \
  ./scripts/deploy.sh --confirm-mainnet mainnet payflow-deployer
```

The script refuses mainnet unless `--confirm-mainnet` is present, `ADMIN` and `FEE_TO` are explicitly provided, both differ from the deployer address as required, and `RPC_URL` is set. Constructor configuration uses the selected admin address for all three contracts.

## After deployment

- [ ] Verify the registry, vault, and subscription contract IDs and constructor arguments on a trusted explorer or RPC.
- [ ] Verify that the vault's subscription address points to the new subscription contract.
- [ ] Verify that the subscription contract uses the intended plan registry, vault, fee basis points, and fee recipient.
- [ ] Confirm the native token contract ID and `START_LEDGER` in `deployments/mainnet.env`.
- [ ] Archive the reviewed source revision, WASM hashes, deployment output, and approvals according to the release process.
- [ ] Do not run `scripts/demo.sh` against mainnet with sample transactions. Use a separately reviewed, low-value operational smoke test if one is required.
