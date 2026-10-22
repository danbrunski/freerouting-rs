//! The via optimizer: moves a via along the cheaper trace direction and
//! pulls plane and fanout vias towards the trace corner (O05).
//!
//! Port of `board/optimize/ViaOptimizer.java` at `a773a70ec`:
//! `optViaLocation` (`:33-158`), `optPlaneOrFanoutVia` (`:161-300`), the three
//! `repositionVia` overloads (`:302-365`, `:367-429`, `:435-713`) and
//! `isWithinTolerance` (`:719`). [`RoutingBoard::opt_changed_area`] calls
//! [`RoutingBoard::opt_via_location`] for every via of the scan when the
//! parameters carry trace costs (`TraceTightener.optChangedArea`, `:156-161`).
//!
//! The orders that decide results are Java's:
//! - the normal contacts of the via, highest id first; one contact makes it a
//!   plane or fanout via, two must be unfixed traces (a conduction area among
//!   them makes it the plane case), any other count leaves the via alone;
//! - the from-corner of a trace is its corner 1 when its first corner is
//!   within the tolerance `(int)(min_width / 2) + 1` (Manhattan distance) of
//!   the via centre, else its corner `count − 2` when its last corner is;
//! - the candidate locations of the cost-based `repositionVia` in Java's
//!   order, with every floating-point expression in Java's operation order;
//! - after a move ([`TraceShover::insert_drill_item_move`] with depths 9/9),
//!   the traces picked at the new location on the first trace's layer, then
//!   the second's, are pull-tightened alone
//!   ([`RoutingBoard::pull_tight_own_net`], no limit), then the first via
//!   picked there (highest id) is optimized with one recursion level less.
//!
//! The nested pull-tights have no limit, as Java's `pullTight(true, accuracy,
//! null)`, but their search-tree visits land on the board's work meter, so
//! the caller's limit sees them at its next check (Java's meter is per
//! thread and counts them the same way).
//!
//! **Beyond Java (K15.18).** [`RoutingBoard::opt_via_location_keeping_planes`]
//! with `keep_off_new_planes` (the pull-tight's
//! [`keep_vias_off_new_planes`](crate::optimize::PullTightParams::keep_vias_off_new_planes),
//! set under `router.power_layer_traces = own_plane | none`) leaves a via
//! between two traces where it is when the move would put it on a plane of
//! its net that it does not touch, on a layer neither trace is on (a trace
//! on the plane's layer runs into the plane, and the move replaces it, as in
//! Java). Under those modes a connection to a
//! plane cut by a window (K15.15) can run on two signal layers to a via in
//! the plane, through a via in the window. Moving the via in the window into
//! the plane makes a cycle through the plane while the drill move inserts
//! the first contact trace's stub (`move_drill_item`, layers descending),
//! and the cycle removal (`remove_if_cycle`, Java's) takes the whole
//! connection, both vias included, off the board. Java's rule (`layer`)
//! routes such a connection on the power layer and keeps Java's moves
//! (DIVERGENCES, "Autorouter").
//!
//! Java logs a warning when the move fails after a passed check; the port
//! returns false at the same point without the log line. A trace-cost array
//! shorter than a contact trace's layer (Java throws
//! `ArrayIndexOutOfBoundsException`; the router always passes one entry per
//! layer) leaves the via unchanged (DIVERGENCES, "Pull-tight").

use freerouting_geometry::jmath::{d2i, min_f64};
use freerouting_geometry::{FloatLine, FloatPoint, IntPoint, Point, Side, Vector};

use crate::autoroute::control::ExpansionCostFactor;
use crate::board::RoutingBoard;
use crate::board::items::ItemId;
use crate::board::selection_filter::{ItemFilter, SelectableChoice};
use crate::board::structure::AngleRestriction;
use crate::shove::{FULL_OK_LENGTH, TraceShover};

/// The recursion depth `TraceTightener.optChangedArea` passes to
/// [`RoutingBoard::opt_via_location`].
pub const OPT_VIA_RECURSION_DEPTH: i32 = 10;

/// The shove depths of the via move (`DrillItemMover.insert(via, delta, 9,
/// 9, …)`).
const MOVE_DEPTH: i32 = 9;

/// The trace costs Java uses without a cost array (`new
/// ExpansionCostFactor(1, 1)`).
const UNIT_COSTS: ExpansionCostFactor = ExpansionCostFactor {
    horizontal: 1.0,
    vertical: 1.0,
};

