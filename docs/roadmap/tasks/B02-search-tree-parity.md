# B02 — Search-tree parity

Opened 2026-10-15. Decision D5 in [port-completion.md](../port-completion.md):
the Rust search tree must have Java's *shape*, not only its query results.
Two Java behaviours depend on the shape:
- The 45° and 90° room completion visits obstacles in the tree's depth-first
  order while its bound shrinks (`ShapeSearchTree45Degree.java:112-305`,
  `ShapeSearchTree90Degree.java:55-216`). The rooms, and so the routes, follow
  the tree's topology.
- The fork's work limits count search-tree nodes visited. At `30baad7e2`
  ("Make routing independent of machine load", the commit J01 re-pins to),
  `WorkMeter.tick()` runs once per popped node in `MinAreaTree.overlapsUnlocked`
  and in the three `completeShape` traversals, and a `WorkLimit` of `n` nominal
  ms allows `n × 64,000` units. A different shape means a limit fires at a
  different point.

The Rust `ShapeTree` already follows `MinAreaTree`'s insertion and removal
rules. It loses Java's shape in four places: trace combine and trace change
remove and reinsert a whole trace where Java reuses leaves, deep copies
(fixed by B01.3), and the shove cutout (ported by O03 on this checklist's tree
operation). This checklist ports the leaf-reuse operations, adds rooms as tree
objects, proves the shape with fingerprints against a Java probe on every
loadable fixture, and adds the work meter. It removes the DIVERGENCES entry
"Combining traces removes and reinserts the tree entries".

**Prerequisites:** B01.1 (probe support), B01.3 (deep copies, for the
`deep-copy` stage), F02.1. B02.7 reads Java at `30baad7e2`, which has landed;
it doesn't wait for J01, but J01.5's regeneration reruns its probe at the pin.

Suggested split: B02.1–B02.4, then B02.5–B02.8. B02.8 is off the critical path.

## What exists and what this adds

Rust paths under `crates/freerouting/src/` at `70389b7`; Java under
`src/main/java/app/freerouting/` at `cda540f`.

| Rust today | Java at the pin |
|---|---|
| `datastructures/shape_tree.rs`: arena `ShapeTree<K>`, `MinAreaTree` insertion (`position_locate`, tie `<=` takes the first child, `:166-175`), removal (sibling moves up, ancestor bounds recomputed while they shrink), `overlaps` sorted by (key, shape index), `set_leaf` (`:102`), `to_array`, `distance_to_root`. No visit counter | `datastructures/ShapeTree.java`, `MinAreaTree.java` (`overlapsUnlocked` `:43-66`, `positionLocate`, `removeLeafUnlocked`) |
| `board/search/mod.rs`: `ShapeSearchTree { kind, compensated_clearance_class, tree: ShapeTree<ItemId>, entries: BTreeMap<ItemId, ItemEntries> }`; `SearchTreeManager` with the default tree first; `get_autoroute_tree` (`:736`) inserts `items()` highest id first | `board/searchtree/ShapeSearchTree.java` (1289 lines), `ShapeSearchTree45Degree.java`, `ShapeSearchTree90Degree.java`, `SearchTreeManager.java` |
| `board/trace_ops.rs`: `combine_trace_at` (`:405-489`) and `change_trace` (`:544-576`) remove and reinsert the whole trace; `change_trace` compares lines by value | `PolylineTrace.combineAtStart/AtEnd` (`board/trace/PolylineTrace.java:201-460`) reuse leaves through `SearchTreeManager.mergeEntriesInFront/AtEnd` (`:237-262`); `PolylineTrace.change` (`:937-1006`) uses `changeEntries` with keep counts found by **reference** comparison of lines |
| Tree keys are items only | Trees hold `SearchTreeObject`s: items and `CompleteFreeSpaceExpansionRoom`s. `Leaf.compareTo` (`ShapeTree.java:273-279`): object order, then shape index. Rooms before items (`CompleteFreeSpaceExpansionRoom.java:46-54` returns -1 against a non-room; `Item.java:96-105` returns 1), each by id descending |
| No `reduceTraceShapeAtTiePin`, `changeItemShape`, `reuseEntriesAfterCutout` | `ShapeSearchTree.java:922-994`, `:362-405` |
| `docs/DIVERGENCES.md`, board model: "Combining traces removes and reinserts the tree entries …" and "`PolylineTrace.change` compares lines by value …" | — |

