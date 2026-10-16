//! C02.7: `autoroute_settings` from a DSN or rules file to a patch.
//!
//! Ports `DsnFileSettingsTest` and `RulesFileSettingsTest`. Not ported:
//! `DsnFileSettingsTest.dsnFileSettingsPriorityIs20` and
//! `dsnFileSettingsSourceNameContainsFilename` /
//! `RulesFileSettingsTest.priorityIs40` assert Java source metadata that is
//! `Source::Dsn` / `Source::RulesFile` here (`settings_merge.rs`).

// Patches are built field by field, as the Java tests build `RouterSettings`.
#![allow(clippy::field_reassign_with_default)]

use std::path::{Path, PathBuf};

use freerouting::dsn::ast::{AutorouteLayerRule, AutorouteSettings, Direction, Pcb, RulesFile};
use freerouting::settings::autoroute_scope::{
    layer_number, patch_from_dsn, patch_from_rules, read_scope, rules_layer_names,
};
use freerouting::settings::{
    LayerSettings, ResolveContext, RouterSettings, SettingsPatch, Source, defaults, resolve,
};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

fn load_pcb(name: &str) -> Pcb {
    let bytes = std::fs::read(fixture(name)).unwrap();
    Pcb::parse(&freerouting::dsn::decode(&bytes)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn load_rules(name: &str) -> RulesFile {
    let bytes = std::fs::read(fixture(name)).unwrap();
    RulesFile::parse(&freerouting::dsn::decode(&bytes)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

/// A patch resolved without defaults, read through Java's getters.
fn view(patch: SettingsPatch) -> RouterSettings {
    resolve(&[(Source::Dsn, patch)], &ResolveContext { cores: 8 }).settings
}

// ---- DsnFileSettingsTest -----------------------------------------------

#[test]
fn dsn_file_settings_reads_2_layer_count_from_2_layer_board_without_autoroute_block() {
    let patch = patch_from_dsn(&load_pcb("Issue413-test.dsn"));
    assert_eq!(patch.layer_count(), 2);
}

#[test]
fn dsn_file_settings_reads_4_layer_count_from_4_layer_board_without_autoroute_block() {
    let patch = patch_from_dsn(&load_pcb("Issue066-Project_GP8B.dsn"));
    assert_eq!(patch.layer_count(), 4);
}

/// A DSN *without* the scope still yields the seeded patch, with only the
/// layer count.
#[test]
fn dsn_without_the_scope_yields_only_the_seed() {
    let patch = patch_from_dsn(&load_pcb("Issue066-Project_GP8B.dsn"));
    let mut seeded = SettingsPatch::default();
    seeded.set_layer_count(4);
    assert_eq!(patch, seeded);
    assert_eq!(patch.costs_initialized, Some(false));
    assert_eq!(
        patch.scoring.preferred_direction_trace_cost,
        Some(vec![1.0; 4])
    );
    assert_eq!(
        patch.scoring.undesired_direction_trace_cost,
        Some(vec![1.0; 4])
    );
    assert!(patch.layers.as_ref().unwrap().iter().all(|l| *l
        == LayerSettings {
            routable: Some(true),
            ..Default::default()
        }));
    assert_eq!(
        patch.set_paths(),
        vec![
            "scoring.preferred_direction_trace_cost",
            "scoring.undesired_direction_trace_cost",
            "layers",
            "costs_initialized"
        ]
    );
    // Not even the run flags: they come only from the scope.
    assert_eq!(patch.autorouter.enabled, None);
    assert_eq!(patch.optimizer.enabled, None);
}

#[test]
fn every_fixture_dsn_gives_a_patch_with_its_layer_count() {
    let mut checked = 0;
    for entry in std::fs::read_dir(fixture("")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("dsn") {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        let Ok(pcb) = Pcb::parse(&freerouting::dsn::decode(&bytes)) else {
            continue; // the fixture Java rejects too
        };
        let layers = pcb.structure.as_ref().map_or(0, |s| s.layers.len());
        let patch = patch_from_dsn(&pcb);
        assert_eq!(patch.layer_count(), layers, "{}", path.display());
        checked += 1;
    }
    assert!(checked > 100, "{checked} DSN files checked");
}

// ---- the three DSN fixtures that carry the scope -----------------------

fn assert_layer(
    settings: &RouterSettings,
    layer: usize,
    horizontal: bool,
    pref: f64,
    against: f64,
) {
    assert!(settings.layer_active(layer));
    assert_eq!(
        settings.preferred_direction_is_horizontal(layer),
        horizontal,
        "layer {layer}"
    );
    assert_eq!(
        settings.preferred_direction_trace_costs(layer),
        pref,
        "layer {layer}"
    );
    assert_eq!(
        settings.against_preferred_direction_trace_costs(layer),
        against,
        "layer {layer}"
    );
}

#[test]
fn dsn_fixtures_with_the_scope() {
    // Issue187 and Issue191's DSN carry the same scope.
    for name in [
        "Issue187-processor.Z80.dsn",
        "Issue191-processor.Z80/processor.Z80.dsn",
    ] {
        let patch = patch_from_dsn(&load_pcb(name));
        assert_eq!(patch.costs_initialized, Some(true), "{name}");
        assert_eq!(patch.allowed_via_types, Some(true));
        assert_eq!(
            (
                patch.scoring.via_costs,
                patch.scoring.plane_via_costs,
                patch.scoring.start_ripup_costs
            ),
            (Some(50), Some(5), Some(100))
        );
        assert_eq!(patch.autorouter.enabled, Some(true));
        assert_eq!(patch.optimizer.enabled, Some(true));
        assert_eq!(
            patch.scoring.preferred_direction_trace_cost,
            Some(vec![1.0, 1.0])
        );
        assert_eq!(
            patch.scoring.undesired_direction_trace_cost,
            Some(vec![2.5, 1.7])
        );
        let layers = patch.layers.clone().unwrap();
        assert_eq!(
            layers[0],
            LayerSettings {
                routable: Some(true),
                preferred_direction_horizontal: Some(true),
                bend_cost: None,
                preferred_direction_trace_cost: Some(1.0),
                undesired_direction_trace_cost: Some(2.5),
            }
        );
        let s = view(patch);
        assert_layer(&s, 0, true, 1.0, 2.5);
        assert_layer(&s, 1, false, 1.0, 1.7);
    }
    // Issue103: `(eu.mihosoft.freerouting.autoroute on)` is not `autoroute`
    // (the router stays on by default) and `(postroute off)` turns the
    // optimizer off.
    let patch = patch_from_dsn(&load_pcb("Issue103-Board-Routed.dsn"));
    assert_eq!(patch.autorouter.enabled, Some(true));
    assert_eq!(patch.optimizer.enabled, Some(false));
    assert_eq!(
        patch.scoring.undesired_direction_trace_cost,
        Some(vec![2.1, 1.9])
    );
    let s = view(patch);
    assert_layer(&s, 0, true, 1.0, 2.1);
    assert_layer(&s, 1, false, 1.0, 1.9);
}

// ---- RulesFileSettingsTest ---------------------------------------------

#[test]
fn parses_autoroute_settings_from_processor_rules() {
    let s = view(patch_from_rules(&load_rules(
        "Issue191-processor.Z80/processor.rules",
    )));
    assert!(s.allowed_via_types);
    assert_eq!(s.scoring.via_costs, 50);
    assert_eq!(s.scoring.plane_via_costs, 5);
    assert_eq!(s.scoring.start_ripup_costs, 100);
    assert!(s.run_router());
    assert!(s.run_optimizer());
    assert_eq!(s.layer_count(), 2);
    assert!(s.layer_active(0));
    assert!(s.layer_active(1));
    // Layer 0: F.Cu horizontal, cost 1.0 / 2.5
    assert!(s.preferred_direction_is_horizontal(0));
    assert_eq!(s.preferred_direction_trace_costs(0), 1.0);
    assert_eq!(s.against_preferred_direction_trace_costs(0), 2.5);
    // Layer 1: B.Cu vertical, cost 1.0 / 1.7
    assert!(!s.preferred_direction_is_horizontal(1));
    assert_eq!(s.preferred_direction_trace_costs(1), 1.0);
    assert_eq!(s.against_preferred_direction_trace_costs(1), 1.7);
}

#[test]
fn parses_autoroute_settings_from_hw48na_rules() {
    let s = view(patch_from_rules(&load_rules("Issue029-hw48na_valid.rules")));
    assert!(s.allowed_via_types);
    assert_eq!(s.scoring.via_costs, 50);
    assert_eq!(s.scoring.plane_via_costs, 5);
    assert_eq!(s.scoring.start_ripup_costs, 100);
    assert_eq!(s.layer_count(), 2);
    // Layer 0: F.Cu vertical, cost 1.0 / 2.0
    assert!(!s.preferred_direction_is_horizontal(0));
    assert_eq!(s.preferred_direction_trace_costs(0), 1.0);
    assert_eq!(s.against_preferred_direction_trace_costs(0), 2.0);
    // Layer 1: B.Cu horizontal, cost 1.0 / 2.0
    assert!(s.preferred_direction_is_horizontal(1));
    assert_eq!(s.preferred_direction_trace_costs(1), 1.0);
    assert_eq!(s.against_preferred_direction_trace_costs(1), 2.0);
}

#[test]
fn every_rules_fixture_reads() {
    let mut with_scope = 0;
    for dir in [fixture(""), fixture("Issue191-processor.Z80")] {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) != Some("rules") {
                continue;
            }
            let bytes = std::fs::read(&path).unwrap();
            let Ok(rules) = RulesFile::parse(&freerouting::dsn::decode(&bytes)) else {
                continue;
            };
            let patch = patch_from_rules(&rules);
            let has_scope = rules
                .items
                .iter()
                .any(|i| matches!(i, freerouting::dsn::ast::RulesItem::AutorouteSettings(_)));
            if has_scope {
                with_scope += 1;
                assert_eq!(
                    patch.layer_count(),
                    rules_layer_names(&rules).len(),
                    "{}",
                    path.display()
                );
                assert!(patch.autorouter.enabled.is_some());
            } else {
                assert_eq!(patch, SettingsPatch::default(), "{}", path.display());
            }
        }
    }
    assert!(with_scope >= 2);
}

// ---- the scope itself --------------------------------------------------

fn rule(layer: &str) -> AutorouteLayerRule {
    AutorouteLayerRule {
        layer: layer.to_string(),
        active: None,
        preferred_direction: None,
        preferred_direction_trace_costs: None,
        against_preferred_direction_trace_costs: None,
    }
}

#[test]
fn costs_are_clamped_and_the_run_flags_default_to_on() {
    let scope = AutorouteSettings {
        via_costs: Some(0),
        plane_via_costs: Some(-3),
        start_ripup_costs: Some(7),
        vias: Some(false),
        layer_rules: vec![AutorouteLayerRule {
            active: Some(false),
            preferred_direction: Some(Direction::Vertical),
            preferred_direction_trace_costs: Some(0.05),
            against_preferred_direction_trace_costs: Some(-1.0),
            ..rule("B.Cu")
        }],
        ..AutorouteSettings::default()
    };
    let patch = read_scope(&scope, &names(&["F.Cu", "B.Cu"])).unwrap();
    assert_eq!(patch.scoring.via_costs, Some(1));
    assert_eq!(patch.scoring.plane_via_costs, Some(1));
    assert_eq!(patch.scoring.start_ripup_costs, Some(7));
    assert_eq!(patch.allowed_via_types, Some(false));
    assert_eq!(patch.autorouter.enabled, Some(true));
    assert_eq!(patch.optimizer.enabled, Some(true));
    let layers = patch.layers.unwrap();
    assert_eq!(
        layers[0],
        LayerSettings {
            routable: Some(true),
            ..Default::default()
        }
    );
    assert_eq!(layers[1].routable, Some(false));
    assert_eq!(layers[1].preferred_direction_horizontal, Some(false));
    assert_eq!(layers[1].preferred_direction_trace_cost, Some(0.1));
    assert_eq!(layers[1].undesired_direction_trace_cost, Some(0.1));
    assert_eq!(
        patch.scoring.preferred_direction_trace_cost,
        Some(vec![1.0, 0.1])
    );
    assert_eq!(patch.costs_initialized, Some(true));
}

#[test]
fn a_layer_rule_without_costs_keeps_costs_uninitialised() {
    let scope = AutorouteSettings {
        autoroute: Some(false),
        postroute: Some(false),
        layer_rules: vec![AutorouteLayerRule {
            active: Some(true),
            ..rule("F.Cu")
        }],
        ..AutorouteSettings::default()
    };
    let patch = read_scope(&scope, &names(&["F.Cu"])).unwrap();
    assert_eq!(patch.costs_initialized, Some(false));
    assert_eq!(patch.autorouter.enabled, Some(false));
    assert_eq!(patch.optimizer.enabled, Some(false));
}

#[test]
fn an_unknown_layer_aborts_the_whole_scope() {
    let scope = AutorouteSettings {
        via_costs: Some(9),
        layer_rules: vec![
            AutorouteLayerRule {
                preferred_direction_trace_costs: Some(3.0),
                ..rule("F.Cu")
            },
            rule("In7.Cu"),
        ],
        ..AutorouteSettings::default()
    };
    assert_eq!(read_scope(&scope, &names(&["F.Cu", "B.Cu"])), None);
    // A DSN with such a scope is the seeded patch, nothing else.
    let pcb = Pcb {
        structure: Some(freerouting::dsn::ast::Structure {
            layers: ["F.Cu", "B.Cu"]
                .iter()
                .map(|n| freerouting::dsn::ast::Layer {
                    name: n.to_string(),
                    layer_type: None,
                    rules: vec![],
                    use_net: vec![],
                })
                .collect(),
            autoroute_settings: Some(scope),
            ..Default::default()
        }),
        ..Pcb::default()
    };
    let mut seeded = SettingsPatch::default();
    seeded.set_layer_count(2);
    assert_eq!(patch_from_dsn(&pcb), seeded);
}

#[test]
fn layer_lookup_is_exact_then_top_and_bottom() {
    let layers = names(&["F.Cu", "In1.Cu", "B.Cu"]);
    assert_eq!(layer_number(&layers, "In1.Cu"), Some(1));
    assert_eq!(layer_number(&layers, "Top"), Some(0));
    assert_eq!(layer_number(&layers, "Electra Top"), Some(0));
    assert_eq!(layer_number(&layers, "Bottom"), Some(2));
    assert_eq!(
        layer_number(&layers, "f.cu"),
        None,
        "names are case-sensitive"
    );
    assert_eq!(layer_number(&layers, "Inner"), None);
}

#[test]
fn rules_layer_names_come_from_layer_and_layer_rule_in_file_order() {
    let rules = RulesFile::parse(
        "(rules PCB x (autoroute_settings (layer_rule In2.Cu (active on)) (layer_rule F.Cu (active on))) (layer B.Cu (rule (width 200))) (layer F.Cu (rule (width 300))))",
    )
    .unwrap();
    assert_eq!(
        rules_layer_names(&rules),
        names(&["In2.Cu", "F.Cu", "B.Cu"])
    );
    let patch = patch_from_rules(&rules);
    assert_eq!(patch.layer_count(), 3);
    // `In2.Cu` is layer 0, `F.Cu` layer 1 in this file's own structure.
    assert_eq!(patch.layers.unwrap()[1].routable, Some(true));
    // Without any layer name the structure is F.Cu and B.Cu.
    let empty = RulesFile::parse("(rules PCB x (rule (width 200)))").unwrap();
    assert_eq!(rules_layer_names(&empty), names(&["F.Cu", "B.Cu"]));
    assert_eq!(patch_from_rules(&empty), SettingsPatch::default());
}

#[test]
fn dsn_patch_merges_under_the_cli_and_over_the_defaults() {
    let ctx = ResolveContext { cores: 8 };
    let dsn = patch_from_dsn(&load_pcb("Issue187-processor.Z80.dsn"));
    let mut cli = SettingsPatch::default();
    cli.scoring.via_costs = Some(77);
    let resolved = resolve(
        &[
            (Source::Defaults, defaults(&ctx)),
            (Source::Dsn, dsn),
            (Source::Cli, cli),
        ],
        &ctx,
    );
    assert_eq!(resolved.settings.scoring.via_costs, 77);
    assert_eq!(resolved.settings.scoring.plane_via_costs, 5);
    assert_eq!(resolved.settings.layer_count(), 2);
    assert!(resolved.patch.costs_initialized == Some(true));
    assert_eq!(resolved.provenance.get("layers"), Some(&Source::Dsn));
}
