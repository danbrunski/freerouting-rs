//! N01.4: the stage catalog lists every registered stage id, the profile's
//! choice for each slot and the slot's options with the profile's defaults.

use std::collections::BTreeSet;

use freerouting::autoroute::stages::catalog::{SLOTS, SlotArity, catalog, registered_ids};
use freerouting::autoroute::stages::{DEFAULT_FANOUT, FREEROUTING_BUNDLE, StageRegistry};
use freerouting::settings::profile::{Difference, with_enhanced_differences};
use freerouting::settings::{FieldValue, Profile, field_by_path};

const CORES: usize = 8;

/// The ids listed for the registry map `kind`.
fn listed(registry: &StageRegistry, kind: &str) -> BTreeSet<&'static str> {
    let board_stage_slots = ["board_stages", "length_matching", "diff_pairs"];
    catalog(registry, Profile::Enhanced, CORES)
        .into_iter()
        .filter(|slot| {
            slot.desc.kind == kind
                || (kind == "board_stages" && board_stage_slots.contains(&slot.desc.kind))
        })
        .flat_map(|slot| slot.ids)
        .collect()
}

fn assert_lists_every_id(registry: &StageRegistry) {
    for (kind, ids) in registered_ids(registry) {
        let listed = listed(registry, kind);
        for id in ids {
            assert!(listed.contains(id), "{kind}: {id} is not listed");
        }
    }
}

#[test]
fn every_registered_stage_id_is_listed() {
    let registry = StageRegistry::builtin();
    assert_lists_every_id(&registry);
    // Every registry map has a slot.
    let kinds: BTreeSet<&str> = SLOTS.iter().map(|s| s.kind).collect();
    for (kind, _) in registered_ids(&registry) {
        assert!(kinds.contains(kind), "{kind} has no slot");
    }
}

#[test]
fn an_added_stage_is_listed() {
    let mut registry = StageRegistry::builtin();
    registry.bundles.insert("x-bundle", FREEROUTING_BUNDLE);
    let fanout = registry.fanout[DEFAULT_FANOUT];
    registry.fanout.insert("x-fanout", fanout);
    let ordering = registry.ordering["freerouting-ordering"];
    registry.ordering.insert("x-ordering", ordering);
    let router = registry.connection_router["freerouting-connection"];
    registry.connection_router.insert("x-router", router);
    let stage = registry.board_stages["freerouting-unrouted-report"];
    registry.board_stages.insert("x-stage", stage);
    assert_lists_every_id(&registry);
    assert!(listed(&registry, "connection_router").contains("x-router"));
}

#[test]
fn slots_name_settings_fields() {
    for slot in SLOTS {
        assert!(
            field_by_path(slot.key).is_some(),
            "{} is no settings path",
            slot.key
        );
        for option in slot.options {
            assert!(
                field_by_path(option).is_some(),
                "{}: {option} is no settings path",
                slot.key
            );
        }
    }
}

#[test]
fn defaults_follow_the_profile() {
    let registry = StageRegistry::builtin();
    let legacy = catalog(&registry, Profile::Legacy, CORES);
    let slot = |kind: &str| {
        legacy
            .iter()
            .find(|s| s.desc.kind == kind)
            .expect("a slot")
            .clone()
    };
    assert_eq!(slot("algorithm").default, vec!["freerouting-router"]);
    assert_eq!(
        slot("connection_router").default,
        vec!["freerouting-connection"]
    );
    assert_eq!(slot("fanout").default, vec![DEFAULT_FANOUT]);
    assert_eq!(slot("optimizer").default, vec!["freerouting-optimizer"]);
    assert!(slot("diagnostics").default.is_empty());
    assert_eq!(slot("diagnostics").desc.arity, SlotArity::List);
    let passes = slot("fanout")
        .options
        .into_iter()
        .find(|o| o.path == "fanout.max_passes")
        .expect("the option");
    assert_eq!(passes.default, Some(FieldValue::Int(20)));
    // `enhanced` lists the same apart from its table's keys that are stage
    // options: the neck width (N05.3; `power_layer_traces` is no option).
    let mut expected = legacy.clone();
    for slot in &mut expected {
        for option in &mut slot.options {
            if option.path == "neck_width_um" {
                assert_eq!(option.default, Some(FieldValue::Double(0.0)));
                option.default = Some(FieldValue::Double(300.0));
            }
        }
    }
    assert_ne!(expected, legacy);
    assert_eq!(catalog(&registry, Profile::Enhanced, CORES), expected);
    // A difference of `enhanced` shows in its listing only.
    const DUMMY: &[Difference] = &[Difference {
        path: "fanout.max_passes",
        value: "7",
        measurement: "test-only dummy (N01.4)",
    }];
    with_enhanced_differences(DUMMY, || {
        let enhanced = catalog(&registry, Profile::Enhanced, CORES);
        let fanout = enhanced
            .iter()
            .find(|s| s.desc.kind == "fanout")
            .expect("fanout");
        let passes = fanout
            .options
            .iter()
            .find(|o| o.path == "fanout.max_passes");
        assert_eq!(
            passes.and_then(|o| o.default.clone()),
            Some(FieldValue::Int(7))
        );
        assert_eq!(catalog(&registry, Profile::Legacy, CORES), legacy);
    });
}

#[test]
fn experimental_options_are_real_listed_marked_and_unset_in_every_profile() {
    use freerouting::autoroute::stages::catalog::{EXPERIMENTAL, ExperimentalKind};
    let registry = StageRegistry::builtin();
    let mut seen = BTreeSet::new();
    for e in EXPERIMENTAL {
        assert!(seen.insert((e.slot, e.key)), "{} listed twice", e.key);
        assert!(
            !e.class.is_empty() && e.help.len() > 40,
            "{}: no help",
            e.key
        );
        assert!(
            SLOTS.iter().any(|s| s.kind == e.slot),
            "{}: no slot {}",
            e.key,
            e.slot
        );
        if e.kind != ExperimentalKind::Option {
            continue;
        }
        assert!(field_by_path(e.key).is_some(), "{} is no field", e.key);
        for profile in [Profile::Legacy, Profile::Enhanced] {
            let slots = catalog(&registry, profile, CORES);
            let slot = slots.iter().find(|s| s.desc.kind == e.slot).unwrap();
            let option = slot
                .options
                .iter()
                .find(|o| o.path == e.key)
                .unwrap_or_else(|| panic!("{} is not an option of {}", e.key, e.slot));
            assert_eq!(option.experimental, Some(e), "{} is marked", e.key);
            assert_eq!(
                option.default,
                None,
                "{}: experimental means unset in {}",
                e.key,
                profile.name()
            );
        }
    }
    // The N02.4 sort: these four options are the in-tree opt-ins.
    for key in [
        "via_clearance",
        "cleanup.remove_stubs",
        "room_order",
        "work_unit",
    ] {
        assert!(seen.iter().any(|(_, k)| *k == key), "{key} is experimental");
    }
    // Nothing else is marked.
    for slot in catalog(&registry, Profile::Enhanced, CORES) {
        for option in &slot.options {
            assert_eq!(
                option.experimental.is_some(),
                EXPERIMENTAL
                    .iter()
                    .any(|e| e.kind == ExperimentalKind::Option && e.key == option.path),
                "{}",
                option.path
            );
        }
    }
}
