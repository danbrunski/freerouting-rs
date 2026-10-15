//! Ports of `app.freerouting.geometry.planar.Vector`, `IntVector` and `RationalVector`.

use std::sync::Arc;

use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{One, Signed, Zero};

use crate::big_int_aux::{
    add_rational_coordinates, binary_gcd, determinant, divides, to_f64, to_i32_wrapping,
};
use crate::direction::{BigIntDirection, Direction, IntDirection};
use crate::float_point::FloatPoint;
use crate::limits::{CRIT_INT, crit_int_big};
use crate::point::{IntPoint, Point, RationalPoint};
use crate::side::Side;
use crate::signum::Signum;

/// A vector with integer coordinates.
///
/// Java: `app.freerouting.geometry.planar.IntVector`. Arithmetic wraps like Java `int`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct IntVector {
    pub x: i32,
    pub y: i32,
}

impl IntVector {
    /// `Vector.ZERO`.
    pub const ZERO: IntVector = IntVector { x: 0, y: 0 };

    /// Creates a vector from two integer coordinates (no range check, as in Java).
    pub const fn new(x: i32, y: i32) -> IntVector {
        IntVector { x, y }
    }

    /// True if both coordinates are 0.
    pub fn is_zero(&self) -> bool {
        self.x == 0 && self.y == 0
    }

    /// The opposite vector.
    pub fn negate(&self) -> IntVector {
        IntVector::new(self.x.wrapping_neg(), self.y.wrapping_neg())
    }

    /// True if the vector is horizontal or vertical.
    pub fn is_orthogonal(&self) -> bool {
        self.x == 0 || self.y == 0
    }

    /// True if the vector is diagonal.
    pub fn is_diagonal(&self) -> bool {
        self.x.wrapping_abs() == self.y.wrapping_abs()
    }

    /// True if the vector is orthogonal or diagonal.
    pub fn is_multiple_of_45_degree(&self) -> bool {
        self.is_orthogonal() || self.is_diagonal()
    }

    /// Determinant of the matrix `(self, other)` as a Java `long`.
    pub fn determinant(&self, other: IntVector) -> i64 {
        (i64::from(self.x) * i64::from(other.y))
            .wrapping_sub(i64::from(self.y) * i64::from(other.x))
    }

    /// Turns this vector by `factor` times 90 degrees.
    pub fn turn_90_degree(&self, factor: i32) -> IntVector {
        match factor.rem_euclid(4) {
            0 => IntVector::new(self.x, self.y),
            1 => IntVector::new(self.y.wrapping_neg(), self.x),
            2 => IntVector::new(self.x.wrapping_neg(), self.y.wrapping_neg()),
            _ => IntVector::new(self.y, self.x.wrapping_neg()),
        }
    }

    /// Mirrors this vector at the y axis.
    pub fn mirror_at_y_axis(&self) -> IntVector {
        IntVector::new(self.x.wrapping_neg(), self.y)
    }

    /// Mirrors this vector at the x axis.
    pub fn mirror_at_x_axis(&self) -> IntVector {
        IntVector::new(self.x, self.y.wrapping_neg())
    }

    /// Sum of two integer vectors (wrapping).
    pub fn add_int(&self, other: IntVector) -> IntVector {
        IntVector::new(self.x.wrapping_add(other.x), self.y.wrapping_add(other.y))
    }

    /// Side of the line from zero through `other` on which this vector lies, computed in
    /// doubles exactly like Java's `IntVector.sideOf`.
    pub fn side_of_int(&self, other: IntVector) -> Side {
        // Java: this.sideOf(other) = other.sideOf(this).negate(), where
        // other.sideOf(this) = Side.of((double) this.x * other.y - (double) this.y * other.x).
        let det = f64::from(self.x) * f64::from(other.y) - f64::from(self.y) * f64::from(other.x);
        Side::of(det).negate()
    }

