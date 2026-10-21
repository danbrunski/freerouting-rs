//! One routing pass over the board (A11.4, A11.5) and the optimizer's
//! entry point (A11.6).
//!
//! Port of `AutoroutePassRunner.runSingleThread` (`:40-253`),
//! `BatchAutorouter.removeTails` (`:534-550`) and
//! `BatchAutorouter.autoroutePassesForOptimizingItem` (`:253-289`) at
//! `a773a70ec`.
//!
//! [`PassRunner`] holds what Java's `BatchAutorouter` instance keeps for its
//! passes: the unconnected-via rule, the trace costs (with or without the
//! preferred directions), the start ripup costs, the pull-tight accuracy,
//! `max_items` and the count of items routed so far across passes
//! (`totalItemsRouted`). The stages come in per call ([`PassStages`]), so
//! the batch loop (A13) can swap the board between passes.
//!
//! **One pass, in Java's order.**
//! - `select`, then `order` when `pass > 1`; an empty list ends the pass with
//!   nothing to route.
//! - For every item and every net of the item: stop on a router stop; when
//!   `max_items` is reached, request a **full** stop (`requestStop`, so the
//!   optimizer is skipped too) and stop; count the item; start marking the
//!   changed area; route with a fresh ripped set. `Routed` counts as routed,
//!   `AlreadyConnected`, `NoUnconnectedNets` and `ConnectedToPlane` as
//!   skipped; anything else is recorded in the board's failure log and the
//!   ripup policy's `after_failure` gets the item's failure count.
//! - At the end: [`PassRunner::remove_tails`] (`NONE` when the pass removes
//!   unconnected vias, else `FANOUT_VIA`), the incomplete count and nets from
//!   the routing ledger (Java `routingLedger()`), and the ordering's
//!   `on_pass_end`.
//! - The result is Java's: something was routed or failed.
//!
//! **Events.** A [`Progress`] after each connection and one at the end of the
//! pass, only while someone listens: the counters, the ledger's incomplete
//! count, the statistics of the last ten items (Java's
//! `PROGRESS_STATISTICS_ITEM_INTERVAL`) and the attempt's airline. Java
//! throttles its updates to four a second; the port sends every one and the
//! channel drops what a slow listener cannot take, so nothing depends on
//! time.
//!
//! **Audit.** With `router.debug.audit_incremental` (P02.1) the runner
//! compares the board's search trees with a rebuild after each connection
//! that ripped items or failed, and after the tail removal
//! ([`IncrementalAudit`]). The audit only reads.
//!
//! Not ported: Java's trace logging, its per-net debug dumps and its
//! benchmark profile. A Java exception inside the pass returns false; the
//! port has no such path.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::autoroute::AttemptState;
use crate::autoroute::batch::connection::{connection_params, pull_tight_after_connection};
use crate::autoroute::events::{Phase, Progress, flush_journal};
use crate::autoroute::stages::{
    ConnectionOutcome, ConnectionRouter, NetOrdering, PassReport, RipupPolicy, RouteTask, StageCtx,
    StageRegistry, StageSelection,
};
use crate::board::RoutingBoard;
use crate::board::items::StopConnectionOption;
use crate::board::search::audit::{AuditPoint, AuditReport, IncrementalAudit};
use crate::scoring::BoardStatistics;
use crate::settings::RouterSettings;
use crate::settings::board::BoardRouterSettings;
use crate::settings::layers::TraceCostFactor;

/// Progress statistics are recomputed after this many items (Java
/// `BatchAutorouter.PROGRESS_STATISTICS_ITEM_INTERVAL`).
pub const PROGRESS_STATISTICS_ITEM_INTERVAL: usize = 10;

/// The stages one pass calls.
pub struct PassStages<'a> {
    /// Picks and orders the items.
    pub ordering: &'a mut dyn NetOrdering,
    /// Routes one connection.
    pub router: &'a mut dyn ConnectionRouter,
    /// The ripup costs, the strict check and the repeated-failure ripup.
    pub ripup: &'a mut dyn RipupPolicy,
}

