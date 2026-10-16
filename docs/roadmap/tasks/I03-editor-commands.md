# I03 — Editor and commands

Opened 2026-10-15. I01 and I02 port the two long-running interactions (route
and drag). This checklist ports the rest of the reachable editor at
`cda540f` behind one entry point, `Editor::apply(Command) -> Response`:
mode handling, selection, pin swap, keepout construction, and undo/redo with
board deltas for the GUI. It also ports delete and fix/unfix of the
selection if Dan approves decision 1 in [I00](I00-interactive-README.md)
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

- [ ] **I03.1 — `Editor` and dispatch.**
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

- [ ] **I03.2 — Selection.**
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

- [ ] **I03.3 — Pin swap.**
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

- [ ] **I03.4 — Keepout construction.**
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

- [ ] **I03.5 — History.**
  - Java: `GuiBoardHistoryController.undo/redo` (`:30-56`) and the snapshot
    points listed in I00.
  - Rust: `interactive/history.rs`.
  - **Accept:** `route_undo_redo_round_trip`: a scripted route, drag, pin
    swap and keepout on J2, then undo of each and redo of each, gives
    `state_hash` values equal to those recorded after each step; undo in a
    routing mode is ignored.

- [ ] **I03.6 — Delete and fix/unfix of the selection (decision 1).**
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

- [ ] **I03.7 — Probe `editor`.**
  - Java: `EditorProbe` calls the board operations headless: the selection
    extensions (board queries), region queries, `checkChangeNet` + `Pin.swap`,
    `checkShape` + `insertObstacle`; it transcribes the states' few board
    calls and says so in its header.
  - Boards: `Issue026-J2_reference` + SES, `Issue508-DAC2020_bm05` + SES,
    the in-memory pin-swap board from I03.3.
  - **Accept:** selection id sets, swap results and inserted keepouts equal
    on all three boards.

- [ ] **I03.8 — Replay and determinism.**
  - `examples/edit_script.rs` accepts every `Command`.
  - **Accept:** a committed mixed script (route, drag, select, swap,
    keepout, undo, redo) on J2 replays to identical output under
    `tools/determinism-check.sh`.

**Note on copy.** The series map put copy in I03. `CopyItemState` is dead at
the pin like `MoveItemState`, and both pick a selection up and place it with
a preview, new ids (`copy(0)` burns one) and, for copy, new padstacks and
packages. They share that machinery, so copy is in I04.
