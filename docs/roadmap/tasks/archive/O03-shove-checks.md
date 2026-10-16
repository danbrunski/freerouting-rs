# O03 — Shove checks

Opened 2026-10-15. The maze search asks "could this trace or via go here if
the traces in the way were shoved aside?" while it expands rooms, even with
ripup off (`MazeSearchEngine.java:537-565`, `:696`;
`MazeExpansionEngine.java:384-421`). Those questions are the check half of
Java's push-and-shove stack. This checklist ports that half, so A04 (maze
search) can start. It merges the autoroute report's AR-2 and the board-gap
report's "shove support" checklist (entry sides, `ShapeTraceEntries`,
cutouts). See [O00](../O00-optimizer-README.md) for the series rules.

"Without item changes" is exact but not "without side effects": Java's checks
burn item ids for temporary substitute traces and set the board's
shove-failing obstacle and layer. Both are reproduced. The cutout half of
`ShapeTraceEntries` does change items; it is ported here because it belongs
to the same structure, and its first caller is O04.

**Prerequisites:**
- B01 (B01.2 `burn_item_id`; B01.6 the ledger's `note_changed`; B01.8
  `shove_failing_obstacle`/`shove_failing_layer` and
  `clear_shove_failing_obstacle` on `RoutingBoard`);
