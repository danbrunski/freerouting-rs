# Profiling and baselines

How to find out where the time goes on this machine, and how to tell whether a
change made the router faster. Written for P01 (2026-10-16); the policy is in
[performance.md](../roadmap/performance.md).

## The two clocks

- **Wall time** on the `benchmark` set is the number that counts. It is only
  worth recording on a quiet machine: `freerouting-dev bench` refuses to time
  anything while the 1-minute load average is above 1.0. This machine (WSL, 8
  cores) is shared with other sessions and is rarely quiet. Always write down
  the load average next to a wall time.
- **Callgrind instruction counts** are the steady second signal. They are
  deterministic (see "Why the counts repeat" below), so they show a 1 %
  regression on a machine where wall time moves by 30 %.

Each speed-up records both, before and after.

## What works here and what does not

| Tool | State on this machine |
|---|---|
| `valgrind --tool=callgrind` + `callgrind_annotate` | Works on the release build, about 50× slower than native: a DRC of `Issue178-KeebMaker_Sofle_Choc` takes 2.4 s |
| `samply record` | **Does not work.** `perf_event_paranoid` is 2 and samply refuses to start: `In order for samply to work with a non-root user, this level needs to be set to 1 or lower` (samply 0.13.1, installed with `cargo install samply --locked`). It needs `echo 1 \| sudo tee /proc/sys/kernel/perf_event_paranoid`, a system setting that was not changed. On a machine where that is 1, the recipe below applies. There is no `perf` binary for the WSL kernel either |
| `freerouting --stats json` | Works; per-stage work counts and times (P01.2) |
| `freerouting-dev microbench` | Works; fixed-work kernels (P01.3) |
| `tools/bench/callgrind.sh` | Works; instruction-count baselines (P01.4) |

## Build for a profiler

The workspace has a `profiling` profile: `release` plus line tables, so
profilers can name functions and lines. The code is the same code as
`release`.

```sh
cargo build --profile profiling -p freerouting-cli       # target/profiling/freerouting
cargo build --profile profiling -p freerouting-dev
```

Use `CARGO_BUILD_JOBS=2` here; the first build takes about 45 s.

## Recipe 1: callgrind on a whole run

```sh
taskset -c 0 valgrind --tool=callgrind --callgrind-out-file=drc.out \
    target/profiling/freerouting -drc -de fixtures/Issue178-KeebMaker_Sofle_Choc.dsn \
    < /dev/null > /dev/null
python3 tools/bench/callgrind_top.py drc.out --top 5
```

`taskset -c 0` matters: the DRC sizes its thread pool by the CPUs the process
may use, and callgrind counts only the thread a toggle is on (see below).
Pinned to one CPU the run is single-threaded and the whole cost is in one
place.

Run on `Issue178-KeebMaker_Sofle_Choc`'s DRC (676 M instructions, 2.4 s),
`callgrind_top.py` gives the top five functions by exclusive instructions:

| Instructions | Share | Function |
|---:|---:|---|
| 138.5 M | 20.5 % | `btree::map::Iter::next` |
| 101.4 M | 15.0 % | `Map<I,F>::try_fold` (iterator adapters over those maps) |
| 23.3 M | 3.4 % | `core::str::count::char_count_general_case` |
| 20.4 M | 3.0 % | `ShapeTree::insert` |
| 15.6 M | 2.3 % | `TileShape::intersects_tile` |

To profile only the DRC and leave the load out, collect only below the
function you care about (callgrind matches the demangled name, `*` is a
wildcard):

```sh
taskset -c 0 valgrind --tool=callgrind --callgrind-out-file=drc-only.out \
    --toggle-collect='*DesignRulesChecker::generate_report*' \
    target/profiling/freerouting -drc -de fixtures/Issue178-KeebMaker_Sofle_Choc.dsn \
    < /dev/null > /dev/null
```

That run collects 341 M of the 676 M instructions (the rest is the DSN load,
47 % of the whole). Its top five are `btree::map::Iter::next` 40.5 %,
`Map::try_fold` 29.7 %, `TileShape::intersects_tile` 4.6 %,
`ShapeTree::overlaps` 3.6 % and `TileShape::intersects_octagon` 3.6 %: the
DRC's time is item-list iteration, not geometry. (A lead for P02, not a
target yet: DRC is already fast.)

Callers and callees of one function:

```sh
callgrind_annotate --auto=no --tree=caller --inclusive=yes --threshold=100 drc.out | less
```

A `<` line is a caller with its call count, a `*` line is the function.

## Reading inlined Rust frames

Rust inlines aggressively, and callgrind charges inlined code to the **file
it came from**. `callgrind_annotate` therefore prints one row per
(source file, function), and one function shows up as several rows: its own
file, `btree/node.rs`, `iter/adapters/map.rs`, `slice/index.rs`. In the
profile above `btree::map::Iter::next` is three rows of 5.5 %, 5.1 % and
4.8 % and more, which sum to 20.5 %. Compare the sum, not a row:

- `tools/bench/callgrind_top.py <out> [--top N] [--inclusive]` sums the rows
  of each function (`--inclusive` keeps the largest row, since every row of a
  function carries its whole inclusive cost).
- To split a function that matters, find its callers with `--tree=caller`, or
  mark it `#[inline(never)]` for the measurement. `freerouting-dev microbench`
  does this for its timed loops (`timed_overlap`, ...).
- Line numbers: `callgrind_annotate drc.out path/to/file.rs` (without
  `--auto=no`) prints the source with a count per line. They are accurate
  with `debug = "line-tables-only"`, but an inlined line is charged to the
  inlined code's file.

## Recipe 2: samply (needs `perf_event_paranoid` <= 1)

