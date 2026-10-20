# Performance

Recorded 2026-10-15. The owner wants the port to perform better than Java
(decision D1). This document says how speed is measured, what may change at
each stage of the port, and where the time is expected to go. The checklists
are the [P series](tasks/P00-performance-README.md).

## What is true today

- **Loading and DRC are already fast.** In release, loading a board plus a
  full DRC takes 0.04–0.5 s on the largest fixtures and 0.09 s on the owner's
  `canary-pwr` (measured 2026-10-15 at `b119467`). Nothing there needs work
  yet.
- **The router exists since A13/A14** and is where the time goes; at parity it is as fast as Java
  (median 1.16× faster on the boards where Java takes 10 s or more, see "First parity baseline" below).
  The Java figures that follow are from before it existed: the Java router took 82 s of routing plus about 5 minutes of optimizer on
  `Issue508-DAC2020_bm01` in a GUI run. Its fanout on that board ran 8 passes
  in 1.8 s.
- **The test loop is the slow part of development:** 1.5–4 min for the
  workspace in plain debug, 23 s at opt-level 1 ([F01](tasks/archive/F01-dev-loop-and-ci.md)).
- **First profile.** A callgrind profile of a release DRC run on
  `Issue178-KeebMaker_Sofle_Choc` (670 M instructions) spends 20.6 % in
  `BTreeMap` iteration and 14.5 % in an iterator `try_fold`. It's a hint, not
  a target: DRC is already fast.

## Rules

1. **Correct first, then fast.** Until rung 3 of the verification ladder
   (board parity), speed work is limited to choices that cost nothing in
   parity. Those are data layout, arenas, avoiding allocation, and caches
   that cannot change a result. These are made while porting (D1), not
   added afterwards.
2. **After parity, an optimization must leave results byte-identical.**
   `freerouting-dev ses-diff` finds no difference on the `benchmark` set, the
   probe logs are identical, and so is the work-unit count. A speed-up that
   changes a route is a quality change and follows the Q-series measurement
   rules ([routing-quality.md](routing-quality.md)).
3. **Measure before and after, the same way every time:**
   - a release build on a quiet machine: the bench runner refuses a
     load average above 1.0 (F02.3)
   - several runs, reporting the median and spread
   - per-unit costs (per connection, per room, per overlap query), not shares
     of run time. A share divides by a run time that moves with the board,
     the machine and the load. That rule comes from KiCadRoutingTools.
4. **Instruction counts as a second signal.** `valgrind --tool=callgrind`
   works on this machine. Its instruction counts are deterministic, so a
   regression gate on them is steady on a machine where wall time is noisy
   (other sessions share these 8 cores). Wall time stays the number that
   counts.
5. **Parallelism keeps D2.** Results are merged in submission order and never
   depend on the thread count. Threads=1 and threads=N give a byte-identical
   SES, and `tools/determinism-check.sh` passes under load.
6. **SIMD only where a profile says so** (P05). It must be bit-identical to
   the scalar path, portable to Windows, macOS (x86_64 and aarch64) and Linux
   on stable Rust, with a scalar fallback, and never `target-cpu=native`.

## Where the time is expected to go

From the research into the Java router at the pin (the A-series research
report, §6.4) and from KiCadRoutingTools' Rust core:

| Area | Why it is hot | What is free to change in parity | What isn't |
|---|---|---|---|
| Search-tree overlap queries | Every room completion, shove check and DRC query | The per-call allocation, the storage of tree shapes, the arena layout | Tree shape and visit order (D5) |
| Room completion (`complete_shape`) | Runs per room per connection | Scratch vectors, octagon reuse | Obstacle visit order, which follows the tree's depth-first order |
| Maze priority queue | Every expansion | A `BinaryHeap` plus a key set instead of a `BTreeSet`, if the pop order and the drop-on-equal rule stay | The queue key, the drop-on-equal rule |
| Door shapes and section segments | Recomputed in Java | Caching, invalidated on `set_shape` | — |
| `FoundConnectionInserter` trace logging | Java builds trace strings unconditionally (`if (true) FRLogger.trace(...)`) | Dropping them | — |
| Board copies (history, best board, optimizer candidates) | Java serializes whole boards | `Arc`-shared immutable base plus a wiring overlay; copy-on-write | What a restore resets (B01) |
| Per-pass statistics | Java recomputes them per event | Computing them once per pass | The values |
| Pull-tight and the optimizer | Most of a GUI run's time in Java | Parallel candidates on copy-on-write copies with an ordered merge (O06) | Candidate order, acceptance rules |

The Java run time is only a starting estimate. P01 replaces it with
measurements of the Rust port.

## Stages

