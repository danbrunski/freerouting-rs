//! Ellipses with float coordinates.

use std::f64::consts::PI;

use crate::float_point::FloatPoint;

/// An ellipse in the plane. Not a convex shape of the router, because its coordinates are
/// float.
///
/// Port of `app.freerouting.geometry.planar.Ellipse`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ellipse {
    pub center: FloatPoint,
    /// Rotation in radian, normalized to `0 <= rotation < PI`.
    pub rotation: f64,
    pub bigger_radius: f64,
    pub smaller_radius: f64,
}

impl Ellipse {
    /// Creates an ellipse. If `radius1 < radius2` the radii are swapped and the
    /// rotation is increased by `PI / 2`.
    pub fn new(center: FloatPoint, rotation: f64, radius1: f64, radius2: f64) -> Ellipse {
        let (bigger_radius, smaller_radius, mut curr_rotation) = if radius1 >= radius2 {
            (radius1, radius2, rotation)
        } else {
            (radius2, radius1, rotation + 0.5 * PI)
        };
        while curr_rotation >= PI {
            curr_rotation -= PI;
        }
        while curr_rotation < 0.0 {
            curr_rotation += PI;
        }
        Ellipse {
            center,
            rotation: curr_rotation,
            bigger_radius,
            smaller_radius,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn radii_are_ordered_and_rotation_normalized() {
        let e = Ellipse::new(FloatPoint::new(1.0, 2.0), 0.25, 1.0, 3.0);
        assert_eq!(e.bigger_radius, 3.0);
        assert_eq!(e.smaller_radius, 1.0);
        assert_eq!(e.rotation, 0.25 + 0.5 * PI);
        let e = Ellipse::new(FloatPoint::new(0.0, 0.0), -0.5, 3.0, 1.0);
        assert_eq!(e.rotation, -0.5 + PI);
        let e = Ellipse::new(FloatPoint::new(0.0, 0.0), 2.0 * PI + 0.5, 3.0, 1.0);
        assert!((e.rotation - 0.5).abs() < 1e-12);
        assert!((0.0..PI).contains(&e.rotation));
    }
}
