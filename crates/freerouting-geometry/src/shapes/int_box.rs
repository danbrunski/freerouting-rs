//! Axis parallel rectangles with integer coordinates.

use std::fmt;

use crate::direction::IntDirection;
use crate::float_point::{FloatPoint, java_max};
use crate::jmath::{d2i, round_to_i32};
use crate::limits::CRIT_INT;
use crate::line::Line;
use crate::point::{IntPoint, Point};
use crate::side::Side;
use crate::vector::Vector;

use super::{IntOctagon, PolylineShapeOps, RegularTileShape, Simplex, TileShape, TileShapeOps};

/// An axis parallel rectangle with integer coordinates.
///
/// Port of `app.freerouting.geometry.planar.IntBox`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IntBox {
    /// The lower left corner.
    pub ll: IntPoint,
    /// The upper right corner.
    pub ur: IntPoint,
}

impl IntBox {
    /// The standard empty box.
    pub const EMPTY: IntBox = IntBox {
        ll: IntPoint::new(CRIT_INT, CRIT_INT),
        ur: IntPoint::new(-CRIT_INT, -CRIT_INT),
    };

    /// Creates a box from the coordinates of its lower left and upper right corners.
    pub const fn new(
        lower_left_x: i32,
        lower_left_y: i32,
        upper_right_x: i32,
        upper_right_y: i32,
    ) -> IntBox {
        IntBox {
            ll: IntPoint::new(lower_left_x, lower_left_y),
            ur: IntPoint::new(upper_right_x, upper_right_y),
        }
    }

    /// Creates a box from its lower left and upper right corners.
    pub const fn from_points(ll: IntPoint, ur: IntPoint) -> IntBox {
        IntBox { ll, ur }
    }

    /// The horizontal extension.
    pub fn width(&self) -> i32 {
        self.ur.x.wrapping_sub(self.ll.x)
    }

    /// The vertical extension.
    pub fn height(&self) -> i32 {
        self.ur.y.wrapping_sub(self.ll.y)
    }

    /// True if `point` lies in the interior of this box.
    pub fn contains_inside_int(&self, point: IntPoint) -> bool {
        point.x > self.ll.x && point.x < self.ur.x && point.y > self.ll.y && point.y < self.ur.y
    }

    /// The nearest point of this box to `from_point`.
    pub fn nearest_point(&self, from_point: FloatPoint) -> FloatPoint {
        let x = if from_point.x <= f64::from(self.ll.x) {
            f64::from(self.ll.x)
        } else if from_point.x >= f64::from(self.ur.x) {
            f64::from(self.ur.x)
        } else {
            from_point.x
        };
        let y = if from_point.y <= f64::from(self.ll.y) {
            f64::from(self.ll.y)
        } else if from_point.y >= f64::from(self.ur.y) {
            f64::from(self.ur.y)
        } else {
            from_point.y
        };
        FloatPoint::new(x, y)
    }

    /// The (at most 2) nearest points on the border of this box to `point`, which is
    /// assumed to be inside the box, nearest first.
    pub fn nearest_border_projections(
        &self,
        point: IntPoint,
        max_result_points: usize,
    ) -> Vec<IntPoint> {
        if max_result_points == 0 {
            return Vec::new();
        }
        let max_result_points = max_result_points.min(2);
        let lower_horizontal_difference = point.x.wrapping_sub(self.ll.x);
        let upper_horizontal_difference = self.ur.x.wrapping_sub(point.x);
        let lower_vertical_difference = point.y.wrapping_sub(self.ll.y);
        let upper_vertical_difference = self.ur.y.wrapping_sub(point.y);

        let mut min_diff;
        let mut second_min_diff;
        let mut nearest_x;
        let mut nearest_y = point.y;
        let mut second_x;
        let mut second_y = point.y;
        if lower_horizontal_difference <= upper_horizontal_difference {
            min_diff = lower_horizontal_difference;
            second_min_diff = upper_horizontal_difference;
            nearest_x = self.ll.x;
            second_x = self.ur.x;
        } else {
            min_diff = upper_horizontal_difference;
            second_min_diff = lower_horizontal_difference;
            nearest_x = self.ur.x;
            second_x = self.ll.x;
        }
        if lower_vertical_difference < min_diff {
            second_min_diff = min_diff;
            min_diff = lower_vertical_difference;
            second_x = nearest_x;
            second_y = nearest_y;
            nearest_x = point.x;
            nearest_y = self.ll.y;
        } else if lower_vertical_difference < second_min_diff {
            second_min_diff = lower_vertical_difference;
            second_x = point.x;
            second_y = self.ll.y;
        }
        if upper_vertical_difference < min_diff {
            second_x = nearest_x;
            second_y = nearest_y;
            nearest_x = point.x;
            nearest_y = self.ur.y;
        } else if upper_vertical_difference < second_min_diff {
            second_x = point.x;
            second_y = self.ur.y;
        }
        let mut result = vec![IntPoint::new(nearest_x, nearest_y)];
        if max_result_points > 1 {
            result.push(IntPoint::new(second_x, second_y));
        }
        result
    }

