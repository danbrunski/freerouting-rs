//! The batch loop (A13.2): `AutorouteBatchLoop.run`
//! (`AutorouteBatchLoop.java:43-669` at `a773a70ec`) around A10's
//! [`PassController`].
//!
//! [`run_batch_loop`] runs, in Java's order:
//!
//! 1. the routable-layer check ([`check_routable_layers`]); an error ends
//!    the job before anything changes;
//! 2. the initial unrouted count (the routing ledger, Java
//!    `calculateIncompleteCount`) and the ordering's `reset`
//!    (`resetAntiOscillationState`);
//! 3. the fanout stage when `fanout.enabled` and the board has SMD pins;
//! 4. while the router is enabled (`autorouter.enabled && max_passes >= 0`)
//!    and no router stop was requested, the pass loop (`:313-603`):
//!    [`PassController::before_pass`] (a timed-out job asks for a router
//!    stop there, and the pass limit stops the loop); the board added to
//!    the history; the pass ([`PassExecutor::run_pass`]); the statistics
//!    once; [`PassController::after_pass`] and the decisions it hands out
//!    one at a time: a restore (the history's board swapped in, `Replaced`
//!    sent, the ordering reset, the statistics recomputed; no board means
//!    stop), the fanout recovery (`remove_tails(NONE)`), the last-mile
//!    ripup ([`rip_blockers`]), a stop. The pass number advances only on
//!    [`PassDecision::Continue`]. With `quantum_stop.repeat` (Q04.5, not
//!    Java) a pass that leaves the board with the state hash it started
//!    with ends the loop ([`StopReason::Repeat`]) unless the controller
//!    already stops;
//! 5. the finish (`:607-642`): the history's best board when it beats the
//!    final score (`Replaced` sent), Q04.3's output guard when
//!    `router.autorouter.output_guard` is on (not Java; it may swap in a
//!    history board or the input board, `Replaced` sent), the final
//!    `remove_tails(NONE)` when the router ran and was not stopped, and the
//!    history cleared.
//!
//! **Statistics once per board state.** Java builds `BoardStatistics`
//! three or four times per pass with the same result; the loop builds it
//! once after the pass and again only after a restore, a fanout recovery, a
//! last-mile ripup that removed something, the final restore and the final
//! tail removal. The history entry of a pass reuses the score of the
//! statistics that described the board when it was added
//! ([`ScoredBoard`]).
//!
//! **A restore is a B01 snapshot restore** ([`SnapshotHistory`]), never a
//! `Clone`. The restored board is swapped in only once the controller has
//! seen its rank (Java checks `BOARD_RANK_LIMIT` before it swaps).
//!
//! **Events.** The pass log line (`Auto-routing pass #N ... with score S`)
//! is recorded in [`LoopOutcome::passes`] and [`LoopOutcome::log`] after
//! the restore check, as Java logs it, and a [`Progress`] with the same
//! figures goes out when someone listens. While someone listens the board's
//! change journal (B01.7) is on and drained into a [`RouterEvents::delta`]
//! after the fanout, after each pass and after each loop action; a journal
//! overflow becomes a `Replaced`. The pass runner and the fanout take the
//! board by `&RoutingBoard` in their observers, so the drain happens at
//! these boundaries, not after every connection. None of this changes a
//! result.
//!
//! The [`PassExecutor`] seam lets a test drive the loop with a fake pass
//! runner and scripted scores; [`PassRunner`] is the real one.
//!
//! Not ported: Java's profiler and resource-usage log values, the per-net
//! trace breakdown, `saveIntermediateStages` snapshots (no listener in the
//! port consumes them), the commented-out same-hash stop and its hash set.

use crate::autoroute::batch::last_mile::rip_blockers;
use crate::autoroute::batch::pass::{PassOutcome, PassRunner, PassStages};
use crate::autoroute::batch::report::{NoRoutableLayer, check_routable_layers};
use crate::autoroute::batch::snapshot::{BoardFigures, ScoredBoard, SnapshotHistory};
use crate::autoroute::events::{Phase, Progress, flush_journal};
use crate::autoroute::stages::{
    ActionOutcome, BoardScore, BoardStage, ConnectionRouter, FanoutStage, FanoutSummary,
    NetOrdering, PassController, PassDecision, PassReport, PassStart, RestoredBoard, RipupPolicy,
    StageCtx, StagePhase, StopReason,
};
use crate::board::RoutingBoard;
use crate::board::items::StopConnectionOption;
use crate::board::search::audit::{AuditPoint, AuditReport};
use crate::job::{LogLevel, LogLine, log};
use crate::settings::RouterSettings;