Not usable on this machine (see above). Where it is allowed:

```sh
cargo install samply --locked
samply record --save-only -o drc.json.gz -- \
    target/profiling/freerouting -drc -de fixtures/Issue178-KeebMaker_Sofle_Choc.dsn
samply load drc.json.gz            # the Firefox Profiler UI in a browser
```

A sampling profile shows the wall-clock time including waiting, which
callgrind cannot, and it sees every thread. Its numbers are not repeatable,
so use it to find a hot spot and callgrind to prove a change.

## Why the counts repeat

Two runs of one build give the same callgrind count, to the last instruction,
for the kernels and the routing entries of `callgrind.sh`. That took four
things:

1. **One CPU** (`taskset -c 0`). The DRC and the optimizer size their thread
   pools by the CPUs they may use.
2. **A fixed hasher in the engine's hash maps** (`util::HashMap`,
   `util::HashSet`). `std`'s `RandomState` takes new keys in every process,
   which changes bucket order and probing, and with them the allocation
   pattern, and glibc's `malloc` then executes different instructions
   (about 1 % of a small DRC run). Nothing that reaches a result depends on
   hash order (D2); this only makes the cost repeat.
3. **`--toggle-collect`** on the function under test. The deadline watcher
   thread wakes on the clock; it is not on the main thread's stack, so it is
   not counted.
4. **`< /dev/null`**: the CLI's stdin reader thread.

A new `std::collections::HashMap` in the engine undoes (2): use
`crate::util::HashMap` and build it with `default()`.

## `--stats json`: counts per stage

```sh
target/release/freerouting -de board.dsn -do out.ses --stats json
# writes out.ses.stats.json; --stats-out <file> names the file
```

The file has, per stage (`load`, `fanout`, each autorouter `pass`, each
`optimizer_pass`, `drc`, `write`), the wall time and these counts:
`connections_attempted`, `connections_routed`, `connections_failed`,
`ripped_items`, `maze_queue_pops`, `maze_queue_pushes`,
`maze_drop_on_equal_skips`, `maze_skipped_occupied`, `rooms_completed`,
`doors_expanded`, `drills_computed`, `maze_work_units`, `maze_work_limit_hits`
and `fanout_work_limit_hits` (connections and fanout pins whose work limit was
exceeded), `tree_queries` (overlap queries and room completions),
`tree_leaves_returned` (N03.1's `leaves` unit, counted in either unit) and
`tree_nodes_visited` (D5's work unit), plus
`unit_costs`: microseconds per connection, per room, per tree query and per
1000 tree nodes. A `total` block sums the stages. `-drc --stats json` gives
`load` and `drc`.

The counts of a run repeat exactly; the times do not. The statistics change no
result (the session is byte-identical with and without the flag, a test pins
it) and cost nothing when off: the connection routers use the A04
`NoopSink`, and the work meter does one relaxed load of a flag. Measured at
`61b494a` plus P01: +0.017 % instructions with the flag off against a build
without the hooks (921,626,780 against 921,466,737 on `Issue433-my-board`);
wall median of 5: 0.75 s against 0.75 s on `Issue508-DAC2020_bm02` at load 9.

Rule of thumb for reading it: a per-unit cost that moves with the board is a
hot kernel; a count that moves is a change in what is searched.

## `freerouting-dev microbench`: kernels with fixed work

```sh
target/release/freerouting-dev microbench fixtures/Issue145-smoothieboard.dsn \
    [--runs 9] [--kernel overlap,complete_shape,maze,pull_tight,drc] [--json out.json]
```

Five kernels (overlap queries on the default tree, 45° room completion,
`maze` over 8 connections, a whole-board pull-tight, 20 DRC clearance passes)
each run 9 times. It prints the minimum time, the spread and the work counts,
and **fails if the work counts differ between runs**: the work is fixed, only
the time may change. `crates/freerouting-dev/tests/microbench.rs` pins the
counts on two fixtures; an optimization that claims to keep results identical
must not move them. The timed loops are `#[inline(never)]` functions named
`timed_*` so callgrind can isolate them.

## `tools/bench/callgrind.sh`: instruction-count baselines

```sh
tools/bench/callgrind.sh                     # write tools/bench/callgrind-baseline.json
tools/bench/callgrind.sh --compare           # each count against the baseline, in percent
tools/bench/callgrind.sh --compare --threshold 0.5 --only kernel/overlap
```

Entries: `kernel/<name>` (the timed loop of a microbenchmark on
`Issue433-my-board`) and `route/<board>` (the autorouter, `run_batch_loop`, 3
passes, on three `connection-parity` boards). `--compare` exits 1 when a count
grew by more than the threshold (1 % by default). The whole script takes about
12 s. Update the baseline in the commit that changes the work on purpose, with
the reason in the commit message.

A deliberately slower build (one extra `Vec::with_capacity` in
`BasicBoard::overlapping_tree_entries`) was reported as `kernel/overlap
+5.54 % REGRESSED`, `kernel/maze +0.34 %`, `kernel/pull_tight +0.56 %`, and
`kernel/complete_shape` unchanged (+0.00 %), which does not call it.

## Wall time against Java

```sh
cargo build --release -p freerouting-cli -p freerouting-dev
target/release/freerouting-dev bench --set tools/bench/sets/benchmark.txt \
    --java ~/code/freerouting/logs/jars/freerouting-a773a70ec.jar \
    --rust target/release/freerouting --runs 3 --out <scratch>/bench
```

See `tools/bench/README.md` for the options and the quiet-machine rule. The
table of the first run is in [performance.md](../roadmap/performance.md).
Run one JVM at a time (`JAVA_TOOL_OPTIONS=-Xmx3g`) and never beside a build.