    /// The weighted distance to the box `other`.
    pub fn weighted_distance(
        &self,
        other: &IntBox,
        horizontal_weight: f64,
        vertical_weight: f64,
    ) -> f64 {
        let max_ll_x = f64::from(self.ll.x.max(other.ll.x));
        let max_ll_y = f64::from(self.ll.y.max(other.ll.y));
        let min_ur_x = f64::from(self.ur.x.min(other.ur.x));
        let min_ur_y = f64::from(self.ur.y.min(other.ur.y));
        if min_ur_x >= max_ll_x {
            java_max(vertical_weight * (max_ll_y - min_ur_y), 0.0)
        } else if min_ur_y >= max_ll_y {
            java_max(horizontal_weight * (max_ll_x - min_ur_x), 0.0)
        } else {
            let delta_x = (max_ll_x - min_ur_x) * horizontal_weight;
            let delta_y = (max_ll_y - min_ur_y) * vertical_weight;
            (delta_x * delta_x + delta_y * delta_y).sqrt()
        }
    }

    /// The smallest box containing this box and `other`.
    pub fn union_box(&self, other: &IntBox) -> IntBox {
        IntBox::new(
            self.ll.x.min(other.ll.x),
            self.ll.y.min(other.ll.y),
            self.ur.x.max(other.ur.x),
            self.ur.y.max(other.ur.y),
        )
    }

    /// The smallest octagon containing this box and `other`.
    pub fn union_octagon(&self, other: &IntOctagon) -> IntOctagon {
        other.union_octagon(&self.to_int_octagon())
    }

    /// True if this box and `other`, each grown by one unit on every side, do not
    /// meet, so no shape inside one has a point in common with a shape inside the
    /// other (K13.9). An empty box is never apart. A simplex's box comes from its
    /// corners in `f64`, rounded outwards; the unit covers a corner that rounding put
    /// on the wrong side of an integer.
    pub fn is_apart_from(&self, other: &IntBox) -> bool {
        if self.is_empty() || other.is_empty() {
            return false;
        }
        i64::from(self.ur.x) + 1 < i64::from(other.ll.x) - 1
            || i64::from(other.ur.x) + 1 < i64::from(self.ll.x) - 1
            || i64::from(self.ur.y) + 1 < i64::from(other.ll.y) - 1
            || i64::from(other.ur.y) + 1 < i64::from(self.ll.y) - 1
    }

    /// The intersection of this box with `other`.
    pub fn intersection_box(&self, other: &IntBox) -> IntBox {
        if other.ll.x > self.ur.x
            || other.ll.y > self.ur.y
            || self.ll.x > other.ur.x
            || self.ll.y > other.ur.y
        {
            return IntBox::EMPTY;
        }
        IntBox::new(
            self.ll.x.max(other.ll.x),
            self.ll.y.max(other.ll.y),
            self.ur.x.min(other.ur.x),
            self.ur.y.min(other.ur.y),
        )
    }

