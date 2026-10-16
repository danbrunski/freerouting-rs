//! C02.2: merge with Java's `copyFields` semantics.
//!
//! Ports `SettingsMergerTest` (each Java method keeps its name in
//! snake_case), `Issue885SettingsInitializationTest` (the settings half) and
//! `ReflectionUtilArrayTest.testCopyFieldsOverwritesExistingNonEmptyArray`.
//!
//! Not ported: `SettingsMergerTest.nullValueHandling` has a source whose
//! `getSettings()` is `null`; a patch can't be null, so the test below uses
//! an empty patch at priority 100, which is the same "no opinion". The two
//! board halves of `Issue885SettingsInitializationTest`
//! (`applyBoardSpecificOptimizations*`, `applyNetClassExclusions`) are C03's.

// Patches are built field by field, as the Java tests build `RouterSettings`.
#![allow(clippy::field_reassign_with_default)]

use freerouting::settings::env::parse_environment;
use freerouting::settings::{
    LayerSettings, ResolveContext, Resolved, SettingsPatch, Source, defaults, parse_cli_arguments,
    resolve,
};

const CORES: usize = 16;

fn ctx() -> ResolveContext {
    ResolveContext { cores: CORES }
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| s.to_string()).collect()
}

fn env(vars: &[(&str, &str)]) -> SettingsPatch {
    parse_environment(vars.iter().copied()).patch
}

fn merge(sources: Vec<(Source, SettingsPatch)>) -> Resolved {
    resolve(&sources, &ctx())
}

fn with_defaults(mut rest: Vec<(Source, SettingsPatch)>) -> Resolved {
    let mut sources = vec![(Source::Defaults, defaults(&ctx()))];
    sources.append(&mut rest);
    merge(sources)
}

#[test]
fn default_settings_only() {
    let merged = merge(vec![(Source::Defaults, defaults(&ctx()))]);
    assert_eq!(merged.patch.autorouter.max_passes, Some(0));
    assert_eq!(merged.patch.autorouter.enabled, Some(true));
    assert_eq!(merged.patch.allowed_via_types, Some(true));
    assert!(merged.settings.run_optimizer());
}

#[test]
fn empty_sources_list() {
    let merged = merge(vec![]);
    assert_eq!(merged.patch.autorouter.max_passes, None);
    assert!(
        merged
            .warnings
            .contains(&"No settings sources provided, using defaults".to_string())
    );
}

#[test]
fn source_priorities() {
    assert_eq!(Source::Defaults.priority(), 0);
    assert_eq!(Source::Defaults.name(), "Default Settings");
    let priorities: Vec<i32> = [
        Source::Defaults,
        Source::ConfigFile,
        Source::Dsn,
        Source::RulesFile,
        Source::Environment,
        Source::Cli,
        Source::Api,
        Source::Testing,
    ]
    .iter()
    .map(Source::priority)
    .collect();
    assert_eq!(priorities, vec![0, 10, 20, 40, 55, 60, 70, 80]);
}

#[test]
fn multiple_sources_merging() {
    let merged = with_defaults(vec![(
        Source::Environment,
        env(&[("FREEROUTING__ROUTER__MAX_PASSES", "50")]),
    )]);
    assert_eq!(merged.patch.autorouter.max_passes, Some(50));
    assert_eq!(merged.patch.autorouter.enabled, Some(true));
}

#[test]
fn priority_ordering() {
    // The environment (55) beats the config file (10), whatever the order
    // of the list.
    let environment = env(&[
        ("FREEROUTING__ROUTER__MAX_PASSES", "100"),
        ("FREEROUTING__ROUTER__OPTIMIZER__MAX_THREADS", "8"),
    ]);
    let mut file = SettingsPatch::default();
    file.autorouter.max_passes = Some(3);
    file.optimizer.max_threads = Some(2);
    for order in [
        vec![
            (Source::Defaults, defaults(&ctx())),
            (Source::ConfigFile, file.clone()),
            (Source::Environment, environment.clone()),
        ],
        vec![
            (Source::Environment, environment.clone()),
            (Source::ConfigFile, file.clone()),
            (Source::Defaults, defaults(&ctx())),
        ],
    ] {
        let merged = merge(order);
        assert_eq!(merged.patch.autorouter.max_passes, Some(100));
        assert_eq!(merged.patch.optimizer.max_threads, Some(8));
    }
}

