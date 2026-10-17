//! The `krt-grid` connection router (X05): one connection routed by the
//! vendored grid A* on the X04 raster, inserted as ordinary board copper
//! behind the exact DRC.
//!
//! Written from the X05 checklist's design, which follows the idea of
//! KiCadRoutingTools' single-ended routing (`py_router/single_ended_routing.py`,
//! `routing_context.py` and the frontier probe at `0486e799`); the ported
//! parts are in [`super::path`].
//!
//! **One connection.** [`KrtGrid::route`] takes the route sets of
//! Freerouting's connection router (A11.1: the unconnected items to the
//! connected set; a plane net from its connected set to the plane taps of
//! [`FreeroutingPlaneTaps`], then to the fallback), and for each set tries
//! the steps of [`KrtGrid::steps`]: 0.1 mm, then 0.05 mm, then 0.025 mm,
//! each on a fresh raster ([`raster::build_raster`]). The next step is
//! tried only if the previous one found no terminal cell, no path, or a
//! path that failed the insert. A window of more than [`MAX_FRAME_CELLS`]
//! cells per layer is not rasterized at that step (a bound on memory, not
//! in KRT; it depends on geometry only).
//!
//! **Search.** KRT's defaults (`routing_defaults.py`): heuristic weight
//! 2.3, turn cost 1000, a via cost of 75 steps of 0.1 mm at any step
//! (`via_cost_units`), via proximity cost 10; no attraction, no layer or
//! direction costs, the earned iteration extension off. The iteration
//! budget is the connection's [`ConnectionParams::work_budget`] (one work
//! unit per grid iteration, capped at `u32::MAX`), shared by all attempts
//! of the connection; every attempt's iterations are added to the board's
//! work meter (D5). The search runs with the frontier sink
//! (`route_with_frontier`).
//!
//! **Insert.** The path becomes [`Piece`]s ([`path::plan`]); both ends get
//! their connectors ([`terminals::connector`]); each via takes the first
//! via info of the net's via rule whose padstack spans both layers. All of
//! it is inserted in one B09 transaction ([`RoutingBoard::try_change`],
//! new items checked): the closure fails if a connector to a trace cannot
//! be made, if a new via's copper overlaps any pin's pad (D8), or if the
//! net has no fewer connected sets afterwards (the insert joined nothing;
//! items are compared by set count because the board's trace normalization
//! may replace the end items); the transaction then
//! reverts on any clearance violation of a new item. A reverted attempt
//! leaves the board's items and undo structure as before (the redo data is
//! dropped); only ids stay taken (B09). There is no ripup inside the stage
//! and no pull-tight (X06 adds it). Not covered by the gate:
//! `BoardRules::hole_clearance` (drill to drill where a padstack has no
//! copper on a layer), which neither the raster nor Rust's clearance check
//! tests.
//!
//! **Failure record.** On failure the outcome's [`FailureRecord::blocking`]
//! lists the items behind the last failed search's frontier
//! ([`super::frontier`]).
//!
//! Determinism (D2): the route sets are ordered sets, the vendored search
//! is seedless, the raster's order is fixed, and the transaction's checks
//! are sorted. Nothing reads a clock.

use std::collections::BTreeSet;

use freerouting::autoroute::AttemptState;
use freerouting::autoroute::batch::connection::{
    FreeroutingPlaneTaps, calculate_airline, net_contains_plane,
};
use freerouting::autoroute::stages::{
    BlockingItem, ConnectionOutcome, ConnectionParams, ConnectionRouter, Destinations,
    FailureRecord, PlaneTapPolicy, RouteTask, StageCtx, WorkStats,
};
use freerouting::board::items::{Item, ItemId};
use freerouting::board::structure::FixedState;
use freerouting::board::transaction::{CheckPolicy, TransactionResult};
use freerouting::board::{BasicBoard, RoutingBoard};
use freerouting::datastructures::IdGenerator;
use freerouting::geometry::{IntBox, IntPoint, Point, Polyline, TileShape};
use freerouting::rules::ViaInfoId;
use freerouting::settings::RouterSettings;
use krt_grid_router::{GridRouter, TrackMarginArg};

use super::frame::{Frame, window_for};
use super::frontier::blocking_items;
use super::path::{self, GridPoint, Piece, PlanError};
use super::raster::{self, NetRules, Raster};
use super::terminals::{self, Anchor, Connector, TraceSpec};
use super::{DEFAULT_GRID_STEP_UM, FINER_GRID_STEPS_UM, GridTask, step_in_board_units};

