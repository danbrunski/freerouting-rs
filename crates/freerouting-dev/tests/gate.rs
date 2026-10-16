//! `freerouting-dev gate`: the problem classes, the pass rule, the baseline
//! file and the command line (F03.1), preregistration (F03.4) and the records
//! collected from a bench (via-in-pad on a real session).

use std::path::{Path, PathBuf};
use std::process::Command;

use freerouting_dev::gate::{
    Class, Mark, PassRule, Settings, baseline_from, check, default_metrics, trial,
};
use freerouting_dev::gate_prereg::{Prereg, check_registered, check_registration};
use freerouting_dev::gate_selftest::{self, records_text, row_text};
use freerouting_dev::json_read::Json;

fn parse(text: &str) -> Json {
    Json::parse(text).unwrap()
}

type Values = Vec<(&'static str, f64)>;

fn unrouted(off: f64, on: f64) -> (Values, Values) {
    (
        vec![("unrouted", off), ("via_in_pad", 0.0)],
        vec![("unrouted", on), ("via_in_pad", 0.0)],
    )
}

fn row(name: &str, board: &str, off: f64, on: f64) -> String {
    let (a, b) = unrouted(off, on);
    row_text(name, board, &a, &b)
}

fn three_boards() -> Json {
    parse(&records_text(
        &[
            row("a", "A", 10.0, 7.0),
            row("b", "B", 20.0, 15.0),
            row("c", "C", 5.0, 5.0),
        ],
        "",
    ))
}

fn baseline() -> Json {
    baseline_from(&three_boards(), &Settings::default()).unwrap()
}

fn has(outcome: &freerouting_dev::gate::Outcome, class: Class, prefix: &str) -> bool {
    outcome
        .problems
        .iter()
        .any(|p| p.class == class && p.text.starts_with(prefix))
}

#[test]
fn the_self_test_passes() {
    gate_selftest::run().unwrap();
}

#[test]
fn a_run_matches_its_own_baseline_and_passes() {
    let outcome = check(&three_boards(), &baseline(), &Settings::default());
    assert!(outcome.passed(), "{}", outcome.to_text());
    let text = outcome.to_text();
    assert!(text.contains("improved 2/3 board(s)"), "{text}");
    assert!(text.contains("neutral: C"), "{text}");
    assert!(text.contains("1 run in 8"), "{text}");
}

#[test]
fn a_reversed_row_is_inverted_not_drift() {
    let reversed = parse(&records_text(
        &[
            row("a", "A", 10.0, 12.0),
            row("b", "B", 20.0, 15.0),
            row("c", "C", 5.0, 5.0),
        ],
        "",
    ));
    let outcome = check(&reversed, &baseline(), &Settings::default());
    assert!(has(
        &outcome,
        Class::Inverted,
        "a.unrouted: baseline 10 -> 7, measured 10 -> 12"
    ));
    assert!(has(&outcome, Class::Inverted, "a: mark improve -> regress"));
    assert!(!outcome.problems.iter().any(|p| p.class == Class::Drift));
    assert!(!outcome.passed());
}

#[test]
fn an_inverted_float_is_inverted_even_inside_the_tolerance_band() {
    let rows = |on: f64| {
        parse(&records_text(
            &[
                row_text("a", "A", &[("length_mm", 1000.0)], &[("length_mm", on)]),
                row_text("b", "B", &[("length_mm", 1000.0)], &[("length_mm", 900.0)]),
                row_text("c", "C", &[("length_mm", 1000.0)], &[("length_mm", 900.0)]),
            ],
            "",
        ))
    };
    let base = baseline_from(&rows(1000.0000001), &Settings::default()).unwrap();
    // 1000.0000001 is "equal" at the 1e-6 band; 999.9999 is a reversal of
    // the (tiny) upward move and must be INVERTED, not waved through.
    let outcome = check(&rows(999.9999), &base, &Settings::default());
    assert!(
        has(&outcome, Class::Inverted, "a.length_mm"),
        "{}",
        outcome.to_text()
    );
}

#[test]
fn integers_drift_exactly_and_floats_by_tolerance() {
    let drifted = parse(&records_text(
        &[
            row("a", "A", 10.0, 6.0),
            row("b", "B", 20.0, 15.0),
            row("c", "C", 5.0, 5.0),
        ],
        "",
    ));
    let outcome = check(&drifted, &baseline(), &Settings::default());
    assert!(has(
        &outcome,
        Class::Drift,
        "a.on.unrouted: baseline 7, measured 6"
    ));
    assert!(!outcome.problems.iter().any(|p| p.class == Class::Inverted));

    let float_rows = |off: f64, on: f64| {
        parse(&records_text(
            &[
                row_text("a", "A", &[("length_mm", off)], &[("length_mm", on)]),
                row_text("b", "B", &[("length_mm", 10.0)], &[("length_mm", 9.0)]),
                row_text("c", "C", &[("length_mm", 10.0)], &[("length_mm", 9.0)]),
            ],
            "",
        ))
    };
    let base = baseline_from(&float_rows(10.0, 9.0), &Settings::default()).unwrap();
    let inside = check(&float_rows(10.0, 9.0 + 1e-9), &base, &Settings::default());
    assert!(inside.passed(), "{}", inside.to_text());
    let outside = check(&float_rows(10.0, 9.05), &base, &Settings::default());
    assert!(
        has(&outside, Class::Drift, "a.on.length_mm"),
        "{}",
        outside.to_text()
    );
}

#[test]
fn a_changed_mark_is_inverted() {
    let mut base = baseline();
    let Json::Object(top) = &mut base else {
        panic!()
    };
    let Some(Json::Object(rows)) = top.get_mut("rows") else {
        panic!()
    };
    let Some(Json::Object(a)) = rows.get_mut("a") else {
        panic!()
    };
    a.insert("mark".into(), Json::String("regress".into()));
    let outcome = check(&three_boards(), &base, &Settings::default());
    assert!(has(&outcome, Class::Inverted, "a: mark regress -> improve"));
}

#[test]
fn a_deleted_row_is_an_orphan_and_scope_never_hides_it() {
    let deleted = parse(&records_text(
        &[row("a", "A", 10.0, 7.0), row("b", "B", 20.0, 15.0)],
        "",
    ));
    let outcome = check(&deleted, &baseline(), &Settings::default());
    assert!(has(&outcome, Class::Orphan, "c:"), "{}", outcome.to_text());
    let narrow = Settings {
        scope: Some(["a".to_string()].into_iter().collect()),
        ..Settings::default()
    };
    let outcome = check(&deleted, &baseline(), &narrow);
    assert!(has(&outcome, Class::Orphan, "c:"));
}

#[test]
fn a_declared_row_that_was_not_measured_is_missing_unless_out_of_scope() {
    let run = parse(&records_text(
        &[row("a", "A", 10.0, 7.0), row("b", "B", 20.0, 15.0)],
        "\"declared\": [\"a\", \"b\", \"c\"], ",
    ));
    let outcome = check(&run, &baseline(), &Settings::default());
    assert!(has(&outcome, Class::Missing, "c:"));
    let narrow = Settings {
        scope: Some(["a".to_string(), "b".to_string()].into_iter().collect()),
        ..Settings::default()
    };
    assert!(
        !check(&run, &baseline(), &narrow)
            .problems
            .iter()
            .any(|p| p.class == Class::Missing)
    );
}

#[test]
fn a_new_row_is_named() {
    let run = parse(&records_text(
        &[
            row("a", "A", 10.0, 7.0),
            row("b", "B", 20.0, 15.0),
            row("c", "C", 5.0, 5.0),
            row("d", "D", 5.0, 4.0),
        ],
        "",
    ));
    assert!(has(
        &check(&run, &baseline(), &Settings::default()),
        Class::NewRow,
        "d:"
    ));
}

#[test]
fn malformed_baselines_are_reported_not_raised() {
    for text in [
        "[1]",
        "3",
        "\"x\"",
        "null",
        "{}",
        "{\"rows\": 3}",
        "{\"rows\": []}",
    ] {
        let outcome = check(&three_boards(), &parse(text), &Settings::default());
        assert!(
            has(&outcome, Class::Malformed, "baseline"),
            "{text}: {}",
            outcome.to_text()
        );
        assert!(!outcome.passed());
    }
    let with_row_a = |row: Json| {
        let mut base = baseline();
        let Json::Object(top) = &mut base else {
            panic!()
        };
        let Some(Json::Object(rows)) = top.get_mut("rows") else {
            panic!()
        };
        rows.insert("a".into(), row);
        base
    };
    let outcome = check(
        &three_boards(),
        &with_row_a(Json::Number(5.0)),
        &Settings::default(),
    );
    assert!(has(&outcome, Class::Malformed, "baseline row a"));
    let sideways = parse("{\"mark\": \"sideways\", \"arms\": {}}");
    let outcome = check(&three_boards(), &with_row_a(sideways), &Settings::default());
    assert!(has(&outcome, Class::Malformed, "baseline row a: bad mark"));
}

#[test]
fn malformed_records_are_reported_not_raised() {
    for text in ["[]", "{\"rows\": 1}", "{\"rows\": {\"a\": 4}}", "{}"] {
        let outcome = check(&parse(text), &baseline(), &Settings::default());
        assert!(
            outcome.problems.iter().any(|p| p.class == Class::Malformed),
            "{text}: {}",
            outcome.to_text()
        );
    }
    let bad_value = parse(&records_text(
        &[
            row_text("a", "A", &[("unrouted", 10.0)], &[("unrouted", 7.0)])
                .replace("7", "\"seven\""),
        ],
        "",
    ));
    let outcome = check(&bad_value, &baseline(), &Settings::default());
    assert!(has(
        &outcome,
        Class::Malformed,
        "a.on.unrouted: expected a number"
    ));
    let fractional = parse(&records_text(&[row("a", "A", 10.0, 7.5)], ""));
    let outcome = check(&fractional, &baseline(), &Settings::default());
    assert!(has(
        &outcome,
        Class::Malformed,
        "a.on.unrouted: 7.5 is not an integer"
    ));
}

#[test]
fn two_improving_rows_on_one_board_are_refused_as_n_1() {
    let run = parse(&records_text(
        &[row("a1", "A", 10.0, 7.0), row("a2", "A", 20.0, 15.0)],
        "",
    ));
    let base = baseline_from(&run, &Settings::default()).unwrap();
    let outcome = check(&run, &base, &Settings::default());
    assert!(!outcome.rule_ok);
    assert!(
        outcome.to_text().contains("N=1 distinct boards"),
        "{}",
        outcome.to_text()
    );
    // And two boards are too few as well.
    let two = parse(&records_text(
        &[row("a", "A", 10.0, 7.0), row("b", "B", 20.0, 15.0)],
        "",
    ));
    let out = check(
        &two,
        &baseline_from(&two, &Settings::default()).unwrap(),
        &Settings::default(),
    );
    assert!(!out.rule_ok && out.to_text().contains("N=2 distinct boards"));
}

#[test]
fn the_pass_rule_branches() {
    let rule = PassRule::default();
    let run = |marks: &[Mark]| {
        let rows: Vec<_> = marks
            .iter()
            .enumerate()
            .map(|(i, m)| (format!("r{i}"), format!("B{i}"), *m, false))
            .collect();
        trial(&rows, &rule).0
    };
    use Mark::{Improve as I, Neutral as N, Regress as R, Skip as S};
    assert!(run(&[I, I, I]));
    assert!(run(&[I, I, N]), "N-1 improve, none regress");
    assert!(!run(&[I, N, N]));
    assert!(!run(&[I, I, R]), "any regression fails");
    assert!(!run(&[I, I]), "N=2 is too few");
    assert!(run(&[I, I, I, N]), "3 of 4");
    assert!(!run(&[I, I, N, N]), "2 of 4");
    assert!(run(&[I, I, N, S]), "a skipped row is not a board");
    assert!(run(&[]));
    // A rejected row is not on trial; it is named.
    let rows = vec![
        ("a".to_string(), "A".to_string(), Mark::Regress, true),
        ("b".to_string(), "B".to_string(), Mark::Improve, false),
        ("c".to_string(), "C".to_string(), Mark::Improve, false),
        ("d".to_string(), "D".to_string(), Mark::Improve, false),
    ];
    let (ok, lines) = trial(&rows, &rule);
    assert!(ok && lines[0].contains("rejected: 1 row(s)"));
    // A board with a regressing row regresses even if another row improves.
    let rows = vec![
        ("a1".to_string(), "A".to_string(), Mark::Improve, false),
        ("a2".to_string(), "A".to_string(), Mark::Regress, false),
        ("b".to_string(), "B".to_string(), Mark::Improve, false),
        ("c".to_string(), "C".to_string(), Mark::Improve, false),
    ];
    assert!(!trial(&rows, &rule).0);
    // A stricter registered rule.
    let strict = PassRule {
        improve_all_but: 0,
        ..rule
    };
    let rows: Vec<_> = [I, I, N]
        .iter()
        .enumerate()
        .map(|(i, m)| (format!("r{i}"), format!("B{i}"), *m, false))
        .collect();
    assert!(!trial(&rows, &strict).0);
}

#[test]
fn via_in_pad_is_a_hard_failure_even_if_unrouted_improved() {
    let run = parse(&records_text(
        &[
            row_text(
                "a",
                "A",
                &[("unrouted", 10.0), ("via_in_pad", 0.0)],
                &[("unrouted", 1.0), ("via_in_pad", 1.0)],
            ),
            row("b", "B", 20.0, 15.0),
            row("c", "C", 5.0, 3.0),
        ],
        "",
    ));
    let outcome = check(
        &run,
        &baseline_from(&run, &Settings::default()).unwrap(),
        &Settings::default(),
    );
    assert!(
        has(&outcome, Class::Hard, "a.on.via_in_pad = 1"),
        "{}",
        outcome.to_text()
    );
    assert!(outcome.to_text().contains("a [A]: regress"));
    assert!(!outcome.passed());
}

#[test]
fn a_guard_that_got_worse_regresses_even_if_unrouted_improved() {
    let run = parse(&records_text(
        &[
            row_text(
                "a",
                "A",
                &[("unrouted", 10.0), ("via_in_pad", 0.0), ("drc_real", 0.0)],
                &[("unrouted", 1.0), ("via_in_pad", 0.0), ("drc_real", 2.0)],
            ),
            row_text(
                "b",
                "B",
                &[("unrouted", 10.0), ("via_in_pad", 0.0), ("drc_real", 0.0)],
                &[("unrouted", 8.0), ("via_in_pad", 0.0), ("drc_real", 0.0)],
            ),
            row_text(
                "c",
                "C",
                &[("unrouted", 10.0), ("via_in_pad", 0.0), ("drc_real", 0.0)],
                &[("unrouted", 8.0), ("via_in_pad", 0.0), ("drc_real", 0.0)],
            ),
        ],
        "",
    ));
    let outcome = check(
        &run,
        &baseline_from(&run, &Settings::default()).unwrap(),
        &Settings::default(),
    );
    assert!(
        outcome.to_text().contains("a [A]: regress"),
        "{}",
        outcome.to_text()
    );
    assert!(!outcome.rule_ok);
}

#[test]
fn ungraded_rows_and_null_hard_metrics_fail() {
    let run = parse(
        "{\"rows\": {\"a\": {\"arms\": {\"off\": {\"unrouted\": 1}, \"on\": \
         {\"graded\": false, \"reason\": \"kicad-cli failed\"}}}}}",
    );
    let outcome = check(&run, &baseline(), &Settings::default());
    assert!(has(
        &outcome,
        Class::Ungraded,
        "a.on: not graded: kicad-cli failed"
    ));
    let null_hard = parse(
        "{\"rows\": {\"a\": {\"arms\": {\"off\": {\"via_in_pad\": 0}, \"on\": \
         {\"via_in_pad\": null}}}}}",
    );
    let outcome = check(&null_hard, &baseline(), &Settings::default());
    assert!(has(&outcome, Class::Ungraded, "a.on.via_in_pad: null"));
}

#[test]
fn a_registered_metric_the_run_lacks_is_missing() {
    let run = parse(&records_text(
        &[
            row("a", "A", 10.0, 7.0),
            row("b", "B", 20.0, 15.0),
            "\"c\": {\"board\": \"C\", \"arms\": {\"off\": {\"unrouted\": 5, \"via_in_pad\": 0}, \
             \"on\": {\"via_in_pad\": 0}}}"
                .to_string(),
        ],
        "\"metrics\": [\"unrouted\", \"via_in_pad\"], ",
    ));
    let outcome = check(&run, &baseline(), &Settings::default());
    assert!(
        has(&outcome, Class::Missing, "c.on.unrouted"),
        "{}",
        outcome.to_text()
    );
    let missing_arm = parse(&records_text(
        &[
            "\"a\": {\"board\": \"A\", \"arms\": {\"off\": {\"unrouted\": 1, \"via_in_pad\": 0}}}"
                .to_string(),
        ],
        "",
    ));
    assert!(has(
        &check(&missing_arm, &baseline(), &Settings::default()),
        Class::Missing,
        "a.on: the row has no such arm"
    ));
}

#[test]
fn counts_tags_and_input_hashes_are_compared() {
    let make = |by_type: &str, version: &str, hash: &str| {
        parse(&format!(
            "{{\"rows\": {{\"a\": {{\"board\": \"A\", \"input_sha256\": \"{hash}\", \"arms\": \
             {{\"off\": {{\"unrouted\": 4, \"via_in_pad\": 0, \"drc_real_by_type\": {by_type}, \
             \"kicad_version\": \"{version}\"}}, \"on\": {{\"unrouted\": 3, \"via_in_pad\": 0, \
             \"drc_real_by_type\": {by_type}, \"kicad_version\": \"{version}\"}}}}}}}}}}"
        ))
    };
    let base = baseline_from(
        &make("{\"clearance\": 1}", "10.0.7", "aa"),
        &Settings::default(),
    )
    .unwrap();
    let same = check(
        &make("{\"clearance\": 1}", "10.0.7", "aa"),
        &base,
        &Settings::default(),
    );
    assert!(same.problems.is_empty(), "{}", same.to_text());
    let by_type = check(
        &make("{\"clearance\": 2, \"shorting_items\": 1}", "10.0.7", "aa"),
        &base,
        &Settings::default(),
    );
    assert!(has(
        &by_type,
        Class::Drift,
        "a.on.drc_real_by_type[clearance]: baseline 1, measured 2"
    ));
    assert!(has(
        &by_type,
        Class::Drift,
        "a.off.drc_real_by_type[shorting_items]"
    ));
    let version = check(
        &make("{\"clearance\": 1}", "10.0.5", "aa"),
        &base,
        &Settings::default(),
    );
    assert!(has(&version, Class::Drift, "a.off.kicad_version"));
    let hash = check(
        &make("{\"clearance\": 1}", "10.0.7", "bb"),
        &base,
        &Settings::default(),
    );
    assert!(has(&hash, Class::Drift, "a.input_sha256"));
    let not_counts = parse(
        "{\"rows\": {\"a\": {\"arms\": {\"off\": {\"unrouted\": 4, \"via_in_pad\": 0, \
         \"drc_real_by_type\": 3}, \"on\": {\"unrouted\": 3, \"via_in_pad\": 0, \
         \"drc_real_by_type\": {}}}}}}",
    );
    let out = check(&not_counts, &base, &Settings::default());
    assert!(
        out.problems
            .iter()
            .any(|p| p.class == Class::Malformed && p.text.contains("drc_real_by_type"))
    );
}

#[test]
fn null_against_a_number_is_drift() {
    let base_run = parse(&records_text(
        &[row_text(
            "a",
            "A",
            &[("unrouted", 4.0), ("via_in_pad", 0.0), ("drc_real", 1.0)],
            &[("unrouted", 3.0), ("via_in_pad", 0.0), ("drc_real", 1.0)],
        )],
        "",
    ));
    let base = baseline_from(&base_run, &Settings::default()).unwrap();
    let run = parse(
        "{\"rows\": {\"a\": {\"board\": \"A\", \"arms\": {\"off\": {\"unrouted\": 4, \"via_in_pad\": 0, \
         \"drc_real\": null}, \"on\": {\"unrouted\": 3, \"via_in_pad\": 0, \"drc_real\": 1}}}}}",
    );
    let out = check(&run, &base, &Settings::default());
    assert!(
        has(
            &out,
            Class::Drift,
            "a.off.drc_real: baseline 1, measured null"
        ),
        "{}",
        out.to_text()
    );
}

#[test]
fn write_baseline_refuses_partial_runs() {
    let ok = three_boards();
    assert!(baseline_from(&ok, &Settings::default()).is_ok());
    let partial = parse(&records_text(
        &[
            row("a", "A", 10.0, 7.0),
            row("b", "B", 20.0, 15.0),
            row("c", "C", 5.0, 5.0),
        ],
        "\"partial\": true, ",
    ));
    let err = baseline_from(&partial, &Settings::default()).unwrap_err();
    assert!(
        err.iter()
            .any(|p| p.class == Class::Refused && p.text.contains("partial"))
    );
    let declared = parse(&records_text(
        &[row("a", "A", 10.0, 7.0)],
        "\"declared\": [\"a\", \"zzz\"], ",
    ));
    assert!(baseline_from(&declared, &Settings::default()).is_err());
    let narrow = Settings {
        scope: Some(["a".to_string()].into_iter().collect()),
        ..Settings::default()
    };
    assert!(baseline_from(&ok, &narrow).is_err());
    assert!(baseline_from(&parse("{\"rows\": {}}"), &Settings::default()).is_err());
    assert!(baseline_from(&parse("[]"), &Settings::default()).is_err());
    let hole = parse(&records_text(
        &["\"a\": {\"arms\": {\"off\": {\"unrouted\": 1, \"via_in_pad\": 0}, \"on\": {\"via_in_pad\": 0}}}".to_string()],
        "\"metrics\": [\"unrouted\", \"via_in_pad\"], ",
    ));
    assert!(baseline_from(&hole, &Settings::default()).is_err());
}

#[test]
fn rejected_rows_are_judged_against_their_marks_not_the_rule() {
    let run = parse(&records_text(
        &[
            row("a", "A", 10.0, 12.0).replace("\"board\"", "\"rejected\": true, \"board\""),
            row("b", "B", 10.0, 9.0),
            row("c", "C", 10.0, 9.0),
            row("d", "D", 10.0, 9.0),
        ],
        "",
    ));
    let base = baseline_from(&run, &Settings::default()).unwrap();
    let outcome = check(&run, &base, &Settings::default());
    assert!(outcome.passed(), "{}", outcome.to_text());
    assert!(outcome.to_text().contains("rejected: 1 row(s)"));
}

#[test]
fn the_default_metric_table_ranks_hard_limits_first() {
    let names: Vec<_> = default_metrics().into_iter().map(|m| m.name).collect();
    assert_eq!(&names[..2], ["via_in_pad", "below_minimum"]);
}

// --- the command line ---------------------------------------------------------

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("gate")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn gate(args: &[&str]) -> (i32, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_freerouting-dev"))
        .arg("gate")
        .args(args)
        .output()
        .unwrap();
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned()
            + &String::from_utf8_lossy(&output.stderr),
    )
}

