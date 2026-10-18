//! Directed lines through two integer points.

use std::sync::Arc;

use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{Signed, ToPrimitive, Zero};

use crate::direction::{Direction, IntDirection};
use crate::float_point::FloatPoint;
use crate::jmath::round_to_i32;
use crate::limits::CRIT_INT;
use crate::point::{IntPoint, Point, RationalPoint};
use crate::shapes::TileShapeOps;
use crate::side::Side;
use crate::signum::Signum;
use crate::vector::Vector;

/// A directed line in the plane through the two points `a` and `b`.
///
/// Port of `app.freerouting.geometry.planar.Line`. The Java class stores two general
/// `Point`s, but nearly all of its methods cast them to `IntPoint` (and log a warning
/// otherwise), so this port stores `IntPoint`s directly. The direction is computed
/// eagerly instead of lazily.
///
/// Equality is the geometric equality of Java's `Line.equals`: two lines are equal if
/// they contain the same points and point in the same direction.
#[derive(Debug, Clone, Copy)]
pub struct Line {
    pub(crate) a: IntPoint,
    pub(crate) b: IntPoint,
    dir: IntDirection,
}

impl Line {
    /// Creates the directed line from `a` to `b` (Java `new Line(Point, Point)`).
    pub fn new(a: IntPoint, b: IntPoint) -> Line {
        let dir = b.difference_by_int(a).to_normalized_direction();
        Line { a, b, dir }
    }

    /// Creates a directed line from four integer coordinates.
    pub fn from_coords(ax: i32, ay: i32, bx: i32, by: i32) -> Line {
        Line::new(IntPoint::new(ax, ay), IntPoint::new(bx, by))
    }

    /// Creates a directed line from two general points.
    ///
    /// # Panics
    /// If one of the points is rational (Java only implements lines through `IntPoint`s
    /// and fails with a `ClassCastException` later).
    pub fn from_points(a: &Point, b: &Point) -> Line {
        Line::new(a.expect_int(), b.expect_int())
    }

    /// Creates the line through `a` with direction `dir`, keeping `dir` as given
    /// (Java `new Line(Point, Direction)`).
    pub fn from_point_and_direction(a: IntPoint, dir: IntDirection) -> Line {
        let b = a.translate_by_int(dir.get_vector());
        Line { a, b, dir }
    }

    /// Creates the line through `a` with direction `dir`; the stored direction is
    /// recomputed from the two points (Java `Line.getInstance`).
    pub fn get_instance(a: IntPoint, dir: IntDirection) -> Line {
        let b = a.translate_by_int(dir.get_vector());
        Line::new(a, b)
    }

    /// The line with direction `dir` through integer points that is nearest to `point`
    /// with `point` on its left (`point_on_left`) or on its right, anchored at its integer
    /// point nearest to `point` (N06.14, `simplex_octagon_half_corner`).
    ///
    /// A line through a point that is not an integer point may contain no integer point
    /// at all; the two nearest lines through integer points on either side lie at most
    /// `1 / |dir|` apart. A line through `point` that contains integer points is returned
    /// on both sides. Exact: `dir.y * x - dir.x * y` is rounded in `BigInt` at `point` to
    /// the next value an integer point takes, and the anchor solves the equation.
    /// `None` for the zero direction, a point at infinity, or an anchor outside the
    /// `i32` range.
    pub fn nearest_through_integer_points(
        point: &Point,
        dir: IntDirection,
        point_on_left: bool,
    ) -> Option<Line> {
        let (px, py, pz) = match point {
            Point::Int(p) => return Some(Line::from_point_and_direction(*p, dir)),
            Point::Rational(r) => (r.x().clone(), r.y().clone(), r.z().clone()),
        };
        let (dx, dy) = (BigInt::from(dir.x), BigInt::from(dir.y));
        // At integer points `f = dy * x - dx * y` takes the multiples of `g`:
        // `dy * u - dx * v = g`.
        let ext = dy.extended_gcd(&-&dx);
        let (g, u, v) = if ext.gcd.is_negative() {
            (-ext.gcd, -ext.x, -ext.y)
        } else {
            (ext.gcd, ext.x, ext.y)
        };
        if g.is_zero() || pz.is_zero() {
            return None;
        }
        // `point` is on the left of the line `f = g * m` exactly when `f(point) < g * m`.
        let f_num: BigInt = &dy * &px - &dx * &py;
        let f_den: BigInt = &pz * &g;
        let m = if point_on_left {
            -((-f_num).div_floor(&f_den))
        } else {
            f_num.div_floor(&f_den)
        };
        let (x0, y0) = (u * &m, v * &m);
        // Along the line the integer points step by `dir / g`; take the one nearest to
        // `point`, rounding half up.
        let (sx, sy) = (&dx / &g, &dy / &g);
        let along: BigInt = (&px - &pz * &x0) * &sx + (&py - &pz * &y0) * &sy;
        let step_den: BigInt = &pz * (&sx * &sx + &sy * &sy) * 2;
        let t_num: BigInt = along * 2 + &step_den / 2;
        let t = t_num.div_floor(&step_den);
        let x = (x0 + &t * &sx).to_i32()?;
        let y = (y0 + t * sy).to_i32()?;
        Some(Line::from_point_and_direction(IntPoint::new(x, y), dir))
    }

