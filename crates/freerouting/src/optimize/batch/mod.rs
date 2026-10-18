//! The batch optimizer (O06): after routing, rip up one via or trace at a
//! time, reroute it, and keep the best improvement of each pass.
//!
//! Port of `autoroute/pipeline/BatchOptimizer.java` at `a773a70ec`, as the
//! `freerouting-optimizer` stage of the D9 pipeline ([`FreeroutingOptimizer`]).
//! The parts:
//!
//! - [`guards`]: the preflight guards, the per-pass veto and the pass
//!   improvement (O06.1);
//! - [`candidates`]: the candidate order of a pass (O06.2);
//! - [`item`]: rerouting one candidate (O06.3);
//! - [`worker`]: a pass's candidates on copies of the baseline, in
//!   parallel, merged in candidate order (O06.4);
//! - this module: the pass loop (`runBatchLoop`, O06.5) and the stage
//!   (O06.6).
//!
//! **The pass loop** ([`run_batch_optimizer`]), in Java's order: the
//! statistics of the board and the guards; the incumbent is a deep copy;
//! the threshold is reset to 2.5 when it is NaN, infinite or negative, and
//! scaled by 100 in (0, 0.1); passes run while `pass < max_passes`, `items <
//! max_items` and no full stop is requested; odd passes use the preferred
//! directions. After a pass the winner is adopted, a pass without an
//! adopted improvement (or a winner whose `improvement_percentage` is 0,
//! which the integer division of `ItemRouteResult` makes common) clears the
//! increased ripup costs; the result is gated against the incumbent
//! (accepted: a new incumbent; rejected: a deep copy of the incumbent comes
//! back); the improvement is `(after − before) / |before| × 100` of the
//! ranking scores; the loop goes on once if the increased costs were still
//! on and the score did not rise (clearing them), and otherwise stops below
//! the threshold. At the end the incumbent comes back if the final score is
//! lower (D8: the best board is the one kept).
//!
//! **Kept Java quirks** (DIVERGENCES, "Java bugs kept on purpose"): a first
//! pass whose adopted winner has an `improvement_percentage` of 0 (or that
//! has no winner) clears the increased ripup costs inside the pass, so the
//! loop sees an improvement below the threshold and stops after one pass
//! (`first_pass_without_winner_stops`); the equal-key skip of the candidate
//! order; the threshold auto-scale for values in (0, 0.1).
//!
//! **Stops and time.** A full stop (cancel, or the optimizer or job
//! deadline, which the pipeline arms with `StopLevel::All`) is checked
//! before each pass and each chunk; the pass it lands in adopts no winner
//! that was not complete before the stop. Java also reads the clock inside
//! each candidate's "improved" decision; the port reads no clock
//! (DIVERGENCES, "Time"). A router stop (`stop_router`) lets the optimizer
//! run, but, as in Java, the inner autorouter of every candidate then
//! routes no pass (`autoroutePassesForOptimizingItem` checks
//! `isStopAutoRouterRequested`), so no candidate improves.
//!
//! **Events.** Through the stage's event sink, when it listens:
//! a [`Progress`] with [`Phase::Optimize`] at the start of each pass, after
//! an adoption and at the end of each pass (`pass_end`), with the board's
//! statistics, the router score and the last candidate's position; a
//! [`RouterEvents::replaced`](crate::autoroute::events::RouterEvents::replaced) whenever another board is swapped in (an
//! adopted winner, an incumbent restore), because the winner is a different
//! board object, as in Java. The summary goes back in [`OptimizerSummary`].

pub mod candidates;
pub mod guards;
pub mod item;
mod lookahead;
pub mod worker;

use std::collections::BTreeMap;

use freerouting_geometry::FloatPoint;

use crate::autoroute::events::{Phase, Progress};
use crate::autoroute::item_route_result::ItemRouteResult;
use crate::autoroute::stages::{Optimizer, OptimizerSummary, StageCtx};
use crate::board::BoardSnapshot;
use crate::board::items::ItemId;
use crate::board::routing_board::RoutingBoard;
use crate::drc::json::java_format_fixed;
use crate::job::{LogLevel, LogLine};
use crate::scoring::BoardStatistics;
use crate::settings::RouterSettings;

use self::candidates::prepare_candidates;
use self::guards::{
    BypassReason, GateFigures, GuardFigures, RejectionReason, all_vias_mandatory_layer_transitions,
    candidate_rejection_reason, evaluate_preflight_guards, pass_improvement_percent, via_contacts,
};
use self::item::ItemPassParams;
use self::worker::{PassSetup, default_chunk_size, evaluate_pass};

