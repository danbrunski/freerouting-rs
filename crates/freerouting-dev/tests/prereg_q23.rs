//! Pins Q23.2's registration (`docs/roadmap/prereg/Q23.json`): one restart from the best board
//! (`router.autorouter.restarts_from_best=1`, arm `restart`) against the same run without it
//! (arm `base`), both profile `enhanced` with `--max-passes 1`, judged by the paired rule on the
//! router's unrouted count with the hard limits, the real-DRC guard and a work guard (restart's
//! maze work at most twice base's). Registered 2026-10-22 before any run of either arm. A later
//! change goes in its `amendments` with the reason and updates the hash below in the same change.
//!
//! The PCU row is read by absolute path (not in git); a missing file is skipped, a present one
//! must hash as registered, as must the rules files beside it. The other rows are fixtures.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use freerouting::quality::feasibility::report::sha256_hex;
use freerouting_dev::gate::Role;
use freerouting_dev::gate_prereg::Prereg;
use freerouting_dev::json_read::Json;

const FILE: &str = "docs/roadmap/prereg/Q23.json";
const SET: &str = "tools/bench/sets/q23-restart.txt";
const SETTING: &str = "router.autorouter.restarts_from_best";
/// The rows in set order: (name, flags, safety timeout in seconds).
const ROWS: [(&str, &[&str], f64); 7] = [
    (
        "pcu",
        &[
            "--router.plane_obstacle_net_classes=Pour1A,Pour3A",
            "--router.cleanup.remove_stubs=true",
        ],
        900.0,
    ),
    ("canary-pwr-pours1", CANARY, 300.0),
    ("canary-pwr-pours2", CANARY, 300.0),
    ("canary-pwr-pours3", CANARY, 300.0),
    ("coldfire-5213", &[], 300.0),
    ("cpu-85", &[], 300.0),
    ("gp8b", &[], 300.0),
];
const CANARY: &[&str] = &[
    "--router.scoring.start_ripup_costs=1500",
    "--router.min_trace_width_um=150",
];

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn text() -> String {
    std::fs::read_to_string(repo().join(FILE)).unwrap()
}

fn json() -> Json {
    Json::parse(&text()).unwrap()
}

fn registration() -> Prereg {
    Prereg::parse(&text()).unwrap_or_else(|e| panic!("{FILE} is invalid: {e:?}"))
}

fn object<'a>(json: &'a Json, path: &str) -> &'a BTreeMap<String, Json> {
    match json.path(path) {
        Some(Json::Object(o)) => o,
        other => panic!("{path}: {other:?}"),
    }
}

fn string<'a>(json: &'a Json, path: &str) -> &'a str {
    json.path(path)
        .and_then(Json::as_str)
        .unwrap_or_else(|| panic!("{path}"))
}

/// The set file's rows: (path, comment).
fn set_rows() -> Vec<(String, String)> {
    std::fs::read_to_string(repo().join(SET))
        .unwrap()
        .lines()
        .filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let (path, comment) = l.split_once('#').unwrap_or((l, ""));
            (path.trim().to_string(), comment.trim().to_string())
        })
        .collect()
}

/// The bytes of `path`: absolute as given, else under the repository root.
fn read(path: &str) -> Option<Vec<u8>> {
    let p = Path::new(path);
    if p.is_absolute() {
        std::fs::read(p).ok()
    } else {
        std::fs::read(repo().join(p)).ok()
    }
}

#[test]
fn the_registration_is_valid_and_names_its_arms_and_metrics() {
    let prereg = registration();
    assert_eq!(prereg.id, "Q23.4-restart-from-the-best-board");
    assert_eq!(prereg.names.base, "base");
    assert_eq!(prereg.names.cand, "restart");
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    assert_eq!(arms, ["base", "restart"]);
    assert_eq!(prereg.set.as_deref(), Some(SET));
    assert_eq!(prereg.boards.len(), ROWS.len());
    assert!(
        !prereg.totals_registered,
        "an opt-in stage: the paired rule"
    );
    assert_eq!(prereg.amendments, 0);
    assert_eq!(
        (
            prereg.rule.min_boards,
            prereg.rule.improve_all_but,
            prereg.rule.max_regress
        ),
        (3, 1, 0)
    );
    let roles: Vec<(&str, Role, bool)> = prereg
        .specs
        .iter()
        .map(|s| (s.name.as_str(), s.role, s.compared))
        .collect();
    assert_eq!(
        roles,
        [
            ("via_in_pad", Role::Hard, true),
            ("below_minimum", Role::Hard, true),
            ("drc_real", Role::Guard, true),
            ("unrouted", Role::Score, true),
            ("kicad_unconnected", Role::Report, false),
            ("maze_work_units", Role::Report, false),
            ("wall_seconds", Role::Report, false),
        ]
    );
}

