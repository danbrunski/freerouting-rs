//! Pins K15.43's registration (`docs/roadmap/prereg/K15.43.json`): `router.pour_fill_contacts`
//! (K15.31) together with `router.pour_fill_target` (K15.25) under profile `enhanced` and the KiCad
//! plugin's default options, both unset against both set, measured again after K15.40, K15.41,
//! K15.42, K15.44 and K15.45 changed what the pair routes and costs. The `.kicad_pcb` input only
//! (neither setting acts on a DSN), over the board set `tools/bench/sets/k15-fill-pair.txt`, which is
//! K15.37's file unchanged: K15.30's 17 rows (K15.14's), the same stripped boards by absolute path.
//! Registered 2026-10-22 before any route or grade of its arms.
//!
//! The registration is K15.37's with the differences it lists in `differences`: its own tree, output
//! and build directories, the plugin's `--board-setup.router.*` flags (K15.44), the grader's
//! `below_minimum` against the minimum KiCad's DRC checks (K15.44), and a decision table whose
//! outcomes name both plugin options. Rule, noise bands, metrics, guards (with K15.37's thresholds,
//! unchanged on purpose: the guards that failed are the ones K15.41 and K15.42 target), `must_judge`
//! and the helper (`tools/bench/k15_all_pours.py`, K15.14's, unchanged) are K15.37's.
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

const FILE: &str = "docs/roadmap/prereg/K15.43.json";
const K15_37: &str = "docs/roadmap/prereg/K15.37.json";
const K15_30: &str = "docs/roadmap/prereg/K15.30.json";
const K15_14: &str = "docs/roadmap/prereg/K15.14.json";
const SET: &str = "tools/bench/sets/k15-fill-pair.txt";
const K15_30_SET: &str = "tools/bench/sets/k15-fill-target.txt";
const ORACLE_FILES: [(&str, &str); 3] = [
    (
        "tools/kicad-oracle/kicad_steps.py",
        "2b0e84315bc0fd09ffba0a8e2556f53c2831176bfd7d99310c1c873640ceda60",
    ),
    (
        "tools/kicad-oracle/oracle_lib.py",
        "da6e047dce8e90606893e73adac362081d8e4a562aca89c1b14ed944cbab835e",
    ),
    (
        "tools/kicad-oracle/grade.sh",
        "16879f6d0ddedbcad2b2e934c31a26ed426fa1a02dfafbcc5d965f47484d7985",
    ),
];
/// The plugin files whose hashes the registration lists (`grader.files`); amendment 1 re-reads them.
const PLUGIN_FILES: [&str; 4] = [
    "kicad-plugin/headless_run.py",
    "kicad-plugin/freerouting_rs/export_flow.py",
    "kicad-plugin/freerouting_rs/options.py",
    "kicad-plugin/freerouting_rs/snapshot.py",
];
const SCRIPT: &str = "tools/kicad-reader/plugin-input-ab.sh";
const HELPER: &str = "tools/bench/k15_all_pours.py";
const OPTIONS: &str = "tools/bench/k15-all-pours.options.json";
/// The candidate arm's flags, in this order (K15.37's).
const FLAGS: &str = "--router.pour_fill_contacts=true --router.pour_fill_target=true";
const ARMS: [&str; 2] = ["pcb-off", "pcb-on"];
const MUST_JUDGE: [&str; 2] = ["pcu-76163c7", "canary-pwr-95882f00"];
const TREE: &str = "target/orchestrator/k1543/tree";
const OUT: &str = "target/orchestrator/k1543/out";
/// 17 rows x 2 arms (`.kicad_pcb` only).
const CALLS: usize = 34;
const AMENDMENTS: usize = 1;
const REGISTRATION_SHA256: &str =
    "8ecf0413cf83fcb0d0f2e2481814268fcedd00c47105e6df5fd0a5e3536fcdfe";
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

