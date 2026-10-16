//! C02.3: the path setter for CLI and environment values.
//!
//! Ports `ReflectionUtilArrayTest` (without `testCopyFieldsOverwritesExistingNonEmptyArray`,
//! which is in `settings_merge.rs`), `ScoringVersionSettingsTest`,
//! `NeckWidthSettingsTest`, the CLI half of `FlatCostSettingsTest` and the
//! `-oit` part of `SettingsMergerTest`.
//!
//! Not ported: `ScoringVersionSettingsTest.clonedSettingsRetainIndependentVersionSelections`
//! and `NeckWidthSettingsTest.cloneCarriesTheField` test Java's
//! `clone()`, which is `Clone` here and needs no test.

// Patches are built field by field, as the Java tests build `RouterSettings`.
#![allow(clippy::field_reassign_with_default)]

use freerouting::settings::path::{canonical_cli_path, is_deprecated_flat_autorouter_path};
use freerouting::settings::{
    FIELDS, FieldValue, LayerSettings, OptimizerScoringVersion, ResolveContext,
    RouterScoringVersion, SetError, SettingsPatch, Source, defaults, parse_cli_arguments, resolve,
};

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| s.to_string()).collect()
}

fn cli(args: &[&str]) -> SettingsPatch {
    parse_cli_arguments(&strings(args)).patch
}

fn ctx() -> ResolveContext {
    ResolveContext { cores: 8 }
}

// ---- ReflectionUtilArrayTest -------------------------------------------

#[test]
fn set_simple_property() {
    let mut p = SettingsPatch::default();
    p.set_path("autorouter.enabled", "false").unwrap();
    assert_eq!(p.autorouter.enabled, Some(false));
    p.set_path("autorouter.enabled", "true").unwrap();
    assert_eq!(p.autorouter.enabled, Some(true));
}

#[test]
fn set_nested_array_properties_when_null() {
    let mut p = SettingsPatch::default();
    p.set_path("layers.routable", "false,true").unwrap();
    let layers = p.layers.unwrap();
    assert_eq!(layers.len(), 2);
    assert_eq!(layers[0].routable, Some(false));
    assert_eq!(layers[1].routable, Some(true));
}

#[test]
fn set_nested_array_properties_when_initialized() {
    let mut p = SettingsPatch::default();
    p.set_layer_count(2);
    p.set_path("layers.routable", "false,true").unwrap();
    let layers = p.layers.as_ref().unwrap();
    assert_eq!(layers[0].routable, Some(false));
    assert_eq!(layers[1].routable, Some(true));
    p.set_path("layers.preferred_direction_horizontal", "true,false")
        .unwrap();
    let layers = p.layers.as_ref().unwrap();
    assert_eq!(layers[0].preferred_direction_horizontal, Some(true));
    assert_eq!(layers[1].preferred_direction_horizontal, Some(false));
}

#[test]
fn case_insensitive_and_serialized_name_matching() {
    let mut p = SettingsPatch::default();
    p.set_layer_count(2);
    let horizontal = |p: &SettingsPatch| {
        p.layers
            .as_ref()
            .unwrap()
            .iter()
            .map(|l| l.preferred_direction_horizontal.unwrap())
            .collect::<Vec<_>>()
    };
    // The serialized name.
    p.set_path("layers.preferred_direction_horizontal", "true,false")
        .unwrap();
    assert_eq!(horizontal(&p), vec![true, false]);
    // The Java field name in camelCase.
    p.set_path("layers.preferredDirectionHorizontal", "false,true")
        .unwrap();
    assert_eq!(horizontal(&p), vec![false, true]);
    // SCREAMING_SNAKE_CASE.
    p.set_path("LAYERS.PREFERRED_DIRECTION_HORIZONTAL", "true,false")
        .unwrap();
    assert_eq!(horizontal(&p), vec![true, false]);
    p.set_path("layers.routable", "true,false").unwrap();
    let layers = p.layers.unwrap();
    assert_eq!(layers[0].routable, Some(true));
    assert_eq!(layers[1].routable, Some(false));
}

// ---- name matching, separators, errors ---------------------------------

