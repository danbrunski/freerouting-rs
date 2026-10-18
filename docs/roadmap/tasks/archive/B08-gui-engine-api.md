# B08 — Engine API for the GUI

Opened 2026-10-15. Decision D7 in [port-completion.md](../../port-completion.md):
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
  without the owner). Java's stale entries are a GUI-only bug. The DIVERGENCES line and
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

- [x] **B08.1 — `ItemGeometry`.**
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

    *Done 2026-10-16:* `board/item_geometry.rs` (`BasicBoard::item_geometry`, `ItemGeometry`,
    `LayerShape`, `GeometryShape`). `cargo test -p freerouting --test item_geometry`: 8 passed.
    `every_item_of_every_probe_board_has_geometry` covers all rows of `fixtures.tsv` (sessions
    imported): 257,037 items, 341,931 shapes; kind, layer range, nets, component and fixed state
    equal the item's, and the union of the shape boxes equals `bounding_box()`. Seven unit tests
    check one literal geometry each (trace, via, pin, the three keepouts and a conduction area
    with a hole, component outline, board outline). Differences from the sketch: `Vec` instead of
    `SmallVec` (no new dependency), each shape carries its layer (`LayerShape`), and a circle
    inside an area *with holes* becomes a 64-corner ring. The drill items and the outline start
    their box union from `IntBox::EMPTY` (corners 2^25), which clamps boxes beyond `CRIT_INT`
    (Issue676 at 1,000,000 units/mm); the test applies the same fold there and compares the true
    union wherever no coordinate exceeds the limit.

- [x] **B08.2 — `ItemInfo`.**
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

    *Done 2026-10-16:* `board/item_info.rs` (`BasicBoard::item_info -> ItemInfo { id, kind, name,
    fields: Vec<InfoField> }`, the fields in the order of each Java `printInfo`; the module doc
    lists them per kind), probe `ItemInfoProbe.java` and `fixtures/reference/probe/item-info/{Issue026-J2_reference,
    Issue145-smoothieboard,Issue039-bug-design}.jsonl` (generated at `a773a70ec`). The probe's
    recording `ItemInfoPrinter` writes one line per printed value (links with their text and the
    class of the linked object, points and numbers as raw bits, the values that follow a label,
    `FixedState` names, contact ids, violation counts) and skips the localized label words, so
    the comparison is of data. `cargo test -p freerouting --test board_probe_item_info`: 6
    passed; the three board tests compare 141, 210 and 140 lines (BoardOutline, ComponentOutline,
    ObstacleArea, Pin, PolylineTrace, Via on J2; also ConductionArea on Issue145; ViaObstacleArea
    on Issue039; items with contacts, fixed states and clearance violations among them). The
    three synthetic tests cover what no fixture selection reaches: an area with holes, and the
    field sets of keepouts, via keepouts and component keepouts. `ComponentObstacleArea` occurs in
    no fixture, so its fields come from the Java source only. Issue039 was added to the two
    boards the checklist names, for the via keepouts.