fn k15_37() -> Json {
    Json::parse(&read(K15_37)).unwrap()
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
    assert_eq!(prereg.id, "K15.43-fill-pair");
    assert_eq!(prereg.names.base, "pcb-off");
    assert_eq!(prereg.names.cand, "pcb-on");
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    assert_eq!(arms, ARMS);
    assert_eq!(prereg.set.as_deref(), Some(SET));
    assert!(prereg.totals_registered);
    assert_eq!(prereg.amendments, AMENDMENTS);
    assert_eq!(prereg.must_judge, MUST_JUDGE);
    // K15.37's metrics, in the same order and roles.
    let k37 = Prereg::parse(&read(K15_37)).unwrap();
    let want: Vec<(&str, Role)> = k37
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
    assert_eq!(json.get("metrics"), k15_37().get("metrics"));
    // Every definition is K15.37's but `below_minimum`, which K15.44 changed.
    let old = k15_37();
    let old_definitions = object(&old, "metric_definitions");
    for (name, text) in definitions {
        if name == "below_minimum" {
            continue;
        }
        assert_eq!(Some(text), old_definitions.get(name), "{name}");
    }
    let below = string(&json, "metric_definitions.below_minimum");
    assert!(below.contains("oracle_lib.effective_minimums"));
    assert!(below.contains("unconditional"));
    assert_ne!(
        json.path("metric_definitions.below_minimum"),
        old.path("metric_definitions.below_minimum")
    );
}

/// K15.37's totals rule, noise and guards (thresholds unchanged), with both owner boards judged;
/// validity reads both settings from the manifests and names a row only the candidate times out on.
#[test]
fn the_rule_noise_and_guards_are_k15_37s() {
    let json = json();
    let k37 = k15_37();
    assert_eq!(string(&json, "rule"), "totals");
    assert_eq!(json.get("totals"), k37.get("totals"));
    assert_eq!(json.get("totals"), k15_30().get("totals"));
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
        "59.34",
        "67.09",
        "72.32",
        "852",
        "331",
        "k1537/out/records.json",
        "k1543pre/runs",
        "Nothing of this set was run to derive it",
        "pcu-76163c7 43, canary-pwr-95882f00 10",
    ] {
        assert!(noise_why.contains(fact), "noise_why: {fact}");
    }
    // The bands are K15.37's list.
    let bands = |text: &str| {
        let from = text.find("the bands are ").unwrap();
        let to = text[from..].find(": a set noise of 72.32").unwrap();
        text[from..from + to].to_string()
    };
    assert_eq!(bands(noise_why), bands(string(&k37, "noise_why")));
    // The guards, keep rule and validity (a) and (b) are K15.37's.
    assert_eq!(json.get("guards"), k37.get("guards"));
    assert_eq!(json.get("keep_rule"), k37.get("keep_rule"));
    assert_eq!(json.get("stop_rule"), k37.get("stop_rule"));
    let guards = string(&json, "guards");
    for clause in [
        "absolutely",
        "no row worse than its own noise band",
        "1.25 x",
        "COMPLETED in pcb-off and TIMED_OUT in pcb-on",
    ] {
        assert!(guards.contains(clause), "{clause}");
    }
    let differences = strings(&json, "differences").join("\n");
    assert!(differences.contains("thresholds unchanged"));
    assert!(differences.contains("guards that failed"));
    let validity = string(&json, "validity");
    assert!(validity.contains("left out of the judgement and reported by name"));
    assert!(validity.contains("fewer than min_boards (3) distinct rows are judged"));
    assert!(validity.contains("pcu-76163c7 or canary-pwr-95882f00 is not judged"));
    // Validity (a): both settings reached the router, from the result manifests.
    assert!(validity.contains("settings_snapshot.pour_fill_target"));
    assert!(validity.contains("settings_snapshot.pour_fill_contacts"));
    assert!(validity.contains("COMPLETED in pcb-off and TIMED_OUT in pcb-on"));
    assert!(validity.contains("--test prereg_k15_43"));
    assert_eq!(strings(&json, "must_judge"), MUST_JUDGE);
    assert!(json.get("dsn_rows").is_none());
    let no_dsn = string(&json, "no_dsn_why");
    assert!(no_dsn.contains("read_from_kicad_pcb"));
    assert!(no_dsn.contains("pour_fill_contacts"));
    // The limits are K15.37's (pass cap, router deadline, call timeout, memory).
    let limits = string(&json, "limits");
    assert!(limits.contains("--max-passes 2 --timeout 240"));
    assert!(limits.contains("ROUTE_TIMEOUT_S"));
    assert!(limits.contains("34 calls"));
    assert!(limits.contains("49.7 s"));
    assert!(limits.contains("192.3 s"));
    assert!(string(&json, "grading").contains("SHARE_GRADE_WITH"));
}

