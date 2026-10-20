//! Q22.4: the reader carries copper shapes and copper texts, which KiCad's
//! DSN export leaves out, as keepouts; a shape of a net with pins is left out
//! with a warning (`freerouting_kicad::copper_graphics`).
//!
//! The fixture (`crates/freerouting/tests/data/kicad/q22-copper-graphics.kicad_pcb`,
//! shared with the plugin's `test_copper_graphics.py` and the engine's
//! `tests/copper_graphics.rs`): J1's two pads numbered 2 (net N2) at (12, 10)
//! and (18, 10) mm with a 0.5 mm B.Cu `fp_line` of no net from (13.2, 10) to
//! (16.8, 10) between them, a 1.5 mm `gr_text "GND"` on F.Cu at (15, 10), and
//! a filled F.Cu `gr_poly` of net FG, (3, 3)-(7, 5), under P1, which is left
//! out.

use freerouting_dsn::ast::{Keepout, LayerRef, Shape, WireType, WiringItem};
use freerouting_kicad::copper_graphics::{KEEPOUT_NAME, TEXT_KEEPOUT_NAME};

const BOARD: &str =
    include_str!("../../freerouting/tests/data/kicad/q22-copper-graphics.kicad_pcb");

fn named<'a>(keepouts: &'a [Keepout], name: &str) -> Vec<&'a Keepout> {
    keepouts
        .iter()
        .filter(|k| k.area.name.as_deref() == Some(name))
        .collect()
}

fn polygon(k: &Keepout) -> (String, Vec<(f64, f64)>) {
    let Shape::Polygon {
        layer: LayerRef::Named(layer),
        points,
        ..
    } = &k.area.shape
    else {
        panic!("{k:?}");
    };
    (layer.clone(), points.iter().map(|p| (p.x, p.y)).collect())
}

fn close(got: &[(f64, f64)], want: &[(f64, f64)]) -> bool {
    got.len() == want.len()
        && got
            .iter()
            .zip(want)
            .all(|(g, w)| (g.0 - w.0).abs() < 1e-6 && (g.1 - w.1).abs() < 1e-6)
}

/// The fixed wires of the read board (the reader makes them only from
/// locked tracks; the fixtures have none).
fn fixed_wires(read: &freerouting_kicad::KicadRead) -> usize {
    read.pcb
        .wiring
        .as_ref()
        .unwrap()
        .items
        .iter()
        .filter(|i| matches!(i, WiringItem::Wire(w) if w.wire_type == Some(WireType::Fix)))
        .count()
}

const LEFT_OUT: &str = "copper graphics: 1 shape(s) of a net left out (Q22.4 remainder)";

#[test]
fn a_footprint_copper_line_becomes_a_keepout_on_its_layer() {
    let read = freerouting_kicad::read(BOARD, None).unwrap();
    let keepouts = read.pcb.structure.unwrap().keepouts;
    let lines = named(&keepouts, KEEPOUT_NAME);
    assert_eq!(lines.len(), 1, "{keepouts:?}");
    let (layer, poly) = polygon(lines[0]);
    assert_eq!(layer, "B.Cu");
    // the plugin's corners (test_copper_graphics.py), in the DSN frame
    let want = [
        (12950.0, -10250.0),
        (17050.0, -10250.0),
        (17050.0, -9750.0),
        (12950.0, -9750.0),
        (12950.0, -10250.0),
    ];
    assert!(close(&poly, &want), "{poly:?}");
}

#[test]
fn a_copper_text_becomes_a_keepout_box_that_holds_its_glyphs() {
    let read = freerouting_kicad::read(BOARD, None).unwrap();
    let keepouts = read.pcb.structure.unwrap().keepouts;
    let texts = named(&keepouts, TEXT_KEEPOUT_NAME);
    assert_eq!(texts.len(), 1);
    let (layer, poly) = polygon(texts[0]);
    assert_eq!(layer, "F.Cu");
    // 3 x 1.25 x 1.5 mm + 0.3 mm wide, 1.8 x 1.5 mm + 0.3 mm high, centred on
    // (15, 10): it holds KiCad 10.0.7's glyph strokes, 12.842..17.158 x
    // 9.021..10.836 mm (the plugin's pcbnew test)
    let want = [
        (12037.5, -8500.0),
        (17962.5, -8500.0),
        (17962.5, -11500.0),
        (12037.5, -11500.0),
        (12037.5, -8500.0),
    ];
    assert!(close(&poly, &want), "{poly:?}");
}

#[test]
fn a_shape_of_a_net_is_left_out_and_logged() {
    let read = freerouting_kicad::read(BOARD, None).unwrap();
    let keepouts = &read.pcb.structure.as_ref().unwrap().keepouts;
    // no keepout for the FG polygon: one line, one text
    assert_eq!(named(keepouts, KEEPOUT_NAME).len(), 1);
    assert_eq!(named(keepouts, TEXT_KEEPOUT_NAME).len(), 1);
    // and no copper of FG
    assert_eq!(fixed_wires(&read), 0);
    let messages: Vec<&str> = read.warnings.iter().map(|w| w.message.as_str()).collect();
    assert!(messages.contains(&LEFT_OUT), "{messages:?}");
}

#[test]
fn net_tie_shapes_and_shapes_of_a_net_without_pins_are_keepouts() {
    let board = r#"(kicad_pcb (version 20240108) (generator "pcbnew") (generator_version "8.0")
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (net 0 "")
  (net 1 "A")
  (net 2 "B")
  (net 3 "LONE")
  (gr_rect (start 0 0) (end 20 20) (stroke (width 0.05) (type solid)) (fill none) (layer "Edge.Cuts"))
  (footprint "l:tie" (layer "F.Cu") (at 10 10)
    (property "Reference" "NT1") (property "Value" "x")
    (net_tie_pad_groups "1,2")
    (fp_line (start -1 0) (end 1 0) (stroke (width 0.3) (type solid)) (layer "F.Cu") (net 1))
    (pad "1" smd rect (at -1 0) (size 0.6 0.6) (layers "F.Cu") (net 1 "A"))
    (pad "2" smd rect (at 1 0) (size 0.6 0.6) (layers "F.Cu") (net 2 "B")))
  (gr_line (start 2 2) (end 4 2) (stroke (width 0.2) (type solid)) (layer "B.Cu") (net 3))
  (gr_text "hidden" (at 5 15) (layer "F.Cu") (hide yes) (effects (font (size 1 1) (thickness 0.2))))
)"#;
    let read = freerouting_kicad::read(board, None).unwrap();
    let keepouts = read.pcb.structure.as_ref().unwrap().keepouts.clone();
    let layers: Vec<String> = named(&keepouts, KEEPOUT_NAME)
        .iter()
        .map(|k| polygon(k).0)
        .collect();
    // board shapes first, then the footprints'
    assert_eq!(layers, ["B.Cu", "F.Cu"]);
    assert!(named(&keepouts, TEXT_KEEPOUT_NAME).is_empty());
    assert_eq!(fixed_wires(&read), 0);
    // nothing left out: the tie line has no net, LONE has no pins
    assert!(
        read.warnings
            .iter()
            .all(|w| !w.message.contains("left out"))
    );
}
