//! C02.6: environment variables.
//!
//! Ports `EnvironmentVariablesSourceTest`; the priority cases are in
//! `settings_merge.rs::environment_variables_priority`.

// Patches are built field by field, as the Java tests build `RouterSettings`.
#![allow(clippy::field_reassign_with_default)]

use freerouting::settings::env::{EnvParse, parse_environment};
use freerouting::settings::types::JavaEnum;
use freerouting::settings::{
    BoardUpdateStrategy, FIELDS, FieldValue, ItemSelectionStrategy, SettingsPatch,
};

fn parse(vars: &[(&str, &str)]) -> EnvParse {
    parse_environment(vars.iter().copied())
}

#[test]
fn empty_environment() {
    let source = parse(&[]);
    assert_eq!(source.patch, SettingsPatch::default());
    assert_eq!(source.parsed_count(), 0);
}

#[test]
fn simple_router_setting() {
    let source = parse(&[("FREEROUTING__ROUTER__MAX_PASSES", "100")]);
    assert_eq!(source.patch.autorouter.max_passes, Some(100));
    assert_eq!(source.parsed_count(), 1);
}

#[test]
fn nested_autorouter_max_passes() {
    let source = parse(&[("FREEROUTING__ROUTER__AUTOROUTER__MAX_PASSES", "33")]);
    assert_eq!(source.patch.autorouter.max_passes, Some(33));
    assert_eq!(source.parsed_count(), 1);
    assert!(
        source.warnings.is_empty(),
        "the nested path is not deprecated"
    );
}

#[test]
fn nested_router_setting() {
    let source = parse(&[("FREEROUTING__ROUTER__OPTIMIZER__MAX_THREADS", "8")]);
    assert_eq!(source.patch.optimizer.max_threads, Some(8));
    assert_eq!(source.parsed_count(), 1);
}

#[test]
fn boolean_setting() {
    let source = parse(&[("FREEROUTING__ROUTER__VIAS_ALLOWED", "false")]);
    assert_eq!(source.patch.allowed_via_types, Some(false));
    assert_eq!(source.parsed_count(), 1);
}

#[test]
fn multiple_settings() {
    let source = parse(&[
        ("FREEROUTING__ROUTER__MAX_PASSES", "50"),
        ("FREEROUTING__ROUTER__OPTIMIZER__MAX_THREADS", "4"),
        ("FREEROUTING__ROUTER__VIAS_ALLOWED", "true"),
    ]);
    assert_eq!(source.patch.autorouter.max_passes, Some(50));
    assert_eq!(source.patch.optimizer.max_threads, Some(4));
    assert_eq!(source.patch.allowed_via_types, Some(true));
    assert_eq!(source.parsed_count(), 3);
}

#[test]
fn ignores_non_router_variables() {
    let source = parse(&[
        ("FREEROUTING__GUI__INPUT_DIRECTORY", "/some/path"),
        ("FREEROUTING__ROUTER__MAX_PASSES", "100"),
        ("FREEROUTING__USER_DATA_PATH", "/data"),
        ("PATH", "/usr/bin"),
        ("HOME", "/home/user"),
    ]);
    assert_eq!(source.patch.autorouter.max_passes, Some(100));
    assert_eq!(source.parsed_count(), 1);
}

#[test]
fn ignores_variables_without_prefix() {
    let source = parse(&[("MAX_PASSES", "100"), ("ROUTER__MAX_PASSES", "200")]);
    assert_eq!(source.patch.autorouter.max_passes, None);
    assert_eq!(source.parsed_count(), 0);
}

#[test]
fn invalid_property_name() {
    let source = parse(&[
        ("FREEROUTING__ROUTER__INVALID_PROPERTY", "value"),
        ("FREEROUTING__ROUTER__MAX_PASSES", "100"),
    ]);
    assert_eq!(source.patch.autorouter.max_passes, Some(100));
    assert_eq!(source.parsed_count(), 1);
    assert!(source.warnings.contains(
        &"Unknown router setting in environment variable: FREEROUTING__ROUTER__INVALID_PROPERTY (property: invalid_property)"
            .to_string()
    ));
}

#[test]
fn invalid_value() {
    let source = parse(&[("FREEROUTING__ROUTER__MAX_PASSES", "not_a_number")]);
    assert_eq!(source.patch.autorouter.max_passes, None);
    assert_eq!(source.parsed_count(), 0);
    assert!(source.warnings.iter().any(|w| w.starts_with(
        "Failed to parse environment variable: FREEROUTING__ROUTER__MAX_PASSES = not_a_number: "
    )));
}