/// One trace at the via: its data and the corner next to the via.
#[derive(Clone, Copy)]
struct ViaTrace {
    half_width: i32,
    class: i32,
    layer: i32,
    costs: ExpansionCostFactor,
}

/// Java `ViaOptimizer.isWithinTolerance`: the Manhattan distance of the two
/// points (as doubles) is at most `tolerance`.
fn is_within_tolerance(p1: &Point, p2: &Point, tolerance: i32) -> bool {
    let (f1, f2) = (p1.to_float(), p2.to_float());
    let dx = (f1.x - f2.x).abs();
    let dy = (f1.y - f2.y).abs();
    dx + dy <= f64::from(tolerance)
}

impl RoutingBoard {
    /// Optimizes the location of via `via` connected to at most two traces
    /// according to the trace costs of their layers (`None`: horizontal and
    /// vertical cost 1). False if the via was not changed (Java
    /// `ViaOptimizer.optViaLocation(board, via, traceCosts, accuracy,
    /// maxRecursionDepth)`).
    ///
    /// `accuracy` is the pull-tight accuracy of the nested pull-tights;
    /// [`RoutingBoard::opt_changed_area`] passes the tightener's
    /// `min_translate_dist` and depth [`OPT_VIA_RECURSION_DEPTH`]. The work
    /// of the nested pull-tights counts on the board's work meter.
    pub fn opt_via_location(
        &mut self,
        via: ItemId,
        trace_costs: Option<&[ExpansionCostFactor]>,
        accuracy: i32,
        max_recursion_depth: i32,
    ) -> bool {
        self.opt_via_location_keeping_planes(via, trace_costs, accuracy, max_recursion_depth, false)
    }

    /// [`opt_via_location`](Self::opt_via_location); with
    /// `keep_off_new_planes` (K15.18, not Java) a via between two traces is
    /// not moved to a location where it would lie on a conduction area of
    /// its net that it does not touch, on a layer neither trace is on
    /// ([`via_would_join_a_plane`](Self::via_would_join_a_plane)),
    /// in this call and in its recursion. False is Java's
    /// `optViaLocation`. The plane and fanout case is unchanged: it already
    /// keeps a plane via on its plane.
    pub fn opt_via_location_keeping_planes(
        &mut self,
        via: ItemId,
        trace_costs: Option<&[ExpansionCostFactor]>,
        accuracy: i32,
        max_recursion_depth: i32,
        keep_off_new_planes: bool,
    ) -> bool {
        let Some(item) = self.item(via).filter(|i| i.is_via()) else {
            return false;
        };
        if self.is_shove_fixed(item) || max_recursion_depth <= 0 {
            return false;
        }
        let contacts: Vec<ItemId> = self.normal_contacts(via).into_iter().collect();
        let mut plane_or_fanout = contacts.len() == 1;
        let mut traces = [None, None];
        if !plane_or_fanout {
            if contacts.len() != 2 {
                return false;
            }
            for (slot, id) in traces.iter_mut().zip(&contacts) {
                let contact = self.item_ref(*id);
                if self.is_shove_fixed(contact) || !contact.is_trace() {
                    if contact.is_conduction_area() {
                        plane_or_fanout = true;
                    } else {
                        return false;
                    }
                } else {
                    *slot = Some(*id);
                }
            }
        }
        if plane_or_fanout {
            return self.opt_plane_or_fanout_via(via, accuracy, max_recursion_depth);
        }
        let [Some(first), Some(second)] = traces else {
            return false;
        };
        let center = self.via_center(via);
        let tolerance = self.via_tolerance(via);
        let Some(first_from) = self.trace_from_corner(first, &center, tolerance) else {
            return false;
        };
        let Some(second_from) = self.trace_from_corner(second, &center, tolerance) else {
            return false;
        };
        let (Some(t1), Some(t2)) = (
            self.via_trace(first, trace_costs),
            self.via_trace(second, trace_costs),
        ) else {
            return false;
        };
        let new_location =
            match self.reposition_via_by_costs(via, t1, &first_from, t2, &second_from) {
                Some(location) if location != center => location,
                _ => return false,
            };
        if keep_off_new_planes
            && self.via_would_join_a_plane(via, &new_location, &[t1.layer, t2.layer])
        {
            return false;
        }
        let delta = new_location.difference_by(&center);
        if !TraceShover::new(self).insert_drill_item_move(via, &delta, MOVE_DEPTH, MOVE_DEPTH) {
            return false;
        }
        let traces_filter = ItemFilter::only(SelectableChoice::Traces);
        for layer in [t1.layer, t2.layer] {
            for id in self.pick_items_filtered(&new_location, layer, Some(&traces_filter)) {
                self.pull_tight_own_net(id, accuracy);
            }
        }
        let vias_filter = ItemFilter::only(SelectableChoice::Vias);
        let picked = self.pick_items_filtered(&new_location, t1.layer, Some(&vias_filter));
        if let Some(&next) = picked.iter().next() {
            self.opt_via_location_keeping_planes(
                next,
                trace_costs,
                accuracy,
                max_recursion_depth - 1,
                keep_off_new_planes,
            );
        }
        true
    }

