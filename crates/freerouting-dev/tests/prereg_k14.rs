//! Pins K14.6's registration (`docs/roadmap/prereg/K14.json`): `canary-pwr` at canary
//! `95882f00` with its own saved `route-options.json` (class widths, fixed pour classes, dropped
//! pours), routed on the KiCad plugin's two board inputs, `dsn` (KiCad's DSN export with the
//! plugin's DSN fix-ups) against `kicad_pcb` (the board snapshot with K14's router settings).
//! Registered 2026-10-21 before K14.1-K14.5 landed and before any route or grade of either arm.
//!
//! How it is amended: a change goes in the registration's `amendments` with its date and reason,
//! and updates the hash in `the_registration_is_pinned_by_its_hash` in the same change. The
//! registration asks for amendment 1 after the build and before either arm runs: it names the
//! commit, the binary, the grader and plugin hashes and the router flags K14.1, K14.3 and K14.4
//! added. Nothing registered (arms, board, options, guards, rule, noise) is edited after a
//! number exists.
//!
//! The board lives under the main checkout's `target/` (not in git). Its file is looked up under
//! the repository root and its ancestors (a worktree under `target/orchestrator/` finds the main
//! checkout's copy); a missing file is skipped, a present one must hash as registered.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use freerouting::quality::feasibility::report::sha256_hex;
use freerouting_dev::gate::Role;
use freerouting_dev::gate_prereg::Prereg;
use freerouting_dev::json_read::Json;

const FILE: &str = "docs/roadmap/prereg/K14.json";
const SET: &str = "tools/bench/sets/k14-fixups.txt";
const SCRIPT: &str = "tools/kicad-reader/plugin-input-ab.sh";
const ROW: &str = "canary-pwr-95882f00";

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

fn set_text() -> String {
    std::fs::read_to_string(repo().join(SET)).unwrap()
}

/// The set file's rows: (path, comment).
fn set_rows() -> Vec<(String, String)> {
    set_text()
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
    assert_eq!(prereg.id, "K14.6-saved-fixups");
    assert_eq!(prereg.names.base, "dsn");
    assert_eq!(prereg.names.cand, "kicad_pcb");
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    assert_eq!(arms, ["dsn", "kicad_pcb"]);
    assert_eq!(prereg.set.as_deref(), Some(SET));
    assert_eq!(prereg.boards.keys().collect::<Vec<_>>(), [ROW]);
    assert!(prereg.totals_registered);
    assert_eq!(prereg.amendments, 1);
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

/// The unconnected clause's noise is canary-pwr's registered band (K09.json, N02's variant
/// spread), the only row's, and the metrics are K13.6's.
#[test]
fn the_noise_is_canary_pwrs_registered_band() {
    let json = json();
    assert_eq!(json.path("rule").and_then(Json::as_str), Some("totals"));
    assert_eq!(string(&json, "totals.accept"), "no-regress");
    let noise = object(&json, "totals.noise");
    assert_eq!(
        noise.keys().collect::<Vec<_>>(),
        object(&json, "boards.rows").keys().collect::<Vec<_>>()
    );
    let k09 =
        Json::parse(&std::fs::read_to_string(repo().join("docs/roadmap/prereg/K09.json")).unwrap())
            .unwrap();
    let band = k09
        .path("noise.canary-pwr")
        .expect("K09.json noise.canary-pwr");
    assert_eq!(noise.get(ROW), Some(band));
    assert_eq!(band, &Json::Number(10.0));
    assert!(string(&json, "noise_why").contains("K09.json"));
    let k13 =
        Json::parse(&std::fs::read_to_string(repo().join("docs/roadmap/prereg/K13.json")).unwrap())
            .unwrap();
    for key in ["metrics", "metric_definitions"] {
        assert_eq!(json.get(key), k13.get(key), "{key}");
    }
}

/// The set lists exactly the registered input, with its cap and timeout in the comment and the
/// options file in its header; the input present hashes as registered, and so do its rules.
#[test]
fn the_row_matches_the_set_and_its_hashes() {
    let json = json();
    let set = set_rows();
    assert_eq!(set.len(), 1);
    let (path, comment) = &set[0];
    assert_eq!(path, string(&json, &format!("boards.rows.{ROW}.input")));
    assert_eq!(
        json.path(&format!("boards.rows.{ROW}.pass_cap")),
        Some(&Json::Bool(false))
    );
    assert_eq!(
        json.path(&format!("boards.rows.{ROW}.router_args")),
        Some(&Json::Array(Vec::new()))
    );
    let timeout = match json.path(&format!("boards.rows.{ROW}.timeout_s")) {
        Some(Json::Number(n)) => *n,
        other => panic!("timeout_s {other:?}"),
    };
    assert!(comment.ends_with(&format!("{timeout} s")), "{comment}");
    assert!(comment.contains("max_passes 1"), "{comment}");
    assert_eq!(
        string(&json, &format!("boards.rows.{ROW}.rules")),
        "project"
    );
    assert!(set_text().contains(string(&json, "options.path")));
    let Some(bytes) = find(path) else {
        return;
    };
    assert_eq!(
        sha256_hex(&bytes),
        string(&json, &format!("boards.files.{ROW}"))
    );
    let dir = Path::new(path.as_str()).parent().unwrap();
    for (ext, want) in object(&json, &format!("boards.rules_files.{ROW}")) {
        let file = dir.join(format!("canary-pwr.{ext}"));
        let bytes = find(file.to_str().unwrap()).unwrap_or_else(|| panic!("{file:?}"));
        assert_eq!(Some(sha256_hex(&bytes).as_str()), want.as_str(), "{file:?}");
    }
}

/// The options are the board's saved file, recorded verbatim: its text hashes as registered,
/// parses to `options.saved`, and the copy beside the board (when present) is that text.
#[test]
fn the_options_are_the_saved_file_verbatim() {
    let json = json();
    let saved_text = string(&json, "options.saved_text");
    assert_eq!(
        sha256_hex(saved_text.as_bytes()),
        string(&json, "options.sha256")
    );
    let saved = Json::parse(saved_text).unwrap();
    assert_eq!(json.path("options.saved"), Some(&saved));
    assert_eq!(
        json.path(&format!("boards.rows.{ROW}.options")),
        Some(&saved)
    );
    let number = |key: &str| saved.get(key).and_then(Json::as_f64);
    assert_eq!(number("schema"), Some(4.0));
    assert_eq!(number("max_passes"), Some(1.0));
    assert_eq!(number("min_trace_width_um"), Some(150.0));
    assert_eq!(number("start_ripup_costs"), Some(1500.0));
    assert_eq!(number("sweep_passes"), Some(2.0));
    assert_eq!(saved.get("board_input"), None);
    let widths: Vec<(&str, f64)> = object(&saved, "class_widths")
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_f64().unwrap()))
        .collect();
    assert_eq!(
        widths,
        [
            ("GND", 0.4),
            ("Power", 0.3),
            ("Power_High", 0.3),
            ("Switch_Node", 0.3)
        ]
    );
    let names = |key: &str| match saved.get(key) {
        Some(Json::Array(list)) => list.iter().filter_map(Json::as_str).collect::<Vec<_>>(),
        other => panic!("{key}: {other:?}"),
    };
    assert_eq!(names("fixed_pour_classes"), ["Power_High", "Switch_Node"]);
    assert_eq!(names("drop_pour_nets"), ["GND"]);
    if let Some(bytes) = find(string(&json, "options.path")) {
        assert_eq!(bytes, saved_text.as_bytes());
    }
}