/// The registry id.
pub const KRT_GRID_ID: &str = "krt-grid";

/// The most cells per layer a window may have to be rasterized.
pub const MAX_FRAME_CELLS: usize = 1 << 22;

/// KRT `HEURISTIC_WEIGHT`.
pub const HEURISTIC_WEIGHT: f32 = 2.3;
/// KRT `TURN_COST`.
pub const TURN_COST: i32 = 1000;
/// KRT `VIA_COST`, in grid steps of [`REFERENCE_STEP_UM`].
pub const VIA_COST_STEPS: i64 = 75;
/// KRT `REFERENCE_GRID_STEP` in µm.
pub const REFERENCE_STEP_UM: i64 = 100;
/// KRT `VIA_PROXIMITY_COST`.
pub const VIA_PROXIMITY_COST: i32 = 10;

/// KRT `via_cost_units` at step `step_um`: `75 · 1000 · (100 / step_um)`.
pub fn via_cost_units(step_um: u32) -> i32 {
    let units = VIA_COST_STEPS * 1000 * REFERENCE_STEP_UM / i64::from(step_um.max(1));
    i32::try_from(units).unwrap_or(i32::MAX)
}

/// A test hook run on each raster before the search (X05.4 forces a raster
/// bug with it).
pub type RasterHook = fn(&mut Raster);

/// The `krt-grid` connection router.
#[derive(Clone, Copy, Debug)]
pub struct KrtGrid {
    /// The first grid step in µm.
    pub step_um: u32,
    /// Runs on each raster before the search; `None` in production.
    pub raster_hook: Option<RasterHook>,
}

/// What became of one step's attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepResult {
    /// Inserted and committed.
    Routed {
        /// Traces inserted (before the board's normalization).
        traces: usize,
        /// Vias inserted.
        vias: usize,
    },
    /// The window had more than [`MAX_FRAME_CELLS`] cells per layer.
    FrameTooLarge,
    /// These items have no terminal cell at this step (and a side had
    /// none at all).
    NoTerminal(Vec<ItemId>),
    /// The search ended without a path.
    NoPath,
    /// The connection's iteration budget was spent.
    BudgetSpent,
    /// The path could not be turned into copper.
    Plan(PlanError),
    /// The insert was rejected and reverted.
    Rejected(Rejection),
}

/// Why an insert was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rejection {
    /// A path end lies in no item of its side.
    NoOwner,
    /// No connector joins a path end to its item.
    Connector,
    /// No via of the net's rule spans the layers of a layer change.
    NoVia,
    /// The connector onto a trace could not be made.
    TraceConnector,
    /// A new via overlaps a pin's pad (D8).
    ViaInPad,
    /// The insert did not lower the net's number of connected sets.
    NotConnected,
    /// New items have clearance violations: their count and the first.
    Clearance {
        /// The number of violations.
        count: usize,
        /// The first: `(first item, second item, layer)`.
        first: (ItemId, ItemId, i32),
    },
}

/// One step's attempt, for the attempt log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepAttempt {
    /// The destination set: 0 primary, 1 the plane-tap fallback.
    pub dest_set: usize,
    /// The step in µm.
    pub step_um: u32,
    /// The window's cells per layer.
    pub cells: usize,
    /// Source and target terminal cells.
    pub terminals: (usize, usize),
    /// Grid iterations used.
    pub iterations: u32,
    /// The result.
    pub result: StepResult,
    /// The frontier's blocking items (a failed search only).
    pub blocking: Vec<BlockingItem>,
}

impl StepAttempt {
    /// One line of the attempt log.
    pub fn line(&self) -> String {
        format!(
            "krt-grid set {} step {} um: {} cells, {}/{} terminal cells, {} iterations, {:?}, {} blocking",
            self.dest_set,
            self.step_um,
            self.cells,
            self.terminals.0,
            self.terminals.1,
            self.iterations,
            self.result,
            self.blocking.len()
        )
    }
}

/// A routed connection's outcome and its attempt log.
#[derive(Clone, Debug)]
pub struct GridRun {
    /// What the stage returns.
    pub outcome: ConnectionOutcome,
    /// Every attempt, in order.
    pub attempts: Vec<StepAttempt>,
}

impl Default for KrtGrid {
    fn default() -> Self {
        KrtGrid::new(DEFAULT_GRID_STEP_UM)
    }
}

