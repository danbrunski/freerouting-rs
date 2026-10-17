//! Convex shapes whose borders consist of straight lines.

use crate::direction::Direction;
use crate::float_line::FloatLine;
use crate::float_point::{FloatPoint, java_min};
use crate::line::Line;
use crate::line_segment::LineSegment;
use crate::point::{IntPoint, Point};
use crate::polygon::Polygon;
use crate::polyline::Polyline;
use crate::side::Side;
use crate::vector::Vector;

use super::{
    IntBox, IntOctagon, PolylineShapeOps, RegularTileShape, Shape, ShapeBoundingDirections, Simplex,
};

/// Functionality of convex shapes whose borders consist of straight lines.
///
/// Port of the abstract class `app.freerouting.geometry.planar.TileShape`. The provided
/// methods are the concrete methods of the Java class; implementors override some of
/// them like the Java subclasses do. Methods that return shapes are inherent methods
/// of the [`TileShape`] enum.
pub trait TileShapeOps: PolylineShapeOps {
    /// A stable id for deterministic tie-breaking.
    fn get_id(&self) -> i32;

    /// True if this shape is an `IntBox` or can be converted into one.
    fn is_int_box(&self) -> bool;

    /// True if this shape is an `IntOctagon` or can be converted into one.
    fn is_int_octagon(&self) -> bool;

    /// The number of the border line equal to `line`, if any (not implemented for
    /// boxes and octagons, which always return `None` as in Java).
    fn border_line_index(&self, line: &Line) -> Option<usize>;

    /// The shape as a `Simplex`.
    fn to_simplex(&self) -> Simplex;

    /// The shape as a [`TileShape`] enum value (cheap: boxes and octagons are `Copy`,
    /// a simplex is reference counted).
    fn to_tile_shape(&self) -> TileShape;

    /// The maximum diameter of the shape.
    fn max_width(&self) -> f64;

    /// The minimum diameter of the shape.
    fn min_width(&self) -> f64;

    /// Content of the area; `f64::MAX` if unbounded.
    fn area(&self) -> f64 {
        if !self.is_bounded() {
            return f64::MAX;
        }
        if self.dimension() < 2 {
            return 0.0;
        }
        let corner_count = self.border_line_count();
        let mut result = 0.0;
        let mut prev_corner = self.corner_approx(corner_count - 2);
        let mut current_corner = self.corner_approx(corner_count - 1);
        for i in 0..corner_count {
            let next_corner = self.corner_approx(i);
            result += current_corner.x * (next_corner.y - prev_corner.y);
            prev_corner = current_corner;
            current_corner = next_corner;
        }
        0.5 * result.abs()
    }

    /// True if `point` is neither inside nor on the border of the shape.
    fn is_outside(&self, point: &Point) -> bool {
        let line_count = self.border_line_count();
        if line_count == 0 {
            return true;
        }
        (0..line_count).any(|i| self.border_line(i).side_of(point) == Side::OnTheLeft)
    }

    /// True if `point` is inside or on the border of the shape.
    fn contains(&self, point: &Point) -> bool {
        !self.is_outside(point)
    }

    /// True if `point` is contained in this shape.
    fn contains_float(&self, point: FloatPoint) -> bool {
        self.contains_float_tol(point, 0.0)
    }

    /// True if `point` is contained in this shape with `tolerance` (used in the
    /// determinant of the side test, not a distance).
    fn contains_float_tol(&self, point: FloatPoint, tolerance: f64) -> bool {
        let line_count = self.border_line_count();
        if line_count == 0 {
            return false;
        }
        (0..line_count)
            .all(|i| self.border_line(i).side_of_float_tol(point, tolerance) == Side::OnTheRight)
    }

    /// True if this shape contains all corners of `other` (Java `contains(TileShape)`).
    fn contains_shape<T: TileShapeOps + ?Sized>(&self, other: &T) -> bool
    where
        Self: Sized,
    {
        (0..other.border_line_count()).all(|i| self.contains(&other.corner(i)))
    }

    /// True if `point` is inside the shape but not on a border line.
    fn contains_inside(&self, point: &Point) -> bool {
        let line_count = self.border_line_count();
        if line_count == 0 {
            return false;
        }
        (0..line_count).all(|i| self.border_line(i).side_of(point) == Side::OnTheRight)
    }

    /// `Collinear` if `point` is on the border with `tolerance`, `OnTheLeft` if it is
    /// outside and `OnTheRight` if it is inside.
    fn side_of_border(&self, point: FloatPoint, tolerance: f64) -> Side {
        let line_count = self.border_line_count();
        if line_count == 0 {
            return Side::Collinear;
        }
        let mut result = Side::OnTheRight;
        for i in 0..line_count {
            let current_side = self.border_line(i).side_of_float_tol(point, tolerance);
            if current_side == Side::OnTheLeft {
                return Side::OnTheLeft;
            } else if current_side == Side::Collinear {
                result = current_side;
            }
        }
        result
    }

    /// The number of the border line containing `point`, if `point` lies on the border.
    fn contains_on_border_line_no(&self, point: &Point) -> Option<usize> {
        let line_count = self.border_line_count();
        let mut containing_line_no = None;
        for i in 0..line_count {
            let side = self.border_line(i).side_of(point);
            if side == Side::OnTheLeft {
                return None;
            }
            if side == Side::Collinear {
                containing_line_no = Some(i);
            }
        }
        containing_line_no
    }

