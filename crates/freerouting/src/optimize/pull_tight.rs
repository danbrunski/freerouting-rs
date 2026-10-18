//! The board entry points of pull-tight.
//!
//! Port of `PolylineTrace.pullTight(algo)` and `pullTight(ownNetOnly,
//! accuracy, stop)` (`board/trace/PolylineTrace.java:810-890`),
//! `TraceTightener.optChangedArea`, `smoothenEndCornersAtTrace(1, 2)` and
//! `splitTracesAtKeepPoint` (`TraceTightener.java:121-169, 402-513`), and
//! `RoutingBoardOperations.optChangedArea` and `removeItemsAndPullTight`
//! (`board/facade/RoutingBoardOperations.java:52-120`) at `a773a70ec`.
//!
//! With trace costs in the parameters, a via in the changed area is moved by
//! the via optimizer (`optimize::via`, O05); without them it is skipped, as
//! Java does with `traceCosts == null`.
//! `joinGraphicsUpdateBox` is cosmetic and dropped.
//!
//! Kept Java quirks (DIVERGENCES, "Java bugs kept on purpose"), each behind
//! its switch, `true` (Java) in both profiles (N06.8):
//! - `router.java_quirks.smooth_loses_trace`: the end-corner smoothing
//!   removes the trace before it inserts the smoothed one and doesn't put it
//!   back when the insertion fails (a polyline with fewer than two corners,
//!   or of length 0 and not user fixed; `TraceTightener.java:431-441`).
//!   `false` tests those conditions first and keeps the trace, so the
//!   rejected insert's item id is not taken either;
//! - `router.java_quirks.pull_tight_reused_lines_change`: `pull_tight_trace`
//!   returns true for a changed polyline even when `change_trace` finds the
//!   lines unchanged and returns early (`PolylineTrace.java:837-839`), so the
//!   end smoothing and the pin exits are skipped for that trace. `false`
//!   reports a change only when `change_trace` changed the trace.
//!
//! With statistics on, `pull_tight_smooth_lost_traces` counts the smoothed
//! inserts that fail and `pull_tight_reused_lines_flips` the pull-tight
//! results `change_trace` ignores, in either arm (N06.8's reachability
//! counters); `change_trace` counts its strict prefix or suffix case
//! (`trace_change_prefix_suffix_ignored`, `board::trace_ops`).
//!
//! N06.2 removes the duplicate removal of the old trace and restores saved
//! contact pins on a keep-point early return in both profiles.
//!
//! Fixed copper: `pull_tight_trace` and the end-corner smoothing return
//! before they touch a shove-fixed trace, as Java does, so they never move
//! fixed copper. The `normalize_traces` of the smoothed trace's nets after an
//! end-corner smoothing covers every trace of those nets, the fixed ones too,
//! exactly as Java's `normalizeTraces` does.
//!
//! The any-angle mode is `tighten::any_angle` (O02, `TraceTightenerAnyAngle`).

use std::collections::BTreeSet;

use freerouting_geometry::{IntOctagon, Polyline, PolylineShapeOps, TileShape};

use crate::autoroute::control::ExpansionCostFactor;
use crate::board::contact_rule::TJunctions;
use crate::board::items::ItemId;
use crate::board::selection_filter::{ItemFilter, SelectableChoice};
use crate::board::structure::{AngleRestriction, FixedState};
use crate::board::{ChangeOutcome, RoutingBoard};
use crate::optimize::params::{Clip, PullTightParams, REMOVE_ITEMS_PULL_TIGHT_NOMINAL_MS};
use crate::optimize::tighten::{Outcome, Tightener, TrackedPolyline};
use crate::optimize::via::OPT_VIA_RECURSION_DEPTH;
use crate::run_stats::{Counter, record_n06};

/// What [`RoutingBoard::opt_changed_area`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OptChangedAreaRun {
    /// True if the cancel token or the work limit stopped the run.
    pub stopped: bool,
    /// The stop checks made before each object of the scan and in the
    /// tightener's fixpoints.
    pub stop_checks: u64,
    /// The work units done since the limit started, at the last stop check
    /// (the cut-off if `stopped`); `None` without a limit or without a run.
    pub units_at_last_check: Option<u64>,
}

