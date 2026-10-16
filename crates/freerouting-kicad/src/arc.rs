//! Arcs, circles and rounded shapes as polylines, the way KiCad makes them.
//!
//! The DSN exporter and the board-outline builder turn arcs into polylines
//! with KiCad's own segment counts, so the reader repeats the arithmetic
//! (`libs/kimath/src/geometry/shape_arc.cpp`, `geometry_utils.cpp`,
//! `arc_chord_params.cpp`, `libs/kimath/src/trigo.cpp` and
//! `libs/kimath/src/convert_basic_shapes_to_polygon.cpp` at tag `10.0.0`;
//! GPL-2.0-or-later). Points are integer nanometres; everything else is `f64`
//! in the same operation order as the C++.
//!
//! Known simplification: `TransformOvalToPolygon` (used by KiCad only when an
//! arc is almost straight) is not clipped against its bounding box.

use crate::legacy::{Angle, Pt, rotate_origin};
use crate::units::ki_round;

/// `ARC_HIGH_DEF`: 0.005 mm, the board's default maximum error.
pub const ARC_HIGH_DEF: i32 = 5_000;
/// `ARC_LOW_DEF`: 0.02 mm.
pub const ARC_LOW_DEF: i32 = 20_000;

const MIN_SEGCOUNT_FOR_CIRCLE: f64 = 8.0;

/// `GetArcToSegmentCount`.
pub fn arc_to_segment_count(radius: i32, error_max: i32, arc_degrees: f64) -> i32 {
    let radius = radius.max(1);
    let error_max = error_max.max(1);
    let rel_error = f64::from(error_max) / f64::from(radius);
    let mut arc_increment = 180.0 / std::f64::consts::PI * (1.0 - rel_error).acos() * 2.0;
    arc_increment = arc_increment.min(360.0 / MIN_SEGCOUNT_FOR_CIRCLE);
    let seg_count = ki_round(arc_degrees.abs() / arc_increment);
    seg_count.max(2)
}

/// `CircleToEndSegmentDeltaRadius`.
pub fn circle_to_end_segment_delta_radius(radius: i32, seg_count: i32) -> i32 {
    let seg_count = if seg_count <= 2 { 3 } else { seg_count };
    let alpha = std::f64::consts::PI / f64::from(seg_count);
    ki_round((f64::from(radius) * (1.0 - 1.0 / alpha.cos())).abs())
}

/// Which side of the true arc the polyline error lies on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrorLoc {
    Inside,
    Outside,
}

fn pt_f(x: f64, y: f64) -> Pt {
    (ki_round(x), ki_round(y))
}

/// `ConvertArcToPolyline( chain, centre, radius, start angle, arc angle,
/// accuracy, loc )`. Appends to `out`; returns the segment count.
pub fn convert_arc_to_polyline_centre(
    out: &mut Vec<Pt>,
    centre: Pt,
    radius: i32,
    start: Angle,
    arc: Angle,
    accuracy: f64,
    loc: ErrorLoc,
) -> i32 {
    let mut n = 2;
    if f64::from(radius) >= accuracy {
        n = arc_to_segment_count(radius, accuracy as i32, arc.0) + 1;
    }
    let delta = Angle(arc.0 / f64::from(n));
    let (cx, cy) = (f64::from(centre.0), f64::from(centre.1));
    let r = f64::from(radius);
    if loc == ErrorLoc::Inside {
        let mut rot = start;
        for _ in 0..=n {
            out.push(pt_f(cx + r * rot.cos(), cy + r * rot.sin()));
            rot = Angle(rot.0 + delta.0);
        }
    } else {
        let seg360 = ki_round(f64::from(n) * 360.0 / arc.0).abs();
        let actual_delta_radius = circle_to_end_segment_delta_radius(radius, seg360);
        let error_radius = f64::from(radius + actual_delta_radius);
        out.push(pt_f(cx + r * start.cos(), cy + r * start.sin()));
        let mut rot = Angle(start.0 + delta.0 / 2.0);
        for _ in 0..n {
            out.push(pt_f(
                cx + error_radius * rot.cos(),
                cy + error_radius * rot.sin(),
            ));
            rot = Angle(rot.0 + delta.0);
        }
        let end = Angle(start.0 + arc.0);
        out.push(pt_f(cx + r * end.cos(), cy + r * end.sin()));
    }
    n
}

