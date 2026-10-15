//! Line segments given by a line and two closing lines.

use std::sync::OnceLock;

use crate::float_point::{FloatPoint, java_max, java_min};
use crate::jmath::{d2i, round_to_i32};
use crate::line::Line;
use crate::point::{IntPoint, Point};
use crate::polyline::Polyline;
use crate::shapes::{IntBox, IntOctagon, PolylineShapeOps, Simplex, TileShapeOps};
use crate::side::Side;
use crate::signum::Signum;
use crate::vector::IntVector;

/// A line segment, given by its carrying line (`middle`) and two lines that close it at
/// its start and end point. The end points are the intersections of `middle` with the
/// closing lines, so they may be rational.
///
/// Port of `app.freerouting.geometry.planar.LineSegment`.
#[derive(Debug, Clone)]
pub struct LineSegment {
    start: Line,
    middle: Line,
    end: Line,
    start_point: OnceLock<Point>,
    end_point: OnceLock<Point>,
}

impl LineSegment {
    /// Creates the segment on `middle_line` between its intersections with
    /// `start_line` and `end_line`.
    pub fn new(start_line: Line, middle_line: Line, end_line: Line) -> LineSegment {
        LineSegment {
            start: start_line,
            middle: middle_line,
            end: end_line,
            start_point: OnceLock::new(),
            end_point: OnceLock::new(),
        }
    }

    /// The segment number `no` of `polyline`: the line `no` closed by its neighbours.
    ///
    /// # Panics
    /// If `no` is not in `1..polyline.lines().len() - 1` (Java creates a segment with
    /// `null` lines that fails on first use).
    pub fn from_polyline(polyline: &Polyline, no: usize) -> LineSegment {
        let lines = polyline.lines();
        assert!(
            no >= 1 && no + 1 < lines.len(),
            "LineSegment from Polyline: no {no} out of range for {} lines",
            lines.len()
        );
        LineSegment::new(lines[no - 1], lines[no], lines[no + 1])
    }

    /// The border segment number `no` of `shape`, closed by the neighbouring border
    /// lines.
    ///
    /// # Panics
    /// If `no >= shape.border_line_count()` (Java creates a segment with `null` lines
    /// that fails on first use).
    pub fn from_shape<T: PolylineShapeOps + ?Sized>(shape: &T, no: usize) -> LineSegment {
        let line_count = shape.border_line_count();
        assert!(
            no < line_count,
            "LineSegment from TileShape: no {no} out of range for {line_count} border lines"
        );
        let start = if no == 0 {
            shape.border_line(line_count - 1)
        } else {
            shape.border_line(no - 1)
        };
        let end = if no == line_count - 1 {
            shape.border_line(0)
        } else {
            shape.border_line(no + 1)
        };
        LineSegment::new(start, shape.border_line(no), end)
    }

    /// The intersection of the middle line with the start line.
    pub fn start_point(&self) -> Point {
        self.start_point
            .get_or_init(|| self.middle.intersection(&self.start))
            .clone()
    }

    /// The intersection of the middle line with the end line.
    pub fn end_point(&self) -> Point {
        self.end_point
            .get_or_init(|| self.middle.intersection(&self.end))
            .clone()
    }

    /// Approximation of the start point (the exact one if already computed).
    pub fn start_point_approx(&self) -> FloatPoint {
        match self.start_point.get() {
            Some(p) => p.to_float(),
            None => self.start.intersection_approx(&self.middle),
        }
    }

    /// Approximation of the end point (the exact one if already computed).
    pub fn end_point_approx(&self) -> FloatPoint {
        match self.end_point.get() {
            Some(p) => p.to_float(),
            None => self.end.intersection_approx(&self.middle),
        }
    }

    /// The carrying line of this segment.
    pub fn get_line(&self) -> Line {
        self.middle
    }

    /// The line closing this segment at its start point.
    pub fn get_start_closing_line(&self) -> Line {
        self.start
    }

    /// The line closing this segment at its end point.
    pub fn get_end_closing_line(&self) -> Line {
        self.end
    }

    /// The segment with reversed direction.
    pub fn opposite(&self) -> LineSegment {
        LineSegment::new(
            self.end.opposite(),
            self.middle.opposite(),
            self.start.opposite(),
        )
    }