## Decisions made here

- **One key type for all trees:** `enum TreeObject { Room(RoomId), Item(ItemId) }`
  with a derived `Ord`. `Room` is declared first; `RoomId` and `ItemId` order
  by id descending. This is Java's `Leaf.compareTo` exactly. The default tree
  uses the same key; B02.1 measures that against a generic item-only key and
  keeps the generic only if DRC is more than 2% slower.
- **Stored shapes are data, never recomputed.** Java's merge and change move
  the stored tree shape of a kept leaf to its new index (`ShapeSearchTree.java:154-165,241-257,324-346`).
  For a reversed trace, or traces of different clearance classes in a
  compensated tree, a recomputed shape could differ. Rust moves the stored
  `TileShape` with the leaf.
- **Line identity becomes an explicit argument.** Java's `change` finds the
  unchanged prefix and suffix by reference comparison (`PolylineTrace.java:956-982`).
  `change_trace` takes `LineReuse { prefix: usize, suffix: usize }`: the number
  of leading and trailing lines that are *the same Java objects* as the old
  polyline's. Callers (O01, O04, B05.5) compute it from how they built the new
  polyline, following O00's "unchanged is an explicit outcome" rule.
  `LineReuse::by_value(old, new)` exists for callers whose Java code is shown
  to create new line objects exactly where values change; each such caller
  says why in a `// PARITY:` comment.
