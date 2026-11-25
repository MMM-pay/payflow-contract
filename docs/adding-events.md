# Adding a contract event

Contract events are part of the public interface: indexers and clients depend
on their topics, field names, types and ordering. Follow these steps whenever
you add an event or change an existing layout.

## 1. Define the event

Add a typed event to the contract's `events.rs` with `#[contractevent]` and
document every field. Mark only fields intended for filtering as topics:

```rust
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExampleChanged {
    /// Entity identifier used to filter this event.
    #[topic]
    pub entity_id: u64,
    /// New value recorded by the event.
    pub value: u32,
}
```

Treat topic and field order as an API decision. Prefer carrying the data
indexers need over requiring one contract read per event.

## 2. Emit it with the state change

Import the event into the contract module and call `.publish(&env)` when the
corresponding state transition succeeds. Keep the event in the same invocation
as the state change so a failed transaction cannot leave a partial update.
Document the event on the public entry point.

## 3. Pin its layout

Add a layout test in the contract's `src/test_events.rs`. Snapshot both the
event XDR and the generated event-spec XDR; the former pins the emitted values,
and the latter pins the field names, types, ordering and topic/data locations.

For example, update only the affected contract's snapshots with:

```bash
UPDATE_SNAPSHOTS=1 cargo test -p payflow-subscription
```

Use the corresponding package name for `plan-registry` or `vault`. Review each
changed file under `contracts/<contract>/test_snapshots/events/`, then rerun
the same test command without `UPDATE_SNAPSHOTS` to verify the committed
snapshot.

## 4. Update the event reference

Update the relevant flow in
[`sequence-diagrams.md`](./sequence-diagrams.md) and any other documentation
that describes the contract's emitted events. If consumers outside this
repository index the event, coordinate their references and event handling as
well.

## 5. Regenerate bindings

If the contract interface changed, build the wasm and regenerate the checked-in
TypeScript bindings:

```bash
stellar contract build
./scripts/generate-bindings.sh
```

Review the generated `client.ts` and `types.ts` changes. The event's Rust
documentation is embedded in the contract spec and carried into generated
bindings.

See [Contributing: Event snapshots](../CONTRIBUTING.md#event-snapshots) for
the snapshot conventions and full validation commands.
