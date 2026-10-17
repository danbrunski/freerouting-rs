//! The stage traits of the routing pipeline (decision D9, A10.1) and their
//! registry (A10.2).
//!
//! A routing job is a pipeline of interchangeable stages: fanout, net
//! ordering, connection routing, the ripup policy, the pass controller, the
//! plane-tap and neck-down policies inside connection routing, the
//! optimizer, and special [`BoardStage`]s (diagnostics, plane repair,
//! differential pairs, length matching). Each is a trait here; Java
//! Freerouting's stages are the first implementations and the parity
//! baseline, and other algorithms (the X series) plug in behind the same
//! traits without changing them.
//!
//! **Rules every implementation keeps.**
//! - A stage changes the one [`RoutingBoard`] through its API only, so DRC,
//!   undo, the SES writer, the GUI and the plugin work the same whichever
//!   algorithm ran. It keeps no board reference across calls: the batch
//!   loop may swap the board for a restored one between any two calls.
//! - State a stage keeps between calls is keyed by [`ItemId`] or net number,
//!   and `reset` clears it: a restore brings back a board whose ids may have
//!   been reused or removed since.
//! - Budgets are work, never time: a [`ConnectionParams::work_budget`]
//!   counts search-tree nodes visited (B02.7, 64,000 units per nominal
//!   millisecond) on the board's own meter, which lives in its
//!   `SearchTreeManager`, so a stage never holds a meter. A cancel token or
//!   deadline may only stop work, never change how it is done (D2).
//! - Same input and settings, same output (D2): no iteration over a
//!   `HashMap` or `HashSet` reaches a result, and randomness is seeded.
//!
//! **Dispatch** is dynamic between stages (one `Box<dyn ...>` call per
//! connection costs nothing measurable) and static inside them: the
//! Freerouting connection router's maze search stays monomorphized over
//! A04's sinks.
//!
//! **Registry and selection.** [`StageRegistry`] maps ids to factories, one
//! sorted map per trait. [`StageSelection::from_settings`] reads the ids:
//! - `router.autorouter.algorithm` (Java's key) names a bundle of
//!   ordering, connection router, ripup policy, pass controller, plane taps
//!   and neck-down. Any id that is not a registered bundle, including the
//!   legacy `freerouting-router-v19`, logs Java's warning and becomes
//!   `freerouting-router` (`RoutingPipeline.normalizeRouterAlgorithm`).
//! - `router.optimizer.algorithm` (Java's key) names the optimizer, with
//!   the same warn-and-default (`BatchOptimizer.normalizeAlgorithm`).
//! - The per-stage keys `router.fanout.algorithm`,
//!   `router.autorouter.{ordering, connection_router, ripup_policy,
//!   pass_controller}`, `router.special.{plane_taps, neck_down}` override
//!   one stage, and `router.special.stages` lists extra board stages. Java
//!   never sees these keys, so an unknown id there is an error
//!   ([`UnknownStage`]) rather than a silent default (A00 open question 1,
//!   accepted 2026-10-15).
//!
//! The resolved selection ([`StageSelection::entries`]) goes into the run's
//! log and `RunSummary::stages`, never into the SES, which stays
//! byte-identical to Java's.
//!
//! The Freerouting ids: `freerouting-ordering` (A10.4),
//! `freerouting-ripup` (A10.5), `freerouting-passes` (A10.3),
//! `freerouting-unrouted-report` (A10.7), `freerouting-connection` (A11.1),
//! `freerouting-plane-taps` and `freerouting-neck-down` (A11.2);
//! `freerouting-fanout` (A12.3); `freerouting-optimizer` (O06,
//! `optimize::batch::FreeroutingOptimizer`). The registry, the selection and the placeholders are in the
//! `registry` submodule (`stages/registry.rs`), re-exported here.

use std::collections::BTreeSet;

use freerouting_geometry::FloatPoint;

