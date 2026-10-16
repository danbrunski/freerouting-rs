//! The interactive route session (I01.2-I01.5).
//!
//! Port of `gui.interactive.Route` and the board calls of `RouteState`,
//! `DynamicRouteState` and `StitchRouteState` (`gui/interactive/*.java` at
//! `a773a70ec`), without UI types. [`RouteSession::start`] is
//! `RouteState.getInstance` (in `start`), [`RouteSession::next_corner`]
//! `Route.nextCorner` with its neckdown and target connection,
//! [`RouteSession::change_layer`] and [`RouteSession::cancel`]
//! `RouteState.changeLayerAction` and `cancel` with `Route.changeLayer` (in
//! `layer`), and [`RouteSession::handle`] the pointer policy of the two
//! route states ([`RouteMode`]).
//!
//! Step outcomes are explicit ([`RouteStep`]). Where `nextCorner` compares
//! the insert's result with `prevCorner` and `currentCorner` by reference,
//! the session uses O04's [`ForcedInsert`] (`Unchanged` is `prevCorner`,
//! `Reached` is `currentCorner`); where `connect` compares by value, it
//! compares values.
//!
//! Budgets ([`RouteBudget`]) are Java's work limits: a fresh 3000 nominal ms
//! check limit for the insert of each corner (none in stitch mode), inside
//! each neckdown try and for each corner pair of a target connection, and the
//! 2000 nominal ms pull-tight limit. The clock is never read.
//!
//! Kept Java quirks (DIVERGENCES, "Java bugs kept on purpose"):
//! - a layer change without a via of the rule spanning both layers "succeeds"
//!   (`Route.changeLayer` returns true without changing the route's layer),
//!   and the session still switches its current layer and takes a snapshot
//!   ([`RouteSession::change_layer`]);
//! - `Route.connect` breaks only out of the inner loop when a corner pair
//!   fails, so with an added 45 or 90 degree corner the second pair is still
//!   tried from the added corner.
//!
//! Java's `Route` holds its target objects, and a removed object keeps its
//! shape. The session keeps the last seen shape of each trace and area
//! target for the nearest-target search, and a trace start whose
//! `connectToTrace` split the picked trace takes its targets from the removed
//! trace's end contacts (DIVERGENCES, "Interactive routing").
//!
//! A neckdown try at the end of a target connection that damages the board
//! makes Java throw a `NullPointerException` (`currentOkPoint.equals`); the
//! session treats it as the first insert's damage: undo and the route ends
//! (DIVERGENCES, "Interactive routing").

mod layer;
mod make_space;
mod start;

use std::collections::{BTreeMap, BTreeSet};

use freerouting_geometry::{Area, FloatPoint, IntPoint, Point, Polyline, TileShapeOps};

use super::settings::RouteParams;
use crate::board::RoutingBoard;
use crate::board::items::{ItemId, ItemKind};
use crate::board::search::WorkLimit;
use crate::board::structure::AngleRestriction;
use crate::optimize::params::ROUTE_PULL_TIGHT_NOMINAL_MS;
use crate::optimize::{Clip, PullTightParams};
use crate::shove::{ForcedInsert, ForcedTrace, ForcedTraceParams};

pub use layer::LayerChange;
pub use start::{StartError, StartInfo};

/// Java `Route.CHECK_FORCED_TRACE_LIMIT_NOMINAL_MS`: the work limit of
/// each forced-trace insert, in nominal milliseconds.
pub const CHECK_FORCED_TRACE_NOMINAL_MS: i64 = 3000;

/// The work limits of a route session, in nominal milliseconds (`None`:
/// unlimited).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteBudget {
    /// The limit of each forced-trace insert (Java 3000).
    pub check_ms: Option<i64>,
    /// The limit of the pull-tight after each corner and via (Java 2000).
    pub pull_tight_ms: Option<i64>,
}

impl RouteBudget {
    /// Java's limits at the pin: 3000 for each check, 2000 for pull-tight.
    pub const JAVA: RouteBudget = RouteBudget {
        check_ms: Some(CHECK_FORCED_TRACE_NOMINAL_MS),
        pull_tight_ms: Some(ROUTE_PULL_TIGHT_NOMINAL_MS),
    };
    /// No limits.
    pub const UNLIMITED: RouteBudget = RouteBudget {
        check_ms: None,
        pull_tight_ms: None,
    };
}

