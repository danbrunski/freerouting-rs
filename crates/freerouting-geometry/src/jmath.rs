//! Helpers reproducing Java numeric semantics exactly (`Math.round`, `(int)` casts,
//! `Math.rint`, `java.util.Random`).

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
}