/// `ARC_CHORD_PARAMS`.
#[derive(Clone, Copy, Debug)]
pub struct ArcChord {
    pub radius: f64,
    pub half_chord: f64,
    pub sagitta: f64,
    pub centre_offset: f64,
    pub ux: f64,
    pub uy: f64,
    pub nx: f64,
    pub ny: f64,
    pub mid_x: f64,
    pub mid_y: f64,
}

impl ArcChord {
    /// `Compute`; `None` for a degenerate arc.
    pub fn compute(start: Pt, mid: Pt, end: Pt) -> Option<ArcChord> {
        let dx = f64::from(end.0) - f64::from(start.0);
        let dy = f64::from(end.1) - f64::from(start.1);
        let chord_len = (dx * dx + dy * dy).sqrt();
        if chord_len <= 0.0 {
            return None;
        }
        let mx = f64::from(mid.0) - f64::from(start.0);
        let my = f64::from(mid.1) - f64::from(start.1);
        let cross = mx * dy - my * dx;
        if cross == 0.0 {
            return None;
        }
        let sagitta = cross.abs() / chord_len;
        if sagitta <= 0.0 {
            return None;
        }
        let half_chord = chord_len / 2.0;
        let radius = (half_chord * half_chord + sagitta * sagitta) / (2.0 * sagitta);
        if radius <= 0.0 {
            return None;
        }
        let ux = dx / chord_len;
        let uy = dy / chord_len;
        let (mut nx, mut ny) = (-uy, ux);
        if cross < 0.0 {
            nx = -nx;
            ny = -ny;
        }
        Some(ArcChord {
            radius,
            half_chord,
            sagitta,
            centre_offset: radius - sagitta,
            ux,
            uy,
            nx,
            ny,
            mid_x: (f64::from(start.0) + f64::from(end.0)) * 0.5,
            mid_y: (f64::from(start.1) + f64::from(end.1)) * 0.5,
        })
    }

    pub fn start_angle(&self) -> Angle {
        let sin_half = self.half_chord / self.radius;
        let cos_half = self.centre_offset / self.radius;
        Angle::from_vector(
            -sin_half * self.ux - cos_half * self.nx,
            -sin_half * self.uy - cos_half * self.ny,
        )
    }

    pub fn end_angle(&self) -> Angle {
        let sin_half = self.half_chord / self.radius;
        let cos_half = self.centre_offset / self.radius;
        Angle::from_vector(
            sin_half * self.ux - cos_half * self.nx,
            sin_half * self.uy - cos_half * self.ny,
        )
    }

    /// `GetArcAngle` in radians.
    pub fn arc_angle(&self) -> f64 {
        let ratio = (self.half_chord / self.radius).clamp(0.0, 1.0);
        let base = 2.0 * ratio.asin();
        if self.sagitta > self.radius {
            2.0 * std::f64::consts::PI - base
        } else {
            base
        }
    }
}

/// `ConvertArcToPolyline( chain, start, mid, end, accuracy, loc, radial offset )`.
pub fn convert_arc_to_polyline_3pt(
    out: &mut Vec<Pt>,
    start: Pt,
    mid: Pt,
    end: Pt,
    accuracy: f64,
    loc: ErrorLoc,
    radial_offset: f64,
) -> i32 {
    let Some(params) = ArcChord::compute(start, mid, end) else {
        out.push(start);
        if end != start {
            out.push(end);
        }
        return 0;
    };
    let arc_angle = params.arc_angle();
    if arc_angle <= 0.0 {
        out.push(start);
        out.push(end);
        return 0;
    }
    let max_coord = f64::from(i32::MAX);
    let effective_radius = params.radius + radial_offset;
    let append = |out: &mut Vec<Pt>, alpha: f64, r: f64| -> bool {
        let u_offset = r * alpha.sin();
        let n_offset = params.centre_offset - r * alpha.cos();
        let x = params.mid_x + params.ux * u_offset + params.nx * n_offset;
        let y = params.mid_y + params.uy * u_offset + params.ny * n_offset;
        if !x.is_finite() || !y.is_finite() || x.abs() > max_coord || y.abs() > max_coord {
            out.clear();
            out.push(start);
            out.push(end);
            return false;
        }
        out.push(pt_f(x, y));
        true
    };
    let arc_deg = Angle(arc_angle / (std::f64::consts::PI / 180.0));
    let mut n = 2;
    let radius_for_seg = ki_round(effective_radius.abs().min(max_coord));
    if f64::from(radius_for_seg) >= accuracy {
        n = arc_to_segment_count(radius_for_seg, accuracy as i32, arc_deg.0) + 1;
    }
    let half_angle = arc_angle / 2.0;
    let delta = arc_angle / f64::from(n);
    if loc == ErrorLoc::Inside {
        for i in 0..=n {
            if !append(out, -half_angle + delta * f64::from(i), effective_radius) {
                return 0;
            }
        }
    } else {
        let seg360 = ki_round(f64::from(n) * 360.0 / arc_deg.0).abs();
        if seg360 <= 0 {
            for i in 0..=n {
                if !append(out, -half_angle + delta * f64::from(i), effective_radius) {
                    return 0;
                }
            }
            return n;
        }
        let delta_radius = circle_to_end_segment_delta_radius(radius_for_seg, seg360);
        let error_radius = effective_radius + f64::from(delta_radius);
        if !append(out, -half_angle, effective_radius) {
            return 0;
        }
        for i in 0..n {
            if !append(
                out,
                -half_angle + delta * (f64::from(i) + 0.5),
                error_radius,
            ) {
                return 0;
            }
        }
        if !append(out, half_angle, effective_radius) {
            return 0;
        }
    }
    n
}

