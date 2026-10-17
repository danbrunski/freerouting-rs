//! Candidates evaluated ahead of the ordered merge (P03.2).
//!
//! O06's pass evaluated one chunk of candidates at a time and merged it
//! once its slowest candidate finished, so every chunk ended with threads
//! idle. Here `threads` workers take the candidates in candidate order
//! without that barrier, at most `window` candidates past the last one the
//! merge consumed, and the merge takes each outcome as soon as it is there.
//!
//! The merge itself is unchanged ([`super::worker::evaluate_pass`]): it
//! still walks the candidates chunk by chunk with Java's rules, so a stop
//! between chunks, a candidate's own stop request and the failure limit
//! end the pass where they did. A candidate's outcome depends only on the
//! pass baseline, the candidate and the stop level its token starts at, so
//! running it earlier, on another thread or beside other candidates changes
//! nothing; outcomes past the point where the merge stops are dropped, as a
//! chunk's were, and so are their run statistics (`--stats json`): a
//! candidate's counts are added when the merge takes it, so the totals
//! don't depend on the thread count. The window only bounds that speculative work and the boards
//! of improved candidates waiting to be merged.

use std::sync::{Condvar, Mutex, MutexGuard};
use std::thread::Scope;

use super::worker::{CandidateOutcome, PassSetup, STOP_RELAY_INTERVAL, evaluate_with_token, relay};
use crate::autoroute::events::CancelToken;
use crate::board::BoardSnapshot;
use crate::board::items::ItemId;
use crate::run_stats::Counts;

/// The candidates of one pass and their outcomes as the workers finish
/// them.
pub(super) struct Lookahead<'a> {
    snapshot: &'a BoardSnapshot,
    items: &'a [ItemId],
    setup: &'a PassSetup<'a>,
    /// One token per candidate, raised to the job's stop level when the
    /// candidate starts and while it runs.
    tokens: Vec<CancelToken>,
    window: usize,
    state: Mutex<State>,
    /// A worker stored an outcome.
    ready: Condvar,
    /// The merge consumed an outcome or the pass ended.
    room: Condvar,
}

struct State {
    /// The next candidate a worker takes.
    next: usize,
    /// The candidates the merge has consumed (or skipped).
    merged: usize,
    /// The pass ended: no candidate starts any more.
    ended: bool,
    /// A candidate panicked; the merge panics too.
    panicked: bool,
    /// Each finished candidate's outcome and the run statistics it held
    /// back ([`crate::run_stats::capture`]), added when the merge takes it.
    outcomes: Vec<Option<(CandidateOutcome, Counts)>>,
}

impl<'a> Lookahead<'a> {
    /// The candidates `items` of a pass on `snapshot`; at most `window`
    /// (at least 1) run past the merge.
    pub(super) fn new(
        snapshot: &'a BoardSnapshot,
        items: &'a [ItemId],
        setup: &'a PassSetup<'a>,
        window: usize,
    ) -> Self {
        Lookahead {
            snapshot,
            items,
            setup,
            tokens: items.iter().map(|_| CancelToken::new()).collect(),
            window: window.max(1),
            state: Mutex::new(State {
                next: 0,
                merged: 0,
                ended: false,
                panicked: false,
                outcomes: items.iter().map(|_| None).collect(),
            }),
            ready: Condvar::new(),
            room: Condvar::new(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Starts `threads` workers (at least 1) in `scope`.
    pub(super) fn spawn<'scope>(&'scope self, scope: &'scope Scope<'scope, '_>, threads: usize) {
        for _ in 0..threads.clamp(1, self.items.len().max(1)) {
            scope.spawn(|| {
                // P03: a candidate's analyses run inline on its thread.
                let _budget = crate::parallel::ThreadBudget::set(1);
                while let Some(index) = self.claim() {
                    let token = &self.tokens[index];
                    // The candidate starts at the job's current stop level,
                    // as a token made now would.
                    relay(self.setup.cancel, token);
                    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        crate::run_stats::capture(|| {
                            evaluate_with_token(self.snapshot, self.items[index], self.setup, token)
                        })
                    }));
                    match outcome {
                        Ok(outcome) => self.lock().outcomes[index] = Some(outcome),
                        Err(panic) => {
                            self.lock().panicked = true;
                            self.ready.notify_all();
                            std::panic::resume_unwind(panic);
                        }
                    }
                    self.ready.notify_all();
                }
            });
        }
    }

    /// The next candidate to evaluate, once it is inside the window; `None`
    /// when there is none or the pass ended.
    fn claim(&self) -> Option<usize> {
        let mut state = self.lock();
        loop {
            if state.ended || state.next >= self.items.len() {
                return None;
            }
            if state.next < state.merged + self.window {
                state.next += 1;
                return Some(state.next - 1);
            }
            state = self.room.wait(state).unwrap_or_else(|e| e.into_inner());
        }
    }

    /// The outcome of candidate `index`, the next one the merge consumes:
    /// waits for it, relaying the job's stop requests to the candidates
    /// that run meanwhile.
    pub(super) fn take(&self, index: usize) -> CandidateOutcome {
        let mut state = self.lock();
        loop {
            if let Some((outcome, counts)) = state.outcomes[index].take() {
                state.merged = index + 1;
                drop(state);
                self.room.notify_all();
                crate::run_stats::add_counts(&counts);
                return outcome;
            }
            assert!(!state.panicked, "an optimizer candidate panicked");
            state = self
                .ready
                .wait_timeout(state, STOP_RELAY_INTERVAL)
                .unwrap_or_else(|e| e.into_inner())
                .0;
            for token in &self.tokens[state.merged..state.next] {
                relay(self.setup.cancel, token);
            }
        }
    }

    /// Ends the pass: no further candidate starts, and the running ones
    /// see the job's stop level.
    pub(super) fn end(&self) {
        let mut state = self.lock();
        state.ended = true;
        for token in &self.tokens[state.merged.min(state.next)..state.next] {
            relay(self.setup.cancel, token);
        }
        drop(state);
        self.room.notify_all();
        self.ready.notify_all();
    }
}
