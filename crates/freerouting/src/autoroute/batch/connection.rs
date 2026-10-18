//! The Freerouting connection router of the batch passes (A11.1, A11.2).
//!
//! Port of `pipeline/AutorouteConnectionRouter.java` at `a773a70ec`:
//! [`FreeroutingConnectionRouter`] is `route` (`:30-180`), registered as
//! `freerouting-connection`; the two policies it consults are separate
//! registered stages, so an X-series stage can replace one without the
//! other:
//! - [`FreeroutingPlaneTaps`] (`freerouting-plane-taps`, `:59-77`): the
//!   destinations of a plane-net connection. The router itself makes the
//!   rule's two board facts: a plane-net item whose connected set already
//!   holds a conduction area is `ConnectedToPlane`, and a plane net routes
//!   from its connected set. The policy aims the connection at the net's
//!   planes on power layers (`power_plane_areas`) with the unconnected set
//!   as the fallback, which gives every plane-net pad its own via (D8).
//! - [`FreeroutingNeckDown`] (`freerouting-neck-down`,
//!   `retryConnectionNecked`, `:193-273`): a failed connection retried with
//!   every trace half width clamped to `router.neck_width_um / 2`, the
//!   clearance compensation kept, the same start and destination, the same
//!   ripped set and the same work limit. The half width is floored by
//!   `router.min_trace_width_um` (Java `46bf9e2ef`, after the pin; unset by
//!   default, which changes nothing).
//!
//! **Orders that decide results**, in Java's order:
//! - the control is built before the unconnected set is looked at, so a
//!   net whose control cannot be built fails even when it has nothing to
//!   connect (`catch (Exception)`, `:174-177`);
//! - the maze-search [`WorkLimit`] of `min(100000 * 2^(pass-1), i32::MAX)`
//!   nominal ms is created before the session, on the board's own meter,
//!   and the plane attempt, its fallback and the neck-down retry all
//!   continue it (`:83-109`, `:247`): units spent on the first attempt's
//!   insertion count against the retry;
//! - the id watermark is read after the limit and before the strict-DRC
//!   snapshot (`:94-99`);
//! - after a routed connection, `opt_changed_area` with
//!   [`PullTightParams::after_connection`] (all nets, the pass's pull-tight
//!   accuracy and trace costs, so the via optimizer runs, and a limit of
//!   1,000 nominal ms of its own);
//! - a necked success goes through the strict check before it is returned;
//!   a failed neck-down returns the first attempt's result (`:139-171`).
//!
//! **Limits and board replacements.** Every [`WorkLimit`] is created on the
//! board it is checked against. The strict-DRC rollback with `strict_drc`
//! on replaces the board with the snapshot taken before the route; that
//! board has a fresh meter, and no limit of this connection is checked
//! after it, as in Java, where the rollback also comes after the search.
//!
//! Not ported: Java's benchmark profile timers, its trace logging and the
//! `Necked retry routed net ...` info line (no log channel reaches a stage).
//! The pull-tight runs without the cancel token: [`PullTightParams::stop`]
//! takes an owned `Arc`, a stage only borrows the token, and the 1,000 ms
//! work limit bounds the run anyway.

use std::collections::BTreeSet;

use freerouting_geometry::FloatPoint;
use freerouting_geometry::jmath::round_to_i32;

use super::strict_drc::apply_strict_drc_after_route;
use crate::autoroute::connection::{Ripped, RouteLimits, route_connection_counted};
use crate::autoroute::control::AutorouteControl;
use crate::autoroute::path::RipupCosts;
use crate::autoroute::session::budget::BudgetPolicy;
use crate::autoroute::session::drills::DrillCarry;
use crate::autoroute::session::search::SearchSettings;
use crate::autoroute::stages::{
    ConnectionOutcome, ConnectionParams, ConnectionRouter, Destinations, FailureRecord,
    NeckDownAttempt, NeckDownPolicy, PlaneTapPolicy, RouteTask, StageCtx, StageRegistry,
    StageSelection, WorkStats,
};
use crate::autoroute::width_policy::WidthPolicy;
use crate::autoroute::{AttemptResult, AttemptState};
use crate::board::RoutingBoard;
use crate::board::items::ItemId;
use crate::board::search::{UNITS_PER_NOMINAL_MS, WorkLimit};
use crate::board::structure::Unit;
use crate::datastructures::IdGenerator;
use crate::optimize::PullTightParams;
use crate::settings::RouterSettings;
use crate::settings::layers::TraceCostFactor;