/// `TransformArcToPolygon`: the outline of a stroked arc, as KiCad's
/// footprint-graphic export calls it (`width` the stroke width).
pub fn transform_arc_to_polygon(
    start: Pt,
    mid: Pt,
    end: Pt,
    width: i32,
    error: i32,
    loc: ErrorLoc,
) -> Vec<Pt> {
    let dist_to_mid = dist_point_to_segment(mid, start, end);
    if dist_to_mid <= 1 {
        return transform_oval_to_polygon(start, end, width + dist_to_mid, error, loc);
    }
    let cross = i64::from(mid.0 - start.0) * i64::from(end.1 - start.1)
        - i64::from(mid.1 - start.1) * i64::from(end.0 - start.0);
    let (mut p0, mut p1) = (start, end);
    if cross < 0 {
        std::mem::swap(&mut p0, &mut p1);
    }
    let Some(params) = ArcChord::compute(p0, mid, p1) else {
        return transform_oval_to_polygon(start, end, width, error, loc);
    };
    let start_angle = params.start_angle();
    let end_angle = params.end_angle();
    let radial_offset = width / 2;
    let inner_radius = params.radius - f64::from(radial_offset);
    let (loc_inner, loc_outer) = match loc {
        ErrorLoc::Inside => (ErrorLoc::Outside, ErrorLoc::Inside),
        ErrorLoc::Outside => (ErrorLoc::Inside, ErrorLoc::Outside),
    };
    let mut outline = Vec::new();
    let acc = f64::from(error);
    convert_arc_to_polyline_centre(
        &mut outline,
        p0,
        radial_offset,
        Angle(start_angle.0 - 180.0),
        Angle(180.0),
        acc,
        loc,
    );
    convert_arc_to_polyline_3pt(
        &mut outline,
        p0,
        mid,
        p1,
        acc,
        loc_outer,
        f64::from(radial_offset),
    );
    convert_arc_to_polyline_centre(
        &mut outline,
        p1,
        radial_offset,
        end_angle,
        Angle(180.0),
        acc,
        loc,
    );
    if inner_radius > 0.0 {
        convert_arc_to_polyline_3pt(
            &mut outline,
            p1,
            mid,
            p0,
            acc,
            loc_inner,
            -f64::from(radial_offset),
        );
    }
    dedup_consecutive(outline)
}

/// `SHAPE_LINE_CHAIN::Append` ignores a point equal to the last one.
pub fn dedup_consecutive(mut pts: Vec<Pt>) -> Vec<Pt> {
    pts.dedup();
    pts
}

/// `SEG::Distance( point )` truncated to `int` like `int distanceToMid`.
fn dist_point_to_segment(p: Pt, a: Pt, b: Pt) -> i32 {
    let (px, py) = (f64::from(p.0), f64::from(p.1));
    let (ax, ay) = (f64::from(a.0), f64::from(a.1));
    let (bx, by) = (f64::from(b.0), f64::from(b.1));
    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 {
        0.0
    } else {
        (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0)
    };
    let (qx, qy) = (ax + t * dx, ay + t * dy);
    ((px - qx).hypot(py - qy)) as i32
}

