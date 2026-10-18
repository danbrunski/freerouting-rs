//! Octagons with integer coordinates and border lines in multiples of 45 degrees.

use std::fmt;

use crate::direction::FortyfiveDegreeDirection;
use crate::float_point::FloatPoint;
use crate::jmath::{d2i, round_to_i32};
use crate::limits::{CRIT_INT, SQRT2};
use crate::line::Line;
use crate::point::{IntPoint, Point};
use crate::quirks::{SplitQuirks, SplitReach};
use crate::side::Side;
use crate::vector::Vector;

use super::{IntBox, PolylineShapeOps, RegularTileShape, Simplex, TileShape, TileShapeOps};

/// An octagon with integer coordinates whose border lines are horizontal, vertical or
/// diagonal.
///
/// Port of `app.freerouting.geometry.planar.IntOctagon`. The diagonal border lines are
/// given by their intersection with the x axis.
///
/// Java's `isEmpty()` compares by reference with the `EMPTY` instance; this port
/// compares by value with [`IntOctagon::EMPTY`]. Java also memoizes `toSimplex()`; this
/// port recomputes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IntOctagon {
    /// X coordinate of the left vertical border.
    pub left_x: i32,
    /// Y coordinate of the bottom horizontal border.
    pub bottom_y: i32,
    /// X coordinate of the right vertical border.
    pub right_x: i32,
    /// Y coordinate of the top horizontal border.
    pub top_y: i32,
    /// X axis intersection of the upper left diagonal border (-45 degrees).
    pub upper_left_diagonal_x: i32,
    /// X axis intersection of the lower right diagonal border (-45 degrees).
    pub lower_right_diagonal_x: i32,
    /// X axis intersection of the lower left diagonal border (+45 degrees).
    pub lower_left_diagonal_x: i32,
    /// X axis intersection of the upper right diagonal border (+45 degrees).
    pub upper_right_diagonal_x: i32,
}

impl IntOctagon {
    /// The empty octagon.
    pub const EMPTY: IntOctagon = IntOctagon::new(
        CRIT_INT, CRIT_INT, -CRIT_INT, -CRIT_INT, CRIT_INT, -CRIT_INT, CRIT_INT, -CRIT_INT,
    );

