//! Connection insertion (A06.1-A06.4): a located connection becomes copper.
//!
//! Port of `autoroute/path/FoundConnectionInserter.java` at `a773a70ec`:
//! `getInstance` (`:40-63`), `insertSegments` (`:65-142`), `insertTrace`
//! (`:158-354`), `insertFanoutMicroNeckdown` (`:486-554`), `insertNeckdown`
//! (`:556-568`), `tryNeckDown` (`:570-712`) and `insertVia` (`:714-816`).
//! Each trace is inserted corner by corner through the forced insert
//! ([`RoutingBoard::insert_forced_trace_polyline`], O04), which shoves the
//! obstacles aside and pulls the new trace tight; each layer change gets a
//! forced via ([`TraceShover::insert_forced_via`]).
//!
//! **Identity, not value.** Java compares the forced insert's result point
//! with the polyline's first and last corner objects by reference
//! (`:235, :242, :249, :295`) and the neck-down's results with its corner
//! arguments (`:560, :566, :649, :667, :690`). The port matches on
//! [`ForcedInsert`]'s variant instead (`// PARITY:` at each site); a point
//! of equal value never counts as the same corner.
//!
//! **Partial insertion stays** (A06 decisions): a trace that cannot be
//! completed still removes its trace stubs and restores
//! `pin_edge_to_turn_dist`, and leaves what it inserted on the board; the
//! insertion then fails.
//!
//! The opt-in `router.min_trace_width_um` floor of Java `46bf9e2ef` (after
//! the pin; ledger "Port: yes") is ported with the same semantics: the pin
//! neck-downs use [`AutorouteControl::neckdown_half_width`] and the
//! micro-neck-down skips narrower candidates. Unset, nothing changes.
//!
//! Java's unconditional trace logging (`if (true) FRLogger.trace(...)`) and
//! the fanout diagnostics of pins named `U27-*` are dropped.

use freerouting_geometry::{FloatPoint, IntPoint, Point, Polyline};

use super::locate::{Located, ResultItem};
use super::locate45::calculate_additional_corner;
use crate::autoroute::control::AutorouteControl;
use crate::board::items::ItemId;
use crate::board::selection_filter::{ItemFilter, SelectableChoice};
use crate::board::{RoutingBoard, TraceObject};
use crate::shove::{ForcedInsert, ForcedTrace, ForcedTraceParams, ShoveDepths, TraceShover};

/// Java's `tolerance` of `tryNeckDown` (`:592`).
const NECKDOWN_TOLERANCE: f64 = 2.0;

/// The corners where the inserted connection starts and ends (Java's
/// `firstCorner` and `lastCorner` of the inserter instance).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Inserted {
    /// The first corner of the first trace inserted (at the target item).
    pub first_corner: Option<IntPoint>,
    /// The last corner of the last trace inserted (at the start item).
    pub last_corner: Option<IntPoint>,
}

/// Why an insertion failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InsertError {
    /// A via or a trace could not be inserted (Java `getInstance` returns
    /// null). What was inserted before stays on the board.
    Failed,
    /// Java's `NullPointerException` in `insertVia` when a connection
    /// without traces changes the layer (`lastCorner` is null, `:113`).
    /// Java's `autorouteConnection` does not catch it; the batch router's
    /// catch turns it into `FAILED` (`AutorouteConnectionRouter.java:181`).
    NullCorner,
}

/// Java `FoundConnectionInserter.getInstance(connection, board, ctrl)`
/// (`:40-63`): inserts the traces and vias of `located`.
pub fn insert_located(
    board: &mut RoutingBoard,
    ctrl: &AutorouteControl,
    located: &Located,
) -> Result<Inserted, InsertError> {
    insert_segments(
        board,
        ctrl,
        located.start_item,
        located.target_item,
        located.start_layer,
        located.target_layer,
        &located.items,
    )
}

