//! K14: the plugin's DSN fix-ups on the `.kicad_pcb` reader's tree.
//!
//! `tests/data/k14-fixups.kicad_pcb` is made by `tools/kicad-reader/k14_fixture.py` with
//! KiCad 10.0.7's pcbnew, which also records KiCad's export of it
//! (`k14-fixups.kicad10.dsn`) and that export after the plugin's `dsn_fixups.apply` with
//! `class_widths` (Power 0.6 mm, Switch 0.8 mm), `drop_pour_nets` (GND) and
//! `route_on_plane_layers` (In2.Cu) on (`k14-fixups.fixed.dsn`). The reader's tree with
//! the same fix-ups ([`freerouting_kicad::fixups`]) must equal the fixed-up export, parsed,
//! in the parts the fix-ups touch. The engine half (the settings, a DSN input) is at the
//! end.

mod common;

use common::{copper, parity};
use freerouting_dsn::Pcb;
use freerouting_dsn::ast::{LayerRef, LayerType, Rule};
use freerouting_kicad::fixups::{self, FixupError, InputFixups};
use freerouting_kicad::{KicadRead, ReadOptions, read_with};

const BOARD: &str = include_str!("data/k14-fixups.kicad_pcb");
const PROJECT: &str = include_str!("data/k14-fixups.kicad_pro");
const EXPORT: &str = include_str!("data/k14-fixups.kicad10.dsn");
const FIXED: &str = include_str!("data/k14-fixups.fixed.dsn");
// K14.4: `tools/kicad-reader/k14_fixed_pours.py` (no pcbnew) applies `dsn_fixups.apply`
// to the export above with the plugin's board facts `k14-fixups.board.json`.
const FIXED_POURS: &str = include_str!("data/k14-fixups.fixed-pours.dsn");
const FIXED_POURS_ONLY: &str = include_str!("data/k14-fixups.fixed-pours-only.dsn");
const FIXED_POURS_DEFAULT: &str = include_str!("data/k14-fixups.fixed-pours-default.dsn");
const BOARD_FACTS: &str = include_str!("data/k14-fixups.board.json");

fn read() -> KicadRead {
    read_with("board.dsn", BOARD, Some(PROJECT), ReadOptions::default()).unwrap()
}

fn parse(text: &str) -> Pcb {
    Pcb::parse(text).unwrap()
}

/// The plugin's values for the fixture, as `options.input_fixup_args` passes them.
fn all_three() -> InputFixups {
    InputFixups {
        class_widths: vec![("Power".into(), 600.0), ("Switch".into(), 800.0)],
        route_on_plane_layers: vec!["In2.Cu".into()],
        drop_pour_nets: vec!["GND".into()],
        fixed_pour_classes: Vec::new(),
    }
}

fn fixed_tree(fixups: &InputFixups) -> (Pcb, Vec<String>) {
    let mut pcb = read().pcb;
    let log = fixups::apply(&mut pcb, fixups).unwrap();
    (pcb, log)
}