- B02 (B02.6 `reuse_entries_after_cutout` and `tree_shape_on_layer`; B02.7
  `WorkMeter`/`WorkLimit`; B02.8's change log; `getCompensatedHalfWidth`);
- B04 (`check_trace_segment`, `check_trace_shape`, filtered picks);
- J01 (re-pin to `30baad7e2`), for O03.7's limit probe only.

## What exists and what this adds

| Exists | Where |
|---|---|
| `overlapping_items_with_clearance` / `overlapping_tree_entries_with_clearance` (ordered by clearance, then leaf) | `board/search/mod.rs:252,277,303` |
| `overlapping_objects` `:1075`, `is_shove_fixed`, `is_obstacle`, `is_trace_obstacle`, `outline_blocks_nets` `:1300`, `contains_trace_tails` | `board/basic_board.rs`, `board/trace_ops.rs:758` |
| Geometry: `TileShape::{entrance_points, nearest_border_projections, nearest_relative_outside_locations, offset/enlarge, border_line}`, `Shape::cutout(&Polyline)` `shapes/mod.rs:450`, `IntOctagon/IntBox::nearest_border_projections` | `freerouting-geometry/src/shapes/` |
| Nothing of `ShapeEntrySide`, `ShapeTraceEntries`, `TraceShover`, `DrillItemMover`, `ForcedPadRouter`, `ForcedViaInserter` | — |

| Java at `cda540f` (`src/main/java/app/freerouting/`) | Lines |
|---|---|
| `board/model/structure/ShapeEntrySide.java`: constructors from (polyline, no, shape) `:26`, (point, shape) `:68`, (segment, shape, shoveToTheLeft) `:81`, (no, borderIntersection) `:157` | 161 |
| `board/model/structure/ShapeAndEntrySide.java`: constructor `:25`, `calcCutlineAtEnd/Start` `:80,102` | 120 |
| `board/searchtree/ShapeTraceEntries.java`: constructor `:47`, `cutoutTrace` `:67`, `fastCutoutTrace` `:111`, `storeItems` `:178`, `nextSubstituteTracePiece` `:230`, `stackDepth` `:289`, `substituteTraceCount` `:294`, `traceTailsInShape` `:302`, `cutoutTraces` `:310`, `getFoundObstacle` `:319`, `storeTrace` `:327`, `searchFromSide` `:448`, `resort` `:467`, `calculateStackLevels` `:579`, `popPiece` `:676`, `insertEntryPoint` `:732`, `rotateEntryListAroundAnchor` `:769` | 810 |
| `board/optimize/TraceShover.java`: static `check(board, LineSegment, …)` `:57`, `check` `:231-415`, `getIgnoreItemsAtTiePins` `:592`, `springOver` `:611-850`, `springOverObstacles` `:851-899` | 899 |
| `board/actions/DrillItemMover.java`: `check` `:34-105`, `tryShoveViaPoints` `:256-326` | 326 |
| `board/actions/ForcedPadRouter.java`: `calcCheckShapeForFromSide` `:42`, `inFrontOfPad` `:57` (typo `lineB.x + lineB.x` `:78`), `checkForcedPad` `:221-345`, `calcFromSide` `:471`, `CheckDrillResult` | 500 |
| `board/actions/ForcedViaInserter.java`: `checkLayer` `:30-130`, `check` `:131-248`, `holeCheckShape` `:363`, `calculateFromSide` `:377` | 462 |
| `board/facade/RoutingBoard.java`: `checkForcedTracePolyline` `:408-450`, shove-failing obstacle and layer `:1407-1426` | — |

## Decisions made here

- **`freerouting::shove`**, one file per concern, not per class (O00).
- **Entry lists are indexed `Vec`s.** Java keeps a circular linked list of
  `EntryPoint`s with an anchor. Rust stores them in a `Vec` with explicit
  `next` indices and rotates by index. The visiting order must be Java's,
  which the probe checks through the order of substitute pieces.
- **`CheckDrillResult`** is a Rust enum `{NotDrillable, Drillable,
  DrillableWithAttachSmd}`.
- **Cutout identity.** Java tests "nothing cut off" with
  `pieces[0] == traceLines` (`ShapeTraceEntries.java:88`), true only when the
  polyline has no entrance points and starts outside. Rust adds
  `TileShape::cutout_polyline_outcome` returning `Untouched | Pieces(Vec)`
  in the geometry crate, so a value-equal new piece isn't mistaken for
  "nothing cut".
- **The fast cutout** inserts its two pieces directly into the item list
  (`ShapeTraceEntries.java:111-150`): no `insertItem`, so no revision bump,
  no ledger note, no `max/min_trace_half_width` update; it fires the change
  hook for the old trace first (B02.8's change log records it) and relabels
  its tree leaves through B02.6's `reuse_entries_after_cutout`. Ids: start
  piece, then end piece. Java's ledger never hears of the pieces; B01.6
  counts exactly instead, so the fast cutout calls `note_changed` for both
  pieces (B00 records that divergence).
- **Shove-failing obstacle and layer** are B01.8's fields; this checklist
  sets them at Java's points.

## File ownership

| Item | New files | May edit |
|---|---|---|
| O03.1 | `shove/mod.rs`, `shove/entry_side.rs`, `tools/java-reference/probes/board/EntrySideProbe.java` | `lib.rs` (append `pub mod shove;`) |
| O03.2 | `shove/trace_entries.rs`, `tests/support/shove_sampler.rs`, `tools/java-reference/probes/board/TraceEntriesProbe.java` | — |
| O03.3 | `shove/cutout.rs` | `freerouting-geometry/src/shapes/tile_shape.rs` (append `cutout_polyline_outcome`), `shove/trace_entries.rs` |
| O03.4 | `shove/trace_check.rs` | — |
| O03.5 | `shove/spring.rs` | — |
| O03.6 | `shove/drill_check.rs` | — |
| O03.7 | `shove/via_check.rs`, `shove/forced.rs` | — |
| O03.8 | `tools/java-reference/probes/board/ShoveCheckProbe.java`, `tests/reference_shove_check.rs`, `examples/shove_check_dump.rs`, `fixtures/reference/probe/{entry-side,trace-entries,shove-check}/*` | `docs/DIVERGENCES.md`, `tools/java-reference/README.md` |

## Checklist

- [x] **O03.1 — Entry sides.**
  - Java: `ShapeEntrySide` (all four constructors), `ShapeAndEntrySide`
    (constructor, cut lines at start and end).
  - Rust: `shove/entry_side.rs`: `ShapeEntrySide { no: Option<usize>,
    border_intersection: Option<FloatPoint> }` with one constructor fn per
    Java constructor; `ShapeAndEntrySide { shape, from_side }`.
  - **Accept:** `EntrySideProbe`: 300 seeded inputs per constructor on
    `Issue026-J2_reference` + SES and `Issue413-test` as loaded; side numbers
    and intersection points (raw `f64` bits) equal.
  - *Done 2026-10-16:* `crates/freerouting/src/shove/entry_side.rs`
    (`ShapeEntrySide::{from_polyline, from_point, from_segment, from_values}`,
    `ShapeAndEntrySide::new`). Probe `entry-side`
    (`tools/java-reference/probes/board/EntrySideProbe.java`, run at `a773a70ec`)
    on 300 shove samples per board, each with the polyline, point, segment
    (left and right) and values constructors and `ShapeAndEntrySide` for the
    four (orthogonal, inShoveCheck) combinations:
    `cargo test -p freerouting --test board_probe_shove entry_sides_match_java`
    passes, all 3000 lines equal on `Issue026-J2_reference` (fixtures.tsv
    session) and on `Issue413-test` (as loaded), doubles by raw bits
    (`fixtures/reference/probe/entry-side/*.jsonl`; J2 regenerated after
    J01 rewrote its route session).

- [x] **O03.2 — `ShapeTraceEntries` storage and order.**
  - Java: constructor, `storeItems` (`:178-224`), `storeTrace`,
    `searchFromSide`, `resort` (`:467-520`, ordering by float
    `distanceSquare`), `calculateStackLevels` (`:579`),
    `insertEntryPoint`, `rotateEntryListAroundAnchor`, `getFoundObstacle`,
    `traceTailsInShape`, `shoveViaList`.
  - Rust: `shove/trace_entries.rs` with flat `Vec`s and scratch reuse.
  - Orders (exact): `storeItems` walks the obstacle set in its given order
    (clearance, then leaf order) and returns at the first non-shovable
    obstacle with that obstacle recorded; skip rules in Java's order
    (via obstacle areas unless a pad check, component obstacle areas,
    own-net or non-obstacle conduction areas, outlines that don't block the
    nets); `resort`'s float comparisons in Java's operation order.
  - The sampler (`tests/support/shove_sampler.rs`, mirrored in the probes)
    draws from `Random(20261015)` in a fixed order: a trace by index in the
    descending-id trace list, a point on it, a direction (8 for 45°, 4 for
    90°), a length in `[1, 20]` × the minimum trace half width; the shape is
    the segment's offset shape.
  - **Accept:** `TraceEntriesProbe`: 300 sampled shapes on each of
    `Issue026-J2_reference` (with its SES), `Issue413-test`,
    `Issue676-ch32v-tx118s` and `Issue957-smoke-ldo-led` (as loaded: their
    DSN files already hold wiring, and Java's SES reader fails on Issue676's
    session, B01.1): `storeItems`' result, found obstacle id,
    `traceTailsInShape`, `stackDepth`, `substituteTraceCount` and shove-via
    ids are equal.
  - *Done 2026-10-16:* `crates/freerouting/src/shove/trace_entries.rs`
    (`ShapeTraceEntries::{new, store_items, found_obstacle,
    trace_tails_in_shape, stack_depth, substitute_trace_count}`,
    `shove_via_list`; entry list as a `Vec` with `next` indices), sampler
    `crates/freerouting/tests/support/shove_sampler.rs` (mirrored by the
    `ShoveSampler` block of both probes). Probe `trace-entries`
    (`TraceEntriesProbe.java` at `a773a70ec`), stage `store`: 300 samples per
    board, obstacles from `overlappingItemsWithClearance` (Java's `TreeSet`
    order), from side left/right/`null` by `i % 3`, pad check `i % 5 == 4`,
    copper sharing `i % 7 != 6`, the trace's own nets for even `i` and the
    next net for odd `i` (so the sampled trace is foreign).
    `cargo test -p freerouting --test board_probe_shove trace_entries_match_java`
    passes: every line equal on `Issue026-J2_reference` (fixtures.tsv session;
    72 of 300 stores succeed, 4 with shove vias), `Issue413-test` (197, 64
    with stack depth > 0, 16 with vias), `Issue676-ch32v-tx118s` (100, 6 with
    vias) and `Issue957-smoke-ldo-led` (101). No sample put a trace tail in
    the shape (`traceTailsInShape` is `false` throughout).

- [x] **O03.3 — Substitute pieces and cutouts.**
  - Java: `nextSubstituteTracePiece` (`:230-286`; burns one id per piece,
    also on check paths, and recurses past empty pieces), `popPiece`,
    `cutoutTrace` (`:67-108`, slow path: `remove_item` then
    `insert_trace_without_cleaning` per piece), `fastCutoutTrace`
    (`:111-150`), `cutoutTraces` (`:310`).
  - Rust: `shove/trace_entries.rs` (pieces), `shove/cutout.rs`.
  - Offsets: with clearance compensation, one offset by the compensated half
    width + `c_offset_add`; without, two offsets (half width, then
    clearance + `c_offset_add`), in that order (Java: "for symmetry").
  - **Accept:**
    - `TraceEntriesProbe` also dumps every substitute piece's polyline and
      `maxGeneratedId` after draining the pieces; equal;
    - probe mode `cutout`: 100 sampled shapes per board cut out of the
      overlapping foreign traces (`cutoutTraces`); dumps equal, ids
      included, and the fast path's piece ids are start < end;
    - the same mode logs the default tree's walk hash (B02.2's code) after
      every 10 cutouts; Rust's B02.1 fingerprint matches each one. This is
      the Java-backed proof of B02.6's leaf reuse that B02.6 points to;
    - `fast_cutout_skips_insert_side_effects`: revision and trace
      half-width bounds unchanged by the pieces; the ledger's incomplete
      count equals a full recount after the cutout (B01.6).
  - *Done 2026-10-16:* `next_substitute_trace_piece` in
    `shove/trace_entries.rs` (burns one id per returned piece, skips empty
    ones), `BasicBoard::cutout_trace`, `ShapeTraceEntries::cutout_traces` and
    the fast path in `shove/cutout.rs`; `TileShape::cutout_polyline_outcome`
    appended to `freerouting-geometry/src/shapes/tile_shape.rs` (returns
    `Option<Vec<Polyline>>`, `None` for Java's identity case, because the
    geometry crate's `shapes/mod.rs` was not ours to re-export an enum).
    Evidence, all from `cargo test -p freerouting --test board_probe_shove`
    (3 tests pass):
    - pieces: the `store` stage dumps every piece (id, half width, nets,
      lines) and `maxGeneratedId` after draining; equal on all four boards
      (64 pieces on `Issue413-test`, none on the other three);
    - stage `cutout`: 100 samples per board cut out with `cutoutTraces` on a
      fresh load (nets: the next net, so the sampled trace is cut); obstacles,
      removed ids, new traces with ids and lines, and `maxGeneratedId` equal
      on `Issue413-test` (25 fast, 31 slow cuts), `Issue676-ch32v-tx118s`
      (61 fast, 303 slow) and `Issue957-smoke-ldo-led` (71 fast, 454 slow);
      on `Issue026-J2_reference` the first 37 samples are equal and Java
      throws a `NullPointerException` in `reuseEntriesAfterCutout` at sample
      37 (a zero-length piece without a tree leaf), listed in
      `JAVA_CUTOUT_EXCEPTIONS` and in DIVERGENCES ("Shove checks"); every
      fast cut asserts start id < end id;
    - the default tree's walk hash, node and leaf counts after every 10th
      cutout equal B02.1's `fingerprint()` (10 checkpoints on each of the
      three full boards, 3 on J2);
    - `fast_cutout_skips_insert_side_effects` (Issue413): revision moves by
      one per removed trace only, trace half-width bounds unchanged, ledger
      incomplete count equals `DesignRulesChecker::incomplete_count`.

- [x] **O03.4 — Trace shove check.**
  - Java: `TraceShover.check` (instance, `:231-415`, including the via
    branch through `DrillItemMover.tryShoveViaPoints`/`check`, the
    recursion on substitute pieces with `ShapeAndEntrySide`, and
    `springOver` for the spring-over depth), `getIgnoreItemsAtTiePins`
    (`:592`), static `check(board, LineSegment, shoveToTheLeft, …)`
    (`:57-225`, returns the OK length; `Integer.MAX_VALUE` as `f64` on full
    success; 0 for an empty or off-board shape).
  - Rust: `shove/trace_check.rs`: `TraceShover::check(&mut self, …) ->
    bool`, `check_segment_ok_length(…) -> f64`.
  - Set the shove-failing obstacle at each of Java's points (outline when
    off the board, `getFoundObstacle()` on a failed store, …).
  - **Accept:** O03.8.
  - *Partial 2026-10-16:* `crates/freerouting/src/shove/trace_check.rs`
    (`TraceShover::{new, check, check_segment_ok_length}`, `FULL_OK_LENGTH`,
    the tie-pin ignore set; the work limit checked at entry). First part of
    the O03.8 probe: `ShoveCheckProbe.java` stage `trace-check` (45 degree,
    300 samples per board: instance check with depths (20, 8, 5) and
    (0, 0, 0), static OK length shoving left and right, each with the
    shove-failing obstacle and layer and `maxGeneratedId`).
    `cargo test -p freerouting --test reference_shove_check` passes: all
    1501 lines equal on each of `Issue413-test`, `Issue676-ch32v-tx118s`,
    `Issue957-smoke-ldo-led` (as loaded), `Issue026-J2_reference` and
    `Issue508-DAC2020_bm08` (fixtures.tsv sessions, regenerated after J01
    rewrote `fixtures/reference/route/*.ses` at the pin). Coverage is thin outside
    Issue413: only there does the deep check differ from the flat one (209
    against 145 successes), and no OK length is partial (all 0 or
    `Integer.MAX_VALUE`). Remains for O03.8 (second dispatch): forced 90
    degree, `checkForcedTracePolyline` (O03.7), the via grid, the
    determinism check; the box stays open until O03.8's accept holds.
  - *Done 2026-10-16 (second dispatch):* O03.8's accept holds (see there): the
    `trace-check` and `trace-check-90` stages of the extended `shove-check` probe
    (deep and flat instance checks, OK lengths with via depth 8 and 0) are equal on all five
    boards in both 45 degree and forced 90 degree mode
    (`cargo test -p freerouting --test reference_shove_check`, 5 tests
    `shove_checks_match_java_*` pass; the first dispatch's `trace_checks_match_java` became
    these). Fixed on the way: `limit_exceeded` cloned the work meter, and a clone is another
    meter, so a set `WorkLimit` tripped `WorkLimit::units_done`'s debug assertion.

