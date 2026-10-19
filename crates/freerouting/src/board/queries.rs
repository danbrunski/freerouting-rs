//! Read-only clearance checks and picks the router, the shover and
//! pull-tight call before they change the board (B04.3–B04.6).
//!
//! Ports of `BasicBoard.checkShape`, `checkTraceShape` and
//! `checkPolylineTrace`, `Trace.touchingPinsAtEndCorners`, and
//! `RoutingBoardSearchFacade.checkTraceSegment` (both forms),
//! `checkMoveItem`, `checkChangeNet` and `pickNearestRoutingItem` (Java at
//! `a773a70ec`).
//!
//! Orders that decide results:
//! - the checks return on the first obstacle in the tree query's order; the
//!   boolean does not depend on that order;
//! - `check_trace_segment` keeps the minimum over the obstacles in query
//!   order and evaluates `max(0, from·(to, nearest) / length - shorten - 1)`
//!   in Java's operation order (D6);
//! - `pick_nearest_routing_item` walks the overlapping items highest id
//!   first, as Java's `TreeSet<Item>` does, because its tie-breaks depend on
//!   the current best.
//!
//! [`BasicBoard::check_polyline_trace`] takes an item id before it computes
//! anything, as Java's temporary `PolylineTrace` does (B01.2's table).
//!
//! `check_trace_shape` runs in every tightener step. Its own loop does not
//! allocate: the compensated query fills a thread-local scratch vector. The
//! tree queries it calls (`board/search`, `datastructures/shape_tree.rs`)
//! still allocate their stacks and result vectors.

use std::cell::RefCell;
use std::collections::BTreeSet;

use freerouting_geometry::{
    Area, FloatPoint, IntOctagon, LineSegment, Point, Polyline, PolylineShapeOps, Shape, TileShape,
    TileShapeOps, Vector,
};

use crate::board::items::{Item, ItemId, ItemKind, PolylineTrace, point_tile};
use crate::board::search::TreeEntry;
use crate::board::structure::FixedState;
use crate::board::{BasicBoard, RoutingBoard};

thread_local! {
    /// The reused result vector of `check_trace_shape`'s plain tree query.
    static TRACE_SHAPE_ENTRIES: RefCell<Vec<TreeEntry>> = const { RefCell::new(Vec::new()) };
}

/// Java `Math.max(0.0, value)`: NaN stays NaN and `-0.0` becomes `0.0`.
fn java_max_zero(value: f64) -> f64 {
    if value > 0.0 || value.is_nan() {
        value
    } else {
        0.0
    }
}

impl BasicBoard {
    /// Java's five-argument `ShapeSearchTree.overlappingTreeEntriesWithClearance` on the
    /// default tree, which `overlappingObjectsWithClearance` and
    /// `overlappingItemsWithClearance` call: it searches with the clearance even when the tree
    /// is clearance compensated
    /// ([`clearance_search_entries`](crate::board::search::ShapeSearchTree::clearance_search_entries)).
    pub(crate) fn clearance_entries(
        &self,
        shape: &TileShape,
        layer: i32,
        ignore_nets: &[i32],
        class: i32,
    ) -> Vec<TreeEntry> {
        self.search_trees.default_tree().clearance_search_entries(
            shape,
            layer,
            ignore_nets,
            class,
            &self.rules.clearance_matrix,
            &self.items,
        )
    }

    /// Java `BasicBoard.overlappingItemsWithClearance`, with the clearance search of
    /// [`clearance_entries`](Self::clearance_entries) also in a compensated tree.
    pub fn overlapping_items_with_clearance_java(
        &self,
        shape: &TileShape,
        layer: i32,
        ignore_nets: &[i32],
        class: i32,
    ) -> BTreeSet<ItemId> {
        self.clearance_entries(shape, layer, ignore_nets, class)
            .into_iter()
            .map(|e| e.item)
            .collect()
    }