#[test]
fn separators_dot_colon_and_dash() {
    for path in [
        "autorouter.max_passes",
        "autorouter:max_passes",
        "autorouter-max_passes",
        "Autorouter.MaxPasses",
        "autorouter.maxPasses",
    ] {
        let mut p = SettingsPatch::default();
        p.set_path(path, "7").unwrap();
        assert_eq!(p.autorouter.max_passes, Some(7), "{path}");
    }
}

#[test]
fn gson_alternates_and_java_names_match() {
    let mut p = SettingsPatch::default();
    p.set_path("vias_allowed", "false").unwrap();
    assert_eq!(p.allowed_via_types, Some(false), "Java field viasAllowed");
    p.set_path("allowed_via_types", "true").unwrap();
    assert_eq!(p.allowed_via_types, Some(true));
    for alt in [
        "conduction_is_obstacle",
        "planeAsObstacle",
        "conductionIsObstacle",
        "plane_as_obstacle",
    ] {
        let mut p = SettingsPatch::default();
        p.set_path(alt, "1").unwrap();
        assert_eq!(p.plane_as_obstacle, Some(true), "{alt}");
    }
    let mut p = SettingsPatch::default();
    p.set_path("scoring.viaCosts", "9").unwrap();
    p.set_path("job_timeout", "01:00").unwrap();
    p.set_path("jobTimeoutString", "02:00").unwrap();
    assert_eq!(p.scoring.via_costs, Some(9));
    assert_eq!(p.job_timeout.as_deref(), Some("02:00"));
    p.set_path("boardSpecificTraceCostsApplied", "true")
        .unwrap();
    assert_eq!(p.costs_initialized, Some(true));
}

/// No spelling of one field may also match another field of the same
/// class, or the first-match rule of the setter would decide.
#[test]
fn no_name_matches_two_fields() {
    for field in FIELDS {
        let mut spellings = vec![field.name, field.java_name];
        spellings.extend_from_slice(field.alternates);
        for spelling in spellings {
            let mut p = SettingsPatch::default();
            let path = if field.section.is_empty() {
                spelling.to_string()
            } else {
                format!("{}.{spelling}", field.section)
            };
            // A value valid for every kind except lists is not needed:
            // we only check which field changed.
            let value = match field.kind {
                freerouting::settings::FieldKind::Bool => "true",
                freerouting::settings::FieldKind::RouterScoringVersion => "v1",
                freerouting::settings::FieldKind::OptimizerScoringVersion => "V1_LEGACY",
                freerouting::settings::FieldKind::BoardUpdateStrategy => "GLOBAL_OPTIMAL",
                freerouting::settings::FieldKind::ItemSelectionStrategy => "PRIORITIZED",
                freerouting::settings::FieldKind::Str => "x",
                _ => "1",
            };
            let value = if value == "v1" {
                "V2_CONTINUOUS"
            } else {
                value
            };
            p.set_path(&path, value)
                .unwrap_or_else(|e| panic!("{path}: {e}"));
            let changed: Vec<&str> = FIELDS
                .iter()
                .filter(|f| (f.get)(&p).is_some())
                .map(|f| f.path)
                .collect();
            assert_eq!(changed, vec![field.path], "{path}");
        }
    }
}

#[test]
fn unknown_paths_and_bad_values_are_errors() {
    let mut p = SettingsPatch::default();
    assert!(matches!(
        p.set_path("no_such_field", "1"),
        Err(SetError::UnknownPath(_))
    ));
    assert!(matches!(
        p.set_path("autorouter.no_such", "1"),
        Err(SetError::UnknownPath(name)) if name == "no_such"
    ));
    assert!(matches!(
        p.set_path("autorouter.max_passes.x", "1"),
        Err(SetError::UnknownPath(_))
    ));
    assert!(matches!(
        p.set_path("autorouter.max_passes", "not_a_number"),
        Err(SetError::BadValue(m)) if m == "For input string: \"not_a_number\""
    ));
    // `Integer.parseInt` takes no whitespace and no fraction.
    assert!(p.set_path("autorouter.max_passes", " 5").is_err());
    assert!(p.set_path("autorouter.max_passes", "5.0").is_err());
    // An object is not a value.
    assert!(matches!(
        p.set_path("autorouter", "x"),
        Err(SetError::BadValue(_))
    ));
    assert!(matches!(
        p.set_path("layers", "x"),
        Err(SetError::BadValue(_))
    ));
    // An enum that has no such constant.
    assert!(matches!(
        p.set_path("router_scoring.version", "V9"),
        Err(SetError::BadValue(_))
    ));
    assert_eq!(p, SettingsPatch::default(), "failures set nothing");
}

