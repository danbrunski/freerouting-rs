//! `--result-json PATH` and `--router.result_json=PATH` (C08.4): Java's
//! `RoutingResultManifest`, schema version 1, written when the run ends,
//! also when it failed.
//!
//! Java tests ported (at `a773a70ec`): `RoutingResultManifestTest.
//! writeProducesRequiredJsonKeys` (`a_routed_run_writes_every_key`: the keys,
//! `schema_version`, `final_state`, `exit_code`, `output_written`, the
//! statistics and bounds, the phase snapshots). Not ported: `resolve
//! GitShaPrefersEnvironmentVariable` (it only checks `unknown` when the
//! variable is unset; `build.rs` reads `FREEROUTING_GIT_SHA` at build time
//! and `version.rs` checks the shape of the result).

mod common;

use common::*;
use freerouting::drc::json::JsonValue;
use freerouting_cli::manifest::{PHASE_KEYS, TOP_LEVEL_KEYS};

const FIXTURE: &str = "Issue413-test.dsn";
const ROUTED: &str = "Issue026-J2_reference.dsn";

fn keys(value: &JsonValue) -> Vec<String> {
    match value {
        JsonValue::Object(members) => members.iter().map(|(k, _)| k.clone()).collect(),
        other => panic!("not an object: {other:?}"),
    }
}

fn read_manifest(dir: &std::path::Path, name: &str) -> (String, JsonValue) {
    let text = std::fs::read_to_string(dir.join(name)).expect("the manifest exists");
    let json = JsonValue::parse(&text).expect("the manifest is JSON");
    (text, json)
}

fn number(value: Option<&JsonValue>) -> Option<i64> {
    match value {
        Some(JsonValue::Number(n)) => n.parse().ok(),
        _ => None,
    }
}

/// The text without the lines that differ between two runs: the time of the
/// manifest and the durations.
fn comparable(text: &str) -> String {
    text.lines()
        .filter(|l| !l.contains("\"generated_at\"") && !l.contains("\"duration_seconds\""))
        .map(|l| l.to_string() + "\n")
        .collect()
}

/// The key paths of an object tree, `a.b.c`, objects only.
fn key_paths(value: &JsonValue, prefix: &str, out: &mut Vec<String>) {
    if let JsonValue::Object(members) = value {
        for (key, member) in members {
            let path = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            out.push(path.clone());
            key_paths(member, &path, out);
        }
    }
}

#[test]
fn a_routed_run_writes_every_key() {
    let dir = scratch("result-routed");
    copy_fixtures(&dir, &[ROUTED]);
    let out = run_in(
        &dir,
        &[
            "-de",
            ROUTED,
            "-do",
            "out.ses",
            "-mp",
            "2",
            "--result-json",
            "result.json",
        ],
    );
    let log = stderr(&out);
    assert_eq!(code(&out), 0, "{log}");
    let (text, manifest) = read_manifest(&dir, "result.json");

    assert_eq!(keys(&manifest), TOP_LEVEL_KEYS);
    assert_eq!(number(member(&manifest, "schema_version")), Some(1));
    assert_eq!(number(member(&manifest, "exit_code")), Some(0));
    assert_eq!(
        member(&manifest, "output_written"),
        Some(&JsonValue::Bool(true))
    );
    assert_eq!(
        string_of(member(&manifest, "final_state")).as_deref(),
        Some("COMPLETED")
    );
    let fixture = member(&manifest, "fixture").unwrap();
    assert_eq!(
        string_of(member(fixture, "filename")).as_deref(),
        Some(ROUTED)
    );
    assert_eq!(member(fixture, "sha256"), Some(&JsonValue::Null));
    assert_eq!(
        string_of(member(&manifest, "app_version")).as_deref(),
        Some(env!("CARGO_PKG_VERSION"))
    );

    // The JVM figures are null.
    for key in ["resource_usage", "cpu_score"] {
        assert_eq!(member(&manifest, key), Some(&JsonValue::Null), "{key}");
    }
    let phases = member(&manifest, "phases").unwrap();
    assert_eq!(keys(phases), ["fanout", "autorouter", "optimizer"]);
    for name in ["fanout", "autorouter", "optimizer"] {
        let phase = member(phases, name).unwrap();
        assert_eq!(keys(phase), PHASE_KEYS, "{name}");
        for key in ["cpu_seconds", "total_allocated_gb", "peak_heap_mb"] {
            assert_eq!(member(phase, key), Some(&JsonValue::Null), "{name}.{key}");
        }
    }

    // passes_completed is the number of pass lines.
    let pass_lines = log
        .lines()
        .filter(|l| l.starts_with("Auto-routing pass #"))
        .count();
    assert_eq!(pass_lines, 2, "{log}");
    let autorouter = member(phases, "autorouter").unwrap();
    assert_eq!(
        number(member(autorouter, "passes_completed")),
        Some(pass_lines as i64)
    );
    assert!(member(autorouter, "before").is_some_and(|v| *v != JsonValue::Null));
    assert!(member(autorouter, "after").is_some_and(|v| *v != JsonValue::Null));

    // The statistics have Java's keys: the same key tree as the reference
    // load dump of the same board (a routed board has the same shape).
    let reference = JsonValue::parse(
        &std::fs::read_to_string(repo("fixtures/reference/load/Issue026-J2_reference.json"))
            .unwrap(),
    )
    .unwrap();
    let mut expected = Vec::new();
    key_paths(member(&reference, "statistics").unwrap(), "", &mut expected);
    let mut got = Vec::new();
    key_paths(member(&manifest, "board_statistics").unwrap(), "", &mut got);
    assert_eq!(got, expected);
    let mut bounds = Vec::new();
    key_paths(member(&manifest, "bounds").unwrap(), "", &mut bounds);
    assert_eq!(
        bounds,
        ["min_trace_length_mm", "min_via_count", "min_bend_count"]
    );
    assert!(member(&manifest, "normalized_score").is_some_and(|v| *v != JsonValue::Null));
    // The router score is 1000 on a fully routed board.
    assert!(text.contains("\"normalized_score\": 1000.00"), "{text}");

    // The settings snapshot is the final settings, with numbers for threads.
    let settings = member(&manifest, "settings_snapshot").unwrap();
    assert!(member(settings, "autorouter").is_some());
    assert_eq!(
        number(member(
            member(settings, "autorouter").unwrap(),
            "max_passes"
        )),
        Some(2)
    );
}