fn write(dir: &Path, name: &str, text: &str) -> String {
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path.display().to_string()
}

#[test]
fn cli_records_a_baseline_then_passes_and_fails() {
    let dir = scratch("cli");
    let records = write(
        &dir,
        "run.json",
        &records_text(
            &[
                row("a", "A", 10.0, 7.0),
                row("b", "B", 20.0, 15.0),
                row("c", "C", 5.0, 5.0),
            ],
            "",
        ),
    );
    let baseline = dir.join("base.json").display().to_string();
    // A missing baseline fails.
    let (code, out) = gate(&["--records", &records, "--baseline", &baseline]);
    assert_eq!((code, out.contains("does not exist")), (1, true), "{out}");
    // Record, then pass.
    let (code, out) = gate(&[
        "--records",
        &records,
        "--baseline",
        &baseline,
        "--write-baseline",
    ]);
    assert_eq!(code, 0, "{out}");
    let (code, out) = gate(&["--records", &records, "--baseline", &baseline]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("gate: PASS") && out.contains("1 run in 8"),
        "{out}"
    );
    // A reversed run fails with INVERTED.
    let reversed = write(
        &dir,
        "reversed.json",
        &records_text(
            &[
                row("a", "A", 10.0, 12.0),
                row("b", "B", 20.0, 15.0),
                row("c", "C", 5.0, 5.0),
            ],
            "",
        ),
    );
    let (code, out) = gate(&["--records", &reversed, "--baseline", &baseline]);
    assert_eq!(code, 1);
    assert!(
        out.contains("INVERTED a.unrouted") && !out.contains("DRIFT"),
        "{out}"
    );
    // --rows narrows MISSING but never ORPHAN.
    let deleted = write(
        &dir,
        "deleted.json",
        &records_text(&[row("a", "A", 10.0, 7.0), row("b", "B", 20.0, 15.0)], ""),
    );
    let (code, out) = gate(&[
        "--records",
        &deleted,
        "--baseline",
        &baseline,
        "--rows",
        "a",
    ]);
    assert_eq!(code, 1);
    assert!(out.contains("ORPHAN c:"), "{out}");
}