    /// Creates an octagon from its 8 boundary values (in Java constructor order).
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        left_x: i32,
        bottom_y: i32,
        right_x: i32,
        top_y: i32,
        upper_left_diagonal_x: i32,
        lower_right_diagonal_x: i32,
        lower_left_diagonal_x: i32,
        upper_right_diagonal_x: i32,
    ) -> IntOctagon {
        IntOctagon {
            left_x,
            bottom_y,
            right_x,
            top_y,
            upper_left_diagonal_x,
            lower_right_diagonal_x,
            lower_left_diagonal_x,
            upper_right_diagonal_x,
        }
    }

    /// The y coordinate of corner `no`.
    ///
    /// # Panics
    /// If `no > 7`.
    pub fn corner_y(&self, no: usize) -> i32 {
        match no {
            0 | 1 => self.bottom_y,
            2 => self.right_x.wrapping_sub(self.lower_right_diagonal_x),
            3 => self.upper_right_diagonal_x.wrapping_sub(self.right_x),
            4 | 5 => self.top_y,
            6 => self.left_x.wrapping_sub(self.upper_left_diagonal_x),
            7 => self.lower_left_diagonal_x.wrapping_sub(self.left_x),
            _ => panic!("IntOctagon.corner: no out of range"),
        }
    }

    /// The x coordinate of corner `no`.
    ///
    /// # Panics
    /// If `no > 7`.
    pub fn corner_x(&self, no: usize) -> i32 {
        match no {
            0 => self.lower_left_diagonal_x.wrapping_sub(self.bottom_y),
            1 => self.lower_right_diagonal_x.wrapping_add(self.bottom_y),
            2 | 3 => self.right_x,
            4 => self.upper_right_diagonal_x.wrapping_sub(self.top_y),
            5 => self.upper_left_diagonal_x.wrapping_add(self.top_y),
            6 | 7 => self.left_x,
            _ => panic!("IntOctagon.corner: no out of range"),
        }
    }

    /// The corner `no` as an `IntPoint`.
    pub fn int_corner(&self, no: usize) -> IntPoint {
        IntPoint::new(self.corner_x(no), self.corner_y(no))
    }

    /// The octagon translated by `rel_coor`.
    ///
    /// # Panics
    /// If `rel_coor` is a rational vector.
    pub fn translate_by(&self, rel_coor: &Vector) -> IntOctagon {
        if rel_coor.is_zero_constant() {
            return *self;
        }
        let v = rel_coor.expect_int();
        IntOctagon::new(
            self.left_x.wrapping_add(v.x),
            self.bottom_y.wrapping_add(v.y),
            self.right_x.wrapping_add(v.x),
            self.top_y.wrapping_add(v.y),
            self.upper_left_diagonal_x
                .wrapping_add(v.x)
                .wrapping_sub(v.y),
            self.lower_right_diagonal_x
                .wrapping_add(v.x)
                .wrapping_sub(v.y),
            self.lower_left_diagonal_x
                .wrapping_add(v.x)
                .wrapping_add(v.y),
            self.upper_right_diagonal_x
                .wrapping_add(v.x)
                .wrapping_add(v.y),
        )
    }

    /// The octagon offset by `distance` (to the outside if positive), normalized.
    pub fn offset(&self, distance: f64) -> IntOctagon {
        let width = round_to_i32(distance);
        if width == 0 {
            return *self;
        }
        let dia_width = round_to_i32(SQRT2 * distance);
        IntOctagon::new(
            self.left_x.wrapping_sub(width),
            self.bottom_y.wrapping_sub(width),
            self.right_x.wrapping_add(width),
            self.top_y.wrapping_add(width),
            self.upper_left_diagonal_x.wrapping_sub(dia_width),
            self.lower_right_diagonal_x.wrapping_add(dia_width),
            self.lower_left_diagonal_x.wrapping_sub(dia_width),
            self.upper_right_diagonal_x.wrapping_add(dia_width),
        )
        .normalize()
    }

    /// Same as [`IntOctagon::offset`].
    pub fn enlarge(&self, offset: f64) -> IntOctagon {
        self.offset(offset)
    }

    /// True if this octagon contains `other` (Java `contains(RegularTileShape)`).
    pub fn contains_regular(&self, other: &RegularTileShape) -> bool {
        match other {
            RegularTileShape::IntBox(b) => b.is_contained_in_octagon(self),
            RegularTileShape::IntOctagon(o) => o.is_contained_in_octagon(self),
        }
    }

    /// The smallest octagon containing this octagon and `other`.
    pub fn union_octagon(&self, other: &IntOctagon) -> IntOctagon {
        IntOctagon::new(
            self.left_x.min(other.left_x),
            self.bottom_y.min(other.bottom_y),
            self.right_x.max(other.right_x),
            self.top_y.max(other.top_y),
            self.upper_left_diagonal_x.min(other.upper_left_diagonal_x),
            self.lower_right_diagonal_x
                .max(other.lower_right_diagonal_x),
            self.lower_left_diagonal_x.min(other.lower_left_diagonal_x),
            self.upper_right_diagonal_x
                .max(other.upper_right_diagonal_x),
        )
    }

    /// The smallest octagon containing this octagon and `other`.
    pub fn union_box(&self, other: &IntBox) -> IntOctagon {
        self.union_octagon(&other.to_int_octagon())
    }

    /// The intersection with `other`, normalized.
    pub fn intersection_octagon(&self, other: &IntOctagon) -> IntOctagon {
        IntOctagon::new(
            self.left_x.max(other.left_x),
            self.bottom_y.max(other.bottom_y),
            self.right_x.min(other.right_x),
            self.top_y.min(other.top_y),
            self.upper_left_diagonal_x.max(other.upper_left_diagonal_x),
            self.lower_right_diagonal_x
                .min(other.lower_right_diagonal_x),
            self.lower_left_diagonal_x.max(other.lower_left_diagonal_x),
            self.upper_right_diagonal_x
                .min(other.upper_right_diagonal_x),
        )
        .normalize()
    }

    /// The intersection with the box `other`, normalized.
    pub fn intersection_box(&self, other: &IntBox) -> IntOctagon {
        self.intersection_octagon(&other.to_int_octagon())
    }

    /// An equivalent octagon with all redundant bounds tightened; [`IntOctagon::EMPTY`]
    /// if it is empty.
    pub fn normalize(&self) -> IntOctagon {
        if self.left_x > self.right_x
            || self.bottom_y > self.top_y
            || self.lower_left_diagonal_x > self.upper_right_diagonal_x
            || self.upper_left_diagonal_x > self.lower_right_diagonal_x
        {
            return IntOctagon::EMPTY;
        }
        let mut lx = self.left_x;
        let mut rx = self.right_x;
        let mut ly = self.bottom_y;
        let mut uy = self.top_y;
        let mut llx = self.lower_left_diagonal_x;
        let mut ulx = self.upper_left_diagonal_x;
        let mut lrx = self.lower_right_diagonal_x;
        let mut urx = self.upper_right_diagonal_x;

        if lx < llx.wrapping_sub(uy) {
            lx = llx.wrapping_sub(uy);
        }
        if lx < ulx.wrapping_add(ly) {
            lx = ulx.wrapping_add(ly);
        }
        if rx > urx.wrapping_sub(ly) {
            rx = urx.wrapping_sub(ly);
        }
        if rx > lrx.wrapping_add(uy) {
            rx = lrx.wrapping_add(uy);
        }
        if ly < lx.wrapping_sub(lrx) {
            ly = lx.wrapping_sub(lrx);
        }
        if ly < llx.wrapping_sub(rx) {
            ly = llx.wrapping_sub(rx);
        }
        if uy > urx.wrapping_sub(lx) {
            uy = urx.wrapping_sub(lx);
        }
        if uy > rx.wrapping_sub(ulx) {
            uy = rx.wrapping_sub(ulx);
        }
        if llx.wrapping_sub(lx) < ly {
            llx = lx.wrapping_add(ly);
        }
        if rx.wrapping_sub(lrx) < ly {
            lrx = rx.wrapping_sub(ly);
        }
        if urx.wrapping_sub(rx) > uy {
            urx = uy.wrapping_add(rx);
        }
        if lx.wrapping_sub(ulx) > uy {
            ulx = lx.wrapping_sub(uy);
        }
        let diag_upper_y = d2i((f64::from(urx.wrapping_sub(ulx)) / 2.0).ceil());
        if uy > diag_upper_y {
            uy = diag_upper_y;
        }
        let diag_lower_y = d2i((f64::from(llx.wrapping_sub(lrx)) / 2.0).floor());
        if ly < diag_lower_y {
            ly = diag_lower_y;
        }
        let diag_right_x = d2i((f64::from(urx.wrapping_add(lrx)) / 2.0).ceil());
        if rx > diag_right_x {
            rx = diag_right_x;
        }
        let diag_left_x = d2i((f64::from(llx.wrapping_add(ulx)) / 2.0).floor());
        if lx < diag_left_x {
            lx = diag_left_x;
        }
        if lx > rx || ly > uy || llx > urx || ulx > lrx {
            return IntOctagon::EMPTY;
        }
        IntOctagon::new(lx, ly, rx, uy, ulx, lrx, llx, urx)
    }

    /// True if this octagon equals its normalization.
    pub fn is_normalized(&self) -> bool {
        *self == self.normalize()
    }

    /// The side of the point `(x, y)` with respect to border line `border_line_no`.
    ///
    /// # Panics
    /// If `border_line_no > 7` (Java logs a warning and returns `Collinear`).
    pub fn side_of_border_line(&self, x: i32, y: i32, border_line_no: usize) -> Side {
        let tmp = match border_line_no {
            0 => self.bottom_y.wrapping_sub(y),
            1 => x.wrapping_sub(y).wrapping_sub(self.lower_right_diagonal_x),
            2 => x.wrapping_sub(self.right_x),
            3 => x.wrapping_add(y).wrapping_sub(self.upper_right_diagonal_x),
            4 => y.wrapping_sub(self.top_y),
            5 => self.upper_left_diagonal_x.wrapping_add(y).wrapping_sub(x),
            6 => self.left_x.wrapping_sub(x),
            7 => self.lower_left_diagonal_x.wrapping_sub(x).wrapping_sub(y),
            _ => 0,
        };
        if tmp < 0 {
            Side::OnTheLeft
        } else if tmp > 0 {
            Side::OnTheRight
        } else {
            Side::Collinear
        }
    }

    /// True if this octagon is contained in `other`.
    pub fn is_contained_in_octagon(&self, other: &IntOctagon) -> bool {
        self.left_x >= other.left_x
            && self.bottom_y >= other.bottom_y
            && self.right_x <= other.right_x
            && self.top_y <= other.top_y
            && self.lower_left_diagonal_x >= other.lower_left_diagonal_x
            && self.upper_left_diagonal_x >= other.upper_left_diagonal_x
            && self.lower_right_diagonal_x <= other.lower_right_diagonal_x
            && self.upper_right_diagonal_x <= other.upper_right_diagonal_x
    }

    /// True if two normalized octagons intersect.
    pub fn intersects_octagon(&self, other: &IntOctagon) -> bool {
        if other.left_x.max(self.left_x) > other.right_x.min(self.right_x) {
            return false;
        }
        if other.bottom_y.max(self.bottom_y) > other.top_y.min(self.top_y) {
            return false;
        }
        if other.lower_left_diagonal_x.max(self.lower_left_diagonal_x)
            > other
                .upper_right_diagonal_x
                .min(self.upper_right_diagonal_x)
        {
            return false;
        }
        other.upper_left_diagonal_x.max(self.upper_left_diagonal_x)
            <= other
                .lower_right_diagonal_x
                .min(self.lower_right_diagonal_x)
    }

    /// True if this octagon intersects `other` and the intersection is 2-dimensional.
    pub fn overlaps(&self, other: &IntOctagon) -> bool {
        if other.left_x.max(self.left_x) >= other.right_x.min(self.right_x) {
            return false;
        }
        if other.bottom_y.max(self.bottom_y) >= other.top_y.min(self.top_y) {
            return false;
        }
        if other.lower_left_diagonal_x.max(self.lower_left_diagonal_x)
            >= other
                .upper_right_diagonal_x
                .min(self.upper_right_diagonal_x)
        {
            return false;
        }
        other.upper_left_diagonal_x.max(self.upper_left_diagonal_x)
            < other
                .lower_right_diagonal_x
                .min(self.lower_right_diagonal_x)
    }

    /// The x value of the left boundary at `y`.
    pub fn left_x_value(&self, y: i32) -> i32 {
        self.left_x
            .max(self.upper_left_diagonal_x.wrapping_add(y))
            .max(self.lower_left_diagonal_x.wrapping_sub(y))
    }

    /// The x value of the right boundary at `y`.
    pub fn right_x_value(&self, y: i32) -> i32 {
        self.right_x
            .min(self.upper_right_diagonal_x.wrapping_sub(y))
            .min(self.lower_right_diagonal_x.wrapping_add(y))
    }

    /// The y value of the lower boundary at `x`.
    pub fn lower_y_value(&self, x: i32) -> i32 {
        self.bottom_y
            .max(self.lower_left_diagonal_x.wrapping_sub(x))
            .max(x.wrapping_sub(self.lower_right_diagonal_x))
    }

    /// The y value of the upper boundary at `x`.
    pub fn upper_y_value(&self, x: i32) -> i32 {
        self.top_y
            .min(x.wrapping_sub(self.upper_left_diagonal_x))
            .min(self.upper_right_diagonal_x.wrapping_sub(x))
    }

    /// Compares the edge lines with index `edge_index` of this octagon and `other`:
    /// `OnTheLeft` if the edge of this octagon is on the left of the edge of `other`.
    ///
    /// # Panics
    /// If `edge_index > 7`.
    pub fn compare_octagon(&self, other: &IntOctagon, edge_index: usize) -> Side {
        fn side(left: bool, right: bool) -> Side {
            if left {
                Side::OnTheLeft
            } else if right {
                Side::OnTheRight
            } else {
                Side::Collinear
            }
        }
        let (a, b) = (self, other);
        match edge_index {
            0 => side(a.bottom_y > b.bottom_y, a.bottom_y < b.bottom_y),
            1 => side(
                a.lower_right_diagonal_x < b.lower_right_diagonal_x,
                a.lower_right_diagonal_x > b.lower_right_diagonal_x,
            ),
            2 => side(a.right_x < b.right_x, a.right_x > b.right_x),
            3 => side(
                a.upper_right_diagonal_x < b.upper_right_diagonal_x,
                a.upper_right_diagonal_x > b.upper_right_diagonal_x,
            ),
            4 => side(a.top_y < b.top_y, a.top_y > b.top_y),
            5 => side(
                a.upper_left_diagonal_x > b.upper_left_diagonal_x,
                a.upper_left_diagonal_x < b.upper_left_diagonal_x,
            ),
            6 => side(a.left_x > b.left_x, a.left_x < b.left_x),
            7 => side(
                a.lower_left_diagonal_x > b.lower_left_diagonal_x,
                a.lower_left_diagonal_x < b.lower_left_diagonal_x,
            ),
            _ => panic!("IntBox.compare: edgeIndex out of range"),
        }
    }

    /// Compares the edge lines with index `edge_index` of this octagon and `other`.
    pub fn compare_box(&self, other: &IntBox, edge_index: usize) -> Side {
        self.compare_octagon(&other.to_int_octagon(), edge_index)
    }

    /// The border point of this octagon from `point` into the 45 degree direction
    /// `dir`; if it is not an integer point, the nearest integer point outside.
    pub fn border_point(&self, point: IntPoint, dir: FortyfiveDegreeDirection) -> IntPoint {
        let (px, py) = (point.x, point.y);
        let (result_x, result_y);
        match dir {
            FortyfiveDegreeDirection::Right => {
                result_x = self
                    .right_x
                    .min(self.upper_right_diagonal_x.wrapping_sub(py))
                    .min(self.lower_right_diagonal_x.wrapping_add(py));
                result_y = py;
            }
            FortyfiveDegreeDirection::Left => {
                result_x = self
                    .left_x
                    .max(self.upper_left_diagonal_x.wrapping_add(py))
                    .max(self.lower_left_diagonal_x.wrapping_sub(py));
                result_y = py;
            }
            FortyfiveDegreeDirection::Up => {
                result_x = px;
                result_y = self
                    .top_y
                    .min(px.wrapping_sub(self.upper_left_diagonal_x))
                    .min(self.upper_right_diagonal_x.wrapping_sub(px));
            }
            FortyfiveDegreeDirection::Down => {
                result_x = px;
                result_y = self
                    .bottom_y
                    .max(self.lower_left_diagonal_x.wrapping_sub(px))
                    .max(px.wrapping_sub(self.lower_right_diagonal_x));
            }
            FortyfiveDegreeDirection::Right45 => {
                let x = d2i((0.5
                    * f64::from(
                        px.wrapping_sub(py)
                            .wrapping_add(self.upper_right_diagonal_x),
                    ))
                .ceil());
                result_x = x
                    .min(self.right_x)
                    .min(px.wrapping_sub(py).wrapping_add(self.top_y));
                result_y = py.wrapping_sub(px).wrapping_add(result_x);
            }
            FortyfiveDegreeDirection::Up45 => {
                let x = d2i((0.5
                    * f64::from(px.wrapping_add(py).wrapping_add(self.upper_left_diagonal_x)))
                .floor());
                result_x = x
                    .max(self.left_x)
                    .max(px.wrapping_add(py).wrapping_sub(self.top_y));
                result_y = py.wrapping_add(px).wrapping_sub(result_x);
            }
            FortyfiveDegreeDirection::Left45 => {
                let x = d2i((0.5
                    * f64::from(px.wrapping_sub(py).wrapping_add(self.lower_left_diagonal_x)))
                .floor());
                result_x = x
                    .max(self.left_x)
                    .max(px.wrapping_sub(py).wrapping_add(self.bottom_y));
                result_y = py.wrapping_sub(px).wrapping_add(result_x);
            }
            FortyfiveDegreeDirection::Down45 => {
                let x = d2i((0.5
                    * f64::from(
                        px.wrapping_add(py)
                            .wrapping_add(self.lower_right_diagonal_x),
                    ))
                .ceil());
                result_x = x
                    .min(self.right_x)
                    .min(px.wrapping_add(py).wrapping_sub(self.bottom_y));
                result_y = py.wrapping_add(px).wrapping_sub(result_x);
            }
        }
        IntPoint::new(result_x, result_y)
    }

    /// The sorted nearest border points of this octagon in the 45 degree directions from
    /// `point`, which is assumed to be inside the octagon.
    pub fn nearest_border_projections(
        &self,
        point: IntPoint,
        max_result_points: usize,
    ) -> Vec<IntPoint> {
        if !self.contains(&Point::Int(point)) || max_result_points == 0 {
            return Vec::new();
        }
        let max_result_points = max_result_points.min(8);
        let mut result: Vec<Option<IntPoint>> = vec![None; max_result_points];
        let mut min_dist = vec![f64::MAX; max_result_points];
        let inside_point = point.to_float();
        for current_direction in FortyfiveDegreeDirection::VALUES {
            let current_border_point = self.border_point(point, current_direction);
            let current_distance = inside_point.distance_square(current_border_point.to_float());
            for i in 0..max_result_points {
                if current_distance < min_dist[i] {
                    for k in (i + 1..max_result_points).rev() {
                        min_dist[k] = min_dist[k - 1];
                        result[k] = result[k - 1];
                    }
                    min_dist[i] = current_distance;
                    result[i] = Some(current_border_point);
                    break;
                }
            }
        }
        result.into_iter().map_while(|p| p).collect()
    }

    /// The side of `point` with respect to border line `line_index`, with `tolerance`.
    ///
    /// # Panics
    /// If `line_index > 7` (Java logs a warning and returns `Collinear`).
    pub fn border_line_side_of(
        &self,
        point: FloatPoint,
        line_index: usize,
        tolerance: f64,
    ) -> Side {
        fn side(right: bool, left: bool) -> Side {
            if right {
                Side::OnTheRight
            } else if left {
                Side::OnTheLeft
            } else {
                Side::Collinear
            }
        }
        match line_index {
            0 => {
                let v = f64::from(self.bottom_y);
                side(point.y > v + tolerance, point.y < v - tolerance)
            }
            2 => {
                let v = f64::from(self.right_x);
                side(point.x < v - tolerance, point.x > v + tolerance)
            }
            4 => {
                let v = f64::from(self.top_y);
                side(point.y < v - tolerance, point.y > v + tolerance)
            }
            6 => {
                let v = f64::from(self.left_x);
                side(point.x > v + tolerance, point.x < v - tolerance)
            }
            1 => {
                let tmp = point.y - point.x + f64::from(self.lower_right_diagonal_x);
                side(tmp > tolerance, tmp < -tolerance)
            }
            3 => {
                let tmp = point.x + point.y - f64::from(self.upper_right_diagonal_x);
                side(tmp < -tolerance, tmp > tolerance)
            }
            5 => {
                let tmp = point.y - point.x + f64::from(self.upper_left_diagonal_x);
                side(tmp < -tolerance, tmp > tolerance)
            }
            7 => {
                let tmp = point.x + point.y - f64::from(self.lower_left_diagonal_x);
                side(tmp > tolerance, tmp < -tolerance)
            }
            _ => Side::Collinear,
        }
    }

    /// The bounding box if this octagon is a box, else the octagon itself.
    pub fn simplify(&self) -> TileShape {
        if self.is_int_box() {
            return TileShape::IntBox(self.bounding_box());
        }
        TileShape::IntOctagon(*self)
    }

    /// Divides `d` minus this octagon into 8 convex pieces, 4 of which have a corner cut
    /// off (Java `cutoutFrom(IntBox)`).
    pub fn cutout_from_box(&self, d: &IntBox) -> Vec<IntOctagon> {
        self.cutout_from_box_with(d, SplitQuirks::JAVA)
    }

    /// [`Self::cutout_from_box`] with the switches of the cutouts (N06 batch G).
    pub fn cutout_from_box_with(&self, d: &IntBox, quirks: SplitQuirks) -> Vec<IntOctagon> {
        self.cutout_from_box_counted(d, quirks, &mut SplitReach::default())
    }

    /// [`Self::cutout_from_box_with`], adding to `reach` the cutouts whose branch 3
    /// (`octagon_box_cutout_branch3`) changes the upper right octagon.
    pub fn cutout_from_box_counted(
        &self,
        d: &IntBox,
        quirks: SplitQuirks,
        reach: &mut SplitReach,
    ) -> Vec<IntOctagon> {
        let c = self.intersection_box(d);
        if self.is_empty() || c.dimension() < self.dimension() {
            return vec![d.to_int_octagon()];
        }
        let neg = CRIT_INT.wrapping_neg();
        let pos = CRIT_INT;
        let mut boxes = [
            // left box
            IntBox::new(
                d.ll.x,
                c.lower_left_diagonal_x.wrapping_sub(c.left_x),
                c.left_x,
                c.left_x.wrapping_sub(c.upper_left_diagonal_x),
            ),
            // right box
            IntBox::new(
                c.right_x,
                c.right_x.wrapping_sub(c.lower_right_diagonal_x),
                d.ur.x,
                c.upper_right_diagonal_x.wrapping_sub(c.right_x),
            ),
            // lower box
            IntBox::new(
                c.lower_left_diagonal_x.wrapping_sub(c.bottom_y),
                d.ll.y,
                c.lower_right_diagonal_x.wrapping_add(c.bottom_y),
                c.bottom_y,
            ),
            // upper box
            IntBox::new(
                c.upper_left_diagonal_x.wrapping_add(c.top_y),
                c.top_y,
                c.upper_right_diagonal_x.wrapping_sub(c.top_y),
                d.ur.y,
            ),
        ];
        let mut octagons = [
            // upper left
            IntOctagon::new(
                d.ll.x,
                boxes[0].ur.y,
                boxes[3].ll.x,
                d.ur.y,
                neg,
                c.upper_left_diagonal_x,
                neg,
                pos,
            )
            .normalize(),
            // lower left
            IntOctagon::new(
                d.ll.x,
                d.ll.y,
                boxes[2].ll.x,
                boxes[0].ll.y,
                neg,
                pos,
                neg,
                c.lower_left_diagonal_x,
            )
            .normalize(),
            // lower right
            IntOctagon::new(
                boxes[2].ur.x,
                d.ll.y,
                d.ur.x,
                boxes[1].ll.y,
                c.lower_right_diagonal_x,
                pos,
                neg,
                pos,
            )
            .normalize(),
            // upper right
            IntOctagon::new(
                boxes[3].ur.x,
                boxes[1].ur.y,
                d.ur.x,
                d.ur.y,
                neg,
                pos,
                c.upper_right_diagonal_x,
                pos,
            )
            .normalize(),
        ];

        // Optimise the result to minimum cumulative circumference.
        let with = |o: &IntOctagon, lx: i32, ly: i32, rx: i32, uy: i32| {
            IntOctagon::new(
                lx,
                ly,
                rx,
                uy,
                o.upper_left_diagonal_x,
                o.lower_right_diagonal_x,
                o.lower_left_diagonal_x,
                o.upper_right_diagonal_x,
            )
            .normalize()
        };
        let (b, o) = (boxes[0], octagons[0]);
        if b.ur.x.wrapping_sub(b.ll.x) > o.top_y.wrapping_sub(o.bottom_y) {
            boxes[0] = IntBox::new(b.ll.x, b.ll.y, b.ur.x, o.top_y);
            octagons[0] = with(&o, b.ur.x, o.bottom_y, o.right_x, o.top_y);
        }
        let (b, o) = (boxes[3], octagons[0]);
        if b.ur.y.wrapping_sub(b.ll.y) > o.right_x.wrapping_sub(o.left_x) {
            boxes[3] = IntBox::new(o.left_x, b.ll.y, b.ur.x, b.ur.y);
            octagons[0] = with(&o, o.left_x, o.bottom_y, o.right_x, b.ll.y);
        }
        let (b, o) = (boxes[3], octagons[3]);
        if b.ur.y.wrapping_sub(b.ll.y) > o.right_x.wrapping_sub(o.left_x) {
            boxes[3] = IntBox::new(b.ll.x, b.ll.y, o.right_x, b.ur.y);
            // PARITY(quirk, java_quirks.octagon_box_cutout_branch3): IntOctagon.java:1206-1223
            // rebuilds the octagon unchanged, so it overlaps the moved box. Under `false` the
            // octagon's top moves down to the box's bottom, as branch 2 does for the upper
            // left octagon.
            if o.top_y != b.ll.y {
                reach.octagon_box_cutout_branch3 += 1;
            }
            let top_y = if quirks.octagon_box_cutout_branch3 {
                o.top_y
            } else {
                b.ll.y
            };
            octagons[3] = with(&o, o.left_x, o.bottom_y, o.right_x, top_y);
        }
        let (b, o) = (boxes[1], octagons[3]);
        if b.ur.x.wrapping_sub(b.ll.x) > o.top_y.wrapping_sub(o.bottom_y) {
            boxes[1] = IntBox::new(b.ll.x, b.ll.y, b.ur.x, o.top_y);
            octagons[3] = with(&o, o.left_x, o.bottom_y, b.ll.x, o.top_y);
        }
        let (b, o) = (boxes[1], octagons[2]);
        if b.ur.x.wrapping_sub(b.ll.x) > o.top_y.wrapping_sub(o.bottom_y) {
            boxes[1] = IntBox::new(b.ll.x, o.bottom_y, b.ur.x, b.ur.y);
            octagons[2] = with(&o, o.left_x, o.bottom_y, b.ll.x, o.top_y);
        }
        let (b, o) = (boxes[2], octagons[2]);
        if b.ur.y.wrapping_sub(b.ll.y) > o.right_x.wrapping_sub(o.left_x) {
            boxes[2] = IntBox::new(b.ll.x, b.ll.y, o.right_x, b.ur.y);
            octagons[2] = with(&o, o.left_x, b.ur.y, o.right_x, o.top_y);
        }
        let (b, o) = (boxes[2], octagons[1]);
        if b.ur.y.wrapping_sub(b.ll.y) > o.right_x.wrapping_sub(o.left_x) {
            boxes[2] = IntBox::new(o.left_x, b.ll.y, b.ur.x, b.ur.y);
            octagons[1] = with(&o, o.left_x, b.ur.y, o.right_x, o.top_y);
        }
        let (b, o) = (boxes[0], octagons[1]);
        if b.ur.x.wrapping_sub(b.ll.x) > o.top_y.wrapping_sub(o.bottom_y) {
            boxes[0] = IntBox::new(b.ll.x, o.bottom_y, b.ur.x, b.ur.y);
            octagons[1] = with(&o, b.ur.x, o.bottom_y, o.right_x, o.top_y);
        }
        let mut result: Vec<IntOctagon> = boxes.iter().map(IntBox::to_int_octagon).collect();
        result.extend_from_slice(&octagons);
        result
    }

    /// Divides `d` minus this octagon into 8 convex pieces without sharp angles (Java
    /// `cutoutFrom(IntOctagon)`).
    pub fn cutout_from_octagon(&self, d: &IntOctagon) -> Vec<IntOctagon> {
        let c = self.intersection_octagon(d);
        if self.is_empty() || c.dimension() < self.dimension() {
            return vec![*d];
        }
        let mut result = [IntOctagon::EMPTY; 8];
        let mut tmp = c.lower_left_diagonal_x.wrapping_sub(c.left_x);
        result[0] = IntOctagon::new(
            d.left_x,
            tmp,
            c.left_x,
            c.left_x.wrapping_sub(c.upper_left_diagonal_x),
            d.upper_left_diagonal_x,
            d.lower_right_diagonal_x,
            d.lower_left_diagonal_x,
            d.upper_right_diagonal_x,
        );
        let mut tmp2 = c.lower_left_diagonal_x.wrapping_sub(c.bottom_y);
        result[1] = IntOctagon::new(
            d.left_x,
            d.bottom_y,
            tmp2,
            tmp,
            d.upper_left_diagonal_x,
            d.lower_right_diagonal_x,
            d.lower_left_diagonal_x,
            c.lower_left_diagonal_x,
        );
        tmp = c.lower_right_diagonal_x.wrapping_add(c.bottom_y);
        result[2] = IntOctagon::new(
            tmp2,
            d.bottom_y,
            tmp,
            c.bottom_y,
            d.upper_left_diagonal_x,
            d.lower_right_diagonal_x,
            d.lower_left_diagonal_x,
            d.upper_right_diagonal_x,
        );
        tmp2 = c.right_x.wrapping_sub(c.lower_right_diagonal_x);
        result[3] = IntOctagon::new(
            tmp,
            d.bottom_y,
            d.right_x,
            tmp2,
            c.lower_right_diagonal_x,
            d.lower_right_diagonal_x,
            d.lower_left_diagonal_x,
            d.upper_right_diagonal_x,
        );
        tmp = c.upper_right_diagonal_x.wrapping_sub(c.right_x);
        result[4] = IntOctagon::new(
            c.right_x,
            tmp2,
            d.right_x,
            tmp,
            d.upper_left_diagonal_x,
            d.lower_right_diagonal_x,
            d.lower_left_diagonal_x,
            d.upper_right_diagonal_x,
        );
        tmp2 = c.upper_right_diagonal_x.wrapping_sub(c.top_y);
        result[5] = IntOctagon::new(
            tmp2,
            tmp,
            d.right_x,
            d.top_y,
            d.upper_left_diagonal_x,
            d.lower_right_diagonal_x,
            c.upper_right_diagonal_x,
            d.upper_right_diagonal_x,
        );
        tmp = c.upper_left_diagonal_x.wrapping_add(c.top_y);
        result[6] = IntOctagon::new(
            tmp,
            c.top_y,
            tmp2,
            d.top_y,
            d.upper_left_diagonal_x,
            d.lower_right_diagonal_x,
            d.lower_left_diagonal_x,
            d.upper_right_diagonal_x,
        );
        tmp2 = c.left_x.wrapping_sub(c.upper_left_diagonal_x);
        result[7] = IntOctagon::new(
            d.left_x,
            tmp2,
            tmp,
            d.top_y,
            d.upper_left_diagonal_x,
            c.upper_left_diagonal_x,
            d.lower_left_diagonal_x,
            d.upper_right_diagonal_x,
        );
        for r in result.iter_mut() {
            *r = r.normalize();
        }

        let mut curr1 = result[0];
        let mut curr2 = result[7];
        if !(curr1.is_empty() || curr2.is_empty())
            && curr1.right_x.wrapping_sub(curr1.left_x_value(curr1.top_y))
                > curr2
                    .upper_y_value(curr1.right_x)
                    .wrapping_sub(curr2.bottom_y)
        {
            curr1 = IntOctagon::new(
                curr1.left_x.min(curr2.left_x),
                curr1.bottom_y,
                curr1.right_x,
                curr2.top_y,
                curr2.upper_left_diagonal_x,
                curr1.lower_right_diagonal_x,
                curr1.lower_left_diagonal_x,
                curr2.upper_right_diagonal_x,
            );
            curr2 = IntOctagon::new(
                curr1.right_x,
                curr2.bottom_y,
                curr2.right_x,
                curr2.top_y,
                curr2.upper_left_diagonal_x,
                curr2.lower_right_diagonal_x,
                curr2.lower_left_diagonal_x,
                curr2.upper_right_diagonal_x,
            );
            result[0] = curr1.normalize();
            result[7] = curr2.normalize();
        }
        curr1 = result[7];
        curr2 = result[6];
        if !(curr1.is_empty() || curr2.is_empty())
            && curr2
                .upper_y_value(curr1.right_x)
                .wrapping_sub(curr2.bottom_y)
                > curr1
                    .right_x
                    .wrapping_sub(curr1.left_x_value(curr2.bottom_y))
        {
            curr2 = IntOctagon::new(
                curr1.left_x,
                curr2.bottom_y,
                curr2.right_x,
                curr2.top_y.max(curr1.top_y),
                curr1.upper_left_diagonal_x,
                curr2.lower_right_diagonal_x,
                curr1.lower_left_diagonal_x,
                curr2.upper_right_diagonal_x,
            );
            curr1 = IntOctagon::new(
                curr1.left_x,
                curr1.bottom_y,
                curr1.right_x,
                curr2.bottom_y,
                curr1.upper_left_diagonal_x,
                curr1.lower_right_diagonal_x,
                curr1.lower_left_diagonal_x,
                curr1.upper_right_diagonal_x,
            );
            result[7] = curr1.normalize();
            result[6] = curr2.normalize();
        }
        curr1 = result[6];
        curr2 = result[5];
        if !(curr1.is_empty() || curr2.is_empty())
            && curr2
                .upper_y_value(curr1.right_x)
                .wrapping_sub(curr1.bottom_y)
                > curr2
                    .right_x_value(curr1.bottom_y)
                    .wrapping_sub(curr2.left_x)
        {
            curr1 = IntOctagon::new(
                curr1.left_x,
                curr1.bottom_y,
                curr2.right_x,
                curr2.top_y.max(curr1.top_y),
                curr1.upper_left_diagonal_x,
                curr2.lower_right_diagonal_x,
                curr1.lower_left_diagonal_x,
                curr2.upper_right_diagonal_x,
            );
            curr2 = IntOctagon::new(
                curr2.left_x,
                curr2.bottom_y,
                curr2.right_x,
                curr1.bottom_y,
                curr2.upper_left_diagonal_x,
                curr2.lower_right_diagonal_x,
                curr2.lower_left_diagonal_x,
                curr2.upper_right_diagonal_x,
            );
            result[6] = curr1.normalize();
            result[5] = curr2.normalize();
        }
        curr1 = result[5];
        curr2 = result[4];
        if !(curr1.is_empty() || curr2.is_empty())
            && curr2.right_x_value(curr2.top_y).wrapping_sub(curr2.left_x)
                > curr1.upper_y_value(curr2.left_x).wrapping_sub(curr2.top_y)
        {
            curr2 = IntOctagon::new(
                curr2.left_x,
                curr2.bottom_y,
                curr2.right_x.max(curr1.right_x),
                curr1.top_y,
                curr1.upper_left_diagonal_x,
                curr2.lower_right_diagonal_x,
                curr2.lower_left_diagonal_x,
                curr1.upper_right_diagonal_x,
            );
            curr1 = IntOctagon::new(
                curr1.left_x,
                curr1.bottom_y,
                curr2.left_x,
                curr1.top_y,
                curr1.upper_left_diagonal_x,
                curr1.lower_right_diagonal_x,
                curr1.lower_left_diagonal_x,
                curr1.upper_right_diagonal_x,
            );
            result[5] = curr1.normalize();
            result[4] = curr2.normalize();
        }
        curr1 = result[4];
        curr2 = result[3];
        if !(curr1.is_empty() || curr2.is_empty())
            && curr1
                .right_x_value(curr1.bottom_y)
                .wrapping_sub(curr1.left_x)
                > curr1
                    .bottom_y
                    .wrapping_sub(curr2.lower_y_value(curr1.left_x))
        {
            curr1 = IntOctagon::new(
                curr1.left_x,
                curr2.bottom_y,
                curr2.right_x.max(curr1.right_x),
                curr1.top_y,
                curr1.upper_left_diagonal_x,
                curr2.lower_right_diagonal_x,
                curr2.lower_left_diagonal_x,
                curr1.upper_right_diagonal_x,
            );
            curr2 = IntOctagon::new(
                curr2.left_x,
                curr2.bottom_y,
                curr1.left_x,
                curr2.top_y,
                curr2.upper_left_diagonal_x,
                curr2.lower_right_diagonal_x,
                curr2.lower_left_diagonal_x,
                curr2.upper_right_diagonal_x,
            );
            result[4] = curr1.normalize();
            result[3] = curr2.normalize();
        }
        curr1 = result[3];
        curr2 = result[2];
        if !(curr1.is_empty() || curr2.is_empty())
            && curr2.top_y.wrapping_sub(curr2.lower_y_value(curr2.right_x))
                > curr1.right_x_value(curr2.top_y).wrapping_sub(curr2.right_x)
        {
            curr2 = IntOctagon::new(
                curr2.left_x,
                curr1.bottom_y.min(curr2.bottom_y),
                curr1.right_x,
                curr2.top_y,
                curr2.upper_left_diagonal_x,
                curr1.lower_right_diagonal_x,
                curr2.lower_left_diagonal_x,
                curr1.upper_right_diagonal_x,
            );
            curr1 = IntOctagon::new(
                curr1.left_x,
                curr2.top_y,
                curr1.right_x,
                curr1.top_y,
                curr1.upper_left_diagonal_x,
                curr1.lower_right_diagonal_x,
                curr1.lower_left_diagonal_x,
                curr1.upper_right_diagonal_x,
            );
            result[3] = curr1.normalize();
            result[2] = curr2.normalize();
        }
        curr1 = result[2];
        curr2 = result[1];
        if !(curr1.is_empty() || curr2.is_empty())
            && curr1.top_y.wrapping_sub(curr1.lower_y_value(curr1.left_x))
                > curr1.left_x.wrapping_sub(curr2.left_x_value(curr1.top_y))
        {
            curr1 = IntOctagon::new(
                curr2.left_x,
                curr1.bottom_y.min(curr2.bottom_y),
                curr1.right_x,
                curr1.top_y,
                curr2.upper_left_diagonal_x,
                curr1.lower_right_diagonal_x,
                curr2.lower_left_diagonal_x,
                curr1.upper_right_diagonal_x,
            );
            curr2 = IntOctagon::new(
                curr2.left_x,
                curr1.top_y,
                curr2.right_x,
                curr2.top_y,
                curr2.upper_left_diagonal_x,
                curr2.lower_right_diagonal_x,
                curr2.lower_left_diagonal_x,
                curr2.upper_right_diagonal_x,
            );
            result[2] = curr1.normalize();
            result[1] = curr2.normalize();
        }
        curr1 = result[1];
        curr2 = result[0];
        if !(curr1.is_empty() || curr2.is_empty())
            && curr2
                .right_x
                .wrapping_sub(curr2.left_x_value(curr2.bottom_y))
                > curr2
                    .bottom_y
                    .wrapping_sub(curr1.lower_y_value(curr2.right_x))
        {
            curr2 = IntOctagon::new(
                curr2.left_x.min(curr1.left_x),
                curr1.bottom_y,
                curr2.right_x,
                curr2.top_y,
                curr2.upper_left_diagonal_x,
                curr1.lower_right_diagonal_x,
                curr1.lower_left_diagonal_x,
                curr2.upper_right_diagonal_x,
            );
            curr1 = IntOctagon::new(
                curr2.right_x,
                curr1.bottom_y,
                curr1.right_x,
                curr1.top_y,
                curr1.upper_left_diagonal_x,
                curr1.lower_right_diagonal_x,
                curr1.lower_left_diagonal_x,
                curr1.upper_right_diagonal_x,
            );
            result[1] = curr1.normalize();
            result[0] = curr2.normalize();
        }
        result.to_vec()
    }
}

