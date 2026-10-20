//! Pins Q21's preregistration (`docs/roadmap/prereg/Q21.json`, Q21.4): the
//! straight pin escape (`router.fanout.pin_escape = straight`) against off,
//! both through the plugin's export and import, on the row whose
//! connections Q18's floor cost and on canary-pwr. Registered 2026-10-20
//! before any route of either arm. A later change goes in its `amendments`
//! with the reason and updates the hash below in the same change.
use std::path::{Path, PathBuf};

use freerouting_dev::gate_prereg::Prereg;
use freerouting_dev::json_read::Json;

const FILE: &str = "docs/roadmap/prereg/Q21.json";
const SET: &str = "tools/bench/sets/q21-pin-escape.txt";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn text() -> String {
    std::fs::read_to_string(repo().join(FILE)).unwrap()
}

fn registration() -> Prereg {
    Prereg::parse(&text()).unwrap_or_else(|e| panic!("Q21 registration is invalid: {e:?}"))
}

#[test]
fn the_q21_registration_is_valid_and_names_its_arms_and_boards() {
    let prereg = registration();
    assert_eq!(prereg.id, "Q21.4-pin-escape");
    assert_eq!(prereg.names.base, "off");
    assert_eq!(prereg.names.cand, "straight");
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    assert_eq!(arms, ["off", "straight"]);
    assert_eq!(prereg.set.as_deref(), Some(SET));
    assert_eq!(prereg.boards.len(), 2);
    let metrics: Vec<&str> = prereg.specs.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        &metrics[..4],
        [
            "via_in_pad",
            "below_minimum",
            "manufacturing_breaches",
            "kicad_unconnected"
        ]
    );
    let set = std::fs::read_to_string(repo().join(SET)).unwrap();
    let rows: Vec<&str> = set
        .lines()
        .filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty())
        .collect();
    assert_eq!(rows.len(), prereg.boards.len());
    for (path, sha) in &prereg.boards {
        assert!(rows.contains(&path.as_str()), "{path} missing from the set");
        // The rows live under the main checkout's `target/` (not in git).
        assert!(path.starts_with("target/"), "{path}");
        let Ok(bytes) = std::fs::read(repo().join(path)) else {
            continue;
        };
        assert_eq!(
            &freerouting::quality::feasibility::report::sha256_hex(&bytes),
            sha,
            "{path}"
        );
    }
}

/// The two arms differ by the plugin's checkbox and nothing else, and the
/// setting the candidate registers is a settings field.
#[test]
fn the_q21_arms_differ_by_the_pin_escape_only() {
    let json = Json::parse(&text()).unwrap();
    assert_eq!(json.path("rule").and_then(Json::as_str), Some("totals"));
    let options = |arm: &str| -> Vec<(String, String)> {
        let Some(Json::Object(o)) = json.path(&format!("arms.{arm}.options")) else {
            panic!("{arm}: options")
        };
        o.iter().map(|(k, v)| (k.clone(), crate_show(v))).collect()
    };
    let off = options("off");
    let mut straight = options("straight");
    straight.retain(|(k, _)| k != "pin_escape");
    assert_eq!(off, straight);
    assert!(off.contains(&("schema".to_string(), "5".to_string())));
    let Some(Json::Object(settings)) = json.path("arms.straight.settings") else {
        panic!("settings")
    };
    let keys: Vec<&str> = settings.keys().map(String::as_str).collect();
    assert_eq!(keys, ["router.fanout.pin_escape"]);
    assert_eq!(
        settings["router.fanout.pin_escape"].as_str(),
        Some("straight")
    );
    assert!(freerouting::settings::fields::field_by_path("fanout.pin_escape").is_some());
    let command = json
        .path("arms.off.command")
        .and_then(Json::as_str)
        .expect("command");
    assert!(command.contains("--export plugin") && command.contains(" -mp 1 "));
}

fn crate_show(v: &Json) -> String {
    match v {
        Json::String(s) => s.clone(),
        Json::Number(n) => format!("{n}"),
        Json::Bool(b) => b.to_string(),
        other => format!("{other:?}"),
    }
}

#[test]
fn the_q21_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "8431ffbc0fb057aa0170e69517817a429312aa44671ac2b5d4ea68c5bb946576"
    );
}
