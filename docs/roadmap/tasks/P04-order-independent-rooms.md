# P04 — Room completion independent of tree order, then a faster tree

Opened 2026-10-15. Decision D5 makes the search tree reproduce Java's shape
exactly, for two reasons. The 45° and 90° room completion visits obstacles in
the tree's depth-first order (`ShapeSearchTree45Degree.completeShape`). And
Java's work limits count tree nodes visited. Both tie the router's results to
the tree's internal shape, and that rules out the fastest data structures.

This checklist removes the tie on purpose, after parity:
1. Make room completion independent of visit order.
2. Count work in a unit that doesn't depend on shape.
3. Replace the tree with whatever is fastest.
Steps 1 and 2 change routes, so they are a deliberate divergence, measured
under the Q-series rules ([routing-quality.md](../routing-quality.md)).

Prerequisites: P01, P02, the A-series board-parity gate, and F03's gate
comparator.

## What exists and what this adds

| Exists | State |
|---|---|
| `ShapeTree` (`MinAreaTree` insertion strategy) | Shape-exact with Java after B02, proven by fingerprints |
| `complete_shape` (B03) | Visits obstacles in tree depth-first order, updating the room bound per obstacle |
| WorkMeter (B02) | Counts tree nodes visited, as Java does |

## Decisions made here

- **Canonical obstacle order.** `complete_shape` first collects the
  overlapping obstacles, sorts them by (item id descending, shape index),
  which is the order the result sets already use, and then restrains the room
  in that order. The room no longer depends on the tree's shape.
- **Shape-independent work unit.** Count overlap-query *results* (leaves
  returned) instead of nodes visited. Calibrate the limits so that, on the
  `benchmark` set, the same fraction of connections hit them as before.
- **The tree is free only after both changes are accepted.** Until then any
  tree change must keep fingerprints identical.

## File ownership

| Item | New files | May edit |
|---|---|---|
| P04.1 | — | `crates/freerouting/src/board/search/complete_shape.rs` (B03's module), `docs/DIVERGENCES.md` |
| P04.2 | — | `crates/freerouting/src/board/search/work.rs` (B02's module), `docs/DIVERGENCES.md` |
| P04.3 | new tree module under `crates/freerouting/src/board/search/` | the tree's callers |

## Checklist

- [ ] **P04.1 — Canonical obstacle order in room completion.**
  Behind a setting (`router.room_order = "tree" | "canonical"`, default
  `tree`), sort the obstacles before restraining. Measure under the Q rules:
  - `benchmark` plus `owner` plus the F03 corpus where available;
  - KiCad-oracle grading for boards that have a `.kicad_pcb`;
  - the F03.1 gate.
  - **Accept:**
    - With `canonical`, the router's results no longer depend on tree
      shape. A test builds the same board through two insertion orders, and
      the routes are identical.
    - The measurement table is in the done note.
    - If the gate passes (no regression, improves or is neutral on N−1 of
      at least 3 boards), `canonical` becomes the default with a DIVERGENCES
      line. If it fails, the setting is removed and the numbers stay in the
      note.

- [ ] **P04.2 — Shape-independent work unit.**
  Count leaves returned by overlap queries. Calibrate the limits so that the
  number of connections hitting a limit on `benchmark` is within 5 % of
  before. Same measurement and acceptance rules as P04.1, as a separate
  DIVERGENCES line.
  - **Accept:**
    - Two trees with different shapes but the same contents give the same
      work counts on a fixed query sample.
    - The gate result is in the done note.

- [ ] **P04.3 — A faster search tree.**
  Only after P04.1 and P04.2 are accepted. Profile with P01, then try:
  - a bulk-loaded R-tree with struct-of-arrays bounds;
  - incremental leaf updates;
  - P05's SIMD bound tests.
  The result sets must be identical (same items, same sorted order) and so
  must the routes.
  - **Accept:**
    - `ses-diff` identical on `benchmark` against P04.2's build.
    - Wall time and callgrind counts are given before and after.
    - The P02.1 audit is clean.
