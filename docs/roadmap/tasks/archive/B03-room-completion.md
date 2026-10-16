# B03 — Free-space room completion

Opened 2026-10-15. Room completion grows an incomplete expansion room into the
largest free room around a shape it must contain, restrained by the
obstacles in the autoroute search tree. It is the heart of Java's
expansion-room algorithm. Java keeps it in `board/searchtree`, but only the
autorouter calls it (`AutorouteEngine.java:466,517`,
`Sorted45DegreeRoomNeighbours.java:400`, `SortedOrthogonalRoomNeighbours.java:548`).
The 45° and 90° variants walk the tree depth-first while the bound shrinks, so
their result depends on the tree shape that B02 makes exact (D5). This
checklist ports the three variants and their restrain helpers. It defines the
minimal geometric room types in `board::search::rooms`, which the A series
wraps with doors and costs (A02), and proves every variant with a Java
`complete-shape` probe.

**Prerequisites:** B02.1 (rooms in the tree), B02.2 (the tree probe passes on
`load` and `autoroute`), B01.1. B02.7 (the work meter) for B03.7, whose probe
runs at `30baad7e2`.

Size L, risk high: subtle geometry and floating-point tie-breaks, and the
result shapes every route. Suggested split: B03.1–B03.4, then B03.5–B03.7.

## What exists and what this adds

