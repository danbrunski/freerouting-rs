//! Shapes in the plane: convex tiles (boxes, octagons, simplices), polygons, circles
//! and areas with holes.
//!
//! The Java class hierarchy (`Area` > `Shape` > `ConvexShape`/`PolylineShape` >
//! `TileShape` > `RegularTileShape`/`Simplex` > `IntBox`/`IntOctagon`) is ported as
//! enums ([`Area`], [`Shape`], [`ConvexShape`], [`PolylineShape`], [`TileShape`],
//! [`RegularTileShape`]) whose variants hold the concrete structs, plus the traits
//! [`PolylineShapeOps`] and [`TileShapeOps`] carrying the inherited default methods.
//! Java's double dispatch (`intersection`, `intersects`, `cutout`, `union`, `compare`,
//! `bounds`) is a `match` on both operands that follows the Java call chain, since it
//! decides, for example, the order of the lines of a resulting simplex.

mod circle;
mod int_box;
mod int_octagon;
mod polygon_shape;
mod polyline_area;
mod polyline_shape;
mod simplex;
mod tile_shape;

pub use circle::Circle;
pub use int_box::IntBox;
pub use int_octagon::IntOctagon;
pub use polygon_shape::PolygonShape;
pub use polyline_area::PolylineArea;
pub use polyline_shape::{PolylineShape, PolylineShapeOps};
pub use simplex::Simplex;
pub use tile_shape::{TileShape, TileShapeOps};

use crate::float_point::FloatPoint;
use crate::point::{IntPoint, Point};
use crate::polyline::Polyline;
use crate::side::Side;
use crate::stoppable::Stoppable;
use crate::vector::Vector;

/// A tile shape whose border lines have directions from a fixed set: an axis parallel
/// box or a 45 degree octagon.
///
/// Port of the sealed Java class `RegularTileShape`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegularTileShape {
    IntBox(IntBox),
    IntOctagon(IntOctagon),
}

impl From<IntBox> for RegularTileShape {
    fn from(b: IntBox) -> Self {
        RegularTileShape::IntBox(b)
    }
}

impl From<IntOctagon> for RegularTileShape {
    fn from(o: IntOctagon) -> Self {
        RegularTileShape::IntOctagon(o)
    }
}

impl RegularTileShape {
    /// Compares the edge lines with index `edge_index` of this shape and `other`:
    /// `OnTheLeft` if the edge of this shape is on the left of the edge of `other`,
    /// `Collinear` if they are equal, `OnTheRight` otherwise.
    pub fn compare(&self, other: &RegularTileShape, edge_index: usize) -> Side {
        use RegularTileShape::{IntBox as B, IntOctagon as O};
        let result = match (self, other) {
            (B(a), B(b)) => b.compare_box(a, edge_index),
            (B(a), O(b)) => b.compare_box(a, edge_index),
            (O(a), B(b)) => b.compare_octagon(a, edge_index),
            (O(a), O(b)) => b.compare_octagon(a, edge_index),
        };
        result.negate()
    }

    /// The smallest regular shape containing this shape and `other`.
    pub fn union(&self, other: &RegularTileShape) -> RegularTileShape {
        use RegularTileShape::{IntBox as B, IntOctagon as O};
        match (self, other) {
            (B(a), B(b)) => B(b.union_box(a)),
            (B(a), O(b)) => O(b.union_box(a)),
            (O(a), B(b)) => O(b.union_octagon(a)),
            (O(a), O(b)) => O(b.union_octagon(a)),
        }
    }

    /// True if this shape contains `other`.
    pub fn contains_regular(&self, other: &RegularTileShape) -> bool {
        match self {
            RegularTileShape::IntBox(b) => b.contains_regular(other),
            RegularTileShape::IntOctagon(o) => o.contains_regular(other),
        }
    }

    /// True if this shape is contained in `other`.
    pub fn is_contained_in_octagon(&self, other: &IntOctagon) -> bool {
        match self {
            RegularTileShape::IntBox(b) => b.is_contained_in_octagon(other),
            RegularTileShape::IntOctagon(o) => o.is_contained_in_octagon(other),
        }
    }

    /// True if this shape is contained in `other`.
    pub fn is_contained_in(&self, other: &IntBox) -> bool {
        match self {
            RegularTileShape::IntBox(b) => b.is_contained_in(other),
            RegularTileShape::IntOctagon(o) => o.is_contained_in(other),
        }
    }