1. **While porting (A, B, O series):**
   - data-oriented structures (arenas with `u32` ids, flat section state
     reset by generation stamps, the autoroute info as a side table);
   - one search core generic over sinks (A04);
   - a debug audit of incremental state against a fresh build (P02.1).
2. **At parity (P01):** profiles and baselines. These give the first honest
   Rust-against-Java numbers, and P01 sets the release target, which
   port-completion.md proposes as 3× lower median wall time.
3. **After parity, P02:** optimizations that keep results identical. Its
   numbers say how far the parity-shaped tree can go.
4. **Freeing the search tree (P04), right after P02** (owner, 2026-10-16).
   The tree's shape decides routes today (D5). A faster tree first needs
   room completion that doesn't depend on visit order, for example by
   visiting obstacles in item-id order, and a work unit that doesn't count
   nodes. Both change routes, so they are a measured divergence, and they
   land before any route-changing Q item so every later measurement is
   paired against the freed tree once. The Java-shaped tree is kept as a
   selectable mode (`router.search_tree = "java"`): the parity suite and the
   probes keep their exact Java reference on it, and the fast tree becomes
   the default only when its measurement accepts it. The speed target is
   stated for the default tree.
5. **Then P03 and P05**, measured against the freed tree: parallelism with
   an ordered merge, and SIMD where P01 shows a hot data-parallel loop.
   P03.1 (statistics and analyses) may run beside P02, since it changes no
   route.

## First parity baseline against Java (P01.5, 2026-10-16)

Rust at `61b494a` plus P01 against Java `a773a70ec` (the pinned jar, `-Xmx3g`, JDK 25), the 16
boards of `tools/bench/sets/benchmark.txt`, optimizer off, each board's `max_passes` from
`tools/java-reference/route-bounds.tsv`, `freerouting-dev bench`. Times are wall seconds, median
(number of runs). **Only the five small boards were timed on a quiet machine** (1-minute load at
most 1.0, 3 runs each, `max load before` is the highest over those runs). The other eleven are one
Java run and two Rust runs at load 3.2 to 7.3 (shared WSL, 8 cores; other sessions were routing and
building), so read their ratios as rough: the load hits both engines, but not evenly. Re-run them
on a quiet machine before quoting a number (command below).

| Board | max passes | Java s | Rust s (spread) | Java / Rust | max load before | Java cut-offs | Result |
|---|---:|---:|---:|---:|---:|---:|---|
| Issue433-my-board | 100 | 3.16 (3) | 0.04 (0.00) | 78.9 | 0.95 | 0 | equal |
| Issue649-kicad_ecc83-pp_input_board_v1 | 100 | 3.19 (3) | 0.02 (0.00) | 159.8 | 0.98 | 0 | equal |
| Issue508-DAC2020_bm02 | 100 | 3.38 (3) | 0.14 (0.00) | 23.9 | 0.90 | 0 | equal |
| Issue690-sonde_xilinx | 100 | 3.94 (3) | 0.44 (0.00) | 8.9 | 0.94 | 0 | equal |
| Issue508-DAC2020_bm07 | 100 | 3.94 (3) | 0.54 (0.02) | 7.3 | 0.89 | 0 | equal |
| Issue508-DAC2020_bm01 | 3 | 17.44 (1) | 14.82 (0.39) | 1.18 | 3.72 | 0 | equal |
| Issue508-DAC2020_bm10 | 100 | 10.90 (1) | 7.32 (0.11) | 1.49 | 4.34 | 0 | equal |
| Issue093-interf_u | 100 | 11.46 (1) | 2.47 (0.67) | 4.64 | 7.04 | 0 | equal |
| Issue555-CNH_Functional_Tester_1 | 100 | 13.07 (1) | 8.78 (0.21) | 1.49 | 7.04 | 0 | equal |
| Issue269-z10_module | 3 | 158.46 (1) | 154.11 (5.66) | 1.03 | 5.68 | 11 | equal |
| Issue690-kit-dev-coldfire-xilinx_5213 | 3 | 640.36 (1) | 830.17 (14.04) | 0.77 | 4.98 | 0 | equal |
| Issue753-CPU-85_r104 | 3 | 72.70 (1) | 78.61 (1.04) | 0.92 | 3.20 | 0 | equal |
| Issue066-Project_GP8B | 3 | 423.12 (1) | 477.72 (1.29) | 0.89 | 3.97 | 7 | equal |
| Issue145-smoothieboard | 3 | 270.33 (1) | 223.36 (4.80) | 1.21 | 7.28 | 8 | equal |
| Issue219-LogicBoard_smt | 1 | 641.17 (1) | 552.20 (18.38) | 1.16 | 3.87 | 74 | equal |
| Issue420-contribution-board | 3 | 905.75 (1) | 1092.66 (29.24) | 0.83 | 3.30 | 128 | equal |