/// Java `FoundConnectionInserter.insertSegments` (`:65-142`): for each
/// segment a via from the current layer to its layer at its first corner,
/// then the trace; then a via to the start layer at the last corner; then
/// the connections to a target and a start trace; then the net's traces are
/// normalized.
pub fn insert_segments(
    board: &mut RoutingBoard,
    ctrl: &AutorouteControl,
    start_item: Option<ItemId>,
    target_item: Option<ItemId>,
    start_layer: i32,
    target_layer: i32,
    segments: &[ResultItem],
) -> Result<Inserted, InsertError> {
    // PARITY(quirk): Java holds the start and target items as objects
    // (`:108-137`); a trace the insertion splits or removes leaves the board,
    // and `connectToTrace` still connects to the removed object's geometry.
    let target_object = target_item.and_then(|id| board.trace_object(id));
    let start_object = start_item.and_then(|id| board.trace_object(id));
    let mut inserter = Inserter {
        board,
        ctrl,
        done: Inserted::default(),
    };
    let mut current_layer = target_layer;
    for segment in segments {
        let first = segment.corners.first().copied();
        if !inserter.insert_via(first, current_layer, segment.layer)? {
            return Err(InsertError::Failed);
        }
        current_layer = segment.layer;
        if !inserter.insert_trace(segment) {
            return Err(InsertError::Failed);
        }
    }
    let last = inserter.done.last_corner;
    if !inserter.insert_via(last, current_layer, start_layer)? {
        return Err(InsertError::Failed);
    }
    let done = inserter.done;
    let net = ctrl.net_number;
    let class = ctrl.trace_clearance_class_index;
    // PARITY: FoundConnectionInserter.java:113-140, the target trace gets
    // the start layer's half width and the start trace the target layer's.
    if let Some(trace) = trace_object_now(board, target_item, target_object)
        && let Some(corner) = done.first_corner
    {
        let half_width = half_width_at(ctrl, start_layer);
        board.connect_to_trace_object(corner, &trace, half_width, class);
    }
    if let Some(trace) = trace_object_now(board, start_item, start_object)
        && let Some(corner) = done.last_corner
    {
        let half_width = half_width_at(ctrl, target_layer);
        board.connect_to_trace_object(corner, &trace, half_width, class);
    }
    board.normalize_traces(net);
    Ok(done)
}

/// The Java object of start or target item `id`, a polyline trace when the
/// insertion began (`before`): its current geometry while it is on the
/// board (a combine changes the object in place), else the geometry it had
/// when the insertion began. Java's object keeps the geometry it had when
/// it left the board; a trace combined and then removed within one
/// insertion would differ, which no fixture shows.
fn trace_object_now(
    board: &RoutingBoard,
    id: Option<ItemId>,
    before: Option<TraceObject>,
) -> Option<TraceObject> {
    let before = before?;
    id.and_then(|id| board.trace_object(id)).or(Some(before))
}

fn half_width_at(ctrl: &AutorouteControl, layer: i32) -> i32 {
    usize::try_from(layer)
        .ok()
        .and_then(|l| ctrl.trace_half_width.get(l))
        .copied()
        .unwrap_or(0)
}

/// The insertion state (Java's `FoundConnectionInserter` instance).
struct Inserter<'a> {
    board: &'a mut RoutingBoard,
    ctrl: &'a AutorouteControl,
    done: Inserted,
}

