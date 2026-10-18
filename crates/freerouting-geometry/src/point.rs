//! Ports of `app.freerouting.geometry.planar.Point`, `IntPoint` and `RationalPoint`.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{One, Signed, Zero};

use crate::big_int_aux::{
    add_rational_coordinates, determinant, divides, java_hash_code, to_f64, to_i32_wrapping,
};
use crate::direction::Direction;
use crate::float_point::FloatPoint;
use crate::jmath::d2i;
use crate::limits::{CRIT_INT, FLOAT_MAX_F64, crit_int_big};
use crate::line::Line;
use crate::shapes::{IntBox, IntOctagon};
use crate::side::Side;
use crate::vector::{IntVector, RationalVector, Vector};

/// A point with integer coordinates.
///
/// Java: `app.freerouting.geometry.planar.IntPoint`. Integer arithmetic wraps on overflow
/// exactly like Java `int` arithmetic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub struct IntPoint {
    pub x: i32,
    pub y: i32,
}

impl IntPoint {
    /// The zero point (`Point.ZERO`).
    pub const ZERO: IntPoint = IntPoint { x: 0, y: 0 };

    /// Creates an `IntPoint` from two integer coordinates.
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    /// Java `hashCode()` / `getId()`: `31 * x + y`.
    pub fn get_id(&self) -> i32 {
        self.x.wrapping_mul(31).wrapping_add(self.y)
    }

    /// Converts to a [`FloatPoint`].
    pub fn to_float(&self) -> FloatPoint {
        FloatPoint::new(f64::from(self.x), f64::from(self.y))
    }

    /// The smallest box containing this point.
    pub fn surrounding_box(&self) -> IntBox {
        IntBox::from_points(*self, *self)
    }

    /// The smallest octagon containing this point.
    pub fn surrounding_octagon(&self) -> IntOctagon {
        let tmp1 = self.x.wrapping_sub(self.y);
        let tmp2 = self.x.wrapping_add(self.y);
        IntOctagon::new(self.x, self.y, self.x, self.y, tmp1, tmp1, tmp2, tmp2)
    }

    /// True if this point lies inside or on the border of `b`.
    pub fn is_contained_in(&self, b: &IntBox) -> bool {
        self.x >= b.ll.x && self.y >= b.ll.y && self.x <= b.ur.x && self.y <= b.ur.y
    }

    /// Translation by an integer vector (Java `translateBy(IntVector)`), wrapping.
    pub fn translate_by_int(&self, v: IntVector) -> IntPoint {
        IntPoint::new(self.x.wrapping_add(v.x), self.y.wrapping_add(v.y))
    }

    /// Translation by an arbitrary vector.
    pub fn translate_by(&self, vector: &Vector) -> Point {
        if vector.is_zero_constant() {
            return Point::Int(*self);
        }
        vector.add_to_int_point(*self)
    }

    /// Difference vector `self - other` (Java `differenceBy(IntPoint)`), wrapping.
    pub fn difference_by_int(&self, other: IntPoint) -> IntVector {
        IntVector::new(self.x.wrapping_sub(other.x), self.y.wrapping_sub(other.y))
    }

    /// Difference vector `self - other`.
    pub fn difference_by(&self, other: &Point) -> Vector {
        match other {
            Point::Int(o) => Vector::Int(self.difference_by_int(*o)),
            Point::Rational(o) => {
                let tmp = o.difference_by_rational(&RationalPoint::from_int_point(*self));
                Vector::Rational(Box::new(tmp.negate()))
            }
        }
    }

    /// The side of `line` on which this point lies.
    pub fn side_of_line(&self, line: &Line) -> Side {
        let v1 = self.difference_by_int(line.a);
        let v2 = line.b.difference_by_int(line.a);
        v1.side_of_int(v2)
    }

    /// Determinant of the vectors `(x, y)` and `(other.x, other.y)` as a Java `long`.
    pub fn determinant(&self, other: IntPoint) -> i64 {
        (i64::from(self.x) * i64::from(other.y))
            .wrapping_sub(i64::from(self.y) * i64::from(other.x))
    }

    /// The nearest point to this point on `line`.
    ///
    /// The numerators are bounded by about 2^95 for any `i32` input, so they are computed
    /// in `i128` instead of `BigInteger`.
    pub fn perpendicular_projection(&self, line: &Line) -> Point {
        let v = line.b.difference_by_int(line.a);
        let vxvx = i128::from(i64::from(v.x) * i64::from(v.x));
        let vyvy = i128::from(i64::from(v.y) * i64::from(v.y));
        let vxvy = i128::from(i64::from(v.x) * i64::from(v.y));
        let mut denominator = vxvx + vyvy;
        let det = i128::from(line.a.determinant(line.b));
        let px = i128::from(self.x);
        let py = i128::from(self.y);
        let mut proj_x = vxvx * px + vxvy * py + det * i128::from(v.y);
        let mut proj_y = vxvy * px + vyvy * py - det * i128::from(v.x);
        let signum = denominator.signum();
        if signum != 0 {
            if signum < 0 {
                denominator = -denominator;
                proj_x = -proj_x;
                proj_y = -proj_y;
            }
            if proj_x % denominator == 0 && proj_y % denominator == 0 {
                proj_x /= denominator;
                proj_y /= denominator;
                return Point::Int(IntPoint::new(proj_x as i32, proj_y as i32));
            }
        }
        Point::Rational(Arc::new(RationalPoint::new(
            BigInt::from(proj_x),
            BigInt::from(proj_y),
            BigInt::from(denominator),
        )))
    }