impl Default for RouteBudget {
    fn default() -> Self {
        RouteBudget::JAVA
    }
}

/// The result of one corner step ([`RouteSession::next_corner`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteStep {
    /// Nothing tried: an inactive or non-signal layer, a corner off the
    /// board or the same rounded corner (Java `false` before the insert).
    Rejected,
    /// Nothing was inserted; `obstacle` is the shove-failing obstacle.
    Blocked { obstacle: Option<ItemId> },
    /// The trace was extended; the route goes on.
    Advanced,
    /// The route reached a target (Java `true`); it is over.
    Completed,
    /// An insert damaged the board; it was undone. In dynamic mode the
    /// route is over (`route_ended`), in stitch mode it goes on.
    DatabaseDamaged { route_ended: bool },
}

impl RouteStep {
    /// True if Java's `nextCorner` returned true: the route is over.
    pub fn ends_route(&self) -> bool {
        match self {
            RouteStep::Completed => true,
            RouteStep::DatabaseDamaged { route_ended } => *route_ended,
            RouteStep::Rejected | RouteStep::Blocked { .. } | RouteStep::Advanced => false,
        }
    }
}

/// How pointer input drives the route (Java's `DynamicRouteState` and
/// `StitchRouteState`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteMode {
    /// A move adds a corner; a click ends the route; `Snapshot` takes one.
    Dynamic,
    /// A click takes a snapshot, then adds a corner; a move only updates
    /// the nearest target point.
    Stitch,
}

/// The pointer and key input of a route session.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RouteInput {
    Move(FloatPoint),
    Click(FloatPoint),
    /// Java's `s` key in dynamic routing.
    Snapshot,
    /// Java's Enter and Space: end the route as it is.
    Complete,
}

/// What [`RouteSession::handle`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteEvent {
    /// A corner step ran.
    Step(RouteStep),
    /// Only the nearest target point was updated (stitch move).
    TargetUpdated,
    /// A snapshot was taken (dynamic `Snapshot`).
    SnapshotTaken,
    /// The route ended without a board change (dynamic click, `Complete`).
    Ended,
    /// The input means nothing in this mode.
    Ignored,
}

impl RouteEvent {
    /// True if the session is over after this event.
    pub fn ends_route(&self) -> bool {
        match self {
            RouteEvent::Step(step) => step.ends_route(),
            RouteEvent::Ended => true,
            RouteEvent::TargetUpdated | RouteEvent::SnapshotTaken | RouteEvent::Ignored => false,
        }
    }
}

/// A pin reachable by a pin swap from a target or the start pin, with the
/// incomplete from its centre to the nearest other drill item of its net
/// (Java `Route.SwapPinInfo`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SwapPinInfo {
    pub pin: ItemId,
    pub incomplete: Option<(FloatPoint, FloatPoint)>,
}

/// The geometry of a trace or conduction-area target as the session last
/// saw it. Java's `Route` holds the target objects themselves, so a target
/// removed from the board (split, combined or shoved away) keeps the shape
/// it had then, and `calcNearestTargetPoint` still measures it.
#[derive(Clone, Debug)]
enum TargetShape {
    Trace(Polyline),
    Area { area: Area, has_shapes: bool },
}

/// The shove depths of a route (Java's push setting: 20/8/5 on, 0/0/0 off).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShoveDepths {
    pub trace: i32,
    pub via: i32,
    pub spring_over: i32,
}

impl ShoveDepths {
    /// The depths `Route`'s constructor takes for `push_enabled`.
    pub fn for_push(push_enabled: bool) -> Self {
        if push_enabled {
            ShoveDepths {
                trace: 20,
                via: 8,
                spring_over: 5,
            }
        } else {
            ShoveDepths {
                trace: 0,
                via: 0,
                spring_over: 0,
            }
        }
    }
}

