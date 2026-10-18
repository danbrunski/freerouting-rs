//! Q08: the earned budget extension, a connection router that lets a maze
//! search run past its work limit while it is still closing in on its
//! target.
//!
//! Java's per-connection maze search stops at a work limit counted in
//! search-tree nodes (`WorkLimit`, doubled per pass). A search that is still
//! approaching its destination when the limit fires is cut off like one
//! that is lost. KRT's rule (#529, `GridSearch::extend_budget` at
//! `0486e799`) grants one more base budget when the closest approach
//! improved enough during the last one; this module ports that rule onto the
//! Freerouting maze search through the session's budget hook
//! ([`autoroute::session::budget`](crate::autoroute::session::budget)).
//!
//! **The rule** ([`EarnedBudgetPolicy`]). The progress signal is the
//! smallest destination-distance estimate of any element the search popped
//! (Java's `sortingValue - expansionValue`: the lower bound
//! `DestinationDistance` adds to the cost so far; board units weighted by
//! the cheapest trace cost, so board units under the default costs of 1),
//! truncated to an integer. When the limit fires the policy extends it by
//! one base budget if the estimate fell during the budget just spent by at
//! least `max(2 * trace width, estimate_at_start / 50)` (and by more than
//! 0), where `estimate_at_start` is the best estimate when that budget
//! began (for the first budget: the estimate of the first element popped).
//! At most [`DEFAULT_MAX_EXTENSIONS`] extensions per maze search; a search
//! that has reached a zero estimate, popped nothing, or failed the rule once
//! is never extended again. Integer arithmetic only.
//!
//! **The router** ([`EarnedBudget`], id [`ROUTER_ID`], selected by
//! `router.autorouter.connection_router`) runs Java's connection router
//! ([`FreeroutingConnectionRouter::route_with_budget`]) with the policy on
//! the connection's search and on its plane fallback's, each starting the
//! policy anew; the neck-down retry's search keeps Java's stop. The
//! outcome's `work_limit_hit` still says whether the base budget was
//! exceeded. The extensions granted and their units are counted by
//! [`run_stats`](crate::run_stats) (`maze_budget_extensions`,
//! `maze_budget_extra_units`) and by [`EarnedBudget::tally`].
//!
//! **Determinism.** The decision reads only search state and work counts,
//! never time (D2, D5), so the same input and settings give the same
//! session.
//!
//! Not Java: unset by default and in no profile (`docs/DIVERGENCES.md`);
//! Q08.3 measures it under N02's totals rule
//! (`docs/roadmap/prereg/Q08.json`).

use crate::autoroute::batch::connection::FreeroutingConnectionRouter;
use crate::autoroute::session::budget::{BudgetPolicy, BudgetProgress, BudgetStart, LimitDecision};
use crate::autoroute::stages::{
    ConnectionOutcome, ConnectionParams, ConnectionRouter, RouteTask, StageCtx,
};
use crate::board::RoutingBoard;
use crate::settings::RouterSettings;

/// The `router.autorouter.connection_router` id.
pub const ROUTER_ID: &str = "earned-budget";

/// E, the extensions one maze search may earn (preregistered in
/// `docs/roadmap/prereg/Q08.json`).
pub const DEFAULT_MAX_EXTENSIONS: u32 = 4;

/// The share of the estimate at the start of a budget the estimate must
/// fall by, as a divisor (2 %).
const PROGRESS_DIVISOR: i64 = 50;

/// What the policy did over a run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BudgetTally {
    /// Maze searches the policy was set on.
    pub searches: u64,
    /// Times a search's limit fired and the policy was asked.
    pub limits_fired: u64,
    /// Extensions granted.
    pub extensions: u64,
    /// Work units those extensions granted.
    pub extra_units: u64,
}

/// The earned-budget rule (see the module docs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EarnedBudgetPolicy {
    max_extensions: u32,
    base_units: u64,
    trace_width: i64,
    estimate_at_start: Option<i64>,
    extensions: u32,
    stopped: bool,
    tally: BudgetTally,
}

impl Default for EarnedBudgetPolicy {
    fn default() -> Self {
        EarnedBudgetPolicy::new(DEFAULT_MAX_EXTENSIONS)
    }
}

/// An estimate as an integer: truncated toward zero, saturating, NaN as 0.
fn estimate_units(estimate: f64) -> i64 {
    estimate as i64
}

impl EarnedBudgetPolicy {
    /// A policy granting at most `max_extensions` per search.
    pub fn new(max_extensions: u32) -> Self {
        EarnedBudgetPolicy {
            max_extensions,
            base_units: 0,
            trace_width: 0,
            estimate_at_start: None,
            extensions: 0,
            stopped: false,
            tally: BudgetTally::default(),
        }
    }

