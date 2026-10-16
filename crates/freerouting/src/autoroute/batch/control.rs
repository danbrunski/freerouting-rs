//! `FreeroutingPassController` (A10.3): the stop and restore rules of
//! `AutorouteBatchLoop.run` (`AutorouteBatchLoop.java:297-645` at
//! `a773a70ec`), with no board inside.
//!
//! The driver (A13) runs the pass, then feeds the controller a
//! [`PassReport`] and applies the decisions it gets back, one at a time, in
//! Java's order within one loop iteration:
//!
//! 1. the restore check (`:345-405`): [`PassDecision::Restore`], then
//!    [`ActionOutcome::Restored`];
//! 2. after the mandatory passes, while the router still works
//!    (`:482-590`): the pass-local counter, the one-time
//!    [`PassDecision::FanoutRecovery`], at most two
//!    [`PassDecision::LastMileRipup`]s, the pass-local stop and the global
//!    best tracker; otherwise the success reset of `:591-599`;
//! 3. the loop condition (`:602-605`): continue, or stop because the pass
//!    found nothing to route or a router stop was requested.
//!
//! Every score is an `f32` and every comparison is Java's, including the
//! threshold added in `f32` (`lastBestScore + STAGNATION_SCORE_THRESHOLD`).
//! The controller keeps no item or net state, so a restore only changes the
//! counters Java changes.

use super::{
    BOARD_RANK_LIMIT, FANOUT_RECOVERY_STAGNATION_PASSES, LAST_MILE_INCOMPLETE_LIMIT,
    LAST_MILE_MAX_ATTEMPTS, LAST_MILE_STAGNATION_PASSES, MAXIMUM_TRIES_ON_THE_SAME_BOARD,
    STAGNATION_PASS_LIMIT, STAGNATION_SCORE_THRESHOLD, STOP_AT_PASS_MINIMUM, STOP_AT_PASS_MODULO,
};
use crate::autoroute::stages::{
    ActionOutcome, BoardScore, FinishDecision, PassController, PassDecision, PassReport, PassStart,
    StopReason,
};
use crate::settings::RouterSettings;

/// Where the controller is inside one pass's decisions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    /// No pass report pending.
    Idle,
    /// Waiting for the restore's outcome.
    AwaitRestore,
    /// Waiting for the fanout recovery's outcome.
    AwaitFanoutRecovery,
    /// Waiting for the last-mile ripup's outcome.
    AwaitLastMile,
}

/// The pass being decided: its report and the board as it stands after the
/// actions so far.
#[derive(Clone, Copy, Debug)]
struct Current {
    pass: u32,
    continue_routing: bool,
    router_stop_requested: bool,
    board: BoardScore,
}

/// Java Freerouting's pass controller (`freerouting-passes`).
#[derive(Clone, Debug)]
pub struct FreeroutingPassController {
    max_passes: i32,
    fanout_enabled: bool,
    router_run: bool,
    step: Step,
    current: Option<Current>,
    /// `consecutiveNoImprovementPasses`, reset by a restore.
    no_improvement: u32,
    fanout_recovery_applied: bool,
    last_mile_attempts: u32,
    /// `lastBestScore`: the score at the last restore or improvement.
    last_best: f32,
    /// `globalBestScore`, never reset by a restore.
    global_best: f32,
    /// `passOfBestScore`.
    pass_of_best: u32,
    /// `incompleteCountAtBestScore`, for the stop message.
    incomplete_at_best: usize,
}

impl FreeroutingPassController {
    /// A controller with Java's start state.
    ///
    /// - `max_passes`: `router.autorouter.max_passes`, 0 for unlimited;
    /// - `fanout_enabled`: `isFanoutEnabled()`;
    /// - `router_run`: `getRunRouter() && maxPasses >= 0` (`:635-637`).
    pub fn new(max_passes: i32, fanout_enabled: bool, router_run: bool) -> Self {
        FreeroutingPassController {
            max_passes,
            fanout_enabled,
            router_run,
            step: Step::Idle,
            current: None,
            no_improvement: 0,
            fanout_recovery_applied: false,
            last_mile_attempts: 0,
            last_best: f32::NEG_INFINITY,
            global_best: f32::NEG_INFINITY,
            pass_of_best: 0,
            incomplete_at_best: 0,
        }
    }

