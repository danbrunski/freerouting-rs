//! Port of `app.freerouting.geometry.planar.FloatPoint`.

use std::fmt;

use crate::direction::Direction;
use crate::float_line::FloatLine;
use crate::jmath::{d2i, java_round, rint};
use crate::limits::{INT_MAX_F64, INT_MIN_F64};
use crate::line::Line;
use crate::point::IntPoint;
use crate::shapes::{IntBox, IntOctagon};
use crate::side::Side;

/// A point in the plane as a tuple of `f64`. Arithmetic with doubles is in general not
/// exact, so `FloatPoint` is not a [`Point`](crate::Point).
///
/// Java: `app.freerouting.geometry.planar.FloatPoint`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FloatPoint {
    pub x: f64,
    pub y: f64,
}

impl FloatPoint {
    /// The zero point.
    pub const ZERO: FloatPoint = FloatPoint { x: 0.0, y: 0.0 };

    /// Creates a point from two doubles.
    pub const fn new(x: f64, y: f64) -> FloatPoint {
        FloatPoint { x, y }
    }

    /// Creates a point from an [`IntPoint`].
    pub fn from_int_point(p: IntPoint) -> FloatPoint {
        FloatPoint::new(f64::from(p.x), f64::from(p.y))
    }

    /// Calculates the smallest `IntOctagon` containing all the input points.
    ///
    /// Java: `FloatPoint.boundingOctagon(FloatPoint[])`.
    pub fn bounding_octagon_of(points: &[FloatPoint]) -> IntOctagon {
        let mut min_x = INT_MAX_F64;
        let mut min_y = INT_MAX_F64;
        let mut max_x = INT_MIN_F64;
        let mut max_y = INT_MIN_F64;
        let mut min_ul = INT_MAX_F64;
        let mut max_lr = INT_MIN_F64;
        let mut min_ll = INT_MAX_F64;
        let mut max_ur = INT_MIN_F64;
        for current in points {
            min_x = java_min(min_x, current.x);
            min_y = java_min(min_y, current.y);
            max_x = java_max(max_x, current.x);
            max_y = java_max(max_y, current.y);
            let tmp = current.x - current.y;
            min_ul = java_min(min_ul, tmp);
            max_lr = java_max(max_lr, tmp);
            let tmp = current.x + current.y;
            min_ll = java_min(min_ll, tmp);
            max_ur = java_max(max_ur, tmp);
        }
        IntOctagon::new(
            d2i(min_x.floor()),
            d2i(min_y.floor()),
            d2i(max_x.ceil()),
            d2i(max_y.ceil()),
            d2i(min_ul.floor()),
            d2i(max_lr.ceil()),
            d2i(min_ll.floor()),
            d2i(max_ur.ceil()),
        )
    }

    /// Square of the distance to the zero point.
    pub fn size_square(&self) -> f64 {
        self.x * self.x + self.y * self.y
    }

    /// Distance to the zero point.
    pub fn size(&self) -> f64 {
        self.size_square().sqrt()
    }

    /// Square of the distance to `other`.
    pub fn distance_square(&self, other: FloatPoint) -> f64 {
        let dx = other.x - self.x;
        let dy = other.y - self.y;
        dx * dx + dy * dy
    }

    /// Distance to `other`.
    pub fn distance(&self, other: FloatPoint) -> f64 {
        self.distance_square(other).sqrt()
    }

    /// Weighted distance to `other`.
    pub fn weighted_distance(
        &self,
        other: FloatPoint,
        horizontal_weight: f64,
        vertical_weight: f64,
    ) -> f64 {
        let mut delta_x = self.x - other.x;
        let mut delta_y = self.y - other.y;
        delta_x *= horizontal_weight;
        delta_y *= vertical_weight;
        (delta_x * delta_x + delta_y * delta_y).sqrt()
    }

    /// Rounds the coordinates to an [`IntPoint`] (`(int) Math.round`).
    pub fn round(&self) -> IntPoint {
        IntPoint::new(java_round(self.x) as i32, java_round(self.y) as i32)
    }

