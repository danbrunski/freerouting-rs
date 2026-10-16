# G04 — Board windows and display options

Opened 2026-10-15. Java's Info and Display menus show the board's nets,
components and library, explain any item, and let the user set per-layer
visibility, units, rotation, mirroring and colours. The port has on/off
checkboxes and millimetres only. This checklist fills the read-only side of
the Java GUI: everything here reads the board and changes only how it is
shown. It is the report's GUI-4.

**Prerequisites:** G03 (`Selection`, the generic list window
`src/panels/object_list.rs`, `src/text.rs`). G04.3 needs B08's item info and
is partial until it lands. G04.8's version line uses R01.2's build stamp
when it exists.

## What exists and what this adds

| Exists (Rust at `70389b7`, Java at `cda540f`) | State |
|---|---|
| Engine, nets | `board.rules.nets` (`get_by_name`, net classes through `rules::NetClasses::{count, get, iter}`), `BasicBoard::{connectable_items, net_pins, net_via_count, net_trace_length}`, `DesignRulesChecker::incomplete_count_of(net)` |
| Engine, components and library | `board::structure::component::Components::{count, iter, get, get_by_name}`, `Component::{placed_on_front, rotation_in_degree, part_number, package, location}`; `library::package::Packages::{count, iter}`, `library::padstack::Padstacks::{count, iter}`; `BasicBoard::component_items(id)` |
| Engine, outlines | `board::items::ComponentOutline { is_courtyard, is_fabrication, .. }` (public fields) and `is_front()`; set by the DSN reader (`io/specctra/parser/network.rs` ~line 1168) |
| Engine, item info | `DesignRulesChecker::item_description(id) -> String` only. The structured `item_info` (Java `Item.printInfo`, `board/actions/ItemInfoPrinter.java`, 72 lines) is B08 |
| Engine, units | `io::CoordinateTransform` (`dsn_to_board`, `board_to_dsn`) |
| GUI | Layer and kind checkboxes, `1`–`9` active layer, dimming fixed at 0.6, hover text in the status bar, warnings window; `scene.rs` draws all component outlines as one kind |
| Java windows | `gui/windows/board/{WindowNets,WindowComponents,WindowPackages,WindowPadstacks,WindowObjectInfo,WindowVisibility,WindowDisplayMisc,WindowAbout}.java`, `gui/controls/ColorManager.java`, `gui/rendering/{ItemColorTableModel,OtherColorTableModel}.java`, `gui/board/BoardPanelStatus.java`, the unit selector in `gui/board/BoardToolbar.java`, the virtual layers in `gui/rendering/BoardRenderer.java` (`virtualLayerFor`, `drawComponentPartNumbers`) and `gui/windows/routing/WindowSelectParameter.java` |

What this adds: Nets, Placed Components, Library Packages and Library
Padstacks windows; the item-info window; visibility sliders and a dimming
slider; six virtual layers; units, crosshair, rotation and mirroring;
a colour manager; a status bar with counters, a log window and About.

## Decisions made here

- **Every window is a list over engine data, rebuilt on demand.** Rows are
  computed when the window opens and on Recalculate, as Java does, not
  every frame.
- **Rotation and mirroring live in `Camera` only.** Picking and hover use
  the inverse of the same transform, so they keep hitting the right item.
  No board coordinate is ever rotated.
- **Units change display only.** Internal values stay in board units and mm;
  conversion happens at formatting time (`src/text.rs::format_length`).
- **Colours and visibility are persisted** through G01.8's
  `PersistedSettings` (schema bump, new fields `#[serde(default)]`).
- **No telemetry, no sponsor or e-mail dialogs** in About (G00 question 2).

## File ownership

Paths are under `crates/freerouting-gui/`.