    /// Java `BasicBoard.checkShape`: true if an object with shape `area`,
    /// nets `nets` and clearance class `class` fits on `layer`.
    ///
    /// Each convex tile must lie in the board box and may not come closer
    /// than the clearance to an object that is an obstacle for every net in
    /// `nets`. An area that cannot be split into convex pieces (Java throws)
    /// is reported as not fitting.
    pub fn check_shape(&self, area: &Area, layer: i32, nets: &[i32], class: i32) -> bool {
        // PARITY(quirk, java_quirks.octagon_box_cutout_branch3): the board's split
        // switches (all four); Java's split when they are `true`.
        let Some(tiles) = area.split_to_convex_with(None, self.java_quirks.split()) else {
            return false;
        };
        for tile in &tiles {
            if !tile.is_contained_in(&self.bounding_box) {
                return false;
            }
            for entry in self.clearance_entries(tile, layer, nets, class) {
                let item = self.item_ref(entry.item);
                if nets.iter().all(|n| item.is_obstacle_for_net(*n)) {
                    return false;
                }
            }
        }
        true
    }

    /// Java `BasicBoard.checkTraceShape`: true if a trace segment with shape
    /// `shape` fits on `layer` without clearance violation.
    ///
    /// With `contact_pins`, every pin not in it is an obstacle, even of the
    /// own net (no acid traps), and a foreign trace at a contact tie pin is
    /// not an obstacle where the pin's shape contains the overlap.
    pub fn check_trace_shape(
        &self,
        shape: &TileShape,
        layer: i32,
        nets: &[i32],
        class: i32,
        contact_pins: Option<&BTreeSet<ItemId>>,
    ) -> bool {
        if !shape.is_contained_in(&self.bounding_box) {
            return false;
        }
        let tree = self.search_trees.default_tree();
        if tree.is_clearance_compensation_used() {
            TRACE_SHAPE_ENTRIES.with(|scratch| {
                let mut entries = scratch.borrow_mut();
                entries.clear();
                tree.overlapping_tree_entries(shape, layer, &[], &self.items, &mut entries);
                self.no_trace_obstacle(&entries, shape, layer, nets, contact_pins)
            })
        } else {
            let entries = tree.overlapping_tree_entries_with_clearance(
                shape,
                layer,
                &[],
                class,
                &self.rules.clearance_matrix,
                &self.items,
            );
            self.no_trace_obstacle(&entries, shape, layer, nets, contact_pins)
        }
    }

    /// The loop of `checkTraceShape` over the tree entries, in their order.
    fn no_trace_obstacle(
        &self,
        entries: &[TreeEntry],
        shape: &TileShape,
        layer: i32,
        nets: &[i32],
        contact_pins: Option<&BTreeSet<ItemId>>,
    ) -> bool {
        for entry in entries {
            let item = self.item_ref(entry.item);
            if let Some(pins) = contact_pins {
                if pins.contains(&entry.item) {
                    continue;
                }
                if item.is_pin() {
                    return false;
                }
            }
            let mut is_obstacle = nets.iter().all(|n| self.is_trace_obstacle(item, *n));
            if is_obstacle && item.is_trace() {
                if let Some(pins) = contact_pins {
                    is_obstacle = !self.inside_contact_tie_pin(item, entry, shape, layer, pins);
                }
            }
            if is_obstacle {
                return false;
            }
        }
        true
    }

    /// True if a contact tie pin sharing a net with trace `item` contains the
    /// overlap of `shape` and the trace's tile shape on `layer` (the pins in
    /// Java's `TreeSet` order).
    fn inside_contact_tie_pin(
        &self,
        item: &Item,
        entry: &TreeEntry,
        shape: &TileShape,
        layer: i32,
        pins: &BTreeSet<ItemId>,
    ) -> bool {
        let mut intersection: Option<TileShape> = None;
        for pin_id in pins {
            let pin = self.item_ref(*pin_id);
            if pin.net_count() <= 1 || !pin.shares_net(item) {
                continue;
            }
            let overlap = intersection.get_or_insert_with(|| {
                let obstacle = self
                    .tile_shape(item, entry.shape_index)
                    .expect("a tree entry has a tile shape");
                shape.intersection(&obstacle)
            });
            let Some(pin_shape) = self.tile_shape_on_layer(*pin_id, layer) else {
                continue;
            };
            if pin_shape.contains_approx(overlap) {
                return true;
            }
        }
        false
    }

    /// Java `Trace.touchingPinsAtEndCorners` of trace `id`: the pins sharing
    /// a net with it that come closer than the clearance to its first or last
    /// corner, enlarged by the half width.
    pub fn touching_pins_at_end_corners(&self, id: ItemId) -> BTreeSet<ItemId> {
        let Some(item) = self.item(id) else {
            return BTreeSet::new();
        };
        let Some(trace) = item.as_trace() else {
            return BTreeSet::new();
        };
        self.touching_pins_at_corners(
            [trace.first_corner(), trace.last_corner()],
            trace.layer,
            trace.half_width,
            item,
        )
    }