/// The arms differ by `board_input` and nothing else: the same engine, options, settings and
/// command up to the arm's name, with the row's timeout and the registered memory cap; the
/// queued commands are those, spelled out.
#[test]
fn the_arms_differ_by_the_board_input_only() {
    let json = json();
    let arm = |name: &str, key: &str| string(&json, &format!("arms.{name}.{key}")).to_string();
    assert_eq!(arm("dsn", "engine"), arm("kicad_pcb", "engine"));
    assert_eq!(arm("dsn", "board_input"), "dsn");
    assert_eq!(arm("kicad_pcb", "board_input"), "kicad_pcb");
    let swapped = arm("dsn", "command")
        .replace("kicad_pcb", "\0")
        .replace("dsn", "kicad_pcb")
        .replace('\0', "dsn");
    assert_eq!(swapped, arm("kicad_pcb", "command"));
    let command = arm("dsn", "command");
    assert!(command.contains(SCRIPT));
    assert!(command.contains("ROUTE_MEMORY_KB=8000000"));
    assert!(command.contains("ROUTE_TIMEOUT_S=1800 "));
    assert!(command.contains("SHARE_GRADE_WITH=OUT/kicad_pcb/"));
    assert_eq!(
        json.path(&format!("boards.rows.{ROW}.timeout_s")),
        Some(&Json::Number(1800.0))
    );
    let saved = object(&json, "options.saved");
    for name in ["dsn", "kicad_pcb"] {
        let mut o = object(&json, &format!("arms.{name}.options")).clone();
        assert_eq!(
            o.remove("board_input").as_ref().and_then(Json::as_str),
            Some(name)
        );
        assert_eq!(&o, saved, "{name}");
        assert!(object(&json, &format!("arms.{name}.settings")).is_empty());
    }
    let heavy: Vec<&str> = match json.get("heavy_run") {
        Some(Json::Array(list)) => list.iter().filter_map(Json::as_str).collect(),
        other => panic!("heavy_run: {other:?}"),
    };
    let board = string(&json, &format!("boards.rows.{ROW}.input"));
    let options = string(&json, "options.path");
    for name in ["dsn", "kicad_pcb"] {
        let call = format!("plugin-input-ab.sh {name} {board} {options} ");
        assert_eq!(
            heavy.iter().filter(|c| c.contains(&call)).count(),
            1,
            "{name}"
        );
    }
}

/// The measuring script is the registered one, K13.5's unchanged: a change to it after
/// registration is an amendment (of K13.json too).
#[test]
fn the_script_is_the_registered_one() {
    let json = json();
    assert_eq!(string(&json, "script.path"), SCRIPT);
    let bytes = std::fs::read(repo().join(SCRIPT)).unwrap();
    assert_eq!(string(&json, "script.sha256"), sha256_hex(&bytes));
    let k13 =
        Json::parse(&std::fs::read_to_string(repo().join("docs/roadmap/prereg/K13.json")).unwrap())
            .unwrap();
    assert_eq!(k13.path("script.sha256"), json.path("script.sha256"));
}

#[test]
fn the_registration_is_pinned_by_its_hash() {
    assert_eq!(
        registration().sha256,
        "c554c5b0b2183e32eab66cc5311354e58e22c67f5fd516dc42d9db05053350cb"
    );
}