/// The decision table: kept puts both settings in `ENHANCED` and removes both plugin options (and
/// says what an options file with the old keys does), not kept leaves both options, a breach
/// removes the contacts option when the pair causes it; every outcome maps to one row.
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
        object(&k15_37(), "decision_table")
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
        "K15.30's and K15.37's plugin options are removed",
        "RouteOptions fields `pour_fill_target` and `pour_fill_contacts`",
        "options.py",
        "A saved options file with a `pour_fill_target` or `pour_fill_contacts` key is read with the key ignored",
        "adds no pour flag",
        "profile legacy",
    ] {
        assert!(kept.contains(fact), "kept: {fact}");
    }
    for key in ["not kept, no gain", "not kept, guard"] {
        assert!(row(key).contains("opt-in, unset in both profiles"), "{key}");
        assert!(row(key).contains("the plugin keeps both options"), "{key}");
        assert!(row(key).contains("pour_fill_contacts"), "{key}");
        assert!(row(key).contains("pour_fill_target"), "{key}");
        assert!(row(key).contains("options.py"), "{key}");
    }
    assert!(row("not kept, guard").contains("any of guards (2), (3), (4) fails"));
    assert!(row("not kept, no gain").contains("S fails"));
    assert!(row("breach").contains("the plugin does not offer"));
    assert!(row("breach").contains("pour_fill_contacts"));
    assert!(row("breach").contains("pour_fill_target"));
    assert!(row("not measured").contains("K15.43 stays [~]"));
    assert!(row("order").contains("not measured, breach, not kept no gain, not kept guard, kept"));
    assert_eq!(table["order"], object(&k15_37(), "decision_table")["order"]);
}

