//! The result manifest of `--result-json PATH` and `--router.result_json=`
//! (C08.4).
//!
//! The field names are Java's `RoutingResultManifest` (`@SerializedName`),
//! schema version 1, written through the same Gson-style writer as the DRC
//! report (two-space indent, doubles with two decimals). Java omits the
//! fields that are `null`; this writer keeps every key and writes `null`, so
//! a reader can rely on the keys. Fields that need JVM figures or a hash
//! that Rust does not compute are always `null`: `cpu_seconds`,
//! `total_allocated_gb`, `peak_heap_mb`, `resource_usage`, `cpu_score`,
//! `fixture.sha256` and the `current_*` scores of a phase snapshot.
//!
//! The manifest is written when the run ends, **also when it failed**
//! (`output_written` is then `false` and `final_state` is `TERMINATED`, or
//! `CANCELLED` for an aborted run).
//!
//! **Phases.** Java's pipeline records a board snapshot at the start and end
//! of each stage. The pipeline here reports the job as a whole, so only
//! these snapshots exist: `phases.autorouter.before` is the board as loaded
//! and `phases.autorouter.after` the final board (when the optimizer did not
//! run; otherwise `phases.optimizer.after` is the final board). The other
//! snapshots are `null`. `duration_seconds` of the autorouter falls back to
//! the whole job, as in Java. The statistics come from
//! [`BoardStatistics`], in the key order of `fixtures/reference/load`.
//!
//! **Verdicts (Q06).** With `--router.special.diagnostics=verdicts` the
//! manifest gets one more key after Java's, `verdicts`: the run's
//! `freerouting-verdicts/1` report (`docs/development/verdicts.md`). Without
//! it the key is absent and the manifest is Java's.
//!
//! **Rip statistics (Q05.1).** With `rip-stats` among the diagnostics the
//! manifest gets `rip_stats` after `verdicts`: the run's
//! `freerouting-rip-stats/1` report
//! (`freerouting::quality::congestion::rip_stats`). Without it the key is
//! absent.
//!
//! **Widths (Q02.1).** With `--router.special.diagnostics=widths` (or
//! `--router.neck_down.strict=true`) the manifest gets `widths` after
//! `rip_stats`: the `freerouting-widths/1` report of the wide nets' delivered
//! widths and the narrowing ledger (`quality::neckdown`).
//!
//! **Output check (Q20.3).** With `--router.output_check=report` or `repair`
//! (profile `enhanced`'s value) the manifest gets `output_check` after
//! `widths`: `{mode, checked, found, removed, retried, repaired, left_open,
//! remaining, by_rule, input}`
//! (`freerouting::quality::output_check::OutputCheck::to_json`). With `off`
//! the key is absent.

use std::path::Path;

use freerouting::drc::json::{JsonValue, iso_offset_date_time_now};
use freerouting::job::outputs::{OutputError, OutputFormat, write_atomic};
use freerouting::job::run::JobReport;
use freerouting::scoring::BoardStatistics;
use freerouting::settings::dump::dump_patch;

use crate::version::{GIT_SHA, VERSION};

/// `RoutingResultManifest.SCHEMA_VERSION`.
pub const SCHEMA_VERSION: u32 = 1;

/// Every top-level key of the manifest, in Java's order.
pub const TOP_LEVEL_KEYS: &[&str] = &[
    "schema_version",
    "generated_at",
    "app_version",
    "git_sha",
    "fixture",
    "settings_snapshot",
    "phases",
    "board_statistics",
    "bounds",
    "normalized_score",
    "optimizer_score",
    "resource_usage",
    "final_state",
    "exit_code",
    "output_written",
    "cpu_score",
];

/// The keys of each of `phases.fanout`, `phases.autorouter` and
/// `phases.optimizer`.
pub const PHASE_KEYS: &[&str] = &[
    "before",
    "after",
    "duration_seconds",
    "cpu_seconds",
    "passes_completed",
    "total_allocated_gb",
    "peak_heap_mb",
];

/// What the manifest says about the run's end.
pub struct ManifestInput<'a> {
    /// The design file as given (`fixture.filename` is its file name).
    pub design: Option<&'a str>,
    /// Java's `RoutingJobState` name: `COMPLETED`, `TIMED_OUT`,
    /// `CANCELLED` or `TERMINATED`.
    pub final_state: &'a str,
    /// The process exit code.
    pub exit_code: u8,
    /// True if an output of more than 0 bytes was written.
    pub output_written: bool,
    /// What the job knows.
    pub report: &'a JobReport,
}