    /// The controller for `settings`.
    pub fn from_settings(settings: &RouterSettings) -> Self {
        let max_passes = settings.autorouter.max_passes;
        FreeroutingPassController::new(
            max_passes,
            settings.fanout.enabled,
            settings.run_router() && max_passes >= 0,
        )
    }

    /// The pass-local no-improvement counter.
    pub fn no_improvement_passes(&self) -> u32 {
        self.no_improvement
    }

    /// The global best score and the pass it was recorded at.
    pub fn global_best(&self) -> (f32, u32) {
        (self.global_best, self.pass_of_best)
    }

    /// The incomplete count when the global best was recorded (Java's
    /// stop message).
    pub fn incomplete_at_best(&self) -> usize {
        self.incomplete_at_best
    }

    /// The number of last-mile ripups so far.
    pub fn last_mile_attempts(&self) -> u32 {
        self.last_mile_attempts
    }

    fn current(&self) -> Current {
        self.current
            .expect("a pass decision needs a pass report (after_pass first)")
    }

    /// Step 2 from its start: the pass-local counter.
    fn stagnation(&mut self) -> PassDecision {
        let c = self.current();
        // PARITY: AutorouteBatchLoop.java:482.
        if c.pass >= STOP_AT_PASS_MINIMUM && c.continue_routing {
            // PARITY: :485-488, the threshold added in f32.
            if c.board.score > self.last_best + STAGNATION_SCORE_THRESHOLD {
                self.no_improvement = 0;
                self.last_best = c.board.score;
                return self.global_tracker();
            }
            self.no_improvement += 1;
            // PARITY: :495-498, the one-time fanout recovery.
            if self.fanout_enabled
                && !self.fanout_recovery_applied
                && c.board.incomplete > 0
                && self.no_improvement >= FANOUT_RECOVERY_STAGNATION_PASSES
            {
                self.step = Step::AwaitFanoutRecovery;
                return PassDecision::FanoutRecovery;
            }
            return self.last_mile();
        }
        // PARITY: :591-599, a complete board with a positive score.
        if c.board.incomplete == 0 && c.board.score > STAGNATION_SCORE_THRESHOLD {
            self.no_improvement = 0;
            self.last_best = c.board.score;
        }
        self.end_of_pass()
    }

    /// Step 2 after the fanout recovery: the last-mile ripup.
    fn last_mile(&mut self) -> PassDecision {
        let c = self.current();
        let incomplete = c.board.incomplete;
        // PARITY: :517-520.
        if incomplete > 0
            && incomplete <= LAST_MILE_INCOMPLETE_LIMIT
            && self.last_mile_attempts < LAST_MILE_MAX_ATTEMPTS
            && self.no_improvement >= LAST_MILE_STAGNATION_PASSES
        {
            self.step = Step::AwaitLastMile;
            return PassDecision::LastMileRipup;
        }
        self.pass_local_stop()
    }

    /// Step 2 after the last-mile ripup: the pass-local stop.
    fn pass_local_stop(&mut self) -> PassDecision {
        // PARITY: :538.
        if self.no_improvement >= STAGNATION_PASS_LIMIT {
            return self.stop(StopReason::Stagnation);
        }
        self.global_tracker()
    }

    /// The global best tracker (`:566-589`), never reset by a restore.
    fn global_tracker(&mut self) -> PassDecision {
        let c = self.current();
        if c.board.score > self.global_best + STAGNATION_SCORE_THRESHOLD {
            self.global_best = c.board.score;
            self.pass_of_best = c.pass;
            self.incomplete_at_best = c.board.incomplete;
        } else if c.pass - self.pass_of_best >= STAGNATION_PASS_LIMIT {
            return self.stop(StopReason::GlobalStagnation);
        }
        self.end_of_pass()
    }

