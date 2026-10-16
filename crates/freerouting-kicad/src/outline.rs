//! Board outline and closed-contour chaining (K05.4).
//!
//! `BOARD::GetBoardPolygonOutlines` (`board.cpp`) calls
//! `BuildBoardPolygonOutlines` and `doConvertOutlineToPolygon`
//! (`convert_shape_list_to_polygon.cpp`, tag `10.0.0`, GPL-2.0-or-later).
//! The reader repeats the chaining: shapes are taken in file order, each
//! chain is extended forward then backward through the nearest end point
//! within the chaining epsilon (0.01 mm), arcs and circles become polylines
//! with [`ShapeArc::to_polyline`] at the board's maximum error, and contours
//! nest by even/odd containment (outer contours and holes).
//!
//! Differences from KiCad: the start shape of a chain is the first unused
//! shape in file order (KiCad takes the first of a `std::set` of pointers),
//! footprints whose Edge.Cuts shapes enclose copper are tested by the
//! pad *position* instead of the pad polygon, and
//! `SHAPE_POLY_SET::Simplify` is replaced by [`simplify_contour`].

use crate::arc::{ARC_HIGH_DEF, ShapeArc, calc_arc_centre};
use crate::footprints::{Footprint, GKind, Graphic};
use crate::legacy::Pt;
use crate::units::ki_round;

/// `DEFAULT_CHAINING_EPSILON_MM`, 0.01 mm.
pub const CHAINING_EPSILON: i32 = 10_000;

/// An outer contour with its holes. Contours are implicitly closed (no
/// repeated first point).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct PolyOutline {
    pub outer: Vec<Pt>,
    pub holes: Vec<Vec<Pt>>,
}

fn sq(a: Pt, b: Pt) -> i64 {
    let (dx, dy) = (
        i64::from(a.0) - i64::from(b.0),
        i64::from(a.1) - i64::from(b.1),
    );
    dx * dx + dy * dy
}

fn close_enough(a: Pt, b: Pt, limit: i32) -> bool {
    sq(a, b) <= i64::from(limit) * i64::from(limit)
}

fn closer_to_first(r: Pt, a: Pt, b: Pt) -> bool {
    sq(r, a) < sq(r, b)
}

/// Twice the signed area (shoelace) of a closed contour.
pub fn area2(pts: &[Pt]) -> i128 {
    let n = pts.len();
    let mut a: i128 = 0;
    for i in 0..n {
        let (p, q) = (pts[i], pts[(i + 1) % n]);
        a += i128::from(p.0) * i128::from(q.1) - i128::from(q.0) * i128::from(p.1);
    }
    a
}

