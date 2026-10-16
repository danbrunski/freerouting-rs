//! Comparison of the reader's tree with the tree of KiCad's own DSN export
//! (`*.kicad10.dsn`), category by category. Each function returns the
//! differences as text; an empty list means the category agrees.
#![allow(dead_code)]

use freerouting_dsn::ast::{Image, Padstack, Pcb, Point, Rule, Shape};

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

/// `%.6g` rounding step of `v`: the tolerance for a value KiCad printed.
pub fn g6_tol(v: f64) -> f64 {
    let a = v.abs();
    if a < 1e-12 {
        return 1e-6;
    }
    let exp = a.log10().floor();
    0.5 * 10f64.powf(exp - 5.0) * 1.0001
}

fn pt_close(a: Point, b: Point, tol: f64) -> bool {
    close(a.x, b.x, tol.max(g6_tol(b.x))) && close(a.y, b.y, tol.max(g6_tol(b.y)))
}

pub fn layers(r: &Pcb, e: &Pcb) -> Vec<String> {
    let (a, b) = (
        &r.structure.as_ref().unwrap().layers,
        &e.structure.as_ref().unwrap().layers,
    );
    let mut out = Vec::new();
    if a.len() != b.len() {
        out.push(format!("layer count {} vs {}", a.len(), b.len()));
    }
    for (x, y) in a.iter().zip(b) {
        if x.name != y.name || x.layer_type != y.layer_type {
            out.push(format!(
                "layer {:?}/{:?} vs {:?}/{:?}",
                x.name, x.layer_type, y.name, y.layer_type
            ));
        }
    }
    out
}

pub fn network(r: &Pcb, e: &Pcb) -> Vec<String> {
    let (a, b) = (r.network.as_ref().unwrap(), e.network.as_ref().unwrap());
    let mut out = Vec::new();
    if a.nets.len() != b.nets.len() {
        out.push(format!("net count {} vs {}", a.nets.len(), b.nets.len()));
    }
    for (x, y) in a.nets.iter().zip(&b.nets) {
        if x.name != y.name {
            out.push(format!("net name {:?} vs {:?}", x.name, y.name));
            break;
        }
        if x.pins != y.pins {
            out.push(format!(
                "net {:?} pins {:?} vs {:?}",
                x.name, x.pins, y.pins
            ));
        }
    }
    out
}

fn rule_vals(rules: &[Rule]) -> (Option<f64>, Option<f64>, Option<f64>) {
    let (mut w, mut c, mut s) = (None, None, None);
    for r in rules {
        match r {
            Rule::Width(v) => w = Some(*v),
            Rule::Clearance { value, types } if types.is_empty() => c = Some(*value),
            Rule::Clearance { value, .. } => s = Some(*value),
            _ => {}
        }
    }
    (w, c, s)
}

/// Net classes. `sorted`: compare by class name instead of position (KiCad's
/// order of non-default classes is not reproducible).
pub fn classes(r: &Pcb, e: &Pcb) -> Vec<String> {
    let (a, b) = (r.network.as_ref().unwrap(), e.network.as_ref().unwrap());
    let mut out = Vec::new();
    let mut an: Vec<_> = a.classes.iter().collect();
    let mut bn: Vec<_> = b.classes.iter().collect();
    an.sort_by(|x, y| x.name.cmp(&y.name));
    bn.sort_by(|x, y| x.name.cmp(&y.name));
    if an.len() != bn.len() {
        out.push(format!(
            "class count {} vs {}: {:?} / {:?}",
            an.len(),
            bn.len(),
            an.iter().map(|c| &c.name).collect::<Vec<_>>(),
            bn.iter().map(|c| &c.name).collect::<Vec<_>>()
        ));
    }
    for (x, y) in an.iter().zip(&bn) {
        if x.name != y.name {
            out.push(format!("class name {:?} vs {:?}", x.name, y.name));
            continue;
        }
        let mut xn = x.nets.clone();
        let mut yn = y.nets.clone();
        xn.sort();
        yn.sort();
        if xn != yn {
            out.push(format!(
                "class {:?} nets differ: {} vs {}",
                x.name,
                xn.len(),
                yn.len()
            ));
        }
        let (xw, xc, _) = rule_vals(&x.rules);
        let (yw, yc, _) = rule_vals(&y.rules);
        if !(close(
            xw.unwrap_or(f64::NAN),
            yw.unwrap_or(f64::NAN),
            g6_tol(yw.unwrap_or(1.0)),
        ) && close(
            xc.unwrap_or(f64::NAN),
            yc.unwrap_or(f64::NAN),
            g6_tol(yc.unwrap_or(1.0)),
        )) {
            out.push(format!(
                "class {:?} rules {xw:?}/{xc:?} vs {yw:?}/{yc:?}",
                x.name
            ));
        }
        let xv = x.circuit.as_ref().map(|c| c.use_via.clone());
        let yv = y.circuit.as_ref().map(|c| c.use_via.clone());
        if xv != yv {
            out.push(format!("class {:?} use_via {xv:?} vs {yv:?}", x.name));
        }
    }
    out
}

