//! The P/N offset with miter corners (X08.3), in integer geometry.
//!
//! Each centreline segment `c_i → c_{i+1}` (integer corners) is moved to
//! its left and to its right by an integer vector, so each offset line
//! passes through integer points:
//! - an axis-parallel segment moves along its normal by `D/2` (left, rounded
//!   down) and `D - D/2` (right), so the two lines are exactly `D` apart;
//! - a 45° segment moves by `(a, b)` steps along the two axes with
//!   `a + b = k`, which is `k/√2` along its normal; the left and right `k`
//!   add up to `ceil(D·√2)`, so the lines are at least `D` and less than
//!   `D + 1/√2` apart (never closer, so a gap equal to the clearance stays
//!   legal), and `|a - b| ≤ 1` keeps the end points within `1/√2` unit of
//!   the normal through the centreline's end;
//! - any other direction moves by the rounded normal vector, lengthened by
//!   half units until it reaches the offset.
//!
//! The left offset is `D/2` rounded down and the right one the rest, so
//! the centreline is up to `1/√2` unit off the middle; the centreline
//! search adds one unit to the compound half width for it
//! ([`super::model::Pair::search_half_width`]).
//!
//! Consecutive offset lines are intersected for the corners (the miter;
//! KRT's `corner_scale = 2 / bisector_length` gives the same point), so
//! every offset segment stays parallel to its centreline segment at the
//! distance above, and on the inside of a corner the gap is the segments'
//! distance everywhere. On the outside of a corner, the outer track's
//! miter point is `D·(1/cos(θ/2) - 1)` farther from the inner track's
//! corner than `D` (8.2 % of `D` at a 45° turn): that is the miter, not a
//! gap change along the segments. The end corners are the offset start
//! and end points themselves (the cap lines are normal to the first and
//! last segment through them). The corners between two diagonal segments
//! at 90° may be rational; the polyline keeps the exact lines.
//!
//! An offset segment that would run backwards (a centreline segment
//! shorter than its corners' miters need) or a miter longer than 3 times
//! the offset (KRT's cap) is refused.
//!
//! The miter idea is KiCadRoutingTools' (`docs/differential-pairs.md`,
//! "Corner Handling (Miters)", at `0486e799`); the integer construction
//! is written here.

use freerouting::geometry::{FloatPoint, IntPoint, Line, Point, Polyline};

/// The longest miter, as a multiple of the offset (KRT's cap).
pub const MAX_MITER_SCALE: f64 = 3.0;

/// The P and N polylines of one centreline.
#[derive(Clone, Debug)]
pub struct OffsetPair {
    /// The track on the centreline's left.
    pub left: Polyline,
    /// The track on its right.
    pub right: Polyline,
}

/// The integer shift of a segment with direction `(dx, dy)` to its left
/// (`left`) or right, for the centre distance `d`.
fn shift(dx: i64, dy: i64, d: i64, left: bool) -> (i64, i64) {
    let sign = if left { 1 } else { -1 };
    if dx == 0 || dy == 0 {
        let dist = if left { d / 2 } else { d - d / 2 };
        // The left normal of (dx, dy) is (-dy, dx).
        return (-dy.signum() * dist * sign, dx.signum() * dist * sign);
    }
    if dx.abs() == dy.abs() {
        let total = (d as f64 * std::f64::consts::SQRT_2).ceil() as i64;
        let k = if left { total / 2 } else { total - total / 2 };
        let a = k / 2;
        let b = k - a;
        let (sx, sy) = (dx.signum(), dy.signum());
        return (-sy * a * sign, sx * b * sign);
    }
    let len = (dx as f64).hypot(dy as f64);
    let want = if left { d / 2 } else { d - d / 2 } as f64;
    let sign = sign as f64;
    // The rounded normal vector, lengthened until it reaches `want`.
    let mut dist = want;
    loop {
        let nx = (-(dy as f64) / len * dist * sign).round();
        let ny = ((dx as f64) / len * dist * sign).round();
        let reached = (-nx * dy as f64 + ny * dx as f64) * sign / len;
        if reached >= want || dist > want + 4.0 {
            return (nx as i64, ny as i64);
        }
        dist += 0.5;
    }
}

