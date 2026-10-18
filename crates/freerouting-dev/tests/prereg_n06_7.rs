//! Pins N06.7's batch-S registration (`docs/roadmap/prereg/N06.7.json`): the
//! shove's three Java-bug fixes, `router.java_quirks.in_front_of_pad_typo`,
//! `store_trace_class_self_compare` and `rational_perp_projection`, each alone and
//! together, plus the reported (c) arm `in_front_of_pad_side0_flank`, against Java's
//! behaviour, judged by N02's totals rule with `accept: no-regress` after the
//! reachability counters.
//!
//! Registered 2026-10-18 before any routing number. A later change goes in the
//! file's `amendments` with the reason and updates the hash here in the same
//! change.
use std::path::{Path, PathBuf};

use freerouting_dev::gate_prereg::Prereg;
use freerouting_dev::json_read::Json;

const FILE: &str = "docs/roadmap/prereg/N06.7.json";
const SET: &str = "tools/bench/sets/n06-s.txt";
const SWITCHES: [&str; 4] = [
    "in_front_of_pad_typo",
    "store_trace_class_self_compare",
    "rational_perp_projection",
    "in_front_of_pad_side0_flank",
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
fn the_batch_s_registration_is_valid() {
    let prereg = registration();
    assert_eq!(prereg.id, "N06.7-batch-shove");
    assert_eq!(prereg.names.base, "baseline");
    assert_eq!(prereg.names.cand, "all");
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    assert_eq!(
        arms,
        [
            "all",
            "all_flank",
            "baseline",
            "in_front_of_pad_typo",
            "rational_perp_projection",
            "store_trace_class_self_compare"
        ]
    );
    assert_eq!(prereg.set.as_deref(), Some(SET));
    assert_eq!(prereg.boards.len(), 5);
    assert!(prereg.totals_registered);
    let metrics: Vec<&str> = prereg.specs.iter().map(|s| s.name.as_str()).collect();
    for name in [
        "unrouted",
        "clearance_violations",
        "kicad_unconnected",
        "session_differences",
        "shove_in_front_of_pad_typo_flips",
        "shove_in_front_of_pad_flank_flips",
        "shove_store_trace_class_flips",
        "shove_rational_projection_flips",
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

/// Each arm gives all four switches explicitly, `false` exactly for its own, on the
/// command line and in its settings.
#[test]
fn each_arm_sets_only_its_switches() {
    let json = json();
    let expect: [(&str, &[&str]); 6] = [
        ("baseline", &[]),
        ("in_front_of_pad_typo", &SWITCHES[..1]),
        ("store_trace_class_self_compare", &SWITCHES[1..2]),
        ("rational_perp_projection", &SWITCHES[2..3]),
        ("all", &SWITCHES[..3]),
        ("all_flank", &SWITCHES[..]),
    ];
    for (arm, off) in expect {
        let spec = json.path(&format!("arms.{arm}")).expect(arm);
        let command = spec.get("command").and_then(Json::as_str).expect("command");
        assert_eq!(command.matches("--router.java_quirks.").count(), 4, "{arm}");
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
fn the_batch_s_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "5f9b910c8a0258c0aa5fc29b0acde6b788626b481169b79ece235acaaeda94a4"
    );
}
