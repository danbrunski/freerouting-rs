//! Pins N05.3's neck-down registration (`docs/roadmap/prereg/N05-neck-down.json`):
//! `router.neck_width_um = 300` (Java's necked retry, the only neck-down built)
//! with the Board Setup class widths (arm B) against class narrowing (arm A),
//! judged by N02's totals rule with `accept: no-regress` plus a width clause.
//!
//! Registered 2026-10-18 before any routing number as
//! bd0c078410c954a3ae038d0168bdc538d52e217c899520e0200fd90ae5b890a7. A later
//! change goes in the file's `amendments` with the reason and updates the
//! hash here in the same change.
//!
//! The KiCad demo rows are outside the repository (their hashes, of the
//! stripped boards, are K09.json's and are checked by the run); the canary
//! rows' inputs, both the narrowed and the Board Setup DSN, are checked here.
use std::path::{Path, PathBuf};

use freerouting_dev::gate_prereg::Prereg;
use freerouting_dev::json_read::Json;

const FILE: &str = "docs/roadmap/prereg/N05-neck-down.json";
const SET: &str = "tools/bench/sets/n05-neck-down.txt";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn registration() -> Prereg {
    let text = std::fs::read_to_string(repo().join(FILE)).unwrap();
    Prereg::parse(&text).unwrap_or_else(|e| panic!("{FILE} is invalid: {e:?}"))
}

fn sha256(path: &str) -> String {
    let bytes = std::fs::read(repo().join(path)).unwrap_or_else(|e| panic!("{path}: {e}"));
    freerouting::quality::feasibility::report::sha256_hex(&bytes)
}

#[test]
fn the_neck_down_registration_is_valid() {
    let prereg = registration();
    assert_eq!(prereg.id, "N05.3-neck-down");
    assert_eq!(prereg.names.base, "A");
    assert_eq!(prereg.names.cand, "B");
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    assert_eq!(arms, ["A", "B", "plain"]);
    assert_eq!(prereg.set.as_deref(), Some(SET));
    assert_eq!(prereg.boards.len(), 5);
    let metrics: Vec<&str> = prereg.specs.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        &metrics[..7],
        [
            "via_in_pad",
            "below_minimum",
            "power_layer_traces",
            "clearance_errors",
            "drc_real",
            "inner_tracks",
            "kicad_unconnected"
        ]
    );
}

/// Every canary row's narrowed and Board Setup inputs are in the set file and
/// hash as registered; the demo rows are named in the set file.
#[test]
fn the_registered_inputs_hash_as_registered() {
    let text = std::fs::read_to_string(repo().join(FILE)).unwrap();
    let json = Json::parse(&text).unwrap();
    let Some(Json::Object(rows)) = json.path("boards.rows") else {
        panic!("boards.rows")
    };
    let set = std::fs::read_to_string(repo().join(SET)).unwrap();
    for (row, spec) in rows {
        let input = spec.get("input").and_then(Json::as_str).expect("input");
        assert!(set.contains(input), "{input} missing from {SET}");
        if input.starts_with('/') {
            continue;
        }
        for (key, files) in [
            ("input", "boards.files"),
            ("input_board_setup", "boards.board_setup_files"),
        ] {
            let path = spec.get(key).and_then(Json::as_str).expect(key);
            assert!(set.contains(path), "{path} missing from {SET}");
            let want = json
                .path(files)
                .and_then(|f| f.get(row))
                .and_then(Json::as_str)
                .unwrap_or_else(|| panic!("{files}.{row}"));
            assert_eq!(sha256(path), want, "{row} {key}");
        }
    }
}

/// The Board Setup DSN of the 9511d9a row differs from the fixture only in
/// the four class widths the fixture narrowed.
#[test]
fn the_board_setup_dsn_differs_only_in_four_class_widths() {
    let narrow =
        std::fs::read_to_string(repo().join("fixtures/canary-pwr/canary-pwr-9511d9a.dsn")).unwrap();
    let wide = std::fs::read_to_string(repo().join(
        "crates/freerouting/tests/data/canary-pwr-n05/canary-pwr-9511d9a-board-setup-widths.dsn",
    ))
    .unwrap();
    let a: Vec<&str> = narrow.lines().collect();
    let b: Vec<&str> = wide.lines().collect();
    assert_eq!(a.len(), b.len());
    let changed: Vec<(&str, &str)> = a
        .iter()
        .zip(&b)
        .filter(|(x, y)| x != y)
        .map(|(x, y)| (x.trim(), y.trim()))
        .collect();
    assert_eq!(
        changed,
        [
            ("(width 300)", "(width 1500)"),
            ("(width 300)", "(width 500)"),
            ("(width 400)", "(width 500)"),
            ("(width 300)", "(width 1500)")
        ]
    );
}

#[test]
fn the_neck_down_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "bd0c078410c954a3ae038d0168bdc538d52e217c899520e0200fd90ae5b890a7"
    );
}