    /// The shape as a [`TileShape`].
    pub fn to_tile_shape(&self) -> TileShape {
        (*self).into()
    }
}

/// The fixed directions of a [`RegularTileShape`]; used to compute bounding shapes for
/// the search trees.
///
/// Port of the Java interface `ShapeBoundingDirections` and its two singleton
/// implementations `OrthogonalBoundingDirections` and
/// `FortyfiveDegreeBoundingDirections`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShapeBoundingDirections {
    /// The 4 orthogonal directions (bounding shapes are boxes).
    Orthogonal,
    /// The 8 directions in multiples of 45 degrees (bounding shapes are octagons).
    FortyfiveDegree,
}

impl ShapeBoundingDirections {
    /// The number of fixed directions.
    pub fn count(self) -> usize {
        match self {
            ShapeBoundingDirections::Orthogonal => 4,
            ShapeBoundingDirections::FortyfiveDegree => 8,
        }
    }

    /// A surrounding regular shape for `shape`; `None` where Java returns `null` (an
    /// unbounded simplex with 45 degree directions).
    pub fn bounds(self, shape: &ConvexShape) -> Option<RegularTileShape> {
        shape.bounding_shape(self)
    }

    /// A surrounding regular shape for a box.
    pub fn bounds_box(self, b: &IntBox) -> RegularTileShape {
        match self {
            ShapeBoundingDirections::Orthogonal => RegularTileShape::IntBox(*b),
            ShapeBoundingDirections::FortyfiveDegree => {
                RegularTileShape::IntOctagon(b.to_int_octagon())
            }
        }
    }

    /// A surrounding regular shape for an octagon.
    pub fn bounds_octagon(self, oct: &IntOctagon) -> RegularTileShape {
        match self {
            ShapeBoundingDirections::Orthogonal => RegularTileShape::IntBox(oct.bounding_box()),
            ShapeBoundingDirections::FortyfiveDegree => RegularTileShape::IntOctagon(*oct),
        }
    }

    /// A surrounding regular shape for a simplex.
    pub fn bounds_simplex(self, simplex: &Simplex) -> Option<RegularTileShape> {
        match self {
            ShapeBoundingDirections::Orthogonal => {
                Some(RegularTileShape::IntBox(simplex.bounding_box()))
            }
            ShapeBoundingDirections::FortyfiveDegree => {
                simplex.bounding_octagon().map(RegularTileShape::IntOctagon)
            }
        }
    }

    /// A surrounding regular shape for a circle.
    pub fn bounds_circle(self, circle: &Circle) -> RegularTileShape {
        match self {
            ShapeBoundingDirections::Orthogonal => RegularTileShape::IntBox(circle.bounding_box()),
            ShapeBoundingDirections::FortyfiveDegree => {
                RegularTileShape::IntOctagon(circle.bounding_octagon())
            }
        }
    }

    /// A surrounding regular shape for a polygon.
    pub fn bounds_polygon(self, polygon: &PolygonShape) -> RegularTileShape {
        match self {
            ShapeBoundingDirections::Orthogonal => RegularTileShape::IntBox(polygon.bounding_box()),
            ShapeBoundingDirections::FortyfiveDegree => {
                RegularTileShape::IntOctagon(polygon.polygon_bounding_octagon())
            }
        }
    }
}

/// A simply connected shape in the plane.
///
/// Port of the Java interface `Shape` and its implementations.
#[derive(Debug, Clone)]
pub enum Shape {
    Circle(Circle),
    Polyline(PolylineShape),
}

impl From<Circle> for Shape {
    fn from(c: Circle) -> Self {
        Shape::Circle(c)
    }
}

impl From<PolylineShape> for Shape {
    fn from(p: PolylineShape) -> Self {
        Shape::Polyline(p)
    }
}

impl From<TileShape> for Shape {
    fn from(t: TileShape) -> Self {
        Shape::Polyline(PolylineShape::Tile(t))
    }
}

impl From<IntBox> for Shape {
    fn from(b: IntBox) -> Self {
        Shape::from(TileShape::IntBox(b))
    }
}

impl From<IntOctagon> for Shape {
    fn from(o: IntOctagon) -> Self {
        Shape::from(TileShape::IntOctagon(o))
    }
}