/// The pass runner as the loop sees it.
pub trait PassExecutor {
    /// Routes pass `pass` (Java `autoroutePass`).
    fn run_pass(
        &mut self,
        board: &mut RoutingBoard,
        pass: u32,
        stages: &mut PassStages<'_>,
        ctx: &mut StageCtx,
    ) -> PassOutcome;

    /// Java `removeTails(option)`.
    fn remove_tails(&mut self, board: &mut RoutingBoard, option: StopConnectionOption) -> bool;

    /// The statistics figures of `board` (one `BoardStatistics`).
    fn figures(&self, board: &RoutingBoard, settings: &RouterSettings) -> BoardFigures {
        BoardFigures::of(board, settings)
    }

    /// P02.1: audits the board's search trees, if the executor's audit is
    /// on. It only reads the board.
    fn audit(&mut self, _board: &RoutingBoard, _point: AuditPoint) {}

    /// The audit's tally, if the audit is on.
    fn audit_report(&self) -> Option<AuditReport> {
        None
    }
}

impl PassExecutor for PassRunner {
    fn run_pass(
        &mut self,
        board: &mut RoutingBoard,
        pass: u32,
        stages: &mut PassStages<'_>,
        ctx: &mut StageCtx,
    ) -> PassOutcome {
        PassRunner::run_pass(self, board, pass, stages, ctx)
    }

    fn remove_tails(&mut self, board: &mut RoutingBoard, option: StopConnectionOption) -> bool {
        PassRunner::remove_tails(self, board, option)
    }

    fn audit(&mut self, board: &RoutingBoard, point: AuditPoint) {
        PassRunner::audit(self, board, point);
    }

    fn audit_report(&self) -> Option<AuditReport> {
        PassRunner::audit_report(self).cloned()
    }
}

/// The stages the loop calls.
pub struct LoopStages<'a> {
    /// The fanout stage.
    pub fanout: &'a mut dyn FanoutStage,
    /// Picks and orders the items of a pass.
    pub ordering: &'a mut dyn NetOrdering,
    /// Routes one connection.
    pub router: &'a mut dyn ConnectionRouter,
    /// The ripup costs, the strict check and the repeated-failure ripup.
    pub ripup: &'a mut dyn RipupPolicy,
    /// The stop and restore rules.
    pub controller: &'a mut dyn PassController,
    /// The special stages; the loop runs those of the fanout, pass and
    /// routing phases.
    pub board_stages: &'a mut [Box<dyn BoardStage>],
}

/// One pass as Java's pass log line reports it: the figures after the
/// restore check.
#[derive(Clone, Debug, PartialEq)]
pub struct PassRecord {
    /// The pass number.
    pub pass: u32,
    /// The state hash of the board the line names.
    pub hash: u128,
    /// The router score.
    pub score: f32,
    /// Incomplete connections.
    pub incomplete: usize,
    /// Clearance violations.
    pub violations: usize,
}

impl PassRecord {
    /// Java's score text, `FRLogger.formatScore`.
    pub fn score_text(&self) -> String {
        log::format_score(
            self.score,
            u32::try_from(self.incomplete).unwrap_or(u32::MAX),
            u32::try_from(self.violations).unwrap_or(u32::MAX),
        )
    }
}

/// What the loop did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LoopOutcome {
    /// Incomplete connections before the fanout (Java
    /// `initialUnroutedCount`).
    pub initial_unrouted: usize,
    /// The fanout's summary, if it ran.
    pub fanout: Option<FanoutSummary>,
    /// True if the routing passes were enabled (`isRouterEnabled`).
    pub router_enabled: bool,
    /// The last pass started (Java `job.getCurrentPass()`); 0 if none.
    pub current_pass: u32,
    /// The pass log lines' figures, in order.
    pub passes: Vec<PassRecord>,
    /// Why the pass loop stopped, if it ran.
    pub stop: Option<StopReason>,
    /// History restores during the passes.
    pub restores: usize,
    /// True if the final best-board restore swapped the board.
    pub restored_best: bool,
    /// Last-mile ripups and the items they removed.
    pub last_mile: Vec<usize>,
    /// The figures of the board the loop leaves.
    pub final_figures: Option<BoardFigures>,
    /// Java's job log lines, in order.
    pub log: Vec<LogLine>,
    /// True if the finish ran the final `remove_tails(NONE)` (Q10's sweep
    /// repeats it).
    pub final_tails: bool,
    /// Q10: the add-only sweep, if one ran.
    pub sweep: Option<crate::quality::sweep::SweepOutcome>,
    /// Q04.3: the output guard, if `router.autorouter.output_guard` was on.
    pub output_guard: Option<crate::quality::output_guard::GuardOutcome>,
}

