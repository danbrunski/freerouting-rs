//! The report of a bench run: `bench.json` with every run, and `bench.md`
//! with one row per board and engine (median wall time and its spread).

use freerouting::drc::json::JsonValue;

use crate::bench::{BenchResult, EngineResult, RunRecord};
use crate::ses_diff::SessionStatistics;

/// The median of `values` (the mean of the two middle ones for an even
/// count), `None` if empty.
pub fn median(values: &[f64]) -> Option<f64> {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let n = sorted.len();
    match n {
        0 => None,
        _ if n % 2 == 1 => Some(sorted[n / 2]),
        _ => Some((sorted[n / 2 - 1] + sorted[n / 2]) / 2.0),
    }
}

/// Max minus min of `values`, `None` if empty.
pub fn spread(values: &[f64]) -> Option<f64> {
    let max = values.iter().copied().reduce(f64::max)?;
    let min = values.iter().copied().reduce(f64::min)?;
    Some(max - min)
}

fn optional(value: Option<f64>, digits: usize) -> JsonValue {
    match value {
        Some(v) => JsonValue::Number(format!("{v:.digits$}")),
        None => JsonValue::Null,
    }
}

fn run_json(run: &RunRecord) -> JsonValue {
    let mut members = vec![
        ("run".to_string(), JsonValue::int(run.run as i64)),
        (
            "wall_seconds".to_string(),
            optional(Some(run.wall_seconds), 3),
        ),
        ("load_before".to_string(), optional(run.load_before, 2)),
        ("load_after".to_string(), optional(run.load_after, 2)),
        (
            "exit_status".to_string(),
            run.exit_status
                .map_or(JsonValue::Null, |s| JsonValue::int(i64::from(s))),
        ),
        ("timed_out".to_string(), JsonValue::Bool(run.timed_out)),
        (
            "work_limit_cutoffs".to_string(),
            JsonValue::int(run.work_limit_cutoffs as i64),
        ),
        (
            "statistics".to_string(),
            run.statistics
                .as_ref()
                .map_or(JsonValue::Null, SessionStatistics::to_json),
        ),
    ];
    if let Some(note) = &run.note {
        members.push(("note".to_string(), JsonValue::string(note.clone())));
    }
    JsonValue::Object(members)
}

fn walls(engine: &EngineResult) -> Vec<f64> {
    engine.runs.iter().map(|r| r.wall_seconds).collect()
}

fn engine_json(engine: &EngineResult) -> JsonValue {
    let w = walls(engine);
    let mut members = vec![
        ("engine".to_string(), JsonValue::string(engine.engine)),
        ("median_wall_seconds".to_string(), optional(median(&w), 3)),
        ("spread_seconds".to_string(), optional(spread(&w), 3)),
        (
            "runs".to_string(),
            JsonValue::Array(engine.runs.iter().map(run_json).collect()),
        ),
    ];
    if let Some(reason) = &engine.skipped {
        members.push(("skipped".to_string(), JsonValue::string(reason.clone())));
    }
    JsonValue::Object(members)
}

/// `bench.json`.
pub fn to_json(result: &BenchResult) -> JsonValue {
    let o = &result.options;
    let boards = result
        .boards
        .iter()
        .map(|b| {
            JsonValue::Object(vec![
                (
                    "board".to_string(),
                    JsonValue::string(b.entry.written.clone()),
                ),
                (
                    "path".to_string(),
                    JsonValue::string(b.entry.path.display().to_string()),
                ),
                (
                    "max_passes".to_string(),
                    JsonValue::int(i64::from(b.bounds.max_passes)),
                ),
                (
                    "max_items".to_string(),
                    b.bounds
                        .max_items
                        .map_or(JsonValue::Null, |v| JsonValue::int(i64::from(v))),
                ),
                (
                    "engines".to_string(),
                    JsonValue::Array(b.engines.iter().map(engine_json).collect()),
                ),
            ])
        })
        .collect();
    let engines = o
        .engines
        .iter()
        .map(|e| {
            JsonValue::Object(vec![
                ("engine".to_string(), JsonValue::string(e.name)),
                (
                    "command".to_string(),
                    JsonValue::string(e.command.join(" ")),
                ),
            ])
        })
        .collect();
    JsonValue::Object(vec![
        (
            "set".to_string(),
            JsonValue::string(o.set_file.display().to_string()),
        ),
        ("engines".to_string(), JsonValue::Array(engines)),
        (
            "java_version".to_string(),
            o.java_version
                .clone()
                .map_or(JsonValue::Null, JsonValue::String),
        ),
        ("runs_per_board".to_string(), JsonValue::int(o.runs as i64)),
        ("optimizer".to_string(), JsonValue::Bool(o.optimize)),
        ("max_load".to_string(), optional(o.max_load, 2)),
        (
            "bounds_file".to_string(),
            o.bounds_file.as_ref().map_or(JsonValue::Null, |p| {
                JsonValue::string(p.display().to_string())
            }),
        ),
        (
            "default_max_passes".to_string(),
            JsonValue::int(i64::from(o.default_bounds.max_passes)),
        ),
        (
            "aborted".to_string(),
            result
                .aborted
                .clone()
                .map_or(JsonValue::Null, JsonValue::String),
        ),
        (
            "missing".to_string(),
            JsonValue::Array(
                result
                    .missing
                    .iter()
                    .map(|e| JsonValue::string(e.written.clone()))
                    .collect(),
            ),
        ),
        ("boards".to_string(), JsonValue::Array(boards)),
    ])
}

