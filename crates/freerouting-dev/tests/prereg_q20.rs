//! Pins Q20's preregistration (`docs/roadmap/prereg/Q20.json`, Q20.5): the
//! cost of the output check (`router.output_check`, `off` against `repair`)
//! on Q18's KiCad 6+ rows and canary-pwr. Registered 2026-10-20 before any
//! measured number (Q20.1's comparison with KiCad on Q18.6's kept sessions is
//! not a run of these arms). A later change goes in its `amendments` with the
//! reason and updates the hash below in the same change.
use std::path::{Path, PathBuf};

use freerouting_dev::gate_prereg::Prereg;
use freerouting_dev::json_read::Json;

const FILE: &str = "docs/roadmap/prereg/Q20.json";
const SET: &str = "tools/bench/sets/q20-output-check.txt";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn text() -> String {
    std::fs::read_to_string(repo().join(FILE)).unwrap()
}

fn registration() -> Prereg {
    Prereg::parse(&text()).unwrap_or_else(|e| panic!("Q20 registration is invalid: {e:?}"))
}

#[test]
fn the_q20_registration_is_valid_and_names_its_arms_and_boards() {
    let prereg = registration();
    assert_eq!(prereg.id, "Q20.6-output-check-cost");
    assert_eq!(prereg.names.base, "off");
    assert_eq!(prereg.names.cand, "repair");
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    assert_eq!(arms, ["off", "repair"]);
    assert_eq!(prereg.set.as_deref(), Some(SET));
    assert_eq!(prereg.boards.len(), 6);
    let metrics: Vec<&str> = prereg.specs.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        &metrics[..3],
        ["via_in_pad", "check_remaining", "kicad_rule_breaches"]
    );
    let set = std::fs::read_to_string(repo().join(SET)).unwrap();
    let rows: Vec<&str> = set
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| l.split_whitespace().next())
        .collect();
    assert_eq!(rows.len(), 6);
    for (path, sha) in &prereg.boards {
        assert!(rows.contains(&path.as_str()), "{path} missing from the set");
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

/// The arms differ only in `router.output_check`, and the set's row flags
/// are Q18's.
#[test]
fn the_q20_arms_differ_only_in_the_check() {
    let json = Json::parse(&text()).unwrap();
    let Some(Json::Object(arms)) = json.path("arms") else {
        panic!("arms")
    };
    let command = |name: &str| {
        arms[name]
            .get("command")
            .and_then(Json::as_str)
            .expect("command")
            .to_string()
    };
    assert_eq!(
        command("off").replace("output_check=off", "output_check=repair"),
        command("repair")
    );
    let q18 =
        std::fs::read_to_string(repo().join("tools/bench/sets/q18-board-minimums.txt")).unwrap();
    let set = std::fs::read_to_string(repo().join(SET)).unwrap();
    for row in set
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
    {
        assert!(q18.lines().any(|l| l == row), "{row} is not Q18's row");
    }
}

#[test]
fn the_q20_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "246ab47e0365ef9158d9cc877c01c003166693e79061c3ab188ea50b99dfa167"
    );
}
