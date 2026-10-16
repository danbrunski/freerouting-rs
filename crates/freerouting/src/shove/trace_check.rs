//! The trace shove check (O03.4).
//!
//! Port of `TraceShover.check` (both forms) and `getIgnoreItemsAtTiePins`
//! (Java at `a773a70ec`). [`TraceShover::check`] answers "could a trace
//! segment of shape `trace_shape` go on `layer` if the traces and vias in
//! the way were shoved aside?", recursing on the substitute traces it would
//! build, moving vias through the drill check and springing over fixed
//! obstacles. [`TraceShover::check_segment_ok_length`] is Java's static
//! form: how far from its start a segment can be shoved.
//!
//! No item changes, but Java's side effects stay: every substitute piece
//! burns an item id, and a failed check sets the board's shove-failing
//! obstacle at Java's points. Java's trace-level obstacle log is not ported.
//! The work limit is checked at entry (and in the drill check), as at the
//! pin; it only stops work.

use std::collections::BTreeSet;

use freerouting_geometry::jmath::min_f64;
use freerouting_geometry::{Direction, LineSegment, PolylineShapeOps, TileShape, TileShapeOps};

use crate::board::RoutingBoard;
use crate::board::items::{Item, ItemId, ItemKind};
use crate::board::search::WorkLimit;

use super::drill_check::{drill_tree_shape_on_layer, limit_exceeded, via_delta};
use super::entry_side::{ShapeAndEntrySide, ShapeEntrySide, compensated_half_width_of};
use super::spring::SpringOver;
use super::trace_entries::ShapeTraceEntries;

/// Java `TraceShover`: the shove algorithms on one board.
pub struct TraceShover<'a> {
    pub(crate) board: &'a mut RoutingBoard,
}

/// Java's `Integer.MAX_VALUE` as the "fully shovable" OK length.
pub const FULL_OK_LENGTH: f64 = i32::MAX as f64;

impl<'a> TraceShover<'a> {
    /// Java `new TraceShover(board)`.
    pub fn new(board: &'a mut RoutingBoard) -> Self {
        TraceShover { board }
    }

    /// The board.
    pub fn board(&self) -> &RoutingBoard {
        self.board
    }

