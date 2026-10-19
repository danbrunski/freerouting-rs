//! Pins N06.8's batch-P registration (`docs/roadmap/prereg/N06.8.json`):
//! pull-tight's two Java-bug fixes, `router.java_quirks.smooth_loses_trace` and
//! `pull_tight_reused_lines_change`, each alone and together, against Java's
//! behaviour, judged by N02's totals rule with `accept: no-regress` after the
//! reachability counters, plus the counter of `change_trace`'s prefix and suffix
//! identity case.
//!
//! Registered 2026-10-18 before any routing number. A later change goes in the
//! file's `amendments` with the reason and updates the hash here in the same
//! change.
use std::path::{Path, PathBuf};

use freerouting_dev::gate_prereg::Prereg;
use freerouting_dev::json_read::Json;

const FILE: &str = "docs/roadmap/prereg/N06.8.json";
const SET: &str = "tools/bench/sets/n06-p.txt";
const SWITCHES: [&str; 2] = ["smooth_loses_trace", "pull_tight_reused_lines_change"];

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
fn the_batch_p_registration_is_valid() {
    let prereg = registration();
    assert_eq!(prereg.id, "N06.8-batch-pull-tight");
    assert_eq!(prereg.names.base, "baseline");
    assert_eq!(prereg.names.cand, "all");
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    assert_eq!(
        arms,
        [
            "all",
            "baseline",
            "pull_tight_reused_lines_change",
            "smooth_loses_trace"
        ]
    );
    assert_eq!(prereg.set.as_deref(), Some(SET));
    // The DSN rows plus the plugin row (amendment 1: `canary-pwr-plugin`, hashed as
    // its exported DSN, routed outside n06_run.py).
    assert_eq!(prereg.boards.len(), 7);
    assert!(prereg.totals_registered);
    let metrics: Vec<&str> = prereg.specs.iter().map(|s| s.name.as_str()).collect();
    for name in [
        "unrouted",
        "clearance_violations",
        "kicad_unconnected",
        "session_differences",
        "pull_tight_smooth_lost_traces",
        "pull_tight_reused_lines_flips",
        "trace_change_prefix_suffix_ignored",
    ] {
        assert!(metrics.contains(&name), "{name}");
    }
    // The counters the registration reads are the engine's.
    for name in &metrics[metrics.len() - 3..] {
        assert!(
            freerouting::run_stats::COUNTERS
                .iter()
                .any(|c| c.name() == *name),
            "{name} is a run_stats counter"
        );
    }
    // n06_run.py runs the arms in file order and compares every arm with the
    // baseline, so the baseline comes first.
    let text = std::fs::read_to_string(repo().join(FILE)).unwrap();
    let arms = &text[text.find("\"arms\": {").expect("arms")..];
    let first = arms[9..].trim_start().split('"').nth(1);
    assert_eq!(first, Some("baseline"));
}

/// Each arm gives both switches explicitly, `false` exactly for its own, on the
/// command line and in its settings, under profile `legacy`.
#[test]
fn each_arm_sets_only_its_switches() {
    let json = json();
    let expect: [(&str, &[&str]); 4] = [
        ("baseline", &[]),
        ("smooth_loses_trace", &SWITCHES[..1]),
        ("pull_tight_reused_lines_change", &SWITCHES[1..]),
        ("all", &SWITCHES[..]),
    ];
    for (arm, off) in expect {
        let spec = json.path(&format!("arms.{arm}")).expect(arm);
        let command = spec.get("command").and_then(Json::as_str).expect("command");
        assert_eq!(command.matches("--router.java_quirks.").count(), 2, "{arm}");
        assert!(command.contains("--router.profile=legacy"), "{arm}");
        assert!(command.starts_with('/'), "{arm}: an absolute binary path");
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
    let Some(Json::Object(all)) = json.path("boards.files") else {
        panic!("boards.files")
    };
    // The rows routed outside n06_run.py (amendment 1) are the plugin row only; its
    // input is the plugin's export, which is not in the repository.
    let Some(Json::Array(outside)) = json.path("boards.outside_runner") else {
        panic!("boards.outside_runner")
    };
    let outside: Vec<&str> = outside.iter().filter_map(Json::as_str).collect();
    assert_eq!(outside, ["canary-pwr-plugin"]);
    let files: Vec<_> = all
        .iter()
        .filter(|(row, _)| !outside.contains(&row.as_str()))
        .collect();
    let set = std::fs::read_to_string(repo().join(SET)).unwrap();
    let rows: Vec<&str> = set
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    assert_eq!(rows.len(), files.len());
    for &(input, want) in &files {
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
fn the_batch_p_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "82559f27411307f47d9765163e4c32e5b6cd5babce1a06ce4d4ee54c7478d9de"
    );
}