    /// The segment as a polyline of three lines.
    pub fn to_polyline(&self) -> Polyline {
        Polyline::new(&[self.start, self.middle, self.end])
    }

    /// The segment as a (degenerate, one-dimensional) simplex.
    pub fn to_simplex(&self) -> Simplex {
        let first = if self.end_point().side_of_line(&self.start) == Side::OnTheRight {
            self.start.opposite()
        } else {
            self.start
        };
        let last = if self.start_point().side_of_line(&self.end) == Side::OnTheRight {
            self.end.opposite()
        } else {
            self.end
        };
        Simplex::get_instance(&[first, self.middle, self.middle.opposite(), last])
    }

    /// True if `point` lies on this segment. Only implemented for integer points; false
    /// for rational points (Java logs a warning).
    pub fn contains(&self, point: &Point) -> bool {
        let Point::Int(int_point) = point else {
            return false;
        };
        if self.middle.side_of(point) != Side::Collinear {
            return false;
        }
        let perpendicular_direction = self.middle.int_direction().turn_45_degree(2);
        let perpendicular_line =
            Line::from_point_and_direction(*int_point, perpendicular_direction);
        let start_point_side = perpendicular_line.side_of(&self.start_point());
        let end_point_side = perpendicular_line.side_of(&self.end_point());
        start_point_side != end_point_side || start_point_side == Side::Collinear
    }

    /// The smallest box containing the approximated end points.
    pub fn bounding_box(&self) -> IntBox {
        let start_corner = self.middle.intersection_approx(&self.start);
        let end_corner = self.middle.intersection_approx(&self.end);
        let llx = java_min(start_corner.x, end_corner.x);
        let lly = java_min(start_corner.y, end_corner.y);
        let urx = java_max(start_corner.x, end_corner.x);
        let ury = java_max(start_corner.y, end_corner.y);
        IntBox::from_points(
            IntPoint::new(d2i(llx.floor()), d2i(lly.floor())),
            IntPoint::new(d2i(urx.ceil()), d2i(ury.ceil())),
        )
    }

    /// The smallest normalized octagon containing the approximated end points.
    pub fn bounding_octagon(&self) -> IntOctagon {
        let start_corner = self.middle.intersection_approx(&self.start);
        let end_corner = self.middle.intersection_approx(&self.end);
        let lx = java_min(start_corner.x, end_corner.x).floor();
        let ly = java_min(start_corner.y, end_corner.y).floor();
        let rx = java_max(start_corner.x, end_corner.x).ceil();
        let uy = java_max(start_corner.y, end_corner.y).ceil();
        let start_x_minus_y = start_corner.x - start_corner.y;
        let end_x_minus_y = end_corner.x - end_corner.y;
        let ulx = java_min(start_x_minus_y, end_x_minus_y).floor();
        let lrx = java_max(start_x_minus_y, end_x_minus_y).ceil();
        let start_x_plus_y = start_corner.x + start_corner.y;
        let end_x_plus_y = end_corner.x + end_corner.y;
        let llx = java_min(start_x_plus_y, end_x_plus_y).floor();
        let urx = java_max(start_x_plus_y, end_x_plus_y).ceil();
        IntOctagon::new(
            d2i(lx),
            d2i(ly),
            d2i(rx),
            d2i(uy),
            d2i(ulx),
            d2i(lrx),
            d2i(llx),
            d2i(urx),
        )
        .normalize()
    }

    /// A segment with the same start point and approximately the length `new_length`;
    /// the new end line is perpendicular to the middle line.
    pub fn change_length_approx(&self, new_length: f64) -> LineSegment {
        let new_end_point = self
            .start_point_approx()
            .change_length(self.end_point_approx(), new_length);
        let perpendicular_direction = self.middle.int_direction().turn_45_degree(2);
        let new_end_line =
            Line::from_point_and_direction(new_end_point.round(), perpendicular_direction);
        LineSegment::new(self.start, self.middle, new_end_line)
    }