- **The work meter belongs to the board's trees; the limit is passed
  explicitly.** Java's meter is per thread (`WorkMeter.current()`, a
  `ThreadLocal`), and a `WorkLimit` asserts that it is checked on the thread
  that created it (`datastructures/WorkLimit.java:47-53` at `30baad7e2`). Rust
  gives each board's `SearchTreeManager` one `WorkMeter` (an `AtomicU64`)
  instead. Each query counts popped nodes in a local and adds them once at the
  end. The two count the same units whenever a limit's window runs on one
  thread against one board, which holds at every Java limit site (pull-tight,
  maze search, shove, drill mover, fanout per pin, interactive route):
  - Parallel DRC on worker threads adds commutatively to the board's meter, so
    the total equals Java's serial count on the routing thread.
  - A per-thread meter in Rust would leak counts between rayon tasks that share
    a worker thread (work stealing during a nested join). That would break D2,
    so a thread-local is ruled out.
  - Optimizer candidates run on their own board copies, so they never share a
    meter.
  A `WorkLimit` records the meter value at its start and is passed to the
  operations that check it (O00's "passed explicitly"). In debug builds it
  asserts that it is checked against the meter it started on: Rust's form of
  Java's owner-thread assert.
- **Fingerprints hash the shape; content digests don't.** `fingerprint()`
  hashes a preorder walk, so it changes with the topology. `content_digest()`
  hashes the sorted leaf list, so it ignores the topology. P02.1's audit against
  a rebuild can only compare content digests, because a live tree's shape
  depends on its history (that is Java's behaviour too).
- **`delete_all_traces_and_vias` keeps removing tree entries.** Java leaves
  stale entries (`BasicBoard.java:1523-1544`). It is GUI-only, so it is
  recorded as a DIVERGENCES line (B08.7 owns the test), not reproduced.

## File ownership

| Item | New files | May edit |
|---|---|---|
| B02.1 | `board/search/tree_object.rs`, `board/search/fingerprint.rs`, `tests/search_tree_objects.rs` | `datastructures/shape_tree.rs`, `board/search/mod.rs`, `board/basic_board.rs` (call sites of the key type only), `drc/*` (call sites only) |
| B02.2 | `tools/java-reference/probes/board/TreeProbe.java`, `tests/board_probe_tree.rs`, `fixtures/reference/probe/tree/*` | `tools/java-reference/probes/board/README.md`, `tools/java-reference/README.md` |
| B02.3 | `board/search/entries.rs` | `board/trace_ops.rs` (combine), `board/search/mod.rs`, `tests/java_ports.rs` (append), `docs/DIVERGENCES.md` |
| B02.4 | `tools/java-reference/probes/board/TreeOpsProbe.java`, `tests/board_probe_tree_ops.rs`, `fixtures/reference/probe/tree-ops/*` | `board/search/entries.rs`, `board/trace_ops.rs` (`change_trace`), `docs/DIVERGENCES.md` |
| B02.5 | — | `board/basic_board.rs`, `board/trace_ops.rs`, `board/search/mod.rs`, `TreeOpsProbe.java` (append stages), `tests/board_probe_tree_ops.rs` |
| B02.6 | — | `board/search/entries.rs`, `board/search/mod.rs`, `board/basic_board.rs` (`tree_shape_on_layer`) |
| B02.7 | `board/search/work.rs`, `tests/work_limit.rs`, `tools/java-reference/probes/board/TreeVisitsProbe.java`, `tests/board_probe_tree_visits.rs`, `fixtures/reference/probe/tree-visits/*` | `datastructures/shape_tree.rs`, `board/search/mod.rs`, `board/snapshot.rs` (meter reset) |
| B02.8 | `board/side_table.rs`, `board/autoroute_log.rs` | `board/basic_board.rs`, `board/trace_ops.rs` (log calls only) |

## Checklist

- [ ] **B02.1 — Rooms as tree objects, and tree fingerprints.**
  - Java: `ShapeTree.Leaf.compareTo` (`ShapeTree.java:273-279`),
    `CompleteFreeSpaceExpansionRoom.compareTo` (`autoroute/expansion/CompleteFreeSpaceExpansionRoom.java:46-54`),
    `Item.compareTo` (`Item.java:96-105`); a room has one tree shape (its shape),
    `shapeLayer` = its layer, and `isObstacle`/`isTraceObstacle` true for every
    net (`CompleteFreeSpaceExpansionRoom.java:57-84`). `ShapeSearchTree.overlappingObjects` (`:410-433`) returns a
    `TreeSet<SearchTreeObject>`.
  - Rust:
    - `board/search/tree_object.rs`: `RoomId(i32)` with descending `Ord`,
      `TreeObject`, and `ShapeTree<TreeObject>` in every `ShapeSearchTree`.
    - Room storage in each tree: `rooms: BTreeMap<RoomId, RoomEntry { shape:
      TileShape, layer: i32, leaf: Option<LeafId> }>`.
      `ShapeSearchTree::insert_room(id, shape, layer)` and `remove_room(id)`. The
      caller supplies the id (the A series' per-session counter, B03.6).
    - `overlapping_tree_entries` and a new `overlapping_objects` on the tree
      return `TreeObject`s. Rooms pass every net filter. The existing item-only
      board queries keep their signatures and skip rooms (the default tree never
      holds rooms).
    - `board/search/fingerprint.rs`: `ShapeSearchTree::walk_lines() ->
      Vec<String>`, `fingerprint() -> TreeFingerprint { nodes, leaves, max_depth,
      shape_hash: u64 }` and `content_digest() -> u64`.
  - The canonical walk (Java writes the same text): preorder (node, first child's
    subtree, second child's subtree), one line per node, joined by `\n` with a
    trailing `\n`, hashed with FNV-1a 64 over the UTF-8 bytes.
    - Inner node: `<depth> I <bounds>`.
    - Leaf: `<depth> L <bounds> <R|I> <id> <shape index>`.
    - `<bounds>`: an octagon is `O` and its eight fields in Java's declaration
      order: `leftX bottomY rightX topY upperLeftDiagonalX lowerRightDiagonalX
      lowerLeftDiagonalX upperRightDiagonalX` (Rust: `left_x bottom_y right_x
      top_y upper_left_diagonal_x lower_right_diagonal_x lower_left_diagonal_x
      upper_right_diagonal_x`). A box is `B llx lly urx ury`.
    - The root has depth 0.
    - `content_digest` hashes the leaves sorted by (key, shape index), one line
      each, `<R|I> <id> <shape index> <bounds>`.
  - **Accept:**
    - `overlap_order_rooms_then_items_descending` on a tree with rooms 3, 7 and
      items 5, 9 overlapping one query: the order is room 7, room 3, item 9, item 5,
      ties by shape index.
    - `fingerprint_of_hand_built_tree`: 5 leaves inserted in a fixed order give a
      walk equal to a literal in the test.
    - `content_digest_ignores_insertion_order`.
    - `reference_load.rs`, `reference_drc.rs` and `tests/drc.rs` green. DRC wall
      time on Issue190-processor.Z80 is within 2% of before (release, median of 5,
      done note).

- [ ] **B02.2 — The Java `tree` probe on every loadable fixture.**
  The acceptance backbone of D5. It exists before the leaf-reuse ports, so each
  port shrinks a known mismatch list.
  - Java: `TreeProbe.java` reads `ShapeTree.root` (protected) by reflection and
    walks `InnerNode.firstChild/secondChild`, `Leaf.object`,
    `Leaf.shapeIndexInObject`, `boundingShape` (all public). It writes the same
    walk text as B02.1 and its hashes. The tree's identity is
    `ShapeSearchTree.getKey()` (`:83`), which Rust's `ShapeSearchTree::key()`
    already matches.
  - Stages per `fixtures.tsv` row (one JSONL line per tree and stage, with nodes,
    leaves, max depth, `shapeHash`, `contentDigest`):
    1. `load`: the default tree after load.
    2. `autoroute`: call `getAutorouteTree(c)` for every distinct trace clearance
       class `c` of the board's net classes, ascending (the class the router uses,
       `NetClass.getTraceClearanceClass`). Hash each new tree.
    3. `ses` (rows with a session): with those trees present, import the
       session. Hash every tree.
       Combine, split and normalize now run on all trees.
    4. `undo`: from stage 2, `generateSnapshot()`, import the session, `undo(null)`.
       Hash every tree. Then `redo(null)` (stage `redo`).
    5. `deep-copy`: `deepCopy()` of the stage-3 board. Hash its default tree.
  - Rust: `tests/board_probe_tree.rs` runs the same stages (`get_autoroute_tree`,
    `read_session`, `generate_snapshot`/`undo`/`redo`, `deep_copy`) and compares
    line by line through `board_probe::compare`. For a board under 2 000 leaves,
    a failing comparison writes both full walks under `target/probe/tree/` for a
    diff. For bigger boards the probe has a `--walk <board> <stage> <tree>` mode
    that prints Java's walk on demand.
  - **Accept:**
    - Probe outputs committed for every `fixtures.tsv` row.
    - `board_probe_tree` passes, with `EXPECTED_MISMATCHES` naming every
      (board, stage) that differs today and why ("combine reinserts", "change
      reinserts"). Stages `load` and `autoroute` must already match on every
      board. If one doesn't, that is an insertion-order or area-rounding bug
      (`positionLocate` compares `area(union) - area(child)` in `double`,
      `MinAreaTree.java:131-147`): fix it here, and say in the done note what it
      was.
    - B02.5 ends with `EXPECTED_MISMATCHES` empty.

- [ ] **B02.3 — Trace combine reuses Java's leaves.**
  - Java: `PolylineTrace.combineAtStart` (`:201-339`) and `combineAtEnd`
    (`:341-460`), `PolylineTraceSearchTreeAdapter`
    (`board/trace/PolylineTraceSearchTreeAdapter.java`),
    `ShapeSearchTree.mergeEntriesInFront` (`:184-272`) and `mergeEntriesAtEnd`
    (`:274-360`), applied to every tree in list order (`SearchTreeManager.java:237-262`).
  - Rust: `board/search/entries.rs` with `merge_entries_in_front(from, to,
    joined, from_entry_no, to_entry_no)` and `merge_entries_at_end(...)` on
    `ShapeSearchTree`, called for each tree, default first. `combine_trace_at`
    takes Java's two paths:
    - **Fallback** (Java `replaceGeometry`): when `Polyline::new(&new_lines)` has
      fewer lines than `new_line_count` (a skipped parallel line), or when either
      trace has no default-tree entries. Remove this trace's leaves, set the
      polyline, insert it again: today's code.
    - **Reuse**: per tree, in Java's statement order.
  - Orders that decide the shape, all marked `// PARITY:`:
    - front merge removes `from`'s joint leaf (index 0 if `change_order`, else
      the last), **then** `to`'s first leaf;
    - end merge removes `to`'s last leaf **first**, then `from`'s joint leaf
      (the last if `change_order`, else index 0);
    - `change_order` is `from.first_corner == to.first_corner` (front) or
      `from.last_corner == to.last_corner` (end);
    - kept leaves are relabelled in place (`ShapeTree::set_leaf`), never removed
      and reinserted. `from`'s leaves follow in reverse when `change_order`;
    - link shapes come from `offset_shapes(joined, to.half_width +
      compensation(to.class, to.layer), from_entry_no, to_entry_no)` with the
      tree kind's override. They are inserted in ascending index **after** every
      relabel;
    - `from_entry_no` = other lines - 3 (front) or this lines - 3 (end);
      `to_entry_no` = other lines (front) or this lines (end), minus 1 when
      `skip_line`;
    - then `from` loses its entries without tree removals (Java
      `clearSearchTreeEntries`), `this` is removed if it has fewer than 3 lines,
      `from` is removed, and the changed area is joined at the corner.
  - **Accept:**
    - Port `PolylineTraceSplitTest.testCombineAtEndRecoversMissingDefaultTreeEntries`
      (`src/test/java/app/freerouting/board/PolylineTraceSplitTest.java:353`)
      into `tests/java_ports.rs`.
    - `fixtures/Issue723-CombineStackOverflow.dsn` (about 4 000 collinear
      segments) loads on a thread with a 256 KiB stack.
    - `board_probe_tree`: every `ses` and `redo` stage mismatch caused by combine
      is gone from `EXPECTED_MISMATCHES`.
    - The DIVERGENCES entry "Combining traces removes and reinserts the tree
      entries …" is removed, and `board/search/mod.rs:5-9`'s module doc updated.

- [ ] **B02.4 — Trace change reuses Java's leaves.**
  - Java: `PolylineTrace.change` (`:937-1006`) and
    `ShapeSearchTree.changeEntries` (`:123-182`).
    - `change` first calls `additionalUpdateAfterChange` and `saveForUndo`.
    - It then finds `p`, the first index below `min(new, old)` where the lines
      differ, and `s` from the end, both by reference. If either scan finds no
      difference, it returns (after `saveForUndo`).
    - `keep_at_start = max(p - 2, 0)`, `keep_at_end = max(s - 2, 0)`.
    - `changeEntries` runs on every tree: it keeps the first `keep_at_start`
      leaves, removes the old middle leaves in ascending index, relabels the last
      `keep_at_end` leaves to their new indexes, stores the new middle shapes
      (`offset_shapes(new, half_width + compensation, keep_at_start,
      new_lines - 1 - keep_at_end)`), and inserts the middle leaves in ascending
      index.
    - Then the lines are set and the trace is normalized with the changed area's
      clip; an exception is logged and swallowed.
  - Rust: `change_trace(id, polyline, reuse: LineReuse) -> ChangeOutcome
    { Unchanged, Changed }` with Java's early returns. It is only called by later
    checklists today, so the signature can change freely. The normalize error is
    logged, not raised.
  - `TreeOpsProbe.java`, stage `change`: on Issue026-J2_reference with its SES
    and on Issue413-test, for 50 seeded traces with at least 4 corners, build a
    new `Polyline` from a `Line[]` that reuses the old `Line` objects except at
    one or two seeded indexes, where it puts a translated line. Call
    `trace.change(newPolyline)` and log `p`, `s`, the new lines and every tree's
    hash. The Rust side builds the same lines and passes `LineReuse { prefix: p,
    suffix: s }`.
  - **Accept:**
    - `board_probe_tree_ops` stage `change` equal on both boards.
    - `change_trace_unchanged_still_saves_for_undo`: Java's order, so a later undo
      sees a new level.
    - The DIVERGENCES entry "`PolylineTrace.change` compares lines by value …" is
      replaced by a module-doc note on `LineReuse`.

- [ ] **B02.5 — Every other tree mutation in Java's order.**
  Port nothing new unless a stage below differs; the work is to make each path
  provably equal.
  - The paths and their Java order:
    - insert: every tree in list order, an item's shapes in index order
      (`ShapeTree.insert`, `ShapeTree.java:47-62`);
    - remove: every tree, leaves in index order (`ShapeTree.remove`, `:135-147`);
    - split (`PolylineTrace.split(lineIndex, line)`, `:719-766`): remove the trace,
      then insert the two pieces (`insertTraceWithoutCleaning`, first then
      second); `split(clipShape)` (`:465-697`) in its loop order;
    - undo and redo: all cancelled items removed in Java's list order, then all
      restored items inserted (`RoutingBoardUndoFacade.applyUndoRedoSideEffects`);
    - `reinsert_tree_items`, `set_clearance_compensation_used`,
      `clearance_value_changed`: remove everything in item-list order and insert
      everything in item-list order (`SearchTreeManager.java:186-232`). Removing
      every leaf empties a `MinAreaTree` (`removeLeafUnlocked` sets `root = null`
      when the last leaf goes), so Rust's rebuild from a fresh tree is equal;
      `tree-ops` proves it;
    - `insert_via` and `insert_escape_via` (split traces at the centre on each
      layer, `BasicBoard.java:308-313` and `:343-349`), `make_conductive` (remove
      the area, insert the new item);
    - `change_plane_as_obstacle` (`RoutingBoard.java:1290-1322`, ends in
      `reinsertTreeItems`).
  - `TreeOpsProbe.java` stages (fixtures: Issue026-J2_reference with SES,
    Issue145-smoothieboard, Issue413-test, plus every fixture with a session for
    the cheap stages 1–3):
    1. `insert-via`: 20 seeded vias at trace corners, with the board's default via
       padstack, through `insertVia`;
    2. `remove`: remove the 30 highest-id traces (`removeItems`);
    3. `undo-redo`: `generateSnapshot`, stage 2, `undo`, `redo`;
    4. `compensation`: `setClearanceCompensationUsed(true)` then `false`;
    5. `plane-obstacle`: `changePlaneAsObstacle(true)`;
    6. `make-conductive` on a fixture with an obstacle area, if one exists (the
       probe picks it and records which).
    Each stage logs every tree's hash.
  - **Accept:**
    - `board_probe_tree_ops` equal on every stage.
    - `board_probe_tree` passes with `EXPECTED_MISMATCHES` **empty**: fingerprints
      equal on every loadable fixture after load, after creating the autoroute
      trees, after the session import, after undo and redo, and after
      `deep_copy`.
    - The done note gives the fixture count per stage.

- [ ] **B02.6 — Cutout reuse, single-shape change and tie-pin reduction.**
  - Java:
    - `ShapeSearchTree.reuseEntriesAfterCutout` (`:362-405`): the start piece
      takes the first `start_lines - 3` leaves of the old trace (relabelled), then
      gets one new leaf for its last shape; the end piece gets one new leaf for its
      first shape, then takes the old trace's last `end_lines - 3` leaves; taken
      entries are nulled in the old trace's array. Its caller
      `ShapeTraceEntries.fastCutoutTrace` (`:111-150`) then removes the old trace,
      which removes the remaining middle leaves in index order. O03 ports the
      caller.
    - `changeItemShape` (`:967-994`): remove one leaf, store the new shape, insert
      the leaf again; other leaves untouched.
    - `reduceTraceShapeAtTiePin` (`:922-965`): if the trace starts (or ends) at
      the tie pin's centre, cut the pin's tree shape on the trace layer out of the
      first (or last) trace shape. Keep the last 2-dimensional piece that contains
      the compare corner (corner 1, or corner `count - 2`), else the first
      2-dimensional piece, and apply `changeItemShape`. The maze search calls it on
      the autoroute tree (`MazeSearchEngine.java:175`), and the change persists for
      later connections.
    - `DrillItem.getTreeShapeOnLayer` (`DrillItem.java:241-250`).
  - Rust: `reuse_entries_after_cutout(tree, from, start, end)`,
    `change_item_shape(tree, id, index, shape)`,
    `BasicBoard::reduce_trace_shape_at_tie_pin(tree_class, pin, trace)`,
    `BasicBoard::tree_shape_on_layer(tree_class, id, layer)` (B04.7 re-exports
    it). The tie-pin function picks the tree by its compensated class. Also make
    `ShapeSearchTree::offset_shape(polyline, half_width, index)` and
    `offset_shapes(polyline, half_width, from, to)` public, with each tree kind's
    override (`ShapeSearchTree.java:1204-1218`,
    `ShapeSearchTree90Degree.java:507-520`). B02.3 and B02.4 already use them
    internally (`trace_offset_shape`, `board/search/mod.rs:404`), and B04.7 probes
    them.
  - **Accept:**
    - Unit tests: `reuse_after_cutout_leaf_sequence` checks the relabel, insert
      and removal sequence against a hand-built expectation with Java line
      references. O03's cutout probe is the Java-backed proof; it is named in
      O03's Accept.
    - Probe stage `tie-pin` in `TreeOpsProbe.java`: on every fixture with a pin of
      more than one net (the probe lists them; if none exists, a synthetic DSN in
      `fixtures/synthetic/tie-pin.dsn`, with a note), call
      `reduceTraceShapeAtTiePin` on the autoroute tree for every trace ending at
      such a pin. Log the changed shapes and the tree hash. Rust is equal.

- [ ] **B02.7 — Work meter and work limit.**
  Read Java at `30baad7e2` for this item (the commit J01 re-pins to; its
  `PORT_LEDGER.md` line says "Port: yes — use the same budgets and count the
  same units").
  - Java's counting points: one `WorkMeter.tick()` per node popped off the
    traversal stack, before the intersection test, leaf or inner, hit or miss.
    There are four:
    - `MinAreaTree.overlapsUnlocked` (`datastructures/MinAreaTree.java:50,57`),
      which every board and tree overlap query goes through
      (`ShapeSearchTree.overlappingTreeEntriesUnlocked`,
      `…WithClearanceUnlocked`);
    - the plain `ShapeSearchTree.completeShape` collection loop (`:718,720`);
    - `ShapeSearchTree45Degree.completeShape` (`:169,176`);
    - `ShapeSearchTree90Degree.completeShape` (`:90,97`).
    B03 adds the three `complete_shape` counts. This item adds the meter and the
    overlap count.
  - Java's limit (`datastructures/WorkLimit.java`):
    - `UNITS_PER_NOMINAL_MILLISECOND = 64_000` (`:31`);
    - `maxUnits = saturatedMultiply(max(0, nominalMs))`, saturating at
      `Long.MAX_VALUE` (`:46,61-66`);
    - `limitExceeded()` is `units - startUnits > maxUnits`, strictly greater
      (`:53`), and asserts the owner thread (`:51`).
  - Rust: `board/search/work.rs`.
    - `WorkMeter` (an `AtomicU64`) owned by `SearchTreeManager`, zeroed by
      `restore()`. `ShapeTree::overlaps` counts popped nodes into a local and adds
      it once.
    - `WorkLimit { meter_id, start: u64, max_units: u64 }` with
      `WorkLimit::nominal_ms(&meter, ms: i64)` (Java's clamp and saturation),
      `exceeded(&meter) -> bool` (`>`), and a debug assertion that `meter` is the
      one it started on.
    - `pub const UNITS_PER_NOMINAL_MS: u64 = 64_000`.
    - User stops stay on `freerouting_geometry::Stoppable` (`stoppable.rs:6`),
      checked where Java checks `isStopRequested`, which callers do before the
      limit.
    - Wall time never changes the work done (D2).
  - Port `src/test/java/app/freerouting/datastructures/WorkLimitTest.java`
    (151 lines, 9 tests) as `tests/work_limit.rs`:
    - the limit is exceeded only after its work is done;
    - elapsed time alone never exceeds it;
    - work done before the limit was created doesn't count;
    - `workOfOtherThreadsDoesNotCount` becomes "work on another board doesn't
      count";
    - zero and negative limits are exceeded by the first unit;
    - the largest exact limit and the next one don't overflow;
    - `checkingFromAnotherThreadFailsLoudly` becomes "checking against another
      board's meter fails in debug builds";
    - a huge limit saturates;
    - `treeQueriesCountTheNodesTheyVisit`: two boxes, one query, 3 units.
    `fixtures/WorkLimitDeterminismTest.java` (fanout on bm01 with a 1 nominal ms
    per-pin budget, idle and under load) belongs to A12 (fanout) and is named in
    its Accept.
  - `TreeVisitsProbe.java`, run at `30baad7e2`: 1 000 seeded overlap queries per
    board on the default tree, after load and after the session import, on
    Issue026-J2_reference, Issue508-DAC2020_bm08, Issue145-smoothieboard and
    Issue190-processor.Z80. A query is an `IntOctagon` around a seeded point in
    the board box, with a seeded half-size up to 1/20 of the box's smaller side.
    The probe logs `WorkMeter.current().units()` before and after each query (the
    delta) and the sum.
  - **Accept:**
    - `tests/work_limit.rs`: the 9 ported tests green.
    - `board_probe_tree_visits` equal on all four boards: the units per query equal
      Java's.
    - `meter_counts_every_query_once_under_parallel_drc`: the meter total after a
      parallel `all_clearance_violations` equals the single-threaded total.
    - The probe's header records `30baad7e2`. If J01 has landed, the pin in force
      is that commit and the done note says so.

- [ ] **B02.8 — Autoroute side table and change log (off the critical path).**
  Java keeps per-item autoroute scratch on the item (`ItemAutorouteInfo`,
  `Via.getAutorouteDrillInfo`) and invalidates rooms through
  `RoutingBoard.additionalUpdateAfterChange` (`:96-121`). That hook does
  nothing unless an engine maintains its database (`:101-103`), which only
  Java's retained mode does, and A00 doesn't port retained mode. So parity
  needs only the side table; the change log serves a later fast mode.
  - Rust:
    - `board/side_table.rs`: `ItemSideTable<T>`, dense by item id with a
      generation stamp, so clearing all is O(1). The A series' session owns one.
    - `board/autoroute_log.rs`: when enabled (`set_autoroute_change_log(true)`),
      the board records ids at Java's hook points, in Java's order:
      - insert: after the tree insert (`BoardItemRepository.java:162-165`);
      - remove: before the tree remove (`:206-207`);
      - combine: once per successful combine (`PolylineTrace.java:189`);
      - split: each piece when there are more than one (`:687-691`);
      - change: before `saveForUndo` (`:944`);
      - the fast cutout: the old trace (`ShapeTraceEntries.java:113`, O03 calls
        it).
      It also records the ids whose autoroute info Java clears: undo and redo
      restores (`RoutingBoardUndoFacade`), and `clearDerivedData` points
      (`Item.java:1136-1141`: geometry transforms, `assignComponentId`,
      `reinsertTreeItems`).
  - **Accept:**
    - One unit test per hook point, with the expected sequence written from the
      Java lines cited.
    - With the log off, `reference_load.rs` time is unchanged within noise (done
      note).
