//! K15.11: an item's own copper, measured as KiCad measures it.
//!
//! The search trees hold a circle as its bounding octagon and a trace as
//! segments with octagonal ends, so Java's clearance check measures copper
//! up to 8 % larger than it is (a via 0.127 mm from a pad measures 0.095 mm).
//! [`Copper`] is the copper itself, per layer: a trace is its centre line
//! widened by half its width with round ends, a via or a round pad its
//! circle, any other pad its polygon. A drilled hole is a [`Copper`] too: a
//! point (or a slot's centre segment) widened by the drill radius.
//!
//! Distances are in board units, in `f64` (the report prints them with four
//! decimals in mm; nothing here decides a route).

use freerouting_geometry::{FloatPoint, PolylineShapeOps, Shape};

use crate::board::items::{Item, ItemKind};

/// Copper (or a hole) on one layer: segments widened by a radius, or a
/// polygon.
#[derive(Clone, Debug)]
pub(crate) enum Copper {
    /// Segments (a point is a segment of length 0) widened by the radius.
    Rounded(Vec<(FloatPoint, FloatPoint)>, f64),
    /// A polygon's corners.
    Polygon(Vec<FloatPoint>),
}

impl Copper {
    /// The copper of a trace, via or pin on `layer`; `None` for other items or
    /// a layer the item has no copper on.
    pub(crate) fn of(item: &Item, layer: i32) -> Option<Copper> {
        match &item.kind {
            ItemKind::Trace(trace) => {
                if trace.layer != layer {
                    return None;
                }
                let corners = trace.polyline.corner_approx_arr();
                let segments = corners.windows(2).map(|w| (w[0], w[1])).collect();
                Some(Copper::Rounded(segments, f64::from(trace.half_width)))
            }
            ItemKind::Pin(_) | ItemKind::Via(_) => match item.drill()?.shape_on_layer(layer)? {
                Shape::Circle(circle) => Some(Copper::disc(
                    circle.center.to_float(),
                    f64::from(circle.radius),
                )),
                Shape::Polyline(polygon) => Some(Copper::Polygon(polygon.corner_approx_arr())),
            },
            _ => None,
        }
    }

    /// A disc of `radius` around `centre`.
    pub(crate) fn disc(centre: FloatPoint, radius: f64) -> Copper {
        Copper::Rounded(vec![(centre, centre)], radius)
    }

    /// The core segments (a polygon's border) and the radius around them.
    fn core(&self) -> (Vec<(FloatPoint, FloatPoint)>, f64) {
        match self {
            Copper::Rounded(segments, radius) => (segments.clone(), *radius),
            Copper::Polygon(corners) => {
                let n = corners.len();
                (
                    (0..n).map(|i| (corners[i], corners[(i + 1) % n])).collect(),
                    0.0,
                )
            }
        }
    }

    /// The distance from `point` to this copper (0 or less inside it).
    pub(crate) fn distance_to(&self, point: FloatPoint) -> f64 {
        if let Copper::Polygon(corners) = self
            && polygon_contains(corners, point)
        {
            return 0.0;
        }
        let (segments, radius) = self.core();
        segments
            .iter()
            .map(|(a, b)| point_segment_distance(*a, *b, point))
            .fold(f64::MAX, f64::min)
            - radius
    }

    /// True if `point` lies on this copper, its border included.
    pub(crate) fn contains(&self, point: FloatPoint) -> bool {
        self.distance_to(point) <= 0.0
    }

    /// The gap between this copper and `other` (0 or less where they meet).
    pub(crate) fn gap(&self, other: &Copper) -> f64 {
        let ((sa, ra), (sb, rb)) = (self.core(), other.core());
        for (polygon, segments) in [(self, &sb), (other, &sa)] {
            if let Copper::Polygon(corners) = polygon
                && segments
                    .iter()
                    .any(|(start, _)| polygon_contains(corners, *start))
            {
                return 0.0;
            }
        }
        sa.iter()
            .flat_map(|(p, q)| {
                sb.iter()
                    .map(move |(r, s)| segment_distance(*p, *q, *r, *s))
            })
            .fold(f64::MAX, f64::min)
            - ra
            - rb
    }
}