/// `TransformOvalToPolygon` without the bounding-box clamp.
pub fn transform_oval_to_polygon(a: Pt, b: Pt, width: i32, error: i32, loc: ErrorLoc) -> Vec<Pt> {
    let mut radius = width / 2;
    let mut num_segs = arc_to_segment_count(radius, error, 360.0).max(0);
    num_segs = (num_segs + 7) / 8 * 8;
    let delta = Angle(360.0 / f64::from(num_segs));
    if loc == ErrorLoc::Outside {
        radius += circle_to_end_segment_delta_radius(radius, num_segs);
    }
    let (mut startp, mut endp) = (a, (b.0 - a.0, b.1 - a.1));
    if endp.0 < 0 {
        endp = (a.0 - b.0, a.1 - b.1);
        startp = b;
    }
    let delta_angle = Angle::from_vector(f64::from(endp.0), f64::from(endp.1));
    let seg_len = ki_round(f64::from(endp.0).hypot(f64::from(endp.1)));
    let mut poly: Vec<Pt> = vec![(seg_len, radius)];
    let mut angle = Angle(delta.0 / 2.0);
    while angle.0 < 180.0 {
        let c = rotate_origin(0, radius, angle);
        poly.push((c.0 + seg_len, c.1));
        angle = Angle(angle.0 + delta.0);
    }
    poly.push((seg_len, -radius));
    poly.push((0, -radius));
    let mut angle = Angle(delta.0 / 2.0);
    while angle.0 < 180.0 {
        poly.push(rotate_origin(0, -radius, angle));
        angle = Angle(angle.0 + delta.0);
    }
    poly.push((0, radius));
    poly.into_iter()
        .map(|p| {
            let r = rotate_origin(p.0, p.1, Angle(-delta_angle.0));
            (r.0 + startp.0, r.1 + startp.1)
        })
        .collect()
}

/// `TransformCircleToPolygon( SHAPE_POLY_SET&, ... )` for `ERROR_INSIDE`:
/// the points of the ring (no closing repeat).
pub fn circle_polygon(centre: Pt, radius: i32, error: i32, min_segs: i32) -> Vec<Pt> {
    let mut num_segs = arc_to_segment_count(radius, error, 360.0);
    num_segs = num_segs.max(min_segs);
    num_segs = (num_segs + 7) / 8 * 8;
    let delta = Angle(360.0 / f64::from(num_segs));
    let mut out = Vec::new();
    let mut angle = Angle(delta.0 / 2.0);
    while angle.0 < 360.0 {
        let c = rotate_origin(radius, 0, angle);
        out.push((c.0 + centre.0, c.1 + centre.1));
        angle = Angle(angle.0 + delta.0);
    }
    out
}

// ---- SHAPE_ARC ---------------------------------------------------------------

