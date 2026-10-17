//! Pins Q02's preregistration (`docs/roadmap/prereg/Q02.json`, Q02.6): the
//! bounded neck against Java's room-local neck-down. Registered 2026-10-17 as
//! bd65dddcd71428197506b47e5eb8963696a31c772db9c7c18ad30c0eaf1e3e89,
//! before any number; a later change goes in its `amendments` with the
//! reason and updates the hash below in the same commit.
use std::path::{Path, PathBuf};

use freerouting_dev::gate_prereg::Prereg;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn registration() -> Prereg {
    let text = std::fs::read_to_string(repo().join("docs/roadmap/prereg/Q02.json")).unwrap();
    Prereg::parse(&text).unwrap_or_else(|e| panic!("Q02 registration is invalid: {e:?}"))
}

#[test]
fn the_q02_registration_is_valid_and_names_its_arms_and_boards() {
    let prereg = registration();
    assert_eq!(prereg.id, "Q02.6-bounded-neck");
    assert_eq!(prereg.names.base, "java-default");
    assert_eq!(prereg.names.cand, "bounded-neck");
    assert_eq!(
        prereg.set.as_deref(),
        Some("tools/bench/sets/q02-wide-nets.txt")
    );
    assert_eq!(prereg.boards.len(), 9);
    let metrics: Vec<&str> = prereg.specs.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        &metrics[..5],
        [
            "via_in_pad",
            "below_minimum",
            "drc_real",
            "unrouted",
            "median_under_width_mm"
        ]
    );
    // Every registered input file is in the set file and hashes as registered.
    let set = std::fs::read_to_string(repo().join("tools/bench/sets/q02-wide-nets.txt")).unwrap();
    for path in prereg.boards.keys() {
        assert!(set.contains(path.as_str()), "{path} missing from the set");
        assert!(repo().join(path).is_file(), "{path}");
    }
}

#[test]
fn the_q02_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "bd65dddcd71428197506b47e5eb8963696a31c772db9c7c18ad30c0eaf1e3e89"
    );
}
