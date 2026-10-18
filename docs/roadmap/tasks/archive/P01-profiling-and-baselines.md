# P01 — Profiling and baselines

Opened 2026-10-15. Nothing about the router's speed is known yet, and wall
time on this machine is noisy (8 cores shared with other sessions). This
checklist builds the instruments first:
- profiler recipes that work here;
- a stats output that counts work per stage;
- microbenchmarks whose work count is fixed;
- the first Rust-against-Java baseline.
Every later P item measures with these.

## What exists and what this adds

| Exists | State |
|---|---|
| `valgrind --tool=callgrind` | Installed, works on the release CLI: 670 M instructions for a release DRC run on `Issue178-KeebMaker_Sofle_Choc` (2026-10-15) |
| `perf_event_paranoid` | 2, so user-space sampling of your own processes is allowed. No `perf` binary for the WSL kernel |
| `freerouting-dev bench` (F02.3) | Times Java and Rust on board sets with a quiet-machine check |
| The A-series search core | Generic over sinks (A04), so counters compile out of the default build |

What this adds:
- documented profiler recipes;
- `--stats json` on the CLI;
- a microbenchmark harness;
- callgrind instruction-count baselines;
- the first parity baseline against Java, and a recommended speed target.

## Decisions made here

- **Two clocks.** Wall time on the `benchmark` set is the number that counts.
  Callgrind instruction counts are the steady second signal for
  regressions. Both are recorded.
- **No profiler dependency in the product.** Profiling uses external tools
  (callgrind; `samply` installed with `cargo install`, which is a developer
  tool, not a crate dependency). The stats output uses the A04 sink, so it
  costs nothing when it is off.
- **Microbenchmarks fix the work, not the time.** Each microbenchmark asserts
  that its work count is identical across builds, and only the time may
  change. This is KiCadRoutingTools' `microbench.py` rule ("the printed
  iterations must be IDENTICAL across binaries").

## File ownership

| Item | New files | May edit |
|---|---|---|
| P01.1 | `docs/development/profiling.md` | — |
| P01.2 | `crates/freerouting-cli/src/stats.rs` | `crates/freerouting-cli/src/main.rs` (one flag), the A04 sink module (append a stats sink) |
| P01.3 | `crates/freerouting/benches/` or `crates/freerouting-dev/src/microbench.rs` | `crates/freerouting-dev/src/main.rs` (append a subcommand) |
| P01.4 | `tools/bench/callgrind-baseline.json`, `tools/bench/callgrind.sh` | — |
| P01.5 | `docs/roadmap/performance.md` (baseline table) | `docs/roadmap/port-completion.md` (the target line, after the owner agrees) |

## Checklist

- [x] **P01.1 — Profiler recipes that work on this machine.**
  Write `docs/development/profiling.md` covering:
  - callgrind on a release build
    (`valgrind --tool=callgrind --callgrind-out-file=… ./target/release/freerouting …`,
    then `callgrind_annotate`). Set `debug = 1` or `"line-tables-only"` in a
    `[profile.profiling]` that inherits `release`, for symbols.
  - a sampling profiler: try `samply record` (installed with
    `cargo install samply`). If WSL blocks it, say so and why.
  - how to read inlined Rust frames.
  - **Accept:** each recipe was run on `Issue178-KeebMaker_Sofle_Choc`'s DRC
    (until the router exists), and the doc shows its top five functions.
    `cargo build --profile profiling` works and is documented. A recipe that
    doesn't work here is listed with the error.

  *Done 2026-10-16:* `docs/development/profiling.md`; `[profile.profiling]` (release plus `debug = "line-tables-only"`) in the root `Cargo.toml`, `cargo build --profile profiling -p freerouting-cli` builds in 42 s. **Callgrind** was run on the DRC of `Issue178-KeebMaker_Sofle_Choc` (`-drc`, 676,148,680 instructions, 2.4 s under valgrind): top five by function, summed over inlined rows with the new `tools/bench/callgrind_top.py`: `btree::map::Iter::next` 20.5 %, `Map::try_fold` 15.0 %, `char_count_general_case` 3.4 %, `ShapeTree::insert` 3.0 %, `TileShape::intersects_tile` 2.3 % (the DRC alone, `--toggle-collect='*generate_report*'`, 341 M: BTreeMap iteration 40.5 % + `try_fold` 29.7 %). The doc explains why one function is several rows (inlined code is charged to its own file). **samply does not work here**: `cargo install samply --locked` (0.13.1) builds, but `samply record` stops with `In order for samply to work with a non-root user, this level needs to be set to 1 or lower` (`perf_event_paranoid` is 2; the fix is a `sudo` sysctl that was not made); there is no `perf` binary either. The recipe is in the doc for a machine where it is 1. Callgrind counts repeat only with one CPU (`taskset -c 0`), a fixed hasher in the engine's hash maps (`util::HashMap`) and a toggle on the function under test: the doc's "Why the counts repeat".