#[test]
fn cli_reports_a_non_object_baseline_as_malformed_with_exit_1() {
    let dir = scratch("cli-malformed");
    let records = write(
        &dir,
        "run.json",
        &records_text(&[row("a", "A", 10.0, 7.0)], ""),
    );
    for text in ["[1, 2]", "42", "\"rows\"", "not json at all"] {
        let baseline = write(&dir, "base.json", text);
        let (code, out) = gate(&["--records", &records, "--baseline", &baseline]);
        assert_eq!(code, 1, "{text}: {out}");
        assert!(
            out.contains("MALFORMED") && !out.contains("panicked"),
            "{text}: {out}"
        );
    }
}

#[test]
fn cli_refuses_a_partial_baseline_and_two_rows_of_one_board() {
    let dir = scratch("cli-partial");
    let partial = write(
        &dir,
        "partial.json",
        &records_text(
            &[
                row("a", "A", 10.0, 7.0),
                row("b", "B", 20.0, 15.0),
                row("c", "C", 5.0, 5.0),
            ],
            "\"partial\": true, ",
        ),
    );
    let baseline = dir.join("base.json").display().to_string();
    let (code, out) = gate(&[
        "--records",
        &partial,
        "--baseline",
        &baseline,
        "--write-baseline",
    ]);
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains("REFUSED") && !Path::new(&baseline).exists(),
        "{out}"
    );
    let one_board = write(
        &dir,
        "one.json",
        &records_text(&[row("a1", "A", 10.0, 7.0), row("a2", "A", 20.0, 15.0)], ""),
    );
    let (code, out) = gate(&[
        "--records",
        &one_board,
        "--baseline",
        &baseline,
        "--write-baseline",
    ]);
    assert_eq!(code, 0, "recording never fails the rule: {out}");
    let (code, out) = gate(&["--records", &one_board, "--baseline", &baseline]);
    assert_eq!(code, 1);
    assert!(out.contains("N=1 distinct boards"), "{out}");
}

