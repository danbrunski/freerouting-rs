//! Lists of points forming polygons.

use std::f64::consts::PI;

use crate::jmath::round_to_i32;
use crate::point::Point;
use crate::side::Side;

/// A list of points in the plane, where no two consecutive points are equal and no three
/// consecutive points are collinear.
///
/// Port of `app.freerouting.geometry.planar.Polygon`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Polygon {
    corners: Vec<Point>,
}

impl Polygon {
    /// Creates a polygon from `points`. Repeated consecutive points and points collinear
    /// with their previous and next point are removed (the first and last point are not
    /// compared with each other, as in Java).
    pub fn new(points: &[Point]) -> Polygon {
        let mut corners: Vec<Point> = points.to_vec();
        let mut corner_removed = true;
        while corner_removed {
            corner_removed = false;
            if corners.is_empty() {
                break;
            }
            let before = corners.len();
            corners.dedup();
            if corners.len() != before {
                corner_removed = true;
            }
            // Remove the first corner that is collinear with its neighbours.
            if let Some(i) = (1..corners.len().saturating_sub(1))
                .find(|&i| corners[i].side_of(&corners[i - 1], &corners[i + 1]) == Side::Collinear)
            {
                corners.remove(i);
                corner_removed = true;
            }
        }
        Polygon { corners }
    }

    /// The corners of this polygon.
    pub fn corner_array(&self) -> Vec<Point> {
        self.corners.clone()
    }

    /// The corners of this polygon as a slice.
    pub fn corners(&self) -> &[Point] {
        &self.corners
    }

    /// The polygon with the order of the corners reverted.
    pub fn revert_corners(&self) -> Polygon {
        let reversed: Vec<Point> = self.corners.iter().rev().cloned().collect();
        Polygon::new(&reversed)
    }

    /// The winding number of this polygon, treated as closed: positive if the corners
    /// are in counterclockwise order, negative if clockwise.
    pub fn winding_number_after_closing(&self) -> i32 {
        let corners = &self.corners;
        if corners.len() < 2 {
            return 0;
        }
        let first_side_vector = corners[1].difference_by(&corners[0]);
        let mut prev_side_vector = first_side_vector.clone();
        let mut corner_count = corners.len();
        if corners[0] == corners[corner_count - 1] {
            corner_count -= 1;
        }
        let mut angle_sum = 0.0;
        for i in 1..corner_count.saturating_sub(1) {
            let next_side_vector = corners[i + 1].difference_by(&corners[i]);
            angle_sum += prev_side_vector.angle_approx_to(&next_side_vector);
            prev_side_vector = next_side_vector;
        }
        if corner_count > 1 {
            let next_side_vector = corners[0].difference_by(&corners[corner_count - 1]);
            angle_sum += prev_side_vector.angle_approx_to(&next_side_vector);
            prev_side_vector = next_side_vector;
        }
        angle_sum += prev_side_vector.angle_approx_to(&first_side_vector);
        angle_sum /= 2.0 * PI;
        // Java logs a warning if |angle_sum| < 0.5.
        round_to_i32(angle_sum)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::point::IntPoint;

    fn pts(c: &[(i32, i32)]) -> Vec<Point> {
        c.iter()
            .map(|&(x, y)| Point::from(IntPoint::new(x, y)))
            .collect()
    }

    #[test]
    fn removes_duplicates_and_collinear_corners() {
        let p = Polygon::new(&pts(&[
            (0, 0),
            (0, 0),
            (5, 0),
            (10, 0),
            (10, 10),
            (10, 10),
            (0, 10),
        ]));
        assert_eq!(p.corner_array(), pts(&[(0, 0), (10, 0), (10, 10), (0, 10)]));
        assert!(Polygon::new(&[]).corner_array().is_empty());
        assert_eq!(
            Polygon::new(&pts(&[(1, 1), (1, 1)])).corner_array(),
            pts(&[(1, 1)])
        );
    }

    #[test]
    fn winding_number_sign_follows_orientation() {
        let ccw = Polygon::new(&pts(&[(0, 0), (10, 0), (10, 10), (0, 10)]));
        assert_eq!(ccw.winding_number_after_closing(), 1);
        let cw = ccw.revert_corners();
        assert_eq!(
            cw.corner_array(),
            pts(&[(0, 10), (10, 10), (10, 0), (0, 0)])
        );
        assert_eq!(cw.winding_number_after_closing(), -1);
        // A closing point equal to the first corner is ignored.
        let closed = Polygon::new(&pts(&[(0, 0), (10, 0), (10, 10), (0, 10), (0, 0)]));
        assert_eq!(closed.winding_number_after_closing(), 1);
        assert_eq!(
            Polygon::new(&pts(&[(0, 0)])).winding_number_after_closing(),
            0
        );
    }
}
