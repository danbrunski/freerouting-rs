//! Pins Q12's preregistration (`docs/roadmap/prereg/Q12.json`, Q12.5): the
//! pass-1 portfolio (`router.autorouter.portfolio = 4`), with two
//! squeaky-wheel restarts, and composed with Q11's transactional pass 1 and
//! Q10's sweep, against the default passes (`mpdef`, the work reference) and
//! pass 1 alone (`mp1`), judged by N02's totals rule. Registered 2026-10-18
//! before any measured number (the spot checks of Q12.1-Q12.3's done notes
//! run the portfolio arm alone, for crashes and determinism) as
//! 8467d77dcd989648691a0a29a4095aceb650bb6e3fcd2a20d707da2a2e0b5a98. A
//! later change (the `eligible` list, Q11.6's rule for the composed arm)
//! goes in its `amendments` with the reason and updates the hash below in
//! the same change.
use std::path::{Path, PathBuf};

use freerouting_dev::gate_prereg::Prereg;
use freerouting_dev::json_read::Json;

const FILE: &str = "docs/roadmap/prereg/Q12.json";
const SET: &str = "tools/bench/sets/q10-sweep.txt";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn text() -> String {
    std::fs::read_to_string(repo().join(FILE)).unwrap()
}

fn registration() -> Prereg {
    Prereg::parse(&text()).unwrap_or_else(|e| panic!("Q12 registration is invalid: {e:?}"))
}

#[test]
fn the_q12_registration_is_valid_and_names_its_arms_and_boards() {
    let prereg = registration();
    assert_eq!(prereg.id, "Q12.5-pass1-portfolio");
    assert_eq!(prereg.names.base, "mpdef");
    assert_eq!(prereg.names.cand, "portfolio4");
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    assert_eq!(
        arms,
        [
            "mp1",
            "mpdef",
            "portfolio4",
            "portfolio4+Q10+Q11",
            "portfolio4+restart2"
        ]
    );
    assert_eq!(prereg.set.as_deref(), Some(SET));
    assert_eq!(prereg.boards.len(), 30);
    let metrics: Vec<&str> = prereg.specs.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        &metrics[..6],
        [
            "via_in_pad",
            "below_minimum",
            "drc_real",
            "pour_lost_pins",
            "kicad_unconnected",
            "unrouted"
        ]
    );
    // Every registered input is in the set file; those in the repository
    // hash as registered (the corpus rows are skipped while missing).
    let set = std::fs::read_to_string(repo().join(SET)).unwrap();
    for (path, sha) in &prereg.boards {
        assert!(set.contains(path.as_str()), "{path} missing from the set");
        let Ok(bytes) = std::fs::read(repo().join(path)) else {
            assert!(path.starts_with("target/corpus/"), "{path} is missing");
            continue;
        };
        assert_eq!(
            &freerouting::quality::feasibility::report::sha256_hex(&bytes),
            sha,
            "{path}"
        );
    }
}

/// The totals block is N02's, and every arm command names the settings the
/// arm registers.
#[test]
fn the_q12_registration_uses_the_totals_rule_and_its_settings_exist() {
    let json = Json::parse(&text()).unwrap();
    assert_eq!(json.path("rule").and_then(Json::as_str), Some("totals"));
    assert_eq!(
        json.path("totals.accept").and_then(Json::as_str),
        Some("improve")
    );
    let Some(Json::Object(arms)) = json.path("arms") else {
        panic!("arms")
    };
    for (name, arm) in arms {
        let command = arm.get("command").and_then(Json::as_str).expect("command");
        let Some(Json::Object(settings)) = arm.get("settings") else {
            panic!("{name}: settings")
        };
        for (key, value) in settings {
            let value = value.as_str().expect("a string value");
            assert!(
                command.contains(&format!("--{key}={value}")),
                "{name}: {key}={value} not in {command}"
            );
            let path = key.strip_prefix("router.").expect("a router key");
            assert!(
                freerouting::settings::fields::field_by_path(path).is_some(),
                "{name}: {key} is not a setting"
            );
        }
    }
}

#[test]
fn the_q12_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "8467d77dcd989648691a0a29a4095aceb650bb6e3fcd2a20d707da2a2e0b5a98"
    );
}
