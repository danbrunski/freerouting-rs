//! The self-test of the gate, run at the start of every `gate` invocation.
//!
//! A gate whose own logic is untested is how a documented rule stays
//! documentation: it costs microseconds, and a real run can't reach most of
//! the branches (a real baseline has no ORPHAN until someone deletes a row).
//! A failure here is a bug in the gate, reported before any verdict.

use crate::gate::{Class, Mark, PassRule, Problem, Settings, baseline_from, check, trial};
use crate::gate_totals::{Accept, BoardTotal, SpeedClause, TotalsRule, Work, totals_trial};
use crate::json_read::Json;

/// A row piece of a records document: `values` as `name: number` pairs.
pub fn row_text(name: &str, board: &str, off: &[(&str, f64)], on: &[(&str, f64)]) -> String {
    let arm = |values: &[(&str, f64)]| {
        values
            .iter()
            .map(|(k, v)| format!("\"{k}\": {v}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "\"{name}\": {{\"board\": \"{board}\", \"arms\": {{\"off\": {{{}}}, \"on\": {{{}}}}}}}",
        arm(off),
        arm(on)
    )
}

/// A records document from row pieces and extra top-level members.
pub fn records_text(rows: &[String], extra: &str) -> String {
    format!(
        "{{\"format\": \"freerouting-gate-records-1\", {extra}\"rows\": {{{}}}}}",
        rows.join(", ")
    )
}

fn parse(text: &str) -> Json {
    Json::parse(text).expect("self-test JSON")
}

fn classes(problems: &[Problem]) -> Vec<Class> {
    problems.iter().map(|p| p.class).collect()
}

fn improving(name: &str, board: &str) -> String {
    row_text(
        name,
        board,
        &[("unrouted", 10.0), ("via_in_pad", 0.0)],
        &[("unrouted", 7.0), ("via_in_pad", 0.0)],
    )
}

fn neutral(name: &str, board: &str) -> String {
    row_text(
        name,
        board,
        &[("unrouted", 10.0), ("via_in_pad", 0.0)],
        &[("unrouted", 10.0), ("via_in_pad", 0.0)],
    )
}

fn worsening(name: &str, board: &str) -> String {
    row_text(
        name,
        board,
        &[("unrouted", 10.0), ("via_in_pad", 0.0)],
        &[("unrouted", 12.0), ("via_in_pad", 0.0)],
    )
}

fn baseline_of(records: &Json) -> Json {
    baseline_from(records, &Settings::default()).expect("self-test baseline")
}

/// Runs the checks; the labels of the failed ones are the error.
pub fn run() -> Result<(), Vec<String>> {
    let mut failed = Vec::new();
    let mut expect = |label: &str, ok: bool| {
        if !ok {
            failed.push(label.to_string());
        }
    };
    let rule = PassRule::default();
    let t = |marks: &[(&str, &str, Mark)]| {
        let rows: Vec<(String, String, Mark, bool)> = marks
            .iter()
            .map(|(r, b, m)| (r.to_string(), b.to_string(), *m, false))
            .collect();
        trial(&rows, &rule)
    };

    // The pass rule.
    let (ok, lines) = t(&[
        ("a", "A", Mark::Improve),
        ("b", "B", Mark::Improve),
        ("c", "C", Mark::Neutral),
    ]);
    expect("2 improved + 1 neutral on 3 boards passes", ok);
    expect(
        "the luck rate is printed",
        lines.iter().any(|l| l.contains("1 run in 8")),
    );
    expect(
        "neutral boards are printed",
        lines.iter().any(|l| l.contains("neutral: C")),
    );
    let (ok, _) = t(&[
        ("a", "A", Mark::Improve),
        ("b", "B", Mark::Neutral),
        ("c", "C", Mark::Neutral),
    ]);
    expect("1 improved of 3 fails", !ok);
    let (ok, _) = t(&[
        ("a", "A", Mark::Improve),
        ("b", "B", Mark::Improve),
        ("c", "C", Mark::Regress),
    ]);
    expect("any regression fails", !ok);
    let (ok, lines) = t(&[("a1", "A", Mark::Improve), ("a2", "A", Mark::Improve)]);
    expect(
        "two improving rows on one board are refused as N=1 distinct boards",
        !ok && lines.iter().any(|l| l.contains("N=1 distinct boards")),
    );
    let (ok, _) = t(&[
        ("a", "A", Mark::Improve),
        ("b", "B", Mark::Improve),
        ("b2", "B", Mark::Regress),
    ]);
    expect(
        "a board with one regressing row counts as regressed (and 2 boards are too few)",
        !ok,
    );
    let (ok, _) = t(&[]);
    expect("no rows on trial passes the rule", ok);

    // The totals rule (N02): a P04.1-shaped table (17 boards, 5 regressing, -12
    // in total, half the maze work), a total inside the noise, a board beyond
    // twice its band, a hard guard, and the non-regression variant.
    let b = |name: &str, base: f64, cand: f64, band: f64| BoardTotal {
        board: name.to_string(),
        signal: "unrouted".to_string(),
        base,
        cand,
        band,
    };
    let mut p04: Vec<BoardTotal> = (0..12)
        .map(|i| b(&format!("flat{i}"), 10.0, 10.0, 2.0))
        .collect();
    p04.extend([
        b("regress1", 24.0, 26.0, 5.0),
        b("regress2", 57.0, 64.0, 10.0),
        b("regress3", 19.0, 28.0, 10.0),
        b("win1", 285.0, 274.0, 5.0),
        b("win2", 57.0, 43.0, 5.0),
    ]);
    let mut speed_rule = TotalsRule::new(Accept::Improve);
    speed_rule.speed = Some(SpeedClause {
        metric: "maze_nodes".into(),
        min_drop_percent: 20.0,
    });
    let halved = Work {
        metric: "maze_nodes".into(),
        base: 39.1e9,
        cand: 19.6e9,
    };
    expect(
        "totals: a P04.1-shaped table passes through the speed clause",
        totals_trial(&p04, &speed_rule, Some(&halved), &[]).0,
    );
    expect(
        "totals: the same table fails on a total inside the noise without it",
        !totals_trial(&p04, &TotalsRule::new(Accept::Improve), None, &[]).0,
    );
    let three = |gain: f64| {
        vec![
            b("a", 20.0, 20.0 - gain, 3.0),
            b("b", 20.0, 20.0, 4.0),
            b("c", 20.0, 20.0, 0.0),
        ]
    };
    let improve = TotalsRule::new(Accept::Improve);
    expect(
        "totals: an improvement equal to the set noise (5) fails, above it passes",
        !totals_trial(&three(5.0), &improve, None, &[]).0
            && totals_trial(&three(6.0), &improve, None, &[]).0,
    );
    expect(
        "totals: a board beyond twice its band fails a great total",
        !totals_trial(
            &[
                b("a", 100.0, 40.0, 5.0),
                b("b", 40.0, 20.0, 5.0),
                b("c", 10.0, 15.0, 2.0),
            ],
            &improve,
            None,
            &[],
        )
        .0,
    );
    expect(
        "totals: a hard guard failure fails a great total",
        !totals_trial(
            &three(10.0),
            &improve,
            None,
            &["x.via_in_pad = 1".to_string()],
        )
        .0,
    );
    expect(
        "totals: fewer than 3 distinct boards are refused",
        !totals_trial(&three(10.0)[..2], &improve, None, &[]).0,
    );
    let no_regress = TotalsRule::new(Accept::NoRegress);
    expect(
        "totals: no-regress accepts a total within noise (+5) and rejects one beyond it (+6)",
        totals_trial(
            &[
                b("a", 20.0, 25.0, 3.0),
                b("b", 20.0, 20.0, 4.0),
                b("c", 20.0, 20.0, 0.0),
            ],
            &no_regress,
            None,
            &[],
        )
        .0 && !totals_trial(
            &[
                b("a", 20.0, 26.0, 3.0),
                b("b", 20.0, 20.0, 4.0),
                b("c", 20.0, 20.0, 0.0),
            ],
            &no_regress,
            None,
            &[],
        )
        .0,
    );

    // Marks and the comparator.
    let base_rows = vec![improving("a", "A"), improving("b", "B"), neutral("c", "C")];
    let records = parse(&records_text(&base_rows, ""));
    let baseline = baseline_of(&records);
    let same = check(&records, &baseline, &Settings::default());
    expect("a run matches its own baseline", same.passed());

    let reversed = parse(&records_text(
        &[
            row_text(
                "a",
                "A",
                &[("unrouted", 10.0), ("via_in_pad", 0.0)],
                &[("unrouted", 12.0), ("via_in_pad", 0.0)],
            ),
            improving("b", "B"),
            neutral("c", "C"),
        ],
        "",
    ));
    let out = check(&reversed, &baseline, &Settings::default());
    expect(
        "a reversed row is INVERTED, not DRIFT",
        out.problems
            .iter()
            .any(|p| p.class == Class::Inverted && p.text.starts_with("a.unrouted"))
            && !out
                .problems
                .iter()
                .any(|p| p.class == Class::Drift && p.text.starts_with("a.on.unrouted")),
    );
    let drifted = parse(&records_text(
        &[
            row_text(
                "a",
                "A",
                &[("unrouted", 10.0), ("via_in_pad", 0.0)],
                &[("unrouted", 6.0), ("via_in_pad", 0.0)],
            ),
            improving("b", "B"),
            neutral("c", "C"),
        ],
        "",
    ));
    let out = check(&drifted, &baseline, &Settings::default());
    expect(
        "a same-direction change is DRIFT, exactly for integers",
        out.problems
            .iter()
            .any(|p| p.class == Class::Drift && p.text.starts_with("a.on.unrouted"))
            && !classes(&out.problems).contains(&Class::Inverted),
    );

    // Orphans, missing, new rows.
    let deleted = parse(&records_text(
        &[improving("a", "A"), improving("b", "B")],
        "",
    ));
    let out = check(&deleted, &baseline, &Settings::default());
    expect(
        "a deleted row is an ORPHAN",
        out.problems
            .iter()
            .any(|p| p.class == Class::Orphan && p.text.starts_with("c:")),
    );
    let declared = parse(&records_text(
        &[improving("a", "A"), improving("b", "B")],
        "\"declared\": [\"a\", \"b\", \"c\"], ",
    ));
    let out = check(&declared, &baseline, &Settings::default());
    expect(
        "a declared but unmeasured row is MISSING",
        out.problems
            .iter()
            .any(|p| p.class == Class::Missing && p.text.starts_with("c:")),
    );
    let narrow = Settings {
        scope: Some(["a".to_string()].into_iter().collect()),
        ..Settings::default()
    };
    let out = check(&declared, &baseline, &narrow);
    expect(
        "--rows silences MISSING of rows out of scope",
        !classes(&out.problems).contains(&Class::Missing),
    );
    let out = check(&deleted, &baseline, &narrow);
    expect(
        "--rows never silences an ORPHAN",
        classes(&out.problems).contains(&Class::Orphan),
    );
    let extra = parse(&records_text(
        &[
            improving("a", "A"),
            improving("b", "B"),
            neutral("c", "C"),
            neutral("d", "D"),
        ],
        "",
    ));
    let out = check(&extra, &baseline, &Settings::default());
    expect(
        "a row the baseline never saw is a NEW ROW",
        out.problems
            .iter()
            .any(|p| p.class == Class::NewRow && p.text.starts_with("d:")),
    );

    // Malformed baselines are reported, not raised.
    for (label, text) in [
        ("an array", "[1, 2]"),
        ("a number", "3"),
        ("an empty object", "{}"),
        ("rows as an array", "{\"rows\": []}"),
    ] {
        let out = check(&records, &parse(text), &Settings::default());
        expect(
            &format!("a baseline that is {label} is MALFORMED"),
            out.problems.iter().any(|p| p.class == Class::Malformed) && !out.passed(),
        );
    }
    let bad_row = parse("{\"rows\": {\"a\": 5}}");
    let out = check(&records, &bad_row, &Settings::default());
    expect(
        "a baseline row that is not an object is MALFORMED",
        out.problems
            .iter()
            .any(|p| p.class == Class::Malformed && p.text.contains("row a")),
    );

    // Measurement problems.
    let hard = parse(&records_text(
        &[
            row_text(
                "a",
                "A",
                &[("unrouted", 10.0), ("via_in_pad", 0.0)],
                &[("unrouted", 1.0), ("via_in_pad", 2.0)],
            ),
            improving("b", "B"),
            neutral("c", "C"),
        ],
        "",
    ));
    let out = check(&hard, &baseline, &Settings::default());
    expect(
        "a via in a pad is a HARD failure and a regress even if unrouted improved",
        out.problems.iter().any(|p| p.class == Class::Hard) && !out.rule_ok,
    );
    let ungraded = parse(
        "{\"rows\": {\"a\": {\"arms\": {\"off\": {\"unrouted\": 1}, \"on\": {\"graded\": false, \
         \"reason\": \"no SES\"}}}}}",
    );
    let out = check(&ungraded, &baseline, &Settings::default());
    expect(
        "a row the oracle couldn't grade is UNGRADED",
        classes(&out.problems).contains(&Class::Ungraded),
    );
    let holed = parse(&records_text(
        &[
            improving("a", "A"),
            improving("b", "B"),
            "\"c\": {\"board\": \"C\", \"arms\": {\"off\": {\"unrouted\": 10, \"via_in_pad\": 0}, \
             \"on\": {\"via_in_pad\": 0}}}"
                .to_string(),
        ],
        "\"metrics\": [\"unrouted\", \"via_in_pad\"], ",
    ));
    let out = check(&holed, &baseline, &Settings::default());
    expect(
        "a registered metric the run did not measure is MISSING",
        out.problems
            .iter()
            .any(|p| p.class == Class::Missing && p.text.starts_with("c.on.unrouted")),
    );

    // Writing a baseline.
    let partial = parse(&records_text(&base_rows, "\"partial\": true, "));
    expect(
        "--write-baseline refuses a partial run",
        baseline_from(&partial, &Settings::default()).is_err(),
    );
    expect(
        "--write-baseline refuses a narrowed run",
        baseline_from(&records, &narrow).is_err(),
    );
    expect(
        "--write-baseline refuses a run with a hole",
        baseline_from(&holed, &Settings::default()).is_err(),
    );
    expect(
        "a worsening row is marked regress",
        check(
            &parse(&records_text(&[worsening("a", "A")], "")),
            &baseline,
            &Settings::default(),
        )
        .problems
        .iter()
        .any(|p| p.class == Class::Inverted && p.text.contains("mark")),
    );

    if failed.is_empty() {
        Ok(())
    } else {
        Err(failed)
    }
}
