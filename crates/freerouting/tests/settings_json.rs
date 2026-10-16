//! C02.4: JSON settings files and Dan's `freerouting.json`.
//!
//! Ports `JsonFileSettingsTest`, the JSON half of `FlatCostSettingsTest`
//! and the deserialize half of `RouterSettingsSerializationTest`.
//!
//! Not ported: `RouterSettingsSerializationTest.layersFieldRemainsTransient`
//! asserts a Java modifier, and the serialize half writes Java settings
//! (the Rust writer is the comparison dump, tested in
//! `settings_reference.rs`). `JsonFileSettingsTest.priorityIs10` and
//! `sourceNameIsFreeroutingJson` are `Source::ConfigFile`'s priority and
//! name, asserted in `settings_merge.rs::source_priorities`.

// Patches are built field by field, as the Java tests build `RouterSettings`.
#![allow(clippy::field_reassign_with_default)]

use std::path::{Path, PathBuf};

use freerouting::settings::json_file::{
    ConfigError, load_config_file, patch_from_config_text, patch_from_json_text,
};
use freerouting::settings::{
    FIELDS, FieldValue, LayerSettings, OptimizerScoringVersion, ResolveContext,
    RouterScoringVersion, SettingsPatch, Source, defaults, resolve,
};

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/settings")
        .join(name)
}

fn temp_file(name: &str, content: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("settings_json");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    path
}

fn config(text: &str) -> SettingsPatch {
    patch_from_config_text(text).expect("valid settings").patch
}

fn ctx() -> ResolveContext {
    ResolveContext { cores: 8 }
}

// ---- JsonFileSettingsTest ----------------------------------------------