/// The maze search's budget at pass 1 in nominal milliseconds
/// (`AutorouteConnectionRouter.java:83`).
pub const MAZE_BASE_NOMINAL_MS: f64 = 100_000.0;

/// The site name of the maze search's limit in a `WORK_LIMIT_CUTOFF` line.
pub const MAZE_SEARCH_SITE: &str = "maze_search";

/// The maze search's budget at pass `pass` in nominal milliseconds:
/// `(long) min(100000 * 2^(pass-1), Integer.MAX_VALUE)`
/// (`AutorouteConnectionRouter.java:83-85`).
pub fn maze_budget_ms(pass: u32) -> i64 {
    // PARITY: :83-84, in double: Math.pow(2, n) is exact for these n.
    let exponent = i32::try_from(pass).unwrap_or(i32::MAX).saturating_sub(1);
    let ms = MAZE_BASE_NOMINAL_MS * 2f64.powi(exponent);
    ms.min(f64::from(i32::MAX)) as i64
}

/// [`maze_budget_ms`] in work units, the value of
/// [`ConnectionParams::work_budget`].
pub fn maze_work_budget(pass: u32) -> u64 {
    maze_budget_ms(pass) as u64 * UNITS_PER_NOMINAL_MS
}

/// The parameters Java's connection router uses for `task` in pass `pass`
/// (`:39-46`): the plane via costs for a net that contains a plane, else
/// the via costs; ripup allowed with `ripup_cost`; the pass's trace costs,
/// unconnected-via rule, pull-tight accuracy and strict check; the maze
/// budget of [`maze_work_budget`].
#[allow(clippy::too_many_arguments)]
pub fn connection_params(
    board: &RoutingBoard,
    task: &RouteTask,
    pass: u32,
    ctx: &StageCtx,
    ripup_cost: i32,
    trace_costs: &[TraceCostFactor],
    remove_unconnected_vias: bool,
    pull_tight_accuracy: i32,
    strict_check: bool,
) -> ConnectionParams {
    let via_cost = if net_contains_plane(board, task.net) {
        ctx.board_settings.plane_via_costs()
    } else {
        ctx.board_settings.via_costs()
    };
    ConnectionParams {
        pass,
        ripup_allowed: true,
        ripup_cost,
        via_cost,
        trace_costs: trace_costs.to_vec(),
        remove_unconnected_vias,
        work_budget: maze_work_budget(pass),
        pull_tight_accuracy,
        strict_check,
        ripup_factors: None,
    }
}

/// True if net `net` exists and contains a plane (Java
/// `routeNet != null && routeNet.containsPlane()`).
pub fn net_contains_plane(board: &RoutingBoard, net: i32) -> bool {
    board
        .rules
        .nets
        .get(net)
        .is_some_and(|n| n.contains_plane())
}

/// Java `AutorouteAirlineCalculator.calculateAirline`: the closest pair of
/// pin or via centres, one from `from` and one from `to`; the first pair
/// wins a tie. `None` when either set has no pin or via (Java's
/// `FloatLine(null, null)`, which `getAirLine` reports as null).
pub fn calculate_airline(
    board: &RoutingBoard,
    from: &BTreeSet<ItemId>,
    to: &BTreeSet<ItemId>,
) -> Option<(FloatPoint, FloatPoint)> {
    let centres = |set: &BTreeSet<ItemId>| -> Vec<FloatPoint> {
        set.iter()
            .filter_map(|id| board.item(*id))
            .filter_map(|item| item.drill())
            .map(|drill| drill.center.to_float())
            .collect()
    };
    let targets = centres(to);
    let mut best: Option<(FloatPoint, FloatPoint)> = None;
    let mut min_distance = f64::MAX;
    for a in centres(from) {
        for &b in &targets {
            let distance = a.distance_square(b);
            if distance < min_distance {
                min_distance = distance;
                best = Some((a, b));
            }
        }
    }
    best
}

/// The Java control of `task.net` with the pass's via and trace costs and
/// ripup settings (`:42-46`, `:225-229`).
pub(crate) fn router_control(
    board: &RoutingBoard,
    task: &RouteTask,
    p: &ConnectionParams,
    ctx: &StageCtx,
) -> Option<AutorouteControl> {
    let mut control = AutorouteControl::for_net(
        board,
        task.net,
        ctx.board_settings,
        p.via_cost,
        &p.trace_costs,
    )
    .ok()?;
    control.ripup_allowed = p.ripup_allowed;
    control.ripup_costs = p.ripup_cost;
    control.ripup_factors = p.ripup_factors.clone();
    control.remove_unconnected_vias = p.remove_unconnected_vias;
    Some(control)
}

