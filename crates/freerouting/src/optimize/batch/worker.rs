//! One optimizer pass: every candidate evaluated on its own copy of the
//! pass baseline, in parallel, merged in candidate order (O06.4).
//!
//! Port of `BatchOptimizer.optRoutePass`'s candidate loop and
//! `OptimizeCandidateTask` (`BatchOptimizer.java:670-810`, `:1020-1122` at
//! `a773a70ec`), with `WorkerBoardState` replaced by a fresh copy per
//! candidate.
//!
//! **Copies.** The pass takes one [`BoardSnapshot`] of the baseline. Each
//! candidate starts from `snapshot.restore()` followed by
//! [`RoutingBoard::clear_transient_autoroute_state`], which is
//! [`RoutingBoard::deep_copy`]: trees rebuilt in item-list order, transient
//! state reset, the work meter at zero. Then, as Java's worker does, it
//! opens an undo level (`generateSnapshot`) and reroutes; an improved
//! candidate merges that level back (`popSnapshot`) and keeps its board.
//! Java instead reuses one board per thread and undoes it after each
//! candidate, which leaks the id generator, the trace half-width bounds,
//! the normalize-suppressed nets and the routing ledger from one candidate
//! into the next on that thread, so Java's result depends on scheduling.
//! The port's result never does (DIVERGENCES, "Determinism").
//!
//! **Ordered merge.** `threads` scoped threads evaluate the candidates in
//! candidate order, up to a window ahead of the merge
//! ([`lookahead_window`]: a chunk, at least 16 per thread; P03.2's
//! lookahead, which replaced a barrier at the end of every chunk). The
//! merge walks them in chunks (`max(4 × threads, 8)` by default) and
//! consumes the results in candidate order with Java's single-thread
//! rules: every consumed result
//! counts towards `max_items` and goes into the result map; an improved
//! one resets the failure counter and replaces the winner when it is
//! strictly better ([`ItemRouteResult::improved_over`]); any other counts a
//! failure, and the pass stops at the failure limit. The rest of that chunk
//! is discarded. A full stop requested before a chunk ends the pass without
//! a winner. Neither the chunk size nor the thread count changes a result.
//!
//! **Stop requests from inside a candidate.** Java's candidates share the
//! job's `StoppableThread`; the inner autorouter requests a full stop when
//! it reaches `autorouter.max_items` (`AutoroutePassRunner.java:113`), and
//! every task that starts after that returns an unimproved result at once.
//! Which tasks those are depends on scheduling in Java. Here each candidate
//! routes under its own [`CancelToken`], which starts at the job token's
//! level and receives the job's stop requests while it runs (the thread
//! that waits for the chunk relays them). A stop the candidate requested
//! itself reaches the job token when its result is consumed, in candidate
//! order, and every later candidate of the chunk is consumed as Java's
//! single-thread run sees it: unimproved, without a position.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use freerouting_geometry::FloatPoint;

use super::candidates::item_position;
use super::item::{ItemPassParams, opt_route_item};
use super::lookahead::Lookahead;
use crate::autoroute::events::{CancelToken, NullEvents};
use crate::autoroute::item_route_result::ItemRouteResult;
use crate::autoroute::stages::StageCtx;
use crate::board::BoardSnapshot;
use crate::board::items::ItemId;
use crate::board::routing_board::RoutingBoard;
use crate::settings::RouterSettings;
use crate::settings::board::BoardRouterSettings;

/// What a pass's candidates share.
pub struct PassSetup<'a> {
    /// The resolved settings.
    pub settings: &'a RouterSettings,
    /// The settings applied to the board.
    pub board_settings: &'a BoardRouterSettings,
    /// The job's stop request.
    pub cancel: &'a CancelToken,
    /// The pass's switches.
    pub params: ItemPassParams,
    /// Worker threads (at least 1).
    pub threads: usize,
    /// Candidates per chunk (at least 1).
    pub chunk_size: usize,
    /// The failure limit of this pass (12 in pass 1, 50 later by default).
    pub max_consecutive_failures: i32,
}

/// One evaluated candidate.
#[derive(Debug)]
pub struct CandidateOutcome {
    /// Java's `ItemRouteResult`.
    pub result: ItemRouteResult,
    /// The rerouted board, kept only when the result is improved.
    pub board: Option<RoutingBoard>,
    /// The candidate's position (via centre or trace first corner), if it
    /// was on the board.
    pub position: Option<FloatPoint>,
    /// The work the candidate did: search-tree nodes visited on its own
    /// board's meter (B02.7), 0 when it did not run.
    pub work: u64,
    /// True if the candidate itself requested a full stop (the inner
    /// autorouter's `max_items`), not the job.
    pub requested_stop: bool,
}

impl CandidateOutcome {
    /// Java's early-out result: unimproved, no board, no position.
    pub fn skipped(item: ItemId) -> CandidateOutcome {
        CandidateOutcome {
            result: ItemRouteResult::unimproved(item),
            board: None,
            position: None,
            work: 0,
            requested_stop: false,
        }
    }
}

