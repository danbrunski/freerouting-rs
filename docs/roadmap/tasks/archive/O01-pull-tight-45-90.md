# O01 — Pull-tight, 45° and 90°

Opened 2026-10-15. Pull-tight shortens traces without creating clearance
violations. Java runs it after every routed connection, in `removeTails`, in
fanout, at the end of every forced trace insertion, and after interactive
edits. Path insertion (A06) can't be ported without it, and the
`TODO(router)` in `RoutingBoard::clear_changed_area` waits for it. This
checklist ports the shared driver, the 45° and 90° variants, the trace-level
entry with Java's pin-exit recursion, and the board entry points without via
optimization (O05 adds that). See [O00](../O00-optimizer-README.md) for the
series rules.

**Prerequisites:**
- B01 (`burn_item_id`, transient state);
- B02 (B02.7 `WorkMeter`/`WorkLimit`; B02.4 `change_trace(id, polyline,
  LineReuse)` reusing tree leaves as Java's `changeEntries` does);
- B04 (`check_trace_shape`, `touching_pins_at_end_corners`, filtered
  `pick_items`);
- B05 (`swap_connection_to_pin`, `correct_connection_to_pin`);
- J01 (re-pin to `30baad7e2`), for O01.6's limit probe only.

## What exists and what this adds

| Exists (Rust at `70389b7`) | Where |
|---|---|
| Changed-area marking: `start_marking_changed_area`, `join_changed_area`, `mark_all_changed_area`, `changed_area`, `clear_changed_area` (with the `TODO(router)`) | `board/routing_board.rs:51-91` |
| `ChangedArea` per layer (`get_area`, `set_empty`, `join`) | `board/state/mod.rs:171-205` |
| Trace operations: `split_traces` `:72`, `split_trace` `:100`, `split_trace_at_point` `:371`, `combine_trace` `:395`, `normalize_trace` `:497`, `change_trace` `:544`, `combine_traces` `:580`, `normalize_traces` `:636` | `board/trace_ops.rs` |
| `overlapping_objects` (highest id first) `:1075`, `pick_items` (no filter yet) `:1116`, `insert_trace_without_cleaning` `:248`, `remove_items` `:872` | `board/basic_board.rs` |
| Geometry: `Line::{translate, signed_distance, side_of, intersection_approx, is_multiple_of_45_degree}`, `Polyline::{offset_shape, offset_shapes_range, corner_approx}`, `IntOctagon::enlarge` | `freerouting-geometry/src/{line.rs, polyline.rs, shapes/int_octagon.rs}` |
| `changed_area_lifecycle` test | `tests/java_ports.rs:665` |

| Java at `cda540f` (`src/main/java/app/freerouting/`) | Lines |
|---|---|
| `board/optimize/TraceTightener.java`: `getInstance` `:87`, `optChangedArea` `:121`, `pullTight(polyline,…)` `:175`, `isStopRequested` `:195`, `repositionLines` `:215`, `repositionLine` `:235`, `skipSegmentsOfLength0` `:338`, `smoothenEndCornersAtTrace(1,2)` `:402,414,494`, `splitTracesAtKeepPoint` `:476`, dead `avoidAcidTraps` `:517` | 547 |
| `board/optimize/TraceTightener45.java`: `pullTight` `:35`, `reduceCorners` `:52`, `smoothenCorners` `:227`, `smoothenSharpCorner` `:275`, `smoothenNonIntegerCorner` `:316`, `smoothenCorner` `:376`, `smoothenStart/EndCornerAtTrace` `:462,568` | 674 |
| `board/optimize/TraceTightener90.java`: `pullTight` `:26`, `trySkipSecondCorner` `:39`, `trySkipCorners` `:73` | 169 |
| `board/trace/PolylineTrace.java`: `pullTight(algo)` `:810-863`, `pullTight(ownNetOnly, accuracy, stop)` `:869-890` | — |
| `board/facade/RoutingBoardOperations.java`: `optChangedArea` `:52-79`, `removeItemsAndPullTight` `:81-120` | 121 |

What this adds: `freerouting::optimize` with the tightener, and
`RoutingBoard::{opt_changed_area, remove_items_and_pull_tight,
pull_tight_trace, pull_tight_own_net}`.

## Decisions made here

- **One `Tightener` struct, not three classes.** `AngleMode::{Diag45, Ortho,
  Any}` selects the variant's step functions. Per-trace scratch (`layer`,
  compensated half width, nets, clearance class, contact pins) lives in the
  struct, as in Java, so the step order and side effects stay identical.
