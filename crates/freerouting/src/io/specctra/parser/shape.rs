//! DSN shapes with their layers resolved, and their conversion to board
//! geometry.
//!
//! Port of `io.specctra.parser.Shape` and its subclasses `Rectangle`,
//! `Circle`, `Polygon`, `PolygonPath` and `PolylinePath` (the reading half
//! is in `freerouting-dsn`).

use freerouting_dsn::ast;
use freerouting_geometry::jmath::round_to_i32;
use freerouting_geometry::{
    Area, Circle, FloatPoint, IntBox, IntPoint, Point, PolygonShape, PolylineArea, PolylineShape,
    Shape, Simplex,
};

use super::layer::{DsnLayer, DsnLayerStructure};
use crate::io::CoordinateTransform;

/// The kind of a DSN shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DsnShapeKind {
    Rectangle,
    Circle,
    Polygon,
    /// `path`: a polygon with a width.
    PolygonPath,
    /// `polyline_path`: a list of lines with a width.
    PolylinePath,
}

/// A DSN shape on a resolved layer, in DSN coordinates.
///
/// Port of `io.specctra.parser.Shape`. The coordinates are kept as Java
/// keeps them: `[x1, y1, x2, y2]` for a rectangle, `[diameter, x, y]` for a
/// circle and the flat corner list for the others.
#[derive(Clone, Debug)]
pub(crate) struct DsnShape {
    pub layer: DsnLayer,
    pub kind: DsnShapeKind,
    pub width: f64,
    pub coor: Vec<f64>,
}

fn flat(points: &[ast::Point]) -> Vec<f64> {
    points.iter().flat_map(|p| [p.x, p.y]).collect()
}

/// Java `Shape.getLayer`: `pcb` and `signal` are the pseudo layers; other
/// names need a layer structure.
fn get_layer(structure: Option<&DsnLayerStructure>, layer: &ast::LayerRef) -> Option<DsnLayer> {
    match layer {
        ast::LayerRef::Pcb => Some(DsnLayer::pcb()),
        ast::LayerRef::Signal => Some(DsnLayer::signal()),
        ast::LayerRef::Named(name) => {
            let structure = structure?;
            let index = structure.get_no(name);
            if index < 0 || index as usize >= structure.layers.len() {
                return None;
            }
            Some(structure.layers[index as usize].clone())
        }
    }
}

impl DsnShape {
    /// Resolves the layer of `shape` the way the Java reader does while
    /// scanning it. `None` where Java's reader returns `null`.
    pub fn resolve(shape: &ast::Shape, structure: Option<&DsnLayerStructure>) -> Option<DsnShape> {
        match shape {
            ast::Shape::Rect {
                layer,
                x1,
                y1,
                x2,
                y2,
            } => {
                // An unknown layer falls back to `signal`.
                let layer = get_layer(structure, layer).unwrap_or_else(DsnLayer::signal);
                Some(DsnShape {
                    layer,
                    kind: DsnShapeKind::Rectangle,
                    width: 0.0,
                    coor: vec![*x1, *y1, *x2, *y2],
                })
            }
            ast::Shape::Circle {
                layer,
                diameter,
                center,
            } => Some(DsnShape {
                layer: get_layer(structure, layer)?,
                kind: DsnShapeKind::Circle,
                width: 0.0,
                coor: vec![*diameter, center.x, center.y],
            }),
            ast::Shape::Polygon {
                layer,
                aperture_width,
                points,
            } => Some(DsnShape {
                layer: get_layer(structure, layer)?,
                kind: DsnShapeKind::Polygon,
                width: *aperture_width,
                coor: flat(points),
            }),
            ast::Shape::Path {
                layer,
                width,
                points,
            } => {
                // Java skips paths with fewer than two points before it
                // looks at the layer.
                if points.len() < 2 {
                    return None;
                }
                Some(DsnShape {
                    layer: get_layer(structure, layer)?,
                    kind: DsnShapeKind::PolygonPath,
                    width: *width,
                    coor: flat(points),
                })
            }
            ast::Shape::PolylinePath {
                layer,
                width,
                points,
            } => {
                if points.len() < 2 {
                    return None;
                }
                // Java keeps a polyline path with an unknown layer and fails
                // later on its null layer; it is dropped here instead.
                Some(DsnShape {
                    layer: get_layer(structure, layer)?,
                    kind: DsnShapeKind::PolylinePath,
                    width: *width,
                    coor: flat(points),
                })
            }
        }
    }

