# A06 — Connection insertion and `route_connection` (rung 2)

Opened 2026-10-15. A located path becomes copper here. `FoundConnectionInserter`
inserts each layer's polyline corner by corner through the forced insert, which shoves
obstacles and pulls the new trace tight, and inserts a via at each layer change. Before
that, `AutorouteEngine.autorouteConnection` clears the rooms, checks the result, and rips
the connections the search chose to rip. This checklist ports both and proves
**connection parity** (rung 2): one connection routed on the loaded board gives the same
items as Java ([A00](../A00-autorouter-README.md)).

**Prerequisites:**
- A05 (located paths).
- O04: `RoutingBoard::insert_forced_trace_polyline`, `insert_forced_trace_segment`,
  `ForcedViaInserter::{check, insert}`. O01: the pull-tight that runs inside the forced
  insert (`PolylineTrace.pullTight`).
- B05: `connect_to_trace`, pin neck-down widths. B04: `pick_items` with a selection
  filter, `get_trace_tail`. B01: item ids burned where Java burns them (B01.2), and
  `clear_transient_autoroute_state` (B01.8).
- Decisions D1, D2, D8.

## What exists and what this adds

| Exists | State |
|---|---|
| `remove_items`, `remove_trace_tails`, `normalize_traces`, `get_connection_items(stop option)`, `get_trace_tail` (`board/basic_board.rs`, `board/trace_ops.rs`) | Present |
| O04's forced insert and via insert, O01's pull-tight, B05's `connect_to_trace` | Prerequisites |
| `path/FoundConnectionInserter.java` (838 lines): `getInstance` (`:40-63`), `insertSegments` (`:65-142`), `insertTrace` (`:158-484`), `insertFanoutMicroNeckdown` (`:486-554`), `insertNeckdown` (`:556-568`), `tryNeckDown` (`:570-712`), `insertVia` (`:714-816`) | Missing |
| `AutorouteEngine.autorouteConnection` (`:130-262`), `ripConnections` (`:264-290`) | Missing |
| `PlannedConnection.Segment` (corners and a layer; the only part of `PlannedConnection` in use) | Becomes A05's `ResultItem` |

What this adds: `autoroute/path/insert.rs`, `autoroute/connection.rs` with
`route_connection`, the `insert` probe, and an example binary for the determinism check.

## Decisions made here

- **No reference comparisons of points.** Java compares the forced insert's return value
  with `insertPolyline.lastCorner()` and `firstCorner()` by reference
  (`FoundConnectionInserter.java:235, 242, 249, 295`). O04's
  `insert_forced_trace_polyline` returns `ForcedInsert::{DatabaseDamaged, Unchanged, Reached,
  Partial(IntPoint)}`: Java's `null`, `firstCorner`, `lastCorner` and a new point. The
  inserter matches on the variant. `// PARITY:` at each site.
- **Partial insertion stays.** When a trace cannot be completed, Java breaks out, still
  removes trace stubs, restores `pin_edge_to_turn_dist`, and returns failure, leaving the
  inserted part and the ripped items' removal in place (`:433-483`). Rust does the same.
  The pipeline's strict DRC (A11) is what cleans up after it.
- **Clean-up before checks.** `route_connection` clears the session (rooms out of the
  tree in list order, item autoroute data cleared) before it looks at the search result
  (`AutorouteEngine.java:196-204`). That order changes the tree shape (D5), so it is kept.
- **The ripped set is the caller's.** `route_connection` takes `&mut BTreeSet<ItemId>`
  and `&mut Vec<(ItemId, i32)>` from the caller and fills them during backtracking
  (A05). A failure after backtracking (a disabled layer) leaves them filled. Fanout (A12)
  reuses one set across attempts, so a later rip includes items found by an earlier failed
  attempt. `// PARITY(quirk):`.
- **One entry point.** `route_connection(board, start: &BTreeSet<ItemId>, dest:
  &BTreeSet<ItemId>, ctrl, ripped, costs, sink) -> AttemptResult`, generic over A04's
  sink. Java's single-item `RoutingBoard.autoroute(Item...)` has no caller at `cda540f`
  and is not ported; the GUI calls `route_connection` (G series).

