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

- [ ] **P02.1 — Audit incremental state against a rebuild.**
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

- [ ] **P02.2 — The hot-path items from the autorouter research.**
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

- [ ] **P02.3 — Pipeline work done once.**
  - Compute board statistics once per pass, not per event.
  - Never write intermediate SES files unless a setting asks for them. Java
    writes them on events.
  - Keep the GUI's progress events, but build their payload lazily.
  - **Accept:**
    - Identical SES on `benchmark`.
    - The per-pass time on the 3 largest boards drops, with the numbers in
      the note.
    - A test shows that no intermediate file is written by default.

- [ ] **P02.4 — Immutable base, wiring overlay.**
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

- [ ] **P02.5 — Exact-key shape memo.**
  - Memoize clearance-expanded tree shapes by the exact key (shape, clearance
    class, layer). Keys use exact integers; never canonicalize translations,
    which isn't bit-safe under D6.
  - Count hits, misses and evictions, and size the memo from those counts.
  - **Accept:**
    - Identical SES on `benchmark`.
    - Wall time drops on the 3 largest boards.
    - Evictions stay under 50 % of misses. Otherwise resize it or revert.