    /// Java `ViaOptimizer.optPlaneOrFanoutVia`: moves a via with one
    /// connected trace (and at most one plane) towards the trace's corner
    /// next to it, or onto the projection onto the trace's previous line.
    fn opt_plane_or_fanout_via(&mut self, via: ItemId, accuracy: i32, depth: i32) -> bool {
        if depth <= 0 {
            return false;
        }
        let contacts = self.normal_contacts(via);
        if contacts.is_empty() {
            return false;
        }
        let (mut plane, mut trace) = (None, None);
        for id in contacts {
            let contact = self.item_ref(id);
            if contact.is_conduction_area() {
                if plane.is_some() {
                    return false;
                }
                plane = Some(id);
            } else if contact.is_trace() {
                if self.is_shove_fixed(contact) || trace.is_some() {
                    return false;
                }
                trace = Some(id);
            } else {
                return false;
            }
        }
        let Some(trace) = trace else {
            return false;
        };
        let center = self.via_center(via);
        let tolerance = self.via_tolerance(via);
        let polyline = self
            .item_ref(trace)
            .as_trace()
            .expect("a trace")
            .polyline
            .clone();
        let at_first_corner = if is_within_tolerance(&polyline.first_corner(), &center, tolerance) {
            true
        } else if is_within_tolerance(&polyline.last_corner(), &center, tolerance) {
            false
        } else {
            return false;
        };
        let count = polyline.corner_count();
        let check_corner = if at_first_corner {
            polyline.corner(1)
        } else {
            polyline.corner(count - 2)
        };
        let Some(t) = self.via_trace(trace, None) else {
            return false;
        };
        let rounded = check_corner.to_float().round();
        let mut new_location = self.reposition_via_toward(via, rounded, t);
        if new_location.is_none() && count >= 3 {
            let prev_corner = if at_first_corner {
                polyline.corner(2)
            } else {
                polyline.corner(count - 3)
            };
            new_location = self.project_via_to_line(via, &check_corner, &prev_corner, t);
        }
        let Some(new_location) = new_location else {
            return false;
        };
        if let Some(plane) = plane {
            let Some(layer) = self.item_ref(plane).as_conduction_area().map(|a| a.layer()) else {
                return false;
            };
            let filter = ItemFilter::only(SelectableChoice::Conduction);
            if !self
                .pick_items_filtered(&new_location, layer, Some(&filter))
                .contains(&plane)
            {
                return false;
            }
        }
        let delta = new_location.difference_by(&center);
        if !TraceShover::new(self).insert_drill_item_move(via, &delta, MOVE_DEPTH, MOVE_DEPTH) {
            return false;
        }
        let filter = ItemFilter::only(SelectableChoice::Traces);
        for id in self.pick_items_filtered(&new_location, t.layer, Some(&filter)) {
            self.pull_tight_own_net(id, accuracy);
        }
        if new_location == check_corner {
            self.opt_plane_or_fanout_via(via, accuracy, depth - 1);
        }
        true
    }

