//! Q20.3 at the command line: the `output_check` key of the result manifest
//! (`--router.output_check`, `repair` under profile `enhanced`).
//!
//! Fixture: `q20-input-violations.dsn` (nets A and D to route; the input
//! wiring has a 0.1 mm track and a clearance breach, which the check reports
//! as the input's and never changes).

mod common;

use common::*;
use freerouting::drc::json::JsonValue;

const INPUT: &str = "crates/freerouting/tests/data/quality/q20-input-violations.dsn";

fn run(dir: &std::path::Path, extra: &[&str]) -> JsonValue {
    std::fs::copy(repo(INPUT), dir.join("board.dsn")).unwrap();
    let mut args = vec![
        "-de",
        "board.dsn",
        "-do",
        "out.ses",
        "-mp",
        "2",
        "--result-json",
        "result.json",
        "--router.board_minimums.track_width_um=200",
    ];
    args.extend_from_slice(extra);
    let out = run_in(dir, &args);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = std::fs::read_to_string(dir.join("result.json")).expect("the manifest exists");
    JsonValue::parse(&text).expect("the manifest is JSON")
}

fn int(value: Option<&JsonValue>) -> i64 {
    match value {
        Some(JsonValue::Number(n)) => n.parse().unwrap(),
        other => panic!("not a number: {other:?}"),
    }
}

#[test]
fn the_manifest_carries_the_output_check() {
    let dir = scratch("result-output-check-on");
    let on = run(&dir, &[]);
    let check = member(&on, "output_check").expect("an output_check key");
    assert_eq!(string_of(member(check, "mode")).as_deref(), Some("repair"));
    for key in [
        "checked",
        "found",
        "removed",
        "retried",
        "repaired",
        "left_open",
        "remaining",
    ] {
        assert!(int(member(check, key)) >= 0, "{key}");
    }
    assert!(
        int(member(check, "checked")) > 0,
        "the routed copper of A and D"
    );
    assert_eq!(int(member(check, "found")), 0);
    assert!(matches!(
        member(check, "by_rule"),
        Some(JsonValue::Object(_))
    ));
    let input = member(check, "input").expect("input");
    assert_eq!(int(member(input, "track_width")), 1);
    assert!(int(member(input, "clearance")) >= 1);
    let log = std::fs::read_to_string(dir.join("out.ses")).unwrap();
    assert!(!log.is_empty());

    let dir = scratch("result-output-check-off");
    let off = run(&dir, &["--router.output_check=off"]);
    assert!(member(&off, "output_check").is_none());
}

#[test]
fn the_run_log_has_a_line_per_rule_and_a_total() {
    let dir = scratch("result-output-check-log");
    std::fs::copy(repo(INPUT), dir.join("board.dsn")).unwrap();
    let out = run_in(
        &dir,
        &[
            "-de",
            "board.dsn",
            "-do",
            "out.ses",
            "-mp",
            "2",
            "--router.output_check=report",
        ],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let log = format!("{}{}", stdout(&out), stderr(&out));
    assert!(log.contains("Output check: the board already has"), "{log}");
    assert!(log.contains("Output check (report):"), "{log}");
}
