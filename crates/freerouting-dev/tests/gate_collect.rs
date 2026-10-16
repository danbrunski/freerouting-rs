//! `freerouting-dev gate collect`: records from `bench.json` files, with the
//! via-in-pad check on real sessions (Java's route of the Issue508 SMD demo
//! board, and the same session with a via dropped on a pad).

use std::path::{Path, PathBuf};

use freerouting_dev::gate::{Class, Settings, baseline_from, check};
use freerouting_dev::gate_collect::{
    ArmSource, CollectOptions, bench_dir_name, collect, session_via_in_pad,
};
use freerouting_dev::json_read::Json;
use freerouting_dev::{load_board, read_text};

const BOARD: &str = "Issue508-SMD-routing-issue-demo";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn java_ses() -> String {
    read_text(&fixtures().join(format!("reference/route/{BOARD}.ses"))).unwrap()
}

/// The Java session plus a via on pad U1-1 (720000, -446000 in session units).
fn session_with_via_in_pad() -> String {
    let text = java_ses();
    let anchor = "(net \"NET_CROSS_A\"\n";
    assert!(text.contains(anchor));
    text.replacen(
        anchor,
        &format!("{anchor}        (via \"Via[0-1]_600:300_um\" 720000.0 -446000.0\n        )\n"),
        1,
    )
}

/// A directory with a `bench.json` for one board and the session the bench
/// would have saved.
fn bench_dir(name: &str, ses: &str, incomplete: u32) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("gate-collect")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    let written = format!("fixtures/{BOARD}.dsn");
    let board_dir = dir.join(bench_dir_name(&written));
    std::fs::create_dir_all(&board_dir).unwrap();
    std::fs::write(board_dir.join("java-run1.ses"), ses).unwrap();
    let path = fixtures().join(format!("{BOARD}.dsn"));
    let json = format!(
        "{{\"set\": \"s.txt\", \"aborted\": null, \"missing\": [], \
         \"engines\": [{{\"engine\": \"java\", \"command\": \"java -jar /x/freerouting-a773a70ec.jar\"}}], \
         \"boards\": [{{\"board\": \"{written}\", \"path\": {path:?}, \"engines\": [{{\"engine\": \"java\", \
         \"runs\": [{{\"run\": 1, \"wall_seconds\": 2.5, \"timed_out\": false, \"statistics\": \
         {{\"connections.incomplete_count\": {incomplete}, \"clearance_violations.total_count\": 0, \
         \"vias.total_count\": 2, \"traces.total_length_mm\": 123.45, \"bends.total_count\": 7}}}}]}}]}}]}}",
        path = path.display().to_string()
    );
    std::fs::write(dir.join("bench.json"), json).unwrap();
    dir.join("bench.json")
}

fn arm(name: &str, bench: PathBuf) -> ArmSource {
    ArmSource {
        name: name.into(),
        bench_json: bench,
        oracle_dir: None,
    }
}

#[test]
fn via_in_pad_counts_only_the_vias_the_session_adds() {
    let board = load_board(&fixtures().join(format!("{BOARD}.dsn"))).unwrap();
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("gate-collect-ses");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("java.ses"), java_ses()).unwrap();
    std::fs::write(dir.join("bad.ses"), session_with_via_in_pad()).unwrap();
    assert_eq!(
        session_via_in_pad(&board, &dir.join("java.ses")).unwrap(),
        0
    );
    assert_eq!(session_via_in_pad(&board, &dir.join("bad.ses")).unwrap(), 1);
    assert!(session_via_in_pad(&board, &dir.join("missing.ses")).is_err());
}

#[test]
fn collect_builds_records_the_gate_can_judge() {
    let off = bench_dir("off", &java_ses(), 3);
    let on = bench_dir("on", &java_ses(), 2);
    let options = CollectOptions {
        arms: vec![arm("off", off), arm("on", on)],
        engine: "java".into(),
        ..CollectOptions::default()
    };
    let mut notes = Vec::new();
    let records = collect(&options, &mut notes).unwrap();
    assert!(notes.is_empty(), "{notes:?}");
    let row = records.path(&format!("rows.fixtures/{BOARD}.dsn"));
    assert!(row.is_none(), "dots in a name need the object API");
    let Some(Json::Object(rows)) = records.get("rows") else {
        panic!()
    };
    let row = &rows[&format!("fixtures/{BOARD}.dsn")];
    assert_eq!(
        row.path("arms.off.unrouted").and_then(Json::as_f64),
        Some(3.0)
    );
    assert_eq!(
        row.path("arms.on.unrouted").and_then(Json::as_f64),
        Some(2.0)
    );
    assert_eq!(
        row.path("arms.on.via_in_pad").and_then(Json::as_f64),
        Some(0.0)
    );
    assert_eq!(
        row.path("arms.on.length_mm").and_then(Json::as_f64),
        Some(123.45)
    );
    assert_eq!(
        row.path("arms.on.engine_commit").and_then(Json::as_str),
        Some("a773a70ec")
    );
    assert_eq!(
        row.get("input_sha256").and_then(Json::as_str).map(str::len),
        Some(64)
    );
    assert_eq!(records.get("partial"), Some(&Json::Bool(false)));
    // One board is too few for the pass rule, but the baseline can be recorded and re-checked.
    let baseline = baseline_from(&records, &Settings::default()).unwrap();
    let outcome = check(&records, &baseline, &Settings::default());
    assert!(outcome.problems.is_empty(), "{}", outcome.to_text());
    assert!(!outcome.rule_ok);
    assert!(outcome.to_text().contains("N=1 distinct boards"));
}