    /// True if `point` lies exactly on the border.
    fn contains_on_border(&self, point: &Point) -> bool {
        self.contains_on_border_line_no(point).is_some()
    }

    /// True if this shape contains `other` (with some numerical inaccuracy).
    fn contains_approx<T: TileShapeOps + ?Sized>(&self, other: &T) -> bool
    where
        Self: Sized,
    {
        other
            .corner_approx_arr()
            .into_iter()
            .all(|c| self.contains_float(c))
    }

    /// Distance from `point` to the shape; 0 if contained.
    ///
    /// For an empty shape Java fails with a `NullPointerException`; this port returns
    /// `f64::INFINITY`.
    fn distance(&self, point: FloatPoint) -> f64 {
        match self.nearest_point_approx(point) {
            Some(nearest) => nearest.distance(point),
            None => f64::INFINITY,
        }
    }

    /// Distance from `point` to the border; `f64::INFINITY` for an empty shape.
    fn border_distance(&self, point: FloatPoint) -> f64 {
        match self.nearest_border_point_approx(point) {
            Some(nearest) => nearest.distance(point),
            None => f64::INFINITY,
        }
    }

    /// Smallest distance from the centre of gravity to the border.
    fn smallest_radius(&self) -> f64 {
        self.border_distance(self.centre_of_gravity())
    }

    /// The point of this shape nearest to `from_point`; `from_point` itself if it is
    /// contained. `None` for an empty shape.
    fn nearest_point(&self, from_point: &Point) -> Option<Point> {
        if !self.is_outside(from_point) {
            return Some(from_point.clone());
        }
        self.nearest_border_point(from_point)
    }

    /// Approximation of the nearest point of the shape to `from_point`.
    fn nearest_point_approx(&self, from_point: FloatPoint) -> Option<FloatPoint> {
        if self.contains_float(from_point) {
            return Some(from_point);
        }
        self.nearest_border_point_approx(from_point)
    }

    /// The nearest point to `from_point` on the border; `None` for an empty shape.
    fn nearest_border_point(&self, from_point: &Point) -> Option<Point> {
        let line_count = self.border_line_count();
        if line_count == 0 {
            return None;
        }
        let from_point_f = from_point.to_float();
        if line_count == 1 {
            return Some(self.border_line(0).perpendicular_projection(from_point));
        }
        let mut min_dist = f64::MAX;
        let mut min_dist_ind = 0;
        for i in 0..line_count {
            let current_distance = self.corner_approx(i).distance_square(from_point_f);
            if current_distance < min_dist {
                min_dist = current_distance;
                min_dist_ind = i;
            }
        }
        let mut nearest_point = self.corner(min_dist_ind);
        let mut prev_ind = line_count - 2;
        let mut current_ind = line_count - 1;
        for next_ind in 0..line_count {
            let projection = self
                .border_line(current_ind)
                .perpendicular_projection(from_point);
            if (!self.corner_is_bounded(current_ind)
                || self.border_line(prev_ind).side_of(&projection) == Side::OnTheRight)
                && (!self.corner_is_bounded(next_ind)
                    || self.border_line(next_ind).side_of(&projection) == Side::OnTheRight)
            {
                let current_distance = projection.to_float().distance_square(from_point_f);
                if current_distance < min_dist {
                    min_dist = current_distance;
                    nearest_point = projection;
                }
            }
            prev_ind = current_ind;
            current_ind = next_ind;
        }
        Some(nearest_point)
    }

    /// Approximation of the nearest point to `from_point` on the border.
    fn nearest_border_point_approx(&self, from_point: FloatPoint) -> Option<FloatPoint> {
        self.nearest_border_points_approx(from_point, 1)
            .into_iter()
            .next()
    }

    /// Approximations of the `count` nearest points to `from_point` on different border
    /// lines, nearest first.
    ///
    /// Where Java leaves trailing `null` entries (fewer candidates than requested), the
    /// result is shorter instead.
    fn nearest_border_points_approx(
        &self,
        from_point: FloatPoint,
        count: usize,
    ) -> Vec<FloatPoint> {
        if count == 0 {
            return Vec::new();
        }
        let line_count = self.border_line_count();
        if line_count == 0 {
            return Vec::new();
        }
        if line_count == 1 {
            return vec![from_point.projection_approx(&self.border_line(0))];
        }
        if self.dimension() == 0 {
            return vec![self.corner_approx(0)];
        }
        let result_count = count.min(line_count);
        let mut nearest_points: Vec<Option<FloatPoint>> = vec![None; result_count];
        let mut min_dists = vec![f64::MAX; result_count];
        let mut insert = |current_distance: f64, point: FloatPoint| {
            for j in 0..result_count {
                if current_distance < min_dists[j] {
                    for k in j + 1..result_count {
                        min_dists[k] = min_dists[k - 1];
                        nearest_points[k] = nearest_points[k - 1];
                    }
                    min_dists[j] = current_distance;
                    nearest_points[j] = Some(point);
                    break;
                }
            }
        };
        for i in 0..line_count {
            if self.corner_is_bounded(i) {
                let current_corner = self.corner_approx(i);
                insert(current_corner.distance_square(from_point), current_corner);
            }
        }
        let mut prev_ind = line_count - 2;
        let mut current_ind = line_count - 1;
        for next_ind in 0..line_count {
            let projection = from_point.projection_approx(&self.border_line(current_ind));
            if (!self.corner_is_bounded(current_ind)
                || self.border_line(prev_ind).side_of_float(projection) == Side::OnTheRight)
                && (!self.corner_is_bounded(next_ind)
                    || self.border_line(next_ind).side_of_float(projection) == Side::OnTheRight)
            {
                insert(projection.distance_square(from_point), projection);
            }
            prev_ind = current_ind;
            current_ind = next_ind;
        }
        nearest_points.into_iter().map_while(|p| p).collect()
    }

