//! Item query helpers on the router path (B04.7).
//!
//! Ports of `DrillItem.minWidth`, `getTileShapeOnLayer` and
//! `getTraceConnectionShape`, `PolylineTrace.getTraceConnectionShape`,
//! `ConductionArea.getTraceConnectionShape`, `Pin.drillAllowed`,
//! `Item.isConnectedOnLayer`, `Trace.getCompensatedHalfWidth` and
//! `Trace.nearestEndPoint` (Java at `a773a70ec`).
//!
//! Java caches `minWidth` in the item; the port computes it on demand from
//! the same shapes, which gives the same value.

use freerouting_geometry::{LineSegment, Point, TileShape};

use crate::board::BasicBoard;
use crate::board::items::{Item, ItemId, ItemKind, point_tile};
use crate::board::search::ShapeSearchTree;

impl Item {
    /// Java `Pin.drillAllowed`: vias may go through the pads of a pin on a
    /// single layer (an SMD pin). False for items that are not pins.
    pub fn pin_drill_allowed(&self) -> bool {
        self.is_pin() && self.first_layer() == self.last_layer()
    }
}

impl BasicBoard {
    /// Java `DrillItem.minWidth`: the smallest width or height of the
    /// bounding boxes of the pad shapes on the signal layers of drill item
    /// `id`; `Integer.MAX_VALUE` if it has none. `None` if `id` is not a pin
    /// or via.
    pub fn min_width(&self, id: ItemId) -> Option<f64> {
        let drill = self.item(id)?.drill()?;
        let mut min_width = f64::from(i32::MAX);
        for layer in drill.first_layer..=drill.last_layer {
            let is_signal = self
                .layer_structure
                .layers
                .get(layer as usize)
                .is_some_and(|l| l.is_signal);
            if !is_signal {
                continue;
            }
            if let Some(shape) = drill.shape_on_layer(layer) {
                let bounding_box = shape.bounding_box();
                min_width = min_width.min(f64::from(bounding_box.width()));
                min_width = min_width.min(f64::from(bounding_box.height()));
            }
        }
        Some(min_width)
    }

    /// Java `DrillItem.getTileShapeOnLayer`: the default tree's tile shape
    /// of drill item `id` on `layer`; `None` outside its layers or for other
    /// items.
    pub fn tile_shape_on_layer(&self, id: ItemId, layer: i32) -> Option<TileShape> {
        let item = self.item(id)?;
        let drill = item.drill()?;
        if layer < drill.first_layer || layer > drill.last_layer {
            return None;
        }
        self.tile_shape(item, (layer - drill.first_layer) as usize)
    }

    /// Java `getTraceConnectionShape(tree, index)`: where a trace may connect
    /// to item `id`. A pin or via gives the point at its centre; a trace the
    /// simplified simplex of its segment `index`; a conduction area its tree
    /// shape `index` in `tree`. `None` for an index out of range or another
    /// item.
    pub fn trace_connection_shape(
        &self,
        tree: &ShapeSearchTree,
        id: ItemId,
        index: usize,
    ) -> Option<TileShape> {
        let item = self.item(id)?;
        match &item.kind {
            ItemKind::Pin(_) | ItemKind::Via(_) => {
                let center = item.drill()?.center;
                Some(point_tile(&Point::Int(center)))
            }
            ItemKind::Trace(trace) => {
                if index >= trace.tile_shape_count() {
                    return None;
                }
                let segment = LineSegment::from_polyline(&trace.polyline, index + 1);
                Some(segment.to_simplex().simplify())
            }
            ItemKind::Area(_) if item.is_conduction_area() => {
                let ctx = self.shape_context();
                if index >= tree.tree_shape_count(item, &ctx) {
                    return None;
                }
                tree.tree_shape(item, index, &ctx)
            }
            _ => None,
        }
    }

    /// Java `Item.isConnectedOnLayer`: item `id` touches another connectable
    /// item of its net on `layer`.
    pub fn is_connected_on_layer(&self, id: ItemId, layer: i32) -> bool {
        !self.all_contacts(id, Some(layer)).is_empty()
    }

    /// Java `Trace.getCompensatedHalfWidth(tree)`: the half width of trace
    /// `id` enlarged by `tree`'s clearance compensation for its class and
    /// layer.
    pub fn compensated_half_width(&self, tree: &ShapeSearchTree, id: ItemId) -> Option<i32> {
        let item = self.item(id)?;
        let trace = item.as_trace()?;
        let compensation = tree.clearance_compensation_value(
            &self.rules.clearance_matrix,
            item.clearance_class(),
            trace.layer,
        );
        Some(trace.half_width + compensation)
    }

    /// Java `Trace.nearestEndPoint`: the end corner of trace `id` nearer to
    /// `from` (the last corner on a tie).
    pub fn nearest_end_point(&self, id: ItemId, from: &Point) -> Option<Point> {
        let trace = self.item(id)?.as_trace()?;
        let first = trace.first_corner();
        let last = trace.last_corner();
        let from = from.to_float();
        let d1 = from.distance(first.to_float());
        let d2 = from.distance(last.to_float());
        Some(if d1 < d2 { first } else { last })
    }
}
