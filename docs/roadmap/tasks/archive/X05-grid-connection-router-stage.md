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

- [x] **X05.1 — The stage.**
  Rust design: `grid::router::KrtGrid { step_um: u32 }` implementing
  `ConnectionRouter`. `route` builds the frame and raster (X04), collects
  source and target cells, calls `route_multi`, and hands the path to X05.2.
  - **Accept:** with `router.autorouter.connection_router` at its default,
    `ses-diff` shows identical sessions on `benchmark`; an unknown id
    behaves as A10 specifies.
  *Done 2026-10-17:* `crates/freerouting-krt/src/grid/router.rs`: `KrtGrid { step_um,
  raster_hook }` (the hook is a test seam, `None` in production), registered by
  `grid::register` (called from `freerouting_krt::register`) as `krt-grid`. It takes
  Freerouting's route sets (A11.1, plane nets through `FreeroutingPlaneTaps` with the
  fallback), and calls `route_with_frontier` rather than `route_multi`: the same search with
  the frontier sink, which X05.5 needs. KRT's search defaults: heuristic weight 2.3, turn
  cost 1000, via cost 75 steps of 0.1 mm (`via_cost_units`), via proximity cost 10. Not in
  the checklist: a window above `MAX_FRAME_CELLS` (2^22 cells per layer) is not rasterized
  at that step (`FrameTooLarge`), a memory bound that depends on geometry only. No settings
  field for `router.krt.grid_step_um` yet (the factory uses the 0.1 mm default; the
  settings files carry P02's uncommitted hunks in the main tree). Unknown id:
  `the_registry_selects_krt_grid_and_reports_an_unknown_id` (`krt-grid` builds;
  `krt-nope` is `UnknownStage { key: "router.autorouter.connection_router", .. }` listing
  both routers; without `freerouting_krt::register` `krt-grid` is unknown too). Default
  sessions: HEAD `0ada971` CLI against this tree's CLI (release, `freerouting -de <board>
  -do <ses>`; `cmp` and `freerouting-dev ses-diff`, exit 0 every board): `connection-parity`
  10/10 byte-identical; `benchmark` the first 5 boards (Issue433, ecc83, bm02, sonde, bm07)
  with default settings byte-identical, bm01, bm10, interf_u, Issue555, z10 and coldfire with
  `-mp 3 --router.optimizer.enabled=false` byte-identical; the 5 boards from 569
  connections not run (CPU-85 was stopped after coldfire's 28 min for both runs). In-process,
  built-in registry against the one with `freerouting_krt::register`, stage unselected
  (`default_off_sessions_on_a_set`, release): `connection-parity` 10/10 and the 5 benchmark
  boards identical; `registering_krt_grid_changes_no_session_while_it_is_unselected` on the
  three X05 boards.

- [x] **X05.2 — Path simplification and conversion.**
  Source: KRT `simplify_path`, `_path_to_segments_vias`.
  - **Accept:** unit tests: a staircase of 45° steps collapses to one
    diagonal segment; a layer change produces exactly one via at that point;
    a path that returns to the same layer twice produces two vias.
  *Done 2026-10-17:* `crates/freerouting-krt/src/grid/path.rs` (F04 SPDX header, KRT
  `simplify_path` and `_path_to_segments_vias` at `0486e799`, `docs/PROVENANCE.md` row):
  `simplify` drops a same-layer interior point when the two steps have a zero `i64` cross
  product and a positive dot product; `plan` cuts the path into trace and via pieces in path
  order (a via at each layer change, none where an own-net pin or via spans both layers).
  `cargo test -p freerouting-krt --lib grid::` (9 passed):
  `a_staircase_of_45_degree_steps_collapses_to_one_diagonal_segment` (six diagonal steps →
  one segment (0,0)–(5000,5000); a turn keeps its corner, a reversal is kept),
  `a_layer_change_produces_exactly_one_via_at_that_point` (one via at (2000,0), 0→1),
  `a_path_that_returns_to_the_same_layer_twice_produces_two_vias` (vias at (1000,0) 0→1 and
  (3000,0) 1→0, three traces), `an_own_hole_replaces_the_via_and_end_vias_leave_one_corner_runs`,
  `the_collinearity_test_is_exact_at_the_coordinate_limits` (±`i32::MAX/2`).

- [x] **X05.3 — Exact terminal connectors.**
  Source: KRT `_merge_terminal_to_exact`.
  - **Accept:** on `x05-open-two-pin.dsn` both ends of the inserted trace
    lie on the pins' connection points; on a pad whose centre is off-grid by
    half a step, the connector is a straight segment and the DRC is clean.
  *Done 2026-10-17:* `src/grid/terminals.rs`: `end_owner` (the item whose copper holds the
  path end: pins and vias, then traces, then areas, lowest id) and `connector` (pin or via: a
  straight segment to the drill centre if `check_polyline_trace` passes it, else the end
  moved onto the nearest point inside the pad, else failure; trace: a segment to the integer
  projection on its polyline, else `connect_to_trace_object` after insertion; area: none).
  Written from the checklist's design (credit line, no ported code). The fixture is
  `crates/freerouting-krt/tests/data/x05-open-two-pin.dsn`, not `fixtures/krt/` (every DSN
  under `fixtures/` needs a Java reference, as Q01 and X02 found): two SMD pads of net A,
  P1 centred at (10.05, 15.05) mm, half a 0.1 mm step off the grid in x and y.
  `both_ends_of_the_open_two_pin_trace_lie_on_the_pin_centres` (`cargo test -p
  freerouting-krt --test grid_router`): routed at 100 µm in one attempt (191 iterations, one
  trace, no via); the trace ends are exactly the two pin centres; P1's connector is one
  straight segment from (100500, 150500) to the on-grid cell centre (105000, 148000) inside
  P1's pad; `clearance_violations` of the new items 0; the net has no unconnected item.

- [x] **X05.4 — Transactional insert and the DRC gate.**
  - **Accept:**
    - a test that forces a raster bug (one obstacle cell cleared by hand)
      shows the path rejected by the DRC gate and the board unchanged
      (`state_hash` equal before and after, B01);
    - on `connection-parity`, every connection `krt-grid` reports as routed
      has no DRC item and no via in a pad.
  *Done 2026-10-17:* each attempt inserts its pieces in path order in one
  `RoutingBoard::try_change` (`CheckPolicy::NewItemsOnly`); the closure fails (abort, no
  check) on a failed trace connector, a new via whose copper overlaps any pin's pad, or no
  drop in the net's connected-set count; a clean closure is reverted on any clearance
  violation of a new item. After a revert the redo data is dropped (`generate_snapshot` +
  `pop_snapshot`), so the board equals the before-state; ids stay taken (B09). Not
  covered: `BoardRules::hole_clearance` (drill to drill on a layer where a padstack has no
  copper), which neither the raster nor Rust's clearance check tests; vias whose padstack
  has copper on every layer it spans (all the boards here) are covered by the copper
  clearance.
  `a_forced_raster_bug_is_rejected_by_the_drc_gate_and_the_board_is_unchanged`
  (`x05-wall.dsn`, a hook clears the wall's trace cells on F.Cu along P1's row): the path
  runs through the wall at 100, 50 and 25 µm (193, 396, 855 iterations) and each insert is
  `Rejected(Clearance { count: 1, first: (new trace, ItemId(6) the W wall, layer 0) })`;
  `state_hash` and every item's `Debug` text equal before and after; the net is still
  unconnected. `connection-parity`: `connection_level_comparison_on_a_set` (release, pass 1,
  no optimizer; a report-only diagnostic checks every routed connection's new items right
  after it): 31 connections routed by `krt-grid` over the 10 boards, 0 clearance violations,
  0 vias in pads (the test asserts both per board).

- [x] **X05.5 — Retries, failure record, work units.**
  - **Accept:** on `x05-fine-pitch.dsn` (a 0.4 mm pitch part), the
    attempt log shows the 0.1 mm step failing and a finer step either
    succeeding or failing with a frontier naming the blocking pads; work
    units used are reported per connection and sum into the WorkMeter.
  *Done 2026-10-17:* `KrtGrid::steps` is 100, 50, 25 µm; each step builds a fresh window,
  raster and terminal set, and the next step runs only after no terminal cell, no path or a
  rejected insert. The connection's `work_budget` (capped at `u32::MAX`) is the iteration
  budget shared by all its attempts; each attempt's iterations go on the board's meter
  (`WorkMeter::add`), and `ConnectionOutcome::work` is the meter's delta over the whole
  call. On failure `FailureRecord::blocking` (a new engine field in `autoroute/stages.rs`, with `BlockingItem` and `MAX_BLOCKING_ITEMS`; empty for the maze search) holds the last
  failed search's frontier mapped to items by `grid::frontier::blocking_items`: per frontier
  cell, the items closer to its centre than the raster's trace inflation, counted per `(net,
  item, layer)`, sorted, capped at 10,000. Fixture `tests/data/x05-fine-pitch.dsn`: pin 3
  of five 0.25 × 0.8 mm pads at 0.4 mm pitch, centre at x = 20.05 mm (half a 0.1 mm step off
  the grid), width and clearance 0.127 mm, to an on-grid pad.
  `the_fine_pitch_pin_fails_at_0_1_mm_and_a_finer_step_decides`: attempt log
  `step 100 um: 22454 cells, 18/77 terminal cells, 18 iterations, NoPath, 2 blocking`
  (blocking: the neighbouring pins, items 3 and 5, net 0, 13 frontier cells each), then
  `step 50 um: 89404 cells, 85/273 terminal cells, 266 iterations, Routed { traces: 1, vias:
  0 }`; the new items have no clearance violation and the net is connected; the outcome's
  work is 1520 units, equal to the meter's delta, with 284 grid iterations in it.
  `the_wall_blocks_every_step_and_the_frontier_names_the_wall` (`x05-wall.dsn`): NoPath at
  100, 50 and 25 µm (59,538, 240,263 and 972,287 iterations); the record is sorted by `(net,
  item id, layer)` and names both wall traces of net W, on F.Cu and B.Cu.
  `the_work_of_each_connection_sums_into_the_meter`: the outcomes of every pass-1 item of
  the wall board sum to the meter's delta.

- [~] **X05.6 — Determinism and the connection-level comparison.**
  - **Accept:**
    - `tools/determinism-check.sh` passes routing `connection-parity` with
      `krt-grid`;
    - the done note gives, per board in `connection-parity`, connections
      routed by the Freerouting maze search and by `krt-grid`, and the
      median wall time per connection for each. This is a report, not a
      keep/reject decision; X06 measures the full pipeline.
  *Partial 2026-10-17:* determinism and a pre-P04 report are done; the registered run
  (`docs/roadmap/prereg/X05.json`, F03.4 format) waits for P04's default-tree decision (P00
  README "Order"), owner P04 then this item. `tools/determinism-check.sh --out <x05>/det --
  env X05_SET=tools/bench/sets/connection-parity.txt X05_SES_OUT={OUT} <release
  grid_router test> write_krt_grid_sessions --ignored --exact` (default settings with
  `autorouter.connection_router=krt-grid`, optimizer on): `identical` over the 10 SES
  (idle, then 8 burners). Pre-P04 report, `connection_level_comparison_on_a_set`
  (release, pass 1, optimizer off, this tree; times on a loaded machine, load 2–5):

  | board | maze attempted / routed | maze median ms | krt-grid attempted / routed | krt-grid median ms |
  |---|---|---|---|---|
  | min_fr_test | 1 / 1 | 1.19 | 1 / 1 | 1.85 |
  | Issue368-CorneyIslandWireless | 0 / 0 | – | 0 / 0 | – |
  | Issue110 | 4 / 4 | 0.66 | 4 / 4 | 4.15 |
  | Issue143-rpi_splitter | 6 / 6 | 1.01 | 5 / 5 | 5.24 |
  | Issue413-test | 1 / 1 | 10.18 | 2 / 0 | 93.09 |
  | Issue508-SMD-routing-issue-demo | 5 / 5 | 0.73 | 6 / 2 | 153.59 |
  | TestSensel | 14 / 12 | 0.94 | 14 / 3 | 62.91 |
  | Issue269-NoViasOnPowerPlanes | 8 / 8 | 0.98 | 8 / 8 | 18.51 |
  | Issue433-my-board | 8 / 8 | 0.29 | 8 / 8 | 1.04 |
  | PCBench-retroreflectors-CONGFLOCK | 7 / 0 | 0.20 | 7 / 0 | 19.83 |

  Attempted counts differ where one router's connection also completes another's. The
  failed `krt-grid` attempts, every step counted: 24 no path, 26 trace connector, 18 not
  connected, 4 window too large (Issue413 at 25 µm). The trace connector and not-connected
  rejections come from ends on existing traces (TestSensel's pre-routed stubs end next to
  pins; diagonal traces have no integer projection), a lead for X06.