pub fn structure_rules(r: &Pcb, e: &Pcb) -> Vec<String> {
    let (a, b) = (r.structure.as_ref().unwrap(), e.structure.as_ref().unwrap());
    let mut out = Vec::new();
    let (xa, xb) = (rule_vals(&a.rules), rule_vals(&b.rules));
    for (n, p, q) in [
        ("width", xa.0, xb.0),
        ("clearance", xa.1, xb.1),
        ("smd_smd", xa.2, xb.2),
    ] {
        if !close(
            p.unwrap_or(f64::NAN),
            q.unwrap_or(f64::NAN),
            g6_tol(q.unwrap_or(1.0)),
        ) {
            out.push(format!("rule {n}: {p:?} vs {q:?}"));
        }
    }
    let va = a.via.as_ref().map(|v| v.padstacks.clone());
    let vb = b.via.as_ref().map(|v| v.padstacks.clone());
    if va != vb {
        out.push(format!("via list {va:?} vs {vb:?}"));
    }
    out
}

fn area2(pts: &[Point]) -> f64 {
    let n = pts.len();
    (0..n)
        .map(|i| pts[i].x * pts[(i + 1) % n].y - pts[(i + 1) % n].x * pts[i].y)
        .sum::<f64>()
}

/// Boundary and hole keepouts: (areas, vertex counts).
pub fn boundary(r: &Pcb, e: &Pcb) -> (Vec<String>, String) {
    let (a, b) = (r.structure.as_ref().unwrap(), e.structure.as_ref().unwrap());
    let mut out = Vec::new();
    let pts = |s: &Shape| match s {
        Shape::Path { points, .. } | Shape::Polygon { points, .. } => points.clone(),
        _ => Vec::new(),
    };
    let sa: Vec<Vec<Point>> = a
        .boundaries
        .iter()
        .flat_map(|b| b.shapes.iter().map(pts))
        .collect();
    let sb: Vec<Vec<Point>> = b
        .boundaries
        .iter()
        .flat_map(|b| b.shapes.iter().map(pts))
        .collect();
    if sa.len() != sb.len() {
        out.push(format!("boundary paths {} vs {}", sa.len(), sb.len()));
    }
    let mut info = String::new();
    for (x, y) in sa.iter().zip(&sb) {
        let (ax, ay) = (area2(x).abs() / 2.0, area2(y).abs() / 2.0);
        info.push_str(&format!(
            "boundary vertices {} vs {} area {ax:.0} vs {ay:.0}; ",
            x.len(),
            y.len()
        ));
        if (ax - ay).abs() > ay * 0.001 {
            out.push(format!("boundary area {ax} vs {ay}"));
        }
    }
    (out, info)
}

/// The `signal` polygon keepouts of the structure: the holes of the outline
/// (rule areas are `keepout`s on a copper layer, K06.2).
pub fn outline_holes(p: &Pcb) -> Vec<Vec<Point>> {
    p.structure
        .as_ref()
        .unwrap()
        .keepouts
        .iter()
        .filter_map(|k| match &k.area.shape {
            Shape::Polygon { layer, points, .. }
                if *layer == freerouting_dsn::ast::LayerRef::Signal =>
            {
                Some(points.clone())
            }
            _ => None,
        })
        .collect()
}