/// True if [`BasicBoard::insert_trace_without_cleaning`](crate::board::BasicBoard::insert_trace_without_cleaning)
/// rejects `polyline` with `fixed`: fewer than two corners, or zero length and
/// not at least user fixed (Java `BasicBoard.insertTraceWithoutCleaning`). The
/// end-corner smoothing asserts in debug builds that the two agree.
pub(crate) fn smoothed_insert_fails(polyline: &Polyline, fixed: FixedState) -> bool {
    polyline.corner_count() < 2
        || polyline.first_corner() == polyline.last_corner() && fixed < FixedState::UserFixed
}

/// What [`RoutingBoard::replace_smoothed_trace`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmoothedReplace {
    /// The smoothed trace, with this id, replaced the old one.
    Inserted(ItemId),
    /// Nothing changed: the insert would fail and
    /// `router.java_quirks.smooth_loses_trace = false` kept the old trace (or
    /// `id` is not a trace).
    Kept,
    /// The insert failed after the old trace was removed (Java's behaviour).
    Lost,
}

impl RoutingBoard {
    /// Pull-tightens trace `id` with `tightener`. True if the trace
    /// changed (Java `PolylineTrace.pullTight(TraceTightener)`).
    ///
    /// The gates come in Java's order: not on the board, shove-fixed, nets
    /// not normal, other nets than the tightener's `only_nets`, a net class
    /// with pull-tight off. Without a change, and if the angle restriction
    /// isn't 90° and `pin_edge_to_turn_dist > 0`, the pin exits are fixed
    /// (`swap_connection_to_pin` at the start, then the end, then
    /// `correct_connection_to_pin` at the start, then the end); each success
    /// pull-tightens the trace again and returns true.
    pub fn pull_tight_trace(&mut self, id: ItemId, tightener: &mut Tightener) -> bool {
        let Some(item) = self.item(id) else {
            return false;
        };
        let Some(trace) = item.as_trace() else {
            return false;
        };
        if !item.is_on_the_board() || self.is_shove_fixed(item) || !item.nets_normal() {
            return false;
        }
        if !tightener.only_nets().is_empty() && !item.nets_equal_no(tightener.only_nets()) {
            return false;
        }
        if let Some(&net) = item.net_numbers().first() {
            let pull_tight = self
                .rules
                .nets
                .get(net)
                .is_none_or(|n| self.rules.net_classes.get(n.net_class()).pull_tight());
            if !pull_tight {
                return false;
            }
        }
        // N04.4: under `inside_pad` a trace that other traces end on (a T
        // junction) is split there first, so that the junctions become end
        // contacts that pull-tight keeps; the pieces are pulled tight in
        // turn. One it cannot split there stays as it is.
        match self.t_junctions(id) {
            None => {}
            Some(TJunctions::Keep) => return false,
            Some(TJunctions::Split(points)) => {
                let Some(pieces) = self.split_trace_at_point(id, &points[0]) else {
                    return false;
                };
                for piece in pieces.into_iter().flatten() {
                    self.pull_tight_trace(piece, tightener);
                }
                return true;
            }
        }
        let (layer, half_width, class) = (trace.layer, trace.half_width, item.clearance_class());
        let polyline = trace.polyline.clone();
        let nets = item.net_numbers().to_vec();
        let pins = self.touching_pins_at_end_corners(id);
        tightener.set_trace(self, layer, half_width, &nets, class, Some(pins));
        let old_line_count = polyline.lines().len();
        if let Outcome::Changed(result) =
            tightener.pull_tight(self, TrackedPolyline::of_trace(polyline))
        {
            if self.apply_pull_tight_result(id, result, old_line_count) {
                return true;
            }
        }
        let angle = self.rules.trace_angle_restriction();
        if angle != AngleRestriction::NinetyDegree && self.rules.pin_edge_to_turn_dist() > 0.0 {
            let fixed = self.swap_connection_to_pin(id, true)
                || self.swap_connection_to_pin(id, false)
                // the optimizer couldn't improve the trace: remove acid traps
                || self.correct_connection_to_pin(id, true, angle)
                || self.correct_connection_to_pin(id, false, angle);
            if fixed {
                self.pull_tight_trace(id, tightener);
                return true;
            }
        }
        false
    }

