//! The totals rule of `freerouting-dev gate --rule totals` (N02.1, N02.2):
//! the pure verdict on P04.1-shaped tables, the rule on records, the
//! registration format and the command line.
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use freerouting_dev::gate::{Class, Mark, Settings, baseline_from, check, trial};
use freerouting_dev::gate_cli;
use freerouting_dev::gate_prereg::{Prereg, check_registered};
use freerouting_dev::gate_totals::{
    Accept, BoardTotal, SpeedClause, TotalsRule, Work, judge, totals_trial,
};
use freerouting_dev::json_read::Json;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn board(name: &str, base: f64, cand: f64, band: f64) -> BoardTotal {
    BoardTotal {
        board: name.to_string(),
        signal: "unrouted".to_string(),
        base,
        cand,
        band,
    }
}

fn improve() -> TotalsRule {
    TotalsRule::new(Accept::Improve)
}

fn with_speed(mut rule: TotalsRule) -> TotalsRule {
    rule.speed = Some(SpeedClause {
        metric: "maze_nodes".to_string(),
        min_drop_percent: 20.0,
    });
    rule
}

fn work(base: f64, cand: f64) -> Work {
    Work {
        metric: "maze_nodes".to_string(),
        base,
        cand,
    }
}

fn text(lines: &[String]) -> String {
    lines.join("\n")
}

/// P04.1's measured unrouted counts, tree -> canonical (archived P04
/// checklist). The canary-pwr variants are one board and are summed
/// (529 -> 534). `bands` are illustrative: P04.1 registered none, because
/// the totals rule did not exist.
/// P04.1's clearance violations (GP8B 0 -> 2, smoothieboard 210 -> 211) are guard
/// failures under any rule and are left out here: this table is the signal only.
fn p04_1(bands: impl Fn(&str) -> f64) -> Vec<BoardTotal> {
    [
        ("my-board", 0.0, 0.0),
        ("ecc83", 0.0, 0.0),
        ("bm02", 0.0, 0.0),
        ("sonde", 11.0, 11.0),
        ("bm07", 0.0, 0.0),
        ("bm01", 24.0, 26.0),
        ("bm10", 0.0, 0.0),
        ("interf_u", 19.0, 19.0),
        ("CNH", 1.0, 1.0),
        ("z10", 37.0, 37.0),
        ("coldfire", 181.0, 181.0),
        ("CPU-85", 57.0, 64.0),
        ("GP8B", 61.0, 51.0),
        ("smoothieboard", 19.0, 28.0),
        ("LogicBoard", 285.0, 274.0),
        ("contribution-board", 57.0, 43.0),
        ("canary-pwr", 529.0, 534.0),
    ]
    .iter()
    .map(|(n, b, c)| board(n, *b, *c, bands(n)))
    .collect()
}

fn illustrative_band(name: &str) -> f64 {
    match name {
        "canary-pwr" => 10.0,
        "sonde" => 3.0,
        "bm01" | "interf_u" => 5.0,
        "z10" => 8.0,
        "coldfire" => 20.0,
        "CPU-85" | "GP8B" | "smoothieboard" => 10.0,
        "LogicBoard" => 40.0,
        "contribution-board" => 15.0,
        _ => 2.0,
    }
}

#[test]
fn p04_1_shaped_table_passes_only_through_the_speed_clause() {
    let boards = p04_1(illustrative_band);
    let total: (f64, f64) = boards
        .iter()
        .fold((0.0, 0.0), |a, b| (a.0 + b.base, a.1 + b.cand));
    assert_eq!(total, (1281.0, 1269.0), "P04.1's totals");
    // Maze work units 39.1 G -> 19.6 G nodes (the archived P04 checklist).
    let maze = work(39.1e9, 19.6e9);
    let (ok, lines) = totals_trial(&boards, &with_speed(improve()), Some(&maze), &[]);
    assert!(ok, "{}", text(&lines));
    assert!(
        text(&lines).contains("speed clause: maze_nodes"),
        "{lines:?}"
    );
    // The same table on quality alone: -12 is inside the set noise (52.6).
    let (ok, lines) = totals_trial(&boards, &improve(), None, &[]);
    assert!(!ok);
    assert!(
        text(&lines).contains("improvement 12 does not exceed the set noise 52.6"),
        "{}",
        text(&lines)
    );
    // The paired rule rejects it for its regressions: 2 improved, 5 regressed.
    let marks: Vec<(String, String, Mark, bool)> = boards
        .iter()
        .map(|b| {
            let mark = match b.cand.partial_cmp(&b.base) {
                Some(std::cmp::Ordering::Less) => Mark::Improve,
                Some(std::cmp::Ordering::Greater) => Mark::Regress,
                _ => Mark::Neutral,
            };
            (b.board.clone(), b.board.clone(), mark, false)
        })
        .collect();
    assert!(!trial(&marks, &Default::default()).0);
}