fn add(p: IntPoint, v: (i64, i64)) -> Result<IntPoint, String> {
    let x = i32::try_from(i64::from(p.x) + v.0).map_err(|_| "offset outside i32")?;
    let y = i32::try_from(i64::from(p.y) + v.1).map_err(|_| "offset outside i32")?;
    Ok(IntPoint::new(x, y))
}

/// The polyline on one side of `centre` at centre distance `d`.
fn offset_side(centre: &[IntPoint], d: i64, left: bool) -> Result<Polyline, String> {
    let n = centre.len();
    let mut lines: Vec<Line> = Vec::with_capacity(n + 1);
    for i in 0..n - 1 {
        let (a, b) = (centre[i], centre[i + 1]);
        let dx = i64::from(b.x) - i64::from(a.x);
        let dy = i64::from(b.y) - i64::from(a.y);
        let s = shift(dx, dy, d, left);
        let (oa, ob) = (add(a, s)?, add(b, s)?);
        if i == 0 {
            let normal = add(oa, (-dy, dx))?;
            lines.push(Line::new(oa, normal));
        }
        lines.push(Line::new(oa, ob));
        if i == n - 2 {
            let normal = add(ob, (-dy, dx))?;
            lines.push(Line::new(ob, normal));
        }
    }
    let polyline = Polyline::new(&lines);
    if polyline.corner_count() != n {
        return Err(format!(
            "the offset lost corners ({} of {n})",
            polyline.corner_count()
        ));
    }
    check_segments(centre, &polyline, d)?;
    Ok(polyline)
}

/// Refuses segments that run backwards and miters past the cap.
fn check_segments(centre: &[IntPoint], polyline: &Polyline, d: i64) -> Result<(), String> {
    let q: Vec<FloatPoint> = polyline.corners().iter().map(Point::to_float).collect();
    let half = d as f64 / 2.0;
    for i in 0..centre.len() - 1 {
        let cx = f64::from(centre[i + 1].x) - f64::from(centre[i].x);
        let cy = f64::from(centre[i + 1].y) - f64::from(centre[i].y);
        let ox = q[i + 1].x - q[i].x;
        let oy = q[i + 1].y - q[i].y;
        if cx * ox + cy * oy <= 0.0 {
            return Err(format!(
                "centreline segment {i} is too short for the offset at its corners"
            ));
        }
    }
    for i in 1..centre.len() - 1 {
        let c = centre[i].to_float();
        let miter = (q[i].x - c.x).hypot(q[i].y - c.y);
        if miter > MAX_MITER_SCALE * half.max(1.0) {
            return Err(format!("the miter at corner {i} is longer than 3 offsets"));
        }
    }
    Ok(())
}

/// The left and right tracks of `centre` (integer corners, at least two,
/// no repeated points) at centre distance `d` ([module docs](self)).
pub fn offset_pair(centre: &[IntPoint], d: i32) -> Result<OffsetPair, String> {
    if centre.len() < 2 {
        return Err("the centreline has fewer than two corners".to_string());
    }
    if centre.windows(2).any(|w| w[0] == w[1]) {
        return Err("the centreline repeats a corner".to_string());
    }
    let d = i64::from(d);
    Ok(OffsetPair {
        left: offset_side(centre, d, true)?,
        right: offset_side(centre, d, false)?,
    })
}

