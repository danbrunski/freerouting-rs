# O02 — Pull-tight, any angle

Opened 2026-10-15. The any-angle tightener is the third variant of Java's
pull-tight. No fixture routes with it (all routed fixtures use the 45° tree),
so it is off the router's critical path. It is still needed: Java selects it
whenever a board's angle restriction is `NONE`, the interactive editor offers
that mode, and `Issue954CircularKeepoutInsertionTest` runs its insertion and
cleanup cases in all three angle modes (O04.6). It is the numerically
riskiest variant: float corners, cosine thresholds and rounding decide the
result. See [O00](O00-optimizer-README.md) for the series rules.

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

- [ ] **O02.1 — Corner and line reduction.**
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

- [ ] **O02.2 — Smoothing and repositioning.**
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

- [ ] **O02.3 — Board-level comparison.**
  - Extend `PullTightProbe` mode `board` with `--angle none` on the O01
    board set, and `tests/reference_pull_tight.rs` with a `none` case.
  - **Accept:** dumps identical on every board, ids included; at most 4 of
    43 boards may be expected mismatches, each bisected to its first
    differing trace and step, and the cause named next to the test (an
    ulp-level `Math.sqrt`/`cos` difference is acceptable only with the
    differing operands printed).

- [ ] **O02.4 — Termination under a budget.**
  - Build a synthetic one-layer board in the test (no fixture file) with a
    trace whose consecutive corners are closer than one grid unit, the case
    Java's comment at `:43-50` describes.
  - **Accept:** `any_angle_terminates_under_budget`: with a small
    `WorkLimit` the call returns; 100 runs give the same dump and the same
    visit count. After J01, the same board built in a probe gives the same
    dump and visit count in Java.

- [ ] **O02.5 — Wiring and determinism.**
  - `examples/pull_tight_dump.rs --angle none` works.
  - If O04 landed first, enable the any-angle cases of
    `tests/issue954_keepout_insertion.rs` (marked `needs O02`).
  - **Accept:** `tools/determinism-check.sh` reports `identical` for
    `pull_tight_dump --angle none` on J2 + SES; the Issue954 any-angle cases
    pass, or the note says O04 hasn't landed.
