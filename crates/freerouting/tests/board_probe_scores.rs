//! Board probe `scores` (B06.3): the router, optimizer and optimizer-ranking
//! scores of every fixture equal Java's to the bit, for every settings
//! variant of `tools/java-reference/probes/board/ScoresProbe.java`, which
//! writes `fixtures/reference/probe/scores/<board>.jsonl` at `a773a70ec`.
//!
//! Per stage (`load`, then `ses` when the row has a session) the first line
//! holds the statistics the scores read, so a score that differs because a
//! statistic does shows the statistic too. Then one line per variant, built
//! here with C02's `RouterSettings`: `None` (Java's `null`), the defaults
//! (V2), both versions `V1_LEGACY`, one V2 variant per score weight at
//! twice its default, one legacy variant per legacy cost at twice its
//! default, and `lengthFloor = 0`, which makes the V2 optimizer score of a
//! board without traces `NaN`. Scores are `float`s widened to `double`; a
//! `NaN` is written as `f64::NAN` on both sides, because the sign of a
//! computed `NaN` depends on the processor.

mod board_probe;

use board_probe::{Line, Row, compare, fixtures, header, repo_root};
use freerouting::scoring::BoardStatistics;
use freerouting::settings::{
    OptimizerScoringVersion, RouterScoringVersion, RouterSettings, default_patch,
};

const ISSUE721_WHY: &str = "Java keeps the single quotes of the net names in the class \
    net lists, so the nets stay in the default class (0.41 mil clearance); the Rust DSN \
    reader accepts ' as a quote and gives the items their class's 0.4 mil \
    (docs/DIVERGENCES.md, freerouting-dsn): same violations, other violation depths, so \
    the router scores differ";
const ISSUE676_WHY: &str = "Java's SesReader fails on all 48 wires with a \
    ClassCastException (RationalVector to IntVector) while normalizing the traces of this \
    1 000 000-per-mm board and imports none; the port imports them, so the trace length \
    and the violations differ (docs/DIVERGENCES.md)";

/// Boards and stages whose scores differ from Java's, for a statistics
/// difference `tests/reference_drc.rs` already lists with the same reason
/// (its `ISSUE721_WHY` and `ISSUE676_WHY`). Issue289's listed race is not
/// here: the probe waits for the post-load processing before it imports
/// the session.
const EXPECTED_MISMATCHES: &[(&str, &str, &str)] = &[
    (
        "Issue721-Autorouter_CE2632_HarryMu_2026-6-15",
        "load",
        ISSUE721_WHY,
    ),
    (
        "Issue721-Autorouter_CE2632_HarryMu_2026-6-15",
        "ses",
        ISSUE721_WHY,
    ),
    ("Issue676-ch32v-tx118s", "ses", ISSUE676_WHY),
];

/// A change of the default settings; `None` stands for Java's `null`.
type Change = Option<fn(&mut RouterSettings)>;

fn legacy(s: &mut RouterSettings) {
    s.router_scoring.version = RouterScoringVersion::V1Legacy;
    s.optimizer_scoring.version = OptimizerScoringVersion::V1Legacy;
}

/// The probe's variants, in its order.
const VARIANTS: &[(&str, Change)] = &[
    ("null", None),
    ("defaults", Some(|_| {})),
    ("v1", Some(legacy)),
    (
        "router.unroutedConnectionWeight",
        Some(|s| s.router_scoring.unrouted_connection_weight *= 2.0),
    ),
    (
        "router.unroutedFreeFraction",
        Some(|s| s.router_scoring.unrouted_free_fraction *= 2.0),
    ),
    (
        "router.unroutedFirstHalfWeight",
        Some(|s| s.router_scoring.unrouted_first_half_weight *= 2.0),
    ),
    (
        "router.unroutedSecondHalfWeight",
        Some(|s| s.router_scoring.unrouted_second_half_weight *= 2.0),
    ),
    (
        "router.clearanceViolationCountWeight",
        Some(|s| s.router_scoring.clearance_violation_count_weight *= 2.0),
    ),
    (
        "router.clearanceViolationDepthWeight",
        Some(|s| s.router_scoring.clearance_violation_depth_weight *= 2.0),
    ),
    (
        "router.clearanceViolationDepthScale",
        Some(|s| s.router_scoring.clearance_violation_depth_scale *= 2.0),
    ),
    (
        "optimizer.excessWireLengthWeight",
        Some(|s| s.optimizer_scoring.excess_wire_length_weight *= 2.0),
    ),
    (
        "optimizer.excessViaWeight",
        Some(|s| s.optimizer_scoring.excess_via_weight *= 2.0),
    ),
    (
        "optimizer.excessBendWeight",
        Some(|s| s.optimizer_scoring.excess_bend_weight *= 2.0),
    ),
    (
        "optimizer.lengthFloor",
        Some(|s| s.optimizer_scoring.length_floor *= 2.0),
    ),
    (
        "optimizer.difficultyScaleFloor",
        Some(|s| s.optimizer_scoring.difficulty_scale_floor *= 2.0),
    ),
    (
        "v1.bendPenalty",
        Some(|s| {
            legacy(s);
            s.scoring.bend_penalty *= 2.0;
        }),
    ),
    (
        "v1.viaCosts",
        Some(|s| {
            legacy(s);
            s.scoring.via_costs *= 2;
        }),
    ),
    (
        "v1.defaultPreferredDirectionTraceCost",
        Some(|s| {
            legacy(s);
            s.scoring.default_preferred_direction_trace_cost *= 2.0;
        }),
    ),
    (
        "optimizer.lengthFloor=0",
        Some(|s| s.optimizer_scoring.length_floor = 0.0),
    ),
];

