//! The shove insert (O04.1–O04.3).
//!
//! Port of the insert half of Java's push-and-shove stack (at `a773a70ec`):
//! `TraceShover.insert` ([`TraceShover::insert`]), `DrillItemMover.insert`
//! and `shoveVias` ([`TraceShover::insert_drill_item_move`],
//! [`TraceShover::shove_vias`]), `ForcedPadRouter.forcedPad`
//! ([`TraceShover::forced_pad`]) and `ForcedViaInserter.insert`
//! ([`TraceShover::insert_forced_via`]). Unlike the checks these change the
//! board: the traces in the way are cut out of the shape and their
//! substitute pieces inserted around it, vias are moved.
//!
//! A failed insert may leave the board damaged; callers take a snapshot
//! first and undo, as Java's do. The insert passes no work limit: Java's
//! `shoveVias` says "no work limit here because the item database is
//! already changed".
//!
//! Substitute pieces keep the id they burned when
//! [`ShapeTraceEntries::next_substitute_trace_piece`] built them and are
//! inserted with it (`board.insertItem` in Java). A piece that springs over
//! an obstacle only swaps its polyline, as Java's `PolylineTrace.change` does
//! for a trace that is not on the board.
//!
//! Normalize errors: at the pin `PolylineTrace.normalize` no longer throws at
//! its depth limit (it returns false), so the only exception Java catches
//! around the piece normalization in `TraceShover.insert` is the
//! `NullPointerException` of `board.changedArea.getArea(layer)` when no
//! changed area is marked; the port then skips the normalization, as Java
//! does. `forcedPad` normalizes without clip in that case, as Java does.
//!
//! Kept Java quirk: `DrillItemMover.insert` unions the translated shapes into
//! its own copy of the tidy region (`tidyRegion = tidyRegion.union(..)`), so
//! the caller's region never grows. With `java_quirks.move_drill_tidy_region`
//! off, the insertion returns that union through a mutable region (see
//! [`RoutingBoard::move_drill_item`](crate::board::RoutingBoard)).
//!
//! Kept Java bug (N06.15, found by N03.2): a substitute piece goes on the board
//! only after the recursive inserts of its own shapes, and a deeper level's
//! `shoveVias` may move a via of another net onto the piece meanwhile: the move's
//! check (`DrillItemMover.check`, `forcedPad`) cannot see a piece that is not on
//! the board, so Java then inserts the piece over the via, a short
//! (`TraceShover.java:544-569`, `ForcedPadRouter.java:413-438`). With
//! `java_quirks.shove_via_into_pending_piece` off, the piece's shapes are queried
//! again before it is inserted and the shove fails at such a via. A trace of
//! another net inserted there later in the same recursion is not covered (the
//! counter `shove_pending_piece_foreign_traces` measures it). `legacy` keeps
//! Java's bug; profile `enhanced` turns the switch off (N06.13, 2026-10-20).

use std::collections::BTreeSet;

use freerouting_geometry::{IntPoint, Point, PolylineShapeOps, TileShape, TileShapeOps, Vector};

use crate::board::items::{Item, ItemId, ItemKind, StopConnectionOption};
use crate::board::structure::{AngleRestriction, FixedState};
use crate::rules::ViaInfo;

use super::entry_side::{ShapeAndEntrySide, ShapeEntrySide, compensated_half_width_of};
use super::spring::SpringOver;
use super::trace_check::TraceShover;
use super::trace_entries::ShapeTraceEntries;
use super::via_check::hole_check_shape;

/// How the substitute pieces of [`TraceShover::insert`] and
/// [`TraceShover::forced_pad`] differ.
#[derive(Clone, Copy)]
struct PieceRules {
    /// The spring-over depth for the pieces (0 in `forcedPad`).
    spring_over_depth: i32,
    /// True: without a changed area the normalization is skipped (Java's
    /// caught `NullPointerException` in `TraceShover.insert`); false: it runs
    /// without clip (`forcedPad`).
    skip_normalize_without_area: bool,
}

