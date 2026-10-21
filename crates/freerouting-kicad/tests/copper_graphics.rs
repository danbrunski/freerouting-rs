//! Q22.4: the reader carries copper shapes and copper texts, which KiCad's
//! DSN export leaves out, as keepouts, and a shape of a net with pins as
//! conduction-area wires of that net with `(attr obstacle)`
//! (`freerouting_kicad::copper_graphics`).
//!
//! The fixture (`crates/freerouting/tests/data/kicad/q22-copper-graphics.kicad_pcb`,
//! shared with the plugin's `test_copper_graphics.py` and the engine's
//! `tests/copper_graphics.rs`): J1's two pads numbered 2 (net N2) at (12, 10)
//! and (18, 10) mm with a 0.5 mm B.Cu `fp_line` of no net from (13.2, 10) to
//! (16.8, 10) between them, a 1.5 mm `gr_text "GND"` on F.Cu at (15, 10), a
//! filled F.Cu `gr_poly` of net FG, (3, 3)-(7, 5), under FG's pads P1 and P2,
//! and a 1 mm F.Cu `gr_line` of net FG from (20, 14) to (28, 14) across N3's
//! straight path.

use freerouting_dsn::ast::{
    Keepout, LayerRef, Shape, WIRE_ATTR_OBSTACLE, Wire, WireType, WiringItem,
};
use freerouting_kicad::copper_graphics::{KEEPOUT_NAME, NET_TIE_KEEPOUT_NAME, TEXT_KEEPOUT_NAME};

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

/// The fixed wires of the read board that are not net shapes (the reader
/// makes them only from locked tracks; the fixtures have none).
fn fixed_wires(read: &freerouting_kicad::KicadRead) -> usize {
    read.pcb
        .wiring
        .as_ref()
        .unwrap()
        .items
        .iter()
        .filter(|i| {
            matches!(i, WiringItem::Wire(w) if w.wire_type == Some(WireType::Fix) && w.attr.is_none())
        })
        .count()
}

/// The conduction-area wires with `(attr obstacle)`, in wiring order.
fn area_wires(read: &freerouting_kicad::KicadRead) -> Vec<&Wire> {
    read.pcb
        .wiring
        .as_ref()
        .unwrap()
        .items
        .iter()
        .filter_map(|i| match i {
            WiringItem::Wire(w) if w.attr.as_deref() == Some(WIRE_ATTR_OBSTACLE) => Some(w),
            _ => None,
        })
        .collect()
}

fn shape_polygon(shape: &Shape) -> (String, Vec<(f64, f64)>) {
    let Shape::Polygon {
        layer: LayerRef::Named(layer),
        points,
        aperture_width,
    } = shape
    else {
        panic!("{shape:?}");
    };
    assert_eq!(*aperture_width, 0.0);
    (layer.clone(), points.iter().map(|p| (p.x, p.y)).collect())
}

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
fn shapes_of_a_net_become_conduction_areas_of_their_net() {
    let read = freerouting_kicad::read(BOARD, None).unwrap();
    let keepouts = &read.pcb.structure.as_ref().unwrap().keepouts;
    // no keepout for the FG shapes: one line, one text
    assert_eq!(named(keepouts, KEEPOUT_NAME).len(), 1);
    assert_eq!(named(keepouts, TEXT_KEEPOUT_NAME).len(), 1);
    assert_eq!(fixed_wires(&read), 0);
    let areas = area_wires(&read);
    // the plugin's areas (test_copper_graphics.py), in the DSN frame: the
    // polygon, then the line's stroke, 0.5 mm on each side and past each end
    let want = [
        vec![
            (3000.0, -3000.0),
            (7000.0, -3000.0),
            (7000.0, -5000.0),
            (3000.0, -5000.0),
            (3000.0, -3000.0),
        ],
        vec![
            (19500.0, -14500.0),
            (28500.0, -14500.0),
            (28500.0, -13500.0),
            (19500.0, -13500.0),
            (19500.0, -14500.0),
        ],
    ];
    assert_eq!(areas.len(), want.len(), "{areas:?}");
    for (wire, want) in areas.iter().zip(&want) {
        assert_eq!(wire.shapes.len(), 1);
        let (layer, poly) = shape_polygon(&wire.shapes[0]);
        assert_eq!(layer, "F.Cu");
        assert!(close(&poly, want), "{poly:?}");
        assert_eq!(wire.net.as_ref().map(|n| n.name.as_str()), Some("FG"));
        assert_eq!(wire.wire_type, Some(WireType::Fix));
        assert!(wire.windows.is_empty() && wire.clearance_class.is_none());
    }
    // after the board's own wiring (none here), and no warning
    let items = &read.pcb.wiring.as_ref().unwrap().items;
    assert!(matches!(items.last(), Some(WiringItem::Wire(w)) if w.attr.is_some()));
    assert!(
        read.warnings
            .iter()
            .all(|w| !w.message.contains("left out"))
    );
}

#[test]
fn the_written_dsn_keeps_the_areas() {
    let read = freerouting_kicad::read(BOARD, None).unwrap();
    let text = read.pcb.to_dsn();
    let back = freerouting_dsn::Pcb::parse(&text).unwrap();
    assert_eq!(back.wiring, read.pcb.wiring);
    assert_eq!(text.matches("(attr obstacle)").count(), 2, "{text}");
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
    // the board's shape; the tie's line is a `net_tie` keepout (Q22.19)
    assert_eq!(layers, ["B.Cu"]);
    let ties: Vec<String> = named(&keepouts, NET_TIE_KEEPOUT_NAME)
        .iter()
        .map(|k| polygon(k).0)
        .collect();
    assert_eq!(ties, ["F.Cu"]);
    assert!(named(&keepouts, TEXT_KEEPOUT_NAME).is_empty());
    assert_eq!(fixed_wires(&read), 0);
    // no areas: the tie line has no net, LONE has no pins
    assert!(area_wires(&read).is_empty());
}

