//! Ports of `app.freerouting.geometry.planar.Direction`, `IntDirection`, `BigIntDirection`
//! and `FortyfiveDegreeDirection`.

use std::fmt;

use num_bigint::BigInt;
use num_traits::{One, Signed, Zero};

use crate::float_point::FloatPoint;
use crate::jmath::round_to_i32;
use crate::point::Point;
use crate::side::Side;
use crate::signum::Signum;
use crate::vector::{IntVector, RationalVector, Vector};

/// A direction given by an integer vector. Directions created through
/// [`Direction::get_instance`] are normalized (coordinates divided by their gcd).
///
/// Java: `app.freerouting.geometry.planar.IntDirection`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IntDirection {
    pub x: i32,
    pub y: i32,
}

impl IntDirection {
    pub const NULL: IntDirection = IntDirection::new(0, 0);
    pub const RIGHT: IntDirection = IntDirection::new(1, 0);
    pub const RIGHT45: IntDirection = IntDirection::new(1, 1);
    pub const UP: IntDirection = IntDirection::new(0, 1);
    pub const UP45: IntDirection = IntDirection::new(-1, 1);
    pub const LEFT: IntDirection = IntDirection::new(-1, 0);
    pub const LEFT45: IntDirection = IntDirection::new(-1, -1);
    pub const DOWN: IntDirection = IntDirection::new(0, -1);
    pub const DOWN45: IntDirection = IntDirection::new(1, -1);

    /// Creates a direction from raw coordinates (not normalized).
    pub const fn new(x: i32, y: i32) -> IntDirection {
        IntDirection { x, y }
    }

    /// True if horizontal or vertical.
    pub fn is_orthogonal(&self) -> bool {
        self.x == 0 || self.y == 0
    }

    /// True if diagonal.
    pub fn is_diagonal(&self) -> bool {
        self.x.wrapping_abs() == self.y.wrapping_abs()
    }

    /// A vector pointing into this direction.
    pub fn get_vector(&self) -> IntVector {
        IntVector::new(self.x, self.y)
    }

    /// Java `IntDirection.compareTo(IntDirection other)` with `self` as `this`.
    fn compare_to_int_raw(&self, other: &IntDirection) -> i32 {
        if self.y > 0 {
            if other.y < 0 {
                return -1;
            }
            if other.y == 0 {
                if other.x > 0 {
                    return 1;
                }
                return -1;
            }
        } else if self.y < 0 {
            if other.y >= 0 {
                return 1;
            }
        } else {
            if self.x > 0 {
                if other.y != 0 || other.x < 0 {
                    return -1;
                }
                return 0;
            }
            if other.y > 0 || other.y == 0 && other.x > 0 {
                return 1;
            }
            if other.y < 0 {
                return -1;
            }
            return 0;
        }
        let determinant =
            f64::from(other.x) * f64::from(self.y) - f64::from(other.y) * f64::from(self.x);
        Signum::as_int(determinant)
    }

    /// The opposite direction.
    pub fn opposite(&self) -> IntDirection {
        IntDirection::new(self.x.wrapping_neg(), self.y.wrapping_neg())
    }

    /// Turns the direction by `factor` times 45 degrees. As in Java, a negative factor
    /// (whose `% 8` is negative) yields the null direction.
    pub fn turn_45_degree(&self, factor: i32) -> IntDirection {
        let (x, y) = (self.x, self.y);
        match factor % 8 {
            0 => IntDirection::new(x, y),
            1 => IntDirection::new(x.wrapping_sub(y), x.wrapping_add(y)),
            2 => IntDirection::new(y.wrapping_neg(), x),
            3 => IntDirection::new(x.wrapping_neg().wrapping_sub(y), x.wrapping_sub(y)),
            4 => IntDirection::new(x.wrapping_neg(), y.wrapping_neg()),
            5 => IntDirection::new(y.wrapping_sub(x), x.wrapping_neg().wrapping_sub(y)),
            6 => IntDirection::new(y, x.wrapping_neg()),
            7 => IntDirection::new(x.wrapping_add(y), y.wrapping_sub(x)),
            _ => IntDirection::new(0, 0),
        }
    }

