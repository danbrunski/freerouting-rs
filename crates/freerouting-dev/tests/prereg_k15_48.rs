//! Pins K15.48's registration (`docs/roadmap/prereg/K15.48.json`): `router.pour_fill_contacts`
//! (K15.31) together with `router.pour_fill_target` (K15.25) under profile `enhanced` and the KiCad
//! plugin's default options, both unset against both set, measured again after K15.46 (the island
//! repair's zone-join model and the contact rule by the copper's shape), K15.47 (exact caches) and
//! K15.49 (the board snapshot writes uuid-less zones in a fixed order). The `.kicad_pcb` input only,
//! over `tools/bench/sets/k15-fill-pair.txt`, K15.37's file unchanged: the same 17 rows and boards.
//! Registered 2026-10-22 before any route or grade of its arms.
//!
//! The registration is K15.43's with the differences it lists in `differences`: its own tree, output
//! and build directories, validity (d) (both arms of a row route one design, now that K15.49 holds),
//! the grader with `snapshot.py` at K15.49's hash (pinned here, since it decides the design), and the
//! decision table's outcomes for K15.48. Rule, noise bands, metrics, guards (with K15.43's thresholds,
//! unchanged on purpose: the guard that failed is the one K15.47 targets), `must_judge`, the pass cap,
//! the router deadline, the call timeout, the commands' form and the helper are K15.43's.
//!
//! How it is amended: a change goes in the registration's `amendments` with its date and reason,
//! and updates the hashes here in the same change (`AMENDMENTS` too). The registration asks for
//! amendment 1 after the build and before any arm runs (the commit, the binary, the grader and
//! plugin hashes). Nothing registered (arms, boards, options, guards, rule, noise) is edited after a
//! number exists.
//!
//! The registration was written at main `eb48beb` while two seen runs were still to come (K15.46's
//! third part and the orchestrator's check on combined main); their entries were `TODO(k1548pre)`
//! placeholders, which `OPEN_PLACEHOLDERS` counted. Both were filled at main `048677e`, before the
//! registration was committed and before any arm runs, so none is left.
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

const FILE: &str = "docs/roadmap/prereg/K15.48.json";
const K15_43: &str = "docs/roadmap/prereg/K15.43.json";
const SET: &str = "tools/bench/sets/k15-fill-pair.txt";
/// The grader files this test reads from the tree: K15.43's three oracle files (unchanged since
/// K15.43) and the plugin's snapshot, which decides the design the router reads (K15.49).
const PINNED_FILES: [(&str, &str); 4] = [
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
    (
        "kicad-plugin/freerouting_rs/snapshot.py",
        "1c8d65f85c5b0b580ce86f0edb68813e232d21f7eb2ca739c0c4a4434d329046",
    ),
];
const SNAPSHOT: &str = "kicad-plugin/freerouting_rs/snapshot.py";
/// `snapshot.py` as K15.43 registered it, and the commit that changed it since (K15.49).
const SNAPSHOT_K15_43: &str = "8d061a200355ed129e9396524add3521e23b224d8fe212aa1de722edcedd624e";
const SNAPSHOT_COMMIT: &str = "41a36cb";
/// The other plugin files of `grader.files`: unchanged since K15.43; amendment 1 re-reads them.
const PLUGIN_FILES: [&str; 3] = [
    "kicad-plugin/headless_run.py",
    "kicad-plugin/freerouting_rs/export_flow.py",
    "kicad-plugin/freerouting_rs/options.py",
];
const SCRIPT: &str = "tools/kicad-reader/plugin-input-ab.sh";
const HELPER: &str = "tools/bench/k15_all_pours.py";
const OPTIONS: &str = "tools/bench/k15-all-pours.options.json";
/// The candidate arm's flags, in this order (K15.37's and K15.43's).
const FLAGS: &str = "--router.pour_fill_contacts=true --router.pour_fill_target=true";
const ARMS: [&str; 2] = ["pcb-off", "pcb-on"];
const MUST_JUDGE: [&str; 2] = ["pcu-76163c7", "canary-pwr-95882f00"];
const TREE: &str = "target/orchestrator/k1548/tree";
const OUT: &str = "target/orchestrator/k1548/out";
const ENGINE: &str = "/home/thebo/code/freerouting-rs/target/orchestrator/k1548/target-release/release/freerouting-rs";
/// 17 rows x 2 arms (`.kicad_pcb` only).
const CALLS: usize = 34;
const AMENDMENTS: usize = 1;
/// The seen runs still to come when the registration was written (see the module doc).
const PLACEHOLDER: &str = "TODO(k1548pre)";
const OPEN_PLACEHOLDERS: usize = 0;
const REGISTRATION_SHA256: &str =
    "6fcf735a71d1432d919bd41575e9a9bc3e66ba53fcc20e25b44a823f2590be0f";
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

