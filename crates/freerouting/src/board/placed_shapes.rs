//! K15.2: the board's `router.java_quirks.placed_shape_convex` switch.
//!
//! A component's pins, areas (keepouts, auxiliary pad shapes) and outlines get
//! their board shapes from [`PlacementTransform::shape_with`] and
//! [`PlacementTransform::area_with`], with the switch of
//! [`BasicBoard::java_quirks`] as `java_corners`: `true` (unset, both profiles) is
//! Java's shape, `false` the convex hull of the rounded corners (see
//! `freerouting_geometry::placement_transform`). The reader builds the board with its
//! final switches before it inserts an item, so the shapes are placed under them
//! from the start. When the switch changes on a board that has items,
//! [`BasicBoard::set_java_quirks`] places every component item again and reinserts,
//! in item-list order, exactly those whose shapes change, as it does for the
//! split switches (`board/split_switches.rs`); with the rounded corners convex
//! nothing changes.
//!
//! An auxiliary pad shape is placed twice, as in Java (`Network.java:1071`, then
//! `ObstacleArea.getArea` with the component's transform); the second placement
//! follows the switch on a change, the first keeps the switch the shape was read
//! with.
//!
//! [`PlacementTransform::shape_with`]: freerouting_geometry::PlacementTransform::shape_with
//! [`PlacementTransform::area_with`]: freerouting_geometry::PlacementTransform::area_with

use freerouting_geometry::{Area, PolylineShape, Shape};

use crate::board::BasicBoard;
use crate::board::items::{Item, ItemId, ItemKind};
use crate::board::split_switches::same_tile;

impl BasicBoard {
    /// Places the component items again under the board's switches and
    /// reinserts the ones whose shapes changed (see the module docs).
    pub(crate) fn replace_placed_shapes(&mut self) {
        let ids: Vec<ItemId> = self.items.keys().copied().collect();
        for id in ids {
            let Some(item) = self.items.get(&id) else {
                continue;
            };
            let placed = item.component_id > 0
                && matches!(
                    item.kind,
                    ItemKind::Pin(_) | ItemKind::Area(_) | ItemKind::ComponentOutline(_)
                );
            if !placed {
                continue;
            }
            let mut fresh = item.clone();
            self.refresh_derived_data(&mut fresh);
            if same_placed_shapes(item, &fresh) {
                continue;
            }
            let on_the_board = item.on_the_board;
            if on_the_board {
                self.search_trees.remove(id);
            }
            if let Some(slot) = self.items.get_mut(&id) {
                *slot = fresh;
            }
            if on_the_board {
                self.insert_into_trees(id);
                // Its shapes changed, so its nets' incomplete counts are stale.
                self.note_changed(id);
            }
        }
    }
}

/// True if `a` and `b` (the same item, placed twice) have the same shapes.
fn same_placed_shapes(a: &Item, b: &Item) -> bool {
    match (&a.kind, &b.kind) {
        (ItemKind::Pin(x), ItemKind::Pin(y)) => {
            x.drill.center == y.drill.center
                && x.drill.shapes.len() == y.drill.shapes.len()
                && x.drill
                    .shapes
                    .iter()
                    .zip(&y.drill.shapes)
                    .all(|(s, t)| match (s, t) {
                        (None, None) => true,
                        (Some(s), Some(t)) => same_shape(s, t),
                        _ => false,
                    })
        }
        (ItemKind::Area(x), ItemKind::Area(y)) => same_area(x.area(), y.area()),
        (ItemKind::ComponentOutline(x), ItemKind::ComponentOutline(y)) => {
            same_area(x.area(), y.area())
        }
        _ => false,
    }
}

fn same_area(a: &Area, b: &Area) -> bool {
    let (holes_a, holes_b) = (a.get_holes(), b.get_holes());
    same_shape(&a.get_border(), &b.get_border())
        && holes_a.len() == holes_b.len()
        && holes_a.iter().zip(&holes_b).all(|(s, t)| same_shape(s, t))
}

fn same_shape(a: &Shape, b: &Shape) -> bool {
    match (a, b) {
        (Shape::Circle(x), Shape::Circle(y)) => x.center == y.center && x.radius == y.radius,
        (Shape::Polyline(PolylineShape::Tile(x)), Shape::Polyline(PolylineShape::Tile(y))) => {
            same_tile(x, y)
        }
        (
            Shape::Polyline(PolylineShape::Polygon(x)),
            Shape::Polyline(PolylineShape::Polygon(y)),
        ) => x.corners() == y.corners(),
        _ => false,
    }
}
