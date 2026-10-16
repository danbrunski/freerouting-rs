# A05 — Path location

Opened 2026-10-15. After the search reaches a destination, Java walks the backtrack
chain from the destination door to the start, collects the items it decided to rip,
and turns the chain of door sections into trace corners, one polyline per layer. The
corner rules for the 45° case are in `FoundConnectionLocator45Degree`. This checklist
ports both and compares the located path with Java's for every sampled connection, the
last rung-1 step before insertion ([A00](../A00-autorouter-README.md)).

**Prerequisites:** A04 (the search and its result). Decisions D1, D6.

## What exists and what this adds

| Exists | State |
|---|---|
| A04's `SearchResult` and the session's section state (backtrack door and section, `room_ripped`, `ripup_cost`, adjustment) | Prerequisite |
| Geometry: `FloatPoint::round` (Java `Math.round`), `FloatLine` projections, `TileShape::nearest_point_approx`, `IntOctagon` corners, `AngleRestriction` | Present in `freerouting-geometry` |
| `path/FoundConnectionLocator.java` (570 lines): constructor (`:62-183`), `getInstance` (`:185-211`), `calculateStartingPoint` (`:213-223`), `backtrack` (`:225-327`), `ninetyDegreeCorner`, `fortyfiveDegreeCorner`, `calculateAdditionalCorner` (`:329-408`), `calculateNextTrace` (`:410-500`), `adjustStartCorner` (`:522-545`) | Missing |
| `path/FoundConnectionLocator45Degree.java` (372 lines) | Missing |

What this adds: `autoroute/path/` with backtracking and the 45° locator, and the
`locate` probe.

## Decisions made here

- **Identity becomes an explicit enum.** Java compares `FloatPoint`s by reference at
  `FoundConnectionLocator.java:415` (`adjustedStartCorner != this.currentFromPoint`) and
  `:432` (`currentNextCorner != prevCorner`): a new point with an equal value still
  counts as a new corner and advances `previousFromPoint`. Rust passes
  `Corner::Unchanged | Corner::New(FloatPoint)` and never compares values there.
  `// PARITY:` on both sites.
