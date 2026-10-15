//! Shapes whose borders consist of straight lines.

use crate::float_line::FloatLine;
use crate::float_point::FloatPoint;
use crate::line::Line;
use crate::point::{IntPoint, Point};
use crate::side::Side;
use crate::vector::Vector;

use super::{IntBox, IntOctagon, PolygonShape, TileShape, TileShapeOps};

/// Functionality shared by all shapes whose borders consist of straight lines.
///
/// Port of the abstract class `app.freerouting.geometry.planar.PolylineShape`. The
/// required methods are the abstract methods of the Java class (and of the `Shape`/`Area`
/// interfaces it needs); the provided methods are its concrete methods, which
/// implementors may override like Java subclasses do.
///
/// Corner and border line numbers are `usize`; Java methods that return `-1` for "not
/// found" return `Option<usize>` here.
pub trait PolylineShapeOps {
    /// The number of border lines (and corners) of the shape.
    fn border_line_count(&self) -> usize;

    /// The `no`-th corner. The corners are sorted counterclockwise, starting with the
    /// corner with the smallest y (then smallest x) coordinate.
    fn corner(&self, no: usize) -> Point;

    /// The `no`-th border line.
    fn border_line(&self, no: usize) -> Line;

    /// True if the shape has no infinite part at corner `no`.
    fn corner_is_bounded(&self, no: usize) -> bool;

    /// True if the shape is empty.
    fn is_empty(&self) -> bool;

    /// True if the shape is contained in a sufficiently large box.
    fn is_bounded(&self) -> bool;

    /// 2 for a two-dimensional shape, 1 for a curve, 0 for a point and -1 if empty.
    fn dimension(&self) -> i32;

    /// The smallest surrounding box. For unbounded shapes some coordinates may be
    /// `i32::MAX`.
    fn bounding_box(&self) -> IntBox;

    /// The smallest surrounding octagon; `None` where Java returns `null` (an unbounded
    /// `Simplex`).
    fn bounding_octagon(&self) -> Option<IntOctagon>;

    /// Approximation of corner `no`; coordinates are `i32::MAX` at an unbounded corner.
    fn corner_approx(&self, no: usize) -> FloatPoint {
        self.corner(no).to_float()
    }

    /// Approximations of all corners.
    fn corner_approx_arr(&self) -> Vec<FloatPoint> {
        (0..self.border_line_count())
            .map(|i| self.corner_approx(i))
            .collect()
    }

    /// All bounded corners of the shape.
    fn bounded_corners(&self) -> Vec<Point> {
        (0..self.border_line_count())
            .filter(|&i| self.corner_is_bounded(i))
            .map(|i| self.corner(i))
            .collect()
    }

    /// The number of the corner equal to `point`, if any.
    fn equals_corner(&self, point: &Point) -> Option<usize> {
        (0..self.border_line_count()).find(|&i| *point == self.corner(i))
    }

    /// Cumulative border length; `i32::MAX` if the shape is unbounded.
    fn circumference(&self) -> f64 {
        if !self.is_bounded() {
            return f64::from(i32::MAX);
        }
        let corner_count = self.border_line_count();
        if corner_count == 0 {
            return 0.0;
        }
        let mut result = 0.0;
        let mut prev_corner = self.corner_approx(corner_count - 1);
        for i in 0..corner_count {
            let current_corner = self.corner_approx(i);
            result += current_corner.distance(prev_corner);
            prev_corner = current_corner;
        }
        result
    }

    /// The arithmetic middle of the corners.
    fn centre_of_gravity(&self) -> FloatPoint {
        let corner_count = self.border_line_count();
        let mut x = 0.0;
        let mut y = 0.0;
        for i in 0..corner_count {
            let current_point = self.corner_approx(i);
            x += current_point.x;
            y += current_point.y;
        }
        x /= corner_count as f64;
        y /= corner_count as f64;
        FloatPoint::new(x, y)
    }

    /// True if the shape is completely contained in `b`.
    fn is_contained_in(&self, b: &IntBox) -> bool {
        self.bounding_box().is_contained_in(b)
    }

    /// The corner such that all other points of the shape are to the right of the line
    /// from `from_point` to it.
    fn index_of_left_most_corner(&self, from_point: FloatPoint) -> usize {
        let mut left_most_corner = self.corner_approx(0);
        let mut result = 0;
        for i in 1..self.border_line_count() {
            let current_corner = self.corner_approx(i);
            if current_corner.side_of(from_point, left_most_corner) == Side::OnTheLeft {
                left_most_corner = current_corner;
                result = i;
            }
        }
        result
    }