/// An interactive route in progress: the state of Java's `Route` plus the
/// current layer and snapshot count of the route state.
#[derive(Clone, Debug)]
pub struct RouteSession {
    net_numbers: Vec<i32>,
    /// `None` for a make-space route (Java's `null` start item).
    start_item: Option<ItemId>,
    /// `None` for a make-space route (Java's `null` target set).
    target_set: Option<BTreeSet<ItemId>>,
    swap_pins: BTreeMap<ItemId, SwapPinInfo>,
    params: RouteParams,
    depths: ShoveDepths,
    tidy_width: i32,
    accuracy: i32,
    mode: RouteMode,
    neckdown: bool,
    via_snap_to_smd_center: bool,
    highlight_obstacle: bool,
    prev_corner: IntPoint,
    /// The route's layer (Java `Route.layer`).
    layer: i32,
    /// The editor's current layer (Java's workspace layer), which a layer
    /// change without a via moves away from the route's layer.
    current_layer: i32,
    target_points: Vec<(FloatPoint, ItemId)>,
    target_traces_and_areas: Vec<ItemId>,
    target_shapes: BTreeMap<ItemId, TargetShape>,
    nearest_target_point: Option<FloatPoint>,
    nearest_target_item: Option<ItemId>,
    shove_failing_obstacle: Option<ItemId>,
    snapshots: usize,
    corners: usize,
}

impl RouteSession {
    /// The route nets.
    pub fn net_numbers(&self) -> &[i32] {
        &self.net_numbers
    }

    /// The item the route started at (`None` for a make-space route).
    pub fn start_item(&self) -> Option<ItemId> {
        self.start_item
    }

    /// The items the route may end at, highest id first.
    pub fn target_set(&self) -> &BTreeSet<ItemId> {
        static NONE: BTreeSet<ItemId> = BTreeSet::new();
        self.target_set.as_ref().unwrap_or(&NONE)
    }

    /// The swappable pins, highest id first.
    pub fn swap_pins(&self) -> impl Iterator<Item = &SwapPinInfo> {
        self.swap_pins.values()
    }

    pub fn params(&self) -> &RouteParams {
        &self.params
    }

    pub fn depths(&self) -> ShoveDepths {
        self.depths
    }

    pub fn mode(&self) -> RouteMode {
        self.mode
    }

    pub fn is_stitch(&self) -> bool {
        self.mode == RouteMode::Stitch
    }

    /// The last inserted corner (Java `getLastCorner`).
    pub fn last_corner(&self) -> IntPoint {
        self.prev_corner
    }

    /// The route's layer.
    pub fn layer(&self) -> i32 {
        self.layer
    }

    /// The editor's current layer.
    pub fn current_layer(&self) -> i32 {
        self.current_layer
    }

    pub fn nearest_target_point(&self) -> Option<FloatPoint> {
        self.nearest_target_point
    }

    pub fn nearest_target_item(&self) -> Option<ItemId> {
        self.nearest_target_item
    }

    /// The obstacle the last failed shove stopped at.
    pub fn shove_failing_obstacle(&self) -> Option<ItemId> {
        self.shove_failing_obstacle
    }

    /// Whether the overlay shows the shove-failing obstacle.
    pub fn highlights_obstacle(&self) -> bool {
        self.highlight_obstacle
    }

    /// The snapshots this session took and did not undo itself.
    pub fn snapshots_taken(&self) -> usize {
        self.snapshots
    }

    /// The corner steps that extended the trace.
    pub fn corners_added(&self) -> usize {
        self.corners
    }

    /// Java `Route.nearestTargetLayer`: the layer of the nearest target
    /// item nearest to the route's layer.
    pub fn nearest_target_layer(&self, board: &RoutingBoard) -> i32 {
        let Some(item) = self.nearest_target_item.and_then(|id| board.item(id)) else {
            return self.layer;
        };
        let (first, last) = (item.first_layer(), item.last_layer());
        if self.layer < first {
            first
        } else {
            self.layer.min(last)
        }
    }