#[test]
fn cli_usage_errors_exit_2_and_the_self_test_runs() {
    let (code, out) = gate(&["--self-test"]);
    assert_eq!((code, out.contains("self-test: ok")), (0, true), "{out}");
    let (code, _) = gate(&["--records", "/nonexistent/run.json", "--baseline", "b.json"]);
    assert_eq!(code, 2);
    let (code, _) = gate(&["--bogus"]);
    assert_eq!(code, 2);
    let (code, _) = gate(&["--records"]);
    assert_eq!(code, 2);
}

// --- preregistration -----------------------------------------------------------

const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HASH_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn prereg_text(rule: &str, metrics: &str) -> String {
    format!(
        "{{\"format\": \"freerouting-ab-prereg-1\", \"id\": \"t\", \"arms\": {{\"off\": \
         {{\"engine\": \"java\"}}, \"on\": {{\"engine\": \"rust\"}}}}, \"boards\": {{\"files\": \
         {{\"a\": \"{HASH_A}\", \"b\": \"{HASH_A}\", \"c\": \"{HASH_A}\"}}}}, \"metrics\": \
         {metrics}, \"pass_rule\": {rule}}}"
    )
}

const METRICS: &str = "[{\"name\": \"via_in_pad\", \"role\": \"hard\"}, {\"name\": \"unrouted\"}]";

