//! `--stats json` (P01.2): the statistics change no result, and their
//! counts are the same from run to run.

mod common;

use common::*;
use freerouting::drc::json::JsonValue;

const FIXTURES: [&str; 3] = [
    "Issue413-test.dsn",
    "Issue433-my-board.dsn",
    "Issue143-rpi_splitter.dsn",
];

const ROUTE: [&str; 3] = ["-mp", "2", "--router.optimizer.enabled=false"];

fn route(dir: &std::path::Path, fixture: &str, out: &str, extra: &[&str]) -> std::process::Output {
    let mut args = vec!["-de", fixture, "-do", out];
    args.extend(ROUTE);
    args.extend(extra);
    run_in(dir, &args)
}

/// The stage names, their indexes and their `counts` blocks, without any
/// time or time-derived line.
fn counts_of(json: &str) -> Vec<String> {
    let mut kept = Vec::new();
    let mut in_counts = false;
    for line in json.lines() {
        let t = line.trim();
        if t.starts_with("\"stage\"") || t.starts_with("\"index\"") {
            kept.push(t.to_string());
        }
        if t.starts_with("\"counts\"") {
            in_counts = true;
            continue;
        }
        if in_counts {
            if t.starts_with('}') {
                in_counts = false;
            } else {
                kept.push(t.to_string());
            }
        }
    }
    kept
}

fn stages_of(json: &str) -> Vec<String> {
    json.lines()
        .filter_map(|l| l.trim().strip_prefix("\"stage\": \""))
        .map(|l| l.trim_end_matches("\",").to_string())
        .collect()
}

#[test]
fn stats_change_no_session_and_the_counts_repeat() {
    let dir = scratch("stats-route");
    copy_fixtures(&dir, &FIXTURES);
    for fixture in FIXTURES {
        let plain = route(&dir, fixture, "plain.ses", &[]);
        assert_eq!(code(&plain), 0, "{}", stderr(&plain));
        let first = route(&dir, fixture, "a.ses", &["--stats", "json"]);
        assert_eq!(code(&first), 0, "{}", stderr(&first));
        let second = route(
            &dir,
            fixture,
            "b.ses",
            &["--stats", "json", "--stats-out", "b-stats.json"],
        );
        assert_eq!(code(&second), 0, "{}", stderr(&second));
        let plain_ses = std::fs::read(dir.join("plain.ses")).unwrap();
        assert_eq!(
            plain_ses,
            std::fs::read(dir.join("a.ses")).unwrap(),
            "{fixture}: --stats changed the session"
        );
        assert_eq!(plain_ses, std::fs::read(dir.join("b.ses")).unwrap());
        assert!(!dir.join("plain.ses.stats.json").exists());
        let a = std::fs::read_to_string(dir.join("a.ses.stats.json")).unwrap();
        let b = std::fs::read_to_string(dir.join("b-stats.json")).unwrap();
        assert_eq!(counts_of(&a), counts_of(&b), "{fixture}: counts differ");
        // The run did work, in every stage the flow has.
        let stages = stages_of(&a);
        for stage in ["load", "pass", "drc", "write"] {
            assert!(stages.iter().any(|s| s == stage), "{fixture}: {stages:?}");
        }
        assert!(a.contains("\"connections_attempted\": "), "{a}");
        // Read the total instead of depending on the last counter's comma.
        // A load-only stage may legitimately have zero tree visits.
        let report = JsonValue::parse(&a).expect("valid statistics JSON");
        let total = member(&report, "total").expect("total statistics");
        let counts = member(total, "counts").expect("total counters");
        let Some(JsonValue::Number(visited)) = member(counts, "tree_nodes_visited") else {
            panic!("{fixture}: missing tree-work count");
        };
        assert!(
            visited.parse::<u64>().unwrap() > 0,
            "{fixture}: no tree work counted"
        );
        assert!(matches!(
            member(counts, "door_section_reallocations"),
            Some(JsonValue::Number(_))
        ));
        for name in [
            "a.ses",
            "b.ses",
            "plain.ses",
            "a.ses.stats.json",
            "b-stats.json",
        ] {
            let _ = std::fs::remove_file(dir.join(name));
        }
    }
}

#[test]
fn drc_mode_has_load_and_drc_stages() {
    let dir = scratch("stats-drc");
    copy_fixtures(&dir, &FIXTURES[..1]);
    let out = run_in(
        &dir,
        &[
            "-drc",
            "-de",
            FIXTURES[0],
            "--stats",
            "json",
            "--stats-out",
            "s.json",
        ],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let json = std::fs::read_to_string(dir.join("s.json")).unwrap();
    assert_eq!(stages_of(&json), ["load", "drc"], "{json}");
}

#[test]
fn a_bad_format_is_a_usage_error() {
    let dir = scratch("stats-bad");
    let out = run_in(&dir, &["--stats", "xml", "-de", "x.dsn", "-do", "x.ses"]);
    assert_eq!(code(&out), 2, "{}", stderr(&out));
    assert!(stderr(&out).contains("--stats"));
}