use crate::autoroute::AttemptState;
use crate::autoroute::control::AutorouteControl;
pub use crate::autoroute::events::Phase;
use crate::autoroute::events::{CancelToken, RouterEvents};
use crate::autoroute::path::RipupCosts;
use crate::board::items::ItemId;
use crate::board::routing_board::RoutingBoard;
use crate::board::search::WorkLimit;
use crate::settings::RouterSettings;
use crate::settings::board::BoardRouterSettings;
use crate::settings::layers::TraceCostFactor;

// ---------------------------------------------------------------------
// Shared types
// ---------------------------------------------------------------------

/// What every stage call gets besides the board: the settings, the cancel
/// token and the event sink. A stage reads its work budget from the
/// settings or its [`ConnectionParams`], and starts its `WorkLimit` on the
/// board's own meter.
pub struct StageCtx<'a> {
    /// The resolved settings of the job.
    pub settings: &'a RouterSettings,
    /// The settings applied to the board (Java's getters, C03.7).
    pub board_settings: &'a BoardRouterSettings,
    /// The two-level stop request. A stage checks it between work units
    /// and returns early; it never changes how the work is done.
    pub cancel: &'a CancelToken,
    /// Where progress and board changes go.
    pub events: &'a dyn RouterEvents,
}

/// One item to route (an entry of Java's `getAutorouteItems` list).
///
/// [`NetOrdering::select`] yields one task per item, with `net` the item's
/// first net (0 for none), which is the ordering key. The pass runner
/// routes every net of the item in turn, as Java does
/// (`AutoroutePassRunner.runSingleThread`, the loop over `netCount()`),
/// handing the connection router a task with that net.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteTask {
    /// The item to connect.
    pub item: ItemId,
    /// The net being routed.
    pub net: i32,
    /// True if `select` put the item in the plane list (a net with a plane
    /// that needs routing).
    pub is_plane: bool,
}

/// The parameters of one connection attempt (Java's `AutorouteControl`
/// fields that the pass sets).
#[derive(Clone, Debug, PartialEq)]
pub struct ConnectionParams {
    /// The pass number, from 1.
    pub pass: u32,
    /// Whether the search may rip up other connections.
    pub ripup_allowed: bool,
    /// The ripup cost ([`RipupPolicy::ripup_cost`]).
    pub ripup_cost: i32,
    /// The via cost (plane or regular).
    pub via_cost: i32,
    /// The trace cost factors per layer.
    pub trace_costs: Vec<TraceCostFactor>,
    /// Whether unconnected vias are removed after insertion.
    pub remove_unconnected_vias: bool,
    /// The search budget in work units (B02.7), never time.
    pub work_budget: u64,
    /// The accuracy of the pull-tight after a routed connection (Java
    /// `BatchAutorouter.tracePullTightAccuracy`; A11).
    pub pull_tight_accuracy: i32,
    /// True if a routed connection with new clearance violations is
    /// rejected ([`RipupPolicy::strict_check`] of this pass; A11).
    pub strict_check: bool,
}

/// A failed search's account of why it failed: A04's failure record. Only
/// the summary exists until A04 ports the search; A04 adds its fields here.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct FailureRecord {
    /// A one-line reason.
    pub reason: String,
}

impl FailureRecord {
    /// A record with only a reason.
    pub fn new(reason: impl Into<String>) -> Self {
        FailureRecord {
            reason: reason.into(),
        }
    }
}

/// The work a call did, in work units (B02.7).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WorkStats {
    /// Search-tree nodes visited.
    pub units: u64,
}

