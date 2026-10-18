//! Pins N06.5's batch-M registration (`docs/roadmap/prereg/N06.5.json`): the
//! maze search's five Java-bug fixes, `router.java_quirks.init_failure_room_leak`,
//! `failed_attempt_ripped_carryover`, `ortho_zero_width_room`,
//! `edge_corner_not_advanced` and `connect_to_trace_swapped_width`, each alone and
//! together, against Java's behaviour, judged by N02's totals rule with
//! `accept: no-regress` after the reachability counters. Two of the rows route at 90
//! degrees, for `ortho_zero_width_room`.
//!
//! Registered 2026-10-18 before any routing number. A later change goes in the
//! file's `amendments` with the reason and updates the hash here in the same
//! change.
use std::path::{Path, PathBuf};

use freerouting_dev::gate_prereg::Prereg;
use freerouting_dev::json_read::Json;

const FILE: &str = "docs/roadmap/prereg/N06.5.json";
const SET: &str = "tools/bench/sets/n06-m.txt";
const SWITCHES: [&str; 5] = [
    "init_failure_room_leak",
    "failed_attempt_ripped_carryover",
    "ortho_zero_width_room",
    "edge_corner_not_advanced",
    "connect_to_trace_swapped_width",
];

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn json() -> Json {
    Json::parse(&std::fs::read_to_string(repo().join(FILE)).unwrap()).unwrap()
}

fn registration() -> Prereg {
    let text = std::fs::read_to_string(repo().join(FILE)).unwrap();
    Prereg::parse(&text).unwrap_or_else(|e| panic!("{FILE} is invalid: {e:?}"))
}

#[test]
fn the_batch_m_registration_is_valid() {
    let prereg = registration();
    assert_eq!(prereg.id, "N06.5-batch-maze");
    assert_eq!(prereg.names.base, "baseline");
    assert_eq!(prereg.names.cand, "all");
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    assert_eq!(
        arms,
        [
            "all",
            "baseline",
            "connect_to_trace_swapped_width",
            "edge_corner_not_advanced",
            "failed_attempt_ripped_carryover",
            "init_failure_room_leak",
            "ortho_zero_width_room"
        ]
    );
    assert_eq!(prereg.set.as_deref(), Some(SET));
    assert_eq!(prereg.boards.len(), 5);
    // At least one row routes at 90 degrees (the accept clause of N06.5).
    let ninety = prereg
        .boards
        .keys()
        .filter(|b| {
            std::fs::read_to_string(repo().join(b.as_str()))
                .unwrap()
                .contains("(snap_angle ninety_degree)")
        })
        .count();
    assert_eq!(ninety, 2);
    assert!(prereg.totals_registered);
    let metrics: Vec<&str> = prereg.specs.iter().map(|s| s.name.as_str()).collect();
    for name in [
        "unrouted",
        "clearance_violations",
        "kicad_unconnected",
        "session_differences",
        "maze_init_failure_room_leaks",
        "maze_ripped_carryovers",
        "maze_ortho_zero_width_room_flips",
        "maze_edge_sweeps_without_neighbours",
        "maze_edge_corner_flips",
        "insert_connect_to_trace_width_flips",
    ] {
        assert!(metrics.contains(&name), "{name}");
    }
    // n06_run.py runs the arms in file order and compares every arm with the
    // baseline, so the baseline comes first.
    let text = std::fs::read_to_string(repo().join(FILE)).unwrap();
    let arms = &text[text.find("\"arms\": {").expect("arms")..];
    let first = arms[9..].trim_start().split('"').nth(1);
    assert_eq!(first, Some("baseline"));
}

/// Each arm gives all five switches explicitly, `false` exactly for its own, on the
/// command line and in its settings.
#[test]
fn each_arm_sets_only_its_switches() {
    let json = json();
    let expect: [(&str, &[&str]); 7] = [
        ("baseline", &[]),
        ("init_failure_room_leak", &SWITCHES[..1]),
        ("failed_attempt_ripped_carryover", &SWITCHES[1..2]),
        ("ortho_zero_width_room", &SWITCHES[2..3]),
        ("edge_corner_not_advanced", &SWITCHES[3..4]),
        ("connect_to_trace_swapped_width", &SWITCHES[4..]),
        ("all", &SWITCHES[..]),
    ];
    for (arm, off) in expect {
        let spec = json.path(&format!("arms.{arm}")).expect(arm);
        let command = spec.get("command").and_then(Json::as_str).expect("command");
        assert_eq!(command.matches("--router.java_quirks.").count(), 5, "{arm}");
        assert!(command.contains("--router.profile=legacy"), "{arm}");
        for id in SWITCHES {
            let value = if off.contains(&id) { "false" } else { "true" };
            assert!(
                command.contains(&format!("--router.java_quirks.{id}={value} "))
                    || command.ends_with(&format!("--router.java_quirks.{id}={value}")),
                "{arm}: {id}={value} in {command}"
            );
        }
        let Some(Json::Object(settings)) = spec.get("settings") else {
            panic!("{arm}: settings")
        };
        assert_eq!(settings.len(), off.len(), "{arm}");
        for id in off {
            let key = format!("router.java_quirks.{id}");
            assert!(settings.iter().any(|(k, _)| *k == key), "{arm}: {key}");
        }
    }
}

/// Every row is in the set file and hashes as registered.
#[test]
fn the_registered_inputs_hash_as_registered() {
    let json = json();
    let Some(Json::Object(files)) = json.path("boards.files") else {
        panic!("boards.files")
    };
    let set = std::fs::read_to_string(repo().join(SET)).unwrap();
    let rows: Vec<&str> = set
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    assert_eq!(rows.len(), files.len());
    for (input, want) in files {
        assert!(rows.contains(&input.as_str()), "{input} missing from {SET}");
        let bytes = std::fs::read(repo().join(input)).unwrap_or_else(|e| panic!("{input}: {e}"));
        assert_eq!(
            freerouting::quality::feasibility::report::sha256_hex(&bytes),
            want.as_str().unwrap(),
            "{input}"
        );
    }
}

#[test]
fn the_batch_m_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "bf9a692bca53916a6d8f0d02bb49f58ae34c67be872d35c45b34152c82748ec3"
    );
}
