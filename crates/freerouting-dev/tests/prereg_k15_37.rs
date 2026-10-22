//! Pins K15.37's registration (`docs/roadmap/prereg/K15.37.json`): `router.pour_fill_contacts`
//! (K15.31) together with `router.pour_fill_target` (K15.25) under profile `enhanced` and the KiCad
//! plugin's default options, both unset against both set, on the plugin's `.kicad_pcb` input only
//! (neither setting acts on a DSN), over the board set `tools/bench/sets/k15-fill-pair.txt`:
//! K15.30's 17 rows (K15.14's), the same stripped boards by absolute path. Registered 2026-10-22
//! before any route or grade of its arms.
//!
//! The registration is K15.30's with the differences it lists in `differences`: the candidate arm
//! carries two flags, its own tree, output and build directories, validity (a) reads both settings
//! from the result manifests, validity (b) names a row only the candidate times out on and guard
//! (4) counts its time, and the decision table's outcomes name both settings. The helper
//! (`tools/bench/k15_all_pours.py`) is K15.14's, unchanged: it runs and collects whatever
//! `commands` and `metrics` a registration lists, so two flags in one command need nothing in it.
//!
//! How it is amended: a change goes in the registration's `amendments` with its date and reason,
//! and updates the hashes here in the same change (`AMENDMENTS` too). The registration asks for
//! amendment 1 after the build and before any arm runs (the commit, the binary, the grader and
//! plugin hashes). Nothing registered (arms, boards, options, guards, rule, noise) is edited after a
//! number exists.
//!
//! The boards live under the main checkout's `target/` (not in git), by absolute path; a missing
//! board is skipped, a present one must hash as registered, and so must its rules files.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use freerouting::quality::feasibility::report::sha256_hex;
use freerouting_dev::gate::Role;
use freerouting_dev::gate_prereg::Prereg;
use freerouting_dev::json_read::Json;

const FILE: &str = "docs/roadmap/prereg/K15.37.json";
const K15_30: &str = "docs/roadmap/prereg/K15.30.json";
const K15_14: &str = "docs/roadmap/prereg/K15.14.json";
const SET: &str = "tools/bench/sets/k15-fill-pair.txt";
const K15_30_SET: &str = "tools/bench/sets/k15-fill-target.txt";
const SCRIPT: &str = "tools/kicad-reader/plugin-input-ab.sh";
const HELPER: &str = "tools/bench/k15_all_pours.py";
const OPTIONS: &str = "tools/bench/k15-all-pours.options.json";
const K15_30_FLAG: &str = "--router.pour_fill_target=true";
/// The candidate arm's flags, in this order (the order of the orchestrator's check on `c4f74fb`).
const FLAGS: &str = "--router.pour_fill_contacts=true --router.pour_fill_target=true";
const ARMS: [&str; 2] = ["pcb-off", "pcb-on"];
const MUST_JUDGE: [&str; 2] = ["pcu-76163c7", "canary-pwr-95882f00"];
const TREE: &str = "target/orchestrator/k1537/tree";
const OUT: &str = "target/orchestrator/k1537/out";
/// 17 rows x 2 arms (`.kicad_pcb` only).
const CALLS: usize = 34;
const AMENDMENTS: usize = 1;
const REGISTRATION_SHA256: &str =
    "39a247bcc4536cf9191537056b834999c6891392ce1704dc774538086cfe52a1";
const SET_SHA256: &str = "fc34109cde3f899ecfdc40dd50e477a74dcc7e5b6bf50a4b74bf1413aa355adb";
/// K15.14's registered helper (`helper.sha256` there), unchanged.
const HELPER_SHA256: &str = "8de88dd8209eb8d073d596bcba200fdcef9f1d5b0c4a9210136103214723488f";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &str) -> String {
    std::fs::read_to_string(repo().join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn json() -> Json {
    Json::parse(&read(FILE)).unwrap()
}

fn k15_30() -> Json {
    Json::parse(&read(K15_30)).unwrap()
}

fn k15_14() -> Json {
    Json::parse(&read(K15_14)).unwrap()
}

fn registration() -> Prereg {
    Prereg::parse(&read(FILE)).unwrap_or_else(|e| panic!("{FILE} is invalid: {e:?}"))
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

fn strings<'a>(json: &'a Json, key: &str) -> Vec<&'a str> {
    match json.get(key) {
        Some(Json::Array(list)) => list.iter().filter_map(Json::as_str).collect(),
        other => panic!("{key}: {other:?}"),
    }
}

fn number(json: &Json, path: &str) -> f64 {
    match json.path(path) {
        Some(Json::Number(n)) => *n,
        other => panic!("{path}: {other:?}"),
    }
}

/// A set file's rows: (path, comment), in file order.
fn set_rows(set: &str) -> Vec<(String, String)> {
    read(set)
        .lines()
        .filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let (path, comment) = l.split_once('#').unwrap_or((l, ""));
            (path.trim().to_string(), comment.trim().to_string())
        })
        .collect()
}

