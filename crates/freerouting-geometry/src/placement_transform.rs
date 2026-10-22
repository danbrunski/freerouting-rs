//! Placement of component-local geometry on the board.
//!
//! [`PlacementTransform::shape`] adds the placement in floating point and rounds
//! each corner after the whole transformation, as Java's `PlacementTransform.shape`
//! (`PlacementTransform.java:54-68`) does. A tile shape's rounded corners then go to
//! [`TileShape::get_instance_from_points`], which takes them for a convex polygon.
//! Where short edges round to reflex vertices (a chamfer edge of an enlarged simplex
//! a unit or two long), the half-planes of the corners' edges cut corners off the
//! shape, or leave it unbounded: C65's fabrication outline at y = 105.242039 mm in
//! bug 1 of `docs/development/pcu-rev2-board-report.md` gets a box reaching
//! `i32::MAX`.
//!
//! `router.java_quirks.placed_shape_convex` (K15.2) reaches this crate as the
//! `java_corners` argument of [`PlacementTransform::shape_with`] and
//! [`PlacementTransform::area_with`]: `true` (the switch unset, and the functions
//! without `_with`) is Java's; `false` takes the convex hull of the rounded corners
//! ([`PolygonShape::convex_hull`]) first, so a placed tile shape holds every rounded
//! corner and is bounded. Where the rounded corners are convex the hull is the
//! polygon itself, so both values give the same shape
//! (`tests/placed_shape_convex.rs`; DIVERGENCES, "Java quirk switches, opt-in").

use crate::float_point::FloatPoint;
use crate::jmath::{d2i, floor_mod};
use crate::line_segment::LineSegment;
use crate::point::Point;
use crate::polyline::Polyline;
use crate::shapes::{
    Area, Circle, IntBox, PolygonShape, PolylineArea, PolylineShape, Shape, TileShape,
};

/// Transforms component-local coordinates to board coordinates: an optional mirroring
/// at the y axis (before or after the rotation), a rotation in degrees, and a
/// translation to `location`.
///
/// Port of `app.freerouting.geometry.planar.PlacementTransform`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacementTransform {
    location: FloatPoint,
    rotation: f64,
    mirrored: bool,
    rotate_first: bool,
}

impl PlacementTransform {
    /// Creates a transform. `rotation` is in degrees; if `rotate_first` the mirroring
    /// is applied after the rotation, else before.
    pub fn new(location: FloatPoint, rotation: f64, mirrored: bool, rotate_first: bool) -> Self {
        PlacementTransform {
            location,
            rotation,
            mirrored,
            rotate_first,
        }
    }

    /// Transforms the direction vector `point` (mirroring and rotation, no translation).
    pub fn direction(&self, point: FloatPoint) -> FloatPoint {
        let mut result = point;
        if self.mirrored && !self.rotate_first {
            result = FloatPoint::new(-result.x, result.y);
        }
        result = PlacementTransform::rotate(result, self.rotation);
        if self.mirrored && self.rotate_first {
            result = FloatPoint::new(-result.x, result.y);
        }
        result
    }

    /// Transforms the local point `point` to board coordinates.
    pub fn point(&self, point: FloatPoint) -> FloatPoint {
        let offset = self.direction(point);
        FloatPoint::new(self.location.x + offset.x, self.location.y + offset.y)
    }

    /// Rotates `point` by `degrees` around the origin; multiples of 90 degrees are
    /// turned exactly.
    pub fn rotate(point: FloatPoint, degrees: f64) -> FloatPoint {
        if degrees % 90.0 == 0.0 {
            point.turn_90_degree_around(floor_mod(d2i(degrees / 90.0), 4), FloatPoint::ZERO)
        } else {
            point.rotate(degrees.to_radians(), FloatPoint::ZERO)
        }
    }

    /// Places the pad shape `shape`, which is rotated by `pin_rotation` degrees and
    /// translated by `offset` inside the component, on the board. The corners are
    /// rounded after the whole transformation.
    pub fn shape(&self, shape: &Shape, offset: FloatPoint, pin_rotation: f64) -> Shape {
        self.shape_with(shape, offset, pin_rotation, true)
    }