/// `CalcArcCenter( start, mid, end )` for doubles (`trigo.cpp`), including the
/// error-propagation rounding to "nice" numbers.
pub fn calc_arc_centre_f(s: (f64, f64), m: (f64, f64), e: (f64, f64)) -> (f64, f64) {
    let y21 = m.1 - s.1;
    let mut x21 = m.0 - s.0;
    let y32 = e.1 - m.1;
    let mut x32 = e.0 - m.0;
    if (x21 == 0.0 && y32 == 0.0) || (y21 == 0.0 && x32 == 0.0) {
        return ((s.0 + e.0) / 2.0, (s.1 + e.1) / 2.0);
    }
    if x21 == 0.0 {
        x21 = f64::EPSILON;
    }
    if x32 == 0.0 {
        x32 = -f64::EPSILON;
    }
    let mut a_slope = y21 / x21;
    let mut b_slope = y32 / x32;
    let da_slope = a_slope * (0.5 / y21).hypot(0.5 / x21);
    let db_slope = b_slope * (0.5 / y32).hypot(0.5 / x32);
    if a_slope == b_slope {
        if s == e {
            return ((s.0 + m.0) / 2.0, (s.1 + m.1) / 2.0);
        }
        a_slope += f64::EPSILON;
        b_slope -= f64::EPSILON;
    }
    if a_slope == 0.0 {
        a_slope = 1e-10;
    }
    if b_slope == 0.0 {
        b_slope = 1e-10;
    }
    let q = std::f64::consts::FRAC_1_SQRT_2;
    let ab_slope_start_end_y = a_slope * b_slope * (s.1 - e.1);
    let dab = ab_slope_start_end_y
        * ((da_slope / a_slope * da_slope / a_slope)
            + (db_slope / b_slope * db_slope / b_slope)
            + (q / (s.1 - e.1) * q / (s.1 - e.1)))
            .sqrt();
    let b_slope_start_mid_x = b_slope * (s.0 + m.0);
    let dbs = b_slope_start_mid_x
        * ((db_slope / b_slope * db_slope / b_slope) + (q / (s.0 + m.0) * q / (s.0 + m.0))).sqrt();
    let a_slope_mid_end_x = a_slope * (m.0 + e.0);
    let das = a_slope_mid_end_x
        * ((da_slope / a_slope * da_slope / a_slope) + (q / (m.0 + e.0) * q / (m.0 + e.0))).sqrt();
    let twice_ba = 2.0 * (b_slope - a_slope);
    let dtwice_ba = 2.0 * (db_slope * db_slope + da_slope * da_slope).sqrt();
    let centre_num_x = ab_slope_start_end_y + b_slope_start_mid_x - a_slope_mid_end_x;
    let dcentre_num_x = (dab * dab + dbs * dbs + das * das).sqrt();
    let centre_x = (ab_slope_start_end_y + b_slope_start_mid_x - a_slope_mid_end_x) / twice_ba;
    let dcentre_x = centre_x
        * ((dcentre_num_x / centre_num_x * dcentre_num_x / centre_num_x)
            + (dtwice_ba / twice_ba * dtwice_ba / twice_ba))
            .sqrt();
    let centre_num_y = (s.0 + m.0) / 2.0 - centre_x;
    let dcentre_num_y = (1.0 / 8.0 + dcentre_x * dcentre_x).sqrt();
    let centre_first = centre_num_y / a_slope;
    let dcentre_first_y = centre_first
        * ((dcentre_num_y / centre_num_y * dcentre_num_y / centre_num_y)
            + (da_slope / a_slope * da_slope / a_slope))
            .sqrt();
    let centre_y = centre_first + (s.1 + m.1) / 2.0;
    let dcentre_y = (dcentre_first_y * dcentre_first_y + 1.0 / 8.0).sqrt();
    let r100x = ((centre_x + 50.0) / 100.0).floor() * 100.0;
    let r100y = ((centre_y + 50.0) / 100.0).floor() * 100.0;
    let r10x = ((centre_x + 5.0) / 10.0).floor() * 10.0;
    let r10y = ((centre_y + 5.0) / 10.0).floor() * 10.0;
    if (r100x - centre_x).abs() < dcentre_x && (r100y - centre_y).abs() < dcentre_y {
        (r100x, r100y)
    } else if (r10x - centre_x).abs() < dcentre_x && (r10y - centre_y).abs() < dcentre_y {
        (r10x, r10y)
    } else {
        (centre_x, centre_y)
    }
}

/// `CalcArcCenter( VECTOR2I, VECTOR2I, VECTOR2I )`: rounded and clamped.
pub fn calc_arc_centre(s: Pt, m: Pt, e: Pt) -> Pt {
    let f = |p: Pt| (f64::from(p.0), f64::from(p.1));
    let (cx, cy) = calc_arc_centre_f(f(s), f(m), f(e));
    let lo = f64::from(i32::MIN + 100);
    let hi = f64::from(i32::MAX - 100);
    (ki_round(cx.clamp(lo, hi)), ki_round(cy.clamp(lo, hi)))
}

/// `SHAPE_ARC`: three points on the arc and the values derived from them.
#[derive(Clone, Copy, Debug)]
pub struct ShapeArc {
    pub start: Pt,
    pub mid: Pt,
    pub end: Pt,
    pub centre: Pt,
    pub radius: f64,
}

impl ShapeArc {
    /// `SHAPE_ARC( start, mid, end, width )`.
    pub fn from_three_points(start: Pt, mid: Pt, end: Pt) -> ShapeArc {
        let centre = calc_arc_centre(start, mid, end);
        let radius = (f64::from(start.0) - f64::from(centre.0))
            .hypot(f64::from(start.1) - f64::from(centre.1));
        ShapeArc {
            start,
            mid,
            end,
            centre,
            radius,
        }
    }

