//! `run_pipeline`, the routing job (A13.4): `RoutingPipeline.run`
//! (`RoutingPipeline.java:95-136` at `a773a70ec`).
//!
//! The CLI (C07), the GUI (G07) and the plugin route through
//! [`run_pipeline`]:
//!
//! 1. the stages: [`StageSelection::resolve`] over the registry, built with
//!    [`StageRegistry::build`] (an unknown per-stage id is an error before
//!    anything changes);
//! 2. the routing stage (`runRoutingStage`, `:107-128`): when the router is
//!    enabled (`autorouter.enabled && max_passes >= 0`) and no router stop
//!    was requested, the batch loop
//!    ([`run_batch_loop`](crate::autoroute::batch::batch_loop::run_batch_loop),
//!    or Q12's portfolio in its place when it is on); otherwise, with
//!    the fanout enabled, a fanout-only run: Java sets `max_passes = 0`
//!    for the loop; the port also disables the router explicitly (N06.2);
//! 3. the session dropped (`finishAutoroute`): the port keeps no autoroute
//!    engine on the board (a session lives for one connection), so there is
//!    nothing to drop;
//! 4. the optimizer stage of the registry (`runOptimizationStage`,
//!    `:130-142`) when `optimizer.enabled` and no **full** stop was
//!    requested: it runs after a router-only stop (max passes, stagnation)
//!    and not after `max_items`, a timeout or a cancel. The
//!    `optimizer.timeout` limit is armed when the stage starts, as a full
//!    stop ([`StopLevel::All`]), and disarmed when it ends; the stage's log
//!    lines go into the job log;
//! 5. Q16's stub cleanup ([`remove_stubs`](crate::quality::stubs::remove_stubs))
//!    when `router.cleanup.remove_stubs` is on (unset is off): after the
//!    optimizer and the board stages of its phase, also after a stop; it
//!    keeps every trace and via of the board as handed in;
//! 6. [`RouterEvents::finished`] hands the board to the listener by move;
//!    with no listener the board comes back in [`PipelineOutcome::board`].
//!
//! `run_pipeline` writes nothing: SES writing is C05's and the job flow
//! C07's. The board it hands back is the one after the best-board restore
//! and the optimizer (D8: the best board is the one saved), and its figures
//! are the [`RunSummary`]'s.
//!
//! **Time.** A job deadline ([`PipelineJob::deadline`]) only requests a
//! full stop through a [`Watcher`] on the job's cancel token (D2); the
//! batch loop then sees the job as timed out at the top of the next pass.
//! No budget inside the router is time.
//!
//! A fanout-only run explicitly disables the autorouter, including when an API
//! caller supplies a negative `max_passes`.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use crate::autoroute::batch::batch_loop::{LoopOutcome, LoopStages, PassExecutor, PassRecord};
use crate::autoroute::batch::pass::PassRunner;
use crate::autoroute::batch::report::NoRoutableLayer;
use crate::autoroute::events::{CancelToken, CompletionStatus, RouterEvents, RunSummary};
use crate::autoroute::stages::{
    FanoutSummary, OptimizerSummary, StageCtx, StagePhase, StageRegistry, StageSelection,
    StopReason, UnknownStage, lock_diagnostics,
};
use crate::board::RoutingBoard;
use crate::board::search::SearchConfig;
use crate::board::search::audit::AuditReport;
use crate::job::deadline::{StopLevel, Watcher};
use crate::job::{LogLevel, LogLine};
use crate::settings::RouterSettings;
use crate::settings::board::BoardRouterSettings;

/// What a routing job needs besides the board.
pub struct PipelineJob<'a> {
    /// The resolved settings.
    pub settings: &'a RouterSettings,
    /// The settings applied to the board (C03.7).
    pub board_settings: &'a BoardRouterSettings,
    /// The job's stop request, shared with the caller and the deadline.
    pub cancel: Arc<CancelToken>,
    /// Where progress and board changes go.
    pub events: &'a dyn RouterEvents,
    /// The job limit; when it passes, a full stop is requested.
    pub deadline: Option<Duration>,
    /// The stages to pick from; `None` is [`StageRegistry::builtin`].
    pub registry: Option<&'a StageRegistry>,
}

/// Why a job could not run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PipelineError {
    /// A per-stage settings key names no registered stage.
    UnknownStage(UnknownStage),
    /// No layer is active and a signal layer (Java's
    /// `IllegalArgumentException`).
    NoRoutableLayer(NoRoutableLayer),
    /// A search-tree setting (N03.1: `router.room_order`,
    /// `router.work_unit`) has an unknown value.
    BadSearchSetting(String),
}