- **The ripped set and costs are filled during backtracking**, before any acceptance
  check, in Java's order (`backtrack`, `:225-327`). The caller's set is a
  `BTreeSet<ItemId>` (descending id, like Java's `TreeSet<Item>`); the costs are a
  `Vec<(ItemId, i32)>` that keeps the first insertion of each item, like Java's
  `LinkedHashMap` keyed by identity. A rejected result still leaves both filled; A06 and
  A12 rely on that.
- **The drill room index stays as Java has it.** `backtrack` reads
  `roomArr[firstLayer + section]`; every drill starts at layer 0, so it is harmless, but
  it is ported as written, `// PARITY(quirk):`.
- **Locator output** is `Located { items: Vec<ResultItem { corners: Vec<IntPoint>, layer
  }>, start_item, target_item: Option<ItemId>, start_layer, target_layer }`. An empty
  `items` (the start door is not a target door, `:103-111`) is a valid result, as in Java.
- **Exceptions.** Java's `autorouteConnection` catches a locator exception and returns
  `FAILED` (`AutorouteEngine.java:179-195`). The Rust locator returns `Result<Located,
  LocateError>`.

## File ownership

| Item | New files | May edit |
|---|---|---|
| A05.1 | `autoroute/path/mod.rs`, `autoroute/path/backtrack.rs` | `autoroute/mod.rs` (append) |
| A05.2 | `autoroute/path/locate.rs` | — |
| A05.3 | `autoroute/path/locate45.rs` | — |
| A05.4 | — | `path/locate.rs` |
| A05.5 | `crates/freerouting/tests/autoroute_probe_locate.rs`, `fixtures/reference/probe/locate/` | `AutorouteProbe.java`, `fixtures.tsv`, `tests/autoroute_search_sinks.rs` |

## Checklist

- [x] **A05.1 — Backtrack.** Port `backtrack` (`:225-327`) as `path::backtrack(session,
  result, ripped, costs, net) -> Vec<BacktrackElement { door, section, next_room }>`:
  follow each section's backtrack door and section from the destination; for an obstacle
  room entered with a ripup, add the item to `ripped` and its cost to `costs` at the moment
  Java does; resolve a drill's room on the section's layer with the index rule from the
  decisions.
  - Orders: the chain order, and the order of additions to `ripped` and `costs`.
  - **Accept:** covered by A05.5; a unit test that an item met twice keeps its first cost.
  - *Done 2026-10-16:* `autoroute/path/backtrack.rs`: `backtrack(session, result, ripped,
    costs: Option<&mut RipupCosts>) -> Result<Vec<BacktrackElement { door, section, next_room }>,
    LocateError>` (Java's `netNumber` parameter only feeds trace logging and is dropped), the drill
    destination's `roomArr[firstLayer + section]` ported as written (`// PARITY(quirk):`), the
    clamp of a section past the end, the drill destination's rooms of every layer ripped first, then
    each chain element entered with a ripup. `put_cost` is `LinkedHashMap.put` keyed by identity: a
    Java probe at `a773a70ec` (`LinkedProbe.java`: `put(a,100); put(b,200); put(a,300)`) printed
    `a=300 b=200`, so an item met twice keeps its **first position and takes its last cost**; the
    unit test pins that, not the "first cost" this clause's wording says
    (`autoroute::path::backtrack::tests::an_item_met_twice_keeps_its_first_place_and_its_last_cost`,
    `the_ripped_set_iterates_by_descending_id_like_javas_tree_set`). Chain order and the ripped
    ids and costs are compared by A05.5's `locate_matches_java` and `locate_routed_matches_java`
    (three routed connections rip an item: `[100]` cost 1506009 twice, `[387]` cost 1).

- [x] **A05.2 — The locator driver.** Port the constructor's loop (`:62-183`):
  - `calculate_starting_point` (`:213-223`): the rounded centre of the destination
    connection shape (`trace_connection_shape` ∩ door room);
  - a drill destination (fanout only) starts at the drill location, with
    `target_layer = drill.first_layer + section` and `at_fanout_end`;
  - per trace: find the next drill in the chain, or the final target, whose shape is the
    start item's connection shape ∩ the start door's room, shrunk by the compensated
    half width when the intersection is two-dimensional and the shrunk shape is not empty
    (`:160-176`);
  - `calculate_next_trace(layer_changed, at_fanout_end)` (`:410-500`) with
    `adjust_start_corner` (`:522-545`) and the identity enum; round corners with Java
    `Math.round`, drop consecutive duplicates, take the layer from the drill's next room;
  - `get_instance` (`:185-211`) dispatches on the angle restriction: 45° and 90° go to the
    45° locator, any-angle returns `LocateError::UnsupportedAngle` until A07.
  - **Accept:** covered by A05.5.
  - *Done 2026-10-16:* `autoroute/path/locate.rs`: `locate(session, result, angle, ripped, costs)
    -> Result<Located, LocateError>` (Java `getInstance` plus the constructor), `Locator::run`
    (the constructor's loop: the next drill or the final target, the target shape shrunk by the
    compensated half width when two-dimensional and not empty), `calculate_starting_point`,
    `calculate_next_trace` (rounding with `FloatPoint::round`, consecutive duplicates dropped, the
    next layer from the drill's next room), `adjust_start_corner`, and `Corner::{Unchanged, New}`
    for the identity tests at `:415` and `:432` (`// PARITY:` on both). Covered by A05.5: equal on
    all 17 ladder boards and 7 routed boards, 24 located connections with more than one trace and
    88 `ExpansionDrill` chain elements among them. Unit tests in `autoroute::path::tests`
    (`a_found_connection_is_located_from_the_destination_to_the_start`,
    `ninety_degree_routing_uses_the_45_degree_locator_with_orthogonal_corners`,
    `any_angle_routing_is_unsupported_until_a07`).

- [x] **A05.3 — 45° corners.** Port `FoundConnectionLocator45Degree.java`:
  `round_to_integer` (`:40-46`), `calc_horizontal_first_from_door` (`:48-102`),
  `calculate_next_trace_corners` (`:104-317`), `calc_horizontal_first_to_door`
  (`:319-371`), and the base class's `ninety_degree_corner`, `fortyfive_degree_corner` and
  `calculate_additional_corner` (`:329-408`). Door section points come from A02.5's
  `section_segments(compensated_trace_half_width[layer])`, including its reallocation side
  effect.
  - **Accept:** covered by A05.5.
  - *Done 2026-10-16:* `autoroute/path/locate45.rs`: `round_to_integer`,
    `calc_horizontal_first_from_door`, `calculate_next_trace_corners`,
    `calc_horizontal_first_to_door`, `ninety_degree_corner`, `fortyfive_degree_corner`,
    `calculate_additional_corner`; door sections from `Session::section_segments(door,
    compensated_trace_half_width[layer])` with its reallocation (`// PARITY(quirk):`); Java's
    `nearestBorderPointsApprox(p, 2)` array keeps a trailing `null` where the port's list is shorter,
    so a missing second point Java would read is a `NullPointer` error. Covered by A05.5
    (`locate_matches_java`, `locate_routed_matches_java`, every corner equal); unit tests of the
    corner branches (`fortyfive_degree_corners_follow_javas_branches`,
    `ninety_degree_corners_go_horizontal_or_vertical_first`, `no_restriction_returns_the_target`).

- [x] **A05.4 — Result and errors.** The `Located` type from the decisions, the
  `LocateError` variants (one per Java exception site that a probe shows can fire), and a
  `Display` for each that names the Java class and line.
  - **Accept:** unit tests for the empty-items case and for each error variant's mapping.
  - *Done 2026-10-16:* `Located { items: Vec<ResultItem { corners, layer }>, start_item:
    Option<ItemId>, target_item: Option<ItemId>, start_layer, target_layer, backtrack }` (the chain
    is kept for the probe and diagnostics, as Java's `emitDiagnostics` reads it). `LocateError::{
    UnsupportedAngle, NullPointer { at }, IndexOutOfBounds { at }, ClassCast { at }, EndlessLoop {
    at }, Room(RoomError) }`, `java_exception()` and a `Display` naming the Java class and line. No
    `locate` or `locate-routed` board makes Java's locator throw (no `exception` line after a found
    connection in the 24 references), so the variants are one per Java exception kind at the sites
    where Java can throw. `EndlessLoop` is the two warning branches of `calculateNextTraceCorners`
    that return corners without advancing, where Java loops until `OutOfMemoryError`
    (DIVERGENCES.md, "Path location"). Tests:
    `a_chain_that_does_not_start_at_a_target_door_locates_no_items` and
    `a_destination_that_is_neither_a_target_door_nor_a_drill_locates_no_items` (the two empty-items
    cases, `:103-111` and `:130-134`), `each_error_maps_to_its_java_exception_and_names_the_java_site`,
    `a_destination_section_out_of_range_is_an_index_error`,
    `a_warning_branch_without_corners_ends_the_trace_and_with_corners_loops`.

- [x] **A05.5 — `locate` probe; stepped path.**
  - Java kind `locate`: after `findConnection` for each sampled connection, print the
    backtrack array (door kind and id, section, next room id), the `ResultItem`s (corners
    as integer points, layer), the ripped item ids in set order with their costs in map
    order, start and target item ids, start and target layers. A connection whose search
    failed prints one `{"result":"none"}` line.
  - **Accept:**
    - `locate_matches_java` equal for every sampled connection on the fixture ladder up to
      and including `Issue026-J2_reference`, both ripup variants.
    - `stepped_search_matches_find_connection` (A04.8) also compares the located corners:
      equal on its three fixtures.
  - *Done 2026-10-16:* Java kinds `locate` (`@ladder`) and `locate-routed` (the 7 `maze-routed`
    boards: traces unfixed, nets cut one by one, so ripped items and costs appear) appended to
    `AutorouteProbe.java`, with rows in `fixtures.tsv` and the README; references from
    `generate.sh <a773a70ec worktree> locate|locate-routed` in `fixtures/reference/probe/locate/`
    (17 boards, 156 KiB) and `locate-routed/` (7 boards, 120 KiB), a second run byte-identical
    (`diff -r`). `tests/autoroute_probe_locate.rs::locate_matches_java`: equal on all 17 ladder
    boards (to J2 and the three pre-routed boards after it), ripup off and on, `EXPECTED_MISMATCHES`
    empty (J2: 44 trace items, 40 drill chain elements; two failed searches print
    `{"result":"none"}`); `locate_routed_matches_java` equal on 7 boards,
    `ROUTED_EXPECTED_MISMATCHES` empty. `tests/autoroute_search_sinks.rs::
    stepped_search_matches_find_connection` now also locates both searches and requires equal trace
    items on J2, `Issue508-DAC2020_bm08` and `scoring-mixed-layer` (passes).