    /// Rounds this point so that, if it is on the right side of any directed line with
    /// direction `dir`, the result is also on the right side.
    pub fn round_to_the_right(&self, dir: &Direction) -> IntPoint {
        let direction_vector = dir.get_vector().to_float();
        let rounded_x = if direction_vector.y > 0.0 {
            d2i(self.x.ceil())
        } else if direction_vector.y < 0.0 {
            d2i(self.x.floor())
        } else {
            java_round(self.x) as i32
        };
        let rounded_y = if direction_vector.x > 0.0 {
            d2i(self.y.floor())
        } else if direction_vector.x < 0.0 {
            d2i(self.y.ceil())
        } else {
            java_round(self.y) as i32
        };
        IntPoint::new(rounded_x, rounded_y)
    }

    /// Rounds so that x is a multiple of `horizontal_grid` and y of `vertical_grid`.
    pub fn round_to_grid(&self, horizontal_grid: i32, vertical_grid: i32) -> IntPoint {
        let rounded_x = if horizontal_grid > 0 {
            rint(self.x / f64::from(horizontal_grid)) * f64::from(horizontal_grid)
        } else {
            self.x
        };
        let rounded_y = if vertical_grid > 0 {
            rint(self.y / f64::from(vertical_grid)) * f64::from(vertical_grid)
        } else {
            self.y
        };
        IntPoint::new(d2i(rounded_x), d2i(rounded_y))
    }

    /// Rounds this point so that, if it is on the left side of any directed line with
    /// direction `dir`, the result is also on the left side.
    pub fn round_to_the_left(&self, dir: &Direction) -> IntPoint {
        let direction_vector = dir.get_vector().to_float();
        let rounded_x = if direction_vector.y > 0.0 {
            d2i(self.x.floor())
        } else if direction_vector.y < 0.0 {
            d2i(self.x.ceil())
        } else {
            java_round(self.x) as i32
        };
        let rounded_y = if direction_vector.x > 0.0 {
            d2i(self.y.ceil())
        } else if direction_vector.x < 0.0 {
            d2i(self.y.floor())
        } else {
            java_round(self.y) as i32
        };
        IntPoint::new(rounded_x, rounded_y)
    }

    /// Adds the coordinates of `other`.
    pub fn add(&self, other: FloatPoint) -> FloatPoint {
        FloatPoint::new(self.x + other.x, self.y + other.y)
    }

    /// Subtracts the coordinates of `other`.
    pub fn subtract(&self, other: FloatPoint) -> FloatPoint {
        FloatPoint::new(self.x - other.x, self.y - other.y)
    }

    /// Approximation of the perpendicular projection of this point onto `line`.
    pub fn projection_approx(&self, line: &Line) -> FloatPoint {
        let float_line = FloatLine::new(line.a.to_float(), line.b.to_float());
        float_line.perpendicular_projection(*self)
    }

    /// Scalar product of `(p1 - self)` and `(p2 - self)`.
    pub fn scalar_product(&self, p1: FloatPoint, p2: FloatPoint) -> f64 {
        let dx1 = p1.x - self.x;
        let dx2 = p2.x - self.x;
        let dy1 = p1.y - self.y;
        let dy2 = p2.y - self.y;
        dx1 * dx2 + dy1 * dy2
    }

    /// A point on the line from zero to this point with distance `new_size` from zero.
    pub fn change_size(&self, new_size: f64) -> FloatPoint {
        if self.x == 0.0 && self.y == 0.0 {
            return *self;
        }
        let length = (self.x * self.x + self.y * self.y).sqrt();
        FloatPoint::new((self.x * new_size) / length, (self.y * new_size) / length)
    }

    /// A point on the line from this point to `to_point` with distance `new_length` from
    /// this point.
    pub fn change_length(&self, to_point: FloatPoint, new_length: f64) -> FloatPoint {
        let dx = to_point.x - self.x;
        let dy = to_point.y - self.y;
        if dx == 0.0 && dy == 0.0 {
            return to_point;
        }
        let length = (dx * dx + dy * dy).sqrt();
        FloatPoint::new(
            self.x + (dx * new_length) / length,
            self.y + (dy * new_length) / length,
        )
    }

