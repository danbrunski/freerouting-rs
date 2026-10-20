//! Pads to padstacks and image pins (K05.6).
//!
//! A port of `SPECCTRA_DB::makePADSTACK` and the pad loop of
//! `SPECCTRA_DB::makeIMAGE` (`specctra_export.cpp` at tag `10.0.0`,
//! GPL-2.0-or-later): the same shapes, names and pin numbering.
//!
//! - Shapes are built in the pad's own frame; the rotation goes on the pin.
//! - Every padstack has `(attach off)`.
//! - A custom pad becomes the **convex hull** of its anchor and primitives:
//!   KiCad's exporter has `#define EXPORT_CUSTOM_PADS_CONVEX_HULL` set, so
//!   the exact union is never written. The hull of a union equals the hull of
//!   the parts, so no polygon-union library is needed (the checklist's
//!   `i_overlay` is not used). The name ends in a hash of the hull instead of
//!   KiCad's 128-bit hash of the merged polygon.
//! - A circular pad without copper (or whose hole is at least as large as the
//!   pad) is a keepout circle on every copper layer, not a pin.

use std::collections::BTreeMap;

use freerouting_dsn::ast::{LayerRef, Padstack, Point, Shape};

use crate::arc::{
    ARC_HIGH_DEF, ErrorLoc, circle_polygon, convex_hull, round_chamfered_rect,
    transform_arc_to_polygon, transform_oval_to_polygon,
};
use crate::fmt6g::{c_fmt_fixed6, push_c_fmt_6g};
use crate::footprints::{Footprint, GKind, Graphic, Pad, PadShape, normalize_360};
use crate::layers::{CuMask, Layers};
use crate::legacy::{Angle, Pt, rotate_origin};
use crate::units::{fix_neg_zero, iu2um, ki_round, map_pt, um};

/// `isRoundKeepout`.
pub fn is_round_keepout(pad: &Pad, copper_count: usize) -> bool {
    pad.shape == PadShape::Circle
        && (pad.drill.0 >= pad.size.0 || !pad.layers.and(CuMask::all(copper_count)).any())
}

/// A padstack and the text KiCad hashes to compare padstacks.
#[derive(Clone, Debug, PartialEq)]
pub struct PadstackDef {
    pub padstack: Padstack,
    /// `makeHash()`: the printed contents without whitespace, parentheses
    /// and quotes.
    pub key: String,
}

/// The printed form of a shape, stripped like `StripUseless`.
///
/// The numbers are appended in place ([`push_c_fmt_6g`]): the reader keys
/// every outline shape of every footprint, 1.8 million numbers on
/// `jetson-agx-thor-baseboard` (K13.4).
pub fn shape_key(shape: &Shape) -> String {
    let mut s = String::with_capacity(64);
    let numbers = |s: &mut String, points: &[Point]| {
        for p in points {
            push_c_fmt_6g(s, p.x);
            push_c_fmt_6g(s, p.y);
        }
    };
    match shape {
        Shape::Rect {
            layer,
            x1,
            y1,
            x2,
            y2,
        } => {
            s.push_str("rect");
            s.push_str(layer.name());
            for v in [x1, y1, x2, y2] {
                push_c_fmt_6g(&mut s, *v);
            }
        }
        Shape::Circle {
            layer,
            diameter,
            center,
        } => {
            s.push_str("circle");
            s.push_str(layer.name());
            push_c_fmt_6g(&mut s, *diameter);
            if center.x != 0.0 || center.y != 0.0 {
                push_c_fmt_6g(&mut s, center.x);
                push_c_fmt_6g(&mut s, center.y);
            }
        }
        Shape::Polygon {
            layer,
            aperture_width,
            points,
        } => {
            s.push_str("polygon");
            s.push_str(layer.name());
            push_c_fmt_6g(&mut s, *aperture_width);
            numbers(&mut s, points);
        }
        Shape::Path {
            layer,
            width,
            points,
        } => {
            s.push_str("path");
            s.push_str(layer.name());
            push_c_fmt_6g(&mut s, *width);
            numbers(&mut s, points);
        }
        Shape::PolylinePath {
            layer,
            width,
            points,
        } => {
            s.push_str("polyline_path");
            s.push_str(layer.name());
            push_c_fmt_6g(&mut s, *width);
            numbers(&mut s, points);
        }
    }
    s
}

