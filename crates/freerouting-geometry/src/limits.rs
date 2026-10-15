//! Port of `app.freerouting.geometry.planar.Limits`: numerical limits used by the
//! planar geometry.

use num_bigint::BigInt;

/// An upper bound (2^25) so that the product of two integers with absolute value at most
/// `CRIT_INT` is contained in the mantissa of an `f64` with some space left for addition.
///
/// Java: `Limits.CRIT_INT`.
pub const CRIT_INT: i32 = 1 << 25;

/// The biggest double value (2^53), so that all integers smaller than this value are
/// represented exactly as `f64` values.
///
/// Java: `Limits.CRIT_DOUBLE`.
pub const CRIT_DOUBLE: f64 = 9_007_199_254_740_992.0;

/// `Math.sqrt(2)`; the correctly rounded square root of two.
///
/// Java: `Limits.sqrt2`.
pub const SQRT2: f64 = std::f64::consts::SQRT_2;

/// `CRIT_INT` as a big integer.
///
/// Java: `Limits.CRIT_INT_BIG`.
pub fn crit_int_big() -> BigInt {
    BigInt::from(CRIT_INT)
}

/// Java `Integer.MAX_VALUE` as an `f64`, used as an "infinite" coordinate marker.
pub(crate) const INT_MAX_F64: f64 = i32::MAX as f64;

/// Java `Integer.MIN_VALUE` as an `f64`.
pub(crate) const INT_MIN_F64: f64 = i32::MIN as f64;

/// Java `Float.MAX_VALUE` widened to `f64`.
pub(crate) const FLOAT_MAX_F64: f64 = f32::MAX as f64;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqrt2_matches_math_sqrt() {
        assert_eq!(SQRT2, 2f64.sqrt());
    }

    #[test]
    fn crit_double_is_two_to_the_53() {
        assert_eq!(CRIT_DOUBLE, (1u64 << 53) as f64);
    }
}
