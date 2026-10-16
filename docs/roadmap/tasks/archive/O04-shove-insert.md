# O04 — Shove insert and forced insertion

Opened 2026-10-15. Java's autorouter inserts every found connection through
`RoutingBoard.insertForcedTracePolyline` and `insertForcedTraceSegment`
(`FoundConnectionInserter.java:207,509,628,646,672,693`), and its vias
through `ForcedViaInserter.insert` (`:785`). Interactive routing, drag and
make-space use the same entry points. This checklist ports the insert half of
push-and-shove on top of O03's checks and O01's pull-tight, so path insertion
(A06) and interactive routing (I01) can start. It is the autoroute report's
AR-8a; AR-8b (the tightener) is O01. See [O00](../O00-optimizer-README.md) for
the series rules.

**Prerequisites:** O01, O03; B05 (`connect_to_trace` is ported there; this
checklist doesn't need it but A06 and I01 call both); O02 for the any-angle
cases of O04.6 only.

## What exists and what this adds

| Exists | Where |
|---|---|
| Shove checks, entry sides, `ShapeTraceEntries` with substitute pieces and cutouts, `check_forced_trace_polyline` | O03: `shove/*` |
| Pull-tight: `Tightener`, `pull_tight_trace`, `split_at_keep_point`, `opt_changed_area` | O01: `optimize/*` |
| `insert_trace_without_cleaning` `:248`, `insert_via` `:399`, `remove_items` `:872`, `combine_trace` / `normalize_trace` / `get_trace_tail` (`trace_ops.rs:395,497,704`) | `board/basic_board.rs`, `board/trace_ops.rs` |
| `generate_snapshot` / `undo` (`basic_board.rs:1974-2005`) | for the probe's undo after a damaged insert |

| Java at `cda540f` (`src/main/java/app/freerouting/`) | Lines |
|---|---|
| `board/optimize/TraceShover.java` `insert` | `:417-590` |
| `board/actions/DrillItemMover.java` `insert` `:110-170`, `shoveVias` `:173-253` | 326 total |
| `board/actions/ForcedPadRouter.java` `forcedPad` | `:346-470` |
| `board/actions/ForcedViaInserter.java` `insert` | `:249-362` |
| `board/facade/RoutingBoard.java`: `moveDrillItem` `:252-295`, `forcedVia` `:312-355`, `insertForcedTraceSegment` `:361-402`, `insertForcedTracePolyline` `:456-880` | — |
| `src/test/java/app/freerouting/fixtures/Issue954CircularKeepoutInsertionTest.java` | 10 test methods, 75 cases |

## Decisions made here

- **One outcome enum for forced insertion.** Java returns `null` (database
  damaged; undo needed), the caller's `fromCorner` object (nothing
  inserted), the `toCorner` object (reached), or a new point (partial).
  Rust returns `ForcedInsert::{DatabaseDamaged, Unchanged, Reached,
  Partial(IntPoint)}`. `insertForcedTraceSegment`'s identity mapping
  (`okPoint == insertPolyline.firstCorner()`, `:394-401`) becomes a match on
  that enum. A degenerate polyline (null corner, `:472-480`) is
  `DatabaseDamaged`, as Java returns `null` there.