| Item | New files | May edit |
|---|---|---|
| G04.1 | `src/panels/nets.rs`, `tests/ui_windows.rs` | `src/commands.rs`, `src/text.rs` (append) |
| G04.2 | `src/panels/{components,library}.rs` | `src/text.rs` (append) |
| G04.3 | `src/panels/item_info.rs` | `src/input.rs` (I, double-click) |
| G04.4 | — | `src/panels/layers.rs`, `src/view.rs` (colour and alpha per slot and kind only), `src/settings.rs` |
| G04.5 | — | `src/scene.rs` (`Kind`, `Slot` for virtual layers), `src/panels/layers.rs` |
| G04.6 | `src/panels/display.rs` | `src/view.rs` (`Camera`), `src/text.rs` |
| G04.7 | `src/panels/colors.rs` | `src/settings.rs`, `src/view.rs` (colour lookup) |
| G04.8 | `src/panels/{status,log,about}.rs` | `src/app.rs` |

G04.4, G04.5 and G04.7 touch colour lookup in `src/view.rs`; if G02 is
running at the same time, they go through a `Palette` struct that G02's
pipeline reads, and must not change the draw code itself.

## Checklist

- [x] **G04.1 — Info > Nets.**
  - Port of `WindowNets`: rows "Net #n (name) – Class" with pin count, via
    count and trace length (`net_pins`, `net_via_count`,
    `net_trace_length`); a filter; "incomplete connections only" using
    `incomplete_count_of`; Select on Board selects `connectable_items(net)`.
    Sorted by net number, as Java lists them (check `WindowNets` for the
    order and row format).
  - **Accept:** `tests/ui_windows.rs`:
    - `nets_filter_and_select`: on `Issue754-avionics_hub`, typing "GND"
      in the filter leaves only nets whose name contains it; selecting one
      and Select on Board makes the selection equal
      `connectable_items(net)`.
    - `nets_incomplete_only`: with the toggle on, the row count equals the
      number of nets with `incomplete_count_of(net) > 0`.
  *Done 2026-10-15:* `panels/nets.rs`, the nets list on the G03 `ListWindow` (new `ListSpec`: Java hides the nets window's Info, Invert and Recalculate buttons and names Select "Select on Board"), Info > Nets. Rows read `Net #n (name) - Class: c` in net number order; the filter looks at `Net #n (name)` (Java's `Net.toString`); the chosen rows print `Net.printInfo` (terminal items, via count, trace length in the chosen unit) under the list. `tests/ui_windows.rs::nets_filter_and_select` (Issue754, 368 nets, typing "GND" leaves the 39 whose name contains it, Select on Board makes the selection equal `connectable_items(net)`) and `nets_incomplete_only` (183 of 368 nets, equal to the nets with `incomplete_count_of(net) > 0`) pass; unit test `panels::nets::tests`.


- [x] **G04.2 — Placed Components, Library Packages, Library Padstacks.**
  - Ports of `WindowComponents`, `WindowPackages`, `WindowPadstacks`, on the
    generic list window. Select on a component selects
    `component_items(id)` and zooms to them.
  - **Accept:** `tests/ui_windows.rs::library_window_counts`: on
    `Issue159-setonix_2hp-pcb`, the three row counts equal
    `Components::count` (placed ones only, as Java lists them),
    `Packages::count` and `Padstacks::count`; Select on the first component
    zooms so that its items' bounding box is visible.
  *Done 2026-10-15:* `panels/{components,library}.rs`; Info > Placed Components, Library Packages, Library Padstacks, sorted by name ignoring case as Java sorts them. Java's `WindowComponents.fillList` lists `components.count()` components, placed or not (the "placed ones only" in the Accept text is the same set on Issue159: 15 components, all placed). `ui_windows.rs::library_window_counts`: Issue159 has 15 components, 6 packages, 9 padstacks, and the three lists have 15, 6 and 9 rows; Select on the first component selects `component_items(id)` and the visible box of the view contains the box of those items. `package_and_padstack_select`: the padstack rows' items add up to every pin and via on the board.