    /// True if this box and `other` intersect.
    pub fn intersects_box(&self, other: &IntBox) -> bool {
        if other.ll.x > self.ur.x {
            return false;
        }
        if other.ll.y > self.ur.y {
            return false;
        }
        if self.ll.x > other.ur.x {
            return false;
        }
        self.ll.y <= other.ur.y
    }

    /// True if this box intersects `other` and the intersection is 2-dimensional.
    pub fn overlaps(&self, other: &IntBox) -> bool {
        if other.ll.x >= self.ur.x {
            return false;
        }
        if other.ll.y >= self.ur.y {
            return false;
        }
        if self.ll.x >= other.ur.x {
            return false;
        }
        self.ll.y < other.ur.y
    }

    /// True if this box contains `other` (Java `contains(RegularTileShape)`).
    pub fn contains_regular(&self, other: &RegularTileShape) -> bool {
        match other {
            RegularTileShape::IntBox(b) => b.is_contained_in(self),
            RegularTileShape::IntOctagon(o) => o.is_contained_in(self),
        }
    }

    /// The box enlarged by `offset` as an octagon (contrary to [`IntBox::offset`]).
    pub fn enlarge(&self, offset: f64) -> IntOctagon {
        self.to_int_octagon().offset(offset)
    }

    /// The box translated by `rel_coor`.
    ///
    /// # Panics
    /// If `rel_coor` is a rational vector (only integer vectors are implemented in
    /// Java).
    pub fn translate_by(&self, rel_coor: &Vector) -> IntBox {
        if rel_coor.is_zero_constant() {
            return *self;
        }
        let v = rel_coor.expect_int();
        IntBox::from_points(self.ll.translate_by_int(v), self.ur.translate_by_int(v))
    }

    /// The box turned by `factor` times 90 degrees around `pole`.
    pub fn turn_90_degree(&self, factor: i32, pole: IntPoint) -> IntBox {
        let p1 = self.ll.turn_90_degree(factor, pole);
        let p2 = self.ur.turn_90_degree(factor, pole);
        IntBox::new(
            p1.x.min(p2.x),
            p1.y.min(p2.y),
            p1.x.max(p2.x),
            p1.y.max(p2.y),
        )
    }

    /// The box offset by `dist`: to the outside if `dist > 0`, else to the inside.
    ///
    /// DIVERGENCE (K15.1): the coordinates saturate at `i32::MIN` and
    /// `i32::MAX`. Java's `int` sums wrap, so the box of an unbounded shape
    /// (see [`IntBox::has_infinite_coordinate`]) offset outwards came out
    /// empty. Inside the limits the result is Java's.
    pub fn offset(&self, dist: f64) -> IntBox {
        if dist == 0.0 || self.is_empty() {
            return *self;
        }
        let r = round_to_i32(dist);
        IntBox::new(
            self.ll.x.saturating_sub(r),
            self.ll.y.saturating_sub(r),
            self.ur.x.saturating_add(r),
            self.ur.y.saturating_add(r),
        )
    }

    /// True if a coordinate is `i32::MIN` or `i32::MAX`: the box of an
    /// unbounded shape, whose infinite corners round to the limits
    /// (`Simplex::bounding_box`). [`IntBox::EMPTY`] has none.
    pub fn has_infinite_coordinate(&self) -> bool {
        [self.ll.x, self.ll.y, self.ur.x, self.ur.y]
            .iter()
            .any(|&c| c == i32::MIN || c == i32::MAX)
    }

    /// The box with its horizontal extension offset by `dist`.
    pub fn horizontal_offset(&self, dist: f64) -> IntBox {
        if dist == 0.0 || self.is_empty() {
            return *self;
        }
        let r = round_to_i32(dist);
        IntBox::new(
            self.ll.x.wrapping_sub(r),
            self.ll.y,
            self.ur.x.wrapping_add(r),
            self.ur.y,
        )
    }

