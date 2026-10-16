# G05 — Inspect mode, selection tools and safe edits

Opened 2026-10-15. After G03 the app can show a selection, but the user can
only make one through the info lists. This checklist adds Java's inspect
mode: click and region select through the selection filter, extending a
selection to nets, connected sets, connections and components, the
selection toolbar, context menus, undo and redo that patch the scene
instead of rebuilding it, and keepout construction. It is the report's
GUI-5 (GUI, plugin and packaging report §4).

**Prerequisites:**
- [G03](G03-ratsnest-drc-and-info-lists.md) (selection model, overlays,
  delete-all with undo) and [G02](G02-gpu-board-renderer.md) G02.4 (chunk
  patching by `ItemId`).
- B01: the board change journal (`take_changes`), for G05.5.
- B04: the selection filter (`ItemSelectionFilter`, filtered picks), for
  G05.1–G05.3.
- I03 (editor commands: delete, new net, keepout construction) for G05.4's
  edit buttons and G05.7. See "Java's disabled edits" below.

## What exists and what this adds

| Exists | Where |
|---|---|
| Picks and item lists | `BasicBoard::pick_items(&Point, layer)`, `overlapping_items(&Area, layer)`, `connectable_items(net)`, `connected_set(..)`, `connection_items(..)`, `component_items(component_id)`, `bounding_box_of(&[ItemId])` in `crates/freerouting/src/board/basic_board.rs` |
| Undo and redo | `BasicBoard::generate_snapshot()`, `undo(Option<&mut BTreeSet<i32>>) -> bool`, `redo(..)`, returning the changed nets |
| Edits Java would call | `remove_items(ids) -> bool` (false if some were fixed), `is_deletion_forbidden(&Item)`, `set_fixed_state(id, FixedState)`, `unfix(id)`, `change_clearance_class(id, class)`, `assign_net_no(id, net)`, `insert_obstacle(Area, layer, clearance_class, FixedState)` |
| Selection, overlays, `Command`, Java key set | G01 (`src/commands.rs`, `src/input.rs`), G03 (`src/selection.rs`, `src/overlays/`) |
| Chunk patching | G02.4: `ChunkSet::patch(removed, added)` in `src/canvas/chunks.rs`, counter `full_rebuilds` in `src/stats.rs` |
| Java at `cda540f` | `gui/interactive/{InspectMenuState,InspectedItemState,InspectItemsInRegionState,InteractiveStateController,TileConstructionState,CircleConstructionState,PolygonShapeConstructionState,HoleConstructionState,CornerItemConstructionState}.java`, `gui/board/BoardToolbarInspectedItem.java`, `gui/menus/{PopupMenuMain,PopupMenuInspectedItems,PopupMenuDisplay}.java`, `gui/windows/routing/WindowSelectParameter.java`, `gui/workspace/controllers/{GuiBoardHistoryController,GuiBoardLegacyEditActions}.java`, `board/actions/ItemSelectionFilter.java` |

What this adds: `src/tools/inspect.rs`, `src/tools/popup.rs`,
`src/tools/keepout.rs`, `src/panels/select_settings.rs`,
`src/panels/selection_toolbar.rs`, undo/redo with patching in
`src/session.rs`, and `tests/ui_edit.rs`.

**Java's disabled edits.** At `cda540f` the inspect toolbar has only Cancel,
Info, the four "extend selection" buttons, Violations and three zoom
buttons (`BoardToolbarInspectedItem` constructor). Delete, Fix, Unfix, New
Net, New Component, Cutout, Assign Clearance Class, Pull Tight and
Autoroute of a selection all route to `GuiBoardLegacyEditActions`, whose
methods are empty ("intentionally inert compatibility hooks"). Their texts
are still in `BoardToolbarInspectedItem_en.properties`. The GUI report lists
them as Java features; at the pin they are not reachable. Adding them is an
owner decision (the same one I04 records for selection transforms).

## Decisions made here

- **The GUI holds the selection; the engine decides what is selectable.**
  The filter is B04's port of `ItemSelectionFilter`. The GUI stores the
  user's filter choices and passes them to the engine pick. It never
  re-implements the type tests.