/// One statistic over the runs: the value if all runs agree, else
/// `min–max`.
fn statistic_cell(engine: &EngineResult, field: impl Fn(&SessionStatistics) -> f64) -> String {
    let values: Vec<f64> = engine
        .runs
        .iter()
        .filter_map(|r| r.statistics.as_ref().map(&field))
        .collect();
    let format = |v: f64| {
        if v.fract() == 0.0 {
            format!("{}", v as i64)
        } else {
            format!("{v:.2}")
        }
    };
    match (
        values.iter().copied().reduce(f64::min),
        values.iter().copied().reduce(f64::max),
    ) {
        (Some(min), Some(max)) if min == max => format(min),
        (Some(min), Some(max)) => format!("{}–{}", format(min), format(max)),
        _ => "–".to_string(),
    }
}

fn exits_cell(engine: &EngineResult) -> String {
    let mut exits: Vec<String> = engine
        .runs
        .iter()
        .map(|r| match (r.timed_out, r.exit_status) {
            (true, _) => "timeout".to_string(),
            (false, Some(code)) => code.to_string(),
            (false, None) => "signal".to_string(),
        })
        .collect();
    exits.dedup();
    exits.join(",")
}

/// `bench.md`: one row per board and engine.
pub fn to_markdown(result: &BenchResult) -> String {
    let o = &result.options;
    let mut out = String::new();
    out.push_str(&format!(
        "# Bench: {}\n\n{} run(s) per board and engine, optimizer {}, load limit {}. {}\n\n",
        o.set_file.display(),
        o.runs,
        if o.optimize { "on" } else { "off" },
        o.max_load.map_or("off".to_string(), |l| format!("{l:.2}")),
        o.java_version.as_deref().unwrap_or("")
    ));
    out.push_str(
        "| board | engine | passes | runs | median wall s | spread s | max load before | \
         incomplete | violations | vias | length mm | bends | cutoffs | exit |\n",
    );
    out.push_str("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|\n");
    for board in &result.boards {
        for engine in &board.engines {
            let w = walls(engine);
            let cell = |v: Option<f64>| v.map_or("–".to_string(), |v| format!("{v:.2}"));
            let max_load = engine
                .runs
                .iter()
                .filter_map(|r| r.load_before)
                .reduce(f64::max);
            let cutoffs: Vec<f64> = engine
                .runs
                .iter()
                .map(|r| r.work_limit_cutoffs as f64)
                .collect();
            let exit = match &engine.skipped {
                Some(reason) if engine.runs.is_empty() => format!("skipped: {reason}"),
                _ => exits_cell(engine),
            };
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
                board.entry.written,
                engine.engine,
                board.bounds.max_passes,
                engine.runs.len(),
                cell(median(&w)),
                cell(spread(&w)),
                cell(max_load),
                statistic_cell(engine, |s| s.incomplete_connections as f64),
                statistic_cell(engine, |s| s.clearance_violations as f64),
                statistic_cell(engine, |s| s.vias as f64),
                statistic_cell(engine, |s| f64::from(s.trace_length_mm)),
                statistic_cell(engine, |s| s.bends as f64),
                cutoffs
                    .iter()
                    .copied()
                    .reduce(f64::max)
                    .map_or("–".to_string(), |c| format!("{}", c as i64)),
                exit
            ));
        }
    }
    if !result.missing.is_empty() {
        out.push_str("\nSkipped (file missing):\n");
        for entry in &result.missing {
            out.push_str(&format!("- {}\n", entry.written));
        }
    }
    if let Some(reason) = &result.aborted {
        out.push_str(&format!("\nStopped early: {reason}\n"));
    }
    out
}
