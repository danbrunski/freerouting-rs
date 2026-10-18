//! Pins N04's preregistration (N04.3): `router.pad_contacts = inside_pad`
//! against `center` (Java's rule), judged by N02's totals rule with
//! `accept: no-regress` (a correctness fix), on the KiCad demos and corpus
//! boards with existing tracks plus canary-pwr, graded with the tracks kept.
//!
//! Registered 2026-10-18 before any graded number as
//! `docs/roadmap/prereg/N04.json`, and amended twice the same day, still before
//! any graded number (its `amendments`): the rule was widened to end-cap
//! overlap, then to T junctions (a trace end whose cap overlaps a same-net
//! trace's copper, N04.4). It is now sha256
//! f96e590d3217af1f9cb8e14b108ad0d8596d1f2dcc14d3d63f97f2c4aac00da1. The
//! numbers seen before (N04.1's correctness test on ecc83) are named in its
//! `seen_before_registration`. A later change goes in the file's
//! `amendments` with the reason and updates the hash here in the same
//! commit.
use std::path::{Path, PathBuf};

use freerouting_dev::gate_prereg::Prereg;
use freerouting_dev::json_read::Json;

const REG: &str = "docs/roadmap/prereg/N04.json";
const SET: &str = "tools/bench/sets/n04-pad-contacts.txt";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn registration() -> (Prereg, Json) {
    let text = std::fs::read_to_string(repo().join(REG)).unwrap();
    let prereg = Prereg::parse(&text).unwrap_or_else(|e| panic!("{REG} is invalid: {e:?}"));
    (prereg, Json::parse(&text).unwrap())
}

#[test]
fn the_pad_contacts_registration_is_valid() {
    let (prereg, _) = registration();
    assert_eq!(prereg.id, "N04.3-pad-contacts-inside-pad");
    assert_eq!(prereg.names.base, "center");
    assert_eq!(prereg.names.cand, "inside_pad");
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    assert_eq!(arms, ["center", "inside_pad"]);
    assert_eq!(prereg.set.as_deref(), Some(SET));
    assert_eq!(prereg.boards.len(), 43);
    let metrics: Vec<&str> = prereg.specs.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        &metrics[..4],
        [
            "via_in_pad",
            "below_minimum",
            "drc_real",
            "kicad_unconnected"
        ]
    );
}

/// Every row's input is in the set file and, when present on this machine
/// (the demos of a KiCad install, the corpus checkout, canary), hashes as
/// registered.
#[test]
fn the_pad_contacts_inputs_are_the_registered_ones() {
    let (_, json) = registration();
    let Some(Json::Object(rows)) = json.path("boards.rows") else {
        panic!("{REG}: boards.rows")
    };
    let files = json.path("boards.files").expect("boards.files");
    let set: Vec<String> = std::fs::read_to_string(repo().join(SET))
        .unwrap()
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(str::to_string)
        .collect();
    assert_eq!(set.len(), rows.len(), "{SET} lists every row once");
    for (row, spec) in rows {
        let path = spec.get("input").and_then(Json::as_str).expect("input");
        assert!(set.iter().any(|l| l == path), "{path} missing from {SET}");
        let sha = files
            .get(row)
            .and_then(Json::as_str)
            .expect("a hash per row");
        let full = if path.starts_with('/') {
            PathBuf::from(path)
        } else {
            repo().join(path)
        };
        let Ok(bytes) = std::fs::read(&full) else {
            assert!(
                ["/usr/share/kicad/demos/", "target/corpus/", "../canary/"]
                    .iter()
                    .any(|p| path.starts_with(p)),
                "{path} is missing"
            );
            continue;
        };
        assert_eq!(
            freerouting::quality::feasibility::report::sha256_hex(&bytes),
            sha,
            "{REG}: {row}"
        );
    }
}

#[test]
fn the_n04_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().0.sha256,
        "f96e590d3217af1f9cb8e14b108ad0d8596d1f2dcc14d3d63f97f2c4aac00da1"
    );
}
