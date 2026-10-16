//! The forced-trace check (O03.7).
//!
//! Port of `RoutingBoard.checkForcedTracePolyline` (Java at `a773a70ec`):
//! could a trace along a polyline go on a layer if the traces and vias in
//! the way were shoved aside? The maze search asks it for every door it
//! wants to pass with ripup off. Java passes no work limit here.

use freerouting_geometry::{Polyline, PolylineShapeOps, TileShape};

use crate::board::RoutingBoard;
use crate::board::structure::AngleRestriction;

use super::entry_side::ShapeEntrySide;
use super::trace_check::TraceShover;

impl RoutingBoard {
    /// Java `RoutingBoard.checkForcedTracePolyline(polyline, halfWidth,
    /// layer, netNumbers, clearanceClassIndex, maxRecursionDepth,
    /// maxViaRecursionDepth, maxSpringOverRecursionDepth)`: shove-checks the
    /// offset shapes of `polyline` with the compensated half width one by
    /// one (their bounding boxes in 90 degree mode), each entered from the
    /// side where the polyline enters it. False at the first failing shape.
    #[allow(clippy::too_many_arguments)]
    pub fn check_forced_trace_polyline(
        &mut self,
        polyline: &Polyline,
        half_width: i32,
        layer: i32,
        net_numbers: &[i32],
        clearance_class: i32,
        max_recursion_depth: i32,
        max_via_recursion_depth: i32,
        max_spring_over_recursion_depth: i32,
    ) -> bool {
        let compensated_half_width = half_width
            + self
                .search_trees()
                .default_tree()
                .clearance_compensation_value(&self.rules.clearance_matrix, clearance_class, layer);
        let line_count = polyline.lines().len();
        let trace_shapes =
            polyline.offset_shapes_range(compensated_half_width, 0, line_count.saturating_sub(1));
        let orthogonal = self.rules.trace_angle_restriction() == AngleRestriction::NinetyDegree;
        let mut shover = TraceShover::new(self);
        for (i, shape) in trace_shapes.iter().enumerate() {
            let current = if orthogonal {
                TileShape::IntBox(shape.bounding_box())
            } else {
                shape.clone()
            };
            let from_side = ShapeEntrySide::from_polyline(polyline, i + 1, &current);
            if !shover.check(
                &current,
                Some(from_side),
                None,
                layer,
                net_numbers,
                clearance_class,
                max_recursion_depth,
                max_via_recursion_depth,
                max_spring_over_recursion_depth,
                None,
            ) {
                return false;
            }
        }
        true
    }
}

// ----------------------------------------------------------------------
// O04.4–O04.5: forced insertion
// ----------------------------------------------------------------------

use freerouting_geometry::{IntOctagon, IntPoint, Point, Vector};

use crate::board::items::ItemId;
use crate::board::search::WorkLimit;
use crate::board::selection_filter::{ItemFilter, SelectableChoice};
use crate::board::structure::FixedState;
use crate::optimize::{Clip, PullTightParams, Tightener};
use crate::rules::ViaInfo;

use super::spring::SpringOver;

/// What a forced trace insertion did (Java's return value of
/// `RoutingBoard.insertForcedTracePolyline` and `insertForcedTraceSegment`).
///
/// Java signals the outcome by object identity: `null` (database damaged),
/// the polyline's first corner object (nothing inserted), its last corner
/// object (reached the end) or a new point (inserted up to it). The variant
/// is reported where Java returns that object, never inferred from values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ForcedInsert {
    /// An insert failed after the board changed: undo it (Java `null`).
    DatabaseDamaged,
    /// Nothing was inserted (Java returns `fromCorner`).
    Unchanged,
    /// The trace reaches the last corner (Java returns `toCorner`).
    Reached,
    /// The trace was inserted up to this point, short of the last corner.
    Partial(IntPoint),
}

impl ForcedInsert {
    /// Java `insertForcedTraceSegment`'s result point: `from` for
    /// [`Unchanged`](Self::Unchanged), `to` for [`Reached`](Self::Reached),
    /// the partial point, or `None` for a damaged database.
    pub fn point(&self, from: &Point, to: &Point) -> Option<Point> {
        match self {
            ForcedInsert::DatabaseDamaged => None,
            ForcedInsert::Unchanged => Some(from.clone()),
            ForcedInsert::Reached => Some(to.clone()),
            ForcedInsert::Partial(p) => Some(Point::Int(*p)),
        }
    }
}

