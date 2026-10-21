//! Pins Q22.7's registration (`docs/roadmap/prereg/Q22.json`): the re-measurement of Q22's
//! clearance and hole fixes. Arm `before` is the engine and plugin of main `a8b4ae7` (the last
//! main before any Q22 routing or plugin fix), arm `q22` those of main when Q22.8 runs (after
//! Q22.16, Q22.17 and Q22.18 are all `[x]`); both route through the plugin's export with the
//! plugin's default options and are graded by the q22 commit's `tools/kicad-oracle`. The verdict is the absolute guard
//! (zero routing-caused manufacturing breaches in `q22` on every row), not a promotion.
//! Registered 2026-10-20 before any route or grade of either arm. A later change goes in its
//! `amendments` with the reason and updates the hash below in the same change.
//!
//! The boards live under the main checkout's `target/` (not in git). A row's file is looked up
//! under the repository root and its ancestors (a worktree under `target/orchestrator/` finds the
//! main checkout's copies); a missing file is skipped, a present one must hash as registered.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use freerouting::quality::feasibility::report::sha256_hex;
use freerouting_dev::gate::Role;
use freerouting_dev::gate_prereg::Prereg;
use freerouting_dev::json_read::Json;

const FILE: &str = "docs/roadmap/prereg/Q22.json";
const SET: &str = "tools/bench/sets/q22-clearance.txt";
const BEFORE_COMMIT: &str = "a8b4ae7";
const Q22_COMMIT: &str = "Q22_COMMIT";
/// The rows in set order, with their limit (seconds) and whether they keep their tracks.
const ROWS: [(&str, f64, bool); 14] = [
    ("pcietest", 300.0, true),
    ("cm4-edge-ai", 300.0, true),
    ("nudac", 300.0, true),
    ("ch32v003", 300.0, true),
    ("mydevboard", 300.0, true),
    ("vna-usb-pd", 300.0, true),
    ("picodebugger", 300.0, true),
    ("canary-pwr", 600.0, true),
    ("sonde-xilinx", 300.0, true),
    ("pic_programmer", 300.0, true),
    ("flight-computer", 300.0, true),
    ("laser-backplane", 300.0, true),
    ("pocketsdr-v3-0", 300.0, false),
    ("fc-rom", 300.0, false),
];
/// The routing-caused KiCad types of the manufacturing guard (routing-quality.md item 1).
const MANUFACTURING: [&str; 10] = [
    "clearance",
    "hole_clearance",
    "hole_to_hole",
    "copper_edge_clearance",
    "track_width",
    "via_diameter",
    "annular_width",
    "drill_out_of_range",
    "solder_mask_bridge",
    "shorting_items",
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

/// The bytes of `path` (relative to a checkout root) under the repository root or the nearest
/// ancestor that has it.
fn find(path: &str) -> Option<Vec<u8>> {
    let root = repo().canonicalize().ok()?;
    root.ancestors()
        .find_map(|dir| std::fs::read(dir.join(path)).ok())
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

#[test]
fn the_registration_is_valid_and_names_its_arms_and_metrics() {
    let prereg = registration();
    assert_eq!(prereg.id, "Q22.8-clearance-remeasure");
    assert_eq!(prereg.names.base, "before");
    assert_eq!(prereg.names.cand, "q22");
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    assert_eq!(arms, ["before", "q22"]);
    assert_eq!(prereg.set.as_deref(), Some(SET));
    assert_eq!(prereg.boards.len(), ROWS.len());
    assert!(!prereg.totals_registered);
    assert_eq!(prereg.amendments, 0);
    let roles: Vec<(&str, Role)> = prereg
        .specs
        .iter()
        .map(|s| (s.name.as_str(), s.role))
        .collect();
    assert_eq!(
        &roles[..4],
        [
            ("via_in_pad", Role::Hard),
            ("below_minimum", Role::Hard),
            ("manufacturing_breaches", Role::Hard),
            ("kicad_unconnected", Role::Score),
        ]
    );
    assert!(
        prereg.specs[4..]
            .iter()
            .all(|s| s.role == Role::Report && !s.compared)
    );
    // The verdict pair guards q22; the reported pair only prints before's breaches.
    let pairs: Vec<(&str, &str)> = prereg
        .pairs
        .iter()
        .map(|p| (p.arms.base.as_str(), p.arms.cand.as_str()))
        .collect();
    assert_eq!(pairs, [("before", "q22"), ("q22", "before")]);
}

/// The guard's types are the ten manufacturing types, on the copper basis.
#[test]
fn the_guard_counts_the_manufacturing_types_on_the_copper_basis() {
    let json = json();
    let breaches = string(&json, "metric_definitions.manufacturing_breaches");
    assert!(breaches.contains(&MANUFACTURING.join(", ")), "{breaches}");
    assert!(breaches.contains("routing_caused_basis = copper"));
    let below = string(&json, "metric_definitions.below_minimum");
    assert!(below.contains("copper_key") && below.contains("input_copper_keys"));
    let guards = string(&json, "guards");
    assert!(guards.contains(&MANUFACTURING.join(", ")), "{guards}");
    assert!(guards.contains("before is reported, not guarded"));
}

/// The set lists exactly the registered inputs in the registered order, each with its limit,
/// track mode and router timeout in the comment; every input present hashes as registered, and
/// so do the rules files each grade reads.
#[test]
fn the_rows_match_the_set_with_their_limits_and_hashes() {
    let json = json();
    let rows = object(&json, "boards.rows");
    let set = set_rows();
    assert_eq!(set.len(), ROWS.len());
    assert_eq!(rows.len(), ROWS.len());
    for ((path, comment), (row, limit, keep)) in set.iter().zip(ROWS) {
        let spec = rows
            .get(row)
            .unwrap_or_else(|| panic!("{row} is not registered"));
        assert_eq!(
            spec.get("input").and_then(Json::as_str),
            Some(path.as_str())
        );
        assert!(comment.starts_with(&format!("{row}: ")), "{row}: {comment}");
        assert_eq!(spec.get("timeout_s"), Some(&Json::Number(limit)), "{row}");
        assert!(
            comment.ends_with(&format!(", {limit} s")),
            "{row}: {comment}"
        );
        assert_eq!(spec.get("keep_tracks"), Some(&Json::Bool(keep)), "{row}");
        assert_eq!(comment.contains("--keep-tracks"), keep, "{row}: {comment}");
        assert_eq!(comment.contains("routed whole"), !keep, "{row}: {comment}");
        assert_eq!(spec.get("rules").and_then(Json::as_str), Some("project"));
        // Only fc-rom carries the router's own timeout (a plain kill at 300 s left no SES).
        let args = spec.get("router_args").expect("router_args");
        if row == "fc-rom" {
            let want = ["--timeout", "270"].map(|s| Json::String(s.to_string()));
            assert_eq!(args, &Json::Array(want.to_vec()));
            assert!(comment.contains("router --timeout 270"), "{comment}");
        } else {
            assert_eq!(args, &Json::Array(Vec::new()), "{row}");
            assert!(!comment.contains("--timeout"), "{row}: {comment}");
        }
        assert!(spec.get("limit_source").and_then(Json::as_str).is_some());
        let sha = string(&json, &format!("boards.files.{row}"));
        let Some(bytes) = find(path) else {
            continue;
        };
        assert_eq!(sha256_hex(&bytes), sha, "{row}");
        let dir = Path::new(path.as_str()).parent().unwrap();
        let stem = Path::new(path.as_str())
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap();
        let rules = object(&json, &format!("boards.rules_files.{row}"));
        assert!(rules.contains_key("kicad_pro"), "{row}");
        for (ext, want) in rules {
            let file = dir.join(format!("{stem}.{ext}"));
            let bytes = find(file.to_str().unwrap()).unwrap_or_else(|| panic!("{file:?}"));
            assert_eq!(Some(sha256_hex(&bytes).as_str()), want.as_str(), "{file:?}");
        }
    }
    let files = object(&json, "boards.files");
    assert_eq!(
        files.keys().collect::<Vec<_>>(),
        rows.keys().collect::<Vec<_>>()
    );
}

/// The arms differ only by the engine and plugin commit: every field but `what` is the same up
/// to the arm's name and commit, the grader and the options file are shared, and the options are
/// the plugin's defaults.
#[test]
fn the_arms_differ_by_the_engine_and_plugin_commit_only() {
    let json = json();
    let before = object(&json, "arms.before");
    let q22 = object(&json, "arms.q22");
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        q22.keys().collect::<Vec<_>>()
    );
    assert_eq!(string(&json, "arms.before.commit"), BEFORE_COMMIT);
    assert_eq!(string(&json, "arms.q22.commit"), Q22_COMMIT);
    for (key, value) in before {
        if key == "what" {
            continue;
        }
        let swapped = match value {
            Json::String(s) => Json::String(
                s.replace("before", "q22")
                    .replace(BEFORE_COMMIT, Q22_COMMIT),
            ),
            other => other.clone(),
        };
        assert_eq!(Some(&swapped), q22.get(key), "arms.*.{key}");
    }
    assert_eq!(before.get("grader"), q22.get("grader"));
    assert!(string(&json, "arms.before.grader").contains(Q22_COMMIT));
    let command = string(&json, "arms.before.command");
    for part in [
        "bash tools/kicad-oracle/grade.sh --out Q/out/before/ROW KEEP_TRACKS",
        "--export plugin Q/options.json BOARD",
        "ulimit -v 8000000; exec timeout -k 10 \"$LIMIT\"",
        "env LIMIT=LIMIT",
        "Q/target-before/release/freerouting-rs {DSN} {SES} ROW_ARGS",
        "cd Q/before-tree",
    ] {
        assert!(command.contains(part), "{part} not in {command}");
    }
    let options = object(&json, "arms.before.options");
    assert_eq!(Some(&Json::Object(options.clone())), q22.get("options"));
    assert_eq!(options, object(&json, "options.default"));
    assert_eq!(
        options.get("profile").and_then(Json::as_str),
        Some("enhanced")
    );
    assert_eq!(options.get("schema"), Some(&Json::Number(5.0)));
    assert!(object(&json, "arms.before.settings").is_empty());
    assert!(object(&json, "arms.q22.settings").is_empty());
    // The before tree is a8b4ae7 with the q22 commit's grader put in.
    let steps = match json.path("trees.steps") {
        Some(Json::Array(a)) => a.iter().filter_map(Json::as_str).collect::<Vec<_>>(),
        other => panic!("trees.steps: {other:?}"),
    };
    assert!(steps.iter().any(|s| s.contains("Q/before-tree a8b4ae7")));
    assert!(steps.iter().any(|s| s.contains("Q/q22-tree Q22_COMMIT")));
    assert!(steps.iter().any(|s| {
        s.contains("archive Q22_COMMIT tools/kicad-oracle | tar -x -C Q/before-tree")
    }));
}

#[test]
fn the_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "43bd85aaeb488de5a5a2a8c9ac440bc07495d4e11106f0a51fcb779ffced1a72"
    );
}