    /// The middle point between this point and `to_point`.
    pub fn middle_point(&self, to_point: FloatPoint) -> FloatPoint {
        FloatPoint::new(0.5 * (self.x + to_point.x), 0.5 * (self.y + to_point.y))
    }

    /// `OnTheLeft` if this point is on the left of the line from `p1` to `p2`, `OnTheRight`
    /// if on the right. Collinearity is only reported for an exactly zero determinant.
    pub fn side_of(&self, p1: FloatPoint, p2: FloatPoint) -> Side {
        let d21x = p2.x - p1.x;
        let d21y = p2.y - p1.y;
        let d01x = self.x - p1.x;
        let d01y = self.y - p1.y;
        let determinant = d21x * d01y - d21y * d01x;
        Side::of(determinant)
    }

    /// Rotates this point by `angle` (radians) around `pole`.
    pub fn rotate(&self, angle: f64, pole: FloatPoint) -> FloatPoint {
        if angle == 0.0 {
            return *self;
        }
        let dx = self.x - pole.x;
        let dy = self.y - pole.y;
        let sin_angle = angle.sin();
        let cos_angle = angle.cos();
        let new_dx = dx * cos_angle - dy * sin_angle;
        let new_dy = dx * sin_angle + dy * cos_angle;
        FloatPoint::new(pole.x + new_dx, pole.y + new_dy)
    }

    /// Turns this point by `factor` times 90 degrees around zero.
    pub fn turn_90_degree(&self, factor: i32) -> FloatPoint {
        match factor.rem_euclid(4) {
            0 => FloatPoint::new(self.x, self.y),
            1 => FloatPoint::new(-self.y, self.x),
            2 => FloatPoint::new(-self.x, -self.y),
            _ => FloatPoint::new(self.y, -self.x),
        }
    }

    /// Turns this point by `factor` times 90 degrees around `pole`.
    pub fn turn_90_degree_around(&self, factor: i32, pole: FloatPoint) -> FloatPoint {
        let v = self.subtract(pole).turn_90_degree(factor);
        pole.add(v)
    }

    /// Checks whether this point lies in the box spanned by `p1` and `p2` with `tolerance`.
    pub fn is_contained_in_box(&self, p1: FloatPoint, p2: FloatPoint, tolerance: f64) -> bool {
        let (min_x, max_x) = if p1.x < p2.x {
            (p1.x, p2.x)
        } else {
            (p2.x, p1.x)
        };
        if self.x < min_x - tolerance || self.x > max_x + tolerance {
            return false;
        }
        let (min_y, max_y) = if p1.y < p2.y {
            (p1.y, p2.y)
        } else {
            (p2.y, p1.y)
        };
        self.y >= min_y - tolerance && self.y <= max_y + tolerance
    }

    /// The smallest `IntBox` containing this point.
    pub fn bounding_box(&self) -> IntBox {
        IntBox::from_points(
            IntPoint::new(d2i(self.x.floor()), d2i(self.y.floor())),
            IntPoint::new(d2i(self.x.ceil()), d2i(self.y.ceil())),
        )
    }