    /// The intersection with `other`, described by lines: empty if the segments do not
    /// intersect, one line through the intersection point, or two lines delimiting the
    /// common part of overlapping segments.
    pub fn intersection(&self, other: &LineSegment) -> Vec<Line> {
        if !self.bounding_box().intersects_box(&other.bounding_box()) {
            return Vec::new();
        }
        let start_point_side = self.start_point().side_of_line(&other.middle);
        let end_point_side = self.end_point().side_of_line(&other.middle);
        if start_point_side == Side::Collinear && end_point_side == Side::Collinear {
            // The segments are collinear.
            let this_sorted = self.sort_endpoints_in_xy();
            let other_sorted = other.sort_endpoints_in_xy();
            let (left_line, right_line) = if this_sorted
                .start_point()
                .compare_xy(&other_sorted.start_point())
                <= 0
            {
                (this_sorted, other_sorted)
            } else {
                (other_sorted, this_sorted)
            };
            let cmp = left_line.end_point().compare_xy(&right_line.start_point());
            if cmp < 0 {
                return Vec::new();
            }
            if cmp == 0 {
                return vec![left_line.end];
            }
            let second = if right_line.end_point().compare_xy(&left_line.end_point()) >= 0 {
                left_line.end
            } else {
                right_line.end
            };
            return vec![right_line.start, second];
        }
        if start_point_side == end_point_side
            || other.start_point().side_of_line(&self.middle)
                == other.end_point().side_of_line(&self.middle)
        {
            return Vec::new();
        }
        vec![other.middle]
    }

    /// True if this segment and `other` have a common point.
    pub fn intersects(&self, other: &LineSegment) -> bool {
        !self.intersection(other).is_empty()
    }

    /// True if this segment and `other` have a common part of positive length.
    pub fn overlaps(&self, other: &LineSegment) -> bool {
        self.intersection(other).len() > 1
    }

    /// Approximates this segment by an orthogonal stair with stairs of width about
    /// `width`. The corners of the stair are returned; empty if the rounded end points
    /// are equal.
    ///
    /// # Panics
    /// If the stair width rounds to 0 (an `ArithmeticException` in Java).
    pub fn stair_approximation(&self, width: f64, to_the_right: bool) -> Vec<IntPoint> {
        let start_point = self.start_point().to_float().round();
        let end_point = self.end_point().to_float().round();
        if start_point == end_point {
            return Vec::new();
        }
        if start_point.x == end_point.x || start_point.y == end_point.y {
            return vec![start_point, end_point];
        }
        let dx = end_point.x.wrapping_sub(start_point.x);
        let dy = end_point.y.wrapping_sub(start_point.y);
        let abs_dx = dx.wrapping_abs();
        let abs_dy = dy.wrapping_abs();
        let function_of_x = abs_dx >= abs_dy;
        let (mut stair_width, stair_count) = if function_of_x {
            let w = round_to_i32((width * f64::from(abs_dx)) / f64::from(abs_dy));
            (w, (abs_dx - 1) / w + 1)
        } else {
            let w = round_to_i32((width * f64::from(abs_dy)) / f64::from(abs_dx));
            (w, (abs_dy - 1) / w + 1)
        };
        if (function_of_x && end_point.x < start_point.x)
            || (!function_of_x && end_point.y < start_point.y)
        {
            stair_width = stair_width.wrapping_neg();
        }
        let mut result = Vec::with_capacity(2 * stair_count.max(0) as usize + 1);
        result.push(start_point);
        let det = f64::from(dx) * f64::from(dy);
        let change_x_first = (to_the_right && det > 0.0) || (!to_the_right && det < 0.0);
        let mut prev_x = start_point.x;
        let mut prev_y = start_point.y;
        for i in 1..stair_count {
            let (curr_x, curr_y) = if function_of_x {
                let x = start_point.x.wrapping_add(i.wrapping_mul(stair_width));
                (
                    x,
                    round_to_i32(self.get_line().function_value_approx(f64::from(x))),
                )
            } else {
                let y = start_point.y.wrapping_add(i.wrapping_mul(stair_width));
                (
                    round_to_i32(self.get_line().function_in_y_value_approx(f64::from(y))),
                    y,
                )
            };
            if change_x_first {
                result.push(IntPoint::new(curr_x, prev_y));
            } else {
                result.push(IntPoint::new(prev_x, curr_y));
            }
            result.push(IntPoint::new(curr_x, curr_y));
            prev_x = curr_x;
            prev_y = curr_y;
        }
        if change_x_first {
            result.push(IntPoint::new(end_point.x, prev_y));
        } else {
            result.push(IntPoint::new(prev_x, end_point.y));
        }
        result.push(end_point);
        result
    }