    /// Signed area of the parallelogram spanned by `p2 - p1` and `self - p1`.
    pub fn signed_area(&self, p1: IntPoint, p2: IntPoint) -> f64 {
        let d21 = p2.difference_by_int(p1);
        let d01 = self.difference_by_int(p1);
        d21.determinant(d01) as f64
    }

    /// Square of the distance to `to_point`.
    pub fn distance_square(&self, to_point: IntPoint) -> f64 {
        let dx = f64::from(to_point.x.wrapping_sub(self.x));
        let dy = f64::from(to_point.y.wrapping_sub(self.y));
        dx * dx + dy * dy
    }

    /// Distance to `to_point`.
    pub fn distance(&self, to_point: IntPoint) -> f64 {
        self.distance_square(to_point).sqrt()
    }

    /// Snaps this point onto the horizontal or vertical line through `other`.
    pub fn orthogonal_projection(&self, other: IntPoint) -> IntPoint {
        let horizontal_distance = self.x.wrapping_sub(other.x).wrapping_abs();
        let vertical_distance = self.y.wrapping_sub(other.y).wrapping_abs();
        if horizontal_distance <= vertical_distance {
            IntPoint::new(other.x, self.y)
        } else {
            IntPoint::new(self.x, other.y)
        }
    }

    /// Snaps this point onto a 45 degree line through `other`.
    pub fn fortyfive_degree_projection(&self, other: IntPoint) -> IntPoint {
        let dx = self.x.wrapping_sub(other.x);
        let dy = self.y.wrapping_sub(other.y);
        let diagonal1 = (f64::from(dy) - f64::from(dx)) / 2.0;
        let diagonal2 = (f64::from(dy) + f64::from(dx)) / 2.0;
        let dist_arr = [
            f64::from(dx.wrapping_abs()),
            f64::from(dy.wrapping_abs()),
            diagonal1.abs(),
            diagonal2.abs(),
        ];
        let mut min_dist = dist_arr[0];
        for d in &dist_arr[1..] {
            if *d < min_dist {
                min_dist = *d;
            }
        }
        if min_dist == dist_arr[0] {
            IntPoint::new(other.x, self.y)
        } else if min_dist == dist_arr[1] {
            IntPoint::new(self.x, other.y)
        } else if min_dist == dist_arr[2] {
            let diagonal_value = d2i(diagonal2);
            IntPoint::new(
                other.x.wrapping_add(diagonal_value),
                other.y.wrapping_add(diagonal_value),
            )
        } else {
            let diagonal_value = d2i(diagonal1);
            IntPoint::new(
                other.x.wrapping_sub(diagonal_value),
                other.y.wrapping_add(diagonal_value),
            )
        }
    }

    /// A corner `p` so that the lines from this point to `p` and from `p` to `to_point`
    /// are multiples of 45 degrees with a 45 degree angle at `p`. `None` if the line from
    /// this point to `to_point` is already a multiple of 45 degrees.
    pub fn fortyfive_degree_corner(&self, to_point: IntPoint, left_turn: bool) -> Option<IntPoint> {
        let dx = to_point.x.wrapping_sub(self.x);
        let dy = to_point.y.wrapping_sub(self.y);
        let p = IntPoint::new;
        let result = if dy > 0 && dy < dx {
            if left_turn {
                p(to_point.x.wrapping_sub(dy), self.y)
            } else {
                p(self.x.wrapping_add(dy), to_point.y)
            }
        } else if dx > 0 && dy > dx {
            if left_turn {
                p(to_point.x, self.y.wrapping_add(dx))
            } else {
                p(self.x, to_point.y.wrapping_sub(dx))
            }
        } else if dx < 0 && dy > dx.wrapping_neg() {
            if left_turn {
                p(self.x, to_point.y.wrapping_add(dx))
            } else {
                p(to_point.x, self.y.wrapping_sub(dx))
            }
        } else if dy > 0 && dy < dx.wrapping_neg() {
            if left_turn {
                p(self.x.wrapping_sub(dy), to_point.y)
            } else {
                p(to_point.x.wrapping_add(dy), self.y)
            }
        } else if dy < 0 && dy > dx {
            if left_turn {
                p(to_point.x.wrapping_sub(dy), self.y)
            } else {
                p(self.x.wrapping_add(dy), to_point.y)
            }
        } else if dx < 0 && dy < dx {
            if left_turn {
                p(to_point.x, self.y.wrapping_add(dx))
            } else {
                p(self.x, to_point.y.wrapping_sub(dx))
            }
        } else if dx > 0 && dy < dx.wrapping_neg() {
            if left_turn {
                p(self.x, to_point.y.wrapping_add(dx))
            } else {
                p(to_point.x, self.y.wrapping_sub(dx))
            }
        } else if dy < 0 && dy > dx.wrapping_neg() {
            if left_turn {
                p(self.x.wrapping_sub(dy), to_point.y)
            } else {
                p(to_point.x.wrapping_add(dy), self.y)
            }
        } else {
            return None;
        };
        Some(result)
    }

