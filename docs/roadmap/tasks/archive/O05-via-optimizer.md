# O05 — Via optimizer and the full changed-area optimization

Opened 2026-10-15. After every routed connection, in `removeTails` and in
fanout, Java calls `optChangedArea` with the router's trace costs
(`AutorouteConnectionRouter.java:122-128,254-260`,
`BatchAutorouter.java:539-545`, `RoutingBoard.java:973-980,1134-1141`). With
trace costs present, the pull-tight loop also moves vias along the cheaper
trace direction (`ViaOptimizer.optViaLocation`) and pulls plane and fanout
vias towards the trace corner (`optPlaneOrFanoutVia`). Without it, Rust
routes keep vias where Java moves them, so board parity (rung 3) can't hold.
This checklist ports `ViaOptimizer` and completes `opt_changed_area`. See
[O00](../O00-optimizer-README.md) for the series rules.

**Prerequisites:** O04 (`DrillItemMover` insert and check); A01
(`ExpansionCostFactor { horizontal: f64, vertical: f64 }`, Java's record
`AutorouteControl.ExpansionCostFactor`). A01 has no O prerequisite, so it can
land first.

## What exists and what this adds

| Exists | Where |
|---|---|
| `opt_changed_area` without the via branch; `pull_tight_own_net` (no limit) | O01: `optimize/pull_tight.rs`, `optimize/tighten/mod.rs` |
| `DrillItemMover` check and insert, `try_shove_via_points` | O03, O04: `shove/drill_check.rs`, `shove/insert.rs` |
| `check_trace_segment`, filtered `pick_items` (TRACES, VIAS, CONDUCTION) | B04 |
| `normal_contacts` (highest id first) | `board/basic_board.rs:1387` |
| Java `board/optimize/ViaOptimizer.java` (733 lines): `optViaLocation` `:33-158`, `optPlaneOrFanoutVia` `:161-300`, `repositionVia` overloads `:302`, `:367`, `:435-717`, `isWithinTolerance` `:719` | cda540f |
| Java `TraceTightener.optChangedArea` via branch (`:156-161`) | cda540f |

What this adds: `optimize/via.rs`, the `trace_costs` field of
`PullTightParams`, and the documented post-connection entry point the A
series calls.

## Decisions made here

- **Trace costs ride in `PullTightParams`** as `trace_costs:
  Option<Arc<[ExpansionCostFactor]>>`, one entry per layer. `None` keeps
  O01's behaviour (vias skipped), which is Java's `traceCosts == null`.
- **Nested pull-tights have no limit, but their work counts.**
  `ViaOptimizer` calls `PolylineTrace.pullTight(true, accuracy, null)`,
  which builds a tightener with no limit (`-1`). Rust does the same. Its
  tree visits land on the board's meter (B02.7), so the outer
  post-connection limit sees them at its next check. Java does the same at
  `30baad7e2`: its meter is per thread and counts every tree visit on it.
- **The accuracy passed to the via optimizer is the tightener's
  `min_translate_dist`** (`max(accuracy, 100)`), not the caller's accuracy
  (`TraceTightener.java:158`). Recursion depth 10.

## File ownership

| Item | New files | May edit |
|---|---|---|
| O05.1 | `optimize/via.rs` | — |
| O05.2 | — | `optimize/via.rs` |
| O05.3 | — | `optimize/params.rs`, `optimize/tighten/mod.rs` (via branch), `optimize/pull_tight.rs` |
| O05.4 | — | `optimize/pull_tight.rs` (docs and the doc test) |
| O05.5 | `tests/reference_opt_vias.rs`, `fixtures/reference/probe/optvias/*` | `tools/java-reference/probes/board/PullTightProbe.java` (append mode `optvias`) |
| O05.6 | — | `examples/pull_tight_dump.rs` (`--trace-costs <json>`) |

## Checklist