/// One `initAutoroute` plus `autorouteConnection` with `limit`, adding to
/// the caller's ripped set.
#[allow(clippy::too_many_arguments)]
pub(crate) fn attempt(
    board: &mut RoutingBoard,
    start: &BTreeSet<ItemId>,
    dest: &BTreeSet<ItemId>,
    control: &AutorouteControl,
    limit: &WorkLimit,
    ctx: &StageCtx,
    ripped: &mut BTreeSet<ItemId>,
    costs: &mut RipupCosts,
) -> AttemptResult {
    attempt_with_widths(board, start, dest, control, limit, ctx, ripped, costs, None)
}

/// [`attempt`] with the room widths of `widths` (Q17.1).
#[allow(clippy::too_many_arguments)]
pub(crate) fn attempt_with_widths(
    board: &mut RoutingBoard,
    start: &BTreeSet<ItemId>,
    dest: &BTreeSet<ItemId>,
    control: &AutorouteControl,
    limit: &WorkLimit,
    ctx: &StageCtx,
    ripped: &mut BTreeSet<ItemId>,
    costs: &mut RipupCosts,
    widths: Option<&dyn WidthPolicy>,
) -> AttemptResult {
    attempt_budgeted(
        board, start, dest, control, limit, ctx, ripped, costs, widths, None,
    )
}

/// [`attempt_with_widths`] whose search asks `budget` when the work limit
/// fires (Q08); `None` is Java's stop.
#[allow(clippy::too_many_arguments)]
pub(crate) fn attempt_budgeted(
    board: &mut RoutingBoard,
    start: &BTreeSet<ItemId>,
    dest: &BTreeSet<ItemId>,
    control: &AutorouteControl,
    limit: &WorkLimit,
    ctx: &StageCtx,
    ripped: &mut BTreeSet<ItemId>,
    costs: &mut RipupCosts,
    widths: Option<&dyn WidthPolicy>,
    budget: Option<&mut dyn BudgetPolicy>,
) -> AttemptResult {
    let limits = RouteLimits {
        settings: SearchSettings::from_router(ctx.board_settings),
        stop: Some(ctx.cancel),
        work_limit: Some(limit.clone()),
    };
    let ripped = Ripped {
        items: ripped,
        costs: Some(costs),
    };
    route_connection_counted(
        board,
        start,
        dest,
        control,
        limits,
        ripped,
        &mut DrillCarry::default(),
        widths,
        budget,
    )
}

/// The pull-tight after a routed connection (`:121-128`, `:257-264`).
pub(crate) fn pull_tight_after_connection(
    board: &mut RoutingBoard,
    accuracy: i32,
    trace_costs: &[TraceCostFactor],
) {
    board.opt_changed_area(&PullTightParams::after_connection(
        accuracy,
        trace_costs.to_vec(),
    ));
}

/// Java's connection router (`freerouting-connection`).
pub struct FreeroutingConnectionRouter {
    plane_taps: Box<dyn PlaneTapPolicy>,
    neck_down: Box<dyn NeckDownPolicy>,
}

impl std::fmt::Debug for FreeroutingConnectionRouter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FreeroutingConnectionRouter")
            .field("plane_taps", &self.plane_taps.id())
            .field("neck_down", &self.neck_down.id())
            .finish()
    }
}

impl FreeroutingConnectionRouter {
    /// A router that consults `plane_taps` and `neck_down`.
    pub fn with_policies(
        plane_taps: Box<dyn PlaneTapPolicy>,
        neck_down: Box<dyn NeckDownPolicy>,
    ) -> Self {
        FreeroutingConnectionRouter {
            plane_taps,
            neck_down,
        }
    }

    /// The router with the plane-tap and neck-down policies `settings`
    /// select (`router.special.{plane_taps, neck_down}`) from the built-in
    /// registry; an id the registry does not know (which
    /// [`StageRegistry::build`] reports for the job) gives Freerouting's.
    pub fn from_settings(settings: &RouterSettings) -> Self {
        let selection = StageSelection::from_settings(settings);
        let registry = StageRegistry::builtin();
        let plane_taps = registry
            .plane_taps
            .get(selection.plane_taps.id.as_str())
            .map_or_else(
                || Box::new(FreeroutingPlaneTaps) as Box<dyn PlaneTapPolicy>,
                |f| f(settings),
            );
        let neck_down = registry
            .neck_down
            .get(selection.neck_down.id.as_str())
            .map_or_else(
                || Box::new(FreeroutingNeckDown) as Box<dyn NeckDownPolicy>,
                |f| f(settings),
            );
        Self::with_policies(plane_taps, neck_down)
    }