/// An explicit `null`. The JSON writer leaves `Null` members out, as Gson
/// does; the manifest keeps every key, so the text is written verbatim.
fn null() -> JsonValue {
    JsonValue::Number("null".to_string())
}

fn object(members: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        members
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
    )
}

fn double(value: f64) -> JsonValue {
    match JsonValue::two_decimals(value) {
        JsonValue::Null => null(),
        number => number,
    }
}

fn float(value: f32) -> JsonValue {
    double(f64::from(value))
}

fn count(value: usize) -> JsonValue {
    JsonValue::int(i64::try_from(value).unwrap_or(i64::MAX))
}

fn optional<T>(value: Option<T>, f: impl FnOnce(T) -> JsonValue) -> JsonValue {
    value.map_or(null(), f)
}

fn rect(r: &freerouting::scoring::Rect) -> JsonValue {
    object(vec![
        ("x", float(r.x)),
        ("y", float(r.y)),
        ("width", float(r.width)),
        ("height", float(r.height)),
    ])
}

fn bounds(stats: &BoardStatistics) -> JsonValue {
    object(vec![
        (
            "min_trace_length_mm",
            float(stats.bounds.min_trace_length_mm),
        ),
        (
            "min_via_count",
            JsonValue::int(i64::from(stats.bounds.min_via_count)),
        ),
        (
            "min_bend_count",
            JsonValue::int(i64::from(stats.bounds.min_bend_count)),
        ),
    ])
}

/// `BoardStatistics` as Gson writes it (the key names and order of Java's
/// `@SerializedName`s; `fixtures/reference/load/*.json` has them).
pub fn statistics_json(s: &BoardStatistics) -> JsonValue {
    object(vec![
        ("host", JsonValue::string(s.host.clone())),
        ("unit", JsonValue::string(s.unit.clone())),
        (
            "board",
            object(vec![
                ("bounding_box", rect(&s.board.bounding_box)),
                ("size", rect(&s.board.size)),
                ("area_cm2", float(s.board.area_cm2)),
            ]),
        ),
        (
            "layers",
            object(vec![
                ("total_count", count(s.layers.total_count)),
                ("signal_count", count(s.layers.signal_count)),
            ]),
        ),
        (
            "items",
            object(vec![
                ("total_count", count(s.items.total_count)),
                ("trace_count", count(s.items.trace_count)),
                ("via_count", count(s.items.via_count)),
                (
                    "conduction_area_count",
                    count(s.items.conduction_area_count),
                ),
                ("drill_item_count", count(s.items.drill_item_count)),
                ("pin_count", count(s.items.pin_count)),
                ("component_count", count(s.items.component_outline_count)),
                ("other_count", count(s.items.other_count)),
            ]),
        ),
        (
            "components",
            object(vec![("total_count", count(s.components.total_count))]),
        ),
        (
            "pads",
            object(vec![("total_count", count(s.pads.total_count))]),
        ),
        (
            "nets",
            object(vec![
                ("total_count", JsonValue::int(i64::from(s.nets.total_count))),
                ("class_count", count(s.nets.class_count)),
            ]),
        ),
        (
            "connections",
            object(vec![
                ("maximum_count", count(s.connections.maximum_count)),
                ("incomplete_count", count(s.connections.incomplete_count)),
            ]),
        ),
        (
            "traces",
            object(vec![
                ("total_count", count(s.traces.total_count)),
                ("total_segment_count", count(s.traces.total_segment_count)),
                ("total_length", float(s.traces.total_length)),
                ("total_length_mm", float(s.traces.total_length_mm)),
                (
                    "total_weighted_length",
                    float(s.traces.total_weighted_length),
                ),
                ("average_length", float(s.traces.average_length)),
                (
                    "total_vertical_length",
                    float(s.traces.total_vertical_length),
                ),
                (
                    "total_horizontal_length",
                    float(s.traces.total_horizontal_length),
                ),
                ("total_angled_length", float(s.traces.total_angled_length)),
            ]),
        ),
        (
            "bends",
            object(vec![
                ("total_count", count(s.bends.total_count)),
                ("90_degree_count", count(s.bends.ninety_degree_count)),
                ("45_degree_count", count(s.bends.forty_five_degree_count)),
                ("other_angle_count", count(s.bends.other_angle_count)),
            ]),
        ),
        (
            "vias",
            object(vec![
                ("total_count", count(s.vias.total_count)),
                ("through_hole_count", count(s.vias.through_hole_count)),
                ("blind_count", count(s.vias.blind_count)),
                ("buried_count", count(s.vias.buried_count)),
            ]),
        ),
        (
            "clearance_violations",
            object(vec![
                ("total_count", count(s.clearance_violations.total_count)),
                (
                    "pre_existing_count",
                    count(s.clearance_violations.pre_existing_count),
                ),
                (
                    "unfixable_count",
                    count(s.clearance_violations.unfixable_count),
                ),
                (
                    "router_introduced_count",
                    count(s.clearance_violations.router_introduced_count),
                ),
                (
                    "total_violation_um",
                    double(s.clearance_violations.total_violation_um),
                ),
                (
                    "min_violation_um",
                    double(s.clearance_violations.min_violation_um),
                ),
                (
                    "max_violation_um",
                    double(s.clearance_violations.max_violation_um),
                ),
                (
                    "avg_violation_um",
                    double(s.clearance_violations.avg_violation_um),
                ),
            ]),
        ),
        (
            "difficulty",
            object(vec![
                ("pin_count", count(s.difficulty.pin_count)),
                ("signal_layer_count", count(s.difficulty.signal_layer_count)),
                ("complexity_c", count(s.difficulty.complexity_c)),
                ("difficulty_d", float(s.difficulty.difficulty_d)),
                ("board_area_cm2", float(s.difficulty.board_area_cm2)),
            ]),
        ),
        ("bounds", bounds(s)),
        (
            "fanout",
            object(vec![
                ("total_smd_pins", count(s.fanout.total_smd_pins)),
                ("pins_to_escape", count(s.fanout.pins_to_escape)),
                ("escaped_count", count(s.fanout.escaped_count)),
            ]),
        ),
    ])
}