/// Q22.17: a 0.6 mm `gr_arc` of net A (whose pad P1 is at (50, 20)) of
/// radius 5 about (10, 10), from angle 0 through 135° to 270° (KiCad's frame,
/// Y down).
const ARC_BOARD: &str = r#"(kicad_pcb (version 20240108) (generator "pcbnew") (generator_version "8.0")
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (net 0 "")
  (net 1 "A")
  (gr_rect (start 0 0) (end 60 30) (stroke (width 0.05) (type solid)) (fill none) (layer "Edge.Cuts"))
  (footprint "l:pad" (layer "F.Cu") (at 50 20)
    (property "Reference" "P1") (property "Value" "x")
    (pad "1" smd rect (at 0 0) (size 0.6 0.6) (layers "F.Cu") (net 1 "A")))
  (gr_arc (start 15 10) (mid 6.464466 13.535534) (end 10 5) (stroke (width 0.6) (type solid)) (layer "F.Cu") (net 1))
)"#;

/// Whether `p` (µm, DSN frame) is inside the closed polygon `poly`.
fn inside(poly: &[(f64, f64)], p: (f64, f64)) -> bool {
    let mut odd = false;
    for w in poly.windows(2) {
        let ((x0, y0), (x1, y1)) = (w[0], w[1]);
        if (y0 > p.1) != (y1 > p.1) && p.0 < x0 + (p.1 - y0) * (x1 - x0) / (y1 - y0) {
            odd = !odd;
        }
    }
    odd
}

#[test]
fn a_stroked_arc_s_pieces_cover_the_stroke_and_its_round_ends() {
    // decided 2026-10-20 (Q22.17): no merge; the arc stays one area per
    // chord at the board's 5 µm maximum error
    let read = freerouting_kicad::read(ARC_BOARD, None).unwrap();
    let pieces: Vec<Vec<(f64, f64)>> = area_wires(&read)
        .iter()
        .map(|w| {
            assert_eq!(w.shapes.len(), 1);
            assert!(w.windows.is_empty());
            shape_polygon(&w.shapes[0]).1
        })
        .collect();
    assert!(pieces.len() > 20, "{}", pieces.len());
    let covered = |(x, y): (f64, f64)| {
        let p = (x * 1000.0, -y * 1000.0);
        pieces.iter().any(|piece| inside(piece, p))
    };
    let (c, r, half) = ((10.0, 10.0), 5.0, 0.3);
    let at = |deg: f64, radius: f64| {
        let a = deg.to_radians();
        (c.0 + radius * a.cos(), c.1 + radius * a.sin())
    };
    for k in 0..=540 {
        let deg = 270.0 * f64::from(k) / 540.0;
        for side in [-1.0, -0.5, 0.0, 0.5, 1.0] {
            let p = at(deg, r + half * side);
            assert!(covered(p), "{p:?} of the stroke is outside every piece");
        }
    }
    for end in [at(0.0, r), at(270.0, r)] {
        for k in 0..36 {
            let a = f64::from(k * 10).to_radians();
            let p = (end.0 + half * a.cos(), end.1 + half * a.sin());
            assert!(
                covered(p),
                "{p:?} of the stroke's end is outside every piece"
            );
        }
    }
}

/// Q22.19 (`crates/freerouting/tests/data/kicad/q22-net-tie.kicad_pcb`): the
/// net-tie footprint NT1's filled F.Cu polygon (16..24, 13.5..16.5 mm), which
/// joins its pads of nets A and B, is one `net_tie` keepout at the default
/// clearance, the same polygon the KiCad plugin writes (`q22-net-tie.dsn`,
/// `kicad-plugin/tests/test_copper_graphics.py`); the router lets A and B in
/// and keeps every other net out (`crates/freerouting/tests/net_tie.rs`).
#[test]
fn a_net_tie_shape_is_a_net_tie_keepout_the_same_as_the_plugins() {
    let board = include_str!("../../freerouting/tests/data/kicad/q22-net-tie.kicad_pcb");
    let plugin = include_str!("../../freerouting/tests/data/kicad/q22-net-tie.dsn");
    let read = freerouting_kicad::read(board, None).unwrap();
    let keepouts = read.pcb.structure.as_ref().unwrap().keepouts.clone();
    let ties = named(&keepouts, NET_TIE_KEEPOUT_NAME);
    assert_eq!(ties.len(), 1, "{keepouts:?}");
    assert!(named(&keepouts, KEEPOUT_NAME).is_empty());
    assert_eq!(ties[0].area.clearance_class, None);
    let want = freerouting_dsn::Pcb::parse(plugin).unwrap();
    let want = want.structure.as_ref().unwrap().keepouts.clone();
    let want = named(&want, NET_TIE_KEEPOUT_NAME);
    assert_eq!(want.len(), 1);
    let (layer, points) = polygon(ties[0]);
    let (want_layer, want_points) = polygon(want[0]);
    assert_eq!(layer, want_layer);
    assert!(close(&points, &want_points), "{points:?} {want_points:?}");
    assert_eq!(fixed_wires(&read), 0);
    assert!(area_wires(&read).is_empty());
}