    /// [`Self::shape`] with `router.java_quirks.placed_shape_convex` (K15.2) as
    /// `java_corners`: `true` hands a tile shape's rounded corners to
    /// [`TileShape::get_instance_from_points`] as they are (Java's), `false` their
    /// convex hull (see the module docs). A circle or a polygon shape is placed the
    /// same way under both.
    pub fn shape_with(
        &self,
        shape: &Shape,
        offset: FloatPoint,
        pin_rotation: f64,
        java_corners: bool,
    ) -> Shape {
        if shape.is_empty() {
            return shape.clone();
        }
        if let Shape::Circle(circle) = shape {
            let center = self
                .pad_point(circle.center.to_float(), offset, pin_rotation)
                .round();
            return Shape::Circle(Circle::new(center, circle.radius));
        }
        let corners = shape.corner_approx_arr();
        let n = corners.len();
        let mut placed: Vec<Point> = vec![Point::ZERO; n];
        for (i, corner) in corners.iter().enumerate() {
            let index = if self.mirrored { n - 1 - i } else { i };
            placed[index] = Point::Int(self.pad_point(*corner, offset, pin_rotation).round());
        }
        if let Shape::Polyline(PolylineShape::Tile(_)) = shape {
            let mut unique: Vec<Point> = Vec::with_capacity(n);
            for p in placed {
                if !unique.contains(&p) {
                    unique.push(p);
                }
            }
            let normalized = if unique.len() < 3 {
                unique
            } else if java_corners {
                // PARITY(quirk, java_quirks.placed_shape_convex): Java takes the rounded
                // corners for a convex polygon (`PlacementTransform.java:60-61`).
                PolygonShape::new(&unique).corners().to_vec()
            } else {
                PolygonShape::new(&unique).convex_hull().corners().to_vec()
            };
            let tile = match normalized.len() {
                0 => TileShape::IntBox(IntBox::EMPTY),
                1 => TileShape::IntBox(TileShape::get_instance_point(&normalized[0])),
                2 => TileShape::Simplex(
                    LineSegment::from_polyline(&Polyline::from_points(&normalized), 1).to_simplex(),
                ),
                _ => TileShape::get_instance_from_points(&normalized),
            };
            return Shape::from(tile);
        }
        Shape::from(PolygonShape::new(&placed))
    }

    fn pad_point(&self, point: FloatPoint, offset: FloatPoint, pin_rotation: f64) -> FloatPoint {
        let rotated = PlacementTransform::rotate(point, pin_rotation);
        self.point(FloatPoint::new(offset.x + rotated.x, offset.y + rotated.y))
    }

    /// Places `area` on the board.
    ///
    /// # Panics
    /// If the area has holes and the border or a hole places to a circle (a
    /// `ClassCastException` in Java).
    pub fn area(&self, area: &Area) -> Area {
        self.area_with(area, true)
    }

    /// [`Self::area`] with `router.java_quirks.placed_shape_convex` (K15.2) as
    /// `java_corners`, for the border and every hole ([`Self::shape_with`]).
    ///
    /// # Panics
    /// As [`Self::area`].
    pub fn area_with(&self, area: &Area, java_corners: bool) -> Area {
        let border = self.shape_with(&area.get_border(), FloatPoint::ZERO, 0.0, java_corners);
        let holes = area.get_holes();
        if holes.is_empty() {
            return Area::Shape(border);
        }
        let placed_holes: Vec<PolylineShape> = holes
            .iter()
            .map(|h| expect_polyline(self.shape_with(h, FloatPoint::ZERO, 0.0, java_corners)))
            .collect();
        Area::PolylineArea(PolylineArea::new(expect_polyline(border), placed_holes))
    }
}