    /// The extensions granted to the current search.
    pub fn extensions(&self) -> u32 {
        self.extensions
    }

    /// The tally over every search so far.
    pub fn tally(&self) -> BudgetTally {
        self.tally
    }

    /// The fall of the estimate a budget that began at `estimate_at_start`
    /// must show: `max(2 * trace width, estimate_at_start / 50)`.
    pub fn quantum(&self, estimate_at_start: i64) -> i64 {
        self.trace_width
            .saturating_mul(2)
            .max(estimate_at_start / PROGRESS_DIVISOR)
    }

    /// The decision for `progress`, without the latch and the tally.
    fn decide(&self, progress: &BudgetProgress) -> LimitDecision {
        if self.base_units == 0 || self.extensions >= self.max_extensions {
            return LimitDecision::Stop;
        }
        let Some(best) = progress.best_estimate.map(estimate_units) else {
            return LimitDecision::Stop;
        };
        if best <= 0 {
            // The target is reached or as good as: no more work is needed.
            return LimitDecision::Stop;
        }
        let Some(start) = self
            .estimate_at_start
            .or(progress.first_estimate.map(estimate_units))
        else {
            return LimitDecision::Stop;
        };
        let fell = start.saturating_sub(best);
        if fell > 0 && fell >= self.quantum(start) {
            LimitDecision::Extend(self.base_units)
        } else {
            LimitDecision::Stop
        }
    }
}

impl BudgetPolicy for EarnedBudgetPolicy {
    fn on_start(&mut self, start: BudgetStart) {
        self.base_units = start.base_units;
        self.trace_width = start.trace_width;
        self.estimate_at_start = None;
        self.extensions = 0;
        self.stopped = false;
        self.tally.searches += 1;
    }

    fn on_limit(&mut self, progress: BudgetProgress) -> LimitDecision {
        self.tally.limits_fired += 1;

        if self.stopped {
            return LimitDecision::Stop;
        }
        let decision = self.decide(&progress);
        match decision {
            LimitDecision::Extend(units) => {
                self.extensions += 1;
                self.estimate_at_start = progress.best_estimate.map(estimate_units);
                self.tally.extensions += 1;
                self.tally.extra_units = self.tally.extra_units.saturating_add(units);
            }
            LimitDecision::Stop => self.stopped = true,
        }
        decision
    }
}

/// Java's connection router with the earned-budget policy (id
/// [`ROUTER_ID`]).
pub struct EarnedBudget {
    inner: FreeroutingConnectionRouter,
    policy: EarnedBudgetPolicy,
}

impl std::fmt::Debug for EarnedBudget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EarnedBudget")
            .field("inner", &self.inner)
            .field("policy", &self.policy)
            .finish()
    }
}

impl EarnedBudget {
    /// `inner` with a policy granting at most `max_extensions` per search.
    pub fn new(inner: FreeroutingConnectionRouter, max_extensions: u32) -> Self {
        EarnedBudget {
            inner,
            policy: EarnedBudgetPolicy::new(max_extensions),
        }
    }

    /// Java's connection router with the policies `settings` select, and
    /// [`DEFAULT_MAX_EXTENSIONS`].
    pub fn from_settings(settings: &RouterSettings) -> Self {
        EarnedBudget::new(
            FreeroutingConnectionRouter::from_settings(settings),
            DEFAULT_MAX_EXTENSIONS,
        )
    }

    /// What the policy did so far.
    pub fn tally(&self) -> BudgetTally {
        self.policy.tally()
    }
}