/// The depths and pull-tight settings of a forced trace insertion (Java's
/// `maxRecursionDepth`, `maxViaRecursionDepth`,
/// `maxSpringOverRecursionDepth`, `tidyWidth`, `pullTightAccuracy`,
/// `withCheck`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ForcedTraceParams {
    pub max_recursion_depth: i32,
    pub max_via_recursion_depth: i32,
    pub max_spring_over_recursion_depth: i32,
    /// The pull-tight region around the new corner; `i32::MAX` for no
    /// clip, 0 for no pull-tight (a keep-point split still happens).
    pub tidy_width: i32,
    pub pull_tight_accuracy: i32,
    /// Shove-check every shape before inserting it.
    pub with_check: bool,
}

/// The trace being inserted: Java's `halfWidth`, `layer`, `netNumbers` and
/// `clearanceClassIndex`.
#[derive(Clone, Copy, Debug)]
pub struct ForcedTrace<'n> {
    pub half_width: i32,
    pub layer: i32,
    pub net_numbers: &'n [i32],
    pub clearance_class: i32,
}

impl RoutingBoard {
    /// Java `RoutingBoard.insertForcedTraceSegment(fromCorner, toCorner,
    /// halfWidth, layer, netNumbers, clearanceClassIndex, maxRecursionDepth,
    /// maxViaRecursionDepth, maxSpringOverRecursionDepth, tidyWidth,
    /// pullTightAccuracy, withCheck, workLimit)`: inserts a trace from
    /// `from` towards `to`, shoving the traces and vias in the way aside.
    /// [`ForcedInsert::point`] gives Java's result point.
    pub fn insert_forced_trace_segment(
        &mut self,
        from: &Point,
        to: &Point,
        trace: ForcedTrace<'_>,
        params: ForcedTraceParams,
        work_limit: Option<&mut WorkLimit>,
    ) -> ForcedInsert {
        if from == to {
            return ForcedInsert::Reached;
        }
        let polyline = Polyline::from_corners(from, to);
        self.insert_forced_trace_polyline(&polyline, trace, params, work_limit)
    }

