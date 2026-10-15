//! Circles with integer center and radius.

use std::f64::consts::PI;
use std::fmt;

use crate::direction::{Direction, IntDirection};
use crate::float_point::{FloatPoint, format_grouped, java_max};
use crate::jmath::{d2i, round_to_i32};
use crate::line::Line;
use crate::point::{IntPoint, Point};
use crate::vector::{IntVector, Vector};

use super::{IntBox, IntOctagon, Simplex, TileShape, TileShapeOps};

/// A circle with an integer center and radius.
///
/// Port of `app.freerouting.geometry.planar.Circle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Circle {
    pub center: IntPoint,
    pub radius: i32,
}

impl Circle {
    /// Creates a circle; a negative radius is replaced by its absolute value.
    pub fn new(center: IntPoint, radius: i32) -> Circle {
        Circle {
            center,
            radius: if radius < 0 {
                radius.wrapping_neg()
            } else {
                radius
            },
        }
    }

    /// Always false.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// Always true.
    pub fn is_bounded(&self) -> bool {
        true
    }

    /// 0 for radius 0, else 2.
    pub fn dimension(&self) -> i32 {
        if self.radius == 0 { 0 } else { 2 }
    }

    /// The circumference.
    pub fn circumference(&self) -> f64 {
        2.0 * PI * f64::from(self.radius)
    }

    /// The area.
    pub fn area(&self) -> f64 {
        (PI * f64::from(self.radius)) * f64::from(self.radius)
    }

    /// The center.
    pub fn centre_of_gravity(&self) -> FloatPoint {
        self.center.to_float()
    }

    fn radius_square(&self) -> f64 {
        f64::from(self.radius) * f64::from(self.radius)
    }

    /// True if `point` is outside the circle.
    pub fn is_outside(&self, point: &Point) -> bool {
        point.to_float().distance_square(self.center.to_float()) > self.radius_square()
    }

    /// True if `point` is inside or on the circle.
    pub fn contains(&self, point: &Point) -> bool {
        !self.is_outside(point)
    }

    /// True if `point` is inside or on the circle.
    pub fn contains_float(&self, point: FloatPoint) -> bool {
        point.distance_square(self.center.to_float()) <= self.radius_square()
    }

    /// True if `point` is strictly inside the circle.
    pub fn contains_inside(&self, point: &Point) -> bool {
        point.to_float().distance_square(self.center.to_float()) < self.radius_square()
    }

    /// True if `point` is exactly on the circle.
    pub fn contains_on_border(&self, point: &Point) -> bool {
        point.to_float().distance_square(self.center.to_float()) == self.radius_square()
    }

    /// Distance from `point` to the circle; 0 if contained.
    pub fn distance(&self, point: FloatPoint) -> f64 {
        let d = point.distance(self.center.to_float()) - f64::from(self.radius);
        java_max(d, 0.0)
    }

    /// The radius.
    pub fn smallest_radius(&self) -> f64 {
        f64::from(self.radius)
    }

    /// The smallest surrounding box.
    pub fn bounding_box(&self) -> IntBox {
        IntBox::new(
            self.center.x.wrapping_sub(self.radius),
            self.center.y.wrapping_sub(self.radius),
            self.center.x.wrapping_add(self.radius),
            self.center.y.wrapping_add(self.radius),
        )
    }

    /// The smallest surrounding octagon (not normalized).
    pub fn bounding_octagon(&self) -> IntOctagon {
        let (cx, cy, r) = (self.center.x, self.center.y, self.radius);
        let left_x = cx.wrapping_sub(r);
        let right_x = cx.wrapping_add(r);
        let bottom_y = cy.wrapping_sub(r);
        let top_y = cy.wrapping_add(r);
        let sqrt2_minus1 = 2f64.sqrt() - 1.0;
        let ceil_corner_value = d2i((sqrt2_minus1 * f64::from(r)).ceil());
        let floor_corner_value = d2i((sqrt2_minus1 * f64::from(r)).floor());
        IntOctagon::new(
            left_x,
            bottom_y,
            right_x,
            top_y,
            left_x.wrapping_sub(cy.wrapping_add(floor_corner_value)),
            right_x.wrapping_sub(cy.wrapping_sub(ceil_corner_value)),
            left_x.wrapping_add(cy.wrapping_sub(floor_corner_value)),
            right_x.wrapping_add(cy.wrapping_add(ceil_corner_value)),
        )
    }

