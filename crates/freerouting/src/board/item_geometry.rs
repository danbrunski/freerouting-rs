//! Drawable geometry of the board items (B08.1).
//!
//! [`BasicBoard::item_geometry`] flattens one item into [`ItemGeometry`]: the
//! shapes a renderer or the router's event stream draws, in board units, with
//! exact shapes kept. Java has no such class; its `BoardRenderer` reads the
//! items' shapes directly. The desktop app and the live redraw share this one
//! flattening (decision D7), so tessellation stays in the GUI and the engine
//! owns what an item looks like.
//!
//! What each item gives (every shape carries its board layer):
//! - pin and via: one shape per layer from the first to the last layer of the
//!   padstack (`DrillData::shape_on_layer`); a layer without a pad has none;
//! - trace: one [`GeometryShape::Polyline`] with its corners and half width;
//! - obstacle, keepout and conduction area: the area's border, as a polygon
//!   with the area's holes when it has any, else the plain shape;
//! - component outline: its area on the layer of the component's side;
//! - board outline: one shape per outline polygon, on the first layer.
//!
//! A tile shape that is an `IntBox` or `IntOctagon` stays one; any other
//! polygon is its approximated corners (`corner_approx_arr`), which is what
//! Java's renderer draws too. The bounding box of every shape is computed the
//! way the item's own `bounding_box` is, so the union of the shapes' bounding
//! boxes equals the item's (`tests/item_geometry.rs`).
//!
//! Differences in shape from the checklist: the shape list is a `Vec`
//! (the workspace has no `smallvec`), and a circle that is the border or a hole
//! of an area *with holes* is flattened to a 64-corner ring, because a
//! [`GeometryShape::Polygon`] holds rings only. A plain circular area stays a
//! [`GeometryShape::Circle`].

use freerouting_geometry::jmath::d2i;
use freerouting_geometry::{
    Area, FloatPoint, IntBox, IntOctagon, IntPoint, PolylineShapeOps, Shape, TileShape,
};

use crate::board::basic_board::BasicBoard;
use crate::board::items::{BoardItemType, Item, ItemId, ItemKind};
use crate::board::structure::FixedState;

/// The number of corners of a ring made from a circle inside an area with holes.
const CIRCLE_RING_CORNERS: usize = 64;

/// One exact shape of an item, in board units.
#[derive(Clone, Debug, PartialEq)]
pub enum GeometryShape {
    /// A polygon: the approximated corners of its border in order, and the
    /// rings of its holes.
    Polygon {
        outer: Vec<FloatPoint>,
        holes: Vec<Vec<FloatPoint>>,
    },
    /// The corners of a trace's polyline, drawn with width `2 * half_width`.
    Polyline {
        corners: Vec<FloatPoint>,
        half_width: i32,
    },
    Circle {
        center: IntPoint,
        radius: i32,
    },
    Octagon(IntOctagon),
    Box(IntBox),
}

impl GeometryShape {
    /// The bounding box, computed as the item types compute theirs: the
    /// floor/ceiling box of the approximated corners (`Polyline::bounding_box`),
    /// enlarged by the half width for a trace.
    pub fn bounding_box(&self) -> IntBox {
        match self {
            GeometryShape::Polygon { outer, .. } => corner_box(outer),
            GeometryShape::Polyline {
                corners,
                half_width,
            } => corner_box(corners).offset(f64::from(*half_width)),
            GeometryShape::Circle { center, radius } => IntBox::new(
                center.x.wrapping_sub(*radius),
                center.y.wrapping_sub(*radius),
                center.x.wrapping_add(*radius),
                center.y.wrapping_add(*radius),
            ),
            GeometryShape::Octagon(octagon) => octagon.bounding_box(),
            GeometryShape::Box(b) => *b,
        }
    }
}

/// A shape together with its board layer.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerShape {
    pub layer: i32,
    pub shape: GeometryShape,
}

/// The drawable data of one item.
#[derive(Clone, Debug, PartialEq)]
pub struct ItemGeometry {
    pub id: ItemId,
    pub kind: BoardItemType,
    pub first_layer: i32,
    pub last_layer: i32,
    /// The net numbers of the item.
    pub nets: Vec<i32>,
    pub fixed_state: FixedState,
    /// The component id (0 or less: none).
    pub component: i32,
    /// The shapes, in layer order for drill items.
    pub shapes: Vec<LayerShape>,
}

impl ItemGeometry {
    /// The union of the bounding boxes of the shapes ([`IntBox::EMPTY`]
    /// without shapes).
    ///
    /// This is the true union. The bounding box of a drill item or of the
    /// board outline starts its union from [`IntBox::EMPTY`], whose corners
    /// are `CRIT_INT` (2^25): on a board with coordinates beyond that, which
    /// are possible at 1 000 000 units per mm, Java's (and the port's) item
    /// box is clamped to `CRIT_INT`, and the two differ.
    pub fn bounding_box(&self) -> IntBox {
        let mut shapes = self.shapes.iter().map(|s| s.shape.bounding_box());
        match shapes.next() {
            None => IntBox::EMPTY,
            Some(first) => shapes.fold(first, |acc, b| acc.union_box(&b)),
        }
    }
}

