//! Pins N06.9's batch-O registration (`docs/roadmap/prereg/N06.9.json`): the
//! batch optimizer's two Java-bug fixes, `router.java_quirks.opt_first_pass_stops`
//! and `opt_equal_key_skip`, each alone and both, against Java's behaviour,
//! judged by N02's totals rule with `accept: no-regress`.
//!
//! Registered 2026-10-18 before any routing number. A later change goes in the
//! file's `amendments` with the reason and updates the hash here in the same
//! change.
//!
//! The KiCad demo rows are outside the repository (their hashes, of the
//! stripped boards, are K09.json's and are checked by the run); the rows inside
//! the repository are checked here.
use std::path::{Path, PathBuf};

use freerouting_dev::gate_prereg::Prereg;
use freerouting_dev::json_read::Json;

const FILE: &str = "docs/roadmap/prereg/N06.9.json";
const SET: &str = "tools/bench/sets/n06-o.txt";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn registration() -> Prereg {
    let text = std::fs::read_to_string(repo().join(FILE)).unwrap();
    Prereg::parse(&text).unwrap_or_else(|e| panic!("{FILE} is invalid: {e:?}"))
}

#[test]
fn the_batch_o_registration_is_valid() {
    let prereg = registration();
    assert_eq!(prereg.id, "N06.9-batch-optimizer");
    assert_eq!(prereg.names.base, "java");
    assert_eq!(prereg.names.cand, "both");
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    assert_eq!(arms, ["both", "equal_key", "first_pass", "java"]);
    assert_eq!(prereg.set.as_deref(), Some(SET));
    assert_eq!(prereg.boards.len(), 6);
    assert!(prereg.totals_registered);
    let metrics: Vec<&str> = prereg.specs.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        &metrics[..5],
        [
            "via_in_pad",
            "below_minimum",
            "clearance_violations",
            "drc_real",
            "kicad_unconnected"
        ]
    );
    // N06.9 asks for time as well as quality.
    for name in ["optimizer_passes", "optimizer_seconds", "wall_seconds"] {
        assert!(metrics.contains(&name), "{name}");
    }
}

/// Each arm sets exactly its switches to `false`, on the command line and in
/// its settings.
#[test]
fn each_arm_sets_only_its_switches() {
    let text = std::fs::read_to_string(repo().join(FILE)).unwrap();
    let json = Json::parse(&text).unwrap();
    let expect = [
        ("java", &[][..]),
        ("first_pass", &["opt_first_pass_stops"][..]),
        ("equal_key", &["opt_equal_key_skip"][..]),
        ("both", &["opt_first_pass_stops", "opt_equal_key_skip"][..]),
    ];
    for (arm, ids) in expect {
        let spec = json.path(&format!("arms.{arm}")).expect(arm);
        let command = spec.get("command").and_then(Json::as_str).expect("command");
        // Amendment 1: every arm names its profile (enhanced) explicitly.
        assert_eq!(
            command.matches("--router.profile=enhanced").count(),
            1,
            "{arm}: {command}"
        );
        let flags = command.matches("--router.java_quirks.").count();
        assert_eq!(flags, ids.len(), "{arm}: {command}");
        let Some(Json::Object(settings)) = spec.get("settings") else {
            panic!("{arm}: settings")
        };
        assert_eq!(settings.len(), ids.len(), "{arm}");
        for id in ids {
            assert!(
                command.contains(&format!("--router.java_quirks.{id}=false")),
                "{arm}: {command}"
            );
            let key = format!("router.java_quirks.{id}");
            assert!(
                settings.iter().any(|(k, _)| *k == key),
                "{arm}: {key} not in its settings"
            );
        }
    }
}

/// Every row inside the repository is in the set file and hashes as registered.
#[test]
fn the_registered_inputs_hash_as_registered() {
    let text = std::fs::read_to_string(repo().join(FILE)).unwrap();
    let json = Json::parse(&text).unwrap();
    let Some(Json::Object(rows)) = json.path("boards.rows") else {
        panic!("boards.rows")
    };
    let set = std::fs::read_to_string(repo().join(SET)).unwrap();
    for (row, spec) in rows {
        let input = spec.get("input").and_then(Json::as_str).expect("input");
        assert!(set.contains(input), "{input} missing from {SET}");
        if input.starts_with('/') {
            continue;
        }
        let bytes = std::fs::read(repo().join(input)).unwrap_or_else(|e| panic!("{input}: {e}"));
        let want = json
            .path("boards.files")
            .and_then(|f| f.get(row))
            .and_then(Json::as_str)
            .unwrap_or_else(|| panic!("boards.files.{row}"));
        assert_eq!(
            freerouting::quality::feasibility::report::sha256_hex(&bytes),
            want,
            "{row}"
        );
    }
}

#[test]
fn the_batch_o_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "c5af6d94f4788a36f0d4184026cae59d6554a4fce36a83cd5940203283de07bb"
    );
}