    /// Java `RoutingBoard.insertForcedTracePolyline(polyline, halfWidth,
    /// layer, netNumbers, clearanceClassIndex, maxRecursionDepth,
    /// maxViaRecursionDepth, maxSpringOverRecursionDepth, tidyWidth,
    /// pullTightAccuracy, withCheck, workLimit)`: inserts a trace along
    /// `polyline`, shoving the traces and vias in the way aside, as far as
    /// the shove succeeds; then normalizes and pull-tightens it.
    ///
    /// Java's debug-only `pickItems` before and after the pull-tight are
    /// kept as queries without result, because their tree visits count on
    /// the work meter. A trace that `insertTraceWithoutCleaning` rejects
    /// after the shoves (Java's `NullPointerException` on `combine`) is
    /// [`ForcedInsert::DatabaseDamaged`].
    pub fn insert_forced_trace_polyline(
        &mut self,
        polyline: &Polyline,
        trace: ForcedTrace<'_>,
        params: ForcedTraceParams,
        mut work_limit: Option<&mut WorkLimit>,
    ) -> ForcedInsert {
        self.clear_shove_failing_obstacle();
        if polyline.lines().len() < 2 {
            // Java: a null corner, "degenerate polyline".
            return ForcedInsert::DatabaseDamaged;
        }
        let (from_corner, to_corner) = (polyline.first_corner(), polyline.last_corner());
        if from_corner == to_corner {
            return ForcedInsert::Reached;
        }
        let (Point::Int(_), Point::Int(to_int)) = (&from_corner, &to_corner) else {
            // Java warns "only implemented for IntPoints".
            return ForcedInsert::Unchanged;
        };
        let to_int = *to_int;
        let ForcedTrace {
            half_width,
            layer,
            net_numbers,
            clearance_class,
        } = trace;
        self.start_marking_changed_area();
        // An own-net trace ending at the start cuts the dog ears of the
        // check shape.
        let traces = ItemFilter::only(SelectableChoice::Traces);
        let picked = self.pick_items_filtered(&from_corner, layer, Some(&traces));
        let picked_trace = (picked.len() == 1)
            .then(|| *picked.iter().next().expect("one item"))
            .filter(|&id| {
                let item = self.item_ref(id);
                item.nets_equal_no(net_numbers)
                    && item.as_trace().is_some_and(|t| t.half_width == half_width)
                    && item.clearance_class() == clearance_class
            })
            .and_then(|id| self.item_ref(id).as_trace().map(|t| t.polyline.clone()));
        let compensated_half_width = half_width
            + self
                .search_trees()
                .default_tree()
                .clearance_compensation_value(&self.rules.clearance_matrix, clearance_class, layer);
        let mut shover = TraceShover::new(self);
        let mut new_polyline = match shover.spring_over_obstacles(
            polyline,
            compensated_half_width,
            layer,
            net_numbers,
            clearance_class,
            None,
        ) {
            SpringOver::Blocked => return ForcedInsert::Unchanged,
            SpringOver::NoObstacle => polyline.clone(),
            SpringOver::Wrapped(p) => p,
        };
        let combine = |p: &Polyline| match &picked_trace {
            Some(other) => p.combine(other),
            None => p.clone(),
        };
        let mut combined = combine(&new_polyline);
        if combined.lines().len() < 3 {
            return ForcedInsert::Unchanged;
        }
        let combined_lines = combined.lines().len();
        let start_shape_no = combined_lines - new_polyline.lines().len();
        let trace_shapes = combined.offset_shapes_range(
            compensated_half_width,
            start_shape_no,
            combined_lines - 1,
        );
        let shape_count = trace_shapes.len();
        let orthogonal =
            shover.board.rules.trace_angle_restriction() == AngleRestriction::NinetyDegree;
        let as_mode = |shape: &TileShape| {
            if orthogonal {
                TileShape::IntBox(shape.bounding_box())
            } else {
                shape.clone()
            }
        };
        let depths = params;
        let mut last_shape_no = shape_count;
        for (i, shape) in trace_shapes.iter().enumerate() {
            let current = as_mode(shape);
            let side_no = combined.corner_count() - shape_count - 1 + i;
            let from_side = ShapeEntrySide::from_polyline(&combined, side_no, &current);
            if params.with_check
                && !shover.check(
                    &current,
                    Some(from_side),
                    None,
                    layer,
                    net_numbers,
                    clearance_class,
                    depths.max_recursion_depth,
                    depths.max_via_recursion_depth,
                    depths.max_spring_over_recursion_depth,
                    work_limit.as_deref_mut(),
                )
            {
                last_shape_no = i;
                break;
            }
            if !shover.insert(
                &current,
                Some(from_side),
                layer,
                net_numbers,
                clearance_class,
                None,
                depths.max_recursion_depth,
                depths.max_via_recursion_depth,
                depths.max_spring_over_recursion_depth,
            ) {
                return ForcedInsert::DatabaseDamaged;
            }
        }
        let mut new_corner: Option<IntPoint> = None;
        if last_shape_no < shape_count {
            // The shove of shape `last_shape_no` failed: sample the shove
            // line to a shorter distance and try again.
            let mut last_trace_shape = as_mode(&trace_shapes[last_shape_no]);
            let sample_width = 2 * shover.board.min_trace_half_width();
            let last_corner = new_polyline.corner_approx(last_shape_no + 1);
            let prev_last_corner = new_polyline.corner_approx(last_shape_no);
            let last_segment_length = last_corner.distance(prev_last_corner);
            if last_segment_length > f64::from(100 * sample_width) {
                // too many cycles to sample
                return ForcedInsert::Unchanged;
            }
            let mut shape_index = combined.corner_count() - shape_count - 1 + last_shape_no;
            if last_segment_length > f64::from(sample_width) {
                let new_line_count = new_polyline.lines().len() - (shape_count - last_shape_no - 1);
                new_polyline = new_polyline.shorten(new_line_count, f64::from(sample_width));
                let Point::Int(current_last_corner) = new_polyline.last_corner() else {
                    // Java: "IntPoint expected".
                    return ForcedInsert::Unchanged;
                };
                new_corner = Some(current_last_corner);
                combined = combine(&new_polyline);
                if combined.lines().len() < 3 {
                    return ForcedInsert::Partial(current_last_corner);
                }
                shape_index = combined.lines().len() - 3;
                let Some(shape) = combined.offset_shape(compensated_half_width, shape_index) else {
                    // Unreachable: the index is the last segment's.
                    return ForcedInsert::Unchanged;
                };
                last_trace_shape = as_mode(&shape);
            }
            let from_side =
                ShapeEntrySide::from_polyline(&combined, shape_index, &last_trace_shape);
            if !shover.check(
                &last_trace_shape,
                Some(from_side),
                None,
                layer,
                net_numbers,
                clearance_class,
                depths.max_recursion_depth,
                depths.max_via_recursion_depth,
                depths.max_spring_over_recursion_depth,
                work_limit,
            ) {
                return ForcedInsert::Unchanged;
            }
            if !shover.insert(
                &last_trace_shape,
                Some(from_side),
                layer,
                net_numbers,
                clearance_class,
                None,
                depths.max_recursion_depth,
                depths.max_via_recursion_depth,
                depths.max_spring_over_recursion_depth,
            ) {
                return ForcedInsert::DatabaseDamaged;
            }
        }
        // insert the new trace segment
        for i in 0..new_polyline.corner_count() {
            self.join_changed_area(new_polyline.corner_approx(i), layer);
        }
        let Some(mut new_trace) = self.insert_trace_without_cleaning(
            new_polyline,
            layer,
            half_width,
            net_numbers,
            clearance_class,
            FixedState::Unfixed,
        ) else {
            return ForcedInsert::DatabaseDamaged;
        };
        self.combine_trace(new_trace);
        let keep_point = Point::Int(new_corner.unwrap_or(to_int));
        let tidy_width = params.tidy_width;
        let mut pull_params = PullTightParams::new(params.pull_tight_accuracy);
        if tidy_width < i32::MAX {
            let region = keep_point
                .surrounding_octagon()
                .enlarge(f64::from(tidy_width));
            pull_params.clip = Clip::from_tidy_region(region);
        }
        if params.max_recursion_depth <= 0 {
            pull_params.only_nets = net_numbers.to_vec();
        }
        pull_params.keep_point = Some((keep_point.clone(), layer));
        let mut tightener = Tightener::new(self, &pull_params);
        // Remove generated cycles, because otherwise pull-tight may not work
        // correctly. Java catches a normalization exception here; at the pin
        // the normalization has none (it returns false at its depth limit).
        let area = self.changed_area_on_layer(layer);
        let mut current_trace = Some(new_trace);
        if self.normalize_trace(new_trace, area.as_ref()) {
            self.split_at_keep_point(&tightener);
            // otherwise the new corner may no more be on the new trace after
            // optimizing
            let picked = self.pick_items_filtered(&keep_point, layer, Some(&traces));
            current_trace = picked.iter().next().copied();
            if let Some(id) = current_trace {
                new_trace = id;
            }
        }
        if !net_numbers.is_empty() {
            // Java's debug query `dbgBefore`: only its tree visits count.
            self.pick_items_filtered(&keep_point, layer, Some(&traces));
        }
        if tidy_width > 0 && current_trace.is_some() {
            self.pull_tight_trace(new_trace, &mut tightener);
        }
        if !net_numbers.is_empty() {
            // Java's debug query `dbgAfter`.
            self.pick_items_filtered(&keep_point, layer, Some(&traces));
        }
        match new_corner {
            Some(p) => ForcedInsert::Partial(p),
            None => ForcedInsert::Reached,
        }
    }

