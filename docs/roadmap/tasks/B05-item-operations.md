# B05 — Item operations, pin exits and pin connections

Opened 2026-10-15. These are the item-level operations the shover, pull-tight,
path insertion and interactive editing need, and that the board model still
lacks:
- moving, rotating, mirroring and copying items on the board, used by
  `DrillItemMover`, `MoveComponent` and the GUI move and copy states;
- pin exit restrictions and pin widths, used by the maze search
  (`MazeExpansionEngine.java:62`, `MazeSearchEngine.java:460,1233`) and path
  insertion (`FoundConnectionInserter.java:495,498,583`);
- the three pin-connection fixes `PolylineTrace.pullTight` runs after the
  tightener (`PolylineTrace.java:841-861`);
- `RoutingBoard.connectToTrace`, used by path insertion
  (`FoundConnectionInserter.java:110,125`) and interactive routing;
- pin swap.

**Prerequisites:** B01.2 (`burn_item_id`, for `copy(0)`), B02.4
(`change_trace` with `LineReuse`, for B05.5), B04.4 (`check_polyline_trace`, for
B05.5 and B05.6). B01.1 for the probes. O01 (pull-tight) and A06 (path
insertion) need B05.

Suggested split: B05.1–B05.4, then B05.5–B05.8.

## What exists and what this adds

| Rust today (`crates/freerouting/src/`, `70389b7`) | Java at `cda540f` (`src/main/java/app/freerouting/`) |
|---|---|
| Transforms only for `ObstacleArea` and `ComponentOutline` (`board/items/mod.rs:313-474`) and `Components::move_by/turn/rotate/change_side` (`board/structure/component.rs:349-368`) | `Item.moveBy` (`board/model/items/Item.java:302-316`), `translateBy/turn90Degree/rotateApprox/changePlacementSide` per class: `DrillItem.java:63-94`, `PolylineTrace.java:144-170`, `Via.changePlacementSide` (`Via.java:191`), `Pin.java:443-460`, `BoardOutline.java:265-302`; `DrillItem.moveBy` (`:96-146`) inserts connecting traces |
| `Item: Clone`; no board-level copy | `copy(int id)` per class (`PolylineTrace.java:63`, `Via.java:71`, `Pin.java:122`, `ObstacleArea.java:101`, `ConductionArea.java:319`, `ComponentObstacleArea.java:48`, `ViaObstacleArea.java:73`, `ComponentOutline.java:57`, `BoardOutline.java:355`); `Components.copy` (`board/model/structure/Components.java:98`), `Component.restorePose` (`Component.java:148`) |
| Padstack exit directions only (`library/padstack.rs`, `get_trace_exit_directions`); `rules.pin_edge_to_turn_dist()` and its setter (`rules/mod.rs:1195-1201`) | `Pin.getTraceExitRestrictions` (`Pin.java:243-309`), `hasTraceExitRestrictions` (`:310`), `isPlacedOnFront` (`:557`), `getMinWidth` (`:567`), `getTraceNeckdownHalfwidth` (`:586`), `getMaxWidth` (`:592`), `calcNearestExitRestrictionDirection` (`:644`), `nearestTraceExitCorner` (`:713`) |
| — | `PolylineTrace.checkConnectionToPin` (`:1014-1080`), `correctConnectionToPin` (`:1082-1250`), `swapConnectionToPin` (`:1252-1313`) |
| `get_trace_tail` (`board/trace_ops.rs:704`), `insert_trace` (`basic_board.rs:289`) | `RoutingBoard.connectToTrace` (`:1150-1204`) |
| `Pin::changed_to` field for the SES writer | `Pin.getSwappablePins` (`:467-503`), `swap` (`:513-540`) |
| Edge-pin-net cache invalidated at insert, remove, bbox change, net assign, undo/redo (`basic_board.rs:205,800,865,1892,2028`) | Also invalidated by `Item.moveBy` for pins (`Item.java:307-309`) and by outline transforms (`BoardOutline.java:273,285,298,310`) |

