//! Lines through two floating point points.

use crate::float_point::{FloatPoint, java_min};
use crate::limits::CRIT_INT;

/// A line in the plane defined by two `FloatPoint`s. Calculations are not exact; use
/// [`crate::line::Line`] where exactness is needed.
///
/// Port of `app.freerouting.geometry.planar.FloatLine`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FloatLine {
    pub a: FloatPoint,
    pub b: FloatPoint,
}

impl FloatLine {
    /// Creates a line from two points.
    pub const fn new(a: FloatPoint, b: FloatPoint) -> FloatLine {
        FloatLine { a, b }
    }

    /// The line with swapped end points.
    pub fn opposite(&self) -> FloatLine {
        FloatLine::new(self.b, self.a)
    }

    /// Returns this line or its opposite, so that its orientation matches `other`.
    pub fn adjust_direction(&self, other: &FloatLine) -> FloatLine {
        if self.b.side_of(self.a, other.a) == other.b.side_of(self.a, other.a) {
            return *self;
        }
        self.opposite()
    }

    /// The intersection of this line with `other`; `None` if the lines are parallel.
    pub fn intersection(&self, other: &FloatLine) -> Option<FloatPoint> {
        let d1x = self.b.x - self.a.x;
        let d1y = self.b.y - self.a.y;
        let d2x = other.b.x - other.a.x;
        let d2y = other.b.y - other.a.y;
        let det1 = self.a.x * self.b.y - self.a.y * self.b.x;
        let det2 = other.a.x * other.b.y - other.a.y * other.b.x;
        let det = d2x * d1y - d2y * d1x;
        if det == 0.0 {
            return None;
        }
        let is_x = (d2x * det1 - d1x * det2) / det;
        let is_y = (d2y * det1 - d1y * det2) / det;
        Some(FloatPoint::new(is_x, is_y))
    }

    /// Translates the line perpendicular by about `dist`: to the left if `dist > 0`,
    /// else to the right.
    pub fn translate(&self, dist: f64) -> FloatLine {
        let dx = self.b.x - self.a.x;
        let dy = self.b.y - self.a.y;
        let dxdx = dx * dx;
        let dydy = dy * dy;
        let length = (dxdx + dydy).sqrt();
        let new_a = if dxdx <= dydy {
            let rel_x = (dist * length) / dy;
            FloatPoint::new(self.a.x - rel_x, self.a.y)
        } else {
            let rel_y = (dist * length) / dx;
            FloatPoint::new(self.a.x, self.a.y + rel_y)
        };
        let new_b = FloatPoint::new(new_a.x + dx, new_a.y + dy);
        FloatLine::new(new_a, new_b)
    }

    /// Signed distance of this line from `point`; positive if the line is on the left of
    /// `point`.
    pub fn signed_distance(&self, point: FloatPoint) -> f64 {
        let dx = self.b.x - self.a.x;
        let dy = self.b.y - self.a.y;
        let det = dy * (point.x - self.a.x) - dx * (point.y - self.a.y);
        let length = (dx * dx + dy * dy).sqrt();
        det / length
    }

    /// Approximation of the perpendicular projection of `point` onto this line.
    pub fn perpendicular_projection(&self, point: FloatPoint) -> FloatPoint {
        let dx = self.b.x - self.a.x;
        let dy = self.b.y - self.a.y;
        if dx == 0.0 && dy == 0.0 {
            return self.a;
        }
        let dxdx = dx * dx;
        let dydy = dy * dy;
        let dxdy = dx * dy;
        let denominator = dxdx + dydy;
        let det = self.a.x * self.b.y - self.b.x * self.a.y;
        let x = (point.x * dxdx + point.y * dxdy + det * dy) / denominator;
        let y = (point.x * dxdy + point.y * dydy - det * dx) / denominator;
        FloatPoint::new(x, y)
    }

    /// Distance of `point` to the nearest point of the segment from `a` to `b`.
    pub fn segment_distance(&self, point: FloatPoint) -> f64 {
        let projection = self.perpendicular_projection(point);
        if projection.is_contained_in_box(self.a, self.b, 0.01) {
            point.distance(projection)
        } else {
            java_min(point.distance(self.a), point.distance(self.b))
        }
    }

    /// The perpendicular projection of `line_segment` onto this oriented segment; `None`
    /// if the projection is empty.
    pub fn segment_projection(&self, line_segment: &FloatLine) -> Option<FloatLine> {
        if self.b.scalar_product(self.a, line_segment.a) < 0.0 {
            return None;
        }
        if self.a.scalar_product(self.b, line_segment.b) < 0.0 {
            return None;
        }
        let crit = f64::from(CRIT_INT);
        let projected_a = if self.a.scalar_product(self.b, line_segment.a) < 0.0 {
            self.a
        } else {
            let p = self.perpendicular_projection(line_segment.a);
            if p.x.abs() >= crit || p.y.abs() >= crit {
                return None;
            }
            p
        };
        let projected_b = if self.b.scalar_product(self.a, line_segment.b) < 0.0 {
            self.b
        } else {
            self.perpendicular_projection(line_segment.b)
        };
        if projected_b.x.abs() >= crit || projected_b.y.abs() >= crit {
            return None;
        }
        Some(FloatLine::new(projected_a, projected_b))
    }