impl PolylineShapeOps for IntOctagon {
    fn border_line_count(&self) -> usize {
        8
    }

    fn corner(&self, no: usize) -> Point {
        Point::Int(self.int_corner(no))
    }

    /// # Panics
    /// If `no > 7`.
    fn border_line(&self, no: usize) -> Line {
        match no {
            0 => Line::from_coords(0, self.bottom_y, 1, self.bottom_y),
            1 => Line::from_coords(
                self.lower_right_diagonal_x,
                0,
                self.lower_right_diagonal_x.wrapping_add(1),
                1,
            ),
            2 => Line::from_coords(self.right_x, 0, self.right_x, 1),
            3 => Line::from_coords(
                self.upper_right_diagonal_x,
                0,
                self.upper_right_diagonal_x.wrapping_sub(1),
                1,
            ),
            4 => Line::from_coords(0, self.top_y, -1, self.top_y),
            5 => Line::from_coords(
                self.upper_left_diagonal_x,
                0,
                self.upper_left_diagonal_x.wrapping_sub(1),
                -1,
            ),
            6 => Line::from_coords(self.left_x, 0, self.left_x, -1),
            7 => Line::from_coords(
                self.lower_left_diagonal_x,
                0,
                self.lower_left_diagonal_x.wrapping_add(1),
                -1,
            ),
            _ => panic!("IntOctagon.borderLine: no out of range"),
        }
    }