    /// `SHAPE_ARC( centre, start, angle, width )` (a full circle for 360).
    pub fn from_centre_start_angle(centre: Pt, start: Pt, angle: Angle) -> ShapeArc {
        let rot = |a: Angle| -> Pt {
            let a = Angle(-a.0).normalize();
            let (dx, dy) = (
                f64::from(start.0) - f64::from(centre.0),
                f64::from(start.1) - f64::from(centre.1),
            );
            let (s, c) = (a.sin(), a.cos());
            pt_f(
                f64::from(centre.0) + dx * c + dy * s,
                f64::from(centre.1) + dy * c - dx * s,
            )
        };
        let mid = rot(Angle(angle.0 / 2.0));
        let end = rot(angle);
        ShapeArc::from_three_points(start, mid, end)
    }

    fn is_ccw(&self) -> bool {
        let v1 = (
            i64::from(self.end.0) - i64::from(self.mid.0),
            i64::from(self.end.1) - i64::from(self.mid.1),
        );
        let v2 = (
            i64::from(self.start.0) - i64::from(self.mid.0),
            i64::from(self.start.1) - i64::from(self.mid.1),
        );
        v1.0 * v2.1 - v1.1 * v2.0 > 0
    }

    pub fn start_angle(&self) -> Angle {
        Angle::from_vector(
            f64::from(self.start.0 - self.centre.0),
            f64::from(self.start.1 - self.centre.1),
        )
        .normalize()
    }

    /// `GetCentralAngle`, degrees.
    pub fn central_angle(&self) -> Angle {
        if self.start == self.end {
            return Angle(360.0);
        }
        let ea = Angle::from_vector(
            f64::from(self.end.0 - self.centre.0),
            f64::from(self.end.1 - self.centre.1),
        );
        let sa = Angle::from_vector(
            f64::from(self.start.0 - self.centre.0),
            f64::from(self.start.1 - self.centre.1),
        );
        let mut angle = ea.0 - sa.0;
        if self.is_ccw() {
            if angle < 0.0 {
                angle += 360.0;
            }
        } else if angle > 0.0 {
            angle -= 360.0;
        }
        Angle(angle)
    }

    /// `ConvertToPolyline( max_error )`.
    pub fn to_polyline(&self, max_error: i32) -> Vec<Pt> {
        let r = self.radius;
        let sa = self.start_angle();
        let ca = self.central_angle();
        let half_max = (f64::from(max_error) / 2.0).max(1.0);
        let external_radius = r;
        let (n, effective_error);
        if external_radius < half_max
            || f64::from(dist_point_to_segment(self.mid, self.start, self.end)) < half_max
        {
            n = 0;
            effective_error = external_radius;
        } else {
            n = arc_to_segment_count(external_radius as i32, max_error, ca.0);
            let seg360 = (f64::from(n) * 360.0 / ca.0.abs()) as i32;
            effective_error = f64::from(circle_to_end_segment_delta_radius(
                external_radius as i32,
                seg360,
            ));
        }
        let r = r + effective_error / 2.0;
        let n2 = n * 2;
        let mut out = vec![self.start];
        let mut i = 1;
        while i < n2 {
            let a = Angle(sa.0 + (ca.0 * f64::from(i)) / f64::from(n2));
            out.push(pt_f(
                f64::from(self.centre.0) + r * a.cos(),
                f64::from(self.centre.1) + r * a.sin(),
            ));
            i += 2;
        }
        out.push(self.end);
        out
    }
}

// ---- rounded rectangles ---------------------------------------------------------

pub const CHAMFER_TOP_LEFT: i32 = 1;
pub const CHAMFER_TOP_RIGHT: i32 = 2;
pub const CHAMFER_BOTTOM_LEFT: i32 = 4;
pub const CHAMFER_BOTTOM_RIGHT: i32 = 8;

#[derive(Clone, Copy)]
struct RoundedCorner {
    pos: Pt,
    radius: i32,
}