/// The registry id (Java `BatchOptimizer.getId`).
pub const OPTIMIZER_ID: &str = "freerouting-optimizer";

/// Java `DefaultSettings.DEFAULT_OPTIMIZER_IMPROVEMENT_THRESHOLD`.
pub const DEFAULT_IMPROVEMENT_THRESHOLD: f32 = 2.5;

/// How candidates are adopted. Java evaluates every candidate against the
/// pass baseline and adopts the single best one (O00 open question 1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AdoptionPolicy {
    /// One winner per pass, the best improved result (strictly, by
    /// [`ItemRouteResult::compare`]).
    #[default]
    SingleWinner,
}

/// Overrides of the optimizer's settings that do not change a result.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OptimizerOptions {
    /// Worker threads; `None` is `optimizer.max_threads`.
    pub threads: Option<usize>,
    /// Candidates per chunk; `None` is `max(4 × threads, 8)`.
    pub chunk_size: Option<usize>,
    /// The adoption policy.
    pub adoption: AdoptionPolicy,
}

/// What one optimizer pass did.
#[derive(Clone, Debug, PartialEq)]
pub struct OptimizerPass {
    /// The pass number, from 1.
    pub pass: u32,
    /// The candidates the pass was given (after the `max_items` cut).
    pub candidates: Vec<ItemId>,
    /// Every consumed result, in candidate order.
    pub results: Vec<ItemRouteResult>,
    /// The work of every consumed candidate (search-tree nodes visited on
    /// its own board), in the same order.
    pub work: Vec<u64>,
    /// The adopted winner.
    pub winner: Option<ItemRouteResult>,
    /// The ranking score before and after the pass.
    pub score_before: f32,
    /// See `score_before`.
    pub score_after: f32,
    /// Why the gate rejected the pass, `None` when it was accepted.
    pub rejection: Option<RejectionReason>,
    /// True if the failure limit ended the pass early.
    pub early_stop: bool,
    /// The board's [`state_hash`](crate::board::BasicBoard::state_hash)
    /// after the pass (after a restore, if one).
    pub state_hash: u128,
}

/// What [`run_batch_optimizer`] did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OptimizerRun {
    /// Why the stage was skipped, if it was.
    pub bypass: Option<BypassReason>,
    /// Passes run (Java `passesCompleted`).
    pub passes: u32,
    /// The passes, in order.
    pub pass_log: Vec<OptimizerPass>,
    /// Items attempted (Java `totalItemsOptimized`).
    pub items: i32,
    /// True if the final board is the incumbent restored at the end.
    pub restored_best: bool,
    /// True if a full stop ended the run.
    pub stopped: bool,
    /// The ranking score before and after.
    pub score_before: f32,
    /// See `score_before`.
    pub score_after: f32,
    /// Java's job log lines.
    pub log: Vec<LogLine>,
}

fn f2(value: f32) -> String {
    java_format_fixed(f64::from(value), 2)
}

fn f4(value: f64) -> String {
    java_format_fixed(value, 4)
}

/// The full statistics (with the clearance check) of `board`.
fn statistics(board: &RoutingBoard) -> BoardStatistics {
    BoardStatistics::new(board)
}

fn gate_figures(stats: &BoardStatistics, settings: &RouterSettings) -> GateFigures {
    GateFigures {
        incomplete: stats.connections.incomplete_count,
        violations: stats.clearance_violations.total_count,
        score: stats.optimizer_ranking_score(Some(settings)),
    }
}

/// The improvement threshold Java uses after its checks (with the log
/// line it writes). `java_fraction_heuristic` is
/// `router.java_quirks.opt_threshold_fraction` (N06): `false` reads a value in
/// (0, 0.1) as the percentage it says.
fn effective_threshold(threshold: f32, java_fraction_heuristic: bool) -> (f32, Option<LogLine>) {
    if threshold.is_nan() || threshold.is_infinite() || threshold < 0.0 {
        let line = LogLine {
            level: LogLevel::Warn,
            text: format!(
                "Invalid optimizer improvement threshold: {}. Resetting to default {}%.",
                java_format_fixed(f64::from(threshold), 4),
                f2(DEFAULT_IMPROVEMENT_THRESHOLD)
            ),
        };
        (DEFAULT_IMPROVEMENT_THRESHOLD, Some(line))
    } else if java_fraction_heuristic && threshold > 0.0 && threshold < 0.1 {
        // PARITY(quirk, java_quirks.opt_threshold_fraction): :367-377, a
        // fraction is taken for a percentage.
        let scaled = threshold * 100.0;
        let line = LogLine {
            level: LogLevel::Info,
            text: format!(
                "Optimizer improvement threshold appears to be specified as a fraction ({}). \
                 Auto-scaling to percentage ({}%).",
                java_format_fixed(f64::from(threshold), 4),
                f2(scaled)
            ),
        };
        (scaled, Some(line))
    } else {
        (threshold, None)
    }
}

