# Test token

`contracts/test-token` is a test-only SEP-41 token used by the Payflow test
suite. It is not a production asset, is not deployed by `scripts/deploy.sh`,
and has no generated TypeScript client. Never deploy it as a real token: its
admin can mint without limit and can make transfers fail on demand.

## What it implements

The contract implements the subset of SEP-41 that the vault calls:

- `transfer(from, to, amount)`, with the standard authorization and balance
  checks.
- `balance(id)`.
- `decimals()` (`7`), `name()` (`Test Token`) and `symbol()` (`TEST`).

It also exposes an admin-only `mint(to, amount)` so tests can fund wallets
without a Stellar Asset Contract issuer, and two failure-injection switches:

- `set_fail_transfers(fail)` makes every transfer panic with
  `TransferFailed`.
- `set_fail_transfers_to(to, fail)` makes transfers whose recipient is `to`
  fail, leaving other recipients working.

The read views `fail_transfers()` and `fail_transfers_to(to)` report the current
switches. Failure injection is what lets the suite prove that a token failure
rolls back a deposit, withdrawal or charge instead of leaving half of it
applied.

## Why it exists

The integration tests need a token whose behavior they fully control. A real
Stellar Asset Contract cannot mint arbitrarily or fail a transfer on cue, and
the vault's failure paths are part of what the suite must verify. This fixture
keeps those tests local and deterministic, with no testnet dependency.

The fixture is intentionally small and unaudited. It is excluded from the
deployment script, the release artifacts, and the bindings package.

## Using it in tests

Register the contract and use the generated `TestTokenClient`:

```rust
let token_id = env.register(TestToken, (admin.clone(),));
let token = TestTokenClient::new(&env, &token_id);
token.mint(&user, &10_000);
token.transfer(&user, &merchant, &1_000);
```

Both `mint` and `transfer` require the caller's authorization, as their
documented auth trees state. Tests normally call `env.mock_all_auths()` at
setup; negative auth tests install a specific signer instead.

## Security

This contract is a test double, not part of the deployed protocol:

- Its `__constructor` records any admin address supplied by the caller.
- The admin can mint unlimited balances and toggle transfer failures globally
  or per recipient.
- It has no allowances, no metadata beyond the fixed SEP-41 subset above, and
  no upgrade entry point.

Treat any address pointing at this contract as unsafe for real funds. The
testnet deployment in `deployments/testnet.env` uses the native Stellar Asset
Contract for settlement, not this token.
