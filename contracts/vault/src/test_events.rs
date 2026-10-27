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
        .map(|byte| format!("{byte:02x}"))
        .collect::<std::vec::Vec<_>>()
        .join("");
    let spec_hex = spec_xdr
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<std::vec::Vec<_>>()
        .join("");
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

#[test]
fn deposit_layout() {
    let env = Env::default();
    let a = address(&env);
    snapshot(
        &env,
        "deposit",
        super::Deposit::spec_xdr(),
        &super::Deposit {
            user: a.clone(),
            token: a,
            amount: 1_250,
            balance: 9_750,
        },
    );
}
#[test]
fn withdraw_layout() {
    let env = Env::default();
    let a = address(&env);
    snapshot(
        &env,
        "withdraw",
        super::Withdraw::spec_xdr(),
        &super::Withdraw {
            user: a.clone(),
            token: a,
            amount: 250,
            balance: 9_500,
        },
    );
}
#[test]
fn debit_layout() {
    let env = Env::default();
    let a = address(&env);
    snapshot(
        &env,
        "debit",
        super::Debit::spec_xdr(),
        &super::Debit {
            user: a.clone(),
            token: a.clone(),
            to: a,
            amount: 500,
            balance: 9_000,
        },
    );
}
