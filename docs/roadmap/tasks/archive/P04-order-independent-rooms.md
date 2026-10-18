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
under the Q-series rules ([routing-quality.md](../../routing-quality.md)).

**Position (owner, 2026-10-16):** P04 runs right after P02 and before P03,
P05 and every route-changing Q item, so the tree is freed once and every
later measurement is paired against the freed tree. See P00 "Order".

Prerequisites: P01, P02, the A-series board-parity gate (rung 3), and F03's
gate comparator.

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
- **Both trees stay (owner, 2026-10-16).** The Java-shaped tree with Java's
  work unit is not replaced; it becomes a selectable implementation,
  `router.search_tree = "java" | "fast"` (one key beside P04.1's
  `router.room_order`), as D9 does for stages. `java` is the parity mode:
  the board-parity gate (A14), the tree fingerprints (B02), the
  complete-shape and maze-log probes (B03, A04) and `connection-parity`
  keep running on it, bit-exact against Java, for as long as the port
  exists. `fast` becomes the default only when P04.3's measurement accepts
  it. A change to shared code (room completion, the maze search, the
  shover) must keep the `java` mode's probes identical; a change to the
  fast tree must keep `ses-diff` identical against its own previous build.
  The cost is one extra implementation of the tree trait; the parity tree is
  the one the port already has.

## File ownership