    /// The number of the corner nearest to `from_point`.
    ///
    /// Faithful to a Java bug: the minimum is initialised with `Double.MIN_VALUE` (the
    /// smallest positive double), so the result is the first corner at distance exactly
    /// 0, and 0 if there is none.
    fn index_of_nearest_corner(&self, from_point: &Point) -> usize {
        let from_point_f = from_point.to_float();
        let mut result = 0;
        let mut min_dist = f64::from_bits(1);
        for i in 0..self.border_line_count() {
            let current_distance = self.corner_approx(i).distance(from_point_f);
            if current_distance < min_dist {
                min_dist = current_distance;
                result = i;
            }
        }
        result
    }

    /// A segment from the corner 0 to the corner `border_line_count() / 2`; `None` for
    /// an empty shape.
    fn diagonal_corner_segment(&self) -> Option<FloatLine> {
        if self.is_empty() {
            return None;
        }
        let first_corner = self.corner_approx(0);
        let last_corner = self.corner_approx(self.border_line_count() / 2);
        Some(FloatLine::new(first_corner, last_corner))
    }

    /// The maximum of the edge widths of the shape; `i32::MAX` if unbounded.
    fn length(&self) -> f64 {
        if !self.is_bounded() {
            return f64::from(i32::MAX);
        }
        let dimension = self.dimension();
        if dimension <= 0 {
            return 0.0;
        }
        if dimension == 1 {
            return self.circumference() / 2.0;
        }
        let mut max_distance = -1.0;
        let mut max_distance_2 = -1.0;
        let gravity_point = self.centre_of_gravity();
        for i in 0..self.border_line_count() {
            let current_distance = self.border_line(i).signed_distance(gravity_point).abs();
            if current_distance > max_distance {
                max_distance_2 = max_distance;
                max_distance = current_distance;
            } else if current_distance > max_distance_2 {
                max_distance_2 = current_distance;
            }
        }
        max_distance + max_distance_2
    }

    /// The indices of a pair of touching border lines of this shape and `other`, if
    /// they have a common border piece.
    fn touching_sides<T: TileShapeOps + ?Sized>(&self, other: &T) -> Option<(usize, usize)>
    where
        Self: Sized,
    {
        let mut side_no_2 = 0;
        let mut dir2 = None;
        for i in 0..other.border_line_count() {
            let current_direction = other.border_line(i).direction();
            if current_direction.compare_to(&Direction::LEFT) >= 0 {
                side_no_2 = i;
                dir2 = Some(current_direction.opposite());
                break;
            }
        }
        let mut dir2 = dir2?;
        let mut side_no_1 = 0;
        let mut dir1 = self.border_line(0).direction();
        let max_ind = self.border_line_count() + other.border_line_count();
        for _ in 0..max_ind {
            let compare = dir2.compare_to(&dir1);
            if compare == 0
                && self
                    .border_line(side_no_1)
                    .is_equal_or_opposite(&other.border_line(side_no_2))
            {
                return Some((side_no_1, side_no_2));
            }
            if compare >= 0 {
                side_no_1 = (side_no_1 + 1) % self.border_line_count();
                dir1 = self.border_line(side_no_1).direction();
            } else {
                side_no_2 = (side_no_2 + 1) % other.border_line_count();
                dir2 = other.border_line(side_no_2).direction().opposite();
            }
        }
        None
    }

    /// The minimal distance of `line` to this shape, assuming the line is on the left
    /// of the shape; -1 if it is on the right or intersects the interior.
    fn distance_to_the_left(&self, line: &Line) -> f64 {
        let mut result = f64::from(i32::MAX);
        for i in 0..self.border_line_count() {
            let current_corner = self.corner_approx(i);
            let mut line_side = line.side_of_float_tol(current_corner, 1.0);
            if line_side == Side::Collinear {
                line_side = line.side_of(&self.corner(i));
            }
            if line_side == Side::OnTheRight {
                result = -1.0;
                break;
            }
            result = java_min(result, line.signed_distance(current_corner));
        }
        result
    }

    /// `Collinear` if `line` intersects the interior of this shape, `OnTheLeft` if the
    /// shape is completely on the left of `line`, `OnTheRight` otherwise.
    fn side_of_line(&self, line: &Line) -> Side {
        let mut on_the_left = false;
        let mut on_the_right = false;
        for i in 0..self.border_line_count() {
            let current_side = line.side_of(&self.corner(i));
            if current_side == Side::OnTheLeft {
                on_the_right = true;
            } else if current_side == Side::OnTheRight {
                on_the_left = true;
            }
            if on_the_left && on_the_right {
                return Side::Collinear;
            }
        }
        if on_the_left {
            Side::OnTheLeft
        } else {
            Side::OnTheRight
        }
    }