    /// Pull-tightens trace `id` alone, without clip, limit or cancel token
    /// (Java `PolylineTrace.pullTight(true, accuracy, null)`). True if the
    /// trace changed.
    pub fn pull_tight_own_net(&mut self, id: ItemId, accuracy: i32) -> bool {
        let Some(item) = self.item(id) else {
            return false;
        };
        let mut params = PullTightParams::new(accuracy);
        params.only_nets = item.net_numbers().to_vec();
        let mut tightener = Tightener::new(self, &params);
        self.pull_tight_trace(id, &mut tightener)
    }

    /// Smoothens the acute angles of trace `id` with the traces at its
    /// ends. True if something changed (Java
    /// `TraceTightener.smoothenEndCornersAtTrace`).
    pub fn smoothen_end_corners_at_trace(&mut self, id: ItemId, tightener: &mut Tightener) -> bool {
        let Some(item) = self.item(id) else {
            return false;
        };
        if !tightener.only_nets().is_empty() && !item.nets_equal_no(tightener.only_nets()) {
            return false;
        }
        let Some(trace) = item.as_trace() else {
            return false;
        };
        let nets = item.net_numbers().to_vec();
        tightener.set_trace_plain(trace.layer, trace.half_width, &nets, item.clearance_class());
        if self.is_shove_fixed(item) {
            return false;
        }
        // allow the trace to slide to the end of a contact trace ending at a pin
        let saved_pins = tightener.take_contact_pins();
        let mut result = false;
        let mut current = id;
        loop {
            let Some(adjusted) = self.smoothen_end_corners_step(current, tightener) else {
                break;
            };
            let Some(layer) = self
                .item(current)
                .and_then(|i| i.as_trace())
                .map(|t| t.layer)
            else {
                break;
            };
            let replaced = self.replace_smoothed_trace(current, &adjusted, tightener.half_width());
            let SmoothedReplace::Inserted(new_id) = replaced else {
                // Java's loop ends here too; under Java's behaviour the old
                // trace stays removed.
                break;
            };
            result = true;
            current = new_id;
            let nets = self.item_ref(current).net_numbers().to_vec();
            for net in nets {
                self.split_traces(&adjusted.first_corner(), layer, net);
                self.split_traces(&adjusted.last_corner(), layer, net);
                // Java logs and ignores a normalization exception; the
                // port's normalization has no failure path. A stop request
                // may end it early.
                self.normalize_traces_until(net, tightener.stop_request());
                if self.split_at_keep_point(tightener) {
                    tightener.set_contact_pins(saved_pins);
                    return true;
                }
            }
        }
        tightener.set_contact_pins(saved_pins);
        result
    }

    /// Applies the pull-tight result `result` of trace `id` (which had
    /// `old_line_count` lines) with [`change_trace`](crate::board::BasicBoard::change_trace).
    /// True if pull-tight reports a change (the `newLines != lines` branch of
    /// Java `PolylineTrace.pullTight`).
    ///
    /// Under `router.java_quirks.pull_tight_reused_lines_change = true`
    /// (Java) that is always true, even when `change_trace` finds every line
    /// reused and returns early; `false` reports a change only when
    /// `change_trace` changed the trace. With statistics on, each ignored
    /// result counts `pull_tight_reused_lines_flips`, in either arm.
    pub fn apply_pull_tight_result(
        &mut self,
        id: ItemId,
        result: TrackedPolyline,
        old_line_count: usize,
    ) -> bool {
        let reuse = result.line_reuse(old_line_count);
        let outcome = self.change_trace(id, result.polyline, reuse);
        let ignored = outcome == ChangeOutcome::Unchanged;
        if ignored {
            record_n06(Counter::PullTightReusedLinesFlips);
        }
        // PARITY(quirk, java_quirks.pull_tight_reused_lines_change): Java
        // returns true even when `change` finds the lines unchanged, which
        // skips the end smoothing and the pin exits; `false` goes on to them.
        self.java_quirks().pull_tight_reused_lines_change || !ignored
    }

