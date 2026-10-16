# O02 — Pull-tight, any angle

Opened 2026-10-15. The any-angle tightener is the third variant of Java's
pull-tight. No fixture routes with it (all routed fixtures use the 45° tree),
so it is off the router's critical path. It is still needed: Java selects it
whenever a board's angle restriction is `NONE`, the interactive editor offers
that mode, and `Issue954CircularKeepoutInsertionTest` runs its insertion and
cleanup cases in all three angle modes (O04.6). It is the numerically
riskiest variant: float corners, cosine thresholds and rounding decide the
result. See [O00](../O00-optimizer-README.md) for the series rules.

**Prerequisites:** O01 (driver, shared steps, probe and dump format).

## What exists and what this adds

| Exists | Where |
|---|---|
| Tightener driver, `AngleMode::Any` arm unimplemented, shared steps, `PullTightProbe`, canonical dump | O01: `optimize/tighten/mod.rs`, `tests/reference_pull_tight.rs` |
| `Line::cos_angle` `:373`, `Direction::middle_approx` `:330`, `Polyline::length_approx` `:262`, `Polyline::distance` `:596` | `freerouting-geometry/src/{line.rs, direction.rs, polyline.rs}` |
| Java `board/optimize/TraceTightenerAnyAngle.java` (1004 lines): `pullTight` `:35-58`, `reduceCorners` `:61`, `smoothenCorners` `:222`, `repositionLines` override `:251`, `reduceLines` `:280`, `smoothenCorner` `:396`, `repositionLine` override `:497`, `skipLines` `:659`, `smoothenStartCornerAtTrace` `:758`, `smoothenEndCornerAtTrace` `:880`, `SKIP_LENGTH = 10.0` `:22` | cda540f |
| Java constants in `TraceTightener.java`: `c_max_cos_angle = 0.999` `:33`, `c_min_corner_dist_square = 0.9` `:36` | cda540f |

What this adds: `optimize/tighten/any_angle.rs` and the `Any` arm of the
dispatch.

## Decisions made here

- **No new fixtures.** The probe forces `AngleRestriction.NONE` on the O01
  board set (routed 45° traces are valid input; the any-angle tightener
  rewrites them into non-45° lines). The A series owns any synthetic
  any-angle routing fixtures (A07).
- **Keep Java's float operation order (D6).** `cosAngle`, `middleApprox`,
  `intersectionApprox`, `round()` and the `lengthApprox() + 1.5` comparison
  (`:162-163`) keep Java's order and types. No reassociation, no FMA.
- **Budget-bound termination.** Java's own comment (`:43-50`) says
  `reduceCorners` can loop with `smoothenCorners` when corners are closer
  than a grid point. Rust relies on the same stop checks as Java, so a
  looping case ends at the same budget point, never earlier.

## File ownership

| Item | New files | May edit |
|---|---|---|
| O02.1 | `optimize/tighten/any_angle.rs` | `optimize/tighten/mod.rs` (the `Any` arm only) |
| O02.2 | — | `optimize/tighten/any_angle.rs` |
| O02.3 | — | `tools/java-reference/probes/board/PullTightProbe.java` (append the `none` mode to `board` and `steps`), `tests/reference_pull_tight.rs` |
| O02.4 | `tests/pull_tight_any_angle_budget.rs` | — |
| O02.5 | — | `examples/pull_tight_dump.rs` (`--angle none`), `tests/issue954_keepout_insertion.rs` (remove the `needs O02` ignores, if O04 landed first), `docs/DIVERGENCES.md` |

## Checklist

- [x] **O02.1 — Corner and line reduction.**
  - Java: `reduceCorners` (`:61-218`), `reduceLines` (`:280-393`),
    `skipLines` (`:659-756`).
  - Rust: `optimize/tighten/any_angle.rs`.
  - Orders (exact): index loops in Java's direction and bounds;
    `reduceCorners` accepts a skipped corner only if the 3-line check passes
    and `lengthBefore = d(skip, a) + d(skip, b)` beats
    `lengthAfter = lengthApprox() + 1.5`; clip tests use `contains` (not
    `isOutside` as in 45°); each step returns `Unchanged` exactly when Java
    returns its input object, including the `cleanedNewLines` copy paths
    that return a new object.
  - **Accept:** probe mode `steps` with `NONE` for these three steps on 200
    seeded polylines per board (J2 and bm08 with SES): outputs and flags
    equal for every input.
  *Done 2026-10-16:* `optimize/tighten/any_angle.rs`: `reduce_corners_any`, `reduce_lines`,
  `skip_lines` (and `without_lines` for `Polyline.skipLines` with line origins); Java's in-place
  line reversal of `new Polyline(Line[])` replayed (`normalized`). Probe `pull-tight-any`
  (`tools/java-reference/probes/board/PullTightAnyProbe.java`, a new kind, so the O01 references
  stay untouched) section `steps-none` at `a773a70ec`: the 200 seeded polylines of `steps-45`
  per board with the restriction forced to `NONE`, each step alone with changed flag, result and
  changed area. `cargo test -p freerouting --test reference_pull_tight steps_none_match_java`:
  equal on every input on J2 and DAC2020_bm08 (routed SES). Java changed counts (J2 / bm08):
  reduceCorners 69/72, skipLines 8/7, reduceLines 1/0.