impl TraceShover<'_> {
    /// Java `TraceShover.insert(traceShape, fromSide, layer, netNumbers,
    /// clearanceClassIndex, ignoreItems, maxRecursionDepth,
    /// maxViaRecursionDepth, maxSpringOverRecursionDepth)`: puts a trace
    /// segment of shape `trace_shape` on `layer` and shoves the traces and
    /// vias in the way aside. False if the shove failed; the board may then
    /// be damaged (undo it). Call [`check`](Self::check) first.
    #[allow(clippy::too_many_arguments)]
    pub fn insert(
        &mut self,
        trace_shape: &TileShape,
        from_side: Option<ShapeEntrySide>,
        layer: i32,
        net_numbers: &[i32],
        clearance_class: i32,
        ignore_items: Option<&[ItemId]>,
        max_recursion_depth: i32,
        max_via_recursion_depth: i32,
        max_spring_over_recursion_depth: i32,
    ) -> bool {
        if trace_shape.is_empty() {
            // Java warns "traceShape is empty".
            return true;
        }
        if !trace_shape.is_contained_in(&self.board.bounding_box()) {
            let outline = self.board.outline().map(Item::id);
            self.board.set_shove_failing_obstacle(outline);
            return false;
        }
        if !self.shove_vias(
            trace_shape,
            from_side,
            layer,
            net_numbers,
            clearance_class,
            ignore_items,
            max_recursion_depth,
            max_via_recursion_depth,
            true,
        ) {
            return false;
        }
        let mut entries = ShapeTraceEntries::new(
            trace_shape.clone(),
            layer,
            net_numbers,
            clearance_class,
            from_side,
        );
        let ignore = self.tie_pin_contacts(trace_shape, layer, net_numbers);
        let obstacles: Vec<ItemId> = self
            .board
            .overlapping_items_with_clearance(trace_shape, layer, &[], clearance_class)
            .into_iter()
            .filter(|id| !ignore.contains(id))
            .collect();
        let shovable = entries.store_items(self.board, obstacles.iter().copied(), false, true);
        if let Some(&via) = entries.shove_via_list.first() {
            self.board.set_shove_failing_obstacle(Some(via));
            return false;
        }
        if !shovable {
            self.board
                .set_shove_failing_obstacle(entries.found_obstacle());
            return false;
        }
        // Java logs the obstacles here ("[shove_insert_obstacles]").
        if entries.substitute_trace_count() == 0 {
            return true;
        }
        if max_recursion_depth <= 0 {
            self.board
                .set_shove_failing_obstacle(entries.found_obstacle());
            return false;
        }
        let rules = PieceRules {
            spring_over_depth: max_spring_over_recursion_depth,
            skip_normalize_without_area: true,
        };
        self.insert_substitute_pieces(
            &mut entries,
            &obstacles,
            trace_shape,
            layer,
            net_numbers,
            ignore_items,
            max_recursion_depth,
            max_via_recursion_depth,
            rules,
        )
    }

    /// The common tail of Java's `TraceShover.insert` and
    /// `ForcedPadRouter.forcedPad`: cuts the obstacle traces out of the
    /// shape, then inserts each substitute piece after shoving its own
    /// obstacles aside, normalizes it and removes the tails it left.
    #[allow(clippy::too_many_arguments)]
    fn insert_substitute_pieces(
        &mut self,
        entries: &mut ShapeTraceEntries,
        obstacles: &[ItemId],
        shape: &TileShape,
        layer: i32,
        net_numbers: &[i32],
        ignore_items: Option<&[ItemId]>,
        max_recursion_depth: i32,
        max_via_recursion_depth: i32,
        rules: PieceRules,
    ) -> bool {
        let obstacle_set: BTreeSet<ItemId> = obstacles.iter().copied().collect();
        let tails_exist_before = self.board.contains_trace_tails(&obstacle_set, net_numbers);
        let pieces = self.drain_pieces(entries);
        entries.cutout_traces(self.board, obstacles.iter().copied());
        let orthogonal = matches!(shape, TileShape::IntBox(_));
        let mut spring_depth = rules.spring_over_depth;
        for drained in pieces {
            // Java constructs the piece (taking its id) only now, after the
            // cutout and the previous pieces' inserts.
            let id = self.board.burn_item_id();
            let mut piece = Item::new(
                id,
                drained.net_numbers().to_vec(),
                drained.clearance_class(),
                0,
                FixedState::Unfixed,
                drained.kind,
            );
            let polyline = piece.as_trace().expect("substitute trace").polyline.clone();
            if polyline.first_corner() == polyline.last_corner() {
                continue;
            }
            if spring_depth > 0 {
                let tree = self.board.search_trees().default_tree();
                let half_width = compensated_half_width_of(self.board, tree, &piece);
                match self.spring_over(
                    &polyline,
                    half_width,
                    layer,
                    piece.net_numbers(),
                    piece.clearance_class(),
                    false,
                    spring_depth,
                    None,
                ) {
                    SpringOver::Blocked => return false,
                    SpringOver::NoObstacle => {}
                    SpringOver::Wrapped(new_polyline) => {
                        spring_depth -= 1;
                        // Java `change` on a trace not on the board.
                        if let ItemKind::Trace(t) = &mut piece.kind {
                            t.polyline = new_polyline;
                        }
                    }
                }
            }
            let piece_nets = piece.net_numbers().to_vec();
            let piece_class = piece.clearance_class();
            let shape_count = piece
                .as_trace()
                .expect("substitute trace")
                .tile_shape_count();
            let check_pending = self.checks_pending_piece();
            let mut piece_shapes = Vec::new();
            for i in 0..shape_count {
                let Some(current) =
                    ShapeAndEntrySide::new(self.board, &piece, i, orthogonal, false)
                else {
                    return false;
                };
                if !self.insert(
                    &current.shape,
                    current.from_side,
                    layer,
                    &piece_nets,
                    piece_class,
                    ignore_items,
                    max_recursion_depth - 1,
                    max_via_recursion_depth,
                    spring_depth,
                ) {
                    return false;
                }
                if check_pending {
                    piece_shapes.push(current.shape);
                }
            }
            // PARITY(quirk, java_quirks.shove_via_into_pending_piece): Java inserts the
            // piece even when a deeper level of this recursion moved a via of another
            // net onto it (`shoveVias`; the move's check cannot see the piece, which is
            // not on the board yet), a short. `false` fails the shove there, as the
            // insert does for a via in its way (N06.15).
            if check_pending
                && let Some(via) = self.foreign_via_in_pending_piece(
                    &piece_shapes,
                    layer,
                    &piece_nets,
                    piece_class,
                )
            {
                self.board.set_shove_failing_obstacle(Some(via));
                return false;
            }
            let polyline = piece.as_trace().expect("substitute trace").polyline.clone();
            for i in 0..polyline.corner_count() {
                self.board
                    .join_changed_area(polyline.corner_approx(i), layer);
            }
            let end_corners = [polyline.first_corner(), polyline.last_corner()];
            let id = self.board.insert_item(piece);
            match self.board.changed_area_on_layer(layer) {
                Some(area) => {
                    self.board.normalize_trace(id, Some(&area));
                }
                None if rules.skip_normalize_without_area => {
                    // Java: NullPointerException on `changedArea`, logged.
                }
                None => {
                    self.board.normalize_trace(id, None);
                }
            }
            if !tails_exist_before {
                for corner in &end_corners {
                    if let Some(tail) = self.board.get_trace_tail(corner, layer, &piece_nets) {
                        let connection =
                            self.board.connection_items(tail, StopConnectionOption::Via);
                        self.board.remove_items(connection);
                        for &net in &piece_nets {
                            self.board.combine_traces(net);
                        }
                    }
                }
            }
        }
        true
    }

    /// True if [`foreign_via_in_pending_piece`](Self::foreign_via_in_pending_piece)
    /// runs: under `java_quirks.shove_via_into_pending_piece = false`, or to count
    /// with statistics on.
    fn checks_pending_piece(&self) -> bool {
        !self.board.java_quirks().shove_via_into_pending_piece || crate::run_stats::enabled()
    }

    /// N06.15: the via of another net that overlaps one of `shapes` (a substitute
    /// piece's shapes, about to be inserted) with the piece's clearance, which the
    /// shove must fail at under `java_quirks.shove_via_into_pending_piece = false`;
    /// always `None` under `true` (Java). A via there arrived during the recursion
    /// of the piece's own shapes: a via in a shape when its insert starts fails that
    /// insert.
    ///
    /// Under `false` the shapes are queried in order, as the board's other queries
    /// are (on the work meter), up to the first with such a via, whose lowest id is
    /// the answer. Under `true` it only counts, with statistics on, on queries that
    /// leave the work meter alone, so that no work limit cuts off elsewhere.
    ///
    /// The counters count pieces, in either value: `shove_via_into_pending_pieces`
    /// (such a via) and `shove_pending_piece_foreign_traces`, a trace of another net
    /// whose copper overlaps the piece's shapes (no clearance), which neither value
    /// acts on: deeper pieces of other nets are placed against a pending piece at
    /// exactly the clearance, so a clearance query would find those too.
    fn foreign_via_in_pending_piece(
        &mut self,
        shapes: &[TileShape],
        layer: i32,
        piece_nets: &[i32],
        piece_class: i32,
    ) -> Option<ItemId> {
        let fix = !self.board.java_quirks().shove_via_into_pending_piece;
        let foreign = |board: &crate::board::BasicBoard, id: ItemId, via: bool| {
            board.item(id).is_some_and(|item| {
                !item.shares_net_no(piece_nets) && if via { item.is_via() } else { item.is_trace() }
            })
        };
        let mut via = None;
        for shape in shapes {
            let found = if fix {
                self.board
                    .overlapping_items_with_clearance(shape, layer, &[], piece_class)
            } else {
                self.board.overlapping_items_with_clearance_unmetered(
                    shape,
                    layer,
                    &[],
                    piece_class,
                )
            };
            via = found.into_iter().find(|id| foreign(self.board, *id, true));
            if via.is_some() {
                break;
            }
        }
        if crate::run_stats::enabled() {
            use crate::run_stats::{Counter, record_n06};
            if via.is_some() {
                record_n06(Counter::ShoveViaIntoPendingPieces);
            }
            // Clearance class 0 has clearance 0 to every class: copper overlap only.
            let trace = shapes.iter().any(|shape| {
                self.board
                    .overlapping_items_with_clearance_unmetered(shape, layer, &[], 0)
                    .into_iter()
                    .any(|id| foreign(self.board, id, false))
            });
            if trace {
                record_n06(Counter::ShovePendingPieceForeignTraces);
            }
        }
        if fix { via } else { None }
    }

    /// Java's `nextSubstituteTracePiece` loop, run before the cutout: the
    /// pieces in Java's order, with the id generator rewound afterwards.
    ///
    /// [`ShapeTraceEntries::next_substitute_trace_piece`] reads the obstacle
    /// traces from the board, but the cutout removes them, while Java's
    /// entries keep the trace objects. A piece depends only on the stored
    /// entries and on those traces' data, which neither the cutout nor the
    /// pieces' own inserts change, so building all pieces first gives
    /// Java's pieces; the caller then takes each piece's id where Java's
    /// constructor takes it.
    fn drain_pieces(&mut self, entries: &mut ShapeTraceEntries) -> Vec<Item> {
        let generator = self.board.communication.id_generator.clone();
        let mut pieces = Vec::new();
        while let Some(piece) = entries.next_substitute_trace_piece(self.board) {
            pieces.push(piece);
        }
        self.board.communication.id_generator = generator;
        pieces
    }

    /// Java `getIgnoreItemsAtTiePins` for the insert.
    fn tie_pin_contacts(
        &self,
        trace_shape: &TileShape,
        layer: i32,
        net_numbers: &[i32],
    ) -> BTreeSet<ItemId> {
        let board = &*self.board;
        let mut result = BTreeSet::new();
        for id in board.overlapping_objects(trace_shape, layer) {
            if board
                .item(id)
                .is_some_and(|i| i.is_pin() && i.shares_net_no(net_numbers))
            {
                result.extend(board.all_contacts(id, Some(layer)));
            }
        }
        result
    }

    /// Java `DrillItemMover.insert(drillItem, vector, maxRecursionDepth,
    /// maxViaRecursionDepth, tidyRegion, board)`: moves drill item `drill`
    /// by `vector` after shoving the traces and vias in the way aside, layer
    /// by layer from the first to the last. False if the item is shove-fixed
    /// or a shove failed (the board may then be damaged).
    ///
    /// Java's tidy region argument is only grown in the callee's copy, so
    /// the public wrapper discards it. `move_drill_item` uses the internal
    /// mutable-region variant when its quirk is disabled (module docs).
    pub fn insert_drill_item_move(
        &mut self,
        drill: ItemId,
        vector: &Vector,
        max_recursion_depth: i32,
        max_via_recursion_depth: i32,
    ) -> bool {
        self.insert_drill_item_move_with_region(
            drill,
            vector,
            max_recursion_depth,
            max_via_recursion_depth,
            None,
        )
    }

    pub(crate) fn insert_drill_item_move_with_region(
        &mut self,
        drill: ItemId,
        vector: &Vector,
        max_recursion_depth: i32,
        max_via_recursion_depth: i32,
        mut tidy_region: Option<&mut freerouting_geometry::IntOctagon>,
    ) -> bool {
        let Some(item) = self.board.item(drill) else {
            return false;
        };
        if self.board.is_shove_fixed(item) {
            return false;
        }
        let attach_allowed = matches!(&item.kind, ItemKind::Via(v) if v.attach_allowed);
        let Some(data) = item.drill() else {
            return false;
        };
        let (first_layer, last_layer, center) = (data.first_layer, data.last_layer, data.center);
        let nets = item.net_numbers().to_vec();
        let class = item.clearance_class();
        let item = item.clone();
        let ignore = [drill];
        for layer in first_layer..=last_layer {
            let board = &*self.board;
            let tree = board.search_trees().default_tree();
            let index = (layer - first_layer) as usize;
            let Some(current_shape) = tree.tree_shape(&item, index, &board.shape_context()) else {
                continue;
            };
            let new_shape = current_shape.translate_by(vector);
            let tile = if board.rules.trace_angle_restriction() == AngleRestriction::NinetyDegree {
                TileShape::IntBox(new_shape.bounding_box())
            } else {
                match new_shape.bounding_octagon() {
                    Some(o) => TileShape::IntOctagon(o),
                    None => continue,
                }
            };
            if let Some(region) = tidy_region.as_deref_mut()
                && let Some(octagon) = tile.bounding_octagon()
            {
                *region = region.union_octagon(&octagon);
            }
            let from_side = ShapeEntrySide::from_point(&Point::Int(center), &tile);
            if !self.forced_pad(
                &tile,
                Some(from_side),
                layer,
                &nets,
                class,
                attach_allowed,
                Some(&ignore),
                max_recursion_depth,
                max_via_recursion_depth,
            ) {
                return false;
            }
            let old_box = TileShape::IntBox(current_shape.bounding_box());
            for j in 0..4 {
                self.board
                    .join_changed_area(old_box.corner_approx(j), layer);
            }
        }
        self.board.move_item(drill, vector);
        true
    }

    /// Java `DrillItemMover.shoveVias(obstacleShape, fromSide, layer,
    /// netNumbers, clearanceClassIndex, ignoreItems, maxRecursionDepth,
    /// maxViaRecursionDepth, copperSharingAllowed, board)`: moves the
    /// foreign vias in `obstacle_shape` out of it, in store order. A via
    /// that can't be moved stays (the trace insert then fails on it). False
    /// only if a move failed and damaged the board.
    #[allow(clippy::too_many_arguments)]
    pub fn shove_vias(
        &mut self,
        obstacle_shape: &TileShape,
        from_side: Option<ShapeEntrySide>,
        layer: i32,
        net_numbers: &[i32],
        clearance_class: i32,
        ignore_items: Option<&[ItemId]>,
        max_recursion_depth: i32,
        max_via_recursion_depth: i32,
        copper_sharing_allowed: bool,
    ) -> bool {
        let mut entries = ShapeTraceEntries::new(
            obstacle_shape.clone(),
            layer,
            net_numbers,
            clearance_class,
            from_side,
        );
        let obstacles = self.board.overlapping_items_with_clearance(
            obstacle_shape,
            layer,
            &[],
            clearance_class,
        );
        if !entries.store_items(self.board, obstacles, false, copper_sharing_allowed) {
            return true;
        }
        let vias: Vec<ItemId> = entries
            .shove_via_list
            .iter()
            .copied()
            .filter(|id| ignore_items.is_none_or(|ignore| !ignore.contains(id)))
            .collect();
        let shape_radius = 0.5 * obstacle_shape.bounding_box().min_width();
        for via in vias {
            let Some(item) = self.board.item(via) else {
                continue;
            };
            if item.shares_net_no(net_numbers) {
                continue;
            }
            if max_via_recursion_depth <= 0 {
                return true;
            }
            let drill = item.drill().expect("via");
            let center = drill.center;
            let via_width = drill
                .shape_on_layer(layer)
                .map_or(0.0, |s| s.bounding_box().max_width());
            let try_centers =
                self.try_shove_via_points(obstacle_shape, layer, via, clearance_class, true);
            let max_dist = 0.5 * via_width + shape_radius;
            let max_dist_square = max_dist * max_dist;
            let mut found: Option<Vector> = None;
            for (i, target) in try_centers.iter().enumerate() {
                if i == 0 || center.to_float().distance_square(target.to_float()) <= max_dist_square
                {
                    let mut local_ignore: Vec<ItemId> =
                        ignore_items.map(<[ItemId]>::to_vec).unwrap_or_default();
                    let delta = via_vector(*target, center);
                    // No work limit: the item database is already changed.
                    if self.check_drill_item_move(
                        via,
                        &delta,
                        max_recursion_depth,
                        max_via_recursion_depth - 1,
                        &mut local_ignore,
                        None,
                    ) {
                        found = Some(delta);
                        break;
                    }
                }
            }
            let Some(delta) = found else {
                continue;
            };
            if !self.insert_drill_item_move(
                via,
                &delta,
                max_recursion_depth,
                max_via_recursion_depth - 1,
            ) {
                return false;
            }
        }
        true
    }

    /// Java `ForcedPadRouter.forcedPad(padShape, fromSide, layer, netNumbers,
    /// clearanceClassIndex, copperSharingAllowed, ignoreItems,
    /// maxRecursionDepth, maxViaRecursionDepth)`: shoves the traces in the
    /// way aside so that a pad of shape `pad_shape` fits on `layer`. False
    /// if the shove failed; the board may then be damaged.
    #[allow(clippy::too_many_arguments)]
    pub fn forced_pad(
        &mut self,
        pad_shape: &TileShape,
        from_side: Option<ShapeEntrySide>,
        layer: i32,
        net_numbers: &[i32],
        clearance_class: i32,
        copper_sharing_allowed: bool,
        ignore_items: Option<&[ItemId]>,
        max_recursion_depth: i32,
        max_via_recursion_depth: i32,
    ) -> bool {
        if pad_shape.is_empty() {
            // Java warns "padShape is empty".
            return true;
        }
        if !pad_shape.is_contained_in(&self.board.bounding_box()) {
            let outline = self.board.outline().map(Item::id);
            self.board.set_shove_failing_obstacle(outline);
            return false;
        }
        if !self.shove_vias(
            pad_shape,
            from_side,
            layer,
            net_numbers,
            clearance_class,
            ignore_items,
            max_recursion_depth,
            max_via_recursion_depth,
            false,
        ) {
            return false;
        }
        let mut entries = ShapeTraceEntries::new(
            pad_shape.clone(),
            layer,
            net_numbers,
            clearance_class,
            from_side,
        );
        let obstacles: Vec<ItemId> = self
            .board
            .overlapping_items_with_clearance(pad_shape, layer, &[], clearance_class)
            .into_iter()
            .filter(|id| ignore_items.is_none_or(|ignore| !ignore.contains(id)))
            .collect();
        let shovable = entries.store_items(
            self.board,
            obstacles.iter().copied(),
            true,
            copper_sharing_allowed,
        ) && entries.shove_via_list.is_empty();
        if !shovable {
            self.board
                .set_shove_failing_obstacle(entries.found_obstacle());
            return false;
        }
        if entries.substitute_trace_count() == 0 {
            return true;
        }
        if max_recursion_depth <= 0 {
            self.board
                .set_shove_failing_obstacle(entries.found_obstacle());
            return false;
        }
        let rules = PieceRules {
            spring_over_depth: 0,
            skip_normalize_without_area: false,
        };
        self.insert_substitute_pieces(
            &mut entries,
            &obstacles,
            pad_shape,
            layer,
            net_numbers,
            ignore_items,
            max_recursion_depth,
            max_via_recursion_depth,
            rules,
        )
    }

    /// Java `ForcedViaInserter.insert(viaInfo, location, netNumbers,
    /// traceClearanceClassIndex, tracePenHalfwidthArr, maxRecursionDepth,
    /// maxViaRecursionDepth, board)`: shoves the traces in the way aside on
    /// every layer of the via's padstack and inserts an unfixed via of type
    /// `via_info` at `location`. False if a shove failed; the board's
    /// shove-failing layer is then the failing layer and the board may be
    /// damaged.
    ///
    /// `(attach off)` (D8): `via_info.attach_smd_allowed` is the copper
    /// sharing flag of the pad shoves and the via's attach flag, exactly as
    /// in Java; nothing else is added.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_forced_via(
        &mut self,
        via_info: &ViaInfo,
        location: IntPoint,
        net_numbers: &[i32],
        trace_clearance_class: i32,
        trace_pen_half_widths: &[i32],
        max_recursion_depth: i32,
        max_via_recursion_depth: i32,
    ) -> bool {
        let point = Point::Int(location);
        let translate_vector = point.difference_by(&Point::ZERO);
        let calc_from_side_offset = self.board.min_trace_half_width();
        let Some(padstack) = self.board.library.padstacks.get(via_info.padstack).cloned() else {
            return false;
        };
        let hole_shape = hole_check_shape(self.board, &padstack, &point);
        let ninety = self.board.rules.trace_angle_restriction() == AngleRestriction::NinetyDegree;
        let tile_of = |bounding_box: freerouting_geometry::IntBox,
                       octagon: Option<freerouting_geometry::IntOctagon>| {
            if ninety {
                TileShape::IntBox(bounding_box)
            } else {
                TileShape::IntOctagon(octagon.unwrap_or(freerouting_geometry::IntOctagon::EMPTY))
            }
        };
        for layer in padstack.from_layer()..=padstack.to_layer() {
            let (pad_shape, class) = match padstack.get_shape(layer) {
                Some(shape) => (
                    shape.to_shape().translate_by(&translate_vector),
                    via_info.clearance_class,
                ),
                None => match &hole_shape {
                    Some(hole) => (hole.clone(), 0),
                    None => continue,
                },
            };
            let pen = usize::try_from(layer)
                .ok()
                .and_then(|l| trace_pen_half_widths.get(l))
                .copied()
                .unwrap_or(0);
            let tile_shape = tile_of(pad_shape.bounding_box(), pad_shape.bounding_octagon());
            let start_trace_shape = (pen > 0).then(|| {
                let circle = freerouting_geometry::Circle::new(location, pen);
                tile_of(circle.bounding_box(), Some(circle.bounding_octagon()))
            });
            let from_side =
                self.calc_from_side(&tile_shape, &point, layer, calc_from_side_offset, class);
            let attach = via_info.attach_smd_allowed;
            let mut shoves = vec![(tile_shape, class, attach)];
            if class != 0
                && let Some(hole) = &hole_shape
            {
                let hole_tile = tile_of(hole.bounding_box(), hole.bounding_octagon());
                shoves.push((hole_tile, 0, attach));
            }
            if let Some(start) = start_trace_shape {
                // necessary in case the start trace shape is bigger than the pad
                shoves.push((start, trace_clearance_class, true));
            }
            for (shape, shove_class, copper_sharing) in shoves {
                if !self.forced_pad(
                    &shape,
                    Some(from_side),
                    layer,
                    net_numbers,
                    shove_class,
                    copper_sharing,
                    None,
                    max_recursion_depth,
                    max_via_recursion_depth,
                ) {
                    self.board.set_shove_failing_layer(layer);
                    return false;
                }
            }
        }
        self.board.insert_via(
            via_info.padstack,
            location,
            net_numbers,
            via_info.clearance_class,
            FixedState::Unfixed,
            via_info.attach_smd_allowed,
        );
        true
    }
}

/// `target - center` (Java `tryViaCenters[i].differenceBy(currentViaCenter)`).
fn via_vector(target: IntPoint, center: IntPoint) -> Vector {
    Point::Int(target).difference_by(&Point::Int(center))
}

#[cfg(test)]
#[path = "insert_pending_tests.rs"]
mod pending_tests;