/// The arms differ only by the restart setting: the same engine, the same command up to the one
/// flag, and the setting in `settings`.
#[test]
fn the_arms_differ_by_the_restart_setting_only() {
    let json = json();
    let base = object(&json, "arms.base");
    let restart = object(&json, "arms.restart");
    assert_eq!(
        string(&json, "arms.base.engine"),
        string(&json, "arms.restart.engine")
    );
    let command = string(&json, "arms.base.command");
    assert_eq!(
        string(&json, "arms.restart.command"),
        format!("{command} --{SETTING}=1")
    );
    for part in [
        "-de {IN} -do {OUT}",
        "--max-passes 1",
        "--timeout {TIMEOUT_S}",
        "--stats json",
        "--result-json {RESULT}",
        "{ROW_FLAGS}",
    ] {
        assert!(command.contains(part), "{part} not in {command}");
    }
    assert!(
        !command.contains("--profile"),
        "both arms run the default profile, enhanced"
    );
    assert!(object(&json, "arms.base.settings").is_empty());
    let settings = object(&json, "arms.restart.settings");
    assert_eq!(settings.len(), 1);
    assert_eq!(settings.get(SETTING).and_then(Json::as_str), Some("1"));
    assert_eq!(
        base.keys().collect::<Vec<_>>(),
        restart.keys().collect::<Vec<_>>()
    );
    // The checklist's key is Q12's removed one; the registration says why it is not used.
    assert!(string(&json, "setting_name").contains("removed"));
}

/// The set lists exactly the registered rows in the registered order, each with its flags and
/// timeout; every input present hashes as registered, and so do the PCU row's rules files.
#[test]
fn the_rows_match_the_set_with_their_flags_and_hashes() {
    let json = json();
    let rows = object(&json, "boards.rows");
    let files = object(&json, "boards.files");
    let set = set_rows();
    assert_eq!(set.len(), ROWS.len());
    assert_eq!(rows.len(), ROWS.len());
    assert_eq!(files.len(), ROWS.len());
    for ((path, comment), (name, flags, timeout)) in set.iter().zip(ROWS) {
        let spec = match rows.get(path) {
            Some(Json::Object(o)) => o,
            other => panic!("{path}: {other:?}"),
        };
        assert_eq!(spec.get("row").and_then(Json::as_str), Some(name));
        assert!(comment.starts_with(&format!("{name}: ")), "{comment}");
        assert!(comment.ends_with(&format!(", {timeout} s")), "{comment}");
        assert_eq!(
            spec.get("timeout_s"),
            Some(&Json::Number(timeout)),
            "{name}"
        );
        let want: Vec<Json> = flags.iter().map(|f| Json::String(f.to_string())).collect();
        assert_eq!(spec.get("flags"), Some(&Json::Array(want)), "{name}");
        if flags.is_empty() {
            assert!(comment.contains("no flags"), "{comment}");
        } else {
            assert!(comment.contains(&flags.join(" ")), "{comment}");
        }
        let sha = files
            .get(path)
            .and_then(Json::as_str)
            .unwrap_or_else(|| panic!("{path}: no hash"));
        let Some(bytes) = read(path) else {
            assert_eq!(name, "pcu", "only the PCU row may be missing");
            continue;
        };
        assert_eq!(sha256_hex(&bytes), sha, "{name}");
        if name == "pcu" {
            for ext in ["kicad_pro", "kicad_dru"] {
                let beside = Path::new(path).with_extension(ext);
                let bytes = std::fs::read(&beside).unwrap_or_else(|e| panic!("{beside:?}: {e}"));
                assert_eq!(
                    sha256_hex(&bytes),
                    string(&json, &format!("boards.rules_files.{ext}")),
                    "{beside:?}"
                );
            }
        }
    }
}

/// The work guard bounds restart's maze work at twice base's, and the decision table names the
/// pass, fail and not-measured outcomes.
#[test]
fn the_work_guard_and_the_decision_table_are_registered() {
    let json = json();
    assert_eq!(string(&json, "work_guard.metric"), "maze_work_units");
    assert_eq!(json.path("work_guard.max_ratio"), Some(&Json::Number(2.0)));
    let table = object(&json, "decision_table");
    assert_eq!(
        table.keys().collect::<Vec<_>>(),
        ["fail", "not_measured", "pass"]
    );
    assert!(string(&json, "decision_table.fail").contains("remove quality/restart.rs"));
    assert!(string(&json, "keep_rule").contains("N-1"));
    assert!(string(&json, "validity").contains("--left-out"));
}

#[test]
fn the_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "64ca9b3f17860bdf0e39b3d1dfc7291f275a2e04526108b9cbbdf9bfa1524e30"
    );
}