    /// A corner `p` so that the lines from this point to `p` and from `p` to `to_point`
    /// are orthogonal. `None` if the line from this point to `to_point` is orthogonal.
    pub fn ninety_degree_corner(&self, to_point: IntPoint, left_turn: bool) -> Option<IntPoint> {
        let dx = to_point.x.wrapping_sub(self.x);
        let dy = to_point.y.wrapping_sub(self.y);
        if dx > 0 && dy > 0 || dx < 0 && dy < 0 {
            if left_turn {
                Some(IntPoint::new(to_point.x, self.y))
            } else {
                Some(IntPoint::new(self.x, to_point.y))
            }
        } else if dx < 0 && dy > 0 || dx > 0 && dy < 0 {
            if left_turn {
                Some(IntPoint::new(self.x, to_point.y))
            } else {
                Some(IntPoint::new(to_point.x, self.y))
            }
        } else {
            None
        }
    }

    /// Compares the x coordinates: 1, 0 or -1.
    pub fn compare_x_int(&self, other: IntPoint) -> i32 {
        cmp_to_i32(self.x.cmp(&other.x))
    }

    /// Compares the y coordinates: 1, 0 or -1.
    pub fn compare_y_int(&self, other: IntPoint) -> i32 {
        cmp_to_i32(self.y.cmp(&other.y))
    }

    /// Turns this point by `factor` times 90 degrees around `pole`.
    pub fn turn_90_degree(&self, factor: i32, pole: IntPoint) -> IntPoint {
        let v = self.difference_by_int(pole).turn_90_degree(factor);
        pole.translate_by_int(v)
    }

    /// Mirrors this point at the vertical line through `pole`.
    pub fn mirror_vertical(&self, pole: IntPoint) -> IntPoint {
        let v = self.difference_by_int(pole).mirror_at_y_axis();
        pole.translate_by_int(v)
    }

    /// Mirrors this point at the horizontal line through `pole`.
    pub fn mirror_horizontal(&self, pole: IntPoint) -> IntPoint {
        let v = self.difference_by_int(pole).mirror_at_x_axis();
        pole.translate_by_int(v)
    }
}

impl fmt::Display for IntPoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({},{})", self.x, self.y)
    }
}