    /// Replaces trace `id` by a trace along `polyline` with `half_width` and
    /// the old trace's layer, nets, clearance class and fixed state: the
    /// `removeItem` and `insertTraceWithoutCleaning` of Java
    /// `TraceTightener.smoothenEndCornersAtTrace1` (`TraceTightener.java:431-441`).
    ///
    /// The insert fails for a polyline with fewer than two corners, or of
    /// length 0 and not user fixed. Java removes the trace first, so a failed
    /// insert loses it ([`SmoothedReplace::Lost`]);
    /// `router.java_quirks.smooth_loses_trace = false` tests those conditions
    /// first and keeps the trace ([`SmoothedReplace::Kept`]). With statistics
    /// on, each failing insert counts `pull_tight_smooth_lost_traces`, in
    /// either arm.
    pub fn replace_smoothed_trace(
        &mut self,
        id: ItemId,
        polyline: &Polyline,
        half_width: i32,
    ) -> SmoothedReplace {
        let Some(item) = self.item(id) else {
            return SmoothedReplace::Kept;
        };
        let Some(trace) = item.as_trace() else {
            return SmoothedReplace::Kept;
        };
        let (layer, class, fixed) = (trace.layer, item.clearance_class(), item.fixed_state());
        let nets = item.net_numbers().to_vec();
        // PARITY(quirk, java_quirks.smooth_loses_trace): Java removes the
        // trace before an insert that may fail, and the trace stays removed
        // (TraceTightener.java:431-441); `false` tests the insert's failure
        // conditions first and keeps the trace.
        let insert_fails = smoothed_insert_fails(polyline, fixed);
        if insert_fails {
            record_n06(Counter::PullTightSmoothLostTraces);
            if !self.java_quirks().smooth_loses_trace {
                return SmoothedReplace::Kept;
            }
        }
        self.remove_item(id);
        let inserted = self.insert_trace_without_cleaning(
            polyline.clone(),
            layer,
            half_width,
            &nets,
            class,
            fixed,
        );
        debug_assert_eq!(
            inserted.is_none(),
            insert_fails,
            "smoothed_insert_fails must match insert_trace_without_cleaning"
        );
        inserted.map_or(SmoothedReplace::Lost, SmoothedReplace::Inserted)
    }

    /// Java `smoothenEndCornersAtTrace2`: the smoothed polyline of trace
    /// `id`, start first; `None` if nothing changes.
    fn smoothen_end_corners_step(
        &mut self,
        id: ItemId,
        tightener: &mut Tightener,
    ) -> Option<Polyline> {
        if !self.item(id).is_some_and(|i| i.is_on_the_board()) {
            return None;
        }
        let layer = tightener.layer();
        let result = match tightener.smoothen_start_corner_at_trace(self, id) {
            Some(p) => {
                self.join_changed_area(p.corner_approx(0), layer);
                p
            }
            None => {
                let p = tightener.smoothen_end_corner_at_trace(self, id)?;
                self.join_changed_area(p.corner_approx(p.corner_count() - 1), layer);
                p
            }
        };
        let pins = self.touching_pins_at_end_corners(id);
        tightener.set_contact_pins(Some(pins));
        let tracked = TrackedPolyline::all_new(result);
        let skipped = tightener.skip_segments_of_length_0(self, &tracked);
        Some(skipped.or(tracked).polyline)
    }

    /// Splits the traces through the tightener's keep point. True at the
    /// first trace split (Java `TraceTightener.splitTracesAtKeepPoint`;
    /// the TRACES filter, highest id first).
    pub fn split_at_keep_point(&mut self, tightener: &Tightener) -> bool {
        let Some((point, layer)) = tightener.keep_point() else {
            return false;
        };
        let filter = ItemFilter::only(SelectableChoice::Traces);
        let picked = self.pick_items_filtered(point, *layer, Some(&filter));
        for id in picked {
            if self.split_trace_at_point(id, point).is_some() {
                return true;
            }
        }
        false
    }