    fn corner_is_bounded(&self, _no: usize) -> bool {
        true
    }

    fn is_empty(&self) -> bool {
        *self == IntOctagon::EMPTY
    }

    fn is_bounded(&self) -> bool {
        true
    }

    fn dimension(&self) -> i32 {
        if self.is_empty() {
            return -1;
        }
        if self.right_x > self.left_x
            && self.top_y > self.bottom_y
            && self.lower_right_diagonal_x > self.upper_left_diagonal_x
            && self.upper_right_diagonal_x > self.lower_left_diagonal_x
        {
            2
        } else if self.right_x == self.left_x && self.top_y == self.bottom_y {
            0
        } else {
            1
        }
    }

    fn bounding_box(&self) -> IntBox {
        IntBox::new(self.left_x, self.bottom_y, self.right_x, self.top_y)
    }

    fn bounding_octagon(&self) -> Option<IntOctagon> {
        Some(*self)
    }

    fn is_contained_in(&self, b: &IntBox) -> bool {
        self.left_x >= b.ll.x
            && self.bottom_y >= b.ll.y
            && self.right_x <= b.ur.x
            && self.top_y <= b.ur.y
    }
}

impl TileShapeOps for IntOctagon {
    fn get_id(&self) -> i32 {
        let mut result = self.left_x;
        for v in [
            self.right_x,
            self.bottom_y,
            self.top_y,
            self.lower_left_diagonal_x,
            self.upper_right_diagonal_x,
            self.upper_left_diagonal_x,
            self.lower_right_diagonal_x,
        ] {
            result = 31i32.wrapping_mul(result).wrapping_add(v);
        }
        result
    }

