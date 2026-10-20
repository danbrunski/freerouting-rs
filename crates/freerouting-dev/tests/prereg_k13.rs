//! Pins K13.5's registration (`docs/roadmap/prereg/K13.json`): the KiCad plugin's two board
//! inputs, `dsn` (KiCad's DSN export, the default) against `kicad_pcb` (the board snapshot read
//! directly), with the plugin's default options on K10.5's ten boards and the canary-pwr plugin
//! row, judged by K13's switch rule. Registered 2026-10-20 before any route or grade of either
//! arm. A later change goes in its `amendments` with the reason and updates the hash below in the
//! same change.
//!
//! The boards live under the main checkout's `target/` (not in git). A row's file is looked up
//! under the repository root and its ancestors (a worktree under `target/orchestrator/` finds the
//! main checkout's copies); a missing file is skipped, a present one must hash as registered.
use std::path::{Path, PathBuf};

use freerouting::quality::feasibility::report::sha256_hex;
use freerouting_dev::gate::Role;
use freerouting_dev::gate_prereg::Prereg;
use freerouting_dev::json_read::Json;

const FILE: &str = "docs/roadmap/prereg/K13.json";
const SET: &str = "tools/bench/sets/k13-input.txt";
const SCRIPT: &str = "tools/kicad-reader/plugin-input-ab.sh";
/// The rows with K13's pass cap (the few-minute rule), in set order.
const CAPPED: [&str; 4] = [
    "kit-dev-coldfire-xilinx_5213",
    "video",
    "vme-wren",
    "jetson-agx-thor-baseboard",
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

fn object<'a>(json: &'a Json, path: &str) -> &'a std::collections::BTreeMap<String, Json> {
    match json.path(path) {
        Some(Json::Object(o)) => o,
        other => panic!("{path}: {other:?}"),
    }
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
    assert_eq!(prereg.id, "K13.6-board-input");
    assert_eq!(prereg.names.base, "dsn");
    assert_eq!(prereg.names.cand, "kicad_pcb");
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    assert_eq!(arms, ["dsn", "kicad_pcb"]);
    assert_eq!(prereg.set.as_deref(), Some(SET));
    assert_eq!(prereg.boards.len(), 11);
    assert!(prereg.totals_registered);
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
    assert!(prereg.specs[4..].iter().all(|s| s.role == Role::Report));
    // The verdict pair judges kicad_pcb; the reported pair puts dsn under the hard metrics too.
    let pairs: Vec<(&str, &str)> = prereg
        .pairs
        .iter()
        .map(|p| (p.arms.base.as_str(), p.arms.cand.as_str()))
        .collect();
    assert_eq!(pairs, [("dsn", "kicad_pcb"), ("kicad_pcb", "dsn")]);
}

/// The totals rule with noise 0 on every row is K13's "no higher" total exactly.
#[test]
fn the_total_clause_is_no_higher_with_zero_noise() {
    let json = json();
    assert_eq!(json.path("rule").and_then(Json::as_str), Some("totals"));
    assert_eq!(
        json.path("totals.accept").and_then(Json::as_str),
        Some("no-regress")
    );
    let rows = object(&json, "boards.rows");
    let noise = object(&json, "totals.noise");
    assert_eq!(
        noise.keys().collect::<Vec<_>>(),
        rows.keys().collect::<Vec<_>>()
    );
    assert!(
        noise
            .values()
            .all(|b| matches!(b, Json::Number(n) if *n == 0.0))
    );
}