- [x] **O02.2 — Smoothing and repositioning.**
  - Java: `smoothenCorners` (`:222-246`), `smoothenCorner` (`:396-495`,
    `cosAngle > 0.999` early return, `middleApprox`, the translated cut
    line through `currentCorner.round()`), the `repositionLines` /
    `repositionLine` overrides (`:251-273`, `:497-657`), and start/end
    smoothing against contact traces (`:758-1002`, with `SKIP_LENGTH`).
  - Rust: `optimize/tighten/any_angle.rs`.
  - The fixpoint is `skipSegmentsOfLength0 → reduceLines → skipLines →
    reduceCorners → repositionLines → smoothenCorners` (`:38-57`); the stop
    check runs only when the last round changed something.
  - **Accept:** probe mode `steps` covers these steps, equal on every input.
  *Done 2026-10-16:* `smoothen_corners_any`, `smoothen_corner_any`, `reposition_lines_any`,
  `reposition_line_any`, `smoothen_start_corner_any`, `smoothen_end_corner_any` and the six-step
  round `any_angle_round` (dispatch arm `(AngleMode::Any, 0)` in `tighten/mod.rs`; the stop check
  stays the shared driver's). `steps_none_match_java` (above): repositionLines 20/16,
  smoothenCorners 83/77, skipSegmentsOfLength0 28/23, pullTight 120/109 changed of 200 (J2 /
  bm08), all equal; `ends_none_match_java`: both end smoothings of every trace on the imported
  board (`ends-none`) and the split unfixed board (`ends-none-split`, 3 start smoothings on bm08),
  equal. Kept Java quirk: `smoothenEndCornerAtTrace` takes the second last line's direction for
  both directions. Divergence: Java's AIOOBE in the end smoothing of a 3-corner trace with a
  skipped short end segment at a bend returns no smoothing (DIVERGENCES "Pull-tight").

- [x] **O02.3 — Board-level comparison.**
  - Extend `PullTightProbe` mode `board` with `--angle none` on the O01
    board set, and `tests/reference_pull_tight.rs` with a `none` case.
  - **Accept:** dumps identical on every board, ids included; at most 4 of
    43 boards may be expected mismatches, each bisected to its first
    differing trace and step, and the cause named next to the test (an
    ulp-level `Math.sqrt`/`cos` difference is acceptable only with the
    differing operands printed).
  *Done 2026-10-16:* section `board-none` (and `board-none-tree` on J2 and bm08) of
  `PullTightAnyProbe` on the O01 board set; `cargo test -p freerouting --test
  reference_pull_tight`: 13 passed, `board_none_matches_java` equal (dumps, ids, tree
  fingerprints) on 40 of 41 boards. One expected mismatch, `Issue756-minimal-hang`
  (`NONE_EXPECTED_MISMATCHES`, reason next to the test): neither Java nor Rust returns; after the
  first trace's end smoothing `split_traces` (Java `smoothenEndCornersAtTrace1 ->
  BasicBoard.splitTraces -> PolylineTrace.split -> removeIfCycle -> getConnectionItems`) doesn't
  end. Java's `jstack` after 40 s and Rust's `gdb` stacks at 3, 12 and 25 s are in that call; a
  100 nominal ms work limit doesn't fire (one stop check in 40 s), so the probe skips the board
  (`BOARD_SKIP`).

- [x] **O02.4 — Termination under a budget.**
  - Build a synthetic one-layer board in the test (no fixture file) with a
    trace whose consecutive corners are closer than one grid unit, the case
    Java's comment at `:43-50` describes.
  - **Accept:** `any_angle_terminates_under_budget`: with a small
    `WorkLimit` the call returns; 100 runs give the same dump and the same
    visit count. After J01, the same board built in a probe gives the same
    dump and visit count in Java.
  *Done 2026-10-16:* `tests/pull_tight_any_angle_budget.rs`: the board is built in the test from
  DSN text (one layer, 9 rows of 14 pads, 9 weaving traces from `JavaRandom(20261018)` with a line
  within one unit of every inner corner; `budget_board_has_corners_closer_than_one_unit`).
  `any_angle_terminates_under_budget`: limits 1 and 2 nominal ms both fire, 100 runs give the
  same dump and units, and Java's sections `budget-1`/`budget-2` (same board built in
  `PullTightAnyProbe`, pin `a773a70ec`) are equal: 12 stop checks at 66,889 units, 35 at 129,727.
  `cargo test -p freerouting --test pull_tight_any_angle_budget`: 2 passed.

- [x] **O02.5 — Wiring and determinism.**
  - `examples/pull_tight_dump.rs --angle none` works.
  - If O04 landed first, enable the any-angle cases of
    `tests/issue954_keepout_insertion.rs` (marked `needs O02`).
  - **Accept:** `tools/determinism-check.sh` reports `identical` for
    `pull_tight_dump --angle none` on J2 + SES; the Issue954 any-angle cases
    pass, or the note says O04 hasn't landed.
  *Done 2026-10-16:* `examples/pull_tight_dump.rs --angle none` sets `AngleRestriction::None`.
  `tools/determinism-check.sh --out <dir> -- target/release/examples/pull_tight_dump
  fixtures/Issue026-J2_reference.dsn fixtures/reference/route/Issue026-J2_reference.ses {OUT}
  --angle none`: `identical` (load 16.6 idle, 18.1 with 8 burners). O04 landed: the six
  `#[ignore = "needs O02"]` tests of `tests/issue954_keepout_insertion.rs` un-ignored; `cargo test
  -p freerouting --test issue954_keepout_insertion`: 12 passed, 0 ignored (all 75 cases). The
  DIVERGENCES line "Not ported yet: the any-angle tightener" is removed.