    /// True if via `via`, moved to `location`, would lie on a conduction
    /// area of one of its nets that it does not touch now, on one of its
    /// layers other than `trace_layers`, the layers of its two contact
    /// traces (K15.18). A plane on a contact trace's layer does not count:
    /// there the trace runs to the plane, and the move into the plane
    /// replaces it (Q13's fixtures, `tests/power_layer_traces.rs`). The area
    /// is picked as the via optimizer's plane check picks it
    /// (`pick_items_filtered`, the area's own tile shapes), so a window cut
    /// into a plane counts as off the plane.
    pub fn via_would_join_a_plane(
        &self,
        via: ItemId,
        location: &Point,
        trace_layers: &[i32],
    ) -> bool {
        let Some(item) = self.item(via) else {
            return false;
        };
        let Some(drill) = item.drill() else {
            return false;
        };
        let touched = self.normal_contacts(via);
        let filter = ItemFilter::only(SelectableChoice::Conduction);
        let layers = (drill.first_layer..=drill.last_layer).filter(|l| !trace_layers.contains(l));
        layers.into_iter().any(|layer| {
            self.pick_items_filtered(location, layer, Some(&filter))
                .into_iter()
                .any(|id| {
                    !touched.contains(&id) && self.item(id).is_some_and(|a| a.shares_net(item))
                })
        })
    }

    /// The perpendicular projection of the via centre onto the line from
    /// `check_corner` to `prev_corner`, if it is a valid via move
    /// (`optPlaneOrFanoutVia`, `:218-260`).
    fn project_via_to_line(
        &mut self,
        via: ItemId,
        check_corner: &Point,
        prev_corner: &Point,
        t: ViaTrace,
    ) -> Option<Point> {
        let center = self.via_center(via);
        let float_check = check_corner.to_float();
        let float_center = center.to_float();
        let float_prev = prev_corner.to_float();
        if float_check.scalar_product(float_center, float_prev) == 0.0 {
            return None;
        }
        let line = FloatLine::new(float_check, float_prev);
        let projection = Point::Int(line.perpendicular_projection(float_center).round());
        let diff = projection.difference_by(&center);
        let angle = self.rules.trace_angle_restriction();
        if projection == center
            || angle == AngleRestriction::NinetyDegree && !diff.is_orthogonal()
            || angle == AngleRestriction::FortyfiveDegree && !diff.is_multiple_of_45_degree()
        {
            return None;
        }
        if !self.check_via_move(via, &diff) {
            return None;
        }
        let nets = self.item_ref(via).net_numbers().to_vec();
        let ok_length = self.check_trace_segment(
            &center,
            &projection,
            t.layer,
            &nets,
            t.half_width,
            t.class,
            false,
        );
        (ok_length >= FULL_OK_LENGTH).then_some(projection)
    }

    /// Java `repositionVia(board, via, toLocation, traceHalfWidth,
    /// traceLayer, traceClClass)` (`:302-365`): moves the via towards
    /// `to_location` as far as a trace of `t` and the via move allow. The
    /// new location, or `None` if no move is possible.
    fn reposition_via_toward(
        &mut self,
        via: ItemId,
        to_location: IntPoint,
        t: ViaTrace,
    ) -> Option<Point> {
        let from = self.via_center(via);
        let to = Point::Int(to_location);
        if from == to {
            return None;
        }
        let nets = self.item_ref(via).net_numbers().to_vec();
        let ok_length =
            self.check_trace_segment(&from, &to, t.layer, &nets, t.half_width, t.class, false);
        if ok_length <= 0.0 {
            return None;
        }
        let float_from = from.to_float();
        let float_to = to.to_float();
        let new_float_to = if ok_length >= FULL_OK_LENGTH {
            float_to
        } else {
            float_from.change_length(float_to, ok_length)
        };
        let new_to = Point::Int(new_float_to.round());
        if self.check_via_move(via, &new_to.difference_by(&from)) {
            return Some(new_to);
        }
        let min_length = 0.3 * f64::from(t.half_width) + 1.0;
        let ok_length = min_f64(ok_length, float_from.distance(float_to));
        let mut current_length = ok_length / 2.0;
        let mut ok_length = 0.0;
        let mut result = None;
        while current_length >= min_length {
            let check_point = Point::Int(
                float_from
                    .change_length(float_to, ok_length + current_length)
                    .round(),
            );
            if self.check_via_move(via, &check_point.difference_by(&from)) {
                ok_length += current_length;
                result = Some(check_point);
            }
            current_length /= 2.0;
        }
        result
    }

