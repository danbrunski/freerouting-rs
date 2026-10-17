# P03 — Parallelism with an ordered merge

Opened 2026-10-15. The router's core is sequential by design. Each connection
changes the board the next one sees, and the search tree's shape decides
routes (D5). So the parallelism here is limited to work whose result can't
depend on order or thread count:
- independent analyses: DRC, statistics, airlines;
- candidate evaluation on copies (the optimizer);
- whole boards (the bench runner).
The DRC already runs its clearance and island checks on all cores with an
ordered merge.

Prerequisites: P01; O06 (the batch optimizer) for P03.2.

## What exists and what this adds

| Exists | State |
|---|---|
| DRC | Clearance violations and island fills computed on all cores, merged in item order (DIVERGENCES, "Design rule check") |
| O06 | The batch optimizer evaluates candidates on copy-on-write copies with an ordered merge |
| `bench` (F02.3) | Runs boards one after another |

## Decisions made here

- **Not allowed without a recorded divergence:**
  - routing two connections at once;
  - speculative searches for the next connection;
  - reusing rooms or drills across connections.
  Each of these changes the tree topology or the work counts (A-series
  research §6.4). Java measured the same idea and removed it:
  `git -C ../freerouting show main:docs/research/large-board-autorouter-performance.md`
  records that the clone-and-shuffle multithreaded pass
  (`AutoroutePassRunner.runMultiThread`, one deep copy per thread, best
  score kept) and two further parallel phases never beat the single-thread
  pass by enough to keep, so upstream deleted them; `router.autorouter.max_threads`
  stayed in the settings and the live pass ignores it. Its profiles also show
  the maze at over 95 % of item time on the slow boards, with statistics and
  the incomplete scan under 120 ms combined. P03 does not repeat that
  experiment; a parallel pass is a Q-style measured divergence if ever.