    /// The first defining point.
    pub fn a(&self) -> IntPoint {
        self.a
    }

    /// The second defining point.
    pub fn b(&self) -> IntPoint {
        self.b
    }

    /// An id derived from the defining points.
    pub fn get_id(&self) -> i32 {
        31i32
            .wrapping_mul(self.a.get_id())
            .wrapping_add(self.b.get_id())
    }

    /// True if this and `other` define the same directed line. Fast check computed in
    /// doubles.
    pub fn fast_equals(&self, other: &Line) -> bool {
        let dx1 = f64::from(other.a.x.wrapping_sub(self.a.x));
        let dy1 = f64::from(other.a.y.wrapping_sub(self.a.y));
        let dx2 = f64::from(self.b.x.wrapping_sub(self.a.x));
        let dy2 = f64::from(self.b.y.wrapping_sub(self.a.y));
        let det = dx1 * dy2 - dx2 * dy1;
        if det != 0.0 {
            return false;
        }
        self.dir == other.dir
    }

    /// The direction of this directed line.
    pub fn direction(&self) -> Direction {
        Direction::Int(self.dir)
    }

    /// The direction of this directed line as an `IntDirection`.
    pub fn int_direction(&self) -> IntDirection {
        self.dir
    }

    /// `OnTheLeft` if this line is on the left of `point`, `OnTheRight` if on the right,
    /// `Collinear` if the line contains `point`.
    pub fn side_of(&self, point: &Point) -> Side {
        point.side_of_line(self).negate()
    }

    /// [`Line::side_of`] for an integer point.
    pub fn side_of_int(&self, point: IntPoint) -> Side {
        point.side_of_line(self).negate()
    }

    /// `Collinear` if `point` is on the line within `tolerance`; otherwise the side of
    /// `point` on which this line lies.
    pub fn side_of_float_tol(&self, point: FloatPoint, tolerance: f64) -> Side {
        let det = f64::from(self.b.y.wrapping_sub(self.a.y)) * (point.x - f64::from(self.a.x))
            - f64::from(self.b.x.wrapping_sub(self.a.x)) * (point.y - f64::from(self.a.y));
        if det - tolerance > 0.0 {
            Side::OnTheLeft
        } else if det + tolerance < 0.0 {
            Side::OnTheRight
        } else {
            Side::Collinear
        }
    }

    /// [`Line::side_of_float_tol`] with tolerance 0.
    pub fn side_of_float(&self, point: FloatPoint) -> Side {
        self.side_of_float_tol(point, 0.0)
    }

    /// The side of the intersection of `p1` and `p2` on which this line lies;
    /// `Collinear` if all three lines meet in one point.
    pub fn side_of_intersection(&self, p1: &Line, p2: &Line) -> Side {
        let intersection_approx = p1.intersection_approx(p2);
        let result = self.side_of_float_tol(intersection_approx, 1.0);
        if result == Side::Collinear {
            let intersection = p1.intersection(p2);
            return self.side_of(&intersection);
        }
        result
    }

    /// True if no corner of `tile` is on the right of this line.
    pub fn is_on_the_left<T: TileShapeOps + ?Sized>(&self, tile: &T) -> bool {
        (0..tile.border_line_count()).all(|i| self.side_of(&tile.corner(i)) != Side::OnTheRight)
    }

    /// True if no corner of `tile` is on the left of this line.
    pub fn is_on_the_right<T: TileShapeOps + ?Sized>(&self, tile: &T) -> bool {
        (0..tile.border_line_count()).all(|i| self.side_of(&tile.corner(i)) != Side::OnTheLeft)
    }

    /// Signed distance of this line from `point`; positive if the line is on the left
    /// of `point`.
    pub fn signed_distance(&self, point: FloatPoint) -> f64 {
        let dx = f64::from(self.b.x.wrapping_sub(self.a.x));
        let dy = f64::from(self.b.y.wrapping_sub(self.a.y));
        let det = dy * (point.x - f64::from(self.a.x)) - dx * (point.y - f64::from(self.a.y));
        let length = (dx * dx + dy * dy).sqrt();
        det / length
    }

