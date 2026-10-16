# X06 — Grid A* followed by pull-tight (`krt-astar`)

Opened 2026-10-15. D9 names this combination as its example: a grid A*
connection router whose result Freerouting's pull-tight then cleans up.
KRT's paths are octilinear, the same as Freerouting's 45° style, so Java's
`TraceTightener45` (O01) and `ViaOptimizer` (O05) can take them as input.
There is nothing in Java to be at parity with, so this is a measured
experiment.

The expected result, preregistered: the grid router loses in fine-pitch pin
fields, where a conservative 0.1 mm raster closes corridors the gridless
search keeps, and does better, if anywhere, on open runs. The measurement
splits the result by region class to test exactly that. A hybrid (grid for
open runs, the room/door search near pins) is recorded as a follow-up, not
built.

Prerequisites: X05; O01 (pull-tight on the changed area, `opt_changed_area`);
O05 (via optimizer); O06 (batch optimizer); A10, A11; F03.1, F03.4, F02.6.

## What exists and what this adds

| Exists | State |
|---|---|
| Java `AutorouteConnectionRouter` at cda540f: after each routed connection, `optChangedArea` pull-tightens the changed area | ported by A06/O01; the bundle keeps this step |
| Java `TraceTightener45` / `TraceTightenerAnyAngle`, `ViaOptimizer` (`board/optimize/`, cda540f) | ported by O01, O02, O05 |
| Java `BatchOptimizer` | ported by O06 |
| `krt-grid` (X05) | the connection router |

## Decisions made here

- **Bundle id `krt-astar`**, selected by `router.autorouter.algorithm`: the
  Freerouting ordering, ripup policy and pass controller, with `krt-grid` as
  the connection router, then the same pull-tight of the changed area after
  each connection, then the Freerouting optimizer.
- **Grade after the optimizer.** The conservative raster leaves slack that
  pull-tight reclaims; comparing before it would be unfair to the grid.
- **Region classes** for each connection (from the input board, before any
  run):
  - `pin-field`: an end pad is fine-pitch by KRT's rule (a pad of the same
    component within 0.65 mm centre to centre, or a smaller dimension under
    0.35 mm), or lies in a component with at least 32 pads and a pitch of at
    most 0.8 mm;
  - `open`: everything else.
- **Kept only as a selectable implementation.** Even a win on one board
  class doesn't make it a default without the full measurement rules.

## File ownership

| Item | New files | May edit |
|---|---|---|
| X06.1 | `crates/freerouting-krt/src/grid/bundle.rs` | `crates/freerouting-krt/src/lib.rs` (register line) |
| X06.2 | `crates/freerouting-krt/src/grid/regions.rs` | — |
| X06.3 | `tools/bench/sets/x06-grid.txt`, the X06 preregistration and baseline files | `docs/DIVERGENCES.md` (append, only if kept) |
| X06.4 | — | `docs/roadmap/algorithms.md` (the hybrid follow-up paragraph) |

## Checklist

- [ ] **X06.1 — The bundle.**
  - **Accept:**
    - routing `connection-parity` with `krt-astar` gives no DRC item and no
      via in a pad on any board;
    - `tools/determinism-check.sh` passes on two `benchmark` boards;
    - the default bundle's sessions are unchanged (`ses-diff`).

- [ ] **X06.2 — Region classification.**
  Rust design: `grid::regions::classify(board) -> BTreeMap<ConnectionKey,
  Region>` with the rule above, in integer board units.
  - **Accept:** unit tests on a synthetic QFN and an open two-pin
    connection; on `canary-pwr`, the 17 Q03 pins all fall in `pin-field`.

- [ ] **X06.3 — Measurement by region class.**
  Preregister (F03.4) with the hypothesis "krt-astar completes fewer
  `pin-field` connections than the default on every board". Arms: the
  default bundle and `krt-astar`. Boards: X01's five boards (graded by
  F02.6) plus at least three `benchmark` boards (`x06-grid.txt`). Signal:
  unrouted, per region class. Guards: `drc_real`, vias in pads, vias,
  length, wall time. Also record raster build time as a share of the stage's
  time (this decides X04.6).
  - **Accept:** measured, per region class. Keep `krt-astar` as a selectable
    implementation only if it wins on at least one region class on N−1
    boards without regressing on the other class on any board. Otherwise
    remove X06.1's bundle code (X05's stage may stay for other uses) and keep
    the numbers here. State whether the hypothesis held.

- [ ] **X06.4 — Record the hybrid follow-up.**
  - **Accept:** a paragraph in algorithms.md ("Grid A* followed by
    pull-tight") states the measured split by region class and whether a
    hybrid (grid for `open` connections, maze search for `pin-field`) is
    worth a checklist, with the numbers that say so.