- [x] **P01.2 — `--stats json`.**
  A CLI flag that writes per-stage counts and times to a JSON file through the
  A04 stats sink:
  - board load, DRC, fanout, each autorouter pass, each optimizer pass;
  - per stage: wall time, connections attempted and routed, rooms completed,
    doors expanded, maze queue pops and pushes, drop-on-equal skips, tree
    nodes visited (D5 units), overlap queries, inserts and ripups;
  - per-unit costs derived from these (µs per connection, per room, per
    overlap query).
  - **Accept:**
    - On 3 fixtures, routing with and without `--stats` gives byte-identical
      SES.
    - The work counts are identical across two runs.
    - Wall time with stats off is within noise of a build without the sink
      (median of 5).

  *Done 2026-10-16:* `freerouting --stats json` (and `--stats-out <file>`; `crates/freerouting-cli/src/stats.rs`, taken out of argv in `cli::run` before the Java parsers, so no change to `args.rs`) writes `<output>.stats.json` through `freerouting::run_stats` (new engine module; the A04 `WorkCounters` sink is the stats sink, no new sink type). Stages: `load`, `fanout`, each `pass`, each `optimizer_pass`, `drc`, `write` (and `load`, `drc` for `-drc`); per stage the wall time, the 14 counts the item names (connections attempted/routed/failed, ripped items, queue pops/pushes/drop-on-equal skips/skipped occupied, rooms completed, doors expanded, drills, maze work units, tree queries, tree nodes visited in D5 units) and `unit_costs` (µs per connection, per room, per tree query, per 1000 nodes). Sample: `Issue433-my-board`, fanout 8 connections, 563 pops, 2156 pushes, 803 drop-on-equal skips, 199 rooms, 174,904 tree nodes, 2320 µs per connection. **Accept 1 and 2:** `crates/freerouting-cli/tests/stats.rs::stats_change_no_session_and_the_counts_repeat`, on `Issue413-test`, `Issue433-my-board` and `Issue143-rpi_splitter`: the SES with `--stats` equals the SES without (bytes), and the `counts` of two stats runs are equal line by line; `drc_mode_has_load_and_drc_stages`, `a_bad_format_is_a_usage_error`; unit tests in `run_stats.rs` and `stats.rs`. **Accept 3** (stats off against a build without the hooks: HEAD `61b494a` in a private worktree, `examples/route_board`): callgrind 921,466,737 without, 921,626,780 with the hooks and the flag off, +0.017 %; wall median of 5 on `Issue508-DAC2020_bm02`, alternating runs, 0.75 s against 0.75 s (runs 0.70 to 1.23 s; load average 9, not quiet, so the instruction count is the evidence). The route-bound SES of the two builds are byte-identical.