/// The set lists exactly the registered inputs, in its own order, with each row's cap and
/// timeout in the comment; every input present hashes as registered, and so do its rules.
#[test]
fn the_rows_match_the_set_and_their_hashes() {
    let json = json();
    let rows = object(&json, "boards.rows");
    let set = set_rows();
    assert_eq!(set.len(), rows.len());
    for (path, comment) in &set {
        let (row, spec) = rows
            .iter()
            .find(|(_, spec)| spec.get("input").and_then(Json::as_str) == Some(path))
            .unwrap_or_else(|| panic!("{path} is in {SET} but not registered"));
        let capped = matches!(spec.get("pass_cap"), Some(Json::Bool(true)));
        assert_eq!(capped, CAPPED.contains(&row.as_str()), "{row}");
        assert_eq!(comment.contains("max_passes 1"), capped, "{row}: {comment}");
        let timeout = match spec.get("timeout_s") {
            Some(Json::Number(n)) => *n,
            other => panic!("{row}: timeout_s {other:?}"),
        };
        assert!(
            comment.ends_with(&format!("{timeout} s")),
            "{row}: {comment}"
        );
        assert_eq!(spec.get("rules").and_then(Json::as_str), Some("project"));
        let Some(bytes) = find(path) else {
            continue;
        };
        let sha = json
            .path(&format!("boards.files.{row}"))
            .and_then(Json::as_str)
            .unwrap_or_else(|| panic!("boards.files.{row}"));
        assert_eq!(sha256_hex(&bytes), sha, "{row}");
        let dir = Path::new(path.as_str()).parent().unwrap();
        let stem = Path::new(path.as_str())
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap();
        for (ext, want) in object(&json, &format!("boards.rules_files.{row}")) {
            let file = dir.join(format!("{stem}.{ext}"));
            let bytes = find(file.to_str().unwrap()).unwrap_or_else(|| panic!("{file:?}"));
            assert_eq!(Some(sha256_hex(&bytes).as_str()), want.as_str(), "{file:?}");
        }
    }
}

/// The arms differ by `board_input` and nothing else: the same engine, options, settings and
/// command up to the arm's name; the capped rows carry the same cap into both.
#[test]
fn the_arms_differ_by_the_board_input_only() {
    let json = json();
    let arm = |name: &str, key: &str| -> String {
        json.path(&format!("arms.{name}.{key}"))
            .and_then(Json::as_str)
            .unwrap_or_else(|| panic!("arms.{name}.{key}"))
            .to_string()
    };
    assert_eq!(arm("dsn", "engine"), arm("kicad_pcb", "engine"));
    assert_eq!(arm("dsn", "board_input"), "dsn");
    assert_eq!(arm("kicad_pcb", "board_input"), "kicad_pcb");
    let swapped = arm("dsn", "command")
        .replace("kicad_pcb", "\0")
        .replace("dsn", "kicad_pcb")
        .replace('\0', "dsn");
    assert_eq!(swapped, arm("kicad_pcb", "command"));
    assert!(arm("dsn", "command").contains(SCRIPT));
    assert!(arm("dsn", "command").contains("ROUTE_MEMORY_KB=8000000"));
    let options = |name: &str| {
        let mut o = object(&json, &format!("arms.{name}.options")).clone();
        assert_eq!(
            o.remove("board_input").as_ref().and_then(Json::as_str),
            Some(name)
        );
        o
    };
    let dsn = options("dsn");
    assert_eq!(dsn, options("kicad_pcb"));
    assert_eq!(&dsn, object(&json, "options.default"));
    assert_eq!(dsn.get("profile").and_then(Json::as_str), Some("enhanced"));
    for name in ["dsn", "kicad_pcb"] {
        assert!(object(&json, &format!("arms.{name}.settings")).is_empty());
    }
    let cap = object(&json, "options.pass_cap");
    let cap_args = cap.get("router_args").expect("router_args");
    for (row, spec) in object(&json, "boards.rows") {
        let mut want = dsn.clone();
        let capped = CAPPED.contains(&row.as_str());
        if capped {
            want.extend(object(&json, "options.pass_cap.options").clone());
        }
        assert_eq!(spec.get("options"), Some(&Json::Object(want)), "{row}");
        let args = spec.get("router_args").expect("router_args");
        if capped {
            assert_eq!(args, cap_args, "{row}");
        } else {
            assert_eq!(args, &Json::Array(Vec::new()), "{row}");
        }
    }
}

/// The measuring script is the registered one: a change to it after registration is an
/// amendment.
#[test]
fn the_script_is_the_registered_one() {
    let json = json();
    assert_eq!(
        json.path("script.path").and_then(Json::as_str),
        Some(SCRIPT)
    );
    let bytes = std::fs::read(repo().join(SCRIPT)).unwrap();
    assert_eq!(
        json.path("script.sha256").and_then(Json::as_str),
        Some(sha256_hex(&bytes).as_str())
    );
}

#[test]
fn the_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "a470fa311beffa3f80af1663c251fdf3234445b3d213c34bbc00668ee7408770"
    );
}
