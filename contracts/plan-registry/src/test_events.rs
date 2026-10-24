extern crate std;
use soroban_sdk::Symbol;
use soroban_sdk::{
    xdr::{Limits, WriteXdr},
    Address, Env, Event, String,
};
use std::format;

const CONTRACT: &str = "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABSC4";

fn address(env: &Env) -> Address {
    Address::from_str(env, CONTRACT)
}

// The emitted event XDR captures topics and data values. Soroban encodes default
// event data as a name-sorted map, so also snapshot the generated event spec to
// pin source parameter order and each field's topic/data location.
fn snapshot<const N: usize>(env: &Env, event_name: &str, spec_xdr: [u8; N], event: &impl Event) {
    let event_xdr = event
        .to_xdr(env, &address(env))
        .to_xdr(Limits::none())
        .unwrap();
    let event_hex = event_xdr
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<std::vec::Vec<_>>()
        .join("");
    let spec_hex = spec_xdr
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<std::vec::Vec<_>>()
        .join("");
    let actual = format!("event: {event_hex}\nspec: {spec_hex}");
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("test_snapshots/events")
        .join(format!("{event_name}.xdr"));
    if std::env::var("UPDATE_SNAPSHOTS").as_deref() == Ok("1") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, format!("{actual}\n")).unwrap();
    } else {
        let expected = std::fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!(
                "missing event snapshot {} ({error}); run UPDATE_SNAPSHOTS=1 cargo test --all",
                path.display()
            )
        });
        assert_eq!(expected.trim(), actual, "event snapshot differs: {}; review the XDR change, then run UPDATE_SNAPSHOTS=1 cargo test --all", path.display());
    }
}

#[test]
fn plan_created_layout() {
    let env = Env::default();
    let a = address(&env);
    snapshot(
        &env,
        "plan_created",
        super::PlanCreated::spec_xdr(),
        &super::PlanCreated {
            merchant: a.clone(),
            plan_id: 7,
            token: a,
            amount: 12_345,
            period: 86_400,
            name: String::from_str(&env, "sample-plan"),
            description: String::from_str(&env, "sample description"),
            keeper_bps: 125,
            category: Symbol::new(&env, "fitness"),
        },
    );
}

#[test]
fn plan_status_changed_layout() {
    let env = Env::default();
    snapshot(
        &env,
        "plan_status_changed",
        super::PlanStatusChanged::spec_xdr(),
        &super::PlanStatusChanged {
            merchant: address(&env),
            plan_id: 7,
            active: true,
        },
    );
}

#[test]
fn profile_updated_layout() {
    let env = Env::default();
    snapshot(
        &env,
        "profile_updated",
        super::ProfileUpdated::spec_xdr(),
        &super::ProfileUpdated {
            merchant: address(&env),
            name: String::from_str(&env, "Sample Merchant"),
            url: String::from_str(&env, "https://example.test"),
        },
    );
}

#[test]
fn plan_successor_set_layout() {
    let env = Env::default();
    snapshot(
        &env,
        "plan_successor_set",
        super::PlanSuccessorSet::spec_xdr(),
        &super::PlanSuccessorSet {
            merchant: address(&env),
            plan_id: 7,
            successor_id: 8,
        },
    );
}