pub(crate) fn cmp_to_i32(ordering: std::cmp::Ordering) -> i32 {
    match ordering {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

/// A point of the projective plane `(x, y, z)` with infinite precision integer
/// coordinates, meaning the affine point `(x / z, y / z)`. Points with `z == 0` lie on the
/// line at infinity.
///
/// Java: `app.freerouting.geometry.planar.RationalPoint`.
#[derive(Debug, Clone)]
pub struct RationalPoint {
    x: BigInt,
    y: BigInt,
    z: BigInt,
}

impl RationalPoint {
    /// Creates the point `(x / z, y / z)`.
    ///
    /// # Panics
    /// If `z < 0` (Java throws `IllegalArgumentException`).
    pub fn new(x: BigInt, y: BigInt, z: BigInt) -> RationalPoint {
        assert!(!z.is_negative(), "RationalPoint: z is expected to be >= 0");
        RationalPoint { x, y, z }
    }

    /// Creates a `RationalPoint` with denominator 1 from an [`IntPoint`].
    pub fn from_int_point(p: IntPoint) -> RationalPoint {
        RationalPoint {
            x: BigInt::from(p.x),
            y: BigInt::from(p.y),
            z: BigInt::one(),
        }
    }

    /// The numerator of the x coordinate.
    pub fn x(&self) -> &BigInt {
        &self.x
    }

    /// The numerator of the y coordinate.
    pub fn y(&self) -> &BigInt {
        &self.y
    }

    /// The common denominator.
    pub fn z(&self) -> &BigInt {
        &self.z
    }

    /// Approximates the coordinates by doubles; points at infinity map to
    /// `Float.MAX_VALUE`.
    pub fn to_float(&self) -> FloatPoint {
        let mut xd = to_f64(&self.x);
        let mut yd = to_f64(&self.y);
        let zd = to_f64(&self.z);
        if zd == 0.0 {
            xd = FLOAT_MAX_F64;
            yd = FLOAT_MAX_F64;
        } else {
            xd /= zd;
            yd /= zd;
        }
        FloatPoint::new(xd, yd)
    }

    /// Java `getId()`, built from the `BigInteger` hash codes.
    pub fn get_id(&self) -> i32 {
        let mut result = java_hash_code(&self.x);
        result = result
            .wrapping_mul(31)
            .wrapping_add(java_hash_code(&self.y));
        result
            .wrapping_mul(31)
            .wrapping_add(java_hash_code(&self.z))
    }

    /// Java `hashCode()`: hash of the coordinates reduced by their gcd; 0 at infinity.
    pub fn java_hash_code(&self) -> i32 {
        if self.z.is_zero() {
            return 0;
        }
        let gcd = self.x.abs().gcd(&self.y.abs()).gcd(&self.z);
        let (rx, ry, rz) = if gcd > BigInt::one() {
            (&self.x / &gcd, &self.y / &gcd, &self.z / &gcd)
        } else {
            (self.x.clone(), self.y.clone(), self.z.clone())
        };
        let mut result = java_hash_code(&rx);
        result = result.wrapping_mul(31).wrapping_add(java_hash_code(&ry));
        result.wrapping_mul(31).wrapping_add(java_hash_code(&rz))
    }

    /// True if `z == 0`.
    pub fn is_infinite(&self) -> bool {
        self.z.is_zero()
    }

    /// The smallest box with integer coordinates containing this point.
    pub fn surrounding_box(&self) -> IntBox {
        let fp = self.to_float();
        IntBox::new(
            d2i(fp.x.floor()),
            d2i(fp.y.floor()),
            d2i(fp.x.ceil()),
            d2i(fp.y.ceil()),
        )
    }

    /// The smallest octagon with integer coordinates containing this point.
    pub fn surrounding_octagon(&self) -> IntOctagon {
        let fp = self.to_float();
        let lx = d2i(fp.x.floor());
        let ly = d2i(fp.y.floor());
        let rx = d2i(fp.x.ceil());
        let uy = d2i(fp.y.ceil());
        let tmp = fp.x - fp.y;
        let ulx = d2i(tmp.floor());
        let lrx = d2i(tmp.ceil());
        let tmp = fp.x + fp.y;
        let llx = d2i(tmp.floor());
        let urx = d2i(tmp.ceil());
        IntOctagon::new(lx, ly, rx, uy, ulx, lrx, llx, urx)
    }

    /// True if this point lies inside or on the border of `b` (exact).
    pub fn is_contained_in(&self, b: &IntBox) -> bool {
        if self.x < BigInt::from(b.ll.x) * &self.z {
            return false;
        }
        if self.y < BigInt::from(b.ll.y) * &self.z {
            return false;
        }
        if self.x > BigInt::from(b.ur.x) * &self.z {
            return false;
        }
        self.y <= BigInt::from(b.ur.y) * &self.z
    }

    /// Java `translateBy(RationalVector)`.
    pub fn translate_by_rational(&self, vector: &RationalVector) -> RationalPoint {
        let [x, y, z] = add_rational_coordinates(
            [&self.x, &self.y, &self.z],
            [vector.x(), vector.y(), vector.z()],
        );
        RationalPoint::new(x, y, z)
    }

    /// Translation by an arbitrary vector.
    pub fn translate_by(&self, vector: &Vector) -> Point {
        if vector.is_zero_constant() {
            return Point::Rational(Arc::new(self.clone()));
        }
        vector.add_to_rational_point(self)
    }

    /// Java `differenceBy(RationalPoint)`.
    pub fn difference_by_rational(&self, other: &RationalPoint) -> RationalVector {
        let neg_x = -&other.x;
        let neg_y = -&other.y;
        let [x, y, z] =
            add_rational_coordinates([&self.x, &self.y, &self.z], [&neg_x, &neg_y, &other.z]);
        RationalVector::new(x, y, z)
    }

    /// Difference vector `self - other`.
    pub fn difference_by(&self, other: &Point) -> Vector {
        match other {
            Point::Int(o) => Vector::Rational(Box::new(
                self.difference_by_rational(&RationalPoint::from_int_point(*o)),
            )),
            Point::Rational(o) => Vector::Rational(Box::new(self.difference_by_rational(o))),
        }
    }

    /// Java `RationalPoint.perpendicularProjection(Line)`.
    ///
    /// This reproduces the Java implementation exactly, including its known defects: the
    /// determinant term is added with the wrong sign for the y coordinate and the
    /// denominator is not multiplied by `z`, so the result is only correct for some
    /// inputs. Callers that need an exact projection of a rational point must not rely
    /// on it; [`Self::perpendicular_projection_with`] with `java_quirk = false` is exact.
    pub fn perpendicular_projection(&self, line: &Line) -> Point {
        self.perpendicular_projection_with(line, true)
    }

    /// [`Self::perpendicular_projection`] behind the switch
    /// `router.java_quirks.rational_perp_projection` (N06): `true` is Java's formula,
    /// `false` the exact projection (the y term subtracts `det·vx·z` and the
    /// denominator is `(vx² + vy²)·z`).
    pub fn perpendicular_projection_with(&self, line: &Line, java_quirk: bool) -> Point {
        let v = line.b.difference_by_int(line.a);
        let vxvx = BigInt::from(i64::from(v.x) * i64::from(v.x));
        let vyvy = BigInt::from(i64::from(v.y) * i64::from(v.y));
        let vxvy = BigInt::from(i64::from(v.x) * i64::from(v.y));
        let det = BigInt::from(line.a.determinant(line.b));
        let mut proj_x = &vxvx * &self.x + &vxvy * &self.y + &det * BigInt::from(v.y) * &self.z;
        let (mut proj_y, mut denominator) = if java_quirk {
            // PARITY(quirk, java_quirks.rational_perp_projection): RationalPoint.java:239-283.
            (
                &vxvy * &self.x + &vyvy * &self.y + &det * BigInt::from(v.x) * &self.z,
                &vxvx + &vyvy,
            )
        } else {
            (
                &vxvy * &self.x + &vyvy * &self.y - &det * BigInt::from(v.x) * &self.z,
                (&vxvx + &vyvy) * &self.z,
            )
        };
        if !denominator.is_zero() {
            if denominator.is_negative() {
                denominator = -denominator;
                proj_x = -proj_x;
                proj_y = -proj_y;
            }
            if divides(&denominator, &proj_x) && divides(&denominator, &proj_y) {
                proj_x = &proj_x / &denominator;
                proj_y = &proj_y / &denominator;
                let crit = crit_int_big();
                if proj_x.abs() <= crit && proj_y.abs() <= crit {
                    return Point::Int(IntPoint::new(
                        to_i32_wrapping(&proj_x),
                        to_i32_wrapping(&proj_y),
                    ));
                }
                denominator = BigInt::one();
            }
        }
        Point::Rational(Arc::new(RationalPoint::new(proj_x, proj_y, denominator)))
    }

    fn compare_x_rational(&self, other: &RationalPoint) -> i32 {
        cmp_to_i32((&self.x * &other.z).cmp(&(&other.x * &self.z)))
    }

    fn compare_x_int(&self, other: IntPoint) -> i32 {
        cmp_to_i32(self.x.cmp(&(&self.z * BigInt::from(other.x))))
    }

    fn compare_y_rational(&self, other: &RationalPoint) -> i32 {
        cmp_to_i32((&self.y * &other.z).cmp(&(&other.y * &self.z)))
    }

    fn compare_y_int(&self, other: IntPoint) -> i32 {
        cmp_to_i32(self.y.cmp(&(&self.z * BigInt::from(other.y))))
    }
}

impl PartialEq for RationalPoint {
    /// Projective equality: `x1 * z2 == x2 * z1` and `y1 * z2 == y2 * z1`.
    fn eq(&self, other: &Self) -> bool {
        determinant(&self.x, &other.x, &self.z, &other.z).is_zero()
            && determinant(&self.y, &other.y, &self.z, &other.z).is_zero()
    }
}

impl Eq for RationalPoint {}

impl Hash for RationalPoint {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.java_hash_code().hash(state);
    }
}

/// A point in the plane: either an [`IntPoint`] or a [`RationalPoint`].
///
/// Java: the sealed abstract class `app.freerouting.geometry.planar.Point`. An `Int` and a
/// `Rational` point are never equal, even if they describe the same location (Java
/// compares the classes first).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Point {
    Int(IntPoint),
    Rational(Arc<RationalPoint>),
}