impl fmt::Display for PipelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PipelineError::UnknownStage(e) => e.fmt(f),
            PipelineError::NoRoutableLayer(e) => e.fmt(f),
            PipelineError::BadSearchSetting(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for PipelineError {}

/// A job that could not run, with its board as it was handed in or as the
/// error left it.
#[derive(Debug)]
pub struct PipelineFailure {
    /// What went wrong.
    pub error: PipelineError,
    /// The board.
    pub board: Box<RoutingBoard>,
}

/// What a routing job did.
#[derive(Debug, Default)]
pub struct PipelineOutcome {
    /// The final board when no listener took it with
    /// [`RouterEvents::finished`].
    pub board: Option<RoutingBoard>,
    /// What the run reports (also sent with `Finished`).
    pub summary: RunSummary,
    /// True if the routing stage ran the batch loop (`routerEnabled`, or
    /// the fanout-only run).
    pub loop_ran: bool,
    /// The fanout's summary, if it ran.
    pub fanout: Option<FanoutSummary>,
    /// The pass log lines' figures.
    pub passes: Vec<PassRecord>,
    /// Why the pass loop stopped, if it ran passes.
    pub stop: Option<StopReason>,
    /// History restores during the passes.
    pub restores: usize,
    /// True if the final best-board restore swapped the board.
    pub restored_best: bool,
    /// The optimizer's summary, if it ran.
    pub optimizer: Option<OptimizerSummary>,
    /// Q16's stub cleanup, if `router.cleanup.remove_stubs` was on.
    pub stubs: Option<crate::quality::stubs::StubCleanup>,
    /// Q04.3's output guard, if `router.autorouter.output_guard` was on.
    pub output_guard: Option<crate::quality::output_guard::GuardOutcome>,
    /// Q12's pass-1 portfolio, if `router.autorouter.portfolio` or
    /// `restarts` was on.
    pub portfolio: Option<crate::quality::portfolio::PortfolioOutcome>,
    /// True if the job deadline fired.
    pub timed_out: bool,
    /// Java's job log lines.
    pub log: Vec<LogLine>,
    /// P02.1's search-tree audit, if `router.debug.audit_incremental` was on.
    pub audit: Option<AuditReport>,
}

/// The job log lines of P02.1's audit: the tally, and the first difference
/// as an error.
fn audit_log(audit: &AuditReport) -> Vec<LogLine> {
    let mut lines = vec![LogLine {
        level: LogLevel::Info,
        text: audit.summary_line(),
    }];
    if let Some((point, difference)) = &audit.first {
        lines.push(LogLine {
            level: LogLevel::Error,
            text: format!("Incremental search-tree audit, first difference {point}: {difference}"),
        });
    }
    lines
}

/// Java's router-enabled rule (`RoutingPipeline.java:108-111`):
/// `autorouter.enabled && (max_passes == null || max_passes >= 0)`. The
/// port's `max_passes` is never null (its default is 0, unlimited).
pub fn router_enabled(settings: &RouterSettings) -> bool {
    settings.run_router() && settings.autorouter.max_passes >= 0
}

/// Runs the routing job on `board` with the real pass runner (see the
/// module docs).
pub fn run_pipeline(
    board: RoutingBoard,
    job: &PipelineJob<'_>,
) -> Result<PipelineOutcome, Box<PipelineFailure>> {
    run_pipeline_with(board, job, &mut |settings, board_settings| {
        Box::new(PassRunner::for_job(settings, board_settings))
    })
}

/// The settings and board settings a pass executor is built from.
pub type ExecutorFactory<'f> =
    dyn FnMut(&RouterSettings, &BoardRouterSettings) -> Box<dyn PassExecutor> + 'f;

/// [`run_pipeline`] with the pass executor `executor` makes (tests use a
/// fake one).
pub fn run_pipeline_with(
    mut board: RoutingBoard,
    job: &PipelineJob<'_>,
    executor: &mut ExecutorFactory<'_>,
) -> Result<PipelineOutcome, Box<PipelineFailure>> {
    let builtin;
    let registry = match job.registry {
        Some(registry) => registry,
        None => {
            builtin = StageRegistry::builtin();
            &builtin
        }
    };
    let settings = job.settings;
    let fail = |error, board| {
        Box::new(PipelineFailure {
            error,
            board: Box::new(board),
        })
    };
    let selection = StageSelection::resolve(settings, registry);
    let mut log: Vec<LogLine> = selection
        .warnings
        .iter()
        .map(|text| LogLine {
            level: LogLevel::Warn,
            text: text.clone(),
        })
        .collect();
    log.push(LogLine {
        level: LogLevel::Info,
        text: selection.log_line(),
    });
    // N03.1 (P04): the room order and work unit, set before any work limit
    // starts. Logged only when they differ from Java's, so a `legacy` run
    // logs what Java logs.
    match SearchConfig::from_settings(settings) {
        Ok(config) => {
            let reversed_builds = board.search_trees.search_config().reversed_builds;
            board.search_trees.set_search_config(SearchConfig {
                reversed_builds,
                ..config
            });
            if !config.is_java() {
                log.push(LogLine {
                    level: LogLevel::Info,
                    text: config.log_line(),
                });
            }
        }
        Err(e) => return Err(fail(PipelineError::BadSearchSetting(e), board)),
    }

    // Q16: the copper of the board as loaded, which the stub cleanup keeps.
    let input_copper = crate::quality::stubs::enabled(settings)
        .then(|| crate::quality::stubs::input_copper(&board));

    // PARITY: RoutingPipeline.java:115-123, the fanout-only run sets
    // `max_passes = 0` for the loop.
    let cancel = &*job.cancel;
    let enabled = router_enabled(settings);
    let loop_settings = if !cancel.is_router_stop_requested() && enabled {
        Some(settings.clone())
    } else if !cancel.is_router_stop_requested() && settings.fanout.enabled {
        let mut fanout_only = settings.clone();
        fanout_only.autorouter.max_passes = 0;
        fanout_only.autorouter.enabled = false;
        Some(fanout_only)
    } else {
        None
    };

    let mut pipeline = match registry.build(&selection, settings) {
        Ok(pipeline) => pipeline,
        Err(e) => return Err(fail(PipelineError::UnknownStage(e), board)),
    };
    // Q10: an unknown `router.autorouter.sweep` value fails before routing.
    if let Err(e) = crate::quality::sweep::check_settings(settings) {
        return Err(fail(PipelineError::UnknownStage(e), board));
    }
    // Q04.1: an invalid `router.quantum_stop` value fails before routing.
    if let Err(e) = crate::quality::stop_rule::check_settings(settings) {
        return Err(fail(PipelineError::UnknownStage(e), board));
    }
    // Q11: an invalid `router.transaction` value fails before routing.
    if let Err(e) = crate::quality::transaction::check_settings(settings) {
        return Err(fail(PipelineError::UnknownStage(e), board));
    }
    // Q12: a negative `portfolio` or `restarts` fails before routing.
    if let Err(e) = crate::quality::portfolio::check_settings(settings) {
        return Err(fail(PipelineError::UnknownStage(e), board));
    }
    let watcher = Watcher::new(Arc::clone(&job.cancel));
    if let Some(limit) = job.deadline {
        watcher.arm(limit, StopLevel::All);
    }
    let timed_out = || watcher.fired();

    let mut outcome = PipelineOutcome::default();
    let mut loop_outcome = LoopOutcome::default();
    // Q06: the diagnostics that see the job; the loop's, when it runs, as
    // its connection router feeds them.
    let mut diagnostics = Arc::clone(&pipeline.diagnostics);
    if let Some(loop_settings) = &loop_settings {
        // The loop's stages come from the settings the loop sees.
        let mut loop_pipeline = match registry.build(&selection, loop_settings) {
            Ok(pipeline) => pipeline,
            Err(e) => return Err(fail(PipelineError::UnknownStage(e), board)),
        };
        let mut runner = executor(loop_settings, job.board_settings);
        let mut ctx = StageCtx {
            settings: loop_settings,
            board_settings: job.board_settings,
            cancel,
            events: job.events,
        };
        diagnostics = Arc::clone(&loop_pipeline.diagnostics);
        for diagnostic in lock_diagnostics(&diagnostics).iter_mut() {
            diagnostic.on_input(&board, &ctx);
        }
        let mut stages = LoopStages {
            fanout: loop_pipeline.fanout.as_mut(),
            ordering: loop_pipeline.ordering.as_mut(),
            router: loop_pipeline.connection_router.as_mut(),
            ripup: loop_pipeline.ripup_policy.as_mut(),
            controller: loop_pipeline.pass_controller.as_mut(),
            board_stages: &mut loop_pipeline.board_stages,
        };
        // Q12: the batch loop, or the pass-1 portfolio in its place (off by
        // default: then it is `run_batch_loop`).
        match crate::quality::portfolio::run_routing(
            &mut board,
            &mut stages,
            runner.as_mut(),
            &mut ctx,
            &timed_out,
            &crate::quality::portfolio::PortfolioEnv {
                registry,
                selection: &selection,
            },
        ) {
            Ok(result) => {
                loop_outcome = result;
                outcome.audit = runner.audit_report();
            }
            Err(e) => {
                drop(watcher);
                return Err(fail(PipelineError::NoRoutableLayer(e), board));
            }
        }
        // Q10.3: optimizer, then the add-only sweep; the optimizer below
        // runs again. Off by default.
        crate::quality::sweep::after_optimizer(
            &mut board,
            &mut stages,
            runner.as_mut(),
            pipeline.optimizer.as_mut(),
            &mut ctx,
            &watcher,
            &mut loop_outcome,
        );
        // Q11: the connection router's tally (none for Java's).
        for text in loop_pipeline.connection_router.log_lines() {
            log.push(LogLine {
                level: LogLevel::Info,
                text,
            });
        }
        outcome.loop_ran = true;
    }
    log.append(&mut loop_outcome.log);
    if let Some(audit) = &outcome.audit {
        log.extend(audit_log(audit));
    }
    // PARITY: :125, `finishAutoroute()`: nothing on the board to drop.

    // PARITY: :130-136, the optimizer unless a full stop was requested.
    let mut ctx = StageCtx {
        settings,
        board_settings: job.board_settings,
        cancel,
        events: job.events,
    };
    if settings.run_optimizer() && !cancel.is_stop_requested() {
        // C07.8 / O06: `optimizer.timeout` counts from the start of the
        // stage and only requests a full stop.
        let deadline = settings
            .deadlines()
            .optimizer
            .map(|limit| watcher.arm(limit, StopLevel::All));
        let summary = pipeline.optimizer.run(&mut board, &mut ctx);
        if let Some(id) = deadline {
            watcher.disarm(id);
        }
        log.extend(summary.log.iter().cloned());
        outcome.optimizer = Some(summary);
    }
    for stage in pipeline.board_stages.iter_mut() {
        if stage.phase() == StagePhase::AfterOptimizer {
            for text in stage.run(&mut board, &mut ctx).log {
                log.push(LogLine {
                    level: LogLevel::Info,
                    text,
                });
            }
        }
    }
    // Q16: the stub cleanup, last; off by default.
    if let Some(keep) = &input_copper {
        let cleanup = crate::quality::stubs::remove_stubs(&mut board, keep);
        log.push(LogLine {
            level: LogLevel::Info,
            text: cleanup.log_line(),
        });
        outcome.stubs = Some(cleanup);
    }
    outcome.timed_out = watcher.finish();

    let stubs_removed = outcome.stubs.is_some_and(|s| s.removed() > 0);
    let figures =
        if outcome.optimizer.is_some() || stubs_removed || loop_outcome.final_figures.is_none() {
            crate::autoroute::batch::snapshot::BoardFigures::of(&board, settings)
        } else {
            loop_outcome.final_figures.clone().unwrap_or_default()
        };
    let initial_unrouted = if outcome.loop_ran {
        loop_outcome.initial_unrouted
    } else {
        board.routing_ledger().incomplete_count(&board)
    };
    outcome.summary = RunSummary {
        status: if outcome.timed_out {
            CompletionStatus::CompletedWithTimeout
        } else if cancel.is_stop_requested() {
            CompletionStatus::Interrupted
        } else {
            CompletionStatus::Completed
        },
        initial_unrouted,
        passes: i32::try_from(loop_outcome.current_pass).unwrap_or(i32::MAX),
        score: figures.score,
        incomplete: figures.incomplete,
        violations: figures.violations,
        stages: selection.entries(),
        verdicts: None,
        rip_stats: None,
        widths: None,
    };
    for diagnostic in lock_diagnostics(&diagnostics).iter_mut() {
        diagnostic.finish(&board, &ctx, &mut outcome.summary);
    }
    outcome.fanout = loop_outcome.fanout;
    outcome.passes = loop_outcome.passes;
    outcome.stop = loop_outcome.stop;
    outcome.restores = loop_outcome.restores;
    outcome.restored_best = loop_outcome.restored_best;
    outcome.output_guard = loop_outcome.output_guard;
    outcome.portfolio = loop_outcome.portfolio;
    outcome.log = log;
    outcome.board = job.events.finished(board, outcome.summary.clone());
    Ok(outcome)
}