    /// Optimizes the traces in the changed area, and with
    /// [`PullTightParams::trace_costs`] its vias, and stops marking it (Java
    /// `RoutingBoard.optChangedArea`).
    ///
    /// Returns at once when not marking. The tightener runs unless the clip
    /// is [`Clip::Skip`]; then the changed area is dropped.
    ///
    /// # The post-connection entry point (O05.4)
    ///
    /// The autorouter reproduces Java's three call patterns with
    /// [`PullTightParams::after_connection`]`(accuracy, trace_costs)` (all
    /// nets, [`Clip::Unbounded`], the router's pull-tight accuracy and trace
    /// costs, the
    /// [`ENDLESS_LOOP_GUARD_NOMINAL_MS`](crate::optimize::params::ENDLESS_LOOP_GUARD_NOMINAL_MS)
    /// work limit), with the
    /// router's cancel token in [`PullTightParams::stop`]:
    /// - after each routed connection (`AutorouteConnectionRouter.java:126-132`,
    ///   and `:258-264` after a neck-down connection), on the changed area
    ///   the insertion marked;
    /// - in `removeTails` (`BatchAutorouter.java:536-545`):
    ///   `start_marking_changed_area`, `remove_trace_tails`, then the same
    ///   call;
    /// - in fanout (`RoutingBoard.java:1136-1142`) and the interactive
    ///   autoroute of one item (`:975-981`): the same, with
    ///   [`PullTightParams::only_nets`] set to the routed net.
    ///
    /// ```
    /// use freerouting::drc::load_design;
    /// use freerouting::io::read_session;
    /// use freerouting::optimize::PullTightParams;
    /// use freerouting::settings::board::apply_board_specific;
    /// use freerouting::settings::{RouterSettings, default_patch};
    ///
    /// let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/");
    /// let dsn = std::fs::read(format!("{root}Issue508-DAC2020_bm08.dsn")).unwrap();
    /// let ses = std::fs::read(format!("{root}Issue508-DAC2020_bm08-routed.ses")).unwrap();
    /// let mut board = load_design(&dsn).unwrap();
    /// read_session(&mut board, &freerouting::dsn::decode(&ses)).unwrap();
    /// let mut patch = default_patch(1);
    /// apply_board_specific(&mut patch, &board);
    /// let costs = RouterSettings::from_patch(&patch, 1).trace_costs();
    /// assert_eq!(costs.len(), board.layer_count());
    ///
    /// board.start_marking_changed_area();
    /// board.mark_all_changed_area();
    /// let params = PullTightParams::after_connection(500, costs);
    /// let run = board.opt_changed_area(&params);
    /// assert!(!run.stopped);
    /// assert!(board.changed_area().is_none());
    /// ```
    pub fn opt_changed_area(&mut self, params: &PullTightParams) -> OptChangedAreaRun {
        if self.changed_area().is_none() {
            return OptChangedAreaRun::default();
        }
        let mut run = OptChangedAreaRun::default();
        if params.clip != Clip::Skip {
            let mut tightener = Tightener::new(self, params);
            let costs = params.trace_costs.clone();
            run.stopped = self.tighten_changed_area(&mut tightener, costs.as_deref());
            run.stop_checks = tightener.stop_checks();
            run.units_at_last_check = tightener.units_at_last_check();
        }
        self.clear_changed_area();
        run
    }