impl From<Simplex> for Shape {
    fn from(s: Simplex) -> Self {
        Shape::from(TileShape::Simplex(s))
    }
}

impl From<PolygonShape> for Shape {
    fn from(p: PolygonShape) -> Self {
        Shape::Polyline(PolylineShape::Polygon(p))
    }
}

impl From<ConvexShape> for Shape {
    fn from(c: ConvexShape) -> Self {
        match c {
            ConvexShape::Circle(c) => Shape::Circle(c),
            ConvexShape::Tile(t) => Shape::from(t),
        }
    }
}

macro_rules! shape_dispatch {
    ($self:ident, $c:ident => $ce:expr, $p:ident => $pe:expr) => {
        match $self {
            Shape::Circle($c) => $ce,
            Shape::Polyline($p) => $pe,
        }
    };
}

impl Shape {
    /// True if the shape is empty.
    pub fn is_empty(&self) -> bool {
        shape_dispatch!(self, c => c.is_empty(), p => p.is_empty())
    }

    /// True if the shape is contained in a sufficiently large box.
    pub fn is_bounded(&self) -> bool {
        shape_dispatch!(self, c => c.is_bounded(), p => p.is_bounded())
    }

    /// 2, 1, 0, or -1 if empty.
    pub fn dimension(&self) -> i32 {
        shape_dispatch!(self, c => c.dimension(), p => p.dimension())
    }

    /// True if the shape is completely contained in `b`.
    pub fn is_contained_in(&self, b: &IntBox) -> bool {
        shape_dispatch!(self, c => c.is_contained_in(b), p => p.is_contained_in(b))
    }

    /// The smallest surrounding box.
    pub fn bounding_box(&self) -> IntBox {
        shape_dispatch!(self, c => c.bounding_box(), p => p.bounding_box())
    }

    /// The smallest surrounding octagon; `None` for an unbounded simplex.
    pub fn bounding_octagon(&self) -> Option<IntOctagon> {
        shape_dispatch!(self, c => Some(c.bounding_octagon()), p => p.bounding_octagon())
    }

    /// True if `point` is contained in the shape.
    pub fn contains_float(&self, point: FloatPoint) -> bool {
        shape_dispatch!(self, c => c.contains_float(point), p => p.contains_float(point))
    }

    /// True if `point` is inside or on the border of the shape.
    pub fn contains(&self, point: &Point) -> bool {
        shape_dispatch!(self, c => c.contains(point), p => p.contains(point))
    }

    /// Approximation of the nearest point of the shape to `from_point`; `None` where
    /// Java returns `null`.
    pub fn nearest_point_approx(&self, from_point: FloatPoint) -> Option<FloatPoint> {
        shape_dispatch!(self, c => c.nearest_point_approx(from_point), p => p.nearest_point_approx(from_point))
    }

    /// Approximations of the corners.
    pub fn corner_approx_arr(&self) -> Vec<FloatPoint> {
        shape_dispatch!(self, c => c.corner_approx_arr(), p => p.corner_approx_arr())
    }

    /// The division into convex pieces; `None` if it fails.
    pub fn split_to_convex(&self) -> Option<Vec<TileShape>> {
        shape_dispatch!(self, c => Some(c.split_to_convex()), p => p.split_to_convex())
    }

    /// Length of the border; `i32::MAX` if unbounded.
    pub fn circumference(&self) -> f64 {
        shape_dispatch!(self, c => c.circumference(), p => p.circumference())
    }

    /// Content of the area; `f64::MAX` if unbounded.
    pub fn area(&self) -> f64 {
        shape_dispatch!(self, c => c.area(), p => p.area())
    }

    /// The centre of gravity.
    pub fn centre_of_gravity(&self) -> FloatPoint {
        shape_dispatch!(self, c => c.centre_of_gravity(), p => p.centre_of_gravity())
    }

    /// True if `point` is neither inside nor on the border.
    pub fn is_outside(&self, point: &Point) -> bool {
        shape_dispatch!(self, c => c.is_outside(point), p => p.is_outside(point))
    }

    /// True if `point` is inside but not on the border.
    pub fn contains_inside(&self, point: &Point) -> bool {
        shape_dispatch!(self, c => c.contains_inside(point), p => p.contains_inside(point))
    }