    /// Java `repositionVia(board, via, toLocation, …1, connectLocation, …2)`
    /// (`:367-429`): can the via move to `to_location`, with a trace of `t1`
    /// from the centre to it and one of `t2` from it to
    /// `connect_location`?
    fn reposition_via_with_connection(
        &mut self,
        via: ItemId,
        to_location: IntPoint,
        t1: ViaTrace,
        connect_location: IntPoint,
        t2: ViaTrace,
    ) -> bool {
        let from = self.via_center(via);
        let to = Point::Int(to_location);
        if from == to {
            return false;
        }
        let delta = to.difference_by(&from);
        if self.rules.trace_angle_restriction() == AngleRestriction::None
            && delta.length_approx() <= 1.5
        {
            // the any-angle reduce_corners may not remove the overlap this
            // would create, and the move would repeat forever
            return false;
        }
        let nets = self.item_ref(via).net_numbers().to_vec();
        let ok =
            self.check_trace_segment(&from, &to, t1.layer, &nets, t1.half_width, t1.class, false);
        if ok < FULL_OK_LENGTH {
            return false;
        }
        let connect = Point::Int(connect_location);
        let ok = self.check_trace_segment(
            &to,
            &connect,
            t2.layer,
            &nets,
            t2.half_width,
            t2.class,
            false,
        );
        if ok < FULL_OK_LENGTH {
            return false;
        }
        self.check_via_move(via, &delta)
    }

    /// Java `repositionVia(board, via, firstTrace…, secondTrace…)`
    /// (`:435-713`): a better location for a via between two traces
    /// according to the trace costs; `None` if there is none.
    fn reposition_via_by_costs(
        &mut self,
        via: ItemId,
        t1: ViaTrace,
        first_from: &Point,
        t2: ViaTrace,
        second_from: &Point,
    ) -> Option<Point> {
        let via_location = self.via_center(via);
        let first_delta = first_from.difference_by(&via_location);
        let second_delta = second_from.difference_by(&via_location);
        let scalar_product = first_delta.scalar_product(&second_delta);

        let fv = via_location.to_float();
        let f1 = first_from.to_float();
        let f2 = second_from.to_float();
        let first_distance = fv.distance(f1);
        let second_distance = fv.distance(f2);
        let r1 = f1.round();
        let r2 = f2.round();

        // overlapping lines first
        if via_location.side_of(first_from, second_from) == Side::Collinear && scalar_product > 0.0
        {
            if second_distance < first_distance {
                return self.reposition_via_toward(via, r2, t1);
            }
            return self.reposition_via_toward(via, r1, t2);
        }
        let (c1, c2) = (t1.costs, t2.costs);
        let wd1 = fv.weighted_distance(f1, c1.horizontal, c1.vertical);
        let wd2 = fv.weighted_distance(f1, c2.horizontal, c2.vertical);
        if wd1 > wd2 {
            // move the via towards the first trace's corner
            if let Some(result) = self.reposition_via_toward(via, r1, t2) {
                return Some(result);
            }
        }
        let wd1 = fv.weighted_distance(f2, c2.horizontal, c2.vertical);
        let wd2 = fv.weighted_distance(f2, c1.horizontal, c1.vertical);
        if wd1 > wd2 {
            // move the via towards the second trace's corner
            if let Some(result) = self.reposition_via_toward(via, r2, t1) {
                return Some(result);
            }
        }
        if scalar_product > 0.0
            && self.rules.trace_angle_restriction() != AngleRestriction::NinetyDegree
        {
            if let Some(result) = self.reposition_acute_angle(
                via,
                (t1, f1, r1, first_distance),
                (t2, f2, r2, second_distance),
            ) {
                return Some(result);
            }
        }
        // decomposition into axis-parallel parts
        if !first_delta.is_orthogonal()
            && let Some(location) = self.reposition_axis_parallel(via, fv, f1, r1, t1, t2)
        {
            return Some(location);
        }
        if !second_delta.is_orthogonal()
            && let Some(location) = self.reposition_axis_parallel(via, fv, f2, r2, t2, t1)
        {
            return Some(location);
        }
        None
    }

    /// The acute-angle branch of the cost-based `repositionVia`
    /// (`:528-579`): both from-corners at the nearer one's distance.
    fn reposition_acute_angle(
        &mut self,
        via: ItemId,
        first: (ViaTrace, FloatPoint, IntPoint, f64),
        second: (ViaTrace, FloatPoint, IntPoint, f64),
    ) -> Option<Point> {
        let (t1, f1, r1, first_distance) = first;
        let (t2, f2, r2, second_distance) = second;
        let fv = self.via_center(via).to_float();
        let (to1, float_to1, to2, float_to2);
        if first_distance < second_distance {
            to1 = r1;
            float_to1 = f1;
            float_to2 = fv.change_length(f2, first_distance);
            to2 = float_to2.round();
        } else {
            float_to1 = fv.change_length(f1, second_distance);
            to1 = float_to1.round();
            to2 = r2;
            float_to2 = f2;
        }
        let (c1, c2) = (t1.costs, t2.costs);
        let wd1 = float_to1.weighted_distance(float_to2, c1.horizontal, c1.vertical);
        let wd2 = float_to1.weighted_distance(float_to2, c2.horizontal, c2.vertical);
        if wd1 > wd2 {
            // first towards to1
            self.reposition_via_toward(via, to1, t2)
                .or_else(|| self.reposition_via_toward(via, to2, t1))
        } else {
            // first towards to2
            self.reposition_via_toward(via, to2, t1)
                .or_else(|| self.reposition_via_toward(via, to1, t2))
        }
    }

