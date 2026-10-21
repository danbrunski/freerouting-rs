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

/// J1's polygon and U1's rectangle, as they are, on F.Cu (J1's, over pads
/// of two nets, as `solder_mask_shared`: `pad_trim`).
#[test]
fn a_footprints_mask_shapes_become_keepouts_on_their_copper_side() {
    let ks = masks(&read(BOARD, None));
    // Q22.18: each body and a 1 µm strip along each side
    assert_eq!(ks.len(), 10, "{ks:?}");
    assert_eq!(
        (ks[0].0.as_str(), ks[0].1.as_str()),
        ("solder_mask_shared", "F.Cu")
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
        (ks[5].0.as_str(), ks[5].1.as_str()),
        ("solder_mask_pads", "F.Cu")
    );
    let ((x0, x1), (y0, y1)) = span(&ks[5].2);
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
    assert_eq!(ks.len(), 5);
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
    assert_eq!(got, [["B.Cu"; 5], ["F.Cu"; 5]].concat());
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
        close(x0, 13949.0) && close(x1, 26051.0) && close(y0, -19051.0) && close(y1, -10949.0),
        "{:?}",
        span(&j1)
    );
}

/// Q22.18: every mask keepout keeps no clearance of its own (the router's
/// class `null`): the aperture is already grown by Q22.12's clearance.
#[test]
fn the_mask_keepouts_keep_no_clearance_of_their_own() {
    let classes: Vec<Option<String>> = read(BOARD, None)
        .iter()
        .filter(|k| {
            k.area
                .name
                .as_deref()
                .is_some_and(|n| n.starts_with("solder_mask"))
        })
        .map(|k| k.area.clearance_class.clone())
        .collect();
    assert_eq!(classes, vec![Some("null".to_owned()); 10]);
}

/// The DSN library of a read board: each image pin as (image, pin,
/// padstack, x, y, rotation), and the padstack named `name`'s shapes.
fn pins(r: &freerouting_kicad::read::KicadRead, image: &str) -> Vec<(String, String, f64, f64)> {
    let lib = r.pcb.library.as_ref().unwrap();
    let img = lib.images.iter().find(|i| i.name == image).unwrap();
    img.pins
        .iter()
        .map(|p| (p.name.clone(), p.padstack.clone(), p.at.x, p.at.y))
        .collect()
}

/// Q22.16 (option (b)): J1's aperture lies over pads of nets A and B, so it
/// is a `solder_mask_shared` keepout, which the router keeps every net out
/// of, and each finger goes to the router as its part outside the aperture
/// (KiCad 16..18 x 9..10.999 for A, the aperture grown by Q22.18's 1 µm): a
/// 2 x 1.999 mm padstack with the pin's anchor at its centre (rounded to
/// 1 µm), 2.001 mm above the old one. U1's one-net aperture stays.
#[test]
fn a_pad_partly_inside_a_shared_aperture_is_cut_back_to_its_part_outside() {
    let r = freerouting_kicad::read(BOARD, None).unwrap();
    let ks = masks(&r.pcb.structure.as_ref().unwrap().keepouts);
    let names: Vec<&str> = ks.iter().map(|k| k.0.as_str()).collect();
    // J1's body and its top strip (over the fingers) are shared; its other
    // strips touch no pad
    assert_eq!(
        (names[0], names[5]),
        ("solder_mask_shared", "solder_mask_pads")
    );
    assert_eq!(
        names.iter().filter(|n| **n == "solder_mask_shared").count(),
        2
    );
    let cut = "Rect[T]Pad_2000.000000x6000.000000_um_outside_mask_1";
    assert_eq!(
        pins(&r, "l:edge2"),
        vec![
            ("1".to_owned(), cut.to_owned(), -3000.0, 1001.0),
            ("2".to_owned(), cut.to_owned(), 3000.0, 1001.0),
        ]
    );
    let lib = r.pcb.library.as_ref().unwrap();
    let ps = lib.padstacks.iter().find(|p| p.name == cut).unwrap();
    let Shape::Polygon {
        layer: LayerRef::Named(layer),
        points,
        ..
    } = &ps.shapes[0]
    else {
        panic!("{ps:?}");
    };
    assert_eq!(layer, "F.Cu");
    // the plugin's corners (`test_footprint_masks.py`), in its order
    let corners: Vec<(f64, f64)> = points.iter().map(|p| (p.x, p.y)).collect();
    assert_eq!(
        corners,
        vec![
            (1000.0, -1000.0),
            (1000.0, 999.0),
            (-1000.0, 999.0),
            (-1000.0, -1000.0),
            (1000.0, -1000.0)
        ]
    );
    assert_eq!(ps.attach, Some(false));
    // U1's pad and the others keep KiCad's padstacks
    assert_eq!(
        pins(&r, "l:land")[0].1,
        "Rect[T]Pad_1000.000000x1000.000000_um"
    );
    assert!(
        r.warnings
            .iter()
            .all(|w| !w.message.contains("solder-mask"))
    );
    // and the cut pads survive the DSN text
    let back = freerouting_dsn::Pcb::parse(&r.pcb.to_dsn()).unwrap();
    assert_eq!(back.library, r.pcb.library);
}

