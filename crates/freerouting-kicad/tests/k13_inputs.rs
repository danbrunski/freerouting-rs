//! K13.1: what the `.kicad_pcb` reader carries that the plugin's DSN path
//! carries too, pinned where no earlier test named it.
//!
//! - Mask expansions are filtered at the smallest class clearance per side,
//!   as the plugin's `kicad_board.mask_expansions` does
//!   (`kicad-plugin/tests/test_mask_floor.py` has the same pads): a side
//!   whose expansion is at or under the floor is 0, and a pad with no side
//!   left is not listed.
//! - The `.kicad_dru` item-type rules (Q22.5) reach the reader's tree with
//!   `apply_design_rules`, which a `.kicad_pcb` route turns on by default
//!   (the CLI's `cli_kicad.rs::a_kicad_pcb_route_applies_the_item_type_and_hole_rules_by_default`).

use freerouting_dsn::ast::{Pcb, Rule};
use freerouting_kicad::mask::{PadMaskExpansion, keep_above_floor};
use freerouting_kicad::{ReadOptions, read, read_with_rules};

mod common;

const MASK_FLOOR: &str = include_str!("data/k13-mask-floor.kicad_pcb");

fn pad(component: &str, front_nm: i32, back_nm: i32) -> PadMaskExpansion {
    PadMaskExpansion {
        component: component.to_owned(),
        pin: "1".to_owned(),
        front_nm,
        back_nm,
    }
}

/// No project file: the Default class's 0.2 mm is the floor. F1 (0.3 mm,
/// front) and B1 (0.3 mm, back) are listed; F2 (at the floor) and T3
/// (0.1 mm) are not; T1 (0.25 mm on both sides) keeps both; T2 opens on the
/// front only.
#[test]
fn mask_expansions_are_filtered_per_side_at_the_smallest_class_clearance() {
    let board = read(MASK_FLOOR, None).unwrap();
    assert_eq!(
        board.extras.solder_mask,
        vec![
            pad("F1", 300_000, 0),
            pad("T1", 250_000, 250_000),
            pad("T2", 300_000, 0),
            pad("B1", 0, 300_000),
        ]
    );
}

/// The rule itself: each side is judged alone, at or under the floor is 0
/// (the plugin's per-layer expansions can differ by side; the reader's
/// cannot, so this is where the per-side rule shows).
#[test]
fn a_side_at_or_under_the_floor_is_zeroed_and_the_other_kept() {
    let list = vec![
        pad("U1", 300_000, 100_000),
        pad("U2", 200_000, 200_001),
        pad("U3", 200_000, 0),
        pad("U4", 0, 0),
    ];
    assert_eq!(
        keep_above_floor(list, 200_000),
        vec![pad("U1", 300_000, 0), pad("U2", 0, 200_001)]
    );
}

fn structure_typed(pcb: &Pcb, ty: &str) -> Option<f64> {
    pcb.structure.as_ref()?.rules.iter().find_map(|r| match r {
        Rule::Clearance { value, types } if types.iter().any(|t| t == ty) => Some(*value),
        _ => None,
    })
}

/// A `.kicad_dru` rule between vias and tracks becomes the reader's
/// `via_wire` clearance with `apply_design_rules`, and is reported as ignored
/// without it.
#[test]
fn the_kicad_pcb_reader_applies_item_type_rules() {
    let dir = common::fixtures().join("synthetic");
    let board = std::fs::read_to_string(dir.join("classes.kicad_pcb")).unwrap();
    let pro = std::fs::read_to_string(dir.join("classes.kicad_pro")).unwrap();
    let dru = "(version 1)\n(rule \"via to track\"\n\t(constraint clearance (min 0.4mm))\n\t\
               (condition \"A.Type == 'via' && B.Type == 'track'\"))\n";
    let read = |apply: bool| {
        let options = ReadOptions {
            apply_design_rules: apply,
            ..ReadOptions::default()
        };
        read_with_rules("classes.dsn", &board, Some(&pro), Some(dru), options).unwrap()
    };
    assert_eq!(structure_typed(&read(true).pcb, "via_wire"), Some(400.0));
    let off = read(false);
    assert_eq!(structure_typed(&off.pcb, "via_wire"), None);
    assert!(
        format!("{:?}", off.rules.rules[0].constraints[0].status)
            .contains("apply_design_rules is off")
    );
}

const PAD_CLEARANCE: &str = include_str!("data/k13-pad-clearance.kicad_pcb");

/// The pad-clearance circles: (layer, diameter µm, centre µm in the DSN frame).
fn pad_circles(pcb: &Pcb) -> Vec<(String, f64, (f64, f64))> {
    use freerouting_dsn::ast::{LayerRef, Shape};
    let name = freerouting_kicad::pad_clearance::KEEPOUT_NAME;
    pcb.structure
        .as_ref()
        .unwrap()
        .keepouts
        .iter()
        .filter(|k| k.area.name.as_deref() == Some(name))
        .map(|k| match &k.area.shape {
            Shape::Circle {
                layer: LayerRef::Named(layer),
                diameter,
                center,
            } => (layer.clone(), *diameter, (center.x, center.y)),
            other => panic!("not a circle: {other:?}"),
        })
        .collect()
}

/// A pad's own clearance above the default (0.2 mm without a project) is a
/// circle keepout of `max(size) + 2 * (local - default)` on each outer copper
/// layer of the pad, as the plugin's `edge_and_pad_keepouts` writes it
/// (`kicad-plugin/tests/test_mask_floor.py::test_pad_clearance_circles_match_the_readers`
/// gives the same circles for the same pads): FID1 0.6 mm on F.Cu, J1 0.5 mm
/// on both sides at its rotated position (20, 8) mm, B1 0.4 mm on B.Cu; R1's
/// 0.1 mm is within the default and adds nothing.
#[test]
fn pads_own_clearances_become_circle_keepouts() {
    let board = read(PAD_CLEARANCE, None).unwrap();
    assert_eq!(
        pad_circles(&board.pcb),
        vec![
            ("F.Cu".to_owned(), 1800.0, (10000.0, -10000.0)),
            ("F.Cu".to_owned(), 2100.0, (20000.0, -8000.0)),
            ("B.Cu".to_owned(), 2100.0, (20000.0, -8000.0)),
            ("B.Cu".to_owned(), 1200.0, (30000.0, -10000.0)),
        ]
    );
    // without a pad clearance nothing is added
    assert!(pad_circles(&read(MASK_FLOOR, None).unwrap().pcb).is_empty());
}