    fn is_int_box(&self) -> bool {
        self.lower_left_diagonal_x == self.left_x.wrapping_add(self.bottom_y)
            && self.lower_right_diagonal_x == self.right_x.wrapping_sub(self.bottom_y)
            && self.upper_right_diagonal_x == self.right_x.wrapping_add(self.top_y)
            && self.upper_left_diagonal_x == self.left_x.wrapping_sub(self.top_y)
    }

    fn is_int_octagon(&self) -> bool {
        true
    }

    fn border_line_index(&self, _line: &Line) -> Option<usize> {
        None
    }

    fn to_simplex(&self) -> Simplex {
        if self.is_empty() {
            return Simplex::empty();
        }
        let lines: Vec<Line> = (0..8).map(|i| self.border_line(i)).collect();
        Simplex::new(lines).remove_redundant_lines()
    }

    fn to_tile_shape(&self) -> TileShape {
        TileShape::IntOctagon(*self)
    }

    fn max_width(&self) -> f64 {
        let width1 = f64::from(
            self.right_x
                .wrapping_sub(self.left_x)
                .max(self.top_y.wrapping_sub(self.bottom_y)),
        );
        let width2 = f64::from(
            self.upper_right_diagonal_x
                .wrapping_sub(self.lower_left_diagonal_x)
                .max(
                    self.lower_right_diagonal_x
                        .wrapping_sub(self.upper_left_diagonal_x),
                ),
        );
        crate::float_point::java_max(width1, width2 / SQRT2)
    }