/// Sees every connection of a pass and its tail removal (the `pass` probe,
/// diagnostics). The default methods do nothing.
pub trait PassObserver {
    /// The pass's item list, in routing order (empty: nothing to route).
    fn on_items(&mut self, _board: &RoutingBoard, _tasks: &[RouteTask]) {}
    /// Before `task` is routed.
    fn before_connection(&mut self, _board: &RoutingBoard, _task: &RouteTask) {}
    /// After `task` was routed and a failure was handled.
    fn after_connection(
        &mut self,
        _board: &RoutingBoard,
        _task: &RouteTask,
        _outcome: &ConnectionOutcome,
    ) {
    }
    /// Before the tail removal at the end of the pass.
    fn before_tails(&mut self, _board: &RoutingBoard) {}
    /// After the tail removal; `removed` is `remove_trace_tails`' result.
    fn after_tails(&mut self, _board: &RoutingBoard, _removed: bool) {}
}

/// A [`PassObserver`] that sees nothing.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoObserver;

impl PassObserver for NoObserver {}

/// What a pass did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PassOutcome {
    /// Java's `autoroutePass` result: something was routed or failed.
    pub continue_routing: bool,
    /// The item list was empty.
    pub nothing_to_route: bool,
    /// Items in the pass's list.
    pub items: usize,
    /// Connections routed.
    pub routed: usize,
    /// Connections that failed.
    pub failed: usize,
    /// Connections skipped (already connected, no unconnected nets,
    /// connected to a plane).
    pub skipped: usize,
    /// Items ripped up, summed over the connections.
    pub ripped: usize,
    /// Incomplete connections after the tail removal.
    pub incomplete: usize,
    /// The nets with incomplete connections after the tail removal.
    pub incomplete_nets: BTreeSet<i32>,
    /// True if the pass requested a full stop (`max_items` reached).
    pub max_items_reached: bool,
    /// Java's job log lines of the pass, in order.
    pub log: Vec<String>,
}

/// The per-instance state of Java's `BatchAutorouter` that its passes read.
#[derive(Clone, Debug, PartialEq)]
pub struct PassRunner {
    /// The phase the progress events name.
    pub phase: Phase,
    /// `removeUnconnectedVias`: true when the job's fanout is off and in the
    /// optimizer's passes.
    pub remove_unconnected_vias: bool,
    /// The trace costs of the searches and the pull-tight.
    pub trace_costs: Vec<TraceCostFactor>,
    /// `startRipupCosts`.
    pub start_ripup_costs: i32,
    /// `tracePullTightAccuracy`.
    pub pull_tight_accuracy: i32,
    /// `router.max_items`; not positive means no limit.
    pub max_items: i32,
    /// `totalItemsRouted`, across the passes of this runner.
    total_items_routed: i64,
    /// P02.1's audit, when `router.debug.audit_incremental` is on.
    audit: Option<IncrementalAudit>,
    /// The progress events' board statistics by board state (K13.9).
    statistics: StatisticsMemo,
}

/// The board statistics of the progress events, by the board's
/// [`state_hash`](crate::board::RoutingBoard::state_hash) (K13.9). The
/// passes of a run that changes nothing reach the same board states again,
/// and the statistics, which check every item for clearance violations, are
/// a function of the board: a state seen before reuses them. Kept for the
/// runner's passes, at most [`StatisticsMemo::CAP`] states. Equal for
/// `PartialEq` whatever it holds: it is no part of the runner's state.
#[derive(Clone, Default)]
struct StatisticsMemo(BTreeMap<u128, Arc<BoardStatistics>>);

impl StatisticsMemo {
    const CAP: usize = 64;

    fn get(&mut self, board: &RoutingBoard) -> Arc<BoardStatistics> {
        let hash = board.state_hash();
        if let Some(stats) = self.0.get(&hash) {
            return Arc::clone(stats);
        }
        let stats = Arc::new(BoardStatistics::new(board));
        if self.0.len() >= Self::CAP {
            self.0.clear();
        }
        self.0.insert(hash, Arc::clone(&stats));
        stats
    }
}