/// The result of one connection attempt.
#[derive(Clone, Debug, PartialEq)]
pub struct ConnectionOutcome {
    /// The attempt's state (Java `AutorouteAttemptState`).
    pub state: AttemptState,
    /// Java's details text.
    pub details: String,
    /// The items ripped up, in `TreeSet<Item>` order (highest id first).
    pub ripped: BTreeSet<ItemId>,
    /// The ripup cost of each ripped item, in Java's `LinkedHashMap` order.
    pub ripped_costs: Vec<(ItemId, i32)>,
    /// The first id the attempt could have used for a new item (the id
    /// generator before the attempt).
    pub first_new_id: i32,
    /// Why the search failed, if it did.
    pub failure: Option<FailureRecord>,
    /// True if the attempt stopped at its work budget (Q06 reads it).
    pub work_limit_hit: bool,
    /// The work the attempt did.
    pub work: WorkStats,
    /// The airline of the attempt: the nearest pin or via centres of the
    /// start and destination sets (Java `AutorouteAirlineCalculator`), for
    /// the GUI's routing overlay (A11.1). It decides nothing.
    pub airline: Option<(FloatPoint, FloatPoint)>,
}

impl ConnectionOutcome {
    /// An outcome with only a state and details.
    pub fn new(state: AttemptState, details: impl Into<String>) -> Self {
        ConnectionOutcome {
            state,
            details: details.into(),
            ripped: BTreeSet::new(),
            ripped_costs: Vec::new(),
            first_new_id: 0,
            failure: None,
            work_limit_hit: false,
            work: WorkStats::default(),
            airline: None,
        }
    }
}

/// What a pass left behind, as the pass controller and the ordering see it.
/// The driver (A13) fills it after the pass and the tail removal.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PassReport {
    /// The pass number, from 1.
    pub pass: u32,
    /// Java's `autoroutePass` result: the pass routed or failed something.
    pub continue_routing: bool,
    /// The router score after the pass, in `f32` as Java.
    pub score: f32,
    /// Incomplete connections after the pass.
    pub incomplete: usize,
    /// The nets with incomplete connections after the pass.
    pub incomplete_nets: BTreeSet<i32>,
    /// True if a router stop was requested (Java
    /// `isStopAutoRouterRequested`).
    pub router_stop_requested: bool,
    /// True if the job timed out.
    pub timed_out: bool,
    /// Boards in the history, including the one stored before this pass.
    pub history_size: usize,
    /// The best score in the history (Java `getMaxScore`, `-inf` if empty).
    pub history_max_score: f32,
}

/// Why the batch loop stops.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    /// A pass found nothing to route (`autoroutePass` returned false).
    Completed,
    /// A router stop was requested (user, timeout, `max_items`).
    StopRequested,
    /// The pass number passed `max_passes`.
    MaxPasses,
    /// A restore found no board to go back to.
    HistoryExhausted,
    /// The restored board ranked below `BOARD_RANK_LIMIT`.
    RankLimit,
    /// The pass-local no-improvement counter reached its limit.
    Stagnation,
    /// No pass improved the global best score for the limit.
    GlobalStagnation,
}

impl StopReason {
    /// True if Java requests a router stop for this reason, which skips the
    /// final tail removal. Only a pass with nothing left to route ends the
    /// loop without one.
    pub fn requests_router_stop(self) -> bool {
        !matches!(self, StopReason::Completed)
    }
}

/// What the controller says before a pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PassStart {
    /// Run the pass. With `stop_router` the driver first requests a router
    /// stop (a timed-out job), so the pass routes nothing and the restore
    /// check sees the stop.
    Run {
        /// Request a router stop before running.
        stop_router: bool,
    },
    /// Do not run the pass.
    Stop(StopReason),
}

/// What the controller asks the driver to do after a pass. The driver
/// applies one decision, reports what came of it with
/// [`PassController::after_action`], and gets the next decision, until
/// [`Continue`](PassDecision::Continue) or [`Stop`](PassDecision::Stop).
/// This is Java's order inside one iteration of `AutorouteBatchLoop.run`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PassDecision {
    /// Go on with the next pass.
    Continue,
    /// Restore a board from the history (`BoardHistory.restore`) and report
    /// it with [`ActionOutcome::Restored`].
    Restore {
        /// The restore limit per board.
        max_restore_count: i32,
    },
    /// Remove tails including fanout vias (`removeTails(NONE)`) and report
    /// with [`ActionOutcome::FanoutRecovered`].
    FanoutRecovery,
    /// Rip the last-mile blockers (A13.3) and report with
    /// [`ActionOutcome::LastMile`].
    LastMileRipup,
    /// Stop the loop.
    Stop(StopReason),
}