#[test]
fn null_value_handling() {
    let nothing = Source::Custom {
        name: "Null Source".to_string(),
        priority: 100,
    };
    let merged = with_defaults(vec![(nothing, SettingsPatch::default())]);
    assert_eq!(merged.patch.autorouter.max_passes, Some(0));
}

#[test]
fn partial_overrides() {
    let merged = with_defaults(vec![(
        Source::Environment,
        env(&[("FREEROUTING__ROUTER__MAX_PASSES", "200")]),
    )]);
    assert_eq!(merged.patch.autorouter.max_passes, Some(200));
    assert_eq!(merged.patch.allowed_via_types, Some(true));
    assert_eq!(merged.patch.autorouter.enabled, Some(true));
}

#[test]
fn environment_variables_priority() {
    let priority = Source::Environment.priority();
    assert_eq!(priority, 55);
    assert!(priority > 50 && priority < 60);
    assert_eq!(Source::Environment.name(), "Environment Variables");
    // Env (55) loses to the CLI (60) and beats the config file (10).
    let mut file = SettingsPatch::default();
    file.scoring.start_ripup_costs = Some(1500);
    file.scoring.via_costs = Some(11);
    let environment = env(&[
        ("FREEROUTING__ROUTER__START_RIPUP_COSTS", "1600"),
        ("FREEROUTING__ROUTER__VIA_COSTS", "22"),
    ]);
    let cli = parse_cli_arguments(&strings(&["--router.via_costs=33"])).patch;
    let merged = with_defaults(vec![
        (Source::Cli, cli),
        (Source::Environment, environment),
        (Source::ConfigFile, file),
    ]);
    assert_eq!(merged.patch.scoring.start_ripup_costs, Some(1600));
    assert_eq!(merged.patch.scoring.via_costs, Some(33));
}

#[test]
fn complex_merging() {
    let merged = with_defaults(vec![(
        Source::Environment,
        env(&[
            ("FREEROUTING__ROUTER__MAX_PASSES", "150"),
            ("FREEROUTING__ROUTER__OPTIMIZER__MAX_THREADS", "6"),
            ("FREEROUTING__ROUTER__VIAS_ALLOWED", "false"),
            ("FREEROUTING__ROUTER__ALGORITHM", "custom-router"),
        ]),
    )]);
    assert_eq!(merged.patch.autorouter.max_passes, Some(150));
    assert_eq!(merged.patch.optimizer.max_threads, Some(6));
    // `VIAS_ALLOWED` matches the Java field name `viasAllowed`, so it works
    // (Java's test only had it commented out).
    assert_eq!(merged.patch.allowed_via_types, Some(false));
    assert_eq!(
        merged.patch.autorouter.algorithm.as_deref(),
        Some("custom-router")
    );
}

#[test]
fn cli_can_disable_router_and_optimizer() {
    let cli = parse_cli_arguments(&strings(&[
        "--router.enabled=false",
        "--router.optimizer.enabled=false",
    ]));
    let merged = with_defaults(vec![(Source::Cli, cli.patch)]);
    assert!(!merged.settings.run_router());
    assert!(!merged.settings.run_optimizer());
}

#[test]
fn legacy_batch_mode_enables_router_when_json_disables_it() {
    let mut json = SettingsPatch::default();
    json.autorouter.enabled = Some(false);
    let cli = parse_cli_arguments(&strings(&[
        "-de",
        "fixtures/Issue508-DAC2020_bm05.dsn",
        "-do",
        "fixtures/Issue508-DAC2020_bm05.ses",
    ]));
    let merged = with_defaults(vec![
        (
            Source::Custom {
                name: "JSON test source".into(),
                priority: 10,
            },
            json,
        ),
        (Source::Cli, cli.patch),
    ]);
    assert!(merged.settings.run_router());
}

#[test]
fn explicit_router_enabled_false_overrides_legacy_batch_mode() {
    let cli = parse_cli_arguments(&strings(&[
        "-de",
        "fixtures/Issue508-DAC2020_bm05.dsn",
        "-do",
        "fixtures/Issue508-DAC2020_bm05.ses",
        "--router.enabled=false",
    ]));
    let merged = with_defaults(vec![(Source::Cli, cli.patch)]);
    assert!(!merged.settings.run_router());
}