/// The pass loop's state (Java's `BatchOptimizer` fields).
struct Loop<'s, 'c> {
    ctx: &'s StageCtx<'c>,
    options: OptimizerOptions,
    use_increased_ripup_costs: bool,
    /// The pass just run cleared the increased ripup costs (only under
    /// `router.java_quirks.opt_first_pass_stops = false`, N06), so the loop
    /// goes on once at normal costs.
    cleared_in_pass: bool,
    total_items: i32,
    results: BTreeMap<ItemId, ItemRouteResult>,
    position: Option<FloatPoint>,
    best: BoardSnapshot,
    best_figures: GateFigures,
    run: OptimizerRun,
}

impl Loop<'_, '_> {
    fn settings(&self) -> &RouterSettings {
        self.ctx.settings
    }

    fn info(&mut self, text: String) {
        self.run.log.push(LogLine {
            level: LogLevel::Info,
            text,
        });
    }

    fn progress(&self, board: &RoutingBoard, pass: u32, pass_end: bool) {
        let events = self.ctx.events;
        if !events.is_listening() {
            return;
        }
        let stats = statistics(board);
        events.progress(Progress {
            phase: Phase::Optimize,
            pass: i32::try_from(pass).unwrap_or(i32::MAX),
            incomplete: stats.connections.incomplete_count,
            violations: stats.clearance_violations.total_count,
            score: stats.router_score(Some(self.settings())),
            pass_end,
            optimizer_position: self.position,
            ..Progress::default()
        });
    }

    fn replaced(&self, board: &RoutingBoard) {
        if self.ctx.events.is_listening() {
            self.ctx.events.replaced(board.snapshot());
        }
    }

    /// Java `restoreIncumbentBoard`: a deep copy of the incumbent.
    fn restore_incumbent(&self, board: &mut RoutingBoard, pass: u32) {
        *board = worker::candidate_start_board(&self.best);
        self.replaced(board);
        self.progress(board, pass, false);
    }

    /// Java `optRoutePass` (`:640-832`). `before` is the board's full
    /// statistics at the start of the pass.
    fn opt_route_pass(
        &mut self,
        board: &mut RoutingBoard,
        pass: u32,
        with_preferred_directions: bool,
        before: &BoardStatistics,
    ) -> (Vec<ItemId>, worker::PassEvaluation) {
        self.progress(board, pass, false);
        let baseline_length = f64::from(before.traces.total_weighted_length);
        let strategy = self.settings().optimizer.item_selection_strategy;
        let mut ids = prepare_candidates(board, strategy, &mut self.results);
        let max_items = self.settings().optimizer.max_items;
        if max_items > 0 {
            let remaining = max_items.wrapping_sub(self.total_items);
            if remaining <= 0 {
                self.info(format!(
                    "Max items limit reached ({max_items}). Stopping optimizer."
                ));
                return (Vec::new(), worker::PassEvaluation::default());
            }
            ids.truncate(usize::try_from(remaining).unwrap_or(usize::MAX));
        }
        if ids.is_empty() {
            // PARITY: :666-668, returns before the ripup flag is cleared.
            return (ids, worker::PassEvaluation::default());
        }
        let threads = self
            .options
            .threads
            .unwrap_or_else(|| usize::try_from(self.settings().optimizer.max_threads).unwrap_or(1))
            .max(1);
        let optimizer = &self.settings().optimizer;
        let setup = PassSetup {
            settings: self.ctx.settings,
            board_settings: self.ctx.board_settings,
            cancel: self.ctx.cancel,
            params: ItemPassParams {
                baseline_length,
                with_preferred_directions,
                use_increased_ripup_costs: self.use_increased_ripup_costs,
            },
            threads,
            chunk_size: self
                .options
                .chunk_size
                .unwrap_or_else(|| default_chunk_size(threads)),
            max_consecutive_failures: if pass == 1 {
                optimizer.max_consecutive_failures_pass1
            } else {
                optimizer.max_consecutive_failures
            },
        };
        let mut evaluation = evaluate_pass(
            board,
            &ids,
            &setup,
            &mut self.results,
            &mut self.total_items,
        );
        if evaluation.position.is_some() {
            self.position = evaluation.position;
        }
        if evaluation.early_stop {
            self.info(format!(
                "Stopping optimization pass #{pass} early after {} consecutive items could not \
                 be improved.",
                evaluation.consecutive_failures
            ));
        }
        let mut route_improved = 0.0f32;
        if let Some((result, winner)) = evaluation.winner.take() {
            *board = winner;
            self.replaced(board);
            self.progress(board, pass, false);
            route_improved = result.improvement_percentage();
            evaluation.winner_result = Some(result);
        }
        // PARITY(quirk, java_quirks.opt_first_pass_stops): :812-815, no
        // adopted improvement (or a 0 % improvement) ends the increased ripup
        // costs, and the loop then stops below the threshold; `false` lets the
        // loop go on once at normal costs.
        if self.use_increased_ripup_costs && route_improved == 0.0 {
            self.use_increased_ripup_costs = false;
            self.cleared_in_pass = !self.ctx.board_settings.java_quirks().opt_first_pass_stops;
        }
        self.progress(board, pass, true);
        (ids, evaluation)
    }
}