    /// Java `RoutingBoard.forcedVia(viaInfo, location, netNumbers,
    /// traceClearanceClassIndex, tracePenHalfwidthArr, maxRecursionDepth,
    /// maxViaRecursionDepth, tidyWidth, pullTightAccuracy,
    /// pullTightLimitMs)`: shoves the traces in the way aside, inserts a via
    /// of type `via_info` at `location` and pull-tightens around it. False if
    /// the shove failed; the board may then be damaged (undo it).
    ///
    /// The pull-tight clip is `location`'s surrounding octagon enlarged by
    /// `tidy_width` (the point octagon for 0), none for `i32::MAX`; only the
    /// own nets with `max_recursion_depth <= 0`. `pull_tight_limit_ms` is
    /// Java's work limit in nominal milliseconds (none if not positive).
    #[allow(clippy::too_many_arguments)]
    pub fn forced_via(
        &mut self,
        via_info: &ViaInfo,
        location: IntPoint,
        net_numbers: &[i32],
        trace_clearance_class: i32,
        trace_pen_half_widths: &[i32],
        max_recursion_depth: i32,
        max_via_recursion_depth: i32,
        tidy_width: i32,
        pull_tight_accuracy: i32,
        pull_tight_limit_ms: i64,
    ) -> bool {
        self.clear_shove_failing_obstacle();
        self.start_marking_changed_area();
        let result = TraceShover::new(self).insert_forced_via(
            via_info,
            location,
            net_numbers,
            trace_clearance_class,
            trace_pen_half_widths,
            max_recursion_depth,
            max_via_recursion_depth,
        );
        if result {
            let mut params = PullTightParams::new(pull_tight_accuracy);
            if tidy_width < i32::MAX {
                let clip = Point::Int(location)
                    .surrounding_octagon()
                    .enlarge(f64::from(tidy_width));
                params.clip = Clip::Octagon(clip);
            }
            if max_recursion_depth <= 0 {
                params.only_nets = net_numbers.to_vec();
            }
            params.limit = (pull_tight_limit_ms > 0).then_some(pull_tight_limit_ms);
            self.opt_changed_area(&params);
        }
        result
    }