    /// Determinant with `other` computed in doubles.
    pub fn determinant(&self, other: &IntDirection) -> f64 {
        f64::from(self.x) * f64::from(other.y) - f64::from(self.y) * f64::from(other.x)
    }

    /// True if this is the null direction `(0, 0)`.
    pub fn is_null(&self) -> bool {
        self.x == 0 && self.y == 0
    }
}

/// A direction given by a vector with infinite precision integer coordinates.
///
/// Java: `app.freerouting.geometry.planar.BigIntDirection`.
#[derive(Debug, Clone)]
pub struct BigIntDirection {
    x: BigInt,
    y: BigInt,
}

impl BigIntDirection {
    /// Creates a direction from raw coordinates.
    pub fn new(x: BigInt, y: BigInt) -> BigIntDirection {
        BigIntDirection { x, y }
    }

    /// Converts an [`IntDirection`].
    pub fn from_int_direction(dir: &IntDirection) -> BigIntDirection {
        BigIntDirection::new(BigInt::from(dir.x), BigInt::from(dir.y))
    }

    /// The x coordinate.
    pub fn x(&self) -> &BigInt {
        &self.x
    }

    /// The y coordinate.
    pub fn y(&self) -> &BigInt {
        &self.y
    }

    fn compare_to_big_raw(&self, other: &BigIntDirection) -> i32 {
        let x1 = sig(&self.x);
        let y1 = sig(&self.y);
        let x2 = sig(&other.x);
        let y2 = sig(&other.y);
        if y1 > 0 {
            if y2 < 0 {
                return -1;
            }
            if y2 == 0 {
                if x2 > 0 {
                    return 1;
                }
                return -1;
            }
        } else if y1 < 0 {
            if y2 >= 0 {
                return 1;
            }
        } else {
            if x1 > 0 {
                if y2 != 0 || x2 < 0 {
                    return -1;
                }
                return 0;
            }
            if y2 > 0 || y2 == 0 && x2 > 0 {
                return 1;
            }
            if y2 < 0 {
                return -1;
            }
            return 0;
        }
        let det = &self.y * &other.x - &self.x * &other.y;
        sig(&det)
    }
}

fn sig(v: &BigInt) -> i32 {
    if v.is_positive() {
        1
    } else if v.is_negative() {
        -1
    } else {
        0
    }
}

/// An equivalence class of vectors pointing into the same direction. Directions are
/// preferred over angles because the arithmetic stays exact.
///
/// Java: the abstract class `app.freerouting.geometry.planar.Direction`.
#[derive(Debug, Clone)]
pub enum Direction {
    Int(IntDirection),
    BigInt(Box<BigIntDirection>),
}

impl From<IntDirection> for Direction {
    fn from(d: IntDirection) -> Self {
        Direction::Int(d)
    }
}

impl Direction {
    pub const NULL: Direction = Direction::Int(IntDirection::NULL);
    pub const RIGHT: Direction = Direction::Int(IntDirection::RIGHT);
    pub const RIGHT45: Direction = Direction::Int(IntDirection::RIGHT45);
    pub const UP: Direction = Direction::Int(IntDirection::UP);
    pub const UP45: Direction = Direction::Int(IntDirection::UP45);
    pub const LEFT: Direction = Direction::Int(IntDirection::LEFT);
    pub const LEFT45: Direction = Direction::Int(IntDirection::LEFT45);
    pub const DOWN: Direction = Direction::Int(IntDirection::DOWN);
    pub const DOWN45: Direction = Direction::Int(IntDirection::DOWN45);

    /// The normalized direction of `vector`.
    pub fn get_instance(vector: &Vector) -> Direction {
        vector.to_normalized_direction()
    }