    /// [`touching_pins_at_end_corners`](Self::touching_pins_at_end_corners)
    /// of a trace that is not on the board. Empty for a polyline with fewer
    /// than three lines.
    pub fn touching_pins_at_polyline_end_corners(
        &self,
        polyline: &Polyline,
        layer: i32,
        half_width: i32,
        nets: &[i32],
        class: i32,
    ) -> BTreeSet<ItemId> {
        if polyline.lines().len() < 3 {
            return BTreeSet::new();
        }
        let trace = self.unsaved_trace(ItemId(0), polyline, layer, half_width, nets, class);
        let corners = [
            polyline.corner(0),
            polyline.corner(polyline.lines().len() - 2),
        ];
        self.touching_pins_at_corners(corners, layer, half_width, &trace)
    }

    fn touching_pins_at_corners(
        &self,
        corners: [Point; 2],
        layer: i32,
        half_width: i32,
        trace: &Item,
    ) -> BTreeSet<ItemId> {
        let mut result = BTreeSet::new();
        for corner in &corners {
            let octagon = corner.surrounding_octagon().enlarge(f64::from(half_width));
            let overlaps = self.overlapping_items_with_clearance_java(
                &TileShape::IntOctagon(octagon),
                layer,
                &[],
                trace.clearance_class(),
            );
            for other in overlaps {
                let other_item = self.item_ref(other);
                if other_item.is_pin() && other_item.shares_net(trace) {
                    result.insert(other);
                }
            }
        }
        result
    }

    /// An unfixed trace item that is not inserted (Java's temporary
    /// `PolylineTrace`); its layer is clamped to the board's layers as the
    /// Java constructor does.
    fn unsaved_trace(
        &self,
        id: ItemId,
        polyline: &Polyline,
        layer: i32,
        half_width: i32,
        nets: &[i32],
        class: i32,
    ) -> Item {
        let layer = layer.max(0).min(self.layer_count() as i32 - 1);
        Item::new(
            id,
            nets.to_vec(),
            class,
            0,
            FixedState::Unfixed,
            ItemKind::Trace(PolylineTrace {
                polyline: polyline.clone(),
                layer,
                half_width,
            }),
        )
    }

    /// Java `BasicBoard.checkPolylineTrace`: true if a trace along `polyline`
    /// fits without clearance violation.
    ///
    /// Takes an item id first, as Java's temporary trace does, then checks
    /// each tile shape (the default tree's shapes of the temporary trace)
    /// with [`check_trace_shape`](Self::check_trace_shape) and the trace's
    /// touching end pins, in index order, stopping at the first failure.
    pub fn check_polyline_trace(
        &mut self,
        polyline: &Polyline,
        layer: i32,
        half_width: i32,
        nets: &[i32],
        class: i32,
    ) -> bool {
        let id = self.burn_item_id();
        let trace = self.unsaved_trace(id, polyline, layer, half_width, nets, class);
        let layer = trace.as_trace().expect("a trace").layer;
        let contact_pins = if polyline.lines().len() < 3 {
            BTreeSet::new()
        } else {
            self.touching_pins_at_corners(
                [
                    polyline.corner(0),
                    polyline.corner(polyline.lines().len() - 2),
                ],
                layer,
                half_width,
                &trace,
            )
        };
        let shapes = self
            .search_trees
            .default_tree()
            .calculate_tree_shapes(&trace, &self.shape_context());
        shapes.iter().all(|shape| {
            shape
                .as_ref()
                .is_some_and(|s| self.check_trace_shape(s, layer, nets, class, Some(&contact_pins)))
        })
    }
}

impl RoutingBoard {
    /// Java `RoutingBoardSearchFacade.checkTraceSegment(Point, Point, ...)`:
    /// [`check_trace_segment_line`](Self::check_trace_segment_line) of the
    /// segment from `from` to `to`; 0 if the points are equal.
    #[allow(clippy::too_many_arguments)]
    pub fn check_trace_segment(
        &self,
        from: &Point,
        to: &Point,
        layer: i32,
        nets: &[i32],
        half_width: i32,
        class: i32,
        only_not_shovable: bool,
    ) -> f64 {
        if from == to {
            return 0.0;
        }
        let polyline = Polyline::from_corners(from, to);
        let segment = LineSegment::from_polyline(&polyline, 1);
        self.check_trace_segment_line(&segment, layer, nets, half_width, class, only_not_shovable)
    }