    /// Java `TraceShover.check(traceShape, fromSide, dir, layer, netNumbers,
    /// clearanceClassIndex, maxRecursionDepth, maxViaRecursionDepth,
    /// maxSpringOverRecursionDepth, workLimit)`. `dir` keeps the recursion
    /// from bouncing back. False if the shove fails.
    #[allow(clippy::too_many_arguments)]
    pub fn check(
        &mut self,
        trace_shape: &TileShape,
        from_side: Option<ShapeEntrySide>,
        dir: Option<Direction>,
        layer: i32,
        net_numbers: &[i32],
        clearance_class: i32,
        max_recursion_depth: i32,
        max_via_recursion_depth: i32,
        max_spring_over_recursion_depth: i32,
        mut work_limit: Option<&mut WorkLimit>,
    ) -> bool {
        if limit_exceeded(self.board, work_limit.as_deref_mut()) {
            return false;
        }
        if trace_shape.is_empty() {
            // Java logs "traceShape is empty".
            return true;
        }
        if !trace_shape.is_contained_in(&self.board.bounding_box()) {
            let outline = self.board.outline().map(Item::id);
            self.board.set_shove_failing_obstacle(outline);
            return false;
        }
        let ignore = self.ignore_items_at_tie_pins(trace_shape, layer, net_numbers);
        let obstacles: Vec<ItemId> = self
            .board
            .overlapping_items_with_clearance(trace_shape, layer, &[], clearance_class)
            .into_iter()
            .filter(|id| !ignore.contains(id))
            .collect();
        let mut entries = ShapeTraceEntries::new(
            trace_shape.clone(),
            layer,
            net_numbers,
            clearance_class,
            from_side,
        );
        if !entries.store_items(self.board, obstacles.iter().copied(), false, true) {
            self.board
                .set_shove_failing_obstacle(entries.found_obstacle());
            return false;
        }
        if entries.stack_depth() > 1 {
            self.board
                .set_shove_failing_obstacle(entries.found_obstacle());
            return false;
        }
        if !self.check_shove_vias(
            trace_shape,
            &entries,
            layer,
            net_numbers,
            clearance_class,
            max_recursion_depth,
            max_via_recursion_depth,
            work_limit.as_deref_mut(),
        ) {
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
        let orthogonal = matches!(trace_shape, TileShape::IntBox(_));
        let mut spring_depth = max_spring_over_recursion_depth;
        while let Some(mut piece) = entries.next_substitute_trace_piece(self.board) {
            if spring_depth > 0 {
                let tree = self.board.search_trees().default_tree();
                let half_width = compensated_half_width_of(self.board, tree, &piece);
                let polyline = piece.as_trace().expect("piece").polyline.clone();
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
            let mut i = 0;
            while i < piece.as_trace().expect("piece").tile_shape_count() {
                let direction =
                    piece.as_trace().expect("piece").polyline.lines()[i + 1].direction();
                if dir.as_ref().is_none_or(|d| *d == direction) {
                    let Some(current) =
                        ShapeAndEntrySide::new(self.board, &piece, i, orthogonal, true)
                    else {
                        return false;
                    };
                    if !self.check(
                        &current.shape,
                        current.from_side,
                        Some(direction),
                        layer,
                        piece.net_numbers(),
                        piece.clearance_class(),
                        max_recursion_depth - 1,
                        max_via_recursion_depth,
                        spring_depth,
                        work_limit.as_deref_mut(),
                    ) {
                        return false;
                    }
                }
                i += 1;
            }
        }
        true
    }

    /// The via half of Java's instance `check`: every foreign shove via must
    /// move to one of its try centres within reach.
    #[allow(clippy::too_many_arguments)]
    fn check_shove_vias(
        &mut self,
        trace_shape: &TileShape,
        entries: &ShapeTraceEntries,
        layer: i32,
        net_numbers: &[i32],
        clearance_class: i32,
        max_recursion_depth: i32,
        max_via_recursion_depth: i32,
        mut work_limit: Option<&mut WorkLimit>,
    ) -> bool {
        let shape_radius = 0.5 * trace_shape.bounding_box().min_width();
        for &via in &entries.shove_via_list {
            let Some(item) = self.board.item(via) else {
                continue;
            };
            if item.shares_net_no(net_numbers) {
                continue;
            }
            if max_via_recursion_depth <= 0 {
                self.board.set_shove_failing_obstacle(Some(via));
                return false;
            }
            let drill = item.drill().expect("via");
            let center = drill.center.to_float();
            let via_width = drill
                .shape_on_layer(layer)
                .map_or(0.0, |s| s.bounding_box().max_width());
            let try_centers =
                self.try_shove_via_points(trace_shape, layer, via, clearance_class, true);
            let max_dist = 0.5 * via_width + shape_radius;
            let max_dist_square = max_dist * max_dist;
            let mut ok = false;
            for (i, target) in try_centers.iter().enumerate() {
                if i == 0 || center.distance_square(target.to_float()) <= max_dist_square {
                    let delta = via_delta(self.board, via, *target);
                    let mut ignore = Vec::new();
                    if self.check_drill_item_move(
                        via,
                        &delta,
                        max_recursion_depth,
                        max_via_recursion_depth - 1,
                        &mut ignore,
                        work_limit.as_deref_mut(),
                    ) {
                        ok = true;
                        break;
                    }
                }
            }
            if !ok {
                return false;
            }
        }
        true
    }

    /// Java `getIgnoreItemsAtTiePins`: the contacts on `layer` of the pins
    /// of the own nets that overlap `trace_shape`.
    fn ignore_items_at_tie_pins(
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

    /// Java's static `TraceShover.check(board, lineSegment, shoveToTheLeft,
    /// layer, netNumbers, traceHalfWidth, clearanceClassIndex,
    /// maxRecursionDepth, maxViaRecursionDepth)`: the length from the start
    /// of `line_segment` for which the shove succeeds, [`FULL_OK_LENGTH`]
    /// if it succeeds completely, 0 for an empty or off-board shape.
    #[allow(clippy::too_many_arguments)]
    pub fn check_segment_ok_length(
        &mut self,
        line_segment: &LineSegment,
        shove_to_the_left: bool,
        layer: i32,
        net_numbers: &[i32],
        trace_half_width: i32,
        clearance_class: i32,
        max_recursion_depth: i32,
        max_via_recursion_depth: i32,
    ) -> f64 {
        let compensated = self.board.search_trees().is_clearance_compensation_used();
        let matrix_compensation = |board: &RoutingBoard, class: i32| {
            board
                .search_trees()
                .default_tree()
                .clearance_compensation_value(&board.rules.clearance_matrix, class, layer)
        };
        let mut half_width = trace_half_width;
        if compensated {
            half_width += matrix_compensation(self.board, clearance_class);
        }
        let shapes = line_segment.to_polyline().offset_shapes(half_width);
        if shapes.len() != 1 {
            // Java logs "traceShape count 1 expected".
            return 0.0;
        }
        let trace_shape = &shapes[0];
        if trace_shape.is_empty() || !trace_shape.is_contained_in(&self.board.bounding_box()) {
            return 0.0;
        }
        let from_side = ShapeEntrySide::from_segment(line_segment, trace_shape, shove_to_the_left);
        let obstacles =
            self.board
                .overlapping_items_with_clearance(trace_shape, layer, &[], clearance_class);
        let mut entries = ShapeTraceEntries::new(
            trace_shape.clone(),
            layer,
            net_numbers,
            clearance_class,
            Some(from_side),
        );
        if !entries.store_items(self.board, obstacles.iter().copied(), false, true)
            || entries.trace_tails_in_shape()
        {
            return 0.0;
        }
        let piece_count = entries.substitute_trace_count();
        if entries.stack_depth() > 1 {
            return 0.0;
        }
        let start = line_segment.start_point_approx();
        let end = line_segment.end_point_approx();
        let segment_length = end.distance(start);
        let mut result = FULL_OK_LENGTH;
        // Can the obstacle vias be shoved?
        for via in entries.shove_via_list.clone() {
            let Some(item) = self.board.item(via) else {
                continue;
            };
            if item.shares_net_no(net_numbers) {
                continue;
            }
            let mut via_ok = false;
            if max_via_recursion_depth > 0 {
                let centers =
                    self.try_shove_via_points(trace_shape, layer, via, clearance_class, false);
                let Some(first) = centers.first() else {
                    return 0.0;
                };
                let delta = via_delta(self.board, via, *first);
                let mut ignore = Vec::new();
                via_ok = self.check_drill_item_move(
                    via,
                    &delta,
                    max_recursion_depth,
                    max_via_recursion_depth - 1,
                    &mut ignore,
                    None,
                );
            }
            if !via_ok {
                let item = self.board.item(via).expect("via");
                let center = item.drill().expect("via").center.to_float();
                let projection = start.scalar_product(end, center) / segment_length;
                let via_box = drill_tree_shape_on_layer(self.board, item, layer)
                    .map_or(freerouting_geometry::IntBox::EMPTY, |s| s.bounding_box());
                let via_radius = 0.5 * via_box.max_width();
                let mut ok_length = projection - via_radius - f64::from(half_width);
                if !compensated {
                    ok_length -= f64::from(self.board.rules.clearance_matrix.get_value(
                        clearance_class,
                        item.clearance_class(),
                        layer,
                        true,
                    ));
                }
                if ok_length <= 0.0 {
                    return 0.0;
                }
                result = min_f64(result, ok_length);
            }
        }
        if piece_count == 0 {
            return result;
        }
        if max_recursion_depth <= 0 {
            return 0.0;
        }
        let line_direction = line_segment.get_line().direction();
        while let Some(piece) = entries.next_substitute_trace_piece(self.board) {
            let trace = piece.as_trace().expect("piece");
            for i in 0..trace.tile_shape_count() {
                let mut current = LineSegment::from_polyline(&trace.polyline, i + 1);
                if shove_to_the_left {
                    // The shove length is then measured from the right end.
                    current = current.opposite();
                }
                if current.get_line().direction() != line_direction {
                    continue;
                }
                let shove_ok_length = self.check_segment_ok_length(
                    &current,
                    shove_to_the_left,
                    layer,
                    piece.net_numbers(),
                    trace.half_width,
                    piece.clearance_class(),
                    max_recursion_depth - 1,
                    max_via_recursion_depth,
                );
                if shove_ok_length < FULL_OK_LENGTH {
                    if shove_ok_length <= 0.0 {
                        return 0.0;
                    }
                    let projection = min_f64(
                        start.scalar_product(end, current.start_point_approx()),
                        start.scalar_product(end, current.end_point_approx()),
                    ) / segment_length;
                    let mut ok_length = shove_ok_length + projection
                        - f64::from(half_width)
                        - f64::from(trace.half_width);
                    if compensated {
                        ok_length -=
                            f64::from(matrix_compensation(self.board, piece.clearance_class()));
                    } else {
                        ok_length -= f64::from(self.board.rules.clearance_matrix.get_value(
                            clearance_class,
                            piece.clearance_class(),
                            layer,
                            true,
                        ));
                    }
                    if ok_length <= 0.0 {
                        return 0.0;
                    }
                    result = min_f64(ok_length, result);
                }
                break;
            }
        }
        result
    }
}