/// How often the thread that waits for a chunk relays the job's stop
/// requests to the running candidates. Only the latency of a stop depends
/// on it, never a result.
pub(super) const STOP_RELAY_INTERVAL: Duration = Duration::from_millis(20);

/// A candidate's own token at the job token's current level.
fn local_token(job: &CancelToken) -> CancelToken {
    let token = CancelToken::new();
    relay(job, &token);
    token
}

/// Raises `local` to `job`'s level.
pub(super) fn relay(job: &CancelToken, local: &CancelToken) {
    if job.is_stop_requested() {
        local.stop_all();
    } else if job.is_router_stop_requested() {
        local.stop_router();
    }
}

/// The default chunk size for `threads` (Java `Math.max(threads * 4, 8)`).
pub fn default_chunk_size(threads: usize) -> usize {
    (threads.max(1) * 4).max(8)
}

/// The board a candidate starts from: `snapshot.restore()` plus the
/// transient reset, which is `baseline.deep_copy()` (see the module docs).
pub fn candidate_start_board(snapshot: &BoardSnapshot) -> RoutingBoard {
    let mut board = snapshot.restore();
    board.clear_transient_autoroute_state();
    board
}

/// Evaluates candidate `item` on a fresh copy of the pass baseline
/// (Java `OptimizeCandidateTask.call`).
pub fn evaluate_candidate(
    snapshot: &BoardSnapshot,
    item: ItemId,
    setup: &PassSetup<'_>,
) -> CandidateOutcome {
    evaluate_with_token(snapshot, item, setup, &local_token(setup.cancel))
}

/// [`evaluate_candidate`] routing under `token`, the candidate's own.
pub(super) fn evaluate_with_token(
    snapshot: &BoardSnapshot,
    item: ItemId,
    setup: &PassSetup<'_>,
    token: &CancelToken,
) -> CandidateOutcome {
    if setup.cancel.is_stop_requested() || token.is_stop_requested() {
        return CandidateOutcome::skipped(item);
    }
    let mut board = candidate_start_board(snapshot);
    board.generate_snapshot();
    let Some(position) = board.item(item).map(item_position) else {
        return CandidateOutcome::skipped(item);
    };
    let events = NullEvents;
    let mut ctx = StageCtx {
        settings: setup.settings,
        board_settings: setup.board_settings,
        cancel: token,
        events: &events,
    };
    let result = opt_route_item(&mut board, item, &setup.params, setup.settings, &mut ctx);
    let work = board.search_trees().work_meter().units();
    let board = if result.improved() {
        // PARITY: WorkerBoardState.finish(true): `popSnapshot` then
        // `clearTransientAutorouteState`.
        board.pop_snapshot();
        board.clear_transient_autoroute_state();
        Some(board)
    } else {
        None
    };
    CandidateOutcome {
        result,
        board,
        position,
        work,
        requested_stop: token.is_stop_requested() && !setup.cancel.is_stop_requested(),
    }
}

/// Evaluates `items` on `threads` threads; the outcomes come back in the
/// order of `items`, whichever thread ran them. The calling thread relays
/// the job's stop requests to the running candidates.
pub fn evaluate_chunk(
    snapshot: &BoardSnapshot,
    items: &[ItemId],
    setup: &PassSetup<'_>,
) -> Vec<CandidateOutcome> {
    let threads = setup.threads.clamp(1, items.len().max(1));
    let tokens: Vec<CancelToken> = items.iter().map(|_| local_token(setup.cancel)).collect();
    let next = AtomicUsize::new(0);
    let slots: Vec<Mutex<Option<CandidateOutcome>>> =
        items.iter().map(|_| Mutex::new(None)).collect();
    let (done_tx, done_rx) = mpsc::channel::<()>();
    std::thread::scope(|scope| {
        for _ in 0..threads {
            let done = done_tx.clone();
            let (next, slots, tokens) = (&next, &slots, &tokens);
            scope.spawn(move || {
                // P03: a candidate's analyses run inline on its thread.
                let _budget = crate::parallel::ThreadBudget::set(1);
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(index) else {
                        break;
                    };
                    let outcome = evaluate_with_token(snapshot, *item, setup, &tokens[index]);
                    *slots[index].lock().unwrap_or_else(|e| e.into_inner()) = Some(outcome);
                    let _ = done.send(());
                }
            });
        }
        drop(done_tx);
        let mut finished = 0;
        while finished < items.len() {
            match done_rx.recv_timeout(STOP_RELAY_INTERVAL) {
                Ok(()) => finished += 1,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            for token in &tokens {
                relay(setup.cancel, token);
            }
        }
    });
    slots
        .into_iter()
        .zip(items)
        .map(|(slot, item)| {
            slot.into_inner()
                .unwrap_or_else(|e| e.into_inner())
                .unwrap_or_else(|| CandidateOutcome::skipped(*item))
        })
        .collect()
}

