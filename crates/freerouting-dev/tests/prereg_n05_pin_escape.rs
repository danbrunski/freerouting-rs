//! Pins N05.3's pin-escape registration (`docs/roadmap/prereg/N05-pin-escape.json`):
//! `router.fanout.pin_escape = straight` against off under profile `enhanced`, judged by
//! N02's totals rule with `accept: no-regress` and a lower total (its `keep_rule`), on seven
//! KiCad projects that each have a part the stage acts on. Registered 2026-10-20 before any
//! route of either arm. A later change goes in its `amendments` with the reason and updates
//! the hash below in the same change.
//!
//! The rows live under the main checkout's `target/` (not in git); a missing file is skipped,
//! a present one must hash as registered.
use std::path::{Path, PathBuf};

use freerouting_dev::gate::Role;
use freerouting_dev::gate_prereg::Prereg;
use freerouting_dev::json_read::Json;

const FILE: &str = "docs/roadmap/prereg/N05-pin-escape.json";
const SET: &str = "tools/bench/sets/n05-pin-escape.txt";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn text() -> String {
    std::fs::read_to_string(repo().join(FILE)).unwrap()
}

fn registration() -> Prereg {
    Prereg::parse(&text()).unwrap_or_else(|e| panic!("{FILE} is invalid: {e:?}"))
}

#[test]
fn the_registration_is_valid_and_names_its_arms_and_metrics() {
    let prereg = registration();
    assert_eq!(prereg.id, "N05.3-pin-escape");
    assert_eq!(prereg.names.base, "enhanced");
    assert_eq!(prereg.names.cand, "enhanced+pin_escape");
    assert_eq!(prereg.set.as_deref(), Some(SET));
    assert_eq!(prereg.boards.len(), 7);
    let roles: Vec<(&str, Role)> = prereg
        .specs
        .iter()
        .map(|s| (s.name.as_str(), s.role))
        .collect();
    assert_eq!(
        &roles[..4],
        [
            ("via_in_pad", Role::Hard),
            ("below_minimum", Role::Hard),
            ("manufacturing_breaches", Role::Hard),
            ("kicad_unconnected", Role::Score),
        ]
    );
}

/// Every row's input is in the set file, and each noise band is the registered one:
/// max(2, ceil(5 % of the row's connections)), 10 for canary-pwr.
#[test]
fn the_rows_match_the_set_and_the_noise_rule() {
    let json = Json::parse(&text()).unwrap();
    let Some(Json::Object(rows)) = json.path("boards.rows") else {
        panic!("boards.rows")
    };
    let set = std::fs::read_to_string(repo().join(SET)).unwrap();
    let set_rows: Vec<&str> = set
        .lines()
        .map(|l| l.split('#').next().unwrap().trim())
        .filter(|l| !l.is_empty())
        .collect();
    assert_eq!(set_rows.len(), rows.len());
    for (row, spec) in rows {
        let input = spec.get("input").and_then(Json::as_str).expect("input");
        assert!(set_rows.contains(&input), "{input} missing from {SET}");
        let connections = match spec.get("connections") {
            Some(Json::Number(n)) => *n,
            other => panic!("{row}: connections {other:?}"),
        };
        let band = match json.path(&format!("totals.noise.{row}")) {
            Some(Json::Number(n)) => *n,
            other => panic!("{row}: noise {other:?}"),
        };
        let want = if row == "canary-pwr" {
            10.0
        } else {
            (connections * 0.05).ceil().max(2.0)
        };
        assert_eq!(band, want, "{row}");
        let Ok(bytes) = std::fs::read(repo().join(input)) else {
            continue;
        };
        let sha = json
            .path(&format!("boards.files.{row}"))
            .and_then(Json::as_str)
            .unwrap_or_else(|| panic!("boards.files.{row}"));
        assert_eq!(
            freerouting::quality::feasibility::report::sha256_hex(&bytes),
            sha,
            "{row}"
        );
    }
}

/// The two arms differ by the plugin's checkbox and nothing else, and the setting the
/// candidate registers is a settings field.
#[test]
fn the_arms_differ_by_the_pin_escape_only() {
    let json = Json::parse(&text()).unwrap();
    assert_eq!(json.path("rule").and_then(Json::as_str), Some("totals"));
    assert_eq!(
        json.path("totals.accept").and_then(Json::as_str),
        Some("no-regress")
    );
    let options = |arm: &str| -> Vec<(String, String)> {
        let Some(Json::Object(o)) = json.path(&format!("arms.{arm}.options")) else {
            panic!("{arm}: options")
        };
        o.iter().map(|(k, v)| (k.clone(), show(v))).collect()
    };
    let off = options("enhanced");
    let mut straight = options("enhanced+pin_escape");
    straight.retain(|(k, _)| k != "pin_escape");
    assert_eq!(off, straight);
    assert!(off.contains(&("profile".to_string(), "enhanced".to_string())));
    let Some(Json::Object(settings)) = json.path("arms.enhanced+pin_escape.settings") else {
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
        .path("arms.enhanced.command")
        .and_then(Json::as_str)
        .expect("command");
    assert!(command.contains("--export plugin") && command.contains(" -mp 1 "));
}

fn show(v: &Json) -> String {
    match v {
        Json::String(s) => s.clone(),
        Json::Number(n) => format!("{n}"),
        Json::Bool(b) => b.to_string(),
        other => format!("{other:?}"),
    }
}

#[test]
fn the_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "8ebdf20320f6ad23ee830c52e9969defa35b36311fb8694f136d4fc8beacd289"
    );
}
