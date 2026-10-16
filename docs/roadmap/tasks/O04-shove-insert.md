# O04 — Shove insert and forced insertion

Opened 2026-10-15. Java's autorouter inserts every found connection through
`RoutingBoard.insertForcedTracePolyline` and `insertForcedTraceSegment`
(`FoundConnectionInserter.java:207,509,628,646,672,693`), and its vias
through `ForcedViaInserter.insert` (`:785`). Interactive routing, drag and
make-space use the same entry points. This checklist ports the insert half of
push-and-shove on top of O03's checks and O01's pull-tight, so path insertion
(A06) and interactive routing (I01) can start. It is the autoroute report's
AR-8a; AR-8b (the tightener) is O01. See [O00](O00-optimizer-README.md) for
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

- [ ] **O04.1 — Trace shove insert.**
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

- [ ] **O04.2 — Drill item moves and via shoving.**
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

- [ ] **O04.3 — Forced pads and vias.**
  - Java: `ForcedPadRouter.forcedPad` (`:346-470`),
    `ForcedViaInserter.insert` (`:249-362`).
  - Rust: `shove/insert.rs`.
  - `(attach off)` (D8): the via's `ViaInfo.attach_smd_allowed` flows into
    the pad check exactly as in Java; Rust adds no extra rule.
  - **Accept:** O04.7; `forced_via_respects_attach_off`: a forced via onto an
    own-net SMD pad with a via rule whose `ViaInfo` has attach off is
    refused, and the same call in the probe is refused in Java.

- [ ] **O04.4 — Forced trace insertion.**
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

- [ ] **O04.5 — Forced via and drill-item move.**
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

- [ ] **O04.6 — Port `Issue954CircularKeepoutInsertionTest`.**
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

- [ ] **O04.7 — Probe `shove-insert`.**
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

- [ ] **O04.8 — Determinism and speed.**
  - `examples/shove_insert_dump.rs <dsn> <ses> <out>` runs the O04.7 script
    and writes the final dump.
  - **Accept:** `tools/determinism-check.sh` reports `identical` on J2 and
    bm08; a release `#[ignore]` timing test records the J2 script time
    beside the Java probe's own timing in the done note.