    /// True for `path` and `polyline_path` (Java `instanceof Path`).
    pub fn is_path(&self) -> bool {
        matches!(
            self.kind,
            DsnShapeKind::PolygonPath | DsnShapeKind::PolylinePath
        )
    }

    /// The bounding rectangle in DSN coordinates as `[x1, y1, x2, y2]`,
    /// including Java's quirks: a rectangle returns its own (unsorted)
    /// coordinates, a circle uses its diameter as radius, a path adds the
    /// half width to the maximum x after taking the maximum. `None` for a
    /// polyline path.
    ///
    /// `java_quirk` is `router.java_quirks.dsn_shape_bbox` (N06): `false` sorts a rectangle's coordinates, takes half a circle's diameter as
    /// its radius and adds a path's half width to each x.
    pub fn bounding_box_with(&self, java_quirk: bool) -> Option<[f64; 4]> {
        if crate::run_stats::enabled()
            && self.bounding_box_impl(true) != self.bounding_box_impl(false)
        {
            crate::run_stats::record_n06(crate::run_stats::Counter::DsnShapeBbox);
        }
        self.bounding_box_impl(java_quirk)
    }

    fn bounding_box_impl(&self, java_quirk: bool) -> Option<[f64; 4]> {
        let c = &self.coor;
        match self.kind {
            // PARITY(quirk, java_quirks.dsn_shape_bbox): Rectangle.java:25,
            // Circle.java:57-63, PolygonPath.java:111-130.
            DsnShapeKind::Rectangle if !java_quirk => Some([
                c[0].min(c[2]),
                c[1].min(c[3]),
                c[0].max(c[2]),
                c[1].max(c[3]),
            ]),
            DsnShapeKind::Rectangle => Some([c[0], c[1], c[2], c[3]]),
            DsnShapeKind::Circle if !java_quirk => {
                let r = c[0] / 2.0;
                Some([c[1] - r, c[2] - r, c[1] + r, c[2] + r])
            }
            DsnShapeKind::Circle => Some([c[1] - c[0], c[2] - c[0], c[1] + c[0], c[2] + c[0]]),
            DsnShapeKind::Polygon => {
                let mut b = [
                    f64::from(i32::MAX),
                    f64::from(i32::MAX),
                    f64::from(i32::MIN),
                    f64::from(i32::MIN),
                ];
                for (i, v) in c.iter().enumerate() {
                    if i % 2 == 0 {
                        b[0] = b[0].min(*v);
                        b[2] = b[2].max(*v);
                    } else {
                        b[1] = b[1].min(*v);
                        b[3] = b[3].max(*v);
                    }
                }
                Some(b)
            }
            DsnShapeKind::PolygonPath => {
                let offset = self.width / 2.0;
                let mut b = [
                    f64::from(i32::MAX),
                    f64::from(i32::MAX),
                    f64::from(i32::MIN),
                    f64::from(i32::MIN),
                ];
                for (i, v) in c.iter().enumerate() {
                    if i % 2 == 0 {
                        b[0] = b[0].min(*v - offset);
                        b[2] = if java_quirk {
                            b[2].max(*v) + offset
                        } else {
                            b[2].max(*v + offset)
                        };
                    } else {
                        b[1] = b[1].min(*v - offset);
                        b[3] = b[3].max(*v + offset);
                    }
                }
                Some(b)
            }
            DsnShapeKind::PolylinePath => None,
        }
    }