/// The registration's rows in the set file's order (the JSON reader keeps objects sorted).
fn row_names() -> Vec<String> {
    let json = json();
    let rows = object(&json, "boards.rows");
    set_rows(SET)
        .iter()
        .map(|(path, _)| {
            rows.iter()
                .find(|(_, v)| v.get("input").and_then(Json::as_str) == Some(path.as_str()))
                .map(|(k, _)| k.clone())
                .unwrap_or_else(|| panic!("set row {path} is not a registered input"))
        })
        .collect()
}

#[test]
fn the_registration_is_valid_and_names_its_arms_and_metrics() {
    let prereg = registration();
    assert_eq!(prereg.id, "K15.37-fill-pair");
    assert_eq!(prereg.names.base, "pcb-off");
    assert_eq!(prereg.names.cand, "pcb-on");
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    assert_eq!(arms, ARMS);
    assert_eq!(prereg.set.as_deref(), Some(SET));
    assert!(prereg.totals_registered);
    assert_eq!(prereg.amendments, AMENDMENTS);
    assert_eq!(prereg.must_judge, MUST_JUDGE);
    // K15.30's metrics, in the same order and roles (K15.14's without the pour-guard count).
    let k30 = Prereg::parse(&read(K15_30)).unwrap();
    let want: Vec<(&str, Role)> = k30
        .specs
        .iter()
        .map(|s| (s.name.as_str(), s.role))
        .collect();
    let got: Vec<(&str, Role)> = prereg
        .specs
        .iter()
        .map(|s| (s.name.as_str(), s.role))
        .collect();
    assert_eq!(got, want);
    assert!(got.iter().all(|(name, _)| *name != "pour_guard_pours"));
    assert_eq!(
        &got[..4],
        [
            ("via_in_pad", Role::Hard),
            ("below_minimum", Role::Hard),
            ("manufacturing_breaches", Role::Hard),
            ("kicad_unconnected", Role::Score),
        ]
    );
    assert!(prereg.specs[4..].iter().all(|s| s.role == Role::Report));
    let pairs: Vec<(&str, &str)> = prereg
        .pairs
        .iter()
        .map(|p| (p.arms.base.as_str(), p.arms.cand.as_str()))
        .collect();
    assert_eq!(pairs, [("pcb-off", "pcb-on"), ("pcb-on", "pcb-off")]);
    // Every metric the registration lists has its definition.
    let json = json();
    let definitions = object(&json, "metric_definitions");
    for spec in &prereg.specs {
        assert!(definitions.contains_key(&spec.name), "{}", spec.name);
    }
    assert_eq!(definitions.len(), prereg.specs.len());
    assert_eq!(json.get("metrics"), k15_30().get("metrics"));
    assert_eq!(
        json.get("metric_definitions"),
        k15_30().get("metric_definitions")
    );
}