    /// Step 3: the loop condition (`:602-605`).
    fn end_of_pass(&mut self) -> PassDecision {
        let c = self.current();
        if c.router_stop_requested {
            self.stop(StopReason::StopRequested)
        } else if !c.continue_routing {
            self.stop(StopReason::Completed)
        } else {
            self.step = Step::Idle;
            self.current = None;
            PassDecision::Continue
        }
    }

    fn stop(&mut self, reason: StopReason) -> PassDecision {
        self.step = Step::Idle;
        self.current = None;
        PassDecision::Stop(reason)
    }
}

impl PassController for FreeroutingPassController {
    fn id(&self) -> &'static str {
        "freerouting-passes"
    }

    fn before_pass(&mut self, pass: u32, timed_out: bool) -> PassStart {
        // PARITY: AutorouteBatchLoop.java:314-316, a timed-out job requests a
        // router stop and still runs the pass, which then routes nothing.
        // PARITY: :331-336, then the pass limit.
        if self.max_passes > 0 && i64::from(pass) > i64::from(self.max_passes) {
            return PassStart::Stop(StopReason::MaxPasses);
        }
        PassStart::Run {
            stop_router: timed_out,
        }
    }

    fn after_pass(&mut self, r: &PassReport) -> PassDecision {
        let stop_requested = r.router_stop_requested || r.timed_out;
        self.current = Some(Current {
            pass: r.pass,
            continue_routing: r.continue_routing,
            router_stop_requested: stop_requested,
            board: BoardScore {
                score: r.score,
                incomplete: r.incomplete,
            },
        });
        // PARITY: :357-366, the restore check: Java's `(full || stop) &&
        // (check pass || stop)` is `stop || (full && check pass)`; strict `>`
        // so equal scores never restore.
        let history_full = r.history_size >= STOP_AT_PASS_MINIMUM as usize;
        let check_pass = r.pass % STOP_AT_PASS_MODULO == 0 && r.pass >= STOP_AT_PASS_MINIMUM;
        if (stop_requested || (history_full && check_pass)) && r.history_max_score > r.score {
            self.step = Step::AwaitRestore;
            return PassDecision::Restore {
                max_restore_count: MAXIMUM_TRIES_ON_THE_SAME_BOARD,
            };
        }
        self.stagnation()
    }

    fn after_action(&mut self, outcome: &ActionOutcome) -> PassDecision {
        let mut c = self.current();
        match (self.step, outcome) {
            (Step::AwaitRestore, ActionOutcome::Restored(restored)) => {
                // PARITY: :368-374, no board means stop.
                let Some(restored) = restored else {
                    return self.stop(StopReason::HistoryExhausted);
                };
                // PARITY: :376-381.
                if restored.rank.is_some_and(|rank| rank > BOARD_RANK_LIMIT) {
                    return self.stop(StopReason::RankLimit);
                }
                // PARITY: :383-391.
                self.no_improvement = 0;
                c.board = restored.board;
                self.last_best = c.board.score;
                self.current = Some(c);
                self.stagnation()
            }
            (Step::AwaitFanoutRecovery, ActionOutcome::FanoutRecovered(board)) => {
                // PARITY: :500-506.
                c.board = *board;
                self.current = Some(c);
                self.last_best = board.score;
                self.no_improvement = 0;
                self.fanout_recovery_applied = true;
                self.last_mile()
            }
            (Step::AwaitLastMile, ActionOutcome::LastMile { removed, board }) => {
                // PARITY: :521-528, the attempt counts even when nothing was
                // removed.
                self.last_mile_attempts += 1;
                if *removed > 0 {
                    c.board = *board;
                    self.current = Some(c);
                    self.last_best = board.score;
                    self.no_improvement = 0;
                }
                self.pass_local_stop()
            }
            (step, outcome) => panic!("pass controller: {outcome:?} does not answer {step:?}"),
        }
    }

    fn finish(
        &mut self,
        final_score: f32,
        history_max_score: f32,
        router_stopped: bool,
    ) -> FinishDecision {
        FinishDecision {
            // PARITY: :610-612.
            restore_best: history_max_score > final_score,
            // PARITY: :635-639.
            remove_tails: self.router_run && !router_stopped,
        }
    }
}