/// What a pass's candidate loop produced.
#[derive(Debug, Default)]
pub struct PassEvaluation {
    /// The best improved candidate and its board, unless the pass was
    /// stopped.
    pub winner: Option<(ItemRouteResult, RoutingBoard)>,
    /// The adopted winner's result, once the caller has taken the board.
    pub winner_result: Option<ItemRouteResult>,
    /// Every consumed result, in candidate order (the per-candidate log).
    pub consumed: Vec<ItemRouteResult>,
    /// The work of every consumed candidate, in the same order.
    pub consumed_work: Vec<u64>,
    /// The last consumed position (Java `currentPosition`).
    pub position: Option<FloatPoint>,
    /// True if a full stop ended the pass before a chunk.
    pub stopped: bool,
    /// True if the failure limit ended the pass.
    pub early_stop: bool,
    /// The consecutive failures when the pass ended.
    pub consecutive_failures: i32,
}

/// Java `optRoutePass`'s candidate loop over `items` (see the module docs).
/// Adds every consumed result to `results` and counts it in
/// `total_items`.
pub fn evaluate_pass(
    baseline: &RoutingBoard,
    items: &[ItemId],
    setup: &PassSetup<'_>,
    results: &mut BTreeMap<ItemId, ItemRouteResult>,
    total_items: &mut i32,
) -> PassEvaluation {
    let snapshot = baseline.snapshot();
    let chunk_size = setup.chunk_size.max(1);
    let lookahead = Lookahead::new(&snapshot, items, setup, lookahead_window(setup));
    std::thread::scope(|scope| {
        lookahead.spawn(scope, setup.threads);
        let pass = merge_pass(&lookahead, items, chunk_size, setup, results, total_items);
        lookahead.end();
        pass
    })
}

/// How many candidates may run past the merge: a chunk, and at least
/// [`LOOKAHEAD_PER_THREAD`] per thread (P03.2).
pub fn lookahead_window(setup: &PassSetup<'_>) -> usize {
    setup
        .chunk_size
        .max(LOOKAHEAD_PER_THREAD * setup.threads)
        .max(1)
}

/// Candidates per thread the workers may run past the merge. A candidate's
/// time varies a lot, and while a slow one holds the merge the others go
/// on only inside the window. Measured on `Issue508-DAC2020_bm10` (3
/// passes, 200 items, 8 threads, median of 3, load 3 to 8): a window of 16
/// candidates 13.3 s, 32 11.2 s, 64 9.3 s, 128 8.4 s, 256 8.4 s; the old
/// chunk barrier (32) 13.6 s. Improved candidates keep their boards until
/// the merge takes them, so the window also bounds that memory.
pub const LOOKAHEAD_PER_THREAD: usize = 16;

/// [`evaluate_pass`]'s merge: the outcomes in candidate order, chunk by
/// chunk, with Java's rules (see the module docs).
fn merge_pass(
    lookahead: &Lookahead<'_>,
    items: &[ItemId],
    chunk_size: usize,
    setup: &PassSetup<'_>,
    results: &mut BTreeMap<ItemId, ItemRouteResult>,
    total_items: &mut i32,
) -> PassEvaluation {
    let mut pass = PassEvaluation::default();
    let mut winner: Option<CandidateOutcome> = None;
    'chunks: for (number, chunk) in items.chunks(chunk_size).enumerate() {
        // PARITY: :720-729, a stop (the deadline is one) before a chunk.
        if setup.cancel.is_stop_requested() {
            pass.stopped = true;
            break;
        }
        let mut stop_seen = false;
        for (offset, item) in chunk.iter().enumerate() {
            let outcome = if stop_seen {
                // PARITY: :1054-1057, a task that starts after a stop.
                CandidateOutcome::skipped(*item)
            } else {
                let outcome = lookahead.take(number * chunk_size + offset);
                if outcome.requested_stop {
                    // PARITY: AutoroutePassRunner.java:113, `thread.requestStop()`.
                    setup.cancel.stop_all();
                    stop_seen = true;
                }
                outcome
            };
            *total_items = total_items.wrapping_add(1);
            results.insert(outcome.result.item_id(), outcome.result);
            pass.consumed.push(outcome.result);
            pass.consumed_work.push(outcome.work);
            if outcome.position.is_some() {
                pass.position = outcome.position;
            }
            if outcome.result.improved() {
                pass.consecutive_failures = 0;
                let better = winner
                    .as_ref()
                    .is_none_or(|w| outcome.result.improved_over(&w.result));
                if better {
                    winner = Some(outcome);
                }
            } else {
                pass.consecutive_failures += 1;
                if pass.consecutive_failures >= setup.max_consecutive_failures {
                    pass.early_stop = true;
                    break 'chunks;
                }
            }
        }
    }
    if !pass.stopped
        && let Some(CandidateOutcome {
            result,
            board: Some(board),
            ..
        }) = winner
        && result.improved()
    {
        pass.winner = Some((result, board));
    }
    pass
}
