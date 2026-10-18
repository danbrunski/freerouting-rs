//! Pins Q07's two preregistrations (`docs/roadmap/prereg/Q07.json`, the
//! `conflict-rounds` pass-1 order, and `Q07-inside-out.json`, the
//! `inside-out` order), both against Java's item order. Both were registered
//! on 2026-10-17, before any measured number, with the sha256 values below.
//! A later change goes in the file's `amendments`, with its reason, and
//! updates the hash here in the same commit.
//!
//! Both orders failed the gate and their code was removed (Q07.2, Q07.3
//! and Q07.4 done notes); the numbers stay in the baseline files
//! `tools/bench/baselines/Q07.json` and `Q07-inside-out.json`, every row
//! marked `rejected`.
//!
//! The D3-C and F03.3 corpus boards live outside the repository
//! (`target/corpus-d3c`, `target/corpus`); their hashes are checked when the
//! files are present and skipped otherwise.
use std::path::{Path, PathBuf};

use freerouting::quality::feasibility::report::sha256_hex;
use freerouting_dev::gate_prereg::Prereg;
use freerouting_dev::json_read::Json;

const SET: &str = "tools/bench/sets/q07.txt";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn registration(name: &str) -> Prereg {
    let path = format!("docs/roadmap/prereg/{name}.json");
    let text = std::fs::read_to_string(repo().join(&path)).unwrap();
    Prereg::parse(&text).unwrap_or_else(|e| panic!("{path} is invalid: {e:?}"))
}

#[test]
fn the_q07_registrations_are_valid_and_name_their_arms_and_boards() {
    let set = std::fs::read_to_string(repo().join(SET)).unwrap();
    for (name, id, cand, boards) in [
        ("Q07", "Q07.2-conflict-rounds", "conflict-rounds", 61),
        ("Q07-inside-out", "Q07.3-inside-out", "inside-out", 40),
    ] {
        let prereg = registration(name);
        assert_eq!(prereg.id, id);
        assert_eq!(prereg.names.base, "java");
        assert_eq!(prereg.names.cand, cand);
        assert_eq!(prereg.set.as_deref(), Some(SET));
        assert_eq!(prereg.boards.len(), boards, "{name}");
        let metrics: Vec<&str> = prereg.specs.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            &metrics[..3],
            ["via_in_pad", "clearance_violations", "unrouted"]
        );
        for (path, hash) in &prereg.boards {
            assert!(set.contains(path.as_str()), "{path} missing from the set");
            let file = repo().join(path);
            if path.starts_with("target/") && !file.is_file() {
                continue;
            }
            let bytes = std::fs::read(&file).unwrap_or_else(|e| panic!("{path}: {e}"));
            assert_eq!(&sha256_hex(&bytes), hash, "{path}");
        }
    }
}

#[test]
fn the_q07_registrations_are_pinned_by_their_hashes() {
    assert_eq!(
        registration("Q07").sha256,
        "b31fb55555c436d256771d0f84bb83f996cf8e15c3aae2363af2b65e55205ecc"
    );
    assert_eq!(
        registration("Q07-inside-out").sha256,
        "b32f7baafda35ac0cf22e335a08edc9516caa907ef588f2c1a6cc6fdd5a26629"
    );
}

#[test]
fn the_q07_baselines_keep_every_registered_row_marked_rejected() {
    for (name, rows) in [("Q07", 61), ("Q07-inside-out", 40)] {
        let path = repo().join(format!("tools/bench/baselines/{name}.json"));
        let text = std::fs::read_to_string(&path).unwrap();
        let baseline = Json::parse(&text).unwrap();
        let Some(Json::Object(body)) = baseline.get("rows") else {
            panic!("{name}: no rows");
        };
        assert_eq!(body.len(), rows, "{name}");
        let prereg = registration(name);
        for (row, value) in body {
            assert!(
                prereg.boards.contains_key(row),
                "{name}: {row} not registered"
            );
            assert!(
                matches!(value.get("rejected"), Some(Json::Bool(true))),
                "{name}: {row} is not marked rejected"
            );
        }
    }
}
