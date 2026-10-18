//! Pins Q10's preregistration (`docs/roadmap/prereg/Q10.json`, Q10.4): the
//! add-only completion sweep against the unchanged engine. Registered
//! 2026-10-17 as
//! c280ca8e32a3d9944f558c1751e7a6d759677aa3512595f6c3fc4324655ca347,
//! before any number; a later change (the `eligible` list among them) goes
//! in its `amendments` with the reason and updates the hash below in the
//! same commit. Amended three times on 2026-10-18 before any number of the
//! affected arms (the opt arm's preflight guards, the engine at main
//! f6e75ab with its `enhanced`, the `eligible` list and a safety bound):
//! cefea2fafc7e9e487eadd0f6bf777a318ccf7d02721b2620b7c8ad8b44c1f0e2.
use std::path::{Path, PathBuf};

use freerouting_dev::gate_prereg::Prereg;

const FILE: &str = "docs/roadmap/prereg/Q10.json";
const SET: &str = "tools/bench/sets/q10-sweep.txt";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn registration() -> Prereg {
    let text = std::fs::read_to_string(repo().join(FILE)).unwrap();
    Prereg::parse(&text).unwrap_or_else(|e| panic!("Q10 registration is invalid: {e:?}"))
}

#[test]
fn the_q10_registration_is_valid_and_names_its_arms_and_boards() {
    let prereg = registration();
    assert_eq!(prereg.id, "Q10.4-add-only-sweep");
    assert_eq!(prereg.names.base, "mp1");
    assert_eq!(prereg.names.cand, "mp1+sweep-free");
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    assert_eq!(
        arms,
        [
            "mp1",
            "mp1+opt+sweep+opt",
            "mp1+sweep-fixed",
            "mp1+sweep-free",
            "mpdef",
            "mpdef+sweep"
        ]
    );
    assert_eq!(prereg.set.as_deref(), Some(SET));
    assert_eq!(prereg.boards.len(), 30);
    let metrics: Vec<&str> = prereg.specs.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        &metrics[..6],
        [
            "via_in_pad",
            "below_minimum",
            "drc_real",
            "pour_lost_pins",
            "kicad_unconnected",
            "unrouted"
        ]
    );
    // Every registered input is in the set file; those in the repository
    // hash as registered (the corpus rows are skipped while missing).
    let set = std::fs::read_to_string(repo().join(SET)).unwrap();
    for (path, sha) in &prereg.boards {
        assert!(set.contains(path.as_str()), "{path} missing from the set");
        let Ok(bytes) = std::fs::read(repo().join(path)) else {
            assert!(path.starts_with("target/corpus/"), "{path} is missing");
            continue;
        };
        assert_eq!(
            &freerouting::quality::feasibility::report::sha256_hex(&bytes),
            sha,
            "{path}"
        );
    }
}

#[test]
fn the_q10_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "cefea2fafc7e9e487eadd0f6bf777a318ccf7d02721b2620b7c8ad8b44c1f0e2"
    );
}