    /// Freerouting's router with Freerouting's policies.
    pub fn freerouting() -> Self {
        Self::with_policies(
            Box::new(FreeroutingPlaneTaps),
            Box::new(FreeroutingNeckDown),
        )
    }
}

/// The start set, the primary destination and the fallback of a
/// connection, or the state that ends it before routing.
enum RouteSets {
    Skip(AttemptState),
    Route {
        start: BTreeSet<ItemId>,
        dest: BTreeSet<ItemId>,
        fallback: Option<BTreeSet<ItemId>>,
    },
}

impl FreeroutingConnectionRouter {
    /// `:48-77`: the route sets of `task`.
    fn route_sets(&self, board: &RoutingBoard, task: &RouteTask) -> RouteSets {
        let unconnected = board.unconnected_set(task.item, task.net);
        if unconnected.is_empty() {
            return RouteSets::Skip(AttemptState::NoUnconnectedNets);
        }
        let connected = board.connected_set(task.item, task.net, false);
        if !net_contains_plane(board, task.net) {
            return RouteSets::Route {
                start: unconnected,
                dest: connected,
                fallback: None,
            };
        }
        // PARITY: :60-64, a conduction area in the connected set.
        let on_plane = connected
            .iter()
            .any(|id| board.item(*id).is_some_and(|i| i.is_conduction_area()));
        if on_plane {
            return RouteSets::Skip(AttemptState::ConnectedToPlane);
        }
        let Destinations { primary, fallback } =
            self.plane_taps.destinations(board, task, &unconnected);
        RouteSets::Route {
            start: connected,
            dest: primary,
            fallback,
        }
    }
}

impl ConnectionRouter for FreeroutingConnectionRouter {
    fn id(&self) -> &'static str {
        "freerouting-connection"
    }

    fn route(
        &mut self,
        board: &mut RoutingBoard,
        task: &RouteTask,
        p: &ConnectionParams,
        ctx: &mut StageCtx,
    ) -> ConnectionOutcome {
        self.route_with_budget(board, task, p, ctx, None)
    }
}

