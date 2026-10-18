//! C02.1: the settings field table, `SettingsPatch` and `RouterSettings`.
//! Java's defaults are the `legacy` profile's (N01.2): the merged defaults
//! are checked pinned to `legacy`.
//!
//! Ports `BendCostSettingsTest.nullScoringSafety` (the settings half; the
//! board half is C03's) and `ScoringVersionSettingsTest` defaults.

// Patches are built field by field, as the Java tests build `RouterSettings`.
#![allow(clippy::field_reassign_with_default)]

use std::collections::BTreeSet;
use std::fmt::Debug;

use freerouting::scoring::{OptimizerScoreSettings, RouterScoreSettings};
use freerouting::settings::profile::{Difference, legacy_pin, with_enhanced_differences};
use freerouting::settings::{
    FIELDS, FieldKind, FieldValue, OptimizerScoringVersion, ResolveContext, RouterScoringVersion,
    RouterSettings, SettingsPatch, Source, default_patch, resolve,
};

const CORES: usize = 8;

fn ctx() -> ResolveContext {
    ResolveContext { cores: CORES }
}

fn value(patch: &SettingsPatch, path: &str) -> Option<FieldValue> {
    let field = FIELDS
        .iter()
        .find(|f| f.path == path)
        .unwrap_or_else(|| panic!("{path} is not in the table"));
    (field.get)(patch)
}

fn b(v: bool) -> Option<FieldValue> {
    Some(FieldValue::Bool(v))
}
fn i(v: i32) -> Option<FieldValue> {
    Some(FieldValue::Int(v))
}
fn f32v(v: f32) -> Option<FieldValue> {
    Some(FieldValue::Float(v))
}
fn f64v(v: f64) -> Option<FieldValue> {
    Some(FieldValue::Double(v))
}
fn s(v: &str) -> Option<FieldValue> {
    Some(FieldValue::Str(v.to_string()))
}
fn e(v: &'static str) -> Option<FieldValue> {
    Some(FieldValue::Enum(v))
}
fn empty_list() -> Option<FieldValue> {
    Some(FieldValue::StrList(Vec::new()))
}

/// Every default of C02.1, written out independently of the table, for a
/// machine with 8 cores.
#[test]
fn defaults_with_eight_cores() {
    check_java_defaults(&default_patch(CORES), "DefaultSettings");
}

/// N01.2: Java's defaults are the `legacy` profile's. The merge pinned to
/// `legacy` gives every default of [`check_java_defaults`], also when
/// `enhanced` has a (test-only) difference; unpinned, the difference shows.
#[test]
fn java_defaults_are_the_legacy_profile() {
    const DUMMY: &[Difference] = &[Difference {
        path: "fanout.max_passes",
        value: "7",
        measurement: "test-only dummy (N01.2)",
    }];
    let pinned = || {
        let mut patch = resolve(
            &[
                (Source::Defaults, default_patch(CORES)),
                (Source::Testing, legacy_pin()),
            ],
            &ctx(),
        )
        .patch;
        assert_eq!(patch.profile.as_deref(), Some("legacy"));
        patch.profile = None;
        patch
    };
    check_java_defaults(&pinned(), "pinned to legacy");
    with_enhanced_differences(DUMMY, || {
        check_java_defaults(
            &pinned(),
            "pinned to legacy beside a dummy enhanced difference",
        );
        let unpinned = resolve(&[(Source::Defaults, default_patch(CORES))], &ctx()).patch;
        assert_eq!(
            value(&unpinned, "fanout.max_passes"),
            i(7),
            "the dummy is live"
        );
    });
}