    /// Signum of the scalar product with `other`, computed in doubles like Java.
    pub fn projection_int(&self, other: IntVector) -> Signum {
        let tmp = f64::from(other.x) * f64::from(self.x) + f64::from(other.y) * f64::from(self.y);
        Signum::of(tmp)
    }

    /// Scalar product with `other` as a double.
    pub fn scalar_product_int(&self, other: IntVector) -> f64 {
        f64::from(other.x) * f64::from(self.x) + f64::from(other.y) * f64::from(self.y)
    }

    /// Converts to a [`FloatPoint`].
    pub fn to_float(&self) -> FloatPoint {
        FloatPoint::new(f64::from(self.x), f64::from(self.y))
    }

    /// The normalized direction of this vector (coordinates divided by their gcd).
    pub fn to_normalized_direction(&self) -> IntDirection {
        let mut dx = self.x;
        let mut dy = self.y;
        let gcd = binary_gcd(dx.wrapping_abs(), dy.wrapping_abs());
        if gcd > 1 {
            dx /= gcd;
            dy /= gcd;
        }
        IntDirection::new(dx, dy)
    }

    /// An approximation of this vector with the same direction and length `length`.
    pub fn change_length_approx(&self, length: f64) -> IntVector {
        let new_point = self.to_float().change_size(length);
        new_point.round().difference_by_int(IntPoint::ZERO)
    }
}

/// A vector with rational coordinates `(x / z, y / z)`, `z >= 0`.
///
/// Java: `app.freerouting.geometry.planar.RationalVector`.
#[derive(Debug, Clone)]
pub struct RationalVector {
    x: BigInt,
    y: BigInt,
    z: BigInt,
}

impl RationalVector {
    /// Creates the vector `(x / z, y / z)`; a negative `z` is normalized by negating all
    /// three coordinates.
    pub fn new(x: BigInt, y: BigInt, z: BigInt) -> RationalVector {
        if z.is_negative() {
            RationalVector {
                x: -x,
                y: -y,
                z: -z,
            }
        } else {
            RationalVector { x, y, z }
        }
    }

