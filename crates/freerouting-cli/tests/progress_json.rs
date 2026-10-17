//! `--progress json` (C08.3): every stderr line is one JSON object, `start`
//! first and `done` last, and the stream is the same on every run apart from
//! `elapsed_ms` and the times inside log lines.
//!
//! The golden file `tests/data/progress_j2_mp2.jsonl` holds the non-log
//! events of a routed run with `elapsed_ms` removed; `UPDATE_GOLDEN=1`
//! rewrites it. Log events are checked for shape and count only: their
//! texts carry times and paths.

mod common;

use common::*;
use freerouting::drc::json::JsonValue;

const FIXTURE: &str = "Issue413-test.dsn";
const ROUTED: &str = "Issue026-J2_reference.dsn";

fn parse_stream(text: &str) -> Vec<JsonValue> {
    text.lines()
        .map(|line| {
            JsonValue::parse(line).unwrap_or_else(|e| panic!("not JSON ({e}): {line}\n{text}"))
        })
        .collect()
}

fn event_of(value: &JsonValue) -> String {
    string_of(member(value, "event")).expect("an event name")
}

fn number(value: &JsonValue, key: &str) -> Option<i64> {
    match member(value, key) {
        Some(JsonValue::Number(n)) => n.parse().ok(),
        _ => None,
    }
}

/// The line without its `,"elapsed_ms":N` member (always the last).
fn strip_elapsed(line: &str) -> String {
    match line.find(",\"elapsed_ms\":") {
        Some(at) => format!("{}}}", &line[..at]),
        None => line.to_string(),
    }
}

/// The non-log lines of a stream, `elapsed_ms` removed.
fn normalised(text: &str) -> String {
    text.lines()
        .filter(|l| !l.starts_with("{\"event\":\"log\""))
        .map(|l| strip_elapsed(l) + "\n")
        .collect()
}

fn run_json(name: &str, fixture: &str, extra: &[&str]) -> (std::process::Output, String) {
    let dir = scratch(name);
    copy_fixtures(&dir, &[fixture]);
    let mut args = vec!["--progress", "json", "-de", fixture, "-do", "out.ses"];
    args.extend_from_slice(extra);
    let out = run_in(&dir, &args);
    let text = stderr(&out);
    (out, text)
}

#[test]
fn every_line_is_json_between_start_and_done() {
    let (out, text) = run_json(
        "progress-off",
        FIXTURE,
        &["--router.autorouter.enabled=false"],
    );
    assert_eq!(code(&out), 0, "{text}");
    let events = parse_stream(&text);
    assert_eq!(event_of(&events[0]), "start");
    assert_eq!(number(&events[0], "schema_version"), Some(1));
    let last = events.last().unwrap();
    assert_eq!(event_of(last), "done");
    assert_eq!(number(last, "exit_code"), Some(i64::from(code(&out))));
    assert_eq!(
        string_of(member(last, "state")).as_deref(),
        Some("completed")
    );
    assert!(number(last, "elapsed_ms").is_some());
    assert_eq!(events.iter().filter(|e| event_of(e) == "done").count(), 1);

    let phases: Vec<String> = events
        .iter()
        .filter(|e| event_of(e) == "progress")
        .filter_map(|e| string_of(member(e, "phase")))
        .collect();
    assert_eq!(
        phases.first().map(String::as_str),
        Some("load"),
        "{phases:?}"
    );
    assert_eq!(
        phases.last().map(String::as_str),
        Some("write"),
        "{phases:?}"
    );
    for event in events.iter().filter(|e| event_of(e) == "log") {
        assert!(string_of(member(event, "level")).is_some());
        assert!(string_of(member(event, "text")).is_some());
    }
    assert!(
        events.iter().any(|e| event_of(e) == "log"
            && string_of(member(e, "text"))
                .is_some_and(|t| t.contains("finished with state: COMPLETED"))),
        "{text}"
    );
}

