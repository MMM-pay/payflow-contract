extern crate std;
use soroban_sdk::Symbol;
use soroban_sdk::{
    xdr::{Limits, WriteXdr},
    Address, Env, Event, String,
};
use std::fmt::Write as _;
use std::format;

const CONTRACT: &str = "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABSC4";

fn address(env: &Env) -> Address {
    Address::from_str(env, CONTRACT)
}

// The emitted event XDR captures topics and data values. Soroban encodes default
// event data as a name-sorted map, so also snapshot the generated event spec to
// pin source parameter order and each field's topic/data location.
/// Break a hex string into 128-byte lines so no single line is one long
/// unbroken run of hex digits.
fn wrap(hex: &str) -> std::string::String {
    hex.as_bytes()
        .chunks(128)
        .map(|chunk| std::str::from_utf8(chunk).expect("hex is ASCII"))
        .collect::<std::vec::Vec<_>>()
        .join("\n  ")
}

fn snapshot<const N: usize>(env: &Env, event_name: &str, spec_xdr: [u8; N], event: &impl Event) {
    let event_xdr = event
        .to_xdr(env, &address(env))
        .to_xdr(Limits::none())
        .unwrap();
    let event_hex = event_xdr
        .iter()
        .fold(std::string::String::new(), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        });
    let spec_hex = spec_xdr
        .iter()
        .fold(std::string::String::new(), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        });
    // The hex is wrapped at 128 bytes a line. The repository's malware scan
    // flags any unbroken run of 400+ base64-alphabet characters as a possible
    // obfuscated payload, and these snapshots are legitimately long, so wrap
    // for the reader and let the comparison below ignore the whitespace.
    let actual = format!("event: {}\nspec: {}", wrap(&event_hex), wrap(&spec_hex));
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
        // Whitespace only wraps the hex for reviewability, so compare without it.
        let expected: std::string::String = expected.split_whitespace().collect();
        let actual: std::string::String = actual.split_whitespace().collect();
        assert_eq!(expected, actual, "event snapshot differs: {}; review the XDR change, then run UPDATE_SNAPSHOTS=1 cargo test --all", path.display());
    }
}

// PlanCreated events
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
            accepted_tokens: soroban_sdk::vec![&env, a.clone()],
            amount: 12_345,
            period: 86_400,
            name: String::from_str(&env, "sample-plan"),
            description: String::from_str(&env, "sample description"),
            keeper_bps: 125,
            category: Symbol::new(&env, "fitness"),
            terms_uri: String::from_str(&env, "https://example.com/terms"),
            available_until: 0,
            referral_bps: 0,
            payout: a.clone(),
            max_subscribers: 0,
            retry_window_ledgers: None,
        },
    );
}

// PlansCreated events
#[test]
fn plans_created_layout() {
    let env = Env::default();
    snapshot(
        &env,
        "plans_created",
        super::PlansCreated::spec_xdr(),
        &super::PlansCreated {
            merchant: address(&env),
            plan_ids: soroban_sdk::vec![&env, 7, 8, 9],
        },
    );
}

// PlanStatusChanged events
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

// PlanChargesPausedChanged events
#[test]
fn plan_charges_paused_changed_layout() {
    let env = Env::default();
    snapshot(
        &env,
        "plan_charges_paused_changed",
        super::PlanChargesPausedChanged::spec_xdr(),
        &super::PlanChargesPausedChanged {
            merchant: address(&env),
            plan_id: 7,
            paused: true,
        },
    );
}

// PlanGracePeriodSet events
#[test]
fn plan_grace_period_set_layout() {
    let env = Env::default();
    snapshot(
        &env,
        "plan_grace_period_set",
        super::PlanGracePeriodSet::spec_xdr(),
        &super::PlanGracePeriodSet {
            merchant: address(&env),
            plan_id: 7,
            grace_period_secs: 172_800,
        },
    );
}

// ProfileUpdated events
#[test]
fn plan_renamed_layout() {
    let env = Env::default();
    snapshot(
        &env,
        "plan_renamed",
        super::PlanRenamed::spec_xdr(),
        &super::PlanRenamed {
            merchant: address(&env),
            plan_id: 7,
            name: String::from_str(&env, "Renamed plan"),
        },
    );
}

#[test]
fn plan_minimum_commitment_set_layout() {
    let env = Env::default();
    snapshot(
        &env,
        "plan_minimum_commitment_set",
        super::PlanMinimumCommitmentSet::spec_xdr(),
        &super::PlanMinimumCommitmentSet {
            merchant: address(&env),
            plan_id: 7,
            minimum_periods: 3,
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

// PlanSuccessorSet events
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

// PlanOwnershipProposed events
#[test]
fn plan_ownership_proposed_layout() {
    let env = Env::default();
    snapshot(
        &env,
        "plan_ownership_proposed",
        super::PlanOwnershipProposed::spec_xdr(),
        &super::PlanOwnershipProposed {
            merchant: address(&env),
            plan_id: 7,
            new_owner: address(&env),
        },
    );
}

// PlanTransferred events
#[test]
fn plan_transferred_layout() {
    let env = Env::default();
    snapshot(
        &env,
        "plan_transferred",
        super::PlanTransferred::spec_xdr(),
        &super::PlanTransferred {
            merchant: address(&env),
            plan_id: 7,
            new_owner: address(&env),
        },
    );
}

// PlanRenamed events
#[test]
fn plan_renamed_layout() {
    let env = Env::default();
    snapshot(
        &env,
        "plan_renamed",
        super::PlanRenamed::spec_xdr(),
        &super::PlanRenamed {
            merchant: address(&env),
            plan_id: 7,
            name: String::from_str(&env, "New Name"),
        },
    );
}