#[test]
fn cli_routable_layers_does_not_disable_router() {
    let cli = parse_cli_arguments(&strings(&[
        "-de",
        "fixtures/Issue508-DAC2020_bm05.dsn",
        "--router.layers.routable=false,true",
    ]));
    assert!(cli.warnings.is_empty(), "{:?}", cli.warnings);
    let merged = with_defaults(vec![(Source::Cli, cli.patch)]);
    assert!(merged.settings.run_router());
    let layers = merged.patch.layers.expect("layers from the CLI");
    assert_eq!(layers.len(), 2);
    assert_eq!(layers[0].routable, Some(false));
    assert_eq!(layers[1].routable, Some(true));
}

fn patch_with_layers(count: usize, routable: &[bool]) -> SettingsPatch {
    let mut patch = SettingsPatch::default();
    patch.set_layer_count(count);
    for (i, r) in routable.iter().enumerate() {
        patch.layers.as_mut().unwrap()[i].routable = Some(*r);
    }
    patch
}

#[test]
fn layers_array_not_shrunk_on_merge() {
    let mut target = patch_with_layers(6, &[true, true, true]);
    let source = patch_with_layers(2, &[false, true]);
    target.apply_from(&source);
    let layers = target.layers.unwrap();
    assert_eq!(layers.len(), 6);
    assert_eq!(layers[0].routable, Some(false));
    assert_eq!(layers[1].routable, Some(true));
    assert_eq!(layers[2].routable, Some(true));
}

/// The asymmetry of `copyFields`: a target shorter than the source is
/// rebuilt from the source alone, and its per-layer values are lost.
#[test]
fn shorter_target_layers_are_rebuilt_from_the_source_only() {
    let mut target = patch_with_layers(2, &[true, true]);
    target.layers.as_mut().unwrap()[0].bend_cost = Some(3.0);
    target.layers.as_mut().unwrap()[1].preferred_direction_trace_cost = Some(2.5);
    let mut source = SettingsPatch::default();
    source.layers = Some(vec![
        LayerSettings {
            routable: Some(false),
            ..LayerSettings::default()
        };
        4
    ]);
    target.apply_from(&source);
    let layers = target.layers.unwrap();
    assert_eq!(layers.len(), 4);
    assert!(layers.iter().all(|l| l.routable == Some(false)));
    assert_eq!(layers[0].bend_cost, None, "the target's bend cost is lost");
    assert_eq!(layers[1].preferred_direction_trace_cost, None);

    // An equal-length target merges element by element instead.
    let mut target = patch_with_layers(2, &[true, true]);
    target.layers.as_mut().unwrap()[0].bend_cost = Some(3.0);
    let mut source = SettingsPatch::default();
    source.layers = Some(vec![LayerSettings::new(false, true); 2]);
    target.apply_from(&source);
    let layers = target.layers.unwrap();
    assert_eq!(layers[0].bend_cost, Some(3.0));
    assert_eq!(layers[0].routable, Some(false));
}

#[test]
fn cli_optimizer_improvement_threshold_parses_and_scales_fraction() {
    let threshold = |args: &[&str]| {
        let cli = parse_cli_arguments(&strings(args));
        with_defaults(vec![(Source::Cli, cli.patch)])
            .settings
            .optimizer
            .improvement_threshold
    };
    assert!((threshold(&["-oit", "0.035"]) - 3.5).abs() < 0.001);
    assert!((threshold(&["-oit", "4.0"]) - 4.0).abs() < 0.001);
    assert!((threshold(&["--router.optimizer.improvement_threshold=0.015"]) - 1.5).abs() < 0.001);
}

#[test]
fn cli_nested_autorouter_max_threads_is_independent_of_optimizer() {
    let cli = parse_cli_arguments(&strings(&[
        "--router.autorouter.max_threads=1",
        "--router.optimizer.max_threads=4",
    ]));
    let merged = with_defaults(vec![(Source::Cli, cli.patch)]);
    assert_eq!(merged.patch.autorouter.max_threads, Some(1));
    assert_eq!(merged.patch.optimizer.max_threads, Some(4));
    assert_eq!(merged.settings.autorouter_max_threads(CORES), 1);
}

