//! Q18.5: the Edge.Cuts and Margin shapes KiCad's DSN export leaves out, as
//! copper keepouts (decided 2026-10-20: routing never breaks the board's
//! manufacturing rules, graded under the board's own rules).
//!
//! KiCad's DRC (`copper_edge_clearance`) keeps copper `m_CopperEdgeClearance`
//! from every shape on Edge.Cuts (from its centreline: the DRC counts no
//! stroke there) and on Margin (from the edge of its stroke). The export, and
//! [`crate::outline`] after it, carries only the board outline: the outer
//! contours as the boundary, which the router keeps
//! `router.copper_to_edge_clearance_um` from (a `.kicad_pcb` job sets it from
//! the `.kicad_pro`, Q18.3), and the holes as `signal` keepouts, which the
//! router keeps only its default clearance from. Margin shapes, Edge.Cuts
//! lines that close no contour (all of Edge.Cuts when the outline does not
//! close and the bounding box stands in for it) and the edge clearance of the
//! holes are invisible to the router.
//!
//! [`edge_keepouts`] makes each such shape keepouts on every copper layer,
//! named [`KEEPOUT_NAME`]: each straight piece (arcs, circles and curves cut
//! into chords at the board's maximum error) becomes the rectangle within `w`
//! of it, reaching `w` past both ends, where `w` = the edge clearance + half
//! the stroke (Margin only) - the DSN's default clearance (what the router
//! keeps from a keepout), at least [`MIN_HALF_NM`], plus the maximum error and
//! 1 µm for curves. An Edge.Cuts piece on an outer contour (both ends and the
//! middle within [`ON_OUTLINE_NM`]) is skipped: the router's own edge
//! clearance covers it. The KiCad plugin does the same to KiCad's export
//! (`kicad-plugin/freerouting_rs/edge_graphics.py`); the plugin rounds the
//! corners to 1 µm, the reader keeps nanometres like its other shapes.
//! `docs/DIVERGENCES.md` lists the difference from KiCad's export.
//!
//! Q18.7: KiCad's DRC (`solder_mask_bridge`) also flags copper of two nets in
//! one mask aperture. A board-level F.Mask or B.Mask shape (an exposed-copper
//! area no pad owns) becomes keepouts named [`MASK_KEEPOUT_NAME`] on the
//! copper layer of its side ([`mask_polygons`], [`mask_keepouts`]): a filled
//! shape is its own polygon (a circle the maximum error and 1 µm wider), and
//! its stroke, or an unfilled shape's, the rectangles within half the width of
//! each piece. The keepout holds every net out, the net of the copper under
//! the aperture too.

use freerouting_dsn::ast::{Area, Keepout, KeepoutKind, LayerRef, Point, Shape};

use crate::arc::ShapeArc;
use crate::footprints::{Footprint, GKind, Graphic};
use crate::legacy::{Angle, Pt};
use crate::outline::PolyOutline;
use crate::pads::bezier_points;
use crate::units::ki_round;

/// The name of the keepouts, so a reader of the DSN can tell them apart.
pub const KEEPOUT_NAME: &str = "edge_clearance";
/// The name of the board's mask-aperture keepouts (Q18.7).
pub const MASK_KEEPOUT_NAME: &str = "solder_mask";
/// A keepout is at least the line itself (1 µm on each side).
pub const MIN_HALF_NM: f64 = 1_000.0;
/// An Edge.Cuts piece this close to an outer contour is the outline.
pub const ON_OUTLINE_NM: f64 = 20_000.0;

pub(crate) type F = (f64, f64);

fn f(p: Pt) -> F {
    (f64::from(p.0), f64::from(p.1))
}