#[test]
fn a_via_in_a_pad_in_the_candidate_session_is_a_hard_failure() {
    let off = bench_dir("vip-off", &java_ses(), 3);
    let on = bench_dir("vip-on", &session_with_via_in_pad(), 0);
    let options = CollectOptions {
        arms: vec![arm("off", off), arm("on", on)],
        engine: "java".into(),
        ..CollectOptions::default()
    };
    let records = collect(&options, &mut Vec::new()).unwrap();
    let outcome = check(
        &records,
        &baseline_from(&records, &Settings::default()).unwrap(),
        &Settings::default(),
    );
    assert!(
        outcome
            .problems
            .iter()
            .any(|p| p.class == Class::Hard && p.text.contains(".on.via_in_pad = 1")),
        "{}",
        outcome.to_text()
    );
    assert!(outcome.to_text().contains("regress"));
}

#[test]
fn runs_that_disagree_or_have_no_statistics_are_not_graded_and_missing_boards_make_the_run_partial()
{
    let off = bench_dir("dis-off", &java_ses(), 3);
    // A bench with no session file: no statistics, a note.
    let text = std::fs::read_to_string(&off).unwrap().replace(
        "\"statistics\":",
        "\"note\": \"no SES written\", \"statistics_gone\":",
    );
    let broken = off.with_file_name("broken.json");
    std::fs::write(&broken, text).unwrap();
    let options = CollectOptions {
        arms: vec![arm("off", off.clone()), arm("on", broken)],
        engine: "java".into(),
        ..CollectOptions::default()
    };
    let mut notes = Vec::new();
    let records = collect(&options, &mut notes).unwrap();
    assert!(
        notes.iter().any(|n| n.contains("no SES written")),
        "{notes:?}"
    );
    let outcome = check(&records, &parse_baseline_of_ok(), &Settings::default());
    assert!(
        outcome.problems.iter().any(|p| p.class == Class::Ungraded),
        "{}",
        outcome.to_text()
    );
    // A missing board listed by the bench makes the records partial.
    let missing = std::fs::read_to_string(&off)
        .unwrap()
        .replace("\"missing\": []", "\"missing\": [\"fixtures/gone.dsn\"]");
    let with_missing = off.with_file_name("missing.json");
    std::fs::write(&with_missing, missing).unwrap();
    let options = CollectOptions {
        arms: vec![arm("off", with_missing.clone()), arm("on", with_missing)],
        engine: "java".into(),
        ..CollectOptions::default()
    };
    let records = collect(&options, &mut Vec::new()).unwrap();
    assert_eq!(records.get("partial"), Some(&Json::Bool(true)));
    assert!(baseline_from(&records, &Settings::default()).is_err());
    assert!(
        collect(
            &CollectOptions {
                arms: vec![arm("only", off)],
                ..CollectOptions::default()
            },
            &mut Vec::new()
        )
        .is_err()
    );
}

fn parse_baseline_of_ok() -> Json {
    Json::parse("{\"rows\": {}}").unwrap()
}

#[test]
fn groups_map_variants_to_one_board() {
    let off = bench_dir("grp-off", &java_ses(), 3);
    let on = bench_dir("grp-on", &java_ses(), 2);
    let options = CollectOptions {
        arms: vec![arm("off", off), arm("on", on)],
        engine: "java".into(),
        groups: vec![("fixtures/Issue508".into(), "issue508".into())],
        ..CollectOptions::default()
    };
    let records = collect(&options, &mut Vec::new()).unwrap();
    let Some(Json::Object(rows)) = records.get("rows") else {
        panic!()
    };
    let row = rows.values().next().unwrap();
    assert_eq!(row.get("board").and_then(Json::as_str), Some("issue508"));
}