    /// Creates a rational vector with denominator 1 from an [`IntVector`].
    pub fn from_int_vector(v: IntVector) -> RationalVector {
        RationalVector {
            x: BigInt::from(v.x),
            y: BigInt::from(v.y),
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

    /// True if the x and y coordinates are 0.
    pub fn is_zero(&self) -> bool {
        self.x.is_zero() && self.y.is_zero()
    }

    /// The opposite vector.
    pub fn negate(&self) -> RationalVector {
        RationalVector::new(-&self.x, -&self.y, self.z.clone())
    }

    /// Sum of two rational vectors.
    pub fn add_rational(&self, other: &RationalVector) -> RationalVector {
        let [x, y, z] =
            add_rational_coordinates([&self.x, &self.y, &self.z], [&other.x, &other.y, &other.z]);
        RationalVector::new(x, y, z)
    }

    /// Java `RationalVector.sideOf(RationalVector other)`: the sign of
    /// `y * other.x - x * other.y` (numerators only).
    fn side_of_rational(&self, other: &RationalVector) -> Side {
        let det = &self.y * &other.x - &self.x * &other.y;
        Side::of(f64::from(signum_i32(&det)))
    }

    /// True if horizontal or vertical.
    pub fn is_orthogonal(&self) -> bool {
        self.x.is_zero() || self.y.is_zero()
    }

    /// True if diagonal.
    pub fn is_diagonal(&self) -> bool {
        self.x.abs() == self.y.abs()
    }

    fn projection_rational(&self, other: &RationalVector) -> Signum {
        let tmp = &self.x * &other.x + &self.y * &other.y;
        Signum::of(f64::from(signum_i32(&tmp)))
    }

    /// Approximates the coordinates by doubles.
    pub fn to_float(&self) -> FloatPoint {
        let xd = to_f64(&self.x);
        let yd = to_f64(&self.y);
        let zd = to_f64(&self.z);
        FloatPoint::new(xd / zd, yd / zd)
    }

    /// Turns this vector by `factor` times 90 degrees.
    pub fn turn_90_degree(&self, factor: i32) -> RationalVector {
        match factor.rem_euclid(4) {
            0 => self.clone(),
            1 => RationalVector::new(-&self.y, self.x.clone(), self.z.clone()),
            2 => RationalVector::new(-&self.x, -&self.y, self.z.clone()),
            _ => RationalVector::new(self.y.clone(), -&self.x, self.z.clone()),
        }
    }

    /// Mirrors at the y axis.
    pub fn mirror_at_y_axis(&self) -> RationalVector {
        RationalVector::new(-&self.x, self.y.clone(), self.z.clone())
    }

    /// Mirrors at the x axis.
    pub fn mirror_at_x_axis(&self) -> RationalVector {
        RationalVector::new(self.x.clone(), -&self.y, self.z.clone())
    }

    /// The direction of this vector, reduced by the gcd of its numerators.
    ///
    /// # Panics
    /// If both numerators are 0 (Java divides by a zero gcd and throws).
    pub fn to_normalized_direction(&self) -> Direction {
        let gcd = self.x.gcd(&self.y);
        let dx = &self.x / &gcd;
        let dy = &self.y / &gcd;
        let crit = crit_int_big();
        if dx.abs() <= crit && dy.abs() <= crit {
            return Direction::Int(IntDirection::new(
                to_i32_wrapping(&dx),
                to_i32_wrapping(&dy),
            ));
        }
        Direction::BigInt(Box::new(BigIntDirection::new(dx, dy)))
    }

    /// Java `addTo(IntPoint)`: `(z * p.x + x, z * p.y + y, z)`, always rational.
    pub fn add_to_int_point(&self, point: IntPoint) -> RationalPoint {
        let new_x = &self.z * BigInt::from(point.x) + &self.x;
        let new_y = &self.z * BigInt::from(point.y) + &self.y;
        RationalPoint::new(new_x, new_y, self.z.clone())
    }

    /// Java `addTo(RationalPoint)`.
    pub fn add_to_rational_point(&self, point: &RationalPoint) -> RationalPoint {
        let [x, y, z] = add_rational_coordinates(
            [&self.x, &self.y, &self.z],
            [point.x(), point.y(), point.z()],
        );
        RationalPoint::new(x, y, z)
    }
}

impl PartialEq for RationalVector {
    fn eq(&self, other: &Self) -> bool {
        determinant(&self.x, &other.x, &self.z, &other.z).is_zero()
            && determinant(&self.y, &other.y, &self.z, &other.z).is_zero()
    }
}

fn signum_i32(v: &BigInt) -> i32 {
    if v.is_positive() {
        1
    } else if v.is_negative() {
        -1
    } else {
        0
    }
}

/// A vector for translating points: integer or rational.
///
/// Java: the abstract class `app.freerouting.geometry.planar.Vector`. As in Java, an `Int`
/// and a `Rational` vector never compare equal.
#[derive(Debug, Clone, PartialEq)]
pub enum Vector {
    Int(IntVector),
    Rational(Box<RationalVector>),
}

impl From<IntVector> for Vector {
    fn from(v: IntVector) -> Self {
        Vector::Int(v)
    }
}

impl From<RationalVector> for Vector {
    fn from(v: RationalVector) -> Self {
        Vector::Rational(Box::new(v))
    }
}

impl Vector {
    /// `Vector.ZERO`.
    pub const ZERO: Vector = Vector::Int(IntVector::ZERO);

    /// Creates `(x, y)`; rational if a coordinate exceeds `CRIT_INT`.
    pub fn get_instance(x: i32, y: i32) -> Vector {
        let result = IntVector::new(x, y);
        if x.wrapping_abs() > CRIT_INT || y.wrapping_abs() > CRIT_INT {
            return Vector::Rational(Box::new(RationalVector::from_int_vector(result)));
        }
        Vector::Int(result)
    }

    /// Creates `(x / z, y / z)`, reducing to an `IntVector` when possible.
    ///
    /// # Panics
    /// If `z == 0` (Java throws `ArithmeticException`).
    pub fn get_instance_big(x: BigInt, y: BigInt, z: BigInt) -> Vector {
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
                return Vector::Int(IntVector::new(to_i32_wrapping(&x), to_i32_wrapping(&y)));
            }
        }
        Vector::Rational(Box::new(RationalVector::new(x, y, z)))
    }

