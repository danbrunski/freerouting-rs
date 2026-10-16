# A04 — The maze search: one core, many sinks

Opened 2026-10-15. This is the heart of the router: `MazeSearchEngine` (1,280 Java
lines), `MazeExpansionEngine` (422), `MazeRipupResolver` (269) and `MazeTraceShover`
(357). It pops door sections in cost order, completes rooms, prices ripups, checks
shoves, and changes layers through drills. The checklist ends with the `maze-log`
probe: every pop equal to Java's, bit for bit. It also fixes one design requirement for
the whole engine: the search is **one core, generic over a sink**, and every user
(router, probes, diagnostics, statistics, the GUI's live view) steps that same core
([A00](../A00-autorouter-README.md) rules; KRT report §1.1 row 6; P01 and P02 rely on it).

**Prerequisites:**
- A03 (drills) and through it A01, A02.
- O03 (read-only shove checks): `TraceShover::check` (static form),
  `ForcedViaInserter::check_layer`, `ForcedPadRouter::check_forced_pad`,
  `RoutingBoard::check_forced_trace_polyline`.
- B04: `check_trace_segment`, `pick_items` with a selection filter.
- B05: `trace_neckdown_half_width`.
- B02.7 and B03.7: `WorkMeter`, `WorkLimit`, and the ticks in tree queries and in
  `complete_shape`. J01's pin (`30baad7e2`) for the work-limit semantics.
- Decisions D1, D2, D5, D6, D7.

## What exists and what this adds

| Exists | State |
|---|---|
| A02's session, rooms, doors, queue key; A03's pages and drills | Prerequisites |
| `JavaRandom` (`freerouting-geometry/src/jmath.rs`) | No `next_double` |
| `MazeSearchEngine.java`: `occupyNextElement` (`:326-400`), `expandToRoomDoors` (`:402-643`), `expandToTargetDoors` (`:645-721`), `expandToDoor` (`:723-777`), `doorIsSmall` (`:779-807`), `expandToDoorSection` (`:809-989`), `roomShapeIsThick` (`:1127-1150`), `shoveTraceRoom` (`:1152-1227`), `checkNeckDownAtDestPin` (`:1229-1243`), `segmentProjection` (`:185-216`), the fanout queue filter (`:91-137`) | Missing |
| `MazeExpansionEngine.java`: `expandToDrill` (`:31`), `expandToDrillPage` (`:118`), `expandToDrillsOfPage` (`:148`), `expandToOtherLayers` (`:244`), `checkLayerWithAnyMatchingVia` (`:384`) | Missing |
| `MazeRipupResolver.java`, `path/Connection.java`, `MazeTraceShover.java` | Missing |
| `gui/interactive/ExpandTestState.java`: steps `occupyNextElement` one at a time | The model for the GUI stepping API |

What this adds: the search, its sinks, `JavaRandom::next_double`, and the `maze-log`
probe.

## Decisions made here

- **One generic core.** `MazeSearch<'s, S: SearchSink = NoopSink, C: ExtraCost =
  NoExtraCost>` borrows the session. `step(&mut self) -> Step { Continue, Found, Exhausted,
  Stopped }` is Java's `occupyNextElement`, and `find_connection()` loops it. The batch
  router, the probes and the GUI all call `step()`; there is no second search.
- **The sink trait** (`session/sink.rs`) has default-empty, `#[inline]` methods:
  `on_pop(&PopInfo)`, `on_push(&Key, dropped: bool)`, `on_room_completed(RoomId)`,
  `on_door_expanded(DoorRef)`, `on_blocked(&Blocked)`, `on_finish(&SearchEnd)`. Every call
  site goes through a `sink!` macro, which a custom cfg `freerouting_no_search_hooks`
  compiles to nothing. That build exists only to measure the cost of the hooks. Sinks:
  - `NoopSink`: the default; must compile out.
  - `WorkCounters`: pops, pops skipped because the section was occupied, pushes,
    drop-on-equal skips, rooms completed, doors expanded, drills computed, and the
    WorkMeter units the search consumed (B02.7's meter delta, the D5 unit). P01.2 may
    append a statistics sink beside it.
  - `FailureRecord { cap }`: every door, room or layer change refused because of an
    item, with the blocking `ItemId`, the layer and the reason (not rippable, shove
    failed, forced-trace check failed, via check failed). At the end the records are sorted
    by `(blocker net, blocker id, layer, reason)`, deduplicated and cut to the first `cap`
    (default 64). Q03 and Q05 read it. It only reads state Java already computes, such as
    B01.8's shove-failing obstacle.
  - `ProgressEvents`: sends a `SearchProgress { pops, queue_len, rooms, layer, room_shape }`
    every N pops over a channel (D7). The cadence counts pops, never time.
  - `ProbeSink`: writes the `maze-log` lines.
  - `Tee<A, B>` to combine two.
- **A cost hook that is off in parity.** `ExtraCost::door_cost(layer, door_shape) -> f64`.
  With `NoExtraCost` the term is not added at all (adding `0.0` is not bit-safe for
  `-0.0`). Q05 and X03 implement it later; nothing implements it here.
- **The queue** stays a `BTreeSet` keyed by A02's key with first-wins insertion. P02 may
  swap it for a heap plus a set of queued keys; the pop order and the drop rule are fixed
  by the probe.
- **Stop and work checks** sit exactly where Java checks `isStopRequested`: every pop,
  including pops of occupied sections (`MazeSearchEngine.java:335`), and the four init
  points. At `30baad7e2`, `AutorouteEngine.isStopRequested` returns true when the
  connection's `WorkLimit` is exceeded (`units - start > budget`) or a full stop was
  requested. A user stop is the full stop only: Java's maze search ignores a router-only
  stop. The limit is created by the caller before the session (A11.1, A12.1), so it counts
  every tree node the thread pops from then on: room completion, drill computation, shove
  checks and tie-pin reduction included. The search never resets it.

## File ownership

| Item | New files | May edit |
|---|---|---|
| A04.1 | `autoroute/session/sink.rs`, `autoroute/session/search.rs` | `session/mod.rs`, `crates/freerouting/Cargo.toml` (append `[lints.rust] unexpected_cfgs = { level = "warn", check-cfg = ['cfg(freerouting_no_search_hooks)'] }`) |
| A04.2 | — | `session/queue.rs`, `crates/freerouting-geometry/src/jmath.rs` (append `next_double` and its test) |
| A04.3, A04.4 | — | `session/search.rs` |
| A04.5 | `autoroute/session/layers.rs` | — |
| A04.6 | `autoroute/session/ripup.rs` | — |
| A04.7 | `autoroute/session/shove_room.rs` | — |
| A04.8 | `crates/freerouting/tests/autoroute_probe_maze.rs`, `crates/freerouting/tests/autoroute_search_sinks.rs`, `fixtures/reference/probe/maze-log/` | `AutorouteProbe.java`, `fixtures.tsv` |

## Checklist

- [x] **A04.1 — The core and its sinks.** `MazeSearch` with `new(session, ctrl, sink,
  extra_cost)`, `step`, `find_connection -> Option<SearchResult { destination: (ExpandableRef,
  section) }>`, and read accessors the GUI needs (queue length, rooms, the current element).
  The sinks and the `sink!` macro from the decisions. `MazeSearch::new` reseeds a
  `JavaRandom` with `ctrl.ripup_costs` (`MazeSearchEngine.java:86-87`), one per search,
  `// PARITY:`.
  - **Accept:** `noop_sink_has_zero_size` (`size_of::<NoopSink>() == 0`); a unit test that
    `FailureRecord` output is sorted, deduplicated and capped the same way for two input
    orders; the crate builds with `RUSTFLAGS='--cfg freerouting_no_search_hooks'`.
  - *Done 2026-10-16:* `autoroute/session/search.rs`: `MazeSearch<'s, 'b, S: SearchSink = NoopSink,
    C: ExtraCost = NoExtraCost>` with `new(session, sink, extra_cost, settings)` (the control is the
    session's own `ctrl`; `SearchSettings` carries Java's `ctrl.settings` reads: fanout escape
    lengths, start ripup costs), `init` (Java `getInstance`), `step() -> Step {Continue, Found,
    Exhausted, Stopped}`, `find_connection() -> Option<SearchResult { destination }>` and the GUI
    accessors `session()`, `queue_len()`, `current()`, `destination()`, `steps()`, `pops()`,
    `outcome()`; `MazeSearch::new` seeds a `JavaRandom` with `ctrl.ripup_costs` (`// PARITY:`
    `MazeSearchEngine.java:86-87`). `autoroute/session/sink.rs`: the `SearchSink` trait (empty
    `#[inline]` defaults: `on_start`, `on_pop`, `on_push`, `on_room_completed`, `on_door_expanded`,
    `on_drill_page`, `on_blocked`, `on_step_end`, `on_finish`; every call through `sink!`),
    `NoopSink`, `WorkCounters`, `FailureRecord { cap }` (default 64), `ProgressEvents`, `ProbeSink`,
    `Tee`, `ExtraCost`/`NoExtraCost` (`ENABLED = false`: the term is not added). Tests
    `autoroute::session::sink::tests::noop_sink_has_zero_size`,
    `failure_record_is_sorted_deduplicated_and_capped_for_any_input_order` (7 records, two input
    orders, cap 5: same 5). `RUSTFLAGS='--cfg freerouting_no_search_hooks' cargo check -p
    freerouting`: builds without warnings; `crates/freerouting/Cargo.toml` has the `[lints.rust]`
    check-cfg line. `ExpandableRef` gained `Drill` and `DrillPage`; `Session` now borrows a
    `RoutingBoard` (the shove checks need one, as Java's engine holds a `RoutingBoard`).

- [x] **A04.2 — Queue filter and `next_double`.**
  - The fanout filter (`MazeSearchEngine.java:91-137`), applied on insert when
    `ctrl.is_fanout` and the start pin centre is set: on the start layer, an element whose
    entry midpoint is farther than `max_escape_length` from the pin centre is refused; a
    drill nearer than `min_escape_length` is refused. Lengths are `setting_mm * 1000.0 *
    resolution(UM)`. Defaults from settings are 4.5 mm and 2.5 mm
    (`DefaultSettings.java:186-187`); the 3 mm and 0.5 mm in the code apply only to null
    settings; a plane fanout uses 0. Refusals go to `sink.on_push(key, dropped = true)`.
  - `JavaRandom::next_double`: `((next(26) as i64) << 27) + next(27)` times `2^-53`, as
    `java.util.Random.nextDouble`.
  - **Accept:** `next_double_matches_java` against `JavaRandomProbe` values from
    `new Random(42)` (extend F02.1's probe output if needed); filter unit tests at the exact
    boundary distances.
  - *Done 2026-10-16:* `queue.rs`: `FanoutFilter::{new, with_lengths, admits}` (`(mm * 1000.0) *
    resolution(UM)`, a plane fanout's minimum 0) held by `MazeQueue`, applied by
    `Session::queue_element` to every element, the start doors of `init` included (A02's
    `init.rs` now queues through it); refusals reach `sink.on_push(key, dropped = true)`.
    `JavaRandom::next_double` existed (A01); test `jmath::tests::next_double_matches_java`: the bits
    of `new Random(42).nextDouble()` five times and `new Random(3000).nextDouble()`, from a Java
    probe at `a773a70ec` (`probe.sh`). Test `queue::tests::fanout_filter_boundaries`: entry at
    exactly 45000 (4.5 mm at 10 units/um) passes, `+1e-6` is refused on the start layer only; a drill
    at exactly 25000 passes, `-1e-6` is refused; a plane fanout admits a drill at the pin centre.

- [x] **A04.3 — `occupy_next_element` and `expand_to_room_doors`.**
  - Pop loop (`:326-400`): check stop and work on every pop; skip occupied sections; copy
    the backtrack door and section, `room_ripped`, `ripup_cost`, `adjustment` into the
    section; a drill page goes to `expand_to_drills_of_page` and returns without occupying;
    a destination target door ends the search; in a fanout, a drill reached from a drill
    ends it; with vias allowed, a drill not reached from a drill expands to other layers;
    then `expand_to_room_doors`; occupy unless it returned false.
  - `expand_to_room_doors` (`:402-643`): an inactive signal layer stops here; the small-door
    check with neck-down at a destination pin (`check_neckdown_at_dest_pin`, B05's pin
    width); `complete_neighbour_rooms`; neck-down at a start pin; room thickness
    (`min_width < 2 * half_width`, else `nearest_border_points_approx`); the split-plane
    conduction check for drills on inactive layers; target doors; obstacle rooms (A04.6,
    A04.7); then every other door from a **copy** of the door list (`:573`); then drill
    pages (free rooms) or the via's drill (via obstacle rooms, A03's cache).
  - **Accept:** covered by A04.8.
  - *Done 2026-10-16:* `search.rs::occupy_next_element` (stop check on every pop, occupied sections
    skipped, the section's state copied, drill pages, destination, fanout drill-to-drill end, other
    layers, the write of `occupied` through the popped slot even after a section reallocation,
    `// PARITY(quirk):`) and `expand_to_room_doors` (signal-layer stop, small door with neck-down at
    a destination pin, `complete_neighbour_rooms`, neck-down at a start pin, thickness, split-plane
    conduction check, target doors, obstacle rooms, a copy of the door list, drill pages or the
    via's drill). Covered by A04.8: `maze_log_matches_java` equal on all 17 ladder boards in both
    ripup variants.

- [x] **A04.4 — Door expansion and cost.** `expand_to_target_doors` (`:645-721`; thin rooms
  check the connecting line with `check_forced_trace_polyline`), `expand_to_door`
  (`:723-777`; thick rooms use `section_segments(half_width)`, which may reallocate,
  A02.5; thin rooms project the entry with `segment_projection`), `door_is_small`
  (`:779-807`), `room_shape_is_thick` (`:1127-1150`) and `expand_to_door_section`
  (`:809-989`).
  - Float order, `// PARITY:` (D6):
    - bend: `cross * cross > 0.01 * sq_len_prev * sq_len_next`, with both lengths `> 0.0`
      and the backtrack door's shape centre of gravity as the previous point (`:875-893`);
    - `expansion = from.expansion + add_costs as f64 + bend_penalty +
      entry_mid.weighted_distance(from_mid, h, v)`, summed left to right (`:895-902`);
    - `sorting = expansion + destination_distance(entry_mid, layer)` (`:903-904`);
    - `room_ripped = add_costs > 0 && adjustment == None || from.already_checked &&
      from.room_ripped`; `ripup_cost = add_costs` only when `add_costs > 0` and no
      adjustment.
  - **Accept:** covered by A04.8.
  - *Done 2026-10-16:* `search.rs::{expand_to_target_doors, expand_to_door, door_is_small,
    room_shape_is_thick, expand_to_door_section, check_neckdown_at_dest_pin, segment_projection}`
    with the bend test and the left-to-right sums (`// PARITY:` `:875-904`), `room_ripped` and
    `ripup_cost` as specified; `segment_projection` reproduces Java's identity tests by the
    branches of `FloatLine.segmentProjection{,2}` that return `toSegment`'s own end points.
    Covered by A04.8 (`maze_log_matches_java`, every key's bits equal).

- [x] **A04.5 — Layer changes.** Port `MazeExpansionEngine.java`:
  - `expand_to_drill` (`:31-116`): the drill shape shrunk by the via radius, the nearest
    point, cost `+ min_normal_via_cost` unless it came from a page;
  - `expand_to_drill_page` (`:118-146`): adds `weighted_distance` to the **sorting** value
    only;
  - `expand_to_drills_of_page` (`:148-242`): `page.drills(attach_smd_allowed)`, then each
    drill whose room on that layer is the element's next room and whose section is free;
  - `expand_to_other_layers` (`:244-382`): drillability up and down with
    `check_layer_with_any_matching_via` (`:384-421`, O03's `ForcedViaInserter::check_layer`
    for each via of the rule), via masks, SMD attach rules, then queue the drill on each
    target layer at `+ add_via_costs[from][to]` (always 0).
  - **Accept:** covered by A04.8 on the fixtures with vias.
  - *Done 2026-10-16:* `autoroute/session/layers.rs`: `expand_to_drill`, `expand_to_drill_page`,
    `expand_to_drills_of_page`, `expand_to_other_layers`, `check_layer_with_any_matching_via`
    (O03's `TraceShover::check_via_layer` per via of the rule). Covered by A04.8 on the boards with
    vias: J2's log has 584 `DrillPage` and 500 `ExpansionDrill` lines, `Issue508-SMD-routing-issue-
    demo` 852 and 112, `Issue269-min_fr_test` 134 and 184, `scoring-mixed-layer` 4 and 10, all
    equal.

- [x] **A04.6 — Ripup.** Port `MazeRipupResolver.java` and `path/Connection.java`:
  - `calc_fanout_via_ripup_cost_factor` (`:35-70`), `check_ripup` (`:72-198`):
    `ripup = ripup_costs as f64 * cost_factor`; `detour` from `Connection::get(item)`
    unless a fanout or a fanout via; the random factor `0.5 + r * r` from the search's
    `JavaRandom` **only** when `ripup_pass_no >= 4 && ripup_pass_no % 3 != 0`
    (`:158-162`, dead at the pin, `// PARITY(quirk):`); `ripup /= detour; ripup *=
    fanout_factor; result = max(ripup as i32, 1).min(i32::MAX / 100)`;
  - `check_leaving_ripped_item` (`:200-217`), `enter_through_small_door` (`:219-268`; its
    `overlapping_objects` set mixes rooms and items in `TreeObject` order);
  - `Connection::get` (`Connection.java:39-131`) cached in the side table as Java caches it
    in `ItemAutorouteInfo`; `detour()` (`:147-153`): `(len + 100) / (min + 100) + 0.1 *
    (n - 1)`, or `i32::MAX as f64` when an endpoint is missing.
  - **Accept:** a unit test that the random factor is never drawn at `ripup_pass_no = 1`
    and is drawn at 4; covered by A04.8 in the ripup variant.
  - *Done 2026-10-16:* `autoroute/session/ripup.rs`: `Connection { start_point, start_layer,
    end_point, end_layer, items }` with `trace_length`, `detour`, `ConnectionCache::get` (Java's
    `precalculatedConnection` per item), `calc_fanout_via_ripup_cost_factor`, `check_ripup`,
    `check_leaving_ripped_item`, `enter_through_small_door`. Test
    `ripup::tests::the_random_factor_is_drawn_only_from_pass_4_outside_every_third` (not at 1, 2,
    3, 6; drawn at 4, 5, 7). The ladder's ripup variant enters no obstacle room, so the new probe
    kind `maze-routed` (the boards' traces unfixed, nets cut one by one, the sampled connection
    searched through the others' routes) covers it: `maze_routed_matches_java` equal on 7 boards,
    with 134 obstacle-room steps and 26 ripup-cost steps on `Issue676-ch32v-tx118s`, 30 ripup steps
    on `Issue069-TestSensel/TestSensel`, 365 and 119 on `Issue229-display-8-digit-hc595`. Test
    `ripup::tests::a_closed_loop_of_traces_ends_the_connection_walk` pins the one divergence
    (DIVERGENCES.md, "Maze search").

- [x] **A04.7 — Shoving obstacle trace rooms.** Port `MazeTraceShover.checkShoveTraceLine`
  (`MazeTraceShover.java:32-318`: left and right, `check_trace_segment` from B04 and O03's
  static `TraceShover::check`, the door sections that become reachable,
  `end_points_matching`) and `shove_trace_room` (`MazeSearchEngine.java:1152-1227`):
  expand reachable sections with adjustment `Left` or `Right`. When the shove fails and the
  ripup cost is above 0, queue the element again, delayed, with `already_checked`.
  - **Accept:** covered by A04.8 on `Issue413-test`, `Issue676-ch32v-tx118s` and
    `Issue957-smoke-ldo-led`, which have pre-routed traces.
  - *Done 2026-10-16:* `autoroute/session/shove_room.rs`: `check_shove_trace_line` (with
    `end_points_matching`; B04's `check_trace_segment_line`, O03's static `check_segment_ok_length`)
    and `shove_trace_room` (the "left" list shoved with `shoveToTheLeft = false`, as Java), the
    delayed re-queue with `already_checked` in `search.rs::obstacle_room_entry`. Covered by A04.8:
    `maze_log_matches_java` on `Issue413-test` and `Issue957-smoke-ldo-led`, `maze_routed_matches_java`
    on `Issue413-test`, `Issue676-ch32v-tx118s`, `Issue957-smoke-ldo-led` and four more, all equal (an
    instrumented run counted 21 `shove_trace_room` calls with 112 expanded sections on the ladder
    and more on the routed boards). No board reaches a `Left`/`Right` adjustment (a dimension-2 link
    door) or the delayed re-queue; no log line has one.

- [~] **A04.8 — `maze-log` probe and the sink requirements.**
  - Java kind `maze-log`: for each sampled connection, step `occupyNextElement`. Before
    each call, walk `mazeExpansionList` from its first element and record the element the
    call will expand (the first whose section is unoccupied) and how many occupied ones it
    pops first. Print per step: step number, door kind and `getId()`, section, sorting and
    expansion bits, next room kind, id and layer, backtrack door id and section,
    `alreadyChecked`, adjustment, the occupied-pop count, whether the section ended
    occupied, and the thread's `WorkMeter` units since the search was created. At the
    end: destination door and section, room count, queue size. Cap `max_steps` = 200,000
    on both sides. The probe passes no `WorkLimit`, so no cut-off can fire; the units are
    compared so the D5 counting is checked step by step. A difference only in the units
    points to the tree shape (B02) or a counting point (B03.7), not to the search.
  - **Accept:**
    - `maze_log_matches_java`: every step equal for every sampled connection on the fixture
      ladder up to and including `Issue026-J2_reference`, in both ripup variants.
    - `noop_and_counter_sinks_agree`: on every sampled connection of
      `tools/bench/sets/connection-parity.txt`, the search with `NoopSink` and with
      `WorkCounters` reaches the same destination after the same pops; `maze-log` written
      through `Tee<ProbeSink, WorkCounters>` equals the plain one. (The same check on SES
      output is A13.6 and A14.4.)
    - `stepped_search_matches_find_connection`: on J2, `Issue508-DAC2020_bm08` and
      `scoring-mixed-layer`, a search stepped one `step()` at a time with `ProgressEvents`,
      reading the GUI accessors between steps as `ExpandTestState` does, gives the same
      pop count, destination and backtrack chain as `find_connection` with `NoopSink`.
      A05.5 adds the located corners.
    - Hook cost: an `#[ignore]` timing test runs the search on every sampled connection of
      `connection-parity.txt` ten times in release. Its time with `NoopSink` is within 2 %
      of the same test built with `--cfg freerouting_no_search_hooks`, on a quiet machine
      (load average and both times in the done note).
  - *Partial 2026-10-16:* Java kinds `maze-log` (fixtures `@ladder`) and `maze-routed` (7 boards)
    appended to `AutorouteProbe.java`, rows in `fixtures.tsv` and the README; references from
    `generate.sh <a773a70ec worktree> maze-log|maze-routed` (32 s and ~20 s; a second run
    byte-identical, `diff -r`) in `fixtures/reference/probe/maze-{log,routed}/` (`Issue143-rpi_splitter`
    and `Issue413-test` routed as digests). `tests/autoroute_probe_maze.rs::maze_log_matches_java`
    equal on all 17 ladder boards (to J2 and beyond), both variants, `EXPECTED_MISMATCHES` empty
    (J2: 2220 steps, 24 destinations); `maze_routed_matches_java` equal. Both write each log twice,
    by walking the queue between steps and through `Tee<ProbeSink, WorkCounters>`, and require
    equality. `tests/autoroute_search_sinks.rs::noop_and_counter_sinks_agree` (the 10 boards of
    `connection-parity.txt`, both variants: same outcome, steps, pops, destination and backtrack
    chain; the counters' pops equal the search's; the Tee log equals the plain one) and
    `stepped_search_matches_find_connection` (J2, DAC2020_bm08, `scoring-mixed-layer`; one `step()`
    at a time with `ProgressEvents` every 25 pops, accessors read between steps; equal pops,
    destination and chain; events = pops / 25) pass. Hook cost (`hook_cost_timing`, release, three
    runs each): `NoopSink` 1.0736 / 1.0697 / 1.0722 s, `--cfg freerouting_no_search_hooks` 1.0522 /
    1.0692 / 1.0474 s (116,680 pops), +1.5 % on the means, within 2 %, but with load average 6.95
    (concurrent agents). Remains: the hook-cost measurement on a quiet machine (owner P01, whose baselines run on a
    quiet machine; rerun
    `cargo test --release -p freerouting --test autoroute_search_sinks hook_cost_timing -- --ignored
    --nocapture` with and without the cfg).