#[test]
fn values_convert_like_convert_value() {
    let mut p = SettingsPatch::default();
    // Booleans: "0" and "1", otherwise only `true` in any case.
    for (text, want) in [
        ("0", false),
        ("1", true),
        ("TRUE", true),
        ("yes", false),
        ("2", false),
    ] {
        p.set_path("strict_drc", text).unwrap();
        assert_eq!(p.strict_drc, Some(want), "{text}");
    }
    // Doubles: whitespace trimmed, d/f suffix accepted.
    for (text, want) in [
        (" 1.5 ", 1.5),
        ("2.5d", 2.5),
        ("3f", 3.0),
        ("4D", 4.0),
        (".5", 0.5),
    ] {
        p.set_path("neck_width_um", text).unwrap();
        assert_eq!(p.neck_width_um, Some(want), "{text}");
    }
    assert!(p.set_path("neck_width_um", "inf").is_err());
    assert!(p.set_path("neck_width_um", "NaN").is_ok());
    // Long and float.
    p.set_path("fanout.max_milliseconds_per_pin", "5000000000")
        .unwrap();
    assert_eq!(p.fanout.max_milliseconds_per_pin, Some(5_000_000_000));
    p.set_path("optimizer.trace_ripup_cost_factor", "0.6f")
        .unwrap();
    assert_eq!(p.optimizer.trace_ripup_cost_factor, Some(0.6_f32));
    // Enums by name, ignoring case.
    p.set_path("optimizer.item_selection_strategy", "prioritized")
        .unwrap();
    assert_eq!(
        (FIELDS
            .iter()
            .find(|f| f.path == "optimizer.item_selection_strategy")
            .unwrap()
            .get)(&p),
        Some(FieldValue::Enum("PRIORITIZED"))
    );
    // Lists: split on commas, trimmed; an empty value is an empty list.
    p.set_path("plane_nets", " GND , VCC ,3V3").unwrap();
    assert_eq!(
        p.plane_nets,
        Some(vec![
            "GND".to_string(),
            "VCC".to_string(),
            "3V3".to_string()
        ])
    );
    p.set_path("plane_nets", "").unwrap();
    assert_eq!(p.plane_nets, Some(Vec::new()));
    p.set_path("scoring.preferred_direction_trace_cost", "1.5, 2.0")
        .unwrap();
    assert_eq!(
        p.scoring.preferred_direction_trace_cost,
        Some(vec![1.5, 2.0])
    );
    // `layers.<field>=a,b`: only the first min(len, n) elements when set.
    let mut q = SettingsPatch::default();
    q.set_layer_count(3);
    q.set_path("layers.bend_cost", "1.5,2.5").unwrap();
    let layers = q.layers.unwrap();
    assert_eq!(layers.len(), 3);
    assert_eq!(layers[0].bend_cost, Some(1.5));
    assert_eq!(layers[1].bend_cost, Some(2.5));
    assert_eq!(layers[2].bend_cost, None);
    let mut q = SettingsPatch::default();
    q.set_layer_count(1);
    q.set_path("layers.routable", "false,true,false").unwrap();
    assert_eq!(
        q.layers.unwrap(),
        vec![LayerSettings {
            routable: Some(false),
            ..Default::default()
        }]
    );
}

// ---- flat aliases and canonical paths ----------------------------------

#[test]
fn canonical_paths_and_deprecation() {
    for flat in [
        "enabled",
        "algorithm",
        "max_passes",
        "max_items",
        "save_intermediate_stages",
        "ignore_net_classes",
    ] {
        assert_eq!(canonical_cli_path(flat), format!("autorouter.{flat}"));
        assert_eq!(
            canonical_cli_path(&format!("router.{}", flat.to_uppercase())),
            format!("autorouter.{flat}")
        );
        assert!(is_deprecated_flat_autorouter_path(flat), "{flat}");
    }
    for flat in [
        "via_costs",
        "plane_via_costs",
        "start_ripup_costs",
        "default_preferred_direction_trace_cost",
        "default_undesired_direction_trace_cost",
    ] {
        assert_eq!(canonical_cli_path(flat), format!("scoring.{flat}"));
        assert!(
            !is_deprecated_flat_autorouter_path(flat),
            "{flat} is not deprecated"
        );
    }
    // `max_threads` stays the flat top-level field; others are lower-cased.
    assert_eq!(canonical_cli_path("max_threads"), "max_threads");
    assert_eq!(
        canonical_cli_path("Optimizer.Max_Threads"),
        "optimizer.max_threads"
    );
    assert_eq!(
        canonical_cli_path("autorouter.max_passes"),
        "autorouter.max_passes"
    );
}