    /// A bounding tile: the bounding octagon.
    pub fn bounding_tile(&self) -> TileShape {
        TileShape::IntOctagon(self.bounding_octagon())
    }

    /// A bounding tile whose segments are at most `max_segment_length` long (Java
    /// `boundingTile(int)`).
    pub fn bounding_tile_with_max_segment_length(&self, max_segment_length: i32) -> TileShape {
        if max_segment_length <= 0 {
            return TileShape::IntOctagon(self.bounding_octagon());
        }
        let quadrant_division_count = self.radius / max_segment_length + 1;
        self.bounding_tile_with_divisions(quadrant_division_count)
    }

    /// A circumscribed convex polygon with `quadrant_division_count` sides per quadrant;
    /// the bounding octagon for a radius of 0 or at most 2 divisions.
    pub fn bounding_tile_with_divisions(&self, quadrant_division_count: i32) -> TileShape {
        if self.radius <= 0 || quadrant_division_count <= 2 {
            return TileShape::IntOctagon(self.bounding_octagon());
        }
        let divisions = quadrant_division_count as usize;
        let mut tangent_line_arr = vec![Line::from_coords(0, 0, 0, 0); divisions * 4];
        let r = f64::from(self.radius);
        for i in 0..divisions {
            let current_angle = (i as f64 * PI) / (2.0 * divisions as f64);
            let border_delta = if i == 0 {
                IntVector::new(self.radius, 0)
            } else {
                IntVector::new(
                    d2i((current_angle.cos() * r).ceil()),
                    d2i((current_angle.sin() * r).ceil()),
                )
            };
            let current_a = self.center.translate_by_int(border_delta);
            let current_direction: IntDirection = if i == 0 {
                IntDirection::UP
            } else {
                Direction::get_instance_approx(current_angle + PI / 2.0).expect_int()
            };
            let current_tangent = Line::from_point_and_direction(current_a, current_direction);
            tangent_line_arr[divisions + i] = current_tangent;
            tangent_line_arr[2 * divisions + i] = current_tangent.turn_90_degree(1, self.center);
            tangent_line_arr[3 * divisions + i] = current_tangent.turn_90_degree(2, self.center);
            tangent_line_arr[i] = current_tangent.turn_90_degree(3, self.center);
        }
        TileShape::get_instance_from_lines(&tangent_line_arr)
    }

    /// True if the circle is contained in `b`.
    pub fn is_contained_in(&self, b: &IntBox) -> bool {
        let (cx, cy, r) = (self.center.x, self.center.y, self.radius);
        if b.ll.x > cx.wrapping_sub(r) {
            return false;
        }
        if b.ll.y > cy.wrapping_sub(r) {
            return false;
        }
        if b.ur.x < cx.wrapping_add(r) {
            return false;
        }
        b.ur.y >= cy.wrapping_add(r)
    }

    /// The circle turned by `factor` times 90 degrees around `pole`.
    pub fn turn_90_degree(&self, factor: i32, pole: IntPoint) -> Circle {
        Circle::new(self.center.turn_90_degree(factor, pole), self.radius)
    }

    /// The circle with its center rotated by `angle` around `pole`.
    pub fn rotate_approx(&self, angle: f64, pole: FloatPoint) -> Circle {
        Circle::new(
            self.center.to_float().rotate(angle, pole).round(),
            self.radius,
        )
    }

    /// The circle mirrored at the vertical line through `pole`.
    pub fn mirror_vertical(&self, pole: IntPoint) -> Circle {
        Circle::new(self.center.mirror_vertical(pole), self.radius)
    }

    /// The circle mirrored at the horizontal line through `pole`.
    pub fn mirror_horizontal(&self, pole: IntPoint) -> Circle {
        Circle::new(self.center.mirror_horizontal(pole), self.radius)
    }

    /// The diameter.
    pub fn max_width(&self) -> f64 {
        f64::from(2i32.wrapping_mul(self.radius))
    }

    /// The diameter.
    pub fn min_width(&self) -> f64 {
        f64::from(2i32.wrapping_mul(self.radius))
    }