fn pt(x: f64, y: f64) -> Point {
    Point::new(fix_neg_zero(x), fix_neg_zero(y))
}

fn named(n: &str) -> LayerRef {
    LayerRef::Named(n.to_owned())
}

/// The polygon of a custom pad's anchor and primitives, in the pad's frame
/// (`MergePrimitivesAsPolygon` before the union): all their vertices.
fn custom_pad_points(pad: &Pad, error: i32) -> Vec<Pt> {
    let mut pts = Vec::new();
    let (sx, sy) = pad.size;
    if pad.anchor_rect {
        // SHAPE_RECT( -x/2, -y/2, x, y )
        let (x0, y0) = (-sx / 2, -sy / 2);
        pts.extend([(x0, y0), (x0 + sx, y0), (x0 + sx, y0 + sy), (x0, y0 + sy)]);
    } else {
        pts.extend(circle_polygon((0, 0), sx / 2, error, 8));
    }
    for g in &pad.primitives {
        primitive_points(g, error, &mut pts);
    }
    pts
}

/// `TransformShapeToPolygon` of a pad primitive, as vertices (`ERROR_INSIDE`).
fn primitive_points(g: &Graphic, error: i32, out: &mut Vec<Pt>) {
    let width = g.width;
    match &g.kind {
        GKind::Segment { start, end } => out.extend(transform_oval_to_polygon(
            *start,
            *end,
            width,
            error,
            ErrorLoc::Inside,
        )),
        GKind::Circle { start, end } => {
            let r = ki_round(f64::from(end.0 - start.0).hypot(f64::from(end.1 - start.1))).max(1);
            out.extend(circle_polygon(*start, r + width / 2, error, 8));
        }
        GKind::Rect { start, end } => {
            let c = [*start, (end.0, start.1), *end, (start.0, end.1)];
            if g.filled {
                out.extend(c);
            }
            if width > 0 || !g.filled {
                for i in 0..4 {
                    out.extend(transform_oval_to_polygon(
                        c[i],
                        c[(i + 1) % 4],
                        width,
                        error,
                        ErrorLoc::Inside,
                    ));
                }
            }
        }
        GKind::Arc { start, end, .. } => {
            let mid = g.arc_mid().unwrap_or(*start);
            out.extend(transform_arc_to_polygon(
                *start,
                mid,
                *end,
                width,
                error,
                ErrorLoc::Inside,
            ));
        }
        GKind::Poly(pts) => {
            out.extend(pts.iter().copied());
            if width > 0 {
                for i in 0..pts.len() {
                    out.extend(transform_oval_to_polygon(
                        pts[i],
                        pts[(i + 1) % pts.len()],
                        width,
                        error,
                        ErrorLoc::Inside,
                    ));
                }
            }
        }
        GKind::Bezier { start, c1, c2, end } => {
            let poly = bezier_points(*start, *c1, *c2, *end, error);
            for w in poly.windows(2) {
                out.extend(transform_oval_to_polygon(
                    w[0],
                    w[1],
                    width,
                    error,
                    ErrorLoc::Inside,
                ));
            }
        }
    }
}

/// A flattened cubic Bezier: recursive subdivision until the control points
/// lie within `error` of the chord (`BEZIER_POLY` makes the same kind of
/// approximation; the exact points differ, which matters only for custom pads
/// with curves).
pub fn bezier_points(p0: Pt, p1: Pt, p2: Pt, p3: Pt, error: i32) -> Vec<Pt> {
    fn flat(a: (f64, f64), b: (f64, f64), c: (f64, f64), d: (f64, f64), tol: f64) -> bool {
        let dist = |p: (f64, f64)| {
            let (dx, dy) = (d.0 - a.0, d.1 - a.1);
            let len = dx.hypot(dy);
            if len == 0.0 {
                (p.0 - a.0).hypot(p.1 - a.1)
            } else {
                ((p.0 - a.0) * dy - (p.1 - a.1) * dx).abs() / len
            }
        };
        dist(b) <= tol && dist(c) <= tol
    }
    fn rec(
        a: (f64, f64),
        b: (f64, f64),
        c: (f64, f64),
        d: (f64, f64),
        tol: f64,
        depth: u32,
        out: &mut Vec<Pt>,
    ) {
        if depth >= 16 || flat(a, b, c, d, tol) {
            out.push((ki_round(d.0), ki_round(d.1)));
            return;
        }
        let m = |p: (f64, f64), q: (f64, f64)| ((p.0 + q.0) / 2.0, (p.1 + q.1) / 2.0);
        let (ab, bc, cd) = (m(a, b), m(b, c), m(c, d));
        let (abc, bcd) = (m(ab, bc), m(bc, cd));
        let abcd = m(abc, bcd);
        rec(a, ab, abc, abcd, tol, depth + 1, out);
        rec(abcd, bcd, cd, d, tol, depth + 1, out);
    }
    let f = |p: Pt| (f64::from(p.0), f64::from(p.1));
    let mut out = vec![p0];
    rec(
        f(p0),
        f(p1),
        f(p2),
        f(p3),
        f64::from(error.max(1)) / 2.0,
        0,
        &mut out,
    );
    out
}

