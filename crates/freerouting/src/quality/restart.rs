//! Q23.3: restarts from the best board (`router.autorouter.restarts_from_best`).
//!
//! Q23 names the setting `router.autorouter.restarts`; that key is Q12's
//! removed portfolio setting (`settings::removed`, refused with its removal
//! note), so the stage takes `restarts_from_best` instead.
//!
//! Routing a run's own output again was the PCU rev 2.0 board's most
//! effective step (`docs/development/pcu-rev2-board-report.md`), and Q23.1
//! attributed the gain: a second pass 1 on the routed board completes most
//! of it (PCU 206 -> 113 unrouted, canary-pwr 119 -> 107), and running the
//! fanout again first adds a little (103 and 105). A restart does that inside
//! one job.
//!
//! **One restart**, after the batch loop's finish (the history's best board
//! is then on the board, A13) and before Q10's sweep:
//! 1. the board as it stands is kept (a `Clone`, so the search trees keep
//!    their shape) with its figures: the best board so far;
//! 2. the batch loop runs again on it ([`run_batch_loop`]), with the job's
//!    fanout, ordering, connection router, ripup policy and pass runner, a new
//!    pass controller of the job's kind (built from the built-in
//!    [`StageRegistry`] by its id) and no board stages: the fanout stage
//!    (when `fanout.enabled`), then the passes from pass 1, so pass 1's
//!    rip-up costs, strict check and ordering, and the loop's own stop rules,
//!    `max_passes` and best-board restore;
//! 3. the result is kept only if its router score is strictly higher than the
//!    best board's (the history's own comparison); otherwise the kept board
//!    comes back and the restarts stop.
//!
//! Up to `restarts_from_best` restarts run, each from the board the last kept restart
//! left. They also stop when the best board has no incomplete connection.
//! Inside a restart the settings have `restarts_from_best`, `sweep` and
//! `output_guard` unset: no restart nests, the sweep runs once after the
//! restarts, and the output guard has already judged the first loop's board.
//!
//! **Stops.** No restart starts after a requested stop (the loop's
//! [`StopReason::StopRequested`], a timeout, a cancel). The first loop asks
//! for a router stop when it ends on its pass limit or stagnation, so a
//! restart routes with its own [`CancelToken`]; a full stop of the job (the
//! deadline, a cancel) reaches it through [`with_stop_relay`], and the
//! restart's loop then stops and restores its own best board, which is kept
//! only if it beats the best board before the restart. A full stop the
//! restart requests (`max_items`) goes back to the job's token. As for the
//! sweep, a router-only stop of the user is not seen once the first loop has
//! ended.
//!
//! **Deterministic.** No time reaches a decision: the keep rule compares two
//! scores, the counts are the loop's, and the stop relay only stops work.
//! The restart's passes appear in `--stats json` as more `fanout` and `pass`
//! stages numbered from 1 again, after the first loop's.
//!
//! Off by default: with `restarts_from_best` unset, 0 or negative nothing here runs and
//! the job's result and log are the unchanged engine's.

use crate::autoroute::batch::batch_loop::{LoopOutcome, LoopStages, PassExecutor, run_batch_loop};
use crate::autoroute::batch::snapshot::BoardFigures;
use crate::autoroute::events::{CancelToken, RelayLevels, with_stop_relay};
use crate::autoroute::stages::{BoardStage, StageCtx, StageRegistry, StopReason};
use crate::board::RoutingBoard;
use crate::board::search::audit::AuditPoint;
use crate::job::{LogLevel, LogLine, log};
use crate::settings::RouterSettings;

/// The settings key, as the CLI and the JSON file name it.
pub const SETTINGS_KEY: &str = "router.autorouter.restarts_from_best";

/// The number of restarts `settings` ask for: `router.autorouter.restarts_from_best`,
/// 0 when unset, 0 or negative.
pub fn restarts(settings: &RouterSettings) -> u32 {
    settings
        .autorouter
        .restarts_from_best
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or(0)
}

/// Why the restarts stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestartStop {
    /// `restarts_from_best` restarts ran and each was kept.
    Limit,
    /// A restart did not beat the best board; the best board came back.
    NoGain,
    /// The best board has no incomplete connection.
    Complete,
    /// A stop was requested (a timeout, a cancel, `max_items`) or the first
    /// loop ended on one.
    StopRequested,
    /// The job's pass controller is not one of the built-in registry's, so
    /// no new one can be built.
    UnknownController,
}

impl RestartStop {
    fn text(self) -> &'static str {
        match self {
            RestartStop::Limit => "reached the restart limit",
            RestartStop::NoGain => "stopped: a restart did not beat the best board",
            RestartStop::Complete => "stopped: the board is complete",
            RestartStop::StopRequested => "stopped on request",
            RestartStop::UnknownController => "not run: the pass controller is not a built-in one",
        }
    }
}