    /// The direction from `from` to `to`; `None` if the points are equal.
    pub fn get_instance_from_points(from: &Point, to: &Point) -> Option<Direction> {
        if from == to {
            return None;
        }
        Some(Direction::get_instance(&to.difference_by(from)))
    }

    /// A direction whose angle with the x axis is nearly `angle`.
    pub fn get_instance_approx(angle: f64) -> Direction {
        let scale_factor = 10000.0;
        let x = round_to_i32(angle.cos() * scale_factor);
        let y = round_to_i32(angle.sin() * scale_factor);
        Direction::get_instance(&Vector::Int(IntVector::new(x, y)))
    }

    /// The direction as an `IntDirection`.
    ///
    /// # Panics
    /// If it is a `BigIntDirection` (a `ClassCastException` in Java).
    pub fn expect_int(&self) -> IntDirection {
        match self {
            Direction::Int(d) => *d,
            Direction::BigInt(_) => {
                panic!("operation only implemented for IntDirections (ClassCastException in Java)")
            }
        }
    }

    /// Any vector pointing into this direction.
    pub fn get_vector(&self) -> Vector {
        match self {
            Direction::Int(d) => Vector::Int(d.get_vector()),
            Direction::BigInt(d) => Vector::Rational(Box::new(RationalVector::new(
                d.x.clone(),
                d.y.clone(),
                BigInt::one(),
            ))),
        }
    }

    /// True if horizontal or vertical.
    pub fn is_orthogonal(&self) -> bool {
        match self {
            Direction::Int(d) => d.is_orthogonal(),
            Direction::BigInt(d) => d.x.is_zero() || d.y.is_zero(),
        }
    }

    /// True if diagonal.
    pub fn is_diagonal(&self) -> bool {
        match self {
            Direction::Int(d) => d.is_diagonal(),
            Direction::BigInt(d) => d.x.abs() == d.y.abs(),
        }
    }

    /// True if orthogonal or diagonal.
    pub fn is_multiple_of_45_degree(&self) -> bool {
        self.is_orthogonal() || self.is_diagonal()
    }

    /// Turns the direction by `factor` times 45 degrees. Not implemented for
    /// `BigIntDirection`, which is returned unchanged (as in Java).
    pub fn turn_45_degree(&self, factor: i32) -> Direction {
        match self {
            Direction::Int(d) => Direction::Int(d.turn_45_degree(factor)),
            Direction::BigInt(_) => self.clone(),
        }
    }

    /// The opposite direction.
    pub fn opposite(&self) -> Direction {
        match self {
            Direction::Int(d) => Direction::Int(d.opposite()),
            Direction::BigInt(d) => Direction::BigInt(Box::new(BigIntDirection::new(-&d.x, -&d.y))),
        }
    }

    /// The side of `other.get_vector()` on which `self.get_vector()` lies.
    pub fn side_of(&self, other: &Direction) -> Side {
        match (self, other) {
            (Direction::Int(a), Direction::Int(b)) => a.get_vector().side_of_int(b.get_vector()),
            _ => self.get_vector().side_of(&other.get_vector()),
        }
    }

    /// The signum of the scalar product of vectors representing the two directions.
    pub fn projection(&self, other: &Direction) -> Signum {
        self.get_vector().projection(&other.get_vector())
    }

    /// Approximation of the direction in the middle of this direction and `other`.
    pub fn middle_approx(&self, other: &Direction) -> Direction {
        let v1 = self.get_vector().to_float();
        let v2 = other.get_vector().to_float();
        let length1 = v1.size();
        let length2 = v2.size();
        let x = v1.x / length1 + v2.x / length2;
        let y = v1.y / length1 + v2.y / length2;
        let scale_factor = 1000.0;
        let vm = IntVector::new(
            round_to_i32(x * scale_factor),
            round_to_i32(y * scale_factor),
        );
        Direction::get_instance(&Vector::Int(vm))
    }