/// The straight pieces of a graphic, and whether it is a curve.
fn pieces(g: &Graphic, max_error: i32) -> (Vec<(F, F)>, bool) {
    let open = |pts: &[Pt]| -> Vec<(F, F)> { pts.windows(2).map(|w| (f(w[0]), f(w[1]))).collect() };
    let closed = |pts: &[Pt]| -> Vec<(F, F)> {
        (0..pts.len())
            .map(|i| (f(pts[i]), f(pts[(i + 1) % pts.len()])))
            .filter(|(p, q)| p != q)
            .collect()
    };
    match &g.kind {
        GKind::Segment { start, end } => (vec![(f(*start), f(*end))], false),
        GKind::Rect { start, end } => (
            closed(&[*start, (end.0, start.1), *end, (start.0, end.1)]),
            false,
        ),
        GKind::Poly(pts) => (closed(pts), false),
        GKind::Circle { start, end } => {
            let r = ki_round(f64::from(end.0 - start.0).hypot(f64::from(end.1 - start.1))).max(1);
            let arc = ShapeArc::from_centre_start_angle(
                *start,
                (start.0.wrapping_add(r), start.1),
                Angle(360.0),
            );
            (closed(&arc.to_polyline(max_error)), true)
        }
        GKind::Arc { start, end, .. } => {
            let mid = g.arc_mid().unwrap_or(*start);
            let arc = ShapeArc::from_three_points(*start, mid, *end);
            (open(&arc.to_polyline(max_error)), true)
        }
        GKind::Bezier { start, c1, c2, end } => (
            open(&bezier_points(*start, *c1, *c2, *end, max_error)),
            true,
        ),
    }
}

fn distance(p: F, a: F, b: F) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 {
        0.0
    } else {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0)
    };
    (p.0 - a.0 - t * dx).hypot(p.1 - a.1 - t * dy)
}

fn on_outline(p: F, q: F, outline: &[PolyOutline]) -> bool {
    let mid = ((p.0 + q.0) / 2.0, (p.1 + q.1) / 2.0);
    outline.iter().any(|o| {
        let n = o.outer.len();
        let near = |x: F| {
            (0..n).any(|i| distance(x, f(o.outer[i]), f(o.outer[(i + 1) % n])) <= ON_OUTLINE_NM)
        };
        near(p) && near(q) && near(mid)
    })
}

/// The rectangle within `w` of the segment p-q, reaching `w` past both ends.
fn quad(p: F, q: F, w: f64) -> [F; 4] {
    let (mut dx, mut dy) = (q.0 - p.0, q.1 - p.1);
    let mut len = dx.hypot(dy);
    if len == 0.0 {
        (dx, dy, len) = (1.0, 0.0, 1.0);
    }
    let (ux, uy) = (dx / len * w, dy / len * w);
    let (nx, ny) = (-uy, ux);
    let a = (p.0 - ux, p.1 - uy);
    let b = (q.0 + ux, q.1 + uy);
    [
        (a.0 + nx, a.1 + ny),
        (b.0 + nx, b.1 + ny),
        (b.0 - nx, b.1 - ny),
        (a.0 - nx, a.1 - ny),
    ]
}

/// The Edge.Cuts and Margin graphics of the board (`board_shapes`, every
/// layer) and its footprints as the file has them (before the flip), in board
/// then footprint order.
pub fn edge_graphics(board_shapes: &[Graphic], footprints: &[Footprint]) -> Vec<Graphic> {
    board_shapes
        .iter()
        .chain(footprints.iter().flat_map(|fp| fp.graphics.iter()))
        .filter(|g| g.layer == "Edge.Cuts" || g.layer == "Margin")
        .cloned()
        .collect()
}

/// The keepout rectangles (nm, KiCad's frame) of [`edge_graphics`], with the
/// number of shapes they come from.
pub fn edge_rects(
    graphics: &[Graphic],
    outline: &[PolyOutline],
    edge_clearance_nm: i32,
    default_clearance_nm: i32,
    max_error: i32,
) -> (Vec<[F; 4]>, usize) {
    let edge = f64::from(edge_clearance_nm.max(0));
    let mut rects = Vec::new();
    let mut shapes = 0;
    for g in graphics {
        let (mut pieces, curved) = pieces(g, max_error);
        let margin = g.layer == "Margin";
        if !margin {
            pieces.retain(|(p, q)| !on_outline(*p, *q, outline));
        }
        if pieces.is_empty() {
            continue;
        }
        let half_stroke = if margin {
            f64::from(g.width) / 2.0
        } else {
            0.0
        };
        let mut w = (edge + half_stroke - f64::from(default_clearance_nm)).max(MIN_HALF_NM);
        if curved {
            w += f64::from(max_error) + 1_000.0;
        }
        rects.extend(pieces.iter().map(|(p, q)| quad(*p, *q, w)));
        shapes += 1;
    }
    (rects, shapes)
}