/// One restart.
#[derive(Clone, Debug, PartialEq)]
pub struct RestartRecord {
    /// The restart number, from 1.
    pub restart: u32,
    /// The best board's figures before it.
    pub before: BoardFigures,
    /// The figures of the board the restart's loop left (after its own
    /// best-board restore), before the keep rule.
    pub after: BoardFigures,
    /// The restart's passes (its pass log lines).
    pub passes: usize,
    /// Why the restart's loop stopped.
    pub stop: Option<StopReason>,
    /// True if the restart beat the best board and was kept.
    pub kept: bool,
}

/// What the restarts did.
#[derive(Clone, Debug, PartialEq)]
pub struct RestartOutcome {
    /// The restarts, in order.
    pub restarts: Vec<RestartRecord>,
    /// Why they stopped.
    pub stop: RestartStop,
    /// The board's figures before the first restart.
    pub before: BoardFigures,
    /// The figures of the board the restarts leave.
    pub after: BoardFigures,
    /// The job log lines, in order: each restart's own loop lines between
    /// its start and end lines.
    pub log: Vec<LogLine>,
}

/// True if a restart that left `after` beats the best board `best`: a
/// strictly higher router score (the score counts the incomplete
/// connections and the clearance violations).
pub fn beats(after: &BoardFigures, best: &BoardFigures) -> bool {
    after.score > best.score
}

/// The restarts after the batch loop's finish when the settings ask for them
/// (called by `run_batch_loop`); `None` when `restarts_from_best` is unset or not
/// positive, or the router did not run. `figures` are those of `board`.
pub fn after_loop(
    board: &mut RoutingBoard,
    stages: &mut LoopStages<'_>,
    executor: &mut dyn PassExecutor,
    ctx: &StageCtx,
    outcome: &LoopOutcome,
    figures: &BoardFigures,
    timed_out: &dyn Fn() -> bool,
) -> Option<RestartOutcome> {
    let limit = restarts(ctx.settings);
    if limit == 0 || !outcome.router_enabled {
        return None;
    }
    let stopped = outcome.stop == Some(StopReason::StopRequested);
    Some(run_restarts(
        board, stages, executor, ctx, figures, limit, stopped, timed_out,
    ))
}

/// The settings of a restart's loop: the job's with `restarts_from_best`, `sweep` and
/// `output_guard` unset.
fn restart_settings(settings: &RouterSettings) -> RouterSettings {
    let mut inner = settings.clone();
    inner.autorouter.restarts_from_best = None;
    inner.autorouter.sweep = None;
    inner.autorouter.output_guard = None;
    inner
}