#[test]
fn get_parsed_variables() {
    let source = parse(&[
        ("FREEROUTING__ROUTER__MAX_PASSES", "100"),
        ("FREEROUTING__ROUTER__OPTIMIZER__MAX_THREADS", "4"),
    ]);
    assert_eq!(source.parsed.len(), 2);
    assert_eq!(source.parsed["FREEROUTING__ROUTER__MAX_PASSES"], "100");
    assert_eq!(
        source.parsed["FREEROUTING__ROUTER__OPTIMIZER__MAX_THREADS"],
        "4"
    );
}

#[test]
fn string_settings() {
    let source = parse(&[("FREEROUTING__ROUTER__ALGORITHM", "custom-router")]);
    assert_eq!(
        source.patch.autorouter.algorithm.as_deref(),
        Some("custom-router")
    );
    assert_eq!(source.parsed_count(), 1);
}

#[test]
fn save_intermediate_stages() {
    let source = parse(&[("FREEROUTING__ROUTER__SAVE_INTERMEDIATE_STAGES", "true")]);
    assert_eq!(source.patch.autorouter.save_intermediate_stages, Some(true));
    assert_eq!(source.parsed_count(), 1);
}

#[test]
fn optimizer_strategies() {
    let source = parse(&[
        (
            "FREEROUTING__ROUTER__OPTIMIZER__BOARD_UPDATE_STRATEGY",
            "GLOBAL_OPTIMAL",
        ),
        (
            "FREEROUTING__ROUTER__OPTIMIZER__ITEM_SELECTION_STRATEGY",
            "SEQUENTIAL",
        ),
    ]);
    assert_eq!(
        source.patch.optimizer.board_update_strategy,
        Some(BoardUpdateStrategy::GlobalOptimal)
    );
    assert_eq!(
        source.patch.optimizer.item_selection_strategy,
        Some(ItemSelectionStrategy::Sequential)
    );
    assert_eq!(source.parsed_count(), 2);
    // Enum values are matched ignoring case.
    let source = parse(&[(
        "FREEROUTING__ROUTER__OPTIMIZER__ITEM_SELECTION_STRATEGY",
        "prioritized",
    )]);
    assert_eq!(
        source
            .patch
            .optimizer
            .item_selection_strategy
            .map(JavaEnum::java_name),
        Some("PRIORITIZED")
    );
}

#[test]
fn lowercase_environment_variable_names() {
    let source = parse(&[
        ("freerouting__router__max_passes", "100"),
        ("freerouting__router__save_intermediate_stages", "true"),
    ]);
    assert_eq!(source.patch.autorouter.max_passes, Some(100));
    assert_eq!(source.patch.autorouter.save_intermediate_stages, Some(true));
    assert_eq!(source.parsed_count(), 2);
}

#[test]
fn trace_cost_settings() {
    let source = parse(&[
        (
            "FREEROUTING__ROUTER__SCORING__PREFERRED_DIRECTION_TRACE_COST",
            "1.5,2.0",
        ),
        (
            "FREEROUTING__ROUTER__SCORING__UNDESIRED_DIRECTION_TRACE_COST",
            "2.5,3.0",
        ),
    ]);
    assert_eq!(
        source.patch.scoring.preferred_direction_trace_cost,
        Some(vec![1.5, 2.0])
    );
    assert_eq!(
        source.patch.scoring.undesired_direction_trace_cost,
        Some(vec![2.5, 3.0])
    );
    assert_eq!(source.parsed_count(), 2);
}

#[test]
fn comma_lists_for_string_arrays() {
    let source = parse(&[(
        "FREEROUTING__ROUTER__AUTOROUTER__IGNORE_NET_CLASSES",
        "POWER, GND",
    )]);
    assert_eq!(
        source.patch.autorouter.ignore_net_classes,
        Some(vec!["POWER".to_string(), "GND".to_string()])
    );
    let source = parse(&[("FREEROUTING__ROUTER__PLANE_NETS", "")]);
    assert_eq!(source.patch.plane_nets, Some(Vec::new()));
}