    /// The circle with the radius changed by `offset` (rounded).
    pub fn offset(&self, offset: f64) -> Circle {
        let new_radius = f64::from(self.radius) + offset;
        Circle::new(self.center, round_to_i32(new_radius))
    }

    /// The circle with the radius reduced by `offset`, but at least 1.
    pub fn shrink(&self, offset: f64) -> Circle {
        let new_radius = f64::from(self.radius) - offset;
        Circle::new(self.center, round_to_i32(new_radius).max(1))
    }

    /// The circle translated by `vector`; unchanged for a rational vector (as in Java).
    pub fn translate_by(&self, vector: &Vector) -> Circle {
        match vector {
            Vector::Int(v) if !vector.is_zero_constant() => {
                Circle::new(self.center.translate_by_int(*v), self.radius)
            }
            _ => *self,
        }
    }

    /// Not implemented in Java: always `None`.
    pub fn nearest_point_approx(&self, _point: FloatPoint) -> Option<FloatPoint> {
        None
    }

    /// Distance from `point` to the circle line.
    pub fn border_distance(&self, point: FloatPoint) -> f64 {
        (point.distance(self.center.to_float()) - f64::from(self.radius)).abs()
    }

    /// The circle with the radius enlarged by `offset` (rounded).
    pub fn enlarge(&self, offset: f64) -> Circle {
        if offset == 0.0 {
            return *self;
        }
        Circle::new(self.center, self.radius.wrapping_add(round_to_i32(offset)))
    }

    /// True if the two circles intersect.
    pub fn intersects_circle(&self, other: &Circle) -> bool {
        let mut radius_sum_square = f64::from(self.radius.wrapping_add(other.radius));
        radius_sum_square *= radius_sum_square;
        self.center.distance_square(other.center) <= radius_sum_square
    }

    /// True if the circle intersects `b`.
    pub fn intersects_box(&self, b: &IntBox) -> bool {
        b.distance(self.center.to_float()) <= f64::from(self.radius)
    }

    /// True if the circle intersects `oct`.
    pub fn intersects_octagon(&self, oct: &IntOctagon) -> bool {
        oct.distance(self.center.to_float()) <= f64::from(self.radius)
    }

    /// True if the circle intersects `simplex`.
    pub fn intersects_simplex(&self, simplex: &Simplex) -> bool {
        simplex.distance(self.center.to_float()) <= f64::from(self.radius)
    }

    /// A single convex piece: a circumscribed 64-gon for a radius of at least 16, the
    /// bounding octagon otherwise.
    pub fn split_to_convex(&self) -> Vec<TileShape> {
        if self.radius >= 16 {
            vec![self.bounding_tile_with_divisions(16)]
        } else {
            vec![TileShape::IntOctagon(self.bounding_octagon())]
        }
    }

    /// No corners.
    pub fn corner_approx_arr(&self) -> Vec<FloatPoint> {
        Vec::new()
    }
}