    /// True if this is the integer zero vector, i.e. Java `vector.equals(Vector.ZERO)`.
    pub(crate) fn is_zero_constant(&self) -> bool {
        matches!(self, Vector::Int(v) if v.is_zero())
    }

    /// True if this vector is the zero vector.
    pub fn is_zero(&self) -> bool {
        match self {
            Vector::Int(v) => v.is_zero(),
            Vector::Rational(v) => v.is_zero(),
        }
    }

    /// The opposite vector.
    pub fn negate(&self) -> Vector {
        match self {
            Vector::Int(v) => Vector::Int(v.negate()),
            Vector::Rational(v) => Vector::Rational(Box::new(v.negate())),
        }
    }

    /// Sum of `self` and `other`.
    pub fn add(&self, other: &Vector) -> Vector {
        match (self, other) {
            (Vector::Int(a), Vector::Int(b)) => Vector::Int(b.add_int(*a)),
            (Vector::Int(a), Vector::Rational(b)) => Vector::Rational(Box::new(
                b.add_rational(&RationalVector::from_int_vector(*a)),
            )),
            (Vector::Rational(a), Vector::Int(b)) => Vector::Rational(Box::new(
                a.add_rational(&RationalVector::from_int_vector(*b)),
            )),
            (Vector::Rational(a), Vector::Rational(b)) => {
                Vector::Rational(Box::new(b.add_rational(a)))
            }
        }
    }

    /// `OnTheLeft` if this vector is on the left of the line from zero through `other`,
    /// `OnTheRight` if on the right, `Collinear` otherwise.
    pub fn side_of(&self, other: &Vector) -> Side {
        match (self, other) {
            (Vector::Int(a), Vector::Int(b)) => a.side_of_int(*b),
            (Vector::Int(a), Vector::Rational(b)) => b
                .side_of_rational(&RationalVector::from_int_vector(*a))
                .negate(),
            (Vector::Rational(a), Vector::Int(b)) => {
                a.side_of_rational(&RationalVector::from_int_vector(*b))
            }
            (Vector::Rational(a), Vector::Rational(b)) => b.side_of_rational(a).negate(),
        }
    }

    /// True if horizontal or vertical.
    pub fn is_orthogonal(&self) -> bool {
        match self {
            Vector::Int(v) => v.is_orthogonal(),
            Vector::Rational(v) => v.is_orthogonal(),
        }
    }

    /// True if diagonal.
    pub fn is_diagonal(&self) -> bool {
        match self {
            Vector::Int(v) => v.is_diagonal(),
            Vector::Rational(v) => v.is_diagonal(),
        }
    }

    /// True if orthogonal or diagonal.
    pub fn is_multiple_of_45_degree(&self) -> bool {
        self.is_orthogonal() || self.is_diagonal()
    }

    /// The signum of the scalar product of this vector and `other`.
    pub fn projection(&self, other: &Vector) -> Signum {
        match (self, other) {
            (Vector::Int(a), Vector::Int(b)) => b.projection_int(*a),
            (Vector::Int(a), Vector::Rational(b)) => {
                b.projection_rational(&RationalVector::from_int_vector(*a))
            }
            (Vector::Rational(a), Vector::Int(b)) => {
                a.projection_rational(&RationalVector::from_int_vector(*b))
            }
            (Vector::Rational(a), Vector::Rational(b)) => b.projection_rational(a),
        }
    }