/// Every default of C02.1, written out independently of the table, for a
/// machine with 8 cores, checked on `patch`.
fn check_java_defaults(patch: &SettingsPatch, what: &str) {
    let threads = i(7);
    let expected: Vec<(&str, Option<FieldValue>)> = vec![
        // N01: unset, which is `enhanced`; Java has no profile.
        ("profile", None),
        // Top level.
        ("job_timeout", s("12:00:00")),
        ("trace_pull_tight_accuracy", i(500)),
        ("allowed_via_types", b(true)),
        ("automatic_neckdown", b(true)),
        ("max_threads", threads.clone()),
        ("copper_to_edge_clearance_um", f64v(250.0)),
        ("hole_clearance_um", f64v(0.0)),
        ("clearance_tolerance_um", f64v(1.0)),
        ("plane_nets", empty_list()),
        ("plane_as_obstacle", b(false)),
        ("power_layer_traces", None),
        ("plane_obstacle_net_classes", None),
        ("plane_obstacle_block", None),
        ("wire_keepout", None),
        ("via_clearance", None),
        ("neck_width_um", f64v(0.0)),
        ("min_trace_width_um", None),
        ("strict_drc", b(false)),
        ("result_json", None),
        ("room_order", None),
        ("work_unit", None),
        // autorouter.
        ("autorouter.enabled", b(true)),
        ("autorouter.algorithm", s("freerouting-router")),
        ("autorouter.max_passes", i(0)),
        ("autorouter.max_items", i(i32::MAX)),
        ("autorouter.max_threads", threads.clone()),
        ("autorouter.save_intermediate_stages", b(false)),
        ("autorouter.ignore_net_classes", empty_list()),
        // fanout.
        ("fanout.enabled", b(true)),
        ("fanout.max_passes", i(20)),
        ("fanout.max_items", i(i32::MAX)),
        (
            "fanout.max_milliseconds_per_pin",
            Some(FieldValue::Long(10000)),
        ),
        ("fanout.ripup_allowed", b(true)),
        ("fanout.min_escape_length_mm", f64v(2.5)),
        ("fanout.max_escape_length_mm", f64v(4.5)),
        ("fanout.start_via_diameter_mm", f64v(0.25)),
        ("fanout.end_via_diameter_mm", f64v(0.25)),
        ("fanout.pin_sorting_order", s("outer_first")),
        ("fanout.fallback_to_board_vias", b(true)),
        ("fanout.timeout", None),
        ("fanout.algorithm", None),
        // optimizer.
        ("optimizer.enabled", b(true)),
        ("optimizer.algorithm", s("freerouting-optimizer")),
        ("optimizer.max_passes", i(100)),
        ("optimizer.max_items", i(i32::MAX)),
        ("optimizer.max_threads", threads),
        ("optimizer.improvement_threshold", f32v(2.5)),
        ("optimizer.board_update_strategy", e("GLOBAL_OPTIMAL")),
        ("optimizer.item_selection_strategy", e("SEQUENTIAL")),
        ("optimizer.additional_ripup_cost_factor_at_start", i(10)),
        ("optimizer.trace_ripup_cost_factor", f32v(0.6)),
        ("optimizer.max_autoroute_passes", i(6)),
        ("optimizer.enable_preflight_guards", b(true)),
        ("optimizer.max_consecutive_failures", i(50)),
        ("optimizer.max_consecutive_failures_pass1", i(12)),
        ("optimizer.timeout", None),
        // scoring.
        ("scoring.via_costs", i(50)),
        ("scoring.plane_via_costs", i(5)),
        ("scoring.start_ripup_costs", i(100)),
        ("scoring.default_preferred_direction_trace_cost", f64v(1.0)),
        ("scoring.default_undesired_direction_trace_cost", f64v(1.0)),
        ("scoring.default_bend_cost", f64v(0.0)),
        ("scoring.unrouted_net_penalty", f32v(5e6)),
        ("scoring.clearance_violation_penalty", f32v(1e6)),
        ("scoring.bend_penalty", f32v(10.0)),
        ("scoring.preferred_direction_trace_cost", None),
        ("scoring.undesired_direction_trace_cost", None),
        // router_scoring.
        ("router_scoring.version", e("V2_CONTINUOUS")),
        ("router_scoring.unrouted_connection_weight", f32v(1000.0)),
        ("router_scoring.unrouted_free_fraction", f32v(0.5)),
        (
            "router_scoring.unrouted_first_half_weight",
            f32v(1000.0_f32 / 3.0_f32),
        ),
        (
            "router_scoring.unrouted_second_half_weight",
            f32v(2000.0_f32 / 3.0_f32),
        ),
        (
            "router_scoring.clearance_violation_count_weight",
            f32v(25.0),
        ),
        (
            "router_scoring.clearance_violation_depth_weight",
            f32v(300.0),
        ),
        (
            "router_scoring.clearance_violation_depth_scale",
            f32v(1000.0),
        ),
        // optimizer_scoring.
        ("optimizer_scoring.version", e("V2_LOWER_BOUND")),
        ("optimizer_scoring.excess_wire_length_weight", f32v(1000.0)),
        ("optimizer_scoring.excess_via_weight", f32v(2000.0)),
        ("optimizer_scoring.excess_bend_weight", f32v(500.0)),
        ("optimizer_scoring.length_floor", f32v(1.0)),
        ("optimizer_scoring.difficulty_scale_floor", f32v(1.0)),
        // Q02.2: strict sizes, unset (off; not a Java key, so not dumped).
        ("neck_down.strict", None),
        // X07: no length matching unless selected.
        ("length_matching.groups", None),
        ("special.length_matching", None),
        // X08: no differential pairs unless selected.
        ("diff_pairs.infer_from_names", None),
        ("diff_pairs.skew_um", None),
        ("special.diff_pairs", None),
        // The A10.2 stage keys: no default.
        ("autorouter.ordering", None),
        ("autorouter.connection_router", None),
        ("autorouter.ripup_policy", None),
        ("autorouter.pass_controller", None),
        // Q10: unset, which is off.
        ("autorouter.sweep", None),
        ("autorouter.sweep_passes", None),
        ("autorouter.sweep_after_optimizer", None),
        // Q04.3: unset, which is off.
        ("autorouter.output_guard", None),
        // Q04.1: unset, which is the `quantum-stop` defaults.
        ("quantum_stop.grace", None),
        ("quantum_stop.checks", None),
        ("quantum_stop.quantum_abs", None),
        ("quantum_stop.quantum_pct", None),
        ("special.plane_taps", None),
        ("special.neck_down", None),
        ("special.stages", None),
        ("special.diagnostics", None),
        // Q16: unset, which is off.
        ("cleanup.remove_stubs", None),
        // P02.1: unset, which is off.
        ("debug.audit_incremental", None),
    ];
    let tabled: BTreeSet<&str> = FIELDS.iter().map(|f| f.path).collect();
    let listed: BTreeSet<&str> = expected.iter().map(|(p, _)| *p).collect();
    assert_eq!(tabled, listed, "this test must list every table entry");
    for (path, want) in expected {
        assert_eq!(value(patch, path), want, "{what}: default of {path}");
    }
    // `FieldDesc::default` agrees with `default_patch`.
    for field in FIELDS {
        assert_eq!(
            (field.default)(CORES),
            (field.get)(patch),
            "{what}: {}",
            field.path
        );
    }
    assert_eq!(patch.layers, None);
    assert_eq!(patch.costs_initialized, None);
}