    /// Java `RoutingBoardSearchFacade.checkTraceSegment(LineSegment, ...)`:
    /// how far a trace of half width `half_width` can run along `segment`
    /// from its start before it comes too close to an obstacle (that is not
    /// of `nets`); `Integer.MAX_VALUE` if nothing is in the way, 0 if the
    /// start is blocked or the segment is degenerate. With
    /// `only_not_shovable`, routable items that are not shove fixed are not
    /// obstacles.
    pub fn check_trace_segment_line(
        &self,
        segment: &LineSegment,
        layer: i32,
        nets: &[i32],
        half_width: i32,
        class: i32,
        only_not_shovable: bool,
    ) -> f64 {
        let check_polyline = segment.to_polyline();
        if check_polyline.lines().len() != 3 {
            return 0.0;
        }
        let Some(shape) = check_polyline.offset_shape(half_width, 0) else {
            return 0.0;
        };
        let from = segment.start_point_approx();
        let to = segment.end_point_approx();
        let line_length = to.distance(from);
        let mut ok_length = f64::from(i32::MAX);
        let board = self.basic();
        let tree = board.search_trees.default_tree();
        let matrix = &board.rules.clearance_matrix;
        let ctx = board.shape_context();
        let entries = tree.overlapping_tree_entries_with_clearance(
            &shape,
            layer,
            nets,
            class,
            matrix,
            &board.items,
        );
        for entry in entries {
            let obstacle = board.item_ref(entry.item);
            if only_not_shovable && obstacle.is_routable() && !board.is_shove_fixed(obstacle) {
                continue;
            }
            let Some(obstacle_shape) = tree.tree_shape(obstacle, entry.shape_index, &ctx) else {
                continue;
            };
            let (offset_shape, shorten) = if tree.is_clearance_compensation_used() {
                let compensation =
                    matrix.clearance_compensation_value(obstacle.clearance_class(), layer);
                (shape.clone(), f64::from(half_width + compensation))
            } else {
                let clearance = board.clearance_value(obstacle.clearance_class(), class, layer);
                (
                    shape.offset(f64::from(clearance)),
                    f64::from(half_width + clearance),
                )
            };
            let intersection = obstacle_shape.intersection(&offset_shape);
            if intersection.is_empty() {
                continue;
            }
            let Some(nearest) = intersection.nearest_point_approx(from) else {
                continue;
            };
            let projection = from.scalar_product(to, nearest) / line_length;
            let projection = java_max_zero(projection - shorten - 1.0);
            if projection < ok_length {
                ok_length = projection;
                if ok_length <= 0.0 {
                    return 0.0;
                }
            }
        }
        ok_length
    }

    /// Java `RoutingBoardSearchFacade.checkMoveItem`: true if item `id` can
    /// be moved by `vector` without clearance violation.
    ///
    /// Items of more than one net and traces with contacts can't be moved.
    /// The item is added to `ignore` (callers rely on that side effect), and
    /// the items in `ignore` are not obstacles.
    pub fn check_move_item(
        &self,
        id: ItemId,
        vector: &Vector,
        mut ignore: Option<&mut BTreeSet<ItemId>>,
    ) -> bool {
        let board = self.basic();
        let Some(item) = board.item(id) else {
            return false;
        };
        if item.net_count() > 1 {
            return false;
        }
        let contact_count = if item.is_connectable_kind() {
            board.all_contacts(id, None).len()
        } else {
            0
        };
        if item.is_trace() && contact_count > 0 {
            return false;
        }
        if let Some(set) = ignore.as_deref_mut() {
            set.insert(id);
        }
        for i in 0..board.tile_shape_count(item) {
            let Some(shape) = board.tile_shape(item, i) else {
                continue;
            };
            let moved = shape.translate_by(vector);
            if !moved.is_contained_in(&board.bounding_box) {
                return false;
            }
            let obstacles = board.overlapping_items_with_clearance_java(
                &moved,
                item.shape_layer(i),
                item.net_numbers(),
                item.clearance_class(),
            );
            for other in obstacles {
                let ignored = match ignore.as_deref() {
                    Some(set) => set.contains(&other),
                    None => other == id,
                };
                if !ignored && board.is_obstacle(board.item_ref(other), item) {
                    return false;
                }
            }
        }
        true
    }