    /// True if `point` lies exactly on the border.
    pub fn contains_on_border(&self, point: &Point) -> bool {
        shape_dispatch!(self, c => c.contains_on_border(point), p => p.contains_on_border(point))
    }

    /// Distance from `point` to the shape; 0 if contained.
    pub fn distance(&self, point: FloatPoint) -> f64 {
        shape_dispatch!(self, c => c.distance(point), p => p.distance(point))
    }

    /// A bounding convex tile.
    pub fn bounding_tile(&self) -> TileShape {
        shape_dispatch!(self, c => c.bounding_tile(), p => p.bounding_tile())
    }

    /// The bounding regular shape with the fixed directions `dirs`.
    pub fn bounding_shape(&self, dirs: ShapeBoundingDirections) -> Option<RegularTileShape> {
        match self {
            Shape::Circle(c) => Some(dirs.bounds_circle(c)),
            Shape::Polyline(PolylineShape::Tile(t)) => t.bounding_shape(dirs),
            Shape::Polyline(PolylineShape::Polygon(p)) => Some(dirs.bounds_polygon(p)),
        }
    }

    /// Distance from `point` to the border.
    pub fn border_distance(&self, point: FloatPoint) -> f64 {
        shape_dispatch!(self, c => c.border_distance(point), p => p.border_distance(point))
    }

    /// Smallest distance from the centre of gravity to the border.
    pub fn smallest_radius(&self) -> f64 {
        shape_dispatch!(self, c => c.smallest_radius(), p => p.smallest_radius())
    }

    /// The shape with its border moved outwards by `offset`; `None` where Java returns
    /// `null` (`PolygonShape.enlarge` is not implemented).
    pub fn enlarge(&self, offset: f64) -> Option<Shape> {
        match self {
            Shape::Circle(c) => Some(Shape::Circle(c.enlarge(offset))),
            Shape::Polyline(PolylineShape::Tile(t)) => Some(Shape::from(t.enlarge(offset))),
            Shape::Polyline(PolylineShape::Polygon(p)) => p.enlarge(offset).map(Shape::from),
        }
    }

    /// True if this shape and `other` have a nonempty intersection.
    ///
    /// Java's `PolygonShape.intersects(PolygonShape)` recurses forever; this port tests
    /// the convex pieces of both polygons against each other.
    pub fn intersects(&self, other: &Shape) -> bool {
        match self {
            Shape::Circle(c) => other.intersects_circle(c),
            Shape::Polyline(PolylineShape::Tile(TileShape::IntBox(b))) => other.intersects_box(b),
            Shape::Polyline(PolylineShape::Tile(TileShape::IntOctagon(o))) => {
                other.intersects_octagon(o)
            }
            Shape::Polyline(PolylineShape::Tile(TileShape::Simplex(s))) => {
                other.intersects_simplex(s)
            }
            Shape::Polyline(PolylineShape::Polygon(p)) => match other {
                Shape::Polyline(PolylineShape::Polygon(q)) => p.intersects_polygon(q),
                _ => other.intersects(self),
            },
        }
    }

    /// Java `intersects(IntBox)` overload.
    pub fn intersects_box(&self, b: &IntBox) -> bool {
        match self {
            Shape::Circle(c) => c.intersects_box(b),
            Shape::Polyline(PolylineShape::Tile(t)) => t.intersects_box(b),
            Shape::Polyline(PolylineShape::Polygon(p)) => p.intersects_box(b),
        }
    }

    /// Java `intersects(IntOctagon)` overload.
    pub fn intersects_octagon(&self, o: &IntOctagon) -> bool {
        match self {
            Shape::Circle(c) => c.intersects_octagon(o),
            Shape::Polyline(PolylineShape::Tile(t)) => t.intersects_octagon(o),
            Shape::Polyline(PolylineShape::Polygon(p)) => p.intersects_octagon(o),
        }
    }

    /// Java `intersects(Simplex)` overload.
    pub fn intersects_simplex(&self, s: &Simplex) -> bool {
        match self {
            Shape::Circle(c) => c.intersects_simplex(s),
            Shape::Polyline(PolylineShape::Tile(t)) => t.intersects_simplex(s),
            Shape::Polyline(PolylineShape::Polygon(p)) => p.intersects_simplex(s),
        }
    }