impl KrtGrid {
    /// The router starting at `step_um`.
    pub fn new(step_um: u32) -> Self {
        KrtGrid {
            step_um: step_um.max(1),
            raster_hook: None,
        }
    }

    /// The router for a job (the default step; no settings key yet).
    pub fn from_settings(_settings: &RouterSettings) -> Self {
        KrtGrid::default()
    }

    /// The steps tried in order: [`KrtGrid::step_um`], then the finer
    /// [`FINER_GRID_STEPS_UM`].
    pub fn steps(&self) -> Vec<u32> {
        std::iter::once(self.step_um)
            .chain(
                FINER_GRID_STEPS_UM
                    .into_iter()
                    .filter(|s| *s < self.step_um),
            )
            .collect()
    }

    /// Routes `task` and returns the outcome with the attempt log.
    pub fn route_logged(
        &mut self,
        board: &mut RoutingBoard,
        task: &RouteTask,
        p: &ConnectionParams,
        ctx: &StageCtx,
    ) -> GridRun {
        // The work of the whole call, route sets included, so the outcomes
        // of a pass sum to the meter's units.
        let meter_start = board.search_trees().work_meter().units();
        let (start, sets) = match route_sets(board, task) {
            Ok(sets) => sets,
            Err(state) => {
                let mut outcome = ConnectionOutcome::new(state, "");
                let meter = board.search_trees().work_meter();
                outcome.work = WorkStats {
                    units: meter.units().wrapping_sub(meter_start),
                };
                return GridRun {
                    outcome,
                    attempts: Vec::new(),
                };
            }
        };
        let max_id_before = board.communication.id_generator.max_generated_id();
        let budget = p.work_budget.min(u64::from(u32::MAX));
        let mut used = 0u64;
        let mut attempts = Vec::new();
        let mut routed_set = None;
        'sets: for (index, dest) in sets.iter().enumerate() {
            let grid_task = GridTask {
                net: task.net,
                start: start.clone(),
                dest: dest.clone(),
            };
            for step_um in self.steps() {
                if ctx.cancel.is_router_stop_requested() || used >= budget {
                    break 'sets;
                }
                let remaining = (budget - used) as u32;
                let attempt = self.attempt(board, &grid_task, index, step_um, remaining);
                used += u64::from(attempt.iterations);
                let routed = matches!(attempt.result, StepResult::Routed { .. });
                attempts.push(attempt);
                if routed {
                    routed_set = Some(index);
                    break 'sets;
                }
            }
        }
        let airline = calculate_airline(board, &start, &sets[routed_set.unwrap_or(0)]);
        let state = if routed_set.is_some() {
            AttemptState::Routed
        } else {
            AttemptState::Failed
        };
        // A failure's details are the whole attempt log, one attempt per
        // `; `-separated part.
        let details = if attempts.is_empty() {
            "krt-grid: no attempt".to_string()
        } else if routed_set.is_none() {
            attempts
                .iter()
                .map(StepAttempt::line)
                .collect::<Vec<_>>()
                .join("; ")
        } else {
            String::new()
        };
        let mut outcome = ConnectionOutcome::new(state, details.clone());
        outcome.first_new_id = max_id_before.wrapping_add(1);
        outcome.airline = airline;
        let meter = board.search_trees().work_meter();
        outcome.work = WorkStats {
            units: meter.units().wrapping_sub(meter_start),
        };
        outcome.work_limit_hit = used >= budget;
        if routed_set.is_none() {
            let mut failure = FailureRecord::new(details);
            failure.blocking = attempts
                .iter()
                .rev()
                .find(|a| !a.blocking.is_empty())
                .map(|a| a.blocking.clone())
                .unwrap_or_default();
            outcome.failure = Some(failure);
        }
        GridRun { outcome, attempts }
    }

    /// One attempt of `task` at `step_um` with at most `max_iterations`.
    fn attempt(
        &self,
        board: &mut RoutingBoard,
        task: &GridTask,
        dest_set: usize,
        step_um: u32,
        max_iterations: u32,
    ) -> StepAttempt {
        let step = step_in_board_units(board, step_um);
        let frame = window_for(board, task, step);
        let mut attempt = StepAttempt {
            dest_set,
            step_um,
            cells: frame.cell_count(),
            terminals: (0, 0),
            iterations: 0,
            result: StepResult::NoPath,
            blocking: Vec::new(),
        };
        if frame.cell_count() > MAX_FRAME_CELLS {
            attempt.result = StepResult::FrameTooLarge;
            return attempt;
        }
        let mut raster = raster::build_raster(board, task, &frame);
        if let Some(hook) = self.raster_hook {
            hook(&mut raster);
        }
        let (sources, targets) = terminals::for_task(board, &frame, task);
        attempt.terminals = (sources.cells.len(), targets.cells.len());
        if sources.cells.is_empty() || targets.cells.is_empty() {
            let mut empty = sources.empty.clone();
            empty.extend(&targets.empty);
            attempt.result = StepResult::NoTerminal(empty);
            return attempt;
        }
        terminals::mark(&mut raster.map, &sources.cells);
        terminals::mark(&mut raster.map, &targets.cells);
        let router = GridRouter::new(
            via_cost_units(step_um),
            HEURISTIC_WEIGHT,
            Some(TURN_COST),
            Some(VIA_PROXIMITY_COST),
            0,
            0,
            None,
            None,
            None,
            0,
            0,
            0,
            0,
            0,
        );
        let cells = |c: &[[i32; 3]]| -> Vec<(i32, i32, u8)> {
            c.iter().map(|&[i, j, l]| (i, j, l as u8)).collect()
        };
        let (found, iterations, frontier) = router.route_with_frontier(
            &raster.map,
            cells(&sources.cells),
            cells(&targets.cells),
            max_iterations,
            false,
            0,
            None,
            None,
            2,
            TrackMarginArg::Scalar(0.0),
            0,
            0.0,
            0.0,
            0,
            0,
        );
        attempt.iterations = iterations;
        board.search_trees().work_meter().add(u64::from(iterations));
        attempt.result = match found {
            Some(path) => match insert(board, task, &frame, &raster.rules, &path) {
                Ok((traces, vias)) => StepResult::Routed { traces, vias },
                Err(InsertFailure::Plan(e)) => StepResult::Plan(e),
                Err(InsertFailure::Rejected(r)) => StepResult::Rejected(r),
            },
            None => {
                attempt.blocking = blocking_items(board, task.net, &raster, &frontier);
                if iterations >= max_iterations {
                    StepResult::BudgetSpent
                } else {
                    StepResult::NoPath
                }
            }
        };
        attempt
    }
}

