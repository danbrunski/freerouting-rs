//! Wall-clock limits that only stop work (C07.5, D2).
//!
//! Port of the timeout side of `RoutingJobSchedulerActionThread`. Java has a
//! monitor thread that polls once a second, calls `requestStop()` when
//! `timeoutAt` has passed, and allows a 30 s grace period before it marks the
//! job `TIMED_OUT`. Here a [`Watcher`] thread sleeps until the earliest armed
//! deadline and then asks the shared [`CancelToken`] to stop. There is no
//! polling and no grace period, because the stages check the token between
//! steps. A deadline never changes how work is done: it only ends it, and the
//! job then reports `TimedOut` (see [`assign_terminal_state`]) and logs
//! [`TIMEOUT_WARNING`](super::log::TIMEOUT_WARNING).
//!
//! The job limit goes through `run_job` to `run_pipeline`, whose watcher arms it with
//! [`StopLevel::All`]. The fanout and optimizer limits are measured from the
//! start of their stage, so the pipeline arms them when the stage starts
//! (`Watcher::arm`): the fanout limit with [`StopLevel::Router`], the
//! optimizer limit with [`StopLevel::All`]. The pipeline reads the fanout
//! limit itself (`FanoutRun::new`); the optimizer stage is a placeholder
//! until O06, which arms its limit then.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::run::JobState;
use crate::autoroute::events::CancelToken;

/// What a deadline asks the token for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopLevel {
    /// [`CancelToken::stop_router`]: the routing passes stop, the later
    /// stages go on.
    Router,
    /// [`CancelToken::stop_all`].
    All,
}

#[derive(Debug)]
struct Armed {
    id: u64,
    at: Instant,
    level: StopLevel,
}

#[derive(Debug, Default)]
struct State {
    armed: Vec<Armed>,
    next_id: u64,
    finished: bool,
}

#[derive(Debug)]
struct Shared {
    state: Mutex<State>,
    wake: Condvar,
    token: Arc<CancelToken>,
    /// True once a deadline has fired.
    fired: AtomicBool,
}

impl Shared {
    fn fire(&self, level: StopLevel) {
        self.fired.store(true, Ordering::Release);
        match level {
            StopLevel::Router => self.token.stop_router(),
            StopLevel::All => self.token.stop_all(),
        }
    }
}

/// The handle of an armed deadline, for [`Watcher::disarm`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeadlineId(u64);

/// One thread that sleeps until the earliest armed deadline and then stops
/// the token. Dropping the watcher (or [`finish`](Self::finish)) wakes and
/// joins the thread.
#[derive(Debug)]
pub struct Watcher {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Watcher {
    /// A watcher with nothing armed.
    pub fn new(token: Arc<CancelToken>) -> Watcher {
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            wake: Condvar::new(),
            token,
            fired: AtomicBool::new(false),
        });
        let thread_shared = Arc::clone(&shared);
        let thread = std::thread::Builder::new()
            .name("deadline-watcher".to_string())
            .spawn(move || watch(&thread_shared))
            .ok();
        Watcher { shared, thread }
    }

    /// Arms a deadline `after` from now. A zero duration stops the token
    /// before this returns, so the outcome does not depend on thread timing.
    pub fn arm(&self, after: Duration, level: StopLevel) -> DeadlineId {
        let mut state = lock(&self.shared.state);
        let id = state.next_id;
        state.next_id += 1;
        if after.is_zero() {
            drop(state);
            self.shared.fire(level);
            return DeadlineId(id);
        }
        let at = Instant::now().checked_add(after).unwrap_or_else(far_future);
        state.armed.push(Armed { id, at, level });
        drop(state);
        self.shared.wake.notify_all();
        DeadlineId(id)
    }

    /// Cancels a deadline that has not fired yet (the stage ended in time).
    pub fn disarm(&self, id: DeadlineId) {
        lock(&self.shared.state).armed.retain(|a| a.id != id.0);
        self.shared.wake.notify_all();
    }

    /// True once any deadline has fired.
    pub fn fired(&self) -> bool {
        self.shared.fired.load(Ordering::Acquire)
    }

    /// Stops the thread and reports whether a deadline fired.
    pub fn finish(mut self) -> bool {
        self.stop_thread();
        self.fired()
    }

    fn stop_thread(&mut self) {
        lock(&self.shared.state).finished = true;
        self.shared.wake.notify_all();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.stop_thread();
    }
}

fn far_future() -> Instant {
    // About 30 years: later than any limit (the job limit is capped at 24 h).
    Instant::now() + Duration::from_secs(60 * 60 * 24 * 365 * 30)
}

fn lock(state: &Mutex<State>) -> std::sync::MutexGuard<'_, State> {
    state.lock().unwrap_or_else(|e| e.into_inner())
}

fn watch(shared: &Shared) {
    let mut state = lock(&shared.state);
    loop {
        if state.finished {
            return;
        }
        let now = Instant::now();
        // Fire everything that is due, earliest first.
        state.armed.sort_by_key(|a| (a.at, a.id));
        while let Some(first) = state.armed.first() {
            if first.at > now {
                break;
            }
            let level = state.armed.remove(0).level;
            shared.fire(level);
        }
        match state.armed.first().map(|a| a.at) {
            Some(at) => {
                let wait = at.saturating_duration_since(Instant::now());
                state = shared
                    .wake
                    .wait_timeout(state, wait)
                    .unwrap_or_else(|e| e.into_inner())
                    .0;
            }
            None => state = shared.wake.wait(state).unwrap_or_else(|e| e.into_inner()),
        }
    }
}