    /// Feeds one input to the session in its mode (I01.5).
    pub fn handle(
        &mut self,
        board: &mut RoutingBoard,
        input: RouteInput,
        budget: RouteBudget,
    ) -> RouteEvent {
        match (self.mode, input) {
            (RouteMode::Dynamic, RouteInput::Move(p)) => {
                RouteEvent::Step(self.next_corner(board, p, budget))
            }
            (RouteMode::Dynamic, RouteInput::Click(_)) | (_, RouteInput::Complete) => {
                RouteEvent::Ended
            }
            (RouteMode::Dynamic, RouteInput::Snapshot) => {
                board.generate_snapshot();
                self.snapshots += 1;
                RouteEvent::SnapshotTaken
            }
            (RouteMode::Stitch, RouteInput::Click(p)) => {
                board.generate_snapshot();
                self.snapshots += 1;
                RouteEvent::Step(self.next_corner(board, p, budget))
            }
            (RouteMode::Stitch, RouteInput::Move(p)) => {
                self.calc_nearest_target_point(board, p);
                RouteEvent::TargetUpdated
            }
            (RouteMode::Stitch, RouteInput::Snapshot) => RouteEvent::Ignored,
        }
    }

    /// Java `Route.nextCorner`: extends the trace from the last corner
    /// towards `corner`.
    pub fn next_corner(
        &mut self,
        board: &mut RoutingBoard,
        corner: FloatPoint,
        budget: RouteBudget,
    ) -> RouteStep {
        let step = self.next_corner_inner(board, corner, budget);
        if matches!(step, RouteStep::Advanced | RouteStep::Completed) {
            self.corners += 1;
        }
        step
    }

    fn next_corner_inner(
        &mut self,
        board: &mut RoutingBoard,
        corner: FloatPoint,
        budget: RouteBudget,
    ) -> RouteStep {
        if !self.params.is_layer_active(self.layer) {
            return RouteStep::Rejected;
        }
        let mut current = corner.round();
        let prev = self.prev_corner;
        let is_signal = board
            .layer_structure
            .layers
            .get(self.layer as usize)
            .is_some_and(|l| l.is_signal);
        if !(board.contains(&Point::Int(prev)) && board.contains(&Point::Int(current)) && is_signal)
        {
            return RouteStep::Rejected;
        }
        if current == prev {
            return RouteStep::Rejected;
        }
        if let Some(center) = self
            .nearest_target_item
            .and_then(|id| drill_center(board, id))
            && prev == center
        {
            // connection already completed at prevCorner
            return RouteStep::Completed;
        }
        self.shove_failing_obstacle = None;
        match board.rules.trace_angle_restriction() {
            AngleRestriction::NinetyDegree => current = current.orthogonal_projection(prev),
            AngleRestriction::FortyfiveDegree => {
                current = current.fortyfive_degree_projection(prev);
            }
            AngleRestriction::None => {}
        }
        let end_routing_item = board.pick_nearest_routing_item(&Point::Int(prev), self.layer, None);
        // look for a nearby item of this net, which is not connected to
        // end_routing_item
        self.nearest_target_item =
            board.pick_nearest_routing_item(&Point::Int(current), self.layer, end_routing_item);
        let check_ms = if self.is_stitch() {
            // no check before inserting in this case
            None
        } else {
            budget.check_ms
        };
        let half_width = self.half_width();
        let mut ok = self.insert(board, prev, current, half_width, check_ms);
        if ok == ForcedInsert::Unchanged && self.neckdown {
            ok = self.try_neckdown_at_start(board, current, budget);
        }
        if ok == ForcedInsert::Unchanged && self.neckdown {
            ok = self.try_neckdown_at_end(board, prev, current, budget);
        }
        let ok_point = match ok {
            ForcedInsert::DatabaseDamaged => {
                // database may be damaged, restore previous situation
                board.undo(None);
                return RouteStep::DatabaseDamaged {
                    route_ended: !self.is_stitch(),
                };
            }
            ForcedInsert::Unchanged => {
                self.shove_failing_obstacle = board.shove_failing_obstacle();
                return RouteStep::Blocked {
                    obstacle: self.shove_failing_obstacle,
                };
            }
            ForcedInsert::Reached => current,
            ForcedInsert::Partial(p) => p,
        };
        self.prev_corner = ok_point;
        // check, if a target is reached
        let completed =
            ok == ForcedInsert::Reached && self.connect_to_target(board, current, budget);
        let clip = match self.tidy_width {
            i32::MAX => Clip::Unbounded,
            0 => Clip::Skip,
            width => Clip::from_tidy_region(
                Point::Int(ok_point)
                    .surrounding_octagon()
                    .enlarge(f64::from(width)),
            ),
        };
        if completed {
            board.reduce_nets_of_route_items();
            for &net in &self.net_numbers {
                board.combine_traces(net);
            }
        } else {
            self.calc_nearest_target_point(board, self.prev_corner.to_float());
        }
        let mut params = PullTightParams::new(self.accuracy);
        if self.depths.trace <= 0 {
            params.only_nets = self.net_numbers.clone();
        }
        params.clip = clip;
        params.limit = budget.pull_tight_ms;
        params.keep_point = Some((Point::Int(ok_point), self.layer));
        board.opt_changed_area(&params);
        if completed {
            RouteStep::Completed
        } else {
            RouteStep::Advanced
        }
    }