fn registered_run(hash_b: &str, on_engine: &str) -> Json {
    let rows: Vec<String> = ["a", "b", "c"]
        .iter()
        .map(|n| {
            let hash = if *n == "b" { hash_b } else { HASH_A };
            row(n, n, 10.0, 7.0).replace(
                "\"board\"",
                &format!("\"input_sha256\": \"{hash}\", \"board\""),
            )
        })
        .collect();
    parse(&records_text(
        &rows,
        &format!(
            "\"arms\": {{\"off\": {{\"engine\": \"java\"}}, \"on\": {{\"engine\": \"{on_engine}\"}}}}, \
             \"metrics\": [\"via_in_pad\", \"unrouted\"], "
        ),
    ))
}

#[test]
fn a_registered_run_is_checked_against_the_registration() {
    let prereg = Prereg::parse(&prereg_text("{\"min_boards\": 3}", METRICS)).unwrap();
    let run = registered_run(HASH_A, "rust");
    assert!(check_registration(&prereg, &run).is_empty());
    let base = baseline_from(&run, &prereg.settings()).unwrap();
    let outcome = check_registered(&run, &base, &prereg);
    assert!(outcome.passed(), "{}", outcome.to_text());
    assert!(outcome.to_text().contains(&prereg.sha256));
}