    /// The border line hit by the ray from `point` into `direction`; `None` if `point`
    /// is not contained in this shape.
    ///
    /// # Panics
    /// If `point` is rational or `direction` is a `BigIntDirection` (Java only
    /// implements lines through `IntPoint`s).
    fn intersecting_border_line_no(&self, point: &Point, direction: &Direction) -> Option<usize> {
        if !self.contains(point) {
            return None;
        }
        let from_point = point.to_float();
        let intersection_line =
            Line::from_point_and_direction(point.expect_int(), direction.expect_int());
        let second_line_point = intersection_line.b().to_float();
        let mut result = None;
        let mut min_distance = f64::from(f32::MAX);
        for i in 0..self.border_line_count() {
            let current_border_line = self.border_line(i);
            let current_intersection = current_border_line.intersection_approx(&intersection_line);
            if current_intersection.x >= f64::from(i32::MAX) {
                continue;
            }
            let current_distance = current_intersection.distance_square(from_point);
            if current_distance < min_distance {
                let direction_ok = current_border_line.side_of_float(second_line_point)
                    == Side::OnTheLeft
                    || second_line_point.distance_square(current_intersection) < current_distance;
                if direction_ok {
                    result = Some(i);
                    min_distance = current_distance;
                }
            }
        }
        result
    }

    /// Cuts out the parts of `polyline` in the interior of this shape and returns the
    /// remaining pieces. Pieces completely contained in the border are dropped.
    fn cutout_polyline(&self, polyline: &Polyline) -> Vec<Polyline> {
        let intersection_no = self.entrance_points(polyline);
        let first_corner = polyline.first_corner();
        let first_corner_is_inside = self.contains_inside(&first_corner);
        if intersection_no.is_empty() {
            if first_corner_is_inside {
                return Vec::new();
            }
            return vec![polyline.clone()];
        }
        let lines = polyline.lines();
        let mut pieces = Vec::new();
        let mut current_intersection_no = 0;
        let current_tuple = intersection_no[current_intersection_no];
        let first_intersection =
            lines[current_tuple.0].intersection(&self.border_line(current_tuple.1));
        if !first_corner_is_inside {
            if first_corner != first_intersection {
                let current_polyline_intersection_no = current_tuple.0;
                let mut current_lines: Vec<Line> =
                    lines[..=current_polyline_intersection_no].to_vec();
                current_lines.push(self.border_line(current_tuple.1));
                let current_piece = Polyline::new(&current_lines);
                if !current_piece.is_empty() {
                    pieces.push(current_piece);
                }
            }
            current_intersection_no += 1;
        }
        while current_intersection_no + 1 < intersection_no.len() {
            let current_tuple = intersection_no[current_intersection_no];
            let next_tuple = intersection_no[current_intersection_no + 1];
            let current_no_of_polyline = current_tuple.0;
            let next_no_of_polyline = next_tuple.0;
            let insert_piece = (current_no_of_polyline + 1..next_no_of_polyline)
                .any(|i| self.is_outside(&polyline.corner(i)));
            if insert_piece {
                let len = next_no_of_polyline - current_no_of_polyline + 3;
                let mut current_lines = Vec::with_capacity(len);
                current_lines.push(self.border_line(current_tuple.1));
                current_lines.extend_from_slice(
                    &lines[current_no_of_polyline..current_no_of_polyline + len - 2],
                );
                current_lines.push(self.border_line(next_tuple.1));
                let current_piece = Polyline::new(&current_lines);
                if !current_piece.is_empty() {
                    pieces.push(current_piece);
                }
            }
            current_intersection_no += 2;
        }
        if current_intersection_no < intersection_no.len() {
            let current_tuple = intersection_no[current_intersection_no];
            let current_polyline_intersection_no = current_tuple.0;
            let mut current_lines =
                Vec::with_capacity(lines.len() - current_polyline_intersection_no + 1);
            current_lines.push(self.border_line(current_tuple.1));
            current_lines.extend_from_slice(&lines[current_polyline_intersection_no..]);
            let current_piece = Polyline::new(&current_lines);
            if !current_piece.is_empty() {
                pieces.push(current_piece);
            }
        }
        pieces
    }

    /// The points where `polyline` enters or leaves the interior of this shape, as
    /// tuples (number of the polyline line, number of the crossed border line).
    fn entrance_points(&self, polyline: &Polyline) -> Vec<(usize, usize)> {
        let line_count = polyline.lines().len();
        let mut result = Vec::new();
        let mut prev: Option<(usize, usize)> = None;
        for line_index in 1..line_count.saturating_sub(1) {
            let current_line_seg = LineSegment::from_polyline(polyline, line_index);
            for edge_index in current_line_seg.border_intersections(self) {
                if prev != Some((line_index, edge_index)) {
                    result.push((line_index, edge_index));
                    prev = Some((line_index, edge_index));
                }
            }
        }
        result
    }

    /// True if `line_segment` has a common point with the interior of this shape.
    fn is_intersected_interior_by_segment(&self, line_segment: &LineSegment) -> bool {
        self.is_intersected_interior_by(
            &line_segment.start_point(),
            &line_segment.end_point(),
            &line_segment.get_line(),
        )
    }

