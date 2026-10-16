# A03 — Drill pages and drills

Opened 2026-10-15. Layer changes in the maze search go through *drills*: candidate via
locations that Java computes per board page for the current net, by cutting the
obstacles out of the page and splitting the rest into convex pieces. Each drill finds
or creates its complete room on every layer. This checklist ports that and compares
every page's drills with Java. It is a rung-1 comparison ([A00](../A00-autorouter-README.md)).

**Prerequisites:** A02 (rooms, `complete_expansion_room`, the session). Decisions D1,
D5, D6.

## What exists and what this adds

| Exists | State |
|---|---|
| `PolylineArea::split_to_convex` (`freerouting-geometry`) with Java's seeded `Random(99)` per call | Ported and differential-tested |
| `overlapping_tree_entries` on the autoroute tree; B02's `overlapping_objects` returning `TreeObject`s in Java order | B02 prerequisite |
| `BasicBoard::overlapping_items` (default tree) | Present (`board/basic_board.rs`) |
| `autoroute/drill/DrillPageArray.java` (121), `DrillPage.java` (197), `ExpansionDrill.java` (140) | Missing |
| `Via.getAutorouteDrillInfo` (`Via.java:205`, the cached `ExpansionDrill` of a via) | Missing; becomes session state |

What this adds: `autoroute/session/drills.rs` and the `drills` probe.

## Decisions made here

