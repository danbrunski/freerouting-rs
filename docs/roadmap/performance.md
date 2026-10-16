# Performance

Recorded 2026-10-15. Dan wants the port to perform better than Java
(decision D1). This document says how speed is measured, what may change at
each stage of the port, and where the time is expected to go. The checklists
are the [P series](tasks/P00-performance-README.md).

## What is true today

- **Loading and DRC are already fast.** In release, loading a board plus a
  full DRC takes 0.04–0.5 s on the largest fixtures and 0.09 s on Dan's
  `canary-pwr` (measured 2026-10-15 at `b119467`). Nothing there needs work
  yet.
- **The router doesn't exist yet**, and it is where the time will go. The
  Java router took 82 s of routing plus about 5 minutes of optimizer on
  `Issue508-DAC2020_bm01` in a GUI run. Its fanout on that board ran 8 passes
  in 1.8 s.
- **The test loop is the slow part of development:** 1.5–4 min for the
  workspace in plain debug, 23 s at opt-level 1 ([F01](tasks/F01-dev-loop-and-ci.md)).
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
3. **After parity:**
   - P02: optimizations that keep results identical;
   - P03: parallelism with an ordered merge;
   - P05: SIMD where P01 shows a hot data-parallel loop.
4. **Freeing the search tree (P04).** The tree's shape decides routes today
   (D5). A faster tree first needs room completion that doesn't depend on
   visit order, for example by visiting obstacles in item-id order. That
   changes routes, so it is a measured divergence. Once it is accepted, the
   tree can be replaced by anything faster that returns the same result
   sets.

## Large boards in the GUI

The desktop app's renderer is its own critical path: `Issue190` alone is
350,056 triangles, and today every vertex is transformed on the CPU every
frame. That work is [G02](tasks/G00-desktop-app-README.md) (GPU buffers
uploaded once, culling), measured by frame time in release on the largest
fixture and a synthetic 16× board.