    /// The projection of `line_segment` onto this oriented segment by moving it
    /// perpendicular into the direction of this segment; `None` if the projection is
    /// empty or `line_segment` is degenerate.
    pub fn segment_projection_2(&self, line_segment: &FloatLine) -> Option<FloatLine> {
        if line_segment.a.scalar_product(line_segment.b, self.b) <= 0.0 {
            return None;
        }
        if line_segment.b.scalar_product(line_segment.a, self.a) <= 0.0 {
            return None;
        }
        let crit = f64::from(CRIT_INT);
        let projected_a = if line_segment.a.scalar_product(line_segment.b, self.a) < 0.0 {
            let perpendicular = FloatLine::new(
                line_segment.a,
                line_segment.b.turn_90_degree_around(1, line_segment.a),
            );
            let p = perpendicular.intersection(self)?;
            if p.x.abs() >= crit || p.y.abs() >= crit {
                return None;
            }
            p
        } else {
            self.a
        };
        let projected_b = if line_segment.b.scalar_product(line_segment.a, self.b) < 0.0 {
            let perpendicular = FloatLine::new(
                line_segment.b,
                line_segment.a.turn_90_degree_around(1, line_segment.b),
            );
            let p = perpendicular.intersection(self)?;
            if p.x.abs() >= crit || p.y.abs() >= crit {
                return None;
            }
            p
        } else {
            self.b
        };
        Some(FloatLine::new(projected_a, projected_b))
    }

    /// Shrinks this segment on both sides by `offset`; the result contains at least the
    /// midpoint.
    pub fn shrink_segment(&self, offset: f64) -> FloatLine {
        let dx = self.b.x - self.a.x;
        let dy = self.b.y - self.a.y;
        if dx == 0.0 && dy == 0.0 {
            return *self;
        }
        let length = (dx * dx + dy * dy).sqrt();
        let effective_offset = java_min(offset, length / 2.0);
        let new_a = FloatPoint::new(
            self.a.x + (dx * effective_offset) / length,
            self.a.y + (dy * effective_offset) / length,
        );
        let new_length = length - effective_offset;
        let new_b = FloatPoint::new(
            self.a.x + (dx * new_length) / length,
            self.a.y + (dy * new_length) / length,
        );
        FloatLine::new(new_a, new_b)
    }

    /// The nearest point to `from_point` on the segment from `a` to `b`.
    pub fn nearest_segment_point(&self, from_point: FloatPoint) -> FloatPoint {
        let projection = self.perpendicular_projection(from_point);
        if projection.is_contained_in_box(self.a, self.b, 0.01) {
            return projection;
        }
        if from_point.distance_square(self.a) <= from_point.distance_square(self.b) {
            self.a
        } else {
            self.b
        }
    }

    /// Divides this segment into `count` segments of nearly equal length.
    pub fn divide_segment_into_sections(&self, count: usize) -> Vec<FloatLine> {
        if count == 0 {
            return Vec::new();
        }
        if count == 1 {
            return vec![*self];
        }
        let line_length = self.b.distance(self.a);
        let section_length = line_length / count as f64;
        let dx = self.b.x - self.a.x;
        let dy = self.b.y - self.a.y;
        let mut result = Vec::with_capacity(count);
        let mut current_a = self.a;
        for i in 0..count {
            let current_b = if i == count - 1 {
                self.b
            } else {
                let current_distance = (i + 1) as f64 * section_length;
                FloatPoint::new(
                    self.a.x + (dx * current_distance) / line_length,
                    self.a.y + (dy * current_distance) / line_length,
                )
            };
            result.push(FloatLine::new(current_a, current_b));
            current_a = current_b;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fl(ax: f64, ay: f64, bx: f64, by: f64) -> FloatLine {
        FloatLine::new(FloatPoint::new(ax, ay), FloatPoint::new(bx, by))
    }

    #[test]
    fn intersection_of_crossing_and_parallel_lines() {
        let h = fl(0.0, 0.0, 10.0, 0.0);
        let v = fl(3.0, -5.0, 3.0, 5.0);
        assert_eq!(h.intersection(&v), Some(FloatPoint::new(3.0, 0.0)));
        assert_eq!(h.intersection(&fl(0.0, 1.0, 1.0, 1.0)), None);
    }

    #[test]
    fn projection_and_distances() {
        let l = fl(0.0, 0.0, 10.0, 0.0);
        let p = FloatPoint::new(4.0, 3.0);
        assert_eq!(l.perpendicular_projection(p), FloatPoint::new(4.0, 0.0));
        assert_eq!(l.signed_distance(p), -3.0);
        assert_eq!(l.segment_distance(FloatPoint::new(13.0, 4.0)), 5.0);
        assert_eq!(
            l.nearest_segment_point(FloatPoint::new(-2.0, 1.0)),
            FloatPoint::new(0.0, 0.0)
        );
    }

    #[test]
    fn shrink_and_divide() {
        let l = fl(0.0, 0.0, 10.0, 0.0);
        let s = l.shrink_segment(2.0);
        assert_eq!(s.a, FloatPoint::new(2.0, 0.0));
        assert_eq!(s.b, FloatPoint::new(8.0, 0.0));
        let parts = l.divide_segment_into_sections(4);
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[1].a, FloatPoint::new(2.5, 0.0));
        assert_eq!(parts[3].b, l.b);
    }

    #[test]
    fn segment_projections() {
        let l = fl(0.0, 0.0, 10.0, 0.0);
        let other = fl(-5.0, 3.0, 5.0, 3.0);
        let p = l.segment_projection(&other).unwrap();
        assert_eq!(p.a, FloatPoint::new(0.0, 0.0));
        assert_eq!(p.b, FloatPoint::new(5.0, 0.0));
        assert!(l.segment_projection(&fl(-5.0, 3.0, -1.0, 3.0)).is_none());
        let t = l.translate(2.0);
        assert_eq!(t.a, FloatPoint::new(0.0, 2.0));
    }
}