#[test]
fn even_the_smallest_bands_cannot_pass_p04_1_on_quality_alone() {
    // 16 boards at the floor of 2 and canary-pwr at its 10: sqrt(16 * 4 + 100) = 12.8 > 12.
    let boards = p04_1(|n| if n == "canary-pwr" { 10.0 } else { 2.0 });
    let (ok, lines) = totals_trial(&boards, &improve(), None, &[]);
    assert!(!ok);
    assert!(text(&lines).contains("set noise 12.81"), "{}", text(&lines));
}

#[test]
fn a_total_that_improves_only_within_noise_fails() {
    // Bands 3 and 4 and 0: noise exactly 5.
    let at = |gain: f64| {
        vec![
            board("a", 20.0, 20.0 - gain, 3.0),
            board("b", 20.0, 20.0, 4.0),
            board("c", 20.0, 20.0, 0.0),
        ]
    };
    let (ok, lines) = totals_trial(&at(4.0), &improve(), None, &[]);
    assert!(!ok, "{}", text(&lines));
    // An improvement equal to the noise is not "more than" the noise.
    let (ok, lines) = totals_trial(&at(5.0), &improve(), None, &[]);
    assert!(!ok, "{}", text(&lines));
    assert!(text(&lines).contains("set noise 5.00"));
    let (ok, lines) = totals_trial(&at(6.0), &improve(), None, &[]);
    assert!(ok, "{}", text(&lines));
}

#[test]
fn a_board_beyond_twice_its_band_fails_whatever_the_total_does() {
    let mut boards = vec![
        board("a", 100.0, 60.0, 5.0),
        board("b", 40.0, 20.0, 5.0),
        board("c", 10.0, 14.0, 2.0),
    ];
    // c: +4 = exactly 2 x 2, allowed; total -56 against noise 7.3.
    let (ok, lines) = totals_trial(&boards, &improve(), None, &[]);
    assert!(ok, "{}", text(&lines));
    boards[2].cand = 15.0;
    let (ok, lines) = totals_trial(&boards, &improve(), None, &[]);
    assert!(!ok);
    assert!(
        text(&lines).contains("worse by more than 2 x its noise band: c"),
        "{}",
        text(&lines)
    );
    assert!(text(&lines).contains("WORSE than 2 x its band"));
}

#[test]
fn a_hard_guard_failure_rejects_a_great_total() {
    let boards = vec![
        board("a", 100.0, 60.0, 5.0),
        board("b", 40.0, 20.0, 5.0),
        board("c", 10.0, 10.0, 2.0),
    ];
    let guards = vec!["c.via_in_pad = 1".to_string()];
    let (ok, lines) = totals_trial(&boards, &improve(), None, &guards);
    assert!(!ok);
    assert!(text(&lines).contains("hard guard: c.via_in_pad = 1"));
}

#[test]
fn at_least_three_distinct_boards() {
    let boards = vec![board("a", 100.0, 50.0, 2.0), board("b", 100.0, 50.0, 2.0)];
    let (ok, lines) = totals_trial(&boards, &improve(), None, &[]);
    assert!(!ok);
    assert!(text(&lines).contains("N=2"), "{}", text(&lines));
    let none: Vec<BoardTotal> = Vec::new();
    assert!(!totals_trial(&none, &improve(), None, &[]).0);
}