impl FreeroutingConnectionRouter {
    /// [`ConnectionRouter::route`] whose maze searches (the connection's and
    /// its plane fallback's, not the neck-down retry's) ask `budget` when
    /// the work limit fires (Q08's `earned-budget`). Each search starts the
    /// policy anew ([`BudgetPolicy::on_start`]); `None` is Java's router.
    pub fn route_with_budget(
        &mut self,
        board: &mut RoutingBoard,
        task: &RouteTask,
        p: &ConnectionParams,
        ctx: &mut StageCtx,
        mut budget: Option<&mut dyn BudgetPolicy>,
    ) -> ConnectionOutcome {
        // PARITY: :42-46 before :48, and Java's catch returns FAILED.
        let Some(control) = router_control(board, task, p, ctx) else {
            return failed_outcome(AttemptResult::new(AttemptState::Failed));
        };
        let (start, mut dest, fallback) = match self.route_sets(board, task) {
            RouteSets::Skip(state) => return ConnectionOutcome::new(state, ""),
            RouteSets::Route {
                start,
                dest,
                fallback,
            } => (start, dest, fallback),
        };
        let mut airline = calculate_airline(board, &start, &dest);
        // PARITY: :83-91, the limit before the session; :94 the watermark
        // after; :99 the snapshot last.
        let meter_start = board.search_trees().work_meter().units();
        let limit = WorkLimit::with_site(
            board.search_trees().work_meter(),
            (p.work_budget / UNITS_PER_NOMINAL_MS) as i64,
            MAZE_SEARCH_SITE,
        );
        let max_id_before = board.communication.id_generator.max_generated_id();
        let snapshot = ctx.board_settings.strict_drc().then(|| board.snapshot());

        let mut ripped = BTreeSet::new();
        let mut costs = RipupCosts::new();
        let mut result = attempt_budgeted(
            board,
            &start,
            &dest,
            &control,
            &limit,
            ctx,
            &mut ripped,
            &mut costs,
            None,
            budget.as_deref_mut(),
        );
        if result.state == AttemptState::Failed
            && let Some(fallback) = fallback
        {
            // PARITY: :100-109, a new session, the same limit.
            dest = fallback;
            airline = calculate_airline(board, &start, &dest);
            result = attempt_budgeted(
                board,
                &start,
                &dest,
                &control,
                &limit,
                ctx,
                &mut ripped,
                &mut costs,
                None,
                budget,
            );
        }
        if result.state == AttemptState::Routed {
            pull_tight_after_connection(board, p.pull_tight_accuracy, &control.trace_costs);
        }

        let finished = |board: &mut RoutingBoard, result: AttemptResult, ripped, costs| {
            let meter = board.search_trees().work_meter();
            let work = WorkStats {
                units: meter.units().wrapping_sub(meter_start),
            };
            let work_limit_hit = limit.exceeded(meter);
            if work_limit_hit {
                crate::run_stats::record_work_limit_hit(false);
            }
            let mut outcome = ConnectionOutcome::new(result.state, result.details);
            outcome.ripped = ripped;
            outcome.ripped_costs = costs;
            outcome.first_new_id = max_id_before.wrapping_add(1);
            outcome.work = work;
            outcome.work_limit_hit = work_limit_hit;
            outcome.airline = airline;
            outcome
        };

        if matches!(
            result.state,
            AttemptState::Failed | AttemptState::InsertError
        ) {
            let failed = finished(board, result.clone(), ripped.clone(), costs.clone());
            let necked = self.neck_down.retry(
                board,
                task,
                NeckDownAttempt {
                    start: &start,
                    dest: &dest,
                    control: &control,
                    work_limit: &limit,
                    ripped: &mut ripped,
                    ripped_costs: &mut costs,
                },
                &failed,
                p,
                ctx,
            );
            if let Some(necked) = necked {
                let necked = AttemptResult::with_details(necked.state, necked.details);
                let mut outcome = finished(board, necked, ripped, costs);
                if let Some(rejection) = apply_strict_drc_after_route(
                    board,
                    task.net,
                    max_id_before,
                    snapshot,
                    p.strict_check,
                    ctx.events,
                ) {
                    outcome.state = rejection.state;
                    outcome.details = rejection.details;
                }
                return with_failure(outcome);
            }
            let mut outcome = failed;
            // The neck-down may have ripped more before it gave up.
            outcome.ripped = ripped;
            outcome.ripped_costs = costs;
            return with_failure(outcome);
        }
        let mut outcome = finished(board, result, ripped, costs);
        if outcome.state == AttemptState::Routed
            && let Some(rejection) = apply_strict_drc_after_route(
                board,
                task.net,
                max_id_before,
                snapshot,
                p.strict_check,
                ctx.events,
            )
        {
            outcome.state = rejection.state;
            outcome.details = rejection.details;
        }
        with_failure(outcome)
    }
}

/// An outcome of `result` with nothing else known.
fn failed_outcome(result: AttemptResult) -> ConnectionOutcome {
    with_failure(ConnectionOutcome::new(result.state, result.details))
}

/// Fills [`ConnectionOutcome::failure`] for a failed state.
fn with_failure(mut outcome: ConnectionOutcome) -> ConnectionOutcome {
    if matches!(
        outcome.state,
        AttemptState::Failed | AttemptState::InsertError
    ) {
        outcome.failure = Some(FailureRecord::new(outcome.details.clone()));
    }
    outcome
}

// ---------------------------------------------------------------------
// The policies (A11.2)
// ---------------------------------------------------------------------

/// Java's plane-tap rule (`freerouting-plane-taps`,
/// `AutorouteConnectionRouter.java:65-75`): the net's planes on power
/// layers that the unconnected items reach, with the unconnected set as
/// the fallback; the unconnected set alone when there is no such plane.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FreeroutingPlaneTaps;

impl PlaneTapPolicy for FreeroutingPlaneTaps {
    fn id(&self) -> &'static str {
        "freerouting-plane-taps"
    }

    fn destinations(
        &self,
        board: &RoutingBoard,
        _task: &RouteTask,
        unconnected: &BTreeSet<ItemId>,
    ) -> Destinations {
        let planes = board.power_plane_areas(unconnected.iter().copied());
        if planes.is_empty() {
            Destinations {
                primary: unconnected.clone(),
                fallback: None,
            }
        } else {
            Destinations {
                primary: planes,
                fallback: Some(unconnected.clone()),
            }
        }
    }
}

/// Java's neck-down retry (`freerouting-neck-down`,
/// `retryConnectionNecked`, `:193-273`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FreeroutingNeckDown;