    /// Java `intersects(Circle)` overload.
    pub fn intersects_circle(&self, c: &Circle) -> bool {
        match self {
            Shape::Circle(d) => d.intersects_circle(c),
            Shape::Polyline(PolylineShape::Tile(t)) => t.intersects_circle(c),
            Shape::Polyline(PolylineShape::Polygon(p)) => p.intersects_circle(c),
        }
    }

    /// Cuts out the parts of `polyline` in the interior of this shape; `None` where
    /// Java returns `null` (not implemented for circles and polygons).
    pub fn cutout(&self, polyline: &Polyline) -> Option<Vec<Polyline>> {
        match self {
            Shape::Circle(_) | Shape::Polyline(PolylineShape::Polygon(_)) => None,
            Shape::Polyline(PolylineShape::Tile(t)) => Some(t.cutout_polyline(polyline)),
        }
    }

    /// The shape translated by `vector`.
    pub fn translate_by(&self, vector: &Vector) -> Shape {
        shape_dispatch!(self, c => Shape::Circle(c.translate_by(vector)), p => Shape::Polyline(p.translate_by(vector)))
    }

    /// The shape turned by `factor` times 90 degrees around `pole`.
    pub fn turn_90_degree(&self, factor: i32, pole: IntPoint) -> Shape {
        shape_dispatch!(self, c => Shape::Circle(c.turn_90_degree(factor, pole)), p => Shape::Polyline(p.turn_90_degree(factor, pole)))
    }

    /// The shape rotated by `angle` around `pole`; not exact.
    pub fn rotate_approx(&self, angle: f64, pole: FloatPoint) -> Shape {
        shape_dispatch!(self, c => Shape::Circle(c.rotate_approx(angle, pole)), p => Shape::Polyline(p.rotate_approx(angle, pole)))
    }

    /// The shape mirrored at the vertical line through `pole`.
    pub fn mirror_vertical(&self, pole: IntPoint) -> Shape {
        shape_dispatch!(self, c => Shape::Circle(c.mirror_vertical(pole)), p => Shape::Polyline(p.mirror_vertical(pole)))
    }

    /// The shape mirrored at the horizontal line through `pole`.
    pub fn mirror_horizontal(&self, pole: IntPoint) -> Shape {
        shape_dispatch!(self, c => Shape::Circle(c.mirror_horizontal(pole)), p => Shape::Polyline(p.mirror_horizontal(pole)))
    }

    /// The border of the shape (the shape itself).
    pub fn get_border(&self) -> Shape {
        self.clone()
    }

    /// The holes of the shape (none).
    pub fn get_holes(&self) -> Vec<Shape> {
        Vec::new()
    }
}

impl TileShape {
    /// Java `intersects(IntBox)` overload.
    pub fn intersects_box(&self, b: &IntBox) -> bool {
        match self {
            TileShape::IntBox(a) => a.intersects_box(b),
            TileShape::IntOctagon(a) => a.intersects_octagon(&b.to_int_octagon()),
            TileShape::Simplex(a) => a.intersects_box_simplex(b),
        }
    }

    /// Java `intersects(IntOctagon)` overload.
    pub fn intersects_octagon(&self, o: &IntOctagon) -> bool {
        match self {
            TileShape::IntBox(a) => o.intersects_octagon(&a.to_int_octagon()),
            TileShape::IntOctagon(a) => a.intersects_octagon(o),
            TileShape::Simplex(a) => a.intersects_octagon_simplex(o),
        }
    }

    /// Java `intersects(Simplex)` overload.
    pub fn intersects_simplex(&self, s: &Simplex) -> bool {
        match self {
            TileShape::IntBox(a) => s.intersects_box_simplex(a),
            TileShape::IntOctagon(a) => s.intersects_octagon_simplex(a),
            TileShape::Simplex(a) => a.intersects_simplex(s),
        }
    }

    /// Java `intersects(Circle)` overload.
    pub fn intersects_circle(&self, c: &Circle) -> bool {
        match self {
            TileShape::IntBox(a) => c.intersects_box(a),
            TileShape::IntOctagon(a) => c.intersects_octagon(a),
            TileShape::Simplex(a) => c.intersects_simplex(a),
        }
    }
}

/// A convex shape: a circle or a convex polygon.
///
/// Port of the Java interface `ConvexShape` and its implementations.
#[derive(Debug, Clone)]
pub enum ConvexShape {
    Circle(Circle),
    Tile(TileShape),
}

