//! Port of `app.freerouting.datastructures.BigIntAux` plus helpers that reproduce the
//! `java.math.BigInteger` conversions the geometry relies on.

use num_bigint::{BigInt, Sign};
use num_integer::Integer;
use num_traits::{ToPrimitive, Zero};

/// Calculates the determinant of the vectors `(x1, y1)` and `(x2, y2)`: `x1 * y2 - x2 * y1`.
///
/// Java: `BigIntAux.determinant`.
pub fn determinant(x1: &BigInt, y1: &BigInt, x2: &BigInt, y2: &BigInt) -> BigInt {
    x1 * y2 - x2 * y1
}

/// Adds two rational coordinate triples `(x, y, z)` meaning `(x / z, y / z)`.
///
/// Java: `BigIntAux.addRationalCoordinates`.
pub fn add_rational_coordinates(first: [&BigInt; 3], second: [&BigInt; 3]) -> [BigInt; 3] {
    if first[2] == second[2] {
        [first[0] + second[0], first[1] + second[1], first[2].clone()]
    } else {
        let z = first[2] * second[2];
        let x = first[0] * second[2] + second[0] * first[2];
        let y = first[1] * second[2] + second[1] * first[2];
        [x, y, z]
    }
}

/// Greatest common divisor of `a` and `b` interpreted as unsigned 32-bit integers.
///
/// Java: `BigIntAux.binaryGcd`. The result is returned as the same bit pattern Java
/// returns (so a gcd of 2^31 comes back negative).
pub fn binary_gcd(a: i32, b: i32) -> i32 {
    let mut a = a as u32;
    let mut b = b as u32;
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a as i32
}

/// `BigInteger.doubleValue()`: the nearest double (ties to even), infinite on overflow.
pub fn to_f64(value: &BigInt) -> f64 {
    value.to_f64().unwrap_or(match value.sign() {
        Sign::Minus => f64::NEG_INFINITY,
        _ => f64::INFINITY,
    })
}

/// `BigInteger.intValue()`: the low 32 bits in two's complement.
pub fn to_i32_wrapping(value: &BigInt) -> i32 {
    let modulus = BigInt::from(1u64 << 32);
    let low = value.mod_floor(&modulus);
    low.to_u32().unwrap_or(0) as i32
}

/// `BigInteger.hashCode()`.
pub fn java_hash_code(value: &BigInt) -> i32 {
    let mut hash: i32 = 0;
    for word in value.magnitude().to_u32_digits().iter().rev() {
        hash = hash.wrapping_mul(31).wrapping_add(*word as i32);
    }
    match value.sign() {
        Sign::Minus => hash.wrapping_neg(),
        Sign::NoSign => 0,
        Sign::Plus => hash,
    }
}

/// `x.mod(m).signum() == 0` for a positive modulus: true if `m` divides `x`.
pub(crate) fn divides(m: &BigInt, x: &BigInt) -> bool {
    if m.is_zero() {
        panic!("BigInteger: modulus not positive");
    }
    (x % m).is_zero()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gcd_behaves_like_java() {
        assert_eq!(binary_gcd(12, 18), 6);
        assert_eq!(binary_gcd(0, 7), 7);
        assert_eq!(binary_gcd(7, 0), 7);
        assert_eq!(binary_gcd(0, 0), 0);
        // Math.abs(Integer.MIN_VALUE) is still negative; the unsigned gcd is 2^31.
        assert_eq!(binary_gcd(i32::MIN, 0), i32::MIN);
        assert_eq!(binary_gcd(i32::MIN, 6), 2);
    }

    #[test]
    fn rational_addition() {
        let one = BigInt::from(1);
        let two = BigInt::from(2);
        let three = BigInt::from(3);
        let r = add_rational_coordinates([&one, &two, &three], [&two, &one, &three]);
        assert_eq!(r, [three.clone(), three.clone(), three.clone()]);
        let r = add_rational_coordinates([&one, &one, &two], [&one, &one, &three]);
        assert_eq!(r, [BigInt::from(5), BigInt::from(5), BigInt::from(6)]);
    }

    #[test]
    fn conversions_match_big_integer() {
        assert_eq!(to_i32_wrapping(&BigInt::from(-1)), -1);
        assert_eq!(to_i32_wrapping(&(BigInt::from(1u64 << 32) + 5)), 5);
        assert_eq!(to_i32_wrapping(&BigInt::from(0x8000_0000u64)), i32::MIN);
        assert_eq!(java_hash_code(&BigInt::from(0)), 0);
        assert_eq!(java_hash_code(&BigInt::from(5)), 5);
        assert_eq!(java_hash_code(&BigInt::from(-5)), -5);
        // new BigInteger("123456789012345678901234567890").hashCode()
        let big: BigInt = "123456789012345678901234567890".parse().unwrap();
        assert_eq!(java_hash_code(&big), JAVA_HASH_OF_BIG);
        // 2^53 + 1 is not representable; BigInteger.doubleValue rounds half to even.
        let v = BigInt::from((1u64 << 53) + 1);
        assert_eq!(to_f64(&v), 9_007_199_254_740_992.0);
        let v = BigInt::from((1u64 << 53) + 3);
        assert_eq!(to_f64(&v), 9_007_199_254_740_996.0);
    }

    const JAVA_HASH_OF_BIG: i32 = 1_915_528_825;
}
