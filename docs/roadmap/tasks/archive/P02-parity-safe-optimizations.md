# P02 — Optimizations that keep results identical

Opened 2026-10-15. After board parity, every speed-up here must leave the
routed result byte-identical. That covers `ses-diff` on the `benchmark` set,
the A-series probe logs, and D5's work counts. The candidates come from three
sources:
- the autorouter research (`FoundConnectionInserter`'s always-on trace
  strings, the maze queue, door-shape caching);
- the pipeline research (statistics once per pass, no per-event SES writes);
- KiCadRoutingTools' Rust core (an audit of incremental state, an immutable
  base with a wiring overlay, exact-key memos).
Item P02.1 is a safety net and comes first.

Prerequisites: P01 (measurements), the A-series board-parity gate, and B01
(snapshots) for P02.4.

## What exists and what this adds

| Exists | State |
|---|---|
| Incremental search trees | Updated on every insert and remove (B02), and for rooms (A02) |
| Board copies | B01 snapshots; the A-series board history; O06's copy-on-write candidates |
| Java's per-event work | `FoundConnectionInserter` builds trace strings unconditionally (`if (true) FRLogger.trace(...)`, cda540f lines 79, 251, 311, 389); the pipeline writes intermediate SES on events |

## Decisions made here

- **Each item is measured alone.** A done note gives before/after wall time
  (median of 5, release, quiet), the callgrind count change and `ses-diff`
  "identical" on the `benchmark` set. An item with no measurable gain is
  reverted, and its numbers stay in the note.