/// Runs the optimizer passes on `board` (Java `runBatchLoop`; see the
/// module docs). `ctx` carries the settings, the stop request and the
/// event sink.
pub fn run_batch_optimizer(
    board: &mut RoutingBoard,
    ctx: &StageCtx,
    options: &OptimizerOptions,
) -> OptimizerRun {
    let settings = ctx.settings;
    let initial = statistics(board);
    let initial_router_score = initial.router_score(Some(settings));
    let initial_figures = gate_figures(&initial, settings);
    let mut run = OptimizerRun {
        score_before: initial_figures.score,
        score_after: initial_figures.score,
        ..OptimizerRun::default()
    };
    let bypass = evaluate_preflight_guards(
        &GuardFigures::of(&initial, settings),
        settings.optimizer.enable_preflight_guards,
        || all_vias_mandatory_layer_transitions(&via_contacts(board)),
    );
    if let Some(reason) = bypass {
        run.log.push(LogLine {
            level: LogLevel::Info,
            text: format!("Skipping optimization stage: {reason}."),
        });
        run.bypass = Some(reason);
        return run;
    }
    run.log.push(LogLine {
        level: LogLevel::Info,
        text: format!(
            "Optimization stage started. Baseline router score: {}, optimizer score: {}, \
             incomplete connections: {}, clearance violations: {}.",
            f2(initial_router_score),
            f2(initial_figures.score),
            initial_figures.incomplete,
            initial_figures.violations
        ),
    });
    let (threshold, threshold_line) = effective_threshold(
        settings.optimizer.improvement_threshold,
        ctx.board_settings.java_quirks().opt_threshold_fraction,
    );
    run.log.extend(threshold_line);

    let mut state = Loop {
        ctx,
        options: *options,
        use_increased_ripup_costs: true,
        cleared_in_pass: false,
        total_items: 0,
        results: BTreeMap::new(),
        position: None,
        best: board.snapshot(),
        best_figures: initial_figures,
        run,
    };
    let max_passes = settings.optimizer.max_passes;
    let max_items = settings.optimizer.max_items;
    let mut pass: u32 = 0;
    while i64::from(pass) < i64::from(max_passes)
        && state.total_items < max_items
        && !ctx.cancel.is_stop_requested()
    {
        pass += 1;
        let before = statistics(board);
        let score_before = before.optimizer_ranking_score(Some(settings));
        let with_preferred_directions = pass % 2 != 0;
        let scope = crate::run_stats::stage("optimizer_pass", pass);
        let (candidates, evaluation) =
            state.opt_route_pass(board, pass, with_preferred_directions, &before);
        scope.finish();

        let after = statistics(board);
        let after_figures = gate_figures(&after, settings);
        let score_after = after_figures.score;
        let rejection = candidate_rejection_reason(&state.best_figures, &after_figures);
        match rejection {
            None => {
                state.best_figures = after_figures;
                state.best = board.snapshot();
            }
            Some(reason) => {
                state.info(format!(
                    "Optimizer pass #{pass} candidate rejected: {reason}. Restoring incumbent \
                     (optimizer score {}, incomplete connections: {}, clearance violations: {}).",
                    f2(state.best_figures.score),
                    state.best_figures.incomplete,
                    state.best_figures.violations
                ));
                state.restore_incumbent(board, pass);
            }
        }
        let improvement = pass_improvement_percent(score_before, score_after);
        let outcome = if score_after > score_before {
            "IMPROVED"
        } else if score_after < score_before {
            "REGRESSED"
        } else {
            "UNCHANGED"
        };
        let improvement_text = if score_before != 0.0 {
            format!("{}%", f4(improvement))
        } else {
            "n/a (baseline was 0.00)".to_string()
        };
        state.info(format!(
            "Optimizer pass #{pass}: optimizer score {} -> {} ({outcome}, {improvement_text}), \
             router score: {}, incomplete connections: {}, clearance violations: {}.",
            f2(score_before),
            f2(score_after),
            f2(after.router_score(Some(settings))),
            after_figures.incomplete,
            after_figures.violations
        ));
        state.run.pass_log.push(OptimizerPass {
            pass,
            candidates,
            results: evaluation.consumed,
            work: evaluation.consumed_work,
            winner: evaluation.winner_result,
            score_before,
            score_after,
            rejection,
            early_stop: evaluation.early_stop,
            state_hash: board.state_hash(),
        });
        if evaluation.stopped || ctx.cancel.is_stop_requested() {
            state.run.stopped = true;
            break;
        }
        // PARITY: :462-480.
        let increased =
            state.use_increased_ripup_costs || std::mem::take(&mut state.cleared_in_pass);
        let continue_once = increased && score_after <= score_before;
        if continue_once {
            state.use_increased_ripup_costs = false;
        } else if improvement < f64::from(threshold) {
            state.info(format!(
                "Stopping optimizer because the improvement in this pass ({}%) is below the \
                 threshold ({}%).",
                f4(improvement),
                f2(threshold)
            ));
            break;
        }
    }
    state.run.stopped |= ctx.cancel.is_stop_requested();
    let final_score = statistics(board).optimizer_ranking_score(Some(settings));
    if final_score < state.best_figures.score {
        state.info(format!(
            "Restoring best board achieved (score {} vs final {}).",
            f2(state.best_figures.score),
            f2(final_score)
        ));
        state.restore_incumbent(board, pass);
        state.run.restored_best = true;
    }
    let final_stats = statistics(board);
    let status = if state.run.stopped {
        "interrupted:"
    } else {
        "completed:"
    };
    state.run.passes = pass;
    state.run.items = state.total_items;
    state.run.score_after = final_stats.optimizer_ranking_score(Some(settings));
    let text = format!(
        "Optimization stage {status} Baseline router score: {}, baseline optimizer score: {}, \
         final router score: {}, final optimizer score: {}.",
        f2(initial_router_score),
        f2(state.run.score_before),
        f2(final_stats.router_score(Some(settings))),
        f2(state.run.score_after)
    );
    state.info(text);
    state.run
}

