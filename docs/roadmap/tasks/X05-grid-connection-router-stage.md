# X05 — Grid A* as a connection router (`krt-grid`)

Opened 2026-10-15. With the vendored search (X03) and the raster (X04), this
checklist makes an A10 `ConnectionRouter` implementation that routes one
connection on the grid and inserts the result as ordinary board copper. In
KRT, the corresponding Python is `single_ended_routing.py`
(`_path_to_segments_vias`, `simplify_path`, `_merge_terminal_to_exact`),
`routing_context.py` and the probe logic, at `0486e799`. The insertion is the
part that decides whether the grid router is safe to use at all: every path
passes freerouting-rs's exact DRC before it is committed, and a path that
doesn't is rejected, never repaired.

Prerequisites: X04; A10 (the trait, `ConnectionOutcome`, `StageCtx` with its
WorkMeter); B05 (pin connection points and exit restrictions); B09 (insert →
exact DRC → commit or revert); F04.

## What exists and what this adds

| Exists | State |
|---|---|
| `krt-grid-router::GridRouter::route_multi(obstacles, sources, targets, max_iterations, …)` and `route_with_frontier` (X03) | returns `Vec<(gx, gy, layer)>` and, on failure, the sorted blocked frontier |
| A10 `ConnectionRouter::route(&mut self, board, task, params, ctx) -> ConnectionOutcome` (pipeline report §3.5) | the trait |
| Java `FoundConnectionInserter` (cda540f) | how Freerouting inserts a found path; ported by A06. Not used here: X05 inserts its own geometry through the board API |
| KRT: collinear points dropped; a via at each layer change except at a same-net through-hole; connectors from the grid end to the exact off-grid pad point, or the last vertex moved onto it when the grid stand-in grazes a pad | the design |
| KRT `GridRouter::route_multi`'s budget arguments (`max_iterations`, `max_iterations_ceiling`, `quantum_cells`, `quantum_pct`, `grace_tranches`) | budgets in iterations; extension off here (Q08 covers it for the maze search) |

## Decisions made here

- **Id** `krt-grid`, selected by `router.autorouter.connection_router`.
- **Budget:** one work unit per grid iteration; `max_iterations` is the
  connection's WorkLimit as A10 hands it over; the earned extension is off
  (`max_iterations_ceiling = 0`). Work used is reported to the WorkMeter (D5).
- **Retries at finer steps:** 0.1 mm, then 0.05 mm, then 0.025 mm (KRT's
  defaults for normal, tap and rescue routing), each on a fresh X04 raster.
  The next step is tried only if the previous one found no path, found no
  terminal cell, or its path failed the exact DRC.
- **Path → copper:**
  - grid points → board coordinates through the frame (integers);
  - collinear points dropped by an exact integer cross-product test;
  - one trace per run of equal layer, at the net's half-width on that layer;
  - a via at each layer change, with the padstack of the first via info in
    the net's via rule that spans both layers;
  - the ends joined to the pins' exact connection points: a straight segment
    from the path end to the pin's connection point (B05) if it passes the
    clearance check; else the last vertex moved onto the nearest point inside
    the pin shape; else the attempt fails.
- **The exact DRC decides.** The new items are inserted in one B09
  transaction; if Rust's DRC reports any new clearance item, or any new via
  overlaps a pad (D8), the transaction is reverted.
- **No ripup inside the stage.** The grid search can't cross foreign copper.
  The pass loop's ripup policy (A10) still applies between passes.
- **Failure record.** On failure the outcome carries the vendored frontier,
  mapped back to blocking items by a shape query at each frontier cell's
  centre, sorted `(net, item id)`, capped at 10,000 entries like KRT. Q06 can
  read it as it reads the maze search's.

## File ownership

| Item | New files | May edit |
|---|---|---|
| X05.1 | `crates/freerouting-krt/src/grid/router.rs` | `crates/freerouting-krt/src/lib.rs` (register line) |
| X05.2 | `src/grid/path.rs` | — |
| X05.3 | — | `src/grid/terminals.rs` (connectors) |
| X05.4 | — | `src/grid/router.rs` |
| X05.5 | — | `src/grid/router.rs` |
| X05.6 | `fixtures/krt/x05-open-two-pin.dsn`, `fixtures/krt/x05-fine-pitch.dsn`, `crates/freerouting-krt/tests/grid_router.rs` | — |

## Checklist

- [ ] **X05.1 — The stage.**
  Rust design: `grid::router::KrtGrid { step_um: u32 }` implementing
  `ConnectionRouter`. `route` builds the frame and raster (X04), collects
  source and target cells, calls `route_multi`, and hands the path to X05.2.
  - **Accept:** with `router.autorouter.connection_router` at its default,
    `ses-diff` shows identical sessions on `benchmark`; an unknown id
    behaves as A10 specifies.

- [ ] **X05.2 — Path simplification and conversion.**
  Source: KRT `simplify_path`, `_path_to_segments_vias`.
  - **Accept:** unit tests: a staircase of 45° steps collapses to one
    diagonal segment; a layer change produces exactly one via at that point;
    a path that returns to the same layer twice produces two vias.

- [ ] **X05.3 — Exact terminal connectors.**
  Source: KRT `_merge_terminal_to_exact`.
  - **Accept:** on `x05-open-two-pin.dsn` both ends of the inserted trace
    lie on the pins' connection points; on a pad whose centre is off-grid by
    half a step, the connector is a straight segment and the DRC is clean.

- [ ] **X05.4 — Transactional insert and the DRC gate.**
  - **Accept:**
    - a test that forces a raster bug (one obstacle cell cleared by hand)
      shows the path rejected by the DRC gate and the board unchanged
      (`state_hash` equal before and after, B01);
    - on `connection-parity`, every connection `krt-grid` reports as routed
      has no DRC item and no via in a pad.

- [ ] **X05.5 — Retries, failure record, work units.**
  - **Accept:** on `x05-fine-pitch.dsn` (a 0.4 mm pitch part), the
    attempt log shows the 0.1 mm step failing and a finer step either
    succeeding or failing with a frontier naming the blocking pads; work
    units used are reported per connection and sum into the WorkMeter.

- [ ] **X05.6 — Determinism and the connection-level comparison.**
  - **Accept:**
    - `tools/determinism-check.sh` passes routing `connection-parity` with
      `krt-grid`;
    - the done note gives, per board in `connection-parity`, connections
      routed by the Freerouting maze search and by `krt-grid`, and the
      median wall time per connection for each. This is a report, not a
      keep/reject decision; X06 measures the full pipeline.
