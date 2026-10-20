//! Pins Q18's preregistration (`docs/roadmap/prereg/Q18.json`, Q18.6): the
//! board's minimums as hard floors (Q18.2-Q18.5), measured as their cost
//! against main's behaviour on the four rows whose baseline breaks them
//! (pair `before`, `mp1`), and strict transactional rip-up against Java's
//! rip-up with the floors in both arms on Q11's eligible rows (pair `mp1`,
//! `strict`). Registered 2026-10-20 before any measured number (Q18.1's
//! session analysis and Q18.2's spot checks are not measurements). A later
//! change goes in its `amendments` with the reason and updates the hash
//! below in the same change.
use std::path::{Path, PathBuf};

use freerouting_dev::gate_prereg::Prereg;
use freerouting_dev::json_read::Json;

const FILE: &str = "docs/roadmap/prereg/Q18.json";
const SET: &str = "tools/bench/sets/q18-board-minimums.txt";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn text() -> String {
    std::fs::read_to_string(repo().join(FILE)).unwrap()
}

fn registration() -> Prereg {
    Prereg::parse(&text()).unwrap_or_else(|e| panic!("Q18 registration is invalid: {e:?}"))
}

#[test]
fn the_q18_registration_is_valid_and_names_its_arms_and_boards() {
    let prereg = registration();
    assert_eq!(prereg.id, "Q18.6-board-minimums");
    assert_eq!(prereg.names.base, "mp1");
    assert_eq!(prereg.names.cand, "strict");
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    assert_eq!(arms, ["before", "mp1", "strict"]);
    assert_eq!(prereg.set.as_deref(), Some(SET));
    assert_eq!(prereg.boards.len(), 12);
    let metrics: Vec<&str> = prereg.specs.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        &metrics[..4],
        [
            "via_in_pad",
            "below_minimum",
            "edge_clearance",
            "hole_clearance"
        ]
    );
    let set = std::fs::read_to_string(repo().join(SET)).unwrap();
    for (path, sha) in &prereg.boards {
        assert!(set.contains(path.as_str()), "{path} missing from the set");
        let Ok(bytes) = std::fs::read(repo().join(path)) else {
            assert!(path.starts_with("target/corpus"), "{path} is missing");
            continue;
        };
        assert_eq!(
            &freerouting::quality::feasibility::report::sha256_hex(&bytes),
            sha,
            "{path}"
        );
    }
}

/// Every arm command names the settings the arm registers, and the set's row
/// flags are settings.
#[test]
fn the_q18_commands_and_row_flags_are_settings() {
    let json = Json::parse(&text()).unwrap();
    assert_eq!(json.path("rule").and_then(Json::as_str), Some("totals"));
    let Some(Json::Object(arms)) = json.path("arms") else {
        panic!("arms")
    };
    for (name, arm) in arms {
        let command = arm.get("command").and_then(Json::as_str).expect("command");
        assert!(command.contains(" -mp 1 "), "{name}");
        let Some(Json::Object(settings)) = arm.get("settings") else {
            panic!("{name}: settings")
        };
        for (key, value) in settings {
            let value = value.as_str().expect("a string value");
            assert!(command.contains(&format!("--{key}={value}")), "{name}");
            let path = key.strip_prefix("router.").expect("a router key");
            assert!(
                freerouting::settings::fields::field_by_path(path).is_some(),
                "{name}: {key}"
            );
        }
    }
    let set = std::fs::read_to_string(repo().join(SET)).unwrap();
    for line in set.lines().filter(|l| !l.trim_start().starts_with('#')) {
        for flag in line.split_whitespace().skip(1) {
            let key = flag
                .strip_prefix("--router.")
                .and_then(|f| f.split('=').next())
                .unwrap_or_else(|| panic!("{flag}"));
            assert!(
                freerouting::settings::fields::field_by_path(key).is_some(),
                "{flag}"
            );
        }
    }
}

/// The pairs, Q11's eligible rows for pair B and the four pair-A rows.
#[test]
fn the_q18_pairs_and_rows() {
    let json = Json::parse(&text()).unwrap();
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
    assert_eq!(pairs, [["before", "mp1"], ["mp1", "strict"]]);
    let prereg = registration();
    for key in ["eligible", "pair_a_rows"] {
        let Some(Json::Array(rows)) = json.path(key) else {
            panic!("{key}")
        };
        assert_eq!(rows.len(), if key == "eligible" { 11 } else { 4 }, "{key}");
        for row in rows {
            let row = row.as_str().unwrap();
            assert!(prereg.boards.iter().any(|(p, _)| p == row), "{row}");
        }
    }
}

#[test]
fn the_q18_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "00dfd14db566e9ecc6b423b730ed1b79b43c7d633494870b8152894a447fc3f2"
    );
}

/// Amendment 2: d3c-0496, a KiCad 4 board without its own `.kicad_pro`, is
/// reported as information, not judged.
#[test]
fn the_q18_kicad_4_row_is_information_only() {
    let json = Json::parse(&text()).unwrap();
    let Some(Json::Array(rows)) = json.path("information_rows") else {
        panic!("information_rows")
    };
    let rows: Vec<&str> = rows.iter().filter_map(Json::as_str).collect();
    assert_eq!(
        rows,
        ["target/corpus-d3c/prepped/d3c-0496-kitspace-f-91w.dsn"]
    );
    let Some(Json::Array(pair_a)) = json.path("pair_a_rows") else {
        panic!("pair_a_rows")
    };
    assert!(pair_a.iter().all(|r| r.as_str() != Some(rows[0])));
}