    /// Java `compareTo`: 1 if this direction has a strictly bigger angle with the positive
    /// x axis than `other`, 0 if equal, -1 otherwise. Reproduces Java's double dispatch,
    /// including its asymmetric treatment of the null direction.
    pub fn compare_to(&self, other: &Direction) -> i32 {
        // Java, kept in both profiles: Java's double dispatch orders the null
        // direction lopsidedly. N06.12 left it (a convention, not a bug).
        match (self, other) {
            (Direction::Int(a), Direction::Int(b)) => -b.compare_to_int_raw(a),
            (Direction::Int(a), Direction::BigInt(b)) => {
                -b.compare_to_big_raw(&BigIntDirection::from_int_direction(a))
            }
            (Direction::BigInt(a), Direction::Int(b)) => {
                a.compare_to_big_raw(&BigIntDirection::from_int_direction(b))
            }
            (Direction::BigInt(a), Direction::BigInt(b)) => -b.compare_to_big_raw(a),
        }
    }

    /// Compares the angles of `p1` and `p2` measured from this direction.
    pub fn compare_from(&self, p1: &Direction, p2: &Direction) -> i32 {
        if p1.compare_to(self) >= 0 {
            if p2.compare_to(self) >= 0 {
                p1.compare_to(p2)
            } else {
                -1
            }
        } else if p2.compare_to(self) >= 0 {
            1
        } else {
            p1.compare_to(p2)
        }
    }

    /// Approximation of the signed angle of this direction.
    pub fn angle_approx(&self) -> f64 {
        self.get_vector().angle_approx()
    }

    /// The direction as a [`FloatPoint`] vector.
    pub fn to_float(&self) -> FloatPoint {
        self.get_vector().to_float()
    }
}

impl PartialEq for Direction {
    /// Java `Direction.equals`: same class, collinear and pointing the same way.
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Direction::Int(_), Direction::BigInt(_))
            | (Direction::BigInt(_), Direction::Int(_)) => false,
            _ => {
                if self.side_of(other) != Side::Collinear {
                    return false;
                }
                self.get_vector().projection(&other.get_vector()) == Signum::Positive
            }
        }
    }
}

impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names = [
            (Direction::RIGHT, "RIGHT"),
            (Direction::RIGHT45, "UP-RIGHT"),
            (Direction::UP, "UP"),
            (Direction::UP45, "UP-LEFT"),
            (Direction::LEFT, "LEFT"),
            (Direction::LEFT45, "DOWN-LEFT"),
            (Direction::DOWN, "DOWN"),
            (Direction::DOWN45, "DOWN-RIGHT"),
            (Direction::NULL, "NULL"),
        ];
        for (dir, name) in names {
            if self.compare_to(&dir) == 0 {
                return f.write_str(name);
            }
        }
        f.write_str("UNKNOWN")
    }
}

/// The eight 45-degree directions, counterclockwise from right.
///
/// Java: `app.freerouting.geometry.planar.FortyfiveDegreeDirection`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FortyfiveDegreeDirection {
    Right,
    Right45,
    Up,
    Up45,
    Left,
    Left45,
    Down,
    Down45,
}

impl FortyfiveDegreeDirection {
    /// All values in declaration order (`values()`).
    pub const VALUES: [FortyfiveDegreeDirection; 8] = [
        FortyfiveDegreeDirection::Right,
        FortyfiveDegreeDirection::Right45,
        FortyfiveDegreeDirection::Up,
        FortyfiveDegreeDirection::Up45,
        FortyfiveDegreeDirection::Left,
        FortyfiveDegreeDirection::Left45,
        FortyfiveDegreeDirection::Down,
        FortyfiveDegreeDirection::Down45,
    ];