- [x] **P01.3 — Microbenchmarks with fixed work.**
  A harness (`freerouting-dev microbench`, or `cargo bench` with a
  hand-rolled timer; criterion is a `NEW DEP`) for the expected hot kernels:
  - overlap queries on a loaded board's default tree;
  - `complete_shape` (45°) for a fixed room list;
  - one maze search for a fixed connection;
  - one pull-tight of a fixed trace;
  - the DRC clearance pass.
  Each reports the minimum time over 9 runs and its work count.
  - **Accept:**
    - Every kernel prints a work count that a test asserts is unchanged.
    - Running the harness twice gives identical work counts and minimum
      times within 5 %.

  *Done 2026-10-16:* `freerouting-dev microbench board.dsn [--runs N] [--kernel a,b] [--json f]` (`crates/freerouting-dev/src/microbench.rs`, hand-rolled timer, no new dependency): `overlap` (20,000 seeded overlap queries on the default tree), `complete_shape` (4,000 seeded 45° room completions on the autoroute tree of the smallest trace clearance class), `maze` (8 connections through `route_connection`, ripup on, pass-1 work limit), `pull_tight` (whole-board `opt_changed_area`, accuracy 500, after 40 laid connections), `drc` (20 clearance passes). Minimum over 9 runs by default, with the spread. **Accept 1 met:** every kernel prints its work counts (tree nodes visited, queries, entries/rooms found, queue pops/pushes, rooms completed, doors expanded, maze work units, traces and corners, violations); the harness returns an error if a run's counts differ from the first run's (`work_that_changes_between_runs_is_an_error`), and `crates/freerouting-dev/tests/microbench.rs` pins all five kernels' counts on `Issue433-my-board` and `Issue413-test` (`work_counts_of_issue433`, `work_counts_of_issue413`, 2 runs each; debug and release give the same numbers). **Accept 2:** two harness runs on `Issue145-smoothieboard` (release, 9 runs each, load average 0.99 and 1.07, so just above the 1.0 the series asks for): identical counts, minimum times overlap 34.918 / 35.079 ms (+0.5 %), complete_shape 54.677 / 54.888 ms (+0.4 %), maze 562.162 / 555.647 ms (-1.2 %), pull_tight 8.801 / 8.773 ms (-0.3 %), drc 58.627 / 59.245 ms (+1.1 %); within 5 % on all five. The load was 0.99 for the first run and 1.07 for the second (a repeat at a load clearly under 1.0 would be `target/release/freerouting-dev microbench fixtures/Issue145-smoothieboard.dsn --runs 9` twice, then compare the `min` lines). The kernels' instruction counts, which are steady at any load, are P01.4's. Do not time under load: on the same board at load 4.5 the `drc` minimum was 59 ms but the `maze` minimum 1320 ms for 12 connections against 562 ms for 8 at load 1.

- [x] **P01.4 — Callgrind instruction-count baselines.**
  `tools/bench/callgrind.sh` runs each microbenchmark and a small routing run
  (3 boards from `connection-parity`) under callgrind, and writes the
  instruction counts to `tools/bench/callgrind-baseline.json`. A
  `--compare` mode reports each count against the baseline in percent.
  - **Accept:**
    - Two runs of the same build give identical counts.
    - A deliberately slower build (an extra allocation in the overlap-query
      loop) is reported with its percentage on the right kernel.

  *Done 2026-10-16:* `tools/bench/callgrind.sh` (modes: write the baseline, `--compare [--threshold pct] [--only a,b]`) and `tools/bench/callgrind-baseline.json` (8 entries: the five kernels' timed loops on `Issue433-my-board`, via `--toggle-collect='*microbench::timed_<kernel>*'`, and `run_batch_loop` (fanout plus 3 passes, optimizer off) of `Issue143-rpi_splitter`, `Issue508-SMD-routing-issue-demo` and `Issue433-my-board` from `connection-parity`); the whole script takes 11 s. **Accept 1:** two `--compare` runs of one build print `+0.00 %` for all 8 entries (identical counts; the script's own three repeats of the DRC kernel gave 17,062,605 each). Getting there needed three fixes, in the doc: `taskset -c 0` (the DRC spawns one thread per CPU and callgrind counts only the toggled thread; before it the DRC kernel read 0.89 M instead of 17.06 M), a fixed hasher for the engine's hash maps (`util::HashMap`/`HashSet`, used in `drc/*`, `board/basic_board.rs`, `trace_ops.rs`, `scoring/board_statistics.rs`, the DSN parser and the SES writer: `RandomState` made glibc `malloc` run 1 % different instruction counts), and the toggle on `run_batch_loop`, which leaves out the deadline watcher thread. **Accept 2:** a build with one extra `Vec::with_capacity(64)` (black-boxed) at the top of `BasicBoard::overlapping_tree_entries` was reported as `kernel/overlap 67,871,587 -> 71,630,584 +5.54 % REGRESSED`, with `kernel/maze +0.34 %`, `kernel/pull_tight +0.56 %` (both call it), `kernel/drc -0.01 %` and `kernel/complete_shape +0.00 %` (does not); the edit was reverted. The baseline in the repository was rewritten at the end of P01 (git `f9e9ead` plus the working tree): other sessions' uncommitted engine edits had moved the counts by up to 0.14 % since the first write (the numbers above are from the first write); a `--compare` right after the rewrite prints `+0.00 %` for all 8 entries. Rewrite it (`tools/bench/callgrind.sh`) in any commit that changes the work on purpose.

