# I03 — Editor and commands

Opened 2026-10-15. I01 and I02 port the two long-running interactions (route
and drag). This checklist ports the rest of the reachable editor at
`cda540f` behind one entry point, `Editor::apply(Command) -> Response`:
mode handling, selection, pin swap, keepout construction, and undo/redo with
board deltas for the GUI. It also ports delete and fix/unfix of the
selection if Dan approves decision 1 in [I00](../I00-interactive-README.md)
(the same question as G00's question 6); both are disabled in Java's GUI at
the pin. Copy of a selection moves to
I04 with the other "pick up and place" edits (see the note at the end).

**Prerequisites:** I01, I02; B01 (change journal, undo/redo with changed
nets); B04 (`ItemSelectionFilter`, `check_shape`, `check_change_net`);
B05 (`get_swappable_pins`, `swap`; if B00 places pin swap elsewhere, follow
B00); O01 (`remove_items_and_pull_tight`).

## What exists and what this adds

| Exists | Where |
|---|---|
| `insert_obstacle` `:604`, `insert_via_obstacle` `:627`, `overlapping_items` `:1107`, `connectable_items`, `connected_set` `:1535`, `connection_items` `:1788`, `component_items`, `is_deletion_forbidden` `:841`, `remove_items` `:872`, `undo`/`redo` with changed nets `:1986-2005` | `board/basic_board.rs` |
| `RouteSession`, `DragSession`, `EditSettings`, replay example | I01, I02 |

| Java at `cda540f` (`src/main/java/app/freerouting/`) | Lines |
|---|---|
| `gui/interactive/InteractiveStateController.java` (dispatch), `MenuState.java` (keys → actions), `RouteMenuState`, `DragMenuState`, `InspectMenuState` | 300, 136, 36, 28, 288 |
| `gui/interactive/InspectedItemState.java`: `extentToWholeNets` `:75`, `extentToWholeComponents` `:100` (no filter afterwards), `extentToWholeConnectedSets` `:122`, `extentToWholeConnections`, `toggleSelect`, `filter` | 256 |
| `gui/interactive/InspectItemsInRegionState.java`: `selectAllInRegion` `:58-89`, `findItems` `:91-97` | 98 |
| `gui/interactive/PinSwapState.java`: `leftButtonClicked` `:43`, `complete` `:65-108` | 121 |
| `gui/interactive/CornerItemConstructionState.java` (`addCornerForSnapAngle` `:80`, `snap` `:102`), `CircleConstructionState.complete` `:54-95`, `TileConstructionState` (`removeConcaveCorners` `:92`), `PolygonShapeConstructionState`, `HoleConstructionState` | 119, 116, 179, 87, 163 |
| `gui/workspace/controllers/GuiBoardHistoryController.java` (`canChangeHistory` `:58`) | — |
| `src/test/java/app/freerouting/gui/interactive/InteractiveCommandTest.java`, `InteractiveStateCommandAdapterTest.java` (Swing glue; replaced, not ported) | 41, 77 |

## Decisions made here

- **One `Editor`, a small mode enum.** `ModeKind::{Menu(Tool), Routing {
  stitch }, Dragging, MakingSpace, Selecting, PinSwap, Constructing }`. The
  per-state Java classes collapse into match arms over `Command`.
- **Selection is an ordered id set** (`BTreeSet<ItemId>`, highest id first
  as Java's `TreeSet<Item>`), stored in the editor and reported in
  `Response.overlay.selection`.
- **Deltas come from B01.7's `take_changes()`**, drained into `Response.delta` after
  every command. Undo and redo report Java's changed nets so the GUI can
  refresh the ratsnest.
- **Construction keeps Java's shape choice:** a circle keepout becomes its
  bounding box in 90° mode and its bounding octagon in 45° mode before
  `check_shape`; clearance class "none"; snapshot only on success.

## File ownership

| Item | New files | May edit |
|---|---|---|
| I03.1 | `interactive/editor.rs` | `interactive/mod.rs` (append) |
| I03.2 | `interactive/select.rs` | — |
| I03.3 | `interactive/pin_swap.rs` | — |
| I03.4 | `interactive/construct.rs` | — |
| I03.5 | `interactive/history.rs` | — |
| I03.6 | `interactive/edit_ops.rs` | `docs/DIVERGENCES.md` |
| I03.7 | `tools/java-reference/probes/board/EditorProbe.java`, `tests/interactive_editor.rs`, `fixtures/reference/probe/editor/*` | `tools/java-reference/README.md` |
| I03.8 | — | `examples/edit_script.rs` (append) |

## Checklist

- [x] **I03.1 — `Editor` and dispatch.**
  - Java: `InteractiveStateController` dispatch, `MenuState` and the three
    menu states' engine semantics.
  - Rust: `interactive/editor.rs`: `Editor::{new, apply, board, into_board}`;
    `Tool::{Route, Drag, Select}`; `Command` and `Response` as in I00.
  - Rules: `Click`/`Press` in a menu mode starts the tool's session;
    `Complete`/`Cancel` go to the active session; `SetLayer` and the layer
    commands apply Java's layer-change path when routing; `Undo`/`Redo` act
    only in menu modes.
  - **Accept:** `editor_dispatch_table`: one test row per (mode, command)
    pair Java handles, with the expected next mode; it replaces
    `InteractiveStateCommandAdapterTest`.
  - *Done 2026-10-16:* `interactive/editor.rs`: `Editor::{new, apply, board, into_board}`
    (plus `settings`, `options`, `selection`, `mode`, `route_session`), `Tool`, `ModeKind`
    (the checklist's modes plus `SelectingRegion` for `InspectItemsInRegionState`), `Command`
    (every I00 command plus the layer keys, `TogglePush`, `UpdateOptions`, `StartKeepout`,
    `StartHole`, `DeleteSelection`, `FixSelection`), `Response` with the drained B01.7 delta,
    `Overlay` (selection, route overlay, swap pins, construction, region). `editor_dispatch_table`
    (74 rows over every mode: route, drag and select menus, dynamic and stitch routing, dragging,
    making space, selecting, region, pin swap, constructing; next mode and Java's status key per
    row; undo ignored outside menu modes; a via press on routed J2): `cargo test -p freerouting
    --test interactive_editor` → 13 passed. Java's `InteractiveCommandTest` and
    `InteractiveStateCommandAdapterTest` are Swing glue and are replaced by the table, not ported.

- [x] **I03.2 — Selection.**
  - Java: `InspectedItemState` (toggle, extend to nets, components,
    connected sets, connections; filter), `InspectItemsInRegionState`,
    `GuiBoardInteractionController.pickItems` (all-visible-layers fallback).
  - Rust: `interactive/select.rs`.
  - Orders and quirks: extend to nets collects net numbers of connectable
    selected items (ascending) and replaces the selection with their
    connectable items, then filters; extend to components keeps the old
    selection, adds component items and does **not** filter; connected sets
    use `connected_set(-1)`; region select uses `overlapping_items` with
    layer −1 when "select on all visible layers", then drops items on no
    visible layer, then applies the settings' filter; an empty result keeps
    the previous mode with status `nothing_selected`.
  - **Accept:** I03.7 selection cases; unit tests for the component quirk.
  - *Done 2026-10-16:* `interactive/select.rs` (`pick_items`, `toggle`, `extend` with the four
    `Extent`s, `filter`, `region_box`, `select_in_region`). I03.7's selection lines (12 picks and
    four extensions plus filter per board, 6 regions on all layers and on layer 0) equal Java on
    all three boards (`editor_matches_java_{j2,bm05,pin_swap_board}`). Component quirk:
    `extend_to_components_keeps_the_selection_and_does_not_filter` (keeps the pin, adds every U1
    item through a traces-only filter; nets through it stay an empty selection as Java's ignored
    `filter()` result; a lone keepout leaves the state). Empty region → `nothing_selected` and
    the previous mode: dispatch rows `region release on nothing`.

- [x] **I03.3 — Pin swap.**
  - Java: `PinSwapState` (`:43-108`), `Pin.getSwappablePins`, `Pin.swap`,
    `RoutingBoard.checkChangeNet`.
  - Rust: `interactive/pin_swap.rs`.
  - Orders (exact): the second pin must be in the first pin's swappable set
    (PINS pick); pins with more than one net are refused; `check_change_net`
    of the first pin to the second's net (−1 when it has none), then the
    reverse, each with its own status key; snapshot; swap; changed nets of
    both pins in the delta.
  - No fixture has a logical part (no `logical_part` anywhere in
    `fixtures/`). The test and the probe add one to the text of
    `Issue143-rpi_splitter.dsn` in memory (a part with two swappable pins
    of one gate), the way Java's Issue954 test edits its DSN text.
  - **Accept:** a swap on that board matches the Java probe (nets of both
    pins and the board dump); a connected pin is refused with
    `pin_not_swapped_because_it_is_already_connected`.
  - *Done 2026-10-16:* `interactive/pin_swap.rs` (`PinSwap::{start, for_pin, click, complete}`,
    `SwapOutcome`). The probe's pin-swap board (rpi_splitter with `PART_LIBRARY` for J3 in its
    DSN text): the swappable sets of J3's 9 pins, 12 swaps as loaded (all `pin_swap_completed`,
    nets of both pins after each) and the pin-net dump equal Java
    (`editor_matches_java_pin_swap_board`); with the session all 12 are refused with Java's two
    keys. `a_connected_pin_is_refused` (D+ to D- on the routed board:
    `pin_not_swapped_because_it_is_already_connected`, board unchanged). Java's
    `getSwappablePins` indexes the logical part by the package pin number, so pin 24 lists
    itself and GND gets a set; the port reproduces it (B05's `swappable_pins`). Found and fixed
    on the way: `Components::set_logical_part` keeps the undo list's copy in step (an undo used
    to drop the part; `a_logical_part_survives_an_undo`).