    /// Approximates this segment by a stair of 45-degree lines with stairs of width
    /// about `width`. The corners of the stair are returned; empty if the rounded end
    /// points are equal.
    ///
    /// Like Java, the function-of-y case evaluates `function_value_approx` (and not
    /// `function_in_y_value_approx`) at the y coordinate.
    ///
    /// # Panics
    /// If the stair width rounds to 0 (an `ArithmeticException` in Java).
    pub fn stair_approximation_45(&self, width: f64, to_the_right: bool) -> Vec<IntPoint> {
        let start_point = self.start_point().to_float().round();
        let end_point = self.end_point().to_float().round();
        if start_point == end_point {
            return Vec::new();
        }
        let delta = end_point.difference_by_int(start_point);
        if delta.is_multiple_of_45_degree() {
            return vec![start_point, end_point];
        }
        let abs_delta = IntVector::new(delta.x.wrapping_abs(), delta.y.wrapping_abs());
        let function_of_x = abs_delta.x >= abs_delta.y;
        let det = f64::from(delta.x) * f64::from(delta.y);
        let (mut stair_width, stair_count) = if function_of_x {
            let w = round_to_i32((width * f64::from(abs_delta.x)) / f64::from(abs_delta.y));
            (w, (abs_delta.x - 1) / w + 1)
        } else {
            let w = round_to_i32((width * f64::from(abs_delta.y)) / f64::from(abs_delta.x));
            (w, (abs_delta.y - 1) / w + 1)
        };
        if (function_of_x && end_point.x < start_point.x)
            || (!function_of_x && end_point.y < start_point.y)
        {
            stair_width = stair_width.wrapping_neg();
        }
        let sign = Signum::as_int(f64::from(stair_width));
        let mut result = Vec::with_capacity(2 * stair_count.max(0) as usize + 1);
        result.push(start_point);
        let mut prev_line_point = start_point;
        for i in 1..=stair_count {
            let curr_line_point = if i == stair_count {
                end_point
            } else if function_of_x {
                let x = start_point.x.wrapping_add(i.wrapping_mul(stair_width));
                IntPoint::new(
                    x,
                    round_to_i32(self.get_line().function_value_approx(f64::from(x))),
                )
            } else {
                let y = start_point.y.wrapping_add(i.wrapping_mul(stair_width));
                // Java bug kept: function_value_approx instead of function_in_y_value_approx.
                IntPoint::new(
                    round_to_i32(self.get_line().function_value_approx(f64::from(y))),
                    y,
                )
            };
            let (curr_x, curr_y) = if function_of_x {
                let diagonal_first = (to_the_right && det < 0.0) || (!to_the_right && det > 0.0);
                let step = sign.wrapping_mul(
                    curr_line_point
                        .y
                        .wrapping_sub(prev_line_point.y)
                        .wrapping_abs(),
                );
                if diagonal_first {
                    (prev_line_point.x.wrapping_add(step), curr_line_point.y)
                } else {
                    (curr_line_point.x.wrapping_sub(step), prev_line_point.y)
                }
            } else {
                let diagonal_first = (to_the_right && det > 0.0) || (!to_the_right && det < 0.0);
                let step = sign.wrapping_mul(
                    curr_line_point
                        .x
                        .wrapping_sub(prev_line_point.x)
                        .wrapping_abs(),
                );
                if diagonal_first {
                    (curr_line_point.x, prev_line_point.y.wrapping_add(step))
                } else {
                    (prev_line_point.x, curr_line_point.y.wrapping_sub(step))
                }
            };
            result.push(IntPoint::new(curr_x, curr_y));
            result.push(curr_line_point);
            prev_line_point = curr_line_point;
        }
        result
    }