    /// True if the two lines contain the same points (possibly with opposite
    /// directions).
    pub fn overlaps(&self, other: &Line) -> bool {
        self.side_of_int(other.a) == Side::Collinear && self.side_of_int(other.b) == Side::Collinear
    }

    /// The line with the same points and opposite direction.
    pub fn opposite(&self) -> Line {
        Line::new(self.b, self.a)
    }

    /// The intersection point of the two lines; an infinite `RationalPoint` if they are
    /// parallel.
    ///
    /// Java computes the general case with `BigInteger`. For `i32` coordinates the
    /// determinants are bounded by 2^63 and the deltas by 2^31, so every intermediate
    /// value is below 2^95 and `i128` is exact.
    pub fn intersection(&self, other: &Line) -> Point {
        let delta1 = self.b.difference_by_int(self.a);
        let delta2 = other.b.difference_by_int(other.a);
        // Separate handling of orthogonal and 45 degree lines for performance.
        if delta1.x == 0 {
            let this_x = self.a.x;
            if delta2.y == 0 {
                return Point::Int(IntPoint::new(this_x, other.a.y));
            }
            if delta2.x == delta2.y {
                return Point::Int(IntPoint::new(
                    this_x,
                    other.a.y.wrapping_add(this_x).wrapping_sub(other.a.x),
                ));
            }
            if delta2.x == delta2.y.wrapping_neg() {
                return Point::Int(IntPoint::new(
                    this_x,
                    other.a.y.wrapping_add(other.a.x).wrapping_sub(this_x),
                ));
            }
        } else if delta1.y == 0 {
            let this_y = self.a.y;
            if delta2.x == 0 {
                return Point::Int(IntPoint::new(other.a.x, this_y));
            }
            if delta2.x == delta2.y {
                return Point::Int(IntPoint::new(
                    other.a.x.wrapping_add(this_y).wrapping_sub(other.a.y),
                    this_y,
                ));
            }
            if delta2.x == delta2.y.wrapping_neg() {
                return Point::Int(IntPoint::new(
                    other.a.x.wrapping_add(other.a.y).wrapping_sub(this_y),
                    this_y,
                ));
            }
        } else if delta1.x == delta1.y {
            if delta2.x == 0 {
                let other_x = other.a.x;
                return Point::Int(IntPoint::new(
                    other_x,
                    self.a.y.wrapping_add(other_x).wrapping_sub(self.a.x),
                ));
            }
            if delta2.y == 0 {
                let other_y = other.a.y;
                return Point::Int(IntPoint::new(
                    self.a.x.wrapping_add(other_y).wrapping_sub(self.a.y),
                    other_y,
                ));
            }
        } else if delta1.x == delta1.y.wrapping_neg() {
            if delta2.x == 0 {
                let other_x = other.a.x;
                return Point::Int(IntPoint::new(
                    other_x,
                    self.a.y.wrapping_add(self.a.x).wrapping_sub(other_x),
                ));
            }
            if delta2.y == 0 {
                let other_y = other.a.y;
                return Point::Int(IntPoint::new(
                    self.a.x.wrapping_add(self.a.y).wrapping_sub(other_y),
                    other_y,
                ));
            }
        }

        let det1 = i128::from(self.a.determinant(self.b));
        let det2 = i128::from(other.a.determinant(other.b));
        let mut det = i128::from(delta2.determinant(delta1));
        let mut is_x = det1 * i128::from(delta2.x) - det2 * i128::from(delta1.x);
        let mut is_y = det1 * i128::from(delta2.y) - det2 * i128::from(delta1.y);
        if det != 0 {
            if det < 0 {
                det = -det;
                is_x = -is_x;
                is_y = -is_y;
            }
            if is_x % det == 0 && is_y % det == 0 {
                is_x /= det;
                is_y /= det;
                let crit = f64::from(CRIT_INT);
                if (is_x as f64).abs() <= crit && (is_y as f64).abs() <= crit {
                    return Point::Int(IntPoint::new(is_x as i32, is_y as i32));
                }
                det = 1;
            }
        }
        Point::Rational(Arc::new(RationalPoint::new(
            BigInt::from(is_x),
            BigInt::from(is_y),
            BigInt::from(det),
        )))
    }