impl PartialEq for StatisticsMemo {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl std::fmt::Debug for StatisticsMemo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "StatisticsMemo({} states)", self.0.len())
    }
}

impl PassRunner {
    /// Java's seven-argument `BatchAutorouter` constructor (`:131-165`):
    /// without preferred directions every layer's trace costs are its
    /// preferred-direction cost both ways.
    pub fn new(
        board_settings: &BoardRouterSettings,
        remove_unconnected_vias: bool,
        with_preferred_directions: bool,
        start_ripup_costs: i32,
        pull_tight_accuracy: i32,
        max_items: i32,
    ) -> Self {
        let trace_costs = if with_preferred_directions {
            board_settings.trace_costs().to_vec()
        } else {
            (0..board_settings.layer_count())
                .map(|i| {
                    let cost = board_settings.preferred_direction_trace_costs(i);
                    TraceCostFactor {
                        horizontal: cost,
                        vertical: cost,
                    }
                })
                .collect()
        };
        PassRunner {
            phase: Phase::Autoroute,
            remove_unconnected_vias,
            trace_costs,
            start_ripup_costs,
            pull_tight_accuracy,
            max_items,
            total_items_routed: 0,
            audit: None,
            statistics: StatisticsMemo::default(),
        }
    }

    /// The runner of a routing job (Java `BatchAutorouter(RoutingJob)`,
    /// `:116-128`): unconnected vias are removed when the fanout is off,
    /// preferred directions on, the settings' start ripup costs and
    /// pull-tight accuracy.
    pub fn for_job(settings: &RouterSettings, board_settings: &BoardRouterSettings) -> Self {
        let mut runner = PassRunner::new(
            board_settings,
            !settings.fanout.enabled,
            true,
            board_settings.start_ripup_costs(),
            board_settings.trace_pull_tight_accuracy(),
            settings.autorouter.max_items,
        );
        if settings.debug.audit_incremental == Some(true) {
            runner.audit = Some(IncrementalAudit::new());
        }
        runner
    }

    /// P02.1: compares the board's search trees with a rebuild, if the
    /// audit is on.
    pub fn audit(&mut self, board: &RoutingBoard, point: AuditPoint) {
        if let Some(audit) = &mut self.audit {
            audit.check(board, point);
        }
    }

    /// The audit's tally, if the audit is on.
    pub fn audit_report(&self) -> Option<&AuditReport> {
        self.audit.as_ref().map(IncrementalAudit::report)
    }

    /// The items counted against `max_items` so far.
    pub fn total_items_routed(&self) -> i64 {
        self.total_items_routed
    }

    /// Java `removeTails(option)`: starts marking the changed area, removes
    /// the trace tails of every net, then pulls the changed area tight as
    /// after a routed connection. Returns `remove_trace_tails`' result.
    pub fn remove_tails(&self, board: &mut RoutingBoard, option: StopConnectionOption) -> bool {
        board.start_marking_changed_area();
        let removed = board.remove_trace_tails(-1, option);
        pull_tight_after_connection(board, self.pull_tight_accuracy, &self.trace_costs);
        removed
    }

    /// [`run_pass_observed`](Self::run_pass_observed) without an observer.
    pub fn run_pass(
        &mut self,
        board: &mut RoutingBoard,
        pass: u32,
        stages: &mut PassStages<'_>,
        ctx: &mut StageCtx,
    ) -> PassOutcome {
        self.run_pass_observed(board, pass, stages, ctx, &mut NoObserver)
    }