impl LoopOutcome {
    fn info(&mut self, text: String) {
        self.log.push(LogLine {
            level: LogLevel::Info,
            text,
        });
    }

    fn debug(&mut self, text: String) {
        self.log.push(LogLine {
            level: LogLevel::Debug,
            text,
        });
    }
}

/// Java `AutorouteBatchLoop.run` (see the module docs). `timed_out` is the
/// job's `TIMED_OUT` state; it may only stop work.
pub fn run_batch_loop(
    board: &mut RoutingBoard,
    stages: &mut LoopStages<'_>,
    executor: &mut dyn PassExecutor,
    ctx: &mut StageCtx,
    timed_out: &dyn Fn() -> bool,
) -> Result<LoopOutcome, NoRoutableLayer> {
    check_routable_layers(board, ctx.board_settings)?;
    let mut run = Run {
        outcome: LoopOutcome::default(),
        history: SnapshotHistory::new(),
        listening: ctx.events.is_listening(),
        // Q04.3: the input board, before the fanout; off by default.
        guard: crate::quality::output_guard::OutputGuard::start(board, ctx.settings),
    };
    run.journal_on(board);
    // PARITY: AutorouteBatchLoop.java:80-82.
    run.outcome.initial_unrouted = board.routing_ledger().incomplete_count(board);
    stages.ordering.reset();
    run.board_stages(board, stages, ctx, StagePhase::BeforeFanout);
    run.fanout(board, stages, ctx);
    run.board_stages(board, stages, ctx, StagePhase::AfterFanout);
    run.flush(board, ctx);

    let settings = ctx.settings;
    // PARITY: :285-288, `getRunRouter() && (maxPasses == null || maxPasses >= 0)`.
    let router_enabled = settings.run_router() && settings.autorouter.max_passes >= 0;
    run.outcome.router_enabled = router_enabled;
    let mut figures = executor.figures(board, settings);
    if router_enabled {
        run.outcome.info(format!(
            "Auto-routing stage started with baseline score {} for {} unrouted item{}.",
            log::decimal_format_0_00(f64::from(figures.score)),
            figures.incomplete,
            if figures.incomplete == 1 { "" } else { "s" }
        ));
        figures = run.passes(board, stages, executor, ctx, timed_out, figures);
    }
    figures = run.finish(board, stages, executor, ctx, figures);
    // Q10: the add-only sweep (`router.autorouter.sweep`, off by default).
    if let Some(sweep) =
        crate::quality::sweep::after_loop(board, stages, executor, ctx, &run.outcome, timed_out())
    {
        for text in &sweep.log {
            run.outcome.info(text.clone());
        }
        figures = sweep.after.clone();
        run.outcome.sweep = Some(sweep);
    }
    run.board_stages(board, stages, ctx, StagePhase::AfterRouting);
    run.flush(board, ctx);
    if run.listening {
        board.set_journaling(false);
    }
    run.outcome.final_figures = Some(figures);
    Ok(run.outcome)
}

/// The state of one loop run.
struct Run {
    outcome: LoopOutcome,
    history: SnapshotHistory,
    listening: bool,
    guard: Option<crate::quality::output_guard::OutputGuard>,
}

/// What a restore produced before the controller saw its rank.
struct PendingRestore {
    board: RoutingBoard,
    figures: BoardFigures,
}

impl Run {
    fn journal_on(&self, board: &mut RoutingBoard) {
        if self.listening {
            board.set_journaling(true);
        }
    }

    /// Sends what the journal recorded since the last drain (the passes and
    /// the fanout drain it after each connection and pin already, so this
    /// is the rest).
    fn flush(&self, board: &mut RoutingBoard, ctx: &StageCtx) {
        if self.listening {
            flush_journal(board, ctx.events);
        }
    }

    /// Swaps in `restored` and announces it.
    fn swap(&self, board: &mut RoutingBoard, restored: RoutingBoard, ctx: &StageCtx) {
        *board = restored;
        if self.listening {
            ctx.events.replaced(board.snapshot());
        }
        self.journal_on(board);
    }

