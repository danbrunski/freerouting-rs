//! Pins K15.14's registration (`docs/roadmap/prereg/K15.14.json`): `router.plane_obstacle_all_pours`
//! under profile `enhanced` and the KiCad plugin's default options, off against on, on the plugin's
//! two board inputs (`.kicad_pcb` primary on every row, DSN secondary on the two owner boards),
//! over the board set
//! `tools/bench/sets/k15-all-pours.txt`: the owner's pcu and canary-pwr boards, Q01's four pour
//! demos and the 4-layer corpus boards with a signal-layer pour, all stripped of tracks and vias.
//! Registered 2026-10-21 before any route or grade of its arms.
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

use freerouting::quality::feasibility::report::sha256_hex;
use freerouting_dev::gate::Role;
use freerouting_dev::gate_prereg::Prereg;
use freerouting_dev::json_read::Json;

const FILE: &str = "docs/roadmap/prereg/K15.14.json";
const SET: &str = "tools/bench/sets/k15-all-pours.txt";
const SCRIPT: &str = "tools/kicad-reader/plugin-input-ab.sh";
const HELPER: &str = "tools/bench/k15_all_pours.py";
const OPTIONS: &str = "tools/bench/k15-all-pours.options.json";
const FLAG: &str = "--router.plane_obstacle_all_pours=true";
const ARMS: [&str; 4] = ["pcb-off", "pcb-on", "dsn-off", "dsn-on"];
const DSN_ROWS: [&str; 2] = ["pcu-76163c7", "canary-pwr-95882f00"];
/// 17 rows x 2 pcb arms + 2 rows x 2 DSN arms (decided 2026-10-21: no hour-long runs).
const CALLS: usize = 38;
const AMENDMENTS: usize = 3;
const REGISTRATION_SHA256: &str =
    "25e8f26ff986ea702adecb8efe2c928c1732c416ca2faadb89674a5423bb6f5a";