#[test]
fn thread_default_is_cores_minus_one_but_at_least_one() {
    for (cores, want) in [(1, 1), (2, 1), (3, 2), (16, 15)] {
        let patch = default_patch(cores);
        assert_eq!(value(&patch, "max_threads"), i(want), "{cores} cores");
        assert_eq!(value(&patch, "autorouter.max_threads"), i(want));
        assert_eq!(value(&patch, "optimizer.max_threads"), i(want));
    }
}

#[test]
fn resolved_view_has_java_types() {
    let settings = RouterSettings::from_patch(&default_patch(CORES), CORES);
    let _: i64 = settings.fanout.max_milliseconds_per_pin;
    let _: f32 = settings.optimizer.improvement_threshold;
    let _: f64 = settings.copper_to_edge_clearance_um;
    let _: &Vec<String> = &settings.plane_nets;
    assert_eq!(settings.fanout.max_milliseconds_per_pin, 10000);
    assert_eq!(settings.job_timeout.as_deref(), Some("12:00:00"));
    assert_eq!(settings.min_trace_width_um, None);
    assert_eq!(settings.result_json, None);
    assert_eq!(settings.fanout.timeout, None);
    assert_eq!(settings.optimizer.timeout, None);
    assert_eq!(settings.layers, None);
    assert!(!settings.costs_initialized);
    assert_eq!(settings.scoring.preferred_direction_trace_cost, None);
    assert_eq!(
        settings.router_scoring.version,
        RouterScoringVersion::V2Continuous
    );
    assert_eq!(
        settings.optimizer_scoring.version,
        OptimizerScoringVersion::V2LowerBound
    );
    // `ScoringVersionSettingsTest.defaultOptimizerImprovementThresholdIsDefaultPercentage`
    assert_eq!(settings.optimizer.improvement_threshold, 2.5_f32);
}