    /// Java `runSingleThread(pass)`: routes every item of the pass's list
    /// once (see the module docs).
    pub fn run_pass_observed(
        &mut self,
        board: &mut RoutingBoard,
        pass: u32,
        stages: &mut PassStages<'_>,
        ctx: &mut StageCtx,
        observer: &mut dyn PassObserver,
    ) -> PassOutcome {
        let mut outcome = PassOutcome::default();
        let mut tasks = stages.ordering.select(board);
        if tasks.is_empty() {
            observer.on_items(board, &tasks);
            outcome.nothing_to_route = true;
            return outcome;
        }
        if pass > 1 {
            tasks = stages.ordering.order(tasks, board, pass);
        }
        observer.on_items(board, &tasks);
        outcome.items = tasks.len();
        let mut progress = ProgressState::new(
            self.phase,
            pass,
            tasks.len(),
            board,
            ctx,
            &mut self.statistics,
        );
        let ripup_cost = stages
            .ripup
            .ripup_cost(self.phase, pass, self.start_ripup_costs);
        let strict_check = stages
            .ripup
            .strict_check(pass, ctx.board_settings.strict_drc());
        'items: for task in &tasks {
            if ctx.cancel.is_router_stop_requested() {
                break;
            }
            let nets: Vec<i32> = board
                .item(task.item)
                .map(|item| item.net_numbers().to_vec())
                .unwrap_or_default();
            for net in nets {
                if ctx.cancel.is_router_stop_requested() {
                    break 'items;
                }
                // PARITY: AutoroutePassRunner.java:107-115, a full stop.
                if self.max_items > 0 && self.total_items_routed >= i64::from(self.max_items) {
                    outcome.log.push(format!(
                        "Max items limit reached ({}). Stopping auto-router.",
                        self.max_items
                    ));
                    ctx.cancel.stop_all();
                    outcome.max_items_reached = true;
                    break 'items;
                }
                self.total_items_routed += 1;
                board.start_marking_changed_area();
                let net_task = RouteTask { net, ..*task };
                let params = connection_params(
                    board,
                    &net_task,
                    pass,
                    ctx,
                    ripup_cost,
                    &self.trace_costs,
                    self.remove_unconnected_vias,
                    self.pull_tight_accuracy,
                    strict_check,
                );
                observer.before_connection(board, &net_task);
                let result = stages.router.route(board, &net_task, &params, ctx);
                self.count(board, pass, &net_task, &result, stages, &mut outcome);
                observer.after_connection(board, &net_task, &result);
                if self.audit.is_some() && (!result.ripped.is_empty() || is_failure(result.state)) {
                    self.audit(board, AuditPoint::Ripup { pass });
                }
                // G07.4: the listener sees the connection's items before its
                // progress event.
                flush_journal(board, ctx.events);
                progress.after_connection(
                    board,
                    ctx,
                    &outcome,
                    result.airline,
                    &mut self.statistics,
                );
            }
        }
        let option = if self.remove_unconnected_vias {
            StopConnectionOption::None
        } else {
            StopConnectionOption::FanoutVia
        };
        observer.before_tails(board);
        let removed = self.remove_tails(board, option);
        observer.after_tails(board, removed);
        self.audit(board, AuditPoint::PassEnd { pass });
        let ledger = board.routing_ledger();
        outcome.incomplete = ledger.incomplete_count(board);
        outcome.incomplete_nets = ledger.incomplete_net_numbers(board);
        outcome.continue_routing = outcome.routed > 0 || outcome.failed > 0;
        stages.ordering.on_pass_end(&PassReport {
            pass,
            continue_routing: outcome.continue_routing,
            incomplete: outcome.incomplete,
            incomplete_nets: outcome.incomplete_nets.clone(),
            ..PassReport::default()
        });
        progress.at_pass_end(board, ctx, &outcome, &mut self.statistics);
        outcome
    }

    /// The counters and the failure handling of one connection
    /// (`AutoroutePassRunner.java:150-198`).
    fn count(
        &self,
        board: &mut RoutingBoard,
        pass: u32,
        task: &RouteTask,
        result: &ConnectionOutcome,
        stages: &mut PassStages<'_>,
        outcome: &mut PassOutcome,
    ) {
        match result.state {
            AttemptState::Routed => outcome.routed += 1,
            AttemptState::AlreadyConnected
            | AttemptState::NoUnconnectedNets
            | AttemptState::ConnectedToPlane => outcome.skipped += 1,
            _ => {
                let first_net = board
                    .item(task.item)
                    .and_then(|item| item.net_numbers().first().copied())
                    .unwrap_or(-1);
                let pass_no = i32::try_from(pass).unwrap_or(i32::MAX);
                board.failure_log_mut().record_failure(
                    task.item,
                    first_net,
                    pass_no,
                    result.state,
                    Some(result.details.as_str()),
                );
                outcome.log.push(format!("Autorouter {}", result.details));
                let failures = board.failure_log().failure_count(task.item);
                stages.ripup.after_failure(board, task, failures);
                outcome.failed += 1;
            }
        }
        outcome.ripped += result.ripped.len();
    }
}