fn k15_43() -> Json {
    Json::parse(&read(K15_43)).unwrap()
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

/// K15.43's text with this registration's directories and names.
fn ours(k15_43_text: &str) -> String {
    k15_43_text
        .replace("/k1543/", "/k1548/")
        .replace("K15_43_COMMIT", "K15_48_COMMIT")
        .replace("prereg_k15_43", "prereg_k15_48")
        .replace("K15.43.json", "K15.48.json")
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
    assert_eq!(prereg.id, "K15.48-fill-pair");
    assert_eq!(prereg.names.base, "pcb-off");
    assert_eq!(prereg.names.cand, "pcb-on");
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    assert_eq!(arms, ARMS);
    assert_eq!(prereg.set.as_deref(), Some(SET));
    assert!(prereg.totals_registered);
    assert_eq!(prereg.amendments, AMENDMENTS);
    assert_eq!(prereg.must_judge, MUST_JUDGE);
    // K15.43's metrics, in the same order and roles, with K15.43's definitions.
    let k43 = Prereg::parse(&read(K15_43)).unwrap();
    let want: Vec<(&str, Role)> = k43
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
    let json = json();
    let k43 = k15_43();
    for key in [
        "format",
        "rule",
        "metrics",
        "metric_definitions",
        "baseline_arm",
        "candidate_arm",
        "pairs",
        "pairs_why",
        "no_dsn_why",
    ] {
        assert_eq!(json.get(key), k43.get(key), "{key}");
    }
    let definitions = object(&json, "metric_definitions");
    for spec in &prereg.specs {
        assert!(definitions.contains_key(&spec.name), "{}", spec.name);
    }
    assert_eq!(definitions.len(), prereg.specs.len());
}

/// The question is K15.43's after its first sentence, which names what changed since.
#[test]
fn the_question_is_k15_43s() {
    let json = json();
    let k43 = k15_43();
    let tail = |text: &str| text.split_once(". ").map(|(_, t)| t.to_string()).unwrap();
    let question = string(&json, "question");
    assert_eq!(tail(question), tail(string(&k43, "question")));
    for item in ["K15.46", "K15.47", "K15.49"] {
        assert!(
            question.split_once(". ").unwrap().0.contains(item),
            "{item}"
        );
    }
}

/// K15.43's totals rule, noise, guards and thresholds, with both owner boards judged; validity is
/// K15.43's (a)-(c) and a new (d): both arms of a row route one design.
#[test]
fn the_rule_noise_and_guards_are_k15_43s() {
    let json = json();
    let k43 = k15_43();
    assert_eq!(string(&json, "rule"), "totals");
    assert_eq!(json.get("totals"), k43.get("totals"));
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
        "k1543/out/records.json",
        "Nothing of this set was run to derive it",
        "pcu-76163c7 43, canary-pwr-95882f00 10",
    ] {
        assert!(noise_why.contains(fact), "noise_why: {fact}");
    }
    // The bands are K15.43's list.
    let bands = |text: &str| {
        let from = text.find("the bands are ").unwrap();
        let to = text[from..].find(": a set noise of 72.32").unwrap();
        text[from..from + to].to_string()
    };
    assert_eq!(bands(noise_why), bands(string(&k43, "noise_why")));
    // Guards, keep rule, stop rule and must_judge are K15.43's (thresholds unchanged).
    for key in ["guards", "keep_rule", "stop_rule", "must_judge"] {
        assert_eq!(json.get(key), k43.get(key), "{key}");
    }
    assert_eq!(strings(&json, "must_judge"), MUST_JUDGE);
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
    assert!(differences.contains("decided 2026-10-22"));
    assert!(differences.contains("guard that failed"));
    // Validity: K15.43's (a)-(c) with this registration's pin test, then (d).
    let validity = string(&json, "validity");
    let k43_validity = ours(string(&k43, "validity"));
    let upto_c = |text: &str| {
        let to = text.find(" passes in TREE").unwrap();
        text[..to].to_string()
    };
    assert_eq!(upto_c(validity), upto_c(&k43_validity));
    assert!(validity.contains("--test prereg_k15_48"));
    let d = &validity[validity.find("(d) ").expect("validity (d)")..];
    for fact in [
        "design_sha256",
        "summary.json",
        "left out of the judgement and reported by name",
        "both hashes",
        "K15.49",
    ] {
        assert!(d.contains(fact), "validity (d): {fact}");
    }
    assert!(validity.contains("fewer than min_boards (3) distinct rows are judged"));
    assert!(validity.contains("pcu-76163c7 or canary-pwr-95882f00 is not judged"));
    assert!(validity.contains("(a), (b) or (d)"));
    assert!(json.get("dsn_rows").is_none());
    // The limits: K15.43's pass cap, router deadline, call timeout and memory, and why they stay.
    let limits = string(&json, "limits");
    for fact in [
        "--max-passes 2 --timeout 240",
        "ROUTE_TIMEOUT_S",
        "ROUTE_MEMORY_KB",
        "34 calls",
        "104.1 s",
        "104.0 s",
        "pocketsdr-v3-0",
    ] {
        assert!(limits.contains(fact), "limits: {fact}");
    }
    assert!(differences.contains("--max-passes 2 --timeout 240"));
    assert!(string(&json, "grading").contains("SHARE_GRADE_WITH"));
}