    fn board_stages(
        &mut self,
        board: &mut RoutingBoard,
        stages: &mut LoopStages<'_>,
        ctx: &mut StageCtx,
        phase: StagePhase,
    ) {
        for stage in stages.board_stages.iter_mut() {
            if stage.phase() == phase {
                let report = stage.run(board, ctx);
                for text in report.log {
                    self.outcome.info(text);
                }
            }
        }
    }

    /// The fanout stage (`:110-260`).
    fn fanout(
        &mut self,
        board: &mut RoutingBoard,
        stages: &mut LoopStages<'_>,
        ctx: &mut StageCtx,
    ) {
        if !ctx.settings.fanout.enabled {
            return;
        }
        if board.smd_pins().is_empty() {
            self.outcome.info(
                "Fanout stage is enabled but skipped because the board has no SMD pins."
                    .to_string(),
            );
            return;
        }
        let scope = crate::run_stats::stage("fanout", 0);
        let summary = stages.fanout.run(board, ctx);
        scope.finish();
        let status = if summary.timed_out {
            "completed with timeout:"
        } else if ctx.cancel.is_router_stop_requested() {
            "interrupted:"
        } else {
            "completed:"
        };
        let percent = if summary.total == 0 {
            0.0
        } else {
            100.0 * summary.escaped as f64 / summary.total as f64
        };
        self.outcome.info(format!(
            "Fanout stage {status} started with {} total SMD pins, escaped pins: {}/{} ({percent:.1}%).",
            summary.total, summary.escaped, summary.total
        ));
        self.outcome.fanout = Some(summary);
    }

    /// The pass loop (`:313-603`); returns the figures of the board it
    /// leaves.
    fn passes(
        &mut self,
        board: &mut RoutingBoard,
        stages: &mut LoopStages<'_>,
        executor: &mut dyn PassExecutor,
        ctx: &mut StageCtx,
        timed_out: &dyn Fn() -> bool,
        mut figures: BoardFigures,
    ) -> BoardFigures {
        let settings = ctx.settings;
        let mut pass: u32 = 1;
        while !ctx.cancel.is_router_stop_requested() {
            match stages.controller.before_pass(pass, timed_out()) {
                PassStart::Stop(reason) => {
                    // PARITY: :331-336, `requestStopAutoRouter(); break`.
                    ctx.cancel.stop_router();
                    self.outcome.stop = Some(reason);
                    break;
                }
                PassStart::Run { stop_router } => {
                    // PARITY: :314-316, a timed-out job.
                    if stop_router {
                        ctx.cancel.stop_router();
                    }
                }
            }
            self.outcome.current_pass = pass;
            let hash = board.state_hash();
            // PARITY: :344, `bh.add(router.board)`.
            self.history.add(&ScoredBoard {
                board: &*board,
                hash,
                score: figures.score,
            });
            let scope = crate::run_stats::stage("pass", pass);
            let result = {
                let mut pass_stages = PassStages {
                    ordering: &mut *stages.ordering,
                    router: &mut *stages.router,
                    ripup: &mut *stages.ripup,
                };
                executor.run_pass(board, pass, &mut pass_stages, ctx)
            };
            scope.finish();
            for text in &result.log {
                self.outcome.info(text.clone());
            }
            let statistics_scope = crate::run_stats::stage("statistics", pass);
            figures = executor.figures(board, settings);
            statistics_scope.finish();
            let report = PassReport {
                pass,
                continue_routing: result.continue_routing,
                score: figures.score,
                incomplete: figures.incomplete,
                incomplete_nets: result.incomplete_nets.clone(),
                router_stop_requested: ctx.cancel.is_router_stop_requested(),
                timed_out: timed_out(),
                history_size: self.history.len(),
                history_max_score: self.history.max_score(),
            };
            let mut decision = stages.controller.after_pass(&report);
            // Q04.5: `quantum_stop.repeat`, off by default. A pass that
            // leaves the board with the state hash it started with ends the
            // loop, in place of any decision but the controller's own stop.
            // The finish's best-board restore is unchanged.
            if settings.quantum_stop.repeat == Some(true)
                && !matches!(decision, PassDecision::Stop(_))
                && board.state_hash() == hash
            {
                decision = PassDecision::Stop(StopReason::Repeat);
            }
            let (stop, after) = self.decide(board, stages, executor, ctx, pass, decision, figures);
            figures = after;
            self.flush(board, ctx);
            if let Some(reason) = stop {
                // PARITY: every stop but a finished pass requests a router
                // stop (`thread.requestStopAutoRouter()`).
                if reason.requests_router_stop() {
                    ctx.cancel.stop_router();
                }
                self.outcome.stop = Some(reason);
                break;
            }
            // PARITY: :602-605, the pass number advances only on Continue.
            pass += 1;
        }
        figures
    }

