//! Helpers reproducing Java numeric semantics exactly (`Math.round`, `(int)` casts,
//! `Math.rint`, `Math.max` of floats, `DoubleStream.sum`, `java.util.Random`).

/// Java `Math.round(double)`: the closest `long`, ties rounding towards positive infinity.
/// `NaN` maps to 0 and out-of-range values saturate.
pub fn java_round(x: f64) -> i64 {
    if x.is_nan() {
        return 0;
    }
    let floor = x.floor();
    let rounded = if x - floor >= 0.5 { floor + 1.0 } else { floor };
    rounded as i64
}

/// Java `(int) Math.round(x)`: rounds to a `long`, then keeps the low 32 bits.
pub fn round_to_i32(x: f64) -> i32 {
    java_round(x) as i32
}

/// Java `(int) x` for a double: truncation towards zero, saturating, `NaN` to 0.
pub fn d2i(x: f64) -> i32 {
    x as i32
}

/// Java `Math.floorMod(int, int)`.
pub fn floor_mod(x: i32, y: i32) -> i32 {
    x.rem_euclid(y)
}

/// Java `Math.rint`: round half to even.
pub fn rint(x: f64) -> f64 {
    x.round_ties_even()
}

/// Java `Math.max(float, float)`: a `NaN` argument gives `NaN`, and `+0.0` is
/// greater than `-0.0`. Rust's `f32::max` returns the other argument for a `NaN`.
pub fn max_f32(a: f32, b: f32) -> f32 {
    if a.is_nan() {
        return a;
    }
    if a == 0.0 && b == 0.0 && a.is_sign_negative() {
        return b;
    }
    if a >= b { a } else { b }
}

/// Java `Math.max(double, double)`, with the `NaN` and signed-zero rules of
/// [`max_f32`].
pub fn max_f64(a: f64, b: f64) -> f64 {
    if a.is_nan() {
        return a;
    }
    if a == 0.0 && b == 0.0 && a.is_sign_negative() {
        return b;
    }
    if a >= b { a } else { b }
}

/// Java `DoubleStream.sum()` (JDK 25): Kahan summation in input order
/// (`Collectors.sumWithCompensation`), and the final sum minus the
/// compensation; a `NaN` result with an infinite plain sum gives the plain
/// sum (`computeFinalSum`). An empty input sums to `+0.0`. A plain
/// left-to-right sum, or Rust's `Sum` (which starts at `-0.0`), can differ
/// in the last bits.
pub fn double_stream_sum(values: impl IntoIterator<Item = f64>) -> f64 {
    let (mut sum, mut compensation, mut simple) = (0.0f64, 0.0f64, 0.0f64);
    for value in values {
        let tmp = value - compensation;
        let velvel = sum + tmp;
        compensation = (velvel - sum) - tmp;
        sum = velvel;
        simple += value;
    }
    let tmp = sum - compensation;
    if tmp.is_nan() && simple.is_infinite() {
        simple
    } else {
        tmp
    }
}

/// A bit-exact port of `java.util.Random` (48-bit linear congruential generator).
#[derive(Debug, Clone)]
pub struct JavaRandom {
    seed: i64,
}

impl JavaRandom {
    const MULTIPLIER: i64 = 0x5_DEEC_E66D;
    const ADDEND: i64 = 0xB;
    const MASK: i64 = (1 << 48) - 1;

    /// Creates a generator with the given seed (`new Random(seed)`).
    pub fn new(seed: i64) -> JavaRandom {
        JavaRandom {
            seed: (seed ^ Self::MULTIPLIER) & Self::MASK,
        }
    }

    /// `Random.setSeed(seed)`.
    pub fn set_seed(&mut self, seed: i64) {
        self.seed = (seed ^ Self::MULTIPLIER) & Self::MASK;
    }

    fn next(&mut self, bits: u32) -> i32 {
        self.seed = (self
            .seed
            .wrapping_mul(Self::MULTIPLIER)
            .wrapping_add(Self::ADDEND))
            & Self::MASK;
        ((self.seed as u64) >> (48 - bits)) as i32
    }