- [x] **O03.5 — Spring-over.**
  - Java: `springOver` (`:611-850`), `springOverObstacles` (`:851-899`;
    its own depth 20; both calls pass `true`, the second on the reversed
    polyline; the shorter `lengthApprox` wins, `<=` prefers the reversed
    one).
  - Rust: `shove/spring.rs`, returning `SpringOver::{NoObstacle, Wrapped(Polyline), Blocked}`
    for Java's `== polyline`, a new polyline and `null`.
  - **Accept:** the `springOverObstacles` assertions of
    `Issue954CircularKeepoutInsertionTest.insertsDetour` ported: 60 cases (3
    angle modes × 2 diameters × 2 directions × 5 width/offset profiles) on
    the DSN built from `fixtures/Issue954-circle.dsn` as the Java test does;
    the detour exists, has more than 2 corners, and every offset shape is
    clear by the same overlap query.
  - *Done 2026-10-16:* `crates/freerouting/src/shove/spring.rs`
    (`TraceShover::spring_over_obstacles`, `spring_over` with the recursion
    unrolled into a loop, `SpringOver::{NoObstacle, Wrapped, Blocked}`; the
    clockwise result wins on `<=`). `cargo test -p freerouting --test
    shove_spring_over` passes: `spring_over_obstacles_wraps_circular_keepout`
    runs the 60 cases of `insertsDetour` (angle modes none/45/90 x diameters
    5000/6000 x both directions x the 5 profiles) on the fixture with the
    diameter replaced and the vias removed; every detour is `Wrapped` with
    more than 2 corners, every offset shape is clear by
    `overlapping_items_with_clearance(shape, 0, [1], 1)`, and the 6000 keepout
    has more than one tree shape.