- [x] **I03.4 — Keepout construction.**
  - Java: `CornerItemConstructionState` (corner snapping to the angle
    restriction), `CircleConstructionState.complete`,
    `TileConstructionState` (concave corners removed while drawing and at
    close), `PolygonShapeConstructionState`, `HoleConstructionState` (adds a
    hole to an obstacle or conduction area: remove and reinsert).
  - Rust: `interactive/construct.rs` with `Command::InsertKeepout` and
    `Command::AddHole`.
  - **Accept:** I03.7 construction cases: the inserted area items equal
    Java's on 3 boards; an overlapping circle is refused with
    `keepout_cancelled_because_of_overlaps` and no snapshot.
  - *Done 2026-10-16:* `interactive/construct.rs` (`Construction::{keepout, hole, hole_in,
    click, pointer_moved, snap, complete}`, `insert_keepout` for `Command::InsertKeepout`,
    `add_hole` for `Command::AddHole`; the corner logic moved from the GUI). The probe's
    construction lines (per angle restriction 3 circles, 3 tiles, 3 polygons in the board box,
    a hole in each inserted tile and polygon, a circle on a pin; id, tile shapes and
    `maxGeneratedId`) equal Java on J2, bm05 and the pin-swap board
    (`editor_matches_java_{j2,bm05,pin_swap_board}`; keepout and hole lines: J2 40 with 24
    inserted, bm05 35 with 10, pin-swap board 39 with 26). `an_overlapping_circle_is_refused_without_a_snapshot` (circle on U1-2:
    `keepout_cancelled_because_of_overlaps`, state hash unchanged, nothing to undo). The GUI's
    `tools/keepout.rs` now wraps `Construction`, and its Hole entry is enabled
    (`tile_and_circle_keepouts` adds a hole).