/// True if `state` is a failed attempt (`count`'s failure branch, where the
/// ripup policy may rip items).
fn is_failure(state: AttemptState) -> bool {
    !matches!(
        state,
        AttemptState::Routed
            | AttemptState::AlreadyConnected
            | AttemptState::NoUnconnectedNets
            | AttemptState::ConnectedToPlane
    )
}

/// The progress counters of a pass and the statistics they carry.
struct ProgressState {
    progress: Progress,
    items_since_statistics: usize,
}

impl ProgressState {
    fn new(
        phase: Phase,
        pass: u32,
        items: usize,
        board: &RoutingBoard,
        ctx: &StageCtx,
        memo: &mut StatisticsMemo,
    ) -> ProgressState {
        let mut state = ProgressState {
            progress: Progress {
                phase,
                pass: i32::try_from(pass).unwrap_or(i32::MAX),
                items_to_go: items,
                ..Progress::default()
            },
            items_since_statistics: 0,
        };
        if ctx.events.is_listening() {
            state.statistics(board, ctx, memo);
        }
        state
    }

    fn statistics(&mut self, board: &RoutingBoard, ctx: &StageCtx, memo: &mut StatisticsMemo) {
        let stats = memo.get(board);
        self.progress.violations = stats.clearance_violations.total_count;
        self.progress.score = stats.router_score(Some(ctx.settings));
        self.items_since_statistics = 0;
    }

    fn counters(&mut self, outcome: &PassOutcome) {
        let p = &mut self.progress;
        p.routed = outcome.routed;
        p.failed = outcome.failed;
        p.skipped = outcome.skipped;
        p.ripped = outcome.ripped;
    }

    fn after_connection(
        &mut self,
        board: &RoutingBoard,
        ctx: &StageCtx,
        outcome: &PassOutcome,
        airline: Option<(
            freerouting_geometry::FloatPoint,
            freerouting_geometry::FloatPoint,
        )>,
        memo: &mut StatisticsMemo,
    ) {
        self.progress.items_to_go = self.progress.items_to_go.saturating_sub(1);
        if !ctx.events.is_listening() {
            return;
        }
        self.items_since_statistics += 1;
        if self.items_since_statistics >= PROGRESS_STATISTICS_ITEM_INTERVAL {
            self.statistics(board, ctx, memo);
        }
        self.counters(outcome);
        self.progress.incomplete = board.routing_ledger().incomplete_count(board);
        self.progress.airline = airline;
        ctx.events.progress(self.progress.clone());
    }

    fn at_pass_end(
        &mut self,
        board: &RoutingBoard,
        ctx: &StageCtx,
        outcome: &PassOutcome,
        memo: &mut StatisticsMemo,
    ) {
        if !ctx.events.is_listening() {
            return;
        }
        self.statistics(board, ctx, memo);
        self.counters(outcome);
        self.progress.incomplete = outcome.incomplete;
        self.progress.airline = None;
        ctx.events.progress(self.progress.clone());
    }
}