- [x] **O03.6 — Drill and pad checks.**
  - Java: `DrillItemMover.check` (`:34-105`; the ignore collection gets the
    drill item added), `tryShoveViaPoints` (`:256-326`; 90°: box border
    projections, 2 tries extended; octagon: 4 tries; otherwise
    `nearestRelativeOutsideLocations`; `+2` tolerance),
    `ForcedPadRouter.checkForcedPad` (`:221-345`),
    `calcFromSide`, `calcCheckShapeForFromSide`, `inFrontOfPad`.
  - Rust: `shove/drill_check.rs`.
  - Keep the `inFrontOfPad` typo (`lineB.x + lineB.x`, `:78`): DIVERGENCES
    line under "Java bugs kept on purpose", plus a test that fails if it is
    "fixed".
  - `checkForcedPad` returns `DrillableWithAttachSmd` when copper sharing
    is allowed and an obstacle is a pin; the caller decides whether
    attaching is allowed. That is how `(attach off)` (D8) reaches the
    check.
  - **Accept:** O03.8, plus `in_front_of_pad_keeps_java_typo`.
  - *Partial 2026-10-16:* `crates/freerouting/src/shove/drill_check.rs`
    (`TraceShover::{check_drill_item_move, try_shove_via_points,
    check_forced_pad}`, `in_front_of_pad`, `CheckDrillResult`).
    `in_front_of_pad_keeps_java_typo` passes
    (`cargo test -p freerouting --lib in_front_of_pad`); the typo is in the
    module docs and DIVERGENCES ("Java bugs kept on purpose"). The drill
    check runs inside the `shove-check` probe's trace checks (O03.4's
    evidence), but no probe line isolates it yet. Remains: `calcFromSide`
    and `calcCheckShapeForFromSide` (only `ForcedViaInserter` calls them,
    O03.7) and O03.8's via grid; second dispatch.
  - *Done 2026-10-16 (second dispatch):* `TraceShover::calc_from_side` and
    `calc_check_shape_for_from_side` in `shove/drill_check.rs` (first the clearance class,
    then class 0, `NOT_CALCULATED` if no side fits; a rational centre or a `null` offset
    shape skips the side: DIVERGENCES "Shove checks", test
    `calc_check_shape_skips_rational_centre`). `ForcedViaInserter.check` reaches them on
    every padstack layer, and `checkForcedPad` through every via query: the `via-grid`,
    `via-pins` and their `-90` stages are equal on all five boards (O03.8), including 155
    `DRILLABLE_WITH_ATTACH_SMD` answers of `checkLayer` with attaching allowed at own-net pins.
    `cargo test -p freerouting --lib calc_check_shape` and `... --lib in_front_of_pad` pass.