impl From<IntPoint> for Point {
    fn from(p: IntPoint) -> Self {
        Point::Int(p)
    }
}

impl From<RationalPoint> for Point {
    fn from(p: RationalPoint) -> Self {
        Point::Rational(Arc::new(p))
    }
}

impl Point {
    /// `Point.ZERO`.
    pub const ZERO: Point = Point::Int(IntPoint::ZERO);

    /// Creates an `IntPoint`, or a `RationalPoint` if a coordinate exceeds `CRIT_INT`.
    pub fn get_instance(x: i32, y: i32) -> Point {
        let result = IntPoint::new(x, y);
        if x.wrapping_abs() > CRIT_INT || y.wrapping_abs() > CRIT_INT {
            return Point::Rational(Arc::new(RationalPoint::from_int_point(result)));
        }
        Point::Int(result)
    }

    /// Creates a point from the homogeneous coordinates `(x, y, z)`, reducing to an
    /// `IntPoint` when possible.
    ///
    /// # Panics
    /// If `z == 0` (Java throws `ArithmeticException` from `BigInteger.mod`).
    pub fn get_instance_big(x: BigInt, y: BigInt, z: BigInt) -> Point {
        let (mut x, mut y, mut z) = (x, y, z);
        if z.is_negative() {
            x = -x;
            y = -y;
            z = -z;
        }
        if divides(&z, &x) {
            x = &x / &z;
            y = &y / &z;
            z = BigInt::one();
        }
        if z.is_one() {
            let crit = crit_int_big();
            if x.abs() <= crit && y.abs() <= crit {
                return Point::Int(IntPoint::new(to_i32_wrapping(&x), to_i32_wrapping(&y)));
            }
        }
        Point::Rational(Arc::new(RationalPoint::new(x, y, z)))
    }

    /// The point as an `IntPoint`, if it is one.
    pub fn as_int(&self) -> Option<IntPoint> {
        match self {
            Point::Int(p) => Some(*p),
            Point::Rational(_) => None,
        }
    }

    /// The point as an `IntPoint`.
    ///
    /// # Panics
    /// If the point is rational; this is where Java would throw a `ClassCastException`.
    pub fn expect_int(&self) -> IntPoint {
        match self {
            Point::Int(p) => *p,
            Point::Rational(_) => {
                panic!("operation only implemented for IntPoints (ClassCastException in Java)")
            }
        }
    }

    /// Translation by `vector`.
    pub fn translate_by(&self, vector: &Vector) -> Point {
        match self {
            Point::Int(p) => p.translate_by(vector),
            Point::Rational(p) => {
                if vector.is_zero_constant() {
                    return self.clone();
                }
                p.translate_by(vector)
            }
        }
    }