/// Even-odd point in polygon (crossing number).
pub fn point_inside(pts: &[Pt], p: Pt) -> bool {
    let n = pts.len();
    let mut inside = false;
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (a, b) = (pts[i], pts[j]);
        if (a.1 > p.1) != (b.1 > p.1) {
            let t = (i128::from(b.0) - i128::from(a.0)) * (i128::from(p.1) - i128::from(a.1));
            let u = i128::from(b.1) - i128::from(a.1);
            let x_cross_scaled = t; // compare p.x - a.x < t/u
            let lhs = (i128::from(p.0) - i128::from(a.0)) * u;
            let hit = if u > 0 {
                lhs < x_cross_scaled
            } else {
                lhs > x_cross_scaled
            };
            if hit {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// The polyline of one closed shape, or `None` for a shape that needs
/// chaining.
fn closed_shape_contour(g: &Graphic, max_error: i32) -> Option<Vec<Pt>> {
    match &g.kind {
        GKind::Poly(pts) => Some(pts.clone()),
        GKind::Circle { start, end } => {
            let r = ki_round(f64::from(end.0 - start.0).hypot(f64::from(end.1 - start.1))).max(1);
            let arc = ShapeArc::from_centre_start_angle(
                *start,
                (start.0.wrapping_add(r), start.1),
                crate::legacy::Angle(360.0),
            );
            Some(arc.to_polyline(max_error))
        }
        GKind::Rect { start, end } => Some(vec![*start, (end.0, start.1), *end, (start.0, end.1)]),
        _ => None,
    }
}

fn push_unique(out: &mut Vec<Pt>, p: Pt) {
    if out.last() != Some(&p) {
        out.push(p);
    }
}

/// `doConvertOutlineToPolygon`'s chaining part: closed contours, or `None`
/// when a chain stays open.
pub fn chain_contours(shapes: &[&Graphic], max_error: i32, eps: i32) -> Option<Vec<Vec<Pt>>> {
    let n = shapes.len();
    let mut used = vec![false; n];
    // end points in shape order: (point, shape index)
    let mut ends: Vec<(Pt, usize)> = Vec::new();
    for (i, g) in shapes.iter().enumerate() {
        if let Some((s, e)) = endpoints(g) {
            ends.push((s, i));
            ends.push((e, i));
        }
    }
    let find_next = |shape: usize, p: Pt| -> Option<usize> {
        // the two nearest end points, nearest first (stable on ties)
        let mut order: Vec<usize> = (0..ends.len()).collect();
        order.sort_by_key(|&k| sq(ends[k].0, p));
        let mut best: Option<usize> = None;
        let mut best_d = i64::from(eps) * i64::from(eps);
        for &k in order.iter().take(2) {
            let (pt, s) = ends[k];
            if s == shape {
                continue;
            }
            let d = sq(pt, p);
            if d < best_d {
                best_d = d;
                best = Some(s);
            }
        }
        best
    };
    let mut contours: Vec<Vec<Pt>> = Vec::new();
    for first in 0..n {
        if used[first] {
            continue;
        }
        used[first] = true;
        let g = shapes[first];
        if let Some(c) = closed_shape_contour(g, max_error) {
            contours.push(c);
            continue;
        }
        let Some((fs, fe)) = endpoints(g) else {
            continue;
        };
        let mut chain: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
        chain.push_back(first);
        let mut front_pt = fs;
        let mut back_pt = fe;
        let mut closed = false;
        for forward in [true, false] {
            if forward || !closed {
                let mut curr = if forward {
                    *chain.back()?
                } else {
                    *chain.front()?
                };
                let mut prev = if forward { back_pt } else { front_pt };
                loop {
                    let next = find_next(curr, prev);
                    if let Some(nx) = next
                        && !used[nx]
                    {
                        used[nx] = true;
                        if forward {
                            chain.push_back(nx);
                        } else {
                            chain.push_front(nx);
                        }
                        let (s, e) = endpoints(shapes[nx])?;
                        prev = if closer_to_first(prev, s, e) { e } else { s };
                        curr = nx;
                        continue;
                    }
                    if let Some(nx) = next {
                        let chain_end = if forward {
                            *chain.front()?
                        } else {
                            *chain.back()?
                        };
                        let chain_pt = if forward { front_pt } else { back_pt };
                        if nx == chain_end && close_enough(prev, chain_pt, eps) {
                            closed = true;
                        }
                    }
                    if forward {
                        back_pt = prev;
                    } else {
                        front_pt = prev;
                    }
                    break;
                }
            }
        }
        // build the contour
        let first_shape = shapes[*chain.front()?];
        let (f_s, f_e) = endpoints(first_shape)?;
        let start_pt = if chain.len() > 1 {
            let (s_s, s_e) = endpoints(shapes[chain[1]])?;
            if close_enough(f_s, s_s, eps) || close_enough(f_s, s_e, eps) {
                f_e
            } else {
                f_s
            }
        } else {
            f_s
        };
        let mut contour = vec![start_pt];
        let mut prev_pt = start_pt;
        let mut last_arc: Option<ShapeArc> = None;
        for &si in &chain {
            let g = shapes[si];
            match &g.kind {
                GKind::Segment { start, end } => {
                    let next = if closer_to_first(prev_pt, *start, *end) {
                        *end
                    } else {
                        *start
                    };
                    push_unique(&mut contour, next);
                    prev_pt = next;
                    last_arc = None;
                }
                GKind::Arc { start, end, .. } => {
                    let mid = g.arc_mid().unwrap_or(*start);
                    let (mut ps, mut pe) = (*start, *end);
                    if !close_enough(prev_pt, ps, eps) {
                        if !close_enough(prev_pt, *end, eps) {
                            continue;
                        }
                        std::mem::swap(&mut ps, &mut pe);
                    }
                    ps = prev_pt;
                    let sarc = ShapeArc::from_three_points(ps, mid, pe);
                    for p in sarc.to_polyline(max_error) {
                        push_unique(&mut contour, p);
                    }
                    last_arc = Some(sarc);
                    prev_pt = pe;
                }
                GKind::Bezier { start, c1, c2, end } => {
                    let (next, reverse) = if closer_to_first(prev_pt, *start, *end) {
                        (*end, false)
                    } else {
                        (*start, true)
                    };
                    let mut pts = crate::pads::bezier_points(*start, *c1, *c2, *end, max_error);
                    if reverse {
                        pts.reverse();
                    }
                    for p in pts {
                        if p == prev_pt {
                            continue;
                        }
                        push_unique(&mut contour, p);
                        prev_pt = p;
                    }
                    prev_pt = next;
                    last_arc = None;
                }
                _ => {}
            }
        }
        let c0 = contour[0];
        let cl = *contour.last()?;
        if close_enough(c0, cl, eps) {
            if c0 != cl && contour.len() > 2 {
                if let Some(arc) = last_arc {
                    // re-run the last arc so that it ends exactly at the start
                    let sarc = ShapeArc::from_three_points(arc.start, arc.mid, c0);
                    let k = contour
                        .iter()
                        .rposition(|p| *p == arc.start)
                        .unwrap_or(contour.len() - 1);
                    contour.truncate(k);
                    for p in sarc.to_polyline(max_error) {
                        push_unique(&mut contour, p);
                    }
                } else {
                    *contour.last_mut()? = c0;
                }
            }
            contours.push(contour);
        } else {
            return None;
        }
    }
    Some(contours)
}

fn endpoints(g: &Graphic) -> Option<(Pt, Pt)> {
    g.ends()
}

/// Removes repeated points and orients the contour (`Simplify` keeps
/// collinear vertices, as the exports show): positive area for an outer contour, negative for
/// a hole (Clipper's convention).
pub fn simplify_contour(pts: &[Pt], hole: bool) -> Vec<Pt> {
    let mut v: Vec<Pt> = Vec::with_capacity(pts.len());
    for &p in pts {
        if v.last() != Some(&p) {
            v.push(p);
        }
    }
    while v.len() > 1 && v.first() == v.last() {
        v.pop();
    }
    let a = area2(&v);
    if (a > 0) == hole && a != 0 {
        v.reverse();
    }
    if !hole {
        rotate_to_clipper_start(&mut v);
    }
    v
}

/// Where Clipper starts an outer contour. Observed on KiCad 10.0.7 exports
/// (rectangles, octagons, circles), not derived from Clipper's sweep: the
/// contour has positive area and starts at the vertex after the topmost one
/// (smallest Y); with a flat top, after the right end of the top edge. A
/// rectangle then starts at its bottom right corner.
pub fn rotate_to_clipper_start(v: &mut [Pt]) {
    let n = v.len();
    if n < 3 || area2(v) <= 0 {
        return;
    }
    let ymin = v.iter().map(|p| p.1).min().unwrap_or(0);
    let tops: Vec<usize> = (0..n).filter(|&i| v[i].1 == ymin).collect();
    // with a flat top, the right end of the top edge
    let top = *tops.iter().max_by_key(|&&i| v[i].0).unwrap_or(&0);
    let start = (top + 1) % n;
    v.rotate_left(start);
}

/// Even/odd nesting of closed contours into outlines and holes
/// (`buildContourHierarchy`, `addOutlinesToPolygon`, `addHolesToPolygon`).
pub fn nest_contours(contours: &[Vec<Pt>]) -> Vec<PolyOutline> {
    let parents: Vec<Vec<usize>> = (0..contours.len())
        .map(|i| {
            if contours[i].is_empty() {
                return Vec::new();
            }
            let first = contours[i][0];
            (0..contours.len())
                .filter(|&j| j != i && point_inside(&contours[j], first))
                .collect()
        })
        .collect();
    let mut outlines: Vec<PolyOutline> = Vec::new();
    let mut index_of: std::collections::BTreeMap<usize, usize> = std::collections::BTreeMap::new();
    for i in 0..contours.len() {
        if parents[i].len() % 2 == 0 {
            index_of.insert(i, outlines.len());
            outlines.push(PolyOutline {
                outer: simplify_contour(&contours[i], false),
                holes: Vec::new(),
            });
        }
    }
    for i in 0..contours.len() {
        if parents[i].len() % 2 == 1 {
            for &p in &parents[i] {
                if parents[p].len() == parents[i].len() - 1 {
                    if let Some(&o) = index_of.get(&p) {
                        outlines[o].holes.push(simplify_contour(&contours[i], true));
                    }
                    break;
                }
            }
        }
    }
    outlines
}

/// The result of [`board_outline`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoardOutline {
    pub outlines: Vec<PolyOutline>,
    /// KiCad would warn "Board outline is malformed".
    pub malformed: bool,
}

/// `BuildBoardPolygonOutlines( board, outlines, maxError, epsilon, infer )`.
/// `board_shapes` are the board-level graphics, `footprints` the footprints
/// as the file has them (not flipped).
pub fn board_outline(
    board_shapes: &[Graphic],
    footprints: &[Footprint],
    max_error: i32,
    infer: bool,
) -> BoardOutline {
    let mut pool: Vec<&Graphic> = board_shapes
        .iter()
        .filter(|g| g.layer == "Edge.Cuts")
        .collect();
    let mut fp_holes: Vec<Vec<Pt>> = Vec::new();
    for fp in footprints {
        let segs: Vec<&Graphic> = fp
            .graphics
            .iter()
            .filter(|g| g.layer == "Edge.Cuts")
            .collect();
        if segs.is_empty() {
            continue;
        }
        let mut is_hole = false;
        let mut outlines = Vec::new();
        if let Some(contours) = chain_contours(&segs, max_error, CHAINING_EPSILON) {
            outlines = nest_contours(&contours);
            let copper_outside = fp
                .pads
                .iter()
                .any(|p| !outlines.iter().any(|o| point_inside(&o.outer, p.pos)));
            is_hole = copper_outside || outlines.len() > 1;
        }
        if is_hole {
            fp_holes.extend(outlines.into_iter().map(|o| o.outer));
        } else {
            pool.extend(segs);
        }
    }
    let mut result: Vec<PolyOutline> = Vec::new();
    let mut success = true;
    if !pool.is_empty() {
        match chain_contours(&pool, max_error, CHAINING_EPSILON) {
            Some(c) => result = nest_contours(&c),
            None => success = false,
        }
    }
    if (!success || result.is_empty()) && infer {
        let mut pts: Vec<Pt> = Vec::new();
        for g in board_shapes.iter().filter(|g| g.layer == "Edge.Cuts") {
            graphic_extent(g, &mut pts);
        }
        let mut bbox = bbox_of(&pts);
        if bbox.is_none_or(|b| b.2 - b.0 == 0 || b.3 - b.1 == 0) {
            let mut all = Vec::new();
            for g in board_shapes {
                graphic_extent(g, &mut all);
            }
            for fp in footprints {
                all.push(fp.pos);
                for g in &fp.graphics {
                    graphic_extent(g, &mut all);
                }
                for p in &fp.pads {
                    all.push(p.pos);
                }
            }
            bbox = bbox_of(&all);
        }
        let (mut x0, mut y0, mut x1, mut y1) = bbox.unwrap_or((0, 0, 0, 0));
        if x1 - x0 == 0 || y1 - y0 == 0 {
            let d = 1_000_000;
            x0 -= d;
            y0 -= d;
            x1 += d;
            y1 += d;
        }
        result = vec![PolyOutline {
            outer: simplify_contour(&[(x0, y0), (x0, y1), (x1, y1), (x1, y0)], false),
            holes: Vec::new(),
        }];
    }
    for h in fp_holes {
        if let Some(h0) = h.first().copied() {
            for o in &mut result {
                if point_inside(&o.outer, h0) {
                    o.holes.push(simplify_contour(&h, true));
                    break;
                }
            }
        }
    }
    BoardOutline {
        outlines: result,
        malformed: !success || pool.is_empty(),
    }
}

fn graphic_extent(g: &Graphic, out: &mut Vec<Pt>) {
    match &g.kind {
        GKind::Segment { start, end } | GKind::Rect { start, end } => {
            out.extend([*start, *end]);
        }
        GKind::Circle { start, end } => {
            let r = ki_round(f64::from(end.0 - start.0).hypot(f64::from(end.1 - start.1)));
            out.extend([(start.0 - r, start.1 - r), (start.0 + r, start.1 + r)]);
        }
        GKind::Arc { start, end, .. } => {
            let mid = g.arc_mid().unwrap_or(*start);
            let c = calc_arc_centre(*start, mid, *end);
            let r = ki_round(f64::from(start.0 - c.0).hypot(f64::from(start.1 - c.1)));
            out.extend([*start, mid, *end, (c.0 - r, c.1 - r), (c.0 + r, c.1 + r)]);
        }
        GKind::Poly(pts) => out.extend(pts.iter().copied()),
        GKind::Bezier { start, c1, c2, end } => out.extend([*start, *c1, *c2, *end]),
    }
}

fn bbox_of(pts: &[Pt]) -> Option<(i32, i32, i32, i32)> {
    let x0 = pts.iter().map(|p| p.0).min()?;
    let x1 = pts.iter().map(|p| p.0).max()?;
    let y0 = pts.iter().map(|p| p.1).min()?;
    let y1 = pts.iter().map(|p| p.1).max()?;
    Some((x0, y0, x1, y1))
}

/// The board's `m_MaxError` default.
pub const DEFAULT_MAX_ERROR: i32 = ARC_HIGH_DEF;