#[test]
fn changing_a_board_between_registration_and_run_is_refused_naming_the_board() {
    let prereg = Prereg::parse(&prereg_text("{}", METRICS)).unwrap();
    let run = registered_run(HASH_B, "rust");
    let problems = check_registration(&prereg, &run);
    assert!(
        problems.iter().any(|p| p.class == Class::Refused
            && p.text
                .contains("board b changed between registration and run")),
        "{problems:?}"
    );
    assert_eq!(problems.len(), 1);
    let base = baseline_from(&registered_run(HASH_A, "rust"), &prereg.settings()).unwrap();
    assert!(!check_registered(&run, &base, &prereg).passed());
}

#[test]
fn arms_that_differ_from_the_registration_are_refused() {
    let prereg = Prereg::parse(&prereg_text("{}", METRICS)).unwrap();
    let problems = check_registration(&prereg, &registered_run(HASH_A, "other"));
    assert!(
        problems
            .iter()
            .any(|p| p.text.contains("arm on differs from the registration")),
        "{problems:?}"
    );
    let no_arms = parse(&records_text(&[row("a", "a", 1.0, 1.0)], ""));
    assert!(
        check_registration(&prereg, &no_arms)
            .iter()
            .any(|p| p.text.contains("does not describe its arms"))
    );
}