/// 128 bits of FNV-1a over the hull, as 32 hex digits.
fn hull_hash(points: &[Pt]) -> String {
    let mut h: u128 = 0x6c62272e07bb014262b821756295c58d;
    for p in points {
        for b in p.0.to_le_bytes().into_iter().chain(p.1.to_le_bytes()) {
            h ^= u128::from(b);
            h = h.wrapping_mul(0x0000000001000000000000000000013b);
        }
    }
    format!("{h:032x}")
}

/// `makePADSTACK`.
pub fn make_padstack(pad: &Pad, layers: &Layers, max_error: i32) -> PadstackDef {
    let count = layers.count();
    let all_cu = CuMask::all(count);
    let on_all = pad.layers.and(all_cu) == all_cu;
    let mut layer_names: Vec<&str> = Vec::new();
    let mut uniq = String::from("[");
    if on_all {
        uniq.push('A');
    }
    for layer in 0..count {
        let m = CuMask::at(layer, count);
        if on_all || pad.layers.and(m).any() {
            layer_names.push(layers.name(layer));
            if !on_all {
                if layer == 0 {
                    uniq.push('T');
                } else if layer == count - 1 {
                    uniq.push('B');
                } else {
                    uniq.push(char::from(b'0' + layer as u8));
                }
            }
        }
    }
    uniq.push(']');
    let (sx, sy) = pad.size;
    let mut dsn_offset = Point::new(0.0, 0.0);
    if pad.offset.0 != 0 || pad.offset.1 != 0 {
        dsn_offset = map_pt(i64::from(pad.offset.0), i64::from(pad.offset.1));
        uniq.push_str(&format!(
            "[{},{}]",
            c_fmt_fixed6(dsn_offset.x),
            c_fmt_fixed6(dsn_offset.y)
        ));
    }
    let off = |p: Point| Point::new(p.x + dsn_offset.x, p.y + dsn_offset.y);
    let mut shapes: Vec<Shape> = Vec::new();
    let name;
    let fx = |v: i32| c_fmt_fixed6(iu2um(i64::from(v)));
    match pad.shape {
        PadShape::Circle => {
            let diameter = um(i64::from(sx));
            for n in &layer_names {
                shapes.push(Shape::Circle {
                    layer: named(n),
                    diameter,
                    center: dsn_offset,
                });
            }
            name = format!("Round{uniq}Pad_{}_um", fx(sx));
        }
        PadShape::Rect => {
            let dx = um(i64::from(sx)) / 2.0;
            let dy = um(i64::from(sy)) / 2.0;
            let ll = off(Point::new(-dx, -dy));
            let ur = off(Point::new(dx, dy));
            for n in &layer_names {
                shapes.push(Shape::Rect {
                    layer: named(n),
                    x1: fix_neg_zero(ll.x),
                    y1: fix_neg_zero(ll.y),
                    x2: fix_neg_zero(ur.x),
                    y2: fix_neg_zero(ur.y),
                });
            }
            name = format!("Rect{uniq}Pad_{}x{}_um", fx(sx), fx(sy));
        }
        PadShape::Oval => {
            let dx = um(i64::from(sx)) / 2.0;
            let dy = um(i64::from(sy)) / 2.0;
            let mut dr = dx - dy;
            let (radius, a, b);
            if dr >= 0.0 {
                radius = dy;
                a = Point::new(-dr, 0.0);
                b = Point::new(dr, 0.0);
            } else {
                radius = dx;
                dr = -dr;
                a = Point::new(0.0, -dr);
                b = Point::new(0.0, dr);
            }
            let (a, b) = (off(a), off(b));
            for n in &layer_names {
                shapes.push(Shape::Path {
                    layer: named(n),
                    width: 2.0 * radius,
                    points: vec![pt(a.x, a.y), pt(b.x, b.y)],
                });
            }
            name = format!("Oval{uniq}Pad_{}x{}_um", fx(sx), fx(sy));
        }
        PadShape::Trapezoid => {
            let dx = um(i64::from(sx)) / 2.0;
            let dy = um(i64::from(sy)) / 2.0;
            let (delta_x, delta_y) = pad.delta;
            let ddx = um(i64::from(delta_x)) / 2.0;
            let ddy = um(i64::from(delta_y)) / 2.0;
            let pts = [
                off(Point::new(-dx - ddy, -dy - ddx)),
                off(Point::new(-dx + ddy, dy + ddx)),
                off(Point::new(dx - ddy, dy - ddx)),
                off(Point::new(dx + ddy, -dy + ddx)),
            ];
            for n in &layer_names {
                shapes.push(Shape::Polygon {
                    layer: named(n),
                    aperture_width: 0.0,
                    points: pts.to_vec(),
                });
            }
            let sign = |v: i32| if v < 0 { "n" } else { "p" };
            name = format!(
                "Trapz{uniq}Pad_{}x{}_{}{}x{}{}_um",
                fx(sx),
                fx(sy),
                sign(delta_x),
                c_fmt_fixed6(iu2um(i64::from(delta_x)).abs()),
                sign(delta_y),
                c_fmt_fixed6(iu2um(i64::from(delta_y)).abs()),
            );
        }
        PadShape::RoundRect | PadShape::ChamferedRect => {
            let rradius0 = ki_round(f64::from(sx.min(sy)) * pad.rratio);
            let correction = (std::f64::consts::PI / 36.0).cos();
            let extra = ki_round(f64::from(rradius0) * (1.0 - correction));
            let psize = (sx + extra * 2, sy + extra * 2);
            let rradius = rradius0 + extra;
            let do_chamfer = pad.shape == PadShape::ChamferedRect;
            let poly = round_chamfered_rect(
                psize,
                rradius,
                pad.chamfer_ratio,
                if do_chamfer { pad.chamfer } else { 0 },
                max_error,
            );
            let mut points: Vec<Point> = poly
                .iter()
                .map(|p| {
                    off(Point::new(
                        um(i64::from(p.0)),
                        um(i64::from(p.1.wrapping_neg())),
                    ))
                })
                .collect();
            if let Some(first) = points.first().copied() {
                points.push(first);
            }
            for n in &layer_names {
                shapes.push(Shape::Polygon {
                    layer: named(n),
                    aperture_width: 0.0,
                    points: points.clone(),
                });
            }
            name = format!(
                "RoundRect{uniq}Pad_{}x{}_{}_um_{}_{:X}",
                fx(sx),
                fx(sy),
                fx(rradius),
                c_fmt_fixed6(if do_chamfer { pad.chamfer_ratio } else { 0.0 }),
                if do_chamfer { pad.chamfer } else { 0 },
            );
        }
        PadShape::Custom => {
            let all = custom_pad_points(pad, max_error);
            let mut hull = convex_hull(&all);
            if hull.first() != hull.last() {
                if let Some(f) = hull.first().copied() {
                    hull.push(f);
                }
            }
            // the exporter prints the size after the polygon is closed
            let count_hull = hull.len();
            let hash = hull_hash(&hull);
            let points: Vec<Point> = hull
                .iter()
                .map(|p| {
                    off(Point::new(
                        um(i64::from(p.0)),
                        um(i64::from(p.1.wrapping_neg())),
                    ))
                })
                .collect();
            for n in &layer_names {
                shapes.push(Shape::Polygon {
                    layer: named(n),
                    aperture_width: 0.0,
                    points: points.clone(),
                });
            }
            // GetBoundingBox: the hull in the pad's orientation
            let rot: Vec<Pt> = hull
                .iter()
                .map(|p| rotate_origin(p.0, p.1, Angle(pad.orient)))
                .collect();
            let (minx, maxx) = (
                rot.iter().map(|p| p.0).min().unwrap_or(0),
                rot.iter().map(|p| p.0).max().unwrap_or(0),
            );
            let (miny, maxy) = (
                rot.iter().map(|p| p.1).min().unwrap_or(0),
                rot.iter().map(|p| p.1).max().unwrap_or(0),
            );
            name = format!(
                "Cust{uniq}Pad_{}x{}_{}x{}_{}_um_{hash}",
                fx(sx),
                fx(sy),
                fx(maxx - minx),
                fx(maxy - miny),
                count_hull,
            );
        }
    }
    let key = format!(
        "{}attachoff",
        shapes
            .iter()
            .map(|s| format!("shape{}", shape_key(s)))
            .collect::<String>()
    );
    PadstackDef {
        padstack: Padstack {
            name,
            shapes,
            attach: Some(false),
            absolute: None,
        },
        key,
    }
}