fn class_rules(p: &Pcb) -> Vec<(String, Vec<Rule>)> {
    let mut out: Vec<_> = p
        .network
        .as_ref()
        .unwrap()
        .classes
        .iter()
        .map(|c| (c.name.clone(), c.rules.clone()))
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn layer_types(p: &Pcb) -> Vec<(String, Option<LayerType>)> {
    p.structure
        .as_ref()
        .unwrap()
        .layers
        .iter()
        .map(|l| (l.name.clone(), l.layer_type.clone()))
        .collect()
}

fn plane_list(p: &Pcb) -> Vec<(String, String)> {
    p.structure
        .as_ref()
        .unwrap()
        .planes
        .iter()
        .map(|pl| {
            let layer = match pl.area.shape.layer() {
                LayerRef::Named(n) => n.clone(),
                other => format!("{other:?}"),
            };
            (pl.net.clone(), layer)
        })
        .collect()
}

#[test]
fn the_unfixed_tree_equals_the_export_in_classes_layers_and_planes() {
    let (r, e) = (read().pcb, parse(EXPORT));
    assert_eq!(class_rules(&r), class_rules(&e));
    assert_eq!(layer_types(&r), layer_types(&e));
    assert_eq!(plane_list(&r), plane_list(&e));
    assert!(parity::classes(&r, &e).is_empty());
    assert!(copper::planes(&r, &e).is_empty());
    // the fixture exercises all three: the fixed-up export differs in each part
    let f = parse(FIXED);
    assert_ne!(class_rules(&e), class_rules(&f));
    assert_ne!(layer_types(&e), layer_types(&f));
    assert_ne!(plane_list(&e), plane_list(&f));
}

#[test]
fn class_widths_equal_the_fixed_up_export() {
    let fixups = InputFixups {
        class_widths: all_three().class_widths,
        ..InputFixups::default()
    };
    let (r, log) = fixed_tree(&fixups);
    let f = parse(FIXED);
    assert_eq!(class_rules(&r), class_rules(&f));
    assert!(
        parity::classes(&r, &f).is_empty(),
        "{:?}",
        parity::classes(&r, &f)
    );
    assert_eq!(
        log,
        [
            "class Power: width 1000 -> 600 um",
            "class Switch: width 1500 -> 800 um"
        ]
    );
    // nothing else moved
    let e = parse(EXPORT);
    assert_eq!(layer_types(&r), layer_types(&e));
    assert_eq!(plane_list(&r), plane_list(&e));
}

#[test]
fn opened_plane_layers_equal_the_fixed_up_export_and_keep_their_planes() {
    let fixups = InputFixups {
        route_on_plane_layers: vec!["In2.Cu".into()],
        ..InputFixups::default()
    };
    let (r, log) = fixed_tree(&fixups);
    let (e, f) = (parse(EXPORT), parse(FIXED));
    assert_eq!(layer_types(&r), layer_types(&f));
    assert!(parity::layers(&r, &f).is_empty());
    assert_eq!(
        layer_types(&r)[2],
        ("In2.Cu".to_owned(), Some(LayerType::Signal))
    );
    // In1.Cu stays a power layer, and the planes of In2.Cu stay planes
    assert_eq!(
        layer_types(&r)[1],
        ("In1.Cu".to_owned(), Some(LayerType::Power))
    );
    assert_eq!(plane_list(&r), plane_list(&e));
    assert!(copper::planes(&r, &e).is_empty());
    assert_eq!(log, ["layer In2.Cu: power -> signal"]);
}

#[test]
fn dropped_pours_equal_the_fixed_up_export_and_inner_planes_stay() {
    let fixups = InputFixups {
        drop_pour_nets: vec!["GND".into()],
        ..InputFixups::default()
    };
    let (r, log) = fixed_tree(&fixups);
    let f = parse(FIXED);
    assert_eq!(plane_list(&r), plane_list(&f));
    assert!(
        copper::planes(&r, &f).is_empty(),
        "{:?}",
        copper::planes(&r, &f)
    );
    let planes = plane_list(&r);
    assert!(planes.contains(&("GND".to_owned(), "In1.Cu".to_owned())));
    assert!(!planes.iter().any(|(n, l)| n == "GND" && l != "In1.Cu"));
    assert_eq!(log, ["pours: 2 outer plane(s) removed (dropped nets: GND)"]);
}

#[test]
fn all_three_together_equal_the_fixed_up_export() {
    let (r, log) = fixed_tree(&all_three());
    let f = parse(FIXED);
    assert_eq!(class_rules(&r), class_rules(&f));
    assert_eq!(layer_types(&r), layer_types(&f));
    assert_eq!(plane_list(&r), plane_list(&f));
    assert!(copper::planes(&r, &f).is_empty());
    assert_eq!(log.len(), 4);
}

fn err(fixups: InputFixups) -> FixupError {
    let mut pcb = read().pcb;
    let before = pcb.clone();
    let e = fixups::apply(&mut pcb, &fixups).unwrap_err();
    assert_eq!(pcb, before, "an error leaves the tree as it was");
    e
}

#[test]
fn an_unknown_class_is_an_error_that_names_it() {
    let e = err(InputFixups {
        class_widths: vec![("Power".into(), 600.0), ("Nope".into(), 300.0)],
        ..InputFixups::default()
    });
    assert_eq!(e.setting, "router.input_fixups.class_widths");
    assert!(e.to_string().contains("Nope"), "{e}");
}

#[test]
fn an_unknown_layer_or_a_signal_layer_is_an_error_that_names_it() {
    let unknown = err(InputFixups {
        route_on_plane_layers: vec!["In7.Cu".into()],
        ..InputFixups::default()
    });
    assert_eq!(unknown.setting, "router.input_fixups.route_on_plane_layers");
    assert!(unknown.message.contains("In7.Cu"), "{unknown}");
    let signal = err(InputFixups {
        route_on_plane_layers: vec!["F.Cu".into()],
        ..InputFixups::default()
    });
    assert!(signal.message.contains("F.Cu") && signal.message.contains("not a plane"));
}

/// The DSN fix-up passes an unknown net silently; the setting warns and goes on without it
/// (decided 2026-10-21: a run never fails just because of the input choice).
#[test]
fn an_unknown_net_is_a_warning_that_names_it() {
    let (r, log) = fixed_tree(&InputFixups {
        drop_pour_nets: vec!["GND".into(), "NOPE".into()],
        ..InputFixups::default()
    });
    assert_eq!(
        log,
        [
            "WARNING: router.input_fixups.drop_pour_nets: the board has no net NOPE; left out",
            "pours: 2 outer plane(s) removed (dropped nets: GND)"
        ]
    );
    assert_eq!(plane_list(&r), plane_list(&parse(FIXED)));
    let (r, log) = fixed_tree(&InputFixups {
        drop_pour_nets: vec!["NOPE".into()],
        ..InputFixups::default()
    });
    assert_eq!(
        log,
        ["WARNING: router.input_fixups.drop_pour_nets: the board has no net NOPE; left out"]
    );
    assert_eq!(plane_list(&r), plane_list(&parse(EXPORT)));
}

/// A `.kicad_dru` with an applied `track_width` rule for Power (0.9 mm).
const POWER_RULE: &str = "(version 1)\n\
(rule \"power tracks\"\n\
\t(constraint track_width (opt 0.9mm))\n\
\t(condition \"A.NetClass == 'Power'\"))\n";

/// The board's `.kicad_dru` `track_width` wins on both inputs. The DSN path is KiCad's
/// export, then the text fix-up, then `--kicad-dru` (`dru::apply_to_pcb`), so the rule sets
/// the width last; the reader applies the rules first, so `class_widths` skips the classes
/// the rule set, with one line naming the class and the rule, and the result is the same.
#[test]
fn a_class_the_kicad_dru_track_width_sets_keeps_the_rules_width() {
    let options = ReadOptions {
        apply_design_rules: true,
        ..ReadOptions::default()
    };
    let read = freerouting_kicad::read_with_rules(
        "board.dsn",
        BOARD,
        Some(PROJECT),
        Some(POWER_RULE),
        options,
    )
    .unwrap();
    assert_eq!(
        read.rules.track_widths,
        [("Power".to_owned(), "power tracks".to_owned())]
    );
    let mut tree = read.pcb;
    let fixups = InputFixups {
        class_widths: all_three().class_widths,
        ..InputFixups::default()
    };
    let log = fixups::apply_with_rules(&mut tree, &fixups, &read.rules.track_widths).unwrap();
    // the DSN path: the fixed-up export, then the rules
    let mut dsn = parse(FIXED);
    freerouting_kicad::dru::apply_to_pcb(&mut dsn, POWER_RULE).unwrap();
    assert_eq!(class_rules(&tree), class_rules(&dsn));
    let width = |p: &Pcb, class: &str| {
        class_rules(p)
            .into_iter()
            .find(|(n, _)| n == class)
            .and_then(|(_, rules)| {
                rules.into_iter().find_map(|r| match r {
                    Rule::Width(w) => Some(w),
                    _ => None,
                })
            })
    };
    assert_eq!(width(&tree, "Power"), Some(900.0));
    // a class the rule does not select still takes `class_widths`
    assert_eq!(width(&tree, "Switch"), Some(800.0));
    assert_eq!(
        log,
        [
            "class Power: width 900 um kept, set by the .kicad_dru rule 'power tracks' \
             (class_widths skipped)",
            "class Switch: width 1500 -> 800 um"
        ]
    );
}

#[test]
fn the_setting_lists_parse_and_bad_entries_are_errors() {
    let f = InputFixups::from_lists(
        &["Power=600".into(), " Switch = 800.5 ".into(), String::new()],
        &["In2.Cu".into()],
        &["GND".into()],
    )
    .unwrap();
    assert_eq!(
        f.class_widths,
        [("Power".to_owned(), 600.0), ("Switch".to_owned(), 800.5)]
    );
    assert!(!f.is_empty());
    assert!(InputFixups::from_lists(&[], &[], &[]).unwrap().is_empty());
    for bad in ["Power", "Power=", "=600", "Power=-1", "Power=wide"] {
        let e = InputFixups::from_lists(&[bad.into()], &[], &[]).unwrap_err();
        assert_eq!(e.setting, "router.input_fixups.class_widths");
        assert!(e.message.contains(bad), "{e}");
    }
}

// ---- K14.4: pours of listed classes as fixed wires ------------------------------------

fn wiring(p: &Pcb) -> Vec<freerouting_dsn::ast::WiringItem> {
    p.wiring
        .as_ref()
        .map(|w| w.items.clone())
        .unwrap_or_default()
}

fn with_fixed_pours(base: InputFixups, classes: &[&str]) -> InputFixups {
    let classes: Vec<String> = classes.iter().map(|c| c.to_string()).collect();
    base.with_fixed_pour_classes(&classes)
}

fn fixed_pour_tree(fixups: &InputFixups) -> (Pcb, Vec<String>) {
    let mut pcb = read().pcb;
    let fills = fixups::pour_fills(BOARD).unwrap();
    let log = fixups::apply_with_fills(&mut pcb, fixups, &[], &fills).unwrap();
    (pcb, log)
}

/// The wire integers in the order `_wire` writes them, for messages.
fn wire_numbers(items: &[freerouting_dsn::ast::WiringItem]) -> Vec<(String, String, Vec<f64>)> {
    use freerouting_dsn::ast::{Shape, WiringItem};
    items
        .iter()
        .filter_map(|i| match i {
            WiringItem::Wire(w) => Some(w),
            _ => None,
        })
        .map(|w| match &w.shapes[..] {
            [
                Shape::Path {
                    layer: LayerRef::Named(l),
                    width,
                    points,
                },
            ] => (
                w.net.as_ref().map(|n| n.name.clone()).unwrap_or_default(),
                l.clone(),
                std::iter::once(*width)
                    .chain(points.iter().flat_map(|p| [p.x, p.y]))
                    .collect(),
            ),
            other => panic!("not one path: {other:?}"),
        })
        .collect()
}

/// The reader's fill of each outer pour is the plugin's (`kicad_board._zones`, pcbnew's
/// `GetFilledPolysList` after the refill), point for point: the same `f64` millimetres.
#[test]
fn the_fills_read_from_the_file_equal_the_plugins_board_facts() {
    let facts: serde_json::Value = serde_json::from_str(BOARD_FACTS).unwrap();
    let outer: Vec<_> = facts["zones"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|z| z["outer"].as_bool().unwrap())
        .collect();
    let fills = fixups::pour_fills(BOARD).unwrap();
    assert_eq!(fills.len(), outer.len());
    assert_eq!(fills.len(), 3, "GND F.Cu, VBUS F.Cu, GND B.Cu");
    for (fill, z) in fills.iter().zip(&outer) {
        assert_eq!(fill.net, z["net"].as_str().unwrap());
        assert_eq!(fill.layer, z["layer"].as_str().unwrap());
        assert_eq!(fill.filled, z["filled"].as_bool().unwrap());
        let chains: Vec<Vec<(f64, f64)>> = z["fill"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                c.as_array()
                    .unwrap()
                    .iter()
                    .map(|p| (p[0].as_f64().unwrap(), p[1].as_f64().unwrap()))
                    .collect()
            })
            .collect();
        assert_eq!(fill.chains, chains, "{} on {}", fill.net, fill.layer);
    }
}

