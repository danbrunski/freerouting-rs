//! Pins Q14's preregistration (`docs/roadmap/prereg/Q14.json`, Q14.3): the
//! via clearance of a net's class (`router.via_clearance=net_class`) against
//! Java's rule, judged by N02's totals rule. Registered 2026-10-17 as
//! 98380a4e870ede534ba68c88e5012add78d03d023006dd6e2a903d6d08f64635,
//! before any number; a later change goes in its `amendments` with the
//! reason and updates the hash below in the same commit.
use std::path::{Path, PathBuf};

use freerouting_dev::gate_prereg::Prereg;

const FILE: &str = "docs/roadmap/prereg/Q14.json";
const SET: &str = "tools/bench/sets/q14-via-clearance.txt";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn registration() -> Prereg {
    let text = std::fs::read_to_string(repo().join(FILE)).unwrap();
    Prereg::parse(&text).unwrap_or_else(|e| panic!("Q14 registration is invalid: {e:?}"))
}

#[test]
fn the_q14_registration_is_valid_and_names_its_arms_and_boards() {
    let prereg = registration();
    assert_eq!(prereg.id, "Q14.3-via-clearance-net-class");
    assert_eq!(prereg.names.base, "mp1");
    assert_eq!(prereg.names.cand, "mp1+netclass");
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    assert_eq!(
        arms,
        ["mp1", "mp1+netclass", "mp1+sweep", "mp1+sweep+netclass"]
    );
    assert_eq!(prereg.set.as_deref(), Some(SET));
    assert_eq!(prereg.boards.len(), 3);
    let metrics: Vec<&str> = prereg.specs.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        &metrics[..5],
        [
            "via_in_pad",
            "below_minimum",
            "class_via_clearance_errors",
            "drc_real",
            "kicad_unconnected"
        ]
    );
    // Every registered input is in the set file; those in the repository
    // hash as registered (the corpus row is skipped while missing).
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
    // The verdict rule is N02's totals rule, stated in the registration.
    let text = std::fs::read_to_string(repo().join(FILE)).unwrap();
    assert!(text.contains("\"rule\": \"totals\""));
}

#[test]
fn the_q14_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "98380a4e870ede534ba68c88e5012add78d03d023006dd6e2a903d6d08f64635"
    );
}