impl Inserter<'_> {
    fn net_numbers(&self) -> [i32; 1] {
        [self.ctrl.net_number]
    }

    fn params(&self) -> ForcedTraceParams {
        ForcedTraceParams {
            max_recursion_depth: self.ctrl.max_shove_trace_recursion_depth,
            max_via_recursion_depth: self.ctrl.max_shove_via_recursion_depth,
            max_spring_over_recursion_depth: self.ctrl.max_spring_over_recursion_depth,
            tidy_width: i32::MAX,
            pull_tight_accuracy: self.ctrl.pull_tight_accuracy,
            with_check: true,
        }
    }

    /// `insertForcedTraceSegment(from, to, halfWidth, layer, ...)` with the
    /// control's depths, no tidy clip, a check and no work limit.
    fn forced_segment(
        &mut self,
        from: IntPoint,
        to: IntPoint,
        half_width: i32,
        layer: i32,
    ) -> ForcedInsert {
        let nets = self.net_numbers();
        let trace = ForcedTrace {
            half_width,
            layer,
            net_numbers: &nets,
            clearance_class: self.ctrl.trace_clearance_class_index,
        };
        let params = self.params();
        self.board.insert_forced_trace_segment(
            &Point::Int(from),
            &Point::Int(to),
            trace,
            params,
            None,
        )
    }

    /// Java `insertTrace` (`:158-354`): true if the whole trace was
    /// inserted.
    fn insert_trace(&mut self, trace: &ResultItem) -> bool {
        let corners = &trace.corners;
        if corners.len() == 1 {
            // A single-point trace: already at the target.
            if self.done.first_corner.is_none() {
                self.done.first_corner = Some(corners[0]);
            }
            self.done.last_corner = Some(corners[0]);
            return true;
        }
        // Switch off the pin exit correction while the trace is inserted
        // line by line (`:170-173`); restored on every path below.
        let saved_edge_to_turn_dist = self.board.rules.pin_edge_to_turn_dist();
        self.board.rules.set_pin_edge_to_turn_dist(-1.0);
        let (start_pin, end_pin) = self.end_pins(trace);
        let result = self.insert_corners(trace, start_pin, end_pin);
        let nets = self.net_numbers();
        for corner in &corners[..corners.len() - 1] {
            let tail = self
                .board
                .get_trace_tail(&Point::Int(*corner), trace.layer, &nets);
            if let Some(tail) = tail {
                // Java's trace line `compare_trace_stub_found` (`:441-458`) is
                // built eagerly and asks for the stub's start and end
                // contacts: the string goes, the tree visits stay (they count
                // on the work meter, D5).
                let _ = self.board.trace_start_contacts(tail);
                let _ = self.board.trace_end_contacts(tail);
                self.board.remove_item(tail);
            }
        }
        self.board
            .rules
            .set_pin_edge_to_turn_dist(saved_edge_to_turn_dist);
        if self.done.first_corner.is_none() {
            self.done.first_corner = corners.first().copied();
        }
        self.done.last_corner = corners.last().copied();
        result
    }

    /// The pins of the net centred on the first and the last corner, when
    /// neck-down is on (`:175-197`): picked with the PINS filter in set
    /// order (highest id first); a later match overwrites an earlier one.
    fn end_pins(&self, trace: &ResultItem) -> (Option<ItemId>, Option<ItemId>) {
        if !self.ctrl.with_neckdown {
            return (None, None);
        }
        let filter = ItemFilter::only(SelectableChoice::Pins);
        let net = self.ctrl.net_number;
        let pin_at = |corner: IntPoint| {
            let picked =
                self.board
                    .pick_items_filtered(&Point::Int(corner), trace.layer, Some(&filter));
            let mut found = None;
            for id in picked {
                let Some(item) = self.board.item(id) else {
                    continue;
                };
                let centred = item.drill().is_some_and(|d| d.center == corner);
                if item.contains_net(net) && centred {
                    found = Some(id);
                }
            }
            found
        };
        let corners = &trace.corners;
        (pin_at(corners[0]), pin_at(corners[corners.len() - 1]))
    }

    /// The corner loop of `insertTrace` (`:199-331`): false where Java sets
    /// `result = false` and breaks.
    fn insert_corners(
        &mut self,
        trace: &ResultItem,
        start_pin: Option<ItemId>,
        end_pin: Option<ItemId>,
    ) -> bool {
        let corners = &trace.corners;
        let layer = trace.layer;
        let half_width = half_width_at(self.ctrl, layer);
        let nets = self.net_numbers();
        let mut from_corner_no = 0usize;
        for i in 1..corners.len() {
            let current = &corners[from_corner_no..=i];
            let points: Vec<Point> = current.iter().map(|p| Point::Int(*p)).collect();
            let polyline = Polyline::from_points(&points);
            let forced = ForcedTrace {
                half_width,
                layer,
                net_numbers: &nets,
                clearance_class: self.ctrl.trace_clearance_class_index,
            };
            let params = self.params();
            let ok = self
                .board
                .insert_forced_trace_polyline(&polyline, forced, params, None);
            // Java's `okPoint` as a value: the polyline's first corner for
            // `Unchanged`, the new point for `Partial`, null when damaged.
            let ok_point = match ok {
                ForcedInsert::DatabaseDamaged => None,
                ForcedInsert::Unchanged => Some(current[0]),
                ForcedInsert::Reached => Some(current[current.len() - 1]),
                ForcedInsert::Partial(p) => Some(p),
            };
            let two_corners = current.len() == 2;
            let mut neckdown_inserted = false;
            // PARITY: FoundConnectionInserter.java:233-239, `okPoint != null
            // && okPoint != insertPolyline.lastCorner()` by reference.
            if matches!(ok, ForcedInsert::Unchanged | ForcedInsert::Partial(_))
                && self.ctrl.with_neckdown
                && two_corners
                && let Some(from) = ok_point
            {
                neckdown_inserted =
                    self.insert_neckdown(from, current[1], layer, start_pin, end_pin);
            }
            let mut micro_inserted = false;
            // PARITY: FoundConnectionInserter.java:240-247, any result but the
            // last corner object, a damaged database (null) included.
            if !neckdown_inserted
                && ok != ForcedInsert::Reached
                && (self.ctrl.is_fanout || self.ctrl.with_neckdown)
                && two_corners
            {
                micro_inserted = self
                    .insert_fanout_micro_neckdown(ok_point, current[1], layer, start_pin, end_pin);
            }
            // PARITY: FoundConnectionInserter.java:248 and :295, `okPoint ==
            // insertPolyline.lastCorner()` and `== firstCorner()` by reference.
            if ok == ForcedInsert::Reached || neckdown_inserted || micro_inserted {
                from_corner_no = i;
            } else if ok == ForcedInsert::Unchanged && i != corners.len() - 1 {
                // The spring over may have failed; a retry with more distant
                // corners may let it correct the situation (`:296-310`).
                if from_corner_no > 0 && current.len() < 3 {
                    from_corner_no -= 1;
                }
            } else {
                return false;
            }
        }
        true
    }

    /// Java `insertNeckdown` (`:556-568`): a neck-down into the start pin,
    /// else into the end pin. True if it reached its target corner.
    fn insert_neckdown(
        &mut self,
        from: IntPoint,
        to: IntPoint,
        layer: i32,
        start_pin: Option<ItemId>,
        end_pin: Option<ItemId>,
    ) -> bool {
        // PARITY: FoundConnectionInserter.java:560 and :566, `okPoint ==
        // fromCorner` / `== toCorner` by reference: only the last segment of
        // `tryNeckDown` reaching its end returns that object.
        if let Some(pin) = start_pin
            && self.try_neck_down(to, from, layer, pin)
        {
            return true;
        }
        if let Some(pin) = end_pin {
            return self.try_neck_down(from, to, layer, pin);
        }
        false
    }

    /// Java `tryNeckDown` (`:570-712`): runs the trace at full width as far
    /// as it can go towards `to`, then necks down to the pin's width for
    /// the rest. True where Java returns the `toCorner` object (the last
    /// segment reached `to`); Java's null and early `fromCorner` are false.
    fn try_neck_down(&mut self, from: IntPoint, to: IntPoint, layer: i32, pin: ItemId) -> bool {
        let Some(item) = self.board.item(pin) else {
            return false;
        };
        if !item.is_on_layer(layer) {
            return false;
        }
        let Some(center) = item.drill().map(|d| d.center) else {
            return false;
        };
        let pin_class = item.clearance_class();
        let clearance = f64::from(self.board.rules.clearance_matrix.get_value(
            self.ctrl.trace_clearance_class_index,
            pin_class,
            layer,
            true,
        ));
        let pin_neck_down_distance = 2.0 * (0.5 * self.board.pin_max_width(pin, layer) + clearance);
        let to_float = to.to_float();
        if center.to_float().distance(to_float) >= pin_neck_down_distance {
            return false;
        }
        let neck_down_half_width = self.ctrl.neckdown_half_width(self.board, pin, layer);
        let half_width = half_width_at(self.ctrl, layer);
        if neck_down_half_width >= half_width {
            return false;
        }
        let from_float = from.to_float();
        let nets = self.net_numbers();
        let mut ok_length = self.board.check_trace_segment(
            &Point::Int(from),
            &Point::Int(to),
            layer,
            &nets,
            half_width,
            self.ctrl.trace_clearance_class_index,
            true,
        );
        if ok_length >= f64::from(i32::MAX) {
            return false;
        }
        ok_length -= NECKDOWN_TOLERANCE;
        let neck_down_end = if ok_length <= NECKDOWN_TOLERANCE {
            from
        } else {
            match self.full_width_run(from_float, to_float, to, ok_length, half_width, layer) {
                Some(end) => end,
                None => return false,
            }
        };
        self.forced_segment(neck_down_end, to, neck_down_half_width, layer) == ForcedInsert::Reached
    }

    /// The full-width part of `tryNeckDown` (`:617-697`): from `from` to
    /// the point `ok_length` along the way to `to`, with a corner before
    /// and after it so that every segment keeps the angle restriction.
    /// The end of the full-width part, or `None` where Java returns
    /// `fromCorner` (a segment did not reach its end).
    fn full_width_run(
        &mut self,
        from_float: FloatPoint,
        to_float: FloatPoint,
        to: IntPoint,
        ok_length: f64,
        half_width: i32,
        layer: i32,
    ) -> Option<IntPoint> {
        let from = from_float.round();
        let end_float = from_float.change_length(to_float, ok_length);
        let mut end = end_float.round();
        let angle = self.board.rules.trace_angle_restriction();
        let horizontal_first =
            (from_float.x - end_float.x).abs() >= (from_float.y - end_float.y).abs();
        let add_corner =
            calculate_additional_corner(from_float, end_float, horizontal_first, angle).round();
        // PARITY: FoundConnectionInserter.java:649, `currentOkPoint !=
        // addCorner` by reference.
        if self.forced_segment(from, add_corner, half_width, layer) != ForcedInsert::Reached {
            return None;
        }
        // PARITY: FoundConnectionInserter.java:667.
        if self.forced_segment(add_corner, end, half_width, layer) != ForcedInsert::Reached {
            return None;
        }
        let add_corner =
            calculate_additional_corner(end_float, to_float, !horizontal_first, angle).round();
        if add_corner != to {
            // PARITY: FoundConnectionInserter.java:690.
            if self.forced_segment(end, add_corner, half_width, layer) != ForcedInsert::Reached {
                return None;
            }
            end = add_corner;
        }
        Some(end)
    }

    /// Java `insertFanoutMicroNeckdown` (`:486-554`): the last segment at a
    /// narrower width, trying the pins' neck-down widths and then 3/4, 3/5
    /// and 1/2 of the trace width. True if one reached `target`.
    fn insert_fanout_micro_neckdown(
        &mut self,
        ok_point: Option<IntPoint>,
        target: IntPoint,
        layer: i32,
        start_pin: Option<ItemId>,
        end_pin: Option<ItemId>,
    ) -> bool {
        let from = ok_point.unwrap_or(target);
        if from == target {
            return false;
        }
        let base_half_width = half_width_at(self.ctrl, layer);
        let pin_widths = [start_pin, end_pin].map(|pin| {
            pin.filter(|p| self.board.item(*p).is_some_and(|i| i.is_on_layer(layer)))
                .map(|p| self.ctrl.neckdown_half_width(self.board, p, layer))
        });
        let candidates = micro_neckdown_half_widths(pin_widths, base_half_width);
        for half_width in candidates {
            if half_width <= 0
                || half_width >= base_half_width
                || half_width < self.ctrl.min_trace_half_width
            {
                continue;
            }
            // PARITY: FoundConnectionInserter.java:524, `candidateOkPoint ==
            // targetPoint` by reference.
            if self.forced_segment(from, target, half_width, layer) == ForcedInsert::Reached {
                return true;
            }
        }
        false
    }

    /// Java `insertVia` (`:714-816`): no via when the layers are equal;
    /// else the first via of the control's rule whose padstack spans both
    /// layers and passes `ForcedViaInserter.check`, inserted with
    /// `ForcedViaInserter.insert`. Ok(false) if none fits or the insert
    /// failed.
    fn insert_via(
        &mut self,
        location: Option<IntPoint>,
        from_layer: i32,
        to_layer: i32,
    ) -> Result<bool, InsertError> {
        if from_layer == to_layer {
            return Ok(true);
        }
        let (from_layer, to_layer) = if from_layer < to_layer {
            (from_layer, to_layer)
        } else {
            (to_layer, from_layer)
        };
        let nets = self.net_numbers();
        let rules = &self.board.rules;
        let vias: Vec<_> = self
            .ctrl
            .via_rule_vias(rules)
            .iter()
            .map(|id| {
                // Q14: the net class's via clearance class under `net_class`.
                let mut info = rules.via_infos.get(*id).clone();
                info.clearance_class = self.ctrl.via_info_clearance_class(&info);
                info
            })
            .collect();
        let depths = ShoveDepths {
            recursion: self.ctrl.max_shove_trace_recursion_depth,
            via_recursion: self.ctrl.max_shove_via_recursion_depth,
        };
        let mut chosen = None;
        for via in vias {
            let Some(padstack) = self.board.library.padstacks.get(via.padstack) else {
                continue;
            };
            if padstack.from_layer() > from_layer || padstack.to_layer() < to_layer {
                continue;
            }
            // Java dereferences the missing location in the check.
            let location = location.ok_or(InsertError::NullCorner)?;
            let ok = TraceShover::new(self.board).check_via(
                &via,
                &Point::Int(location),
                &nets,
                depths,
                Some(&self.ctrl.trace_half_width),
                self.ctrl.trace_clearance_class_index,
            );
            if ok {
                chosen = Some((via, location));
                break;
            }
        }
        let Some((via, location)) = chosen else {
            return Ok(false);
        };
        Ok(TraceShover::new(self.board).insert_forced_via(
            &via,
            location,
            &nets,
            self.ctrl.trace_clearance_class_index,
            &self.ctrl.trace_half_width,
            self.ctrl.max_shove_trace_recursion_depth,
            self.ctrl.max_shove_via_recursion_depth,
        ))
    }
}