/// K15.30's totals rule, noise and the guards' bands; both owner boards must be judged; validity
/// reads both settings from the manifests and names a row only the candidate times out on.
#[test]
fn the_rule_noise_and_guards_are_k15_30s() {
    let json = json();
    let k30 = k15_30();
    assert_eq!(string(&json, "rule"), "totals");
    assert_eq!(json.get("totals"), k30.get("totals"));
    assert_eq!(json.get("totals"), k15_14().get("totals"));
    assert_eq!(string(&json, "totals.accept"), "improve");
    assert_eq!(number(&json, "totals.min_boards"), 3.0);
    let k09 = Json::parse(&read("docs/roadmap/prereg/K09.json")).unwrap();
    assert_eq!(
        json.path("totals.noise.canary-pwr-95882f00"),
        k09.path("noise.canary-pwr")
    );
    let noise_why = string(&json, "noise_why");
    assert!(noise_why.contains("max(2, ceil(5 % of its connections))"));
    for fact in [
        "67.09",
        "72.32",
        "852",
        "331",
        "k1537pre/runs",
        "Nothing of this set was run to derive it",
    ] {
        assert!(noise_why.contains(fact), "noise_why: {fact}");
    }
    let guards = string(&json, "guards");
    for clause in [
        "absolutely",
        "no row worse than its own noise band",
        "1.25 x",
        "COMPLETED in pcb-off and TIMED_OUT in pcb-on",
    ] {
        assert!(guards.contains(clause), "{clause}");
    }
    let validity = string(&json, "validity");
    assert!(validity.contains("left out of the judgement and reported by name"));
    assert!(validity.contains("fewer than min_boards (3) distinct rows are judged"));
    assert!(validity.contains("pcu-76163c7 or canary-pwr-95882f00 is not judged"));
    // Validity (a): both settings reached the router, from the result manifests.
    assert!(validity.contains("settings_snapshot.pour_fill_target"));
    assert!(validity.contains("settings_snapshot.pour_fill_contacts"));
    assert!(validity.contains("COMPLETED in pcb-off and TIMED_OUT in pcb-on"));
    assert_eq!(strings(&json, "must_judge"), MUST_JUDGE);
    assert!(json.get("dsn_rows").is_none());
    let no_dsn = string(&json, "no_dsn_why");
    assert!(no_dsn.contains("read_from_kicad_pcb"));
    assert!(no_dsn.contains("pour_fill_contacts"));
    assert!(!strings(&json, "differences").is_empty());
    // The limits are K15.30's (pass cap, router deadline, call timeout, memory).
    let limits = string(&json, "limits");
    assert!(limits.contains("--max-passes 2 --timeout 240"));
    assert!(limits.contains("ROUTE_TIMEOUT_S"));
    assert!(limits.contains("34 calls"));
    assert!(string(&json, "grading").contains("SHARE_GRADE_WITH"));
}

/// The decision table: kept puts both settings in `ENHANCED` and settles the plugin's K15.30
/// option, not kept is offered by the plugin as `pour_fill_contacts` beside it, a breach is
/// neither offered nor kept until fixed; every outcome maps to one row.
#[test]
fn the_decision_table_maps_each_outcome_to_one_row() {
    let json = json();
    let table = object(&json, "decision_table");
    assert_eq!(
        table.keys().map(String::as_str).collect::<Vec<_>>(),
        [
            "breach",
            "kept",
            "not kept, guard",
            "not kept, no gain",
            "not measured",
            "order"
        ]
    );
    assert_eq!(
        table.keys().collect::<Vec<_>>(),
        object(&k15_30(), "decision_table")
            .keys()
            .collect::<Vec<_>>()
    );
    let row = |key: &str| table[key].as_str().unwrap();
    let kept = row("kept");
    for fact in [
        "router.pour_fill_contacts",
        "router.pour_fill_target",
        "ENHANCED",
        "DIVERGENCES",
        "parity-check.sh",
        "tests/settings_defaults.rs",
        "removed",
        "RouteOptions.pour_fill_target",
    ] {
        assert!(kept.contains(fact), "kept: {fact}");
    }
    for key in ["not kept, no gain", "not kept, guard"] {
        assert!(row(key).contains("options.py"), "{key}");
        assert!(row(key).contains("unset in both profiles"), "{key}");
        assert!(row(key).contains("pour_fill_contacts"), "{key}");
        assert!(row(key).contains("pour_fill_target"), "{key}");
    }
    assert!(row("breach").contains("the plugin does not offer"));
    assert!(row("breach").contains("pour_fill_contacts"));
    assert!(row("order").contains("not measured, breach, not kept no gain, not kept guard, kept"));
}

/// The set lists K15.30's rows, verbatim and in order, under its own header; the boards and rules
/// files are K15.14's and K15.30's; every present board and its rules files hash as registered.
#[test]
fn the_rows_are_k15_30s_and_match_their_hashes() {
    let json = json();
    let k30 = k15_30();
    let text = read(SET);
    assert_eq!(sha256_hex(text.as_bytes()), SET_SHA256);
    assert_eq!(string(&json, "boards.set"), SET);
    assert_eq!(string(&json, "boards.set_sha256"), SET_SHA256);
    assert!(text.starts_with("# K15.37 (docs/roadmap/prereg/K15.37.json)"));
    assert_eq!(set_rows(SET), set_rows(K15_30_SET));
    for key in ["rows", "files", "rules_files"] {
        let path = format!("boards.{key}");
        assert_eq!(json.path(&path), k30.path(&path), "{path}");
        assert_eq!(json.path(&path), k15_14().path(&path), "{path}");
    }
    let rows = object(&json, "boards.rows");
    assert_eq!(rows.len(), 17);
    assert!(rows.contains_key("pocketsdr-v3-0"));
    for ((path, comment), row) in set_rows(SET).iter().zip(row_names()) {
        assert!(Path::new(path).is_absolute(), "{path}");
        assert!(comment.starts_with(&format!("{row}:")), "{comment}");
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        assert_eq!(
            sha256_hex(&bytes),
            string(&json, &format!("boards.files.{row}")),
            "{row}"
        );
        let board = Path::new(path.as_str());
        let stem = board.file_stem().unwrap().to_str().unwrap();
        for (ext, want) in object(&json, &format!("boards.rules_files.{row}")) {
            let file = board.with_file_name(format!("{stem}.{ext}"));
            let bytes = std::fs::read(&file).unwrap_or_else(|e| panic!("{file:?}: {e}"));
            assert_eq!(Some(sha256_hex(&bytes).as_str()), want.as_str(), "{file:?}");
        }
    }
}