    /// Approximation of the intersection of the two lines. If they are parallel, both
    /// coordinates are `i32::MAX`.
    pub fn intersection_approx(&self, other: &Line) -> FloatPoint {
        let (ta, tb, oa, ob) = (self.a, self.b, other.a, other.b);
        let d1x = f64::from(tb.x.wrapping_sub(ta.x));
        let d1y = f64::from(tb.y.wrapping_sub(ta.y));
        let d2x = f64::from(ob.x.wrapping_sub(oa.x));
        let d2y = f64::from(ob.y.wrapping_sub(oa.y));
        let det1 = f64::from(ta.x) * f64::from(tb.y) - f64::from(ta.y) * f64::from(tb.x);
        let det2 = f64::from(oa.x) * f64::from(ob.y) - f64::from(oa.y) * f64::from(ob.x);
        let det = d2x * d1y - d2y * d1x;
        if det == 0.0 {
            let m = f64::from(i32::MAX);
            return FloatPoint::new(m, m);
        }
        FloatPoint::new(
            (d2x * det1 - d1x * det2) / det,
            (d2y * det1 - d1y * det2) / det,
        )
    }

    /// The perpendicular projection of `point` onto this line.
    pub fn perpendicular_projection(&self, point: &Point) -> Point {
        point.perpendicular_projection(self)
    }

    /// [`Self::perpendicular_projection`] with the switch
    /// `router.java_quirks.rational_perp_projection` (N06).
    pub fn perpendicular_projection_with(&self, point: &Point, java_quirk: bool) -> Point {
        point.perpendicular_projection_with(self, java_quirk)
    }

    /// Translates the line perpendicular by about `dist`: to the left if `dist > 0`,
    /// else to the right.
    pub fn translate(&self, dist: f64) -> Line {
        let v = self.dir.get_vector();
        let vxvx = f64::from(v.x) * f64::from(v.x);
        let vyvy = f64::from(v.y) * f64::from(v.y);
        let length = (vxvx + vyvy).sqrt();
        let new_a = if vxvx <= vyvy {
            let rel_x = round_to_i32((dist * length) / f64::from(v.y));
            IntPoint::new(self.a.x.wrapping_sub(rel_x), self.a.y)
        } else {
            let rel_y = round_to_i32((dist * length) / f64::from(v.x));
            IntPoint::new(self.a.x, self.a.y.wrapping_add(rel_y))
        };
        Line::get_instance(new_a, self.dir)
    }

    /// Translates the line by `vector`.
    ///
    /// # Panics
    /// If the translated points are not integer points.
    pub fn translate_by(&self, vector: &Vector) -> Line {
        if vector.is_zero_constant() {
            return *self;
        }
        let new_a = self.a.translate_by(vector);
        let new_b = self.b.translate_by(vector);
        Line::from_points(&new_a, &new_b)
    }

    /// True if the line is axis parallel.
    pub fn is_orthogonal(&self) -> bool {
        self.dir.is_orthogonal()
    }

    /// True if the line is diagonal.
    pub fn is_diagonal(&self) -> bool {
        self.dir.is_diagonal()
    }

    /// True if the direction is a multiple of 45 degrees.
    pub fn is_multiple_of_45_degree(&self) -> bool {
        self.is_orthogonal() || self.is_diagonal()
    }

    /// True if this line and `other` are parallel.
    pub fn is_parallel(&self, other: &Line) -> bool {
        self.dir.get_vector().side_of_int(other.dir.get_vector()) == Side::Collinear
    }

    /// True if this line and `other` are perpendicular.
    pub fn is_perpendicular(&self, other: &Line) -> bool {
        self.dir.get_vector().projection_int(other.dir.get_vector()) == Signum::Zero
    }

    /// True if this and `other` contain the same points.
    pub fn is_equal_or_opposite(&self, other: &Line) -> bool {
        self.overlaps(other)
    }

    /// Cosine of the angle between this line and `other`.
    pub fn cos_angle(&self, other: &Line) -> f64 {
        let v1 = Vector::Int(self.b.difference_by_int(self.a));
        let v2 = Vector::Int(other.b.difference_by_int(other.a));
        v1.cos_angle(&v2)
    }