impl ConnectionRouter for EarnedBudget {
    fn id(&self) -> &'static str {
        ROUTER_ID
    }

    fn route(
        &mut self,
        board: &mut RoutingBoard,
        task: &RouteTask,
        p: &ConnectionParams,
        ctx: &mut StageCtx,
    ) -> ConnectionOutcome {
        self.inner
            .route_with_budget(board, task, p, ctx, Some(&mut self.policy))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: u64 = 64_000;

    /// A policy started on a search with `trace_width`.
    fn started(trace_width: i64) -> EarnedBudgetPolicy {
        let mut policy = EarnedBudgetPolicy::default();
        policy.on_start(BudgetStart {
            base_units: BASE,
            trace_width,
        });
        policy
    }

    fn progress(first: f64, best: f64, budgets: u64) -> BudgetProgress {
        BudgetProgress {
            first_estimate: Some(first),
            best_estimate: Some(best),
            units_done: budgets * BASE + 1,
            max_units: budgets * BASE,
        }
    }

    /// The decisions for a search whose best estimate after each budget is
    /// `bests`, starting from `first`.
    fn decisions(policy: &mut EarnedBudgetPolicy, first: f64, bests: &[f64]) -> Vec<LimitDecision> {
        bests
            .iter()
            .enumerate()
            .map(|(i, &best)| policy.on_limit(progress(first, best, i as u64 + 1)))
            .collect()
    }

    #[test]
    fn steady_progress_extends_up_to_e_times() {
        let mut policy = started(250);
        // Each budget closes in by 10 000 units, far above the quantum
        // max(500, start / 50).
        let bests = [90_000.0, 80_000.0, 70_000.0, 60_000.0, 50_000.0, 40_000.0];
        let got = decisions(&mut policy, 100_000.0, &bests);
        let extend = LimitDecision::Extend(BASE);
        assert_eq!(
            got,
            [
                extend,
                extend,
                extend,
                extend,
                LimitDecision::Stop,
                LimitDecision::Stop
            ]
        );
        assert_eq!(policy.extensions(), DEFAULT_MAX_EXTENSIONS);
        let tally = policy.tally();
        assert_eq!(tally.extensions, 4);
        assert_eq!(tally.extra_units, 4 * BASE);
        assert_eq!(tally.limits_fired, 6);
    }

    #[test]
    fn progress_below_the_quantum_stops() {
        // The quantum is max(2 * 250, 100 000 / 50) = 2 000.
        let policy = started(250);
        assert_eq!(policy.quantum(100_000), 2_000);
        let mut policy = started(250);
        assert_eq!(
            policy.on_limit(progress(100_000.0, 98_001.0, 1)),
            LimitDecision::Stop
        );
        // Exactly the quantum extends.
        let mut policy = started(250);
        assert_eq!(
            policy.on_limit(progress(100_000.0, 98_000.0, 1)),
            LimitDecision::Extend(BASE)
        );
        // A small estimate: the trace width term rules (2 * 2 500).
        let mut policy = started(2_500);
        assert_eq!(policy.quantum(10_000), 5_000);
        assert_eq!(
            policy.on_limit(progress(10_000.0, 5_001.0, 1)),
            LimitDecision::Stop
        );
    }

    #[test]
    fn the_quantum_is_measured_from_the_start_of_each_budget() {
        let mut policy = started(250);
        // Budget 1: 100 000 -> 90 000 extends; budget 2 starts at 90 000
        // (quantum 1 800) and gains only 1 000.
        let got = decisions(&mut policy, 100_000.0, &[90_000.0, 89_000.0]);
        assert_eq!(got, [LimitDecision::Extend(BASE), LimitDecision::Stop]);
    }

    #[test]
    fn a_zero_estimate_never_asks_for_more() {
        let mut policy = started(250);
        assert_eq!(
            policy.on_limit(progress(100_000.0, 0.0, 1)),
            LimitDecision::Stop
        );
        // A fraction below one board unit truncates to 0 too.
        let mut policy = started(250);
        assert_eq!(
            policy.on_limit(progress(100_000.0, 0.5, 1)),
            LimitDecision::Stop
        );
    }

    #[test]
    fn no_progress_or_no_pop_stops() {
        let mut policy = started(0);
        // No fall with a zero quantum (trace width 0, estimate < 50).
        assert_eq!(
            policy.on_limit(progress(40.0, 40.0, 1)),
            LimitDecision::Stop
        );
        let mut policy = started(250);
        let none = BudgetProgress {
            first_estimate: None,
            best_estimate: None,
            units_done: BASE + 1,
            max_units: BASE,
        };
        assert_eq!(policy.on_limit(none), LimitDecision::Stop);
    }

    #[test]
    fn a_stop_latches_until_the_next_search() {
        let mut policy = started(250);
        assert_eq!(
            policy.on_limit(progress(100_000.0, 99_999.0, 1)),
            LimitDecision::Stop
        );
        // Even a later large fall is not extended within the same search.
        assert_eq!(
            policy.on_limit(progress(100_000.0, 10_000.0, 1)),
            LimitDecision::Stop
        );
        policy.on_start(BudgetStart {
            base_units: BASE,
            trace_width: 250,
        });
        assert_eq!(
            policy.on_limit(progress(100_000.0, 10_000.0, 1)),
            LimitDecision::Extend(BASE)
        );
        assert_eq!(policy.tally().searches, 2);
    }

    #[test]
    fn a_search_without_a_limit_is_never_extended() {
        let mut policy = EarnedBudgetPolicy::default();
        policy.on_start(BudgetStart {
            base_units: 0,
            trace_width: 250,
        });
        assert_eq!(
            policy.on_limit(progress(100_000.0, 1.0, 1)),
            LimitDecision::Stop
        );
    }
}