- [x] **O05.1 — Via between two traces.**
  - Java: `optViaLocation` (`:33-158`), the cost-based `repositionVia`
    (`:435-717`), `isWithinTolerance` (`:719`).
  - Rust: `optimize/via.rs`, `RoutingBoard::opt_via_location(via,
    trace_costs, accuracy, depth, meter) -> bool`.
  - Orders (exact):
    - shove-fixed via → false; depth ≤ 0 → false;
    - normal contacts in their set order (highest id first): one contact →
      plane/fanout case; two → both must be unfixed polyline traces, a
      conduction area makes it the plane case; any other count → false;
    - tolerance `(int)(via.min_width() / 2) + 1`; the from-corner is corner
      1 or `cornerCount − 2` depending on which end is within tolerance
      (first corner tested first);
    - costs `trace_costs[layer]`, or `(1, 1)` for both when absent;
    - a new location equal to the centre → false; move with
      `DrillItemMover.insert(via, delta, 9, 9, no tidy)`; then
      `pull_tight_own_net` on traces picked at the new location on the
      first trace's layer, then on the second's; then recurse on the first
      via picked there (highest id) with depth − 1, and stop.
  - **Accept:** O05.5.
  *Done 2026-10-16:* `crates/freerouting/src/optimize/via.rs`: `RoutingBoard::opt_via_location(via,
  trace_costs, accuracy, depth) -> bool` (no meter argument: the nested pull-tights count on the
  board's own work meter, module docs), the cost-based `repositionVia` split into
  `reposition_via_by_costs`, `reposition_acute_angle`, `reposition_axis_parallel`, with the two
  helper overloads `reposition_via_toward` and `reposition_via_with_connection`, and
  `is_within_tolerance`. Accepted by O05.5 (`opt_vias_match_java`, 41 of 41 boards identical).
  Branches reached on the probe boards (temporary counters, removed): collinear 50, cost towards
  the first corner 80, towards the second 32, acute angle 30, axis-parallel 49, two-trace moves
  215; unit test `optimize::via::tests::tolerance_is_manhattan_and_inclusive`.

- [x] **O05.2 — Plane and fanout vias.**
  - Java: `optPlaneOrFanoutVia` (`:161-300`), `repositionVia` (`:302-433`).
  - Rust: `optimize/via.rs`.
  - Orders: at most one plane and one unfixed trace among the contacts;
    first `repositionVia` toward the rounded check corner; if that fails and
    the trace has ≥ 3 corners, the perpendicular projection onto the
    previous line, accepted only if it isn't the centre, matches the angle
    restriction, passes `DrillItemMover.check(via, diff, 0, 0, …)` and
    `check_trace_segment` returns at least `i32::MAX as f64`; the new
    location must still be in the same plane (CONDUCTION pick); move with
    depths 9/9; pull-tight own net; recurse with depth − 1 only when the
    via landed exactly on the check corner.
  - **Accept:** O05.5.
  *Done 2026-10-16:* `opt_plane_or_fanout_via` and `project_via_to_line` in `optimize/via.rs`.
  Accepted by O05.5 (`opt_vias_match_java`, 41 of 41 identical): 8 plane/fanout entries, 16
  moves, 16 recursions after landing on the check corner. The projection branch is not reached
  by any probe board (it is ported line by line; Java's order of checks kept).

- [x] **O05.3 — The via branch of `opt_changed_area`.**
  - Java: `TraceTightener.optChangedArea` (`:156-161`): a `Via` in the
    scan with trace costs present calls `optViaLocation(board, via, costs,
    minTranslateDist, 10)`; a change marks `somethingChanged` but does not
    `break` the scan.
  - Rust: add `trace_costs` to `PullTightParams`; implement the branch.
  - **Accept:** `opt_changed_area_without_costs_skips_vias` (O01's dumps
    unchanged with `None`), and O05.5.
  *Done 2026-10-16:* `PullTightParams::trace_costs: Option<Arc<[ExpansionCostFactor]>>`
  (`optimize/params.rs`, `None` in `new`); the branch is in `tighten_changed_area`
  (`optimize/pull_tight.rs`, where O01 put the scan; `tighten/mod.rs` untouched): a via with
  costs calls `opt_via_location(id, costs, tightener.min_translate_dist(), 10)`, a change sets
  `something_changed` and the scan goes on. `cargo test -p freerouting --test reference_opt_vias`:
  `opt_changed_area_without_costs_skips_vias` (J2, bm08, bm05, rpi_splitter: no via moves, dumps
  equal to Java's `pull-tight` `board-45`), `opt_vias_match_java` (section `board-optvias`);
  `--test reference_pull_tight` still 13 passed.

- [x] **O05.4 — The post-connection entry point for the A series.**
  - Document on `RoutingBoard::opt_changed_area` the three Java call
    patterns the A series reproduces: after each connection
    (`only_nets` empty, `Clip::Unbounded`, the router's pull-tight
    accuracy, the router's trace costs, the post-connection `WorkLimit`),
    in `removeTails` (same arguments after `remove_trace_tails`), and in
    fanout (`RoutingBoard.java:1134-1141`).
  - Add `PullTightParams::after_connection(accuracy, trace_costs)`, which
    builds exactly that. A11.5 and A12 write the call as
    `opt_changed_area(&[], None, accuracy, trace_costs, budget 1000 units)`;
    that is this helper.
  - **Accept:** a doc test that marks the whole board, calls the entry
    point with default trace costs on `Issue508-DAC2020_bm08` + routed SES,
    and finishes with no changed area left.
  *Done 2026-10-16:* the three call patterns documented on `RoutingBoard::opt_changed_area`
  (after a connection `AutorouteConnectionRouter.java:126-132` and `:258-264`, `removeTails`
  `BatchAutorouter.java:536-545`, fanout `RoutingBoard.java:1136-1142` and the one-item
  autoroute `:975-981`, the last two with `only_nets` = the routed net);
  `PullTightParams::after_connection(accuracy, trace_costs)` (all nets, `Clip::Unbounded`,
  limit `ENDLESS_LOOP_GUARD_NOMINAL_MS` = 1000 nominal ms, the caller sets `stop`). The doc test
  (bm08 + `-routed.ses`, default settings' `apply_board_specific` costs, whole board marked)
  passes: `cargo test -p freerouting --doc optimize`, 1 passed; not stopped, no changed area
  left.

- [x] **O05.5 — Probe `optvias`.**
  - Java: `PullTightProbe` mode `optvias`: like mode `board`, but with
    trace costs built exactly as `AutorouteControl` builds them for the
    batch autorouter (default settings after
    `applyBoardSpecificOptimizations`), no limit. The probe writes the cost
    array it used into the JSON; the Rust test reads it from there, so this
    item doesn't wait for C03.
  - Board set: O01's 43 boards, 45°.
  - **Accept:**
    - traces and vias identical, ids included, on every board; at most 3 of
      43 listed as expected mismatches, each bisected to the first via move
      that differs and its cause named;
    - DRC on every Rust result reports no clearance violation that the
      input board didn't have, and no via inside a pad of another net (D8).
  *Done 2026-10-16:* `tools/java-reference/probes/board/OptViasProbe.java` (kind `opt-vias`, a new
  probe beside `PullTightProbe.java`, which is O01's uncommitted file; the directory is
  `fixtures/reference/probe/opt-vias/` after `generate.sh`'s kind naming), generated at
  `a773a70ec` with `generate.sh <pin> opt-vias`: 41 boards (O01's set has 41, not 43), no Java
  exception. Sections `costs` (Java's `getTraceCosts()` after `applyBoardSpecificOptimizations`
  on default settings, read by the Rust test), `vias-each` (`optViaLocation` of every via) and
  `board-optvias`. `cargo test -p freerouting --test reference_opt_vias`:
  `opt_vias_match_java` identical on 41 of 41 boards, ids and `maxGeneratedId` included, no
  expected mismatch; 55 via moves in `vias-each` (bm07 7, J2 5, bm05 2-3, …). DRC on every
  Rust result: no clearance-violation count per (layer, item kinds and nets) above the input
  board's, no new via overlapping a pin of another net (`drc_regressions`).

- [x] **O05.6 — Determinism.**
  - `examples/pull_tight_dump.rs --trace-costs <json>`.
  - **Accept:** `tools/determinism-check.sh` reports `identical` with trace
    costs on J2 and bm08.
  *Done 2026-10-16:* `crates/freerouting/examples/opt_vias_dump.rs <dsn> <ses> <out>
  [--trace-costs <json>|default]` (a new example beside O01's `pull_tight_dump.rs`, which is
  uncommitted O01 work). `tools/determinism-check.sh --out <dir> -- <release>/opt_vias_dump
  fixtures/<b>.dsn <ses> {OUT}` (default costs): `identical` on Issue026-J2_reference and
  Issue508-DAC2020_bm08. With Java's costs as `--trace-costs '[[1.0,2.8],[1.5,1.0]]'` (J2) and
  `'[[1.0,2.5],[1.7000000000000002,1.0]]'` (bm08) the dump equals Java's `board-optvias` dump,
  and equals the default-cost run (the Rust default costs are Java's on both). Release time of
  `opt_changed_area` with costs: J2 6.5 ms, bm08 5.3 ms; Java probe (cold JVM): J2 80 ms, bm08
  49 ms.