/// All four fix-ups: the tree's wiring (count, net, layer, width and coordinates, in
/// order: exact, the wires' integers come from `um`), planes, classes and layer types
/// equal the plugin's fixed-up export, parsed.
#[test]
fn fixed_pours_equal_the_fixed_up_export() {
    let (r, log) = fixed_pour_tree(&with_fixed_pours(all_three(), &["Power"]));
    let f = parse(FIXED_POURS);
    assert_eq!(wire_numbers(&wiring(&r)).len(), 44);
    assert_eq!(wire_numbers(&wiring(&r)), wire_numbers(&wiring(&f)));
    assert_eq!(wiring(&r), wiring(&f));
    assert_eq!(plane_list(&r), plane_list(&f));
    assert!(copper::planes(&r, &f).is_empty());
    assert_eq!(class_rules(&r), class_rules(&f));
    assert_eq!(layer_types(&r), layer_types(&f));
    assert!(
        log.iter()
            .any(|l| l == "pours: 44 fixed wires for 1 pour(s) of Power; 1 outer plane(s) removed"),
        "{log:?}"
    );
    // VBUS lost its F.Cu plane and keeps In2.Cu's; GND lost both outer ones
    assert_eq!(
        plane_list(&r),
        [
            ("GND".to_owned(), "In1.Cu".to_owned()),
            ("VBUS".to_owned(), "In2.Cu".to_owned()),
            ("V3V3".to_owned(), "In2.Cu".to_owned())
        ]
    );
}