const SET_SHA256: &str = "900c81f41e26f6f688909e4001f3156c182d0712d464e07b8df2efd062f3d408";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &str) -> String {
    std::fs::read_to_string(repo().join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn json() -> Json {
    Json::parse(&read(FILE)).unwrap()
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

fn number(json: &Json, path: &str) -> f64 {
    match json.path(path) {
        Some(Json::Number(n)) => *n,
        other => panic!("{path}: {other:?}"),
    }
}

/// The set file's rows: (path, comment), in file order.
fn set_rows() -> Vec<(String, String)> {
    read(SET)
        .lines()
        .filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let (path, comment) = l.split_once('#').unwrap_or((l, ""));
            (path.trim().to_string(), comment.trim().to_string())
        })
        .collect()
}

/// The registration's rows in file order (the JSON reader keeps objects sorted, so the order
/// is the set file's, which the registration's `rows` and `commands` follow).
fn row_names() -> Vec<String> {
    let json = json();
    let rows = object(&json, "boards.rows");
    set_rows()
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
    assert_eq!(prereg.id, "K15.14-all-pours");
    assert_eq!(prereg.names.base, "pcb-off");
    assert_eq!(prereg.names.cand, "pcb-on");
    let mut arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    arms.sort_unstable();
    let mut want = ARMS.to_vec();
    want.sort_unstable();
    assert_eq!(arms, want);
    assert_eq!(prereg.set.as_deref(), Some(SET));
    assert!(prereg.totals_registered);
    assert_eq!(prereg.amendments, AMENDMENTS);
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
    let names: Vec<&str> = prereg.specs.iter().map(|s| s.name.as_str()).collect();
    for secondary in [
        "zone_zone_unconnected",
        "router_incomplete",
        "kicad_less_router",
        "drc_real",
        "route_seconds",
        "connections",
    ] {
        assert!(names.contains(&secondary), "{secondary}");
    }
    // The verdict pair judges pcb-on; the reported pair puts pcb-off under the hard metrics. The
    // DSN pairs run on `dsn_rows` only and are judged by their own `--arms ... --rows` calls.
    let pairs: Vec<(&str, &str)> = prereg
        .pairs
        .iter()
        .map(|p| (p.arms.base.as_str(), p.arms.cand.as_str()))
        .collect();
    assert_eq!(pairs, [("pcb-off", "pcb-on"), ("pcb-on", "pcb-off")]);
}

/// The totals rule of a change to `enhanced` (`improve`), canary-pwr's registered band (K09.json)
/// and the default band for every other board.
#[test]
fn the_rule_is_the_enhanced_totals_rule_with_canary_pwrs_band() {
    let json = json();
    assert_eq!(string(&json, "rule"), "totals");
    assert_eq!(string(&json, "totals.accept"), "improve");
    assert_eq!(number(&json, "totals.min_boards"), 3.0);
    let noise = object(&json, "totals.noise");
    assert_eq!(noise.keys().collect::<Vec<_>>(), ["canary-pwr-95882f00"]);
    let k09 = Json::parse(&read("docs/roadmap/prereg/K09.json")).unwrap();
    assert_eq!(
        noise.get("canary-pwr-95882f00"),
        k09.path("noise.canary-pwr")
    );
    assert!(string(&json, "noise_why").contains("max(2, ceil(5 % of its connections))"));
    for key in ["guards", "keep_rule", "validity"] {
        assert!(!string(&json, key).is_empty(), "{key}");
    }
    let guards = string(&json, "guards");
    for clause in [
        "absolutely",
        "no row worse than its own noise band",
        "1.25 x",
    ] {
        assert!(guards.contains(clause), "{clause}");
    }
    // A row left out by validity is reported by name; only fewer than min_boards judged rows, or
    // the pcu row not judged, make the measurement "not measured".
    let validity = string(&json, "validity");
    assert!(validity.contains("left out of the judgement and reported by name"));
    assert!(validity.contains("fewer than min_boards (3) distinct rows are judged"));
    assert!(validity.contains("pcu-76163c7 is not judged"));
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
    assert!(table["kept"].as_str().unwrap().contains("ENHANCED"));
    assert!(
        table["not kept, no gain"]
            .as_str()
            .unwrap()
            .contains("options.py")
    );
}

/// The set lists exactly the registered inputs, in the registration's order, with the pass cap and
/// timeout in each comment; the set file is pinned; every present board and its rules files hash
/// as registered.
#[test]
fn the_rows_match_the_set_and_their_hashes() {
    let json = json();
    assert_eq!(sha256_hex(read(SET).as_bytes()), SET_SHA256);
    assert_eq!(string(&json, "boards.set_sha256"), SET_SHA256);
    let set = set_rows();
    let rows = object(&json, "boards.rows");
    assert_eq!(set.len(), rows.len());
    assert_eq!(object(&json, "boards.files").len(), rows.len());
    for ((path, comment), row) in set.iter().zip(row_names()) {
        let at = |key: &str| format!("boards.rows.{row}.{key}");
        assert!(Path::new(path).is_absolute(), "{path}");
        assert!(comment.starts_with(&format!("{row}:")), "{comment}");
        let timeout = number(&json, &at("timeout_s"));
        assert!(
            comment.ends_with(&format!("--max-passes 2 --timeout 240, {timeout} s")),
            "{comment}"
        );
        assert_eq!(string(&json, &at("rules")), "project");
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
        let rules = object(&json, &format!("boards.rules_files.{row}"));
        assert!(rules.contains_key("kicad_pro"), "{row}");
        for (ext, want) in rules {
            let file = board.with_file_name(format!("{stem}.{ext}"));
            let bytes = std::fs::read(&file).unwrap_or_else(|e| panic!("{file:?}: {e}"));
            assert_eq!(Some(sha256_hex(&bytes).as_str()), want.as_str(), "{file:?}");
        }
    }
}

/// The groups the dispatch names: the owner's two boards, Q01's four demos, and the 4-layer
/// corpus rows chosen by the pour rule.
#[test]
fn the_rows_are_the_owner_boards_the_q01_demos_and_the_4_layer_corpus() {
    let json = json();
    let group = |name: &str| -> Vec<String> {
        row_names()
            .into_iter()
            .filter(|r| string(&json, &format!("boards.rows.{r}.group")) == name)
            .collect()
    };
    assert_eq!(group("owner"), ["pcu-76163c7", "canary-pwr-95882f00"]);
    let q01 = read("tools/bench/sets/q01-pours.txt");
    let demos = group("q01-pours");
    assert_eq!(demos.len(), 4);
    for demo in &demos {
        assert!(
            q01.contains(&format!("/demos/{demo}/{demo}.kicad_pcb")),
            "{demo}"
        );
    }
    let corpus = read("tools/bench/sets/kicad-corpus.txt");
    let chosen = group("kicad-corpus");
    assert_eq!(chosen.len(), 11);
    for row in &chosen {
        let line = corpus
            .lines()
            .find(|l| l.contains(&format!("/{row}.dsn")))
            .unwrap_or_else(|| panic!("{row} is not a corpus row"));
        assert!(line.contains("# 4 layers"), "{line}");
        assert!(
            number(&json, &format!("boards.rows.{row}.signal_pours.count")) >= 1.0,
            "{row}"
        );
    }
    assert_eq!(
        demos.len() + chosen.len() + 2,
        object(&json, "boards.rows").len()
    );
    for (row, source) in [
        ("pcu-76163c7", "76163c7"),
        ("canary-pwr-95882f00", "95882f00"),
    ] {
        assert_eq!(
            string(&json, &format!("boards.rows.{row}.source.commit")),
            source
        );
    }
}

/// The options are the plugin's defaults, verbatim.
#[test]
fn the_options_are_the_plugin_defaults() {
    let json = json();
    let text = read(OPTIONS);
    assert_eq!(text, "{\"schema\": 6}\n");
    assert_eq!(string(&json, "options.path"), OPTIONS);
    assert_eq!(string(&json, "options.text"), text);
    assert_eq!(string(&json, "options.sha256"), sha256_hex(text.as_bytes()));
}

/// The arms differ by the board input and the flag only, and every command is the arm's template
/// with the row's board and output directory filled in; the -on arm shares its -off arm's grade.
#[test]
fn the_commands_differ_by_the_input_and_the_flag_only() {
    let json = json();
    let tree = "target/orchestrator/k1514/tree";
    let out = string(&json, "run.out").to_string();
    assert_eq!(out, "target/orchestrator/k1514/out");
    let engine = string(&json, "placeholders.ENGINE")
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    for arm in ARMS {
        let a = |key: &str| format!("arms.{arm}.{key}");
        let input = if arm.starts_with("pcb") {
            "kicad_pcb"
        } else {
            "dsn"
        };
        assert_eq!(string(&json, &a("board_input")), input);
        assert_eq!(string(&json, &a("engine")), "as `engine`");
        let options = object(&json, &a("options"));
        assert_eq!(options.len(), 2);
        assert_eq!(options["schema"], Json::Number(6.0));
        assert_eq!(options["board_input"].as_str(), Some(input));
        let settings = object(&json, &a("settings"));
        let on = arm.ends_with("-on");
        assert_eq!(settings.len(), usize::from(on));
        if on {
            assert_eq!(
                settings.get("router.plane_obstacle_all_pours"),
                Some(&Json::Bool(true))
            );
        }
    }
    let commands = object(&json, "commands");
    assert_eq!(commands.len(), row_names().len());
    let dsn_rows: Vec<&str> = match json.get("dsn_rows") {
        Some(Json::Array(list)) => list.iter().filter_map(Json::as_str).collect(),
        other => panic!("dsn_rows: {other:?}"),
    };
    assert_eq!(dsn_rows, DSN_ROWS);
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
        // Every row runs the pcb arms; the DSN arms run on `dsn_rows` only.
        let arms: Vec<&str> = ARMS
            .into_iter()
            .filter(|a| a.starts_with("pcb") || dsn_rows.contains(&row.as_str()))
            .collect();
        let mut listed: Vec<&str> = by_arm.keys().map(String::as_str).collect();
        listed.sort_unstable();
        let mut want = arms.clone();
        want.sort_unstable();
        assert_eq!(listed, want, "{row}");
        for arm in arms {
            let template = string(&json, &format!("arms.{arm}.command"));
            // Placeholders are whole words between fixed neighbours (`OUT` is also in `ROUTE`).
            let filled = template
                .replace(
                    "FREEROUTING_BIN=ENGINE ",
                    &format!("FREEROUTING_BIN={engine} "),
                )
                .replace(
                    "ROUTE_TIMEOUT_S=TIMEOUT ",
                    &format!("ROUTE_TIMEOUT_S={timeout} "),
                )
                .replace("OUT/ROW/", &format!("{out}/{row}/"))
                .replace(" TREE/", &format!(" {tree}/"))
                .replace(" BOARD ", &format!(" {board} "))
                .replace(" OPTIONS ", &format!(" {tree}/{OPTIONS} "));
            assert_eq!(by_arm[arm].as_str(), Some(filled.as_str()), "{row} {arm}");
            let off_arm = arm.replace("-on", "-off");
            let off = by_arm[off_arm.as_str()].as_str().unwrap();
            if arm.ends_with("-on") {
                // The -on call is its -off call with the shared grade, its own outdir and the flag.
                let memory = "ROUTE_MEMORY_KB=8000000 ";
                let expected =
                    off.replacen(
                        memory,
                        &format!("{memory}SHARE_GRADE_WITH={out}/{row}/{off_arm} "),
                        1,
                    )
                    .replace(
                        &format!(" {out}/{row}/{off_arm} -- "),
                        &format!(" {out}/{row}/{arm} -- "),
                    ) + &format!(" {FLAG}");
                assert_eq!(by_arm[arm].as_str(), Some(expected.as_str()), "{row} {arm}");
            } else {
                assert!(!off.contains(FLAG) && !off.contains("SHARE_GRADE_WITH"));
            }
            let call = by_arm[arm].as_str().unwrap();
            assert!(call.contains(&format!("{tree}/{SCRIPT} ")), "{call}");
            assert!(call.contains("ROUTE_MEMORY_KB=8000000"), "{call}");
            assert!(
                call.contains(&format!("ROUTE_TIMEOUT_S={timeout} ")),
                "{call}"
            );
            assert!(call.contains(" -- --max-passes 2 --timeout 240"), "{call}");
        }
    }
    let heavy: Vec<&str> = match json.get("heavy_run") {
        Some(Json::Array(list)) => list.iter().filter_map(Json::as_str).collect(),
        other => panic!("heavy_run: {other:?}"),
    };
    let run = format!("{tree}/{HELPER} run {tree}/{FILE}");
    assert_eq!(heavy.iter().filter(|c| c.contains(&run)).count(), 1);
}

/// The measuring script is K13.5's, unchanged (K13.json and K14.json pin the same hash); the
/// helper is the registered one.
#[test]
fn the_scripts_are_the_registered_ones() {
    let json = json();
    assert_eq!(string(&json, "script.path"), SCRIPT);
    let script = std::fs::read(repo().join(SCRIPT)).unwrap();
    assert_eq!(string(&json, "script.sha256"), sha256_hex(&script));
    let k13 = Json::parse(&read("docs/roadmap/prereg/K13.json")).unwrap();
    assert_eq!(k13.path("script.sha256"), json.path("script.sha256"));
    assert_eq!(string(&json, "helper.path"), HELPER);
    let helper = std::fs::read(repo().join(HELPER)).unwrap();
    assert_eq!(string(&json, "helper.sha256"), sha256_hex(&helper));
}

#[test]
fn the_registration_is_pinned_by_its_hash() {
    assert_eq!(registration().sha256, REGISTRATION_SHA256);
}