fn expect_polyline(shape: Shape) -> PolylineShape {
    match shape {
        Shape::Polyline(p) => p,
        Shape::Circle(_) => {
            panic!("PlacementTransform.area: polyline shape expected (ClassCastException in Java)")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::point::IntPoint;
    use crate::shapes::PolylineShapeOps;

    // Port of PlacementTransformTest.padRotationAndBothMirrorConventionsComposeBeforeRounding.
    #[test]
    fn pad_rotation_and_both_mirror_conventions_compose_before_rounding() {
        for rotate_first in [false, true] {
            for mirrored in [false, true] {
                let transform = PlacementTransform::new(
                    FloatPoint::new(-2000.49, 3000.49),
                    90.0,
                    mirrored,
                    rotate_first,
                );
                let offset = FloatPoint::new(10.49, 20.49);
                let local = Shape::Circle(Circle::new(IntPoint::new(100, 200), 30));
                let Shape::Circle(shape) = transform.shape(&local, offset, 90.0) else {
                    panic!("circle expected");
                };
                let mut x = -200.0 + offset.x;
                let y = 100.0 + offset.y;
                if mirrored && !rotate_first {
                    x = -x;
                }
                let mut world_x = -y;
                let world_y = x;
                if mirrored && rotate_first {
                    world_x = -world_x;
                }
                assert_eq!(
                    FloatPoint::new(-2000.49 + world_x, 3000.49 + world_y).round(),
                    shape.center
                );
                assert_eq!(30, shape.radius);
                assert_ne!(
                    shape.center,
                    transform.point(offset).round(),
                    "Off-center pads must not be recentered"
                );
            }
        }
    }

    // Port of PlacementTransformTest.holesAndRedundantOctagonCornersStayBounded.
    #[test]
    fn holes_and_redundant_octagon_corners_stay_bounded() {
        let transform =
            PlacementTransform::new(FloatPoint::new(2000.49, -3000.49), 37.0, true, false);
        let octagon = IntBox::new(-100, -50, 100, 50).bounding_octagon().unwrap();
        let shape = transform.shape(
            &Shape::from(TileShape::IntOctagon(octagon)),
            FloatPoint::ZERO,
            0.0,
        );
        assert!(shape.is_bounded());
        assert!(shape.contains(&Point::Int(transform.point(FloatPoint::ZERO).round())));
        let area = Area::PolylineArea(PolylineArea::new(
            PolylineShape::from(IntBox::new(-100, -100, 100, 100)),
            vec![PolylineShape::from(IntBox::new(-20, -20, 20, 20))],
        ));
        let placed = transform.area(&area);
        assert_eq!(1, placed.get_holes().len());
        assert!(!placed.contains(&Point::Int(transform.point(FloatPoint::ZERO).round())));
        assert!(placed.contains(&Point::Int(
            transform.point(FloatPoint::new(50.0, 0.0)).round()
        )));
    }

    // Port of PlacementTransformTest.emptyAndDegenerateShapesDoNotBecomeUnbounded.
    #[test]
    fn empty_and_degenerate_shapes_do_not_become_unbounded() {
        let transform = PlacementTransform::new(FloatPoint::new(100.5, -200.5), 90.0, true, false);
        assert!(
            transform
                .shape(&Shape::from(IntBox::EMPTY), FloatPoint::ZERO, 0.0)
                .is_empty()
        );
        let point = transform.shape(&Shape::from(IntBox::new(0, 0, 0, 0)), FloatPoint::ZERO, 0.0);
        assert!(point.is_bounded());
        assert_eq!(0, point.dimension());
        let line = transform.shape(
            &Shape::from(IntBox::new(0, 0, 100, 0)),
            FloatPoint::ZERO,
            0.0,
        );
        assert!(line.is_bounded());
        assert_eq!(1, line.dimension());
    }

    // Port of PlacementTransformTest.quarterTurnsPreserveExactHalfCoordinates.
    #[test]
    fn quarter_turns_preserve_exact_half_coordinates() {
        let mut point = FloatPoint::new(0.5, -0.5);
        for _ in 0..4 {
            point = PlacementTransform::rotate(point, 90.0);
        }
        assert_eq!(0.5, point.x);
        assert_eq!(-0.5, point.y);
    }
}