    /// Compares the directions of the two lines (Java `Line.compareTo`). Returns a
    /// negative number, 0 or a positive number.
    pub fn compare_to(&self, other: &Line) -> i32 {
        let dx1 = self.b.x.wrapping_sub(self.a.x);
        let dy1 = self.b.y.wrapping_sub(self.a.y);
        let dx2 = other.b.x.wrapping_sub(other.a.x);
        let dy2 = other.b.y.wrapping_sub(other.a.y);
        if dy1 > 0 {
            if dy2 < 0 {
                return -1;
            }
            if dy2 == 0 {
                if dx2 > 0 {
                    return 1;
                }
                return -1;
            }
        } else if dy1 < 0 {
            if dy2 >= 0 {
                return 1;
            }
        } else {
            if dx1 > 0 {
                if dy2 != 0 || dx2 < 0 {
                    return -1;
                }
                return 0;
            }
            if dy2 > 0 || (dy2 == 0 && dx2 > 0) {
                return 1;
            }
            if dy2 < 0 {
                return -1;
            }
            return 0;
        }
        let determinant = f64::from(dx2) * f64::from(dy1) - f64::from(dy2) * f64::from(dx1);
        Signum::as_int(determinant)
    }

    /// Approximation of the function value of this line at `x`; 0 for a vertical line.
    pub fn function_value_approx(&self, x: f64) -> f64 {
        let p1 = self.a.to_float();
        let p2 = self.b.to_float();
        let dx = p2.x - p1.x;
        if dx == 0.0 {
            return 0.0;
        }
        let dy = p2.y - p1.y;
        let det = p1.x * p2.y - p2.x * p1.y;
        (dy * x - det) / dx
    }

    /// Approximation of the function value in y of this line at `y`; 0 for a horizontal
    /// line.
    pub fn function_in_y_value_approx(&self, y: f64) -> f64 {
        let p1 = self.a.to_float();
        let p2 = self.b.to_float();
        let dy = p2.y - p1.y;
        if dy == 0.0 {
            return 0.0;
        }
        let dx = p2.x - p1.x;
        let det = p1.x * p2.y - p2.x * p1.y;
        (dx * y + det) / dy
    }

    /// The direction from `from_point` to the nearest point on this line; `None` if
    /// `from_point` is on the line.
    pub fn perpendicular_direction(&self, from_point: &Point) -> Option<Direction> {
        let line_side = self.side_of(from_point);
        if line_side == Side::Collinear {
            return None;
        }
        let dir1 = self.direction().turn_45_degree(2);
        let dir2 = self.direction().turn_45_degree(6);
        let check_point_1 = from_point.translate_by(&dir1.get_vector());
        if self.side_of(&check_point_1) != line_side {
            return Some(dir1);
        }
        let check_point_2 = from_point.translate_by(&dir2.get_vector());
        if self.side_of(&check_point_2) != line_side {
            return Some(dir2);
        }
        let nearest_line_point = from_point.to_float().projection_approx(self);
        if nearest_line_point.distance_square(check_point_1.to_float())
            <= nearest_line_point.distance_square(check_point_2.to_float())
        {
            Some(dir1)
        } else {
            Some(dir2)
        }
    }

    /// Turns this line by `factor` times 90 degrees around `pole`.
    pub fn turn_90_degree(&self, factor: i32, pole: IntPoint) -> Line {
        Line::new(
            self.a.turn_90_degree(factor, pole),
            self.b.turn_90_degree(factor, pole),
        )
    }

    /// Mirrors this line at the vertical line through `pole`.
    pub fn mirror_vertical(&self, pole: IntPoint) -> Line {
        Line::new(self.b.mirror_vertical(pole), self.a.mirror_vertical(pole))
    }

    /// Mirrors this line at the horizontal line through `pole`.
    pub fn mirror_horizontal(&self, pole: IntPoint) -> Line {
        Line::new(
            self.b.mirror_horizontal(pole),
            self.a.mirror_horizontal(pole),
        )
    }

    /// The Euclidean distance between the two defining points, as a `float`.
    ///
    /// Deltas are computed in floating point to avoid Java's wrapping `int`
    /// square (overflow above ~46k units axis-aligned, ~32k at 45 degrees).
    pub fn length(&self) -> f32 {
        let dx = f64::from(self.b.x) - f64::from(self.a.x);
        let dy = f64::from(self.b.y) - f64::from(self.a.y);
        dx.hypot(dy) as f32
    }
}