    /// Applies the controller's decisions after pass `pass` until it says
    /// continue or stop; returns the stop, if any, and the figures of the
    /// board then.
    #[allow(clippy::too_many_arguments)]
    fn decide(
        &mut self,
        board: &mut RoutingBoard,
        stages: &mut LoopStages<'_>,
        executor: &mut dyn PassExecutor,
        ctx: &mut StageCtx,
        pass: u32,
        mut decision: PassDecision,
        mut figures: BoardFigures,
    ) -> (Option<StopReason>, BoardFigures) {
        let settings = ctx.settings;
        let mut pending: Option<PendingRestore> = None;
        let mut logged = false;
        loop {
            // A restored board is swapped in once the controller accepted
            // its rank (`:376-381` before `:383`).
            if let Some(restore) = pending.take()
                && !matches!(decision, PassDecision::Stop(StopReason::RankLimit))
            {
                self.swap(board, restore.board, ctx);
                executor.audit(board, AuditPoint::Restore);
                // PARITY: :384, `resetAntiOscillationState`.
                stages.ordering.reset();
                figures = restore.figures;
                self.outcome.restores += 1;
                self.outcome.debug(format!(
                    "Restoring an earlier board that has the score of {}.",
                    score_text(&figures)
                ));
            }
            // PARITY: :408-428, the pass line comes after the restore check;
            // a stop of the restore itself breaks before it.
            if !logged
                && !matches!(
                    decision,
                    PassDecision::Restore { .. }
                        | PassDecision::Stop(StopReason::HistoryExhausted | StopReason::RankLimit)
                )
            {
                logged = true;
                self.pass_line(board, stages, ctx, pass, &figures);
            }
            let outcome = match decision {
                PassDecision::Continue => return (None, figures),
                PassDecision::Stop(reason) => {
                    self.stop_line(board, reason, &figures);
                    return (Some(reason), figures);
                }
                PassDecision::Restore { max_restore_count } => {
                    match self.history.restore(max_restore_count) {
                        None => {
                            self.outcome.info(
                                "The router was not able to improve the board, stopping the \
                                 auto-router."
                                    .to_string(),
                            );
                            ActionOutcome::Restored(None)
                        }
                        Some(restored) => {
                            let rank = self.history.rank(restored.state_hash());
                            let restored_figures = executor.figures(&restored, settings);
                            let score = board_score(&restored_figures);
                            pending = Some(PendingRestore {
                                board: restored,
                                figures: restored_figures,
                            });
                            ActionOutcome::Restored(Some(RestoredBoard { rank, board: score }))
                        }
                    }
                }
                PassDecision::FanoutRecovery => {
                    let before = figures.incomplete;
                    executor.remove_tails(board, StopConnectionOption::None);
                    executor.audit(board, AuditPoint::LoopRipup);
                    figures = executor.figures(board, settings);
                    self.outcome.debug(format!(
                        "Applied one-time fanout recovery cleanup (removed fanout tails/vias). \
                         Incompletes: {before} -> {}.",
                        figures.incomplete
                    ));
                    ActionOutcome::FanoutRecovered(board_score(&figures))
                }
                PassDecision::LastMileRipup => {
                    let incomplete_now = figures.incomplete;
                    let removed = rip_blockers(board);
                    executor.audit(board, AuditPoint::LoopRipup);
                    self.outcome.last_mile.push(removed);
                    if removed > 0 {
                        figures = executor.figures(board, settings);
                        self.outcome.info(format!(
                            "Last-mile rip-up removed {removed} blocking trace(s) or via(s) around \
                             {incomplete_now} remaining connection(s)."
                        ));
                    }
                    ActionOutcome::LastMile {
                        removed,
                        board: board_score(&figures),
                    }
                }
            };
            decision = stages.controller.after_action(&outcome);
        }
    }

