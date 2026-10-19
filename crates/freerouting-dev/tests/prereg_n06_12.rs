//! Pins N06.12's registration (`docs/roadmap/prereg/N06.12-ripup-pass.json`): the (c)
//! rows that are measured, `router.java_quirks.ripup_pass_no_unset` (the verdict pair,
//! N02's totals rule with `accept: improve`) and the reported any-angle arm
//! `any_angle_skip_length_squared`, each alone against Java's behaviour.
//!
//! Registered 2026-10-18 before any routing number. A later change goes in the
//! file's `amendments` with the reason and updates the hash here in the same
//! change.
use std::path::{Path, PathBuf};

use freerouting_dev::gate_prereg::Prereg;
use freerouting_dev::json_read::Json;

const FILE: &str = "docs/roadmap/prereg/N06.12-ripup-pass.json";
const SET: &str = "tools/bench/sets/n06-ripup-pass.txt";
const SWITCHES: [&str; 2] = ["ripup_pass_no_unset", "any_angle_skip_length_squared"];

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn json() -> Json {
    Json::parse(&std::fs::read_to_string(repo().join(FILE)).unwrap()).unwrap()
}

fn registration() -> Prereg {
    let text = std::fs::read_to_string(repo().join(FILE)).unwrap();
    Prereg::parse(&text).unwrap_or_else(|e| panic!("{FILE} is invalid: {e:?}"))
}

#[test]
fn the_ripup_pass_registration_is_valid() {
    let prereg = registration();
    assert_eq!(prereg.id, "N06.12-ripup-pass");
    assert_eq!(prereg.names.base, "baseline");
    assert_eq!(prereg.names.cand, "ripup_pass_no_unset");
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    assert_eq!(
        arms,
        [
            "any_angle_skip_length_squared",
            "baseline",
            "ripup_pass_no_unset"
        ]
    );
    assert_eq!(prereg.set.as_deref(), Some(SET));
    // The DSN rows plus the plugin row (amendment 1: `canary-pwr-plugin`, hashed as
    // its exported DSN, routed outside n06_run.py), less Issue026-J2_reference-any
    // and Issue143-rpi_splitter-any (amendments 2 and 3: dropped for memory before
    // any number of them).
    assert_eq!(prereg.boards.len(), 7);
    assert!(prereg.totals_registered);
    assert_eq!(
        json().path("totals.accept").and_then(Json::as_str),
        Some("improve")
    );
    let metrics: Vec<&str> = prereg.specs.iter().map(|s| s.name.as_str()).collect();
    for name in [
        "unrouted",
        "clearance_violations",
        "kicad_unconnected",
        "session_differences",
        "ripup_random_pass_connections",
        "ripup_random_factor_draws",
        "any_angle_skip_length_flips",
    ] {
        assert!(metrics.contains(&name), "{name}");
    }
    // n06_run.py runs the arms in file order and compares every arm with the
    // baseline, so the baseline comes first.
    let text = std::fs::read_to_string(repo().join(FILE)).unwrap();
    let arms = &text[text.find("\"arms\": {").expect("arms")..];
    let first = arms[9..].trim_start().split('"').nth(1);
    assert_eq!(first, Some("baseline"));
}

/// Each arm gives both switches explicitly, `false` exactly for its own, on the
/// command line and in its settings.
#[test]
fn each_arm_sets_only_its_switch() {
    let json = json();
    let expect: [(&str, &[&str]); 3] = [
        ("baseline", &[]),
        ("ripup_pass_no_unset", &SWITCHES[..1]),
        ("any_angle_skip_length_squared", &SWITCHES[1..]),
    ];
    for (arm, off) in expect {
        let spec = json.path(&format!("arms.{arm}")).expect(arm);
        let command = spec.get("command").and_then(Json::as_str).expect("command");
        assert_eq!(command.matches("--router.java_quirks.").count(), 2, "{arm}");
        assert!(command.contains("--router.profile=legacy"), "{arm}");
        for id in SWITCHES {
            let value = if off.contains(&id) { "false" } else { "true" };
            assert!(
                command.contains(&format!("--router.java_quirks.{id}={value} "))
                    || command.ends_with(&format!("--router.java_quirks.{id}={value}")),
                "{arm}: {id}={value} in {command}"
            );
        }
        let Some(Json::Object(settings)) = spec.get("settings") else {
            panic!("{arm}: settings")
        };
        assert_eq!(settings.len(), off.len(), "{arm}");
        for id in off {
            let key = format!("router.java_quirks.{id}");
            assert!(settings.iter().any(|(k, _)| *k == key), "{arm}: {key}");
        }
    }
}

/// Every row is in the set file and hashes as registered; the any-angle rows say
/// `(snap_angle none)`.
#[test]
fn the_registered_inputs_hash_as_registered() {
    let json = json();
    let Some(Json::Object(all)) = json.path("boards.files") else {
        panic!("boards.files")
    };
    // The rows routed outside n06_run.py (amendment 1) are the plugin row only; its
    // input is the plugin's export, which is not in the repository.
    let Some(Json::Array(outside)) = json.path("boards.outside_runner") else {
        panic!("boards.outside_runner")
    };
    let outside: Vec<&str> = outside.iter().filter_map(Json::as_str).collect();
    assert_eq!(outside, ["canary-pwr-plugin"]);
    let files: Vec<_> = all
        .iter()
        .filter(|(row, _)| !outside.contains(&row.as_str()))
        .collect();
    let set = std::fs::read_to_string(repo().join(SET)).unwrap();
    let rows: Vec<&str> = set
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    assert_eq!(rows.len(), files.len());
    for &(input, want) in &files {
        assert!(rows.contains(&input.as_str()), "{input} missing from {SET}");
        let bytes = std::fs::read(repo().join(input)).unwrap_or_else(|e| panic!("{input}: {e}"));
        assert_eq!(
            freerouting::quality::feasibility::report::sha256_hex(&bytes),
            want.as_str().unwrap(),
            "{input}"
        );
        if input.ends_with("-any.dsn") {
            assert!(
                String::from_utf8_lossy(&bytes).contains("(snap_angle none)"),
                "{input}"
            );
        }
    }
    // One any-angle row remains after amendments 2 and 3 dropped J2's and rpi_splitter's.
    assert_eq!(rows.iter().filter(|r| r.ends_with("-any.dsn")).count(), 1);
    assert!(
        json.path("boards.dropped").is_some(),
        "amendments 2 and 3 record the dropped rows"
    );
}

#[test]
fn the_ripup_pass_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "f50fe24ea15af1bdad433eedbc7fdc8b37a2e6ba4a14c99e1d57a42561a465b6"
    );
}