/// One `PhaseSnapshot`: the statistics and the scores of a board.
fn snapshot(stats: &BoardStatistics, report: &JobReport) -> JsonValue {
    let settings = report.router_settings.as_ref();
    let router_score = settings.map(|s| stats.router_score(Some(s)));
    let optimizer_score = settings.map(|s| stats.optimizer_score(Some(s)));
    object(vec![
        ("board_statistics", statistics_json(stats)),
        ("score", optional(router_score, float)),
        ("router_score", optional(router_score, float)),
        ("optimizer_score", optional(optimizer_score, float)),
        ("score_source", JsonValue::string("current")),
        ("current_router_score", null()),
        ("current_optimizer_score", null()),
        ("current_score_source", null()),
    ])
}

fn phase(
    before: JsonValue,
    after: JsonValue,
    duration: Option<f64>,
    passes: Option<usize>,
) -> JsonValue {
    object(vec![
        ("before", before),
        ("after", after),
        ("duration_seconds", optional(duration, double)),
        ("cpu_seconds", null()),
        ("passes_completed", optional(passes, count)),
        ("total_allocated_gb", null()),
        ("peak_heap_mb", null()),
    ])
}

fn phases(report: &JobReport) -> JsonValue {
    let loaded = report.loaded.as_ref().map(|s| snapshot(s, report));
    let finished = report.finished.as_ref().map(|s| snapshot(s, report));
    let optimizer_ran = report.optimizer_passes.is_some();
    let (autorouter_after, optimizer_after) = if optimizer_ran {
        (None, finished)
    } else {
        (finished, None)
    };
    let nonzero = |n: usize| (n > 0).then_some(n);
    object(vec![
        (
            "fanout",
            phase(
                null(),
                null(),
                report.fanout_seconds,
                report.fanout_passes.map(|p| p as usize),
            ),
        ),
        (
            "autorouter",
            phase(
                loaded.unwrap_or(null()),
                autorouter_after.unwrap_or(null()),
                Some(report.router_seconds.unwrap_or(report.total_seconds)),
                nonzero(report.passes_completed),
            ),
        ),
        (
            "optimizer",
            phase(
                null(),
                optimizer_after.unwrap_or(null()),
                None,
                report.optimizer_passes.map(|p| p as usize),
            ),
        ),
    ])
}