#[test]
fn fixed_pours_alone_equal_the_fixed_up_export() {
    let (r, _) = fixed_pour_tree(&with_fixed_pours(InputFixups::default(), &["Power"]));
    let f = parse(FIXED_POURS_ONLY);
    assert_eq!(wiring(&r), wiring(&f));
    assert_eq!(plane_list(&r), plane_list(&f));
    assert!(copper::planes(&r, &f).is_empty());
    // the rest of the tree is the export's
    let e = parse(EXPORT);
    assert_eq!(class_rules(&r), class_rules(&e));
    assert_eq!(layer_types(&r), layer_types(&e));
}

/// Three pours on two layers: the plugin's zone order (GND F.Cu, VBUS F.Cu, GND B.Cu).
#[test]
fn several_fixed_pours_come_in_the_plugins_zone_order() {
    let fixups = with_fixed_pours(InputFixups::default(), &["kicad_default", "Power"]);
    let (r, log) = fixed_pour_tree(&fixups);
    let f = parse(FIXED_POURS_DEFAULT);
    assert_eq!(wire_numbers(&wiring(&r)).len(), 317);
    assert_eq!(wiring(&r), wiring(&f));
    assert_eq!(plane_list(&r), plane_list(&f));
    assert!(
        log.iter().any(|l| l
            == "pours: 317 fixed wires for 3 pour(s) of Power, kicad_default; 3 outer plane(s) \
                removed"),
        "{log:?}"
    );
}