- [x] **O03.7 — Via checks, forced-trace check and work limits.**
  - Java: `ForcedViaInserter.checkLayer` (`:30-130`), `check`
    (`:131-248`), `holeCheckShape` (`:363`, circle of radius
    `ceil(drillRadius + holeClearance + 10)`), `calculateFromSide`;
    `RoutingBoard.checkForcedTracePolyline` (`:408-450`: offset shapes with
    the compensated half width; bounding box per shape in 90° mode;
    `ShapeEntrySide(polyline, i + 1, shape)`).
  - Rust: `shove/via_check.rs`; `shove/forced.rs` with
    `RoutingBoard::check_forced_trace_polyline`.
  - Work limits: check B02.7's `WorkLimit` at `TraceShover.check` entry and
    `DrillItemMover.check` entry, and pass it through `checkForcedPad`, as
    `30baad7e2` does (no other check points there). The `limit` probe waits
    for J01's re-pin; until then this item is `[~]`.
  - **Accept:** O03.8; after J01, probe mode `limit`: 50 sampled checks on
    J2 + SES with a limit that fires; Rust stops at the same visit count
    with the same result.
  - *Done 2026-10-16:* `crates/freerouting/src/shove/via_check.rs`
    (`TraceShover::check_via_layer` = `checkLayer`, `TraceShover::check_via` = `check`, setting
    the shove-failing layer, `hole_check_shape`, `calculate_from_side`, `ShoveDepths`) and
    `crates/freerouting/src/shove/forced.rs` (`RoutingBoard::check_forced_trace_polyline`).
    Work limits: checked at `TraceShover::check` and `check_drill_item_move` entry and passed
    through `check_forced_pad`, as at the pin `a773a70ec` (J01's pin, which already counts
    work, so the `limit` probe did not wait). Accept: O03.8 holds (below), and stage `limit`
    runs on every probe board, J2 with its session included: the first 50 samples with a
    shape get the deep check without a limit (result, units `u`), then a
    `WorkLimit(1 nominal ms)` padded with overlap queries so it fires a quarter, half or three
    quarters through the check. All 50 limited checks per board exceed the limit, and Rust
    stops at the same unit count (`u`) with the same result, shove-failing state and
    `maxGeneratedId` on all five boards (the limit changes the answer in 0 / 13 / 4 / 0 / 2 of
    the 50 on J2 / Issue413 / bm08 / Issue676 / Issue957).
    `cargo test -p freerouting --test reference_shove_check` passes.