#[test]
fn loads_max_passes_from_valid_router_section() {
    let p = config(r#"{ "router": { "max_passes": 42 } }"#);
    assert_eq!(p.autorouter.max_passes, Some(42));
}

#[test]
fn loads_max_passes_from_nested_autorouter_section() {
    let p = config(r#"{ "router": { "autorouter": { "max_passes": 7 } } }"#);
    assert_eq!(p.autorouter.max_passes, Some(7));
}

#[test]
fn returns_empty_settings_when_router_section_is_missing() {
    let p = config(r#"{ "gui": { "enabled": true } }"#);
    assert_eq!(p, SettingsPatch::default());
}

#[test]
fn returns_empty_settings_when_router_section_is_not_an_object() {
    let p = config(r#"{ "router": "not-an-object" }"#);
    assert_eq!(p, SettingsPatch::default());
}

#[test]
fn returns_empty_settings_for_missing_file() {
    let loaded = load_config_file(&data("does_not_exist.json"), false).unwrap();
    assert_eq!(loaded.patch, SettingsPatch::default());
    assert!(loaded.warnings.is_empty());
}

#[test]
fn returns_empty_settings_with_a_warning_for_malformed_json() {
    let path = temp_file("malformed.json", "{ this is not valid json !! }");
    let loaded = load_config_file(&path, false).unwrap();
    assert_eq!(loaded.patch, SettingsPatch::default());
    assert_eq!(loaded.warnings.len(), 1);
    assert!(loaded.warnings[0].starts_with("Failed to parse JSON settings file:"));
    // A document that is not an object is the same case.
    let path = temp_file("array.json", "[1, 2]");
    assert_eq!(load_config_file(&path, false).unwrap().warnings.len(), 1);
}

/// `--settings FILE` is stricter: a missing or malformed file is an error
/// naming the path.
#[test]
fn explicit_settings_file_errors_name_the_path() {
    let missing = data("does_not_exist.json");
    let err = load_config_file(&missing, true).unwrap_err();
    assert!(matches!(err, ConfigError::Missing(_)));
    assert!(err.to_string().contains("does_not_exist.json"), "{err}");
    let path = temp_file("explicit-malformed.json", "{ nope");
    let err = load_config_file(&path, true).unwrap_err();
    assert!(matches!(err, ConfigError::Malformed(..)));
    assert!(err.to_string().contains("explicit-malformed.json"), "{err}");
    // A valid explicit file loads.
    let ok = load_config_file(&data("owner_freerouting.json"), true).unwrap();
    assert_eq!(ok.patch.scoring.start_ripup_costs, Some(1500));
}

// ---- FlatCostSettingsTest (JSON half) ----------------------------------

#[test]
fn flat_cost_keys_reach_scoring() {
    // flatJsonCostKeysReachTheScoringSettings
    let p = patch_from_json_text(
        r#"{"via_costs": 25, "plane_via_costs": 3, "start_ripup_costs": 400}"#,
    )
    .unwrap()
    .patch;
    assert_eq!(p.scoring.via_costs, Some(25));
    assert_eq!(p.scoring.plane_via_costs, Some(3));
    assert_eq!(p.scoring.start_ripup_costs, Some(400));
    let p = patch_from_json_text(
        r#"{"default_preferred_direction_trace_cost": 1.5, "default_undesired_direction_trace_cost": "3.5"}"#,
    )
    .unwrap()
    .patch;
    assert_eq!(p.scoring.default_preferred_direction_trace_cost, Some(1.5));
    assert_eq!(p.scoring.default_undesired_direction_trace_cost, Some(3.5));
}

#[test]
fn nested_json_cost_keys_win_over_flat_ones() {
    let p = patch_from_json_text(r#"{"via_costs": 25, "scoring": {"via_costs": 30}}"#)
        .unwrap()
        .patch;
    assert_eq!(p.scoring.via_costs, Some(30));
}

#[test]
fn flat_autorouter_keys_apply_only_where_nested_is_unset_and_warn_once() {
    let loaded = patch_from_json_text(
        r#"{"max_passes": 42, "enabled": false, "ignore_net_classes": ["A", null, "B"],
            "autorouter": {"max_passes": 5}}"#,
    )
    .unwrap();
    assert_eq!(loaded.patch.autorouter.max_passes, Some(5), "nested wins");
    assert_eq!(loaded.patch.autorouter.enabled, Some(false));
    assert_eq!(
        loaded.patch.autorouter.ignore_net_classes,
        Some(vec!["A".to_string(), "B".to_string()])
    );
    assert_eq!(loaded.warnings.len(), 1);
    assert!(loaded.warnings[0].starts_with(
        "Deprecated settings path 'router.max_passes (and other flat autorouter keys)'"
    ));
    // The cost keys alone do not warn.
    assert!(
        patch_from_json_text(r#"{"via_costs": 1}"#)
            .unwrap()
            .warnings
            .is_empty()
    );
}

// ---- RouterSettingsSerializationTest (deserialize half) ----------------

#[test]
fn router_settings_deserialization() {
    // A document that Gson wrote for settings with layers.
    let p = patch_from_json_text(
        r#"{
            "autorouter": { "max_passes": 42 },
            "layers": [
              {"routable": false, "preferred_direction_horizontal": true},
              {"routable": true, "preferred_direction_horizontal": false}
            ]
        }"#,
    )
    .unwrap()
    .patch;
    assert_eq!(p.autorouter.max_passes, Some(42));
    let layers = p.layers.unwrap();
    assert_eq!(layers.len(), 2);
    assert_eq!(layers[0], LayerSettings::new(false, true));
    assert_eq!(layers[1], LayerSettings::new(true, false));

    // The API payload with the flat key.
    let p = patch_from_json_text(
        r#"{
          "max_passes": 42,
          "layers": [
            {"routable": false, "preferred_direction_horizontal": true},
            {"routable": true, "preferred_direction_horizontal": false}
          ]
        }"#,
    )
    .unwrap()
    .patch;
    assert_eq!(p.autorouter.max_passes, Some(42));
    let layers = p.layers.unwrap();
    assert_eq!(layers[0].routable, Some(false));
    assert_eq!(layers[0].preferred_direction_horizontal, Some(true));
    assert_eq!(layers[1].routable, Some(true));
    assert_eq!(layers[1].preferred_direction_horizontal, Some(false));

    // No `layers` key: unset.
    assert_eq!(patch_from_json_text("{}").unwrap().patch.layers, None);
}

// ---- Gson specifics ----------------------------------------------------

#[test]
fn every_section_file() {
    let text = std::fs::read_to_string(data("every_section.json")).unwrap();
    let loaded = patch_from_config_text(&text).unwrap();
    assert!(loaded.warnings.is_empty());
    let p = loaded.patch;
    assert_eq!(p.job_timeout.as_deref(), Some("00:10:00"));
    // A number in a string is accepted, a whole double is an int.
    assert_eq!(p.trace_pull_tight_accuracy, Some(250));
    assert_eq!(p.optimizer.max_passes, Some(42));
    assert_eq!(p.allowed_via_types, Some(false));
    // Alternates match exactly.
    assert_eq!(p.plane_as_obstacle, Some(true));
    assert_eq!(p.fanout.ripup_allowed, Some(false));
    assert_eq!(p.scoring.via_costs, Some(75));
    assert_eq!(
        p.plane_nets,
        Some(vec!["GND".to_string(), "VCC".to_string()])
    );
    assert_eq!(p.copper_to_edge_clearance_um, Some(100.0));
    assert_eq!(p.neck_width_um, Some(130.5));
    assert_eq!(p.max_threads, Some(3));
    assert_eq!(
        p.autorouter.ignore_net_classes,
        Some(vec!["POWER".to_string()])
    );
    assert_eq!(p.fanout.max_milliseconds_per_pin, Some(2500));
    assert_eq!(p.fanout.timeout.as_deref(), Some("30"));
    assert_eq!(p.optimizer.improvement_threshold, Some(1.25));
    assert_eq!(p.scoring.default_bend_cost, Some(0.5));
    // Enums by exact name; an unknown name is null.
    assert_eq!(
        p.router_scoring.version,
        Some(RouterScoringVersion::V1Legacy)
    );
    assert_eq!(p.optimizer_scoring.version, None);
    assert_eq!(p.router_scoring.unrouted_free_fraction, Some(0.25));
    assert_eq!(p.optimizer_scoring.excess_via_weight, Some(1.0));
    // Java's transient fields are not read.
    assert_eq!(p.optimizer.board_update_strategy, None);
    assert_eq!(p.scoring.preferred_direction_trace_cost, None);
    // `layers`, with a null element read as an empty layer.
    let layers = p.layers.unwrap();
    assert_eq!(layers.len(), 3);
    assert_eq!(layers[0].bend_cost, Some(1.5));
    assert_eq!(layers[1], LayerSettings::default());
    assert_eq!(layers[2].routable, Some(false));
    assert_eq!(layers[2].undesired_direction_trace_cost, Some(4.0));
}

#[test]
fn names_match_exactly_without_case_folding() {
    let p = patch_from_json_text(
        r#"{"Max_Passes": 9, "autorouter": {"MAX_PASSES": 8}, "AUTOROUTER": {"max_passes": 6}}"#,
    )
    .unwrap()
    .patch;
    assert_eq!(p, SettingsPatch::default());
    // The CLI does fold case, the JSON reader does not; the Java field name
    // is not a JSON name either.
    let p = patch_from_json_text(r#"{"viasAllowed": false}"#)
        .unwrap()
        .patch;
    assert_eq!(p.allowed_via_types, None);
}

#[test]
fn a_type_error_anywhere_fails_the_whole_read() {
    for bad in [
        r#"{"autorouter": {"max_passes": 4.5}}"#,
        r#"{"autorouter": {"max_passes": true}}"#,
        r#"{"autorouter": {"enabled": 1}}"#,
        r#"{"autorouter": 3}"#,
        r#"{"plane_nets": "GND"}"#,
        r#"{"layers": {}}"#,
        r#"{"scoring": {"via_costs": "x"}}"#,
        r#"{"autorouter": {"max_passes": 1}, "optimizer": {"max_passes": "many"}}"#,
    ] {
        assert!(patch_from_json_text(bad).is_err(), "{bad}");
    }
    // In a config file that gives empty settings and a warning, not a crash.
    let path = temp_file(
        "type-error.json",
        r#"{"router": {"autorouter": {"max_passes": 4.5}}}"#,
    );
    let loaded = load_config_file(&path, false).unwrap();
    assert_eq!(loaded.patch, SettingsPatch::default());
    assert_eq!(loaded.warnings.len(), 1);
}

#[test]
fn json_null_leaves_a_field_unset_and_comments_are_allowed() {
    let p = config(
        r#"{ // a comment
            "router": { "autorouter": { "max_passes": null, "enabled": true }, /* more */ "layers": null }
        }"#,
    );
    assert_eq!(p.autorouter.max_passes, None);
    assert_eq!(p.autorouter.enabled, Some(true));
    assert_eq!(p.layers, None);
}

// ---- Dan's freerouting.json --------------------------------------------

#[test]
fn owner_settings_file_sets_start_ripup_costs() {
    let loaded = load_config_file(&data("owner_freerouting.json"), true).unwrap();
    assert!(loaded.warnings.is_empty());
    let ctx = ctx();
    let resolved = resolve(
        &[
            (Source::Defaults, defaults(&ctx)),
            (Source::ConfigFile, loaded.patch),
        ],
        &ctx,
    );
    assert_eq!(resolved.settings.scoring.start_ripup_costs, 1500);
    assert_eq!(
        resolved.provenance.get("scoring.start_ripup_costs"),
        Some(&Source::ConfigFile)
    );
    // Every other field is at its default.
    let default = defaults(&ctx);
    for field in FIELDS
        .iter()
        .filter(|f| f.path != "scoring.start_ripup_costs")
    {
        assert_eq!(
            (field.get)(&resolved.patch),
            (field.get)(&default),
            "{}",
            field.path
        );
        let from_defaults = (field.get)(&default).is_some().then_some(&Source::Defaults);
        assert_eq!(
            resolved.provenance.get(field.path),
            from_defaults,
            "{}",
            field.path
        );
    }
    assert_eq!(resolved.patch.layers, None);
    assert_eq!(
        (FIELDS
            .iter()
            .find(|f| f.path == "scoring.start_ripup_costs")
            .unwrap()
            .get)(&resolved.patch),
        Some(FieldValue::Int(1500))
    );
    assert_eq!(
        resolved.settings.optimizer_scoring.version,
        OptimizerScoringVersion::V2LowerBound
    );
}