| Item | New files | May edit |
|---|---|---|
| P04.1 | — | `crates/freerouting/src/board/search/complete_shape.rs` (B03's module), `docs/DIVERGENCES.md` |
| P04.2 | — | `crates/freerouting/src/board/search/work.rs` (B02's module), `docs/DIVERGENCES.md` |
| P04.3 | new tree module under `crates/freerouting/src/board/search/` | the tree's callers |

## Checklist

- [x] **P04.1 — Canonical obstacle order in room completion.**
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
  *Done 2026-10-17 (rejected: the gate failed, the setting and its code are
  removed, the numbers stay here):*
  - **What was measured.** `router.room_order = tree | canonical` on the 45° and
    90° completions. The canonical order is (distance of the leaf's bounding
    shape from the room's contained shape, then the leaf's key, then its shape
    index), walked best first (`ShapeTree::best_first`, a min-heap; the
    distance is the largest octagon or box gap in exact `i64` and never shrinks
    from a node to its children, so pruned subtrees hold no leaf that would meet
    the only-shrinking bound at its turn). This deviates from the checklist's
    "sort by key" on purpose: key order must collect every leaf that meets the
    start shape, usually the whole board on all layers. On 300 seeded
    completions on Issue145-smoothieboard (tree of 2,736 leaves) best first pops
    mean 360 / median 320 nodes, Java's depth-first order 624 / 555, and the key
    order would collect mean 1,725 / median 2,736 leaves before it starts. The
    implementation is kept outside the repository as
    `target/orchestrator/p04/p04-rejected-canonical-leaves.patch`; it applies
    cleanly to `93c326f`.
  - **Order independence (Accept 1), tests in that patch.**
    `tests/search_order_independence.rs::canonical_routes_do_not_depend_on_tree_shape`
    routes Issue433-my-board and CONGFLOCK twice, the second time with every
    whole-tree build in reversed item order (test hook
    `SearchConfig::reversed_builds`, so the trees differ in shape). Both runs
    give equal SES and pass records with `canonical`, in both work units; with
    `tree` my-board's routes differ. The `complete_shape` unit test
    `canonical_order_is_the_sorted_fold_and_ignores_insertion_order` covers 60
    random trees × {45°, 90°}: two insertion orders give equal rooms and leaf
    counts, and best first equals the sorted fold by definition.
    `the_gap_never_shrinks_inside_a_bound` checks the distance (2,000 random
    pairs, plus the coordinate limits). Java mode was unchanged with the patch
    in: `board_probe_complete_shape`, `board_probe_tree`,
    `board_probe_tree_visits`, `autoroute_probe_maze` and `work_limit` pass,
    and all 21 SES of the `tree` arm are byte-identical to
    `fixtures/reference/route` (Java `a773a70ec`).
  - **Measurement.** Preregistration `docs/roadmap/prereg/P04.1.json` (sha256
    `237311ec…01f9b9`, written before the first number, 0 amendments; both P04
    registrations are pinned by `crates/freerouting-dev/tests/prereg_p04.rs`). Set
    `tools/bench/sets/p04.txt`: the 16 `benchmark` boards, plus the five
    in-repository canary-pwr variants counted as one board. Run with
    `freerouting-dev bench --rust <wrapper> --runs 1 --max-load off` (Rust
    only, release, route bounds, optimizer off, one run, D2). The load was 2.6
    to 12, so wall times are not comparable. Unrouted (incomplete) tree →
    canonical: my-board 0 → 0, ecc83 0 → 0, bm02 0 → 0, sonde 11 → 11, bm07
    0 → 0, bm01 24 → 26, bm10 0 → 0, interf_u 19 → 19, CNH 1 → 1, z10 37 → 37,
    coldfire 181 → 181, CPU-85 57 → 64, GP8B 61 → 51 (clearance violations
    0 → 2), smoothieboard 19 → 28 (clearance 210 → 211), LogicBoard 285 → 274,
    contribution-board 57 → 43, canary-pwr pours1 120 → 120, pours2 116 → 121,
    pours3 115 → 119, pours-as-planes 71 → 68, unpoured 107 → 106 (the
    variants sum to 529 → 534). Totals: unrouted 1,281 → 1,269, vias 5,102 →
    5,057, length 113,870 → 113,538 mm, Rust wall time 3,300 → 2,861 s
    (noisy). Search work counted by `--stats json` over the set: maze work
    units 39.1 G → 19.6 G nodes, tree nodes visited 247.2 G → 228.7 G. Work
    limits hit: 1 maze search and 3 fanout pins in both arms.
  - **F03.1 gate** (`freerouting-dev gate --records … --prereg
    docs/roadmap/prereg/P04.1.json --arms tree,canonical`, log
    `target/orchestrator/p04/runs/p04.1-gate.txt`): **FAIL**, exit 1. It
    improved 2 of 17 boards (LogicBoard, contribution-board), regressed 6
    (canary-pwr; GP8B on the clearance guard; smoothieboard; bm01; CPU-85;
    coldfire), and left 9 neutral. Coldfire's regression, like smoothieboard's
    HARD line, comes from `via_in_pad` 6 and 42 that the `tree` arm (= Java)
    has too. The luck rate at N=17 is 1 in 131,072. P04's own rule counts a
    neutral board with the improved ones and still needs no regression: 5
    boards regress (bm01, CPU-85, GP8B, smoothieboard, canary-pwr), so it
    fails too.
  - **KiCad oracle** (F02.6, KiCad 10.0.7, `grade.sh` per project, each arm at
    `max_passes=5`, optimizer off), drc_real and unconnected tree → canonical:
    ecc83 0 → 0 and 0 → 0; interf_u 9 → 11 and 17 → 20; kit-dev-coldfire
    235 → 213 and 85 → 77; canary-pwr (rev1.0) 39 → 38 and 71 → 70; sonde
    9 → 9 and 3 → 3. interf_u regresses on the drc_real guard as well.
    `grade-set.sh` stopped after its first project, so the projects were
    graded one by one with stdin from `/dev/null`.
  - **Not run:** the F03 corpus. More boards cannot undo a regression that
    already exists. Outcome: `router.room_order` is removed, no DIVERGENCES
    line is added, and the default stays Java's tree order.

- [x] **P04.2 — Shape-independent work unit.**
  Count leaves returned by overlap queries. Calibrate the limits so that the
  number of connections hitting a limit on `benchmark` is within 5 % of
  before. Same measurement and acceptance rules as P04.1, as a separate
  DIVERGENCES line.
  - **Accept:**
    - Two trees with different shapes but the same contents give the same
      work counts on a fixed query sample.
    - The gate result is in the done note.
  *Done 2026-10-17 (rejected: the gate failed, and P04.1 was rejected too; no
  code is left, the numbers stay here):*
  - **What was measured.** `router.work_unit = nodes | leaves`, in the same
    patch as P04.1 (`target/orchestrator/p04/p04-rejected-canonical-leaves.patch`).
    `WorkMeter` got a unit. Under `leaves`, an overlap query counts the leaves
    it returns, and a room completion counts the leaves that meet the bound at
    their turn. A `WorkLimit` allows `ms × 3,000` leaves.
  - **Calibration**, the rule registered before the arm ran: the canonical
    arm's `--stats json` over the set gave 11.05 G leaves returned per
    228.7 G nodes visited, a ratio of 0.0483. 64,000 × 0.0483 = 3,091, rounded
    to 3,000. Limit hits on the set: maze search 1 → 1, fanout pins 3 → 3,
    which is within 5 %.
  - **Accept 1:** `tests/search_order_independence.rs::leaf_counts_and_canonical_rooms_do_not_depend_on_tree_shape`
    (in the patch) runs 300 seeded overlap queries and 300 seeded room
    completions on my-board's autoroute tree, 45° and 90°, built in item order
    and in reversed order (the fingerprints differ). With `leaves` and
    `canonical`, every query has equal units and equal results. With `nodes`
    and `tree` the units differ, and the overlap results are equal in every
    mode.
  - **Measurement:** preregistration `docs/roadmap/prereg/P04.2.json` (sha256
    `187cad44…7d6f88c88`, 0 amendments), the same set and harness as P04.1.
    The arms were `canonical` (nodes) and `canonical-leaves`. 15 of 21 rows
    have equal statistics. The differences, unrouted canonical → leaves:
    contribution-board 43 → 37; canary-pwr pours1 120 → 121, pours2
    121 → 128, pours3 119 → 121, pours-as-planes 68 → 74, unpoured 106 → 106
    (the variants sum to 534 → 550). Totals: unrouted 1,269 → 1,279, vias
    5,057 → 5,060, length 113,538 → 112,759 mm. Maze work units were
    784.8 M leaves.
  - **F03.1 gate** (`--prereg docs/roadmap/prereg/P04.2.json --arms
    canonical,canonical-leaves`, log `target/orchestrator/p04/runs/p04.2-gate.txt`):
    **FAIL**, exit 1. It improved 1 of 17 boards (contribution-board) and
    regressed 3: canary-pwr, plus smoothieboard and coldfire on the
    `via_in_pad` HARD line, which both arms share. 13 boards were neutral. P04's
    rule: canary-pwr regresses, so it fails.
  - **KiCad oracle**, drc_real and unconnected canonical → leaves: ecc83
    0 → 0 and 0 → 0; interf_u 11 → 11 and 20 → 20; kit-dev-coldfire 213 → 235
    and 77 → 81; canary-pwr 38 → 38 and 70 → 70; sonde 9 → 9 and 3 → 3.
  - **Outcome:** `router.work_unit` is removed, no DIVERGENCES line is added,
    and the work unit stays Java's nodes visited (D5).

- [~] **P04.3 — A faster search tree.**
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
    - `router.search_tree = "java"` still passes A14's exact-parity set and
      the B02, B03 and A04 probes unchanged; `tools/determinism-check.sh`
      passes in both modes; the CLI logs which tree ran.
  *Partial 2026-10-17:* not started. The item runs "only after P04.1 and P04.2
  are accepted", and both were rejected. The default-tree decision is therefore
  **Java's tree** (`MinAreaTree` shape, depth-first room order, nodes-visited
  work unit, D5). `router.search_tree` is not added, so no Q or X measurement
  needs a re-baseline. Remainder, an operator decision for the owner, owned by
  P00 (the series' open questions):
  - relax P04's pass rule for a divergence made for speed, for example by
    judging totals, or a noise band taken from the canary-pwr variants,
    instead of "no board regresses". The measured change is a coin flip per
    board: total unrouted fell by 12, while 5 boards regressed and 2 improved;
  - or register another canonical definition, such as the checklist's key-only
    order (costly, see P04.1);
  - or close P04.3.
  Either way the patch above and `tools/bench/sets/p04.txt` with the two
  registrations let the run be repeated (about 50 minutes per arm on this
  machine).