/// A board after a driver action: its router score and incomplete count.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoardScore {
    /// The router score.
    pub score: f32,
    /// Incomplete connections.
    pub incomplete: usize,
}

/// The board a history restore brought back.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RestoredBoard {
    /// Its 1-based rank in the history after the restore (Java `getRank`;
    /// `None` for Java's -1).
    pub rank: Option<usize>,
    /// Its score and incomplete count.
    pub board: BoardScore,
}

/// What came of a [`PassDecision`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ActionOutcome {
    /// The restore: `None` if the history had no board to give.
    Restored(Option<RestoredBoard>),
    /// The board after the fanout recovery's tail removal.
    FanoutRecovered(BoardScore),
    /// The last-mile ripup: the items removed, and the board after it.
    LastMile {
        /// Items removed.
        removed: usize,
        /// The board after the ripup.
        board: BoardScore,
    },
}

/// What the controller says once the loop ends.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FinishDecision {
    /// Restore the best board of the history (`restoreBestBoard`).
    pub restore_best: bool,
    /// Run the final `removeTails(NONE)`.
    pub remove_tails: bool,
}

/// What a fanout run reports (Java `FanoutRunSummary`, without its time
/// and the escape percentage).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FanoutSummary {
    /// Passes completed.
    pub completed_passes: u32,
    /// SMD pins with a net that are escaped after the run
    /// (`BoardStatistics.fanout.escapedCount`, A12.3).
    pub escaped: usize,
    /// SMD pins with a net (`BoardStatistics.fanout.totalSmdPins`, A12.3).
    pub total: usize,
    /// True if the fanout deadline stopped it.
    pub timed_out: bool,
}

/// What an optimizer run reports.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct OptimizerSummary {
    /// Passes completed.
    pub passes: u32,
    /// Passes the gate accepted (O06).
    pub accepted_passes: u32,
    /// Items attempted (Java `totalItemsOptimized`, O06).
    pub items: u32,
    /// True if a preflight guard skipped the stage (O06).
    pub bypassed: bool,
    /// True if the incumbent was restored at the end (O06).
    pub restored_best: bool,
    /// Java's job log lines of the stage, in order (O06).
    pub log: Vec<crate::job::LogLine>,
}

/// Where a [`BoardStage`] runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum StagePhase {
    /// Before the fanout.
    BeforeFanout,
    /// After the fanout, before the routing passes.
    AfterFanout,
    /// After each routing pass.
    AfterPass,
    /// After the routing passes, before the optimizer.
    AfterRouting,
    /// After the optimizer.
    AfterOptimizer,
}

/// What a [`BoardStage`] reports.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StageReport {
    /// Log lines, in order.
    pub log: Vec<String>,
}

/// The destination sets of one connection ([`PlaneTapPolicy`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Destinations {
    /// The items to route to first.
    pub primary: BTreeSet<ItemId>,
    /// The items to retry with if routing to `primary` fails.
    pub fallback: Option<BTreeSet<ItemId>>,
}

// ---------------------------------------------------------------------
// The traits
// ---------------------------------------------------------------------

/// The fanout stage: escapes SMD pins before the routing passes.
///
/// May add and remove traces and vias. Keeps nothing across runs.
pub trait FanoutStage {
    /// The registry id.
    fn id(&self) -> &'static str;
    /// Runs the fanout passes on `board`.
    fn run(&mut self, board: &mut RoutingBoard, ctx: &mut StageCtx) -> FanoutSummary;
}