- [x] **I03.5 — History.**
  - Java: `GuiBoardHistoryController.undo/redo` (`:30-56`) and the snapshot
    points listed in I00.
  - Rust: `interactive/history.rs`.
  - **Accept:** `route_undo_redo_round_trip`: a scripted route, drag, pin
    swap and keepout on J2, then undo of each and redo of each, gives
    `state_hash` values equal to those recorded after each step; undo in a
    routing mode is ignored.
  - *Done 2026-10-16:* `interactive/history.rs` (`undo`, `redo`, `HistoryStep` with Java's
    changed nets and keys); the editor calls them only in menu modes. `route_undo_redo_round_trip`
    on J2 (as loaded, with a logical part for U1 in its DSN text, because J2 has none): a route
    J2-A2 → U1-2 (four moves, complete), a drag of U1 by two steps, a pin swap U1-8/U1-9 and a
    tile keepout, then four undos and four redos; each restores the fingerprint recorded after
    that step, and undo while routing is ignored with no message. The fingerprint is the item
    content (id, kind, nets, fixed state, component, class, tile shapes), not `state_hash`:
    `state_hash` follows Java's `getHash`, which covers the undo structure (`redo_possible`,
    the version nodes), so a state reached by undo can never hash equal to the state recorded
    before it, in Java either.