    /// Java `RoutingBoard.moveDrillItem(drillItem, vector, maxRecursionDepth,
    /// maxViaRecursionDepth, tidyWidth, pullTightAccuracy,
    /// pullTightLimitMs)`: moves drill item `drill` by `vector`, shoving the
    /// traces and vias in the way aside, and pull-tightens the changed area.
    /// False if the move failed; the board may then be damaged (undo it).
    ///
    /// The shove-fixed traces touching the item are unfixed first, also when
    /// the move fails. Kept Java quirk (DIVERGENCES, "Java bugs kept on
    /// purpose"): `DrillItemMover.insert` grows only its own copy of the tidy
    /// region, so with a finite `tidy_width` the region stays
    /// `IntOctagon.EMPTY` and the pull-tight is skipped; only
    /// `tidy_width == i32::MAX` (no clip) pull-tightens after a move.
    #[allow(clippy::too_many_arguments)]
    pub fn move_drill_item(
        &mut self,
        drill: ItemId,
        vector: &Vector,
        max_recursion_depth: i32,
        max_via_recursion_depth: i32,
        tidy_width: i32,
        pull_tight_accuracy: i32,
        pull_tight_limit_ms: i64,
    ) -> bool {
        self.clear_shove_failing_obstacle();
        // unfix the connected shove fixed traces
        for contact in self.normal_contacts(drill) {
            if self.item(contact).map(|i| i.fixed_state()) == Some(FixedState::ShoveFixed) {
                self.set_fixed_state(contact, FixedState::Unfixed);
            }
        }
        let (mut tidy_region, calculate_tidy_region) = if tidy_width < i32::MAX {
            (Some(IntOctagon::EMPTY), tidy_width > 0)
        } else {
            (None, false)
        };
        self.start_marking_changed_area();
        if !TraceShover::new(self).insert_drill_item_move(
            drill,
            vector,
            max_recursion_depth,
            max_via_recursion_depth,
        ) {
            return false;
        }
        if calculate_tidy_region {
            tidy_region = tidy_region.map(|r| r.enlarge(f64::from(tidy_width)));
        }
        let mut params = PullTightParams::new(pull_tight_accuracy);
        params.clip = tidy_region.map_or(Clip::Unbounded, Clip::from_tidy_region);
        if max_recursion_depth <= 0 {
            params.only_nets = self
                .item(drill)
                .map(|i| i.net_numbers().to_vec())
                .unwrap_or_default();
        }
        params.limit = (pull_tight_limit_ms > 0).then_some(pull_tight_limit_ms);
        self.opt_changed_area(&params);
        true
    }
}