/// The manifest as JSON.
pub fn build(input: &ManifestInput<'_>) -> JsonValue {
    let report = input.report;
    let file_name = input
        .design
        .and_then(|d| Path::new(d).file_name())
        .map(|n| n.to_string_lossy().into_owned());
    let router_score = report
        .finished
        .as_ref()
        .zip(report.router_settings.as_ref())
        .map(|(stats, settings)| stats.router_score(Some(settings)));
    let optimizer_score = report
        .finished
        .as_ref()
        .zip(report.router_settings.as_ref())
        .filter(|_| report.optimizer_passes.is_some())
        .map(|(stats, settings)| stats.optimizer_score(Some(settings)));
    let manifest = object(vec![
        ("schema_version", JsonValue::int(i64::from(SCHEMA_VERSION))),
        (
            "generated_at",
            JsonValue::string(iso_offset_date_time_now()),
        ),
        ("app_version", JsonValue::string(VERSION)),
        ("git_sha", JsonValue::string(GIT_SHA)),
        (
            "fixture",
            object(vec![
                ("filename", optional(file_name, JsonValue::string)),
                ("sha256", null()),
            ]),
        ),
        (
            "settings_snapshot",
            // A core count no machine has keeps `max_threads` a number.
            optional(report.settings.as_ref(), |p| dump_patch(p, 1 << 30)),
        ),
        ("phases", phases(report)),
        (
            "board_statistics",
            optional(report.finished.as_ref(), statistics_json),
        ),
        ("bounds", optional(report.finished.as_ref(), bounds)),
        ("normalized_score", optional(router_score, float)),
        ("optimizer_score", optional(optimizer_score, float)),
        ("resource_usage", null()),
        ("final_state", JsonValue::string(input.final_state)),
        ("exit_code", JsonValue::int(i64::from(input.exit_code))),
        ("output_written", JsonValue::Bool(input.output_written)),
        ("cpu_score", null()),
    ]);
    // Q06 and Q05.1: the diagnostics' reports, after Java's keys, only when
    // the diagnostic ran.
    let summary = report.summary.as_ref();
    let extra: Vec<(String, JsonValue)> = [
        (
            "verdicts",
            summary.and_then(|s| s.verdicts.as_ref().map(|v| v.to_json())),
        ),
        (
            "rip_stats",
            summary.and_then(|s| s.rip_stats.as_ref().map(|r| r.to_json())),
        ),
        // Q02.1: the delivered widths and the narrowing ledger.
        (
            "widths",
            summary.and_then(|s| s.widths.as_ref().map(|w| w.to_json())),
        ),
        // Q20.3: what the output check did, when `router.output_check` is on.
        (
            "output_check",
            summary.and_then(|s| s.output_check.as_ref().map(|c| c.to_json())),
        ),
    ]
    .into_iter()
    .filter_map(|(key, value)| value.map(|v| (key.to_string(), v)))
    .collect();
    match manifest {
        JsonValue::Object(mut members) => {
            members.extend(extra);
            JsonValue::Object(members)
        }
        manifest => manifest,
    }
}

/// Writes the manifest to `path` atomically, creating its folder.
pub fn write(path: &Path, input: &ManifestInput<'_>) -> Result<u64, OutputError> {
    let text = build(input).to_pretty_string();
    write_atomic(path, OutputFormat::DrcReport, true, None, |w| {
        std::io::Write::write_all(w, text.as_bytes())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failed_input(report: &JobReport) -> ManifestInput<'_> {
        ManifestInput {
            design: Some("some/dir/board.dsn"),
            final_state: "TERMINATED",
            exit_code: 1,
            output_written: false,
            report,
        }
    }

    #[test]
    fn a_failed_run_has_every_key() {
        let report = JobReport::default();
        let JsonValue::Object(members) = build(&failed_input(&report)) else {
            panic!("an object");
        };
        let keys: Vec<&str> = members.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, TOP_LEVEL_KEYS);
        let get = |k: &str| &members.iter().find(|(n, _)| n == k).unwrap().1;
        assert_eq!(get("final_state"), &JsonValue::string("TERMINATED"));
        assert_eq!(get("output_written"), &JsonValue::Bool(false));
        assert_eq!(get("board_statistics"), &null());
        let JsonValue::Object(phases) = get("phases") else {
            panic!("phases");
        };
        assert_eq!(phases.len(), 3);
        for (_, phase) in phases {
            let JsonValue::Object(keys) = phase else {
                panic!("phase");
            };
            let names: Vec<&str> = keys.iter().map(|(k, _)| k.as_str()).collect();
            assert_eq!(names, PHASE_KEYS);
        }
    }

    #[test]
    fn the_fixture_is_a_file_name() {
        let report = JobReport::default();
        let JsonValue::Object(members) = build(&failed_input(&report)) else {
            panic!("an object");
        };
        let (_, fixture) = members.iter().find(|(k, _)| k == "fixture").unwrap();
        let JsonValue::Object(fixture) = fixture else {
            panic!("fixture");
        };
        assert_eq!(
            fixture[0],
            ("filename".to_string(), JsonValue::string("board.dsn"))
        );
        assert_eq!(fixture[1], ("sha256".to_string(), null()));
    }
}
