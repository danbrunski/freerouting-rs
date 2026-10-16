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

- [x] **G05.1 — Inspect mode: click, toggle and region select.**
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
  *Done 2026-10-16:* `tools/inspect.rs` and `tools/mod.rs` (`App::execute_tool`). The left button is Java's `InspectMenuState`: a click calls `pick_items_filtered` through the user's filter (the active layer first, then the other visible layers when "All visible"; `inspect::pick`), a click with items selected toggles (`inspect::click`, Java's `toggleSelect` rules: one picked item toggles, several do nothing, none or the last one removed ends the selection), Ctrl+click is the same toggle also with nothing selected, a left drag draws a rectangle (`ViewState::tool_drag`) and on release adds the filtered `overlapping_items` of the box (`inspect::region`, layer -1 plus the visibility drop for "All visible"). Esc, then the toolbar's Cancel, clears. Status texts `In inspection mode`, `Nothing selected.`, `In the inspection menu` and, while dragging, `Drag with the left mouse button to select items in a region.`. `tests/ui_edit.rs` (raw pointer events): `region_select_picks_pins_only` (Issue159, the pins of the component with most pins, filter pins only: the selection equals the engine's filtered `overlapping_items` of the dragged box, all pins), `click_on_empty_space_clears_selection`, `ctrl_click_toggles`, `left_drag_does_not_pan` (centre and zoom unchanged). Unit tests in `tools::inspect::tests`. Deliberate differences (region starts at the press point; Ctrl+click) are in `docs/DIVERGENCES.md`.

- [x] **G05.2 — Selection settings panel.**
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
  *Done 2026-10-16:* `panels/select_settings.rs`: `SelectionSettings { layers: SelectionLayers, selectable: BTreeSet<SelectableChoice> }` (B04's enum; `filter()` builds the engine's `ItemFilter`), Java's defaults (all visible, `ItemFilter::default()`), the "General Settings" window (Settings > Select) with Java's texts, persisted in `PersistedSettings::{selection_layers, selectable_items}` (Java constant names; defaults are stored as `None`). `tests/ui_edit.rs::current_layer_only_ignores_other_layers` (Issue026-J2 with its session: a trace alone on layer 1 is not found with "Current layer only" on layer 0, is found with "All visible", and is not found when layer 1 is hidden), `selection_settings_persist` (save, fresh app: equal settings; defaults not stored), `the_settings_window_edits_the_choices` (through the menu and the window's buttons).

- [x] **G05.3 — Extend the selection.**
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
  *Done 2026-10-16:* `inspect::extend` and `Command::ExtendSelection(ExtendKind)`; keys `N`, `S`, `E` with items selected (B stays redo); an empty result clears the selection (`In the inspection menu`). `tests/ui_edit.rs::extend_selection_matches_engine` (Issue754, one pin on a net): Nets, Connected Sets, Connections and Components each equal the set the test builds from `connectable_items`, `connected_set(pin, -1, false)`, `connection_items(pin, None)` and `component_items` with Java's rules (filter after the first three, not after components, which also keeps the unselectable component outline); with only Keepout selectable the Nets result is empty and the selection is cleared. `extend_keys_n_s_e`.

- [x] **G05.4 — Selection toolbar.**
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
  *Done 2026-10-16:* `panels/selection_toolbar.rs`: a toolbar that floats over the top of the canvas while items are selected (so the canvas does not resize and fit again), with Java's ten buttons in Java's four groups (Cancel, Info | Nets, Connected Sets, Connections, Components | Violations | Zoom Selection, Zoom All, Zoom Region) and, by decision G00 question 6 (accepted by Dan), Delete, Fix and Unfix in a fifth group (`tools/edit.rs`; DIVERGENCES line). Violations shows the selected items' clearance violations as `N clearance violations found` (Java's `aggregateSortedBySeverity`: the sum of each item's own report, no deduplication) and draws their markers; Info opens the item info window (G04.3 done with it). `tests/ui_edit.rs`: `toolbar_buttons_match_java_set` (labels equal Java's ten, then the three added; each is one button on the canvas while a selection exists and none without), `violations_of_selection_counts` (Issue015, whose one violation is reported by one of its two items: V shows `1 clearance violations found`), `violations_seen_from_both_items_count_twice` (Issue684 shows 2, as Java's code does), `toolbar_zoom_and_extend_buttons_work`, `delete_keeps_fixed_items_and_undo_restores`, `fix_and_unfix_set_the_fixed_state`. Ports of the Java tests `BoardMenuToolbarA11yTest.inspectToolbarActionsAreGroupedSeparatorsAndAccessible` (`selection_toolbar::tests::javas_toolbar_is_four_groups_with_distinct_names`); the Hungarian-locale test is not ported (English only). Known gap: Fix and Unfix are not undoable, because `BasicBoard::set_fixed_state` and `unfix` do not save the item for undo (as in Java); I03.6 owns that.

- [x] **G05.5 — Undo and redo with scene patching.**
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
  *Done 2026-10-16:* `Session` turns on the board's change journal after the load; `Loaded::sync_scene` (`session.rs`) drains `take_changes` and calls `Tessellation::patch` for the removed, changed and inserted ids, updates the scene's items and pick index, and rebuilds in full only when the journal overflowed (`SceneSync`); undo, redo, delete all, keepouts and Delete/Fix/Unfix all go through it (`App::after_board_edit`, `stats.patches`). Undo and redo show Java's `Undo`, `Redo`, `Nothing more to undo.` and `Nothing more to redo.`, are refused while a keepout is drawn, and keep the changed nets in `Tools::changed_nets`; the background analysis recomputes the ratsnest for all nets rather than for the changed ones only (not observable). `tests/ui_edit.rs`: `random_edit_undo_redo_matches_fresh_scene` (Issue754, `JavaRandom` seed 7, 20 steps of keepout insert, delete, undo, redo: after every step the chunk draw list, keyed by slot, kind, id and sequence, the triangle count and the scene items equal those of a fresh build; `full_rebuilds == 0`), `delete_all_and_undo_patch_the_scene` (the journal sees delete all, which does not bump `revision()`), `undo_at_start_says_nothing_more`, `undo_is_refused_while_a_keepout_is_drawn`, `undo_with_a_selection_works`. FT4 (`ft4_undo_of_200_and_200_items`, Issue190, one undo of 200 removed plus 200 added items, `Loaded::sync_scene` including the scene and pick index): release median 1.349 ms, max 2.145 ms over 20 alternating undo and redo (load average 21 on this machine, not quiet; the test asserts at most 2 ms in release).

- [x] **G05.6 — Context menus.**
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
  *Done 2026-10-16:* `tools/popup.rs`: the right button opens the menu of the mode at the click location: nothing selected, Java's `PopupMenuMain` (Select item (I), Start route (T) disabled until I01, Create keepout > Tile, Circle, Polygon, Hole (disabled until I03.4), Swap pin (W) only when the board has logical parts, disabled until I03.3), then the display rows; items selected, `PopupMenuInspectedItems` ("Extend selection to:" header, the four extend entries, a separator, the display rows); a keepout under construction, Done/Insert and Cancel. The display rows act on the click location (`CenterAt`, `ZoomInAt`, `ZoomOutAt`). Esc or a click elsewhere closes it. `tests/ui_edit.rs`: `popup_entries_by_mode` (raw secondary click; the labels found in the AccessKit tree equal `popup::labels` for both modes, Start route is disabled), `popup_display_rows_act_on_the_click_location`, `popup_select_item_selects_at_the_click`, `key_i_selects_at_the_cursor_and_shows_info`. Port of `PopupMenuInspectedItemsTest.popupContainsExtendItemsAndDispatchesActions` (`popup::tests::extend_items_dispatch_and_a_separator_follows`); the German-header test is not ported (English only).

- [~] **G05.7 — Keepout construction.**
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
  *Partial 2026-10-16:* the GUI side is done: `tools/keepout.rs` (corner snapping to the 45/90 degree restriction, `FloatPoint.round` before snapping, concave-corner removal for tiles, `addCornerForSnapAngle`, the circle as a box in 90 degree mode and an octagon in 45 degree mode), `overlays/construction.rs` (the rubber band), Create keepout > Tile, Circle, Polygon in the popup, Enter completes, Esc cancels, Java's three status texts. `tests/ui_edit.rs::polygon_keepout_inserts_one_obstacle` (right click, Create keepout, Polygon, three clicks, Enter: exactly one new `AreaKind::Obstacle` item of about 2 mm, patched into the chunks with `full_rebuilds == 0`, scene equal to a fresh build; U removes it), `overlapping_keepout_is_refused` (a polygon over a pad adds nothing, the status reads `Keepout canceled because it overlaps existing items.`, and no snapshot was taken), `tile_and_circle_keepouts`, `the_rubber_band_follows_the_cursor`. Xvfb screenshot (release binary, `xvfb-run`, Issue159, driven with `xdotool`: right click, Create keepout, Polygon, three clicks, the cursor moved; `target/orchestrator/g05/keepout.png`), which I opened: the popup shows Select item, Start route greyed out, Create keepout (expanded to Tile, Circle, Polygon, a greyed Hole), Center display, Zoom in, Zoom out; during construction a white rubber band joins the three clicked corners with small square markers and a last line to the cursor, snapped to the horizontal, and the status bar reads `Creating a polygon keepout`. Remainder: (1) the shape check and the insertion are written in `keepout::complete` from `check_shape`, `generate_snapshot` and `insert_obstacle`, because the editor command it should call (I03.4, `Command::InsertKeepout`) has not landed; replace that function by the command and keep the tests; (2) the Hole keepout (`HoleConstructionState`: add a hole to an obstacle or conduction area) needs I03.4's `AddHole`; its popup entry is disabled. Owner: I03.4.