    /// One trace's decomposition into axis-parallel parts (`:583-643` for
    /// the first trace, `:645-711` for the second with the roles swapped):
    /// `own` is the trace whose from-corner `f_own` (rounded `r_own`) is
    /// tried, `other` the trace on the via's other side.
    fn reposition_axis_parallel(
        &mut self,
        via: ItemId,
        fv: FloatPoint,
        f_own: FloatPoint,
        r_own: IntPoint,
        own: ViaTrace,
        other: ViaTrace,
    ) -> Option<Point> {
        let (co, cx) = (own.costs, other.costs);
        let wd1 = fv.weighted_distance(f_own, co.horizontal, co.vertical);
        for check in [
            FloatPoint::new(fv.x, f_own.y),
            FloatPoint::new(f_own.x, fv.y),
        ] {
            let wd2 = fv.weighted_distance(check, cx.horizontal, cx.vertical);
            let wd3 = check.weighted_distance(f_own, co.horizontal, co.vertical);
            if wd1 > wd2 + wd3 {
                let location = check.round();
                if self.reposition_via_with_connection(via, location, other, r_own, own) {
                    return Some(Point::Int(location));
                }
            }
        }
        None
    }

    /// Java `DrillItemMover.check(via, delta, 0, 0, null, board, null)`.
    fn check_via_move(&mut self, via: ItemId, delta: &Vector) -> bool {
        TraceShover::new(self).check_drill_item_move(via, delta, 0, 0, &mut Vec::new(), None)
    }

    fn via_center(&self, via: ItemId) -> Point {
        Point::Int(self.item_ref(via).drill().expect("a via").center)
    }

    /// `(int) (via.minWidth() / 2) + 1`.
    fn via_tolerance(&self, via: ItemId) -> i32 {
        let min_width = self.min_width(via).unwrap_or(f64::from(i32::MAX));
        d2i(min_width / 2.0).wrapping_add(1)
    }

    /// The corner of trace `id` next to the via: corner 1 if its first
    /// corner is within `tolerance` of `center`, else corner `count − 2` if
    /// its last corner is.
    fn trace_from_corner(&self, id: ItemId, center: &Point, tolerance: i32) -> Option<Point> {
        let polyline = &self.item_ref(id).as_trace()?.polyline;
        if is_within_tolerance(&polyline.first_corner(), center, tolerance) {
            Some(polyline.corner(1))
        } else if is_within_tolerance(&polyline.last_corner(), center, tolerance) {
            Some(polyline.corner(polyline.corner_count() - 2))
        } else {
            None
        }
    }

    /// The data of trace `id` the reposition steps use, with the costs of
    /// its layer; `None` if the cost array has no entry for the layer.
    fn via_trace(&self, id: ItemId, costs: Option<&[ExpansionCostFactor]>) -> Option<ViaTrace> {
        let item = self.item_ref(id);
        let trace = item.as_trace()?;
        let costs = match costs {
            Some(costs) => *costs.get(usize::try_from(trace.layer).ok()?)?,
            None => UNIT_COSTS,
        };
        Some(ViaTrace {
            half_width: trace.half_width,
            class: item.clearance_class(),
            layer: trace.layer,
            costs,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tolerance_is_manhattan_and_inclusive() {
        let a = Point::Int(IntPoint::new(0, 0));
        assert!(is_within_tolerance(&a, &Point::Int(IntPoint::new(3, 4)), 7));
        assert!(!is_within_tolerance(
            &a,
            &Point::Int(IntPoint::new(3, 4)),
            6
        ));
        assert!(is_within_tolerance(
            &a,
            &Point::Int(IntPoint::new(-7, 0)),
            7
        ));
    }
}