    /// True if the segment from `start_point` to `end_point` on `line` has a common
    /// point with the interior of this shape.
    fn is_intersected_interior_by(
        &self,
        start_point: &Point,
        end_point: &Point,
        line: &Line,
    ) -> bool {
        let float_start_point = start_point.to_float();
        let float_end_point = end_point.to_float();
        let n = self.border_line_count();
        let mut start_sides = Vec::with_capacity(n);
        let mut end_sides = Vec::with_capacity(n);
        for i in 0..n {
            let current_border_line = self.border_line(i);
            let mut start_side = current_border_line.side_of_float_tol(float_start_point, 1.0);
            if start_side == Side::Collinear {
                start_side = current_border_line.side_of(start_point);
            }
            let mut end_side = current_border_line.side_of_float_tol(float_end_point, 1.0);
            if end_side == Side::Collinear {
                end_side = current_border_line.side_of(end_point);
            }
            if start_side != Side::OnTheRight && end_side != Side::OnTheRight {
                return false;
            }
            start_sides.push(start_side);
            end_sides.push(end_side);
        }
        if start_sides.iter().all(|&s| s == Side::OnTheRight) {
            return true;
        }
        if end_sides.iter().all(|&s| s == Side::OnTheRight) {
            return true;
        }
        for i in 0..n {
            let start_side = start_sides[i];
            let end_side = end_sides[i];
            if start_side != end_side {
                if (start_side == Side::Collinear && end_side == Side::OnTheLeft)
                    || (end_side == Side::Collinear && start_side == Side::OnTheLeft)
                {
                    continue;
                }
                let mut prev_corner_side = line.side_of_float_tol(self.corner_approx(i), 1.0);
                if prev_corner_side == Side::Collinear {
                    prev_corner_side = line.side_of(&self.corner(i));
                }
                let next_corner_index = if i == n - 1 { 0 } else { i + 1 };
                let mut next_corner_side =
                    line.side_of_float_tol(self.corner_approx(next_corner_index), 1.0);
                if next_corner_side == Side::Collinear {
                    next_corner_side = line.side_of(&self.corner(next_corner_index));
                }
                if (prev_corner_side == Side::OnTheLeft && next_corner_side == Side::OnTheRight)
                    || (prev_corner_side == Side::OnTheRight && next_corner_side == Side::OnTheLeft)
                {
                    return true;
                }
            }
        }
        false
    }
}

/// A convex shape whose border consists of straight lines.
///
/// Port of the sealed Java class `TileShape` and its subclasses `IntBox`, `IntOctagon`
/// (via `RegularTileShape`) and `Simplex`.
#[derive(Debug, Clone)]
pub enum TileShape {
    IntBox(IntBox),
    IntOctagon(IntOctagon),
    Simplex(Simplex),
}

impl From<IntBox> for TileShape {
    fn from(b: IntBox) -> Self {
        TileShape::IntBox(b)
    }
}

impl From<IntOctagon> for TileShape {
    fn from(o: IntOctagon) -> Self {
        TileShape::IntOctagon(o)
    }
}

impl From<Simplex> for TileShape {
    fn from(s: Simplex) -> Self {
        TileShape::Simplex(s)
    }
}

impl From<RegularTileShape> for TileShape {
    fn from(r: RegularTileShape) -> Self {
        match r {
            RegularTileShape::IntBox(b) => TileShape::IntBox(b),
            RegularTileShape::IntOctagon(o) => TileShape::IntOctagon(o),
        }
    }
}

macro_rules! dispatch {
    ($self:ident, $s:ident => $e:expr) => {
        match $self {
            TileShape::IntBox($s) => $e,
            TileShape::IntOctagon($s) => $e,
            TileShape::Simplex($s) => $e,
        }
    };
}

impl PolylineShapeOps for TileShape {
    fn border_line_count(&self) -> usize {
        dispatch!(self, s => s.border_line_count())
    }
    fn corner(&self, no: usize) -> Point {
        dispatch!(self, s => s.corner(no))
    }
    fn border_line(&self, no: usize) -> Line {
        dispatch!(self, s => s.border_line(no))
    }
    fn corner_is_bounded(&self, no: usize) -> bool {
        dispatch!(self, s => s.corner_is_bounded(no))
    }
    fn is_empty(&self) -> bool {
        dispatch!(self, s => s.is_empty())
    }
    fn is_bounded(&self) -> bool {
        dispatch!(self, s => s.is_bounded())
    }
    fn dimension(&self) -> i32 {
        dispatch!(self, s => s.dimension())
    }
    fn bounding_box(&self) -> IntBox {
        dispatch!(self, s => s.bounding_box())
    }
    fn bounding_octagon(&self) -> Option<IntOctagon> {
        dispatch!(self, s => s.bounding_octagon())
    }
    fn corner_approx(&self, no: usize) -> FloatPoint {
        dispatch!(self, s => s.corner_approx(no))
    }
    fn corner_approx_arr(&self) -> Vec<FloatPoint> {
        dispatch!(self, s => s.corner_approx_arr())
    }
    fn circumference(&self) -> f64 {
        dispatch!(self, s => s.circumference())
    }
    fn is_contained_in(&self, b: &IntBox) -> bool {
        dispatch!(self, s => s.is_contained_in(b))
    }
}