/// The half width of the neck-down traces in board units:
/// `max(max(1, neckWidth / 2), minTraceHalfWidth)` with
/// `neckWidth = (int) Math.round(Unit.scale(neck_width_um * resolution,
/// UM, unit))` (`:204-215`, the floor from `46bf9e2ef`).
pub fn neck_half_width(board: &RoutingBoard, neck_width_um: f64, min_trace_half_width: i32) -> i32 {
    let resolution = board.communication.resolution.max(1);
    let neck_width = round_to_i32(Unit::scale(
        neck_width_um * f64::from(resolution),
        Unit::Um,
        board.communication.unit,
    ));
    (neck_width / 2).max(1).max(min_trace_half_width)
}

/// The neck-down control: `control`'s half widths clamped to `half_width`
/// on every layer, each compensated width moved by the same amount
/// (`compensated = clamped + (old compensated - old)`, `:230-234`).
pub fn clamp_half_widths(control: &mut AutorouteControl, half_width: i32) {
    for i in 0..control.layer_count {
        let compensation = control.compensated_trace_half_width[i] - control.trace_half_width[i];
        control.trace_half_width[i] = control.trace_half_width[i].min(half_width);
        control.compensated_trace_half_width[i] = control.trace_half_width[i] + compensation;
    }
}

/// True if a neck-down to `half_width` narrows a trace somewhere: an
/// active layer of `control` has a wider half width (`:216-223`).
pub fn narrower_somewhere(control: &AutorouteControl, half_width: i32) -> bool {
    (0..control.layer_count)
        .any(|i| control.layer_active[i] && control.trace_half_width[i] > half_width)
}

impl NeckDownPolicy for FreeroutingNeckDown {
    fn id(&self) -> &'static str {
        "freerouting-neck-down"
    }

    fn retry(
        &mut self,
        board: &mut RoutingBoard,
        task: &RouteTask,
        attempt_in: NeckDownAttempt<'_>,
        _failed: &ConnectionOutcome,
        p: &ConnectionParams,
        ctx: &mut StageCtx,
    ) -> Option<ConnectionOutcome> {
        let neck_width_um = ctx.board_settings.neck_width_um();
        // PARITY: :140-141, only with a positive neck width.
        if neck_width_um.is_nan() || neck_width_um <= 0.0 {
            return None;
        }
        let half_width = neck_half_width(
            board,
            neck_width_um,
            attempt_in.control.min_trace_half_width,
        );
        if !narrower_somewhere(attempt_in.control, half_width) {
            return None;
        }
        // PARITY: :225-229, a new control on the board as it is now.
        let mut control = router_control(board, task, p, ctx)?;
        clamp_half_widths(&mut control, half_width);
        let first_new = board
            .communication
            .id_generator
            .max_generated_id()
            .wrapping_add(1);
        let result = attempt(
            board,
            attempt_in.start,
            attempt_in.dest,
            &control,
            attempt_in.work_limit,
            ctx,
            attempt_in.ripped,
            attempt_in.ripped_costs,
        );
        if result.state != AttemptState::Routed {
            return None;
        }
        // Q17.3, opt-in (`router.neck_down.widen_back`, unset: off).
        if crate::quality::neckdown::bounded_neck::widen_back_on(ctx.settings) {
            // The class widths: the retry's control before the clamp.
            if let Some(class) = router_control(board, task, p, ctx) {
                crate::quality::neckdown::bounded_neck::widen_after_retry(
                    board,
                    task.net,
                    first_new,
                    &class,
                    Vec::new(),
                    true,
                );
            }
        }
        pull_tight_after_connection(board, p.pull_tight_accuracy, &control.trace_costs);
        Some(ConnectionOutcome::new(result.state, result.details))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_maze_budget_doubles_per_pass_up_to_i32_max() {
        assert_eq!(maze_budget_ms(1), 100_000);
        assert_eq!(maze_budget_ms(2), 200_000);
        assert_eq!(maze_budget_ms(5), 1_600_000);
        // 100000 * 2^14 = 1638400000 < i32::MAX < 100000 * 2^15.
        assert_eq!(maze_budget_ms(15), 1_638_400_000);
        assert_eq!(maze_budget_ms(16), i64::from(i32::MAX));
        assert_eq!(maze_budget_ms(1000), i64::from(i32::MAX));
        assert_eq!(maze_work_budget(1), 100_000 * 64_000);
        assert_eq!(maze_work_budget(40), i32::MAX as u64 * UNITS_PER_NOMINAL_MS);
    }
}
