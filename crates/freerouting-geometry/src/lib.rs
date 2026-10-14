//! Planar geometry for the router: integer and rational points, lines, and the
//! convex tile shapes the router builds its rooms from.
//!
//! Port of `app.freerouting.geometry.planar`.

/// Coordinate bound: the product of two coordinates with absolute value at most
/// this fits in the mantissa of an `f64` with room left for addition.
pub const CRIT_INT: i32 = 1 << 25;

/// Integer point on the board grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IntPoint {
    pub x: i32,
    pub y: i32,
}

impl IntPoint {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coordinate_products_fit_in_f64_mantissa() {
        let max = i64::from(CRIT_INT);
        assert!(max * max < 1 << 53);
    }
}