    /// The corner such that all other points of the shape are to the left of the line
    /// from `from_point` to it.
    fn index_of_right_most_corner(&self, from_point: FloatPoint) -> usize {
        let mut right_most_corner = self.corner_approx(0);
        let mut result = 0;
        for i in 1..self.border_line_count() {
            let current_corner = self.corner_approx(i);
            if current_corner.side_of(from_point, right_most_corner) == Side::OnTheRight {
                right_most_corner = current_corner;
                result = i;
            }
        }
        result
    }

    /// A segment from (approximately) the left most to the right most corner seen from
    /// `from_point`; `None` for an empty shape.
    fn polar_line_segment(&self, from_point: FloatPoint) -> Option<FloatLine> {
        if self.is_empty() {
            return None;
        }
        let mut left_most_corner = self.corner_approx(0);
        let mut right_most_corner = self.corner_approx(0);
        for i in 1..self.border_line_count() {
            let current_corner = self.corner_approx(i);
            if current_corner.side_of(from_point, right_most_corner) == Side::OnTheRight {
                right_most_corner = current_corner;
            }
            if current_corner.side_of(from_point, left_most_corner) == Side::OnTheLeft {
                left_most_corner = current_corner;
            }
        }
        Some(FloatLine::new(left_most_corner, right_most_corner))
    }

    /// The previous border line or corner number.
    fn prev_no(&self, no: usize) -> usize {
        if no == 0 {
            self.border_line_count() - 1
        } else {
            no - 1
        }
    }

    /// The next border line or corner number.
    fn next_no(&self, no: usize) -> usize {
        (no + 1) % self.border_line_count()
    }

    /// True if this shape and `line` have a common point.
    fn intersects_line(&self, line: &Line) -> bool {
        let side_of_first_corner = line.side_of(&self.corner(0));
        if side_of_first_corner == Side::Collinear {
            return true;
        }
        (1..self.border_line_count()).any(|i| line.side_of(&self.corner(i)) != side_of_first_corner)
    }

    /// The left most corner of the shape seen from `from_point`.
    fn left_most_corner(&self, from_point: &Point) -> Point {
        if self.is_empty() {
            return from_point.clone();
        }
        let mut result = self.corner(0);
        for i in 1..self.border_line_count() {
            let current_corner = self.corner(i);
            if current_corner.side_of(from_point, &result) == Side::OnTheLeft {
                result = current_corner;
            }
        }
        result
    }

    /// The right most corner of the shape seen from `from_point`.
    fn right_most_corner(&self, from_point: &Point) -> Point {
        if self.is_empty() {
            return from_point.clone();
        }
        let mut result = self.corner(0);
        for i in 1..self.border_line_count() {
            let current_corner = self.corner(i);
            if current_corner.side_of(from_point, &result) == Side::OnTheRight {
                result = current_corner;
            }
        }
        result
    }
}

/// A shape whose border consists of straight lines: a convex [`TileShape`] or a general
/// [`PolygonShape`].
///
/// Port of the Java class hierarchy below `PolylineShape`.
#[derive(Debug, Clone)]
pub enum PolylineShape {
    Tile(TileShape),
    Polygon(PolygonShape),
}

impl From<TileShape> for PolylineShape {
    fn from(t: TileShape) -> Self {
        PolylineShape::Tile(t)
    }
}

impl From<PolygonShape> for PolylineShape {
    fn from(p: PolygonShape) -> Self {
        PolylineShape::Polygon(p)
    }
}

impl From<IntBox> for PolylineShape {
    fn from(b: IntBox) -> Self {
        PolylineShape::Tile(TileShape::IntBox(b))
    }
}

impl From<IntOctagon> for PolylineShape {
    fn from(o: IntOctagon) -> Self {
        PolylineShape::Tile(TileShape::IntOctagon(o))
    }
}

macro_rules! dispatch {
    ($self:ident, $s:ident => $e:expr) => {
        match $self {
            PolylineShape::Tile($s) => $e,
            PolylineShape::Polygon($s) => $e,
        }
    };
}

impl PolylineShapeOps for PolylineShape {
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

impl PolylineShape {
    /// Content of the area; `f64::MAX` if unbounded.
    pub fn area(&self) -> f64 {
        match self {
            PolylineShape::Tile(t) => t.area(),
            PolylineShape::Polygon(p) => p.area(),
        }
    }

    /// True if `point` is neither inside nor on the border of the shape.
    pub fn is_outside(&self, point: &Point) -> bool {
        match self {
            PolylineShape::Tile(t) => t.is_outside(point),
            PolylineShape::Polygon(p) => p.is_outside(point),
        }
    }