pub fn placement(r: &Pcb, e: &Pcb) -> Vec<String> {
    let (a, b) = (r.placement.as_ref().unwrap(), e.placement.as_ref().unwrap());
    let mut out = Vec::new();
    if a.components.len() != b.components.len() {
        out.push(format!(
            "component count {} vs {}",
            a.components.len(),
            b.components.len()
        ));
    }
    for (x, y) in a.components.iter().zip(&b.components) {
        if x.image != y.image {
            out.push(format!("component image {:?} vs {:?}", x.image, y.image));
            continue;
        }
        if x.places.len() != y.places.len() {
            out.push(format!("{}: place count", x.image));
            continue;
        }
        for (p, q) in x.places.iter().zip(&y.places) {
            let (pl, ql) = (p.location.unwrap(), q.location.unwrap());
            let rot_d = ((pl.rotation - ql.rotation + 540.0).rem_euclid(360.0) - 180.0).abs();
            if p.name != q.name
                || pl.side != ql.side
                || !pt_close(pl.at, ql.at, 0.001)
                || rot_d > 1e-6 + g6_tol(ql.rotation)
                || p.part_number != q.part_number
            {
                out.push(format!(
                    "place {:?} vs {:?}",
                    (&p.name, pl.at, pl.side, pl.rotation, &p.part_number),
                    (&q.name, ql.at, ql.side, ql.rotation, &q.part_number)
                ));
            }
        }
    }
    out
}

fn shape_close(a: &Shape, b: &Shape, tol: f64) -> bool {
    match (a, b) {
        (
            Shape::Rect {
                layer: l1,
                x1,
                y1,
                x2,
                y2,
            },
            Shape::Rect {
                layer: l2,
                x1: a1,
                y1: b1,
                x2: a2,
                y2: b2,
            },
        ) => {
            l1 == l2
                && [(x1, a1), (y1, b1), (x2, a2), (y2, b2)]
                    .iter()
                    .all(|(p, q)| close(**p, **q, tol.max(g6_tol(**q))))
        }
        (
            Shape::Circle {
                layer: l1,
                diameter: d1,
                center: c1,
            },
            Shape::Circle {
                layer: l2,
                diameter: d2,
                center: c2,
            },
        ) => l1 == l2 && close(*d1, *d2, tol.max(g6_tol(*d2))) && pt_close(*c1, *c2, tol),
        (
            Shape::Polygon {
                layer: l1,
                aperture_width: w1,
                points: p1,
            },
            Shape::Polygon {
                layer: l2,
                aperture_width: w2,
                points: p2,
            },
        )
        | (
            Shape::Path {
                layer: l1,
                width: w1,
                points: p1,
            },
            Shape::Path {
                layer: l2,
                width: w2,
                points: p2,
            },
        ) => {
            l1 == l2
                && close(*w1, *w2, tol.max(g6_tol(*w2)))
                && p1.len() == p2.len()
                && p1.iter().zip(p2).all(|(a, b)| pt_close(*a, *b, tol))
        }
        _ => false,
    }
}

/// A padstack name with what the reader does not reproduce cut off: a
/// custom pad's bounding box and 128-bit hash (`Cust..Pad_WxH_BBW x BBH_N_um_HASH`).
pub fn comparable_padstack_name(name: &str) -> String {
    if name.starts_with("Cust") {
        let parts: Vec<&str> = name.split('_').collect();
        if parts.len() >= 6 {
            return format!("{}_{}_{}", parts[0], parts[1], parts[3]);
        }
    }
    name.to_owned()
}

pub fn padstacks(r: &Pcb, e: &Pcb) -> Vec<String> {
    let (a, b) = (
        &r.library.as_ref().unwrap().padstacks,
        &e.library.as_ref().unwrap().padstacks,
    );
    let mut out = Vec::new();
    let na: Vec<String> = a
        .iter()
        .map(|p| comparable_padstack_name(&p.name))
        .collect();
    let nb: Vec<String> = b
        .iter()
        .map(|p| comparable_padstack_name(&p.name))
        .collect();
    if na != nb {
        out.push(format!(
            "padstack names/order:\n  ours   {na:?}\n  kicad  {nb:?}"
        ));
        return out;
    }
    for (x, y) in a.iter().zip(b) {
        // custom pads: the hull within 1 um; others: %.6g rounding
        let tol = if y.name.starts_with("Cust") { 1.0 } else { 0.0 };
        if !padstack_close(x, y, tol) {
            out.push(format!(
                "padstack {:?} shapes differ:\n  ours  {:?}\n  kicad {:?}",
                y.name, x.shapes, y.shapes
            ));
        }
    }
    out
}