/// The decision table: K15.43's rows; kept puts both settings in `ENHANCED` and removes both plugin
/// options (and says what an options file with the old keys does), not kept leaves both options and
/// puts this measurement's numbers in the tooltip and the handover, breach and not measured are
/// K15.43's.
#[test]
fn the_decision_table_maps_each_outcome_to_one_row() {
    let json = json();
    let k43 = k15_43();
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
    let k43_table = object(&k43, "decision_table");
    assert_eq!(
        table.keys().collect::<Vec<_>>(),
        k43_table.keys().collect::<Vec<_>>()
    );
    let row = |key: &str| table[key].as_str().unwrap();
    let k43_row = |key: &str| k43_table[key].as_str().unwrap().replace("K15.43", "K15.48");
    for key in ["breach", "not measured", "order"] {
        assert_eq!(row(key), k43_row(key), "{key}");
    }
    let kept = row("kept");
    for fact in [
        "router.pour_fill_contacts",
        "router.pour_fill_target",
        "ENHANCED",
        "settings/profile.rs",
        "DIVERGENCES",
        "parity-check.sh",
        "tests/settings_defaults.rs",
        "K15.30's and K15.37's plugin options are removed",
        "RouteOptions fields `pour_fill_target` and `pour_fill_contacts`",
        "options.py",
        "checkboxes",
        "test_route_dialog.py",
        "A saved options file with a `pour_fill_target` or `pour_fill_contacts` key is read with the key ignored",
        "adds no pour flag",
        "profile legacy",
    ] {
        assert!(kept.contains(fact), "kept: {fact}");
    }
    for key in ["not kept, no gain", "not kept, guard"] {
        for fact in [
            "opt-in, unset in both profiles",
            "the plugin keeps both options",
            "pour_fill_contacts",
            "pour_fill_target",
            "options.py",
            "tooltip",
            "docs/development/kicad-plugin.md",
            "this measurement's numbers",
            "in place of K15.43's",
        ] {
            assert!(row(key).contains(fact), "{key}: {fact}");
        }
    }
    assert!(row("not kept, guard").contains("any of guards (2), (3), (4) fails"));
    assert!(row("not kept, no gain").contains("S fails"));
    assert!(row("breach").contains("the plugin does not offer"));
    assert!(row("not measured").contains("K15.48 stays [~]"));
    assert!(row("order").contains("not measured, breach, not kept no gain, not kept guard, kept"));
}