## Decisions made here

- **Operations live in new files:** `board/item_ops.rs` (moves, transforms,
  copies, `connect_to_trace`), `board/pin_exits.rs` (exit restrictions, widths,
  swap), `board/trace_pin_connection.rs` (the three fixes). Each holds `impl
  BasicBoard` / `impl RoutingBoard` blocks.
- **A board-level move is Java's sequence.** `save_for_undo`, remove from the
  trees, transform, clear derived data (B02.8's log records it), insert into the
  trees, and invalidate the edge-pin cache for pins and outlines. Only the item
  structs in `board/items/mod.rs` change for the per-class transform math.
- **`atan2` and `toRadians` keep Java's expressions.** Java's own results can
  differ by one ulp across JVMs (D6). The probes run on the pinned JDK, and a
  direction that lands on a rounding boundary is reported, not hidden.
- **Pin swap is ported** although only the interactive pin-swap state uses it:
  it is small, and I03 needs it.

## File ownership

| Item | New files | May edit |
|---|---|---|
| B05.1 | `board/item_ops.rs`, `tests/item_transform.rs`, `tools/java-reference/probes/board/ItemOpsProbe.java`, `fixtures/reference/probe/item-ops/*` | `board/mod.rs` (append), `board/items/mod.rs` (transform math per item kind) |
| B05.2 | — | `board/item_ops.rs`, `board/structure/component.rs` (`Components::copy`, `Component::restore_pose`) |
| B05.3 | `board/pin_exits.rs`, `tools/java-reference/probes/board/PinExitsProbe.java`, `tests/board_probe_pin_exits.rs`, `fixtures/reference/probe/pin-exits/*` | `board/mod.rs` (append) |
| B05.4 | — | `board/pin_exits.rs` |
| B05.5 | `board/trace_pin_connection.rs`, `tools/java-reference/probes/board/PinConnectionProbe.java`, `tests/board_probe_pin_connection.rs`, `fixtures/reference/probe/pin-connection/*` | `board/mod.rs` (append) |
| B05.6 | — | `board/item_ops.rs`, `ItemOpsProbe.java` (append a stage) |
| B05.7 | — | `board/pin_exits.rs` |
| B05.8 | — | `tests/java_ports.rs`, `tests/item_transform.rs` |

## Checklist

- [ ] **B05.1 — Move and transform items on the board.**
  - Java: `Item.moveBy` (`Item.java:302-316`): `saveForUndo`, remove from the
    trees, `translateBy`, insert into the trees, invalidate the edge-pin cache
    for a pin. The per-class transforms (table above) each end in
    `clearDerivedData`. `DrillItem.moveBy` (`DrillItem.java:96-146`):
    - before moving, collect a `TraceInfo(layer, halfWidth, clearanceClass)` for
      every trace among `getNormalContacts()` (highest id first) into a
      `TreeSet` ordered by layer **descending**, which keeps only the first trace
      per layer (`TraceInfo.compareTo`, `:412-414`);
    - move;
    - then for each kept `TraceInfo`, `insertTrace` from the old centre to the new
      one through a 45° corner (`fortyfiveDegreeCorner(new, true)`) or 90° corner
      (`ninetyDegreeCorner`) by the angle restriction, or straight for any angle.
  - Rust: `BasicBoard::move_item(id, vector)`, `turn_90_degree(id, factor,
    pole)`, `rotate_approx(id, degrees, pole)`, `change_placement_side(id,
    pole)` for traces, vias, pins and the outline, with the per-kind math in
    `board/items/mod.rs`. A drill item's `DrillData` shapes are recomputed after
    its centre moves. The edge-pin cache is invalidated for pins and outline
    transforms (Java `BoardOutline.java:273-310`).
  - Probe `item-ops` stage `move`: on Issue508-DAC2020_bm05 with its SES, move the
    20 highest-id vias by (+500, +250) one after another. Log after each move
    every item's id, kind and geometry, `maxGeneratedId`, and the default tree's
    fingerprint (B02.1). Then undo once and log the state hash's equality with
    the state before the moves (B01.5).
  - **Accept:**
    - The `move` stage equal.
    - `edge_pin_cache_invalidated_on_pin_move` and
      `edge_pin_cache_invalidated_on_outline_transform`.
    - Unit tests for `turn_90_degree` (exact), `change_placement_side` and
      `rotate_approx` on a trace, a via and a pin, against values from the probe.

- [ ] **B05.2 — Copies.**
  - Java: `copy(int id)` takes a new id when `id <= 0`. Callers: `copy(0)` in
    `gui/interactive/CopyItemState.java:56,63,237` and `MoveItemState.java:73`
    (both inert at the pin, but I03 may revive them); `copy(getId())` in
    `ComponentItemTransform.java:86` and `Item.clone` (`Item.java:261`).
    `Components.copy` (`Components.java:98`); `Component.restorePose`
    (`Component.java:148`), used by `ComponentItemTransform`'s rollback.
  - Rust: `BasicBoard::copy_item(id, new_id: Option<ItemId>) -> Item`, which
    burns an id (B01.2) for `None`, as `copy(0)` does. The copy is not inserted;
    the caller inserts it. Plus `Components::copy` and `Component::restore_pose`.
  - **Accept:** `copy_zero_burns_an_id`, `copy_with_id_burns_none`,
    `restore_pose_round_trip` (move, rotate, then restore gives the original pose
    exactly).

- [ ] **B05.3 — Pin exit restrictions.**
  - Java: `Pin.getTraceExitRestrictions(layer)` (`:243-309`) turns the padstack's
    exit directions into board directions. It applies the component's rotation
    and side, then `Math.atan2` and `Direction.getInstanceApprox`.
    `hasTraceExitRestrictions` (`:310`), `isPlacedOnFront` (`:557`),
    `calcNearestExitRestrictionDirection(polyline, halfWidth, layer)` (`:644-711`),
    `nearestTraceExitCorner(from, halfWidth, layer)` (`:713-749`).
  - Rust: `board/pin_exits.rs`: `trace_exit_restrictions(pin, layer) ->
    SmallVec<[TraceExitRestriction; 4]>` in Java's order, plus the other four.
  - Probe `pin-exits`: every pin of Issue955-rotated-pad-contacts, Issue145-smoothieboard,
    Issue026-J2_reference and one board with back-side components (the probe
    finds the first in `fixtures.tsv` and records it). It logs the restrictions
    per layer, `hasTraceExitRestrictions`, `isPlacedOnFront`, and
    `nearestTraceExitCorner` for 5 seeded points per pin.
  - **Accept:** `board_probe_pin_exits` equal; any line that differs only because
    `atan2` landed on a direction boundary is listed in the test with the raw
    values, not dropped.

- [ ] **B05.4 — Pin widths.**
  - Java: `Pin.getMinWidth(layer)` (`:567-584`), `getTraceNeckdownHalfwidth(layer)`
    (`:586-590`), `getMaxWidth(layer)` (`:592-606`).
  - Rust: `pin_min_width`, `trace_neckdown_half_width` (the spelling A04 and Q03 use), `pin_max_width` in `board/pin_exits.rs`, each taking the pin id and
    the layer.
  - **Accept:** added to the `pin-exits` probe output (every pin, every layer),
    equal.

- [ ] **B05.5 — Pin-connection fixes.**
  - Java: `PolylineTrace.checkConnectionToPin` (`:1014-1080`),
    `correctConnectionToPin(atStart, angleRestriction)` (`:1082-1250`, which calls
    `checkPolylineTrace` at `:1207`, so an id is burned, and `change` at
    `:1229`), `swapConnectionToPin(atStart)` (`:1252-1313`, which sets the contact
    trace's fixed state and calls `combine`). `pullTight` runs them only when the
    angle restriction isn't 90° and `pinEdgeToTurnDist > 0`, in the order: swap
    start, swap end, correct start, correct end (`:841-861`). Headless Java
    leaves `pinEdgeToTurnDist` at 0, so this matters for interactive work and
    for O01's recursion.
  - Rust: `RoutingBoard::check_connection_to_pin(trace, at_start)`,
    `correct_connection_to_pin(trace, at_start, angle) -> bool`,
    `swap_connection_to_pin(trace, at_start) -> bool`. The `change` call passes
    `LineReuse` derived from how Java builds the corrected polyline; say how in
    a `// PARITY:` comment.
  - Probe `pin-connection`: on Issue026-J2_reference with its SES,
    Issue508-DAC2020_bm08 with the routed SES and Issue145-smoothieboard, set
    `rules.setPinEdgeToTurnDist` to 2× the smallest trace half width. For every
    trace end at a pin, in item-list order, call check, then correct, then swap.
    Log the booleans, the resulting polyline and `maxGeneratedId`.
  - **Accept:** `board_probe_pin_connection` equal on all three boards.
    `smoothenEndCornersFork` (`:893`) has no caller at the pin and is not ported;
    a one-line note in the module doc says so.

- [ ] **B05.6 — `connect_to_trace`.**
  - Java: `RoutingBoard.connectToTrace(fromPoint, toTrace, halfWidth, class)`
    (`:1150-1204`):
    - only polyline traces; true if the trace already contains the point;
    - else the projection line from the point onto the trace, which must be a
      3-line polyline;
    - `checkPolylineTrace` (an id is burned);
    - join the changed area at each corner while marking;
    - `insertTrace` of the connection, unfixed;
    - then remove the trace tail at the trace's first corner, and then at its
      last corner, each unless the point is that corner or the tail is
      user-fixed.
  - Rust: `RoutingBoard::connect_to_trace(from, trace, half_width, class) -> bool`.
  - Probe `item-ops` stage `connect`: on Issue026-J2_reference with its SES, for
    50 seeded (point, trace) pairs with the point within 5 trace widths of the
    trace, log the result, the inserted and removed ids and `maxGeneratedId`.
  - **Accept:** stage `connect` equal.

- [ ] **B05.7 — Pin swap.**
  - Java: `Pin.getSwappablePins` (`:467-503`) through the component's logical
    part (`library/LogicalPart`), and `Pin.swap(other)` (`:513-540`), which
    exchanges the nets and sets `changedTo`.
  - Rust: `swappable_pins(pin) -> BTreeSet<ItemId>` (highest id first) and
    `swap_pins(a, b) -> bool` in `board/pin_exits.rs`.
  - **Accept:** unit tests on a fixture with a logical part (find one with
    `rg logical_part fixtures/*.dsn`; if none, a synthetic DSN in
    `fixtures/synthetic/`): the swappable set, and the nets and `changed_to` after
    a swap and after undo.

- [ ] **B05.8 — The remaining placement tests.**
  - Java tests: `fixtures/Issue955PlacementLifecycleTest.java` (14 tests: contact
    kept through the DSN round trip, quarter, arbitrary and mirror turns, copy
    rebinding, mirrored exit directions); `fixtures/Issue955RotatedPadContactTest.java`
    (load-only: S21 pins `2@1` and `1` each have exactly one normal contact after
    import, and the contact ids are as Java reports them; added 2026-10-16);
    `board/BoardItemRenderingMetadataTest.java`
    (86 lines); `board/actions/ComponentItemTransformTest.java` (the cases that
    don't need `board.actions.ComponentItemTransform`).
  - Rust: port into `tests/item_transform.rs` and `tests/java_ports.rs`. Cases
    that need the SES writer (C05) or `ComponentItemTransform` (I04) are listed in
    the test file's header with their owner.
  - **Accept:** ported tests green; the header names each skipped Java test and its
    owner.