    /// The corresponding [`IntDirection`].
    pub fn get_direction(self) -> IntDirection {
        match self {
            FortyfiveDegreeDirection::Right => IntDirection::RIGHT,
            FortyfiveDegreeDirection::Right45 => IntDirection::RIGHT45,
            FortyfiveDegreeDirection::Up => IntDirection::UP,
            FortyfiveDegreeDirection::Up45 => IntDirection::UP45,
            FortyfiveDegreeDirection::Left => IntDirection::LEFT,
            FortyfiveDegreeDirection::Left45 => IntDirection::LEFT45,
            FortyfiveDegreeDirection::Down => IntDirection::DOWN,
            FortyfiveDegreeDirection::Down45 => IntDirection::DOWN45,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(x: i32, y: i32) -> Direction {
        Direction::Int(IntDirection::new(x, y))
    }

    fn big(x: i64, y: i64) -> Direction {
        Direction::BigInt(Box::new(BigIntDirection::new(
            BigInt::from(x),
            BigInt::from(y),
        )))
    }

    #[test]
    fn compare_orders_by_angle() {
        let order = [
            Direction::RIGHT,
            Direction::RIGHT45,
            Direction::UP,
            Direction::UP45,
            Direction::LEFT,
            Direction::LEFT45,
            Direction::DOWN,
            Direction::DOWN45,
        ];
        for (i, a) in order.iter().enumerate() {
            for (j, b) in order.iter().enumerate() {
                assert_eq!(
                    a.compare_to(b),
                    (i as i32 - j as i32).signum(),
                    "{a} vs {b}"
                );
            }
        }
        assert_eq!(d(2, 1).compare_to(&big(1, 1)), -1);
        assert_eq!(big(1, 1).compare_to(&d(2, 1)), 1);
        assert_eq!(big(-1, -3).compare_to(&big(1, -3)), -1);
    }

    #[test]
    fn null_direction_compare_is_asymmetric_like_java() {
        // IntDirection.compareTo for (y == 0, x == 0) falls into the "x < 0" branch.
        assert_eq!(Direction::RIGHT.compare_to(&Direction::NULL), -1);
        assert_eq!(Direction::NULL.compare_to(&Direction::RIGHT), 0);
    }

    #[test]
    fn turning() {
        assert_eq!(IntDirection::RIGHT.turn_45_degree(1), IntDirection::RIGHT45);
        assert_eq!(IntDirection::RIGHT.turn_45_degree(2), IntDirection::UP);
        assert_eq!(
            IntDirection::new(3, 1).turn_45_degree(1),
            IntDirection::new(2, 4)
        );
        assert_eq!(IntDirection::UP.turn_45_degree(6), IntDirection::RIGHT);
        assert_eq!(IntDirection::UP.turn_45_degree(-2), IntDirection::NULL);
        assert_eq!(Direction::UP.opposite(), Direction::DOWN);
    }

    #[test]
    fn equality_and_display() {
        assert_eq!(d(2, 4), d(1, 2));
        assert_ne!(d(1, 2), d(-1, -2));
        assert_ne!(d(1, 1), big(1, 1));
        assert_eq!(big(2, 2), big(5, 5));
        assert_eq!(d(5, 5).to_string(), "UP-RIGHT");
        assert_eq!(d(0, -7).to_string(), "DOWN");
        assert_eq!(d(3, 1).to_string(), "UNKNOWN");
        assert_eq!(Direction::get_instance_approx(0.0), Direction::RIGHT);
        assert_eq!(
            Direction::get_instance_approx(std::f64::consts::FRAC_PI_4),
            Direction::RIGHT45
        );
        assert_eq!(
            Direction::RIGHT.middle_approx(&Direction::UP),
            Direction::RIGHT45
        );
    }

    #[test]
    fn fortyfive_degree_directions() {
        let dirs: Vec<IntDirection> = FortyfiveDegreeDirection::VALUES
            .iter()
            .map(|d| d.get_direction())
            .collect();
        assert_eq!(dirs[0], IntDirection::RIGHT);
        assert_eq!(dirs[7], IntDirection::DOWN45);
    }
}