    /// The touching points of the tangents from this point to the circle around `to_point`
    /// with radius `distance`. Empty if this point is inside the circle.
    pub fn tangential_points(&self, to_point: FloatPoint, distance: f64) -> Vec<FloatPoint> {
        let dx = (self.x - to_point.x).abs();
        let dy = (self.y - to_point.y).abs();
        let situation_turned = dy > dx;
        let (pole, circle_center) = if situation_turned {
            (
                FloatPoint::new(-self.y, self.x),
                FloatPoint::new(-to_point.y, to_point.x),
            )
        } else {
            (*self, to_point)
        };
        let dx = pole.x - circle_center.x;
        let dy = pole.y - circle_center.y;
        let dx_square = dx * dx;
        let dy_square = dy * dy;
        let dist_square = dx_square + dy_square;
        let radius_square = distance * distance;
        let discriminant = radius_square * dy_square - (radius_square - dx_square) * dist_square;
        if discriminant <= 0.0 {
            return Vec::new();
        }
        let square_root = discriminant.sqrt();
        let a1 = radius_square * dy;
        let dy1 = (a1 + distance * square_root) / dist_square;
        let dy2 = (a1 - distance * square_root) / dist_square;
        let first_y = dy1 + circle_center.y;
        let first_x = (radius_square - dy * dy1) / dx + circle_center.x;
        let second_y = dy2 + circle_center.y;
        let second_x = (radius_square - dy * dy2) / dx + circle_center.x;
        if situation_turned {
            vec![
                FloatPoint::new(first_y, -first_x),
                FloatPoint::new(second_y, -second_x),
            ]
        } else {
            vec![
                FloatPoint::new(first_x, first_y),
                FloatPoint::new(second_x, second_y),
            ]
        }
    }

    /// The left tangential point from this point to the circle around `to_point`; `None`
    /// if this point is inside the circle.
    pub fn left_tangential_point(&self, to_point: FloatPoint, distance: f64) -> Option<FloatPoint> {
        let tangent_points = self.tangential_points(to_point, distance);
        if tangent_points.len() < 2 {
            return None;
        }
        if to_point.side_of(*self, tangent_points[0]) == Side::OnTheRight {
            Some(tangent_points[0])
        } else {
            Some(tangent_points[1])
        }
    }

    /// The right tangential point from this point to the circle around `to_point`; `None`
    /// if this point is inside the circle.
    pub fn right_tangential_point(
        &self,
        to_point: FloatPoint,
        distance: f64,
    ) -> Option<FloatPoint> {
        let tangent_points = self.tangential_points(to_point, distance);
        if tangent_points.len() < 2 {
            return None;
        }
        if to_point.side_of(*self, tangent_points[0]) == Side::OnTheLeft {
            Some(tangent_points[0])
        } else {
            Some(tangent_points[1])
        }
    }

    /// Centre of the circle through this point, `p1` and `p2`.
    pub fn circle_center(&self, p1: FloatPoint, p2: FloatPoint) -> FloatPoint {
        let slope1 = (p1.y - self.y) / (p1.x - self.x);
        let slope2 = (p2.y - p1.y) / (p2.x - p1.x);
        let center_x = (slope1 * slope2 * (self.y - p2.y) + slope2 * (self.x + p1.x)
            - slope1 * (p1.x + p2.x))
            / (2.0 * (slope2 - slope1));
        let center_y = (0.5 * (self.x + p1.x) - center_x) / slope1 + 0.5 * (self.y + p1.y);
        FloatPoint::new(center_x, center_y)
    }

    /// True if this point is inside the circle through `p1`, `p2` and `p3`.
    pub fn inside_circle(&self, p1: FloatPoint, p2: FloatPoint, p3: FloatPoint) -> bool {
        let center = p1.circle_center(p2, p3);
        let radius_square = center.distance_square(p1);
        self.distance_square(center) < radius_square - 1.0
    }

    /// Java `toString(Locale, fractionDigits, padding)` for an English locale:
    /// `"X <x>   Y <-y>"` with fixed fraction digits, right-aligned to `padding`.
    pub fn to_padded_string(&self, fraction_digits: usize, padding: usize) -> String {
        let fx = format_grouped(self.x, fraction_digits, fraction_digits);
        let fy = format_grouped(-self.y, fraction_digits, fraction_digits);
        format!("X {fx:>padding$}   Y {fy:>padding$}")
    }
}

/// `Math.min(double, double)`: `NaN` if either argument is `NaN`, `-0.0 < 0.0`.
pub(crate) fn java_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == 0.0 && b == 0.0 {
        if a.is_sign_negative() { a } else { b }
    } else if a <= b {
        a
    } else {
        b
    }
}