    /// The pass log line, its record and its progress event.
    fn pass_line(
        &mut self,
        board: &mut RoutingBoard,
        stages: &mut LoopStages<'_>,
        ctx: &mut StageCtx,
        pass: u32,
        figures: &BoardFigures,
    ) {
        let record = PassRecord {
            pass,
            hash: board.state_hash(),
            score: figures.score,
            incomplete: figures.incomplete,
            violations: figures.violations,
        };
        self.outcome.info(format!(
            "Auto-routing pass #{pass} on board '{:032x}' was completed with score {}.",
            record.hash,
            record.score_text()
        ));
        if self.listening {
            ctx.events.progress(Progress {
                phase: Phase::Autoroute,
                pass: i32::try_from(pass).unwrap_or(i32::MAX),
                incomplete: figures.incomplete,
                violations: figures.violations,
                score: figures.score,
                pass_end: true,
                ..Progress::default()
            });
        }
        self.outcome.passes.push(record);
        self.board_stages(board, stages, ctx, StagePhase::AfterPass);
    }

    fn stop_line(&mut self, board: &RoutingBoard, reason: StopReason, figures: &BoardFigures) {
        match reason {
            StopReason::Quantum => {
                self.outcome.info(format!(
                    "The router's score ({}) gained less than the quantum at the last restore \
                     checks ({} item{} still unconnected). Stopping the auto-router \
                     (quantum-stop).",
                    log::decimal_format_0_00(f64::from(figures.score)),
                    figures.incomplete,
                    if figures.incomplete == 1 { "" } else { "s" }
                ));
            }
            StopReason::Repeat => {
                self.outcome.info(format!(
                    "The auto-router pass left the board in the state it started with (score {}, \
                     {} item{} still unconnected). Stopping the auto-router (quantum-stop repeat).",
                    log::decimal_format_0_00(f64::from(figures.score)),
                    figures.incomplete,
                    if figures.incomplete == 1 { "" } else { "s" }
                ));
            }
            StopReason::Stagnation | StopReason::GlobalStagnation => {
                let report = crate::autoroute::batch::report::unrouted_report(board);
                self.outcome.info(format!(
                    "The router's score ({}) has not improved enough ({} item{} still \
                     unconnected). Stopping the auto-router.\nThe following connections could \
                     not be routed -- please review your design (e.g. check pad clearances, \
                     trace width rules, and available routing space):\n{report}",
                    log::decimal_format_0_00(f64::from(figures.score)),
                    figures.incomplete,
                    if figures.incomplete == 1 { "" } else { "s" }
                ));
            }
            _ => {}
        }
    }

    /// The finish (`:607-642`); returns the final figures.
    fn finish(
        &mut self,
        board: &mut RoutingBoard,
        stages: &mut LoopStages<'_>,
        executor: &mut dyn PassExecutor,
        ctx: &mut StageCtx,
        mut figures: BoardFigures,
    ) -> BoardFigures {
        let settings = ctx.settings;
        let decision = stages.controller.finish(
            figures.score,
            self.history.max_score(),
            ctx.cancel.is_router_stop_requested(),
        );
        // PARITY: :610-631, the best board ever seen.
        if decision.restore_best
            && let Some(best) = self.history.restore_best()
        {
            let before = score_text(&figures);
            self.swap(board, best, ctx);
            executor.audit(board, AuditPoint::Restore);
            figures = executor.figures(board, settings);
            self.outcome.restored_best = true;
            self.outcome.debug(format!(
                "The final board state (score {before}) is worse than the best board seen during \
                 routing (score {}). Restoring the best board as the final result.",
                score_text(&figures)
            ));
        }
        // Q04.3: the output guard, off by default; before the tail removal,
        // which changes neither of its axes.
        if let Some(guard) = &self.guard {
            let (result, replacement) = guard.check(board, &self.history);
            for text in &result.log {
                self.outcome.info(text.clone());
            }
            if let Some(saved) = replacement {
                self.swap(board, saved, ctx);
                executor.audit(board, AuditPoint::Restore);
                figures = executor.figures(board, settings);
            }
            self.outcome.output_guard = Some(result);
        }
        // PARITY: :635-639.
        self.outcome.final_tails = decision.remove_tails;
        if decision.remove_tails {
            executor.remove_tails(board, StopConnectionOption::None);
            executor.audit(board, AuditPoint::Finish);
            figures = executor.figures(board, settings);
        }
        self.history.clear();
        figures
    }
}

fn board_score(figures: &BoardFigures) -> BoardScore {
    BoardScore {
        score: figures.score,
        incomplete: figures.incomplete,
    }
}

fn score_text(figures: &BoardFigures) -> String {
    log::format_score(
        figures.score,
        u32::try_from(figures.incomplete).unwrap_or(u32::MAX),
        u32::try_from(figures.violations).unwrap_or(u32::MAX),
    )
}