/// The CLI half of `FlatCostSettingsTest`.
#[test]
fn flat_cost_keys_reach_scoring() {
    // flatCliCostKeysReachTheScoringSettings
    let parsed = parse_cli_arguments(&strings(&[
        "--router.via_costs=25",
        "--router.plane_via_costs=3",
        "--router.start_ripup_costs=400",
        "--router.default_preferred_direction_trace_cost=1.5",
        "--router.default_undesired_direction_trace_cost=3.5",
    ]));
    assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
    let s = parsed.patch.scoring;
    assert_eq!(s.via_costs, Some(25));
    assert_eq!(s.plane_via_costs, Some(3));
    assert_eq!(s.start_ripup_costs, Some(400));
    assert_eq!(s.default_preferred_direction_trace_cost, Some(1.5));
    assert_eq!(s.default_undesired_direction_trace_cost, Some(3.5));
    // nestedCliCostKeysStillWork
    assert_eq!(
        cli(&["--router.scoring.via_costs=30"]).scoring.via_costs,
        Some(30)
    );
}

#[test]
fn deprecated_flat_autorouter_keys_warn() {
    let parsed = parse_cli_arguments(&strings(&["--router.max_passes=12"]));
    assert_eq!(parsed.patch.autorouter.max_passes, Some(12));
    assert_eq!(
        parsed.warnings,
        vec![
            "Deprecated settings path 'router.max_passes'; use 'router.autorouter.max_passes' instead. The old path will be removed in a future release."
                .to_string()
        ]
    );
    // `-mp` maps to the flat key and warns the same way; `-mt` is the flat
    // top-level field.
    let parsed = parse_cli_arguments(&strings(&["-mp", "7", "-mt", "3"]));
    assert_eq!(parsed.patch.autorouter.max_passes, Some(7));
    assert_eq!(parsed.patch.max_threads, Some(3));
    assert_eq!(parsed.warnings.len(), 1);
    assert_eq!(parsed.parsed_arguments["router.max_passes"], "7");
}

#[test]
fn unknown_cli_settings_become_warnings_not_values() {
    let parsed = parse_cli_arguments(&strings(&["--router.nonsense=1", "--router.max_passes=x"]));
    assert_eq!(parsed.patch, SettingsPatch::default());
    assert!(
        parsed
            .warnings
            .iter()
            .any(|w| w.starts_with("Failed to apply CLI router setting: router.nonsense:"))
    );
    assert!(
        parsed
            .warnings
            .iter()
            .any(|w| w.contains("router.max_passes: For input string: \"x\""))
    );
    assert!(parsed.parsed_arguments.is_empty());
}

// ---- ScoringVersionSettingsTest ----------------------------------------

#[test]
fn default_settings_use_current_router_and_optimizer_versions() {
    let s = resolve(&[(Source::Defaults, defaults(&ctx()))], &ctx()).settings;
    assert_eq!(s.router_scoring.version, RouterScoringVersion::V2Continuous);
    assert_eq!(
        s.optimizer_scoring.version,
        OptimizerScoringVersion::V2LowerBound
    );
}

#[test]
fn router_and_optimizer_versions_can_be_selected_independently() {
    let p = cli(&[
        "--router-scoring-version=v2",
        "--optimizer-scoring-version=v1",
    ]);
    assert_eq!(
        p.router_scoring.version,
        Some(RouterScoringVersion::V2Continuous)
    );
    assert_eq!(
        p.optimizer_scoring.version,
        Some(OptimizerScoringVersion::V1Legacy)
    );
}