- [x] **I03.6 — Delete and fix/unfix of the selection (decision 1).**
  - Only if Dan approves; otherwise mark `[~]` with the decision as the
    remainder.
  - Rust: `interactive/edit_ops.rs`.
    - Delete: one snapshot; items that are deletion-forbidden or
      user-fixed stay and the status is
      `some_items_are_fixed_and_could_therefore_not_be_removed`; the rest go
      through `remove_items_and_pull_tight` with the tidy width and accuracy
      when push is on, else `remove_items` (the pattern of
      `RouteState.cancel`).
    - Fix/unfix: set the fixed state of the selection (one snapshot).
  - Each gets a DIVERGENCES line ("disabled in Java's GUI at the pin;
    enabled here by decision").
  - **Accept:** unit tests per operation on J2 (item counts, fixed states,
    the status for a fixed item), and undo restores the `state_hash`.
  - *Done 2026-10-16:* decision 1 accepted by Dan (I00, 2026-10-16). `interactive/edit_ops.rs`
    (`delete_selection`, `fix_selection`, `set_fixed_state_undoable`). On routed J2:
    `delete_selection_keeps_fixed_items_and_undo_restores` (push on and off: 6 traces and a pin
    selected, one trace user fixed → 5 removed, 2 refused with
    `some_items_are_fixed_and_could_therefore_not_be_removed`; one undo restores the count and
    the content); `fix_and_unfix_are_undoable` (4 vias unfixed then fixed, a second fix changes
    nothing and takes no snapshot, two undos restore the states and the content). DIVERGENCES,
    "Interactive editor", has both lines ("disabled in Java's GUI at the pin; enabled here by
    decision"). The GUI's `tools/edit.rs` now calls these (delete with push off, as before).

- [x] **I03.7 — Probe `editor`.**
  - Java: `EditorProbe` calls the board operations headless: the selection
    extensions (board queries), region queries, `checkChangeNet` + `Pin.swap`,
    `checkShape` + `insertObstacle`; it transcribes the states' few board
    calls and says so in its header.
  - Boards: `Issue026-J2_reference` + SES, `Issue508-DAC2020_bm05` + SES,
    the in-memory pin-swap board from I03.3.
  - **Accept:** selection id sets, swap results and inserted keepouts equal
    on all three boards.
  - *Done 2026-10-16:* `tools/java-reference/probes/board/EditorProbe.java`,
    `fixtures/reference/probe/editor/{Issue026-J2_reference,Issue508-DAC2020_bm05,Issue143-rpi_splitter}.jsonl`
    (60, 55 and 94 lines), `tests/interactive_editor.rs`. Generated at `a773a70ec` with
    `JDK_JAVA_OPTIONS=-Xmx3g REFERENCE_WORK_DIR=… TMPDIR=… tools/java-reference/probe.sh <pin>
    tools/java-reference/probes/board/EditorProbe.java fixtures/<board>.dsn
    fixtures/reference/route/<board>.ses <out>` per board (Java J2 1.4 s, bm05 1.6 s, pin-swap
    0.6 s). `cargo test -p freerouting --test interactive_editor editor_matches_java` → 3 passed:
    selection id sets, swap results and inserted keepouts equal on all three boards.

- [x] **I03.8 — Replay and determinism.**
  - `examples/edit_script.rs` accepts every `Command`.
  - **Accept:** a committed mixed script (route, drag, select, swap,
    keepout, undo, redo) on J2 replays to identical output under
    `tools/determinism-check.sh`.
  - *Done 2026-10-16:* `tests/support/editor_script.rs` (a name for every `Command`;
    `"tool": "editor"` scripts, `dsn_insert` puts a logical part before `(network`),
    `examples/edit_script.rs` dispatches on it, `tests/data/edit_script_editor_j2.json` (64
    commands: route, undo, redo, drag, make-space, select with all four extensions and filter,
    region, fix, unfix, delete, undo, pin swap, tile, circle and polygon keepouts, layer keys,
    settings, three undos, two redos, a stitch route with a snapshot, a layer change and a
    cancel). `tools/determinism-check.sh --out … -- <target>/release/examples/edit_script
    fixtures/Issue026-J2_reference.dsn crates/freerouting/tests/data/edit_script_editor_j2.json
    {OUT}` → `identical` (74 lines; load 2.79 idle, 3.37 with 8 burners).
    `editor_script_replays_identically`: two replays and an unlimited-budget replay are equal,
    and the route, drag, pin swap, keepout, undo and redo lines show their outcomes.

**Note on copy.** The series map put copy in I03. `CopyItemState` is dead at
the pin like `MoveItemState`, and both pick a selection up and place it with
a preview, new ids (`copy(0)` burns one) and, for copy, new padstacks and
packages. They share that machinery, so copy is in I04.
