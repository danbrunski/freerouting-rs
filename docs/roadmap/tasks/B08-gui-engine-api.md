# B08 — Engine API for the GUI

Opened 2026-10-15. Decision D7 in [port-completion.md](../port-completion.md):
the engine owns behaviour and the GUI owns input. The desktop app (G series)
therefore needs engine functions that Java's GUI got by reaching into the board:
- drawable geometry per item (G07.4 live redraw, and G02's renderer);
- the item info window's fields (G04.3);
- the route-stubs list (G03.6);
- rule edits that keep the search trees right (G06);
- a few interactive leftovers of the Java facade.

This is the GUI report's ENG-GUI-A without its first and fourth items. The
change journal (ENG-GUI-A.1) is B01.7, and the selection filter (ENG-GUI-A.4) is
B04.2. Nothing here is on the routing critical path.

**Prerequisites:** none for B08.1–B08.3 and B08.5–B08.6. B08.4 needs B02.5 (tree
mutations in Java's order). B08.7 needs B02.1. G04.3, G06 and G07.4 wait for the
matching items (G00's table).

## What exists and what this adds

| Rust today (`crates/freerouting/src/`, `70389b7`) | Java at `cda540f` (`src/main/java/app/freerouting/`) |
|---|---|
| The GUI flattens items itself: `crates/freerouting-gui` `Scene::from_board` | `gui/rendering/BoardRenderer.java` draws from `Item` shapes; `BasicBoard.DominantSide` (`:1616`) is used only there |
| — | `Item.printInfo` per class and `board/actions/ItemInfoPrinter.java` (the info window) |
| `connection_items`, `is_tail`, `trace_start_contacts`/`trace_end_contacts`, `all_contacts` (`board/basic_board.rs:1484-1788`) | `gui/windows/routing/WindowRouteStubs.java` `fillList` (`:36-100`): one-net traces with no contact at one end, and one-net vias whose contacts all share one layer range, in a `TreeSet<RouteStubInfo>` ordered by (net name, x, y, layer) (`:156-169`) |
| Rule parts: `BoardRules::change_clearance_class_index` (`rules/mod.rs:1133`), `remove_clearance_class_from_rules` (`:1171`); tree parts: `SearchTreeManager::clearance_value_changed`, `clearance_class_removed` (`board/search/mod.rs`) | `BoardRules.changeClearanceClassIndex` (`rules/BoardRules.java:266-293`) and `removeClearanceClass` (`:298-345`) renumber the board items too; the dialogs `WindowClearanceMatrix`, `WindowNetClasses`, `WindowAssignNetClass`, `WindowVia`, `WindowViaRule`, `WindowEditVias`, `board/WindowPadstacks` call them plus `searchTreeManager.clearanceValueChanged/clearanceClassRemoved` |
| No `detach_item_for_move`; `insert_item` is `pub(crate)` (`basic_board.rs:191`) | `BoardItemRepository.detachItemForMove` (`:176-184`); public `insertItem` (`BasicBoard.java:1332`) |
| `AreaKind::Conduction { is_filled }` field only | `ConductionArea.setIsFilled` (`:37`), `BasicBoard.unfillConductionAreas` (`:1546-1560`), `getNon45DegreeTraceCount` (`:1585-1595`) |
| `ClearanceMatrix` without `get_row`; no `Net::items`, `BoardLibrary::is_used`, `Nets::new_net` | `ClearanceMatrix.getRow`, `Net.getItems` (`rules/Net.java:195`), `BoardLibrary.isUsed`, `Nets.newNet` |
| `delete_all_traces_and_vias` removes tree entries too (`basic_board.rs:889-901`) | `deleteAllTracksAndVias` deletes from the item list only and leaves the tree entries (`BasicBoard.java:1523-1544`) |

## Decisions made here

- **`ItemGeometry` is the one flattening of items.** The GUI and the router's
  event stream share it, so `Scene::from_board` becomes a loop over it (that
  rewrite is a G checklist's). It is in board units and keeps exact shapes:
  polygons with holes, polylines with half widths, circles. Tessellation stays
  in the GUI.
- **`ItemInfo` is structured data, not text.** The GUI localizes. Java's field
  set and order (`printInfo`) decide what is in it.
- **The rules-edit facade replays Java's dialog sequences**, including Java's
  tree calls, so the trees match Java after an edit. Where a dialog leaves a tree
  stale (`setClearanceClassIndex` changes the class without touching the
  autoroute trees, `Item.java:1001-1008`), the facade does the same and its doc
  says so. A fix would be a measured divergence.
- **`delete_all_traces_and_vias` keeps removing tree entries** (B00, decided
  without Dan). Java's stale entries are a GUI-only bug. The DIVERGENCES line and
  a test pin the Rust behaviour.

## File ownership

| Item | New files | May edit |
|---|---|---|
| B08.1 | `board/item_geometry.rs`, `tests/item_geometry.rs` | `board/mod.rs` (append) |
| B08.2 | `board/item_info.rs`, `tools/java-reference/probes/board/ItemInfoProbe.java`, `fixtures/reference/probe/item-info/*`, `tests/board_probe_item_info.rs` | `board/mod.rs` (append) |
| B08.3 | `drc/route_stubs.rs`, `tools/java-reference/probes/board/RouteStubsProbe.java`, `fixtures/reference/probe/route-stubs/*` | `drc/mod.rs` (append), `tests/drc.rs` (append) |
| B08.4 | `rules/edit.rs`, `tests/rules_edit.rs` | `rules/mod.rs` (append `mod`), `rules/clearance_matrix.rs` |
| B08.5 | `board/gui_ops.rs` | `board/mod.rs` (append), `board/basic_board.rs` (make `insert_item` public only) |
| B08.6 | — | `rules/clearance_matrix.rs`, `rules/mod.rs`, `library/mod.rs`, `board/gui_ops.rs` |
| B08.7 | — | `tests/java_ports.rs` (append), `docs/DIVERGENCES.md` |

## Checklist

- [ ] **B08.1 — `ItemGeometry`.**
  - Rust: `board/item_geometry.rs`, `BasicBoard::item_geometry(id) ->
    ItemGeometry { kind, first_layer, last_layer, nets, fixed_state,
    component, shapes: SmallVec<[GeometryShape; 2]> }`, with `GeometryShape::{
    Polygon { outer, holes }, Polyline { corners, half_width }, Circle { center,
    radius }, Octagon(IntOctagon), Box(IntBox) }` in board units. One shape per
    layer for drill items (`DrillData::shape_on_layer`), the polyline for traces,
    the area's polygons with holes for areas and outlines.
  - **Accept:** `tests/item_geometry.rs`: on every `board_probe/fixtures.tsv` row,
    every item has a geometry; the union of its shapes' bounding boxes equals the
    item's `bounding_box()`; kinds and layer ranges match the item. A unit test per
    item kind checks one literal geometry.

- [ ] **B08.2 — `ItemInfo`.**
  - Java: `Item.printInfo(ItemInfoPrinter, Locale)` for each class (`Pin.java:608`,
    `Via.java:242`, `Trace.java:413`, `ConductionArea.java:425`, the area and
    outline classes), through `board/actions/ItemInfoPrinter`.
  - Rust: `board/item_info.rs`, `BasicBoard::item_info(id) -> ItemInfo` with the
    fields Java prints, in Java's order: kind, id, nets (names), layers,
    clearance class name, fixed state, component and pin name, padstack, trace
    width and length, area name.
  - Probe `item-info` on Issue026-J2_reference with its SES and
    Issue145-smoothieboard: one item of each kind present, using a recording
    `ItemInfoPrinter` that writes each printed field as a key and value.
  - **Accept:** `board_probe_item_info` equal field by field (values compared as
    data, not as localized text).

- [ ] **B08.3 — Route stubs.**
  - Java: `gui/windows/routing/WindowRouteStubs.java` `fillList` (`:36-100`).
    Items in item-list order; only traces and vias with exactly one net.
    - A via is a stub if all its `getAllContacts()` have the same first and last
      layer (or it has none). Its layer is the via's first layer without
      contacts, else the contacts' first layer when the via's range satisfies
      Java's test at `:72-77` (copy it as written), else their last layer. Its
      location is the centre.
    - A trace is a stub at its first corner if it has no start contacts, else at
      its last corner if it has no end contacts.
    - `RouteStubInfo.compareTo` (`:156-169`) orders by net name, then the sign of
      the x difference, then y, then layer. Equal keys are dropped by the
      `TreeSet`.
  - Rust: `drc/route_stubs.rs`, `route_stubs(&BasicBoard) -> Vec<RouteStub>` in
    Java's order.
  - Probe `route-stubs` on 3 fixtures with sessions (Issue026-J2_reference,
    Issue508-DAC2020_bm05, Issue690-ecc83), using the window's logic copied into
    the probe as a stand-in (the window needs a GUI), marked as such.
  - **Accept:** the stub list equal on all three boards.

- [ ] **B08.4 — Rules-edit facade.**
  - Java: the dialogs' engine calls. Read each dialog at the pin and list its call
    sequence in the module doc:
    - `WindowClearanceMatrix`: set a value, add a class, prune: a value change
      then `searchTreeManager.clearanceValueChanged()`; a prune via
      `rules.changeClearanceClassIndex` + `removeClearanceClass` +
      `searchTreeManager.clearanceClassRemoved`;
    - `WindowNetClasses`: width, clearance class, via rule, layer settings,
      reapplied to unfixed items as Java does;
    - `WindowAssignNetClass`, `WindowVia`, `WindowViaRule`, `WindowEditVias`
      (`Item.setClearanceClassIndex`, `Via.setPadstack`), `board/WindowPadstacks`.
  - Rust: `rules/edit.rs` with one function per dialog action, on `RoutingBoard`.
    It renumbers items (`changeClearanceClassIndex` and `removeClearanceClass`
    walk `getItems()`, highest id first) and calls the tree functions Java calls,
    in Java's order. Plus `ClearanceMatrix::get_row`.
  - **Accept:** `tests/rules_edit.rs` on Issue026-J2_reference and
    Issue145-smoothieboard. After each action, `DesignRulesChecker`'s report
    equals that of a fresh board loaded from the DSN text edited to the same
    rule (the test edits the DSN rule text). The tree fingerprints (B02.1) equal a
    sequence of Rust calls that mirrors the Java call list in the module doc.

- [ ] **B08.5 — Interactive leftovers on the board.**
  - Java:
    - `BoardItemRepository.detachItemForMove` (`:176-184`): error for user-fixed
      items or a component with `positionFixed`; removes a component item even
      when `isDeletionForbidden`;
    - public `insertItem` (`BasicBoard.java:1332`);
    - `ConductionArea.setIsFilled` and `BasicBoard.unfillConductionAreas`
      (`:1546-1560`);
    - `getNon45DegreeTraceCount` (`:1585-1595`, `!polyline().isMultipleOf45Degree()`).
  - Rust: `board/gui_ops.rs` with `detach_item_for_move(id) -> Result<(), DetachError>`,
    `set_conduction_is_filled(id, bool)`, `unfill_conduction_areas()`,
    `non_45_degree_trace_count()`; `insert_item` becomes `pub`.
  - **Accept:** unit tests: detach refuses a user-fixed item and an item of a
    position-fixed component, and detaches a pin of a free component;
    `unfill_conduction_areas` clears every area's filled flag and is undoable;
    `non_45_degree_trace_count` equals a direct count on Issue676-ch32v-tx118s.

- [ ] **B08.6 — Queries for the rule dialogs.**
  - Java: `ClearanceMatrix.getRow`, `Net.getItems` (all items of a net, including
    non-connectable ones, `rules/Net.java:195`), `BoardLibrary.isUsed(padstack,
    board)`, `Nets.newNet(Locale)`, and `BasicBoard.DominantSide` if the G series
    asks for it in the engine.
  - Rust: `ClearanceMatrix::row(class, layer)`, `BasicBoard::net_items(net)`
    (highest id first), `BoardLibrary::is_used(padstack, &BasicBoard)`,
    `Nets::new_net()` with Java's naming.
  - **Accept:** unit tests for each, on Issue145-smoothieboard.

- [ ] **B08.7 — Pin `delete_all_traces_and_vias`'s difference from Java.**
  - Java leaves the tree entries of deleted traces and vias in place
    (`BasicBoard.java:1523-1544`), so later queries can return deleted items. It is
    GUI-only (`BoardToolbar.java:266`, `BoardMenuOther.java:37`).
  - **Accept:** `delete_all_traces_and_vias_leaves_no_tree_entries` (no overlap
    query on the default tree returns a removed id) and a DIVERGENCES line under
    "Board model".
