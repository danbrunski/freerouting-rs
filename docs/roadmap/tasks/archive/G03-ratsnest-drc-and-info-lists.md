# G03 — Ratsnest, DRC and info lists

Opened 2026-10-15. The engine already computes airlines, clearance
violations, unconnected items, length violations and zone islands, and its
results match Java (`tests/reference_load.rs`, `tests/reference_drc.rs`).
The app shows none of it. This checklist adds a selection model, the
ratsnest and violation overlays, the Info menu lists, delete-all with undo,
and DRC report export. It is the report's GUI-3 and items 1–2 of `gui.md`
"What's next".

**Prerequisites:** G01 (`Session`, `Command`, the kittest harness, stats).
G03.7 needs B08's port of `BasicBoard.unfillConductionAreas` for full Java
behaviour; it is partial until then. G03.6's route stubs need B08 and are
optional here.

## What exists and what this adds

| Exists (Rust at `70389b7`, Java at `cda540f`) | State |
|---|---|
| `freerouting::drc::DesignRulesChecker` (`drc/checker.rs`) | `new(&BasicBoard)`, `check() -> DrcFindings`, `all_airlines()`, `incomplete_count()`, `all_clearance_violations()`, `all_unconnected_items()`, `zone_island_violations()`, `length_violation_count()`, `length_violation(net)`, `item_description(id)`, `generate_report(source_file, coordinate_unit) -> DrcReport` |
| `drc::incompletes::AirLine` | `net_number`, `from_item`, `from_corner`, `from_layers`, `to_item`, `to_corner`, … |
| `drc::clearance::ClearanceViolation` | `first_item`, `second_item`, `shape: TileShape`, `layer`, `expected_clearance`, `actual_clearance` |
| `BasicBoard` | `bounding_box_of(&[ItemId])`, `generate_snapshot`, `undo`, `redo`, `delete_all_traces_and_vias`. No port of `unfillConductionAreas` |
| CLI | `freerouting -de x.dsn -drc out.json` writes the DRC report (`drc/runner.rs`, `drc/json.rs`) |
| Java lists | `gui/windows/board/WindowObjectList.java` (Select, Invert, Info, Recalculate, filter), `WindowIncompletes.java`, `gui/windows/routing/{WindowClearanceViolations,WindowLengthViolations,WindowUnconnectedRoute,WindowRouteStubs}.java` |
| Java overlays | `gui/rendering/NetIncompletesGraphics.java`, `gui/workspace/progress/{RatsNest,ClearanceViolations}.java`; G toggles the ratsnest and V the violations (`gui/interactive/MenuState.java`) |
| Java delete-all | `gui/menus/BoardMenuOther.java` and `gui/board/BoardToolbar.java` (~line 262): `deleteAllTracksAndVias()`, then `unfillConductionAreas()` (`board/facade/BasicBoard.java:1546`: sets `ignoreConduction`, unfills every conduction area, reinserts the tree items), then rebuilds the ratsnest. No confirmation and no undo |
| Java texts | `gui/menus/BoardMenuInfo_en.properties` (menu labels), `gui/windows/routing/WindowClearanceViolations_en.properties` (`clearance_violation_message_template`, `list_empty_message`), `WindowLengthViolations_en.properties`, `gui/windows/board/WindowObjectList_en.properties`, `gui/menus/BoardMenuOther_en.properties` |

What this adds: `Selection` and its highlight; ratsnest and violation
overlays; background DRC; six Info lists; delete-all with confirmation and
undo; File > Export DRC report.

## Decisions made here

- **Analysis runs on worker threads** over an `Arc` clone of the session
  board (G01.2). A result is tagged with the `(revision, edit_generation)`
  it was computed for and dropped if the session moved on; a new run starts
  instead. `DesignRulesChecker` borrows the board, so the worker owns the
  `Arc` for the run.
- **Overlays are egui shapes** drawn after the canvas, in
  `src/overlays/`: airlines as 1 px lines, violations as their
  `TileShape` outline plus a filled translucent marker, the selection as a
  highlight colour over each selected item's outline. They read engine
  results only (G00 lanes).
- **`Selection` is a `BTreeSet<ItemId>`**, so iteration order is stable for
  lists and tests.
- **Delete-all gets a confirmation and undo**, which Java lacks. This is GUI
  convenience, not a routing difference. Like Java, it also unfills
  conduction areas and sets "ignore conduction", which changes later
  routing of that board; the confirmation text says so.
- **Texts follow the Java bundles** and live in `src/text.rs`.

## File ownership

Paths are under `crates/freerouting-gui/`.