#[test]
fn unregistered_missing_and_hashless_boards_are_refused() {
    let prereg = Prereg::parse(&prereg_text("{}", METRICS)).unwrap();
    let run = parse(&records_text(
        &[
            row("a", "a", 1.0, 1.0).replace(
                "\"board\"",
                &format!("\"input_sha256\": \"{HASH_A}\", \"board\""),
            ),
            row("b", "b", 1.0, 1.0),
            row("zzz", "zzz", 1.0, 1.0),
        ],
        "\"arms\": {\"off\": {\"engine\": \"java\"}, \"on\": {\"engine\": \"rust\"}}, \
         \"baseline_arm\": \"on\", ",
    ));
    let problems = check_registration(&prereg, &run);
    let texts: Vec<_> = problems.iter().map(|p| p.text.as_str()).collect();
    assert!(
        texts
            .iter()
            .any(|t| t.contains("board zzz was run but is not registered")),
        "{texts:?}"
    );
    assert!(
        texts
            .iter()
            .any(|t| t.contains("board b: the run records no input hash"))
    );
    assert!(
        texts
            .iter()
            .any(|t| t.contains("board c is registered but missing"))
    );
    assert!(texts.iter().any(|t| t.contains("baseline_arm is \"on\"")));
}

#[test]
fn a_registered_metric_missing_from_the_run_is_missing() {
    let prereg = Prereg::parse(&prereg_text(
        "{}",
        "[{\"name\": \"via_in_pad\", \"role\": \"hard\"}, {\"name\": \"unrouted\"}, {\"name\": \"vias\"}]",
    ))
    .unwrap();
    let run = registered_run(HASH_A, "rust");
    let base = baseline_from(
        &run,
        &Settings {
            specs: prereg.specs.clone(),
            ..Settings::default()
        },
    );
    // The baseline can't be recorded with a hole in it...
    assert!(base.is_err());
    // ...and the run is told which metric is missing, at the list and per row.
    let outcome = check_registered(&run, &parse("{\"rows\": {}}"), &prereg);
    assert!(has(
        &outcome,
        Class::Missing,
        "metric vias: registered, but the run does not list it"
    ));
    assert!(
        has(&outcome, Class::Missing, "a.off.vias"),
        "{}",
        outcome.to_text()
    );
}

#[test]
fn a_registration_may_not_loosen_the_global_rule() {
    for (rule, needle) in [
        ("{\"min_boards\": 2}", "min_boards is 2"),
        ("{\"improve_all_but\": 2}", "improve_all_but"),
        ("{\"max_regress\": 1}", "max_regress"),
    ] {
        let errors = Prereg::parse(&prereg_text(rule, METRICS)).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|p| p.class == Class::Refused && p.text.contains(needle)),
            "{rule}: {errors:?}"
        );
    }
    // Hard limits stay hard, and via_in_pad is required.
    let errors = Prereg::parse(&prereg_text(
        "{}",
        "[{\"name\": \"via_in_pad\", \"role\": \"report\"}]",
    ))
    .unwrap_err();
    assert!(errors.iter().any(|p| p.text.contains("hard limit")));
    let errors = Prereg::parse(&prereg_text("{}", "[{\"name\": \"unrouted\"}]")).unwrap_err();
    assert!(
        errors
            .iter()
            .any(|p| p.text.contains("via_in_pad (role hard) is required"))
    );
    // A stricter rule is fine.
    assert!(
        Prereg::parse(&prereg_text(
            "{\"min_boards\": 5, \"improve_all_but\": 0}",
            METRICS
        ))
        .is_ok()
    );
}

#[test]
fn malformed_registrations_are_reported() {
    for text in [
        "[]",
        "not json",
        "{}",
        "{\"format\": \"freerouting-ab-prereg-1\", \"id\": \"x\", \"arms\": {\"off\": {}}}",
    ] {
        let errors = Prereg::parse(text).unwrap_err();
        assert!(
            errors.iter().any(|p| p.class == Class::Malformed),
            "{text}: {errors:?}"
        );
    }
    let bad = prereg_text("{}", "[{\"name\": \"x\", \"direction\": \"sideways\"}]");
    assert!(
        Prereg::parse(&bad)
            .unwrap_err()
            .iter()
            .any(|p| p.text.contains("direction"))
    );
    let bad = prereg_text("{}", "[{\"direction\": \"lower\"}]");
    assert!(
        Prereg::parse(&bad)
            .unwrap_err()
            .iter()
            .any(|p| p.text.contains("no `name`"))
    );
    let bad = prereg_text("{\"min_boards\": 1.5}", METRICS);
    assert!(
        Prereg::parse(&bad)
            .unwrap_err()
            .iter()
            .any(|p| p.text.contains("must be a count"))
    );
}

#[test]
fn a_registered_higher_is_better_metric_is_judged_the_other_way() {
    let metrics = "[{\"name\": \"via_in_pad\", \"role\": \"hard\"}, \
                   {\"name\": \"completion\", \"direction\": \"higher\"}]";
    let prereg = Prereg::parse(&prereg_text("{}", metrics)).unwrap();
    let rows: Vec<String> = ["a", "b", "c"]
        .iter()
        .map(|n| {
            row_text(
                n,
                n,
                &[("completion", 80.0), ("via_in_pad", 0.0)],
                &[("completion", 90.0), ("via_in_pad", 0.0)],
            )
            .replace(
                "\"board\"",
                &format!("\"input_sha256\": \"{HASH_A}\", \"board\""),
            )
        })
        .collect();
    let run = parse(&records_text(
        &rows,
        "\"arms\": {\"off\": {\"engine\": \"java\"}, \"on\": {\"engine\": \"rust\"}}, ",
    ));
    let base = baseline_from(&run, &prereg.settings()).unwrap();
    let outcome = check_registered(&run, &base, &prereg);
    assert!(outcome.passed(), "{}", outcome.to_text());
    assert!(outcome.to_text().contains("improved 3/3"));
}

