//! Q22.16: a footprint's own F.Mask / B.Mask shapes become keepouts on the
//! copper of their side (`freerouting_kicad::footprint_masks`).
//!
//! Fixture `crates/freerouting/tests/data/quality/q22-fp-mask.kicad_pcb`
//! (mm, KiCad's frame): J1 at (20, 11) has an F.Mask `fp_poly` over
//! (14..26, 11..19) and two 2 x 6 mm pads of nets A (16..18, 9..15) and B
//! (22..24, 9..15), which reach 2 mm above it, as fc-rom's edge fingers do;
//! U1 at (32, 25) has an F.Mask `fp_rect` (31.3..32.7) over its one 1 mm pad
//! of net D, which has no mask layer of its own. The keepouts are named
//! `solder_mask_pads`; what they let through is the router's
//! (`crates/freerouting/tests/footprint_mask.rs`).

use freerouting_dsn::ast::{Keepout, LayerRef, Shape};

const BOARD: &str = include_str!("../../freerouting/tests/data/quality/q22-fp-mask.kicad_pcb");

fn read(text: &str, project: Option<&str>) -> Vec<Keepout> {
    let r = freerouting_kicad::read(text, project).unwrap();
    r.pcb.structure.unwrap().keepouts
}

/// A mask keepout's name, layer and points (DSN frame: µm, Y negated).
type Mask = (String, String, Vec<(f64, f64)>);

/// The mask keepouts (`solder_mask` and `solder_mask_pads`).
fn masks(keepouts: &[Keepout]) -> Vec<Mask> {
    keepouts
        .iter()
        .filter(|k| {
            k.area
                .name
                .as_deref()
                .is_some_and(|n| n.starts_with("solder_mask"))
        })
        .map(|k| {
            let Shape::Polygon {
                layer: LayerRef::Named(layer),
                points,
                ..
            } = &k.area.shape
            else {
                panic!("{k:?}");
            };
            (
                k.area.name.clone().unwrap(),
                layer.clone(),
                points.iter().map(|p| (p.x, p.y)).collect(),
            )
        })
        .collect()
}

fn span(points: &[(f64, f64)]) -> ((f64, f64), (f64, f64)) {
    points.iter().fold(
        ((f64::MAX, f64::MIN), (f64::MAX, f64::MIN)),
        |((x0, x1), (y0, y1)), p| ((x0.min(p.0), x1.max(p.0)), (y0.min(p.1), y1.max(p.1))),
    )
}

/// J1's polygon and U1's rectangle, as they are, on F.Cu.
#[test]
fn a_footprints_mask_shapes_become_keepouts_on_their_copper_side() {
    let ks = masks(&read(BOARD, None));
    assert_eq!(ks.len(), 2, "{ks:?}");
    assert_eq!(
        (ks[0].0.as_str(), ks[0].1.as_str()),
        ("solder_mask_pads", "F.Cu")
    );
    assert_eq!(
        ks[0].2,
        vec![
            (14000.0, -11000.0),
            (26000.0, -11000.0),
            (26000.0, -19000.0),
            (14000.0, -19000.0),
            (14000.0, -11000.0)
        ]
    );
    assert_eq!(
        (ks[1].0.as_str(), ks[1].1.as_str()),
        ("solder_mask_pads", "F.Cu")
    );
    let ((x0, x1), (y0, y1)) = span(&ks[1].2);
    assert_eq!((x0, x1, y0, y1), (31300.0, 32700.0, -25700.0, -24300.0));
    // they survive the DSN text
    let r = freerouting_kicad::read(BOARD, None).unwrap();
    let back = freerouting_dsn::Pcb::parse(&r.pcb.to_dsn()).unwrap();
    assert_eq!(masks(&back.structure.unwrap().keepouts), ks);
}

/// A footprint KiCad lets bridge (`allow_soldermask_bridges`, a solder
/// jumper) has no keepout: KiCad's DRC ignores its apertures.
#[test]
fn a_footprint_that_allows_mask_bridges_has_no_keepout() {
    let text = BOARD.replace(
        "(property \"Reference\" \"J1\") (property \"Value\" \"x\")",
        "(property \"Reference\" \"J1\") (property \"Value\" \"x\") (attr smd allow_soldermask_bridges)",
    );
    assert_ne!(text, BOARD);
    let ks = masks(&read(&text, None));
    assert_eq!(ks.len(), 1);
    let ((x0, _), _) = span(&ks[0].2);
    assert_eq!(x0, 31300.0, "U1's only");
}

/// A B.Mask shape goes on the bottom copper.
#[test]
fn a_back_mask_shape_goes_on_the_bottom_copper() {
    let text = BOARD.replace(
        "(fill yes) (layer \"F.Mask\"))\n    (pad \"1\" smd rect (at -3 1)",
        "(fill yes) (layer \"B.Mask\"))\n    (pad \"1\" smd rect (at -3 1)",
    );
    assert_ne!(text, BOARD);
    let ks = masks(&read(&text, None));
    let got: Vec<&str> = ks.iter().map(|k| k.1.as_str()).collect();
    assert_eq!(got, vec!["B.Cu", "F.Cu"]);
}

/// Q22.12's `solder_mask_to_copper_clearance` grows the footprint apertures
/// like the board's: the body and a strip within 0.05 mm of each side.
#[test]
fn the_mask_to_copper_clearance_grows_the_footprint_apertures() {
    let pro =
        r#"{"board": {"design_settings": {"rules": {"solder_mask_to_copper_clearance": 0.05}}}}"#;
    let ks = masks(&read(BOARD, Some(pro)));
    assert_eq!(ks.len(), 10, "{ks:?}");
    let j1: Vec<(f64, f64)> = ks[..5].iter().flat_map(|k| k.2.iter().copied()).collect();
    let ((x0, x1), (y0, y1)) = span(&j1);
    let close = |a: f64, b: f64| (a - b).abs() < 1e-6;
    assert!(
        close(x0, 13950.0) && close(x1, 26050.0) && close(y0, -19050.0) && close(y1, -10950.0),
        "{:?}",
        span(&j1)
    );
}