impl From<Circle> for ConvexShape {
    fn from(c: Circle) -> Self {
        ConvexShape::Circle(c)
    }
}

impl From<TileShape> for ConvexShape {
    fn from(t: TileShape) -> Self {
        ConvexShape::Tile(t)
    }
}

impl ConvexShape {
    /// The offset shape: enlarged if `distance > 0`, else shrunk.
    pub fn offset(&self, distance: f64) -> ConvexShape {
        match self {
            ConvexShape::Circle(c) => ConvexShape::Circle(c.offset(distance)),
            ConvexShape::Tile(t) => ConvexShape::Tile(t.offset(distance)),
        }
    }

    /// The shape shrunk by `offset`; not empty.
    pub fn shrink(&self, offset: f64) -> ConvexShape {
        match self {
            ConvexShape::Circle(c) => ConvexShape::Circle(c.shrink(offset)),
            ConvexShape::Tile(t) => ConvexShape::Tile(t.shrink(offset)),
        }
    }

    /// The maximum diameter.
    pub fn max_width(&self) -> f64 {
        match self {
            ConvexShape::Circle(c) => c.max_width(),
            ConvexShape::Tile(t) => t.max_width(),
        }
    }

    /// The minimum diameter.
    pub fn min_width(&self) -> f64 {
        match self {
            ConvexShape::Circle(c) => c.min_width(),
            ConvexShape::Tile(t) => t.min_width(),
        }
    }

    /// True if the shape is empty.
    pub fn is_empty(&self) -> bool {
        match self {
            ConvexShape::Circle(c) => c.is_empty(),
            ConvexShape::Tile(t) => t.is_empty(),
        }
    }

    /// The bounding regular shape with the fixed directions `dirs`.
    pub fn bounding_shape(&self, dirs: ShapeBoundingDirections) -> Option<RegularTileShape> {
        match self {
            ConvexShape::Circle(c) => Some(dirs.bounds_circle(c)),
            ConvexShape::Tile(t) => t.bounding_shape(dirs),
        }
    }

    /// The shape as a [`Shape`].
    pub fn to_shape(&self) -> Shape {
        Shape::from(self.clone())
    }
}

/// A not necessarily simply connected shape: a [`Shape`] or a polyline shape with
/// holes.
///
/// Port of the Java interface `Area` and its implementations.
#[derive(Debug, Clone)]
pub enum Area {
    Shape(Shape),
    PolylineArea(PolylineArea),
}

impl From<Shape> for Area {
    fn from(s: Shape) -> Self {
        Area::Shape(s)
    }
}

impl From<PolylineArea> for Area {
    fn from(a: PolylineArea) -> Self {
        Area::PolylineArea(a)
    }
}

impl Area {
    /// True if the area is empty.
    pub fn is_empty(&self) -> bool {
        match self {
            Area::Shape(s) => s.is_empty(),
            Area::PolylineArea(a) => a.is_empty(),
        }
    }

    /// True if the area is contained in a sufficiently large box.
    pub fn is_bounded(&self) -> bool {
        match self {
            Area::Shape(s) => s.is_bounded(),
            Area::PolylineArea(a) => a.is_bounded(),
        }
    }

    /// 2, 1, 0, or -1 if empty.
    pub fn dimension(&self) -> i32 {
        match self {
            Area::Shape(s) => s.dimension(),
            Area::PolylineArea(a) => a.dimension(),
        }
    }

    /// True if the area is completely contained in `b`.
    pub fn is_contained_in(&self, b: &IntBox) -> bool {
        match self {
            Area::Shape(s) => s.is_contained_in(b),
            Area::PolylineArea(a) => a.is_contained_in(b),
        }
    }

    /// The border shape.
    pub fn get_border(&self) -> Shape {
        match self {
            Area::Shape(s) => s.get_border(),
            Area::PolylineArea(a) => Shape::Polyline(a.get_border().clone()),
        }
    }

    /// The holes.
    pub fn get_holes(&self) -> Vec<Shape> {
        match self {
            Area::Shape(s) => s.get_holes(),
            Area::PolylineArea(a) => a.get_holes().iter().cloned().map(Shape::Polyline).collect(),
        }
    }

    /// The smallest surrounding box.
    pub fn bounding_box(&self) -> IntBox {
        match self {
            Area::Shape(s) => s.bounding_box(),
            Area::PolylineArea(a) => a.bounding_box(),
        }
    }

