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

- [x] **X06.1 — The bundle.**
  - **Accept:**
    - routing `connection-parity` with `krt-astar` gives no DRC item and no
      via in a pad on any board;
    - `tools/determinism-check.sh` passes on two `benchmark` boards;
    - the default bundle's sessions are unchanged (`ses-diff`).
  *Done 2026-10-17:* `crates/freerouting-krt/src/grid/bundle.rs`: `KRT_ASTAR_BUNDLE`
  (`FREEROUTING_BUNDLE` with connection router `krt-astar`) and `KrtAstar { grid: KrtGrid }`:
  `krt-grid`'s `route_logged`, then on `Routed` `opt_changed_area(PullTightParams::after_connection(
  p.pull_tight_accuracy, p.trace_costs))` as Freerouting's router does, the outcome's work the
  meter delta over both; `grid::bundle::register` (called from `freerouting_krt::register`)
  adds the bundle and the router id. No engine file changed. Tests (`cargo test -p
  freerouting-krt --test astar_bundle`, 10 passed, 4 ignored, the two below included):
  `the_registry_selects_the_krt_astar_bundle` (no warning; every other stage id Freerouting's;
  without `register` Java's normalisation warning and `freerouting-connection`),
  `the_bundle_pulls_the_grid_path_tight_after_the_connection` (`tests/data/x06-detour.dsn`, a
  path around a keepout end: trace length 225417 → 223731 board units, work 2353 → 4736
  units, 0 clearance violations, the changed area consumed),
  `the_bundle_routes_the_fixtures_and_pulls_the_grid_path_tight` (4 boards complete, DRC equal
  to the input, no via in a pad, length ≤ `krt-grid`'s), `the_wall_board_stays_clean_under_the_bundle`,
  `registering_the_bundle_changes_no_session_while_it_is_unselected`. Gates, release, default
  settings (optimizer on, 20 passes): `X06_SET=tools/bench/sets/connection-parity.txt
  X06_ARMS=freerouting-router,krt-astar … route_a_set --ignored --exact`: on all 10 boards
  `krt-astar` has drc_new 0 (violations with an item the run created), via_in_pad 0, and the
  routed board's clearance violations equal the input's (TestSensel 9 = 9, CONGFLOCK 8 = 8,
  else 0). Determinism: `tools/determinism-check.sh --out <x06>/det -- env X06_SET=<Issue433,
  Issue649 ecc83> X06_SES_OUT={OUT} <release astar_bundle binary> route_a_set --ignored
  --exact`: `identical` (idle load 5.82, then 8 burners, load 6.40–7.01; both boards fully
  routed; rerun after the fix below: `identical`, loads 4.15 and 4.78). Default sessions: X05's `default_off_sessions_on_a_set` (built-in registry against
  `freerouting_krt::register` with the bundle, default settings) on `connection-parity` (10)
  plus `benchmark`'s first five (Issue433, ecc83, bm02, sonde, bm07): `identical` on all 15;
  `cmp` 0 and `freerouting-dev ses-diff` `equivalent` (exit 0) on every pair. The CLI does not
  link this crate (C07) and no engine file changed, so its sessions are unchanged by
  construction. Not in the checklist: the strict-DRC rollback of Freerouting's router is not
  applied (the grid insert is already a DRC-checked transaction). **A `krt-grid` gate bug
  found and fixed here** (X04/X05 files, X06.3's dry run on bm07: 1 clearance violation after
  the optimizer, 3 without, each a new via 0.06–0.13 mm from an SMD pad of its own net with
  0.2 mm required): with `(attach off)` Java's `Via` obstacle rule keeps a via at clearance from
  its own net's pins and vias, but the raster stamped own pads only at `r_via + h`, and the
  insert's `CheckPolicy::NewItemsOnly` missed it because only the pin's side of
  `clearance_violations` reports a same-net via. Fixed in `raster.rs`
  (`is_own_via_obstacle`, own pads and vias stamped at `c_via + r_via + h`) and `router.rs`
  (`pin_side_violations`: a new via that a nearby pin's check reports aborts the insert,
  `Rejection::Clearance`). Tests (`tests/data/x06-own-pad-via.dsn`):
  `a_via_keeps_its_clearance_from_an_smd_pad_of_its_own_net` (routed at 100 µm, the via at
  x = 11.1 mm, the full DRC empty; with the raster fix undone every step is `Rejected(
  Clearance { first: (P1, via, 0) })`), `the_gate_rejects_a_via_only_the_pads_side_of_the_drc_sees`
  (via cells beside P1 unblocked by a hook: rejected at 100, 50 and 25 µm). After the fix the
  `connection-parity` rows are unchanged and bm07 has 0 violations. Measured at base
  `266a16e`; on the patch rebased onto `3203b9f` (X07, Q02) the release gates were rerun:
  the 20 `connection-parity` rows equal, default-off sessions byte-identical on all 15
  boards (`cmp`), `tools/determinism-check.sh` `identical` (loads 2.51 and 3.03).

- [~] **X06.2 — Region classification.**
  Rust design: `grid::regions::classify(board) -> BTreeMap<ConnectionKey,
  Region>` with the rule above, in integer board units.
  - **Accept:** unit tests on a synthetic QFN and an open two-pin
    connection; on `canary-pwr`, the 17 Q03 pins all fall in `pin-field`.
  *Partial 2026-10-17:* `crates/freerouting-krt/src/grid/regions.rs`: `classify(board) ->
  BTreeMap<ConnectionKey, Region>` (key `(net, lower end item, higher end item)` of each
  input-board airline), with `pin_regions`, `connections` (anchors per end) and `tally`
  (per class: total, still incomplete, unknown on a routed board) for X06.3. The rule in board
  units (`PitchRule::for_board`): a same-component pin at a distinct centre within 0.65 mm,
  or the pad bounding box's smaller side (first layer) under 0.35 mm, or ≥ 32 pins with a
  pitch (smallest centre distance) ≤ 0.8 mm; both distances carry KRT's 1 µm `_DIST_TOL`;
  squared distances in `i128` (`distances_are_exact_at_the_coordinate_limits`, ±`i32`
  corners). An end's pads: the pin itself, or the pins of a trace or via end's connected set
  (pre-routed stubs), none for an area. Unit tests (`cargo test -p freerouting-krt --lib
  grid::regions`, 5 passed): exact 0.65 mm and the tolerance, 0.66 mm open; a 0.3499 mm pad;
  32 pins at 0.8 mm vs 31 pins or 0.81 mm; coincident centres. Synthetic QFN
  (`tests/data/x06-regions.dsn`, `the_synthetic_qfn_is_a_pin_field_and_the_resistors_are_open`):
  all 32 pins of the 0.5 mm QFN U1 and of the 0.8 mm / 0.45 mm-pad QFP U2 `pin-field`, header
  and resistors `open`; nets Q and L `pin-field`, O `open`; tally 2+1 incomplete before routing,
  0 after. Open two-pin (`an_open_two_pin_connection_is_open`, `x05-open-two-pin.dsn`): its one
  connection `open`; X05's 0.4 mm row `pin-field`. **Remainder (operator decision):** on
  `canary-pwr` (`fixtures/canary-pwr/canary-pwr-2026-10-15-1747-unpoured.dsn`, Q03's board;
  `the_q03_pins_of_canary_pwr_by_the_rule`) the rule puts 11 of the 17 Q03 pins in `pin-field`
  (124 of 443 pins overall). Six meet none of its conditions: U2-1 (0.7 × 1.0 mm, 0.677 mm
  from U2-2), U2-8 and U2-12 (0.4 × 1.0 mm, 1.085 mm from their neighbours), U2-10 (2.4 ×
  0.4 mm, 1.275 mm), U7-3 and U7-10 (SOIC-14, 1.27 mm pitch, 0.6 mm pads, 14 pins). Q03 flags
  them for the 1.5 mm Power_High width, not for pitch, so the Accept clause and the rule
  disagree; the test pins the rule's result. Owner: the operator (amend the Accept to 11/17,
  or add a width-relative condition to the rule), then X06.3 uses the amended rule.

- [~] **X06.3 — Measurement by region class.**
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
  *Partial 2026-10-17:* registered, not run: the measurement changes routes, so it waits for
  P04's default-tree decision (P00 README "Order"); owner P04, then this item.
  Preregistration `docs/roadmap/prereg/X06.json` (F03.4 format, within `freerouting-dev
  gate`'s global rules: ≥ 3 boards, N−1, no regression, `via_in_pad` hard): arms `default`
  (`freerouting-router`) and `krt-astar`, default settings; boards X01's five
  (`tools/bench/sets/x01-h2h.txt`, demos graded by `tools/kicad-oracle/grade.sh`, canary-pwr
  by canary-23) and `tools/bench/sets/x06-grid.txt` (ecc83, bm02, sonde, bm07; Rust DRC);
  signal `unrouted_pin_field`, `unrouted_open` (`regions::tally`); guards drc_real/drc_new,
  via_in_pad, total unrouted, vias, length, wall time; the keep rule and the hypothesis rule
  as above. The grader's engine command is the ignored test `route_one_dsn` (`X06_DSN`,
  `X06_SES`, `X06_ALGO`), since the CLI does not register the bundle before C07.
  **Pre-P04 dry run** (informational, not the registered result; this tree, release,
  in-process `route_a_set`, default settings, Rust DRC only, machine load 4–8): drc_new 0 and
  via_in_pad 0 for both arms on all 14 boards. Per board: the class sizes (pin-field / open
  connections), unrouted connections per class, vias, length (mm), seconds:

  | board | classes | default unrouted | krt-astar unrouted | vias d / k | length d / k | s d / k |
  |---|---|---|---|---|---|---|
  | min_fr_test | 0 / 3 | 0 / 0 | 0 / 0 | 4 / 2 | 20.3 / 20.4 | 0.0 / 0.0 |
  | Issue368 | 0 / 3 | 0 / 0 | 0 / 0 | 0 / 0 | 25.7 / 25.7 | 0.0 / 0.0 |
  | Issue110 | 0 / 4 | 0 / 0 | 0 / 0 | 1 / 2 | 134.0 / 132.1 | 0.1 / 0.1 |
  | Issue143 | 0 / 5 | 0 / 0 | 0 / 0 | 4 / 4 | 1064.7 / 1120.5 | 0.6 / 0.3 |
  | Issue413 | 0 / 1 | 0 / 0 | 0 / 0 | 0 / 0 | 1723.8 / 1723.8 | 1.6 / 1.4 |
  | Issue508-SMD | 6 / 0 | 0 / 0 | **2** / 0 | 2 / 4 | 350.1 / 310.5 | 0.1 / 92.1 |
  | TestSensel | 0 / 8 | 0 / 6 | 0 / 6 | 3 / 5 | 34.6 / 56.9 | 0.3 / 14.4 |
  | Issue269-NoVias | 0 / 9 | 0 / 0 | 0 / 0 | 4 / 2 | 245.8 / 230.0 | 0.5 / 0.5 |
  | Issue433 | 0 / 9 | 0 / 0 | 0 / 0 | 3 / 2 | 152.9 / 145.6 | 0.1 / 0.1 |
  | CONGFLOCK | 5 / 5 | 5 / 0 | 5 / 0 | 0 / 0 | 10.2 / 10.2 | 0.1 / 6.5 |
  | ecc83 | 0 / 14 | 0 / 0 | 0 / 0 | 0 / 0 | 163.6 / 158.6 | 0.0 / 0.2 |
  | bm02 | 0 / 34 | 0 / 0 | 0 / 0 | 9 / 6 | 288.9 / 296.3 | 2.0 / 16.7 |
  | sonde | 0 / 20 | 0 / 11 | 0 / **18** | 3 / 3 | 646.7 / 645.4 | 0.3 / 105.0 |
  | bm07 | 39 / 47 | 0 / 0 | 0 / 0 | 26 / 42 | 787.4 / 821.6 | 3.6 / 23.7 |

  Read as the registered rules would read it: only three boards have pin-field connections;
  `krt-astar` loses there on Issue508-SMD and ties on CONGFLOCK and bm07; on open connections
  it loses on sonde and ties on the rest. It wins on no class on any board, so the keep rule
  would reject it, and the hypothesis ("fewer pin-field completions on every board") would
  fail by the two ties. The registered run decides. **Raster share (X04.6):** pass-1
  emulation on the input boards (`raster_share_on_a_set`: window, raster and terminal cells
  against the grid search, each step until a path): 98.5 % on `x06-grid.txt` (1918 against
  30 ms), 20.5 % on `connection-parity` (Issue508-SMD's failing 25 µm searches take 4.65 s).
  Whole `krt-astar` runs (`stage_time_split_on_a_set`, a second timed build of each attempted
  window), raster share of the connection router's time / router share of the run: ecc83
  0.90 / 0.40, bm02 0.06 / 0.18, sonde 0.975 / 0.998 (1938 attempts for 646 connections: 18
  connections fail at all three steps in each of 20 passes; 101.1 of 103.9 s are raster
  builds), bm07 0.25 / 0.24. The raster dominates only where the same connections fail pass
  after pass; where the grid routes, the optimizer and the insert dominate.

- [~] **X06.4 — Record the hybrid follow-up.**
  - **Accept:** a paragraph in algorithms.md ("Grid A* followed by
    pull-tight") states the measured split by region class and whether a
    hybrid (grid for `open` connections, maze search for `pin-field`) is
    worth a checklist, with the numbers that say so.
  *Partial 2026-10-17:* the paragraph is in `docs/roadmap/algorithms.md` ("Grid A* followed
  by pull-tight", **Measured split (X06)**) with the pre-P04 dry run's split (X06.3's note):
  pin-field, a loss on Issue508-SMD (2 of 6 unrouted against 0) and ties on CONGFLOCK and
  bm07; open, ties on 12 of 13 boards and a loss on sonde (18 of 20 against 11). Verdict on
  these numbers: a hybrid is not worth a checklist, since the grid never completes an open
  connection the maze search misses; reopen only if the registered run shows `krt-astar`
  winning on open connections on N−1 boards. Remainder: replace the dry-run numbers with the
  registered run's once X06.3 runs (after P04); owner X06.3.