    /// The box with its vertical extension offset by `dist`.
    pub fn vertical_offset(&self, dist: f64) -> IntBox {
        if dist == 0.0 || self.is_empty() {
            return *self;
        }
        let r = round_to_i32(dist);
        IntBox::new(
            self.ll.x,
            self.ll.y.wrapping_sub(r),
            self.ur.x,
            self.ur.y.wrapping_add(r),
        )
    }

    /// Shrinks width and height by `width`; the box does not vanish completely (Java
    /// `IntBox.shrink(int)`).
    pub fn shrink_int(&self, width: i32) -> IntBox {
        let (llx, urx) = if width.wrapping_mul(2) <= self.ur.x.wrapping_sub(self.ll.x) {
            (self.ll.x.wrapping_add(width), self.ur.x.wrapping_sub(width))
        } else {
            let m = self.ll.x.wrapping_add(self.ur.x) / 2;
            (m, m)
        };
        let (lly, ury) = if width.wrapping_mul(2) <= self.ur.y.wrapping_sub(self.ll.y) {
            (self.ll.y.wrapping_add(width), self.ur.y.wrapping_sub(width))
        } else {
            let m = self.ll.y.wrapping_add(self.ur.y) / 2;
            (m, m)
        };
        IntBox::new(llx, lly, urx, ury)
    }

    /// Compares the edge lines with index `edge_index` of this box and `other`:
    /// `OnTheLeft` if the edge of this box is on the left of the edge of `other`.
    ///
    /// # Panics
    /// If `edge_index > 3`.
    pub fn compare_box(&self, other: &IntBox, edge_index: usize) -> Side {
        fn side(left: bool, right: bool) -> Side {
            if left {
                Side::OnTheLeft
            } else if right {
                Side::OnTheRight
            } else {
                Side::Collinear
            }
        }
        match edge_index {
            0 => side(self.ll.y > other.ll.y, self.ll.y < other.ll.y),
            1 => side(self.ur.x < other.ur.x, self.ur.x > other.ur.x),
            2 => side(self.ur.y < other.ur.y, self.ur.y > other.ur.y),
            3 => side(self.ll.x > other.ll.x, self.ll.x < other.ll.x),
            _ => panic!("IntBox.compare: edgeIndex out of range"),
        }
    }

    /// Compares the edge lines with index `edge_index` of this box and `other`.
    pub fn compare_octagon(&self, other: &IntOctagon, edge_index: usize) -> Side {
        self.to_int_octagon().compare_octagon(other, edge_index)
    }

    /// The octagon describing the same shape.
    pub fn to_int_octagon(&self) -> IntOctagon {
        IntOctagon::new(
            self.ll.x,
            self.ll.y,
            self.ur.x,
            self.ur.y,
            self.ll.x.wrapping_sub(self.ur.y),
            self.ur.x.wrapping_sub(self.ll.y),
            self.ll.x.wrapping_add(self.ll.y),
            self.ur.x.wrapping_add(self.ur.y),
        )
    }

    /// True if this box is contained in `other`.
    pub fn is_contained_in_octagon(&self, other: &IntOctagon) -> bool {
        self.to_int_octagon().is_contained_in_octagon(other)
    }

    /// True if `other` is contained in the interior of this box.
    pub fn contains_in_interior(&self, other: &IntBox) -> bool {
        if other.is_empty() {
            return true;
        }
        other.ll.x > self.ll.x
            && other.ll.y > self.ll.y
            && other.ur.x < self.ur.x
            && other.ur.y < self.ur.y
    }

    /// The part of `from_box` with minimal distance to this box.
    pub fn nearest_part(&self, from_box: &IntBox) -> IntBox {
        let ll_x = if from_box.ll.x >= self.ll.x {
            from_box.ll.x
        } else {
            from_box.ur.x.min(self.ll.x)
        };
        let ur_x = if from_box.ur.x <= self.ur.x {
            from_box.ur.x
        } else {
            from_box.ll.x.max(self.ur.x)
        };
        let ll_y = if from_box.ll.y >= self.ll.y {
            from_box.ll.y
        } else {
            from_box.ur.y.min(self.ll.y)
        };
        let ur_y = if from_box.ur.y <= self.ur.y {
            from_box.ur.y
        } else {
            from_box.ll.y.max(self.ur.y)
        };
        IntBox::new(ll_x, ll_y, ur_x, ur_y)
    }