    /// The smallest surrounding octagon.
    pub fn bounding_octagon(&self) -> Option<IntOctagon> {
        match self {
            Area::Shape(s) => s.bounding_octagon(),
            Area::PolylineArea(a) => a.bounding_octagon(),
        }
    }

    /// True if `point` is contained in the area but not in a hole.
    pub fn contains_float(&self, point: FloatPoint) -> bool {
        match self {
            Area::Shape(s) => s.contains_float(point),
            Area::PolylineArea(a) => a.contains_float(point),
        }
    }

    /// True if `point` is inside or on the border of the area, but not inside a hole.
    pub fn contains(&self, point: &Point) -> bool {
        match self {
            Area::Shape(s) => s.contains(point),
            Area::PolylineArea(a) => a.contains(point),
        }
    }

    /// Approximation of the nearest point of the area to `from_point`.
    pub fn nearest_point_approx(&self, from_point: FloatPoint) -> Option<FloatPoint> {
        match self {
            Area::Shape(s) => s.nearest_point_approx(from_point),
            Area::PolylineArea(a) => a.nearest_point_approx(from_point),
        }
    }

    /// [`Area::nearest_point_approx`] with the switches of the cutouts (N06), which
    /// reach only a [`PolylineArea`]'s holes.
    pub fn nearest_point_approx_with(
        &self,
        from_point: FloatPoint,
        quirks: crate::quirks::SplitQuirks,
    ) -> Option<FloatPoint> {
        match self {
            Area::Shape(s) => s.nearest_point_approx(from_point),
            Area::PolylineArea(a) => a.nearest_point_approx_with(from_point, quirks),
        }
    }

    /// The area turned by `factor` times 90 degrees around `pole`.
    pub fn turn_90_degree(&self, factor: i32, pole: IntPoint) -> Area {
        match self {
            Area::Shape(s) => Area::Shape(s.turn_90_degree(factor, pole)),
            Area::PolylineArea(a) => Area::PolylineArea(a.turn_90_degree(factor, pole)),
        }
    }

    /// The area rotated by `angle` around `pole`; not exact.
    pub fn rotate_approx(&self, angle: f64, pole: FloatPoint) -> Area {
        match self {
            Area::Shape(s) => Area::Shape(s.rotate_approx(angle, pole)),
            Area::PolylineArea(a) => Area::PolylineArea(a.rotate_approx(angle, pole)),
        }
    }

    /// The area translated by `vector`.
    pub fn translate_by(&self, vector: &Vector) -> Area {
        match self {
            Area::Shape(s) => Area::Shape(s.translate_by(vector)),
            Area::PolylineArea(a) => Area::PolylineArea(a.translate_by(vector)),
        }
    }

    /// The area mirrored at the horizontal line through `pole`.
    pub fn mirror_horizontal(&self, pole: IntPoint) -> Area {
        match self {
            Area::Shape(s) => Area::Shape(s.mirror_horizontal(pole)),
            Area::PolylineArea(a) => Area::PolylineArea(a.mirror_horizontal(pole)),
        }
    }

    /// The area mirrored at the vertical line through `pole`.
    pub fn mirror_vertical(&self, pole: IntPoint) -> Area {
        match self {
            Area::Shape(s) => Area::Shape(s.mirror_vertical(pole)),
            Area::PolylineArea(a) => Area::PolylineArea(a.mirror_vertical(pole)),
        }
    }

    /// Approximations of the corners.
    pub fn corner_approx_arr(&self) -> Vec<FloatPoint> {
        match self {
            Area::Shape(s) => s.corner_approx_arr(),
            Area::PolylineArea(a) => a.corner_approx_arr(),
        }
    }

    /// The division into convex pieces; `None` if it fails.
    pub fn split_to_convex(&self) -> Option<Vec<TileShape>> {
        match self {
            Area::Shape(s) => s.split_to_convex(),
            Area::PolylineArea(a) => a.split_to_convex(None),
        }
    }

    /// [`Area::split_to_convex`] that can be interrupted through `stoppable`.
    pub fn split_to_convex_stoppable(&self, stoppable: &dyn Stoppable) -> Option<Vec<TileShape>> {
        match self {
            Area::Shape(s) => s.split_to_convex(),
            Area::PolylineArea(a) => a.split_to_convex(Some(stoppable)),
        }
    }