- **Extend operations copy Java's set semantics** (`InspectedItemState`):
  - nets (N): the selection is *replaced* by `connectable_items` of every
    net of the selected connectable items, then filtered;
  - connected sets (S) and connections (E): replaced by the union over the
    selected connectable items, then filtered;
  - components (B button only): the old selection *plus* the items of
    every component with id > 0; Java does not filter this one.
  If the result is empty, Java leaves inspect mode; the port clears the
  selection. The `Selection` is a `BTreeSet<ItemId>`, so its order is item
  id order, as Java's `TreeSet<Item>`.
- **B stays redo.** The toolbar tooltip says "(B)" for components, but
  `InspectedItemState.keyTyped` has no `b` case, and `MenuState` maps `b` to
  redo. Components is a toolbar and context-menu action only.
- **Undo and redo patch the scene.** Each undoable command calls
  `generate_snapshot()` first, as Java does. After `undo`/`redo`, the GUI
  drains B01's journal and patches chunks; it rebuilds only when the journal
  reports overflow. The changed nets that `undo` returns go to the ratsnest
  update (Java `GuiBoardHistoryController.undo`: `changedNets.forEach(updateRatsnest)`).
- **Status texts are Java's**: `GuiBoardManager_en.properties` `undo`,
  `redo`, `no_more_undo_possible`, `no_more_redo_possible`; Java allows
  undo only in a menu state (`canChangeHistory`), so the port refuses undo
  while a construction or route tool is active.
- **Edit buttons beyond Java are behind a decision.** G05.4 builds the
  toolbar with Java's reachable buttons first. The disabled ones are added
  only if Dan says so, and then call I03 commands, never board methods
  directly from the GUI.

## File ownership

Paths are under `crates/freerouting-gui/`.

| Item | New files | May edit |
|---|---|---|
| G05.1 | `src/tools/mod.rs`, `src/tools/inspect.rs`, `tests/ui_edit.rs` | `src/input.rs`, `src/commands.rs` (append variants), `src/lib.rs` (module list) |
| G05.2 | `src/panels/select_settings.rs` | `src/settings.rs` (persist the filter) |
| G05.3 | — | `src/tools/inspect.rs`, `src/commands.rs` |
| G05.4 | `src/panels/selection_toolbar.rs` | `src/text.rs` |
| G05.5 | — | `src/session.rs`, `src/canvas/chunks.rs` (calls only), `src/stats.rs` |
| G05.6 | `src/tools/popup.rs` | `src/input.rs` |
| G05.7 | `src/tools/keepout.rs`, `src/overlays/construction.rs` | `src/commands.rs` |

## Checklist