#[test]
fn resolved_score_settings_reuse_the_scoring_module_types() {
    let settings = RouterSettings::from_patch(&default_patch(CORES), CORES);
    assert_eq!(
        settings.router_scoring.score_weights(),
        RouterScoreSettings::default()
    );
    assert_eq!(
        settings.optimizer_scoring.score_weights(),
        OptimizerScoreSettings::default()
    );
}

/// `BendCostSettingsTest.nullScoringSafety` (settings half): a patch whose
/// `scoring` is empty resolves through Java's getter fallbacks.
#[test]
fn null_scoring_safety() {
    let mut patch = SettingsPatch::default();
    patch.set_layer_count(2);
    // `settings.scoring = null`
    patch.scoring = Default::default();
    let ctx = ctx();
    let sources = [(Source::Testing, patch.clone())];
    let resolved = resolve(&sources, &ctx);
    let s = &resolved.settings;
    assert_eq!(s.scoring.start_ripup_costs, 1);
    assert_eq!(s.scoring.via_costs, 1);
    assert_eq!(s.scoring.plane_via_costs, 1);
    assert_eq!(s.preferred_direction_trace_costs(0), 1.0);
    assert_eq!(s.against_preferred_direction_trace_costs(1), 1.0);

    // setStartRipupCosts(5), setViaCosts(3)
    patch.set_start_ripup_costs(5);
    patch.set_via_costs(3);
    let s = resolve(&[(Source::Testing, patch.clone())], &ctx).settings;
    assert_eq!(s.scoring.start_ripup_costs, 5);
    assert_eq!(s.scoring.via_costs, 3);

    // settings.scoring = null; setPreferredDirectionTraceCosts(0, 2.0)
    patch.scoring = Default::default();
    patch.set_preferred_direction_trace_costs(0, 2.0);
    let s = resolve(&[(Source::Testing, patch)], &ctx).settings;
    assert_eq!(s.preferred_direction_trace_costs(0), 2.0);

    // An empty patch resolves through the same fallbacks.
    let s = resolve(&[(Source::Testing, SettingsPatch::default())], &ctx).settings;
    assert_eq!(
        (
            s.scoring.start_ripup_costs,
            s.scoring.via_costs,
            s.scoring.plane_via_costs
        ),
        (1, 1, 1)
    );
    assert!(!s.run_optimizer(), "getRunOptimizer falls back to false");
    assert!(s.run_router(), "getRunRouter falls back to true");
    assert!(!s.automatic_neckdown);
    assert!(!s.fanout.enabled, "isFanoutEnabled falls back to false");
    assert!(s.allowed_via_types, "getViasAllowed falls back to true");
}

// ---- every_field_has_a_table_entry -------------------------------------

/// The leaf member paths of a struct, read from its pretty `Debug` output.
fn member_paths(debug: &str) -> BTreeSet<String> {
    let mut stack: Vec<String> = Vec::new();
    let mut leaves = BTreeSet::new();
    for line in debug.lines().skip(1) {
        let line = line.trim();
        if line == "}" || line == "}," {
            stack.pop();
            continue;
        }
        let Some((name, rest)) = line.split_once(": ") else {
            continue;
        };
        if !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            continue;
        }
        let path = stack
            .iter()
            .map(String::as_str)
            .chain([name])
            .collect::<Vec<_>>()
            .join(".");
        if rest.ends_with('{') {
            stack.push(name.to_string());
        } else {
            leaves.insert(path);
        }
    }
    leaves
}

