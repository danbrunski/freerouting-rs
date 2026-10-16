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
[O00](O00-optimizer-README.md) for the series rules.

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

- [ ] **O05.1 — Via between two traces.**
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

- [ ] **O05.2 — Plane and fanout vias.**
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

- [ ] **O05.3 — The via branch of `opt_changed_area`.**
  - Java: `TraceTightener.optChangedArea` (`:156-161`): a `Via` in the
    scan with trace costs present calls `optViaLocation(board, via, costs,
    minTranslateDist, 10)`; a change marks `somethingChanged` but does not
    `break` the scan.
  - Rust: add `trace_costs` to `PullTightParams`; implement the branch.
  - **Accept:** `opt_changed_area_without_costs_skips_vias` (O01's dumps
    unchanged with `None`), and O05.5.

- [ ] **O05.4 — The post-connection entry point for the A series.**
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

- [ ] **O05.5 — Probe `optvias`.**
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

- [ ] **O05.6 — Determinism.**
  - `examples/pull_tight_dump.rs --trace-costs <json>`.
  - **Accept:** `tools/determinism-check.sh` reports `identical` with trace
    costs on J2 and bm08.