// --- the worked example and the template ------------------------------------------

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn the_template_and_the_example_registration_parse() {
    let template = std::fs::read_to_string(repo().join("tools/ab/TEMPLATE.json")).unwrap();
    // The template is deliberately incomplete (placeholders), but is JSON of the right shape.
    let json = parse(&template);
    assert_eq!(
        json.get("format").and_then(Json::as_str),
        Some("freerouting-ab-prereg-1")
    );
    let example =
        std::fs::read_to_string(repo().join("tools/ab/examples/x01-head-to-head.prereg.json"))
            .unwrap();
    let prereg = Prereg::parse(&example).unwrap();
    assert_eq!(prereg.id, "X01-head-to-head");
    assert_eq!(prereg.rule, PassRule::default());
}

/// Pins the worked example: a changed registration changes this hash. A
/// legitimate change goes in its `amendments` list, and this value is then
/// updated in the same commit, with the reason.
#[test]
fn the_example_registration_is_pinned_by_its_hash() {
    let example =
        std::fs::read_to_string(repo().join("tools/ab/examples/x01-head-to-head.prereg.json"))
            .unwrap();
    let prereg = Prereg::parse(&example).unwrap();
    assert_eq!(
        prereg.sha256, "7cbd101b8c54ae32b30fd216e0d84590da3998928748d225fdde18d9c3ac9f55",
        "the example registration changed"
    );
}

#[test]
fn the_template_with_its_placeholders_filled_in_is_a_valid_registration() {
    let template = std::fs::read_to_string(repo().join("tools/ab/TEMPLATE.json")).unwrap();
    let filled = template.replace("REPLACE: sha256 of the input file, 64 hex digits", HASH_A);
    let prereg = Prereg::parse(&filled).unwrap();
    assert_eq!(prereg.specs.len(), 7);
    assert_eq!(prereg.rule, PassRule::default());
    // Unfilled, it is refused with the hash named.
    let errors = Prereg::parse(&template).unwrap_err();
    assert!(
        errors.iter().any(|p| p.text.contains("64 hex digits")),
        "{errors:?}"
    );
}

/// The worked example of tools/ab/README.md: the X01 head-to-head,
/// re-registered. A run that follows the registration is checked; one board
/// changed after registration is refused with the board named.
#[test]
fn the_x01_example_registration_checks_a_run_and_refuses_a_changed_board() {
    let example =
        std::fs::read_to_string(repo().join("tools/ab/examples/x01-head-to-head.prereg.json"))
            .unwrap();
    let prereg = Prereg::parse(&example).unwrap();
    let run = |changed: Option<&str>| {
        let rows: Vec<String> = prereg
            .boards
            .iter()
            .map(|(name, hash)| {
                let hash = if Some(name.as_str()) == changed {
                    HASH_B
                } else {
                    hash
                };
                // krt routes 3 more connections on every board but one, where it ties.
                let (java, krt) = if name == "video" {
                    (9.0, 9.0)
                } else {
                    (12.0, 9.0)
                };
                let fields = |unconnected: f64| {
                    vec![
                        ("via_in_pad", 0.0),
                        ("below_minimum", 0.0),
                        ("drc_real", 0.0),
                        ("kicad_unconnected", unconnected),
                        ("vias", 40.0),
                        ("length_mm", 1000.0),
                        ("wall_seconds", 5.0),
                    ]
                };
                row_text(name, name, &fields(java), &fields(krt))
                    .replace(
                        "{\"board\"",
                        &format!("{{\"input_sha256\": \"{hash}\", \"board\""),
                    )
                    .replace("\"arms\": {\"off\"", "\"arms\": {\"java\"")
                    .replace(", \"on\": {", ", \"krt\": {")
            })
            .collect();
        let arms: Json = parse(&example);
        let mut text = records_text(&rows, "");
        text.insert_str(
            1,
            &format!(
                "\"arms\": {}, \"baseline_arm\": \"java\", \"candidate_arm\": \"krt\", ",
                to_text(arms.get("arms").unwrap())
            ),
        );
        parse(&text)
    };
    let good = run(None);
    let baseline = baseline_from(&good, &prereg.settings()).unwrap();
    let outcome = check_registered(&good, &baseline, &prereg);
    assert!(outcome.passed(), "{}", outcome.to_text());
    assert!(
        outcome.to_text().contains("improved 4/5 board(s)"),
        "{}",
        outcome.to_text()
    );
    assert!(outcome.to_text().contains("1 run in 32"));
    let changed = run(Some("interf_u"));
    let outcome = check_registered(&changed, &baseline, &prereg);
    assert!(!outcome.passed());
    assert!(
        outcome
            .problems
            .iter()
            .any(|p| p.class == Class::Refused && p.text.contains("board interf_u changed between")),
        "{}",
        outcome.to_text()
    );
}

fn to_text(v: &Json) -> String {
    match v {
        Json::Null => "null".into(),
        Json::Bool(b) => b.to_string(),
        Json::Number(n) => n.to_string(),
        Json::String(s) => format!("{s:?}"),
        Json::Array(a) => format!("[{}]", a.iter().map(to_text).collect::<Vec<_>>().join(", ")),
        Json::Object(o) => format!(
            "{{{}}}",
            o.iter()
                .map(|(k, v)| format!("{k:?}: {}", to_text(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}
