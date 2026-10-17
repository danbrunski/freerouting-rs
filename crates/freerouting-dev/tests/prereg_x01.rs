//! Pins X01's preregistration (`docs/roadmap/prereg/X01.json`, X01.1): the
//! head-to-head of the Java Freerouting fork against KiCadRoutingTools.
//!
//! The registration is committed before any result. A legitimate change after
//! that goes in its `amendments` list with the reason, and the hash below is
//! updated in the same commit. Registered 2026-10-16 as
//! fd154707107403119cbd3176846131e2eb2fcc2f0bbddebc256074e532bf3085; amendment 1
//! (prepared-input hashes, `python -E -s`, krt-kicad not run) before any engine run;
//! amendment 2 (`-Xmx3g` after the WSL out-of-memory crash; java runs repeated);
//! amendment 3 (after the runs: KRT reached KiCad's exact fill through /usr/bin/python3).
use std::path::{Path, PathBuf};

use freerouting_dev::gate_prereg::Prereg;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn registration() -> Prereg {
    let text = std::fs::read_to_string(repo().join("docs/roadmap/prereg/X01.json")).unwrap();
    Prereg::parse(&text).unwrap_or_else(|e| panic!("X01 registration is invalid: {e:?}"))
}

#[test]
fn the_x01_registration_is_valid_and_names_its_arms_and_boards() {
    let prereg = registration();
    assert_eq!(prereg.id, "X01-krt-head-to-head");
    assert_eq!(prereg.names.base, "java");
    assert_eq!(prereg.names.cand, "krt");
    let boards: Vec<&str> = prereg.boards.keys().map(String::as_str).collect();
    assert_eq!(
        boards,
        [
            "canary-pwr-9511d9a",
            "interf_u",
            "kit-dev-coldfire-xilinx_5213",
            "pic_programmer",
            "video"
        ]
    );
    assert_eq!(prereg.set.as_deref(), Some("tools/bench/sets/x01-h2h.txt"));
    let metrics: Vec<&str> = prereg.specs.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        &metrics[..6],
        [
            "via_in_pad",
            "below_minimum",
            "drc_real",
            "kicad_unconnected",
            "vias",
            "length_mm"
        ]
    );
}

#[test]
fn the_x01_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "32a86ec80839cfe81522f8102cbcb0e13f513ac802869d1e5d9e4c79c14e4638",
        "docs/roadmap/prereg/X01.json changed: record the change in its amendments"
    );
}
