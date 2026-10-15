//! Port of `app.freerouting.geometry.planar.Side`.

use std::fmt;

/// The side of a directed line on which something lies.
///
/// Java: `app.freerouting.geometry.planar.Side`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    OnTheLeft,
    OnTheRight,
    Collinear,
}

impl Side {
    /// Returns `OnTheLeft` if `value > 0`, `OnTheRight` if `value < 0`, and `Collinear`
    /// otherwise (including `NaN`).
    pub fn of(value: f64) -> Side {
        if value > 0.0 {
            Side::OnTheLeft
        } else if value < 0.0 {
            Side::OnTheRight
        } else {
            Side::Collinear
        }
    }

    /// Returns the opposite side.
    pub fn negate(self) -> Side {
        match self {
            Side::OnTheLeft => Side::OnTheRight,
            Side::OnTheRight => Side::OnTheLeft,
            Side::Collinear => Side::Collinear,
        }
    }
}

impl fmt::Display for Side {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Side::OnTheLeft => "onTheLeft",
            Side::OnTheRight => "onTheRight",
            Side::Collinear => "collinear",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn of_and_negate() {
        assert_eq!(Side::of(1.0), Side::OnTheLeft);
        assert_eq!(Side::of(-1.0), Side::OnTheRight);
        assert_eq!(Side::of(0.0), Side::Collinear);
        assert_eq!(Side::OnTheLeft.negate(), Side::OnTheRight);
        assert_eq!(Side::Collinear.negate(), Side::Collinear);
        assert_eq!(Side::OnTheRight.to_string(), "onTheRight");
    }
}