    /// The pen half width on the route's layer.
    fn half_width(&self) -> i32 {
        self.params
            .half_widths
            .get(self.layer as usize)
            .copied()
            .unwrap_or(0)
    }

    /// Java `board.insertForcedTraceSegment(from, to, halfWidth, layer,
    /// netNumbers, clearanceClass, depths…, tidy, accuracy, !stitch,
    /// new WorkLimit(check_ms))`.
    fn insert(
        &self,
        board: &mut RoutingBoard,
        from: IntPoint,
        to: IntPoint,
        half_width: i32,
        check_ms: Option<i64>,
    ) -> ForcedInsert {
        let mut limit = check_ms.map(|ms| {
            WorkLimit::with_site(board.search_trees.work_meter(), ms, "interactive_route")
        });
        board.insert_forced_trace_segment(
            &Point::Int(from),
            &Point::Int(to),
            ForcedTrace {
                half_width,
                layer: self.layer,
                net_numbers: &self.net_numbers,
                clearance_class: self.params.clearance_class,
            },
            ForcedTraceParams {
                max_recursion_depth: self.depths.trace,
                max_via_recursion_depth: self.depths.via,
                max_spring_over_recursion_depth: self.depths.spring_over,
                tidy_width: self.tidy_width,
                pull_tight_accuracy: self.accuracy,
                with_check: !self.is_stitch(),
            },
            limit.as_mut(),
        )
    }

    /// Java `Route.connectToTarget`: connects the corner just reached to the
    /// nearest target item if it is one of the route's targets.
    fn connect_to_target(
        &mut self,
        board: &mut RoutingBoard,
        from: IntPoint,
        budget: RouteBudget,
    ) -> bool {
        if let Some(id) = self.nearest_target_item
            && let Some(target_set) = &self.target_set
            && !target_set.contains(&id)
        {
            self.nearest_target_item = None;
        }
        let Some(item) = self.nearest_target_item.and_then(|id| board.item(id)) else {
            return false;
        };
        if !item.shares_net_no(&self.net_numbers) {
            return false;
        }
        let connection_point = match &item.kind {
            ItemKind::Pin(_) | ItemKind::Via(_) => item.drill().map(|d| d.center),
            ItemKind::Trace(_) => {
                let trace = item.id();
                return board.connect_to_trace(
                    from,
                    trace,
                    self.half_width(),
                    self.params.clearance_class,
                );
            }
            ItemKind::Area(area) if area.is_conduction_area() => Some(from),
            _ => None,
        };
        match connection_point {
            Some(point) => self.connect(board, from, point, budget),
            None => false,
        }
    }