/// Picks the items a pass routes and their order.
///
/// Never changes the board. State kept between passes (Freerouting's
/// anti-oscillation counters) is keyed by net or item id and cleared by
/// [`reset`](NetOrdering::reset), which the driver calls after every board
/// restore.
///
/// A minimal custom ordering, highest net number first:
///
/// ```
/// use freerouting::autoroute::stages::{NetOrdering, PassReport, RouteTask};
/// use freerouting::board::RoutingBoard;
///
/// struct HighNetsFirst;
///
/// impl NetOrdering for HighNetsFirst {
///     fn id(&self) -> &'static str {
///         "high-nets-first"
///     }
///     fn select(&mut self, board: &RoutingBoard) -> Vec<RouteTask> {
///         board
///             .items()
///             .filter(|item| item.is_pin() && item.net_count() > 0)
///             .map(|item| RouteTask { item: item.id(), net: item.net_number(0), is_plane: false })
///             .collect()
///     }
///     fn order(&mut self, mut tasks: Vec<RouteTask>, _: &RoutingBoard, _: u32) -> Vec<RouteTask> {
///         tasks.sort_by_key(|task| (std::cmp::Reverse(task.net), task.item.0));
///         tasks
///     }
///     fn on_pass_end(&mut self, _: &PassReport) {}
///     fn reset(&mut self) {}
/// }
///
/// let mut ordering = HighNetsFirst;
/// let tasks = vec![
///     RouteTask { item: freerouting::board::items::ItemId(7), net: 1, is_plane: false },
///     RouteTask { item: freerouting::board::items::ItemId(3), net: 2, is_plane: false },
/// ];
/// let board = freerouting::drc::load_design(include_bytes!("../../../../fixtures/empty_board.dsn")).unwrap();
/// let ordered = ordering.order(tasks, &board, 2);
/// assert_eq!(ordered[0].net, 2);
/// ```
pub trait NetOrdering {
    /// The registry id.
    fn id(&self) -> &'static str;
    /// The items to route this pass, in the pass-1 order.
    fn select(&mut self, board: &RoutingBoard) -> Vec<RouteTask>;
    /// The order for pass `pass`; the driver calls it for passes after the
    /// first.
    fn order(&mut self, tasks: Vec<RouteTask>, board: &RoutingBoard, pass: u32) -> Vec<RouteTask>;
    /// Called after each pass with what it left behind.
    fn on_pass_end(&mut self, report: &PassReport);
    /// Clears all state kept between passes (after a restore).
    fn reset(&mut self);
}

/// Routes one connection.
///
/// May add, change and remove traces and vias (insertion, ripup, pull
/// tight). Starts its work limit at `p.work_budget` on the board's meter.
/// Keeps nothing across calls that a restore could invalidate.
pub trait ConnectionRouter {
    /// The registry id.
    fn id(&self) -> &'static str;
    /// Routes `task` with the parameters `p`.
    fn route(
        &mut self,
        board: &mut RoutingBoard,
        task: &RouteTask,
        p: &ConnectionParams,
        ctx: &mut StageCtx,
    ) -> ConnectionOutcome;
}

/// The ripup cost schedule, the strict-DRC rule and what happens after a
/// repeated failure.
///
/// Only [`after_failure`](RipupPolicy::after_failure) changes the board
/// (it may remove traces and vias). State kept between calls is keyed by
/// item id.
pub trait RipupPolicy {
    /// The registry id.
    fn id(&self) -> &'static str;
    /// The ripup cost in `phase` at pass `pass`.
    fn ripup_cost(&self, phase: Phase, pass: u32, start_ripup_costs: i32) -> i32;
    /// True if pass `pass` rejects connections with new clearance
    /// violations.
    fn strict_check(&self, pass: u32, strict_drc: bool) -> bool;
    /// Called after `task` failed; `failures` is the item's failure count
    /// including this one.
    fn after_failure(&mut self, board: &mut RoutingBoard, task: &RouteTask, failures: u32);
}

