extern crate std;
use soroban_sdk::{
    xdr::{Limits, WriteXdr},
    Address, Env, Event,
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
fn subscribed_layout() {
    let env = Env::default();
    let a = address(&env);
    snapshot(
        &env,
        "subscribed",
        super::Subscribed::spec_xdr(),
        &super::Subscribed {
            mandate_id: 11,
            subscriber: a.clone(),
            merchant: a.clone(),
            plan_id: 3,
            amount: 4_500,
            period: 86_400,
            next_charge: 1_700_000_000,
            max_charges: 24,
            max_total: 108_000,
            fee_bps: 250,
            keeper_bps: 100,
        },
    );
}
#[test]
fn charged_layout() {
    let env = Env::default();
    let a = address(&env);
    snapshot(
        &env,
        "charged",
        super::Charged::spec_xdr(),
        &super::Charged {
            mandate_id: 11,
            subscriber: a.clone(),
            merchant: a.clone(),
            amount: 4_000,
            fee: 250,
            tip: 50,
            keeper: a,
            charges_made: 2,
            next_charge: 1_700_086_400,
        },
    );
}
#[test]
fn cancelled_layout() {
    let env = Env::default();
    let a = address(&env);
    snapshot(
        &env,
        "cancelled",
        super::Cancelled::spec_xdr(),
        &super::Cancelled {
            mandate_id: 11,
            subscriber: a,
            charges_made: 2,
        },
    );
}
#[test]
fn mandate_ended_layout() {
    let env = Env::default();
    let a = address(&env);
    snapshot(
        &env,
        "mandate_ended",
        super::MandateEnded::spec_xdr(),
        &super::MandateEnded {
            mandate_id: 11,
            merchant: a.clone(),
            subscriber: a,
            charges_made: 2,
        },
    );
}
#[test]
fn pause_changed_layout() {
    let env = Env::default();
    snapshot(
        &env,
        "pause_changed",
        super::PauseChanged::spec_xdr(),
        &super::PauseChanged {
            mandate_id: 11,
            subscriber: address(&env),
            paused: true,
        },
    );
}
#[test]
fn mandate_completed_layout() {
    let env = Env::default();
    snapshot(
        &env,
        "mandate_completed",
        super::MandateCompleted::spec_xdr(),
        &super::MandateCompleted {
            mandate_id: 11,
            subscriber: address(&env),
            charges_made: 24,
        },
    );
}
#[test]
fn fee_changed_layout() {
    let env = Env::default();
    snapshot(
        &env,
        "fee_changed",
        super::FeeChanged::spec_xdr(),
        &super::FeeChanged {
            admin: address(&env),
            old_fee_bps: 250,
            new_fee_bps: 300,
        },
    );
}

#[test]
fn refunded_layout() {
    let env = Env::default();
    let a = address(&env);
    snapshot(
        &env,
        "refunded",
        super::Refunded::spec_xdr(),
        &super::Refunded {
            mandate_id: 11,
            subscriber: a.clone(),
            merchant: a,
            amount: 400,
            refunded_total: 900,
        },
    );
}