- [x] **G04.3 — Item info window.**
  *Waits on B08 (`item_info`).* Until then the item is partial.
  - Port of `WindowObjectInfo`: opened with I over an item, by
    double-click, or from a list's Info button; shows B08's `ItemInfo`
    fields (kind, nets, layers, clearance class name, fixed state,
    component and pin, padstack, trace width and length, area) in Java's
    order and wording (`Item.printInfo`, `ItemInfoPrinter`).
  - **Accept:** `tests/ui_windows.rs::item_info_fields`: on Issue159, the
    window for one pad and one trace shows exactly the fields B08's
    `item_info` returns for them, in order; the implementer compares one of
    each with Java's `printInfo` output for the same item (a probe through
    F02.1's `probe.sh`, or B08's own test) and records both in the done
    note.
  *Partial 2026-10-15:* nothing done. `BasicBoard::item_info` and `ItemInfo` (B08.2, `board/item_info.rs`) have not landed, and the item info window is a rendering of exactly those fields. The Info buttons of all lists stay disabled (`ListSpec::show_info`) and the I key stays free. Remainder: the window (`src/panels/item_info.rs`), the I key and double-click in `input.rs`/`viewport.rs`, the Info button of the lists, `item_info_fields`, and the one-pad-one-trace comparison with Java's `printInfo`. Owner: B08.2.
  *Done 2026-10-16:* B08.2 landed (commit 0597f09); G05 wired the window because its selection toolbar has an Info button. `panels/item_info.rs` is Java's `WindowObjectInfo`: one line per item from `BasicBoard::item_info` (the engine's fields, in its order, with Java's words between them: `Pin: component U1, pin 15, padstack X at (108.41 , -78.55), clearance class smd, Net +3V3, contacts 1`), a bold heading, the `Summary:` line (pins, vias, traces and the cumulative trace length) and the title `Selected Items`. It opens from the selection toolbar's Info button, from the key `I` (with items selected), and from the Info button of every list that has one (Info > Clearance Violations, Incomplete Connections, ...; the nets window hides it as in Java); lengths and points are in the unit of the toolbar selector. `tests/ui_edit.rs::item_info_fields` (the acceptance test lives in G05's file): on `Issue026-J2_reference` with its session, the window for one pad (U1 pin 15) and one trace shows exactly the line the test builds from `item_info`'s fields, in order; the engine's fields themselves equal Java's `printInfo` output for the same boards in B08.2's `tests/board_probe_item_info.rs::item_info_matches_java_on_j2` (pads and traces are among its items), which is the Java comparison the item asked for. `list_info_button_opens_the_item_info`: the Info button of the clearance violations list opens a window for both items of the chosen violation. Not ported: the buttons Java puts next to component, net and padstack names (they open their own info windows); the names are text. Deliberate differences are in `docs/DIVERGENCES.md` (G05 section).


- [x] **G04.4 — Visibility sliders and layer dimming.**
  - Port of `WindowVisibility`: 0–100 % per layer and per object type, plus
    Reset; `WindowDisplayMisc`'s automatic layer-dimming slider replaces the
    fixed 0.6. 0 % hides, as today's unchecked box does.
  - **Accept:**
    - `tests/ui_windows.rs::visibility_slider_levels`: snapshots of
      Issue690 with the top copper layer at 0 %, 50 % and 100 %; the
      implementer opens all three and describes the difference.
    - `visibility_persists`: the slider values survive a save and restore
      (G01.8's round trip).
  *Done 2026-10-15:* `Look` (`look.rs`) holds a 0 to 100 % level per copper layer and per object kind (ten kinds the port draws) and the dimming slider (default 40 % = the old fixed 0.6); `layers::visibility_ui` is the window (sliders in steps of 10, Reset), `panels/display.rs` has the dimming slider; 0 hides (the layer or kind leaves the plan), a level between mixes opaque colours into the background and lowers the alpha of translucent ones. `ui_windows.rs::visibility_slider_levels` (Issue690, top layer at 0, 50, 100 %: items of that layer in the draw list 0, 2442, 2442; the colour-table entry is the layer's own colour at 100 % and `background.lerp_to_gamma(own, 0.5)` at 50 %; snapshots `visibility_issue690_{0,50,100}.png`). I opened all three: at 0 % the red top-layer traces, pads and vias are gone and the green, orange and blue inner and bottom copper shows through (the via discs of the other layers stay); at 50 % the top layer is a dark red; at 100 % it is the bright red of before. `visibility_persists`: the layer level, two object levels, the dimming and a virtual layer survive a save and a fresh app (`settings.rs`, by layer name and object label; the schema stays 1, because the new fields are `#[serde(default)]` and a bump would drop the user's recent files). Unit tests in `look::tests`.


- [x] **G04.5 — Virtual layers: silkscreen, courtyard, fabrication.**
  - Split component outlines by `is_courtyard`, `is_fabrication` and
    `is_front()` into six slots (F/B silkscreen, courtyard, fabrication)
    with six toggles, as in `WindowSelectParameter`. Part numbers are drawn
    on the fabrication layers, as `BoardRenderer.drawComponentPartNumbers`
    does; reference designators stay as today.
  - **Accept:** `tests/ui_windows.rs::virtual_layer_counts`: on a fixture
    whose DSN has courtyard outlines (find one with `is_courtyard` set; name
    it in the test), the six slots' item counts sum to the number of
    `ComponentOutline` items on the board, and hiding the courtyard toggle
    removes exactly the courtyard items from the draw list (stats counter);
    a snapshot with only courtyards visible, opened and described.
  *Done 2026-10-15:* `Slot::{Courtyard, Fab}(Side)` next to `Silk(Side)` (six virtual layers in Java's order, `look::virtual_index`), six toggles in the side panel (`WindowSelectParameter` names and tooltips), and the part numbers of placed components drawn on the fabrication layers (`Scene::part_numbers`, `paint_part_numbers`; under the designator in a smaller type instead of Java's fixed 12 pt at the component's location, see DIVERGENCES). `ui_windows.rs::virtual_layer_counts` on Issue159 (164 `ComponentOutline` items: 31 courtyard, 67 fabrication, 66 silkscreen): the six slots hold 66, 0, 30, 1, 60, 7 scene items, summing to 164; hiding the two courtyard toggles takes the draw list from 164 to 133 outline items, exactly the 31 courtyard items; 15 part numbers. `snapshot_courtyards_only` (`courtyards_issue159.png`): only the magenta and cyan courtyard circles and rectangles and the yellow board outline are drawn, the copper, other outlines, designators and airlines hidden. The G01 canvas golden test now sets the courtyard and fabrication colours to the silkscreen ones, because the golden predates the split.


- [x] **G04.6 — Units, crosshair, rotation and mirroring.**
  - A unit selector (mm, mil, inch, µm) used by the status bar, the lists
    and the item-info window (Java `gui/board/BoardToolbar.java` ~line 456,
    `gui/workspace/GuiBoardManager.java` `changeUserUnit`).
  - `WindowDisplayMisc`: crosshair (small or large; `,` toggles), rotation
    0/90/180/−90, mirroring left-right and top-bottom, all in `Camera`.
  - **Accept:**
    - `text::tests::mil_is_mm_over_0_0254`: formatting 2.54 mm in mil gives
      "100" (Java's number format; check `GuiBoardManager` and
      `BoardPanelStatus` for the digits shown).
    - `tests/ui_windows.rs::hover_under_rotation`: on Issue159, the hover
      text over a known pad's centre names that pad at rotation 0, 90, 180
      and −90 and with each mirroring.
  *Done 2026-10-15:* `view::Orientation` (quarter turns, counterclockwise, then left-right or top-bottom mirroring, Java's order) in `Camera`; the 2x2 basis (entries 0, 1, -1) goes to both shaders through the camera uniform (48 bytes now) and to `paint_cpu`, so the map is exact and at rotation 0 bit for bit the old one (the existing snapshots and the G01 golden are unchanged). Picking, hover, pan, zoom, zoom-to-rectangle, fit and culling use the same map or its inverse; labels stay upright. `panels/display.rs` (Display > Miscellaneous: crosshair small or large, rotation, mirroring, dimming), the unit selector in the toolbar (mil, in, mm, um), `text::{format_length, format_fixed, format_position, unit_label}`, `,` unchanged. `text::tests::mil_is_mm_over_0_0254` (2.54 mm in mil is "100"; the lists use the four-digit `NumberFormat`, the status bar two fixed digits padded to 10, as `FloatPoint.toString(locale, 2, 10)`). `ui_windows.rs::hover_under_rotation`: on Issue159 the hover text over a known pad's centre names that pad (`Pad · U… · net · layer`) at each of the 4 rotations with each of the 3 mirror settings (12 cases), and the cursor reads back the pad centre within 0.05 mm. `ui_render.rs::cpu_and_gpu_match_when_rotated`: GPU and CPU differ on 0.0000 % of the canvas at 5 orientations and differ from the upright board by more than 2 %; `view::tests` pin the eight maps against Java's `ScreenTransform`; `snapshot_rotated_and_mirrored` (`rotated_mirrored_issue159.png`, looked at: the board lies on its side with the LED row at the right, text upright). Also `unit_selector_changes_the_status_bar`.


- [x] **G04.7 — Colour manager.**
  - Port of `ColorManager`: per-layer item colours (traces, vias, pins,
    conduction areas, keepouts, via and place keepouts) and the "other"
    colours (background, highlight, incompletes, violations, outline,
    component front and back, length-matching area, drill hole, silkscreen,
    courtyard, fabrication); Reset to defaults; persisted.
  - **Accept:** `tests/ui_windows.rs::colour_change_persists`: changing the
    trace colour of layer 0 changes the colour the canvas uses for that
    batch (read from the palette, not from pixels), and the change survives
    a save and restore.
  *Done 2026-10-15:* `panels/colors.rs` (Display > Color Manager: a colour button per layer and kind: traces, vias, pins, conduction areas, keepouts, via keepouts, component keepouts; and the other colours: background, highlight, incomplete connections, clearance violations, outline, silkscreen, courtyard and fabrication, front and back; Reset to Defaults) over `Look`, which the canvas's colour table, the highlight, the airlines and the violation markers read. Not ported (nothing draws them or the scene lacks the data): fixed traces and vias, component front and back, length matching area, drill hole. `ui_windows.rs::colour_change_persists`: `SetItemColor(0, Trace, [10, 200, 30])` changes the colour-table entry of (layer 0, traces) from red to that colour (read from `canvas::color_table`, not pixels), the background colour change comes back with a fresh app, the layer colour comes back after the board is opened (by layer name), another layer is untouched, and Reset restores the table and empties the stored colours. `look::tests::colour_edits_round_trip_by_layer_name`. Screenshot of the real app under Xvfb: the colour window shows the 2-layer table and the eleven other colours.


- [x] **G04.8 — Status bar, log window, About.**
  - Port of `BoardPanelStatus`: cursor position in the chosen unit, active
    layer, a status line, and error and warning counters. Clicking a
    counter opens the log window.
  - The log collects DSN reader warnings, `BasicBoard::log_messages()` and
    GUI events (loads, saves, DRC runs), newest last.
  - About (`WindowAbout`): version, git commit (from R01.2's build stamp, or
    "unknown" until it lands), GPL-3.0, project links. No telemetry.
  - **Accept:** `tests/ui_windows.rs::warning_counter_opens_log`: on
    `Issue754-avionics_hub`, the warning counter equals the number of
    loader warnings, and clicking it opens the log listing them in order.
    `about_shows_version`: About shows `CARGO_PKG_VERSION`.
  *Done 2026-10-15:* `panels/{status,log,about}.rs`, `windows.rs`. The status bar has `Errors: n` and `Warnings: n` (clicking opens the log), the status line, `Current layer: name` and the cursor position with the unit label. `Log::entries` lists the loader warnings, the engine's `log_messages` and the app's events (a load, each design rules check, an export), newest last; the counters count the Error and Warning entries. `ui_windows.rs::warning_counter_opens_log` (Issue754: 8 loader warnings): the bar reads `Warnings: 8` and `Errors: 0`, clicking the counter opens the log, whose warning entries are the 8 loader warnings in order and are the first 8 entries; `errors_are_counted` (an unreadable file gives `Errors: 1`); `about_shows_version` (Help > About shows `Version: 0.1.0` = `CARGO_PKG_VERSION`, the license and the links; `Commit: unknown` until R01.2 sets `FREEROUTING_GIT_COMMIT`); `status_bar_names_the_layer`. Looked at the real app under Xvfb (`freerouting-gui` on Issue159, Info, Display and Help menus, Nets, Display Options, Color Manager and About windows open): all draw as expected. Java's "current board score" field is not ported (no score yet).