    /// The shape in board coordinates (Java `transformToBoard`). `None`
    /// for a polyline path.
    pub fn transform_to_board(&self, transform: &CoordinateTransform) -> Option<Shape> {
        let c = &self.coor;
        match self.kind {
            DsnShapeKind::Rectangle => Some(Shape::from(rectangle_to_board(c, transform))),
            DsnShapeKind::Circle => {
                let center = transform.dsn_to_board_point(c[1], c[2]).round();
                let radius = round_to_i32(transform.dsn_to_board(c[0]) / 2.0);
                Some(Shape::Circle(Circle::new(center, radius)))
            }
            DsnShapeKind::Polygon => {
                let corners: Vec<Point> = c
                    .chunks_exact(2)
                    .map(|p| Point::Int(transform.dsn_to_board_point(p[0], p[1]).round()))
                    .collect();
                Some(Shape::from(PolygonShape::new(&corners)))
            }
            DsnShapeKind::PolygonPath => {
                let corners: Vec<FloatPoint> = c
                    .chunks_exact(2)
                    .map(|p| transform.dsn_to_board_point(p[0], p[1]))
                    .collect();
                Some(polygon_path_to_board(
                    &corners,
                    transform.dsn_to_board(self.width) / 2.0,
                ))
            }
            DsnShapeKind::PolylinePath => None,
        }
    }

    /// The shape relative to a package origin in board coordinates (Java
    /// `transformToBoardRel`). `None` for a polyline path.
    pub fn transform_to_board_rel(&self, transform: &CoordinateTransform) -> Option<Shape> {
        let c = &self.coor;
        match self.kind {
            DsnShapeKind::Rectangle => {
                let b: Vec<i32> = c
                    .iter()
                    .map(|v| round_to_i32(transform.dsn_to_board(*v)))
                    .collect();
                let result = if b[1] <= b[3] {
                    IntBox::new(b[0], b[1], b[2], b[3])
                } else {
                    IntBox::new(b[0], b[3], b[2], b[1])
                };
                Some(Shape::from(result))
            }
            DsnShapeKind::Circle => {
                let radius = round_to_i32(transform.dsn_to_board(c[0]) / 2.0);
                let x = round_to_i32(transform.dsn_to_board(c[1]));
                let y = round_to_i32(transform.dsn_to_board(c[2]));
                Some(Shape::Circle(Circle::new(IntPoint::new(x, y), radius)))
            }
            DsnShapeKind::Polygon => {
                if c.len() < 2 {
                    return Some(Shape::from(Simplex::empty()));
                }
                let corners: Vec<Point> = c
                    .chunks_exact(2)
                    .map(|p| {
                        Point::Int(IntPoint::new(
                            round_to_i32(transform.dsn_to_board(p[0])),
                            round_to_i32(transform.dsn_to_board(p[1])),
                        ))
                    })
                    .collect();
                Some(Shape::from(PolygonShape::new(&corners)))
            }
            DsnShapeKind::PolygonPath => {
                let corners: Vec<FloatPoint> = c
                    .chunks_exact(2)
                    .map(|p| transform.dsn_to_board_rel(p[0], p[1]))
                    .collect();
                Some(polygon_path_to_board(
                    &corners,
                    transform.dsn_to_board(self.width) / 2.0,
                ))
            }
            DsnShapeKind::PolylinePath => None,
        }
    }
}

/// Java `Rectangle.transformToBoard`: the box between the rounded minimum
/// and maximum corners.
pub(crate) fn rectangle_to_board(c: &[f64], transform: &CoordinateTransform) -> IntBox {
    let lower_left = transform
        .dsn_to_board_point(c[0].min(c[2]), c[1].min(c[3]))
        .round();
    let upper_right = transform
        .dsn_to_board_point(c[0].max(c[2]), c[1].max(c[3]))
        .round();
    IntBox::from_points(lower_left, upper_right)
}

fn polygon_path_to_board(corners: &[FloatPoint], offset: f64) -> Shape {
    if corners.len() <= 2 {
        return Shape::from(FloatPoint::bounding_octagon_of(corners).enlarge(offset));
    }
    let rounded: Vec<Point> = corners.iter().map(|p| Point::Int(p.round())).collect();
    let result = Shape::from(PolygonShape::new(&rounded));
    if offset > 0.0 {
        return Shape::from(result.bounding_tile().enlarge(offset));
    }
    result
}

/// An area read from a DSN file: the border, then the holes.
///
/// Port of `Shape.ReadAreaScopeResult`.
#[derive(Clone, Debug)]
pub(crate) struct DsnArea {
    pub name: Option<String>,
    pub shapes: Vec<DsnShape>,
    pub clearance_class: Option<String>,
}