    /// Divides this box into sections of about equal size with width and height at most
    /// `max_section_width`.
    pub fn divide_into_sections(&self, max_section_width: f64) -> Vec<IntBox> {
        if max_section_width <= 0.0 {
            return Vec::new();
        }
        let length = f64::from(self.ur.x.wrapping_sub(self.ll.x));
        let height = f64::from(self.ur.y.wrapping_sub(self.ll.y));
        let xcount = d2i((length / max_section_width).ceil());
        let ycount = d2i((height / max_section_width).ceil());
        let section_length_x = d2i((length / f64::from(xcount)).ceil());
        let section_length_y = d2i((height / f64::from(ycount)).ceil());
        let mut result = Vec::new();
        for j in 0..ycount {
            let current_lly = self.ll.y.wrapping_add(j.wrapping_mul(section_length_y));
            let current_ury = if j == ycount - 1 {
                self.ur.y
            } else {
                current_lly.wrapping_add(section_length_y)
            };
            for i in 0..xcount {
                let current_llx = self.ll.x.wrapping_add(i.wrapping_mul(section_length_x));
                let current_urx = if i == xcount - 1 {
                    self.ur.x
                } else {
                    current_llx.wrapping_add(section_length_x)
                };
                result.push(IntBox::new(
                    current_llx,
                    current_lly,
                    current_urx,
                    current_ury,
                ));
            }
        }
        result
    }

    /// Divides `d` minus this box into 4 boxes (Java `cutoutFrom(IntBox)`).
    pub fn cutout_from_box(&self, d: &IntBox) -> Vec<IntBox> {
        let c = self.intersection_box(d);
        if self.is_empty() || c.dimension() < self.dimension() {
            return vec![*d];
        }
        let mut result = [
            IntBox::new(d.ll.x, d.ll.y, c.ur.x, c.ll.y),
            IntBox::new(d.ll.x, c.ll.y, c.ll.x, d.ur.y),
            IntBox::new(c.ur.x, d.ll.y, d.ur.x, c.ur.y),
            IntBox::new(c.ll.x, c.ur.y, d.ur.x, d.ur.y),
        ];
        // Optimise the division so that the cumulative circumference is minimal.
        if c.ll.x.wrapping_sub(d.ll.x) > c.ll.y.wrapping_sub(d.ll.y) {
            let b = result[0];
            result[0] = IntBox::new(c.ll.x, b.ll.y, b.ur.x, b.ur.y);
            let b = result[1];
            result[1] = IntBox::new(b.ll.x, d.ll.y, b.ur.x, b.ur.y);
        }
        if d.ur.y.wrapping_sub(c.ur.y) > c.ll.x.wrapping_sub(d.ll.x) {
            let b = result[1];
            result[1] = IntBox::new(b.ll.x, b.ll.y, b.ur.x, c.ur.y);
            let b = result[3];
            result[3] = IntBox::new(d.ll.x, b.ll.y, b.ur.x, b.ur.y);
        }
        if d.ur.x.wrapping_sub(c.ur.x) > d.ur.y.wrapping_sub(c.ur.y) {
            let b = result[2];
            result[2] = IntBox::new(b.ll.x, b.ll.y, b.ur.x, d.ur.y);
            let b = result[3];
            result[3] = IntBox::new(b.ll.x, b.ll.y, c.ur.x, b.ur.y);
        }
        if c.ll.y.wrapping_sub(d.ll.y) > d.ur.x.wrapping_sub(c.ur.x) {
            let b = result[0];
            result[0] = IntBox::new(b.ll.x, b.ll.y, d.ur.x, b.ur.y);
            let b = result[2];
            result[2] = IntBox::new(b.ll.x, c.ll.y, b.ur.x, b.ur.y);
        }
        result.to_vec()
    }
}

impl PolylineShapeOps for IntBox {
    fn border_line_count(&self) -> usize {
        4
    }