fn padstack_close(x: &Padstack, y: &Padstack, tol: f64) -> bool {
    x.attach == y.attach
        && x.shapes.len() == y.shapes.len()
        && x.shapes
            .iter()
            .zip(&y.shapes)
            .all(|(a, b)| shape_close(a, b, tol))
}

pub struct ImageReport {
    pub names: Vec<String>,
    pub pins: Vec<String>,
    pub outlines: Vec<String>,
    pub outline_points: Vec<String>,
    pub keepouts: Vec<String>,
}

pub fn images(r: &Pcb, e: &Pcb) -> ImageReport {
    let (a, b) = (
        &r.library.as_ref().unwrap().images,
        &e.library.as_ref().unwrap().images,
    );
    let mut rep = ImageReport {
        names: vec![],
        pins: vec![],
        outlines: vec![],
        outline_points: vec![],
        keepouts: vec![],
    };
    let na: Vec<&str> = a.iter().map(|p| p.name.as_str()).collect();
    let nb: Vec<&str> = b.iter().map(|p| p.name.as_str()).collect();
    if na != nb {
        rep.names.push(format!(
            "image names/order:\n  ours  {na:?}\n  kicad {nb:?}"
        ));
    }
    for y in b {
        let Some(x) = a.iter().find(|p| p.name == y.name) else {
            continue;
        };
        image_pair(x, y, &mut rep);
    }
    rep
}

fn image_pair(x: &Image, y: &Image, rep: &mut ImageReport) {
    if x.pins.len() != y.pins.len() {
        rep.pins.push(format!(
            "{}: pin count {} vs {}",
            y.name,
            x.pins.len(),
            y.pins.len()
        ));
    } else {
        for (p, q) in x.pins.iter().zip(&y.pins) {
            let rot = |v: Option<f64>| v.unwrap_or(0.0);
            let rd = ((rot(p.rotation) - rot(q.rotation) + 540.0).rem_euclid(360.0) - 180.0).abs();
            if p.name != q.name
                || comparable_padstack_name(&p.padstack) != comparable_padstack_name(&q.padstack)
                || !pt_close(p.at, q.at, 0.1)
                || rd > 1e-6 + g6_tol(rot(q.rotation))
            {
                rep.pins.push(format!("{}: pin {:?} vs {:?}", y.name, p, q));
            }
        }
    }
    if x.outlines.len() != y.outlines.len() {
        rep.outlines.push(format!(
            "{}: outline count {} vs {}",
            y.name,
            x.outlines.len(),
            y.outlines.len()
        ));
    } else {
        for (i, (p, q)) in x.outlines.iter().zip(&y.outlines).enumerate() {
            if !shape_close(p, q, 1.0) {
                rep.outline_points.push(format!(
                    "{}: outline {i}\n  ours  {:?}\n  kicad {:?}",
                    y.name, p, q
                ));
            }
        }
    }
    // hole keepouts are circles; footprint rule areas (polygons) are K06.3
    let circles = |i: &Image| -> Vec<freerouting_dsn::ast::Keepout> {
        i.keepouts
            .iter()
            .filter(|k| matches!(k.area.shape, Shape::Circle { .. }))
            .cloned()
            .collect()
    };
    let (xk, yk) = (circles(x), circles(y));
    if xk.len() != yk.len() {
        rep.keepouts.push(format!(
            "{}: keepout count {} vs {}",
            y.name,
            xk.len(),
            yk.len()
        ));
    } else {
        for (p, q) in xk.iter().zip(&yk) {
            if p.kind != q.kind || !shape_close(&p.area.shape, &q.area.shape, 0.1) {
                rep.keepouts
                    .push(format!("{}: keepout {:?} vs {:?}", y.name, p, q));
            }
        }
    }
}