    /// Java `Route.connect`: a trace from `from` to `to` along the angle
    /// restriction, corner pair by corner pair. True also when an insert
    /// damaged the board (undone).
    fn connect(
        &mut self,
        board: &mut RoutingBoard,
        from: IntPoint,
        to: IntPoint,
        budget: RouteBudget,
    ) -> bool {
        let corners = angled_connection(board, from, to);
        let mut succeeded = true;
        for pair in corners.windows(2) {
            let (mut from_corner, to_corner) = (pair[0], pair[1]);
            let mut limit = budget.check_ms.map(|ms| {
                WorkLimit::with_site(board.search_trees.work_meter(), ms, "interactive_route")
            });
            while from_corner != to_corner {
                let result = board.insert_forced_trace_segment(
                    &Point::Int(from_corner),
                    &Point::Int(to_corner),
                    ForcedTrace {
                        half_width: self.half_width(),
                        layer: self.layer,
                        net_numbers: &self.net_numbers,
                        clearance_class: self.params.clearance_class,
                    },
                    ForcedTraceParams {
                        max_recursion_depth: self.depths.trace,
                        max_via_recursion_depth: self.depths.via,
                        max_spring_over_recursion_depth: self.depths.spring_over,
                        tidy_width: self.tidy_width,
                        pull_tight_accuracy: self.accuracy,
                        with_check: !self.is_stitch(),
                    },
                    limit.as_mut(),
                );
                let Some(mut ok) = int_result(result, from_corner, to_corner) else {
                    // database may be damaged, restore previous situation
                    board.undo(None);
                    return true;
                };
                if ok == from_corner && self.neckdown {
                    let neck = self.try_neckdown_at_end(board, from_corner, to_corner, budget);
                    let Some(p) = int_result(neck, from_corner, to_corner) else {
                        // Java: NullPointerException; treated as the damage above
                        board.undo(None);
                        return true;
                    };
                    ok = p;
                }
                if ok == from_corner {
                    self.prev_corner = from_corner;
                    succeeded = false;
                    // Java breaks the inner loop only (kept quirk)
                    break;
                }
                from_corner = ok;
            }
        }
        succeeded
    }

    /// Java `Route.tryNeckdownAtStart`: the insert again with the start
    /// pin's neckdown half width, if the route starts near that pin.
    fn try_neckdown_at_start(
        &self,
        board: &mut RoutingBoard,
        to: IntPoint,
        budget: RouteBudget,
    ) -> ForcedInsert {
        let Some(pin) = self.start_item else {
            return ForcedInsert::Unchanged;
        };
        let Some(neck) = self.neckdown_half_width(board, pin, self.prev_corner) else {
            return ForcedInsert::Unchanged;
        };
        // check, that the neck_down started inside the pin shape
        if drill_center(board, pin) != Some(self.prev_corner) {
            let picked =
                board.pick_nearest_routing_item(&Point::Int(self.prev_corner), self.layer, None);
            if let Some(trace) = picked
                .and_then(|id| board.item(id))
                .and_then(|i| i.as_trace())
                && trace.half_width > neck
            {
                return ForcedInsert::Unchanged;
            }
        }
        self.insert(board, self.prev_corner, to, neck, budget.check_ms)
    }

    /// Java `Route.tryNeckdownAtEnd`: the insert from `from` again with the
    /// target pin's neckdown half width, if `from` is near that pin.
    fn try_neckdown_at_end(
        &self,
        board: &mut RoutingBoard,
        from: IntPoint,
        to: IntPoint,
        budget: RouteBudget,
    ) -> ForcedInsert {
        let Some(pin) = self.nearest_target_item else {
            return ForcedInsert::Unchanged;
        };
        let Some(neck) = self.neckdown_half_width(board, pin, from) else {
            return ForcedInsert::Unchanged;
        };
        self.insert(board, from, to, neck, budget.check_ms)
    }

    /// The neckdown half width at pin `pin` for a trace end at `at`: `None`
    /// if `pin` is no pin on the route's layer, `at` is at least twice the
    /// pin's half width plus the clearance from its centre, or the neckdown
    /// is not narrower than the pen.
    fn neckdown_half_width(&self, board: &RoutingBoard, pin: ItemId, at: IntPoint) -> Option<i32> {
        let item = board.item(pin)?;
        if !item.is_pin() || !item.is_on_layer(self.layer) {
            return None;
        }
        let center = item.drill()?.center;
        let clearance = f64::from(board.rules.clearance_matrix.get_value(
            self.params.clearance_class,
            item.clearance_class(),
            self.layer,
            true,
        ));
        let neck_down_distance = 2.0 * (0.5 * board.pin_max_width(pin, self.layer) + clearance);
        if center.to_float().distance(at.to_float()) >= neck_down_distance {
            return None;
        }
        let neck = board.trace_neckdown_half_width(pin, self.layer);
        (neck < self.half_width()).then_some(neck)
    }