#[test]
fn convenience_version_selects_both() {
    let p = cli(&["--scoring-version=v1"]);
    assert_eq!(
        p.router_scoring.version,
        Some(RouterScoringVersion::V1Legacy)
    );
    assert_eq!(
        p.optimizer_scoring.version,
        Some(OptimizerScoringVersion::V1Legacy)
    );
    // `v2` and `continuous` mean the stage's own V2.
    let p = cli(&["--scoring-version=continuous"]);
    assert_eq!(
        p.router_scoring.version,
        Some(RouterScoringVersion::V2Continuous)
    );
    assert_eq!(
        p.optimizer_scoring.version,
        Some(OptimizerScoringVersion::V2LowerBound)
    );
    let p = cli(&[
        "--router.scoring.version=legacy",
        "--optimizer.scoring.version=lower-bound",
    ]);
    assert_eq!(
        p.router_scoring.version,
        Some(RouterScoringVersion::V1Legacy)
    );
    assert_eq!(
        p.optimizer_scoring.version,
        Some(OptimizerScoringVersion::V2LowerBound)
    );
    let p = cli(&["--scoring-version=V1_LEGACY"]);
    assert_eq!(
        p.router_scoring.version,
        Some(RouterScoringVersion::V1Legacy)
    );
}

#[test]
fn scoring_version_cli_overrides_default_v2_and_keeps_v2_weights() {
    let merged = resolve(
        &[
            (Source::Defaults, defaults(&ctx())),
            (Source::Cli, cli(&["--scoring-version=v1"])),
        ],
        &ctx(),
    )
    .settings;
    assert_eq!(
        merged.router_scoring.version,
        RouterScoringVersion::V1Legacy
    );
    assert_eq!(
        merged.optimizer_scoring.version,
        OptimizerScoringVersion::V1Legacy
    );
    assert!((merged.router_scoring.unrouted_first_half_weight - 1000.0 / 3.0).abs() < 0.001);
    assert!((merged.optimizer_scoring.excess_wire_length_weight - 1000.0).abs() < 0.001);
    assert_eq!(merged.scoring.via_costs, 50);
}

// ---- NeckWidthSettingsTest ---------------------------------------------

#[test]
fn neck_width_cli_flag_reaches_router_settings() {
    let merged = resolve(
        &[(Source::Cli, cli(&["--router.neck_width_um=250"]))],
        &ctx(),
    )
    .settings;
    assert!((merged.neck_width() - 250.0).abs() < 1e-9);
}

#[test]
fn neck_width_default_is_off() {
    let s = resolve(&[(Source::Defaults, defaults(&ctx()))], &ctx()).settings;
    assert_eq!(s.neck_width(), 0.0);
    let mut p = SettingsPatch::default();
    p.neck_width_um = Some(-5.0);
    assert_eq!(
        resolve(&[(Source::Cli, p)], &ctx()).settings.neck_width(),
        0.0
    );
}

// ---- -oit ---------------------------------------------------------------

#[test]
fn improvement_threshold_scales_only_inside_zero_and_one_and_only_on_the_cli() {
    assert_eq!(
        cli(&["-oit", "0.035"]).optimizer.improvement_threshold,
        Some(0.035_f32 * 100.0_f32)
    );
    assert_eq!(
        cli(&["-oit", "4.0"]).optimizer.improvement_threshold,
        Some(4.0)
    );
    assert_eq!(
        cli(&["-oit", "1"]).optimizer.improvement_threshold,
        Some(1.0)
    );
    assert_eq!(
        cli(&["-oit", "0"]).optimizer.improvement_threshold,
        Some(0.0)
    );
    let rounded = cli(&["--router.optimizer.improvement_threshold=0.015"])
        .optimizer
        .improvement_threshold
        .unwrap();
    assert!((rounded - 1.5).abs() < 1e-6);
    // Not on `set_path`, which is what environment variables use.
    let mut p = SettingsPatch::default();
    p.set_path("optimizer.improvement_threshold", "0.035")
        .unwrap();
    assert_eq!(p.optimizer.improvement_threshold, Some(0.035_f32));
}

#[test]
fn legacy_batch_mode_switches_the_router_on() {
    let p = cli(&["-de", "a.dsn", "-do", "a.ses"]);
    assert_eq!(p.autorouter.enabled, Some(true));
    let p = cli(&["-de", "a.dsn"]);
    assert_eq!(p.autorouter.enabled, None);
}