/// Java iterates `System.getenv()` in hash order, so a collision between
/// two variables has a platform-dependent winner. Rust processes them in
/// sorted order of their names, so the later name wins whatever the
/// order they are given in.
#[test]
fn env_collision_is_deterministic() {
    // "…__AUTOROUTER__MAX_PASSES" sorts before "…__MAX_PASSES".
    let vars = [
        ("FREEROUTING__ROUTER__AUTOROUTER__MAX_PASSES", "2"),
        ("FREEROUTING__ROUTER__MAX_PASSES", "1"),
    ];
    let forward = parse_environment(vars.iter().copied());
    let backward = parse_environment(vars.iter().rev().copied());
    assert_eq!(forward.patch.autorouter.max_passes, Some(1));
    assert_eq!(forward.patch, backward.patch);
    assert_eq!(forward.parsed, backward.parsed);
    assert_eq!(forward.warnings, backward.warnings);
    // Upper and lower case spellings of one name also collide in sorted
    // order of the raw names ("FREEROUTING…" < "freerouting…").
    let vars = [
        ("freerouting__router__max_passes", "9"),
        ("FREEROUTING__ROUTER__MAX_PASSES", "8"),
    ];
    assert_eq!(
        parse_environment(vars.iter().copied())
            .patch
            .autorouter
            .max_passes,
        Some(9)
    );
    assert_eq!(
        parse_environment(vars.iter().rev().copied())
            .patch
            .autorouter
            .max_passes,
        Some(9)
    );
}

/// The env case of `flat_cost_keys_reach_scoring`.
#[test]
fn flat_cost_keys_reach_scoring() {
    let source = parse(&[("FREEROUTING__ROUTER__START_RIPUP_COSTS", "1500")]);
    assert_eq!(source.patch.scoring.start_ripup_costs, Some(1500));
    assert!(
        source.warnings.is_empty(),
        "D8's flat cost keys do not warn"
    );
    let source = parse(&[
        ("FREEROUTING__ROUTER__VIA_COSTS", "25"),
        ("FREEROUTING__ROUTER__PLANE_VIA_COSTS", "3"),
        (
            "FREEROUTING__ROUTER__DEFAULT_PREFERRED_DIRECTION_TRACE_COST",
            "1.5",
        ),
        (
            "FREEROUTING__ROUTER__DEFAULT_UNDESIRED_DIRECTION_TRACE_COST",
            "3.5",
        ),
    ]);
    assert_eq!(source.patch.scoring.via_costs, Some(25));
    assert_eq!(source.patch.scoring.plane_via_costs, Some(3));
    assert_eq!(
        source.patch.scoring.default_preferred_direction_trace_cost,
        Some(1.5)
    );
    assert_eq!(
        source.patch.scoring.default_undesired_direction_trace_cost,
        Some(3.5)
    );
}

#[test]
fn deprecated_flat_autorouter_variables_warn() {
    let source = parse(&[("FREEROUTING__ROUTER__MAX_PASSES", "5")]);
    assert_eq!(
        source.warnings,
        vec![
            "Deprecated settings path 'FREEROUTING__ROUTER__MAX_PASSES'; use 'FREEROUTING__ROUTER__AUTOROUTER__MAX_PASSES' instead. The old path will be removed in a future release."
                .to_string()
        ]
    );
}

/// Every table field is reachable by its environment variable name.
#[test]
fn every_field_is_reachable_through_its_variable() {
    for field in FIELDS {
        let name = format!(
            "FREEROUTING__ROUTER__{}",
            field.path.replace('.', "__").to_uppercase()
        );
        let value = match field.kind {
            freerouting::settings::FieldKind::Bool => "true",
            freerouting::settings::FieldKind::Str => "x",
            freerouting::settings::FieldKind::RouterScoringVersion => "V1_LEGACY",
            freerouting::settings::FieldKind::OptimizerScoringVersion => "V1_LEGACY",
            freerouting::settings::FieldKind::BoardUpdateStrategy => "GLOBAL_OPTIMAL",
            freerouting::settings::FieldKind::ItemSelectionStrategy => "PRIORITIZED",
            _ => "3",
        };
        let source = parse_environment([(name.as_str(), value)]);
        assert_eq!(source.parsed_count(), 1, "{name}: {:?}", source.warnings);
        assert!((field.get)(&source.patch).is_some(), "{name}");
        let _: Option<FieldValue> = (field.get)(&source.patch);
    }
}

#[test]
fn case_insensitive_property_names() {
    // Property names are converted to lower case.
    let source = parse(&[("FREEROUTING__ROUTER__MAX_PASSES", "100")]);
    assert_eq!(source.patch.autorouter.max_passes, Some(100));
    let source = parse(&[("FREEROUTING__ROUTER__Optimizer__Max_Threads", "5")]);
    assert_eq!(source.patch.optimizer.max_threads, Some(5));
}