impl DsnArea {
    /// Resolves an area scope. `None` if the border shape can't be read,
    /// as in Java. Holes that can't be read are dropped (Java keeps them as
    /// `null` and fails when the area is used).
    pub fn resolve(
        area: &ast::Area,
        structure: Option<&DsnLayerStructure>,
        skip_windows: bool,
    ) -> Option<DsnArea> {
        let border = DsnShape::resolve(&area.shape, structure)?;
        let mut shapes = vec![border];
        if !skip_windows {
            shapes.extend(
                area.windows
                    .iter()
                    .filter_map(|w| DsnShape::resolve(w, structure)),
            );
        }
        Some(DsnArea {
            name: area.name.clone().filter(|n| !n.is_empty()),
            shapes,
            clearance_class: area.clearance_class.clone(),
        })
    }

    /// The layer of the border shape.
    pub fn layer(&self) -> &DsnLayer {
        &self.shapes[0].layer
    }
}

fn to_area(shapes: &[DsnShape], transform: impl Fn(&DsnShape) -> Option<Shape>) -> Option<Area> {
    let (border, holes) = shapes.split_first()?;
    let border = transform(border)?;
    if holes.is_empty() {
        return Some(Area::Shape(border));
    }
    let Shape::Polyline(border) = border else {
        return None;
    };
    let mut hole_shapes: Vec<PolylineShape> = Vec::with_capacity(holes.len());
    for hole in holes {
        match transform(hole)? {
            Shape::Polyline(p) => hole_shapes.push(p),
            Shape::Circle(_) => return None,
        }
    }
    Some(Area::PolylineArea(PolylineArea::new(border, hole_shapes)))
}

/// Java `Shape.transformAreaToBoard`: the first shape is the border, the
/// others are holes. `None` where Java returns `null`.
pub(crate) fn transform_area_to_board(
    shapes: &[DsnShape],
    transform: &CoordinateTransform,
) -> Option<Area> {
    to_area(shapes, |s| s.transform_to_board(transform))
}

/// Java `Shape.transformAreaToBoardRel`.
pub(crate) fn transform_area_to_board_rel(
    shapes: &[DsnShape],
    transform: &CoordinateTransform,
) -> Option<Area> {
    to_area(shapes, |s| s.transform_to_board_rel(transform))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(kind: DsnShapeKind, width: f64, coor: &[f64]) -> DsnShape {
        DsnShape {
            layer: super::super::layer::DsnLayer::pcb(),
            kind,
            width,
            coor: coor.to_vec(),
        }
    }

    /// N06 `dsn_shape_bbox`: `true` keeps Java's three box bugs, `false` fixes them.
    #[test]
    fn bounding_box_switch_both_behaviours() {
        let rect = shape(DsnShapeKind::Rectangle, 0.0, &[10.0, 20.0, 0.0, 5.0]);
        assert_eq!(rect.bounding_box_with(true), Some([10.0, 20.0, 0.0, 5.0]));
        assert_eq!(rect.bounding_box_with(false), Some([0.0, 5.0, 10.0, 20.0]));
        // A circle of diameter 4 at (10, 20).
        let circle = shape(DsnShapeKind::Circle, 0.0, &[4.0, 10.0, 20.0]);
        assert_eq!(
            circle.bounding_box_with(true),
            Some([6.0, 16.0, 14.0, 24.0])
        );
        assert_eq!(
            circle.bounding_box_with(false),
            Some([8.0, 18.0, 12.0, 22.0])
        );
        // A path of width 2 from (0, 0) to (10, 0) to (5, 0): Java adds the half
        // width to max x at every x step.
        let path = shape(
            DsnShapeKind::PolygonPath,
            2.0,
            &[0.0, 0.0, 10.0, 0.0, 5.0, 0.0],
        );
        assert_eq!(path.bounding_box_with(true), Some([-1.0, -1.0, 12.0, 1.0]));
        assert_eq!(path.bounding_box_with(false), Some([-1.0, -1.0, 11.0, 1.0]));
    }
}
