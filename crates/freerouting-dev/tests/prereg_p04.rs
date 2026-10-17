//! Pins P04's two preregistrations (`docs/roadmap/prereg/P04.1.json`, the
//! canonical room order, and `P04.2.json`, the leaf work unit). Both were
//! registered on 2026-10-17, before any number existed, with the sha256 values
//! below. Both candidates failed the gate and were removed (P04.1 and P04.2
//! done notes), so these files record what was measured. A later change goes
//! in the file's `amendments`, with its reason, and updates the hash here in
//! the same commit.
use std::path::{Path, PathBuf};

use freerouting::quality::feasibility::report::sha256_hex;
use freerouting_dev::gate_prereg::Prereg;

const SET: &str = "tools/bench/sets/p04.txt";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn registration(name: &str) -> Prereg {
    let path = format!("docs/roadmap/prereg/{name}.json");
    let text = std::fs::read_to_string(repo().join(&path)).unwrap();
    Prereg::parse(&text).unwrap_or_else(|e| panic!("{path} is invalid: {e:?}"))
}

#[test]
fn the_p04_registrations_are_valid_and_name_their_arms_and_boards() {
    for (name, id, base, cand) in [
        ("P04.1", "P04.1-canonical-room-order", "tree", "canonical"),
        (
            "P04.2",
            "P04.2-leaf-work-unit",
            "canonical",
            "canonical-leaves",
        ),
    ] {
        let prereg = registration(name);
        assert_eq!(prereg.id, id);
        assert_eq!(prereg.names.base, base);
        assert_eq!(prereg.names.cand, cand);
        assert_eq!(prereg.set.as_deref(), Some(SET));
        assert_eq!(prereg.boards.len(), 21, "{name}");
        let metrics: Vec<&str> = prereg.specs.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            &metrics[..3],
            ["via_in_pad", "clearance_violations", "unrouted"]
        );
        // Every registered input is in the set file and hashes as registered.
        let set = std::fs::read_to_string(repo().join(SET)).unwrap();
        for (path, hash) in &prereg.boards {
            assert!(set.contains(path.as_str()), "{path} missing from the set");
            let bytes = std::fs::read(repo().join(path)).unwrap();
            assert_eq!(&sha256_hex(&bytes), hash, "{path}");
        }
    }
}

#[test]
fn the_p04_registrations_are_pinned_by_their_hashes() {
    assert_eq!(
        registration("P04.1").sha256,
        "237311ec653be9171fcffe1d39c509194ebf297c365b5f1eac73283d7d01f9b9"
    );
    assert_eq!(
        registration("P04.2").sha256,
        "187cad4403220fd0321959146666fa243e6eeb244a639757fa7efeab2a0f6c88"
    );
}