/// The clearance between the own copper of `a` and `b` on `layer`, in board
/// units (0 or less where they overlap); `None` if one of them is no trace,
/// via or pin there.
pub(crate) fn exact_clearance(a: &Item, b: &Item, layer: i32) -> Option<f64> {
    Some(Copper::of(a, layer)?.gap(&Copper::of(b, layer)?))
}

pub(crate) fn point_segment_distance(a: FloatPoint, b: FloatPoint, p: FloatPoint) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let length_squared = dx * dx + dy * dy;
    let t = if length_squared == 0.0 {
        0.0
    } else {
        (((p.x - a.x) * dx + (p.y - a.y) * dy) / length_squared).clamp(0.0, 1.0)
    };
    (a.x + t * dx - p.x).hypot(a.y + t * dy - p.y)
}

fn cross(o: FloatPoint, a: FloatPoint, b: FloatPoint) -> f64 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

fn segment_distance(a: FloatPoint, b: FloatPoint, c: FloatPoint, d: FloatPoint) -> f64 {
    let crosses = cross(a, b, c) * cross(a, b, d) < 0.0 && cross(c, d, a) * cross(c, d, b) < 0.0;
    if crosses {
        return 0.0;
    }
    point_segment_distance(a, b, c)
        .min(point_segment_distance(a, b, d))
        .min(point_segment_distance(c, d, a))
        .min(point_segment_distance(c, d, b))
}

/// Even-odd point in polygon, the border included.
fn polygon_contains(corners: &[FloatPoint], p: FloatPoint) -> bool {
    let n = corners.len();
    let mut inside = false;
    for i in 0..n {
        let (a, b) = (corners[i], corners[(i + n - 1) % n]);
        if point_segment_distance(a, b, p) == 0.0 {
            return true;
        }
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> FloatPoint {
        FloatPoint::new(x, y)
    }

    #[test]
    fn segment_distances() {
        assert_eq!(
            segment_distance(p(0.0, 0.0), p(10.0, 0.0), p(5.0, 3.0), p(5.0, 7.0)),
            3.0
        );
        assert_eq!(
            segment_distance(p(0.0, 0.0), p(10.0, 0.0), p(5.0, -3.0), p(5.0, 7.0)),
            0.0
        );
        assert_eq!(
            segment_distance(p(0.0, 0.0), p(0.0, 0.0), p(3.0, 4.0), p(3.0, 4.0)),
            5.0
        );
    }

    #[test]
    fn point_in_polygon_with_its_border() {
        let square = [p(0.0, 0.0), p(4.0, 0.0), p(4.0, 4.0), p(0.0, 4.0)];
        assert!(polygon_contains(&square, p(2.0, 2.0)));
        assert!(polygon_contains(&square, p(4.0, 2.0)));
        assert!(!polygon_contains(&square, p(5.0, 2.0)));
    }

    #[test]
    fn gaps_between_discs_segments_and_polygons() {
        let a = Copper::disc(p(0.0, 0.0), 1.0);
        let b = Copper::disc(p(5.0, 0.0), 1.5);
        assert_eq!(a.gap(&b), 2.5);
        let trace = Copper::Rounded(vec![(p(0.0, 3.0), p(10.0, 3.0))], 0.5);
        assert_eq!(a.gap(&trace), 1.5);
        let square = Copper::Polygon(vec![p(2.0, -1.0), p(4.0, -1.0), p(4.0, 1.0), p(2.0, 1.0)]);
        assert_eq!(a.gap(&square), 1.0);
        // A disc whose centre is inside the polygon overlaps it.
        assert_eq!(Copper::disc(p(3.0, 0.0), 0.1).gap(&square), 0.0);
        assert_eq!(square.distance_to(p(3.0, 0.0)), 0.0);
        assert_eq!(square.distance_to(p(7.0, 0.0)), 3.0);
        assert!(trace.contains(p(5.0, 3.5)));
        assert!(!trace.contains(p(5.0, 3.6)));
    }
}
