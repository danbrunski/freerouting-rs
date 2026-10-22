//! Image outlines (K05.7): the courtyard first, then every other footprint
//! graphic, as `makeIMAGE` writes them (`specctra_export.cpp`, tag `10.0.0`).
//!
//! - The courtyard is the footprint's F.CrtYd and B.CrtYd graphics chained
//!   into closed contours (`FOOTPRINT::BuildCourtyardCaches`, max error
//!   0.005 mm, chaining epsilon 0.02 mm), shrunk by the max error, front and
//!   back appended: one `(outline (polygon signal 0 ...))` per contour.
//! - Then, in file order: a segment is a `path` with its stroke width, a
//!   circle a `path` polyline (`ConvertArcToPolyline`, `ARC_HIGH_DEF`), a
//!   rectangle a closed `path`, an arc a `polygon` of the stroked arc
//!   (`TransformShapeToPolygon`). Polygons and Bezier curves are skipped,
//!   exactly as in KiCad's `default: continue`.
//! - Coordinates are relative to the footprint after the back-side flip
//!   (`mapPt( pt, footprint )`).
//!
//! These outlines are not obstacles, but they reach routing: every outline's
//! box enters the board's bounding box
//! (`BasicBoard::expand_bounding_box_to_include_all_items`), which bounds every
//! room the autorouter completes, and the courtyard boxes of the bounded
//! neck-down come from them (`quality::neckdown::bounded_neck`). They also feed
//! the GUI drawing and the board's component-outline count. An outline whose
//! placed shape is unbounded (bug 1 of `docs/development/pcu-rev2-board-report.md`,
//! a circle drawn as a polyline that rounds to a non-convex polygon) is left
//! out of the board box with a warning since K15.1. Not reproduced: the union of
//! overlapping front and back courtyards (`SHAPE_POLY_SET::Simplify`) and the
//! start point Clipper gives a contour; the points are the same.

use freerouting_dsn::ast::{LayerRef, Point, Shape};

use crate::arc::{
    ARC_HIGH_DEF, ErrorLoc, convert_arc_to_polyline_centre, transform_arc_to_polygon,
};
use crate::footprints::{Footprint, GKind, Graphic};
use crate::legacy::{Angle, Pt};
use crate::outline::{chain_contours, nest_contours, point_inside, simplify_contour};
use crate::units::{ki_round, map_pt, um};

type Pt2 = Vec<Pt>;

const COURTYARD_EPSILON: i32 = 20_000;

fn rel_pt(fp: &Footprint, p: Pt) -> Point {
    let r = fp.relative(p);
    map_pt(i64::from(r.0), i64::from(r.1))
}

/// Shrinks a contour by `d` nanometres (`SHAPE_POLY_SET::Inflate( -d,
/// CHAMFER_ACUTE_CORNERS )`): every edge moves to its left (the material
/// side of an outer contour with positive area), corners are mitred, and a
/// reflex corner sharper than the miter limit is chamfered.
pub fn shrink_contour(pts: &[Pt], d: i32) -> Vec<Pt> {
    let n = pts.len();
    if n < 3 {
        return pts.to_vec();
    }
    let dist = f64::from(d);
    let mut out = Vec::with_capacity(n + 2);
    let unit = |a: Pt, b: Pt| {
        let (dx, dy) = (f64::from(b.0 - a.0), f64::from(b.1 - a.1));
        let l = dx.hypot(dy);
        if l == 0.0 {
            (0.0, 0.0)
        } else {
            (dx / l, dy / l)
        }
    };
    for i in 0..n {
        let (a, b, c) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
        let (u0, u1) = (unit(a, b), unit(b, c));
        let n0 = (-u0.1, u0.0);
        let n1 = (-u1.1, u1.0);
        let dot = n0.0 * n1.0 + n0.1 * n1.1;
        let cross = u0.0 * u1.1 - u0.1 * u1.0;
        let (bx, by) = (f64::from(b.0), f64::from(b.1));
        let reflex = cross < 0.0;
        if 1.0 + dot < 1e-9 {
            out.push(b);
            continue;
        }
        let k = dist / (1.0 + dot);
        let miter = (bx + (n0.0 + n1.0) * k, by + (n0.1 + n1.1) * k);
        let ratio = (2.0 / (1.0 + dot)).sqrt();
        if reflex && ratio > 1.5 {
            out.push((ki_round(bx + n0.0 * dist), ki_round(by + n0.1 * dist)));
            out.push((ki_round(bx + n1.0 * dist), ki_round(by + n1.1 * dist)));
        } else {
            out.push((ki_round(miter.0), ki_round(miter.1)));
        }
    }
    out
}