/// `TransformRoundChamferedRectToPolygon` with zero inflate and
/// `ERROR_INSIDE` (what the DSN export uses), centred on the origin and not
/// rotated. `size` is the full width and height.
pub fn round_chamfered_rect(
    size: (i32, i32),
    corner_radius: i32,
    chamfer_ratio: f64,
    chamfer_corners: i32,
    error: i32,
) -> Vec<Pt> {
    let half = (size.0 / 2, size.1 / 2);
    let cham_cnt = (chamfer_corners & 0xff).count_ones();
    let mut corners = vec![
        RoundedCorner {
            pos: (-half.0, -half.1),
            radius: corner_radius,
        },
        RoundedCorner {
            pos: (half.0, -half.1),
            radius: corner_radius,
        },
        RoundedCorner {
            pos: (half.0, half.1),
            radius: corner_radius,
        },
        RoundedCorner {
            pos: (-half.0, half.1),
            radius: corner_radius,
        },
    ];
    if chamfer_corners != 0 {
        let shorter = size.0.min(size.1);
        let chamfer = ki_round(chamfer_ratio * f64::from(shorter)).max(0);
        let cham_id = [
            CHAMFER_TOP_LEFT,
            CHAMFER_TOP_RIGHT,
            CHAMFER_BOTTOM_RIGHT,
            CHAMFER_BOTTOM_LEFT,
        ];
        let sign: [i32; 8] = [0, 1, -1, 0, 0, -1, 1, 0];
        let mut pos = 0usize;
        for (cc, id) in cham_id.iter().enumerate() {
            if chamfer_corners & id != 0 {
                corners[pos].radius = 0;
                if chamfer != 0 {
                    let copy = corners[pos];
                    corners.insert(pos + 1, copy);
                    corners[pos].pos.0 += sign[(2 * cc) & 7] * chamfer;
                    corners[pos].pos.1 += sign[(2 * cc + 6) & 7] * chamfer;
                    corners[pos + 1].pos.0 += sign[(2 * cc + 1) & 7] * chamfer;
                    corners[pos + 1].pos.1 += sign[(2 * cc + 7) & 7] * chamfer;
                    pos += 1;
                }
            }
            pos += 1;
        }
        if cham_cnt > 1 && 2 * chamfer >= shorter {
            corner_list_remove_duplicates(&mut corners);
        }
    }
    corner_list_to_polygon_inside(&corners, error)
}

fn corner_list_remove_duplicates(corners: &mut Vec<RoundedCorner>) {
    let mut prev = corners[0].pos;
    for pos in (0..corners.len()).rev() {
        if corners[pos].pos == prev {
            corners.remove(pos);
        } else {
            prev = corners[pos].pos;
        }
    }
}

/// `CornerListToPolygon` for `aInflate == 0` and `ERROR_INSIDE`.
fn corner_list_to_polygon_inside(corners: &[RoundedCorner], error: i32) -> Vec<Pt> {
    let count = corners.len();
    let mut out: Vec<Pt> = Vec::new();
    let last = corners[count - 1].pos;
    let first = corners[0].pos;
    let mut incoming = (first.0 - last.0, first.1 - last.1);
    for n in 0..count {
        let cur = corners[n];
        let next = corners[(n + 1) % count];
        let outgoing = (next.pos.0 - cur.pos.0, next.pos.1 - cur.pos.1);
        if cur.radius == 0 {
            out.push(cur.pos);
        } else {
            let radius = cur.radius;
            let (end_angle, tan_angle2);
            if (incoming.0 == 0 && outgoing.1 == 0) || (incoming.1 == 0 && outgoing.0 == 0) {
                end_angle = Angle(90.0);
                tan_angle2 = 1.0;
            } else {
                let cos_num = f64::from(incoming.0) * f64::from(outgoing.0)
                    + f64::from(incoming.1) * f64::from(outgoing.1);
                let cos_den = euclid_norm(incoming) * euclid_norm(outgoing);
                let angle = (cos_num / cos_den).acos();
                tan_angle2 = ((std::f64::consts::PI - angle) / 2.0).tan();
                end_angle = Angle::from_radians(angle);
            }
            let num_segs = 16.max(arc_to_segment_count(radius, error, 360.0));
            let ang_delta = Angle(360.0 / f64::from(num_segs));
            let mut last_seg = end_angle.0;
            if last_seg > 0.0 {
                while last_seg > ang_delta.0 {
                    last_seg -= ang_delta.0;
                }
            } else {
                while last_seg < -ang_delta.0 {
                    last_seg += ang_delta.0;
                }
            }
            let mut ang_pos = if last_seg == 0.0 {
                ang_delta.0
            } else {
                (ang_delta.0 + last_seg) / 2.0
            };
            let arc_transition = if tan_angle2 > 0.0 {
                f64::from(radius) / tan_angle2
            } else {
                0.0
            };
            let inc_resized = vec_resize(incoming, arc_transition as i32);
            let arc_start = (cur.pos.0 - inc_resized.0, cur.pos.1 - inc_resized.1);
            let perp = (-incoming.1, incoming.0);
            let perp_r = vec_resize(perp, radius);
            let arc_centre = (arc_start.0 + perp_r.0, arc_start.1 + perp_r.1);
            // reflect arcStart over the line corner--centre
            let arc_end = reflect_point(cur.pos, arc_centre, arc_start);
            let arc_start_origin = (arc_start.0 - arc_centre.0, arc_start.1 - arc_centre.1);
            out.push(arc_start);
            let end_deg = end_angle.0;
            while ang_pos < end_deg {
                let p = rotate_origin(arc_start_origin.0, arc_start_origin.1, Angle(-ang_pos));
                out.push((p.0 + arc_centre.0, p.1 + arc_centre.1));
                ang_pos += ang_delta.0;
            }
            out.push(arc_end);
        }
        incoming = outgoing;
    }
    out
}