    /// Java `Route.calcNearestTargetPoint`: the target point or trace or
    /// area point nearest to `from` (the first of equals), for the
    /// incomplete.
    pub fn calc_nearest_target_point(&mut self, board: &RoutingBoard, from: FloatPoint) {
        let mut min_dist = f64::MAX;
        let mut nearest: Option<(FloatPoint, ItemId)> = None;
        for &(location, item) in &self.target_points {
            let distance = from.distance(location);
            if distance < min_dist {
                min_dist = distance;
                nearest = Some((location, item));
            }
        }
        for i in 0..self.target_traces_and_areas.len() {
            let id = self.target_traces_and_areas[i];
            self.refresh_target_shape(board, id);
            let Some(shape) = self.target_shapes.get(&id) else {
                continue;
            };
            let candidate = match shape {
                TargetShape::Trace(polyline) => (polyline.bounding_box().distance(from) < min_dist)
                    .then(|| polyline.nearest_point_approx(from))
                    .flatten(),
                TargetShape::Area { area, has_shapes } => (*has_shapes
                    && area.bounding_box().distance(from) < min_dist)
                    .then(|| area.nearest_point_approx(from))
                    .flatten(),
            };
            if let Some(point) = candidate {
                let distance = from.distance(point);
                if distance < min_dist {
                    min_dist = distance;
                    nearest = Some((point, id));
                }
            }
        }
        if let Some((point, item)) = nearest {
            self.nearest_target_point = Some(point);
            self.nearest_target_item = Some(item);
        }
    }
}

impl RouteSession {
    /// Takes the shape of trace or conduction-area target `id` from the
    /// board while it is there; a removed target keeps its last shape.
    fn refresh_target_shape(&mut self, board: &RoutingBoard, id: ItemId) {
        let Some(item) = board.item(id) else {
            return;
        };
        let shape = match &item.kind {
            ItemKind::Trace(trace) => TargetShape::Trace(trace.polyline.clone()),
            ItemKind::Area(area) if area.is_conduction_area() => TargetShape::Area {
                area: area.area().clone(),
                has_shapes: board.tile_shape_count(item) > 0,
            },
            _ => return,
        };
        self.target_shapes.insert(id, shape);
    }

    /// True if target `id` is a conduction area (also after its removal).
    pub(crate) fn is_area_target(&self, id: ItemId) -> bool {
        matches!(self.target_shapes.get(&id), Some(TargetShape::Area { .. }))
    }
}

/// The integer centre of drill item `id`.
fn drill_center(board: &RoutingBoard, id: ItemId) -> Option<IntPoint> {
    board.item(id)?.drill().map(|d| d.center)
}

/// Java's result point of an insert from `from` to `to`; `None` for damage.
fn int_result(result: ForcedInsert, from: IntPoint, to: IntPoint) -> Option<IntPoint> {
    match result {
        ForcedInsert::DatabaseDamaged => None,
        ForcedInsert::Unchanged => Some(from),
        ForcedInsert::Reached => Some(to),
        ForcedInsert::Partial(p) => Some(p),
    }
}

/// Java `Route.angledConnection`: `from`, the 90 or 45 degree corner (left
/// turn) if the restriction asks for one and it exists, and `to`.
fn angled_connection(board: &RoutingBoard, from: IntPoint, to: IntPoint) -> Vec<IntPoint> {
    let corner = match board.rules.trace_angle_restriction() {
        AngleRestriction::NinetyDegree => from.ninety_degree_corner(to, true),
        AngleRestriction::FortyfiveDegree => from.fortyfive_degree_corner(to, true),
        AngleRestriction::None => None,
    };
    let mut corners = vec![from];
    corners.extend(corner);
    corners.push(to);
    corners
}