impl ConnectionRouter for KrtGrid {
    fn id(&self) -> &'static str {
        KRT_GRID_ID
    }

    fn route(
        &mut self,
        board: &mut RoutingBoard,
        task: &RouteTask,
        p: &ConnectionParams,
        ctx: &mut StageCtx,
    ) -> ConnectionOutcome {
        self.route_logged(board, task, p, ctx).outcome
    }
}

/// The start set and the destination sets of `task` (Freerouting's route
/// sets, A11.1), or the state that ends it before routing.
fn route_sets(
    board: &RoutingBoard,
    task: &RouteTask,
) -> Result<(BTreeSet<ItemId>, Vec<BTreeSet<ItemId>>), AttemptState> {
    let unconnected = board.unconnected_set(task.item, task.net);
    if unconnected.is_empty() {
        return Err(AttemptState::NoUnconnectedNets);
    }
    let connected = board.connected_set(task.item, task.net, false);
    if !net_contains_plane(board, task.net) {
        return Ok((unconnected, vec![connected]));
    }
    let on_plane = connected
        .iter()
        .any(|id| board.item(*id).is_some_and(Item::is_conduction_area));
    if on_plane {
        return Err(AttemptState::ConnectedToPlane);
    }
    let Destinations { primary, fallback } =
        FreeroutingPlaneTaps.destinations(board, task, &unconnected);
    let mut sets = vec![primary];
    sets.extend(fallback);
    Ok((connected, sets))
}

enum InsertFailure {
    Plan(PlanError),
    Rejected(Rejection),
}

/// True if an own-net pin or via at `at` spans layers `a` and `b`.
fn own_hole(board: &BasicBoard, net: i32, at: IntPoint, a: i32, b: i32) -> bool {
    let (lo, hi) = (a.min(b), a.max(b));
    let probe = TileShape::IntBox(IntBox::new(at.x, at.y, at.x, at.y));
    let p = Point::from(at);
    board
        .overlapping_tree_entries(&probe, lo)
        .iter()
        .filter_map(|e| board.item(e.item))
        .filter(|item| item.contains_net(net))
        .filter_map(Item::drill)
        .any(|d| {
            d.first_layer <= lo
                && d.last_layer >= hi
                && [lo, hi]
                    .iter()
                    .all(|&l| d.shape_on_layer(l).is_some_and(|s| s.contains(&p)))
        })
}