/// The options are the plugin's defaults, verbatim (K15.14's file).
#[test]
fn the_options_are_the_plugin_defaults() {
    let json = json();
    let text = read(OPTIONS);
    assert_eq!(text, "{\"schema\": 6}\n");
    assert_eq!(json.get("options"), k15_30().get("options"));
    assert_eq!(string(&json, "options.sha256"), sha256_hex(text.as_bytes()));
}

/// Every command is the arm's template filled in, and it is K15.30's command of the row with this
/// registration's tree, output and build directories and, on `pcb-on`, the two flags.
#[test]
fn the_commands_are_k15_30s_with_the_two_flags() {
    let json = json();
    let k30 = k15_30();
    assert_eq!(string(&json, "run.out"), OUT);
    assert_eq!(string(&json, "run.root"), string(&k30, "run.root"));
    let engine = string(&json, "placeholders.ENGINE")
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    assert_eq!(
        engine,
        "/home/thebo/code/freerouting-rs/target/orchestrator/k1537/target-release/release/freerouting-rs"
    );
    for arm in ARMS {
        let a = |key: &str| format!("arms.{arm}.{key}");
        assert_eq!(string(&json, &a("board_input")), "kicad_pcb");
        assert_eq!(string(&json, &a("engine")), "as `engine`");
        assert_eq!(json.path(&a("options")), k30.path(&a("options")));
        let settings = object(&json, &a("settings"));
        let on = arm == "pcb-on";
        assert_eq!(settings.len(), if on { 2 } else { 0 });
        if on {
            for key in ["router.pour_fill_contacts", "router.pour_fill_target"] {
                assert_eq!(settings.get(key), Some(&Json::Bool(true)), "{key}");
            }
        }
        let template = string(&json, &a("command"));
        let k30_template = string(&k30, &a("command"))
            .replace("/k1530/", "/k1537/")
            .replace(K15_30_FLAG, FLAGS);
        assert_eq!(template, k30_template, "{arm}");
    }
    let commands = object(&json, "commands");
    assert_eq!(commands.len(), row_names().len());
    let calls: usize = commands
        .values()
        .map(|c| match c {
            Json::Object(o) => o.len(),
            other => panic!("{other:?}"),
        })
        .sum();
    assert_eq!(calls, CALLS);
    // K15.30's directories become this registration's; the boards stay where K15.14 made them.
    let ours = |k30_command: &str| {
        k30_command
            .replace("/k1530/", "/k1537/")
            .replace(K15_30_FLAG, FLAGS)
    };
    for row in row_names() {
        let board = string(&json, &format!("boards.rows.{row}.input"));
        let timeout = number(&json, &format!("boards.rows.{row}.timeout_s"));
        let by_arm = object(&json, &format!("commands.{row}"));
        assert_eq!(
            by_arm.keys().map(String::as_str).collect::<Vec<_>>(),
            ARMS,
            "{row}"
        );
        for arm in ARMS {
            let template = string(&json, &format!("arms.{arm}.command"));
            let filled = template
                .replace(
                    "FREEROUTING_BIN=ENGINE ",
                    &format!("FREEROUTING_BIN={engine} "),
                )
                .replace(
                    "ROUTE_TIMEOUT_S=TIMEOUT ",
                    &format!("ROUTE_TIMEOUT_S={timeout} "),
                )
                .replace("OUT/ROW/", &format!("{OUT}/{row}/"))
                .replace(" TREE/", &format!(" {TREE}/"))
                .replace(" BOARD ", &format!(" {board} "))
                .replace(" OPTIONS ", &format!(" {TREE}/{OPTIONS} "));
            let call = by_arm[arm].as_str().unwrap();
            assert_eq!(call, filled, "{row} {arm}");
            let k30_call = string(&k30, &format!("commands.{row}.{arm}"));
            assert_eq!(call, ours(k30_call), "{row} {arm}");
            assert!(call.contains(&format!(" {board} ")), "{call}");
            assert!(call.contains(" -- --max-passes 2 --timeout 240"), "{call}");
            assert_eq!(call.ends_with(FLAGS), arm == "pcb-on", "{call}");
            assert_eq!(
                call.contains("SHARE_GRADE_WITH="),
                arm == "pcb-on",
                "{call}"
            );
            // No other setting reaches either arm.
            assert_eq!(
                call.matches(" --router.").count(),
                2 * usize::from(arm == "pcb-on")
            );
        }
    }
    let heavy = strings(&json, "heavy_run");
    let run = format!("{TREE}/{HELPER} run {TREE}/{FILE}");
    assert_eq!(heavy.iter().filter(|c| c.contains(&run)).count(), 1);
    assert!(heavy[0].contains("<K15_37_COMMIT"));
    assert!(heavy.iter().any(|c| c.contains("--test prereg_k15_37")));
    assert!(heavy.iter().any(|c| c.contains("amendment 1")));
    assert!(heavy.iter().any(|c| c.contains("--left-out ROW=REASON")));
    assert!(heavy.iter().any(|c| c.contains("--all-pairs")));
}