- **The clip is an enum.** `Clip::{Unbounded, Skip, Octagon(IntOctagon)}`
  maps Java's `null`, the `IntOctagon.EMPTY` instance and any other octagon.
  `RoutingBoardOperations.optChangedArea` skips the tightener only for
  `Skip`. Verified at `cda540f`: `IntOctagon.offset` normalizes its result,
  and `normalize` returns the `EMPTY` instance for an empty octagon, so an
  empty tidy region enlarged by any width below 2^25 is still `Skip`.
- **"Unchanged" is an explicit outcome** (`Outcome::Unchanged |
  Changed(Polyline)`) for every step and for the fixpoint loops, which stop
  on Java's reference equality.
- **Line origins are tracked.** `PolylineTrace.change` compares lines by
  reference to decide how many tree leaves to keep, and returns without
  changing anything when one polyline is a same-object prefix and suffix of
  the other (`PolylineTrace.java:956-982`). B02.4's `change_trace` therefore
  takes `LineReuse { prefix, suffix }`. The tightener carries, for every
  line of its working polyline, `LineOrigin::{Kept(index_in_trace), New}`.
  Java keeps a `Line` object when it copies arrays (`System.arraycopy`) and
  creates one in `new Line`, `Line.getInstance`, `translate` and
  `opposite`. The `Polyline(Line[])` constructor drops parallel and
  overlapping lines and may flip a line with `opposite()`
  (`Polyline.java:78-100`); Rust gets that map from a new
  `Polyline::new_mapped(lines) -> (Polyline, Vec<Option<usize>>)` in the
  geometry crate (each output line's input index, `None` when flipped).
- **`avoidAcidTraps` stays dead.** It returns its input (`if (true) return
  polyline`, `TraceTightener.java:517-520`). Port it as the identity, with a
  comment.
- **`PullTightParams` has no trace costs yet.** O05 adds the field and the via
  branch. Until then, a via in the changed area is skipped, which is what
  Java does with `traceCosts == null`.
- **Graphics update box.** `joinGraphicsUpdateBox` is cosmetic. Join the
  board's dirty region if B01's journal provides one; otherwise drop it.

## File ownership

| Item | New files | May edit |
|---|---|---|
| O01.1 | `optimize/mod.rs`, `optimize/params.rs`, `optimize/tighten/mod.rs` | `lib.rs` (append `pub mod optimize;`), `freerouting-geometry/src/polyline.rs` (append `new_mapped`) |
| O01.2 | — | `optimize/tighten/mod.rs` |
| O01.3 | `optimize/tighten/diag45.rs` | `optimize/tighten/mod.rs` |
| O01.4 | `optimize/tighten/ortho.rs` | `optimize/tighten/mod.rs` |
| O01.5 | `optimize/pull_tight.rs` | `docs/DIVERGENCES.md` |
| O01.6 | — | `optimize/pull_tight.rs`, `optimize/params.rs`, `board/routing_board.rs` (`clear_changed_area` doc and TODO only) |
| O01.7 | `tools/java-reference/probes/board/PullTightProbe.java`, `tests/support/item_dump.rs`, `tests/reference_pull_tight.rs`, `fixtures/reference/probe/pulltight/*` | `tools/java-reference/README.md` |
| O01.8 | `examples/pull_tight_dump.rs` | — |

## Checklist

- [x] **O01.1 — Tightener types and dispatch.**
  - Java: `TraceTightener` constructor and `getInstance` (`:63-114`).
  - Rust: `optimize/params.rs` with `PullTightParams { only_nets:
    Vec<i32>, clip: Clip, accuracy: i32, keep_point: Option<(Point,
    i32)>, limit: Option<WorkLimit> }` and `Clip`. `optimize/tighten/mod.rs`
    with `Tightener::new(board, &params)`: `AngleMode` from
    `rules.trace_angle_restriction()`, `min_translate_dist =
    max(accuracy, 100)`. The limit starts counting at construction, as
    Java's `TimeLimit` starts in the constructor. A cancel token (the
    geometry crate's `Stoppable`) is checked first, then the limit.
  - Line origins: `LineOrigin`, a `TrackedPolyline { polyline, origins }`
    that every step consumes and returns, and `Polyline::new_mapped` (see
    the decisions). `new_mapped(lines).0` must equal `Polyline::new(lines)`.
  - **Accept:** unit tests for the angle dispatch, `min_translate_dist` at
    accuracy 0, 100 and 500, and `Clip` built from Java's three cases
    (`null`, `EMPTY`, `EMPTY.enlarge(250)`, which is `Skip`);
    `new_mapped_matches_new` on 1,000 seeded line arrays (equal polylines;
    maps checked by hand on cases with a dropped and a flipped line).
  *Done 2026-10-16:* `optimize/{mod.rs, params.rs, tighten/mod.rs}`. `PullTightParams` carries
  `limit: Option<i64>` (nominal ms, started in `Tightener::new` on the board's meter, as
  Java's `WorkLimit` starts in the constructor) and `stop` (the cancel token, checked first).
  Tests: `optimize::tighten::tests::angle_dispatch`, `clip_from_java_cases` (`null`,
  `EMPTY`, `EMPTY.enlarge(250)` = `Skip`), `line_reuse_counts_same_index_origins`;
  `reference_pull_tight::min_translate_dist_is_at_least_100` (accuracy 0, 100, 500 -> 100,
  100, 500); geometry `polyline::new_mapped_tests::new_mapped_matches_new` (1,000 seeded
  arrays from `JavaRandom(20261016)`, equal polylines, dropped and flipped lines seen) and
  `new_mapped_hand_cases` (a dropped parallel line, a turned middle line).

- [x] **O01.2 — Shared line steps.**
  - Java: `repositionLines` / `repositionLine` (`:215-331`),
    `skipSegmentsOfLength0` (`:338-398`).
  - Rust: functions in `optimize/tighten/mod.rs` taking `&mut Tightener`
    and the line slice, reusing scratch buffers.
  - Orders that decide results (exact):
    - `repositionLines` tries `i = 2 .. len-2` and stops at the first
      success, then runs `skipSegmentsOfLength0` on the result.
    - `repositionLine`: the clip test of the two corners (`isOutside`);
      `prevDist`/`nextDist` by `signed_distance`; the nearer corner by
      `abs(prev) < abs(next)` (ties pick `next`); the first try through the
      corner itself when it is an `IntPoint`; `checkLines[1].equals(translateLine)`
      returns no change; an overshoot shortens all three distances by
      `sign * 0.5` and `continue`s without halving; otherwise `deltaDist /= 2`
      after each check and the loop runs while `abs(deltaDist) >
      min_translate_dist`. The check is `check_trace_shape` of
      `offset_shape(current_half_width, 0)` of the 3-line polyline. On
      success it joins four corners into the changed area in Java's order.
    - `skipSegmentsOfLength0`: exact corner equality at `i == 1` and the
      last index, else `distance_square < 0.9`; no check for 45°-multiple
      lines; `--i` after a skip; returns the input object when nothing was
      skipped.
  - **Accept:** `PullTightProbe` mode `steps` (O01.7) applies each step
    alone to 200 seeded polylines per board on `Issue026-J2_reference` and
    `Issue508-DAC2020_bm08` with their routed SES. Rust output and
    changed/unchanged flags are equal for every input.
  *Done 2026-10-16:* `repositionLines`, `repositionLine` (with Java's in-place reversal of the
  middle check line replayed) and `skipSegmentsOfLength0` in `optimize/tighten/mod.rs`.
  Probe `pull-tight` sections `steps-45`/`steps-90` on J2 and DAC2020_bm08 (routed SES): 200
  seeded polylines per board and angle (own polyline, bump, jitter, zigzag, a line inserted
  through a corner; every third clipped), each step alone plus the fixpoint, with the changed
  flag, the result and the changed area: `reference_pull_tight::steps_45_match_java`,
  `steps_90_match_java` equal on every input. Java changed counts (J2 / bm08, 45°):
  repositionLines 36/40 of 200, skipSegmentsOfLength0 28/23.

- [x] **O01.3 — 45° variant.**
  - Java: `TraceTightener45` (all methods listed above).
  - Rust: `optimize/tighten/diag45.rs`.
  - Orders: the fixpoint `reduceCorners → smoothenCorners → repositionLines`
    until `Unchanged` or a stop; the stop check runs only when the last
    result was `Changed` (Java's `&&` short-circuit); clip tests use
    `isOutside`; start-corner smoothing before end-corner smoothing.
  - **Accept:** probe mode `steps` covers `reduceCorners`,
    `smoothenCorners` and both end smoothings, equal on every input; O01.7
    in 45° mode.
  *Done 2026-10-16:* `optimize/tighten/diag45.rs`; the fixpoint and stop check in
  `Tightener::pull_tight`. `steps_45_match_java`: reduceCorners 29/29 changed of 200 (J2 /
  bm08), smoothenCorners 32/43, pullTight 50/49, all equal; `ends_45_match_java`: both end
  smoothings of every trace, on the imported board (user-fixed contacts end them) and on the
  unfixed board with every trace split at its middle corner (`ends-45-split`, 3 smoothings
  on bm08), equal; `board_45_matches_java` (O01.7) equal.

- [x] **O01.4 — 90° variant.**
  - Java: `TraceTightener90` (`trySkipSecondCorner → trySkipCorners →
    repositionLines`; clip tests use `contains`; end smoothing returns null).
  - Rust: `optimize/tighten/ortho.rs`.
  - **Accept:** probe mode `steps` with the restriction forced to 90°;
    O01.7 in forced 90° mode.
  *Done 2026-10-16:* `optimize/tighten/ortho.rs`. `steps_90_match_java` (restriction forced
  to 90°): trySkipSecondCorner 11/13 changed (J2 / bm08), trySkipCorners 19/13, pullTight
  44/51, all equal; `board_90_matches_java` (O01.7) equal on all 41 boards.

- [x] **O01.5 — Trace-level pull-tight and end-corner smoothing.**
  - Java: `PolylineTrace.pullTight(algo)` (`:810-863`),
    `pullTight(ownNetOnly, accuracy, stop)` (`:869-890`),
    `smoothenEndCornersAtTrace(1,2)` (`:402-513`), `splitTracesAtKeepPoint`
    (`:476-490`).
  - Rust: `optimize/pull_tight.rs` with `RoutingBoard::pull_tight_trace(id,
    &mut Tightener) -> bool` and `pull_tight_own_net(id, accuracy) -> bool`
    (no limit, no cancel, as Java passes `-1` and `null`).
  - Orders and quirks to reproduce:
    - gates in Java's order: not on the board, shove-fixed, nets not normal,
      `only_nets` not equal, net class with pull-tight off;
    - the tightener gets `half_width` plus the clearance compensation and
      `touching_pins_at_end_corners()`; a `Changed` result goes through
      `change_trace` with the `LineReuse` counted from the line origins the
      way `PolylineTrace.change` counts (`:956-982`), and `pull_tight_trace`
      returns true even when `change_trace` returns early (Java does);
    - pin-exit branch, only if the angle isn't 90° and
      `pin_edge_to_turn_dist > 0`: `swap_connection_to_pin(start)`, then
      `(end)`, then `correct_connection_to_pin(start, angle)`, then `(end)`
      (B05). Each success re-runs `pull_tight_trace` on the same id and
      returns true. Headless Java never sets `pinEdgeToTurnDist > 0`
      (only the GUI does), so this branch matters for interactive work;
    - end smoothing: remove the trace, insert the smoothed polyline with
      the uncompensated half width, remove the old trace again (a no-op,
      kept), then for each net `split_traces` at the first and last corner,
      `normalize_traces` (an error is logged and ignored), and
      `split_at_keep_point`; `contact_pins` isn't restored on the early
      `return true` path;
    - `split_at_keep_point` picks traces with the TRACES filter, highest id
      first, and returns at the first successful split.
  - **Accept:** probe mode `pin-exit`: on `Issue026-J2_reference`,
    `Issue508-DAC2020_bm08` and `Issue143-rpi_splitter` with their routed
    SES and `pinEdgeToTurnDist` set to 4 × the board's minimum trace half
    width, `pull_tight_own_net` on every trace ending at a pin (descending
    id) gives the same dump and `maxGeneratedId`.
  *Done 2026-10-16:* `optimize/pull_tight.rs`: `pull_tight_trace`, `pull_tight_own_net`,
  `smoothen_end_corners_at_trace`, `split_at_keep_point`; kept quirks in the module docs and
  DIVERGENCES ("Pull-tight"). `reference_pull_tight::pin_exit_matches_java` on J2,
  DAC2020_bm08 and Issue143-rpi_splitter (routed SES, traces and vias unfixed because
  `SesReader` imports them user-fixed, `pinEdgeToTurnDist` = 4 × the minimum half width,
  every trace ending at a pin, highest id first): per-trace results, `maxGeneratedId` and
  the dump equal (3 traces changed on J2).

- [x] **O01.6 — Board entry points and work limits.**
  - Java: `TraceTightener.optChangedArea` (`:121-169`),
    `RoutingBoardOperations.optChangedArea` (`:52-79`) and
    `removeItemsAndPullTight` (`:81-120`), `RoutingBoard.optChangedArea`
    (`:151,171`).
  - Rust: `RoutingBoard::opt_changed_area(&PullTightParams)` and
    `remove_items_and_pull_tight(ids, tidy_width, accuracy) -> bool` in
    `optimize/pull_tight.rs`. Replace the `TODO(router)` on
    `clear_changed_area` with a doc line saying it drops the area without
    optimizing and that `opt_changed_area` is the Java path.
  - Orders (exact):
    - return if not marking; if the clip isn't `Skip`, run the tightener;
      then drop the changed area;
    - the outer loop repeats while something changed; layers ascending;
      skip an empty area; set it empty before the scan; enlarge by
      `1.5 * (clearance_matrix.max_value(layer) + 2 * rules.max_trace_half_width())`
      (integer sum, then one `f64` multiply);
    - scan a snapshot of `overlapping_objects` (highest id first); check
      stop or limit before each object; a trace that pull-tight changed
      marks a change and `break`s the scan only if `split_at_keep_point`
      split something; a trace it didn't change gets end smoothing, and a
      successful smoothing marks a change and always `break`s;
    - `remove_items_and_pull_tight`: the tidy region starts as `Skip` for
      a finite width (built by `union` of the removed shapes' bounding
      octagons when the width is positive) or `Unbounded` for
      `i32::MAX`; forbidden or user-fixed items make the result false;
      changed nets in ascending order get `combine_traces`.
  - Work limits: named constants in `optimize/params.rs`, one per Java site
    at `30baad7e2` (post-connection and `removeTails` guard 1000,
    `RoutingBoardOperations.PULL_TIGHT_LIMIT_NOMINAL_MS` 2000, Route 2000,
    MoveComponent 1000), each nominal ms × 64,000 units
    (`WorkLimit.UNITS_PER_NOMINAL_MILLISECOND`); exceeded when `used >
    budget`. The `limit` probe below waits for J01's re-pin, and this item
    is `[~]` until then.
  - **Accept:**
    - `changed_area_lifecycle` extended: after `opt_changed_area` the area
      is gone, and with `Clip::Skip` no trace changes;
    - `remove_items_and_pull_tight_matches_java`: probe mode `remove` on J2
      with its SES removes 10 seeded traces with tidy widths `i32::MAX`, 0
      and 2000; equal dumps;
    - after J01: probe mode `limit` runs `optChangedArea` on J2 + SES with
      a limit small enough to fire (pick it from the Java log); the Rust
      dump and the visit count at the cut-off are equal.
  *Done 2026-10-16:* `opt_changed_area` (returns an `OptChangedAreaRun`: stopped, stop checks,
  units at the last check) and `remove_items_and_pull_tight` in `optimize/pull_tight.rs`;
  `clear_changed_area`'s `TODO(router)` replaced by the doc line. Constants in
  `optimize/params.rs`: `ENDLESS_LOOP_GUARD_NOMINAL_MS` 1000, `REMOVE_ITEMS_PULL_TIGHT_NOMINAL_MS`
  2000, `ROUTE_PULL_TIGHT_NOMINAL_MS` 2000, `MOVE_COMPONENT_PULL_TIGHT_NOMINAL_MS` 1000,
  `budget_units` (× 64,000; exceeded when `used > budget`, B02.7's `WorkLimit`). Tests:
  `reference_pull_tight::changed_area_lifecycle_with_pull_tight` (in this file, because
  `tests/java_ports.rs` carries other sessions' uncommitted hunks: no marking does nothing,
  `Clip::Skip` drops the area without a stop check or a change, a full run drops it and
  changes traces); `remove_items_and_pull_tight_matches_java` (J2, 10 seeded traces, widths
  `i32::MAX`, 0, 2000: results and dumps equal); `work_limit_cuts_off_like_java` (pin
  `a773a70ec`; on DAC2020_bm08, because on J2 the whole run takes 50,652 units, less than the
  smallest limit of 1 nominal ms = 64,000: limit 1 ms fires after 161 stop checks at 64,251
  units in Java and Rust, dumps equal).

- [x] **O01.7 — Differential probe `pulltight` and Rust comparison.**
  Build the probe and the dump first: O01.2–O01.6 accept against its modes.
  - Java: `tools/java-reference/probes/board/PullTightProbe.java`, modes `board`
    (default), `steps`, `pin-exit`, `remove`, `limit`. `board` loads DSN +
    SES, calls `startMarkingChangedArea(); markAllChangedArea();
    optChangedArea(new int[0], null, 500, null, null, 0)` and dumps the
    board. Board set: the 40 smallest fixtures (by DSN size) with a
    `fixtures/reference/route/*.ses`, skipping `Issue676-ch32v-tx118s`
    (Java's SES reader fails on its session, B01.1), plus
    `Issue508-DAC2020_bm08` + `-routed.ses`, `Issue508-DAC2020_bm05` +
    `.ses` and `Issue026-J2_reference` + `.ses` where not already in; at
    most 43 boards. Run in 45° and with the restriction forced to 90°.
  - Rust: `tests/support/item_dump.rs` (the canonical dump, see O00) and
    `tests/reference_pull_tight.rs`.
  - **Accept:** dumps identical on every board in both modes, ids included;
    on J2 and bm08 the default tree's fingerprint (B02.1) after the call
    equals the Java tree hash from B02.2's walk, which proves the
    `LineReuse` counts.
    A board may be an expected mismatch only with a named cause found by
    bisecting to the first differing trace (for example a Java exception
    path), written next to the test, at most 2 of 43 per mode.
  *Done 2026-10-16:* `tools/java-reference/probes/board/PullTightProbe.java` (kind
  `pull-tight`, generated with `generate.sh <pin> pull-tight` at `a773a70ec` into
  `fixtures/reference/probe/pull-tight/`; the directory follows `generate.sh`'s kind naming,
  not `pulltight`), `tests/support/item_dump.rs` (geometry-first, then ids),
  `tests/reference_pull_tight.rs`. The board-level modes unfix the imported traces and vias
  first (`SesReader` makes them user-fixed; without it nothing changes). Board set: the 40
  smallest rows with a route session (minus Issue676-ch32v-tx118s) plus
  Issue508-DAC2020_bm05 = 41 boards (bm08 and J2 are among the 40; all use the route
  sessions of `fixtures.tsv`). `board_45_matches_java`, `board_90_matches_java`: dumps and ids
  identical on 41 of 41 boards in both modes, no expected mismatch; `board-45-tree` on J2 and
  bm08: the default tree's fingerprint equals Java's walk hash.

- [x] **O01.8 — Speed and determinism.**
  - Scratch buffers for line arrays and offset shapes; no per-check
    `BTreeSet`; reuse the tightener across traces of one call.
  - `examples/pull_tight_dump.rs <dsn> <ses> <out> [--angle 45|90|none]`
    writes the canonical dump (`none` errors until O02).
  - **Accept:**
    - `tools/determinism-check.sh` reports `identical` for the example on
      `Issue508-DAC2020_bm08` and `Issue026-J2_reference`;
    - a release-build `#[ignore]` timing test records Rust time for the
      `board` mode on bm08 and J2 next to the Java probe's own timing; the
      done note gives both (measured, not gated).
  *Done 2026-10-16:* `examples/pull_tight_dump.rs` (`--angle none` errors until O02).
  `tools/determinism-check.sh --out <dir> -- target/release/examples/pull_tight_dump
  fixtures/<b>.dsn fixtures/reference/route/<b>.ses {OUT}`: `identical` on
  Issue508-DAC2020_bm08 and Issue026-J2_reference (loaded runs at load 13.8 and 14.0).
  `reference_pull_tight::board_mode_timing` (`#[ignore]`, release, median of 5, machine at
  load 13-20 from other sessions): bm08 8.3 ms, J2 1.2 ms; Java probe's own `TIMING`
  (one cold run, JIT included): bm08 408 ms, J2 68 ms. The tightener is reused across the
  traces of a run and no stop check builds a `BTreeSet`; no further scratch buffers were
  added, because nothing measured hot.