    /// [`Area::split_to_convex`] with the switches of the cutouts (N06 batch G) and an
    /// optional `stoppable`. A plain shape's split makes no cutout, so the switches
    /// reach only a [`PolylineArea`]'s holes.
    pub fn split_to_convex_with(
        &self,
        stoppable: Option<&dyn Stoppable>,
        quirks: crate::quirks::SplitQuirks,
    ) -> Option<Vec<TileShape>> {
        match self {
            Area::Shape(s) => s.split_to_convex(),
            Area::PolylineArea(a) => a.split_to_convex_with(stoppable, quirks),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Port of SealedGeometryHierarchyTest.tileShapeHierarchyHasOnlyRegularAndSimplexBranches:
    // the exhaustive matches below fail to compile if a variant is added.
    #[test]
    fn tile_shape_hierarchy_is_closed() {
        fn kind(t: &TileShape) -> &'static str {
            match t {
                TileShape::IntBox(_) | TileShape::IntOctagon(_) => "regular",
                TileShape::Simplex(_) => "simplex",
            }
        }
        fn regular_kind(r: &RegularTileShape) -> &'static str {
            match r {
                RegularTileShape::IntBox(_) => "box",
                RegularTileShape::IntOctagon(_) => "octagon",
            }
        }
        assert_eq!(kind(&TileShape::IntBox(IntBox::EMPTY)), "regular");
        assert_eq!(kind(&TileShape::Simplex(Simplex::empty())), "simplex");
        assert_eq!(
            regular_kind(&RegularTileShape::IntOctagon(IntOctagon::EMPTY)),
            "octagon"
        );
        assert_eq!(
            regular_kind(&RegularTileShape::IntBox(IntBox::EMPTY)),
            "box"
        );
    }

    #[test]
    fn double_dispatch_intersections() {
        let b = TileShape::IntBox(IntBox::new(0, 0, 10, 10));
        let o = TileShape::IntOctagon(IntBox::new(5, 5, 20, 20).to_int_octagon());
        let s = TileShape::Simplex(IntBox::new(8, -5, 30, 9).to_simplex());
        for (x, y) in [(&b, &o), (&o, &s), (&b, &s)] {
            assert!(x.intersects_tile(y));
            assert!(y.intersects_tile(x));
            let i1 = x.intersection(y);
            let i2 = y.intersection(x);
            assert_eq!(i1.bounding_box(), i2.bounding_box());
        }
        assert_eq!(b.intersection(&o).bounding_box(), IntBox::new(5, 5, 10, 10));
        assert_eq!(
            b.intersection(&s).simplify().bounding_box(),
            IntBox::new(8, 0, 10, 9)
        );
        let far = TileShape::IntBox(IntBox::new(100, 100, 110, 110));
        assert!(!b.intersects_tile(&far));
        assert!(!s.intersects_tile(&far));
        let circle = Shape::Circle(Circle::new(IntPoint::new(15, 5), 5));
        assert!(Shape::from(b.clone()).intersects(&circle));
        assert!(circle.intersects(&Shape::from(b.clone())));
        assert!(!circle.intersects(&Shape::from(far)));
    }

    #[test]
    fn regular_shape_union_and_compare() {
        let a = RegularTileShape::IntBox(IntBox::new(0, 0, 10, 10));
        let b = RegularTileShape::IntBox(IntBox::new(5, -5, 20, 5));
        assert_eq!(
            a.union(&b),
            RegularTileShape::IntBox(IntBox::new(0, -5, 20, 10))
        );
        assert_eq!(a.compare(&b, 0), Side::OnTheLeft);
        assert_eq!(a.compare(&b, 1), Side::OnTheLeft);
        assert_eq!(a.compare(&a, 2), Side::Collinear);
        let o = RegularTileShape::IntOctagon(IntBox::new(-1, -1, 30, 30).to_int_octagon());
        assert!(o.contains_regular(&a));
        assert!(!a.contains_regular(&o));
        assert!(matches!(a.union(&o), RegularTileShape::IntOctagon(_)));
        assert_eq!(
            ShapeBoundingDirections::FortyfiveDegree.bounds_box(&IntBox::new(0, 0, 1, 1)),
            RegularTileShape::IntOctagon(IntBox::new(0, 0, 1, 1).to_int_octagon())
        );
    }
}
