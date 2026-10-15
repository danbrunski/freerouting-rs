//! Port of `app.freerouting.datastructures.Signum`.

use std::fmt;

/// The mathematical signum function as a three-valued enum.
///
/// Java: `app.freerouting.datastructures.Signum`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Signum {
    Positive,
    Negative,
    Zero,
}

impl Signum {
    /// Returns the signum of `value` (`NaN` maps to `Zero`, as in Java).
    pub fn of(value: f64) -> Signum {
        if value > 0.0 {
            Signum::Positive
        } else if value < 0.0 {
            Signum::Negative
        } else {
            Signum::Zero
        }
    }

    /// Returns the signum of `value` as an int: +1, 0 or -1.
    pub fn as_int(value: f64) -> i32 {
        if value > 0.0 {
            1
        } else if value < 0.0 {
            -1
        } else {
            0
        }
    }

    /// Returns the opposite signum.
    pub fn negate(self) -> Signum {
        match self {
            Signum::Positive => Signum::Negative,
            Signum::Negative => Signum::Positive,
            Signum::Zero => Signum::Zero,
        }
    }
}

impl fmt::Display for Signum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Signum::Positive => "positive",
            Signum::Negative => "negative",
            Signum::Zero => "zero",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn of_and_negate() {
        assert_eq!(Signum::of(2.0), Signum::Positive);
        assert_eq!(Signum::of(-0.5), Signum::Negative);
        assert_eq!(Signum::of(0.0), Signum::Zero);
        assert_eq!(Signum::of(f64::NAN), Signum::Zero);
        assert_eq!(Signum::Positive.negate(), Signum::Negative);
        assert_eq!(Signum::Zero.negate(), Signum::Zero);
        assert_eq!(Signum::as_int(-3.0), -1);
        assert_eq!(Signum::Negative.to_string(), "negative");
    }
}