/// The set is K15.37's file, unchanged; the rows, boards and rules files are K15.43's; every present
/// board and its rules files hash as registered.
#[test]
fn the_rows_are_k15_43s_and_match_their_hashes() {
    let json = json();
    let k43 = k15_43();
    let text = read(SET);
    assert_eq!(sha256_hex(text.as_bytes()), SET_SHA256);
    assert_eq!(string(&json, "boards.set"), SET);
    assert_eq!(string(&json, "boards.set_sha256"), SET_SHA256);
    assert_eq!(string(&k43, "boards.set_sha256"), SET_SHA256);
    assert!(text.starts_with("# K15.37 (docs/roadmap/prereg/K15.37.json)"));
    for key in ["rows", "files", "rules_files"] {
        let path = format!("boards.{key}");
        assert_eq!(json.path(&path), k43.path(&path), "{path}");
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
    assert_eq!(json.get("options"), k15_43().get("options"));
    assert_eq!(string(&json, "options.sha256"), sha256_hex(text.as_bytes()));
}

/// Every command is the arm's template filled in, and it is K15.43's command of the row with this
/// registration's tree, output and build directories.
#[test]
fn the_commands_are_k15_43s_with_k15_48s_directories() {
    let json = json();
    let k43 = k15_43();
    assert_eq!(string(&json, "run.out"), OUT);
    assert_eq!(string(&json, "run.root"), string(&k43, "run.root"));
    assert_eq!(string(&json, "run.script"), string(&k43, "run.script"));
    assert_eq!(string(&json, "run.what"), ours(string(&k43, "run.what")));
    let engine = string(&json, "placeholders.ENGINE")
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    assert_eq!(engine, ENGINE);
    for (key, text) in object(&json, "placeholders") {
        let k43_text = string(&k43, &format!("placeholders.{key}"));
        assert_eq!(text.as_str(), Some(ours(k43_text).as_str()), "{key}");
    }
    for arm in ARMS {
        let a = |key: &str| format!("arms.{arm}.{key}");
        assert_eq!(string(&json, &a("board_input")), "kicad_pcb");
        assert_eq!(string(&json, &a("engine")), "as `engine`");
        assert_eq!(string(&json, &a("role")), string(&k43, &a("role")));
        assert_eq!(json.path(&a("options")), k43.path(&a("options")));
        assert_eq!(json.path(&a("settings")), k43.path(&a("settings")));
        let settings = object(&json, &a("settings"));
        let on = arm == "pcb-on";
        assert_eq!(settings.len(), if on { 2 } else { 0 });
        if on {
            for key in ["router.pour_fill_contacts", "router.pour_fill_target"] {
                assert_eq!(settings.get(key), Some(&Json::Bool(true)), "{key}");
            }
        }
        let template = string(&json, &a("command"));
        assert_eq!(template, ours(string(&k43, &a("command"))), "{arm}");
        // The arms' text names the plugin's flags and K15.49's zone order.
        assert!(string(&json, &a("what")).contains("--board-setup.router.*"));
        assert!(string(&json, &a("what")).contains("order_zones"));
    }
    assert!(string(&json, "arms.pcb-on.what").contains("K15.46"));
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
            let k43_call = string(&k43, &format!("commands.{row}.{arm}"));
            assert_eq!(call, ours(k43_call), "{row} {arm}");
            assert!(!call.contains("k1543"), "{call}");
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
    assert!(heavy[0].contains("<K15_48_COMMIT"));
    assert!(heavy.iter().any(|c| c.contains("--test prereg_k15_48")));
    assert!(heavy.iter().any(|c| c.contains("amendment 1")));
    assert!(heavy.iter().any(|c| c.contains("--left-out ROW=REASON")));
    assert!(heavy.iter().any(|c| c.contains("--all-pairs")));
    assert!(heavy.iter().any(|c| c.contains("design_sha256")));
    assert!(heavy.iter().all(|c| !c.contains("k1543/")));
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

/// The measuring script and the helper are K15.43's, unchanged; the grader lists K15.43's files at
/// their current hashes, names the one that changed (`snapshot.py`, K15.49) with its commit, and
/// this test reads the oracle files and the snapshot from the tree.
#[test]
fn the_scripts_and_the_grader_are_the_registered_ones() {
    let json = json();
    let k43 = k15_43();
    assert_eq!(json.get("script"), k43.get("script"));
    let script = std::fs::read(repo().join(SCRIPT)).unwrap();
    assert_eq!(string(&json, "script.sha256"), sha256_hex(&script));
    assert_eq!(json.get("helper"), k43.get("helper"));
    assert_eq!(string(&json, "helper.path"), HELPER);
    let helper = std::fs::read(repo().join(HELPER)).unwrap();
    assert_eq!(sha256_hex(&helper), HELPER_SHA256);
    assert_eq!(string(&json, "helper.sha256"), HELPER_SHA256);
    // The grader: K15.43's files, the pinned ones as they are in the tree.
    let files = object(&json, "grader.files");
    let k43_files = object(&k43, "grader.files");
    assert_eq!(
        files.keys().collect::<Vec<_>>(),
        k43_files.keys().collect::<Vec<_>>()
    );
    for (path, want) in PINNED_FILES {
        assert_eq!(files[path].as_str(), Some(want), "{path}");
        let bytes = std::fs::read(repo().join(path)).unwrap_or_else(|e| panic!("{path}: {e}"));
        assert_eq!(sha256_hex(&bytes), want, "{path}");
    }
    let pinned = object(&json, "grader")["pinned_by_test"].clone();
    let Json::Array(pinned) = pinned else {
        panic!("grader.pinned_by_test is not a list");
    };
    let pinned: Vec<&str> = pinned.iter().filter_map(Json::as_str).collect();
    assert_eq!(pinned, PINNED_FILES.map(|(path, _)| path));
    // Every hash but the snapshot's is K15.43's; the snapshot's change is named with its commit.
    for (path, hash) in files {
        let same = path != SNAPSHOT;
        assert_eq!(hash == &k43_files[path], same, "{path}");
    }
    assert_eq!(k43_files[SNAPSHOT].as_str(), Some(SNAPSHOT_K15_43));
    let changed = object(&json, "grader.changed_since_k15_43");
    assert_eq!(
        changed.keys().map(String::as_str).collect::<Vec<_>>(),
        [SNAPSHOT]
    );
    let why = changed[SNAPSHOT].as_str().unwrap();
    for fact in [
        SNAPSHOT_K15_43,
        PINNED_FILES[3].1,
        SNAPSHOT_COMMIT,
        "K15.49",
        "order_zones",
    ] {
        assert!(why.contains(fact), "{fact}");
    }
    for path in PLUGIN_FILES {
        assert_eq!(files[path], k43_files[path], "{path}");
    }
}

/// What was seen before the registration is recorded with its sessions, so the later run can be
/// read against it; no placeholder is left.
#[test]
fn the_registration_records_what_was_seen_before_it() {
    let json = json();
    let text = read(FILE);
    assert!(!text.contains("@@"));
    assert_eq!(text.matches(PLACEHOLDER).count(), OPEN_PLACEHOLDERS);
    let seen = string(&json, "seen_before_registration");
    for fact in [
        "eb48beb",
        "048677e",
        "179d85a8d570b95ac664c17d7d35a1efecd89ba2d89aa781546d290c9d248165",
        "300873bd",
        "2d505df0",
        "k1543/out",
        "guards.txt",
        "k1546/runs/final-",
        "k1546b",
        "k1547/out",
        "k1549/out",
        "k1546c",
        "k1548pre",
        "runs.sh",
        "044e7bcc",
        "acb48099",
        "e9572570",
        "cf00bc73",
        "d124cb86",
        "9f998f1d",
        "37c65be3",
        "2e357747",
        "962d509f",
        "pcu-76163c7",
        "canary-pwr-95882f00",
        "Not seen",
    ] {
        assert!(seen.contains(fact), "{fact}");
    }
    assert!(string(&json, "timing").contains("eb48beb"));
    assert!(string(&json, "timing").contains("048677e"));
    let known = string(&json, "known_effects");
    for fact in [
        "1.276 x",
        "370 -> 269",
        "15 -> 11",
        "Guard (2)",
        "Guard (4)",
        "1.248 x",
        "464.9 s -> 580.0 s",
        "moteus",
    ] {
        assert!(known.contains(fact), "known_effects: {fact}");
    }
    let engine = string(&json, "engine");
    for fact in [
        "K15_48_COMMIT",
        "Amendment 1",
        "de95a42",
        "5dae208",
        "41a36cb",
        "eb48beb",
        "048677e",
    ] {
        assert!(engine.contains(fact), "engine: {fact}");
    }
    let not_registered = strings(&json, "not_registered").join("\n");
    assert!(not_registered.contains("K15.46"));
    assert!(not_registered.contains("profile legacy"));
    // Amendment 1: the commit and the binary, before any arm runs.
    let amendment = match json.get("amendments") {
        Some(Json::Array(list)) => list.first().map_or("", |a| string(a, "reason")),
        other => panic!("amendments: {other:?}"),
    };
    assert!(amendment.contains("K15_48_COMMIT = main cc99ede"));
    assert!(amendment.contains("d6741c038ffe41e9dee4fc10be7deaf963d8f6dbc2950c2271941586057d6c4a"));
}

#[test]
fn the_registration_is_pinned_by_its_hash() {
    assert_eq!(registration().sha256, REGISTRATION_SHA256);
}