| Rust today (`crates/freerouting/src/`, `70389b7`) | Java at `cda540f` (`src/main/java/app/freerouting/`) |
|---|---|
| `board/search/mod.rs` doc says room completion is not ported (`:5-9`). `TreeKind::{Plain, FortyfiveDegree, NinetyDegree}` computes tree shapes for all three; `get_autoroute_tree` picks the kind from the angle restriction (`:736-760`) | `ShapeSearchTree.completeShape` (`:668-805`), `restrainShape` (`:806-920`), `divideLargeRoom` (`:1220-1244`) |
| — | `ShapeSearchTree45Degree`: `obstacleSegmentTouchesInside` (`:40`), `signedLineDistance` (`:69`), `completeShape` (`:98-305`), `divideLargeRoom` override (`:308-322`), `restrainShape` (`:324-442`), `calcOutsideRestrainedShape` (`:443-476`), `calcInsideRestrainedShape` (`:477-506`); debug tracing `:562-667` (drop it) |
| — | `ShapeSearchTree90Degree`: `completeShape` (`:41-216`), `restrainShape` (`:217-342`); debug tracing `:343-453` (drop it) |
| — | Room classes: `autoroute/expansion/IncompleteFreeSpaceExpansionRoom.java` (shape, layer, contained shape; a `null` shape is the whole plane), `CompleteFreeSpaceExpansionRoom.java` (B02.1's tree object) |
| Geometry in `freerouting-geometry`: `IntOctagon` (`normalize`, `overlaps`, `intersection`, `union`), `IntBox`, `TileShape::{intersection, dimension, contains, divide_into_sections, bounding_octagon}`, `Simplex` | `geometry.planar` (ported and checked against Java) |

## Decisions made here

- **Room types are geometry only.** `board/search/rooms.rs` holds
  `IncompleteRoom { shape: Option<TileShape>, layer: i32, contained:
  Option<TileShape> }` (Java allows a `null` contained shape and warns) and
  `RoomId`, re-exported from B02.1. Doors, costs, net dependence and room ids
  live in the A series' arenas (A02). Room completion doesn't depend on A.
- **One entry point per board:** `BasicBoard::complete_shape(tree_class, room,
  net, ignore: Option<TreeObject>, ignore_shape: Option<&TileShape>) ->
  Vec<IncompleteRoom>`. It dispatches on the tree's `TreeKind`, like Java's
  virtual call.
- **No allocation in the hot loop.** A reusable `CompletionScratch` (the DFS
  stack, the result vectors) is passed in or kept per thread by the caller.
  Java's `completeShapeStack` is a reused `ThreadLocal` for the same reason.
- **Exceptions are results.** Java returns an empty list after warnings for
  a missing contained shape, a wrong shape type or an empty tree, and the
  engine catches exceptions as failure (`AutorouteEngine.java:540-543`). Rust
  returns the same empty list at the same points. The probe records Java
  exceptions, so any that occur are reproduced on purpose.
- **Debug tracing is dropped** (`isCompleteShapeDebugAnchor`,
  `traceCompleteShape*`). Rust has no counterpart; A01's probe sink replaces it.

## File ownership

| Item | New files | May edit |
|---|---|---|
| B03.1 | `board/search/rooms.rs`, `board/search/complete_shape.rs` (skeleton), `tools/java-reference/probes/board/CompleteShapeProbe.java`, `tests/board_probe_complete_shape.rs`, `fixtures/reference/probe/complete-shape/*` | `board/search/mod.rs` (append `mod`), `tools/java-reference/probes/board/{fixtures.tsv, README.md}` |
| B03.2 | — | `board/search/complete_shape.rs` |
| B03.3 | — | `board/search/complete_shape.rs` |
| B03.4 | `board/search/restrain45.rs` | `board/search/complete_shape.rs` |
| B03.5 | `board/search/restrain90.rs` | `board/search/complete_shape.rs` |
| B03.6 | — | `board/search/complete_shape.rs`, `CompleteShapeProbe.java` (append a stage), `tests/board_probe_complete_shape.rs` |
| B03.7 | — | `board/search/complete_shape.rs`, `board/search/work.rs` (B02's; counting calls only) |

P04 later edits `complete_shape.rs` for its measured divergence; until then
this file is B03's.

## Checklist

- [x] **B03.1 — Room types and the `complete-shape` probe.**
  - Java probe `CompleteShapeProbe.java`. For each board and tree variant:
    - **Variants:** 45° on Issue026-J2_reference, Issue508-DAC2020_bm08,
      Issue413-test, Issue145-smoothieboard and Issue676-ch32v-tx118s (large
      coordinates). 90° and plain on Issue026-J2_reference and
      Issue508-DAC2020_bm08, forced by `rules.setTraceAngleRestriction` right
      after load, before `getAutorouteTree` creates the tree. No fixture uses 90°
      or any angle; A07 adds synthetic boards later.
    - **Tree:** `getAutorouteTree(c)` with `c` the smallest non-zero trace
      clearance class of the net classes, else 0.
    - **Queries:** 500 per board and variant from `JavaRandom(20261015)`, drawn in
      this order: layer `nextInt(layers)`; `x = ll.x + nextInt(width + 1)`, the
      same for `y`; half-size `h = 1 + nextInt(max(1, min(width, height) / 50))`;
      net `nextInt(maxNet + 1)`; and `nextBoolean()` for a bounded start room.
      The contained shape is an `IntOctagon` (45°, plain) or `IntBox` (90°) of
      half-size `h` around the point. A bounded start room is the same kind of
      shape with half-size `20 * h`; otherwise the start shape is `null`.
      `ignoreObject` and `ignoreShape` are `null`.
    - **Output** per query: the inputs, then the result rooms in order (shape,
      layer, contained shape), or an exception record. Shapes are written as
      `O …`/`B …` (B02.1) or, for a `Simplex`, `S` followed by each line's two
      points.
  - Rust: `rooms.rs`, the `complete_shape` entry point returning an empty list,
    and `tests/board_probe_complete_shape.rs`, which draws the same inputs with
    `freerouting_geometry::jmath::JavaRandom` and compares line by line. The
    test's `EXPECTED_MISMATCHES` starts with every (board, variant), and each
    later item removes its variant.
  - **Accept:** outputs committed for all 9 (board, variant) pairs; the
    generator is deterministic (two runs, `cmp`); the Rust sampler reproduces
    Java's inputs exactly, which the test checks by comparing the input fields
    before the results.
  - *Done 2026-10-16:* `board/search/rooms.rs` (`IncompleteRoom { shape, layer, contained }`,
    `RoomId` re-exported), `board/search/complete_shape.rs` (`BasicBoard::complete_shape(tree_class,
    room, net, ignore, ignore_shape, &mut CompletionScratch)`, `BasicBoard::insert_room`/`remove_room`
    for the tree of a class, `ShapeSearchTree::complete_shape` dispatching on `TreeKind`),
    `tools/java-reference/probes/board/CompleteShapeProbe.java`, `tests/board_probe_complete_shape.rs`.
    Probed at `a773a70ec` (J01's pin, which has `WorkMeter`) in a private worktree with a private
    `REFERENCE_WORK_DIR`: `generate.sh <pin> complete-shape` printed `generate.sh: complete-shape: 5
    boards` (the 9 (board, variant) pairs plus `with-rooms` on the 5 boards; 173-481 KB each, no
    digest, no exception record). A second run was byte-identical (`cmp`, all 5 files).
    `sampled_inputs_match_java` passes: the input fields (layer, x, y, h, net, bounded) of all 500
    queries of every pair equal Java's. `EXPECTED_MISMATCHES` is empty from the start because every
    pair matched on the first comparison (B03.2-B03.5 were written before the probe ran).

- [x] **B03.2 — The plain variant.**
  - Java: `ShapeSearchTree.completeShape` (`:668-805`), `restrainShape`
    (`:806-920`), `divideLargeRoom` (`:1220-1244`).
  - Orders that decide results (`// PARITY:`):
    - the start shape is the board box intersected with the room shape; a start
      room exists only if that is 2-dimensional;
    - leaves are collected by a DFS against the start's bounding shape, then
      **sorted** in `Leaf` order (`:732`), which equals the sorted `overlaps`
      result. The bound updated after each obstacle doesn't prune the collected
      list;
    - per leaf: an obstacle only if `is_trace_obstacle(net)`, on the room's
      layer, and not `ignore` (identity, so `TreeObject` equality);
    - per current room: restrain when the intersection is 2-dimensional, unless
      the obstacle is a room and `ignore_shape` contains the intersection;
    - `newBoundingShape` is the union over all rooms accumulated so far, rebuilt
      after each restrain (the `v1.9` comment, `:778-784`);
    - `divideLargeRoom` splits a single room larger than half the board in either
      direction into sections of width `0.5 * max(height, width)`
      (`divide_into_sections`).
  - Rust: `complete_shape.rs` `complete_plain`, `restrain_plain`, `divide_large_room`.
  - **Accept:** `board_probe_complete_shape` equal for both plain pairs.
  - *Done 2026-10-16:* `complete_plain`, `restrain_plain`, `divide_large_room` in
    `complete_shape.rs`. The leaf collection is `ShapeTree::overlaps` against the start bound (the
    same DFS, the same sorted `Leaf` order and the same visit count as Java's loop); Java's
    `newBoundingShape` is never read after the collection, which a `// PARITY:` comment says.
    `cargo test --release --test board_probe_complete_shape`: 2 passed; stage `plain` equal on
    Issue026-J2_reference and Issue508-DAC2020_bm08 (500 lines each; on J2, 397 queries return
    simplex rooms, 557 in all).

- [x] **B03.3 — The 45° traversal.**
  - Java: `ShapeSearchTree45Degree.completeShape` (`:98-305`) and its
    `divideLargeRoom` override (`:308-322`).
  - Orders that decide results (`// PARITY:`):
    - the contained shape becomes its bounding octagon; `null` gives an empty
      list. The start shape is the board box's bounding octagon, intersected with
      the room's bounding octagon when the room has a shape that is an
      `IntOctagon` (another kind gives an empty list);
    - the DFS pops a node and descends only if its bounds intersect the
      **current** bound. It pushes the first child, then the second, so the
      second is visited first;
    - at an obstacle leaf, every current room is checked in list order: a room
      not overlapping the obstacle's bounding octagon is kept as is; an
      overlapping one is skipped by the `ignore_shape` rule for rooms (`:211-231`)
      or replaced by `restrain_shape`'s result. `newBoundingShape` grows with
      every room added;
    - after the loop: `divide_large_room`, then each room's shape and contained
      shape replaced by their bounding octagons, then rooms whose contained shape
      contains the room shape removed (`removeIf`, `:296-298`).
  - Rust: `complete_45` in `complete_shape.rs`, with the DFS over B02's arena and
    the scratch stack. `restrain_45` is a stub until B03.4.
  - **Accept:** a unit test on a hand-built 5-leaf tree shows the visit order
    (second child first) and the pruning by the shrinking bound. The probe pairs
    are compared in B03.4.
  - *Done 2026-10-16:* `complete_45` in `complete_shape.rs` over the new
    `ShapeTree::depth_first(stack, visit)` (`datastructures/shape_tree.rs`: pop, call `visit`,
    push the first then the second child when it returns true; returns the nodes popped), with
    the stack and the next room list kept in `CompletionScratch`. Unit tests in
    `complete_shape::tests`, 3 passed: `depth_first_visits_the_second_child_first` (5 rooms
    inserted A, E, B, D, C give `I(I(A, I(B, C)), I(E, D))`, pinned by its walk; leaf visit order
    D, E, C, B, A, 9 nodes popped), `the_shrinking_bound_prunes_the_traversal` (a contained shape
    between D and E: D and E restrain the room to x in [310, 400], so `I(A, I(B, C))` no longer
    meets the bound and is not descended: 5 units, not 9),
    `missing_contained_shape_or_wrong_start_type_gives_no_rooms`.

- [x] **B03.4 — 45° restraining.**
  - Java: `ShapeSearchTree45Degree.restrainShape` (`:324-442`),
    `calcOutsideRestrainedShape` (`:443-476`), `calcInsideRestrainedShape`
    (`:477-506`), `obstacleSegmentTouchesInside` (`:40-67`),
    `signedLineDistance` (`:69-96`).
  - Orders that decide results: which edge line of the obstacle restrains first
    (the one furthest from the contained shape, with Java's tie rule); the
    0.5 diagonal factor and the operation order inside `signedLineDistance` (D6,
    no reassociation); `IntOctagon::normalize` after each restrain; the order in
    which the outside and inside pieces are appended.
  - Rust: `restrain45.rs`.
  - **Accept:** `board_probe_complete_shape` equal for all five 45° pairs; any
    pair left differing is investigated to its first differing room and fixed, not
    listed.
  - *Done 2026-10-16:* `board/search/restrain45.rs` (`restrain_45`, `obstacle_segment_touches_inside`,
    `signed_line_distance` with Java's `int` differences (`wrapping_sub`), then `0.5 *` for the
    diagonals; `calc_outside_restrained_shape`, `calc_inside_restrained_shape`, each normalized;
    the outside piece is appended before the rest's pieces). Stage `45` equal on all five boards
    (J2, DAC2020_bm08, Issue413, Issue145, Issue676; 500 lines each),
    `cargo test --release --test board_probe_complete_shape`: 2 passed. No pair differed.

- [x] **B03.5 — The 90° variant.**
  - Java: `ShapeSearchTree90Degree.completeShape` (`:41-216`) and `restrainShape`
    (`:217-342`). The traversal is B03.3's with `IntBox` bounds; a contained shape
    that isn't an `IntBox` gives an empty list (`:60-63`). Unlike the 45°
    variant it returns the room list as it stands: no `divideLargeRoom` and no
    `removeIf` (`:196-210`).
  - Rust: `complete_90` in `complete_shape.rs`, `restrain90.rs`.
  - **Accept:** `board_probe_complete_shape` equal for both 90° pairs.
  - *Done 2026-10-16:* `complete_90` in `complete_shape.rs` (a contained or start shape that is not
    an `IntBox` gives no rooms; no `divideLargeRoom`, no `removeIf`; a room whose overlap with an
    ignored tree room lies in `ignore_shape` is dropped, unlike 45°) and
    `board/search/restrain90.rs`. Stage `90` equal on J2 and DAC2020_bm08 (500 lines each).

- [x] **B03.6 — Completion against trees that hold rooms.**
  During a search the autoroute tree holds the complete rooms found so far, and
  Java completes new rooms against them, with the neighbour room as `ignore`
  and the door shape as `ignore_shape`. Room ids decide the room-before-item
  order (B02.1). The engine numbers them from a per-session counter
  (`AutorouteEngine.java:697`), so the caller passes the id.
  - Probe stage `with-rooms` (45° pairs): after the 500 queries, insert the
    first 100 result rooms as `new CompleteFreeSpaceExpansionRoom(shape, layer,
    id)` with ids 1, 2, … in result order (`tree.insert(room)`). Run 200 more
    seeded queries; each takes one inserted room as `ignoreObject` (seeded index)
    and its shape intersected with the query's contained shape as `ignoreShape`.
    Then remove the rooms in insertion order, as `AutorouteEngine.clear` does
    (`:321-330`), and hash the tree (B02's fingerprint).
  - Rust: the same through `insert_room`/`remove_room` and `complete_shape` with
    `ignore`.
  - **Accept:** stage `with-rooms` equal, including the final tree fingerprint.
    That fingerprint also proves that room insertion and removal keep Java's
    shape.
  - *Done 2026-10-16:* the probe and the test run stage `with-rooms` on the five 45° boards: rooms
    inserted with `BasicBoard::insert_room` (ids 1..100 in result order), 200 queries with
    `ignore = TreeObject::Room(id)` (the index drawn last, `nextInt(inserted)`) and `ignore_shape`
    its shape intersected with the contained shape, then `remove_room` in insertion order. Equal on
    all five boards, both fingerprint lines included (after insertion and after removal; J2
    inserted: 353 nodes, 177 leaves, depth 26, `shapeHash 562ed69de49a1367`; removed: 153 nodes,
    77 leaves, `bec0f7d1c5091da9`).

- [x] **B03.7 — Work counting inside room completion.**
  At `cda540f` the completion loops count nothing. At `30baad7e2` (J01's new
  pin) each of the three calls `WorkMeter.tick()` once per node popped off its
  stack, before the intersection test: the plain collection loop
  (`ShapeSearchTree.java:718,720`), the 45° loop
  (`ShapeSearchTree45Degree.java:169,176`) and the 90° loop
  (`ShapeSearchTree90Degree.java:90,97`). The count depends on the shrinking
  bound in the 45° and 90° loops, so a pruning difference shows up here even
  when the rooms agree.
  - Rust: add B02.7's meter to the three DFS loops at the same points, counted in
    a local and added once per call.
  - **Accept:** the `complete-shape` probe, run at `30baad7e2`, also logs
    `WorkMeter.current().units()` before and after each call. Rust's meter delta
    is equal for every query of every pair, including stage `with-rooms`.
  - *Done 2026-10-16:* the 45° and 90° loops add `depth_first`'s popped count to the tree's meter
    once per call; the plain variant counts through `ShapeTree::overlaps` (B02.7), which pops the
    same nodes as Java's collection loop. `work.rs` needed no change. The probe ran at `a773a70ec`
    (J01's pin; `30baad7e2` is its ancestor, with the same three ticks) and logs
    `WorkMeter.current().units()` before and after each call: the `units` field is equal for every
    query of every pair and of `with-rooms` (Issue145 stage `45`: up to 3 233 units per call).