impl TileShapeOps for TileShape {
    fn get_id(&self) -> i32 {
        dispatch!(self, s => s.get_id())
    }
    fn is_int_box(&self) -> bool {
        dispatch!(self, s => s.is_int_box())
    }
    fn is_int_octagon(&self) -> bool {
        dispatch!(self, s => s.is_int_octagon())
    }
    fn border_line_index(&self, line: &Line) -> Option<usize> {
        dispatch!(self, s => s.border_line_index(line))
    }
    fn to_simplex(&self) -> Simplex {
        dispatch!(self, s => s.to_simplex())
    }
    fn to_tile_shape(&self) -> TileShape {
        self.clone()
    }
    fn max_width(&self) -> f64 {
        dispatch!(self, s => s.max_width())
    }
    fn min_width(&self) -> f64 {
        dispatch!(self, s => s.min_width())
    }
    fn area(&self) -> f64 {
        dispatch!(self, s => s.area())
    }
    fn contains_float(&self, point: FloatPoint) -> bool {
        dispatch!(self, s => s.contains_float(point))
    }
    fn distance(&self, point: FloatPoint) -> f64 {
        dispatch!(self, s => s.distance(point))
    }
}

impl TileShape {
    /// The intersection of the half-planes on the left of the directed `lines`,
    /// simplified (Java `TileShape.getInstance(Line[])`).
    pub fn get_instance_from_lines(lines: &[Line]) -> TileShape {
        Simplex::get_instance(lines).simplify()
    }

    /// The convex shape with the given corners (Java `TileShape.getInstance(Point[])`).
    ///
    /// # Panics
    /// If a corner is rational or `convex_polygon` is empty.
    pub fn get_instance_from_points(convex_polygon: &[Point]) -> TileShape {
        let n = convex_polygon.len();
        let mut lines = Vec::with_capacity(n);
        for j in 0..n - 1 {
            lines.push(Line::from_points(
                &convex_polygon[j],
                &convex_polygon[j + 1],
            ));
        }
        lines.push(Line::from_points(
            &convex_polygon[n - 1],
            &convex_polygon[0],
        ));
        TileShape::get_instance_from_lines(&lines)
    }

    /// The half-plane on the left of `line`.
    pub fn get_instance_half_plane(line: Line) -> TileShape {
        TileShape::Simplex(Simplex::get_instance(&[line]))
    }

    /// A normalized `IntOctagon` from its eight boundary values.
    #[allow(clippy::too_many_arguments)]
    pub fn get_instance_octagon(
        lx: i32,
        ly: i32,
        rx: i32,
        uy: i32,
        ulx: i32,
        lrx: i32,
        llx: i32,
        urx: i32,
    ) -> IntOctagon {
        IntOctagon::new(lx, ly, rx, uy, ulx, lrx, llx, urx).normalize()
    }

    /// A box-like octagon.
    pub fn get_instance_box(
        lower_left_x: i32,
        lower_left_y: i32,
        upper_right_x: i32,
        upper_right_y: i32,
    ) -> IntOctagon {
        IntBox::new(lower_left_x, lower_left_y, upper_right_x, upper_right_y).to_int_octagon()
    }

    /// The smallest box containing `point`.
    pub fn get_instance_point(point: &Point) -> IntBox {
        point.surrounding_box()
    }

    /// The physical shape converted to a simpler one, if possible.
    pub fn simplify(&self) -> TileShape {
        match self {
            TileShape::IntBox(_) => self.clone(),
            TileShape::IntOctagon(o) => o.simplify(),
            TileShape::Simplex(s) => s.simplify(),
        }
    }

    /// The intersection of this shape with `other`, simplified.
    pub fn intersection_with_simplify(&self, other: &TileShape) -> TileShape {
        self.intersection(other).simplify()
    }

    /// The intersection of this shape with `other`.
    ///
    /// Follows the Java double dispatch, which determines the order of the lines of a
    /// resulting simplex.
    pub fn intersection(&self, other: &TileShape) -> TileShape {
        use TileShape::{IntBox as B, IntOctagon as O, Simplex as S};
        match (self, other) {
            (B(a), B(b)) => B(b.intersection_box(a)),
            (B(a), O(b)) => O(b.intersection_box(a)),
            (B(a), S(b)) => S(b.intersection_box(a)),
            (O(a), B(b)) => O(a.intersection_octagon(&b.to_int_octagon())),
            (O(a), O(b)) => O(b.intersection_octagon(a)),
            (O(a), S(b)) => S(b.intersection_octagon(a)),
            (S(a), B(b)) => S(a.intersection_simplex(&b.to_simplex())),
            (S(a), O(b)) => S(a.intersection_octagon(b)),
            (S(a), S(b)) => S(b.intersection_simplex(a)),
        }
    }

    /// True if this shape and `other` have a nonempty intersection.
    pub fn intersects(&self, other: &Shape) -> bool {
        Shape::Polyline(self.clone().into()).intersects(other)
    }

    /// True if this shape and `other` have a nonempty intersection.
    ///
    /// The dispatch of [`intersects`](Self::intersects) on two tile shapes
    /// (`Shape::intersects` picks `other`'s overload by this shape's kind),
    /// without cloning either shape into a [`Shape`] (P02.2).
    pub fn intersects_tile(&self, other: &TileShape) -> bool {
        match self {
            TileShape::IntBox(b) => other.intersects_box(b),
            TileShape::IntOctagon(o) => other.intersects_octagon(o),
            TileShape::Simplex(s) => other.intersects_simplex(s),
        }
    }

    /// The shape translated by `vector`.
    pub fn translate_by(&self, vector: &Vector) -> TileShape {
        match self {
            TileShape::IntBox(b) => TileShape::IntBox(b.translate_by(vector)),
            TileShape::IntOctagon(o) => TileShape::IntOctagon(o.translate_by(vector)),
            TileShape::Simplex(s) => TileShape::Simplex(s.translate_by(vector)),
        }
    }

