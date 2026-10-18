//! Pins K09's preregistration (`docs/roadmap/prereg/K09.json`, K09.9): which
//! DSN fix-ups and router settings the plugin's `enhanced` profile turns on
//! by default, judged by N02's totals rule on canary-pwr and three KiCad
//! demos through the plugin's own headless path. Registered 2026-10-17 as
//! d4dcfedcf808ff3c3c69983333909c0997e4a68ad8fe59d3158d9c6510242f8b,
//! before any routing number; a later change goes in its `amendments` with
//! the reason and updates the hash below in the same change. Amendment 1
//! (the same day, after the first canary-pwr runs): the plugin sets imported
//! via drills from the session, now
//! 3ba6cc46e36a8b6ecc6868a9a523bc6e3fa128bbc4e1400e4245d718a337d0de.
//! Amendment 2 (after round 1 on every board): the edge keepouts also follow
//! outline segments off the bounding box, and every arm runs again; now
//! 626f62d42664509bac8b203daa4bc6555165c88f382374b90a313a47d9efb973.
//!
//! The boards are outside the repository (Dan's canary repo and KiCad's
//! demos), so their hashes are checked by the run, not here.
use std::path::{Path, PathBuf};

use freerouting_dev::gate_prereg::Prereg;

const FILE: &str = "docs/roadmap/prereg/K09.json";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn registration() -> Prereg {
    let text = std::fs::read_to_string(repo().join(FILE)).unwrap();
    Prereg::parse(&text).unwrap_or_else(|e| panic!("K09 registration is invalid: {e:?}"))
}

#[test]
fn the_k09_registration_is_valid_and_names_its_arms_and_boards() {
    let prereg = registration();
    assert_eq!(prereg.id, "K09.9-plugin-enhanced-defaults");
    assert_eq!(prereg.names.base, "legacy");
    assert_eq!(prereg.names.cand, "enh-auto-mp1");
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    assert_eq!(
        arms,
        [
            "enh-auto-mp1",
            "enh-auto-mpdef",
            "enh-auto-nowidth-mp1",
            "legacy",
            "pass-hint",
            "plugin-0",
            "preset-engine",
            "preset-in2-open",
            "preset-nosweep",
            "preset-wires"
        ]
    );
    assert_eq!(prereg.boards.len(), 4);
    let metrics: Vec<&str> = prereg.specs.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        &metrics[..6],
        [
            "via_in_pad",
            "below_minimum",
            "inner_tracks",
            "drc_real",
            "copper_edge_clearance",
            "kicad_unconnected"
        ]
    );
    assert_eq!(prereg.amendments, 2);
}

#[test]
fn the_k09_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "626f62d42664509bac8b203daa4bc6555165c88f382374b90a313a47d9efb973"
    );
}