/// A closed polygon keepout (KiCad's frame, nm) in the DSN frame (µm, Y
/// negated).
pub(crate) fn keepout(name: &str, layer: &str, poly: &[F]) -> Keepout {
    let pt = |p: F| Point::new(um_f(p.0), -um_f(p.1));
    Keepout {
        kind: KeepoutKind::Keepout,
        area: Area {
            name: Some(name.to_owned()),
            shape: Shape::Polygon {
                layer: LayerRef::Named(layer.to_owned()),
                aperture_width: 0.0,
                points: poly.iter().chain(poly.first()).map(|p| pt(*p)).collect(),
            },
            windows: Vec::new(),
            clearance_class: None,
        },
    }
}

/// [`edge_rects`] as DSN keepouts: every rectangle on every layer of
/// `layers`, layer by layer, in the DSN frame (µm, Y negated).
pub fn edge_keepouts(rects: &[[F; 4]], layers: &[String]) -> Vec<Keepout> {
    layers
        .iter()
        .flat_map(|layer| rects.iter().map(move |r| keepout(KEEPOUT_NAME, layer, r)))
        .collect()
}

/// Q18.7: the polygons (nm, KiCad's frame) of the board-level F.Mask and
/// B.Mask shapes of `board_shapes`, each with its side (`true` for B.Mask), in
/// file order.
pub fn mask_polygons(board_shapes: &[Graphic], max_error: i32) -> Vec<(bool, Vec<F>)> {
    let mut out = Vec::new();
    for g in board_shapes {
        let back = match g.layer.as_str() {
            "F.Mask" => false,
            "B.Mask" => true,
            _ => continue,
        };
        let (body, strokes) = shape_polygons(g, max_error);
        out.extend(body.into_iter().map(|b| (back, b)));
        out.extend(strokes.into_iter().map(|s| (back, s)));
    }
    out
}

/// The filled body of a closed shape (a polygon, rectangle or circle; a
/// circle the maximum error and 1 µm wider), if it is filled, and the
/// rectangles within half the stroke of each straight piece (curves cut into
/// chords at the maximum error, then the maximum error and 1 µm wider),
/// unless a filled shape has no stroke. KiCad's frame, nm. Q18.7's mask
/// apertures and Q22.4's copper graphics.
pub(crate) fn shape_polygons(g: &Graphic, max_error: i32) -> (Option<Vec<F>>, Vec<Vec<F>>) {
    let mut body = None;
    if g.filled {
        let pts: Vec<F> = match &g.kind {
            GKind::Poly(pts) => pts.iter().map(|p| f(*p)).collect(),
            GKind::Rect { start, end } => {
                vec![f(*start), f((end.0, start.1)), f(*end), f((start.0, end.1))]
            }
            GKind::Circle { start, end } => {
                let r = f64::from(end.0 - start.0).hypot(f64::from(end.1 - start.1))
                    + f64::from(max_error)
                    + 1_000.0;
                let arc = ShapeArc::from_centre_start_angle(
                    *start,
                    (start.0.wrapping_add(ki_round(r)), start.1),
                    Angle(360.0),
                );
                arc.to_polyline(max_error).into_iter().map(f).collect()
            }
            _ => Vec::new(),
        };
        if pts.len() >= 3 {
            body = Some(pts);
        }
        if g.width <= 0 {
            return (body, Vec::new());
        }
    }
    let (pieces, curved) = pieces(g, max_error);
    let mut w = (f64::from(g.width) / 2.0).max(MIN_HALF_NM);
    if curved {
        w += f64::from(max_error) + 1_000.0;
    }
    let strokes = pieces
        .iter()
        .map(|(p, q)| quad(*p, *q, w).to_vec())
        .collect();
    (body, strokes)
}

/// [`mask_polygons`] as DSN keepouts on `front` (F.Mask) or `back` (B.Mask).
pub fn mask_keepouts(polygons: &[(bool, Vec<F>)], front: &str, back: &str) -> Vec<Keepout> {
    polygons
        .iter()
        .map(|(is_back, poly)| {
            keepout(MASK_KEEPOUT_NAME, if *is_back { back } else { front }, poly)
        })
        .collect()
}

/// nm (as a float) to the DSN's µm, [`crate::units::um`]'s scale.
fn um_f(nm: f64) -> f64 {
    let v = nm / 1000.0;
    if v == 0.0 { 0.0 } else { v }
}