impl PartialEq for Line {
    fn eq(&self, other: &Line) -> bool {
        if self.a == other.a && self.b == other.b {
            // Java's identity shortcut.
            return true;
        }
        if self.side_of_int(other.a) != Side::Collinear {
            return false;
        }
        if self.side_of_int(other.b) != Side::Collinear {
            return false;
        }
        let dir1 = self.b.difference_by_int(self.a);
        let dir2 = other.b.difference_by_int(other.a);
        dir1.projection_int(dir2) == Signum::Positive
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_integer::Integer;
    use num_traits::{Signed, ToPrimitive, Zero};

    /// N06.14: the nearest lines through integer points beside an inexact point bracket
    /// it, one on each side, with no such line strictly between them; a line through
    /// the point that contains integer points serves both sides. Exact at the
    /// coordinate limits (`CRIT_INT`, and directions up to `2 * CRIT_INT`).
    #[test]
    fn nearest_lines_through_integer_points_bracket_the_point() {
        let rational = |x: i64, y: i64, z: i64| {
            Point::Rational(Arc::new(RationalPoint::new(
                BigInt::from(x),
                BigInt::from(y),
                BigInt::from(z),
            )))
        };
        // `f = dy * x - dx * y` at `point` times its denominator, and at a line.
        let f_line = |l: &Line| {
            let d = l.int_direction();
            BigInt::from(d.y) * l.a.x - BigInt::from(d.x) * l.a.y
        };
        // Whether the line through `point` with direction `dir` has integer points.
        let has_integer_points = |point: &Point, dir: IntDirection| match point {
            Point::Int(_) => true,
            Point::Rational(r) => {
                let f = BigInt::from(dir.y) * r.x() - BigInt::from(dir.x) * r.y();
                f.is_multiple_of(r.z())
            }
        };
        let check = |point: &Point, dir: IntDirection, exact_line: bool| {
            // The divisions' directions are normalized, as these.
            assert_eq!(dir.get_vector().to_normalized_direction(), dir);
            assert_eq!(
                has_integer_points(point, dir),
                exact_line,
                "{point:?} {dir:?}"
            );
            let left = Line::nearest_through_integer_points(point, dir, true).unwrap();
            let right = Line::nearest_through_integer_points(point, dir, false).unwrap();
            assert_eq!(left.int_direction(), dir);
            assert_eq!(right.int_direction(), dir);
            if exact_line {
                assert_eq!(left, right);
                assert_eq!(point.side_of_line(&left), Side::Collinear);
                return;
            }
            assert_eq!(
                point.side_of_line(&left),
                Side::OnTheLeft,
                "{point:?} {left:?}"
            );
            assert_eq!(
                point.side_of_line(&right),
                Side::OnTheRight,
                "{point:?} {right:?}"
            );
            // Adjacent: the two values differ by the gcd of the direction, 1.
            assert_eq!(f_line(&left) - f_line(&right), BigInt::from(1));
            // The anchors are the integer points of their lines nearest to the point.
            let p = point.to_float();
            let step = f64::from(dir.x).hypot(f64::from(dir.y));
            for l in [left, right] {
                let a = l.a.to_float();
                assert!((a.x - p.x).hypot(a.y - p.y) <= step, "{l:?} {p:?}");
            }
        };
        // An odd-parity octagon's corner, with an axis, a diagonal and a steep direction.
        let half = rational(25523, -4577, 2);
        check(&half, IntDirection::new(0, 1), false);
        check(&half, IntDirection::new(-1, 0), false);
        check(&half, IntDirection::new(1, 1), true);
        check(&half, IntDirection::new(49597, 6885), true);
        check(&half, IntDirection::new(49597, 6884), false);
        // A corner of an enlarged simplex, with a denominator that is not 2.
        check(
            &rational(1_000_003, -2_000_001, 7),
            IntDirection::new(-2251, 2552),
            false,
        );
        // At the coordinate limits.
        let c = i64::from(CRIT_INT);
        let near_limit = rational(2 * c - 1, -(2 * c - 3), 2);
        let big = CRIT_INT * 2 - 1;
        check(&near_limit, IntDirection::new(big, -(big - 2)), true);
        check(&near_limit, IntDirection::new(big, -(big - 1)), false);
        check(&near_limit, IntDirection::new(0, -1), false);
        check(
            &rational(3 * c - 1, c + 1, 3),
            IntDirection::new(-big, 2),
            false,
        );
        check(
            &rational(3 * c - 1, c + 1, 3),
            IntDirection::new(-big, 4),
            false,
        );
        // An integer point is its own anchor.
        let p = IntPoint::new(7, -3);
        let line =
            Line::nearest_through_integer_points(&Point::Int(p), IntDirection::new(2, 5), true);
        assert_eq!(line.map(|l| l.a), Some(p));
        assert_eq!(
            Line::nearest_through_integer_points(&half, IntDirection::NULL, true),
            None
        );
    }

    #[test]
    fn geometric_equality() {
        let l1 = Line::from_coords(0, 0, 10, 10);
        let l2 = Line::from_coords(-5, -5, 20, 20);
        assert_eq!(l1, l2);
        assert_ne!(l1, l1.opposite());
        assert!(l1.overlaps(&l1.opposite()));
        assert!(l1.fast_equals(&l2));
        assert!(!l1.fast_equals(&l1.opposite()));
    }

    #[test]
    fn direction_is_normalized() {
        let l = Line::from_coords(0, 0, 10, 20);
        assert_eq!(l.direction(), Direction::Int(IntDirection::new(1, 2)));
        let l2 = Line::from_point_and_direction(IntPoint::new(3, 3), IntDirection::UP);
        assert_eq!(l2.b(), IntPoint::new(3, 4));
    }

    #[test]
    fn sides() {
        let l = Line::from_coords(0, 0, 10, 0);
        // The line is on the right of a point above it.
        assert_eq!(l.side_of_int(IntPoint::new(5, 5)), Side::OnTheRight);
        assert_eq!(l.side_of_int(IntPoint::new(5, -5)), Side::OnTheLeft);
        assert_eq!(l.side_of_int(IntPoint::new(50, 0)), Side::Collinear);
        assert_eq!(l.side_of_float(FloatPoint::new(1.0, 0.5)), Side::OnTheRight);
        assert_eq!(
            l.side_of_float_tol(FloatPoint::new(1.0, 0.5), 10.0),
            Side::Collinear
        );
    }

    #[test]
    fn orthogonal_and_diagonal_intersections() {
        let v = Line::from_coords(3, 0, 3, 10);
        let h = Line::from_coords(0, 7, 10, 7);
        let d = Line::from_coords(0, 0, 1, 1);
        let ad = Line::from_coords(0, 0, 1, -1);
        assert_eq!(v.intersection(&h), Point::Int(IntPoint::new(3, 7)));
        assert_eq!(h.intersection(&v), Point::Int(IntPoint::new(3, 7)));
        assert_eq!(v.intersection(&d), Point::Int(IntPoint::new(3, 3)));
        assert_eq!(v.intersection(&ad), Point::Int(IntPoint::new(3, -3)));
        assert_eq!(h.intersection(&d), Point::Int(IntPoint::new(7, 7)));
        assert_eq!(h.intersection(&ad), Point::Int(IntPoint::new(-7, 7)));
        assert_eq!(d.intersection(&v), Point::Int(IntPoint::new(3, 3)));
        assert_eq!(ad.intersection(&h), Point::Int(IntPoint::new(-7, 7)));
    }

    #[test]
    fn general_intersections() {
        let l1 = Line::from_coords(0, 0, 2, 1);
        let l2 = Line::from_coords(0, 3, 1, 1);
        // x/2 = 3 - 2x  =>  x = 6/5, y = 3/5
        let p = l1.intersection(&l2);
        match &p {
            Point::Rational(r) => {
                assert_eq!(r.x(), &BigInt::from(6));
                assert_eq!(r.y(), &BigInt::from(3));
                assert_eq!(r.z(), &BigInt::from(5));
            }
            Point::Int(_) => panic!("expected a rational point"),
        }
        let l3 = Line::from_coords(0, 0, 1, 2);
        let l4 = Line::from_coords(10, 0, 9, 2);
        assert_eq!(l3.intersection(&l4), Point::Int(IntPoint::new(5, 10)));
        // Parallel lines give an infinite point.
        let l5 = Line::from_coords(0, 1, 2, 2);
        assert!(l1.intersection(&l5).is_infinite());
        let approx = l1.intersection_approx(&l2);
        assert!((approx.x - 1.2).abs() < 1e-12 && (approx.y - 0.6).abs() < 1e-12);
        assert_eq!(l1.intersection_approx(&l5).x, f64::from(i32::MAX));
    }

    /// Reference implementation of the general intersection with `BigInt`, exactly as
    /// in Java.
    fn big_intersection(l: &Line, o: &Line) -> (BigInt, BigInt, BigInt) {
        let d1 = l.b.difference_by_int(l.a);
        let d2 = o.b.difference_by_int(o.a);
        let det1 = BigInt::from(l.a.determinant(l.b));
        let det2 = BigInt::from(o.a.determinant(o.b));
        let mut det = BigInt::from(d2.determinant(d1));
        let mut is_x = &det1 * BigInt::from(d2.x) - &det2 * BigInt::from(d1.x);
        let mut is_y = &det1 * BigInt::from(d2.y) - &det2 * BigInt::from(d1.y);
        if !det.is_zero() {
            if det.is_negative() {
                det = -det;
                is_x = -is_x;
                is_y = -is_y;
            }
            if is_x.mod_floor(&det).is_zero() && is_y.mod_floor(&det).is_zero() {
                is_x /= &det;
                is_y /= &det;
                let crit = f64::from(CRIT_INT);
                if is_x.to_f64().unwrap().abs() <= crit && is_y.to_f64().unwrap().abs() <= crit {
                    return (is_x, is_y, BigInt::from(0x7fff_ffff_i64 + 1));
                }
                det = BigInt::from(1);
            }
        }
        (is_x, is_y, det)
    }

    #[test]
    fn i128_intersection_matches_bigint_at_coordinate_limits() {
        let m = i32::MAX;
        let n = i32::MIN;
        let c = CRIT_INT;
        let lines = [
            Line::from_coords(n, n, m, m - 1),
            Line::from_coords(m, n, n, m - 3),
            Line::from_coords(n, m, m, n + 7),
            Line::from_coords(-c, -c, c, c - 1),
            Line::from_coords(c, -c, -c, c - 1),
            Line::from_coords(n, 0, m, 1),
            Line::from_coords(0, n, 1, m),
            Line::from_coords(-c, c, c - 3, -c),
            Line::from_coords(n, n + 1, m, m),
            Line::from_coords(m, m, n, n + 2),
        ];
        for l in &lines {
            for o in &lines {
                let (x, y, z) = big_intersection(l, o);
                let p = l.intersection(o);
                match p {
                    Point::Int(ip) => {
                        assert_eq!(z, BigInt::from(1i64 << 31), "{l:?} {o:?}");
                        assert_eq!(BigInt::from(ip.x), x);
                        assert_eq!(BigInt::from(ip.y), y);
                    }
                    Point::Rational(r) => {
                        assert_eq!(r.x(), &x, "{l:?} {o:?}");
                        assert_eq!(r.y(), &y);
                        assert_eq!(r.z(), &z);
                    }
                }
            }
        }
    }

    #[test]
    fn translate_and_compare() {
        let l = Line::from_coords(0, 0, 10, 0);
        let t = l.translate(5.0);
        assert_eq!(t.a(), IntPoint::new(0, 5));
        assert_eq!(t.direction(), Direction::RIGHT);
        let up = Line::from_coords(0, 0, 0, 10);
        let t2 = up.translate(5.0);
        assert_eq!(t2.a(), IntPoint::new(-5, 0));
        assert!(l.compare_to(&up) < 0);
        assert!(up.compare_to(&l) > 0);
        assert_eq!(l.compare_to(&Line::from_coords(4, 4, 7, 4)), 0);
        let left = Line::from_coords(0, 0, -1, 0);
        let down = Line::from_coords(0, 0, 0, -1);
        assert!(left.compare_to(&down) < 0);
        assert!(down.compare_to(&left) > 0);
        assert!(l.is_perpendicular(&up));
        assert!(l.is_parallel(&left));
    }

    #[test]
    fn function_values_and_length() {
        let l = Line::from_coords(0, 1, 2, 5);
        assert_eq!(l.function_value_approx(1.0), 3.0);
        assert_eq!(l.function_in_y_value_approx(3.0), 1.0);
        assert_eq!(Line::from_coords(0, 0, 3, 4).length(), 5.0);
        // Java overflows its squared integer distance here.
        assert_eq!(Line::from_coords(0, 0, 50_000, 0).length(), 50_000.0);
        assert_eq!(Line::from_coords(0, 0, 30_000, 40_000).length(), 50_000.0);
        assert_eq!(
            Line::from_coords(i32::MIN, 0, i32::MAX, 0).length(),
            (f64::from(i32::MAX) - f64::from(i32::MIN)) as f32,
        );
    }

    #[test]
    fn perpendicular_direction_and_transforms() {
        let l = Line::from_coords(0, 0, 10, 0);
        assert_eq!(
            l.perpendicular_direction(&Point::Int(IntPoint::new(3, 5))),
            Some(Direction::DOWN)
        );
        assert_eq!(
            l.perpendicular_direction(&Point::Int(IntPoint::new(3, -5))),
            Some(Direction::UP)
        );
        assert_eq!(
            l.perpendicular_direction(&Point::Int(IntPoint::new(3, 0))),
            None
        );
        let turned = l.turn_90_degree(1, IntPoint::ZERO);
        assert_eq!(turned.direction(), Direction::UP);
        let mirrored = l.mirror_vertical(IntPoint::ZERO);
        assert_eq!(mirrored.a(), IntPoint::new(-10, 0));
        assert_eq!(mirrored.direction(), Direction::RIGHT);
        let mh = Line::from_coords(0, 0, 0, 10).mirror_horizontal(IntPoint::ZERO);
        assert_eq!(mh.direction(), Direction::UP);
    }
}