#[test]
fn no_regress_accepts_a_total_within_noise_and_needs_no_gain() {
    let rule = TotalsRule::new(Accept::NoRegress);
    let boards = |d: f64| {
        vec![
            board("a", 30.0, 30.0 + d, 3.0),
            board("b", 30.0, 30.0, 4.0),
            board("c", 30.0, 30.0, 0.0),
        ]
    };
    for d in [-9.0, 0.0, 5.0] {
        let (ok, lines) = totals_trial(&boards(d), &rule, None, &[]);
        assert!(ok, "change {d}: {}", text(&lines));
    }
    // +6 is worse than the noise 5 (and beyond 2 x 3 on board a).
    let (ok, lines) = totals_trial(&boards(6.0), &rule, None, &[]);
    assert!(!ok);
    assert!(text(&lines).contains("worse by 6, more than the set noise 5.00"));
    // The per-board clause still holds: a +8 board with a -9 board nets -1.
    let mixed = vec![
        board("a", 30.0, 38.0, 3.0),
        board("b", 30.0, 21.0, 4.0),
        board("c", 30.0, 30.0, 0.0),
    ];
    let (ok, lines) = totals_trial(&mixed, &rule, None, &[]);
    assert!(!ok);
    assert!(text(&lines).contains("worse by more than 2 x its noise band: a"));
}

#[test]
fn the_speed_clause_needs_quality_within_noise_and_a_20_percent_drop() {
    let rule = with_speed(improve());
    // Quality: +3 over noise 5 (within), no gain.
    let flat = vec![
        board("a", 30.0, 33.0, 3.0),
        board("b", 30.0, 30.0, 4.0),
        board("c", 30.0, 30.0, 0.0),
    ];
    assert!(totals_trial(&flat, &rule, Some(&work(100.0, 80.0)), &[]).0);
    assert!(!totals_trial(&flat, &rule, Some(&work(100.0, 81.0)), &[]).0);
    assert!(!totals_trial(&flat, &rule, None, &[]).0);
    assert!(!totals_trial(&flat, &improve(), Some(&work(100.0, 10.0)), &[]).0);
    // Worse than noise: a 90 % drop does not buy it back.
    let worse = vec![
        board("a", 30.0, 36.0, 3.0),
        board("b", 30.0, 30.0, 4.0),
        board("c", 30.0, 30.0, 0.0),
    ];
    assert!(!totals_trial(&worse, &rule, Some(&work(100.0, 10.0)), &[]).0);
    // The zero base is not a drop.
    assert!(!totals_trial(&flat, &rule, Some(&work(0.0, 0.0)), &[]).0);
}

// --- the rule on records ------------------------------------------------------

