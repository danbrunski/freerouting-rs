//! Pins Q11's preregistration (`docs/roadmap/prereg/Q11.json`, Q11.6):
//! transactional rip-up (`router.autorouter.connection_router =
//! transactional-ripup`, rules `strict` and `count`, depth 1, rip-count
//! protection) against Java's rip-up in pass 1 and the best start ripup
//! cost of Q11.1's no-code sweep, each also followed by Q10's sweep, judged
//! by N02's totals rule. Registered 2026-10-18 before any measured number
//! (the spot checks of the code's done notes are not measurements) as
//! d1757b3d1093e4c605ba4adfcd18e10ad50b7827b0f1c73e9130cf6f27f6bea1. A
//! later change (Q11.1's `ripcost-best` value and the `eligible` list among
//! them) goes in its `amendments` with the reason and updates the hash below
//! in the same change.
//!
//! Amendment 1 (decided 2026-10-20: measurement runs stay a few minutes, before any
//! registered run): the 15 quick rows of `tools/bench/sets/q10-sweep-quick.txt`, the arms
//! `mp1`, Q11.1's sweep arms, `ripcost-best` and `strict` (the others dropped), rip stats in
//! every command, one KiCad grade per distinct session; the hash below is the amended file's.
use std::path::{Path, PathBuf};

use freerouting_dev::gate_prereg::Prereg;
use freerouting_dev::json_read::Json;

const FILE: &str = "docs/roadmap/prereg/Q11.json";
const SET: &str = "tools/bench/sets/q10-sweep-quick.txt";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn text() -> String {
    std::fs::read_to_string(repo().join(FILE)).unwrap()
}

fn registration() -> Prereg {
    Prereg::parse(&text()).unwrap_or_else(|e| panic!("Q11 registration is invalid: {e:?}"))
}

#[test]
fn the_q11_registration_is_valid_and_names_its_arms_and_boards() {
    let prereg = registration();
    assert_eq!(prereg.id, "Q11.6-transactional-ripup");
    assert_eq!(prereg.names.base, "mp1");
    assert_eq!(prereg.names.cand, "strict");
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    // Amendment 1: two arms where the question allows (mp1 and strict), Q11.1's sweep arms
    // and ripcost-best, which reuses the best sweep arm's runs.
    assert_eq!(
        arms,
        [
            "mp1",
            "ripcost-20000",
            "ripcost-5000",
            "ripcost-best",
            "ripcost-near-off",
            "strict"
        ]
    );
    assert_eq!(prereg.set.as_deref(), Some(SET));
    assert_eq!(prereg.boards.len(), 15);
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
fn the_q11_registration_uses_the_totals_rule_and_its_settings_exist() {
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
fn the_q11_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "9a1c2d795c92c3849b745807debf2585397fad55f9729470e257c9ef4c0e783c"
    );
}

/// Amendment 1: recorded with the rule's reason; the pairs are the two that judge `strict`,
/// and every command carries the report-only rip stats.
#[test]
fn amendment_1_keeps_two_pairs_and_the_quick_rows() {
    let json = Json::parse(&text()).unwrap();
    let Some(Json::Array(amendments)) = json.path("amendments") else {
        panic!("amendments")
    };
    assert_eq!(amendments.len(), 1);
    let reason = amendments[0].get("reason").and_then(Json::as_str).unwrap();
    assert!(reason.starts_with("decided 2026-10-20: measurement runs stay a few minutes"));
    let Some(Json::Array(pairs)) = json.path("pairs") else {
        panic!("pairs")
    };
    let pairs: Vec<Vec<&str>> = pairs
        .iter()
        .map(|p| match p {
            Json::Array(a) => a.iter().filter_map(Json::as_str).collect(),
            _ => panic!("pair"),
        })
        .collect();
    assert_eq!(pairs, [["mp1", "strict"], ["ripcost-best", "strict"]]);
    let Some(Json::Object(arms)) = json.path("arms") else {
        panic!("arms")
    };
    for (name, arm) in arms {
        let command = arm.get("command").and_then(Json::as_str).unwrap();
        assert!(
            command.contains(" -mp 1 --router.special.diagnostics=rip-stats"),
            "{name}"
        );
    }
}