    /// The numbers of the border lines of `shape` crossed by this segment where it
    /// enters or leaves the interior of `shape`, ordered by the distance of the
    /// intersection from the start point. Empty if the segment does not cross the
    /// interior; at most two numbers, because `shape` is convex.
    pub fn border_intersections<T: TileShapeOps + ?Sized>(&self, shape: &T) -> Vec<usize> {
        if !self.bounding_box().intersects_box(&shape.bounding_box()) {
            return Vec::new();
        }
        let edge_count = shape.border_line_count();
        if edge_count == 0 {
            return Vec::new();
        }
        let mut prev_line = shape.border_line(edge_count - 1);
        let mut current_line = shape.border_line(0);
        let mut result: Vec<usize> = Vec::with_capacity(2);
        let mut intersections: Vec<Point> = Vec::with_capacity(2);
        let line_start = self.start_point();
        let line_end = self.end_point();

        for edge_line_no in 0..edge_count {
            let next_line = if edge_line_no == edge_count - 1 {
                shape.border_line(0)
            } else {
                shape.border_line(edge_line_no + 1)
            };
            let start_point_side = current_line.side_of(&line_start);
            let end_point_side = current_line.side_of(&line_end);
            if start_point_side == Side::OnTheLeft && end_point_side == Side::OnTheLeft {
                // The segment is outside the shape.
                return Vec::new();
            }
            if start_point_side == Side::Collinear && end_point_side != Side::OnTheRight {
                return Vec::new();
            }
            if end_point_side == Side::Collinear && start_point_side != Side::OnTheRight {
                return Vec::new();
            }
            if start_point_side != Side::OnTheRight || end_point_side != Side::OnTheRight {
                // The segment crosses the current border line.
                let is = self.middle.intersection(&current_line);
                let prev_line_side_of_is = prev_line.side_of(&is);
                let next_line_side_of_is = next_line.side_of(&is);
                if prev_line_side_of_is != Side::OnTheLeft
                    && next_line_side_of_is != Side::OnTheLeft
                {
                    // The intersection is on the border of the shape.
                    if prev_line_side_of_is == Side::Collinear {
                        // The segment passes through the corner at the start of the
                        // current border line.
                        let prev_prev_corner = if edge_line_no == 0 {
                            shape.corner(edge_count - 1)
                        } else {
                            shape.corner(edge_line_no - 1)
                        };
                        let next_corner = if edge_line_no == edge_count - 1 {
                            shape.corner(0)
                        } else {
                            shape.corner(edge_line_no + 1)
                        };
                        let prev_prev_corner_side = self.middle.side_of(&prev_prev_corner);
                        let next_corner_side = self.middle.side_of(&next_corner);
                        if prev_prev_corner_side == Side::Collinear
                            || next_corner_side == Side::Collinear
                            || prev_prev_corner_side == next_corner_side
                        {
                            return Vec::new();
                        }
                    }
                    if next_line_side_of_is == Side::Collinear {
                        // The segment passes through the corner at the end of the
                        // current border line.
                        let prev_corner = shape.corner(edge_line_no);
                        let next_next_corner = if edge_count >= 2 && edge_line_no == edge_count - 2
                        {
                            shape.corner(0)
                        } else if edge_line_no == edge_count - 1 {
                            shape.corner(1)
                        } else {
                            shape.corner(edge_line_no + 2)
                        };
                        let prev_corner_side = self.middle.side_of(&prev_corner);
                        let next_next_corner_side = self.middle.side_of(&next_next_corner);
                        if prev_corner_side == Side::Collinear
                            || next_next_corner_side == Side::Collinear
                            || prev_corner_side == next_next_corner_side
                        {
                            return Vec::new();
                        }
                    }
                    let already_handled = intersections.contains(&is);
                    // Java logs a warning if more than two intersections are found.
                    if !already_handled && intersections.len() < 2 {
                        result.push(edge_line_no);
                        intersections.push(is);
                    }
                }
            }
            prev_line = current_line;
            current_line = next_line;
        }
        if result.len() == 2 {
            let is0 = intersections[0].to_float();
            let is1 = intersections[1].to_float();
            let current_start = line_start.to_float();
            if current_start.distance_square(is1) < current_start.distance_square(is0) {
                result.swap(0, 1);
            }
        }
        result
    }