    /// Approximation of the scalar product with `other`.
    pub fn scalar_product(&self, other: &Vector) -> f64 {
        match (self, other) {
            (Vector::Int(a), Vector::Int(b)) => b.scalar_product_int(*a),
            _ => {
                let v1 = self.to_float();
                let v2 = other.to_float();
                v1.x * v2.x + v1.y * v2.y
            }
        }
    }

    /// Approximates the coordinates by doubles.
    pub fn to_float(&self) -> FloatPoint {
        match self {
            Vector::Int(v) => v.to_float(),
            Vector::Rational(v) => v.to_float(),
        }
    }

    /// Turns this vector by `factor` times 90 degrees.
    pub fn turn_90_degree(&self, factor: i32) -> Vector {
        match self {
            Vector::Int(v) => Vector::Int(v.turn_90_degree(factor)),
            Vector::Rational(v) => Vector::Rational(Box::new(v.turn_90_degree(factor))),
        }
    }

    /// Mirrors at the x axis.
    pub fn mirror_at_x_axis(&self) -> Vector {
        match self {
            Vector::Int(v) => Vector::Int(v.mirror_at_x_axis()),
            Vector::Rational(v) => Vector::Rational(Box::new(v.mirror_at_x_axis())),
        }
    }

    /// Mirrors at the y axis.
    pub fn mirror_at_y_axis(&self) -> Vector {
        match self {
            Vector::Int(v) => Vector::Int(v.mirror_at_y_axis()),
            Vector::Rational(v) => Vector::Rational(Box::new(v.mirror_at_y_axis())),
        }
    }

    /// Approximation of the Euclidean length.
    pub fn length_approx(&self) -> f64 {
        self.to_float().size()
    }

    /// Approximation of the cosine of the angle between this vector and `other`.
    pub fn cos_angle(&self, other: &Vector) -> f64 {
        let mut result = self.scalar_product(other);
        result /= self.to_float().size() * other.to_float().size();
        result
    }

    /// Approximation of the signed angle between this vector and `other`.
    pub fn angle_approx_to(&self, other: &Vector) -> f64 {
        let mut result = self.cos_angle(other).acos();
        if self.side_of(other) == Side::OnTheLeft {
            result = -result;
        }
        result
    }

    /// Approximation of the signed angle between this vector and the x axis.
    pub fn angle_approx(&self) -> f64 {
        Vector::Int(IntVector::new(1, 0)).angle_approx_to(self)
    }

    /// An approximation vector with the same direction and length `length`. Not
    /// implemented for rational vectors, which are returned unchanged (as in Java).
    pub fn change_length_approx(&self, length: f64) -> Vector {
        match self {
            Vector::Int(v) => Vector::Int(v.change_length_approx(length)),
            Vector::Rational(_) => self.clone(),
        }
    }

    /// The normalized direction of this vector.
    pub fn to_normalized_direction(&self) -> Direction {
        match self {
            Vector::Int(v) => Direction::Int(v.to_normalized_direction()),
            Vector::Rational(v) => v.to_normalized_direction(),
        }
    }

    /// Java `addTo(IntPoint)`.
    pub(crate) fn add_to_int_point(&self, point: IntPoint) -> Point {
        match self {
            Vector::Int(v) => Point::Int(point.translate_by_int(*v)),
            Vector::Rational(v) => Point::Rational(Arc::new(v.add_to_int_point(point))),
        }
    }

    /// Java `addTo(RationalPoint)`.
    pub(crate) fn add_to_rational_point(&self, point: &RationalPoint) -> Point {
        match self {
            Vector::Int(v) => Point::Rational(Arc::new(
                point.translate_by_rational(&RationalVector::from_int_vector(*v)),
            )),
            Vector::Rational(v) => Point::Rational(Arc::new(v.add_to_rational_point(point))),
        }
    }