/// `drop_pour_nets` wins over `fixed_pour_classes`: GND (kicad_default) is dropped, not
/// covered, as `dsn_fixups.apply` skips a dropped net before it looks at the class.
#[test]
fn a_dropped_net_is_not_covered_by_fixed_wires() {
    let both = InputFixups {
        drop_pour_nets: vec!["GND".into()],
        ..InputFixups::default()
    };
    let (r, _) = fixed_pour_tree(&with_fixed_pours(both, &["kicad_default", "Power"]));
    let f = parse(FIXED_POURS);
    assert_eq!(wiring(&r), wiring(&f));
    assert_eq!(plane_list(&r), plane_list(&f));
}

#[test]
fn an_unknown_fixed_pour_class_is_an_error_that_names_it() {
    let mut pcb = read().pcb;
    let fills = fixups::pour_fills(BOARD).unwrap();
    let fixups = with_fixed_pours(InputFixups::default(), &["Power", "Nope"]);
    let e = fixups::apply_with_fills(&mut pcb, &fixups, &[], &fills).unwrap_err();
    assert_eq!(e.setting, "router.input_fixups.fixed_pour_classes");
    assert!(e.message.contains("Nope"), "{e}");
    assert_eq!(pcb, read().pcb, "nothing changed");
    // without the fills the setting cannot apply
    let e = fixups::apply(
        &mut pcb,
        &with_fixed_pours(InputFixups::default(), &["Power"]),
    );
    assert_eq!(
        e.unwrap_err().setting,
        "router.input_fixups.fixed_pour_classes"
    );
}

