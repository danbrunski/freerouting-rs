//! Pins K15.51's registration (`docs/roadmap/prereg/K15.51.json`), which also gives K15.50 its
//! preregistered measurement. Three arms per row under profile `enhanced` and the KiCad plugin's
//! default options: `enhanced` (K15.48's `pcb-off`), `fix` (`enhanced` plus
//! `router.java_quirks.fanout_two_pass_cycle=false`, K15.50's switch) and `fix-pair` (`fix` plus
//! K15.48's two pour settings). Two verdicts on one run: the pair, the gate's verdict pair
//! `fix -> fix-pair` with K15.48's totals rule, guards and thresholds; and the switch, the reported
//! pair `enhanced -> fix`, judged by hand with N06's no-regress semantics and the route time not
//! up. The `.kicad_pcb` input only, over `tools/bench/sets/k15-fill-pair.txt`, K15.37's file
//! unchanged: the same 17 rows and boards. Registered 2026-10-23 before any route or grade of its
//! arms.
//!
//! The registration is K15.48's with the differences it lists in `differences`: three arms, two
//! verdicts, validity (a) per arm and (b), (d) per pair, a decision table for both verdicts, its
//! own tree, output and build directories, and the post-run scripts pinned in `post_run`. Rule,
//! totals, noise bands, metrics, guards (with the pair's arms in place of `pcb-off` and `pcb-on`),
//! `must_judge`, the pass cap, the router deadline, the call timeout, the script, the helper and the
//! grader's hashes are K15.48's.
//!
//! How it is amended: a change goes in the registration's `amendments` with its date and reason,
//! and updates the hashes here in the same change (`AMENDMENTS` too). The registration asks for
//! amendment 1 after the build and before any arm runs (the commit, the binary, the grader, plugin
//! and post-run hashes). Nothing registered (arms, boards, options, guards, rules, noise) is edited
//! after a number exists.
//!
//! The boards and the post-run scripts live under the main checkout's `target/` (not in git), by
//! absolute path; a missing one is skipped, a present one must hash as registered.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use freerouting::quality::feasibility::report::sha256_hex;
use freerouting_dev::gate::Role;
use freerouting_dev::gate_prereg::{PairRole, Prereg};
use freerouting_dev::json_read::Json;

const FILE: &str = "docs/roadmap/prereg/K15.51.json";
const K15_48: &str = "docs/roadmap/prereg/K15.48.json";
const SET: &str = "tools/bench/sets/k15-fill-pair.txt";
/// The grader files this test reads from the tree, at K15.48's hashes (none changed since).
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
const SCRIPT: &str = "tools/kicad-reader/plugin-input-ab.sh";
const HELPER: &str = "tools/bench/k15_all_pours.py";
const OPTIONS: &str = "tools/bench/k15-all-pours.options.json";
/// K15.50's switch, as `fix` and `fix-pair` set it.
const SWITCH_FLAG: &str = "--router.java_quirks.fanout_two_pass_cycle=false";
/// K15.48's candidate flags, in its order.
const PAIR_FLAGS: &str = "--router.pour_fill_contacts=true --router.pour_fill_target=true";
/// The arms in run order (and the JSON reader's sorted order).
const ARMS: [&str; 3] = ["enhanced", "fix", "fix-pair"];
/// The arm each arm shares a byte-identical session's grade with.
const SHARES_WITH: [Option<&str>; 3] = [None, Some("enhanced"), Some("fix")];
const MUST_JUDGE: [&str; 2] = ["pcu-76163c7", "canary-pwr-95882f00"];
const TREE: &str = "target/orchestrator/k1551/tree";
const OUT: &str = "target/orchestrator/k1551/out";
const ENGINE: &str = "/home/thebo/code/freerouting-rs/target/orchestrator/k1551/target-release/release/freerouting-rs";
/// 17 rows x 3 arms (`.kicad_pcb` only).
const CALLS: usize = 51;
const AMENDMENTS: usize = 1;
const REGISTRATION_SHA256: &str =
    "a1d724d1dd2cabebe005923091f164b885eaa2ffcddc082674093ab8573f63d0";