    /// Java `TraceTightener.optChangedArea`: repeats until nothing changes;
    /// per layer (ascending) with a non-empty area, empties it and scans a
    /// snapshot of the objects overlapping it, enlarged by 1.5 × (the
    /// largest clearance + twice the largest half width). True if a stop
    /// ended the run.
    ///
    /// With `trace_costs`, a via of the scan goes to
    /// [`opt_via_location`](Self::opt_via_location) with the tightener's
    /// `min_translate_dist` and depth [`OPT_VIA_RECURSION_DEPTH`]; a moved
    /// via marks a change but doesn't end the scan (Java
    /// `TraceTightener.java:156-161`).
    fn tighten_changed_area(
        &mut self,
        tightener: &mut Tightener,
        trace_costs: Option<&[ExpansionCostFactor]>,
    ) -> bool {
        let mut something_changed = true;
        while something_changed {
            something_changed = false;
            for layer in 0..self.layer_count() {
                let Some(area) = self.changed_area() else {
                    return false;
                };
                let region = area.get_area(layer);
                if region.is_empty() {
                    continue;
                }
                if let Some(area) = self.changed_area.as_mut() {
                    area.set_empty(layer);
                }
                let offset = 1.5
                    * f64::from(
                        self.rules.clearance_matrix.max_value_on_layer(layer as i32)
                            + 2 * self.rules.max_trace_half_width(),
                    );
                let region = region.enlarge(offset);
                let items = self.overlapping_objects(&TileShape::from(region), layer as i32);
                for id in items {
                    if tightener.is_stop_requested(self) {
                        return true;
                    }
                    let kind = self.item(id).map(|i| (i.is_trace(), i.is_via()));
                    let Some((is_trace, is_via)) = kind else {
                        continue;
                    };
                    if !is_trace {
                        if is_via
                            && trace_costs.is_some()
                            && self.opt_via_location(
                                id,
                                trace_costs,
                                tightener.min_translate_dist(),
                                OPT_VIA_RECURSION_DEPTH,
                            )
                        {
                            something_changed = true;
                        }
                        continue;
                    }
                    if self.pull_tight_trace(id, tightener) {
                        something_changed = true;
                        if self.split_at_keep_point(tightener) {
                            break;
                        }
                    } else if self.smoothen_end_corners_at_trace(id, tightener) {
                        something_changed = true;
                        // items may have been removed
                        break;
                    }
                }
            }
        }
        false
    }

    /// Removes the items `ids` and pull-tightens the traces around them.
    /// False if an item could not be removed because it is fixed or its
    /// deletion is forbidden (Java `removeItemsAndPullTight`).
    ///
    /// A finite `tidy_width` restricts the optimization to the removed
    /// shapes' bounding octagons enlarged by it, and a width of 0 skips it;
    /// `i32::MAX` optimizes without restriction. The changed nets are
    /// combined in ascending order first; the limit is
    /// [`REMOVE_ITEMS_PULL_TIGHT_NOMINAL_MS`].
    pub fn remove_items_and_pull_tight(
        &mut self,
        ids: &[ItemId],
        tidy_width: i32,
        accuracy: i32,
    ) -> bool {
        let mut result = true;
        let (mut tidy_region, calculate) = if tidy_width < i32::MAX {
            (Some(IntOctagon::EMPTY), tidy_width > 0)
        } else {
            (None, false)
        };
        self.start_marking_changed_area();
        let mut changed_nets = BTreeSet::new();
        for &id in ids {
            let Some(item) = self.item(id) else {
                continue;
            };
            if self.is_deletion_forbidden(item) || item.is_user_fixed() {
                result = false;
                continue;
            }
            let shapes: Vec<(TileShape, i32)> = (0..self.tile_shape_count(item))
                .filter_map(|i| Some((self.tile_shape(item, i)?, item.shape_layer(i))))
                .collect();
            let nets = item.net_numbers().to_vec();
            for (shape, layer) in &shapes {
                if let Some(area) = self.changed_area.as_mut() {
                    area.join_shape(shape, *layer as usize);
                }
                if calculate
                    && let (Some(region), Some(o)) = (tidy_region, shape.bounding_octagon())
                {
                    tidy_region = Some(region.union_octagon(&o));
                }
            }
            changed_nets.extend(nets);
            self.remove_item(id);
        }
        for net in changed_nets {
            self.combine_traces(net);
        }
        if calculate {
            tidy_region = tidy_region.map(|r| r.enlarge(f64::from(tidy_width)));
        }
        let mut params = PullTightParams::new(accuracy);
        params.clip = tidy_region.map_or(Clip::Unbounded, Clip::from_tidy_region);
        params.limit = Some(REMOVE_ITEMS_PULL_TIGHT_NOMINAL_MS);
        self.opt_changed_area(&params);
        result
    }
}