    /// Java `RoutingBoardSearchFacade.checkChangeNet`: true if item `id` can
    /// change to net `net` without coming too close to a connectable item of
    /// another net.
    pub fn check_change_net(&self, id: ItemId, net: i32) -> bool {
        let board = self.basic();
        let Some(item) = board.item(id) else {
            return false;
        };
        let nets = [net];
        for i in 0..board.tile_shape_count(item) {
            let Some(shape) = board.tile_shape(item, i) else {
                continue;
            };
            let obstacles = board.overlapping_items_with_clearance_java(
                &shape,
                item.shape_layer(i),
                &nets,
                item.clearance_class(),
            );
            for other in obstacles {
                let other_item = board.item_ref(other);
                if other != id && other_item.is_connectable_kind() && !other_item.contains_net(net)
                {
                    return false;
                }
            }
        }
        true
    }

    /// Java `RoutingBoardSearchFacade.pickNearestRoutingItem`: the
    /// connectable item at `location` on `layer` (any layer if negative)
    /// that a route would start from, skipping the items connected to
    /// `from_item`.
    ///
    /// Items are visited highest id first. A trace is a candidate if no
    /// drill item is the current best, and it is nearer than the best and
    /// within its half width. A drill item is a candidate if its centre is
    /// nearer than the best, or the best is a trace. A conduction area is a
    /// candidate only while there is no best.
    pub fn pick_nearest_routing_item(
        &self,
        location: &Point,
        layer: i32,
        from_item: Option<ItemId>,
    ) -> Option<ItemId> {
        let board = self.basic();
        let found = board.overlapping_objects(&point_tile(location), layer);
        let pick_location = location.to_float();
        let mut min_dist = f64::from(i32::MAX);
        let mut nearest: Option<&Item> = None;
        let mut ignore_set: Option<BTreeSet<ItemId>> = None;
        for id in found {
            let item = board.item_ref(id);
            if !item.is_connectable() {
                continue;
            }
            let Some(distance) = nearest_candidate(item, layer, pick_location, min_dist, nearest)
            else {
                continue;
            };
            if let Some(from) = from_item {
                let ignore = ignore_set.get_or_insert_with(|| board.connected_set(from, -1, false));
                if ignore.contains(&id) {
                    continue;
                }
            }
            min_dist = distance;
            nearest = Some(item);
        }
        nearest.map(Item::id)
    }
}

/// The distance of `item` if it is a candidate of `pickNearestRoutingItem`
/// against the current best `nearest` at `min_dist`.
fn nearest_candidate(
    item: &Item,
    layer: i32,
    pick_location: FloatPoint,
    min_dist: f64,
    nearest: Option<&Item>,
) -> Option<f64> {
    match &item.kind {
        ItemKind::Trace(trace) => {
            if layer >= 0 && trace.layer != layer {
                return None;
            }
            if nearest.is_some_and(Item::is_drill_item) {
                return None;
            }
            let distance = trace.polyline.distance(pick_location);
            (distance < min_dist && distance <= f64::from(trace.half_width)).then_some(distance)
        }
        ItemKind::Pin(_) | ItemKind::Via(_) => {
            if layer >= 0 && !item.is_on_layer(layer) {
                return None;
            }
            let center = item.drill().expect("a drill item").center.to_float();
            let distance = center.distance(pick_location);
            (distance < min_dist || nearest.is_some_and(Item::is_trace)).then_some(distance)
        }
        ItemKind::Area(area) if item.is_conduction_area() => ((layer < 0 || area.layer() == layer)
            && nearest.is_none())
        .then_some(f64::from(i32::MAX)),
        _ => None,
    }
}

/// The octagon `point.surroundingOctagon().enlarge(half_width)` as an area,
/// the shape the `board-queries` probe passes to `check_shape`.
pub fn point_octagon_area(point: &Point, half_width: i32) -> Area {
    let octagon: IntOctagon = point.surrounding_octagon().enlarge(f64::from(half_width));
    Area::from(Shape::from(octagon))
}