/// The candidate half widths of the micro-neck-down (`:493-501`), Java's
/// `LinkedHashSet<Integer>`: the start pin's and the end pin's neck-down
/// widths (where given), then `max(1, 3w/4)`, `max(1, 3w/5)` and
/// `max(1, w/2)` of the base half width `w`, each once, in first-insertion
/// order.
pub fn micro_neckdown_half_widths(pin_widths: [Option<i32>; 2], base_half_width: i32) -> Vec<i32> {
    let mut result: Vec<i32> = Vec::with_capacity(5);
    let mut add = |value: i32| {
        if !result.contains(&value) {
            result.push(value);
        }
    };
    for width in pin_widths.into_iter().flatten() {
        add(width);
    }
    // PARITY: Java int arithmetic, `(baseHalfWidth * 3) / 4` truncating.
    add(1.max(base_half_width.wrapping_mul(3) / 4));
    add(1.max(base_half_width.wrapping_mul(3) / 5));
    add(1.max(base_half_width / 2));
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn micro_neckdown_widths_keep_first_insertion_order_without_repeats() {
        // 3/4, 3/5 and 1/2 of 100.
        assert_eq!(
            micro_neckdown_half_widths([None, None], 100),
            vec![75, 60, 50]
        );
        // A pin width equal to a fraction comes first and is not repeated.
        assert_eq!(
            micro_neckdown_half_widths([Some(60), Some(40)], 100),
            vec![60, 40, 75, 50]
        );
        // Both pins with the same width: one entry.
        assert_eq!(
            micro_neckdown_half_widths([Some(30), Some(30)], 100),
            vec![30, 75, 60, 50]
        );
        // Tiny widths floor at 1, once.
        assert_eq!(micro_neckdown_half_widths([None, None], 1), vec![1]);
        assert_eq!(micro_neckdown_half_widths([None, None], 3), vec![2, 1]);
    }
}