/// `Math.max(double, double)`: `NaN` if either argument is `NaN`, `0.0 > -0.0`.
pub(crate) fn java_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == 0.0 && b == 0.0 {
        if a.is_sign_positive() { a } else { b }
    } else if a >= b {
        a
    } else {
        b
    }
}

/// Formats like `NumberFormat.getInstance(Locale.ENGLISH)` with the given fraction digits:
/// grouping separators and half-even rounding of the decimal value.
pub(crate) fn format_grouped(value: f64, min_fraction: usize, max_fraction: usize) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "∞" } else { "-∞" }.to_string();
    }
    let formatted = format!("{:.*}", max_fraction, value.abs());
    let (int_part, frac_part) = match formatted.split_once('.') {
        Some((i, f)) => (i.to_string(), f.to_string()),
        None => (formatted.clone(), String::new()),
    };
    let mut frac = frac_part;
    while frac.len() > min_fraction && frac.ends_with('0') {
        frac.pop();
    }
    let mut grouped = String::new();
    for (i, c) in int_part.chars().enumerate() {
        if i > 0 && (int_part.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    // Like `DecimalFormat`, a negative value that rounds to zero (and -0.0) keeps its sign.
    let negative = value.is_sign_negative();
    let mut result = String::new();
    if negative {
        result.push('-');
    }
    result.push_str(&grouped);
    if !frac.is_empty() {
        result.push('.');
        result.push_str(&frac);
    }
    result
}

impl fmt::Display for FloatPoint {
    /// Java `toString()`: `"(x , y)"` with at most 4 fraction digits (English locale).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "({} , {})",
            format_grouped(self.x, 0, 4),
            format_grouped(self.y, 0, 4)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding_helpers() {
        let p = FloatPoint::new(2.5, -2.5);
        assert_eq!(p.round(), IntPoint::new(3, -2));
        assert_eq!(
            FloatPoint::new(14.0, -6.0).round_to_grid(4, 4),
            IntPoint::new(16, -8)
        );
        let right = Direction::RIGHT;
        assert_eq!(
            FloatPoint::new(1.5, 1.5).round_to_the_right(&right),
            IntPoint::new(2, 1)
        );
        assert_eq!(
            FloatPoint::new(1.5, 1.5).round_to_the_left(&right),
            IntPoint::new(2, 2)
        );
    }

    #[test]
    fn tangents_and_sides() {
        let from = FloatPoint::new(10.0, 0.0);
        let pts = from.tangential_points(FloatPoint::ZERO, 5.0);
        assert_eq!(pts.len(), 2);
        for p in &pts {
            assert!((p.size() - 5.0).abs() < 1e-9);
        }
        assert!(
            FloatPoint::new(1.0, 0.0)
                .tangential_points(FloatPoint::ZERO, 5.0)
                .is_empty()
        );
        let left = from.left_tangential_point(FloatPoint::ZERO, 5.0).unwrap();
        assert_eq!(FloatPoint::ZERO.side_of(from, left), Side::OnTheRight);
        assert_eq!(
            FloatPoint::new(0.0, 1.0).side_of(FloatPoint::ZERO, FloatPoint::new(1.0, 0.0)),
            Side::OnTheLeft
        );
    }

    #[test]
    fn display_like_java() {
        assert_eq!(FloatPoint::new(1234.5, -2.0).to_string(), "(1,234.5 , -2)");
        assert_eq!(FloatPoint::new(0.123_456, 0.0).to_string(), "(0.1235 , 0)");
        assert_eq!(
            FloatPoint::new(1.0, 2.0).to_padded_string(2, 8),
            "X     1.00   Y    -2.00"
        );
    }

    #[test]
    fn java_min_max_handle_nan() {
        assert!(java_min(f64::NAN, 1.0).is_nan());
        assert!(java_max(1.0, f64::NAN).is_nan());
        assert_eq!(java_min(1.0, 2.0), 1.0);
        assert_eq!(java_max(1.0, 2.0), 2.0);
    }
}
