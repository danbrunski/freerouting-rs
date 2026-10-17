//! The engine's analysis threads (P03): ordered parallel maps on scoped
//! threads.
//!
//! Work whose result can't depend on order runs here: the design rules
//! check, the board statistics of every pass (their clearance check, the
//! fanout counts and the routing bounds) and the airlines. The router
//! itself stays sequential (D5).
//!
//! - **Ordered merge.** [`map`] and [`map_with`] return their results in
//!   input order, whichever thread computed them, and a caller merges them
//!   in that order: an `f64` sum is summed sequentially afterwards, never
//!   per thread. So no result depends on the thread count.
//! - **One budget.** A thread runs at most [`threads`] workers at once:
//!   the budget set on it by [`ThreadBudget::set`] (a job sets it from
//!   `autorouter.max_threads`, which `--threads` sets), otherwise the
//!   machine's cores. A worker's own budget is 1, so a parallel analysis
//!   inside another one (or inside an optimizer candidate) runs inline
//!   and the threads never multiply.
//! - **No pool object, no new dependency.** The workers are
//!   `std::thread::scope` threads started per call, as O06's optimizer
//!   does; starting them costs tens of microseconds, against the
//!   milliseconds to seconds of the analyses that use them. Inputs are
//!   handed out by an atomic counter, so a slow item doesn't hold up the
//!   others.

use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};

thread_local! {
    static BUDGET: Cell<Option<usize>> = const { Cell::new(None) };
}

/// The machine's cores (`available_parallelism`, 1 if unknown).
pub fn available_cores() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get())
}

/// The workers an analysis on this thread may use: the budget set by
/// [`ThreadBudget::set`], else [`available_cores`]. Always at least 1.
pub fn threads() -> usize {
    BUDGET
        .with(Cell::get)
        .unwrap_or_else(available_cores)
        .max(1)
}

/// Sets this thread's budget until it is dropped, then restores the
/// previous one.
#[must_use = "the budget is restored when the guard is dropped"]
pub struct ThreadBudget {
    previous: Option<usize>,
}

impl ThreadBudget {
    /// Limits the analyses started on this thread to `threads` workers (at
    /// least 1) while the guard lives.
    pub fn set(threads: usize) -> ThreadBudget {
        let previous = BUDGET.with(|b| b.replace(Some(threads.max(1))));
        ThreadBudget { previous }
    }
}

impl Drop for ThreadBudget {
    fn drop(&mut self) {
        BUDGET.with(|b| b.set(self.previous));
    }
}

/// `inputs.iter().map(f)` on up to [`threads`] workers, in input order.
pub fn map<T: Sync, R: Send>(inputs: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    map_with(inputs, || (), |(), input| f(input)).0
}

/// [`map`] where each worker owns a state made by `init` (a cache of pure
/// values, which must not change a result). Returns the results in input
/// order and the workers' final states, in no particular order.
pub fn map_with<T: Sync, S: Send, R: Send>(
    inputs: &[T],
    init: impl Fn() -> S + Sync,
    f: impl Fn(&mut S, &T) -> R + Sync,
) -> (Vec<R>, Vec<S>) {
    let workers = threads().min(inputs.len());
    if workers <= 1 {
        let mut state = init();
        let results = inputs.iter().map(|input| f(&mut state, input)).collect();
        return (results, vec![state]);
    }
    let next = AtomicUsize::new(0);
    let done: Vec<(Vec<(usize, R)>, S)> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let _budget = ThreadBudget::set(1);
                    let mut state = init();
                    let mut results = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(input) = inputs.get(index) else {
                            return (results, state);
                        };
                        results.push((index, f(&mut state, input)));
                    }
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e)))
            .collect()
    });
    let mut indexed = Vec::with_capacity(inputs.len());
    let mut states = Vec::with_capacity(done.len());
    for (results, state) in done {
        indexed.extend(results);
        states.push(state);
    }
    indexed.sort_unstable_by_key(|(index, _)| *index);
    (indexed.into_iter().map(|(_, r)| r).collect(), states)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_are_in_input_order_for_every_budget() {
        let inputs: Vec<u64> = (0..1000).collect();
        let expected: Vec<u64> = inputs.iter().map(|x| x * x + 1).collect();
        for budget in [1, 2, 3, 8, 64] {
            let _b = ThreadBudget::set(budget);
            assert_eq!(map(&inputs, |x| x * x + 1), expected, "budget {budget}");
        }
    }

    #[test]
    fn workers_run_nested_maps_inline() {
        let _b = ThreadBudget::set(4);
        let nested = map(&[0u8; 16], |_| threads());
        assert!(nested.iter().all(|&t| t == 1), "{nested:?}");
        assert_eq!(threads(), 4);
    }

    #[test]
    fn a_budget_is_restored_when_dropped() {
        let outer = threads();
        {
            let _b = ThreadBudget::set(3);
            assert_eq!(threads(), 3);
            {
                let _c = ThreadBudget::set(0);
                assert_eq!(threads(), 1, "a budget is at least 1");
            }
            assert_eq!(threads(), 3);
        }
        assert_eq!(threads(), outer);
    }

    #[test]
    fn worker_states_cover_every_input() {
        let inputs: Vec<usize> = (0..257).collect();
        for budget in [1, 5] {
            let _b = ThreadBudget::set(budget);
            let (results, states) = map_with(&inputs, Vec::new, |seen: &mut Vec<usize>, x| {
                seen.push(*x);
                *x
            });
            assert_eq!(results, inputs);
            assert!(states.len() <= budget);
            let mut all: Vec<usize> = states.into_iter().flatten().collect();
            all.sort_unstable();
            assert_eq!(all, inputs);
        }
    }

    #[test]
    fn empty_and_single_inputs_run_inline() {
        let _b = ThreadBudget::set(8);
        let (none, states) = map_with(&[] as &[u8], || 0u8, |_, x| *x);
        assert!(none.is_empty());
        assert_eq!(states.len(), 1);
        assert_eq!(map(&[7u8], |x| *x), vec![7]);
    }
}