/// The first via info of `net`'s via rule whose padstack spans `a` and `b`.
fn via_for(board: &BasicBoard, net: i32, a: i32, b: i32) -> Option<ViaInfoId> {
    let rules = &board.rules;
    let class = rules
        .nets
        .get(net)
        .map(|n| rules.net_classes.get(n.net_class()));
    let rule = class
        .and_then(|c| c.via_rule())
        .or_else(|| rules.via_rules.nth(0))?;
    let (lo, hi) = (a.min(b), a.max(b));
    rules
        .via_rules
        .get(rule)
        .vias()
        .iter()
        .copied()
        .find(|&id| {
            board
                .library
                .padstacks
                .get(rules.via_infos.get(id).padstack)
                .is_some_and(|ps| ps.from_layer() <= lo && ps.to_layer() >= hi)
        })
}

/// True if the copper of via `via` overlaps a pin's pad on any layer.
pub fn via_in_pad(board: &BasicBoard, via: &Item) -> bool {
    let Some(drill) = via.drill() else {
        return false;
    };
    (drill.first_layer..=drill.last_layer).any(|layer| {
        let Some(shape) = drill.shape_on_layer(layer) else {
            return false;
        };
        let probe = TileShape::IntBox(shape.bounding_box());
        board
            .overlapping_tree_entries(&probe, layer)
            .iter()
            .filter_map(|e| board.item(e.item))
            .filter(|item| item.is_pin())
            .filter_map(Item::drill)
            .any(|pin| {
                pin.shape_on_layer(layer)
                    .is_some_and(|pad| pad.intersects(shape))
            })
    })
}

/// A connector with the end it belongs to.
struct End {
    connector: Connector,
    layer: i32,
}

/// The end of the path at `point` on `layer` in `items`, with its
/// connector.
fn path_end(
    board: &mut RoutingBoard,
    items: &BTreeSet<ItemId>,
    point: IntPoint,
    spec: TraceSpec,
) -> Result<End, Rejection> {
    let (owner, anchor): (ItemId, Anchor) =
        terminals::end_owner(board, items, point, spec.layer).ok_or(Rejection::NoOwner)?;
    let connector =
        terminals::connector(board, owner, anchor, point, spec).ok_or(Rejection::Connector)?;
    Ok(End {
        connector,
        layer: spec.layer,
    })
}

/// Applies `connector` to the first (`at_start`) or last corner of
/// `corners`.
fn apply_connector(corners: &mut Vec<IntPoint>, connector: Connector, at_start: bool) {
    match connector {
        Connector::Segment(p) if at_start => corners.insert(0, p),
        Connector::Segment(p) => corners.push(p),
        Connector::Moved(p) => {
            let index = if at_start { 0 } else { corners.len() - 1 };
            corners[index] = p;
        }
        Connector::None | Connector::ToTrace(_) => {}
    }
}

/// The corners of the end traces of `pieces`.
fn end_corner(pieces: &[Piece], first: bool) -> (i32, IntPoint) {
    let piece = if first {
        &pieces[0]
    } else {
        &pieces[pieces.len() - 1]
    };
    match piece {
        Piece::Trace { layer, corners } => {
            let p = if first {
                corners[0]
            } else {
                corners[corners.len() - 1]
            };
            (*layer, p)
        }
        Piece::Via { .. } => unreachable!("a plan starts and ends with a trace piece"),
    }
}

