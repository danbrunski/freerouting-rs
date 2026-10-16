# B04 — Board checks and picks

Opened 2026-10-15. These are read-only board queries that the router, the
shover, pull-tight and interactive routing call before they change anything:
- `checkTraceShape` has 17 call sites in the tightener family alone
  (`TraceTightener90.java:55,103`, …);
- `checkTraceSegment` is called by `MazeTraceShover.java:178`,
  `FoundConnectionInserter.java:597` and `ViaOptimizer`;
- filtered `pickItems` is called by `MazeSearchEngine.java:497`,
  `FoundConnectionInserter.java:182`, `TraceTightener.java:483` and
  `ViaOptimizer.java:142`.

None exists in Rust. All are cheap to prove with seeded probes. One of them,
`checkPolylineTrace`, takes an item id for a temporary trace on every call, so
its probe also compares `maxGeneratedId`.

**Prerequisites:** B01.1 (probe support), B01.2 (`burn_item_id`). B02.6 for
`tree_shape_on_layer` and the public `offset_shape(s)`, which B04.7 uses. Can run beside B02 and B03:
B04's code goes in new files.

## What exists and what this adds

| Rust today (`crates/freerouting/src/`, `70389b7`) | Java at `cda540f` (`src/main/java/app/freerouting/`) |
|---|---|
| `pick_items(location, layer) -> BTreeSet<ItemId>` without a filter (`board/basic_board.rs:1116`) | `BasicBoard.pickItems(Point, int, ItemSelectionFilter)` (`board/facade/BasicBoard.java:1199-1211`); `board/actions/ItemSelectionFilter.java` (93 lines); `Item.isSelectedByFilter` per class (`Trace.java:379`, `Via.java:234`, `Pin.java:505`, `ConductionArea.java:417`, the area and outline classes) |
| `overlapping_items_with_clearance` (`basic_board.rs:1084`), tree queries with clearance (`board/search/mod.rs:252-330`), `overlapping_items` (`:1107`), `is_trace_obstacle` (`:1283`), `is_obstacle` (`:1173`) | `BasicBoard.checkShape` (`:1069-1094`), `checkTraceShape` (`:1101-1158`), `checkPolylineTrace` (`:1166-1188`); `Trace.touchingPinsAtEndCorners` (`Trace.java:390-410`) |
| — | `RoutingBoardSearchFacade.checkTraceSegment` ×2 (`:29-107`), `checkMoveItem` (`:111-145`), `checkChangeNet` (`:147-163`), `pickNearestRoutingItem` (`:165-217`) |
| `DrillData::shape_on_layer` (`board/items/mod.rs:109`), `tile_shape` (`basic_board.rs:1054`), private `pin_drill_allowed` (`:1146`), private `trace_offset_shape` (`board/search/mod.rs:404`) | `DrillItem.getTileShapeOnLayer` (`:252`), `getTraceConnectionShape` (`:359`), `minWidth` (`:369`); `PolylineTrace.getTraceConnectionShape` (`:918`); `ConductionArea.getTraceConnectionShape` (`:368`); `Pin.drillAllowed` (`:324`); `Item.isConnectedOnLayer` (`:635`); `Trace.getCompensatedHalfWidth` (`:86`), `nearestEndPoint` (`:256`); `ShapeSearchTree.offsetShape(s)` (`:1204-1218`, 90°: `ShapeSearchTree90Degree.java:507-520`) |

## Decisions made here

- **The queries live in new files** with `impl BasicBoard` / `impl RoutingBoard`
  blocks (`board/queries.rs`, `board/selection_filter.rs`,
  `board/item_queries.rs`), so B04 runs beside B02 without touching its files.
  `pick_items` keeps its signature. `pick_items_filtered` is added next to it.