- **The audit first.** P02.1 compares incremental state with a from-scratch
  build. Every later caching item runs with it on in its tests, because a
  cache that drifts produces wrong routes, not slow ones. KiCadRoutingTools
  found balanced reference counts while 30,862 of 47,448 committed cells were
  not blocked (its #806). Balance alone is not enough.

## File ownership

| Item | New files | May edit |
|---|---|---|
| P02.1 | `crates/freerouting/src/board/search/audit.rs` | `crates/freerouting/src/autoroute/` (call sites behind a debug setting) |
| P02.2 | — | `crates/freerouting/src/autoroute/` (queue, doors, insertion) |
| P02.3 | — | `crates/freerouting/src/autoroute/` (pipeline statistics and events) |
| P02.4 | — | `crates/freerouting/src/board/` (snapshot representation) |
| P02.5 | `crates/freerouting/src/board/search/memo.rs` | the search-tree query paths |

## Checklist

- [~] **P02.1 — Audit incremental state against a rebuild.**
  Behind a debug setting (`router.debug.audit_incremental`, off by
  default), after each ripup and restore and at the end of each pass:
  - rebuild the default tree and each autoroute tree from the current board
    in Java's insertion order;
  - compare *contents*, not shape: the result sets of a fixed query sample
    (every item's bounding box on every layer) and a content digest of each
    tree's entries. An incrementally updated tree legitimately has a
    different shape from a fresh build (D5: Java's insert/remove history
    decides the shape), so B02's shape fingerprints can't be compared with a
    rebuild;
  - report the first difference with its `ItemId`, layer and tree.
  - **Accept:**
    - The `benchmark` set routes with the audit on and 0 differences.
    - A seeded fault (one skipped tree removal in a test build) is reported
      with its `ItemId` and layer.
    - With the audit off, the SES is identical and wall time is within noise.
  *Partial 2026-10-17:* `crates/freerouting/src/board/search/audit.rs` (`IncrementalAudit`,
  `audit_board`), switched on by `router.debug.audit_incremental` (a new non-Java `debug`
  settings section, unset = off, so Java's settings dumps don't see it; `DIVERGENCES.md`
  settings section). `PassRunner` audits after each connection that ripped or failed and at
  each pass end; `batch_loop` after each restore, loop ripup and the final tail removal; the
  pipeline logs the tally and the first difference (`PipelineOutcome::audit`). Each check
  compares every tree (default and autoroute trees) three ways, through `ShapeTree::depth_first`
  so the work meter (D5) is never touched: entries against a fresh shape calculation, a sorted
  leaf digest against a rebuild in Java's insertion order (item list order, rooms by id), and
  every on-board item's bounding box as a query on both. Finding: Java's own incremental trees
  differ from a rebuild in three legitimate ways (kept leaves of reversed merges, kept ±1-unit
  rounded leaves from `changeEntries`, and `reduceTraceShapeAtTiePin` end shapes), so those are
  tolerated and counted (operator decision in the report); the leaf and query comparisons use a
  rebuild from the stored shapes. Accept clauses:
  - `benchmark` with the audit on: all 16 boards, 0 differences (3 to 444 checks per board, at
    most 2 rounded shapes tolerated), gate EXIT 0, all 16 SES byte-identical to the audit-off
    gate's (`PARITY_SET=tools/bench/sets/benchmark.txt PARITY_AUDIT=1 cargo test --release -p
    freerouting --test autoroute_board_parity -- --ignored autoroute_board_parity
    --test-threads=1`, 2,435 s, base `8e0cdf2`; log `target/orchestrator/p02/gate-audit.log`).
    `connection-parity` (10 sessions, `route_board -- --router.debug.audit_incremental=true`):
    0 differences, SES identical to HEAD's. `tests/search_audit.rs`
    (`audit_is_clean_and_changes_nothing_on_a_routed_board`) pins audit-on = audit-off on
    CONGFLOCK.
  - Seeded fault: `board::search::audit` unit tests
    `skipped_tree_removal_is_reported_with_item_and_layer` (one skipped removal in a test
    build, `SearchTreeManager::remove`'s `cfg(test)` hook: reported as `StaleItem` with the
    `ItemId`, layer and tree), `orphaned_leaf_of_a_skipped_removal_is_reported`,
    `moved_trace_without_tree_update_is_reported`, plus `loaded_and_edited_board_audits_clean`
    and `exact_shape_equality_tells_simplexes_apart` (5 pass).
  - Audit off: the A14 gate (`PARITY_SUBDIRS=1`, base `25b0f2e` + P02, all three P02.2 steps in)
    passes on all 140 boards (135 byte-identical to Java, D8 breaches 0, 4,875 s) and its SES are
    identical to HEAD's on all 139 boards that write one (136 against a `b502cc0` gate cut short
    by a reboot, 3 against a targeted `25b0f2e` run). Callgrind with the audit off, P02.1 alone
    against `b502cc0`: route +0.390 % / +0.099 % / +0.078 % (rpi_splitter / my-board /
    SMD-demo), kernels +0.49 % at most (codegen; the kernels never reach the audit).
  Remainder: the quiet-machine wall time (median of 5) with the audit off; the machine stayed
  at load 2 to 13 throughout. Rerun `freerouting-dev bench --set tools/bench/sets/benchmark.txt
  --rust target/release/freerouting --java <pinned jar> --runs 5 --out <dir>` on `b502cc0` and on P02.1 alone.

- [~] **P02.2 — The hot-path items from the autorouter research.**
  - **Queue.** Replace the maze queue's `BTreeSet` with a
    `BinaryHeap<Reverse<Key>>` plus a set of the keys currently queued. Pop
    order and the drop-on-equal rule must be the same as Java's (A04).
  - **Door shapes.** Cache door shapes and section segments per room,
    invalidated on `set_shape` (45° completed rooms never change shape).
  - **Scratch reuse.** Reuse `complete_shape`'s scratch vectors and room
    octagons, so the hot loop doesn't allocate.
  - **Trace strings.** Never build trace-log strings unless trace logging is
    on.
  - **Distances.** Precompute `DestinationDistance` per layer group.
  Each sub-item is measured separately.
  - **Accept:**
    - Per sub-item: `ses-diff` identical on `benchmark`, probe logs
      identical on `connection-parity`, and work counts identical.
    - Each sub-item's wall-time and callgrind change is in the done note.
      Sub-items with no gain are reverted.
  *Partial 2026-10-17:* the profile chose the work, not the research list. Callgrind
  (`tools/bench/callgrind.sh`, release, `taskset -c 0`, counts deterministic) of the
  autorouter on `Issue093-interf_u` (12.4 G instructions, inclusive): `queue_element` 0.1 %,
  `section_segments` 0.1 %, `DestinationDistance::calculate` 0.0 %; trace strings were never
  ported (`autoroute::batch::pass` docs: "Not ported: Java's trace logging"). So **Queue, Door
  shapes, Distances and Trace strings were not implemented**: there is nothing to gain. What the
  profile showed instead (`Issue143-rpi_splitter`: `remove_redundant_lines` 8.6 %,
  `intersects_tile` 7.7 %, `intersects_octagon` 6.3 %, `malloc` family 16 %) became three
  *Scratch reuse* steps, each measured alone with `callgrind.sh --compare` against the step
  before (route = `run_batch_loop`, fanout + 3 passes):
  1. `TileShape::intersects_tile` dispatches on the two shapes directly instead of cloning both
     into a `Shape` (`freerouting-geometry/src/shapes/tile_shape.rs`): route rpi_splitter
     -9.21 %, my-board -10.40 %, SMD-demo -8.89 %; kernels overlap -25.49 %, drc -21.88 %,
     pull_tight -11.93 %, maze -7.82 %, complete_shape +0.00 %.
  2. The simplex emptiness tests (`Simplex::intersects_simplex`, new
     `intersects_octagon_simplex` and `intersects_box_simplex`, used by `TileShape::intersects_*`)
     run `remove_redundant_lines`' steps on a stack buffer instead of building three simplexes
     (`shapes/simplex.rs`: `dedup_lines`, `reduce_redundant_lines`, `lines_intersect`; the same
     lines in the same order, so the same answer): route -5.99 % / -2.51 % / -2.04 %; overlap
     -6.84 %, drc -5.48 %, maze -2.82 %, pull_tight -2.88 %, complete_shape -0.00 %.
  3. `ShapeTree::overlaps` tests octagon-in-octagon and box-in-box nodes directly
     (`datastructures/shape_tree.rs::bounds_intersect`, the calls `intersects_tile` makes):
     route -3.20 % / -3.12 % / -2.95 %; overlap -9.62 %, drc -8.35 %, maze -2.29 %, pull_tight
     -3.74 %, complete_shape +0.00 %.
  Together against HEAD (`target/orchestrator/p02/cg-final.json`, base `25b0f2e`): route
  -16.64 % / -15.07 % / -12.94 %, kernels overlap -37.26 %, drc -31.51 %, pull_tight -17.13 %,
  maze -12.25 %; complete_shape +0.75 % comes from the commits merged in between (Q06, X02,
  Q05: it was +0.00 % after step 3 on `b502cc0`). Work counts unchanged
  (`cargo test -p freerouting-dev --test microbench`, both fixtures). Bit-identity: the scalar
  paths stay the reference in `crates/freerouting-geometry/tests/simplex_predicates.rs` (25,000
  random simplex/octagon/box triples on a 25-unit grid and 5,000 near ±4·10⁸, against
  `!intersection_simplex(..).is_empty()` and `intersects(&Shape)`); the A14 gate after all three
  steps (P02.1's note) is byte-identical to HEAD on all 140 boards, which covers `benchmark`; the
  10 `connection-parity` sessions are byte-identical to HEAD's (`route_board`, bounds from
  `route-bounds.tsv`). Remainder: the quiet-machine wall times (median of 5) per step. The
  machine was never below load 1.0 (2 to 13 during this work); rerun with
  `freerouting-dev bench --set tools/bench/sets/benchmark.txt --rust target/release/freerouting
  --java <pinned jar> --runs 5 --out <dir>` at each step's tree. The only wall figure is the gate's: Rust route time summed over
  the 137 boards both gates routed, 5,307 s at HEAD (load 5 to 7) against 3,841 s with P02 (load
  2 to 5), which says nothing reliable.

- [~] **P02.3 — Pipeline work done once.**
  - Compute board statistics once per pass, not per event.
  - Never write intermediate SES files unless a setting asks for them. Java
    writes them on events.
  - Keep the GUI's progress events, but build their payload lazily.
  - **Accept:**
    - Identical SES on `benchmark`.
    - The per-pass time on the 3 largest boards drops, with the numbers in
      the note.
    - A test shows that no intermediate file is written by default.
  *Partial 2026-10-17:* all three already hold in the port, so there was no code to change:
  - statistics once per pass: in the callgrind of `Issue093-interf_u` (18 passes)
    `BoardFigures::of` runs 20 times (each pass, the baseline and the finish), from
    `run_batch_loop` only;
  - no intermediate session: `crates/freerouting-cli/tests/no_intermediate_files.rs`
    (`a_routing_job_writes_no_intermediate_files`: a three-pass route leaves exactly the design
    and `out.ses`). At `a773a70ec` Java writes none either: `save_intermediate_stages` only
    fires a board-snapshot event (`AutorouteBatchLoop.java:468-470`), which the port does not
    send (see the report's observation);
  - lazy progress payload: `ProgressState` computes the statistics only while someone listens
    (`autoroute::batch::pass` docs).
  SES identical on `benchmark`: nothing changed here (the gate in P02.1's note). Remainder: the
  "per-pass time drops" clause cannot be met by this item, because there is no per-event work
  to remove. The per-pass statistics themselves are expensive: 20.4 % of the `interf_u` route
  (`all_clearance_violations` 9.7 %, `unconnected_set_cached` 7.3 %, `calculate_bounds` 4.0 %),
  a whole DRC per pass as in Java. Making them incremental or parallel is P03.1's (or a new P02
  item), an operator decision in the P02 report.

- [~] **P02.4 — Immutable base, wiring overlay.**
  - Split the board into an `Arc`-shared base and a mutable overlay. The
    base holds the outline, keepouts, fixed and locked items, pins and their
    tree entries. The overlay holds routed wiring.
  - Snapshots, the best-board copy and optimizer candidates clone only the
    overlay.
  - Restores must still reset exactly what B01 says a Java restore resets.
  - **Accept:**
    - Identical SES on `benchmark`, and the P02.1 audit is clean.
    - Copy time and peak RSS are given before and after on the 3 largest
      boards.
    - Copy time drops by at least 2×; otherwise revert.
  *Partial 2026-10-17:* the base/overlay split as written cannot keep D5: Java's `readObject`
  inserts every item highest id first, so a restore's default tree interleaves wiring (newer,
  higher ids) and fixed items, and a tree of the base alone cannot be extended into that shape.
  What a shared immutable base buys under D5 is a **restore memo** (`board/snapshot.rs`,
  "Repeated restores"): a restore is a pure function of the snapshot, so the second restore of a
  snapshot (or of its clones) keeps a compact clone of the board it built, and every later
  restore clones it instead of rebuilding the tree. The optimizer restores its pass baseline once
  per candidate (O06.4), which is where copies cost. The first restore builds as before, so a
  snapshot restored once (history, strict DRC, `deep_copy`) costs and keeps nothing more;
  `BoardSnapshot::rebuild` always builds. Not done: `Arc` for rules, library, components and
  layer structure (B01.4's remainder; they are part of the clone below, and the 2× holds without
  them). Restores reset exactly what B01 says: `tests/snapshot_memo.rs`
  (`repeated_restores_equal_a_rebuild`: restores 1 to 3 against `rebuild()` on J2 with its
  session, equal `state_hash`, default-tree walk and content digest, tree count, meter count,
  revision 0, compensation off, edge-pin cache and ledger unbuilt, P02.1 `audit_board` clean;
  `memoized_restore_counts_from_the_snapshot_meter`; `memoized_restore_routes_like_a_rebuild`:
  my-board routed with the optimizer from a memoized restore and from a rebuild, equal SES, pass
  records, summary and optimizer summary), plus B01.3's `tests/board_snapshot.rs` (13 pass).
  Copy time on the 3 largest fixtures by item count (`examples/snapshot_bench.rs`, new
  `re-restore` and `memo bytes` columns, release, median of 7, load 5.5): build / repeated
  restore Issue190-processor.Z80 8.49 ms / 0.41 ms, Issue054-tairakb 8.22 / 0.19,
  Issue690-kit-dev-coldfire 6.62 / 0.23; callgrind of the same bench, instructions per call:
  116.15 M / 8.7 M (13×), 119.66 M / about 2.6 M (about 46×), 92.17 M / 4.38 M (21×);
  `snapshot()` unchanged (185 / 60 / 93 µs). Peak RSS of the bench process (`/usr/bin/time`),
  HEAD → this tree: 25.6 → 33.4 MB, 17.5 → 24.0 MB, 15.9 → 21.2 MB: a memoized snapshot keeps one
  board (heap kept with the snapshot 5.83 / 5.35 / 3.39 MB). Optimizer routes
  (`--router.optimizer.enabled=true`, 3 passes): peak RSS Issue508-DAC2020_bm07 38.5 → 38.8 MB,
  Issue508-DAC2020_bm02 27.0 → 25.1 MB; with P02.5 too, bm07 38.8 → 40.1 MB, bm01 54.1 →
  57.5 MB. Callgrind of the bm07 optimizer route (one thread):
  61.08 G → 59.46 G instructions (−2.65 %; `BoardSnapshot::restore` was 2.86 % of it); plain
  routes (`callgrind.sh`'s route entries, `run_batch_loop`) move by codegen only: rpi_splitter
  −0.02 %, SMD-demo −0.07 %, my-board −0.11 %, bm07 −0.03 %, interf_u −0.19 %. SES: identical
  to HEAD's with the optimizer on (9 `benchmark` boards up to Issue508-DAC2020_bm01, the
  optimizer on its default threads; and the one-thread bm07 callgrind run). The `benchmark` gate
  with the audit on is clean and byte-identical: see P02.5's note (one gate for both items).
  Remainder: the quiet-machine wall time (median of 5) of `snapshot_bench` and of an optimizer
  route; the load stayed at 4 to 6.

- [~] **P02.5 — Exact-key shape memo.**
  - Memoize clearance-expanded tree shapes by the exact key (shape, clearance
    class, layer). Keys use exact integers; never canonicalize translations,
    which isn't bit-safe under D6.
  - Count hits, misses and evictions, and size the memo from those counts.
  - **Accept:**
    - Identical SES on `benchmark`.
    - Wall time drops on the 3 largest boards.
    - Evictions stay under 50 % of misses. Otherwise resize it or revert.
  *Partial 2026-10-17:* `crates/freerouting/src/board/search/memo.rs` (`ShapeMemo`,
  `MemoCounts`): a trace's tree shapes keyed by its polyline's lines (their defining integer
  points, compared exactly, not by Java's geometric `Line.equals`), the total offset (half width
  plus the tree's compensation for the trace's class on its layer) and the box kind of a 90 degree
  tree; nothing is canonicalized (D6). Two generations of 16,384 polylines; a board's trees, its
  copies, snapshots and restores share one memo behind a mutex (it stores a pure function, so a
  hit or which thread fills it never changes a result). The first version memoized every trace
  shape calculation and failed the eviction rule on the 3 largest `benchmark` boards (routed
  with their gate bounds, one thread; SES identical to P02's gate): Issue145-smoothieboard
  825,160 hits / 144,339 misses / 121,163 evictions (84 %), Issue219-LogicBoard_smt 482,478 /
  184,226 / 157,001 (85 %), Issue420-contribution-board 256,420 / 280,424 / 259,945 (93 %); the
  misses are new and candidate polylines that never come back. Resized by scope instead of
  capacity: only whole-tree builds use the memo (`insert_memoized`: a restore's build, an
  autoroute tree, a compensation rebuild); single insertions and other shape calculations
  compute. Measured since: 0 evictions everywhere (`examples/route_board` prints the counts):
  Issue508-DAC2020_bm07 with the optimizer 57,050 hits / 579 misses, Issue143-rpi_splitter with
  the optimizer 611 / 74, Issue093-interf_u 653 / 653 (its history restores), plain routes
  without restores 0 lookups. Also, `tree_shape` and `tree_shape_count` of a trace outside the
  tree compute only the segment asked for or the count, where they computed every shape (the
  same values; this was most of the first version's hits). Tests: `board::search::memo` unit
  tests (`a_hit_returns_the_computed_shapes`, `keys_are_exact`: another offset, the box kind, a
  translated copy and the same line through other points all miss,
  `a_full_generation_ages_and_old_hits_move_back`); `tests/shape_memo.rs`
  (`memoized_trees_equal_uncached_builds`: on J2 with its session the memoized default and
  compensated trees equal uncached builds walk for walk and shape for shape;
  `restores_share_the_board_memo`). Callgrind, against P02.4 (instructions): plain routes
  (`run_batch_loop`, 3 passes) rpi_splitter −0.25 %, SMD-demo −0.39 %, my-board +0.12 %, bm07
  −0.45 %, interf_u −0.06 %; the bm07 optimizer route −1.62 %. Split with a build whose memo is
  off: the memo alone 0.00 % (bm07), −0.16 % (interf_u), −0.82 % (bm07 optimizer); the direct
  `tree_shape` −0.45 %, +0.10 %, −0.80 %. P02.4 and P02.5 together against HEAD: bm07 optimizer
  route 61.08 G → 58.50 G (−4.22 %), plain bm07 −0.47 %, interf_u −0.25 %, rpi_splitter
  −0.27 %, SMD-demo −0.46 %, my-board +0.02 %. SES identical to HEAD on every callgrind run.
  `benchmark` with the audit on, both items in (`PARITY_SET=tools/bench/sets/benchmark.txt
  PARITY_AUDIT=1 cargo test --release -p freerouting --test autoroute_board_parity -- --ignored
  autoroute_board_parity --test-threads=1`, base `94b344f`): EXIT 0, 16 of 16 pass (10 exact,
  6 cutoff), 16 SES byte-identical to Java, no audit difference (one fails its board), 2,375 s;
  the 16 SES are byte-identical to P02.1's audit gate's. Gate route times (audit on, load 3 to
  8, not comparable): Issue420 652.3 s (P02.1's gate 658.1 s), Issue219 390.1 s (408.0 s),
  Issue145 161.7 s (185.7 s).
  Remainder: "wall time drops on the 3 largest boards" is not shown: their gate routes rebuild
  trees only on history restores, so the memo saves little there; the gain is in optimizer runs
  and boards with restores (O06.4). Quiet-machine wall times (median of 5) are owed for P02.4
  and P02.5 as for P02.1 to P02.3, and whether to keep the memo for a gain of under 1 % is an
  operator decision (report).