/// The `freerouting-optimizer` stage (O06.6).
#[derive(Clone, Copy, Debug, Default)]
pub struct FreeroutingOptimizer {
    /// Overrides that do not change a result.
    pub options: OptimizerOptions,
}

impl FreeroutingOptimizer {
    /// The stage for `settings` (nothing is read up front).
    pub fn from_settings(_settings: &RouterSettings) -> FreeroutingOptimizer {
        FreeroutingOptimizer::default()
    }
}

impl Optimizer for FreeroutingOptimizer {
    fn id(&self) -> &'static str {
        OPTIMIZER_ID
    }

    fn run(&mut self, board: &mut RoutingBoard, ctx: &mut StageCtx) -> OptimizerSummary {
        let run = run_batch_optimizer(board, ctx, &self.options);
        OptimizerSummary {
            passes: run.passes,
            accepted_passes: run
                .pass_log
                .iter()
                .filter(|p| p.rejection.is_none())
                .count()
                .try_into()
                .unwrap_or(u32::MAX),
            items: u32::try_from(run.items.max(0)).unwrap_or(0),
            bypassed: run.bypass.is_some(),
            restored_best: run.restored_best,
            log: run.log,
            ..OptimizerSummary::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::effective_threshold;

    /// N06 `opt_threshold_fraction`: `true` reads 0.05 as 5 %, `false` as 0.05 %.
    #[test]
    fn threshold_fraction_switch_both_behaviours() {
        let (java, line) = effective_threshold(0.05, true);
        assert_eq!(java, 0.05 * 100.0);
        assert!(line.is_some());
        assert_eq!(effective_threshold(0.05, false), (0.05, None));
        // Outside (0, 0.1) the switch changes nothing.
        assert_eq!(effective_threshold(2.5, true), (2.5, None));
        assert_eq!(effective_threshold(2.5, false), (2.5, None));
    }
}