fn courtyard_polygons(fp: &Footprint) -> Vec<Pt2> {
    let mut result: Vec<Vec<Vec<Pt>>> = Vec::new();
    for side in ["F.CrtYd", "B.CrtYd"] {
        let list: Vec<&Graphic> = fp.graphics.iter().filter(|g| g.layer == side).collect();
        if list.is_empty() {
            continue;
        }
        let Some(contours) = chain_contours(&list, ARC_HIGH_DEF, COURTYARD_EPSILON) else {
            continue;
        };
        for o in nest_contours(&contours) {
            let mut polys = vec![simplify_contour(
                &shrink_contour(&o.outer, ARC_HIGH_DEF),
                false,
            )];
            polys.extend(
                o.holes
                    .iter()
                    .map(|h| simplify_contour(&shrink_contour(h, ARC_HIGH_DEF), true)),
            );
            // The union of the front and back courtyards (`Simplify`) merges
            // identical polygons.
            if let Some(same) = result.iter_mut().find(|r| **r == polys) {
                // Seen on the exports: the union of identical front and back
                // courtyards starts at the topmost vertex when there is only
                // one (a circle), not after it.
                let ymin = same[0].iter().map(|p| p.1).min().unwrap_or(0);
                if same[0].iter().filter(|p| p.1 == ymin).count() == 1 {
                    same[0].rotate_right(1);
                }
            } else {
                result.push(polys);
            }
        }
    }
    // The union also absorbs a polygon that lies inside another one.
    let inside = |a: &[Pt], b: &[Pt]| a.iter().all(|p| b.contains(p) || point_inside(b, *p));
    let keep: Vec<bool> = (0..result.len())
        .map(|i| {
            !(0..result.len())
                .any(|j| j != i && result[j].len() == 1 && inside(&result[i][0], &result[j][0]))
        })
        .collect();
    result
        .into_iter()
        .zip(keep)
        .filter_map(|(r, k)| k.then_some(r))
        .flatten()
        .collect()
}

/// The image outlines of a footprint that has been flipped to the front.
pub fn image_outlines(fp: &Footprint) -> Vec<Shape> {
    let signal = LayerRef::Signal;
    let mut out = Vec::new();
    for poly in courtyard_polygons(fp) {
        out.push(Shape::Polygon {
            layer: signal.clone(),
            aperture_width: 0.0,
            points: poly.iter().map(|p| rel_pt(fp, *p)).collect(),
        });
    }
    for g in &fp.graphics {
        if g.layer == "F.CrtYd" || g.layer == "B.CrtYd" {
            continue;
        }
        let width = um(i64::from(g.width));
        match &g.kind {
            GKind::Segment { start, end } => out.push(Shape::Path {
                layer: signal.clone(),
                width,
                points: vec![rel_pt(fp, *start), rel_pt(fp, *end)],
            }),
            GKind::Circle { start, end } => {
                let r =
                    ki_round(f64::from(end.0 - start.0).hypot(f64::from(end.1 - start.1))).max(1);
                let mut poly = Vec::new();
                convert_arc_to_polyline_centre(
                    &mut poly,
                    *start,
                    r,
                    Angle(0.0),
                    Angle(360.0),
                    f64::from(ARC_HIGH_DEF),
                    ErrorLoc::Inside,
                );
                out.push(Shape::Path {
                    layer: signal.clone(),
                    width,
                    points: poly.iter().map(|p| rel_pt(fp, *p)).collect(),
                });
            }
            GKind::Rect { start, end } => {
                let c = [*start, (end.0, start.1), *end, (start.0, end.1), *start];
                out.push(Shape::Path {
                    layer: signal.clone(),
                    width,
                    points: c.iter().map(|p| rel_pt(fp, *p)).collect(),
                });
            }
            GKind::Arc { start, end, .. } => {
                let mid = g.arc_mid().unwrap_or(*start);
                let poly = transform_arc_to_polygon(
                    *start,
                    mid,
                    *end,
                    g.width,
                    ARC_HIGH_DEF,
                    ErrorLoc::Inside,
                );
                out.push(Shape::Polygon {
                    layer: signal.clone(),
                    aperture_width: 0.0,
                    points: poly.iter().map(|p| rel_pt(fp, *p)).collect(),
                });
            }
            GKind::Poly(_) | GKind::Bezier { .. } => {}
        }
    }
    out
}