/// Inserts `path` for `task` in one transaction (module docs). Returns the
/// traces and vias inserted.
fn insert(
    board: &mut RoutingBoard,
    task: &GridTask,
    frame: &Frame,
    rules: &NetRules,
    path: &[GridPoint],
) -> Result<(usize, usize), InsertFailure> {
    let net = task.net;
    let mut pieces = path::plan(path, frame, |at, a, b| own_hole(board, net, at, a, b))
        .map_err(InsertFailure::Plan)?;
    let spec = |layer: i32| TraceSpec {
        layer,
        half_width: rules.half_width_on(layer),
        net,
        clearance_class: rules.trace_class,
    };
    let (start_layer, start_point) = end_corner(&pieces, true);
    let (end_layer, end_point) = end_corner(&pieces, false);
    let start = path_end(board, &task.start, start_point, spec(start_layer))
        .map_err(InsertFailure::Rejected)?;
    let end =
        path_end(board, &task.dest, end_point, spec(end_layer)).map_err(InsertFailure::Rejected)?;
    let last = pieces.len() - 1;
    if let Piece::Trace { corners, .. } = &mut pieces[0] {
        apply_connector(corners, start.connector, true);
    }
    if let Piece::Trace { corners, .. } = &mut pieces[last] {
        apply_connector(corners, end.connector, false);
    }
    let mut vias = Vec::new();
    for piece in &pieces {
        if let Piece::Via {
            from_layer,
            to_layer,
            ..
        } = piece
        {
            let via = via_for(board, net, *from_layer, *to_layer)
                .ok_or(InsertFailure::Rejected(Rejection::NoVia))?;
            vias.push(via);
        }
    }
    // The traces the ends connect to, read before the path's own traces
    // are inserted: the board's normalization may replace them (Java's
    // `connectToTrace` reads only the trace object, as A14's inserter).
    let trace_of = |end: &End| match end.connector {
        Connector::ToTrace(trace) => board.trace_object(trace),
        _ => None,
    };
    let (start_trace, end_trace) = (trace_of(&start), trace_of(&end));
    if matches!(start.connector, Connector::ToTrace(_)) && start_trace.is_none()
        || matches!(end.connector, Connector::ToTrace(_)) && end_trace.is_none()
    {
        return Err(InsertFailure::Rejected(Rejection::TraceConnector));
    }
    let max_id_before = board.communication.id_generator.max_generated_id();
    let components_before = board.connected_sets(net).len();
    let (start_point, end_point) = (end_corner(&pieces, true).1, end_corner(&pieces, false).1);
    let result = board.try_change(
        |b| {
            let mut traces = 0;
            let mut via_iter = vias.iter();
            for piece in &pieces {
                match piece {
                    Piece::Trace { layer, corners } => {
                        if corners.windows(2).any(|w| w[0] != w[1]) {
                            // While the pass marks the changed area, the
                            // board's normalization splits other traces only
                            // inside it (as `connect_to_trace_object` does).
                            if b.changed_area().is_some() {
                                for c in corners {
                                    b.join_changed_area(c.to_float(), *layer);
                                }
                            }
                            let points: Vec<Point> =
                                corners.iter().copied().map(Point::from).collect();
                            b.insert_trace(
                                Polyline::from_points(&points),
                                *layer,
                                rules.half_width_on(*layer),
                                &[net],
                                rules.trace_class,
                                FixedState::Unfixed,
                            );
                            traces += 1;
                        }
                    }
                    Piece::Via { at, .. } => {
                        let info = b
                            .rules
                            .via_infos
                            .get(*via_iter.next().expect("a via per piece"))
                            .clone();
                        b.insert_via(
                            info.padstack,
                            *at,
                            &[net],
                            info.clearance_class,
                            FixedState::Unfixed,
                            info.attach_smd_allowed,
                        );
                    }
                }
            }
            for (end, point, object) in [
                (&start, start_point, &start_trace),
                (&end, end_point, &end_trace),
            ] {
                if let Some(object) = object {
                    let s = spec(end.layer);
                    if !b.connect_to_trace_object(point, object, s.half_width, s.clearance_class) {
                        return Err(Rejection::TraceConnector);
                    }
                }
            }
            let new_vias: Vec<ItemId> = b
                .items()
                .filter(|i| i.id().0 > max_id_before && i.is_via())
                .map(Item::id)
                .collect();
            if new_vias
                .iter()
                .any(|&id| b.item(id).is_some_and(|via| via_in_pad(b, via)))
            {
                return Err(Rejection::ViaInPad);
            }
            if b.connected_sets(net).len() >= components_before {
                return Err(Rejection::NotConnected);
            }
            Ok((traces, new_vias.len()))
        },
        CheckPolicy::NewItemsOnly,
    );
    match result {
        TransactionResult::Committed(counts) => Ok(counts),
        TransactionResult::Reverted { violations, .. } => {
            drop_redo(board);
            let v = &violations[0];
            Err(InsertFailure::Rejected(Rejection::Clearance {
                count: violations.len(),
                first: (v.first_item, v.second_item, v.layer),
            }))
        }
        TransactionResult::Aborted(rejection) => {
            drop_redo(board);
            Err(InsertFailure::Rejected(rejection))
        }
    }
}

/// Drops the redo data a reverted transaction leaves, so the undo
/// structure is the one before it (B09: the next snapshot drops it).
fn drop_redo(board: &mut RoutingBoard) {
    board.generate_snapshot();
    board.pop_snapshot();
}