| Item | New files | May edit |
|---|---|---|
| G03.1 | `src/selection.rs`, `src/overlays/{mod,highlight}.rs`, `tests/ui_drc.rs` | `src/commands.rs`, `src/input.rs` (W), `src/app.rs` |
| G03.2 | `src/overlays/ratsnest.rs` | `src/session.rs` (job runner), `src/panels/board_stats.rs`, `src/input.rs` (G) |
| G03.3 | `src/overlays/violations.rs` | `src/session.rs`, `src/input.rs` (V) |
| G03.4 | `src/panels/{object_list,violations}.rs`, `src/text.rs` | `src/commands.rs` |
| G03.5 | `src/panels/incompletes.rs` | `src/text.rs` (append) |
| G03.6 | `src/panels/{length,unconnected,islands}.rs` | `src/text.rs` (append) |
| G03.7 | — | `src/commands.rs`, `src/session.rs` |
| G03.8 | — | `src/commands.rs` |

## Checklist

- [x] **G03.1 — Selection model, highlight, zoom to selection.**
  - `src/selection.rs`: `Selection { items: BTreeSet<ItemId> }` with
    `set`, `toggle`, `clear`; `Command::Select(Vec<ItemId>)`,
    `Command::ClearSelection`, `Command::ZoomSelection` (key W while the
    inspect tool isn't active; Java's W in a selection state).
  - Zoom to selection fits the camera to `BasicBoard::bounding_box_of` of
    the selection plus 10 %.
  - `src/overlays/highlight.rs` draws each selected item's outline in the
    highlight colour.
  - **Accept:** `tests/ui_drc.rs`:
    - `zoom_to_selection_contains_items`: select two pads far apart on
      Issue159 through `Command::Select`, press W; the visible board
      rectangle contains both pads' boxes and is at most 1.25 × the larger
      side of their union.
    - `snapshot_highlight`: the snapshot `highlight_issue159.png` shows the
      two pads highlighted (the implementer opens it and describes it).
  *Done 2026-10-15:* `src/selection.rs` (`Selection { items: BTreeSet<ItemId> }`,
  `set`, `toggle`, `clear`), `Command::{Select, ClearSelection, ZoomSelection}`, key
  `W`, `overlays/highlight.rs`. Zoom to selection fits the `bounding_box_of` box
  plus 10 % through `Camera::zoom_to_rect`.
  `ui_drc::zoom_to_selection_contains_items` (Issue159, the two pads farthest apart
  in a row; the pair has to be wider than tall, because the canvas is wider than
  tall and a tall union cannot fit in 1.25 times its larger side): the visible
  rectangle contains both boxes and its larger side is at most 1.25 times the
  larger side of the union. `w_without_a_selection_keeps_the_view`. Snapshot
  `tests/snapshots/highlight_issue159.png`, opened: the view is zoomed to the two
  pads and each is a circle with a cyan outline and a translucent cyan fill (the
  board's yellow airlines and the "Checks" panel are visible too).

- [x] **G03.2 — Ratsnest overlay.**
  - After every load and every edit, a worker computes
    `DesignRulesChecker::all_airlines()`; the overlay draws them as
    hairlines in the incompletes colour. G and a toolbar toggle switch it.
  - The side panel shows "Incomplete connections: n" from
    `incomplete_count()`.
  - **Accept:**
    - `tests/ui_drc.rs::incomplete_counts_match_reference`: after the load,
      the panel count equals `statistics.connections.incomplete_count` of
      `fixtures/reference/load/<board>.json` for
      `Issue508-SMD-routing-issue-demo` (6), `Issue143-rpi_splitter` (5)
      and `Issue684-Autorouter_PCB1_2026-5-8` (75).
    - The overlay draws exactly as many lines as `all_airlines().len()`
      (counter in `stats.rs`).
    - `snapshot_ratsnest_issue143`, opened and described by the implementer.
  *Done 2026-10-15:* `analysis.rs` (a worker over an `Arc` clone, results tagged
  `(load, revision, edit_generation)`, stale ones hidden, one worker at a time;
  the ratsnest is sent first), `overlays/ratsnest.rs`, `G` and a toolbar toggle
  ("Ratsnest", on by default), side panel "Incomplete connections: n".
  `ui_drc::incomplete_counts_match_reference`: the panel label equals
  `statistics.connections.incomplete_count` of the load reference for
  Issue508-SMD-routing-issue-demo (6), Issue143-rpi_splitter (5) and
  Issue684-Autorouter_PCB1_2026-5-8 (75), the engine's `incomplete_count()` equals
  it, and `stats().overlay.airlines` (counter in `stats.rs`, `OverlayCounts`)
  equals `all_airlines().len()`. `g_toggles_the_ratsnest`.
  Snapshot `ratsnest_issue143.png`, opened: the Issue143 board with five thin
  yellow lines between pads of J1, J2 and J3, and "Incomplete connections: 5" in
  the side panel.

- [x] **G03.3 — Background DRC and violation markers.**
  - A worker runs `DesignRulesChecker::check()` after the load and after
    each edit; Run DRC in the Info menu forces a run. V and a toolbar toggle
    show the markers.
  - **Accept:**
    - `tests/ui_drc.rs::violation_markers_match_reference`: the marker count
      equals `statistics.clearance_violations.total_count` in the load
      reference for `PCBench-retroreflectors-CONGFLOCK` (8),
      `Issue684-Autorouter_PCB1_2026-5-8` (5) and `Issue313-FastTest` (40).
    - Release, `#[ignore]` in debug:
      `drc_does_not_block_frames_issue190`: while the DRC of
      `Issue190-processor.Z80` runs, the stats' maximum frame CPU stays
      ≤ 50 ms.
    - An Xvfb screenshot of Issue313 with markers on, taken with the
      implementer recipe (or `--screenshot`), opened and described.
  *Done 2026-10-15:* the same worker runs the rest of the check
  (`all_clearance_violations`, `all_unconnected_items`, `zone_island_violations`);
  Info > Run DRC forces a run (`Command::RunDrc`); `V` and a toolbar toggle
  ("Violations", off by default as in Java); `overlays/violations.rs` draws each
  violation's shape with a ring around its centre.
  `ui_drc::violation_markers_match_reference`: `stats().overlay.markers` equals
  `statistics.clearance_violations.total_count` of the load reference for
  PCBench-retroreflectors-CONGFLOCK (8), Issue684 (5) and Issue313-FastTest (40),
  and the panel shows the count. `edits_start_a_new_analysis_and_stale_results_are_hidden`.
  Release: `drc_does_not_block_frames_issue190` ran 72 frames while the worker ran,
  maximum frame CPU 13.9 ms (limit 50 ms; machine not measured quiet, other agents
  building). Xvfb screenshot (`tools/gui/smoke.sh fixtures/Issue313-FastTest.dsn
  --keys v`, release binary), opened: Issue313 with 40 red ringed translucent
  markers on the vias and pads, three thin yellow airlines, and "Incomplete
  connections: 3", "Clearance violations: 40" in the side panel.

- [x] **G03.4 — Info > Clearance Violations.**
  - `src/panels/object_list.rs`: a generic list window with Java's
    `WindowObjectList` buttons (Select, Invert, Info, Recalculate) and a
    filter field. Info opens the item-info window once G04.3 exists and is
    disabled before.
  - Rows use `clearance_violation_message_template` ("A clearance violation
    of {{clearance_mm}} mm exists between {{item_a}} and {{item_b}} at
    {{location}} on layer {{layer}}."). Which of
    `expected_clearance` and `actual_clearance` fills `{{clearance_mm}}`,
    and how items are named, comes from
    `gui/windows/routing/WindowClearanceViolations.java`; read it, don't
    guess. Empty list: "No clearance violations found."
  - Select selects the violation's two items and zooms to them.
  - **Accept:** `tests/ui_drc.rs::clearance_list_select`: on
    `Issue684-Autorouter_PCB1_2026-5-8`, the list has 5 rows; clicking the
    first row and Select sets the selection to that violation's
    `{first_item, second_item}`; the row text equals the template filled by
    the same rule as Java (a unit test in `panels/violations.rs` checks one
    row against a string built from the Java source's formatting).
  *Done 2026-10-15:* `panels/object_list.rs` (the generic window: Select, Invert,
  Info (disabled, tooltip from Java, until G04.3), Recalculate, filter, Ctrl+click,
  double click selects), `panels/violations.rs`, `text.rs` (Java's bundle texts and
  `TextManager.getText` / `String.valueOf(float)` / `NumberFormat` formatting). The
  row follows `WindowClearanceViolations.ViolationInfo`: `{{clearance_mm}}` is
  `(expected_clearance - actual_clearance) / 10000` as `%.4f` (the shortfall, not
  the actual clearance; Java divides by 10000 whatever the board unit is), items are
  `pin`, `via [net]`, `trace [net]`, `conduction area`, `via keepout`,
  `component keepout`, `keepout`, `board outline`, else `unknown`, the location is
  the centre of gravity in design-file units as `(x , y)`, the layer is its name;
  sorted by shortfall (largest first), then layer. Empty text "No clearance
  violations found."
  `ui_drc::clearance_list_select` (through Info > Clearance Violations on Issue684):
  5 rows, all found in the UI, the first row equals `violations::row_text` of the
  violation with the largest shortfall, clicking it and Select sets the selection to
  `{first_item, second_item}` and the view contains both items.
  `panels::violations::tests::row_text_follows_the_java_template` checks a row
  against the string built from the Java template (pin and trace, then via and
  unknown). `clearance_list_empty_text`. Divergence: Java's `TreeSet` drops
  violations with equal shortfall and layer (67 violations show as 3 rows on
  Issue575-drc_BBD_Mars-64_6_track_1_hole_clearance_violations), this list keeps
  all; pinned by `java_a11y_list_counts`, `docs/DIVERGENCES.md` "freerouting-gui".

- [x] **G03.5 — Info > Incomplete Connections.**
  - One row per airline, grouped by net, with Java's `WindowIncompletes`
    text (read the class for its row format). Select selects both endpoint
    items.
  - **Accept:** `tests/ui_drc.rs::incompletes_list`: on
    `Issue143-rpi_splitter`, the row count equals `all_airlines().len()`;
    Select on the first row sets the selection to its `from_item` and
    `to_item`.
  *Done 2026-10-15:* `panels/incompletes.rs`: one row per airline sorted (stable) by
  net name, text `AirLine.toString` (`net: <from> - <to>` with `Item.toString`:
  lower-case class name, ` #pin` for a pin index, ` of component #n`). Empty text
  "Route completed." Select selects `from_item` and `to_item` and zooms.
  `ui_drc::incompletes_list` (Issue143): the row count equals `all_airlines().len()`,
  rows are grouped by net, Select on the first row sets the selection to its two
  items. `java_a11y_list_counts` (the count of Java's
  `ViolationsIncompletesListA11yTest`: 9 incompletes on
  Issue575-drc_dev-board_4_hole_clearance_violations).

- [x] **G03.6 — Length violations, unconnected traces and vias, zone islands.**
  - Three lists from `length_violation`, `all_unconnected_items()` and
    `zone_island_violations()`, with Java's texts
    (`length_violation_max_message`, `length_violation_min_message`,
    `list_empty` "No length violations found.", `WindowUnconnectedRoute`).
  - Route stubs (`WindowRouteStubs`) are added only if B08's `route_stubs`
    has landed; otherwise the menu entry is absent and the done note says
    so.
  - **Accept:** `tests/ui_drc.rs::other_lists_match_engine`: row counts
    equal `length_violation_count()`, `all_unconnected_items().len()` and
    `zone_island_violations().len()` on fixtures with non-zero values. Use
    `Issue143-rpi_splitter_mod` (one unconnected group in the Java DRC
    digest), `Issue054-tairakb` (one dangling trace) and
    `Issue029-hw48na` (one dangling via); for length violations use a
    fixture with `(length …)` rules or a small DSN in
    `crates/freerouting-gui/tests/data/`. Each empty list shows Java's empty
    text (a test on a clean fixture).
  *Done 2026-10-15:* `panels/length.rs` (Java's `WindowLengthViolations`: nets sorted by
  name ignoring case, `length_violation_max_message` / `_min_message`, lengths in the
  design unit through `String.valueOf(float)`, Select selects every item of the
  net), `panels/unconnected.rs` (one row per `all_unconnected_items()` finding with
  `unconnected_route_row_message`; Java's window lists a different notion, see
  DIVERGENCES), `panels/islands.rs` (the DRC report's text; Java has no window).
  `ui_drc::other_lists_match_engine`: row counts equal `all_unconnected_items().len()`
  and `zone_island_violations().len()` and `length_violation_count()` on
  Issue143-rpi_splitter_mod, Issue054-tairakb and Issue029-hw48na, each with at
  least one unconnected finding. No fixture has `(length ...)` rules, so
  `tests/data/length_violation.dsn` (new) has one net too long (10000 against a
  maximum of 5000) and one too short: `length_violations_list` checks both rows
  as text. `tests/data/zone_islands.dsn` (new) gives copper islands:
  `zone_islands_list`. `empty_lists_show_javas_texts`: "Route completed.", "No
  clearance violations found.", "No unconnected traces or vias found.", "No zone
  islands found." on a clean board, "No length violations found." on Issue143.
  Route stubs: the menu entry is absent, `route_stubs` (B08.3) has not landed.

- [x] **G03.7 — Delete all traces and vias, with undo.**
  - Other > Delete All Traces and Vias (Java label) and a toolbar button.
    A confirmation dialog says that conduction areas are unfilled too and
    that later routing ignores them, as in Java.
  - On confirm: `generate_snapshot()`, `delete_all_traces_and_vias()`, then
    B08's `unfill_conduction_areas()` when it exists; ratsnest and DRC
    rerun. U and Ctrl+Z call `undo`, B and Ctrl+Y `redo`. The scene is
    rebuilt in full on undo and redo here; G05.5 replaces that with
    patching.
  - **Accept:** `tests/ui_drc.rs::delete_all_and_undo`: on
    `Issue684-Autorouter_PCB1_2026-5-8`, after confirming, the trace and via
    counts are 0 and the incomplete count is at least the value before;
    after U, `BoardStatistics` (traces, vias, incomplete connections) equals
    the values before the delete; after B, it equals the values after the
    delete. Until B08's unfill lands, the item is partial with "needs
    `unfill_conduction_areas` (B08)".
  *Partial 2026-10-15:* needs `unfill_conduction_areas` (B08.5, `board/gui_ops.rs`),
  which has not landed: Delete All deletes the traces and vias but does not unfill
  conduction areas or set "ignore conduction" yet. Everything else is done: Other >
  Delete All Traces and Vias and a toolbar button "Delete All" open a confirmation
  that says conduction areas are unfilled and later routing ignores them (the text is
  there now, the behaviour waits for B08.5); on confirm `generate_snapshot()` then
  `delete_all_traces_and_vias()` through `Session::edit`, the scene is rebuilt in full
  (`stats.full_rebuilds`), the analysis reruns; `U`/`Ctrl+Z` undo, `B`/`Ctrl+Y` redo.
  `ui_drc::delete_all_and_undo` (Issue684, as the item says; its load reference has
  0 traces and 0 vias, so the check is vacuous there) and
  `delete_all_and_undo_with_wiring` (Issue026-J2_reference with its session, which
  has traces and vias): after confirming, traces and vias are 0 and the incomplete
  count is at least the value before; after U the `BoardStatistics` (traces, vias,
  incomplete) equal the values before; after B the values after.
  `ctrl_z_and_ctrl_y_undo_and_redo`, `cancelling_delete_all_keeps_the_wiring`.
  When B08.5 lands: call `unfill_conduction_areas()` inside `App::delete_all`
  (`app_analysis.rs`) after `delete_all_traces_and_vias()`, and add a conduction
  area board to the test.
  *Done 2026-10-16:* B08.5 landed (commit 0597f09). `App::delete_all` now calls
  `board.unfill_conduction_areas()` inside the same `session.edit` closure, after the
  snapshot and `delete_all_traces_and_vias()`. `tests/ui_edit.rs::delete_all_unfills_conduction_areas`
  (G05 owns the file) loads `Issue163-pic_programmer.dsn`, which has 1 filled conduction
  area: after Delete All the filled count is 0 and `rules.ignore_conduction()` is set; U
  restores the area (count 1 again). The scene is patched from the change journal now
  (G05.5), so the scene and chunks equal a fresh build after the delete, the undo and the redo
  (`delete_all_and_undo_patch_the_scene` on `Issue026-J2_reference` with its session; the
  journal saw every trace and via, `last_sync.removed == wiring`, 0 full rebuilds).
  `ui_drc::delete_all_and_undo` and `delete_all_and_undo_with_wiring` still pass.

- [x] **G03.8 — File > Export DRC report.**
  - Writes `generate_report(source, "mm")` as JSON through the same
    serializer the CLI uses (`drc/json.rs`), where `source` is the design's
    file name, as `drc/runner.rs::run_drc` computes it. Writes to a temp
    file and renames it.
  - **Accept:** `tests/ui_drc.rs::drc_export_equals_cli`: for
    `Issue313-FastTest.dsn`, the exported JSON equals the report of
    `freerouting::drc::run_drc` with default options on the same path, with
    every field compared except the report's `date`
    (`drc/report.rs` stamps the current time).
  *Done 2026-10-15:* `drc_export.rs`, File > Export DRC report… (a small window with the
  path, default `<design stem>_drc.json` next to the design), `Command::ExportDrcReport`.
  A worker runs `generate_report(file name of the design, "mm")` over an `Arc` clone
  and writes `to_json()` (the CLI's serializer) to `<path>.tmp`, then renames it.
  `ui_drc::drc_export_equals_cli`: for Issue313-FastTest.dsn the exported JSON equals
  `run_drc` with default options on the same path in every member but `date`;
  `drc_export_equals_cli_with_session` does the same with the `.ses` on top;
  `export_menu_opens_a_dialog_with_a_default_path`.