## File ownership

| Item | New files | May edit |
|---|---|---|
| A06.1–A06.4 | `autoroute/path/insert.rs` | `path/mod.rs` |
| A06.5 | `autoroute/connection.rs` | `autoroute/mod.rs` (append) |
| A06.6 | — | `autoroute/connection.rs` |
| A06.7 | `crates/freerouting/tests/autoroute_probe_insert.rs`, `fixtures/reference/probe/insert/` | `AutorouteProbe.java`, `fixtures.tsv` |
| A06.8 | `crates/freerouting/examples/route_connections.rs` | — |

## Checklist

- [x] **A06.1 — `insert_segments`.** Port `getInstance` and `insertSegments` (`:40-142`):
  for each `ResultItem`, a via from the current layer to the item's layer at its first
  corner, then the trace; then a via to the start layer at the last corner; then
  `connect_to_trace` at the first corner for a trace target and at the last corner for a
  trace start, with `trace_half_width[layer]` and the net's clearance class; then
  `normalize_traces(net)`. Any failed step returns `None`. Drop the unconditional trace
  logging (`if (true) FRLogger.trace(...)`).
  - **Accept:** covered by A06.7.
  - *Done 2026-10-16:* `autoroute/path/insert.rs`: `insert_located` (Java `getInstance`) and
    `insert_segments` (`insertSegments`): per `ResultItem` the via at its first corner, then the
    trace; the via to the start layer at the last corner; `connect_to_trace` for a trace target
    (first corner, the start layer's half width) and a trace start (last corner, the target
    layer's); then `normalize_traces(net)`. Any failed step is `Err(InsertError::Failed)`; the trace
    logging is dropped. Covered by A06.7: `insert_matches_java` equal on all 17 ladder boards (52
    sampled connections: 45 `ROUTED`, 5 `FAILED` "could not be inserted" with their partial copper
    compared, 1 "no connection was found") and `insert_routed_matches_java` on 7 routed boards (46
    connections).

- [x] **A06.2 — `insert_trace`.** Port `insertTrace` (`:158-484`):
  - a one-corner trace sets the first and last corner and succeeds;
  - save `pin_edge_to_turn_dist`, set it to -1, restore it at the end on every path;
  - with neck-down on, pick pins at the first and last corner with the PINS filter
    (`pick_items`, highest id first); a pin of the net whose centre equals the corner is
    taken, and a later match overwrites an earlier one;
  - the corner loop: forced-insert the polyline from `from_corner_no` to `i`; neck-down
    (A06.3) is tried on `Partial` or `Unchanged` when neck-down is on and the polyline has
    two corners (`:233-239`); if that did not insert, the micro-neck-down is tried on any
    result but `Reached`, **including** `DatabaseDamaged` (Java passes `null`), when fanout
    or neck-down is on (`:240-247`); on `Reached` or a successful neck-down, advance
    `from_corner_no`; on `Unchanged` with corners left,
    step `from_corner_no` back once when the polyline had fewer than three corners
    (`:295-310`); otherwise fail (`:433`);
  - remove the trace stubs at every corner but the last (`get_trace_tail`);
  - set the first and last corner.
  - **Accept:** covered by A06.7; a unit test that `pin_edge_to_turn_dist` is restored
    after a failed insert.
  - *Done 2026-10-16:* `Inserter::{insert_trace, insert_corners, end_pins}` in `path/insert.rs`:
    the one-corner case; `pin_edge_to_turn_dist` saved, set to -1 and restored on every path; the
    PINS pick at both ends (set order, a later match overwrites); the corner loop matching on
    `ForcedInsert` (`// PARITY:` at Java's `:233, :240, :248, :295`; the micro-neck-down also on
    `DatabaseDamaged`); the step back on `Unchanged` with corners left; the stub removal at every
    corner but the last. Found by the units: Java builds its `compare_trace_stub_found` trace
    string eagerly and asks each stub for its start and end contacts (`:441-458`); the port runs
    both queries, so the work meter counts the same (`Issue957-smoke-ldo-led` was 126 and 106 units
    short without them). Covered by A06.7; unit test
    `autoroute::connection::tests::pin_edge_to_turn_dist_is_restored_after_a_failed_insert` (a
    trace out of the board gives `Err(Failed)`, the value 55.0 is restored).

- [x] **A06.3 — Neck-down.** Port `insertNeckdown` (`:556-568`), `tryNeckDown`
  (`:570-712`; B05's `trace_neckdown_halfwidth`, `insert_forced_trace_segment`) and
  `insertFanoutMicroNeckdown` (`:486-554`). The `LinkedHashSet<Integer>` of candidate
  widths (`:493`) becomes a `Vec<i32>` with a contains check, keeping insertion order.
  Java's micro-neckdown tries 3/4, 3/5 and 1/2 of the class width with no lower bound,
  which gave a 0.12 mm track on `canary-pwr`. At `46bf9e2ef` the opt-in
  `router.min_trace_width_um` (C02) floors it, the pin neck-downs here and in A04, and
  `neck_width_um`. Port that floor with the same semantics; unset changes nothing.
  - **Accept:** covered by A06.7 on `Issue508-SMD-routing-issue-demo` and
    `Issue508-DAC2020_bm08`; a unit test of the width list order.
  - *Done 2026-10-16:* `insert_neckdown`, `try_neck_down` (with `full_width_run` and the two extra
    corners of `locate45::calculate_additional_corner`) and `insert_fanout_micro_neckdown` in
    `path/insert.rs`; the candidate widths are `micro_neckdown_half_widths` (a `Vec<i32>` with a
    contains check, first-insertion order). Covered by A06.7: on `Issue508-SMD-routing-issue-demo`
    4 of 10 inserted traces and on `Issue508-DAC2020_bm08` 5 of 15 have half width 750 against the
    class's 1000 (necked down), all equal to Java. Unit test
    `autoroute::path::insert::tests::micro_neckdown_widths_keep_first_insertion_order_without_repeats`.
    The `router.min_trace_width_um` floor of Java `46bf9e2ef` (not an ancestor of the pin; ledger
    "Port: yes") is ported with its semantics: `BoardRouterSettings::min_trace_width_um`,
    `AutorouteControl::{min_trace_half_width, neckdown_half_width}` (`(int) ceil(width / 2)` in
    board units), used by both pin neck-downs of the maze search (`session/search.rs`), by
    `try_neck_down` and the micro-neck-down's pin widths; the micro-neck-down skips candidates below
    it. Unset it is 0 and every probe above is unchanged. Test
    `autoroute::connection::tests::min_trace_width_floors_the_pin_neckdown_and_unset_changes_nothing`.
    The `neck_width_um` necking it also floors is A11's (`retryConnectionNecked`).

- [x] **A06.4 — `insert_via`.** Port `insertVia` (`:714-816`): no via when the layers are
  equal; otherwise the first via of the rule whose padstack spans both layers and passes
  `ForcedViaInserter::check`, then `ForcedViaInserter::insert` with the control's shove
  depths. Via masks honour `(attach off)` (D8, A01.3).
  - **Accept:** covered by A06.7 on `scoring-mixed-layer` and `Issue143-rpi_splitter`.
  - *Done 2026-10-16:* `Inserter::insert_via` in `path/insert.rs`: no via on equal layers; the
    layers sorted; the first via of `ctrl.via_rule` whose padstack spans both and passes
    `TraceShover::check_via` (O03's `ForcedViaInserter.check`, the control's shove depths and trace
    half widths), then `TraceShover::insert_forced_via` (O04). `(attach off)` reaches the check
    through `ViaInfo.attach_smd_allowed` as in Java. Covered by A06.7: `scoring-mixed-layer` (1 via)
    and `Issue143-rpi_splitter` (6 vias in 3 routed connections), also `min_fr_test` (3) and J2
    (10), every via equal (id, padstack, centre).

- [x] **A06.5 — `route_connection`.** Port `AutorouteEngine.autorouteConnection`
  (`:130-262`) and `ripConnections` (`:264-290`):
  - build the search (A02 init); `None` or an init error gives `Failed` with Java's detail
    text;
  - `find_connection`; if found, locate (A05), filling the caller's ripped set and costs;
  - clear the session, always, before any check;
  - no search result or no located path gives `Failed`; a disabled signal layer at either
    end gives `Failed` (the ripped set stays filled); `items == None` gives `Skipped`;
  - rip: for each ripped item, `get_connection_items(NONE if remove_unconnected_vias else
    FANOUT_VIA)` into a `BTreeSet`; record the item's nets in a `BTreeSet<i32>`;
    `remove_items(set)`; then `remove_trace_tails(net, option)` for each net in ascending
    order;
  - insert (A06.1); `None` gives `Failed` ("could not be inserted"), else `Routed`;
  - an angle restriction other than 45° gives `Failed` with "unsupported angle
    restriction" until A07.
  - **Accept:** covered by A06.7; unit tests for the disabled-layer and empty-items paths.
  - *Done 2026-10-16:* `autoroute/connection.rs`: `route_connection(board, start, dest, ctrl,
    RouteLimits { settings, stop, work_limit }, Ripped { items, costs }, sink) -> (AttemptResult,
    S)` (the sink is handed back), `finish_connection` (checks, rip and insert after the clear),
    `rip_connections`, `describe_connection`, Java's detail texts. The session is cleared before
    any check (`// PARITY:`), except after an init failure, where Java returns before `clear()`:
    the port leaks the rooms too (the decision taken for A06, `// PARITY(quirk):`, DIVERGENCES
    "Connection insertion"), and a later session's room with the same id moves the leaked one to a
    free negative key in place (`ShapeSearchTree::rekey_room_aside`). Java's `connectionItems ==
    null` (`SKIPPED`) cannot occur: the locator always returns a list, and an empty one is inserted
    as in Java. Covered by A06.7; the rip by `insert_routed_matches_java` (4 connections rip, e.g.
    item 100 at cost 1506009 removes `[96,97,98,100,102]`). Unit tests in
    `autoroute::connection::tests`: `a_path_ending_on_a_disabled_signal_layer_fails_and_keeps_the_ripped_set`,
    `an_active_layer_or_a_plane_layer_is_not_a_disabled_signal_layer`,
    `an_empty_located_path_on_one_layer_is_routed_without_new_items`,
    `an_empty_located_path_across_layers_fails_like_javas_null_pointer`,
    `no_search_result_and_no_located_path_fail_with_javas_details`,
    `an_init_failure_leaks_its_rooms_and_a_later_session_moves_them_aside`. A 90 degree or
    any-angle board gives `Failed` "unsupported angle restriction" up front (A07).

- [x] **A06.6 — Board lifecycle and the two-pin oddity.** Java's
  `RoutingBoard.initAutoroute` (`:882-898`), `finishAutoroute` (`:900-909`) and
  `clearAllItemTemporaryAutorouteData` (`:1275`) become session creation and drop: the
  session takes the net's autoroute tree from the board (`get_autoroute_tree(class)`) and
  owns its B02.8 side table, so dropping it clears the item data. No engine is stored on
  the board (retained mode is not ported, A00). B01.8's `clear_transient_autoroute_state`
  stays the restore-time reset. Then explain
  `scoring-perfect-two-pin.dsn`, which Java leaves unrouted (reason unverified): run the
  `locate` and `insert` probes on it, find the Java step that fails, and make Rust fail at
  the same step for the same reason.
  - **Accept:** the done note names the failing Java step; the `insert` probe matches on
    that board.
  - *Done 2026-10-16:* the lifecycle is in `autoroute::connection`'s module docs: one `Session`
    per connection takes the class's autoroute tree from the board and owns the items' autoroute
    data in its side table, dropped with it; nothing is stored on the board.
    `scoring-perfect-two-pin.dsn` has no via padstack, so its class `kicad_default` has no via rule.
    The failing Java step is the `AutorouteControl` constructor: `initNet` takes
    `currentNetClass.getViaRule()` = null (`AutorouteControl.java:217`) and `rebuildViaInfo`
    dereferences it at `viaRule.viaCount()` (`:241`), a `NullPointerException` that
    `AutorouteConnectionRouter.route`'s catch turns into `FAILED`; the search never starts. Rust
    fails at the same step for the same reason: `AutorouteControl::for_net` returns
    `ControlError::NoViaRule` (`java_exception()` = `NullPointerException`), which the batch caller
    (A11) maps to `Failed`. The `insert` probe matches on that board
    (`{"stage":"insert:1","net":1,"exception":"NullPointerException"}` on both sides), as do
    `locate` and `start-rooms`.

- [x] **A06.7 — `insert` probe (connection parity).** Java kind `insert`: for each
  sampled connection, on a **fresh load** of the board, build the control as the router
  does (pass 1: `ripupAllowed = true`, `ripupCosts = startRipupCosts`) and call
  `autorouteConnection`. Print the state; the ripped ids; then every item added (id, kind,
  net, layer, corners and half width for traces; padstack and centre for vias) and every
  item removed (id), in id order; `maxGeneratedId` after; and the `BoardStatistics`
  deltas (incomplete, vias, total trace length bits); and the `WorkMeter` units the
  connection used (at `30baad7e2`).
  - **Accept:** `insert_matches_java` equal for every sampled connection on the fixture
    ladder up to and including `Issue026-J2_reference`, units included. A connection
    whose units exceed its budget (a cut-off) is listed in the test with its reason until
    B02 proves the tree shape.
  - *Done 2026-10-16:* Java kinds `insert` (`@ladder`) and `insert-routed` (the 7 `maze-routed`
    boards, for the rip) appended to `AutorouteProbe.java`, rows in `fixtures.tsv` and the README;
    references from `generate.sh <a773a70ec worktree> insert|insert-routed` in
    `fixtures/reference/probe/insert/` (17 boards, 92 KiB) and `insert-routed/` (7 boards, 60 KiB).
    Each line also has the costs, the start and destination ids, the items' clearance class and
    fixed state, and items changed in place (same id). Rust: `tests/autoroute_probe_insert.rs`, the
    writers in `tests/support/insert_lines.rs`. `cargo test -p freerouting --test
    autoroute_probe_insert`: 2 passed. `insert_matches_java` is equal on all 17 ladder boards up to
    `Issue026-J2_reference` and the pre-routed boards after it, units included,
    `EXPECTED_MISMATCHES` empty; `insert_routed_matches_java` is equal on 7 boards,
    `ROUTED_EXPECTED_MISMATCHES` empty. No connection is cut off: the most units of one call is
    296,186 against the budget of 100,000 nominal ms (6.4e9 units).

- [x] **A06.8 — Determinism.** `examples/route_connections.rs <board.dsn> <out.jsonl>`
  writes the `insert` probe lines for every sampled connection.
  - **Accept:** `tools/determinism-check.sh` reports `identical` for it on every board of
    `tools/bench/sets/connection-parity.txt`; load averages in the done note.
  - *Done 2026-10-16:* `crates/freerouting/examples/route_connections.rs <board.dsn> <out.jsonl>`
    (the writers of `tests/support/insert_lines.rs`). `tools/determinism-check.sh --out <dir> --
    route_connections <dsn> {OUT}` (release build) printed `identical` on all 10 boards of
    `connection-parity.txt`. 1-minute load averages, idle run then loaded run: min_fr_test 7.59 /
    7.86, Issue368 7.86 / 8.44, Issue110 8.44 / 8.88, Issue143 8.88 / 9.05, Issue413 9.05 / 9.29,
    SMD demo 9.29 / 9.50, TestSensel 9.50 / 9.70, Issue269-NoViasOnPowerPlanes 9.70 / 9.89,
    Issue433 9.89 / 10.06, CONGFLOCK 10.06 / 10.21 (other sessions kept the machine busy even for
    the idle runs). The ladder boards' outputs equal Java's references below the header (checked on
    min_fr_test, Issue110 and the SMD demo).