    /// True if `point` is inside or on the border of the shape.
    pub fn contains(&self, point: &Point) -> bool {
        match self {
            PolylineShape::Tile(t) => t.contains(point),
            PolylineShape::Polygon(p) => p.contains(point),
        }
    }

    /// True if `point` is contained in the shape.
    pub fn contains_float(&self, point: FloatPoint) -> bool {
        match self {
            PolylineShape::Tile(t) => t.contains_float(point),
            PolylineShape::Polygon(p) => p.contains_float(point),
        }
    }

    /// True if `point` is inside the shape but not on its border.
    pub fn contains_inside(&self, point: &Point) -> bool {
        match self {
            PolylineShape::Tile(t) => t.contains_inside(point),
            PolylineShape::Polygon(p) => p.contains_inside(point),
        }
    }

    /// True if `point` lies exactly on the border.
    pub fn contains_on_border(&self, point: &Point) -> bool {
        match self {
            PolylineShape::Tile(t) => t.contains_on_border(point),
            PolylineShape::Polygon(p) => p.contains_on_border(point),
        }
    }

    /// Distance from `point` to the shape; 0 if contained.
    pub fn distance(&self, point: FloatPoint) -> f64 {
        match self {
            PolylineShape::Tile(t) => t.distance(point),
            PolylineShape::Polygon(p) => p.distance(point),
        }
    }

    /// Distance from `point` to the border.
    pub fn border_distance(&self, point: FloatPoint) -> f64 {
        match self {
            PolylineShape::Tile(t) => t.border_distance(point),
            PolylineShape::Polygon(p) => p.border_distance(point),
        }
    }

    /// Smallest distance from the centre of gravity to the border.
    pub fn smallest_radius(&self) -> f64 {
        match self {
            PolylineShape::Tile(t) => t.smallest_radius(),
            PolylineShape::Polygon(p) => p.smallest_radius(),
        }
    }

    /// Approximation of the nearest point of the shape to `from_point`.
    pub fn nearest_point_approx(&self, from_point: FloatPoint) -> Option<FloatPoint> {
        match self {
            PolylineShape::Tile(t) => t.nearest_point_approx(from_point),
            PolylineShape::Polygon(p) => p.nearest_point_approx(from_point),
        }
    }

    /// A bounding convex shape.
    pub fn bounding_tile(&self) -> TileShape {
        match self {
            PolylineShape::Tile(t) => t.bounding_tile(),
            PolylineShape::Polygon(p) => p.bounding_tile(),
        }
    }

    /// The division into convex pieces; `None` if it fails.
    pub fn split_to_convex(&self) -> Option<Vec<TileShape>> {
        match self {
            PolylineShape::Tile(t) => Some(t.split_to_convex()),
            PolylineShape::Polygon(p) => p.split_to_convex(),
        }
    }

    /// The shape translated by `vector`.
    pub fn translate_by(&self, vector: &Vector) -> PolylineShape {
        match self {
            PolylineShape::Tile(t) => PolylineShape::Tile(t.translate_by(vector)),
            PolylineShape::Polygon(p) => PolylineShape::Polygon(p.translate_by(vector)),
        }
    }

    /// The shape turned by `factor` times 90 degrees around `pole`.
    pub fn turn_90_degree(&self, factor: i32, pole: IntPoint) -> PolylineShape {
        match self {
            PolylineShape::Tile(t) => PolylineShape::Tile(t.turn_90_degree(factor, pole)),
            PolylineShape::Polygon(p) => PolylineShape::Polygon(p.turn_90_degree(factor, pole)),
        }
    }

    /// The shape rotated by `angle` around `pole`; not exact.
    pub fn rotate_approx(&self, angle: f64, pole: FloatPoint) -> PolylineShape {
        match self {
            PolylineShape::Tile(t) => PolylineShape::Tile(t.rotate_approx(angle, pole)),
            PolylineShape::Polygon(p) => PolylineShape::Polygon(p.rotate_approx(angle, pole)),
        }
    }

    /// The shape mirrored at the vertical line through `pole`.
    pub fn mirror_vertical(&self, pole: IntPoint) -> PolylineShape {
        match self {
            PolylineShape::Tile(t) => PolylineShape::Tile(t.mirror_vertical(pole)),
            PolylineShape::Polygon(p) => PolylineShape::Polygon(p.mirror_vertical(pole)),
        }
    }

    /// The shape mirrored at the horizontal line through `pole`.
    pub fn mirror_horizontal(&self, pole: IntPoint) -> PolylineShape {
        match self {
            PolylineShape::Tile(t) => PolylineShape::Tile(t.mirror_horizontal(pole)),
            PolylineShape::Polygon(p) => PolylineShape::Polygon(p.mirror_horizontal(pole)),
        }
    }
}