    /// Difference vector `self - other`.
    pub fn difference_by(&self, other: &Point) -> Vector {
        match self {
            Point::Int(p) => p.difference_by(other),
            Point::Rational(p) => p.difference_by(other),
        }
    }

    /// Approximates the coordinates by doubles.
    pub fn to_float(&self) -> FloatPoint {
        match self {
            Point::Int(p) => p.to_float(),
            Point::Rational(p) => p.to_float(),
        }
    }

    /// A stable id for deterministic tie-breaking.
    pub fn get_id(&self) -> i32 {
        match self {
            Point::Int(p) => p.get_id(),
            Point::Rational(p) => p.get_id(),
        }
    }

    /// True for a rational point with denominator 0.
    pub fn is_infinite(&self) -> bool {
        match self {
            Point::Int(_) => false,
            Point::Rational(p) => p.is_infinite(),
        }
    }

    /// The smallest box with integer coordinates containing this point.
    pub fn surrounding_box(&self) -> IntBox {
        match self {
            Point::Int(p) => p.surrounding_box(),
            Point::Rational(p) => p.surrounding_box(),
        }
    }

    /// The smallest octagon with integer coordinates containing this point.
    pub fn surrounding_octagon(&self) -> IntOctagon {
        match self {
            Point::Int(p) => p.surrounding_octagon(),
            Point::Rational(p) => p.surrounding_octagon(),
        }
    }

    /// True if this point lies inside or on the border of `b`.
    pub fn is_contained_in(&self, b: &IntBox) -> bool {
        match self {
            Point::Int(p) => p.is_contained_in(b),
            Point::Rational(p) => p.is_contained_in(b),
        }
    }

    /// The side of `line` on which this point lies.
    pub fn side_of_line(&self, line: &Line) -> Side {
        match self {
            Point::Int(p) => p.side_of_line(line),
            Point::Rational(_) => self.side_of(&Point::Int(line.a), &Point::Int(line.b)),
        }
    }

    /// `OnTheLeft` if this point is on the left of the line from `p1` to `p2`,
    /// `OnTheRight` if on the right, `Collinear` otherwise.
    pub fn side_of(&self, p1: &Point, p2: &Point) -> Side {
        let v1 = self.difference_by(p1);
        let v2 = p2.difference_by(p1);
        v1.side_of(&v2)
    }

    /// The nearest point to this point on `line`.
    pub fn perpendicular_projection(&self, line: &Line) -> Point {
        self.perpendicular_projection_with(line, true)
    }

    /// [`Self::perpendicular_projection`] with the switch
    /// `router.java_quirks.rational_perp_projection` (N06; see
    /// [`RationalPoint::perpendicular_projection_with`]). An integer point is exact
    /// either way.
    pub fn perpendicular_projection_with(&self, line: &Line, java_quirk: bool) -> Point {
        match self {
            Point::Int(p) => p.perpendicular_projection(line),
            Point::Rational(p) => p.perpendicular_projection_with(line, java_quirk),
        }
    }

    /// The perpendicular direction from this point to `line`; `Direction::NULL` if the
    /// point lies on the line.
    pub fn perpendicular_direction(&self, line: &Line) -> Direction {
        let side = self.side_of_line(line);
        if side == Side::Collinear {
            return Direction::NULL;
        }
        if side == Side::OnTheRight {
            line.direction().turn_45_degree(2)
        } else {
            line.direction().turn_45_degree(6)
        }
    }

    /// 1 if this point has a strictly bigger x coordinate than `other`, 0 if equal, -1
    /// otherwise.
    pub fn compare_x(&self, other: &Point) -> i32 {
        match (self, other) {
            (Point::Int(a), Point::Int(b)) => a.compare_x_int(*b),
            (Point::Int(a), Point::Rational(b)) => -b.compare_x_int(*a),
            (Point::Rational(a), Point::Int(b)) => a.compare_x_int(*b),
            (Point::Rational(a), Point::Rational(b)) => -b.compare_x_rational(a),
        }
    }

    /// 1 if this point has a strictly bigger y coordinate than `other`, 0 if equal, -1
    /// otherwise.
    pub fn compare_y(&self, other: &Point) -> i32 {
        match (self, other) {
            (Point::Int(a), Point::Int(b)) => a.compare_y_int(*b),
            (Point::Int(a), Point::Rational(b)) => -b.compare_y_int(*a),
            (Point::Rational(a), Point::Int(b)) => a.compare_y_int(*b),
            (Point::Rational(a), Point::Rational(b)) => -b.compare_y_rational(a),
        }
    }

    /// `compare_x`, or `compare_y` if the x coordinates are equal.
    pub fn compare_xy(&self, other: &Point) -> i32 {
        let result = self.compare_x(other);
        if result == 0 {
            self.compare_y(other)
        } else {
            result
        }
    }

    /// Turns this point by `factor` times 90 degrees around `pole`.
    pub fn turn_90_degree(&self, factor: i32, pole: &Point) -> Point {
        let v = self.difference_by(pole).turn_90_degree(factor);
        pole.translate_by(&v)
    }