/// Runs up to `limit` restarts on `board` (see the module docs).
/// `loop_stopped` is true when the first loop ended on a requested stop.
#[allow(clippy::too_many_arguments)]
fn run_restarts(
    board: &mut RoutingBoard,
    stages: &mut LoopStages<'_>,
    executor: &mut dyn PassExecutor,
    ctx: &StageCtx,
    figures: &BoardFigures,
    limit: u32,
    loop_stopped: bool,
    timed_out: &dyn Fn() -> bool,
) -> RestartOutcome {
    let listening = ctx.events.is_listening();
    let inner_settings = restart_settings(ctx.settings);
    let factory = StageRegistry::builtin()
        .pass_controller
        .get(stages.controller.id())
        .copied();
    let mut best = figures.clone();
    let mut outcome = RestartOutcome {
        restarts: Vec::new(),
        stop: RestartStop::Limit,
        before: best.clone(),
        after: best.clone(),
        log: Vec::new(),
    };
    for restart in 1..=limit {
        if loop_stopped || timed_out() || ctx.cancel.is_stop_requested() {
            outcome.stop = RestartStop::StopRequested;
            break;
        }
        if best.incomplete == 0 {
            outcome.stop = RestartStop::Complete;
            break;
        }
        let Some(factory) = factory else {
            outcome.stop = RestartStop::UnknownController;
            break;
        };
        info(
            &mut outcome.log,
            format!(
                "Restart #{restart} from the best board (score {}): the fanout, then the passes \
                 from pass 1.",
                score_text(&best)
            ),
        );
        let saved = board.clone();
        let local = CancelToken::new();
        let mut inner_ctx = StageCtx {
            settings: &inner_settings,
            board_settings: ctx.board_settings,
            cancel: &local,
            events: ctx.events,
        };
        let mut controller = factory(&inner_settings);
        let mut no_board_stages: Vec<Box<dyn BoardStage>> = Vec::new();
        let result = {
            let mut inner = LoopStages {
                fanout: &mut *stages.fanout,
                ordering: &mut *stages.ordering,
                router: &mut *stages.router,
                ripup: &mut *stages.ripup,
                controller: controller.as_mut(),
                board_stages: &mut no_board_stages,
            };
            // A job stop also ends the connection in progress.
            with_stop_relay(ctx.cancel, &local, RelayLevels::FullStop, || {
                run_batch_loop(board, &mut inner, executor, &mut inner_ctx, timed_out)
            })
        };
        // `max_items` in the restart is a full stop of the job.
        if local.is_stop_requested() && !ctx.cancel.is_stop_requested() && !timed_out() {
            ctx.cancel.stop_all();
        }
        if listening {
            board.set_journaling(true);
        }
        let (after, passes, stop) = match result {
            Ok(inner) => {
                outcome.log.extend(inner.log.iter().cloned());
                let after = inner
                    .final_figures
                    .clone()
                    .unwrap_or_else(|| executor.figures(board, ctx.settings));
                (after, inner.passes.len(), inner.stop)
            }
            // The first loop passed the same check on the same layers.
            Err(e) => {
                info(&mut outcome.log, format!("Restart #{restart}: {e}"));
                (best.clone(), 0, None)
            }
        };
        let kept = beats(&after, &best);
        info(
            &mut outcome.log,
            format!(
                "Restart #{restart} ended with score {} after {passes} pass(es): {}.",
                score_text(&after),
                if kept {
                    "it beats the best board and is kept".to_string()
                } else {
                    format!("the best board (score {}) is kept", score_text(&best))
                }
            ),
        );
        outcome.restarts.push(RestartRecord {
            restart,
            before: best.clone(),
            after: after.clone(),
            passes,
            stop,
            kept,
        });
        if kept {
            best = after;
        } else {
            swap_in(board, saved, ctx);
            executor.audit(board, AuditPoint::Restore);
        }
        // A stop names itself whether the restart gained or not.
        if local.is_stop_requested() || ctx.cancel.is_stop_requested() || timed_out() {
            outcome.stop = RestartStop::StopRequested;
            break;
        }
        if !kept {
            outcome.stop = RestartStop::NoGain;
            break;
        }
    }
    info(
        &mut outcome.log,
        format!(
            "Restarts from the best board {}: {} restart(s), {} kept, unrouted {} -> {}, \
             violations {} -> {}.",
            outcome.stop.text(),
            outcome.restarts.len(),
            outcome.restarts.iter().filter(|r| r.kept).count(),
            outcome.before.incomplete,
            best.incomplete,
            outcome.before.violations,
            best.violations
        ),
    );
    outcome.after = best;
    outcome
}

fn info(log: &mut Vec<LogLine>, text: String) {
    log.push(LogLine {
        level: LogLevel::Info,
        text,
    });
}

fn score_text(figures: &BoardFigures) -> String {
    log::format_score(
        figures.score,
        u32::try_from(figures.incomplete).unwrap_or(u32::MAX),
        u32::try_from(figures.violations).unwrap_or(u32::MAX),
    )
}

/// Swaps `saved` in and announces it, as the batch loop's restore does.
fn swap_in(board: &mut RoutingBoard, saved: RoutingBoard, ctx: &StageCtx) {
    *board = saved;
    if ctx.events.is_listening() {
        ctx.events.replaced(board.snapshot());
        board.set_journaling(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn figures(score: f32, incomplete: usize) -> BoardFigures {
        BoardFigures {
            score,
            incomplete,
            violations: 0,
        }
    }

    #[test]
    fn unset_zero_and_negative_are_off() {
        let mut settings =
            crate::settings::RouterSettings::from_patch(&crate::settings::default_patch(1), 1);
        assert_eq!(restarts(&settings), 0);
        for (value, want) in [(0, 0), (-3, 0), (1, 1), (4, 4)] {
            settings.autorouter.restarts_from_best = Some(value);
            assert_eq!(restarts(&settings), want, "{value}");
        }
    }

    #[test]
    fn only_a_strictly_higher_score_beats_the_best() {
        let best = figures(900.0, 5);
        assert!(beats(&figures(900.5, 4), &best));
        assert!(!beats(&figures(900.0, 5), &best));
        assert!(!beats(&figures(899.0, 6), &best));
    }

    #[test]
    fn a_restart_runs_with_restarts_sweep_and_output_guard_unset() {
        let mut settings =
            crate::settings::RouterSettings::from_patch(&crate::settings::default_patch(1), 1);
        settings.autorouter.restarts_from_best = Some(2);
        settings.autorouter.sweep = Some("free".to_string());
        settings.autorouter.output_guard = Some(true);
        settings.autorouter.max_passes = 3;
        let inner = restart_settings(&settings);
        assert_eq!(inner.autorouter.restarts_from_best, None);
        assert_eq!(inner.autorouter.sweep, None);
        assert_eq!(inner.autorouter.output_guard, None);
        assert_eq!(inner.autorouter.max_passes, 3);
    }
}