    /// `Random.nextDouble()`: 53 random bits times 2^-53.
    pub fn next_double(&mut self) -> f64 {
        let high = i64::from(self.next(26)) << 27;
        (high + i64::from(self.next(27))) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// `Random.nextBoolean()`.
    pub fn next_boolean(&mut self) -> bool {
        self.next(1) != 0
    }

    /// `Random.nextInt(bound)`; panics if `bound <= 0`, as Java throws.
    pub fn next_int(&mut self, bound: i32) -> i32 {
        assert!(bound > 0, "bound must be positive");
        let mut r = self.next(31);
        let m = bound - 1;
        if bound & m == 0 {
            ((i64::from(bound) * i64::from(r)) >> 31) as i32
        } else {
            let mut u = r;
            loop {
                r = u % bound;
                if u.wrapping_sub(r).wrapping_add(m) >= 0 {
                    break;
                }
                u = self.next(31);
            }
            r
        }
    }

    /// `Collections.shuffle(list, random)`.
    pub fn shuffle<T>(&mut self, list: &mut [T]) {
        for i in (2..=list.len()).rev() {
            let j = self.next_int(i as i32) as usize;
            list.swap(i - 1, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_round_matches_java_semantics() {
        assert_eq!(java_round(2.5), 3);
        assert_eq!(java_round(-2.5), -2);
        assert_eq!(java_round(-0.5), 0);
        assert_eq!(java_round(0.499_999_999_999_999_94), 0);
        assert_eq!(java_round(-1.5), -1);
        assert_eq!(java_round(f64::NAN), 0);
        assert_eq!(java_round(1e300), i64::MAX);
        assert_eq!(java_round(-1e300), i64::MIN);
        // (int) of a long keeps the low 32 bits.
        assert_eq!(round_to_i32(4_294_967_297.0), 1);
    }

    #[test]
    fn d2i_saturates_like_java() {
        assert_eq!(d2i(1e20), i32::MAX);
        assert_eq!(d2i(-1e20), i32::MIN);
        assert_eq!(d2i(f64::NAN), 0);
        assert_eq!(d2i(-2.7), -2);
    }

    #[test]
    fn rint_is_half_even() {
        assert_eq!(rint(2.5), 2.0);
        assert_eq!(rint(3.5), 4.0);
        assert_eq!(rint(-2.5), -2.0);
    }

    #[test]
    fn java_random_sequence() {
        // Values produced by `new java.util.Random(99)`.
        let mut r = JavaRandom::new(99);
        let got: Vec<i32> = (0..5).map(|_| r.next_int(10)).collect();
        assert_eq!(got, vec![7, 8, 9, 1, 0]);
        let mut r = JavaRandom::new(42);
        assert_eq!(r.next_int(100), 30);
        assert_eq!(r.next_int(100), 63);
        assert_eq!(r.next_int(16), 10);
        // Powers of two take the multiply branch.
        let mut r = JavaRandom::new(7);
        let got: Vec<i32> = (0..3).map(|_| r.next_int(16)).collect();
        assert_eq!(got, vec![11, 10, 11]);
    }

    #[test]
    fn shuffle_matches_java() {
        // List<Integer> l = 0..9; Collections.shuffle(l, new Random(99))
        let mut list: Vec<i32> = (0..10).collect();
        JavaRandom::new(99).shuffle(&mut list);
        assert_eq!(list, [8, 1, 4, 3, 9, 6, 0, 2, 5, 7]);
        let mut short = vec![0, 1, 2];
        JavaRandom::new(99).shuffle(&mut short);
        assert_eq!(short, [2, 0, 1]);
    }

    /// `Math.max` on every pair of signed zeros, NaN, infinities and ±1.5, as
    /// `tools/java-reference/probes/JavaMathMaxProbe.java` printed them at
    /// `a773a70ec` (raw bits of a, b and the result).
    #[test]
    fn max_matches_java_math_max() {
        const JAVA_MAX_F32: [(u32, u32, u32); 49] = [
            (0x00000000, 0x00000000, 0x00000000),
            (0x00000000, 0x80000000, 0x00000000),
            (0x00000000, 0x3fc00000, 0x3fc00000),
            (0x00000000, 0xbfc00000, 0x00000000),
            (0x00000000, 0x7fc00000, 0x7fc00000),
            (0x00000000, 0x7f800000, 0x7f800000),
            (0x00000000, 0xff800000, 0x00000000),
            (0x80000000, 0x00000000, 0x00000000),
            (0x80000000, 0x80000000, 0x80000000),
            (0x80000000, 0x3fc00000, 0x3fc00000),
            (0x80000000, 0xbfc00000, 0x80000000),
            (0x80000000, 0x7fc00000, 0x7fc00000),
            (0x80000000, 0x7f800000, 0x7f800000),
            (0x80000000, 0xff800000, 0x80000000),
            (0x3fc00000, 0x00000000, 0x3fc00000),
            (0x3fc00000, 0x80000000, 0x3fc00000),
            (0x3fc00000, 0x3fc00000, 0x3fc00000),
            (0x3fc00000, 0xbfc00000, 0x3fc00000),
            (0x3fc00000, 0x7fc00000, 0x7fc00000),
            (0x3fc00000, 0x7f800000, 0x7f800000),
            (0x3fc00000, 0xff800000, 0x3fc00000),
            (0xbfc00000, 0x00000000, 0x00000000),
            (0xbfc00000, 0x80000000, 0x80000000),
            (0xbfc00000, 0x3fc00000, 0x3fc00000),
            (0xbfc00000, 0xbfc00000, 0xbfc00000),
            (0xbfc00000, 0x7fc00000, 0x7fc00000),
            (0xbfc00000, 0x7f800000, 0x7f800000),
            (0xbfc00000, 0xff800000, 0xbfc00000),
            (0x7fc00000, 0x00000000, 0x7fc00000),
            (0x7fc00000, 0x80000000, 0x7fc00000),
            (0x7fc00000, 0x3fc00000, 0x7fc00000),
            (0x7fc00000, 0xbfc00000, 0x7fc00000),
            (0x7fc00000, 0x7fc00000, 0x7fc00000),
            (0x7fc00000, 0x7f800000, 0x7fc00000),
            (0x7fc00000, 0xff800000, 0x7fc00000),
            (0x7f800000, 0x00000000, 0x7f800000),
            (0x7f800000, 0x80000000, 0x7f800000),
            (0x7f800000, 0x3fc00000, 0x7f800000),
            (0x7f800000, 0xbfc00000, 0x7f800000),
            (0x7f800000, 0x7fc00000, 0x7fc00000),
            (0x7f800000, 0x7f800000, 0x7f800000),
            (0x7f800000, 0xff800000, 0x7f800000),
            (0xff800000, 0x00000000, 0x00000000),
            (0xff800000, 0x80000000, 0x80000000),
            (0xff800000, 0x3fc00000, 0x3fc00000),
            (0xff800000, 0xbfc00000, 0xbfc00000),
            (0xff800000, 0x7fc00000, 0x7fc00000),
            (0xff800000, 0x7f800000, 0x7f800000),
            (0xff800000, 0xff800000, 0xff800000),
        ];
        const JAVA_MAX_F64: [(u64, u64, u64); 49] = [
            (0x0000000000000000, 0x0000000000000000, 0x0000000000000000),
            (0x0000000000000000, 0x8000000000000000, 0x0000000000000000),
            (0x0000000000000000, 0x3ff8000000000000, 0x3ff8000000000000),
            (0x0000000000000000, 0xbff8000000000000, 0x0000000000000000),
            (0x0000000000000000, 0x7ff8000000000000, 0x7ff8000000000000),
            (0x0000000000000000, 0x7ff0000000000000, 0x7ff0000000000000),
            (0x0000000000000000, 0xfff0000000000000, 0x0000000000000000),
            (0x8000000000000000, 0x0000000000000000, 0x0000000000000000),
            (0x8000000000000000, 0x8000000000000000, 0x8000000000000000),
            (0x8000000000000000, 0x3ff8000000000000, 0x3ff8000000000000),
            (0x8000000000000000, 0xbff8000000000000, 0x8000000000000000),
            (0x8000000000000000, 0x7ff8000000000000, 0x7ff8000000000000),
            (0x8000000000000000, 0x7ff0000000000000, 0x7ff0000000000000),
            (0x8000000000000000, 0xfff0000000000000, 0x8000000000000000),
            (0x3ff8000000000000, 0x0000000000000000, 0x3ff8000000000000),
            (0x3ff8000000000000, 0x8000000000000000, 0x3ff8000000000000),
            (0x3ff8000000000000, 0x3ff8000000000000, 0x3ff8000000000000),
            (0x3ff8000000000000, 0xbff8000000000000, 0x3ff8000000000000),
            (0x3ff8000000000000, 0x7ff8000000000000, 0x7ff8000000000000),
            (0x3ff8000000000000, 0x7ff0000000000000, 0x7ff0000000000000),
            (0x3ff8000000000000, 0xfff0000000000000, 0x3ff8000000000000),
            (0xbff8000000000000, 0x0000000000000000, 0x0000000000000000),
            (0xbff8000000000000, 0x8000000000000000, 0x8000000000000000),
            (0xbff8000000000000, 0x3ff8000000000000, 0x3ff8000000000000),
            (0xbff8000000000000, 0xbff8000000000000, 0xbff8000000000000),
            (0xbff8000000000000, 0x7ff8000000000000, 0x7ff8000000000000),
            (0xbff8000000000000, 0x7ff0000000000000, 0x7ff0000000000000),
            (0xbff8000000000000, 0xfff0000000000000, 0xbff8000000000000),
            (0x7ff8000000000000, 0x0000000000000000, 0x7ff8000000000000),
            (0x7ff8000000000000, 0x8000000000000000, 0x7ff8000000000000),
            (0x7ff8000000000000, 0x3ff8000000000000, 0x7ff8000000000000),
            (0x7ff8000000000000, 0xbff8000000000000, 0x7ff8000000000000),
            (0x7ff8000000000000, 0x7ff8000000000000, 0x7ff8000000000000),
            (0x7ff8000000000000, 0x7ff0000000000000, 0x7ff8000000000000),
            (0x7ff8000000000000, 0xfff0000000000000, 0x7ff8000000000000),
            (0x7ff0000000000000, 0x0000000000000000, 0x7ff0000000000000),
            (0x7ff0000000000000, 0x8000000000000000, 0x7ff0000000000000),
            (0x7ff0000000000000, 0x3ff8000000000000, 0x7ff0000000000000),
            (0x7ff0000000000000, 0xbff8000000000000, 0x7ff0000000000000),
            (0x7ff0000000000000, 0x7ff8000000000000, 0x7ff8000000000000),
            (0x7ff0000000000000, 0x7ff0000000000000, 0x7ff0000000000000),
            (0x7ff0000000000000, 0xfff0000000000000, 0x7ff0000000000000),
            (0xfff0000000000000, 0x0000000000000000, 0x0000000000000000),
            (0xfff0000000000000, 0x8000000000000000, 0x8000000000000000),
            (0xfff0000000000000, 0x3ff8000000000000, 0x3ff8000000000000),
            (0xfff0000000000000, 0xbff8000000000000, 0xbff8000000000000),
            (0xfff0000000000000, 0x7ff8000000000000, 0x7ff8000000000000),
            (0xfff0000000000000, 0x7ff0000000000000, 0x7ff0000000000000),
            (0xfff0000000000000, 0xfff0000000000000, 0xfff0000000000000),
        ];
        for (a, b, max) in JAVA_MAX_F32 {
            let got = max_f32(f32::from_bits(a), f32::from_bits(b)).to_bits();
            assert_eq!(got, max, "Math.max({a:#x}, {b:#x})");
        }
        for (a, b, max) in JAVA_MAX_F64 {
            let got = max_f64(f64::from_bits(a), f64::from_bits(b)).to_bits();
            assert_eq!(got, max, "Math.max({a:#x}, {b:#x})");
        }
        // Rust's own `max` drops the NaN, which is why these exist.
        assert_eq!(0.0f32.max(f32::NAN), 0.0);
        assert!(max_f32(0.0, f32::NAN).is_nan());
    }

    /// `DoubleStream.sum()` on the inputs of
    /// `tools/java-reference/probes/JavaDoubleSumProbe.java`, as it printed
    /// them at `a773a70ec` on JDK 25: `(seed, count, stream sum, plain sum)`.
    #[test]
    fn double_stream_sum_matches_java() {
        const JAVA_SUMS: [(i64, i64, u64, u64); 28] = [
            (1, 0, 0x0000000000000000, 0x0000000000000000),
            (1, 1, 0x4125ab9ead7215d8, 0x4125ab9ead7215d8),
            (1, 2, 0xc1fa7e3ea8c8d511, 0xc1fa7e3ea8c8d511),
            (1, 3, 0x41ec1bfb4e3c3bfd, 0x41ec1bfb4e3c3bfd),
            (1, 10, 0xc18742813ec24d00, 0xc18742813ec24d01),
            (1, 100, 0x4220c34851bff4f5, 0x4220c34851bff4f5),
            (1, 1000, 0x4255af5a11f959da, 0x4255af5a11f959d8),
            (2, 0, 0x0000000000000000, 0x0000000000000000),
            (2, 1, 0xc1f739054b4fdc43, 0xc1f739054b4fdc43),
            (2, 2, 0x4233e09e1b7b5947, 0x4233e09e1b7b5947),
            (2, 3, 0x41c295bfd8f351f9, 0x41c295bfd8f351f8),
            (2, 10, 0xc1c5130c679159b3, 0xc1c5130c679159b3),
            (2, 100, 0xc2381573cb752ffb, 0xc2381573cb752ffe),
            (2, 1000, 0x4245c72a38466853, 0x4245c72a3846684f),
            (3, 0, 0x0000000000000000, 0x0000000000000000),
            (3, 1, 0xbfecd2bbf81f8421, 0xbfecd2bbf81f8421),
            (3, 2, 0x40bae0d125a40cfa, 0x40bae0d125a40cfa),
            (3, 3, 0xc234f7804b1167f0, 0xc234f7804b1167f0),
            (3, 10, 0xc23496466cd883d8, 0xc23496466cd883d9),
            (3, 100, 0x424047e1e5f020fc, 0x424047e1e5f020fa),
            (3, 1000, 0xc265afc8ca26fa79, 0xc265afc8ca26fa78),
            (4, 0, 0x0000000000000000, 0x0000000000000000),
            (4, 1, 0x4089423635914645, 0x4089423635914645),
            (4, 2, 0xc1c90e11df751ff9, 0xc1c90e11df751ff9),
            (4, 3, 0x417f7c3affb14000, 0x417f7c3affb14000),
            (4, 10, 0xc1dbf4fef0005caa, 0xc1dbf4fef0005cab),
            (4, 100, 0x42553e1dc7fa365b, 0x42553e1dc7fa3658),
            (4, 1000, 0x426348ad109f79ad, 0x426348ad109f79a7),
        ];
        let mut differs = 0;
        for (seed, count, stream_sum, plain_sum) in JAVA_SUMS {
            let mut random = JavaRandom::new(seed * 1000 + count);
            let values: Vec<f64> = (0..count)
                .map(|_| {
                    let v = random.next_double() * 10f64.powi(random.next_int(12));
                    if random.next_boolean() { -v } else { v }
                })
                .collect();
            let plain = values.iter().fold(0.0, |a, b| a + b);
            assert_eq!(plain.to_bits(), plain_sum, "plain sum {seed} {count}");
            let got = double_stream_sum(values.iter().copied());
            assert_eq!(got.to_bits(), stream_sum, "DoubleStream.sum {seed} {count}");
            differs += usize::from(stream_sum != plain_sum);
        }
        assert!(differs > 0, "the table shows the compensation matters");
        // Rust's `Sum` of nothing is -0.0; Java's is +0.0.
        assert_eq!(double_stream_sum([]).to_bits(), 0);
        assert!(std::iter::empty::<f64>().sum::<f64>().is_sign_negative());
    }
}