/// A zone `(fill ...)` without `yes` is not filled (`ZONE::IsFilled`): its pour keeps
/// its plane and gives no wires, whatever `filled_polygon`s it has.
#[test]
fn an_unfilled_pour_keeps_its_plane() {
    let marker = "(fill yes\n\t\t\t(thermal_gap 0.5)";
    let at = BOARD.find("(net \"VBUS\")\n\t\t(layer \"F.Cu\")").unwrap();
    let rel = BOARD[at..].find(marker).unwrap();
    let text = format!(
        "{}(fill{}",
        &BOARD[..at + rel],
        &BOARD[at + rel + "(fill yes".len()..]
    );
    let fills = fixups::pour_fills(&text).unwrap();
    assert!(
        !fills[1].filled && fills[1].chains.is_empty(),
        "{:?}",
        fills[1].net
    );
    let mut pcb = read().pcb;
    let fixups = with_fixed_pours(InputFixups::default(), &["Power"]);
    let log = fixups::apply_with_fills(&mut pcb, &fixups, &[], &fills).unwrap();
    assert!(wiring(&pcb).is_empty());
    assert_eq!(plane_list(&pcb), plane_list(&parse(EXPORT)));
    assert_eq!(
        log,
        ["pours: 0 fixed wires for 0 pour(s) of Power; 0 outer plane(s) removed"]
    );
}

// ---- the engine: `router.input_fixups.*` in `prepare_job` ------------------------------

mod engine {
    use std::path::PathBuf;

    use freerouting::job::prepare::{JobError, JobInput, RulesChoice, prepare_job};
    use freerouting::settings::{ResolveContext, SettingsPatch};