- [~] **P01.5 — The first parity baseline against Java.**
  After the A-series board-parity gate passes, run `bench` on the `benchmark`
  set: Java (the pinned jar, optimizer off) against Rust, 3 runs each,
  quiet machine. Do the same with the optimizer on, once O06 lands. Put the
  table in performance.md:
  - per-board median wall time and spread
  - the ratio Java/Rust
  - routing results equal (by `ses-diff`) or the known reason they aren't
  Recommend the release target from these numbers.
  - **Accept:**
    - The table covers every `benchmark` board.
    - The recommendation names a median ratio and the slowest boards.
    - The owner's decision on the target is recorded in port-completion.md "What
      complete means". Until then, this item stays partial with "needs
      the owner".

  *Partial 2026-10-16:* the table, the ratios, the result comparison and the recommendation are in [performance.md](../../performance.md), "First parity baseline against Java". Summary: `freerouting-dev bench --set tools/bench/sets/benchmark.txt`, Java `a773a70ec` (JDK 25, `-Xmx3g`) against Rust, optimizer off, all 16 boards. **Quiet machine (load at most 1.0, 3 runs) only for the five boards where Java takes under 4 s**; the machine was not quiet for the other eleven (one Java run and two Rust runs at load 3.2 to 7.3), and `bench` stopped when the load rose again (`Stopped early: ... load average is 2.68`). **Results equal:** `ses-diff` of every Rust session against `fixtures/reference/route/<board>.ses` (Java at the same pin and bounds): 16 of 16 equivalent, including the five boards where Java's work limit fired. **Medians:** 1.35 over all 16 boards (dominated by Java's 3 s JVM start-up on the five small ones), **1.16 on the 11 boards where Java takes 10 s or more**; summed over the set Rust 3,443 s against Java 3,182 s. **Slowest against Java:** `Issue690-kit-dev-coldfire-xilinx_5213` 0.77, `Issue420-contribution-board` 0.83, `Issue066-Project_GP8B` 0.89, `Issue753-CPU-85_r104` 0.92. **Recommendation:** keep 3× and measure it on the boards where Java takes 10 s or more; the first parity build is at 1.16×; the route profile (geometry predicates 14 %, the `malloc` family 16 %, `remove_redundant_lines` 8.6 %) gives P02 and P05 clear targets but 2.6× more probably needs P04. **Remainder:** (1) the owner's decision on the target, to be recorded in port-completion.md "What complete means" (**needs the owner**; that file is untouched); (2) the quiet-machine re-run of the eleven busy boards with `--runs 3` (command in performance.md; about 5 hours, or about 1 hour 45 minutes with `--runs 1`); (3) the optimizer-on table (`--optimize`; O06 has landed; Java's optimizer depends on thread scheduling, J01, so aggregates only), not run. A first attempt wrote no session (a relative `--out`: the bench runs each engine in the board's directory; `freerouting-dev bench` now makes `--out` absolute); its wall times are the ones in the table, and the sessions for the comparison came from a Rust-only rerun with an absolute path.