/// `m_padstackset`: padstacks ordered by contents, then name
/// (`PADSTACK::Compare`), each once.
#[derive(Clone, Debug, Default)]
pub struct PadstackSet {
    set: BTreeMap<(String, String), Padstack>,
}

impl PadstackSet {
    pub fn insert(&mut self, def: &PadstackDef) {
        self.set
            .entry((def.key.clone(), def.padstack.name.clone()))
            .or_insert_with(|| def.padstack.clone());
    }

    pub fn into_padstacks(self) -> Vec<Padstack> {
        self.set.into_values().collect()
    }
}

/// An image pin before it is named in the netlist.
#[derive(Clone, Debug, PartialEq)]
pub struct PinOut {
    pub padstack: String,
    /// The pin id with the `@n` suffix of repeated numbers.
    pub id: String,
    pub rotation: f64,
    pub at: Point,
    pub net: u32,
    pub pad_index: usize,
}

/// A copper-less round hole: a keepout circle per copper layer.
#[derive(Clone, Debug, PartialEq)]
pub struct HoleKeepout {
    pub diameter: f64,
    pub at: Point,
}

/// What the pads of one footprint contribute to its image.
#[derive(Clone, Debug, Default)]
pub struct PadsOut {
    pub pins: Vec<PinOut>,
    pub keepouts: Vec<HoleKeepout>,
    pub padstacks: Vec<PadstackDef>,
}