#[test]
fn a_failed_load_still_writes_the_manifest() {
    let dir = scratch("result-failed");
    std::fs::write(dir.join("bad.dsn"), "this is not a design").unwrap();
    let out = run_in(
        &dir,
        &[
            "-de",
            "bad.dsn",
            "-do",
            "out.ses",
            "--result-json",
            "sub/result.json",
        ],
    );
    assert_eq!(code(&out), 1, "{}", stderr(&out));
    assert!(!dir.join("out.ses").exists());
    let (_, manifest) = read_manifest(&dir, "sub/result.json");
    assert_eq!(keys(&manifest), TOP_LEVEL_KEYS);
    assert_eq!(
        member(&manifest, "output_written"),
        Some(&JsonValue::Bool(false))
    );
    assert_eq!(
        string_of(member(&manifest, "final_state")).as_deref(),
        Some("TERMINATED")
    );
    assert_eq!(number(member(&manifest, "exit_code")), Some(1));
    assert_eq!(
        member(&manifest, "board_statistics"),
        Some(&JsonValue::Null)
    );
    assert!(stderr(&out).contains("Routing result manifest written to"));

    // A missing input file too.
    let out = run_in(
        &dir,
        &[
            "-de",
            "missing.dsn",
            "-do",
            "out.ses",
            "--result-json",
            "m.json",
        ],
    );
    assert_eq!(code(&out), 1);
    let (_, manifest) = read_manifest(&dir, "m.json");
    assert_eq!(number(member(&manifest, "exit_code")), Some(1));

    // And a usage error found before any work.
    let out = run_in(
        &dir,
        &[
            "-de",
            "bad.dsn",
            "-do",
            "out.xyz",
            "--result-json",
            "u.json",
        ],
    );
    assert_eq!(code(&out), 2);
    let (_, manifest) = read_manifest(&dir, "u.json");
    assert_eq!(number(member(&manifest, "exit_code")), Some(2));
    assert_eq!(
        member(&manifest, "output_written"),
        Some(&JsonValue::Bool(false))
    );
}

#[test]
fn the_flag_and_the_setting_write_equal_files() {
    let dir = scratch("result-equal");
    copy_fixtures(&dir, &[FIXTURE]);
    let base = [
        "-de",
        FIXTURE,
        "-do",
        "out.ses",
        "--router.autorouter.enabled=false",
    ];
    let mut with_flag = base.to_vec();
    with_flag.extend(["--result-json", "flag.json"]);
    assert_eq!(code(&run_in(&dir, &with_flag)), 0);
    let mut with_setting = base.to_vec();
    with_setting.push("--router.result_json=setting.json");
    assert_eq!(code(&run_in(&dir, &with_setting)), 0);
    let (flag, _) = read_manifest(&dir, "flag.json");
    let (setting, _) = read_manifest(&dir, "setting.json");
    // Equal apart from the time of the manifest and the durations. The
    // settings snapshot differs in `result_json` itself, which names the file.
    let strip = |text: &str| {
        comparable(text)
            .lines()
            .filter(|l| !l.contains("\"result_json\""))
            .map(|l| l.to_string() + "\n")
            .collect::<String>()
    };
    assert_eq!(strip(&flag), strip(&setting));
    assert!(flag.contains("\"output_written\": true"));
}
