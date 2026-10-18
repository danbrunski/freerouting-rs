//! Planar geometry for the router: integer and rational points, lines, and the
//! convex tile shapes the router builds its rooms from.
//!
//! Port of `app.freerouting.geometry.planar` (plus `datastructures.Signum`,
//! `datastructures.BigIntAux` and `datastructures.Stoppable`). Arithmetic is exact:
//! coordinates are bounded by [`CRIT_INT`], so products of coordinates are computed in
//! `i64`/`i128` where the bound makes that provably exact, and in `BigInt` otherwise.
//! `AwtAreas` (a `java.awt.geom.Area` bridge) is not ported.

pub mod big_int_aux;
pub mod direction;
pub mod ellipse;
pub mod float_line;
pub mod float_point;
pub mod jmath;
pub mod limits;
pub mod line;
pub mod line_segment;
pub mod placement_transform;
pub mod point;
pub mod polygon;
pub mod polyline;
pub mod quirks;
pub mod shapes;
pub mod side;
pub mod signum;
pub mod stoppable;
pub mod vector;

pub use direction::{BigIntDirection, Direction, FortyfiveDegreeDirection, IntDirection};
pub use ellipse::Ellipse;
pub use float_line::FloatLine;
pub use float_point::FloatPoint;
pub use limits::{CRIT_DOUBLE, CRIT_INT, SQRT2};
pub use line::Line;
pub use line_segment::LineSegment;
pub use placement_transform::PlacementTransform;
pub use point::{IntPoint, Point, RationalPoint};
pub use polygon::Polygon;
pub use polyline::Polyline;
pub use quirks::SplitQuirks;
pub use shapes::{
    Area, Circle, ConvexShape, IntBox, IntOctagon, PolygonShape, PolylineArea, PolylineShape,
    PolylineShapeOps, RegularTileShape, Shape, ShapeBoundingDirections, Simplex, TileShape,
    TileShapeOps,
};
pub use side::Side;
pub use signum::Signum;
pub use stoppable::Stoppable;
pub use vector::{IntVector, RationalVector, Vector};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coordinate_products_fit_in_f64_mantissa() {
        let max = i64::from(CRIT_INT);
        assert!(max * max < 1 << 53);
    }
}