const SET_SHA256: &str = "fc34109cde3f899ecfdc40dd50e477a74dcc7e5b6bf50a4b74bf1413aa355adb";
/// K15.14's registered helper (`helper.sha256` there), unchanged.
const HELPER_SHA256: &str = "8de88dd8209eb8d073d596bcba200fdcef9f1d5b0c4a9210136103214723488f";
/// The post-run scripts (`post_run.files`), outside the repository.
const POST_RUN: [&str; 3] = [
    "target/orchestrator/k1551/guards.py",
    "target/orchestrator/k1551/post.sh",
    "target/orchestrator/k1551/check_k1548.py",
];

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &str) -> String {
    std::fs::read_to_string(repo().join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn json() -> Json {
    Json::parse(&read(FILE)).unwrap()
}

fn k15_48() -> Json {
    Json::parse(&read(K15_48)).unwrap()
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

/// K15.48's text with this registration's directories and names.
fn ours(k15_48_text: &str) -> String {
    k15_48_text
        .replace("/k1548/", "/k1551/")
        .replace("K15_48_COMMIT", "K15_51_COMMIT")
        .replace("prereg_k15_48", "prereg_k15_51")
        .replace("K15.48.json", "K15.51.json")
}

/// K15.48's two arms as the pair's: `pcb-off` is `fix`, `pcb-on` is `fix-pair`.
fn pair_names(k15_48_text: &str) -> String {
    k15_48_text
        .replace("pcb-on", "fix-pair")
        .replace("pcb-off", "fix")
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
fn the_registration_is_valid_and_names_its_arms_pairs_and_metrics() {
    let prereg = registration();
    assert_eq!(prereg.id, "K15.51-switch-fill-pair");
    assert_eq!(prereg.names.base, "fix");
    assert_eq!(prereg.names.cand, "fix-pair");
    let arms: Vec<&str> = prereg.arms.keys().map(String::as_str).collect();
    assert_eq!(arms, ARMS);
    assert_eq!(prereg.set.as_deref(), Some(SET));
    assert!(prereg.totals_registered);
    assert_eq!(prereg.amendments, AMENDMENTS);
    assert_eq!(prereg.must_judge, MUST_JUDGE);
    // K15.48's metrics, in the same order and roles, with K15.48's definitions.
    let k48 = Prereg::parse(&read(K15_48)).unwrap();
    let want: Vec<(&str, Role)> = k48
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
    // The pair is the one verdict; the switch's pair and its reverse are reported, not counted.
    let pairs: Vec<(&str, &str, PairRole)> = prereg
        .pairs
        .iter()
        .map(|p| (p.arms.base.as_str(), p.arms.cand.as_str(), p.role))
        .collect();
    assert_eq!(
        pairs,
        [
            ("fix", "fix-pair", PairRole::Verdict),
            ("enhanced", "fix", PairRole::Reported),
            ("fix", "enhanced", PairRole::Reported),
        ]
    );
    assert!(prereg.pairs.iter().all(|p| p.rows.is_none()));
    let json = json();
    let k48 = k15_48();
    for key in [
        "format",
        "rule",
        "totals",
        "metrics",
        "metric_definitions",
        "options",
        "script",
        "stop_rule",
        "must_judge",
    ] {
        assert_eq!(json.get(key), k48.get(key), "{key}");
    }
    let definitions = object(&json, "metric_definitions");
    for spec in &prereg.specs {
        assert!(definitions.contains_key(&spec.name), "{}", spec.name);
    }
    assert_eq!(definitions.len(), prereg.specs.len());
    // The DSN reason is K15.48's, plus why the switch gets no DSN arm either.
    let no_dsn = string(&json, "no_dsn_why");
    assert!(no_dsn.starts_with(string(&k48, "no_dsn_why")));
    assert!(no_dsn.contains("The switch acts on any input"));
    let pairs_why = string(&json, "pairs_why");
    for fact in [
        "HARD lines",
        "candidate only",
        "--all-pairs",
        "one `accept`",
    ] {
        assert!(pairs_why.contains(fact), "pairs_why: {fact}");
    }
}

/// The question asks the switch's question first, then K15.48's after its first sentence, with
/// the switch on.
#[test]
fn the_questions_are_the_switchs_and_k15_48s_pair() {
    let json = json();
    let k48 = k15_48();
    let question = string(&json, "question");
    let (first, _) = question.split_once(". ").unwrap();
    assert!(first.contains("K15.50"));
    assert!(first.contains("fanout_two_pass_cycle"));
    let (_, k48_tail) = string(&k48, "question").split_once(". ").unwrap();
    let pair = k48_tail.replacen(
        "Under profile enhanced and",
        "under profile enhanced with `router.java_quirks.fanout_two_pass_cycle=false` and",
        1,
    );
    assert_ne!(pair, k48_tail);
    let (switch, rest) = question
        .split_once(
            "(2) The pair (the verdict pair fix -> fix-pair, judged only if the switch is kept): ",
        )
        .unwrap();
    assert_eq!(rest, pair);
    for fact in [
        "(1) The switch",
        "enhanced -> fix",
        "not worse than the set's noise",
        "the route time not up in total",
        "`false` joins the ENHANCED table",
    ] {
        assert!(switch.contains(fact), "{fact}");
    }
}

/// K15.48's totals rule, noise and guards for the pair, with `fix` and `fix-pair` in place of
/// `pcb-off` and `pcb-on` and guard (1) over all three arms; validity (a) per arm, (b) and (d) per
/// pair.
#[test]
fn the_rule_noise_and_guards_are_k15_48s() {
    let json = json();
    let k48 = k15_48();
    assert_eq!(string(&json, "rule"), "totals");
    assert_eq!(string(&json, "totals.accept"), "improve");
    assert_eq!(number(&json, "totals.min_boards"), 3.0);
    let k09 = Json::parse(&read("docs/roadmap/prereg/K09.json")).unwrap();
    assert_eq!(
        json.path("totals.noise.canary-pwr-95882f00"),
        k09.path("noise.canary-pwr")
    );
    let noise_why = string(&json, "noise_why");
    for fact in [
        "max(2, ceil(5 % of its connections))",
        "67.09",
        "72.32",
        "852",
        "331",
        "k1548/out/records.json",
        "k1550/runs/q-*",
        "the largest of the row's three arms",
        "Nothing of this set was run to derive it",
    ] {
        assert!(noise_why.contains(fact), "noise_why: {fact}");
    }
    // The bands are K15.48's list.
    let bands = |text: &str| {
        let from = text.find("the bands are ").unwrap();
        let to = text[from..].find(": a set noise of 72.32").unwrap();
        text[from..from + to].to_string()
    };
    assert_eq!(bands(noise_why), bands(string(&k48, "noise_why")));
    // The guards and the keep rule are K15.48's with the pair's arms; guard (1) covers all three.
    let guards = pair_names(string(&k48, "guards"))
        .replacen(
            "(1) in every arm that runs",
            "(1) in every arm that runs (enhanced, fix and fix-pair)",
            1,
        )
        .replacen(
            "the verdict pair and the reported pair",
            "the verdict pair and the two reported pairs",
            1,
        );
    let got = string(&json, "guards");
    assert!(got.starts_with(&guards), "guards");
    assert!(got[guards.len()..].contains("keep_rule_by_hand (2)"));
    for clause in [
        "absolutely",
        "no row worse than its own noise band",
        "1.25 x",
        "COMPLETED in fix and TIMED_OUT in fix-pair",
    ] {
        assert!(got.contains(clause), "{clause}");
    }
    assert_eq!(
        string(&json, "keep_rule"),
        format!(
            "the pair (the verdict pair fix -> fix-pair), read only when the switch is kept: {}",
            pair_names(string(&k48, "keep_rule"))
        )
    );
    let differences = strings(&json, "differences").join("\n");
    for fact in [
        "thresholds unchanged",
        "decided 2026-10-23",
        "--max-passes 2 --timeout 240",
        "8557bff",
        "51 calls",
        "No --all-pairs",
        "changed_since_k15_48",
        "post_run",
    ] {
        assert!(differences.contains(fact), "differences: {fact}");
    }
    // Validity: (a) per arm with the switch's snapshot key, (b) and (d) per pair, (c) this test.
    let validity = string(&json, "validity");
    for fact in [
        "without being left out of the other's",
        "settings_snapshot",
        "`java_quirks.fanout_two_pass_cycle` false",
        "(unset, so absent)",
        "--test prereg_k15_51",
        "design_sha256",
        "both hashes",
        "K15.49",
        "(a), (b) or (d) for a pair",
        "for either pair, fewer than min_boards (3) distinct rows are judged",
        "pcu-76163c7 or canary-pwr-95882f00 is not judged",
        "LEFT_OUT_PAIR",
        "LEFT_OUT_SWITCH",
    ] {
        assert!(validity.contains(fact), "validity: {fact}");
    }
    assert!(json.get("dsn_rows").is_none());
    // The limits: K15.48's pass cap, router deadline, call timeout and memory, and why they stay.
    let limits = string(&json, "limits");
    for fact in [
        "--max-passes 2 --timeout 240",
        "ROUTE_TIMEOUT_S",
        "ROUTE_MEMORY_KB",
        "51 calls",
        "111.2 s",
        "109.2 s",
        "pocketsdr-v3-0",
        "240.1 s and 242.0 s",
        "4.7 h",
    ] {
        assert!(limits.contains(fact), "limits: {fact}");
    }
    let grading = string(&json, "grading");
    assert!(grading.contains("SHARE_GRADE_WITH"));
    assert!(grading.contains("`cmp -s`"));
}

/// The switch's rule is written out as arithmetic, so the post-run script applies it mechanically.
#[test]
fn the_switch_is_judged_by_hand_with_its_arithmetic() {
    let json = json();
    let rule = string(&json, "keep_rule_by_hand");
    for fact in [
        "no-regress",
        "(i) S: delta = sum over J of unc(fix, r) - sum over J of unc(enhanced, r)",
        "holds when delta <= 0 or delta^2 <= sum over J of band(r)^2",
        "(ii) no row worse than twice its band: unc(fix, r) - unc(enhanced, r) <= 2 x band(r)",
        "(iii) guard (1) in every graded arm of every row (all three arms)",
        "(iv) guard (2): the sum over J of fix's drc_real is at most enhanced's",
        "(v) guard (3): unc(fix, r) <= unc(enhanced, r) + band(r)",
        "(vi) the route time is not up",
        "COMPLETED in enhanced and TIMED_OUT in fix",
        "is at most the sum of enhanced's over the same rows",
        "tenths of a second",
        "--arms enhanced,fix",
        "--arms fix,enhanced",
        "--write-baseline",
        "exit status and totals verdict",
        "target/orchestrator/k1551/guards.py",
    ] {
        assert!(rule.contains(fact), "keep_rule_by_hand: {fact}");
    }
    assert!(!rule.contains("--all-pairs"));
}

/// The decision table: not measured and breach for three arms, then the switch's verdict, then
/// (only after the switch is kept) the pair's three rows, K15.48's with this measurement's names.
#[test]
fn the_decision_table_maps_each_outcome_to_one_row() {
    let json = json();
    let k48 = k15_48();
    let table = object(&json, "decision_table");
    assert_eq!(
        table.keys().map(String::as_str).collect::<Vec<_>>(),
        [
            "breach",
            "not measured",
            "order",
            "pair kept",
            "pair not kept, guard",
            "pair not kept, no gain",
            "switch kept",
            "switch not kept",
        ]
    );
    let row = |key: &str| table[key].as_str().unwrap();
    let k48_row = |key: &str| string(&k48, &format!("decision_table.{key}")).to_string();
    let not_measured = row("not measured");
    for fact in [
        "for either pair",
        "fewer than min_boards (3)",
        "validity (c) fails",
        "K15.51 and K15.50 stay [~]",
        "nothing changes in the product",
    ] {
        assert!(not_measured.contains(fact), "not measured: {fact}");
    }
    let breach = row("breach");
    for fact in [
        "any of the three arms",
        "a defect of today's default",
        "a defect of the switch",
        "a defect of the pair",
        "the plugin does not offer `router.pour_fill_contacts`",
        "K15.30's `pour_fill_target` option, measured alone, stays",
        "decided 2026-10-20",
    ] {
        assert!(breach.contains(fact), "breach: {fact}");
    }
    let kept = row("switch kept");
    for fact in [
        "keep_rule_by_hand (2)",
        "ENHANCED",
        "settings/profile.rs",
        "`fanout_two_pass_cycle: Some(false)`",
        "N05.2",
        "DIVERGENCES",
        "settings/fields.rs",
        "profile `enhanced` uses `false`",
        "tests/settings_java_quirks.rs",
        "`PROMOTED`",
        "the_setting_is_unset_in_both_profiles_and_false_parses",
        "parity-check.sh",
        "K15.50 [x]",
        "the pair's verdict is read",
    ] {
        assert!(kept.contains(fact), "switch kept: {fact}");
    }
    let not_kept = row("switch not kept");
    for fact in [
        "the switch stays in neither profile",
        "K15.50 [x]",
        "The pair is not judged",
        "keep K15.48's numbers",
        "reported only",
        "the plugin keeps both options",
    ] {
        assert!(not_kept.contains(fact), "switch not kept: {fact}");
    }
    // The pair's rows are K15.48's, with this measurement's lead, note and numbers.
    let body = k48_row("kept")
        .split_once("over the judged rows: ")
        .unwrap()
        .1
        .replace(
            "with this measurement's totals",
            "with this measurement's totals (fix -> fix-pair, measured with the switch on)",
        )
        .replace("K15.48's note", "K15.51's note");
    assert_eq!(
        row("pair kept"),
        format!(
            "the switch kept (its row applied first), and validity, guard (1), S and guards (2), \
             (3), (4) of the verdict pair fix -> fix-pair all hold over its judged rows: {body}"
        )
    );
    let numbers = |text: &str| {
        text.replace(
            "in place of K15.43's",
            "in place of K15.48's, saying that they are measured with the switch on (fix -> \
             fix-pair: enhanced's fanout once the switch's row is applied)",
        )
        .replace("K15.48's note", "K15.51's note")
    };
    let swap = |key: &str, old: &str, new: &str| {
        let text = k48_row(key);
        numbers(&format!("{new}{}", text.strip_prefix(old).unwrap()))
    };
    assert_eq!(
        row("pair not kept, guard"),
        swap(
            "not kept, guard",
            "none of the above, S holds and any of guards (2), (3), (4) fails over the judged rows: ",
            "the switch kept, S of the verdict pair fix -> fix-pair holds and any of guards (2), \
             (3), (4) fails over its judged rows: "
        )
    );
    assert_eq!(
        row("pair not kept, no gain"),
        swap(
            "not kept, no gain",
            "neither of the above, and S fails over the judged rows: ",
            "the switch kept, and S of the verdict pair fix -> fix-pair fails over its judged rows: "
        )
    );
    for key in ["pair not kept, guard", "pair not kept, no gain"] {
        for fact in [
            "opt-in, unset in both profiles",
            "the plugin keeps both options",
            "tooltip",
            "docs/development/kicad-plugin.md",
            "in place of K15.48's, saying that they are measured with the switch on",
        ] {
            assert!(row(key).contains(fact), "{key}: {fact}");
        }
    }
    assert!(row("order").contains(
        "not measured, breach, switch not kept, switch kept; after switch kept, the pair's rows in \
         the order pair not kept no gain, pair not kept guard, pair kept"
    ));
}

/// The set is K15.37's file, unchanged; the rows, boards and rules files are K15.48's; every present
/// board and its rules files hash as registered.
#[test]
fn the_rows_are_k15_48s_and_match_their_hashes() {
    let json = json();
    let k48 = k15_48();
    let text = read(SET);
    assert_eq!(sha256_hex(text.as_bytes()), SET_SHA256);
    assert_eq!(string(&json, "boards.set"), SET);
    assert_eq!(string(&json, "boards.set_sha256"), SET_SHA256);
    assert_eq!(string(&k48, "boards.set_sha256"), SET_SHA256);
    assert!(text.starts_with("# K15.37 (docs/roadmap/prereg/K15.37.json)"));
    for key in ["rows", "files", "rules_files"] {
        let path = format!("boards.{key}");
        assert_eq!(json.path(&path), k48.path(&path), "{path}");
    }
    assert!(string(&json, "boards.prepare").contains("K15.51.json"));
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

/// The options are the plugin's defaults, verbatim (K15.14's file): the pour options are off in
/// them, so the arms' settings come from the command line alone.
#[test]
fn the_options_are_the_plugin_defaults() {
    let json = json();
    let text = read(OPTIONS);
    assert_eq!(text, "{\"schema\": 6}\n");
    assert_eq!(string(&json, "options.sha256"), sha256_hex(text.as_bytes()));
}

/// Every command is its arm's template filled in; `enhanced` is K15.48's `pcb-off` command with
/// this registration's directories, `fix` adds the switch and `fix-pair` adds K15.48's two flags.
#[test]
fn the_commands_are_k15_48s_form_for_three_arms() {
    let json = json();
    let k48 = k15_48();
    assert_eq!(string(&json, "run.out"), OUT);
    assert_eq!(string(&json, "run.root"), string(&k48, "run.root"));
    assert_eq!(string(&json, "run.script"), string(&k48, "run.script"));
    let run_what = string(&json, "run.what");
    assert!(
        run_what.starts_with(
            &ours(string(&k48, "run.what"))
                .split("pcb-off then pcb-on")
                .next()
                .unwrap()
                .to_string()
        )
    );
    assert!(run_what.contains("enhanced, then fix, then fix-pair"));
    let engine = string(&json, "placeholders.ENGINE")
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    assert_eq!(engine, ENGINE);
    for (key, text) in object(&json, "placeholders") {
        let k48_text = string(&k48, &format!("placeholders.{key}"));
        assert_eq!(text.as_str(), Some(ours(k48_text).as_str()), "{key}");
    }
    let base =
        ours(string(&k48, "arms.pcb-off.command")).replace("OUT/ROW/pcb-off", "OUT/ROW/enhanced");
    for (arm, shares) in ARMS.iter().zip(SHARES_WITH) {
        let a = |key: &str| format!("arms.{arm}.{key}");
        assert_eq!(string(&json, &a("board_input")), "kicad_pcb");
        assert_eq!(string(&json, &a("engine")), "as `engine`");
        assert_eq!(json.path(&a("options")), k48.path("arms.pcb-off.options"));
        let settings = object(&json, &a("settings"));
        let want: Vec<(&str, bool)> = match *arm {
            "enhanced" => vec![],
            "fix" => vec![("router.java_quirks.fanout_two_pass_cycle", false)],
            _ => vec![
                ("router.java_quirks.fanout_two_pass_cycle", false),
                ("router.pour_fill_contacts", true),
                ("router.pour_fill_target", true),
            ],
        };
        let got: Vec<(&str, bool)> = settings
            .iter()
            .map(|(k, v)| (k.as_str(), *v == Json::Bool(true)))
            .collect();
        assert_eq!(got, want, "{arm}");
        let flags = match *arm {
            "enhanced" => String::new(),
            "fix" => format!(" {SWITCH_FLAG}"),
            _ => format!(" {SWITCH_FLAG} {PAIR_FLAGS}"),
        };
        let mut template = base.replace("OUT/ROW/enhanced -- ", &format!("OUT/ROW/{arm} -- "));
        if let Some(other) = shares {
            template = template.replace(
                "ROUTE_MEMORY_KB=8000000 ",
                &format!("ROUTE_MEMORY_KB=8000000 SHARE_GRADE_WITH=OUT/ROW/{other} "),
            );
        }
        assert_eq!(
            string(&json, &a("command")),
            format!("{template}{flags}"),
            "{arm}"
        );
        // The arms' text names the plugin's flags and K15.49's zone order.
        assert!(string(&json, &a("what")).contains("--board-setup.router.*"));
        assert!(string(&json, &a("what")).contains("order_zones"));
        assert!(string(&json, &a("what")).contains("fanout_two_pass_cycle"));
    }
    assert!(string(&json, "arms.fix-pair.what").contains("K15.46"));
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
        for (arm, shares) in ARMS.iter().zip(SHARES_WITH) {
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
            let call = by_arm[*arm].as_str().unwrap();
            assert_eq!(call, filled, "{row} {arm}");
            let k48_off = ours(string(&k48, &format!("commands.{row}.pcb-off")));
            if *arm == "enhanced" {
                assert_eq!(
                    call,
                    k48_off.replace(&format!("{row}/pcb-off"), &format!("{row}/enhanced")),
                    "{row}"
                );
            }
            assert!(!call.contains("k1548"), "{call}");
            assert!(call.contains(&format!(" {board} ")), "{call}");
            assert!(call.contains(" -- --max-passes 2 --timeout 240"), "{call}");
            assert!(call.contains("ROUTE_TIMEOUT_S=300 "), "{call}");
            assert!(call.contains("ROUTE_MEMORY_KB=8000000 "), "{call}");
            assert!(call.contains(&format!(" {OUT}/{row}/{arm} -- ")), "{call}");
            assert_eq!(
                call.split_whitespace()
                    .find_map(|w| w.strip_prefix("SHARE_GRADE_WITH=")),
                shares
                    .map(|other| format!("{OUT}/{row}/{other}"))
                    .as_deref(),
                "{call}"
            );
            // No other setting reaches any arm.
            let settings = call.matches(" --router.").count();
            assert_eq!(
                settings,
                [0, 1, 3][ARMS.iter().position(|a| a == arm).unwrap()]
            );
            assert_eq!(call.ends_with(SWITCH_FLAG), *arm == "fix", "{call}");
            assert_eq!(call.ends_with(PAIR_FLAGS), *arm == "fix-pair", "{call}");
        }
    }
    let heavy = strings(&json, "heavy_run");
    let run = format!("{TREE}/{HELPER} run {TREE}/{FILE}");
    assert_eq!(heavy.iter().filter(|c| c.contains(&run)).count(), 1);
    assert!(heavy[0].contains("<K15_51_COMMIT"));
    assert!(heavy.iter().any(|c| c.contains("--test prereg_k15_51")));
    assert!(heavy.iter().any(|c| c.contains("amendment 1")));
    let last = heavy.last().unwrap();
    for fact in [
        "--left-out ROW=REASON",
        "LEFT_OUT_PAIR",
        "LEFT_OUT_SWITCH",
        "--write-baseline PAIR",
        "--arms enhanced,fix SWITCH",
        "--arms fix,enhanced SWITCH",
        "design_sha256",
        "target/orchestrator/k1551/guards.py",
        "post.sh",
    ] {
        assert!(last.contains(fact), "heavy_run: {fact}");
    }
    assert!(heavy.iter().all(|c| !c.contains("k1548/")));
    assert!(heavy.iter().all(|c| !c.contains("--all-pairs")));
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

/// The measuring script and the helper are K15.48's, unchanged; the grader lists K15.48's files at
/// K15.48's hashes (none changed since), and this test reads the oracle files and the snapshot
/// from the tree.
#[test]
fn the_scripts_and_the_grader_are_the_registered_ones() {
    let json = json();
    let k48 = k15_48();
    let script = std::fs::read(repo().join(SCRIPT)).unwrap();
    assert_eq!(string(&json, "script.sha256"), sha256_hex(&script));
    assert_eq!(string(&json, "helper.path"), HELPER);
    assert_eq!(json.path("helper.sha256"), k48.path("helper.sha256"));
    let helper = std::fs::read(repo().join(HELPER)).unwrap();
    assert_eq!(sha256_hex(&helper), HELPER_SHA256);
    assert_eq!(string(&json, "helper.sha256"), HELPER_SHA256);
    let helper_what = string(&json, "helper.what");
    for fact in [
        "three",
        "51 commands",
        "`baseline_arm` fix",
        "`candidate_arm` fix-pair",
    ] {
        assert!(helper_what.contains(fact), "helper: {fact}");
    }
    assert_eq!(json.path("grader.files"), k48.path("grader.files"));
    assert_eq!(
        json.path("grader.pinned_by_test"),
        k48.path("grader.pinned_by_test")
    );
    assert!(object(&json, "grader.changed_since_k15_48").is_empty());
    let files = object(&json, "grader.files");
    for (path, want) in PINNED_FILES {
        assert_eq!(files[path].as_str(), Some(want), "{path}");
        let bytes = std::fs::read(repo().join(path)).unwrap_or_else(|e| panic!("{path}: {e}"));
        assert_eq!(sha256_hex(&bytes), want, "{path}");
    }
}

/// The post-run scripts are pinned by sha256; where they are present (the main checkout's
/// `target/`, not in git) they must hash as registered.
#[test]
fn the_post_run_scripts_hash_as_registered() {
    let json = json();
    let files = object(&json, "post_run.files");
    assert_eq!(files.keys().map(String::as_str).collect::<Vec<_>>(), {
        let mut sorted = POST_RUN.to_vec();
        sorted.sort_unstable();
        sorted
    });
    let what = string(&json, "post_run.what");
    for fact in [
        "CHECK OK",
        "368 -> 264",
        "1.262 x",
        "--no-switch",
        "amendments",
    ] {
        assert!(what.contains(fact), "post_run: {fact}");
    }
    let root = Path::new(string(&json, "run.root"));
    for path in POST_RUN {
        let want = files[path].as_str().unwrap();
        assert_eq!(want.len(), 64, "{path}");
        let Ok(bytes) = std::fs::read(root.join(path)) else {
            continue;
        };
        assert_eq!(sha256_hex(&bytes), want, "{path}");
    }
}

/// What was seen before the registration is recorded with its sessions, so the later run can be
/// read against it.
#[test]
fn the_registration_records_what_was_seen_before_it() {
    let json = json();
    let text = read(FILE);
    assert!(!text.contains("@@"));
    assert!(!text.contains("TODO"));
    let seen = string(&json, "seen_before_registration");
    for fact in [
        "k1548/out",
        "guards.txt",
        "368 -> 264",
        "1.262 x",
        "k1550/",
        "d9a06d31",
        "runs/q-*",
        "runs/u-*",
        "runs/d-*",
        "d71ed1e6",
        "d0da7c8a",
        "c144b6f4",
        "f1d2da90",
        "cf00bc73",
        "1fa9e1d7",
        "23d27870",
        "316bd50a",
        "e9572570",
        "4519725e",
        "23bfbafe",
        "72.7 s",
        "78.6 s",
        "Not seen",
    ] {
        assert!(seen.contains(fact), "{fact}");
    }
    assert!(string(&json, "timing").contains("8557bff"));
    assert!(string(&json, "timing").contains("51 calls"));
    let known = string(&json, "known_effects");
    for fact in [
        "1.262 x",
        "445.5 s -> 434.7 s",
        "434.7 s -> 528.8 s",
        "1.217 x",
        "368 -> 268",
        "24 -> 28",
        "tests and does not assume",
    ] {
        assert!(known.contains(fact), "known_effects: {fact}");
    }
    let engine = string(&json, "engine");
    for fact in [
        "K15_51_COMMIT",
        "Amendment 1",
        "cc99ede",
        "422484d",
        "b6fa54a",
        "8557bff",
        "post_run",
    ] {
        assert!(engine.contains(fact), "engine: {fact}");
    }
    let not_registered = strings(&json, "not_registered").join("\n");
    for fact in [
        "profile legacy",
        "period-3 cycle on kit-dev-coldfire",
        "about period 4 on openmuscle",
        "decided 2026-10-23",
        "a repeated A B A rather than the first",
        "K15.46",
    ] {
        assert!(not_registered.contains(fact), "not_registered: {fact}");
    }
    // Amendment 1: the commit and the binary, before any arm runs.
    let amendment = match json.get("amendments") {
        Some(Json::Array(list)) => list.first().map_or("", |a| string(a, "reason")),
        other => panic!("amendments: {other:?}"),
    };
    assert!(amendment.contains("K15_51_COMMIT = main a673040"));
    assert!(amendment.contains("79f9fc48dc1efabc4bcbf494a7ce769054d1df23321c38261ba78e2df4d45069"));
}

#[test]
fn the_registration_is_pinned_by_its_hash() {
    assert_eq!(registration().sha256, REGISTRATION_SHA256);
}
