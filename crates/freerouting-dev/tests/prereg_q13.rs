//! Pins Q13's preregistration (`docs/roadmap/prereg/Q13.json`, Q13.3): a
//! plane net's traces kept inside its own plane on a power layer
//! (`router.power_layer_traces=own_plane`) against Java's rule, judged by
//! N02's totals rule. Registered 2026-10-17 as
//! ec1d28e7918a5df6dde7c9bf11cd118ec878f8cb38fff2ab3eb918d59d09b95f,
//! before any number; a later change goes in its `amendments` with the
//! reason and updates the hash below in the same commit. Amended once
//! (2026-10-17, now 063615a66fd628ba009ad90ed946468bb5f8ac4ee4607edf385f177905b97169): the candidate's
//! hop check was fixed after its first run failed a hard clause, and the
//! candidate arms run again.
use std::path::{Path, PathBuf};

use freerouting_dev::gate_prereg::Prereg;

const FILE: &str = "docs/roadmap/prereg/Q13.json";
const SET: &str = "tools/bench/sets/q13-power-layer-traces.txt";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn registration() -> Prereg {
    let text = std::fs::read_to_string(repo().join(FILE)).unwrap();
    Prereg::parse(&text).unwrap_or_else(|e| panic!("Q13 registration is invalid: {e:?}"))
}

#[test]
fn the_q13_registration_is_valid_and_names_its_arms_and_boards() {
    let prereg = registration();
    assert_eq!(prereg.id, "Q13.3-power-layer-traces-own-plane");
    assert_eq!(prereg.names.base, "plugin");
    assert_eq!(prereg.names.cand, "plugin+ownplane");
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    assert_eq!(
        arms,
        [
            "mp1",
            "mp1+netclass",
            "mp1+netclass+ownplane",
            "mp1+ownplane",
            "mp1+sweep",
            "mp1+sweep+netclass",
            "mp1+sweep+netclass+ownplane",
            "mp1+sweep+ownplane",
            "plugin",
            "plugin+netclass",
            "plugin+netclass+ownplane",
            "plugin+ownplane"
        ]
    );
    assert_eq!(prereg.set.as_deref(), Some(SET));
    assert_eq!(prereg.boards.len(), 6);
    let metrics: Vec<&str> = prereg.specs.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        &metrics[..7],
        [
            "via_in_pad",
            "outside_own_plane",
            "foreign_power_traces",
            "below_minimum",
            "drc_real",
            "inner_tracks",
            "kicad_unconnected"
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
    // The verdict rule is N02's totals rule, stated in the registration.
    let text = std::fs::read_to_string(repo().join(FILE)).unwrap();
    assert!(text.contains("\"rule\": \"totals\""));
}

#[test]
fn the_q13_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "063615a66fd628ba009ad90ed946468bb5f8ac4ee4607edf385f177905b97169"
    );
}