/// The pad loop of `makeIMAGE`. `fp` must already be flipped to the front.
pub fn make_pads(fp: &Footprint, layers: &Layers, hole_clearance: i32, max_error: i32) -> PadsOut {
    let count = layers.count();
    let mut out = PadsOut::default();
    let mut pinmap: BTreeMap<String, u32> = BTreeMap::new();
    for (index, pad) in fp.pads.iter().enumerate() {
        if is_round_keepout(pad, count) {
            let rel = fp.relative(pad.pos);
            let at = map_pt(i64::from(rel.0), i64::from(rel.1));
            let diameter = um(i64::from(pad.drill.0)) + um(i64::from(hole_clearance) * 2);
            out.keepouts.push(HoleKeepout { diameter, at });
            continue;
        }
        if !pad.layers.and(CuMask::all(count)).any() {
            continue;
        }
        let def = make_padstack(pad, layers, max_error);
        let padstack_name = def.padstack.name.clone();
        out.padstacks.push(def);
        let mut id = pad.number.clone();
        if !pad.number.is_empty() && !pinmap.contains_key(&pad.number) {
            pinmap.insert(pad.number.clone(), 0);
        } else {
            let n = pinmap.entry(pad.number.clone()).or_insert(0);
            *n += 1;
            id.push_str(&format!("@{n}"));
        }
        let angle = normalize_360(pad.orient - fp.orient);
        let rel = fp.relative(pad.pos);
        out.pins.push(PinOut {
            padstack: padstack_name,
            id,
            rotation: angle,
            at: map_pt(i64::from(rel.0), i64::from(rel.1)),
            net: pad.net,
            pad_index: index,
        });
    }
    out
}

/// The board's `m_MaxError` default, used for pad polygons.
pub const DEFAULT_MAX_ERROR: i32 = ARC_HIGH_DEF;