#[test]
fn a_usage_error_is_a_complete_stream() {
    let dir = scratch("progress-usage");
    let out = run_in(&dir, &["--progress", "json", "-de", "x.dsn"]);
    assert_eq!(code(&out), 2);
    let events = parse_stream(&stderr(&out));
    let kinds: Vec<String> = events.iter().map(event_of).collect();
    assert_eq!(kinds, ["start", "log", "done"]);
    assert_eq!(number(&events[2], "exit_code"), Some(2));
    assert_eq!(
        string_of(member(&events[2], "state")).as_deref(),
        Some("usage_error")
    );
    assert_eq!(
        string_of(member(&events[1], "level")).as_deref(),
        Some("error")
    );
}

#[test]
fn a_routed_stream_has_the_pass_events_and_matches_the_golden_file() {
    let (out1, text1) = run_json("progress-routed-1", ROUTED, &["-mp", "2"]);
    let (out2, text2) = run_json("progress-routed-2", ROUTED, &["-mp", "2"]);
    assert_eq!(code(&out1), 0, "{text1}");
    assert_eq!(code(&out2), 0, "{text2}");
    let events = parse_stream(&text1);
    assert_eq!(parse_stream(&text2).len(), events.len(), "same events");

    // Two `route` pass-end events, passes 1 and 2, with the pass lines' counts.
    let pass_ends: Vec<&JsonValue> = events
        .iter()
        .filter(|e| {
            event_of(e) == "progress"
                && string_of(member(e, "phase")).as_deref() == Some("route")
                && member(e, "pass_end") == Some(&JsonValue::Bool(true))
        })
        .collect();
    assert_eq!(pass_ends.len(), 2, "{text1}");
    assert_eq!(number(pass_ends[0], "pass"), Some(1));
    assert_eq!(number(pass_ends[1], "pass"), Some(2));
    let pass_lines: Vec<String> = events
        .iter()
        .filter(|e| event_of(e) == "log")
        .filter_map(|e| string_of(member(e, "text")))
        .filter(|t| t.starts_with("Auto-routing pass #"))
        .collect();
    assert_eq!(pass_lines.len(), 2, "{text1}");
    for (event, line) in pass_ends.iter().zip(&pass_lines) {
        let (_, tail) = line.rsplit_once(" (").expect("a score in parentheses");
        let (unrouted, tail) = tail.split_once(" unrouted and ").unwrap();
        let violations = tail.split(' ').next().unwrap();
        assert_eq!(
            number(event, "incomplete"),
            unrouted.parse::<i64>().ok(),
            "{line}"
        );
        assert_eq!(
            number(event, "violations"),
            violations.parse::<i64>().ok(),
            "{line}"
        );
    }
    // The phases come in order: load, fanout (if any), route, write.
    let order = ["load", "fanout", "route", "optimize", "write"];
    let ranks: Vec<usize> = events
        .iter()
        .filter(|e| event_of(e) == "progress")
        .map(|e| {
            let phase = string_of(member(e, "phase")).unwrap();
            order.iter().position(|p| *p == phase).expect(&phase)
        })
        .collect();
    assert!(ranks.windows(2).all(|w| w[0] <= w[1]), "{ranks:?}");
    assert!(ranks.contains(&2) && ranks.contains(&4), "{ranks:?}");

    // The same stream on two runs, and the golden file.
    let first = normalised(&text1);
    assert_eq!(first, normalised(&text2), "two runs differ");
    let golden = repo("crates/freerouting-cli/tests/data/progress_j2_mp2.jsonl");
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&golden, &first).unwrap();
    }
    let expected = std::fs::read_to_string(&golden).expect("the golden file");
    assert_eq!(first, expected, "UPDATE_GOLDEN=1 rewrites the golden file");
}

#[test]
fn without_the_flag_the_log_stays_text() {
    let (out, text) = run_json("progress-text", FIXTURE, &["--progress", "off"]);
    assert_eq!(code(&out), 0, "{text}");
    assert!(!text.contains("\"event\""), "{text}");
    assert!(text.contains("finished with state: COMPLETED"), "{text}");
}