fn euclid_norm(v: Pt) -> f64 {
    f64::from(v.0).hypot(f64::from(v.1))
}

/// `rescale( num, value, den )` for 64-bit integers: `num * value / den`
/// rounded half away from zero (`libs/kimath/src/math/util.cpp`).
fn rescale(num: i64, value: i64, den: i64) -> i64 {
    let n = i128::from(num) * i128::from(value);
    let d = i128::from(den);
    let r = if (n < 0) ^ (d < 0) {
        (n - d / 2) / d
    } else {
        (n + d / 2) / d
    };
    r as i64
}

/// `VECTOR2I::Resize( newLength )` (`vector2d.h`): the vector scaled to the
/// new length with integer rounding.
pub fn vec_resize(v: Pt, new_len: i32) -> Pt {
    if v == (0, 0) {
        return (0, 0);
    }
    let (new_x, new_y);
    if v.0.abs() == v.1.abs() {
        new_x = f64::from(new_len.abs()) * std::f64::consts::FRAC_1_SQRT_2;
        new_y = new_x;
    } else {
        let x_sq = i64::from(v.0) * i64::from(v.0);
        let y_sq = i64::from(v.1) * i64::from(v.1);
        let l_sq = x_sq + y_sq;
        let nl_sq = i64::from(new_len) * i64::from(new_len);
        new_x = (rescale(nl_sq, x_sq, l_sq) as f64).sqrt();
        new_y = (rescale(nl_sq, y_sq, l_sq) as f64).sqrt();
    }
    let sign = new_len.signum();
    let rx = if v.0 < 0 {
        -ki_round(new_x)
    } else {
        ki_round(new_x)
    };
    let ry = if v.1 < 0 {
        -ki_round(new_y)
    } else {
        ki_round(new_y)
    };
    (rx * sign, ry * sign)
}

/// `SEG( a, b ).ReflectPoint( p )` (`seg.cpp`): `p` mirrored over the line
/// `a`-`b`, with the integer rounding of KiCad.
pub fn reflect_point(a: Pt, b: Pt, p: Pt) -> Pt {
    let d = (
        i64::from(b.0) - i64::from(a.0),
        i64::from(b.1) - i64::from(a.1),
    );
    let l_sq = d.0 * d.0 + d.1 * d.1;
    let t = d.0 * (i64::from(p.0) - i64::from(a.0)) + d.1 * (i64::from(p.1) - i64::from(a.1));
    let c = if l_sq == 0 {
        (i64::from(p.0), i64::from(p.1))
    } else {
        (
            i64::from(a.0) + rescale(t, d.0, l_sq),
            i64::from(a.1) + rescale(t, d.1, l_sq),
        )
    };
    (
        (2 * c.0 - i64::from(p.0)) as i32,
        (2 * c.1 - i64::from(p.1)) as i32,
    )
}

// ---- convex hull -------------------------------------------------------------------

/// `BuildConvexHull`: Andrew's monotone chain, counter-clockwise from the
/// lowest-leftmost point, collinear points dropped.
pub fn convex_hull(points: &[Pt]) -> Vec<Pt> {
    let mut pts: Vec<Pt> = points.to_vec();
    pts.sort();
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }
    let cross = |o: Pt, a: Pt, b: Pt| -> i64 {
        (i64::from(a.0) - i64::from(o.0)) * (i64::from(b.1) - i64::from(o.1))
            - (i64::from(a.1) - i64::from(o.1)) * (i64::from(b.0) - i64::from(o.0))
    };
    let mut lower: Vec<Pt> = Vec::new();
    for &p in &pts {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0 {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<Pt> = Vec::new();
    for &p in pts.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}