    fn min_width(&self) -> f64 {
        let width1 = f64::from(
            self.right_x
                .wrapping_sub(self.left_x)
                .min(self.top_y.wrapping_sub(self.bottom_y)),
        );
        let width2 = f64::from(
            self.upper_right_diagonal_x
                .wrapping_sub(self.lower_left_diagonal_x)
                .min(
                    self.lower_right_diagonal_x
                        .wrapping_sub(self.upper_left_diagonal_x),
                ),
        );
        crate::float_point::java_min(width1, width2 / SQRT2)
    }

    fn area(&self) -> f64 {
        let lx = self.left_x;
        let ly = self.bottom_y;
        let rx = self.right_x;
        let uy = self.top_y;
        let ulx = self.upper_left_diagonal_x;
        let lrx = self.lower_right_diagonal_x;
        let llx = self.lower_left_diagonal_x;
        let urx = self.upper_right_diagonal_x;
        let f = f64::from;
        let mut result = f(llx.wrapping_sub(ly)) * f(ly.wrapping_sub(llx).wrapping_add(lx));
        result += f(lrx.wrapping_add(ly)) * f(rx.wrapping_sub(lrx).wrapping_sub(ly));
        result += f(rx)
            * f(urx
                .wrapping_sub(2i32.wrapping_mul(rx))
                .wrapping_sub(ly)
                .wrapping_add(uy)
                .wrapping_add(lrx));
        result += f(urx.wrapping_sub(uy)) * f(uy.wrapping_sub(urx).wrapping_add(rx));
        result += f(ulx.wrapping_add(uy)) * f(lx.wrapping_sub(ulx).wrapping_sub(uy));
        result += f(lx)
            * f(llx
                .wrapping_sub(2i32.wrapping_mul(lx))
                .wrapping_sub(uy)
                .wrapping_add(ly)
                .wrapping_add(ulx));
        0.5 * result.abs()
    }