- **Pages and drills live in the session**, in `pages: Vec<DrillPage>` (row-major, the
  order Java's `pages[j][i]` is scanned) and `drills: Vec<ExpansionDrill>` with
  `DrillId(u32)`. Section state uses A02's flat section array: a page has one section per
  layer, a drill one per layer of its span.
- **Java creates a fresh page array per engine**, so per connection. The port does the
  same in the parity phase. Reusing pages across connections changes drill ids and tree
  topology, so it is a P-series question, not done here.
- **A stopped split leaves the page empty and not recomputed.** When
  `splitToConvex` returns null on a stop, Java has already set `netNumber` and an empty
  drill list (`DrillPage.java:63-104`), so the page stays empty for the rest of the
  connection. Keep that.
- **The via drill cache** (`Via.getAutorouteDrillInfo`) becomes an `Option<DrillId>` in
  the session's side-table payload (A02), created lazily as Java does
  (`MazeSearchEngine.java:635` calls it for via obstacle rooms; A04 uses it).

## File ownership

| Item | New files | May edit |
|---|---|---|
| A03.1–A03.4 | `autoroute/session/drills.rs` | `session/mod.rs` (append), `session/rooms.rs` (the invalidation call from A02.2) |
| A03.5 | `crates/freerouting/tests/autoroute_probe_drills.rs`, `fixtures/reference/probe/drills/` | `AutorouteProbe.java`, `fixtures.tsv` |

## Checklist

- [x] **A03.1 — `DrillPageArray`.** Port `DrillPageArray.java`:
  - constructor (`:34-61`): `max_page_width = max((5.0 * default_via_diameter) as i32,
    10000)` (`AutorouteEngine.java:88-89`), `column_count = ceil(length / width) as i32`,
    `row_count` likewise, `page_width = ceil(length / column_count) as i32`; the last
    column and row end at the board bounds. All divisions in `f64` as in Java;
  - `overlapping_pages(shape)` (`:76-98`): the shape's box intersected with the bounds;
    `min_j = floor(..)` as `i32`, `max_j` kept as an `f64` and the loop bound is
    `j < max_j` (likewise `i`); a page is included when the intersection's dimension is
    above 1. Row-major order, `// PARITY:`;
  - `invalidate(shape)`, `reset()`.
  - **Accept:** a unit test on a board whose width is not a multiple of the page width,
    and on a shape whose box ends exactly on a page boundary (the `j < max_j` case).
  - *Done 2026-10-16:* `autoroute/session/drills.rs`: `DrillPageArray::{new, max_page_width,
    overlapping_pages, invalidate, page_at}` (all divisions in `f64`, `d2i` for Java's `(int)`,
    row-major `pages: Vec<DrillPage>`, `// PARITY:` on the `j < max_j` loops); the session
    builds the array on first use (`Session::drill_pages`) from `board.bounding_box()` and
    `rules.default_via_diameter`, one section per layer per page in the flat section array;
    `Session::reset_drill_pages` is `reset()`. Tests in `autoroute::session::drills::tests`:
    `pages_of_a_board_whose_width_is_no_multiple_of_the_page_width` (25000 wide, limit 10000:
    3 columns of 8334, the last ending at 25000), `a_shape_ending_on_a_page_boundary_does_not_reach_the_next_page`
    (box ending at x = 8334, y = 6000 gives page 0 only; row-major order across both
    boundaries; clipping), `an_empty_board_has_no_pages`,
    `page_widths_follow_the_via_diameter_with_a_floor_of_10000`.

- [x] **A03.2 — `DrillPage::drills`.** Port `DrillPage.getDrills` (`:63-135`):
  - recompute when the page has no drills yet or the session net differs;
  - `overlapping_tree_entries(page, all layers)` on the autoroute tree, in tree order;
    skip non-items, items drillable for the net, and (with `attach_smd`) own-net pins with
    `drill_allowed`;
  - the `prev_obstacle_shape` rule: cut out an obstacle only if the previous obstacle's
    shape does not contain it, and set the previous shape after every obstacle that was not
    skipped (`:85-96`);
  - `PolylineArea::new(page, holes).split_to_convex(stop)`; `None` leaves the page empty
    (see the decisions);
  - the drill location: with `attach_smd`, `calc_pin_center_in_drill` on the first layer,
    then the last (`:49-61`). It walks `overlapping_items(shape, layer)` and keeps the
    **last** matching pin, which is the lowest id. Otherwise the drill shape's
    `centre_of_gravity().round()` with Java `Math.round` semantics;
  - every drill spans layer 0 to the last layer; keep it only if
    `calculate_expansion_rooms` succeeds.
  - **Accept:** covered by A03.5.
  - *Done 2026-10-16:* `Session::page_drills(page, attach_smd)` (cache check on `None` drills or
    another net; net and an empty list set before the split), `page_cutouts` (items of
    `overlapping_tree_entries` on the autoroute tree in tree order, the drillable and
    `attach_smd` own-net SMD pin skips, the previous-obstacle rule with `IntBox::EMPTY` as
    the first previous shape and `contains_shape` = Java `TileShape.contains(TileShape)`),
    `PolylineArea::new(page, holes).split_to_convex(self.stop)` (`None` leaves the page empty:
    `a_stopped_split_leaves_the_page_empty_for_the_net`), `pin_center_in_drill` (default-tree
    `overlapping_objects` in descending id, last match wins) on the first then the last layer,
    else `centre_of_gravity().round()`; drills span 0 to the last layer and are kept only when
    `calculate_expansion_rooms` succeeds. Unit test
    `page_drills_avoid_obstacles_and_attach_to_own_smd_pins`; equality with Java in A03.5.

- [x] **A03.3 — `ExpansionDrill`.** Port `ExpansionDrill.java`:
  - `calculate_expansion_rooms` (`:55-92`): one `overlapping_objects(point tile, all
    layers)` query before the layer loop. For each layer, scan the remaining objects in
    `TreeObject` order, dropping non-rooms as the scan passes them; the first complete room
    on that layer is taken and removed (rooms come first in descending id, so the highest
    id wins). Rooms created for earlier layers are not in the list. With no room, complete
    an incomplete room around the point (`shape = None`, contained = the point tile); it
    must give exactly one room, else the drill fails;
  - `id()` from A01.2, `reset`, one section per layer.
  - **Accept:** covered by A03.5; a unit test where a room on another layer precedes the
    wanted one in the scan.
  - *Done 2026-10-16:* `ExpansionDrill { shape, location, first_layer, last_layer, rooms,
    sections }` in the arena `Session::drills` (`DrillId(u32)`, `java_id()` from
    `java_ids::expansion_drill_id`), `Session::{new_drill, calculate_expansion_rooms,
    reset_drill, drill_sections}`: one `overlapping_objects(point tile, -1)` query, per layer
    `take_room_on_layer` (non-rooms removed as passed, first room on the layer taken and
    removed, `// PARITY:`), else an incomplete room in the arena only (`shape = None`,
    contained = the point tile) through `complete_expansion_room`, failing unless it gives
    exactly one room. Tests: `a_drill_takes_the_first_room_of_each_layer_in_tree_object_order`
    (room 7 on layer 1 is scanned before room 5 on layer 0, room 5 wins over room 3; result
    `[5, 7]`), `a_drill_completes_a_room_where_a_layer_has_none`, `a_drill_on_a_foreign_pin_fails`;
    equality with Java in A03.5.

- [x] **A03.4 — Page and drill reset; the via drill cache.** `DrillPage.reset` resets its
  drills and its own sections (`:158-170`); `invalidate` drops the drills. The via drill
  entry from the decisions, with the same lazy creation and reset semantics as
  `Via.getAutorouteDrillInfo` (`Via.java:205`).
  - **Accept:** unit tests for reset and invalidate.
  - *Done 2026-10-16:* `Session::{reset_drill_page, reset_drill_pages, invalidate_drill_page,
    overlapping_drill_pages}`, `DrillPage::invalidate`; `rooms.rs::invalidate_drill_pages` now
    invalidates the pages of a removed room (A02.2's hook); `clear` drops pages and drills
    (`clear_drills`). Via cache: `ItemAutorouteInfo.drill: Option<DrillId>` and
    `Session::via_drill(via)` (`Via.getAutorouteDrillInfo`: autoroute info first, drill at the
    via centre with its point tile, the via's obstacle room per layer; cached; dropped by
    `clear` with the item data as `clearAutorouteInfo` does; `None` for a non-via). Tests:
    `resetting_a_page_resets_its_drills_and_its_sections`,
    `page_drills_are_cached_until_invalidated` (only the overlapped page loses its drills;
    `clear` drops the array), `a_via_drill_is_created_once_and_dropped_with_the_autoroute_data`.
    `cargo test -p freerouting --lib autoroute::session`: 29 passed.

- [x] **A03.5 — `drills` probe.** Java kind `drills`, standalone: for each sampled
  connection, after `getInstance` (so the start rooms exist, as in a real search), call
  `getDrills` on **every** page of the engine's page array in row-major order, with
  `attach_smd = ctrl.attachSmdAllowed`. Print per page: page id, bounds, drill count; per
  drill: location, shape, first and last layer, room id per layer. Then the room id
  counter.
  - **Accept:** `drills_match_java` equal on `scoring-mixed-layer`, `Issue143-rpi_splitter`,
    `Issue143-rpi_splitter_mod`, `Issue508-DAC2020_bm08`, `Issue508-SMD-routing-issue-demo`
    (attach rules) and `Issue026-J2_reference`, both ripup variants.
  - *Done 2026-10-16:* Java kind `drills` appended to `AutorouteProbe.java` (pages by
    reflection on `DrillPageArray.pages`), row `drills` in `fixtures.tsv`, README row;
    references from `generate.sh <a773a70ec worktree> drills` (6 boards, 29.5 s; a second run
    byte-identical, `diff -r`) in `fixtures/reference/probe/drills/` (J2, `rpi_splitter` and
    the SMD demo as digests). No fixture's via rule allows attaching to SMD pins
    (`attachSmdAllowed` is false in every `control` reference), so the probe adds a third
    variant `smd` (ripup off, `attach_smd` forced on); it changes the drills on all six boards
    (J2: 3636 drills off, 3560 smd). `tests/autoroute_probe_drills.rs::drills_match_java`
    equal on all six boards in all three variants (`EXPECTED_MISMATCHES` empty; J2: 36
    connection runs with pages, 3024 page lines, 10832 drills, tree walk hash after `clear()`
    equal); `cargo test -p freerouting --test autoroute_probe_drills`: 1 passed. A changed
    reference line is reported (edited line 500 of the DAC2020 reference: "line 500 differs").