/// The integer corners of `polyline`, `None` if one is rational.
pub fn int_corners(polyline: &Polyline) -> Option<Vec<IntPoint>> {
    polyline.corners().iter().map(Point::as_int).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diffpair::measure::{point_line_distance, polyline_distance};

    const HW: f64 = 1000.0;
    const G: f64 = 2000.0;
    const D: i32 = 4000;

    fn p(x: i32, y: i32) -> IntPoint {
        IntPoint::new(x, y)
    }

    /// Samples `n` points on each segment of `a`; returns, per sample, the
    /// edge gap to the corresponding segment line of `b` and to the whole
    /// polyline `b` (nearest point).
    fn gaps(a: &Polyline, b: &Polyline, n: usize) -> Vec<(f64, f64)> {
        let qa: Vec<FloatPoint> = a.corners().iter().map(Point::to_float).collect();
        let qb: Vec<FloatPoint> = b.corners().iter().map(Point::to_float).collect();
        let mut out = Vec::new();
        for i in 0..qa.len() - 1 {
            for s in 0..n {
                let t = s as f64 / (n - 1) as f64;
                let x = FloatPoint::new(
                    qa[i].x + (qa[i + 1].x - qa[i].x) * t,
                    qa[i].y + (qa[i + 1].y - qa[i].y) * t,
                );
                let seg = point_line_distance(x, qb[i], qb[i + 1]) - 2.0 * HW;
                let near = polyline_distance(x, &qb) - 2.0 * HW;
                out.push((seg, near));
            }
        }
        out
    }

    fn check(centre: &[IntPoint]) {
        let pair = offset_pair(centre, D).expect("offset");
        for (a, b, name) in [
            (&pair.left, &pair.right, "left to right"),
            (&pair.right, &pair.left, "right to left"),
        ] {
            let g = gaps(a, b, 100);
            assert_eq!(g.len(), 100 * (centre.len() - 1));
            for (i, (seg, near)) in g.iter().enumerate() {
                assert!((seg - G).abs() <= 1.0, "{name} sample {i}: gap {seg}");
                assert!(*near >= G - 1.0, "{name} sample {i}: nearest gap {near}");
            }
        }
        // The inner track sees the outer one at the gap everywhere.
        assert_eq!(pair.left.corner_count(), centre.len());
        assert_eq!(pair.right.corner_count(), centre.len());
    }

    #[test]
    fn a_45_and_a_90_degree_corner_keep_the_gap() {
        // East, a 45° turn to north-east, a 90° turn to north-west.
        check(&[
            p(0, 0),
            p(100_000, 0),
            p(150_000, 50_000),
            p(110_000, 90_000),
        ]);
        // East, then a 90° turn north, then a 45° turn north-west.
        check(&[p(0, 0), p(80_000, 0), p(80_000, 60_000), p(50_000, 90_000)]);
    }

    #[test]
    fn the_inner_track_sees_the_outer_at_the_gap_and_the_miter_is_outside() {
        let centre = [p(0, 0), p(100_000, 0), p(150_000, 50_000)];
        let pair = offset_pair(&centre, D).expect("offset");
        // The turn is to the left: the left track is inside.
        let inner: Vec<FloatPoint> = pair.left.corners().iter().map(Point::to_float).collect();
        let outer: Vec<FloatPoint> = pair.right.corners().iter().map(Point::to_float).collect();
        let at_corner = polyline_distance(inner[1], &outer) - 2.0 * HW;
        assert!((at_corner - G).abs() <= 1.0, "{at_corner}");
        let miter = polyline_distance(outer[1], &inner) - 2.0 * HW;
        let expected = f64::from(D) / (22.5f64.to_radians()).cos() - 2.0 * HW;
        assert!((miter - expected).abs() <= 1.0, "{miter} vs {expected}");
    }

    #[test]
    fn ends_and_axis_offsets_are_integer_and_exact() {
        let centre = [p(0, 0), p(100_000, 0), p(100_000, 50_000)];
        let pair = offset_pair(&centre, 4001).expect("offset");
        let left = int_corners(&pair.left).expect("integer corners");
        let right = int_corners(&pair.right).expect("integer corners");
        assert_eq!(left, vec![p(0, 2000), p(98_000, 2000), p(98_000, 50_000)]);
        assert_eq!(
            right,
            vec![p(0, -2001), p(102_001, -2001), p(102_001, 50_000)]
        );
        // Diagonal ends lie within 1/√2 unit of the normal.
        let centre = [p(0, 0), p(70_000, 70_000)];
        let pair = offset_pair(&centre, D).expect("offset");
        for poly in [&pair.left, &pair.right] {
            let q = poly.first_corner().to_float();
            let along = (q.x + q.y) / std::f64::consts::SQRT_2;
            assert!(
                along.abs() <= std::f64::consts::FRAC_1_SQRT_2 + 1e-9,
                "{along}"
            );
        }
    }

    #[test]
    fn a_too_short_segment_is_refused() {
        // Two 90° turns 1000 units apart: the inner offset folds over.
        let centre = [p(0, 0), p(50_000, 0), p(50_000, 1000), p(0, 1000)];
        assert!(offset_pair(&centre, D).is_err());
        assert!(offset_pair(&[p(0, 0)], D).is_err());
        assert!(offset_pair(&[p(0, 0), p(0, 0), p(5, 5)], D).is_err());
    }
}