- **One thread pool.** Use one `rayon` pool (a `NEW DEP` for the engine
  crate if it isn't one already; check `Cargo.toml`). It is sized by
  `router.max_threads`, which defaults to the core count. Threads=1 must give
  the same bytes.

## File ownership

| Item | New files | May edit |
|---|---|---|
| P03.1 | — | `crates/freerouting/src/scoring/`, `crates/freerouting/src/drc/` |
| P03.2 | — | `crates/freerouting/src/optimize/` (O06's module) |
| P03.3 | — | `crates/freerouting-dev/src/bench.rs` |

## Checklist

- [~] **P03.1 — Parallel statistics and analyses.**
  Run the per-pass statistics, airline computation and DRC inside the
  pipeline on the pool, with merges in item order.
  - **Accept:**
    - Threads=1 and threads=8 give byte-identical SES and statistics JSON on
      `benchmark`.
    - `tools/determinism-check.sh` passes.
    - The per-pass time on the 3 largest boards is in the note.
  *Partial 2026-10-17:* every clause holds; the remainder is the timing rule. All timings here
  are pre-P04 (the Java-shaped tree) at load 2 to 12 from other sessions. Code:
  `crates/freerouting/src/parallel.rs` (new; `ThreadBudget`, `map`, `map_with`: scoped threads
  with an atomic work counter and an ordered merge, a worker's own budget is 1 so nested
  analyses run inline; no `NEW DEP`, as O06), sized per job by `autorouter.max_threads`
  (`--threads`; `job/run.rs`); the DRC's `parallel_map` (clearance, zone islands, feasibility)
  now runs on it; the airlines (`DesignRulesChecker::calculate_all_incompletes`) per net with
  one contacts cache per worker, merged in net order; the statistics' fanout counts take the
  pins' connectable items from one scan and each item's contacts once on the threads, then
  each pin's `isPinEscaped` and connected set on the threads; the bounds take the terminal
  items of all nets from one scan (1.66 to 0.08 ms on `Issue093-interf_u`, then sequential:
  thread start-up cost more than the work); `--stats json` has a new `statistics` stage after
  every `pass`. Tests: `parallel` unit tests (5); `tests/parallel_analyses.rs`
  (`statistics_do_not_depend_on_the_thread_budget`, `fanout_counts_equal_the_per_pin_definition`
  on 4 boards loaded and routed with their Java route sessions, both answers seen,
  `drc_findings_do_not_depend_on_the_thread_budget`, budgets 1/2/3/8); `reference_load`
  (Java's bounds and fanout counts on every fixture) passes. **Statistics time** per pass
  (`examples/stats_bench.rs`, new; routed with the Java route session, release, median of 7,
  load 3.0 to 3.5), HEAD `9685d7f` (its DRC on 8 cores) against this tree at 1 / 8 threads:
  `Issue420-contribution-board` 226.35 ms against 87.09 / 19.71 ms, `Issue219-LogicBoard_smt`
  134.26 against 73.85 / 21.70, `Issue145-smoothieboard` 95.13 against 48.69 / 15.32,
  `Issue093-interf_u` 9.68 against 11.76 / 5.72 (on Issue420 at 8 threads: clearance 4.70,
  fanout and the rest 14.43 where HEAD took 221.06, bounds 0.54 where HEAD took 18.78, airlines
  24.77 where HEAD took 62.46). **Per-pass time** on the 3 largest (`--stats json`, `pass` +
  `statistics` stages, benchmark bounds): Issue420 threads 1: 92.8 s + 61 ms, 124.5 s +
  130 ms, 63.0 s + 111 ms; threads 8: 92.4 s + 19 ms, 122.9 s + 39 ms, 62.2 s + 34 ms. Issue219
  threads 1: 169.2 s + 83 ms; threads 8: 164.6 s + 23 ms. Issue145 threads 1: 59.9 s + 41 ms,
  25.3 s + 45 ms, 10.9 s + 47 ms; threads 8: 61.2 s + 17 ms, 25.1 s + 15 ms, 10.8 s + 17 ms.
  On these boards the statistics are under 0.5 % of a pass (the routing is; P02's 20.4 % was
  `interf_u`'s instruction share), and on `interf_u` (18 passes) they are 0.121 s at 1 thread and
  0.075 s at 8 against passes of 1.348 / 1.135 s. **Byte-identical:** the 16 `benchmark` boards
  routed with their bounds at `--threads 1` and `--threads 8` (`--stats json --result-json`, the
  optimizer on, 3 passes and 200 items; logs in `target/orchestrator/p03/scratch/bm-t1`, `bm-t8`):
  16 of 16 SES and result manifests identical; the stats JSON identical except
  `Issue508-DAC2020_bm10`, whose optimizer counted candidates run past a stop (fixed in P03.2:
  `run_stats::capture`); after the fix the five boards the optimizer runs on (the other eleven
  are not completely routed, so its guards skip it) are identical at 1 and 8 threads,
  statistics included, and their SES equal HEAD's. `tools/determinism-check.sh` (those five
  boards at 8 threads, SES, stats and result JSON and logs, timing lines ignored): `identical`,
  exit 0, load 10 to 15. Remainder: quiet-machine wall times, and re-timing against P04's
  default tree once P04 lands.

- [~] **P03.2 — Tune the optimizer's parallel evaluation.**
  O06 makes candidate evaluation parallel and deterministic; this item
  measures and tunes it. Look at chunk size, copy cost (with P02.4) and the
  pool size against the core count.
  - **Accept:**
    - Threads=1 against threads=N byte-identical on `benchmark` with the
      optimizer on.
    - Optimizer wall time against thread count (1, 2, 4, 8) on the 3
      largest boards is in the note.
    - The default setting is the best measured one.
  *Partial 2026-10-17:* every clause holds on a busy machine (load 3 to 12, pre-P04); the
  remainder is the quiet-machine re-measure. Changes: `optimize/batch/lookahead.rs` (new): the
  threads run the candidates in order up to a window ahead of the merge
  (`lookahead_window`: the chunk, at least `LOOKAHEAD_PER_THREAD` = 16 per thread) instead of
  waiting for every chunk's slowest candidate; the merge still walks chunks with Java's rules,
  so stops and the failure limit end a pass at the same candidate, and a candidate's
  `--stats json` counts are added only when the merge consumes it (`run_stats::capture`);
  the candidates' before/after figures compute only the via count and the weighted length
  they read (`BoardStatistics::via_count_and_weighted_length`, bit for bit), not the fanout and
  bounds. Tests: `lookahead_past_an_early_stop_changes_nothing` (bm08 and J2, failure limits 1
  and 12, threads/chunk 8/8, 8/64, 3/5, 2/1 against 1/1: consumed results, work, early stop,
  winner and result map equal; one pass does stop early),
  `lean_optimizer_figures_equal_the_cheap_statistics`; `tests/batch_optimizer.rs` 21 pass
  (`optimizer_is_thread_count_independent`, `chunk_size_does_not_change_results`, ...),
  `tests/reference_optimize.rs` 3 pass. **Wall time against threads**
  (`examples/optimize_board`, the Java route session, 3 passes, 200 items, median of 3,
  seconds; this tree / HEAD `9685d7f`). The three largest `benchmark` boards are not completely
  routed at their bounds, so the guards skip the optimizer there (with `--no-guards` 16
  candidates of Issue145 took 295 s and 1.47 GB at 8 threads); the three largest it runs on:
  `Issue508-DAC2020_bm10` threads 1: 53.26 / 56.35, 2: 26.75 / 35.39, 4: 13.90 / 22.01,
  7: 10.52 / 15.33, 8: 10.28 / 14.54; `Issue508-DAC2020_bm07` 1: 3.213 / 3.724, 2: 1.630 /
  2.029, 4: 0.862 / 1.192, 7: 0.696 / 0.881, 8: 0.684 / 0.911; `Issue508-DAC2020_bm02`
  1: 1.033 / 1.198, 2: 0.533 / 0.649, 4: 0.290 / 0.356, 7: 0.232 / 0.265, 8: 0.235 / 0.268
  (`target/orchestrator/p03/scratch/opt_matrix.tsv`); every SES of a board identical across
  binaries and thread counts. **Chunk size** (bm10, 8 threads): with the chunk barrier
  (P03.1's build) 8: 25.8 s, 16: 18.0, 32 (the default): 17.4, 64: 12.8, 128: 11.4, 256: 10.0
  (load 8 to 12); with the lookahead, window 16: 13.3, 32: 11.2, 64: 9.3, 128: 8.4, 256: 8.4,
  so 16 per thread (128 at 8 threads) is the default. **Copy cost:** a candidate's start board
  is P02.4's memoized restore, 0.2 to 0.4 ms on the largest fixtures against tens to hundreds
  of milliseconds of rerouting per candidate here, so it is not a lever. **Pool size:** 7
  threads (Java's default, cores − 1) and 8 are within the spread on all three boards; the
  default stays 7. **Byte-identical with the optimizer on:** the `benchmark` pair in P03.1's
  note (16 of 16 SES at 1 and 8 threads, the optimizer's five boards rerun with the final
  build, statistics included). Remainder: the quiet-machine re-measure of the matrix (whether
  8 threads beats 7 there), and re-timing against P04's default tree.

- [x] **P03.3 — Boards in parallel in `bench`.**
  `bench --jobs N` routes N boards at once, each pinned to a share of the
  cores.
  - Timing runs keep `--jobs 1`; the quiet-machine rule still applies.
  - `--jobs` is for correctness and parity sweeps.
  - **Accept:**
    - A parity sweep of `connection-parity` with `--jobs 4` gives the same
      `ses-diff` results as `--jobs 1`.
    - The sweep's wall time is in the note.
  *Done 2026-10-17:* `freerouting-dev bench --jobs N` (`crates/freerouting-dev/src/bench.rs`:
  `run_board`, `thread_share`, `thread_arguments`; `main.rs`, `bench_report.rs` record `jobs`).
  N workers take the boards in set order and the results are merged in set order; each run gets
  `--router.autorouter.max_threads=S --router.optimizer.max_threads=S`, `S = max(1, cores / N)`;
  above 1 the load is not checked and `bench.md` says the wall times are not timings;
  `--jobs 1` keeps the old command line and the quiet-machine rule. Tests:
  `parallel_boards_share_the_cores`, `jobs_route_boards_at_once_and_merge_in_set_order`
  (`tests/bench.rs`, 13 pass). **Sweep:** `bench --set tools/bench/sets/connection-parity.txt
  --rust <this tree> --max-load off` with `--jobs 1` and `--jobs 4`: 10 of 10 SES
  byte-identical, `ses-diff` of each pair `equivalent` 10 of 10, and the statistics in the two
  `bench.json` equal. Wall time (load 6.4): `--jobs 1` 0.97 s, `--jobs 4` 0.38 s
  (`target/orchestrator/p03/scratch/cp-j1`, `cp-j4`).