/// Decides when the routing passes stop and when to go back to an earlier
/// board. Sees no board: the driver reports scores and counts, and applies
/// the decisions.
pub trait PassController {
    /// The registry id.
    fn id(&self) -> &'static str;
    /// Before pass `pass`; `timed_out` is the job's state.
    fn before_pass(&mut self, pass: u32, timed_out: bool) -> PassStart;
    /// After pass `pass`: the first decision.
    fn after_pass(&mut self, r: &PassReport) -> PassDecision;
    /// After the driver applied a decision: the next decision.
    fn after_action(&mut self, outcome: &ActionOutcome) -> PassDecision;
    /// Once the loop ended: `final_score` is the board's score,
    /// `history_max_score` the history's best and `router_stopped` whether
    /// a router stop was requested.
    fn finish(
        &mut self,
        final_score: f32,
        history_max_score: f32,
        router_stopped: bool,
    ) -> FinishDecision;
}

/// Picks the destination items of a connection (plane taps). Never changes
/// the board.
pub trait PlaneTapPolicy {
    /// The registry id.
    fn id(&self) -> &'static str;
    /// The destinations of `task`, given its unconnected items.
    fn destinations(
        &self,
        board: &RoutingBoard,
        task: &RouteTask,
        unconnected: &BTreeSet<ItemId>,
    ) -> Destinations;
}

/// What a failed attempt hands to the [`NeckDownPolicy`] (A11.2): Java's
/// `retryConnectionNecked` gets the attempt's own route sets, control and
/// work limit, and adds to its ripped set.
pub struct NeckDownAttempt<'a> {
    /// The start set the attempt routed from.
    pub start: &'a BTreeSet<ItemId>,
    /// The destination set it routed to (the fallback, if it was used).
    pub dest: &'a BTreeSet<ItemId>,
    /// The attempt's control (its layers and trace half widths).
    pub control: &'a AutorouteControl,
    /// The attempt's maze-search limit; a retry continues it.
    pub work_limit: &'a WorkLimit,
    /// The items ripped so far, in `TreeSet<Item>` order.
    pub ripped: &'a mut BTreeSet<ItemId>,
    /// Their ripup costs, in Java's `LinkedHashMap` order.
    pub ripped_costs: &'a mut RipupCosts,
}

/// Retries a failed connection with narrower traces (neck-down). May add
/// and remove traces and vias like a [`ConnectionRouter`].
pub trait NeckDownPolicy {
    /// The registry id.
    fn id(&self) -> &'static str;
    /// A retry of `task` after `failed`, or `None` to keep the failure.
    fn retry(
        &mut self,
        board: &mut RoutingBoard,
        task: &RouteTask,
        attempt: NeckDownAttempt<'_>,
        failed: &ConnectionOutcome,
        p: &ConnectionParams,
        ctx: &mut StageCtx,
    ) -> Option<ConnectionOutcome>;
}

/// A special stage run at a fixed [`StagePhase`]: diagnostics, plane
/// repair, differential pairs, length matching. What it may change is its
/// own contract; a diagnostic changes nothing.
pub trait BoardStage {
    /// The registry id.
    fn id(&self) -> &'static str;
    /// Where it runs.
    fn phase(&self) -> StagePhase;
    /// Runs it.
    fn run(&mut self, board: &mut RoutingBoard, ctx: &mut StageCtx) -> StageReport;
}

/// The optimizer stage (O06).
pub trait Optimizer {
    /// The registry id.
    fn id(&self) -> &'static str;
    /// Runs the optimizer passes on `board`.
    fn run(&mut self, board: &mut RoutingBoard, ctx: &mut StageCtx) -> OptimizerSummary;
}

mod registry;

pub use registry::{
    Bundle, DEFAULT_FANOUT, DEFAULT_OPTIMIZER_ALGORITHM, DEFAULT_ROUTER_ALGORITHM,
    FREEROUTING_BUNDLE, Factory, NotPorted, Pipeline, StageChoice, StageRegistry, StageSelection,
    UnknownStage,
};