/// The members of `value` missing from the table, and the table paths
/// missing from `value`. `extras` are members that are deliberately not
/// scalar table entries.
fn table_mismatches<T: Debug>(value: &T, paths: &BTreeSet<&str>, extras: &[&str]) -> Vec<String> {
    let members = member_paths(&format!("{value:#?}"));
    let mut problems = Vec::new();
    for member in &members {
        if !paths.contains(member.as_str()) && !extras.contains(&member.as_str()) {
            problems.push(format!("member {member} has no table entry"));
        }
    }
    for path in paths {
        if !members.contains(*path) {
            problems.push(format!("table entry {path} has no member"));
        }
    }
    problems
}

const EXTRAS: &[&str] = &["layers", "costs_initialized", "validation_warnings"];

#[test]
fn every_field_has_a_table_entry() {
    let paths: BTreeSet<&str> = FIELDS.iter().map(|f| f.path).collect();
    assert_eq!(paths.len(), FIELDS.len(), "paths are unique");
    let patch = default_patch(CORES);
    let resolved = RouterSettings::from_patch(&patch, CORES);
    assert_eq!(
        table_mismatches(&patch, &paths, EXTRAS),
        Vec::<String>::new()
    );
    assert_eq!(
        table_mismatches(&resolved, &paths, EXTRAS),
        Vec::<String>::new()
    );
    // The table kinds line up with the optional marker.
    for field in FIELDS {
        if (field.default)(CORES).is_none() {
            assert!(
                field.optional,
                "{} has no default, so it is optional",
                field.path
            );
        }
        assert!(!field.doc.is_empty(), "{} has a doc line", field.path);
        let _ = field.kind;
    }
    assert!(FIELDS.iter().any(|f| f.kind == FieldKind::DoubleList));
}

/// The negative control: a struct with a member the table does not know.
#[test]
fn table_check_flags_an_untabled_member() {
    #[derive(Debug)]
    #[allow(dead_code)]
    struct Probe {
        max_threads: Option<i32>,
        untabled_probe: Option<i32>,
    }
    let paths: BTreeSet<&str> = ["max_threads", "job_timeout"].into_iter().collect();
    let problems = table_mismatches(
        &Probe {
            max_threads: None,
            untabled_probe: None,
        },
        &paths,
        &[],
    );
    assert_eq!(
        problems,
        vec![
            "member untabled_probe has no table entry".to_string(),
            "table entry job_timeout has no member".to_string(),
        ]
    );
}

// ---- work bounds and deadlines (D2) ------------------------------------

#[test]
fn work_bounds_carry_counts_and_deadlines_carry_durations() {
    use freerouting::settings::{Deadlines, FanoutBounds, StageBounds, WorkBounds};
    use std::time::Duration;

    let settings = RouterSettings::from_patch(&default_patch(CORES), CORES);
    assert_eq!(
        settings.work_bounds(),
        WorkBounds {
            autorouter: StageBounds {
                max_passes: 0,
                max_items: i32::MAX
            },
            fanout: FanoutBounds {
                max_passes: 20,
                max_items: i32::MAX,
                max_milliseconds_per_pin: 10000
            },
            optimizer: StageBounds {
                max_passes: 100,
                max_items: i32::MAX
            },
        }
    );
    assert_eq!(
        settings.deadlines(),
        Deadlines {
            job: Some(Duration::from_secs(12 * 3600)),
            fanout: None,
            optimizer: None
        }
    );

    let mut patch = default_patch(CORES);
    patch.job_timeout = Some("48:00:00".to_string());
    patch.fanout.timeout = Some("00:30".to_string());
    patch.optimizer.timeout = Some("90".to_string());
    let d = RouterSettings::from_patch(&patch, CORES).deadlines();
    assert_eq!(
        d.job,
        Some(Duration::from_secs(24 * 3600)),
        "capped at 24 h"
    );
    assert_eq!(d.fanout, Some(Duration::from_secs(30)));
    assert_eq!(d.optimizer, Some(Duration::from_secs(90)));

    // Java's "5m" is a no-op: unparsable text means no limit.
    patch.job_timeout = Some("5m".to_string());
    patch.fanout.timeout = Some("".to_string());
    let d = RouterSettings::from_patch(&patch, CORES).deadlines();
    assert_eq!((d.job, d.fanout), (None, None));
}