- [x] **B08.3 — Route stubs.**
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

    *Done 2026-10-16:* `drc/route_stubs.rs` (`route_stubs(&BasicBoard) -> Vec<RouteStub>`, with the
    board-to-user conversion in Java's operation order, `String.compareTo` order on UTF-16 units and
    `TreeSet` equal-key dropping), probe `RouteStubsProbe.java` (the stand-in is marked in its doc
    comment) and `fixtures/reference/probe/route-stubs/{Issue026-J2_reference,Issue508-DAC2020_bm05,
    Issue690-ecc83}.jsonl`, generated at `a773a70ec`. `cargo test -p freerouting --test
    board_probe_route_stubs`: 3 passed, line for line equal (including the user coordinates as raw
    double bits). The routed sessions alone give 0 stubs on J2, 5 on DAC2020_bm05 and 0 on ecc83,
    so the probe also removes every 7th trace (item-list order) and lists again: 2, 42 and 15
    stubs (the test reads the counts from Java's file). The new test file replaces the
    "`tests/drc.rs` (append)" of the file table. **Session churn:** the probes of B08.2, B08.3 and
    B08.4 import `fixtures/reference/route/<board>.ses`. J01 overwrote those sessions at 02:26 on
    2026-10-16, after my first generation; I regenerated the three probes' fixtures afterwards
    against the new sessions (the numbers above are from them). If J01 changes the sessions
    again, `probes/board/generate.sh <pin> all` (J01.5) regenerates these fixtures too.

- [x] **B08.4 — Rules-edit facade.**
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

    *Done 2026-10-16:* `rules/edit.rs` (module doc: the call list of each dialog, read at
    `a773a70ec`; functions on `BasicBoard`, so `RoutingBoard` has them through `Deref`),
    `ClearanceMatrix::get_row` and `row` (B08.6). `cargo test -p freerouting --test rules_edit`:
    8 passed.
    - `clearance_value_change_matches_a_dsn_with_the_edited_rule` (both boards): the DSN text's
      `(clearance 50 (type smd_smd))` becomes 400; the report (`violations` and
      `unconnected_items`, zone islands off) of the facade-edited board equals that of the
      fresh board, and differs from the report before the edit; the trees equal those of the
      hand-run primitive calls.
    - `add_class_then_prune_restores_the_board` (both boards): add a class, move every 10th
      default-class item to it, give it an autoroute tree, prune confirming only that class:
      the class count and the report are the original's, moved items sit in a class with
      the same row, the others are untouched; the trees equal those of the primitive calls.
      Only value changes and prune have a DSN-edit equivalent; a net class edit changes no
      item, so its test checks the rules and that report, trees and item classes do not change
      (`net_class_edits_change_the_rules_only`, `via_edits_follow_the_dialogs`,
      `clearance_value_on_one_layer_and_inner_layers`,
      `remove_clearance_class_refuses_a_class_in_use`).
    - Java differential, which the checklist did not ask for: probe `rules-edit`
      (`RulesEditProbe.java`, fixtures `fixtures/reference/probe/rules-edit/`) runs the
      dialog's calls on Java's board and hashes every tree after five stages; the Rust
      sequence is line for line equal on J2 and Issue145 (`java::rules_edit_trees_match_java_on_*`).
      Java leaves the autoroute trees stale after the prune renumbering; the stage hashes show
      the same on both sides, and the module doc says so.
    - `tools/determinism-check.sh --out <dir> -- env BOARD_PROBE_OUT={OUT} <rules_edit test binary> java`:
      identical (idle and beside 8 busy loops).

- [x] **B08.5 — Interactive leftovers on the board.**
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

    *Done 2026-10-16:* `board/gui_ops.rs` (new), `insert_item` is `pub`, and
    `basic_board.rs` got one more named edit: `remove_item` delegates to
    `remove_item_checked(id, for_move)` (Java's `removeItem(item, forMove)` branch, which
    `detach_item_for_move` needs because `removal_note` is private to that module).
    `cargo test -p freerouting --test board_gui_ops`: 10 passed:
    `detach_refuses_a_user_fixed_item`, `detach_refuses_an_item_of_a_position_fixed_component`,
    `detach_removes_a_pin_of_a_free_component_though_deletion_is_forbidden` (also gone from the
    trees), `detach_keeps_a_power_plane_like_java`, `unfill_clears_every_area_and_the_router_ignores_conduction`,
    `unfill_is_undone_by_undo` (with redo), `set_conduction_is_filled_only_touches_conduction_areas`,
    `non_45_degree_count_counts_only_odd_angles`, and
    `non_45_degree_count_matches_a_direct_count_on_issue676` (10 traces, counted line by line).
    Java also sets `rules.ignoreConduction` and rebuilds the trees in `unfillConductionAreas`;
    both are ported. Undo is a deliberate difference (DIVERGENCES, "Board model").
    The error type is `DetachError::{NotOnBoard, Fixed}`; a forbidden non-component item (a power
    plane) is kept with `Ok(())`, as Java returns silently.

- [x] **B08.6 — Queries for the rule dialogs.**
  - Java: `ClearanceMatrix.getRow`, `Net.getItems` (all items of a net, including
    non-connectable ones, `rules/Net.java:195`), `BoardLibrary.isUsed(padstack,
    board)`, `Nets.newNet(Locale)`, and `BasicBoard.DominantSide` if the G series
    asks for it in the engine.
  - Rust: `ClearanceMatrix::row(class, layer)`, `BasicBoard::net_items(net)`
    (highest id first), `BoardLibrary::is_used(padstack, &BasicBoard)`,
    `Nets::new_net()` with Java's naming.
  - **Accept:** unit tests for each, on Issue145-smoothieboard.

    *Done 2026-10-16:* `ClearanceMatrix::get_row` (a `ClearanceRow` view: name, value, max value,
    layer dependence) and `ClearanceMatrix::row(class, layer)` in `rules/clearance_matrix.rs`;
    `BasicBoard::net_items` in `board/gui_ops.rs`; `BoardLibrary::is_used(PadstackId,
    &BasicBoard)` in `library/mod.rs`; `Nets::new_net(default_class)` in `rules/mod.rs`.
    `cargo test -p freerouting --test board_gui_ops`: 15 passed, of which the B08.6 ones are
    `clearance_rows_read_like_java` (every class and layer of the smoothieboard matrix against
    `get_value`), `net_items_are_all_items_of_the_net_highest_id_first` (every net, against a
    direct filter), `padstack_is_used_by_an_item_or_a_package_pin` (all 56 padstacks, all used by
    items, plus a fresh unused one), `padstack_used_only_by_a_package_pin_is_used` and
    `new_net_is_named_after_the_count_in_the_default_class`. Java quirk kept:
    `Nets.newNet` builds the name from `TextManager.getText("net#")` in the `NetClasses` bundle,
    which does not exist at the pin (the key is only in `WindowNetClasses_*`), so `getText`
    returns the key and the name is `net#<n>` in every locale. `DominantSide` was not asked
    for by the G series, so it is not ported.

- [x] **B08.7 — Pin `delete_all_traces_and_vias`'s difference from Java.**
  - Java leaves the tree entries of deleted traces and vias in place
    (`BasicBoard.java:1523-1544`), so later queries can return deleted items. It is
    GUI-only (`BoardToolbar.java:266`, `BoardMenuOther.java:37`).
  - **Accept:** `delete_all_traces_and_vias_leaves_no_tree_entries` (no overlap
    query on the default tree returns a removed id) and a DIVERGENCES line under
    "Board model".

    *Done 2026-10-16:* `tests/java_ports.rs`, group `delete_all_traces_and_vias`:
    `delete_all_traces_and_vias_leaves_no_tree_entries` (5 traces and 3 vias; the overlap query
    on layer 0 returned all 8 before and only the outline after). DIVERGENCES line added under
    "Board model". The behaviour itself already existed (`delete_all_traces_and_vias` removes
    the tree entries); Java is the reference for what the line says, not probed.