- [ ] **G05.1 — Inspect mode: click, toggle and region select.**
  - Java: `InspectMenuState` (click picks with the filter;
    `mouseDragged` starts `InspectItemsInRegionState`),
    `InspectedItemState.toggleSelect` (a single picked item is toggled; a
    click on nothing, or removing the last item, ends the state),
    `InteractiveStateController.selectItems`.
  - Rust: `tools::inspect::InspectTool` with states `Idle`,
    `Selecting { region_start }`. Left click calls B04's filtered pick at
    the cursor on the current layer (or all visible layers, G05.2); left
    drag draws a rectangle overlay and, on release, selects
    `overlapping_items` of that box through the filter. Ctrl+click toggles.
    Esc clears. The status line shows Java's
    `drag_left_mouse_button_to_select_items_in_region` and
    `in_inspect_mode` (`InteractiveState_en.properties`).
  - Order: when a pick returns several items, Java only toggles when
    exactly one is returned (`pickedItems.size() == 1`). Keep that rule.
  - **Waits on B04.**
  - **Accept:** `tests/ui_edit.rs`:
    - `region_select_picks_pins_only`: with the filter set to pins only, a
      raw-event left drag over a known pad cluster on
      `Issue159-setonix_2hp-pcb.dsn` selects exactly the pins whose ids the
      test computes with `overlapping_items` and the same filter.
    - `click_on_empty_space_clears_selection` and `ctrl_click_toggles`.
    - `left_drag_does_not_pan`: the camera centre is unchanged after a left
      drag (G01.7's rule still holds with the tool active).

- [ ] **G05.2 — Selection settings panel.**
  - Java: `WindowSelectParameter` ("General Settings"): selection layers
    (`all_visible`, `current_only`), selectable items (`TRACES`, `VIAS`,
    `PINS`, `CONDUCTION`, `KEEPOUT`, `VIA_KEEPOUT`, `COMPONENT_KEEPOUT`,
    `BOARD_OUTLINE`, `FIXED`, `UNFIXED`), and the current layer
    (`WindowSelectParameter_en.properties`).
  - Rust: `panels::select_settings` edits a `SelectionSettings { layers:
    SelectionLayers, selectable: BTreeSet<SelectableChoice> }` held by the
    app and persisted with G01.8's settings. `SelectableChoice` is B04's
    enum; the GUI does not define its own.
  - **Waits on B04.**
  - **Accept:** `tests/ui_edit.rs::current_layer_only_ignores_other_layers`:
    with "Current layer only" on layer 0, a click on a trace that is only
    on layer 1 selects nothing; with "All visible" it selects that trace.
    `selection_settings_persist`: a save/restore round trip keeps the
    choices.

- [ ] **G05.3 — Extend the selection.**
  - Java: `InspectedItemState.extentToWholeNets`,
    `extentToWholeConnectedSets`, `extentToWholeConnections`,
    `extentToWholeComponents`; keys N, S, E in `InspectedItemState.keyTyped`;
    toolbar texts `nets`, `conn_sets`, `connections`, `components`
    (`BoardToolbarInspectedItem_en.properties`).
  - Rust: `Command::ExtendSelection(ExtendKind)`, implemented in
    `tools::inspect` with the engine calls listed in the decisions, the
    filter applied after nets, sets and connections only.
  - **Accept:** `tests/ui_edit.rs::extend_selection_matches_engine`: on
    `Issue754-avionics_hub.dsn`, starting from one pin, each of N, S, E and
    the Components button gives exactly the set the test builds from
    `connectable_items`, `connected_set`, `connection_items` and
    `component_items` with the same rules. An empty result clears the
    selection.

- [ ] **G05.4 — Selection toolbar.**
  - Java (reachable at the pin): `BoardToolbarInspectedItem` buttons
    `cancel`, `info` (opens G04.3's item info), `nets`, `conn_sets`,
    `connections`, `components`, `violations` (shows the clearance
    violations of the selected items, status text
    `clearance_violations_found` prefixed by the count, Java
    `InspectedItemState.toggleClearanceViolations`), `zoom_selection` (W),
    `zoom_all`, `zoom_region`.
  - Rust: `panels::selection_toolbar` shown while the selection is not
    empty. Every button issues a `Command`. Violations of the selection
    filter G03's DRC findings to those touching a selected item.
  - Buttons beyond Java (`delete`, `fix`, `unfix`, `spacing`, `new_net`):
    *Operator decision* (G00, "Java's disabled edits"). The
    recommendation is to add Delete, Fix and Unfix only. If Dan says
    yes, each calls the I03 editor command, which owns the rules
    (`remove_items` returns false for fixed items and the status text is
    `some_items_are_fixed_and_could_therefore_not_be_removed`). Until then
    they are not shown, and this item can be ticked on the Java set alone.
  - **Accept:** `tests/ui_edit.rs::toolbar_buttons_match_java_set`: the
    toolbar's AccessKit labels are exactly Java's ten reachable buttons;
    `violations_of_selection_counts`: on a fixture with violations (pick one
    from `tests/reference_drc.rs`), selecting both items of one violation
    and pressing V shows `1 clearance violations found`.

- [ ] **G05.5 — Undo and redo with scene patching.**
  - Java: `GuiBoardHistoryController.undo/redo/generateSnapshot`, keys U
    and B (`MenuState.keyTyped`), plus Ctrl+Z and Ctrl+Y (G00 question 1).
  - Rust: `Session::undo()` and `redo()` take the board through
    `Arc::make_mut`, call `BasicBoard::undo(Some(&mut changed_nets))`, bump
    `edit_generation`, then drain B01's `take_changes()` and call
    `ChunkSet::patch(removed, added)`. On journal overflow they rebuild and
    count it in `full_rebuilds`. The ratsnest is recomputed for
    `changed_nets` only. G03.7's delete-all switches to this path.
  - Note: `BasicBoard::delete_all_traces_and_vias` does not increment
    `revision()` at HEAD (it deletes from the arena directly). Stale-result
    checks keyed on `revision()` miss it; B01's journal must record it.
  - **Waits on B01.**
  - **Accept:** `tests/ui_edit.rs`:
    - `random_edit_undo_redo_matches_fresh_scene`: 20 seeded random steps
      (`JavaRandom` seed 7) of delete-all, fix/unfix (or keepout insert),
      undo and redo on `Issue754-avionics_hub.dsn` leave the chunk draw
      list equal to one built from scratch from the final board, and
      `full_rebuilds == 0`.
    - `undo_at_start_says_nothing_more`: the status line shows
      `Nothing more to undo.`.
    - FT4 on Issue190 with a real undo of 200 removed and 200 added items:
      CPU ≤ 2 ms in release, recorded in the done note.

- [ ] **G05.6 — Context menus.**
  - Java: `PopupMenuMain` (`select_item`, `start_route`, `swap_pin`,
    `create_keepout` with `tile`, `circle`, `polygon`, `hole`;
    `PopupMenuMain_en.properties`), `PopupMenuInspectedItems` (an "extend
    selection to" header with the four extend items), and the display rows
    of `PopupMenuDisplay` (`center_display`, `zoom_in`, `zoom_out`; its
    bundle location is unverified). Right-click opens the menu of the
    current mode (`BoardPanel` javadoc).
  - Rust: `tools::popup` builds the menu from the mode and the selection;
    entries whose engine work has not landed (route, pin swap) are shown
    disabled with a tooltip naming the missing series.
  - **Accept:** `tests/ui_edit.rs::popup_entries_by_mode`: `click_secondary`
    on the canvas in inspect mode with nothing selected lists Java's
    `PopupMenuMain` entries; with a selection it lists the four extend
    entries plus the display rows (labels compared through AccessKit).

- [ ] **G05.7 — Keepout construction.**
  - Java: `TileConstructionState`, `CircleConstructionState`,
    `PolygonShapeConstructionState.complete` (at least 3 corners;
    `splitToConvex() == null` fails; then `RoutingBoard.checkShape(shape,
    layer, new int[0], clearanceClassNone)`; then `generateSnapshot()` and
    `insertObstacle(shape, layer, clearanceClassNone, UNFIXED)`),
    `CornerItemConstructionState.snap` and `addCornerForSnapAngle` (corners
    snapped to the snap angle), `HoleConstructionState`. Texts:
    `creating_tile`, `creating_circle`, `creating_polygonshape`,
    `keepout_successful_completed`, `keepout_cancelled_because_of_overlaps`,
    `adding_hole_*` (`InteractiveState_en.properties`).
  - Rust: `tools::keepout` collects clicked corners in board units and
    draws the rubber band in `src/overlays/construction.rs`. Snapping, the
    shape check and the insertion are one I03 command; the GUI passes the
    raw corner list, layer and kind. `checkShape` is not ported at HEAD.
  - Order: corners are rounded (`FloatPoint.round`) before snapping, as in
    `CornerItemConstructionState.addCorner`; the I03 command owns that.
  - **Waits on I03.**
  - **Accept:** `tests/ui_edit.rs`:
    - `polygon_keepout_inserts_one_obstacle`: four clicks plus Enter on an
      empty board region add exactly one `AreaKind::Obstacle` item, and U
      removes it (patched, `full_rebuilds == 0`).
    - `overlapping_keepout_is_refused`: a polygon over a pad adds nothing
      and the status line reads `Keepout canceled because it overlaps
      existing items.`
    - A Xvfb screenshot during construction (rubber band visible), looked
      at by the implementer and described in the done note.