- [x] **O03.8 — Probe `shove-check`, comparison and determinism.**
  - Java: `ShoveCheckProbe` on `Issue413-test`, `Issue676-ch32v-tx118s`,
    `Issue957-smoke-ldo-led` (these three as loaded, see O03.2),
    `Issue026-J2_reference` and `Issue508-DAC2020_bm08` (with their routed
    SES), 45° and forced 90°:
    - 300 sampled segments: `checkForcedTracePolyline` with depths
      (20, 8, 5) and (0, 0, 0); the static `TraceShover.check` OK length
      for both shove sides;
    - via locations on a 20 × 20 grid over the board: `ForcedViaInserter.check`
      and `checkLayer` per layer with the board's first via rule;
    - after each query: the boolean or `CheckDrillResult`, the
      shove-failing obstacle id and layer, and `maxGeneratedId`.
  - Rust: `tests/reference_shove_check.rs`;
    `examples/shove_check_dump.rs <dsn> <ses> <out>`.
  - **Accept:**
    - every query equal on every board (OK lengths by raw `f64` bits);
      a mismatch is fixed, never listed, because the maze search depends on
      each answer;
    - `tools/determinism-check.sh` reports `identical` for the example on
      J2 and bm08.
  - *Done 2026-10-16:* probe `ShoveCheckProbe.java` (at `a773a70ec`), stages in order
    `trace-check` (deep, flat, OK lengths left and right with via depth 8 and 0,
    `checkForcedTracePolyline` with (20, 8, 5) and (0, 0, 0)), `via-grid` (first via of the
    first via rule on the 20 x 20 grid: `ForcedViaInserter.check` with depths (20, 8), every
    second cell with trace pens, and `checkLayer` per padstack layer as the maze search calls
    it, in seven room kinds so every branch of `calculateFromSide` is taken), `via-pins` (the
    same at the first 100 pins with a net, plus `checkLayer` with attaching allowed), `limit`
    (O03.7), then `trace-check-90`, `via-grid-90`, `via-pins-90` after
    `setTraceAngleRestriction(NINETY_DEGREE)`. Rust side: `tests/support/shove_check_stages.rs`,
    run by `tests/reference_shove_check.rs` (one test per board) and
    `examples/shove_check_dump.rs <dsn> <ses|-> <out>`.
    - every query equal, by raw bits for OK lengths:
      `cargo test -p freerouting --test reference_shove_check` passes, 5 tests, all lines of
      `fixtures/reference/probe/shove-check/*.jsonl` equal (J2 9392, Issue413 8828, bm08 9188,
      Issue676 9020, Issue957 8876 lines). Coverage: per board 600 instance checks and 600
      forced-polyline checks per mode, 1200 OK lengths, 400 via checks and one `checkLayer`
      per cell and padstack layer per grid mode, 100 pins per pin stage; `checkLayer` answers
      all three results. Thin spots: no OK length is
      partial (all 0 or `Integer.MAX_VALUE`), and deep and flat checks differ only on
      Issue413.
    - `tools/determinism-check.sh --out <dir> -- target/release/examples/shove_check_dump
      fixtures/<board>.dsn fixtures/reference/route/<board>.ses {OUT}` prints `identical` for
      `Issue026-J2_reference` and `Issue508-DAC2020_bm08` (idle and beside 8 burners, load 28),
      and each output equals the Java reference below the header.