#[test]
fn cli_settings_parses_inc_flag_into_ignore_net_classes() {
    let cli = parse_cli_arguments(&strings(&["-inc", "POWER,GND"]));
    assert_eq!(
        cli.patch.autorouter.ignore_net_classes,
        Some(vec!["POWER".to_string(), "GND".to_string()])
    );
    // `-inc` is deprecated in favour of the nested path.
    assert!(cli.warnings.iter().any(|w| w.contains("'-inc'")));
}

#[test]
fn reflection_util_copies_explicit_empty_array() {
    let mut target = SettingsPatch::default();
    target.autorouter.ignore_net_classes = Some(vec!["OLD_CLASS".to_string()]);
    let mut source = SettingsPatch::default();
    source.autorouter.ignore_net_classes = Some(Vec::new());
    let changed = target.apply_from(&source);
    assert!(changed > 0);
    assert_eq!(target.autorouter.ignore_net_classes, Some(Vec::new()));
}

#[test]
fn copy_fields_overwrites_existing_non_empty_array() {
    let mut source = SettingsPatch::default();
    source.plane_nets = Some(vec!["VCC".to_string(), "GND".to_string()]);
    let mut target = SettingsPatch::default();
    target.plane_nets = Some(vec!["DEFAULT_NET".to_string()]);
    let changed = target.apply_from(&source);
    assert!(changed > 0);
    assert_eq!(
        target.plane_nets,
        Some(vec!["VCC".to_string(), "GND".to_string()])
    );
}

#[test]
fn some_false_and_some_zero_overwrite_and_none_never_does() {
    let mut target = defaults(&ctx());
    let mut source = SettingsPatch::default();
    source.allowed_via_types = Some(false);
    source.autorouter.max_items = Some(0);
    source.optimizer.enabled = Some(false);
    target.apply_from(&source);
    assert_eq!(target.allowed_via_types, Some(false));
    assert_eq!(target.autorouter.max_items, Some(0));
    assert_eq!(target.optimizer.enabled, Some(false));
    // Fields the source leaves unset keep their value.
    assert_eq!(target.trace_pull_tight_accuracy, Some(500));
    assert_eq!(target.autorouter.enabled, Some(true));
    // And `costs_initialized` is copied whenever it is `Some`.
    let mut with_flag = SettingsPatch::default();
    with_flag.costs_initialized = Some(false);
    target.costs_initialized = Some(true);
    target.apply_from(&with_flag);
    assert_eq!(target.costs_initialized, Some(false));
    target.apply_from(&SettingsPatch::default());
    assert_eq!(target.costs_initialized, Some(false));
}

#[test]
fn equal_priorities_keep_insertion_order() {
    let source = |name: &str, passes: i32| {
        let mut patch = SettingsPatch::default();
        patch.autorouter.max_passes = Some(passes);
        (
            Source::Custom {
                name: name.to_string(),
                priority: 30,
            },
            patch,
        )
    };
    let merged = merge(vec![source("first", 1), source("second", 2)]);
    assert_eq!(merged.patch.autorouter.max_passes, Some(2));
    let merged = merge(vec![source("second", 2), source("first", 1)]);
    assert_eq!(merged.patch.autorouter.max_passes, Some(1));
}

#[test]
fn provenance_names_the_last_source_that_set_a_field() {
    let mut file = SettingsPatch::default();
    file.scoring.start_ripup_costs = Some(1500);
    let merged = with_defaults(vec![(Source::ConfigFile, file)]);
    assert_eq!(
        merged.provenance.get("scoring.start_ripup_costs"),
        Some(&Source::ConfigFile)
    );
    assert_eq!(
        merged.provenance.get("scoring.via_costs"),
        Some(&Source::Defaults)
    );
    assert!(
        merged
            .provenance_lines()
            .contains(&"scoring.start_ripup_costs = 1500 (config file)".to_string())
    );
}

#[test]
fn merge_base_clone_drops_result_json() {
    // Java's `RouterSettings.clone()` does not copy `resultJsonPath`.
    let mut first = SettingsPatch::default();
    first.result_json = Some("out.json".to_string());
    let merged = merge(vec![(Source::ConfigFile, first.clone())]);
    assert_eq!(merged.patch.result_json, None);
    // From a later source it is copied like any other field.
    let merged = merge(vec![
        (Source::Defaults, defaults(&ctx())),
        (Source::ConfigFile, first),
    ]);
    assert_eq!(merged.patch.result_json.as_deref(), Some("out.json"));
}
