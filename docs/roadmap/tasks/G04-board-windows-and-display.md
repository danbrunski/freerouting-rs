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

- [ ] **G04.1 — Info > Nets.**
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

- [ ] **G04.2 — Placed Components, Library Packages, Library Padstacks.**
  - Ports of `WindowComponents`, `WindowPackages`, `WindowPadstacks`, on the
    generic list window. Select on a component selects
    `component_items(id)` and zooms to them.
  - **Accept:** `tests/ui_windows.rs::library_window_counts`: on
    `Issue159-setonix_2hp-pcb`, the three row counts equal
    `Components::count` (placed ones only, as Java lists them),
    `Packages::count` and `Padstacks::count`; Select on the first component
    zooms so that its items' bounding box is visible.

- [ ] **G04.3 — Item info window.**
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

- [ ] **G04.4 — Visibility sliders and layer dimming.**
  - Port of `WindowVisibility`: 0–100 % per layer and per object type, plus
    Reset; `WindowDisplayMisc`'s automatic layer-dimming slider replaces the
    fixed 0.6. 0 % hides, as today's unchecked box does.
  - **Accept:**
    - `tests/ui_windows.rs::visibility_slider_levels`: snapshots of
      Issue690 with the top copper layer at 0 %, 50 % and 100 %; the
      implementer opens all three and describes the difference.
    - `visibility_persists`: the slider values survive a save and restore
      (G01.8's round trip).

- [ ] **G04.5 — Virtual layers: silkscreen, courtyard, fabrication.**
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

- [ ] **G04.6 — Units, crosshair, rotation and mirroring.**
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

- [ ] **G04.7 — Colour manager.**
  - Port of `ColorManager`: per-layer item colours (traces, vias, pins,
    conduction areas, keepouts, via and place keepouts) and the "other"
    colours (background, highlight, incompletes, violations, outline,
    component front and back, length-matching area, drill hole, silkscreen,
    courtyard, fabrication); Reset to defaults; persisted.
  - **Accept:** `tests/ui_windows.rs::colour_change_persists`: changing the
    trace colour of layer 0 changes the colour the canvas uses for that
    batch (read from the palette, not from pixels), and the change survives
    a save and restore.

- [ ] **G04.8 — Status bar, log window, About.**
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