    fn contains_float(&self, point: FloatPoint) -> bool {
        if f64::from(self.left_x) > point.x
            || f64::from(self.bottom_y) > point.y
            || f64::from(self.right_x) < point.x
            || f64::from(self.top_y) < point.y
        {
            return false;
        }
        let tmp1 = point.x - point.y;
        let tmp2 = point.x + point.y;
        f64::from(self.upper_left_diagonal_x) <= tmp1
            && f64::from(self.lower_right_diagonal_x) >= tmp1
            && f64::from(self.lower_left_diagonal_x) <= tmp2
            && f64::from(self.upper_right_diagonal_x) >= tmp2
    }
}

impl fmt::Display for IntOctagon {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "IntOctagon(leftX={}, bottomY={}, rightX={}, topY={}, upperLeftDiagonalX={}, \
             lowerRightDiagonalX={}, lowerLeftDiagonalX={}, upperRightDiagonalX={})",
            self.left_x,
            self.bottom_y,
            self.right_x,
            self.top_y,
            self.upper_left_diagonal_x,
            self.lower_right_diagonal_x,
            self.lower_left_diagonal_x,
            self.upper_right_diagonal_x
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Ported from IntOctagonTest.java. Java checks reference identity (`assertSame`);
    // with value semantics the equivalent is equality with the input.
    #[test]
    fn normalize_returns_same_value_when_bounds_are_already_tight() {
        let octagon = IntOctagon::new(0, 0, 10, 10, -10, 10, 0, 20);
        assert_eq!(octagon, octagon.normalize());
        assert!(octagon.is_normalized());
    }