/// The helper's dry run prints exactly the registered commands of every present row, in order
/// (`run` runs them verbatim). Skipped where `python3` cannot be started.
#[test]
fn the_helper_runs_the_registered_commands() {
    let output = match Command::new("python3")
        .args(["-I", HELPER, "run", FILE, "--dry-run"])
        .current_dir(repo())
        .output()
    {
        Ok(output) => output,
        Err(e) => {
            eprintln!("python3 not started ({e}); the dry run is not checked");
            return;
        }
    };
    assert!(output.status.success(), "{output:?}");
    let json = json();
    let mut want = Vec::new();
    for row in row_names() {
        if !Path::new(string(&json, &format!("boards.rows.{row}.input"))).is_file() {
            continue;
        }
        for arm in ARMS {
            let call = string(&json, &format!("commands.{row}.{arm}"));
            want.push(format!("{row} {arm}: {call}"));
        }
    }
    let got: Vec<&str> = std::str::from_utf8(&output.stdout)
        .unwrap()
        .lines()
        .collect();
    assert_eq!(got, want);
}

/// The measuring script is K13.5's and the helper K15.14's, both unchanged.
#[test]
fn the_scripts_are_the_registered_ones() {
    let json = json();
    assert_eq!(json.get("script"), k15_14().get("script"));
    assert_eq!(json.get("script"), k15_30().get("script"));
    let script = std::fs::read(repo().join(SCRIPT)).unwrap();
    assert_eq!(string(&json, "script.sha256"), sha256_hex(&script));
    let k13 = Json::parse(&read("docs/roadmap/prereg/K13.json")).unwrap();
    assert_eq!(k13.path("script.sha256"), json.path("script.sha256"));
    assert_eq!(string(&json, "helper.path"), HELPER);
    let helper = std::fs::read(repo().join(HELPER)).unwrap();
    assert_eq!(sha256_hex(&helper), HELPER_SHA256);
    assert_eq!(string(&json, "helper.sha256"), HELPER_SHA256);
    assert_eq!(string(&k15_14(), "helper.sha256"), HELPER_SHA256);
    assert_eq!(string(&k15_30(), "helper.sha256"), HELPER_SHA256);
}

/// What was seen before the registration is recorded with its sessions, so the later run can be
/// read against it.
#[test]
fn the_registration_records_what_was_seen_before_it() {
    let json = json();
    let seen = string(&json, "seen_before_registration");
    for fact in [
        "c4f74fb",
        "2e8bf394",
        "aa9c84fd",
        "f1d2da90",
        "7811ea5a",
        "k1531/runs",
        "k1536/runs",
        "k1538/runs",
        "k1537pre/runs",
        "pcu-76163c7",
        "canary-pwr-95882f00",
    ] {
        assert!(seen.contains(fact), "{fact}");
    }
    assert!(string(&json, "timing").contains("c4f74fb"));
    assert!(string(&json, "known_effects").contains("41"));
    assert!(string(&json, "engine").contains("K15_37_COMMIT"));
    assert!(string(&json, "engine").contains("K15.39"));
}

#[test]
fn the_registration_is_pinned_by_its_hash() {
    assert_eq!(registration().sha256, REGISTRATION_SHA256);
}
