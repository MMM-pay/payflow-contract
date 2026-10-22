import assert from "node:assert/strict";
import { Networks } from "@stellar/stellar-sdk";
import { PlanRegistryClient } from "./dist/index.js";

const contractId = process.env.PLAN_REGISTRY_ID;
assert.ok(contractId, "PLAN_REGISTRY_ID is missing from deployment environment");

const client = new PlanRegistryClient({
  contractId,
  networkPassphrase: Networks.TESTNET,
  rpcUrl: "https://soroban-testnet.stellar.org",
});
const transaction = await client.next_plan_id();
await transaction.simulate();
const nextPlanId = transaction.result.unwrap();
assert.ok(nextPlanId >= 1n, `Unexpected next plan id: ${nextPlanId}`);
console.log(`plan-registry next_plan_id: ${nextPlanId}`);