impl BasicBoard {
    /// The drawable geometry of item `id`; `None` if it is not on the board.
    pub fn item_geometry(&self, id: ItemId) -> Option<ItemGeometry> {
        let item = self.item(id)?;
        Some(geometry_of(item))
    }
}

fn geometry_of(item: &Item) -> ItemGeometry {
    let mut shapes = Vec::new();
    match &item.kind {
        ItemKind::Pin(pin) => drill_shapes(&pin.drill, &mut shapes),
        ItemKind::Via(via) => drill_shapes(&via.drill, &mut shapes),
        ItemKind::Trace(trace) => shapes.push(LayerShape {
            layer: trace.layer,
            shape: GeometryShape::Polyline {
                corners: trace.polyline.corner_approx_arr(),
                half_width: trace.half_width,
            },
        }),
        ItemKind::Area(area) => shapes.push(LayerShape {
            layer: area.layer(),
            shape: area_shape(area.area()),
        }),
        ItemKind::ComponentOutline(outline) => shapes.push(LayerShape {
            layer: outline.layer(),
            shape: area_shape(outline.area()),
        }),
        ItemKind::BoardOutline(outline) => {
            for shape in outline.shapes() {
                shapes.push(LayerShape {
                    layer: 0,
                    shape: plain_shape(&Shape::Polyline(shape.clone())),
                });
            }
        }
    }
    ItemGeometry {
        id: item.id(),
        kind: item.board_item_type(),
        first_layer: item.first_layer(),
        last_layer: item.last_layer(),
        nets: item.net_numbers().to_vec(),
        fixed_state: item.fixed_state(),
        component: item.component_id(),
        shapes,
    }
}

fn drill_shapes(drill: &crate::board::items::DrillData, out: &mut Vec<LayerShape>) {
    for layer in drill.first_layer..=drill.last_layer {
        if let Some(shape) = drill.shape_on_layer(layer) {
            out.push(LayerShape {
                layer,
                shape: plain_shape(shape),
            });
        }
    }
}

/// An area: the plain shape without holes, else a polygon with the holes.
fn area_shape(area: &Area) -> GeometryShape {
    let holes = area.get_holes();
    let border = area.get_border();
    if holes.is_empty() {
        return plain_shape(&border);
    }
    GeometryShape::Polygon {
        outer: ring(&border),
        holes: holes.iter().map(ring).collect(),
    }
}

/// A shape without holes, exact where it is a box, octagon or circle.
fn plain_shape(shape: &Shape) -> GeometryShape {
    match shape {
        Shape::Circle(c) => GeometryShape::Circle {
            center: c.center,
            radius: c.radius,
        },
        Shape::Polyline(freerouting_geometry::PolylineShape::Tile(TileShape::IntBox(b))) => {
            GeometryShape::Box(*b)
        }
        Shape::Polyline(freerouting_geometry::PolylineShape::Tile(TileShape::IntOctagon(o))) => {
            GeometryShape::Octagon(*o)
        }
        other => GeometryShape::Polygon {
            outer: other.corner_approx_arr(),
            holes: Vec::new(),
        },
    }
}

/// The corner ring of a contour of an area with holes.
fn ring(shape: &Shape) -> Vec<FloatPoint> {
    match shape {
        Shape::Circle(c) => {
            let (cx, cy) = (f64::from(c.center.x), f64::from(c.center.y));
            let r = f64::from(c.radius);
            (0..CIRCLE_RING_CORNERS)
                .map(|i| {
                    let angle = std::f64::consts::TAU * i as f64 / CIRCLE_RING_CORNERS as f64;
                    FloatPoint::new(cx + r * angle.cos(), cy + r * angle.sin())
                })
                .collect()
        }
        other => other.corner_approx_arr(),
    }
}

/// `Polyline::bounding_box_range` on plain corners: floor of the minima,
/// ceiling of the maxima.
fn corner_box(corners: &[FloatPoint]) -> IntBox {
    if corners.is_empty() {
        return IntBox::EMPTY;
    }
    let mut llx = f64::from(i32::MAX);
    let mut lly = llx;
    let mut urx = f64::from(i32::MIN);
    let mut ury = urx;
    for c in corners {
        llx = llx.min(c.x);
        lly = lly.min(c.y);
        urx = urx.max(c.x);
        ury = ury.max(c.y);
    }
    IntBox::from_points(
        IntPoint::new(d2i(llx.floor()), d2i(lly.floor())),
        IntPoint::new(d2i(urx.ceil()), d2i(ury.ceil())),
    )
}
