//! Q18.5: the reader carries the Edge.Cuts and Margin shapes KiCad's DSN
//! export leaves out as copper keepouts (`freerouting_kicad::edge_keepouts`),
//! and (Q18.7) the board's own solder-mask apertures.

use freerouting_dsn::ast::{Keepout, KeepoutKind, LayerRef, Shape};
use freerouting_kicad::edge_keepouts::{KEEPOUT_NAME, MASK_KEEPOUT_NAME};

fn read(text: &str, project: Option<&str>) -> Vec<Keepout> {
    let r = freerouting_kicad::read(text, project).unwrap();
    r.pcb.structure.unwrap().keepouts
}

fn edge(keepouts: &[Keepout]) -> Vec<&Keepout> {
    keepouts
        .iter()
        .filter(|k| k.area.name.as_deref() == Some(KEEPOUT_NAME))
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

const BOARD: &str = include_str!("data/q18-edge-graphics.kicad_pcb");

/// The fixture: a 50 x 40 mm Edge.Cuts rectangle, a Margin line and a Margin
/// arc inside it, and a footprint's loose Edge.Cuts line (which closes
/// nothing, so the outline is malformed and the bounding box stands in, as in
/// KiCad's export).
#[test]
fn margin_and_loose_edge_cuts_shapes_become_keepouts_on_every_copper_layer() {
    let all = read(BOARD, None);
    let ks = edge(&all);
    assert!(ks.iter().all(|k| k.kind == KeepoutKind::Keepout));
    let layers: Vec<String> = ks.iter().map(|k| polygon(k).0).collect();
    let half = layers.len() / 2;
    assert!(layers[..half].iter().all(|l| l == "F.Cu"));
    assert!(layers[half..].iter().all(|l| l == "B.Cu"));
    // the Margin line first: w = 0.5 mm edge (KiCad's default) + 0.05 mm half
    // stroke - 0.2 mm default clearance = 0.35 mm, reaching 0.35 mm past both ends
    let (_, first) = polygon(ks[0]);
    let expected = [
        (19650.0, -4650.0),
        (19650.0, -35350.0),
        (20350.0, -35350.0),
        (20350.0, -4650.0),
        (19650.0, -4650.0),
    ];
    for (got, want) in first.iter().zip(expected) {
        assert!(
            (got.0 - want.0).abs() < 1e-6 && (got.1 - want.1).abs() < 1e-6,
            "{first:?}"
        );
    }
    // the rectangle is the outline: nothing of it is a keepout; the footprint's
    // loose line (37..43 mm at y 30) is
    let covers = |x: f64, y: f64| {
        ks[..half].iter().any(|k| {
            let (_, p) = polygon(k);
            let (x0, x1) = p
                .iter()
                .fold((f64::MAX, f64::MIN), |a, q| (a.0.min(q.0), a.1.max(q.0)));
            let (y0, y1) = p
                .iter()
                .fold((f64::MAX, f64::MIN), |a, q| (a.0.min(q.1), a.1.max(q.1)));
            (x0..=x1).contains(&(x * 1000.0)) && (y0..=y1).contains(&(-y * 1000.0))
        })
    };
    assert!(covers(40.0, 30.0) && covers(37.0, 30.0) && covers(43.0, 30.0));
    assert!(covers(28.535534, 11.464466) && covers(25.0, 10.0) && covers(30.0, 15.0));
    assert!(!covers(10.0, 0.0) && !covers(50.0, 20.0) && !covers(10.0, 40.0));
    // no signal keepout (the outline has no hole)
    assert!(all.iter().all(|k| !matches!(
        k.area.shape,
        Shape::Polygon {
            layer: LayerRef::Signal,
            ..
        }
    )));
}

#[test]
fn the_keepouts_follow_the_projects_edge_clearance() {
    let pro = r#"{"board": {"design_settings": {"rules": {"min_copper_edge_clearance": 1.0}}}}"#;
    let ks = read(BOARD, Some(pro));
    let (_, first) = polygon(edge(&ks)[0]);
    // w = 1.0 + 0.05 - 0.2 = 0.85 mm
    assert!((first[0].0 - 19150.0).abs() < 1e-6, "{first:?}");
    // a closing outline rectangle with no other edge shape adds nothing
    let plain = BOARD
        .lines()
        .filter(|l| !l.contains("\"Margin\"") && !l.contains("fp_line"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(edge(&read(&plain, Some(pro))).is_empty());
}

/// The keepouts survive the DSN text (a session written from the tree).
#[test]
fn the_keepouts_round_trip_through_dsn_text() {
    let r = freerouting_kicad::read(BOARD, None).unwrap();
    let n = edge(&r.pcb.structure.as_ref().unwrap().keepouts).len();
    assert!(n > 4);
    let back = freerouting_dsn::Pcb::parse(&r.pcb.to_dsn()).unwrap();
    assert_eq!(edge(&back.structure.unwrap().keepouts).len(), n);
}

/// Q18.7, `data/q18-mask-graphics.kicad_pcb`: a filled F.Mask polygon, a
/// filled B.Mask circle (r 2 mm at 35, 25) and a 0.4 mm F.Mask line on the
/// board, and a footprint's F.Mask polygon (its pad's aperture, not the
/// board's).
const MASKS: &str = include_str!("data/q18-mask-graphics.kicad_pcb");

fn masks(keepouts: &[Keepout]) -> Vec<(String, Vec<(f64, f64)>)> {
    keepouts
        .iter()
        .filter(|k| k.area.name.as_deref() == Some(MASK_KEEPOUT_NAME))
        .inspect(|k| assert_eq!(k.kind, KeepoutKind::Keepout))
        .map(polygon)
        .collect()
}

#[test]
fn the_boards_mask_apertures_become_keepouts_on_their_copper_side() {
    let all = read(MASKS, None);
    assert!(edge(&all).is_empty());
    let ks = masks(&all);
    assert_eq!(ks.len(), 3, "{ks:?}");
    // the polygon as it is, closed, on F.Cu (DSN frame: µm, Y negated)
    assert_eq!(ks[0].0, "F.Cu");
    assert_eq!(
        ks[0].1,
        vec![
            (10000.0, -10000.0),
            (20000.0, -10000.0),
            (20000.0, -20000.0),
            (10000.0, -20000.0),
            (10000.0, -10000.0)
        ]
    );
    // the circle on B.Cu, its chords outside the 2 mm circle
    assert_eq!(ks[1].0, "B.Cu");
    let ring = &ks[1].1;
    assert!(ring.len() > 16);
    for w in ring.windows(2) {
        let mid = ((w[0].0 + w[1].0) / 2.0, (w[0].1 + w[1].1) / 2.0);
        let r = (mid.0 - 35000.0).hypot(mid.1 + 25000.0);
        assert!(r >= 2000.0, "chord midpoint at {r} µm");
    }
    // the line: the rectangle within half its 0.4 mm stroke, on F.Cu
    assert_eq!(ks[2].0, "F.Cu");
    let xs: Vec<f64> = ks[2].1.iter().map(|p| p.0).collect();
    let ys: Vec<f64> = ks[2].1.iter().map(|p| p.1).collect();
    let span = |v: &[f64]| {
        v.iter()
            .fold((f64::MAX, f64::MIN), |a, x| (a.0.min(*x), a.1.max(*x)))
    };
    let close = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6;
    assert!(close(span(&xs), (4800.0, 15200.0)), "{xs:?}");
    assert!(close(span(&ys), (-30200.0, -29800.0)), "{ys:?}");
    // they survive the DSN text
    let r = freerouting_kicad::read(MASKS, None).unwrap();
    let back = freerouting_dsn::Pcb::parse(&r.pcb.to_dsn()).unwrap();
    assert_eq!(masks(&back.structure.unwrap().keepouts).len(), 3);
}

fn span(points: &[(f64, f64)]) -> ((f64, f64), (f64, f64)) {
    points.iter().fold(
        ((f64::MAX, f64::MIN), (f64::MAX, f64::MIN)),
        |((x0, x1), (y0, y1)), p| ((x0.min(p.0), x1.max(p.0)), (y0.min(p.1), y1.max(p.1))),
    )
}

fn mask_pro(clearance_mm: f64) -> String {
    format!(
        r#"{{"board": {{"design_settings": {{"rules": {{"solder_mask_to_copper_clearance": {clearance_mm}}}}}}}}}"#
    )
}

/// Q22.12: the `.kicad_pro`'s `solder_mask_to_copper_clearance` grows every
/// board mask aperture by it; 0 (KiCad's default, as with no key) keeps
/// Q18.7's keepouts byte for byte.
#[test]
fn the_mask_apertures_grow_by_the_projects_mask_to_copper_clearance() {
    let plain = masks(&read(MASKS, None));
    assert_eq!(masks(&read(MASKS, Some(&mask_pro(0.0)))), plain);
    let no_key = r#"{"board": {"design_settings": {"rules": {"max_error": 0.005}}}}"#;
    assert_eq!(masks(&read(MASKS, Some(no_key))), plain);
    let grown = masks(&read(MASKS, Some(&mask_pro(0.05))));
    let close = |a: f64, b: f64| (a - b).abs() < 1e-6;
    // the filled polygon: its body as it was, then a rectangle within 0.05 mm
    // of each side, reaching 0.05 mm past both ends
    assert_eq!(grown[0], plain[0]);
    let square: Vec<(f64, f64)> = grown[1..5]
        .iter()
        .inspect(|k| assert_eq!(k.0, "F.Cu"))
        .flat_map(|k| k.1.clone())
        .collect();
    let ((x0, x1), (y0, y1)) = span(&square);
    assert!(close(x0, 9950.0) && close(x1, 20050.0), "{square:?}");
    assert!(close(y0, -20050.0) && close(y1, -9950.0), "{square:?}");
    for k in &grown[1..5] {
        let ((a, b), (c, d)) = span(&k.1);
        // each strip is 0.1 mm across, the side's length plus 0.1 mm long
        let (w, h) = (b - a, d - c);
        assert!(close(w.min(h), 100.0) && close(w.max(h), 10100.0), "{k:?}");
    }
    // the filled circle on B.Cu: its body, then strips around its chords whose
    // inner edge lies 0.05 mm (plus the maximum error) outside the 2 mm circle
    assert_eq!(grown[5], plain[1]);
    let circle: Vec<_> = grown.iter().filter(|k| k.0 == "B.Cu").skip(1).collect();
    assert!(circle.len() > 16);
    let inside = |poly: &[(f64, f64)], p: (f64, f64)| {
        let crosses: Vec<f64> = poly
            .windows(2)
            .map(|w| (w[1].0 - w[0].0) * (p.1 - w[0].1) - (w[1].1 - w[0].1) * (p.0 - w[0].0))
            .collect();
        crosses.iter().all(|c| *c >= 0.0) || crosses.iter().all(|c| *c <= 0.0)
    };
    for i in 0..360 {
        let a = f64::from(i).to_radians();
        let p = (35000.0 + 2049.9 * a.cos(), -25000.0 + 2049.9 * a.sin());
        assert!(circle.iter().any(|k| inside(&k.1, p)), "{p:?} uncovered");
    }
    for k in &circle {
        for p in &k.1 {
            let r = (p.0 - 35000.0).hypot(p.1 + 25000.0);
            assert!(r < 2120.0, "a strip corner at {r} µm");
        }
    }
    // the 0.4 mm line: the rectangle within 0.2 + 0.05 mm of it
    let line = &grown.last().unwrap().1;
    let ((x0, x1), (y0, y1)) = span(line);
    assert!(close(x0, 4750.0) && close(x1, 15250.0), "{line:?}");
    assert!(close(y0, -30250.0) && close(y1, -29750.0), "{line:?}");
    assert_eq!(grown.len(), 1 + 4 + 1 + circle.len() + 1);
}