    /// The shape turned by `factor` times 90 degrees around `pole`.
    pub fn turn_90_degree(&self, factor: i32, pole: IntPoint) -> TileShape {
        if let TileShape::IntBox(b) = self {
            return TileShape::IntBox(b.turn_90_degree(factor, pole));
        }
        let new_lines: Vec<Line> = (0..self.border_line_count())
            .map(|i| self.border_line(i).turn_90_degree(factor, pole))
            .collect();
        TileShape::get_instance_from_lines(&new_lines)
    }

    /// The shape rotated by `angle` around `pole`; not exact.
    ///
    /// Java builds the two-corner case with `new LineSegment(polyline, 0)`, which is out
    /// of range and fails with a `NullPointerException`; this port uses the segment
    /// number 1, the only valid one for a polyline through two points.
    pub fn rotate_approx(&self, angle: f64, pole: FloatPoint) -> TileShape {
        if angle == 0.0 {
            return self.clone();
        }
        let new_corners: Vec<Point> = (0..self.border_line_count())
            .map(|i| Point::Int(self.corner_approx(i).rotate(angle, pole).round()))
            .collect();
        let polygon_corners = Polygon::new(&new_corners).corner_array();
        match polygon_corners.len() {
            0 => TileShape::Simplex(Simplex::empty()),
            1 => TileShape::IntBox(TileShape::get_instance_point(&polygon_corners[0])),
            2 => {
                let polyline = Polyline::from_points(&polygon_corners);
                TileShape::Simplex(LineSegment::from_polyline(&polyline, 1).to_simplex())
            }
            _ => TileShape::get_instance_from_points(&polygon_corners),
        }
    }

    /// The shape mirrored at the vertical line through `pole`.
    pub fn mirror_vertical(&self, pole: IntPoint) -> TileShape {
        let new_lines: Vec<Line> = (0..self.border_line_count())
            .map(|i| self.border_line(i).mirror_vertical(pole))
            .collect();
        TileShape::get_instance_from_lines(&new_lines)
    }

    /// The shape mirrored at the horizontal line through `pole`.
    pub fn mirror_horizontal(&self, pole: IntPoint) -> TileShape {
        let new_lines: Vec<Line> = (0..self.border_line_count())
            .map(|i| self.border_line(i).mirror_horizontal(pole))
            .collect();
        TileShape::get_instance_from_lines(&new_lines)
    }

    /// The offset shape: enlarged if `distance > 0`, shrunk otherwise.
    pub fn offset(&self, distance: f64) -> TileShape {
        match self {
            TileShape::IntBox(b) => TileShape::IntBox(b.offset(distance)),
            TileShape::IntOctagon(o) => TileShape::IntOctagon(o.offset(distance)),
            TileShape::Simplex(s) => TileShape::Simplex(s.offset(distance)),
        }
    }

    /// The shape with its border moved outwards by `offset`.
    pub fn enlarge(&self, offset: f64) -> TileShape {
        match self {
            TileShape::IntBox(b) => TileShape::IntOctagon(b.enlarge(offset)),
            TileShape::IntOctagon(o) => TileShape::IntOctagon(o.enlarge(offset)),
            TileShape::Simplex(s) => TileShape::Simplex(s.enlarge(offset)),
        }
    }

    /// Shrinks the shape by `offset`; the result is not empty.
    pub fn shrink(&self, offset: f64) -> TileShape {
        let result = self.offset(-offset);
        if result.is_empty() {
            let centre_box = self.centre_of_gravity().bounding_box();
            return self.intersection(&TileShape::IntBox(centre_box));
        }
        result
    }

    /// A bounding tile of this shape (the shape itself).
    pub fn bounding_tile(&self) -> TileShape {
        self.clone()
    }

    /// The bounding regular shape with the fixed directions `dirs`; `None` where Java
    /// returns `null` (an unbounded simplex with 45 degree directions).
    pub fn bounding_shape(&self, dirs: ShapeBoundingDirections) -> Option<RegularTileShape> {
        match self {
            TileShape::IntBox(b) => Some(dirs.bounds_box(b)),
            TileShape::IntOctagon(o) => Some(dirs.bounds_octagon(o)),
            TileShape::Simplex(s) => dirs.bounds_simplex(s),
        }
    }

    /// Cuts `shape` out of this shape and divides the rest into convex pieces. `None`
    /// where Java returns `null` (the cut shape is a simplex of dimension below 2).
    pub fn cutout(&self, shape: &TileShape) -> Option<Vec<TileShape>> {
        match self {
            TileShape::IntBox(d) => {
                let tmp = shape.cutout_from_box(d)?;
                Some(tmp.iter().map(TileShape::simplify).collect())
            }
            TileShape::IntOctagon(d) => shape.cutout_from_octagon(d),
            TileShape::Simplex(d) => shape.cutout_from_simplex(d),
        }
    }

    fn cutout_from_box(&self, d: &IntBox) -> Option<Vec<TileShape>> {
        Some(match self {
            TileShape::IntBox(c) => c.cutout_from_box(d).into_iter().map(Into::into).collect(),
            TileShape::IntOctagon(c) => c.cutout_from_box(d).into_iter().map(Into::into).collect(),
            TileShape::Simplex(c) => c
                .cutout_from(&d.to_simplex())?
                .into_iter()
                .map(Into::into)
                .collect(),
        })
    }