impl fmt::Display for Circle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Circle: ")?;
        if self.center != IntPoint::ZERO {
            write!(f, "center {}", self.center)?;
        }
        write!(f, "radius {}", format_grouped(f64::from(self.radius), 0, 3))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shapes::PolylineShapeOps;

    // Ported from CircleTest.java.

    #[test]
    fn small_circle_uses_octagon() {
        let small_circle = Circle::new(IntPoint::new(100, 100), 10);
        let shapes = small_circle.split_to_convex();
        assert_eq!(1, shapes.len());
        assert!(matches!(shapes[0], TileShape::IntOctagon(_)));
    }

    #[test]
    fn large_circle_uses_64_gon() {
        let circle = Circle::new(IntPoint::new(10000, -10000), 25000);
        let shapes = circle.split_to_convex();
        assert_eq!(1, shapes.len());
        let TileShape::Simplex(simplex) = &shapes[0] else {
            panic!("expected a Simplex");
        };
        assert_eq!(64, simplex.border_line_count());
        assert_eq!(64, simplex.corner_approx_arr().len());
    }

    #[test]
    fn sixty_four_gon_eliminates_octagonal_overshoot_at_22_degrees() {
        let center = IntPoint::new(10000, -10000);
        let radius = 25000;
        let circle = Circle::new(center, radius);
        let octagon = TileShape::IntOctagon(circle.bounding_octagon());
        let sixty_four_gon = circle.split_to_convex().remove(0);
        let angle_rad = 22.5f64.to_radians();
        let distance = f64::from(radius) * 1.04;
        let test_point = IntPoint::new(
            round_to_i32(f64::from(center.x) + distance * angle_rad.cos()),
            round_to_i32(f64::from(center.y) + distance * angle_rad.sin()),
        );
        assert!(octagon.contains(&Point::Int(test_point)));
        assert!(!sixty_four_gon.contains(&Point::Int(test_point)));
        assert!(sixty_four_gon.contains(&Point::Int(center)));
        for p in [
            IntPoint::new(center.x + radius, center.y),
            IntPoint::new(center.x, center.y + radius),
            IntPoint::new(center.x - radius, center.y),
            IntPoint::new(center.x, center.y - radius),
        ] {
            assert!(sixty_four_gon.contains(&Point::Int(p)));
        }
        let circle_area = circle.area();
        let poly_area = sixty_four_gon.area();
        assert!(poly_area >= circle_area);
        assert!(poly_area < circle_area * 1.002);
    }

    #[test]
    fn bounding_tile_with_divisions() {
        let circle = Circle::new(IntPoint::ZERO, 5000);
        assert!(matches!(
            circle.bounding_tile_with_divisions(2),
            TileShape::IntOctagon(_)
        ));
        let poly32 = circle.bounding_tile_with_divisions(8);
        assert!(matches!(poly32, TileShape::Simplex(_)));
        assert_eq!(32, poly32.border_line_count());
        let poly64 = circle.bounding_tile_with_divisions(16);
        assert!(matches!(poly64, TileShape::Simplex(_)));
        assert_eq!(64, poly64.border_line_count());
    }

    #[test]
    fn zero_or_small_radius_returns_octagon() {
        let zero_circle = Circle::new(IntPoint::ZERO, 0);
        assert!(matches!(
            zero_circle.split_to_convex()[0],
            TileShape::IntOctagon(_)
        ));
        assert!(matches!(
            zero_circle.bounding_tile_with_divisions(16),
            TileShape::IntOctagon(_)
        ));
        let small_circle = Circle::new(IntPoint::ZERO, 15);
        assert!(matches!(
            small_circle.split_to_convex()[0],
            TileShape::IntOctagon(_)
        ));
    }

    #[test]
    fn cutoff_overshoot_and_fallback() {
        let below_cutoff = Circle::new(IntPoint::ZERO, 15);
        assert!(matches!(
            below_cutoff.split_to_convex()[0],
            TileShape::IntOctagon(_)
        ));
        for r in [16, 17] {
            let at_cutoff = Circle::new(IntPoint::ZERO, r);
            let shape = at_cutoff.split_to_convex().remove(0);
            assert!(matches!(shape, TileShape::Simplex(_)));
            assert!(shape.border_line_count() > 8 && shape.border_line_count() <= 64);
            let max_corner_dist = shape
                .corner_approx_arr()
                .iter()
                .map(|c| (c.x * c.x + c.y * c.y).sqrt())
                .fold(0.0, f64::max);
            assert!(max_corner_dist / f64::from(r) < 1.065);
        }
        let large_circle = Circle::new(IntPoint::ZERO, 25000);
        let large_shape = large_circle.split_to_convex().remove(0);
        let max_large_corner_dist = large_shape
            .corner_approx_arr()
            .iter()
            .map(|c| (c.x * c.x + c.y * c.y).sqrt())
            .fold(0.0, f64::max);
        assert!(max_large_corner_dist / 25000.0 < 1.0013);
    }

    #[test]
    fn bounding_tile_respects_max_segment_length() {
        let circle = Circle::new(IntPoint::ZERO, 10000);
        let shape = circle.bounding_tile_with_max_segment_length(100);
        assert!(matches!(shape, TileShape::Simplex(_)));
        assert_eq!(404, shape.border_line_count());
    }

    #[test]
    fn display_matches_java() {
        assert_eq!(
            Circle::new(IntPoint::ZERO, 25000).to_string(),
            "Circle: radius 25,000"
        );
        assert_eq!(
            Circle::new(IntPoint::new(1, 2), 5).to_string(),
            "Circle: center (1,2)radius 5"
        );
    }
}