- **Substitute traces keep the id they burned.** A substitute piece takes its
  id when `nextSubstituteTracePiece` constructs it and is inserted later with
  `board.insertItem` (`TraceShover.java:569`). Rust inserts it with that
  id (B01's insert-with-id path), never a fresh one.
- **Debug code is dropped, its work isn't.** `insertForcedTracePolyline`
  builds trace strings and runs two debug-only `pickItems` queries
  (`dbgBefore`/`dbgAfter`). The strings go. The queries' tree visits stay:
  at `30baad7e2` the meter is per thread and counts every `MinAreaTree`
  visit, and `Route.connect` reuses one limit across several
  `insertForcedTraceSegment` calls, so one call's debug visits count
  against the next call's checks. Rust adds the same visits to the meter
  with a count-only traversal of the same two point queries.
- **Kept quirk: drag never pull-tightens with a finite tidy width.**
  `moveDrillItem` hands its `IntOctagon.EMPTY` tidy region to
  `DrillItemMover.insert`, which unions into its own parameter copy
  (`DrillItemMover.java:145`), so the caller's region stays `EMPTY`;
  `EMPTY.enlarge(w)` normalizes back to `EMPTY`; `optChangedArea` then skips
  pull-tight. Only `tidyWidth == Integer.MAX_VALUE` (no clip) pull-tightens
  after a drag. Kept, tested, listed in DIVERGENCES. (The board-gap report's
  H2 says the opposite; it is wrong.)
- **Normalize errors.** `insertForcedTracePolyline` and `TraceShover.insert`
  catch normalize exceptions and continue (`RoutingBoard.java:787-840`, `TraceShover.java:571-575`).
  Rust's normalize returns an error value at the same point and the caller
  continues the same way.

## File ownership

| Item | New files | May edit |
|---|---|---|
| O04.1 | `shove/insert.rs` | `shove/mod.rs` (re-exports) |
| O04.2 | — | `shove/insert.rs` |
| O04.3 | — | `shove/insert.rs` |
| O04.4 | — | `shove/forced.rs` (append) |
| O04.5 | — | `shove/forced.rs` (append), `docs/DIVERGENCES.md` |
| O04.6 | `tests/issue954_keepout_insertion.rs` | — |
| O04.7 | `tools/java-reference/probes/board/ShoveInsertProbe.java`, `tests/reference_shove_insert.rs`, `fixtures/reference/probe/shove-insert/*` | `tests/support/shove_sampler.rs` (append), `tools/java-reference/README.md` |
| O04.8 | `examples/shove_insert_dump.rs` | — |

## Checklist

- [x] **O04.1 — Trace shove insert.**
  - Java: `TraceShover.insert` (`:417-590`).
  - Rust: `shove/insert.rs`, `TraceShover::insert(…) -> bool`.
  - Orders (exact):
    1. empty shape → true; off the board → shove-failing obstacle = outline,
       false;
    2. `shove_vias` first (O04.2); false → false;
    3. store items with copper sharing; any remaining shove via → false with
       that via as the failing obstacle;
    4. no substitute pieces → true; depth 0 → false;
    5. `tails_exist_before = contains_trace_tails(obstacles, nets)`, then
       `cutout_traces(obstacles)`;
    6. for each substitute piece: skip if first corner == last corner;
       spring-over when depth > 0 (`Blocked` → false; `Wrapped` → depth − 1
       and replace the piece's polyline: the piece isn't on the board yet, so
       Java's `change` just swaps `lines`, `PolylineTrace.java:938-941`, and
       no `LineReuse` is needed); recurse into
       `insert` for each tile shape with `ShapeAndEntrySide(…, false)`;
       join corners into the changed area; insert the piece with its id;
       normalize within the changed area; if no tails existed before, remove
       new tails at both ends (`get_trace_tail`, connection items stopping at
       vias) and `combine_traces` per net.
  - **Accept:** O04.7.
  - *Done 2026-10-16:* `crates/freerouting/src/shove/insert.rs` (`TraceShover::insert`; the
    piece loop, shared with `forced_pad`, is `insert_substitute_pieces`). Orders as listed; the
    pieces keep the id they burned. One port detail: `ShapeTraceEntries::next_substitute_trace_piece`
    reads the obstacle traces from the board, and the cutout removes them (Java's entries keep
    the objects), so `drain_pieces` builds all pieces before the cutout, rewinds the id
    generator, and each piece takes its id where Java constructs it (after the cutout and the
    previous pieces' inserts). Without that, Issue270's step 22 lost a substitute piece and 9
    ids; with it the `shove-insert` probe (O04.7) is equal on all 10 boards, ids included (`cargo test -p freerouting --test reference_shove_insert`, 12 tests pass); 62 of the 470 segment inserts take more than 2 ids (shoves).

- [x] **O04.2 — Drill item moves and via shoving.**
  - Java: `DrillItemMover.insert` (`:110-170`), `shoveVias` (`:173-253`).
  - Rust: `shove/insert.rs`.
  - Orders: layers from first to last; per layer the tile shape is the
    translated tree shape's bounding box (90°) or bounding octagon; join the
    old bounding box's 4 corners into the changed area after each layer;
    `move_by` at the end (B05). `shoveVias` walks `shoveViaList` in store
    order, skips own-net vias, stops at depth 0, tries
    `tryShoveViaPoints(…, true)` candidates in order (the first always;
    later ones only within `maxDist²`), checks each with a fresh copy of
    the ignore list and no limit, and inserts the first that passes.
  - **Accept:** O04.7.
  - *Done 2026-10-16:* `TraceShover::insert_drill_item_move` (Java `DrillItemMover.insert`;
    its tidy-region argument only grows the callee's copy, so it is not ported, see O04.5) and
    `TraceShover::shove_vias` in `shove/insert.rs`; the move itself is B05's `move_item`. Stage
    `move` of the probe (10 `moveDrillItem` per board, 39 of 100 succeed) and the vias in the
    way of the segment and via stages: the `shove-insert` probe (O04.7) is equal on all 10 boards, ids included (`cargo test -p freerouting --test reference_shove_insert`, 12 tests pass).

- [x] **O04.3 — Forced pads and vias.**
  - Java: `ForcedPadRouter.forcedPad` (`:346-470`),
    `ForcedViaInserter.insert` (`:249-362`).
  - Rust: `shove/insert.rs`.
  - `(attach off)` (D8): the via's `ViaInfo.attach_smd_allowed` flows into
    the pad check exactly as in Java; Rust adds no extra rule.
  - **Accept:** O04.7; `forced_via_respects_attach_off`: a forced via onto an
    own-net SMD pad with a via rule whose `ViaInfo` has attach off is
    refused, and the same call in the probe is refused in Java.
  - *Done 2026-10-16:* `TraceShover::forced_pad` and `TraceShover::insert_forced_via` in
    `shove/insert.rs`; `ViaInfo.attach_smd_allowed` is the copper-sharing flag of the pad
    shoves and the via's attach flag, nothing more. Stage `via` (10 `forcedVia` per board, 37
    of 100 succeed) and stage `attach`: the `shove-insert` probe (O04.7) is equal on all 10 boards, ids included (`cargo test -p freerouting --test reference_shove_insert`, 12 tests pass). `forced_via_respects_attach_off` passes: on
    Issue742 (the probe's state after all stages) the via with attach off onto own-net SMD
    pin 461 is refused with the pin as the shove-failing obstacle, in Rust and in Java's
    output, and the same call with attach on succeeds; every probe board gives that pair.

- [x] **O04.4 — Forced trace insertion.**
  - Java: `insertForcedTracePolyline` (`:456-880`),
    `insertForcedTraceSegment` (`:361-402`).
  - Rust: `RoutingBoard::insert_forced_trace_polyline(…) -> ForcedInsert`
    and `insert_forced_trace_segment` in `shove/forced.rs`.
  - Orders and rules (exact):
    - clear the shove-failing obstacle; start marking the changed area;
    - combine with a picked trace only when exactly one trace is picked at
      `fromCorner` (TRACES filter) and it matches nets, half width and
      clearance class;
    - `springOverObstacles` with the compensated half width; `null` →
      `Unchanged`; fewer than 3 combined lines → `Unchanged`;
    - check (when `with_check`) then insert each offset shape from
      `startShapeNo`, with `ShapeEntrySide(combined, cornerCount −
      shapes − 1 + i, shape)`; a failed check sets `lastShapeNo`; a failed
      insert → `DatabaseDamaged`;
    - retry on failure: `sampleWidth = 2 * min_trace_half_width()`; give up
      (`Unchanged`) if the last segment is longer than `100 *
      sampleWidth`; shorten to `sampleWidth` when longer than it; check and
      insert the last shape again;
    - insert the trace, `combine`, then with a tidy region
      (`newCorner.surrounding_octagon().enlarge(tidy)` unless
      `tidy == i32::MAX`) build a tightener with `only_nets` = own nets
      when `max_recursion_depth <= 0` else all, keep point = `newCorner`,
      **no limit**; normalize within the layer's changed area; if it
      normalized, `split_at_keep_point` and re-pick the trace at
      `newCorner` (highest id); pull-tight it when `tidy > 0`.
  - **Accept:** O04.6 and O04.7.
  - *Done 2026-10-16:* `RoutingBoard::insert_forced_trace_polyline` and
    `insert_forced_trace_segment` in `shove/forced.rs`, returning `ForcedInsert`
    (`ForcedInsert::point` gives Java's result point), the arguments grouped as `ForcedTrace`
    and `ForcedTraceParams`, plus the work limit. Java's two debug `pickItems` run as queries
    without result. At the pin the normalization no longer throws (it returns false at its
    depth limit), so no error value is needed; the Java exceptions left are `DatabaseDamaged`
    (DIVERGENCES "Shove insert"). O04.7: the `shove-insert` probe (O04.7) is equal on all 10 boards, ids included (`cargo test -p freerouting --test reference_shove_insert`, 12 tests pass); of 470 segment inserts 203 reach the end,
    262 are unchanged, 5 partial (the shortening retry), none damaged. O04.6: the 44 cases that
    don't need O02 pass, and the insertion half of the 31 `NONE` cleanup cases passes too
    (`cargo test -p freerouting --test issue954_keepout_insertion -- --ignored`: 6 pass); their
    pull-tight is O02's.

- [x] **O04.5 — Forced via and drill-item move.**
  - Java: `forcedVia` (`:312-355`), `moveDrillItem` (`:252-295`).
  - Rust: `RoutingBoard::forced_via(…) -> bool`, `move_drill_item(…) ->
    bool` in `shove/forced.rs`. Both take the pull-tight limit in nominal ms
    as a parameter (Java's `pullTightTimeLimit`, a WorkLimit at
    `30baad7e2`).
  - `move_drill_item`: unfix shove-fixed normal contacts first (this stays
    even if the move fails; callers undo); the tidy-region quirk above;
    own nets only when `max_recursion_depth <= 0`.
  - `forced_via`: on success, the clip is
    `location.surrounding_octagon().enlarge(tidy)` (with `tidy == 0` that is
    the point octagon, not `Skip`), or `Unbounded` for `i32::MAX`.
  - **Accept:** O04.7; `move_drill_item_finite_tidy_skips_pull_tight`
    (the quirk, pinned, with a DIVERGENCES line).
  - *Done 2026-10-16:* `RoutingBoard::forced_via` and `RoutingBoard::move_drill_item` in
    `shove/forced.rs`, both taking the pull-tight limit in nominal ms (none if not positive).
    Stages `via` and `move`: the `shove-insert` probe (O04.7) is equal on all 10 boards, ids included (`cargo test -p freerouting --test reference_shove_insert`, 12 tests pass). `move_drill_item_finite_tidy_skips_pull_tight` passes:
    on Issue742 with its session (traces and vias unfixed) every successful move with tidy
    width 1000 equals the same move without optimization, and some moves with `i32::MAX`
    differ from it; `Clip::from_tidy_region(IntOctagon::EMPTY.enlarge(1000))` is `Skip`. Module
    docs (`shove::insert`, `move_drill_item`) and DIVERGENCES ("Shove insert", "Java bugs kept
    on purpose").

- [x] **O04.6 — Port `Issue954CircularKeepoutInsertionTest`.**
  - All 10 methods: `insertsDetour` (60 cases), `invalidCircularKeepoutPlacementStillFailsDrc`,
    `endpointInsideKeepoutDoesNotInsertCopper`,
    `nonConvexKeepoutIsNotReplacedWithOneOfItsSections`,
    `holedKeepoutIsNotReplacedWithItsOuterBoundary`,
    `obliqueCircularDetourSurvivesCleanup` (3),
    `asymmetricPolygonDetourSurvivesCleanup` (3),
    `partitionedRectangleSupportsDetour`,
    `successiveCircularObstaclesRemainClearAfterCleanup`,
    `circularCopperAreaRespectsNetAndObstacleFlag` (3). The board is built
    from `fixtures/Issue954-circle.dsn` with Java's text replacements, angle
    restriction `NONE`, vias removed.
  - The cleanup step calls `pull_tight_own_net(…, 100)` per trace. Cases in
    angle mode `NONE` need O02; until it lands they are
    `#[ignore = "needs O02"]` and this item is `[~]`.
  - **Accept:** all 75 cases pass (or the 45°/90° ones, with the remainder
    named).
  - *Partial 2026-10-16:* `crates/freerouting/tests/issue954_keepout_insertion.rs`, all 10
    methods ported. `cargo test -p freerouting --test issue954_keepout_insertion`: 6 tests
    pass, 44 cases (`insertsDetour` 45° and 90°, 40; `invalidCircularKeepoutPlacementStillFailsDrc`;
    `endpointInsideKeepoutDoesNotInsertCopper`; `nonConvexKeepoutIsNotReplacedWithOneOfItsSections`;
    `holedKeepoutIsNotReplacedWithItsOuterBoundary`). Remainder, owner O02: the 31 cases whose
    cleanup pull-tightens in angle mode `NONE` (`insertsDetour` `NONE` 20, oblique 3,
    asymmetric 3, partitioned rectangle 1, successive circles 1, circular copper 3; Java's
    `loadBoard` sets `NONE`), `#[ignore = "needs O02"]`. They pass with `--ignored` (6 tests)
    against today's no-op any-angle tightener, so un-ignoring them once O02 lands is the check
    that counts.
  *Done 2026-10-16 (O02):* the `#[ignore = "needs O02"]` attributes removed (6 tests) with the
  any-angle tightener in place; `cargo test -p freerouting --test issue954_keepout_insertion`:
  12 passed, 0 ignored, all 75 cases.

- [x] **O04.7 — Probe `shove-insert`.**
  - Java: `ShoveInsertProbe` on `Issue026-J2_reference`, `Issue413-test`,
    `Issue676-ch32v-tx118s`, `Issue957-smoke-ldo-led`,
    `Issue508-DAC2020_bm05`, `Issue508-DAC2020_bm08`,
    `Issue143-rpi_splitter`, `Issue508-SMD-routing-issue-demo`,
    `Issue110-testProjectFromFreeroutingBugTest` and one of the 20 smallest
    routed boards with vias (each with its routed SES, except Issue413,
    Issue676 and Issue957, which are used as loaded, see O03.2), 45°.
  - Script per board (sampler appended to `tests/support/shove_sampler.rs`):
    30 `insertForcedTraceSegment` calls (200 on J2) from a sampled pin or
    trace end of a sampled net to a sampled point, depths (20, 8, 5), tidy
    width `Integer.MAX_VALUE` for even steps and 0 for odd ones, accuracy
    500, `withCheck = true`, no limit; then 10 `forcedVia` calls at sampled
    points with the net class's first via. A snapshot is taken before each
    step and undone after `DatabaseDamaged`, as `Route` does.
  - Dump after each step: the outcome, the shove-failing obstacle and
    `maxGeneratedId`; the full canonical dump after every 10th step and at
    the end.
  - Rust: `tests/reference_shove_insert.rs`; on a mismatch it prints the
    first differing step.
  - **Accept:** all 10 boards identical at every step, ids included; a
    board may be listed only with its first differing step bisected and the
    cause named, at most 1 of 10.
  - *Done 2026-10-16:* `tools/java-reference/probes/board/ShoveInsertProbe.java` (at
    `a773a70ec`; one `Random(20261016)`; the start sampler `insert_starts`/`draw_start` appended
    to `tests/support/shove_sampler.rs`; the stages in `tests/support/shove_insert_stages.rs`),
    run with `generate.sh <pin> shove-insert`. Besides the script it has stage `move` (10
    `moveDrillItem`) and `attach` (O04.3). The tenth board is `Issue742-tastexx-pcb` (6 vias),
    because `Issue270-non-ansi_bracket`, the smallest routed board with vias, gives output
    identical to `Issue143-rpi_splitter`'s. `cargo test -p freerouting --test
    reference_shove_insert`: every line of `fixtures/reference/probe/shove-insert/*.jsonl` is
    equal on all 10 boards (J2 3492 lines for 200 steps; bm05 1787; the others 292-554), ids and
    dumps included; no board listed. Steps that shove (more than 2 new ids): J2 21 of 200,
    Issue413 8, SMD demo 9, Issue110 6, bm08 5, Issue742 4, the others 2-3 of 30; no step is `damaged`.

- [x] **O04.8 — Determinism and speed.**
  - `examples/shove_insert_dump.rs <dsn> <ses> <out>` runs the O04.7 script
    and writes the final dump.
  - **Accept:** `tools/determinism-check.sh` reports `identical` on J2 and
    bm08; a release `#[ignore]` timing test records the J2 script time
    beside the Java probe's own timing in the done note.
  - *Done 2026-10-16:* `crates/freerouting/examples/shove_insert_dump.rs <dsn> <ses|-> <out>`.
    `tools/determinism-check.sh --out <dir> -- target/.../examples/shove_insert_dump
    fixtures/<board>.dsn fixtures/reference/route/<board>.ses {OUT}` prints `identical` for
    `Issue026-J2_reference` and `Issue508-DAC2020_bm08`, and both outputs equal the Java
    reference below the header. Timing (`shove_insert_j2_timing`, `#[ignore]`, release, the
    script only, dumps included): Rust 90.9, 88.7, 89.4, 91.1, 92.0 ms; Java's probe, same
    script, its own stderr timing in three fresh JVMs: 1176.8, 767.1, 535.9 ms.