/// A pad wholly inside a shared aperture has no part outside it: it is left
/// as it is, unreachable, and the reader names it with its net.
#[test]
fn a_pad_wholly_inside_a_shared_aperture_is_named_in_a_warning() {
    let text = BOARD.replace(
        "(pad \"2\" smd rect (at 3 1) (size 2 6)",
        "(pad \"2\" smd rect (at 3 4) (size 2 2)",
    );
    assert_ne!(text, BOARD);
    let r = freerouting_kicad::read(&text, None).unwrap();
    let pins = pins(&r, "l:edge2");
    assert_eq!(pins[1].1, "Rect[T]Pad_2000.000000x2000.000000_um");
    let named: Vec<&str> = r
        .warnings
        .iter()
        .map(|w| w.message.as_str())
        .filter(|m| m.contains("solder-mask aperture"))
        .collect();
    assert_eq!(named.len(), 1, "{:?}", r.warnings);
    assert!(
        named[0].starts_with("pad J1-2 (net B) lies inside"),
        "{named:?}"
    );
}

/// An image that other components use too is copied for the component whose
/// pads are cut, so theirs stay as they are.
#[test]
fn a_shared_image_is_copied_for_the_component_whose_pads_are_cut() {
    let j2 = "  (footprint \"l:edge2\" (layer \"F.Cu\") (at 8 24)\n    \
              (property \"Reference\" \"J2\") (property \"Value\" \"x\")\n    \
              (pad \"1\" smd rect (at -3 1) (size 2 6) (layers \"F.Cu\") (net 1 \"A\"))\n    \
              (pad \"2\" smd rect (at 3 1) (size 2 6) (layers \"F.Cu\") (net 2 \"B\")))\n";
    let text = BOARD.replace(
        "  (footprint \"l:pad\" (layer \"F.Cu\") (at 17 3)",
        &format!("{j2}  (footprint \"l:pad\" (layer \"F.Cu\") (at 17 3)"),
    );
    assert_ne!(text, BOARD);
    let r = freerouting_kicad::read(&text, None).unwrap();
    let placement = r.pcb.placement.as_ref().unwrap();
    let blocks: Vec<(&str, Vec<&str>)> = placement
        .components
        .iter()
        .map(|c| {
            (
                c.image.as_str(),
                c.places.iter().map(|p| p.name.as_str()).collect(),
            )
        })
        .collect();
    assert_eq!(
        &blocks[..2],
        &[("l:edge2", vec!["J2"]), ("l:edge2_J1", vec!["J1"])]
    );
    assert_eq!(pins(&r, "l:edge2")[0].2, -3000.0);
    assert_eq!(pins(&r, "l:edge2")[0].3, -1000.0);
    assert_eq!(pins(&r, "l:edge2_J1")[0].3, 1001.0);
}
