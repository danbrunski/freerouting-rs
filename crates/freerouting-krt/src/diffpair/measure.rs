//! Float distances for checks and the coupled-length report (X08.5).
//!
//! These decide nothing on the board: the stage's DRC gate is the exact
//! clearance checker. They measure what the tests and the report state.

use freerouting::geometry::FloatPoint;

/// The distance from `p` to the segment `a b`.
pub fn point_segment_distance(p: FloatPoint, a: FloatPoint, b: FloatPoint) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    if len2 == 0.0 {
        return (p.x - a.x).hypot(p.y - a.y);
    }
    let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0);
    (p.x - (a.x + t * dx)).hypot(p.y - (a.y + t * dy))
}

/// The distance from `p` to the line through `a` and `b`.
pub fn point_line_distance(p: FloatPoint, a: FloatPoint, b: FloatPoint) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len = dx.hypot(dy);
    if len == 0.0 {
        return (p.x - a.x).hypot(p.y - a.y);
    }
    ((p.x - a.x) * dy - (p.y - a.y) * dx).abs() / len
}

/// The distance from `p` to the polyline through `corners`.
pub fn polyline_distance(p: FloatPoint, corners: &[FloatPoint]) -> f64 {
    match corners {
        [] => f64::INFINITY,
        [a] => (p.x - a.x).hypot(p.y - a.y),
        _ => corners
            .windows(2)
            .map(|s| point_segment_distance(p, s[0], s[1]))
            .fold(f64::INFINITY, f64::min),
    }
}

fn cross(o: FloatPoint, a: FloatPoint, b: FloatPoint) -> f64 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

/// True if the segments `a b` and `c d` cross or touch.
fn segments_meet(a: FloatPoint, b: FloatPoint, c: FloatPoint, d: FloatPoint) -> bool {
    let d1 = cross(c, d, a);
    let d2 = cross(c, d, b);
    let d3 = cross(a, b, c);
    let d4 = cross(a, b, d);
    ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
}

/// The distance between the segments `a b` and `c d`.
pub fn segment_distance(a: FloatPoint, b: FloatPoint, c: FloatPoint, d: FloatPoint) -> f64 {
    if segments_meet(a, b, c, d) {
        return 0.0;
    }
    point_segment_distance(a, c, d)
        .min(point_segment_distance(b, c, d))
        .min(point_segment_distance(c, a, b))
        .min(point_segment_distance(d, a, b))
}

/// True if `p` is inside the convex polygon `polygon` (either winding).
fn convex_contains(p: FloatPoint, polygon: &[FloatPoint]) -> bool {
    if polygon.len() < 3 {
        return false;
    }
    let n = polygon.len();
    let mut sign = 0.0;
    for i in 0..n {
        let c = cross(polygon[i], polygon[(i + 1) % n], p);
        if c != 0.0 {
            if sign == 0.0 {
                sign = c.signum();
            } else if c.signum() != sign {
                return false;
            }
        }
    }
    true
}

/// The distance between the segment `a b` and the convex polygon
/// `polygon` (0 if they meet).
pub fn segment_polygon_distance(a: FloatPoint, b: FloatPoint, polygon: &[FloatPoint]) -> f64 {
    match polygon {
        [] => f64::INFINITY,
        [p] => point_segment_distance(*p, a, b),
        _ => {
            if convex_contains(a, polygon) || convex_contains(b, polygon) {
                return 0.0;
            }
            let n = polygon.len();
            (0..n)
                .map(|i| segment_distance(a, b, polygon[i], polygon[(i + 1) % n]))
                .fold(f64::INFINITY, f64::min)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(x: f64, y: f64) -> FloatPoint {
        FloatPoint::new(x, y)
    }

    #[test]
    fn distances() {
        assert_eq!(
            point_segment_distance(f(0.0, 3.0), f(-1.0, 0.0), f(1.0, 0.0)),
            3.0
        );
        assert_eq!(
            point_segment_distance(f(4.0, 4.0), f(-1.0, 0.0), f(1.0, 0.0)),
            5.0
        );
        assert_eq!(
            point_line_distance(f(4.0, 4.0), f(-1.0, 0.0), f(1.0, 0.0)),
            4.0
        );
        assert_eq!(
            segment_distance(f(0.0, 0.0), f(2.0, 2.0), f(0.0, 2.0), f(2.0, 0.0)),
            0.0
        );
        let square = [f(0.0, 0.0), f(2.0, 0.0), f(2.0, 2.0), f(0.0, 2.0)];
        assert_eq!(
            segment_polygon_distance(f(5.0, -1.0), f(5.0, 3.0), &square),
            3.0
        );
        assert_eq!(
            segment_polygon_distance(f(1.0, 1.0), f(9.0, 9.0), &square),
            0.0
        );
        assert_eq!(
            segment_polygon_distance(f(-1.0, 1.0), f(3.0, 1.0), &square),
            0.0
        );
    }
}