    /// Mirrors this point at the vertical line through `pole`.
    pub fn mirror_vertical(&self, pole: &Point) -> Point {
        let v = self.difference_by(pole).mirror_at_y_axis();
        pole.translate_by(&v)
    }

    /// Mirrors this point at the horizontal line through `pole`.
    pub fn mirror_horizontal(&self, pole: &Point) -> Point {
        let v = self.difference_by(pole).mirror_at_x_axis();
        pole.translate_by(&v)
    }
}

impl fmt::Display for Point {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Point::Int(p) => p.fmt(f),
            Point::Rational(p) => write!(f, "RationalPoint({}, {}, {})", p.x, p.y, p.z),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rp(x: i64, y: i64, z: i64) -> RationalPoint {
        RationalPoint::new(BigInt::from(x), BigInt::from(y), BigInt::from(z))
    }

    fn hash_of<T: Hash>(t: &T) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        let mut h = DefaultHasher::new();
        t.hash(&mut h);
        h.finish()
    }

    // Port of PointEqualsHashCodeTest.intPointEqualsAndHashCodeContract.
    #[test]
    fn int_point_equals_and_hash_code_contract() {
        let p1 = IntPoint::new(100, 200);
        let p2 = IntPoint::new(100, 200);
        assert_eq!(p1, p2);
        assert_eq!(p1.get_id(), p2.get_id());
        assert_eq!(hash_of(&p1), hash_of(&p2));
        assert_ne!(p1, IntPoint::new(100, 201));
        assert_ne!(p1, IntPoint::new(101, 200));
        assert_ne!(Point::Int(p1), Point::from(rp(100, 200, 1)));
    }

    // Port of PointEqualsHashCodeTest.rationalPointEqualsAndHashCodeContract.
    #[test]
    fn rational_point_equals_and_hash_code_contract() {
        let p1 = rp(100, 200, 50);
        let p2 = rp(2, 4, 1);
        assert_eq!(p1, p2);
        assert_eq!(p1.java_hash_code(), p2.java_hash_code());
        assert_eq!(hash_of(&p1), hash_of(&p2));
        let p3 = rp(-4, 6, 2);
        let p4 = rp(-2, 3, 1);
        assert_eq!(p3, p4);
        assert_eq!(p3.java_hash_code(), p4.java_hash_code());
        assert_ne!(p1, p3);
    }

    // Port of PointEqualsHashCodeTest.rationalPointInfinitePoints.
    #[test]
    fn rational_point_infinite_points() {
        let inf1 = rp(10, 20, 0);
        let inf2 = rp(30, 40, 0);
        assert_eq!(inf1, inf2);
        assert_eq!(inf1.java_hash_code(), inf2.java_hash_code());
        assert!(inf1.is_infinite());
        assert_eq!(
            inf1.to_float(),
            FloatPoint::new(FLOAT_MAX_F64, FLOAT_MAX_F64)
        );
    }

    // Port of SealedGeometryHierarchyTest.pointHierarchyHasOnlyFiniteAndRationalImplementations:
    // the enum is closed, so this exhaustive match stops compiling if a variant is added.
    #[test]
    fn point_hierarchy_is_closed() {
        fn kind(p: &Point) -> &'static str {
            match p {
                Point::Int(_) => "int",
                Point::Rational(_) => "rational",
            }
        }
        assert_eq!(kind(&Point::ZERO), "int");
        assert_eq!(kind(&Point::from(rp(1, 1, 2))), "rational");
    }

    #[test]
    fn get_instance_reduces() {
        assert_eq!(Point::get_instance(5, 6), Point::Int(IntPoint::new(5, 6)));
        assert!(matches!(
            Point::get_instance(CRIT_INT + 1, 0),
            Point::Rational(_)
        ));
        // Math.abs(Integer.MIN_VALUE) is negative in Java, so this stays an IntPoint.
        assert!(matches!(Point::get_instance(i32::MIN, 0), Point::Int(_)));
        assert_eq!(
            Point::get_instance_big(BigInt::from(-10), BigInt::from(4), BigInt::from(-2)),
            Point::Int(IntPoint::new(5, -2))
        );
        assert!(matches!(
            Point::get_instance_big(BigInt::from(1), BigInt::from(4), BigInt::from(2)),
            Point::Rational(_)
        ));
    }

    #[test]
    fn compare_and_side() {
        let a = Point::Int(IntPoint::new(1, 1));
        let half = Point::from(rp(1, 1, 2));
        assert_eq!(a.compare_x(&half), 1);
        assert_eq!(half.compare_x(&a), -1);
        assert_eq!(half.compare_xy(&half), 0);
        let p1 = Point::Int(IntPoint::new(0, 0));
        let p2 = Point::Int(IntPoint::new(10, 0));
        assert_eq!(a.side_of(&p1, &p2), Side::OnTheLeft);
        assert_eq!(half.side_of(&p1, &p2), Side::OnTheLeft);
        let below = Point::from(rp(1, -1, 3));
        assert_eq!(below.side_of(&p1, &p2), Side::OnTheRight);
    }

    #[test]
    fn perpendicular_projection_int() {
        let line = Line::new(IntPoint::new(0, 0), IntPoint::new(10, 10));
        let p = IntPoint::new(10, 0);
        assert_eq!(
            p.perpendicular_projection(&line),
            Point::Int(IntPoint::new(5, 5))
        );
        let q = IntPoint::new(1, 0);
        assert_eq!(q.perpendicular_projection(&line), Point::from(rp(1, 1, 2)));
    }

    /// N06 `rational_perp_projection`: `true` is Java's formula, `false` the exact
    /// projection of a rational point.
    #[test]
    fn rational_perpendicular_projection_switch_both_behaviours() {
        // The horizontal line y = 10 and the point (1/2, 1/2): the exact foot is (1/2, 10).
        let line = Line::new(IntPoint::new(0, 10), IntPoint::new(10, 10));
        let p = Point::from(rp(1, 1, 2));
        let java = p.perpendicular_projection_with(&line, true);
        assert_eq!(java, p.perpendicular_projection(&line));
        assert_eq!(java.to_float(), FloatPoint::new(1.0, -20.0));
        let exact = p.perpendicular_projection_with(&line, false);
        assert_eq!(exact.to_float(), FloatPoint::new(0.5, 10.0));
        assert_eq!(line.perpendicular_projection_with(&p, false), exact);
        // Through the origin only the missing `z` shows: (1/2, 0) onto y = x.
        let diagonal = Line::new(IntPoint::new(0, 0), IntPoint::new(10, 10));
        let q = Point::from(rp(1, 0, 2));
        assert_eq!(
            q.perpendicular_projection_with(&diagonal, true).to_float(),
            FloatPoint::new(0.5, 0.5)
        );
        assert_eq!(
            q.perpendicular_projection_with(&diagonal, false).to_float(),
            FloatPoint::new(0.25, 0.25)
        );
        // An integer point is exact under both values.
        let i = Point::Int(IntPoint::new(10, 0));
        assert_eq!(
            i.perpendicular_projection_with(&diagonal, false),
            i.perpendicular_projection_with(&diagonal, true)
        );
    }

    #[test]
    fn perpendicular_projection_at_coordinate_limits_matches_big_integer() {
        // The i128 shortcut must agree with a BigInt evaluation of the Java formula for
        // extreme coordinates (full i32 range, beyond CRIT_INT).
        let cases = [
            (
                IntPoint::new(i32::MIN, i32::MAX),
                IntPoint::new(i32::MAX, i32::MIN),
                IntPoint::new(7, -3),
            ),
            (
                IntPoint::new(-CRIT_INT, CRIT_INT),
                IntPoint::new(CRIT_INT, CRIT_INT - 1),
                IntPoint::new(CRIT_INT, -CRIT_INT),
            ),
            (
                IntPoint::new(i32::MAX, i32::MAX),
                IntPoint::new(i32::MIN, 3),
                IntPoint::new(i32::MIN, i32::MIN),
            ),
        ];
        for (a, b, p) in cases {
            let line = Line::new(a, b);
            let v = b.difference_by_int(a);
            let vxvx = BigInt::from(i64::from(v.x) * i64::from(v.x));
            let vyvy = BigInt::from(i64::from(v.y) * i64::from(v.y));
            let vxvy = BigInt::from(i64::from(v.x) * i64::from(v.y));
            let den = &vxvx + &vyvy;
            let det = BigInt::from(a.determinant(b));
            let px = &vxvx * p.x + &vxvy * p.y + &det * v.y;
            let py = &vxvy * p.x + &vyvy * p.y - &det * v.x;
            let expected = if divides(&den, &px) && divides(&den, &py) {
                Point::Int(IntPoint::new(
                    to_i32_wrapping(&(&px / &den)),
                    to_i32_wrapping(&(&py / &den)),
                ))
            } else {
                Point::from(RationalPoint::new(px, py, den))
            };
            assert_eq!(p.perpendicular_projection(&line), expected);
        }
    }

    #[test]
    fn wrapping_determinant_like_java_long() {
        let a = IntPoint::new(i32::MIN, i32::MAX);
        let b = IntPoint::new(i32::MAX, i32::MIN);
        let exact = i128::from(a.x) * i128::from(b.y) - i128::from(a.y) * i128::from(b.x);
        assert_eq!(a.determinant(b), exact as i64);
    }

    #[test]
    fn corners_and_projections() {
        let p = IntPoint::new(0, 0);
        assert_eq!(
            p.fortyfive_degree_corner(IntPoint::new(10, 3), true),
            Some(IntPoint::new(7, 0))
        );
        assert_eq!(p.fortyfive_degree_corner(IntPoint::new(10, 10), true), None);
        assert_eq!(
            p.ninety_degree_corner(IntPoint::new(5, 5), false),
            Some(IntPoint::new(0, 5))
        );
        assert_eq!(
            IntPoint::new(10, 3).orthogonal_projection(p),
            IntPoint::new(10, 0)
        );
        assert_eq!(
            IntPoint::new(10, 9).fortyfive_degree_projection(p),
            IntPoint::new(9, 9)
        );
        assert_eq!(
            IntPoint::new(3, 4).turn_90_degree(1, p),
            IntPoint::new(-4, 3)
        );
        assert_eq!(
            IntPoint::new(3, 4).mirror_vertical(IntPoint::new(1, 0)),
            IntPoint::new(-1, 4)
        );
    }
}