    /// This segment, or its reverse if the start point is greater than the end point in
    /// the lexicographic xy order.
    pub fn sort_endpoints_in_xy(&self) -> LineSegment {
        if self.start_point().compare_xy(&self.end_point()) > 0 {
            LineSegment {
                start: self.end,
                middle: self.middle,
                end: self.start,
                start_point: self.end_point.clone(),
                end_point: self.start_point.clone(),
            }
        } else {
            self.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shapes::TileShape;

    fn seg(x0: i32, y0: i32, x1: i32, y1: i32) -> LineSegment {
        let polyline = Polyline::from_corners(&ip(x0, y0), &ip(x1, y1));
        LineSegment::from_polyline(&polyline, 1)
    }

    fn ip(x: i32, y: i32) -> Point {
        Point::from(IntPoint::new(x, y))
    }

    #[test]
    fn end_points_and_contains() {
        let s = seg(0, 0, 10, 5);
        assert_eq!(s.start_point(), ip(0, 0));
        assert_eq!(s.end_point(), ip(10, 5));
        assert!(s.contains(&ip(2, 1)));
        assert!(s.contains(&ip(10, 5)));
        assert!(!s.contains(&ip(12, 6)));
        assert!(!s.contains(&ip(2, 2)));
        assert_eq!(s.bounding_box(), IntBox::new(0, 0, 10, 5));
        let o = s.opposite();
        assert_eq!(o.start_point(), ip(10, 5));
        assert_eq!(o.end_point(), ip(0, 0));
    }

    #[test]
    fn intersection_cases() {
        let a = seg(0, 0, 10, 10);
        let b = seg(0, 10, 10, 0);
        assert_eq!(a.intersection(&b).len(), 1);
        assert!(a.intersects(&b));
        assert!(!a.overlaps(&b));
        let c = seg(5, 5, 20, 20);
        assert_eq!(a.intersection(&c).len(), 2);
        assert!(a.overlaps(&c));
        let d = seg(10, 10, 20, 20);
        assert_eq!(a.intersection(&d).len(), 1);
        let e = seg(11, 11, 20, 20);
        assert!(!a.intersects(&e));
        let f = seg(20, 0, 30, 10);
        assert!(!a.intersects(&f));
    }

    #[test]
    fn sort_endpoints_swaps_cached_points() {
        let s = seg(10, 0, 0, 0);
        let _ = s.start_point();
        let sorted = s.sort_endpoints_in_xy();
        assert_eq!(sorted.start_point(), ip(0, 0));
        assert_eq!(sorted.end_point(), ip(10, 0));
    }

    #[test]
    fn to_simplex_is_the_segment() {
        let s = seg(0, 0, 10, 5).to_simplex();
        assert_eq!(s.dimension(), 1);
        assert!(s.contains(&ip(2, 1)));
        assert!(!s.contains(&ip(12, 6)));
    }

    #[test]
    fn border_intersections_with_box() {
        let b = TileShape::IntBox(IntBox::new(0, 0, 10, 10));
        // Crosses the box from left to right: enters through the left border line
        // (index 3), leaves through the right one (index 1).
        let s = seg(-5, 5, 15, 5);
        assert_eq!(s.border_intersections(&b), vec![3, 1]);
        assert_eq!(s.opposite().border_intersections(&b), vec![1, 3]);
        // Starts inside.
        assert_eq!(seg(5, 5, 15, 5).border_intersections(&b), vec![1]);
        // Completely inside or outside.
        assert!(seg(2, 2, 8, 8).border_intersections(&b).is_empty());
        assert!(seg(20, 0, 30, 10).border_intersections(&b).is_empty());
        // Through a corner only.
        assert!(seg(-5, 15, 15, -5).border_intersections(&b).len() == 2);
        assert!(seg(-5, 5, 5, 15).border_intersections(&b).is_empty());
    }

    #[test]
    fn stair_approximation_corners() {
        let s = seg(0, 0, 10, 4);
        let stairs = s.stair_approximation(2.0, true);
        assert_eq!(stairs.first(), Some(&IntPoint::new(0, 0)));
        assert_eq!(stairs.last(), Some(&IntPoint::new(10, 4)));
        for w in stairs.windows(2) {
            assert!(w[0].x == w[1].x || w[0].y == w[1].y);
        }
        let s45 = s.stair_approximation_45(2.0, false);
        assert_eq!(s45.first(), Some(&IntPoint::new(0, 0)));
        assert_eq!(s45.last(), Some(&IntPoint::new(10, 4)));
        for w in s45.windows(2) {
            assert!(w[1].difference_by_int(w[0]).is_multiple_of_45_degree());
        }
        assert_eq!(seg(0, 0, 10, 0).stair_approximation(2.0, true).len(), 2);
    }

    #[test]
    fn change_length_approx_keeps_start() {
        let s = seg(0, 0, 100, 0).change_length_approx(30.0);
        assert_eq!(s.start_point(), ip(0, 0));
        assert_eq!(s.end_point(), ip(30, 0));
    }
}