    fn cutout_from_octagon(&self, d: &IntOctagon) -> Option<Vec<TileShape>> {
        Some(match self {
            TileShape::IntBox(c) => c
                .to_int_octagon()
                .cutout_from_octagon(d)
                .into_iter()
                .map(Into::into)
                .collect(),
            TileShape::IntOctagon(c) => c
                .cutout_from_octagon(d)
                .into_iter()
                .map(Into::into)
                .collect(),
            TileShape::Simplex(c) => c
                .cutout_from(&d.to_simplex())?
                .into_iter()
                .map(Into::into)
                .collect(),
        })
    }

    fn cutout_from_simplex(&self, d: &Simplex) -> Option<Vec<TileShape>> {
        let inner = match self {
            TileShape::IntBox(c) => c.to_simplex(),
            TileShape::IntOctagon(c) => c.to_simplex(),
            TileShape::Simplex(c) => c.clone(),
        };
        Some(inner.cutout_from(d)?.into_iter().map(Into::into).collect())
    }

    /// The division into convex pieces: the shape itself.
    pub fn split_to_convex(&self) -> Vec<TileShape> {
        vec![self.clone()]
    }

    /// Divides this shape into sections with width and height at most
    /// `max_section_width`.
    pub fn divide_into_sections(&self, max_section_width: f64) -> Vec<TileShape> {
        if let TileShape::IntBox(b) = self {
            return b
                .divide_into_sections(max_section_width)
                .into_iter()
                .map(TileShape::IntBox)
                .collect();
        }
        if self.is_empty() {
            return vec![self.clone()];
        }
        let section_boxes = self.bounding_box().divide_into_sections(max_section_width);
        let mut result = Vec::new();
        for section in section_boxes {
            let current_section = self.intersection_with_simplify(&TileShape::IntBox(section));
            if current_section.dimension() == 2 {
                result.push(current_section);
            }
        }
        result
    }

    /// Approximations of the `count` nearest relative outside locations of `shape` in
    /// the direction of different border lines of this shape, shortest first.
    pub fn nearest_relative_outside_locations(
        &self,
        shape: &TileShape,
        count: usize,
    ) -> Vec<FloatPoint> {
        let line_count = self.border_line_count();
        if count == 0 || line_count < 3 || !self.intersects_tile(shape) {
            return Vec::new();
        }
        let result_count = count.min(line_count);
        let mut translate_coors: Vec<Option<FloatPoint>> = vec![None; result_count];
        let mut min_dists = vec![f64::MAX; result_count];
        let mut current_ind = line_count - 1;
        let other_line_count = shape.border_line_count();
        for next_ind in 0..line_count {
            let mut current_max_dist = 0.0;
            let mut current_translate_coor = FloatPoint::ZERO;
            let border_line = self.border_line(current_ind);
            for corner_index in 0..other_line_count {
                let current_corner = shape.corner_approx(corner_index);
                if border_line.side_of_float(current_corner) == Side::OnTheRight {
                    let projection = current_corner.projection_approx(&border_line);
                    let current_distance = projection.distance_square(current_corner);
                    if current_distance > current_max_dist {
                        current_max_dist = current_distance;
                        current_translate_coor = projection.subtract(current_corner);
                    }
                }
            }
            for j in 0..result_count {
                if current_max_dist < min_dists[j] {
                    for k in j + 1..result_count {
                        min_dists[k] = min_dists[k - 1];
                        translate_coors[k] = translate_coors[k - 1];
                    }
                    min_dists[j] = current_max_dist;
                    translate_coors[j] = Some(current_translate_coor);
                    break;
                }
            }
            current_ind = next_ind;
        }
        translate_coors.into_iter().map_while(|p| p).collect()
    }

    /// The `IntBox`, if this shape is one.
    pub fn as_int_box(&self) -> Option<&IntBox> {
        match self {
            TileShape::IntBox(b) => Some(b),
            _ => None,
        }
    }

    /// The `IntOctagon`, if this shape is one.
    pub fn as_int_octagon(&self) -> Option<&IntOctagon> {
        match self {
            TileShape::IntOctagon(o) => Some(o),
            _ => None,
        }
    }

    /// The `Simplex`, if this shape is one.
    pub fn as_simplex(&self) -> Option<&Simplex> {
        match self {
            TileShape::Simplex(s) => Some(s),
            _ => None,
        }
    }
}

impl TileShape {
    /// [`cutout_polyline`](TileShapeOps::cutout_polyline) with Java's
    /// identity outcome made explicit: `None` where Java's
    /// `TileShape.cutout(Polyline)` returns its input polyline itself (no
    /// entrance points and the first corner not in the interior), else the
    /// pieces Java builds (possibly none).
    ///
    /// Java's `ShapeTraceEntries.cutoutTrace` tells "nothing cut off" by
    /// object identity (`pieces[0] == traceLines`); a newly built piece that
    /// equals the input is `Some`, so value equality never stands in for it.
    pub fn cutout_polyline_outcome(&self, polyline: &Polyline) -> Option<Vec<Polyline>> {
        if self.entrance_points(polyline).is_empty()
            && !self.contains_inside(&polyline.first_corner())
        {
            return None;
        }
        Some(self.cutout_polyline(polyline))
    }
}