/// A metric of a row: name, baseline value and candidate value (`None`: null).
type Metric = (&'static str, Option<f64>, Option<f64>);

/// A row: `(metric, base, cand)` triples, `None` for null.
fn row(name: &str, board: &str, extra: &str, metrics: &[Metric]) -> String {
    let arm = |pick: fn(&Metric) -> Option<f64>| {
        metrics
            .iter()
            .map(|m| match pick(m) {
                Some(v) => format!("\"{}\": {v}", m.0),
                None => format!("\"{}\": null", m.0),
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "\"{name}\": {{\"board\": \"{board}\", {extra}\"arms\": {{\"off\": {{{}}}, \"on\": {{{}}}}}}}",
        arm(|m| m.1),
        arm(|m| m.2)
    )
}

fn records(rows: &[String]) -> Json {
    Json::parse(&format!(
        "{{\"format\": \"freerouting-gate-records-1\", \"rows\": {{{}}}}}",
        rows.join(", ")
    ))
    .unwrap()
}

fn totals_settings(rule: TotalsRule) -> Settings {
    Settings {
        totals: Some(rule),
        ..Settings::default()
    }
}

fn run(rows: &[String], rule: TotalsRule) -> freerouting_dev::gate::Outcome {
    let rec = records(rows);
    let settings = totals_settings(rule);
    let base = baseline_from(&rec, &settings).unwrap();
    check(&rec, &base, &settings)
}

const ZERO_GUARDS: [Metric; 2] = [
    ("via_in_pad", Some(0.0), Some(0.0)),
    ("drc_real", Some(0.0), Some(0.0)),
];

fn metrics(more: &[Metric]) -> Vec<Metric> {
    let mut out: Vec<Metric> = ZERO_GUARDS.to_vec();
    for (n, b, c) in more {
        let n: &'static str = match *n {
            "kicad_unconnected" => "kicad_unconnected",
            "unrouted" => "unrouted",
            "maze_nodes" => "maze_nodes",
            "below_minimum" => "below_minimum",
            "wall_seconds" => "wall_seconds",
            "via_in_pad" => "via_in_pad",
            "drc_real" => "drc_real",
            other => panic!("unknown test metric {other}"),
        };
        out.retain(|m| m.0 != n);
        out.push((n, *b, *c));
    }
    out
}

fn noisy(rule: &mut TotalsRule) {
    rule.noise.insert("A".into(), 3.0);
    rule.noise.insert("B".into(), 4.0);
    rule.noise.insert("C".into(), 0.0);
}

#[test]
fn records_are_summed_per_board_and_the_canary_variants_count_once() {
    let mut rule = improve();
    rule.noise.insert("canary-pwr".into(), 10.0);
    rule.noise.insert("B".into(), 2.0);
    rule.noise.insert("C".into(), 2.0);
    let m = |b, c| metrics(&[("kicad_unconnected", Some(b), Some(c))]);
    let rows = [
        row("v1", "canary-pwr", "", &m(120.0, 60.0)),
        row("v2", "canary-pwr", "", &m(116.0, 60.0)),
        row("v3", "canary-pwr", "", &m(115.0, 60.0)),
        row("b", "B", "", &m(10.0, 10.0)),
        row("c", "C", "", &m(10.0, 10.0)),
    ];
    let out = run(&rows, rule.clone());
    let report = out.to_text();
    assert!(out.passed(), "{report}");
    // 3 variant rows are one board: 351 -> 180, plus the two others.
    assert!(
        report.contains("canary-pwr [kicad_unconnected]: 351 -> 180"),
        "{report}"
    );
    assert!(report.contains("total 371 -> 200"));
    // Two boards in all, because the variants count once.
    let two = run(&rows[..4], rule);
    assert!(!two.passed());
    assert!(two.to_text().contains("N=2"), "{}", two.to_text());
}

#[test]
fn kicad_unconnected_is_the_signal_and_unrouted_the_reported_fallback() {
    let mut rule = improve();
    noisy(&mut rule);
    let rows = [
        row(
            "a",
            "A",
            "",
            &metrics(&[
                ("kicad_unconnected", Some(40.0), Some(20.0)),
                ("unrouted", Some(99.0), Some(99.0)),
            ]),
        ),
        // No KiCad grade: the router's own count.
        row(
            "b",
            "B",
            "",
            &metrics(&[
                ("kicad_unconnected", None, None),
                ("unrouted", Some(30.0), Some(20.0)),
            ]),
        ),
        row(
            "c",
            "C",
            "",
            &metrics(&[
                ("kicad_unconnected", Some(5.0), Some(5.0)),
                ("unrouted", Some(5.0), Some(5.0)),
            ]),
        ),
    ];
    let out = run(&rows, rule);
    let report = out.to_text();
    assert!(out.passed(), "{report}");
    assert!(
        report.contains("A [kicad_unconnected]: 40 -> 20"),
        "{report}"
    );
    assert!(report.contains("B [unrouted]: 30 -> 20"));
    assert!(report.contains("signal [unrouted] summed separately: 30 -> 20"));
    assert!(report.contains("signal [kicad_unconnected] summed separately: 45 -> 25"));
}

#[test]
fn a_row_without_a_signal_is_ungraded_and_a_board_without_a_band_is_missing() {
    let mut rule = improve();
    noisy(&mut rule);
    let rows = [
        row(
            "a",
            "A",
            "",
            &metrics(&[
                ("kicad_unconnected", Some(9.0), Some(1.0)),
                ("unrouted", None, None),
            ]),
        ),
        row(
            "b",
            "B",
            "",
            &metrics(&[
                ("kicad_unconnected", Some(9.0), Some(1.0)),
                ("unrouted", None, None),
            ]),
        ),
        row(
            "c",
            "C",
            "",
            &metrics(&[("kicad_unconnected", None, None), ("unrouted", None, None)]),
        ),
        row(
            "d",
            "D",
            "",
            &metrics(&[
                ("kicad_unconnected", Some(9.0), Some(1.0)),
                ("unrouted", None, None),
            ]),
        ),
    ];
    let out = run(&rows, rule);
    assert!(!out.passed());
    assert!(
        out.problems
            .iter()
            .any(|p| p.class == Class::Ungraded && p.text.starts_with("c:"))
    );
    assert!(
        out.problems
            .iter()
            .any(|p| p.class == Class::Missing && p.text.starts_with("D: no noise band"))
    );
}

#[test]
fn the_default_band_is_five_percent_of_the_connections_and_at_least_two() {
    for (connections, band) in [
        (0, 2.0),
        (40, 2.0),
        (41, 3.0),
        (100, 5.0),
        (101, 6.0),
        (1000, 50.0),
    ] {
        assert_eq!(TotalsRule::default_band(connections), band, "{connections}");
    }
    // A board of 1000 connections tolerates +100 (twice its band of 50).
    let m = |b, c| metrics(&[("kicad_unconnected", Some(b), Some(c))]);
    let conn = |n: u32| format!("\"connections\": {n}, ");
    let rows = [
        row("a", "A", &conn(1000), &m(500.0, 400.0)),
        row("b", "B", &conn(1000), &m(300.0, 400.0)),
        row("c", "C", &conn(1000), &m(500.0, 300.0)),
    ];
    let out = run(&rows, improve());
    assert!(out.passed(), "{}", out.to_text());
    // 5 % of 1000 = 50, so b at +101 is beyond 2 x 50.
    let rows = [
        row("a", "A", &conn(1000), &m(500.0, 300.0)),
        row("b", "B", &conn(1000), &m(300.0, 401.0)),
        row("c", "C", &conn(1000), &m(500.0, 100.0)),
    ];
    assert!(!run(&rows, improve()).passed());
}

#[test]
fn hard_metrics_and_guards_fail_the_totals_rule() {
    let mut rule = improve();
    noisy(&mut rule);
    let good = |b, c| metrics(&[("kicad_unconnected", Some(b), Some(c))]);
    let mut rows = vec![
        row("a", "A", "", &good(40.0, 10.0)),
        row("b", "B", "", &good(40.0, 10.0)),
        row("c", "C", "", &good(40.0, 10.0)),
    ];
    assert!(run(&rows, rule.clone()).passed());
    // A via in a pad in one candidate row.
    rows[2] = row(
        "c",
        "C",
        "",
        &metrics(&[
            ("kicad_unconnected", Some(40.0), Some(10.0)),
            ("via_in_pad", Some(0.0), Some(1.0)),
        ]),
    );
    let out = run(&rows, rule.clone());
    assert!(!out.passed());
    assert!(out.problems.iter().any(|p| p.class == Class::Hard));
    assert!(
        out.to_text().contains("hard guard: c.via_in_pad = 1"),
        "{}",
        out.to_text()
    );
    // A new DRC error.
    rows[2] = row(
        "c",
        "C",
        "",
        &metrics(&[
            ("kicad_unconnected", Some(40.0), Some(10.0)),
            ("drc_real", Some(2.0), Some(3.0)),
        ]),
    );
    let out = run(&rows, rule);
    assert!(!out.rule_ok);
    assert!(
        out.to_text().contains("hard guard: c.drc_real 2 -> 3"),
        "{}",
        out.to_text()
    );
}

#[test]
fn rejected_rows_are_left_out_and_the_speed_clause_sums_the_work_measure() {
    let mut rule = with_speed(improve());
    noisy(&mut rule);
    let m = |b, c, wb, wc| {
        metrics(&[
            ("kicad_unconnected", Some(b), Some(c)),
            ("maze_nodes", Some(wb), Some(wc)),
        ])
    };
    let rows = [
        row("a", "A", "", &m(30.0, 31.0, 100.0, 70.0)),
        row("b", "B", "", &m(30.0, 30.0, 100.0, 70.0)),
        row("c", "C", "", &m(30.0, 30.0, 100.0, 70.0)),
        row(
            "x",
            "X",
            "\"rejected\": true, ",
            &m(30.0, 90.0, 100.0, 100.0),
        ),
    ];
    let out = run(&rows, rule);
    let report = out.to_text();
    assert!(out.passed(), "{report}");
    assert!(report.contains("rejected: 1 row(s)"));
    assert!(
        report.contains("maze_nodes 300 -> 210 (30.0 % drop, needs 20 %): holds"),
        "{report}"
    );
}

// --- registration ---------------------------------------------------------------

fn sample_text() -> String {
    std::fs::read_to_string(repo().join("tools/ab/examples/n02-totals.prereg.json")).unwrap()
}

#[test]
fn the_sample_totals_registration_validates_and_is_pinned() {
    let prereg = Prereg::parse(&sample_text()).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(prereg.id, "N02-sample-totals");
    assert!(prereg.totals_registered);
    let totals = prereg.totals.as_ref().unwrap();
    assert_eq!(totals.accept, Accept::Improve);
    assert_eq!(totals.signal, ["kicad_unconnected", "unrouted"]);
    assert_eq!(totals.noise["canary-pwr"], 10.0);
    assert_eq!(totals.noise["pic_programmer"], 3.0);
    assert_eq!(totals.speed.as_ref().unwrap().metric, "maze_nodes");
    assert!(prereg.settings().totals.is_some());
    assert_eq!(
        prereg.sha256, "d6ec15d625718714e10332d91ec3e98b7ef860d34d7c1632a5548700036ac75c",
        "the sample registration changed"
    );
}

/// Records for the sample registration: canary-pwr variants a and b are one board.
fn sample_records(prereg: &Prereg, cand_unconnected: f64, interf_cand: f64) -> (Json, Json) {
    let rows: Vec<String> = prereg
        .boards
        .iter()
        .map(|(name, hash)| {
            let (board, base, cand) = match name.as_str() {
                "canary-pwr-a" => ("canary-pwr", 60.0, cand_unconnected),
                "canary-pwr-b" => ("canary-pwr", 60.0, cand_unconnected),
                "interf_u" => ("interf_u", 20.0, interf_cand),
                _ => ("pic_programmer", 8.0, 8.0),
            };
            let m = metrics(&[
                ("below_minimum", Some(0.0), Some(0.0)),
                ("kicad_unconnected", Some(base), Some(cand)),
                ("unrouted", None, None),
                ("maze_nodes", Some(1000.0), Some(900.0)),
                ("wall_seconds", Some(5.0), Some(5.0)),
            ]);
            row(
                name,
                board,
                &format!("\"input_sha256\": \"{hash}\", \"connections\": 400, "),
                &m,
            )
        })
        .collect();
    let arms = Json::parse(&sample_text()).unwrap();
    let mut body = format!(
        "{{\"format\": \"freerouting-gate-records-1\", \"baseline_arm\": \"off\", \"candidate_arm\": \"on\", \"rows\": {{{}}}}}",
        rows.join(", ")
    );
    let arms_text = match arms.get("arms").unwrap() {
        Json::Object(o) => format!(
            "{{{}}}",
            o.iter()
                .map(|(k, v)| format!("{k:?}: {}", json_text(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        _ => unreachable!(),
    };
    body.insert_str(1, &format!("\"arms\": {arms_text}, "));
    let rec = Json::parse(&body).unwrap();
    let base = baseline_from(&rec, &prereg.settings()).unwrap();
    (rec, base)
}

fn json_text(v: &Json) -> String {
    match v {
        Json::Null => "null".into(),
        Json::Bool(b) => b.to_string(),
        Json::Number(n) => n.to_string(),
        Json::String(s) => format!("{s:?}"),
        Json::Array(a) => format!(
            "[{}]",
            a.iter().map(json_text).collect::<Vec<_>>().join(", ")
        ),
        Json::Object(o) => format!(
            "{{{}}}",
            o.iter()
                .map(|(k, v)| format!("{k:?}: {}", json_text(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

#[test]
fn the_sample_registration_judges_a_run_by_the_totals_rule() {
    let prereg = Prereg::parse(&sample_text()).unwrap();
    // canary-pwr 120 -> 70 (band 10), interf_u 20 -> 19, pic 8 -> 8: -51 against noise
    // sqrt(100 + 3^2 + default(400 connections = 20)^2 = 509) = 22.6.
    let (rec, base) = sample_records(&prereg, 35.0, 19.0);
    let out = check_registered(&rec, &base, &prereg);
    assert!(out.passed(), "{}", out.to_text());
    assert!(
        out.to_text()
            .contains("canary-pwr [kicad_unconnected]: 120 -> 70"),
        "{}",
        out.to_text()
    );
    // The speed clause is not needed here (10 % drop); an improvement inside the
    // noise with a 10 % drop fails.
    let (rec, base) = sample_records(&prereg, 55.0, 19.0);
    let out = check_registered(&rec, &base, &prereg);
    assert!(!out.passed());
    assert!(
        out.to_text().contains("does not exceed the set noise"),
        "{}",
        out.to_text()
    );
    // interf_u at +41 against a default band of 20 is beyond twice its band.
    let (rec, base) = sample_records(&prereg, 35.0, 61.0);
    let out = check_registered(&rec, &base, &prereg);
    assert!(!out.passed());
    assert!(
        out.to_text()
            .contains("worse by more than 2 x its noise band: interf_u")
    );
}

fn sample_with(from: &str, to: &str) -> String {
    let text = sample_text();
    assert!(text.contains(from), "{from}");
    text.replace(from, to)
}

fn errors_of(text: &str) -> String {
    format!("{:?}", Prereg::parse(text).unwrap_err())
}

#[test]
fn a_bad_totals_registration_is_refused_or_malformed() {
    // Rule values.
    assert!(
        errors_of(&sample_with("\"rule\": \"totals\"", "\"rule\": \"best\""))
            .contains("`rule` must be")
    );
    // A totals block without the rule.
    assert!(
        errors_of(&sample_with("\"rule\": \"totals\"", "\"rule\": \"paired\""))
            .contains("needs `\\\"rule")
    );
    // accept is required and spelled out.
    assert!(
        errors_of(&sample_with(
            "\"accept\": \"improve\"",
            "\"accept\": \"better\""
        ))
        .contains("`accept` is required")
    );
    assert!(
        errors_of(&sample_with("\"accept\": \"improve\",", "")).contains("`accept` is required")
    );
    // Unknown keys are typos.
    assert!(
        errors_of(&sample_with("\"min_boards\": 3,", "\"min_board\": 3,")).contains("unknown key")
    );
    // The floor of three boards and of a 20 % drop cannot be lowered.
    let e = errors_of(&sample_with("\"min_boards\": 3,", "\"min_boards\": 2,"));
    assert!(
        e.contains("Refused") && e.contains("min_boards is 2"),
        "{e}"
    );
    let e = errors_of(&sample_with(
        "\"min_drop_percent\": 20",
        "\"min_drop_percent\": 10",
    ));
    assert!(e.contains("Refused") && e.contains("at least 20"), "{e}");
    // The speed clause goes with `improve`.
    let no_regress = sample_with("\"accept\": \"improve\"", "\"accept\": \"no-regress\"");
    assert!(errors_of(&no_regress).contains("goes with `accept: improve`"));
    let e = errors_of(&sample_with(
        "\"metric\": \"maze_nodes\"",
        "\"metric\": \"nodes\"",
    ));
    assert!(e.contains("not a registered metric"), "{e}");
    // A negative band; a signal that is not registered.
    assert!(
        errors_of(&sample_with("\"canary-pwr\": 10", "\"canary-pwr\": -1"))
            .contains("expected a number >= 0")
    );
    assert!(
        errors_of(&sample_with(
            "[\"kicad_unconnected\", \"unrouted\"]",
            "[\"kicad\"]"
        ))
        .contains("is not a registered metric")
    );
    // The no-regress variant without a speed clause validates.
    let ok = no_regress.replace(
        ",\n    \"speed\": { \"metric\": \"maze_nodes\", \"min_drop_percent\": 20 }",
        "",
    );
    let p = Prereg::parse(&ok).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(p.totals.unwrap().accept, Accept::NoRegress);
}

#[test]
fn a_noise_band_for_a_board_the_run_does_not_have_is_refused() {
    let typo = sample_with("\"canary-pwr\": 10", "\"canary-pwr-x\": 10");
    let prereg = Prereg::parse(&typo).unwrap();
    let good = Prereg::parse(&sample_text()).unwrap();
    let (rec, base) = sample_records(&good, 35.0, 19.0);
    let out = check_registered(&rec, &base, &prereg);
    assert!(
        out.problems.iter().any(|p| p.class == Class::Refused
            && p.text.contains("totals.noise names board canary-pwr-x")),
        "{}",
        out.to_text()
    );
}

#[test]
fn q14_registration_parses_but_a_run_against_it_is_refused() {
    // Q14's hand-judged registration states `"rule": "totals"` with prose, not the block.
    let text = std::fs::read_to_string(repo().join("docs/roadmap/prereg/Q14.json")).unwrap();
    let prereg = Prereg::parse(&text).unwrap();
    assert!(prereg.totals_registered && prereg.totals.is_none());
    let records = Json::parse("{\"rows\": {}}").unwrap();
    let out = check_registered(&records, &Json::parse("{\"rows\": {}}").unwrap(), &prereg);
    assert!(
        out.problems
            .iter()
            .any(|p| p.class == Class::Refused && p.text.contains("no `totals` block")),
        "{}",
        out.to_text()
    );
}

// --- command line -----------------------------------------------------------------

fn status(code: ExitCode) -> String {
    format!("{code:?}")
}

fn gate(args: &[&str]) -> Result<String, String> {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    gate_cli::run(&args).map(status)
}

#[test]
fn the_command_line_applies_rule_totals_and_refuses_a_run_that_picks_its_criteria() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("gate_totals_cli");
    std::fs::create_dir_all(&dir).unwrap();
    let rec = dir.join("records.json");
    let base = dir.join("baseline.json");
    let m = |b, c| metrics(&[("kicad_unconnected", Some(b), Some(c))]);
    let conn = "\"connections\": 400, ";
    let rows = [
        row("a", "A", conn, &m(40.0, 10.0)),
        row("b", "B", conn, &m(40.0, 10.0)),
        row("c", "C", conn, &m(40.0, 10.0)),
    ];
    std::fs::write(
        &rec,
        format!(
            "{{\"format\": \"freerouting-gate-records-1\", \"rows\": {{{}}}}}",
            rows.join(", ")
        ),
    )
    .unwrap();
    let (rec_s, base_s) = (rec.to_str().unwrap(), base.to_str().unwrap());
    let ok = status(ExitCode::SUCCESS);
    let failed = status(ExitCode::from(1));
    let write = ["--records", rec_s, "--baseline", base_s, "--write-baseline"];
    assert_eq!(gate(&write).unwrap(), ok);
    let with = |more: &[&str]| {
        let mut v: Vec<&str> = vec!["--records", rec_s, "--baseline", base_s];
        v.extend_from_slice(more);
        gate(&v)
    };
    // The paired rule has no score metric in these rows, so nothing improves.
    assert_eq!(with(&[]).unwrap(), failed);
    assert_eq!(with(&["--rule", "totals"]).unwrap(), ok);
    assert_eq!(
        with(&["--rule", "totals", "--accept", "no-regress"]).unwrap(),
        ok
    );
    // Three boards of band 20 (400 connections): -90 beats sqrt(3 * 400) = 34.6; bands of
    // 60 each make the noise 103.9 and the same run fails.
    assert_eq!(
        with(&[
            "--rule", "totals", "--noise", "A=60", "--noise", "B=60", "--noise", "C=60"
        ])
        .unwrap(),
        failed
    );
    // The speed clause on a metric the rows lack does not hold.
    assert_eq!(
        with(&[
            "--rule",
            "totals",
            "--noise",
            "A=60",
            "--noise",
            "B=60",
            "--noise",
            "C=60",
            "--speed",
            "maze_nodes"
        ])
        .unwrap(),
        failed
    );
    // Usage errors.
    assert!(with(&["--rule", "best"]).is_err());
    assert!(with(&["--accept", "improve"]).is_err());
    assert!(with(&["--rule", "totals", "--accept", "better"]).is_err());
    assert!(with(&["--rule", "totals", "--noise", "A"]).is_err());
    assert_eq!(
        with(&["--rule", "totals", "--speed", "maze_nodes=10"]).unwrap(),
        failed
    );
    // With a registration the registration states the rule.
    let prereg = repo().join("tools/ab/examples/n02-totals.prereg.json");
    let prereg_s = prereg.to_str().unwrap();
    assert_eq!(
        with(&["--prereg", prereg_s, "--rule", "paired"]).unwrap(),
        failed
    );
    assert_eq!(
        with(&["--prereg", prereg_s, "--accept", "improve"]).unwrap(),
        failed
    );
}

#[test]
fn judge_is_the_function_check_calls() {
    // The same verdict through `judge` and `check`.
    let mut rule = improve();
    noisy(&mut rule);
    let m = |b, c| metrics(&[("kicad_unconnected", Some(b), Some(c))]);
    let rows = [
        row("a", "A", "", &m(40.0, 10.0)),
        row("b", "B", "", &m(40.0, 10.0)),
        row("c", "C", "", &m(40.0, 10.0)),
    ];
    let rec = records(&rows);
    let mut problems = Vec::new();
    let parsed = freerouting_dev::gate::records_rows(&rec, &mut problems);
    let specs = freerouting_dev::gate::active_specs(&rec, &Settings::default());
    let (ok, _, more) = judge(&parsed, &Settings::default().arms, &specs, &rule);
    assert!(ok && more.is_empty() && problems.is_empty());
}
