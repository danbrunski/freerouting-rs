//! K15.9: a footprint-level `(clearance v)` reaches the pads without one of
//! their own (`pad_clearance`); the pad's own value wins. KiCad 10.0.7
//! (`pcbnew` Python, `PAD.GetOwnClearance` on this layout) gives the same:
//! pad 1 and pad 3 (`(clearance 0)`, which loads as "none") 0.6 mm, pad 2
//! 0.3 mm.

use freerouting_dsn::ast::{LayerRef, Pcb, Shape};
use freerouting_kicad::read;

const FIDUCIAL: &str = include_str!("data/k15-fiducial.kicad_pcb");

fn board(footprints: &str) -> String {
    format!(
        "(kicad_pcb (version 20240108) (generator \"pcbnew\") (generator_version \"8.0\")
  (layers (0 \"F.Cu\" signal) (31 \"B.Cu\" signal) (38 \"B.Mask\" user) (39 \"F.Mask\" user) (44 \"Edge.Cuts\" user))
  (setup (pad_to_mask_clearance 0))
  (net 0 \"\")
  (net 1 \"A\")
  (gr_rect (start 0 0) (end 40 20) (stroke (width 0.05) (type solid)) (fill none) (layer \"Edge.Cuts\"))
{footprints})"
    )
}

/// The pad-clearance circles: (layer, diameter µm, centre µm in the DSN frame).
fn circles(pcb: &Pcb) -> Vec<(String, f64, (f64, f64))> {
    pcb.structure
        .as_ref()
        .unwrap()
        .keepouts
        .iter()
        .filter(|k| k.area.name.as_deref() == Some(freerouting_kicad::pad_clearance::KEEPOUT_NAME))
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

/// The footprint's 0.6 mm reaches pad 1 (none of its own) and pad 3
/// (`(clearance 0)`); pad 2 keeps its own 0.3 mm. Circle diameters are
/// `1 + 2 * (local - 0.2)` mm.
#[test]
fn a_footprint_clearance_reaches_the_pads_without_their_own() {
    let text = board(
        "  (footprint \"l:fid\" (layer \"F.Cu\") (at 10 10)
    (property \"Reference\" \"U1\") (property \"Value\" \"x\")
    (clearance 0.6)
    (pad \"1\" smd circle (at 0 0) (size 1 1) (layers \"F.Cu\" \"F.Mask\") (net 1 \"A\"))
    (pad \"2\" smd circle (at 3 0) (size 1 1) (layers \"F.Cu\" \"F.Mask\") (net 1 \"A\") (clearance 0.3))
    (pad \"3\" smd circle (at 6 0) (size 1 1) (layers \"F.Cu\" \"F.Mask\") (net 1 \"A\") (clearance 0)))
",
    );
    assert_eq!(
        circles(&read(&text, None).unwrap().pcb),
        vec![
            ("F.Cu".to_owned(), 1800.0, (10000.0, -10000.0)),
            ("F.Cu".to_owned(), 1200.0, (13000.0, -10000.0)),
            ("F.Cu".to_owned(), 1800.0, (16000.0, -10000.0)),
        ]
    );
}

/// A pad's own larger value wins over a smaller footprint one, and a
/// footprint value within the default adds nothing.
#[test]
fn the_pads_own_clearance_wins_over_the_footprints() {
    let text = board(
        "  (footprint \"l:a\" (layer \"F.Cu\") (at 10 10)
    (property \"Reference\" \"U1\") (property \"Value\" \"x\")
    (clearance 0.1)
    (pad \"1\" smd circle (at 0 0) (size 1 1) (layers \"F.Cu\" \"F.Mask\") (net 1 \"A\") (clearance 0.6))
    (pad \"2\" smd circle (at 3 0) (size 1 1) (layers \"F.Cu\" \"F.Mask\") (net 1 \"A\")))
  (footprint \"l:b\" (layer \"B.Cu\") (at 30 10)
    (property \"Reference\" \"U2\") (property \"Value\" \"x\")
    (clearance 0.5)
    (pad \"1\" smd rect (at 0 0) (size 0.8 0.6) (layers \"B.Cu\" \"B.Mask\") (net 1 \"A\")))
",
    );
    assert_eq!(
        circles(&read(&text, None).unwrap().pcb),
        vec![
            ("F.Cu".to_owned(), 1800.0, (10000.0, -10000.0)),
            ("B.Cu".to_owned(), 1400.0, (30000.0, -10000.0)),
        ]
    );
}

/// The power-control unit's FID1 (a pad-level 0.6 mm): the circle is there.
#[test]
fn the_pcu_fiducial_has_its_circle() {
    assert_eq!(
        circles(&read(FIDUCIAL, None).unwrap().pcb),
        vec![("F.Cu".to_owned(), 1800.0, (52000.0, -33000.0))]
    );
}