    /// The vector as an `IntVector`.
    ///
    /// # Panics
    /// If the vector is rational (a `ClassCastException` in Java).
    pub fn expect_int(&self) -> IntVector {
        match self {
            Vector::Int(v) => *v,
            Vector::Rational(_) => {
                panic!("operation only implemented for IntVectors (ClassCastException in Java)")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rv(x: i64, y: i64, z: i64) -> Vector {
        Vector::Rational(Box::new(RationalVector::new(
            BigInt::from(x),
            BigInt::from(y),
            BigInt::from(z),
        )))
    }

    #[test]
    fn side_of_is_consistent_across_representations() {
        let a = Vector::Int(IntVector::new(1, 2));
        let b = Vector::Int(IntVector::new(3, 1));
        assert_eq!(a.side_of(&b), Side::OnTheLeft);
        assert_eq!(b.side_of(&a), Side::OnTheRight);
        let ra = rv(2, 4, 2);
        let rb = rv(6, 2, 2);
        assert_eq!(ra.side_of(&b), Side::OnTheLeft);
        assert_eq!(a.side_of(&rb), Side::OnTheLeft);
        assert_eq!(ra.side_of(&rb), Side::OnTheLeft);
        assert_eq!(rb.side_of(&ra), Side::OnTheRight);
        assert_eq!(a.side_of(&ra), Side::Collinear);
    }

    #[test]
    fn projection_and_scalar_product() {
        let a = Vector::Int(IntVector::new(1, 0));
        let b = Vector::Int(IntVector::new(-1, 5));
        assert_eq!(a.projection(&b), Signum::Negative);
        assert_eq!(a.projection(&rv(1, 9, 3)), Signum::Positive);
        assert_eq!(a.scalar_product(&b), -1.0);
        assert_eq!(rv(1, 0, 2).scalar_product(&b), -0.5);
    }

    #[test]
    fn equality_respects_representation() {
        assert_eq!(rv(1, 2, 1), rv(2, 4, 2));
        assert_ne!(Vector::Int(IntVector::new(1, 2)), rv(1, 2, 1));
        assert!(Vector::ZERO.is_zero_constant());
        assert!(!rv(0, 0, 1).is_zero_constant());
    }

    #[test]
    fn get_instance_and_directions() {
        assert_eq!(
            Vector::get_instance_big(BigInt::from(6), BigInt::from(-4), BigInt::from(-2)),
            Vector::Int(IntVector::new(-3, 2))
        );
        assert!(matches!(
            Vector::get_instance(CRIT_INT + 1, 0),
            Vector::Rational(_)
        ));
        assert_eq!(
            IntVector::new(6, -4).to_normalized_direction(),
            IntDirection::new(3, -2)
        );
        let big = rv(i64::from(CRIT_INT) * 4 + 4, 4, 1);
        assert!(matches!(
            big.to_normalized_direction(),
            Direction::BigInt(_)
        ));
        assert_eq!(
            rv(10, 20, 7).to_normalized_direction(),
            Direction::Int(IntDirection::new(1, 2))
        );
    }

    #[test]
    fn turning_and_angles() {
        let v = Vector::Int(IntVector::new(3, 4));
        assert_eq!(v.turn_90_degree(1), Vector::Int(IntVector::new(-4, 3)));
        assert_eq!(v.turn_90_degree(-1), Vector::Int(IntVector::new(4, -3)));
        assert_eq!(rv(3, 4, 1).turn_90_degree(2), rv(-3, -4, 1));
        let up = Vector::Int(IntVector::new(0, 1));
        assert!((up.angle_approx() - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        assert_eq!(
            Vector::Int(IntVector::new(3, 4)).change_length_approx(10.0),
            Vector::Int(IntVector::new(6, 8))
        );
    }
}