    fn board_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/k14-fixups.kicad_pcb")
    }

    fn cli(layers: &[&str], classes: &[&str], nets: &[&str]) -> SettingsPatch {
        let list = |v: &[&str]| Some(v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        let mut patch = SettingsPatch::default();
        patch.input_fixups.route_on_plane_layers = list(layers);
        patch.input_fixups.class_widths = list(classes);
        patch.input_fixups.drop_pour_nets = list(nets);
        patch
    }

    fn kicad_job(patch: SettingsPatch) -> JobInput {
        let read =
            freerouting::io::kicad::read_kicad_input(&board_path(), Default::default()).unwrap();
        let mut input = JobInput::new(Vec::new(), "k14-fixups");
        input.rules = RulesChoice::None;
        input.set_kicad(read);
        input.cli = Some(patch);
        input
    }

    fn dsn_job(patch: SettingsPatch) -> JobInput {
        let mut input = JobInput::new(super::EXPORT.as_bytes().to_vec(), "k14-fixups");
        input.rules = RulesChoice::None;
        input.cli = Some(patch);
        input
    }

    const CTX: ResolveContext = ResolveContext { cores: 1 };

    fn log_of(job: &freerouting::job::prepare::PreparedJob) -> Vec<String> {
        job.log.iter().map(|l| l.text.clone()).collect()
    }

    #[test]
    fn a_kicad_pcb_job_applies_the_settings_to_the_tree() {
        let patch = cli(&["In2.Cu"], &["Power=600", "Switch=800"], &["GND"]);
        let job = prepare_job(&kicad_job(patch), &CTX).unwrap();
        let log = log_of(&job);
        for line in [
            "input fix-up: class Power: width 1000 -> 600 um",
            "input fix-up: class Switch: width 1500 -> 800 um",
            "input fix-up: pours: 2 outer plane(s) removed (dropped nets: GND)",
            "input fix-up: layer In2.Cu: power -> signal",
        ] {
            assert!(log.iter().any(|l| l == line), "{line} not in {log:?}");
        }
        let layers = &job.board.layer_structure.layers;
        assert_eq!(
            layers
                .iter()
                .map(|l| (l.name.as_str(), l.is_signal))
                .collect::<Vec<_>>(),
            [
                ("F.Cu", true),
                ("In1.Cu", false),
                ("In2.Cu", true),
                ("B.Cu", true)
            ]
        );
    }

    /// Off by default under both profiles.
    #[test]
    fn without_the_settings_a_kicad_pcb_job_is_unchanged() {
        for profile in [None, Some("legacy"), Some("enhanced")] {
            let patch = SettingsPatch {
                profile: profile.map(str::to_owned),
                ..SettingsPatch::default()
            };
            let job = prepare_job(&kicad_job(patch), &CTX).unwrap();
            assert!(!log_of(&job).iter().any(|l| l.contains("input fix-up")));
            assert!(!job.board.layer_structure.layers[2].is_signal);
            let fixups = &job.patch.input_fixups;
            assert_eq!(
                (
                    &fixups.class_widths,
                    &fixups.route_on_plane_layers,
                    &fixups.drop_pour_nets,
                    &fixups.fixed_pour_classes
                ),
                (&None, &None, &None, &None),
                "{profile:?}"
            );
        }
    }

    #[test]
    fn a_fixup_the_board_cannot_take_fails_the_job_naming_it() {
        for (patch, name) in [
            (cli(&[], &["Nope=300"], &[]), "Nope"),
            (cli(&["F.Cu"], &[], &[]), "F.Cu"),
            (cli(&["In9.Cu"], &[], &[]), "In9.Cu"),
            (cli(&[], &["Power"], &[]), "Power"),
        ] {
            match prepare_job(&kicad_job(patch), &CTX) {
                Err(JobError::Settings(m)) => {
                    assert!(
                        m.starts_with("router.input_fixups.") && m.contains(name),
                        "{m}"
                    )
                }
                other => panic!("{name}: {:?}", other.map(|_| ())),
            }
        }
    }

    #[test]
    fn an_unknown_net_warns_and_the_job_goes_on() {
        let job = prepare_job(&kicad_job(cli(&[], &[], &["NOPE", "GND"])), &CTX).unwrap();
        let log = log_of(&job);
        assert!(log.iter().any(|l| l
            == "input fix-up: WARNING: router.input_fixups.drop_pour_nets: the board has no net \
                NOPE; left out"), "{log:?}");
        assert!(
            log.iter()
                .any(|l| l.ends_with("2 outer plane(s) removed (dropped nets: GND)"))
        );
    }

    /// The `.kicad_dru` beside the board sets Power's width; the job keeps it.
    #[test]
    fn a_kicad_pcb_job_keeps_the_kicad_dru_track_width() {
        let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("k14-dru");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for ext in ["kicad_pcb", "kicad_pro"] {
            std::fs::copy(
                board_path().with_extension(ext),
                dir.join(format!("k14-fixups.{ext}")),
            )
            .unwrap();
        }
        std::fs::write(dir.join("k14-fixups.kicad_dru"), super::POWER_RULE).unwrap();
        let options = freerouting::io::kicad::ReadOptions {
            apply_design_rules: true,
            ..Default::default()
        };
        let read =
            freerouting::io::kicad::read_kicad_input(&dir.join("k14-fixups.kicad_pcb"), options)
                .unwrap();
        let mut input = JobInput::new(Vec::new(), "k14-fixups");
        input.rules = RulesChoice::None;
        input.set_kicad(read);
        input.cli = Some(cli(&[], &["Power=600", "Switch=800"], &[]));
        let log = log_of(&prepare_job(&input, &CTX).unwrap());
        assert!(log.iter().any(|l| l
            == "input fix-up: class Power: width 900 um kept, set by the .kicad_dru rule 'power \
                tracks' (class_widths skipped)"), "{log:?}");
        assert!(
            log.iter()
                .any(|l| l == "input fix-up: class Switch: width 1500 -> 800 um")
        );
    }

    fn with_fixed_pours(mut patch: SettingsPatch, classes: &[&str]) -> SettingsPatch {
        patch.input_fixups.fixed_pour_classes =
            Some(classes.iter().map(|s| s.to_string()).collect());
        patch
    }

    /// K14.4: the fixed wires reach the board as `SYSTEM_FIXED` traces of VBUS on F.Cu,
    /// and neither the session nor the `.kicad_pcb` output holds them (the DSN path's
    /// property: `ImportSpecctraSES` never sees them).
    #[test]
    fn the_fixed_pour_wires_are_on_the_board_and_in_no_output() {
        use freerouting::board::structure::FixedState;
        use freerouting::io::specctra::{SesStyle, write_ses};
        let read =
            freerouting::io::kicad::read_kicad_input(&board_path(), Default::default()).unwrap();
        let hash = read.source_hash;
        let mut input = JobInput::new(Vec::new(), "k14-fixups");
        input.rules = RulesChoice::None;
        input.set_kicad(read);
        let plain = prepare_job(&input, &CTX).unwrap();
        input.cli = Some(with_fixed_pours(SettingsPatch::default(), &["Power"]));
        let job = prepare_job(&input, &CTX).unwrap();
        assert!(log_of(&job).iter().any(|l| l
            == "input fix-up: pours: 44 fixed wires for 1 pour(s) of Power; 1 outer plane(s) \
                removed"));
        let vbus = job
            .board
            .rules
            .nets
            .iter()
            .find(|n| n.name == "VBUS")
            .unwrap()
            .net_number;
        let fixed: Vec<_> = job
            .board
            .items()
            .filter(|i| i.as_trace().is_some() && i.net_numbers() == [vbus])
            .collect();
        assert!(!fixed.is_empty());
        assert!(fixed.iter().all(
            |i| i.fixed_state() == FixedState::SystemFixed && i.as_trace().unwrap().layer == 0
        ));
        // the board's own copper is the same with and without the wires
        let traces =
            |b: &freerouting::board::RoutingBoard| b.items().filter(|i| i.is_trace()).count();
        assert_eq!(traces(&plain.board), 0);
        let ses = write_ses(&job.board, "k14-fixups", SesStyle::Java);
        assert!(!ses.contains("(wire"), "{ses}");
        assert_eq!(ses, write_ses(&plain.board, "k14-fixups", SesStyle::Java));
        let (text, _) =
            freerouting::io::kicad::render_kicad_board(&board_path(), &job.board, Some(hash))
                .unwrap();
        assert!(!text.contains("(segment"), "a fill wire became a track");
        assert_eq!(text, super::BOARD, "nothing to write back");
    }

    #[test]
    fn an_unknown_fixed_pour_class_fails_the_job_naming_it() {
        let patch = with_fixed_pours(SettingsPatch::default(), &["Nope"]);
        match prepare_job(&kicad_job(patch), &CTX) {
            Err(JobError::Settings(m)) => assert_eq!(
                m,
                "router.input_fixups.fixed_pour_classes: the board has no net class Nope"
            ),
            other => panic!("{:?}", other.map(|_| ())),
        }
    }

    /// A DSN input has no reader's tree: the settings are ignored with one warning, and
    /// the board is KiCad's export as it is (In2.Cu stays a plane layer), even for names
    /// the board does not have.
    #[test]
    fn a_dsn_job_ignores_the_settings_with_a_warning() {
        let patch = with_fixed_pours(
            cli(&["In2.Cu", "In9.Cu"], &["Nope=300"], &["GND"]),
            &["Power", "Nope"],
        );
        let job = prepare_job(&dsn_job(patch), &CTX).unwrap();
        let warnings: Vec<_> = log_of(&job)
            .into_iter()
            .filter(|l| l.contains("router.input_fixups"))
            .collect();
        assert_eq!(
            warnings,
            [
                "router.input_fixups.class_widths, router.input_fixups.route_on_plane_layers, \
                 router.input_fixups.drop_pour_nets, router.input_fixups.fixed_pour_classes only \
                 apply to a .kicad_pcb input; ignored for this DSN input"
            ]
        );
        assert!(!job.board.layer_structure.layers[2].is_signal);
        assert_eq!(job.board.items().filter(|i| i.is_trace()).count(), 0);
    }
}