    #[test]
    fn normalize_tightens_bounds() {
        let octagon = IntOctagon::new(0, 0, 10, 20, -10, 10, 0, 20);
        let normalized = octagon.normalize();
        assert_ne!(octagon, normalized);
        assert_eq!(15, normalized.top_y);
    }

    #[test]
    fn corners_and_border_lines_of_a_square_octagon() {
        let oct = IntBox::new(0, 0, 10, 10).to_int_octagon();
        assert!(oct.is_int_box());
        assert_eq!(oct.dimension(), 2);
        assert_eq!(oct.area(), 100.0);
        assert_eq!(oct.int_corner(0), IntPoint::new(0, 0));
        assert_eq!(oct.int_corner(3), IntPoint::new(10, 10));
        assert!(oct.contains(&Point::Int(IntPoint::new(5, 5))));
        assert!(oct.contains_float(FloatPoint::new(10.0, 10.0)));
        assert!(!oct.contains_float(FloatPoint::new(10.5, 10.0)));
        for i in 0..8 {
            assert_eq!(
                oct.border_line(i).side_of_int(IntPoint::new(5, 5)),
                Side::OnTheRight
            );
        }
        assert!(IntOctagon::EMPTY.is_empty());
        assert_eq!(
            IntOctagon::new(5, 5, 0, 0, 0, 0, 0, 0).normalize(),
            IntOctagon::EMPTY
        );
    }

    /// N06.4, `octagon_box_cutout_branch3`: an octagon in the lower middle of a box with
    /// its upper right corner cut. Branch 3 (`IntOctagon.java:1206-1223`) widens the
    /// upper box to the right edge; Java keeps the upper right octagon's top, so the two
    /// overlap, and the fix trims the octagon to the box's bottom.
    #[test]
    fn box_cutout_branch3_switch_both_behaviours() {
        let d = IntBox::new(0, 0, 100, 100);
        let hole = IntOctagon::new(40, 10, 60, 30, 10, 50, 50, 80).normalize();
        let overlap_and_total = |quirks: SplitQuirks| {
            let mut reach = SplitReach::default();
            let pieces: Vec<TileShape> = hole
                .cutout_from_box_counted(&d, quirks, &mut reach)
                .into_iter()
                .filter(|p| p.dimension() == 2)
                .map(TileShape::IntOctagon)
                .collect();
            let mut overlap = 0.0;
            for (i, a) in pieces.iter().enumerate() {
                for b in &pieces[i + 1..] {
                    let c = a.intersection(b);
                    if c.dimension() == 2 {
                        overlap += c.area();
                    }
                }
            }
            let total: f64 = pieces.iter().map(TileShapeOps::area).sum();
            (overlap, total, reach.octagon_box_cutout_branch3)
        };
        let rest = 10_000.0 - hole.area();
        let (overlap, total, reached) = overlap_and_total(SplitQuirks::JAVA);
        assert_eq!(reached, 1);
        assert!(overlap > 0.0 && total > rest, "{overlap} {total} {rest}");
        let fixed = SplitQuirks {
            octagon_box_cutout_branch3: false,
            ..SplitQuirks::JAVA
        };
        assert_eq!(overlap_and_total(fixed), (0.0, rest, 1));
    }

    #[test]
    fn octagon_cutout_pieces_cover_the_difference() {
        let outer = IntOctagon::new(0, 0, 100, 100, -100, 100, 0, 200);
        let inner = IntOctagon::new(40, 40, 60, 60, -10, 10, 90, 110).normalize();
        let pieces = inner.cutout_from_octagon(&outer);
        assert_eq!(pieces.len(), 8);
        let total: f64 = pieces
            .iter()
            .filter(|p| !p.is_empty())
            .map(|p| p.area())
            .sum();
        assert!((total + inner.area() - outer.area()).abs() < 1e-6);
        let box_pieces = inner.cutout_from_box(&IntBox::new(0, 0, 100, 100));
        assert_eq!(box_pieces.len(), 8);
        let total: f64 = box_pieces
            .iter()
            .filter(|p| !p.is_empty())
            .map(|p| p.area())
            .sum();
        assert!((total + inner.area() - 10_000.0).abs() < 1e-6);
    }

    #[test]
    fn border_points_and_projections() {
        let oct = IntOctagon::new(0, 0, 100, 100, -100, 100, 0, 200);
        let p = IntPoint::new(50, 10);
        assert_eq!(
            oct.border_point(p, FortyfiveDegreeDirection::Down),
            IntPoint::new(50, 0)
        );
        let proj = oct.nearest_border_projections(p, 2);
        assert_eq!(proj[0], IntPoint::new(50, 0));
        assert_eq!(proj.len(), 2);
    }
}
