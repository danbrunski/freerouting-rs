//! A per-net view of clearance-inflated obstacles in a window (B09.2).
//!
//! For a grid router's obstacle raster and for widest-path feasibility
//! (Q03.1): the shapes a trace centreline of one net must stay out of, on
//! one layer, inside a window. KiCadRoutingTools gets the same from its grid
//! (`obstacle_cache.precompute_net_obstacles`,
//! `obstacle_map.build_base_obstacle_map`: C-space inflation in Python); on
//! the shape-based board it is a query of the autoroute search tree.
//!
//! [`RoutingBoard::net_obstacles`]:
//! - takes the search tree compensated for the net's trace clearance class
//!   (`get_autoroute_tree`; a missing net falls back to class 1, as
//!   `AutorouteControl.initNet` does). The tree is created if missing,
//!   exactly as the router creates it; no other tree is touched;
//! - queries that tree on `layer` with the window enlarged by `half_width`,
//!   so every inflated shape that reaches the window is found;
//! - keeps the items that are trace obstacles for the net
//!   (`is_trace_obstacle`, so the board's area policy, B09.4, applies), and
//!   the via keepouts of other nets, flagged
//!   [`is_via_only`](ObstacleShape::is_via_only);
//! - enlarges each kept tree shape by `half_width`.
//!
//! A compensated tree shape is the item's shape enlarged by its clearance to
//! the class minus the class's own compensation (half its clearance to
//! itself, rounded up). So `half_width` is Java's compensated trace half
//! width: the trace half width plus that compensation
//! (`AutorouteControl.compensatedTraceHalfWidth`); then a centreline point
//! outside every shape keeps the trace at its clearance.
//!
//! The order is the item order (highest id first), then the shape index, so
//! no hash order reaches the result (D2). The tree query counts work units
//! (B02.7) like every tree query.

use freerouting_geometry::{IntBox, TileShape};

use crate::board::RoutingBoard;
use crate::board::items::{AreaKind, Item, ItemId, ItemKind};
use crate::board::search::TreeEntry;

/// One inflated obstacle shape of [`RoutingBoard::net_obstacles`].
#[derive(Clone, Debug)]
pub struct ObstacleShape {
    /// The item the shape belongs to.
    pub item: ItemId,
    /// The index of the shape among the item's tree shapes.
    pub shape_index: usize,
    /// The compensated tree shape enlarged by the half width.
    pub shape: TileShape,
    /// True for a via keepout: it blocks vias of the net, not traces.
    pub is_via_only: bool,
}

impl RoutingBoard {
    /// The obstacles for the traces (and vias) of net `net` on `layer` that
    /// reach `window`, inflated by `half_width` (module docs).
    pub fn net_obstacles(
        &mut self,
        net: i32,
        layer: i32,
        window: IntBox,
        half_width: i32,
    ) -> Vec<ObstacleShape> {
        let class = self.net_trace_clearance_class(net);
        self.autoroute_search_tree(class);
        let tree = self
            .search_trees
            .tree_by_class(class)
            .expect("get_autoroute_tree created the tree");
        let query = TileShape::IntBox(window.offset(f64::from(half_width)));
        let mut entries: Vec<TreeEntry> = Vec::new();
        tree.overlapping_tree_entries(&query, layer, &[], &self.items, &mut entries);
        entries.sort_by(|a, b| (a.item, a.shape_index).cmp(&(b.item, b.shape_index)));
        entries.dedup();
        let ctx = self.shape_context();
        let mut result = Vec::with_capacity(entries.len());
        for entry in entries {
            let item = self.item_ref(entry.item);
            let Some(is_via_only) = self.obstacle_kind(item, net) else {
                continue;
            };
            let Some(shape) = tree.tree_shape(item, entry.shape_index, &ctx) else {
                continue;
            };
            result.push(ObstacleShape {
                item: entry.item,
                shape_index: entry.shape_index,
                shape: shape.enlarge(f64::from(half_width)),
                is_via_only,
            });
        }
        result
    }

    /// The trace clearance class of net `net`'s class; 1 for a missing net
    /// (`AutorouteControl.java:220-222`).
    fn net_trace_clearance_class(&self, net: i32) -> i32 {
        match self.rules.nets.get(net) {
            Some(n) => self
                .rules
                .net_classes
                .get(n.net_class())
                .trace_clearance_class(),
            None => 1,
        }
    }

    /// `Some(false)` for a trace obstacle of `net`, `Some(true)` for a via
    /// keepout of another net, `None` for neither.
    fn obstacle_kind(&self, item: &Item, net: i32) -> Option<bool> {
        if self.is_trace_obstacle(item, net) {
            return Some(false);
        }
        match &item.kind {
            ItemKind::Area(area) if area.kind == AreaKind::ViaObstacle => {
                (!item.contains_net(net)).then_some(true)
            }
            _ => None,
        }
    }
}