/// Java `BatchAutorouter.autoroutePassesForOptimizingItem` (`:253-289`):
/// routes passes `1..=max_passes` on `board` while a pass routed or failed
/// something and no router stop is requested, then removes the tails
/// including unconnected vias. The stages are the ones `settings` select,
/// fresh (Java builds a new `BatchAutorouter`); unconnected vias are
/// removed in every pass.
///
/// Returns the number of nonempty routing passes performed. A pass that finds
/// nothing to route is not counted.
pub fn route_passes_for_optimizer(
    board: &mut RoutingBoard,
    max_passes: u32,
    ripup_costs: i32,
    accuracy: i32,
    with_preferred_directions: bool,
    settings: &RouterSettings,
    ctx: &mut StageCtx,
) -> u32 {
    let mut runner = PassRunner::new(
        ctx.board_settings,
        true,
        with_preferred_directions,
        ripup_costs,
        accuracy,
        settings.autorouter.max_items,
    );
    let registry = StageRegistry::builtin();
    let selection = StageSelection::resolve(settings, &registry);
    let mut ordering = registry
        .ordering
        .get(selection.ordering.id.as_str())
        .map_or_else(
            || {
                Box::new(crate::autoroute::batch::ordering::FreeroutingOrdering::new())
                    as Box<dyn NetOrdering>
            },
            |f| f(settings),
        );
    let mut router = registry
        .connection_router
        .get(selection.connection_router.id.as_str())
        .map_or_else(
            || {
                Box::new(
                    crate::autoroute::batch::connection::FreeroutingConnectionRouter::from_settings(
                        settings,
                    ),
                ) as Box<dyn ConnectionRouter>
            },
            |f| f(settings),
        );
    let mut ripup = registry
        .ripup_policy
        .get(selection.ripup_policy.id.as_str())
        .map_or_else(
            || {
                Box::new(
                    crate::autoroute::batch::ripup_policy::FreeroutingRipupPolicy::from_settings(
                        settings,
                    ),
                ) as Box<dyn RipupPolicy>
            },
            |f| f(settings),
        );
    let mut stages = PassStages {
        ordering: ordering.as_mut(),
        router: router.as_mut(),
        ripup: ripup.as_mut(),
    };
    let mut still_unrouted = true;
    let mut pass = 1;
    while still_unrouted && !ctx.cancel.is_router_stop_requested() && pass <= max_passes {
        still_unrouted = runner
            .run_pass(board, pass, &mut stages, ctx)
            .continue_routing;
        pass += 1;
    }
    runner.remove_tails(board, StopConnectionOption::None);
    // PARITY(quirk): BatchAutorouter.java:285-287 returns one too many.
    // Fixed outright in N06.2; the optimizer discards this count.
    let completed = pass - 1;
    if still_unrouted {
        completed
    } else {
        completed.saturating_sub(1)
    }
}

#[cfg(test)]
mod statistics_memo_tests {
    use super::*;

    fn board() -> RoutingBoard {
        let crate::io::specctra::BoardReadResult::Success { board, .. } =
            crate::io::specctra::read_board_from_str(include_str!(
                "../../../../../fixtures/Issue413-test.dsn"
            ))
        else {
            panic!("fixture")
        };
        *board
    }

    /// K13.9: a board state seen before reuses its statistics; a changed board
    /// gets its own, equal to the statistics computed from scratch.
    #[test]
    fn a_board_state_seen_before_reuses_its_statistics() {
        let mut board = board();
        let mut memo = StatisticsMemo::default();
        let first = memo.get(&board);
        assert_eq!(*first, BoardStatistics::new(&board));
        assert!(Arc::ptr_eq(&first, &memo.get(&board)));
        let removed = board
            .item_ids()
            .into_iter()
            .rev()
            .any(|id| board.remove_item(id));
        assert!(removed, "no item to remove");
        let changed = memo.get(&board);
        assert!(!Arc::ptr_eq(&first, &changed));
        assert_eq!(*changed, BoardStatistics::new(&board));
        assert_ne!(*changed, *first);
        assert!(Arc::ptr_eq(&changed, &memo.get(&board)));
    }
}
