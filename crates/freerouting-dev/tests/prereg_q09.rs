//! Pins Q09's preregistration (`tools/ab/q09-trace-only-keepouts.json`, Q09.5):
//! trace-only keepouts against Java's reading of `wire_keepout`. Registered
//! 2026-10-17 as
//! d1283eb9df39d66f30a75e126823028b1e6b45e298de5f8912900a6d416db01b,
//! before any number; a later change goes in its `amendments` with the
//! reason and updates the hash below in the same commit.
use std::path::{Path, PathBuf};

use freerouting_dev::gate_prereg::Prereg;

const FILE: &str = "tools/ab/q09-trace-only-keepouts.json";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn registration() -> Prereg {
    let text = std::fs::read_to_string(repo().join(FILE)).unwrap();
    Prereg::parse(&text).unwrap_or_else(|e| panic!("Q09 registration is invalid: {e:?}"))
}

#[test]
fn the_q09_registration_is_valid_and_names_its_arms_and_boards() {
    let prereg = registration();
    assert_eq!(prereg.id, "Q09.5-trace-only-keepouts");
    assert_eq!(prereg.names.base, "keepout");
    assert_eq!(prereg.names.cand, "traces_only");
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    assert_eq!(arms, ["keepout", "plane_traces", "traces_only"]);
    assert_eq!(
        prereg.set.as_deref(),
        Some("tools/bench/sets/q09-wire-keepouts.txt")
    );
    assert_eq!(prereg.boards.len(), 3);
    let metrics: Vec<&str> = prereg.specs.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        &metrics[..5],
        [
            "via_in_pad",
            "below_minimum",
            "drc_real",
            "unrouted",
            "foreign_island_mm"
        ]
    );
    // Every registered input file is in the set file and hashes as registered.
    let set =
        std::fs::read_to_string(repo().join("tools/bench/sets/q09-wire-keepouts.txt")).unwrap();
    for (path, sha) in &prereg.boards {
        assert!(set.contains(path.as_str()), "{path} missing from the set");
        let bytes = std::fs::read(repo().join(path)).unwrap_or_else(|e| panic!("{path}: {e}"));
        assert_eq!(
            &freerouting::quality::feasibility::report::sha256_hex(&bytes),
            sha,
            "{path}"
        );
    }
}

#[test]
fn the_q09_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "d1283eb9df39d66f30a75e126823028b1e6b45e298de5f8912900a6d416db01b"
    );
}