/// The set is K15.37's file, unchanged, listing K15.30's rows verbatim and in order; the boards and
/// rules files are K15.14's and K15.30's; every present board and its rules files hash as
/// registered.
#[test]
fn the_rows_are_k15_37s_and_match_their_hashes() {
    let json = json();
    let k37 = k15_37();
    let text = read(SET);
    assert_eq!(sha256_hex(text.as_bytes()), SET_SHA256);
    assert_eq!(string(&json, "boards.set"), SET);
    assert_eq!(string(&json, "boards.set_sha256"), SET_SHA256);
    assert_eq!(string(&k37, "boards.set_sha256"), SET_SHA256);
    assert!(text.starts_with("# K15.37 (docs/roadmap/prereg/K15.37.json)"));
    assert_eq!(set_rows(SET), set_rows(K15_30_SET));
    for key in ["rows", "files", "rules_files"] {
        let path = format!("boards.{key}");
        assert_eq!(json.path(&path), k37.path(&path), "{path}");
        assert_eq!(json.path(&path), k15_30().path(&path), "{path}");
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

/// The options are the plugin's defaults, verbatim (K15.14's file): both pour options are off in
/// them, so the arms' settings come from the command line alone.
#[test]
fn the_options_are_the_plugin_defaults() {
    let json = json();
    let text = read(OPTIONS);
    assert_eq!(text, "{\"schema\": 6}\n");
    assert_eq!(json.get("options"), k15_37().get("options"));
    assert_eq!(string(&json, "options.sha256"), sha256_hex(text.as_bytes()));
}

/// Every command is the arm's template filled in, and it is K15.37's command of the row with this
/// registration's tree, output and build directories.
#[test]
fn the_commands_are_k15_37s_with_k15_43s_directories() {
    let json = json();
    let k37 = k15_37();
    assert_eq!(string(&json, "run.out"), OUT);
    assert_eq!(string(&json, "run.root"), string(&k37, "run.root"));
    let engine = string(&json, "placeholders.ENGINE")
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    assert_eq!(
        engine,
        "/home/thebo/code/freerouting-rs/target/orchestrator/k1543/target-release/release/freerouting-rs"
    );
    let ours = |k37_command: &str| k37_command.replace("/k1537/", "/k1543/");
    for arm in ARMS {
        let a = |key: &str| format!("arms.{arm}.{key}");
        assert_eq!(string(&json, &a("board_input")), "kicad_pcb");
        assert_eq!(string(&json, &a("engine")), "as `engine`");
        assert_eq!(json.path(&a("options")), k37.path(&a("options")));
        assert_eq!(json.path(&a("settings")), k37.path(&a("settings")));
        let settings = object(&json, &a("settings"));
        let on = arm == "pcb-on";
        assert_eq!(settings.len(), if on { 2 } else { 0 });
        if on {
            for key in ["router.pour_fill_contacts", "router.pour_fill_target"] {
                assert_eq!(settings.get(key), Some(&Json::Bool(true)), "{key}");
            }
        }
        let template = string(&json, &a("command"));
        assert_eq!(template, ours(string(&k37, &a("command"))), "{arm}");
    }
    // The arms' text names the plugin's new flags.
    assert!(string(&json, "arms.pcb-off.what").contains("--board-setup.router.*"));
    assert!(string(&json, "arms.pcb-on.what").contains("--board-setup.router.*"));
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
    // K15.37's directories become this registration's; the boards stay where K15.14 made them.
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
            let k37_call = string(&k37, &format!("commands.{row}.{arm}"));
            assert_eq!(call, ours(k37_call), "{row} {arm}");
            assert!(!call.contains("k1537"), "{call}");
            assert!(call.contains(&format!(" {board} ")), "{call}");
            assert!(call.contains(" -- --max-passes 2 --timeout 240"), "{call}");
            assert!(call.contains("ROUTE_TIMEOUT_S=300 "), "{call}");
            assert!(call.contains("ROUTE_MEMORY_KB=8000000 "), "{call}");
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
    assert!(heavy[0].contains("<K15_43_COMMIT"));
    assert!(heavy.iter().any(|c| c.contains("--test prereg_k15_43")));
    assert!(heavy.iter().any(|c| c.contains("amendment 1")));
    assert!(heavy.iter().any(|c| c.contains("--left-out ROW=REASON")));
    assert!(heavy.iter().any(|c| c.contains("--all-pairs")));
    assert!(heavy.iter().all(|c| !c.contains("k1537")));
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

/// The measuring script is K13.5's and the helper K15.14's, both unchanged; the grader's files
/// (`tools/kicad-oracle`) are pinned at the hashes of K15.44's change, and the registration lists
/// the plugin files' hashes for amendment 1 to re-read.
#[test]
fn the_scripts_and_the_grader_are_the_registered_ones() {
    let json = json();
    assert_eq!(json.get("script"), k15_37().get("script"));
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
    assert_eq!(string(&k15_37(), "helper.sha256"), HELPER_SHA256);
    assert_eq!(string(&k15_14(), "helper.sha256"), HELPER_SHA256);
    assert_eq!(string(&k15_30(), "helper.sha256"), HELPER_SHA256);
    // The grader: the three oracle files, registered and as they are in the tree.
    let files = object(&json, "grader.files");
    for (path, want) in ORACLE_FILES {
        assert_eq!(files[path].as_str(), Some(want), "{path}");
        let bytes = std::fs::read(repo().join(path)).unwrap_or_else(|e| panic!("{path}: {e}"));
        assert_eq!(sha256_hex(&bytes), want, "{path}");
    }
    let pinned = object(&json, "grader")["pinned_by_test"].clone();
    let Json::Array(pinned) = pinned else {
        panic!("grader.pinned_by_test is not a list");
    };
    let pinned: Vec<&str> = pinned.iter().filter_map(Json::as_str).collect();
    assert_eq!(pinned, ORACLE_FILES.map(|(path, _)| path));
    assert_eq!(files.len(), ORACLE_FILES.len() + PLUGIN_FILES.len());
    // K15.37 pinned grade.sh at this hash too; oracle_lib.py and kicad_steps.py changed in K15.44.
    let k37_amendment = k15_37()
        .path("amendments")
        .and_then(|a| match a {
            Json::Array(list) => list.first().cloned(),
            _ => None,
        })
        .and_then(|a| a.get("reason").and_then(Json::as_str).map(str::to_string))
        .unwrap();
    assert!(k37_amendment.contains(ORACLE_FILES[2].1));
    assert!(!k37_amendment.contains(ORACLE_FILES[1].1));
    // The plugin files: listed with a sha256 for amendment 1 to compare against.
    for path in PLUGIN_FILES {
        let want = files[path].as_str().unwrap();
        assert_eq!(want.len(), 64, "{path}");
        assert!(want.bytes().all(|b| b.is_ascii_hexdigit()), "{path}");
    }
    // headless_run.py and snapshot.py are K15.37's; export_flow.py and options.py are not (K15.44).
    for (path, same) in [
        ("headless_run.py", true),
        ("freerouting_rs/snapshot.py", true),
        ("freerouting_rs/export_flow.py", false),
        ("freerouting_rs/options.py", false),
    ] {
        let want = files[&format!("kicad-plugin/{path}")].as_str().unwrap();
        assert_eq!(k37_amendment.contains(want), same, "{path}");
    }
}

/// What was seen before the registration is recorded with its sessions, so the later run can be
/// read against it; nothing is left unfilled.
#[test]
fn the_registration_records_what_was_seen_before_it() {
    let json = json();
    assert!(!read(FILE).contains("@@"));
    let seen = string(&json, "seen_before_registration");
    for fact in [
        "ebde9a2",
        "k1537/out",
        "k1541/runs",
        "k1542b",
        "k1545/runs",
        "k1543pre",
        "2e8bf394",
        "044e7bcc",
        "aa9c84fd",
        "f1d2da90",
        "7811ea5a",
        "f286d7f6",
        "9f998f1d",
        "pcu-76163c7",
        "canary-pwr-95882f00",
        "Not seen",
    ] {
        assert!(seen.contains(fact), "{fact}");
    }
    assert!(string(&json, "timing").contains("ebde9a2"));
    assert!(string(&json, "known_effects").contains("1.95 x"));
    let engine = string(&json, "engine");
    assert!(engine.contains("K15_43_COMMIT"));
    assert!(engine.contains("K15.46"));
    assert!(engine.contains("K15.39"));
    let not_registered = strings(&json, "not_registered").join("\n");
    assert!(not_registered.contains("K15.46"));
    assert!(not_registered.contains("not on main"));
    // Amendment 1: the commit and the binary, before any arm runs.
    let amendment = match json.get("amendments") {
        Some(Json::Array(list)) => list.first().map_or("", |a| string(a, "reason")),
        other => panic!("amendments: {other:?}"),
    };
    assert!(amendment.contains("K15_43_COMMIT = main 29a4cf1"));
    assert!(amendment.contains("9240307c48ca14e8122efe970069611b5dfb8192ae9ca7826ab67cff95ee1093"));
}

#[test]
fn the_registration_is_pinned_by_its_hash() {
    assert_eq!(registration().sha256, REGISTRATION_SHA256);
}