**Result** is `freerouting-dev ses-diff` of the Rust session against the Java session
(`fixtures/reference/route/<board>.ses`, routed at the same pin with the same bounds): all 16
are *equivalent*, with the same incomplete count, violations, vias, length and bends, including the
five boards on which Java's work limit fired. For the five quiet boards the same-run Java session
gives the same statistics (`bench.md`). Rust spread is two runs (the bench run and a second run for
the result comparison); Java is one run on the busy boards.

**What the numbers say.**
- The median ratio over all 16 boards is 1.35 and the geometric mean 3.3, but that is the JVM: on the
  five boards where Java takes under 4 s, 3 s of it is JVM start-up and class loading, which Rust
  does not pay. Those five rows say nothing about the router.
- On the **11 boards where Java takes at least 10 s** (routing dominates) the median ratio is **1.16**
  and the geometric mean 1.22. Summed over all 16, Rust took 3,443 s against Java's 3,182 s: **the
  port is at parity, not ahead, on real routing work.**
- **Slowest against Java:** `Issue690-kit-dev-coldfire-xilinx_5213` (0.77, 830 s against 640 s),
  `Issue420-contribution-board` (0.83), `Issue066-Project_GP8B` (0.89) and `Issue753-CPU-85_r104`
  (0.92): the large many-connection boards. Rust is ahead on `Issue093-interf_u` (4.6),
  `Issue508-DAC2020_bm10` and `Issue555-CNH_Functional_Tester_1` (1.5 each).
- **Where Rust's time goes** (callgrind of the autorouter, fanout plus 3 passes, on
  `Issue143-rpi_splitter`, 763 M instructions; `tools/bench/callgrind_top.py`):
  `Simplex::remove_redundant_lines` 8.6 %, `TileShape::intersects_tile` 7.7 %,
  `TileShape::intersects_octagon` 6.3 %, `ShapeTree::overlaps` 6.0 %, the `malloc` family about 16 %
  together (`_int_free` 5.2 %, `malloc` 3.7 %, `free` 2.5 %, `_int_malloc` 2.1 %, `realloc` 1.8 %),
  `Line::side_of_intersection` 3.6 %. Geometry predicates and allocation, as the expected-hot-spots table predicted. A
  DRC of `Issue178-KeebMaker_Sofle_Choc` is different: 70 % of its check is `BTreeMap` iteration
  (P02).

**Recommended release target.** Keep the proposal, at least 3× lower median wall time than Java at
equal or better results, but **measure it on the boards where Java takes at least 10 s**, not on all
16: the small boards' ratios (8 to 160) are Java's start-up, and a median over them would hide the
routing speed. The first parity build is at 1.16× on that set, so 3× needs about 2.6× more. The
profile gives P02 two clear targets (allocation, 16 %, and `remove_redundant_lines` scratch
vectors, 8.6 %) and P05 one (the box and octagon overlap tests, about 14 % together); neither
looks like 2.6× on its own, so P04 (the freed search tree) is probably needed to reach 3×, which
is the order the owner chose (P01, P02, P04, P03/P05). If P02's numbers show less, reconsider the number
then; set it for the default tree. **Needs the owner:** the number is recorded in
[port-completion.md](port-completion.md) "What complete means" only after he agrees.

**Re-running on a quiet machine** (stop builds and tests first; `bench` waits up to `--load-wait`
for load 1.0 and then refuses, which is the point):

```sh
cargo build --release -p freerouting-cli -p freerouting-dev
JAVA_TOOL_OPTIONS=-Xmx3g target/release/freerouting-dev bench \
    --set tools/bench/sets/benchmark.txt \
    --java ~/code/freerouting/logs/jars/freerouting-a773a70ec.jar \
    --rust "$PWD/target/release/freerouting-rs" --runs 3 --out <scratch>/bench
```

`--out` is made absolute by the bench (it was not at first: the bench runs each engine in the board's
directory, and a relative path wrote no session). A run of the
whole set is about 5 hours with 3 runs each (the six big boards take 25 to 135 minutes each); with
`--runs 1` about 1 hour 45 minutes. The optimizer-on table waits for the optimizer: Java's depends on
thread scheduling (J01), so it is compared on aggregates only; run the same command with `--optimize`
once the machine is quiet.

## Large boards in the GUI

The desktop app's renderer is its own critical path: `Issue190` alone is
350,056 triangles, and today every vertex is transformed on the CPU every
frame. That work is [G02](tasks/G00-desktop-app-README.md) (GPU buffers
uploaded once, culling), measured by frame time in release on the largest
fixture and a synthetic 16× board.