/// What the end of a run knows (the inputs of `assignTerminalState`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RunFacts {
    /// A user asked to cancel (`isCancelledByUser`).
    pub cancelled_by_user: bool,
    /// The job limit (`timeoutAt`) has passed.
    pub clock_expired: bool,
    /// A full stop was requested (`isStopRequested`).
    pub stop_requested: bool,
}

/// `RoutingJobSchedulerActionThread.assignTerminalState`: one terminal state
/// for a finished pipeline.
///
/// - A state that is not `Running` or `Stopping` is kept (a `Completed` job
///   is never downgraded, a timeout set by the watcher stays).
/// - A user cancel is `Cancelled`, even if the job limit has passed.
/// - A stop after the job limit is `TimedOut`; a stop before it is
///   `Completed`.
pub fn assign_terminal_state(state: JobState, facts: RunFacts) -> JobState {
    if !matches!(state, JobState::Running | JobState::Stopping) {
        return state;
    }
    if facts.cancelled_by_user {
        return JobState::Cancelled;
    }
    if facts.clock_expired && facts.stop_requested {
        return JobState::TimedOut;
    }
    JobState::Completed
}

/// `markTimedOutIfStillActive`: a job that is still running or stopping is
/// timed out; a job in a terminal state keeps it.
pub fn mark_timed_out_if_still_active(state: JobState) -> JobState {
    if matches!(state, JobState::Running | JobState::Stopping) {
        JobState::TimedOut
    } else {
        state
    }
}

/// The state of a run that ended with an engine error
/// (`threadAction`'s `catch (Throwable)`): `Terminated` while running.
pub fn terminate_on_error(state: JobState) -> JobState {
    if matches!(state, JobState::Running | JobState::Stopping) {
        JobState::Terminated
    } else {
        state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(cancelled_by_user: bool, clock_expired: bool, stop_requested: bool) -> RunFacts {
        RunFacts {
            cancelled_by_user,
            clock_expired,
            stop_requested,
        }
    }

    /// `RoutingJobSchedulerActionThreadTest`, one row per test method.
    #[test]
    fn terminal_state_mapping_table() {
        use JobState::*;
        // (java test method, state before, facts, state after)
        let table = [
            (
                "jobClockStopBecomesTimedOut",
                Running,
                facts(false, true, true),
                TimedOut,
            ),
            (
                "stopBeforeTheJobClockCompletes",
                Running,
                facts(false, false, true),
                Completed,
            ),
            (
                "userCancelWinsOverTheExpiredJobClock",
                Stopping,
                facts(true, true, true),
                Cancelled,
            ),
            // Not in Java's tests, but in its code: a clock that passed
            // without a stop request is not a timeout.
            (
                "clockExpiredButNoStop",
                Running,
                facts(false, true, false),
                Completed,
            ),
            (
                "completedIsNeverDowngraded",
                Completed,
                facts(false, true, true),
                Completed,
            ),
            (
                "terminatedIsKept",
                Terminated,
                facts(true, true, true),
                Terminated,
            ),
            (
                "timedOutIsKept",
                TimedOut,
                facts(true, false, false),
                TimedOut,
            ),
        ];
        for (name, before, facts, after) in table {
            assert_eq!(assign_terminal_state(before, facts), after, "{name}");
        }
    }

    /// `finishedJobStaysCompletedWhenTheMonitorAssignsTimeout`,
    /// `monitorTimeoutIsKeptWhenThePipelineFinishes` and
    /// `testFatalErrorSetsJobStateToTerminated`.
    #[test]
    fn monitor_and_error_paths() {
        use JobState::*;
        assert_eq!(mark_timed_out_if_still_active(Completed), Completed);
        let marked = mark_timed_out_if_still_active(Running);
        assert_eq!(marked, TimedOut);
        assert_eq!(
            assign_terminal_state(marked, facts(false, false, false)),
            TimedOut
        );
        assert_eq!(terminate_on_error(Running), Terminated);
        assert_eq!(terminate_on_error(Completed), Completed);
    }

    #[test]
    fn a_zero_deadline_stops_before_arm_returns() {
        let token = Arc::new(CancelToken::new());
        let watcher = Watcher::new(Arc::clone(&token));
        watcher.arm(Duration::ZERO, StopLevel::All);
        assert!(token.is_stop_requested());
        assert!(watcher.fired());
    }

    #[test]
    fn the_earliest_deadline_fires_and_a_far_one_does_not() {
        let token = Arc::new(CancelToken::new());
        let watcher = Watcher::new(Arc::clone(&token));
        watcher.arm(Duration::from_secs(3600), StopLevel::All);
        watcher.arm(Duration::from_millis(30), StopLevel::Router);
        let start = Instant::now();
        while !token.is_router_stop_requested() && start.elapsed() < Duration::from_secs(10) {
            std::thread::yield_now();
        }
        assert!(token.is_router_stop_requested());
        assert!(!token.is_stop_requested(), "only the router level fired");
        assert!(watcher.finish());
    }

    #[test]
    fn a_disarmed_deadline_never_fires() {
        let token = Arc::new(CancelToken::new());
        let watcher = Watcher::new(Arc::clone(&token));
        let id = watcher.arm(Duration::from_millis(40), StopLevel::All);
        watcher.disarm(id);
        std::thread::sleep(Duration::from_millis(150));
        assert!(!token.is_router_stop_requested());
        assert!(!watcher.finish());
    }

    #[test]
    fn dropping_the_watcher_joins_its_thread() {
        let token = Arc::new(CancelToken::new());
        {
            let watcher = Watcher::new(Arc::clone(&token));
            watcher.arm(Duration::from_secs(3600), StopLevel::All);
        }
        assert!(!token.is_router_stop_requested());
    }
}
