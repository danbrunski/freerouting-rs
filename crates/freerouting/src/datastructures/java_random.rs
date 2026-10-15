//! `java.util.Random` and `Collections.shuffle`, for algorithms whose Java
//! results depend on a seeded random sequence.

/// The linear congruential generator of `java.util.Random`.
#[derive(Clone, Debug)]
pub struct JavaRandom {
    seed: u64,
}

const MULTIPLIER: u64 = 0x5_DEEC_E66D;
const ADDEND: u64 = 0xB;
const MASK: u64 = (1 << 48) - 1;

impl JavaRandom {
    /// `new Random(seed)`.
    pub fn new(seed: i64) -> Self {
        let mut random = JavaRandom { seed: 0 };
        random.set_seed(seed);
        random
    }

    /// `Random.setSeed`.
    pub fn set_seed(&mut self, seed: i64) {
        self.seed = (seed as u64 ^ MULTIPLIER) & MASK;
    }

    /// `Random.next(bits)`.
    fn next(&mut self, bits: u32) -> i32 {
        self.seed = self.seed.wrapping_mul(MULTIPLIER).wrapping_add(ADDEND) & MASK;
        (self.seed >> (48 - bits)) as i64 as i32
    }

    /// `Random.nextInt(bound)` for a positive `bound`.
    pub fn next_int(&mut self, bound: i32) -> i32 {
        assert!(bound > 0, "bound must be positive");
        let mut r = self.next(31);
        let m = bound - 1;
        if bound & m == 0 {
            return ((i64::from(bound) * i64::from(r)) >> 31) as i32;
        }
        let mut u = r;
        loop {
            r = u % bound;
            if u.wrapping_sub(r).wrapping_add(m) >= 0 {
                return r;
            }
            u = self.next(31);
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
    fn matches_java_sequence() {
        // new Random(42).nextInt(100) five times in Java
        let mut random = JavaRandom::new(42);
        let values: Vec<i32> = (0..5).map(|_| random.next_int(100)).collect();
        assert_eq!(values, [30, 63, 48, 84, 70]);
        // Powers of two take the multiply branch.
        let mut random = JavaRandom::new(7);
        let values: Vec<i32> = (0..3).map(|_| random.next_int(16)).collect();
        assert_eq!(values, [11, 10, 11]);
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