    /// # Panics
    /// If `no > 3`.
    fn corner(&self, no: usize) -> Point {
        Point::Int(match no {
            0 => self.ll,
            1 => IntPoint::new(self.ur.x, self.ll.y),
            2 => self.ur,
            3 => IntPoint::new(self.ll.x, self.ur.y),
            _ => panic!("IntBox.corner: no out of range"),
        })
    }

    /// # Panics
    /// If `no > 3`.
    fn border_line(&self, no: usize) -> Line {
        match no {
            0 => Line::from_coords(0, self.ll.y, 1, self.ll.y),
            1 => Line::from_coords(self.ur.x, 0, self.ur.x, 1),
            2 => Line::from_coords(0, self.ur.y, -1, self.ur.y),
            3 => Line::from_coords(self.ll.x, 0, self.ll.x, -1),
            _ => panic!("IntBox.borderLine: no out of range"),
        }
    }

    fn corner_is_bounded(&self, _no: usize) -> bool {
        true
    }

    fn is_empty(&self) -> bool {
        self.ll.x > self.ur.x || self.ll.y > self.ur.y
    }

    fn is_bounded(&self) -> bool {
        true
    }

    fn dimension(&self) -> i32 {
        if self.is_empty() {
            return -1;
        }
        if self.ll == self.ur {
            return 0;
        }
        if self.ur.x == self.ll.x || self.ll.y == self.ur.y {
            return 1;
        }
        2
    }

    fn bounding_box(&self) -> IntBox {
        *self
    }

    fn bounding_octagon(&self) -> Option<IntOctagon> {
        Some(self.to_int_octagon())
    }

    fn circumference(&self) -> f64 {
        f64::from(
            2i32.wrapping_mul(
                self.ur
                    .x
                    .wrapping_sub(self.ll.x)
                    .wrapping_add(self.ur.y.wrapping_sub(self.ll.y)),
            ),
        )
    }

    fn is_contained_in(&self, other: &IntBox) -> bool {
        if self.is_empty() || self == other {
            return true;
        }
        self.ll.x >= other.ll.x
            && self.ll.y >= other.ll.y
            && self.ur.x <= other.ur.x
            && self.ur.y <= other.ur.y
    }
}

impl TileShapeOps for IntBox {
    fn get_id(&self) -> i32 {
        31i32
            .wrapping_mul(self.ll.get_id())
            .wrapping_add(self.ur.get_id())
    }

    fn is_int_box(&self) -> bool {
        true
    }

    fn is_int_octagon(&self) -> bool {
        true
    }

    fn border_line_index(&self, _line: &Line) -> Option<usize> {
        None
    }

    fn to_simplex(&self) -> Simplex {
        if self.is_empty() {
            return Simplex::new(Vec::new());
        }
        Simplex::new(vec![
            Line::get_instance(self.ll, IntDirection::RIGHT),
            Line::get_instance(self.ur, IntDirection::UP),
            Line::get_instance(self.ur, IntDirection::LEFT),
            Line::get_instance(self.ll, IntDirection::DOWN),
        ])
    }

    fn to_tile_shape(&self) -> TileShape {
        TileShape::IntBox(*self)
    }

    fn max_width(&self) -> f64 {
        f64::from(
            self.ur
                .x
                .wrapping_sub(self.ll.x)
                .max(self.ur.y.wrapping_sub(self.ll.y)),
        )
    }

    fn min_width(&self) -> f64 {
        f64::from(
            self.ur
                .x
                .wrapping_sub(self.ll.x)
                .min(self.ur.y.wrapping_sub(self.ll.y)),
        )
    }

    fn area(&self) -> f64 {
        f64::from(self.ur.x.wrapping_sub(self.ll.x)) * f64::from(self.ur.y.wrapping_sub(self.ll.y))
    }

    fn distance(&self, from_point: FloatPoint) -> f64 {
        from_point.distance(self.nearest_point(from_point))
    }
}

impl fmt::Display for IntBox {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "IntBox(ll={}, ur={})", self.ll, self.ur)
    }
}
