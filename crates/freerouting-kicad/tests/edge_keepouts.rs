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