- **`ItemFilter` is a bitset** over Java's `SelectableChoices` order: `TRACES,
  VIAS, PINS, CONDUCTION, KEEPOUT, VIA_KEEPOUT, COMPONENT_KEEPOUT,
  BOARD_OUTLINE, FIXED, UNFIXED`. The constructors match Java's three:
  `ItemFilter::default()` is all but the keepouts, conduction and outline;
  `ItemFilter::only(choice)` and `ItemFilter::of(&[choice])` add `FIXED` and
  `UNFIXED`.
- **No allocation in `check_trace_shape`.** It runs in every tightener step.
  Java builds a `LinkedList` per call; Rust reuses a caller-owned or
  thread-local scratch `Vec<TreeEntry>` and returns on the first obstacle, as
  Java does.
- **`check_polyline_trace` burns an id on every call** (B01.2's table), before
  it computes anything, as Java's constructor does.
- **The probe sampler is code shared by name, not by library.** Java and Rust
  each implement the sampler from the text in B04.1, and the test checks the
  echoed inputs before the results.

## File ownership

| Item | New files | May edit |
|---|---|---|
| B04.1 | `tools/java-reference/probes/board/BoardQueriesProbe.java`, `tests/board_probe_queries.rs`, `fixtures/reference/probe/board-queries/*` | `tools/java-reference/probes/board/{fixtures.tsv, README.md}` |
| B04.2 | `board/selection_filter.rs` | `board/mod.rs` (append) |
| B04.3 | `board/queries.rs` | `board/mod.rs` (append) |
| B04.4 | — | `board/queries.rs` |
| B04.5 | — | `board/queries.rs` |
| B04.6 | — | `board/queries.rs` |
| B04.7 | `board/item_queries.rs` | `board/mod.rs` (append) |

## Checklist

- [ ] **B04.1 — The `board-queries` probe.**
  One Java probe covers every method of this checklist, so each later item
  only adds the Rust side.
  - Fixtures: Issue026-J2_reference (+ its SES), Issue508-DAC2020_bm08
    (+ `Issue508-DAC2020_bm08-routed.ses`), Issue413-test, Issue957-smoke-ldo-led,
    Issue145-smoothieboard, Issue676-ch32v-tx118s (load only), the first fixture
    with a pin of more than one net (the probe scans `fixtures.tsv` and records
    which), and Issue026-J2_reference again with
    `searchTreeManager.setClearanceCompensationUsed(true)` (stage `compensated`).
  - Sampler, from `JavaRandom(20261015)` per board, drawn in this order for each
    sample:
    - a point: `x = ll.x + nextInt(width + 1)`, `y` the same;
    - a layer: `nextInt(layers)`;
    - a net: an existing net number from `nextInt(maxNet) + 1`;
    - a half width: `1 + nextInt(maxTraceHalfWidth * 2)`;
    - a clearance class: `nextInt(classCount)`;
    - for segments, a second point drawn like the first, within
      `min(width, height) / 10` of it.
    Plus, deterministically, every pair of pins of one net on a common signal
    layer (pins in item-list order, pairs in nested order, at most 200 per board):
    the straight segment between their centres.
  - Per sample, depending on the method:
    - `checkTraceShape` of the segment's offset shape, with the
      `touchingPinsAtEndCorners` of a temporary trace as contact pins;
    - `checkShape` of the octagon around the point;
    - `checkPolylineTrace`, with `maxGeneratedId` after the call;
    - `checkTraceSegment` both forms (raw bits of the returned double), and
      with `onlyNotShovableObstacles` both ways;
    - `pickItems` with each single-choice filter and the default filter;
    - `pickNearestRoutingItem` with and without `fromItem` (the first pick);
    - `checkMoveItem` (vector from the sample) and `checkChangeNet` for the
      highest-id item under the point;
    - B04.7's item helpers for every pin, via and trace (no sampling).
  - 300 samples per method and board, JSONL as B00 "Board probes".
  - **Accept:** outputs committed; two generator runs byte-identical;
    `tests/board_probe_queries.rs` compares the echoed inputs for every board
    (the Rust sampler reproduces Java's) and holds one `#[test]` per method that
    later items switch on.

- [ ] **B04.2 — `ItemFilter` and `pick_items_filtered`.**
  - Java: `ItemSelectionFilter` (`board/actions/ItemSelectionFilter.java`);
    `isSelectedByFilter` per item class: a trace needs `TRACES`, a via `VIAS`, a
    pin `PINS`, and so on, each also gated by `FIXED`/`UNFIXED` on the item's
    fixed state; `filter()` returns a new `TreeSet`, so the order stays highest
    id first.
  - Rust: `board/selection_filter.rs`: `ItemFilter`, `SelectableChoice`,
    `Item::is_selected_by(&ItemFilter)`, and `BasicBoard::pick_items_filtered(
    location, layer, Option<&ItemFilter>) -> BTreeSet<ItemId>`. G05.1 and G05.2
    also use them.
  - **Accept:** `pick_items_filtered` equal to the probe for every filter on every
    board; a unit test per item kind and fixed state.

- [ ] **B04.3 — `check_shape` and `check_trace_shape`.**
  - Java: `BasicBoard.checkShape` (`:1069-1094`): split the area into convex tiles;
    false if a tile leaves the board box or overlaps (with clearance) an object
    that is an obstacle for all the nets. `checkTraceShape` (`:1101-1158`):
    - false if the shape leaves the board box;
    - query the default tree: plain overlap when compensation is on, else
      overlap with clearance;
    - skip contact pins; any other pin is an obstacle (no acid traps);
    - an item is an obstacle if `isTraceObstacle` holds for **every** net;
    - a foreign trace at a tie pin is not an obstacle when the pin's tile shape on
      the layer `containsApprox` the intersection;
    - return on the first obstacle, in the query's order.
  - Rust: `BasicBoard::check_shape(area, layer, nets, class)` and
    `check_trace_shape(shape, layer, nets, class, contact_pins: Option<&BTreeSet<ItemId>>)`.
  - **Accept:** both equal to the probe on every board, including stage
    `compensated` and the tie-pin board. A micro-benchmark (1 000 checks on
    Issue145-smoothieboard, release) is in the done note, with zero heap
    allocations per call after warm-up (counted with a counting allocator in the
    benchmark only).

- [ ] **B04.4 — `touching_pins_at_end_corners` and `check_polyline_trace`.**
  - Java: `Trace.touchingPinsAtEndCorners` (`Trace.java:390-410`): for the first
    and then the last corner, the corner's surrounding octagon enlarged by the
    half width, `overlappingItemsWithClearance` on the layer with no ignored
    nets, pins that share a net, collected into a `TreeSet`.
    `checkPolylineTrace` (`BasicBoard.java:1166-1188`): build a temporary
    `PolylineTrace` (**an id is taken**), get its touching pins, and check each
    tile shape with `checkTraceShape` in index order, stopping at the first
    failure.
  - Rust: `BasicBoard::touching_pins_at_end_corners(trace_id)` and a variant for
    an unsaved polyline; `check_polyline_trace(polyline, layer, half_width, nets,
    class)`, which calls `burn_item_id()` first.
  - **Accept:** the probe's booleans equal **and** `maxGeneratedId` equal after the
    whole sequence on every board.

- [ ] **B04.5 — `check_trace_segment`, both forms.**
  - Java: `RoutingBoardSearchFacade.checkTraceSegment` (`:29-107`):
    - equal points give 0. The line-segment form needs a 3-line polyline, else 0;
    - the shape to check is `offsetShape(halfWidth, 0)`;
    - obstacles come from the default tree's `overlappingTreeEntriesWithClearance`
      with the nets as ignored nets;
    - with `onlyNotShovableObstacles`, routable items that aren't shove-fixed are
      skipped;
    - the shorten value is `halfWidth + compensation value` (compensation on) or
      `halfWidth + clearance` with the shape offset by the clearance (off);
    - `projection = max(0.0, from.scalarProduct(to, nearest) / lineLength -
      shorten - 1)`, where `nearest` is `intersection.nearestPointApprox(from)`;
    - keep the minimum, starting at `Integer.MAX_VALUE` as a double; return 0 as
      soon as it is `<= 0`.
  - Orders: the obstacle order of the query; the floating-point expression in
    exactly Java's order (D6).
  - Rust: `RoutingBoard::check_trace_segment(from, to, …)` and
    `check_trace_segment_line(segment, …) -> f64`.
  - **Accept:** results bit-equal to the probe on every board, both forms, both
    `only_not_shovable` values.

- [ ] **B04.6 — `pick_nearest_routing_item`, `check_move_item`, `check_change_net`.**
  - Java: `RoutingBoardSearchFacade.java:111-217`.
    - `pickNearestRoutingItem` walks `overlappingItems(point, layer)` (highest id
      first) and skips non-connectable items. A trace is a candidate if it is on
      the layer, no drill item is the current best, its distance is `< minDist`
      and `<= halfWidth`. A drill item is a candidate if it is on the layer and its
      centre distance is `< minDist`, **or** the current best is a trace. A
      conduction area is a candidate only while there is no best (distance
      `Integer.MAX_VALUE`). The `fromItem` connected set is computed lazily at the
      first candidate and excludes its members.
    - `checkMoveItem` adds the item to the caller's `ignoreItems` (a side effect
      callers rely on).
    - `checkChangeNet` tests each tile shape against connectable items not in the
      new net.
  - Rust: `RoutingBoard::pick_nearest_routing_item(point, layer, from:
    Option<ItemId>)`, `check_move_item(id, vector, ignore: Option<&mut
    BTreeSet<ItemId>>)`, `check_change_net(id, net)`.
  - **Accept:** all three equal to the probe on every board; a unit test that
    `check_move_item` adds the item to the ignore set.

- [ ] **B04.7 — Item query helpers on the router path.**
  - Java and Rust names:
    - `DrillItem.minWidth` (`DrillItem.java:369`) → `min_width(id)`;
    - `getTraceConnectionShape` for drill items (a point tile at the centre,
      `:359`), polyline traces (`PolylineTrace.java:918`) and conduction areas
      (`ConductionArea.java:368`) → `trace_connection_shape(tree_class, id, index)`;
    - `DrillItem.getTileShapeOnLayer` (`:252`) → `tile_shape_on_layer`;
    - `getTreeShapeOnLayer` → re-export B02.6's `tree_shape_on_layer`;
    - `Pin.drillAllowed` (`Pin.java:324`) → public `pin_drill_allowed`;
    - `Item.isConnectedOnLayer` (`Item.java:635`);
    - `Trace.getCompensatedHalfWidth(tree)` (`Trace.java:86`);
    - `Trace.nearestEndPoint` (`:256`);
    - `ShapeSearchTree::offset_shape(polyline, half_width, index)` and
      `offset_shapes(polyline, half_width, from, to)` with each tree kind's
      override (`ShapeSearchTree.java:1204-1218`,
      `ShapeSearchTree90Degree.java:507-520`): B02.6 makes them public, because
      B02 owns `board/search/mod.rs`; B04.7 only tests them.
  - **Accept:** the probe's helper lines equal for every pin, via and trace of
    every board; `offset_shapes` equal for 200 seeded polylines per tree kind (a
    probe stage that builds all three trees).