/// Java's `DefaultSettings`, merged.
fn defaults() -> RouterSettings {
    RouterSettings::from_patch(&default_patch(4), 4)
}

/// A score as the probe writes it: widened, with a canonical `NaN`.
fn canonical(score: f32) -> f64 {
    if score.is_nan() {
        f64::NAN
    } else {
        f64::from(score)
    }
}

fn stage(lines: &mut Vec<String>, name: &str, s: &BoardStatistics) {
    lines.push(
        Line::new()
            .str("stage", name)
            .num("maximum", s.connections.maximum_count as i64)
            .num("incomplete", s.connections.incomplete_count as i64)
            .num("violations", s.clearance_violations.total_count as i64)
            .dbl("violationUm", s.clearance_violations.total_violation_um)
            .dbl("difficulty", f64::from(s.difficulty.difficulty_d))
            .num("bends", s.bends.total_count as i64)
            .num("vias", s.vias.total_count as i64)
            .dbl("lengthMm", f64::from(s.traces.total_length_mm))
            .dbl("minLengthMm", f64::from(s.bounds.min_trace_length_mm))
            .num("minVias", i64::from(s.bounds.min_via_count))
            .num("minBends", i64::from(s.bounds.min_bend_count))
            .end(),
    );
    for (variant, change) in VARIANTS {
        let settings = change.map(|change| {
            let mut settings = defaults();
            change(&mut settings);
            settings
        });
        let settings = settings.as_ref();
        lines.push(
            Line::new()
                .str("variant", variant)
                .dbl("router", canonical(s.router_score(settings)))
                .dbl("optimizer", canonical(s.optimizer_score(settings)))
                .dbl("ranking", canonical(s.optimizer_ranking_score(settings)))
                .end(),
        );
    }
}

fn rust_lines(row: &Row) -> Vec<String> {
    let mut board = board_probe::load(row);
    let mut lines = vec![header("scores", &row.board)];
    stage(&mut lines, "load", &BoardStatistics::new(&board));
    if board_probe::import_session(&mut board, row).is_some() {
        stage(&mut lines, "ses", &BoardStatistics::new(&board));
    }
    lines
}

/// The stages whose lines differ between the committed Java output and
/// `rust`, with the first differing line of each.
fn differing_stages(row: &Row, rust: &[String]) -> Vec<(String, String)> {
    let path = repo_root().join(format!(
        "fixtures/reference/probe/scores/{}.jsonl",
        row.board
    ));
    let java = std::fs::read_to_string(&path).unwrap_or_default();
    let java: Vec<&str> = java.lines().collect();
    let mut stages: Vec<(String, String)> = Vec::new();
    let mut current = String::new();
    for i in 1..java.len().max(rust.len()) {
        let (j, r) = (java.get(i).copied(), rust.get(i).map(String::as_str));
        if let Some(name) = r.and_then(|r| r.strip_prefix("{\"stage\":\"")) {
            current = name.split('"').next().unwrap_or_default().to_string();
        }
        if j != r && stages.last().is_none_or(|(s, _)| *s != current) {
            let detail = format!(
                "  Java: {}\n  Rust: {}",
                j.unwrap_or("<end>"),
                r.unwrap_or("<end>")
            );
            stages.push((current.clone(), detail));
        }
    }
    stages
}

#[test]
fn scores_match_java() {
    let mut failures = Vec::new();
    let mut seen = Vec::new();
    for row in fixtures() {
        let lines = rust_lines(&row);
        let Err(message) = compare("scores", &row, &lines) else {
            continue;
        };
        let stages = differing_stages(&row, &lines);
        if stages.is_empty() || message.contains("no Java output") {
            failures.push(message);
            continue;
        }
        for (stage, detail) in stages {
            match EXPECTED_MISMATCHES
                .iter()
                .find(|(board, s, _)| *board == row.board && *s == stage)
            {
                Some(entry) => seen.push(*entry),
                None => failures.push(format!("scores/{} {stage}:\n{detail}", row.board)),
            }
        }
    }
    for entry in EXPECTED_MISMATCHES {
        if !seen.contains(entry) {
            failures.push(format!(
                "listed mismatch {} {} no longer occurs; remove it from EXPECTED_MISMATCHES",
                entry.0, entry.1
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differences:\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}
