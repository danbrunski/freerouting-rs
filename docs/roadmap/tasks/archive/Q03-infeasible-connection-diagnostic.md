# Q03 — Infeasible-connection diagnostic

Opened 2026-10-15. Today a connection that no trace of its net's width can make
is reported as plain "unrouted", the same as one the router gave up on. On
`canary-pwr` there are 17 such pins (canary-09's list, below): each has a
foreign pad closer to its centre than half the 1.5 mm trace width plus the
0.25 mm clearance. This checklist builds a static check that names them:
"cannot route `<net>` to `<pin>` at width W: widest corridor C at (layer, x, y)
between A and B".

The design is KRT's widest-path instrument (`py_placer/placement/reachability.py`,
`slack_field` and `widest_path`; CLI `py_tools/check_reachability.py`, at
`0486e799`). It rasterises a window, computes per cell the widest trace that
fits, and runs a Kruskal union-find in descending order of that width. The
width at which the pin first joins a target is the exact bottleneck for the
raster. It shares no code with the router, so it can't share the router's bugs.

**This is the one Q checklist that starts before parity.** It changes no
routes and needs only the loaded board and its rules, which exist today.
Prerequisites: F02.2 (the `freerouting-dev` crate, for the subcommand);
B05 (`Pin` neck-down half-width) for Q03.5 only. Q06 later turns its result
into the `infeasible` verdict.

**Fixture:** `fixtures/canary-pwr/canary-pwr-2026-10-15-1747-unpoured.dsn` is the board the 17 infeasible pins in this checklist come from (with its net-class bug: VCAP, +12V_RAW and +5V_SHUNT at 0.2 mm).

## What exists and what this adds

| Exists | Used for |
|---|---|
| `BasicBoard::items`, `item`, `tile_shape(item, i)`, `overlapping_objects(shape, layer)` (`crates/freerouting/src/board/basic_board.rs`) | foreign and own items in a window, their convex shapes per layer |
| `rules::clearance_matrix::ClearanceMatrix::get_value(i, j, layer, add_safety_margin)` | the clearance between the routed net's class and each item, per layer |
| `rules::NetClass::trace_half_width(layer)`, `trace_clearance_class()`, `via_rule()`; `rules::ViaRule::get_layer_range`, `rules::ViaInfos` | the required width, the via padstack and its layer span |
| `board::items::AreaKind` (keepout, via keepout, conduction) | keepouts block traces; via keepouts block vias |
| `drc::clearance::parallel_map` (crate-visible, ordered by input index) | per-pin checks on all cores with an ordered merge |
| `io::specctra::ses_reader::read_session`, `drc` | importing Java's routed sessions for Q03.7 |
| `fixtures/reference/route/*.ses` (118 boards, Java at `cda540f`) | Java-routed sessions for the false-positive check |
| Java `Pin.getTraceNeckdownHalfwidth` (`board/model/items/Pin.java:586`): `max(0.5·getMinWidth(layer) − 1, 1)` | the neck width for Q03.5; ported by B05 |
| Java: nothing comparable. `AutorouteUnroutedReport` lists unrouted connections only | — |

What this adds:
- `crates/freerouting/src/quality/feasibility/`: window raster, exact squared
  Euclidean distance transform with nearest-item labels, margin field,
  Kruskal widest path with via edges, and the report;
- `freerouting-dev feasibility <board.dsn> [--json out.json] [--all-pins]
  [--step-um N]`;
- synthetic fixtures reproducing the `canary-pwr` U1 and U7 geometry.

## Decisions made here

- **Every simplification errs toward "passable".** An `infeasible` verdict is
  meant as a proof, up to the raster tolerance. So:
  - clearances come from the matrix with `add_safety_margin = false`;
  - via legality checks only the via pad against foreign copper, keepouts
    and pads (D8: no via in any pad); drill-to-drill rules are ignored;
  - every signal layer in the via's span counts as routable;
  - moves are 8-connected on a layer.
- **Raster tolerance.** With step `s` (default 10 µm in board units), the
  distance error is at most `δ = s·√2`. A pin is `infeasible` only if
  `C + δ < W`. If `C < W ≤ C + δ` it is `marginal`. Otherwise `passable`.
- **Fixed raster origin.** Cell (i, j) has its centre at `(i·s, j·s)` in board
  coordinates, independent of the window, so overlapping windows agree.
- **Window and targets.** The window is the pin's bounding box enlarged by
  `R_w = max(3 mm, 4·(W/2 + c_max))`, where `c_max` is the largest clearance in
  the window's layers. Targets are own-net copper other than the pin, plus
  every cell on the window border. A pin that cannot reach either is walled
  in: any real route must end on own copper in the window or leave it. This
  avoids KRT's measured bug of claiming "caged" when no other copper of the
  net is in view.
- **Margin, not ratio.** For each cell `margin(p) = 2·D(p) − req(p)` in board
  units, floored to `i64`. `D(p) = min_k(dist_k(p) − c_k)` over clearance
  groups k. `req(p)` is the class width `W(layer)`, or the neck width inside the
  neck radius (Q03.5). Own-net cells have `margin = i64::MAX`. The Kruskal
  works on margins, so a per-cell required width costs nothing extra.
- **Report only.** The result is a value type; nothing in routing reads it.
  Q06 attaches it to incomplete connections; the GUI (G series) shows it.
- **No new dependency.** The squared EDT is Felzenszwalb–Huttenlocher's
  lower-envelope algorithm, about 150 lines.

## File ownership

| Item | New files | May edit |
|---|---|---|
| Q03.1 | `crates/freerouting/src/quality/mod.rs`, `quality/feasibility/mod.rs`, `quality/feasibility/raster.rs` | `crates/freerouting/src/lib.rs` (append `pub mod quality;`) |
| Q03.2 | `quality/feasibility/edt.rs` | — |
| Q03.3 | `quality/feasibility/widest_path.rs` | — |
| Q03.4 | `quality/feasibility/report.rs`, `crates/freerouting-dev/src/feasibility.rs` | `crates/freerouting-dev/src/main.rs` (append one subcommand) |
| Q03.5 | `quality/feasibility/neck.rs` | — |
| Q03.6 | `fixtures/quality/q03-qfn-0p5.dsn`, `fixtures/quality/q03-soic-boundary.dsn`, `crates/freerouting/tests/quality_feasibility.rs` | — |
| Q03.7 | `crates/freerouting/tests/quality_feasibility_java.rs` | — |
| Q03.8 | — | `crates/freerouting/tests/quality_feasibility.rs` |

## Checklist

- [x] **Q03.1 — Window raster with clearance groups.**
  Source: KRT `reachability.slack_field` (rasterises foreign copper per
  clearance class at `step=0.01` mm; rule-area keepouts as obstacles).
  Rust design, `quality::feasibility::raster`:
  - `struct Frame { step: i32, i0: i32, j0: i32, nx: usize, ny: usize,
    layers: Vec<i32> }` with cells in one `Vec` per layer, row-major
    (`index = (layer_slot·ny + j)·nx + i`).
  - `struct Window { frame, groups: Vec<Group>, own, start, via_blocked }`,
    where the last three are per-cell bitsets in `Vec<u64>`. A `Group` holds
    one clearance value `c_k` (board units) and a source mask per layer, plus
    a per-cell source label: the lowest `ItemId` whose shape covers the cell
    centre.
  - For each layer and each item overlapping the window: an item of the pin's
    net goes into `own`; the pin's own shapes go into `start`; any other item,
    including net-less pads and keepouts, goes into the group of
    `get_value(net_class.trace_clearance_class(), item.clearance_class, layer,
    false)`. Via keepouts and pads (any net) go into `via_blocked` for their
    layers.
  - Scan conversion: a cell is a source if its centre lies inside the convex
    `TileShape` (test the cells of the shape's bounding box). A shape covering
    no cell centre marks the cell containing its first corner, so thin items
    never vanish.
  - **Orders that decide results:** items visited in ascending `ItemId`;
    groups sorted by `c_k` ascending; a cell's label is the lowest `ItemId`.
  - **Accept:** unit tests in the module: a 0.25 × 0.8 mm pad at a 10 µm step
    covers exactly the expected cell count; a 5 µm wide shape still marks one
    cell; two items with different clearance classes land in two groups;
    a via keepout blocks vias only.
  *Done 2026-10-15:* `crates/freerouting/src/quality/feasibility/raster.rs`
  (`Frame`, `Window`, `Group`, `Bits`, `build_window`, `rasterize`). Unit tests,
  `cargo test -p freerouting --lib quality` (15 pass): a 0.25 × 0.8 mm pad
  covers 25 × 80 = 2000 cells (`pad_covers_the_expected_cell_count`); a
  5 × 7 µm shape marks the cell of its first corner (≤ 4 cells) and a
  zero-width line a run of 101 (`thin_shape_still_marks_a_cell`); two pads of
  classes with clearances 200 and 600 land in two groups, sorted ascending
  (`items_with_different_clearance_classes_land_in_two_groups`); a
  `via_keepout` cell is in `via_blocked` and in no group, pads of any net are
  via-blocked, B.Cu is not (`via_keepout_blocks_vias_only`). Additions to the
  design: a shape that covers no cell centre marks every cell whose square it
  touches (so a long thin outline segment stays a wall); pins and vias use their
  true pad shapes (a round pad is a disk), other items the shapes of an
  uncompensated plain tree; own traces narrower than the class width are
  ignored (a connection at the class width can't continue on them; this is what
  makes U2-10, which has a 0.4 mm pre-route, infeasible); a group is keyed by
  (trace clearance, clearance of each via of the rule), so the via field uses
  the via's own clearance.

- [x] **Q03.2 — Exact squared EDT with nearest-item labels.**
  Source: KRT uses `scipy.ndimage.distance_transform_edt`; this is
  Felzenszwalb and Huttenlocher, "Distance Transforms of Sampled Functions"
  (2012), applied by rows then columns.
  Rust design, `quality::feasibility::edt`: `fn squared_edt(mask: &[bool], nx,
  ny) -> (Vec<i64>, Vec<u32>)` returning the squared distance in cell units
  and the index of the nearest source cell; `i64` throughout, no floats.
  `dist_k(p) = sqrt(d²)·step` is computed once per cell and group as `f64`.
  - **Orders that decide results:** among equidistant sources the lower
    envelope keeps the first parabola in index order; the label is the
    source cell's `ItemId`. Only `d²` is compared exactly in tests; any
    source at that distance is a valid label.
  - **Accept:** a test compares `d²` with brute force on 200 random 64 × 48
    masks (fixed `JavaRandom` seed) for exact equality, and checks that every
    returned source lies at that distance; an all-empty mask returns
    `i64::MAX`.
  *Done 2026-10-15:* `quality/feasibility/edt.rs` (`squared_edt`,
  `squared_edt_by`): Felzenszwalb–Huttenlocher with the envelope boundaries kept
  as exact fractions (`i128` cross-multiplication), so ties are exact.
  `matches_brute_force_on_random_masks`: 200 random 64 × 48 masks
  (`JavaRandom::new(20261015)`, densities 0.1–6 %) equal brute force exactly and
  every label is a source at that distance; `empty_mask_is_infinite` (`INF` =
  `i64::MAX`, `NO_SOURCE`); `single_source_and_ties` (first parabola in index
  order wins).

- [x] **Q03.3 — Margin field and Kruskal widest path with via edges.**
  Source: KRT `reachability.widest_path` (cells added in descending slack
  order under union-find; via edges join layers only where a via is legal;
  `return_throat=True`).
  Rust design, `quality::feasibility::widest_path`:
  - `margin(p)` as in the decisions. The via field uses the same `dist_k` with
    the via's clearance: `via_ok(x, y)` holds when, on every layer of the via's
    span, `min_k(dist_k − c_via_k) ≥ via pad radius` and no `via_blocked` cell
    is set.
  - Sort all cells by `(margin desc, layer slot asc, j asc, i asc)`. Start
    cells and own cells are active from the beginning.
  - Disjoint-set union with union by size and path halving; each root keeps
    `has_start` and `has_target` flags. Activating a cell unions it with its
    active 8-neighbours on the same layer and, where `via_ok`, with the active
    cells at the same `(i, j)` on the other layers of the via span.
  - Stop when a root has both flags. The activating cell is the throat;
    `C = margin(throat) + req(throat)`. If the queue empties first, `C` is
    negative infinity (fully enclosed).
  - **Orders that decide results:** the sort key above is a total order;
    neighbours are visited in a fixed order (E, NE, N, NW, W, SW, S, SE, then
    layers ascending).
  - **Accept:** synthetic tests: a straight corridor of width 1.0 mm between
    two foreign pads gives `C` within `δ` of `1.0 − 2c`; a pin enclosed by a
    ring of foreign copper is `infeasible` at every width; the same pin with a
    legal via site inside the ring and an open layer below is `passable`; the
    throat lies in the corridor.
  *Done 2026-10-15:* `quality/feasibility/widest_path.rs` (`field`,
  `margins`, `widest_path`, `Requirement`, `ViaSpan`, `PathOutcome`). Tests:
  `corridor_width_is_found_within_delta` (1.0 mm corridor, c = 0.1 mm: C = 0.800
  mm, |C − (1.0 − 2c)| ≤ δ, throat inside the corridor, labels the two pads);
  `enclosed_pin_is_infeasible_at_every_width` (widths 1, 100, 1000 units: C =
  −2c, infeasible); `via_inside_the_ring_opens_the_layer_below` (passable with
  C ≥ W; with the ring's inside via-blocked it is infeasible again);
  `own_copper_touching_the_pin_is_connected`. Note: every cell, obstacle or not,
  is eventually activated, so a pin behind copper gets a negative C (how far it
  got) rather than −∞; `Enclosed` is left for a window with no target at all.
  A net with a plane counts every legal via site as a target.

- [x] **Q03.4 — Candidates, verdicts and the report.**
  Rust design, `quality::feasibility::{mod, report}`:
  - `pub fn check_board(board: &BasicBoard, opts: &FeasibilityOptions) ->
    FeasibilityReport`. Candidates are the pins of nets whose class width on
    one of the pin's layers exceeds the pin's minimum width on that layer
    (Java's neck-down condition), plus every pin with `opts.all_pins`.
    Candidates run through `parallel_map` in ascending `ItemId`.
  - Per pin: net, component and pin name, `ItemId`, layers, required width
    per layer, `C`, `margin`, verdict, throat (layer name, board units and
    mm), step, window, and up to two `near` items: the throat's label, and the
    nearest different label among the throat's neighbours whose direction is
    more than 90° from the first. Each `near` entry has id, kind, net, and
    component and pin name where it has one.
  - Text line for each infeasible pin: `cannot route <net> to <component>-<pin>
    at width <W> mm: widest corridor <C> mm at (<layer>, <x>, <y>) between
    <A> and <B>`.
  - JSON with `"schema": "freerouting-feasibility/1"`, the board file's sha256
    and the options. Pins sorted by `ItemId`.
  - `freerouting-dev feasibility` prints the text and writes the JSON. Exit
    code 0 whatever the verdicts; 2 on a load error.
  - **Accept:** a golden JSON test on `q03-qfn-0p5.dsn`; the subcommand runs on
    `Issue145-smoothieboard` and prints the candidate count and time.
  *Done 2026-10-15:* `quality/feasibility/{mod,report}.rs` (`check_board`,
  `check_pin`, `candidate_pins`, `FeasibilityReport::{to_text,to_json}`,
  `sha256_hex`), `crates/freerouting-dev/src/feasibility.rs` and the
  `feasibility` subcommand in `main.rs` (`--json`, `--all-pins`, `--step-um`,
  plus `--start shape|centre`, see Q03.6). Golden test
  `qfn_fixture_json_matches_golden` against
  `crates/freerouting/tests/data/q03-qfn-0p5.feasibility.json`
  (`FEASIBILITY_UPDATE_GOLDEN=1` rewrites it); `sha256_known_vectors` checks the
  digest. `freerouting-dev feasibility fixtures/Issue145-smoothieboard.dsn`
  prints `0 candidate pins` and `0 candidates checked in 0.04 s` (exit 0; no pin
  there is narrower than its class width); with `--all-pins`: `1017 candidate
  pins: 0 infeasible, 0 marginal, 1017 passable`, 27.8–45.4 s release on a busy
  machine (load 16–28). Candidates also need a net with at least two
  connectable items (a one-pin net has nothing to route).

- [x] **Q03.5 — Neck ladder: how long a neck would make it routable.**
  For each `infeasible` pin, recompute with `req(p)` equal to the neck width
  `2·trace_neckdown_half_width(layer)` (B05's port of
  `Pin.getTraceNeckdownHalfwidth`) within radius R of the pin centre, and `W`
  outside. R runs through 0.25, 0.5, 1, 2 and 4 mm. Report `neck_needed`: the
  smallest R that gives `passable`, or `none`. This is the input Q02 needs to
  choose its distance D.
  - **Accept:** on `q03-qfn-0p5.dsn`, the QFN pin needs a neck of at most the
    pad length plus 0.5 mm; on an enclosed pin `neck_needed` is `none`. If B05
    has not landed, mark this item `[~]` with that remainder.
  *Partial 2026-10-15:* `quality/feasibility/neck.rs` (`neck_ladder`,
  `NECK_RADII_MM`). Both clauses hold: `qfn_neck_ladder` (U1-5 of
  `q03-qfn-0p5.dsn`: neck 2·1249 units, `neck_needed` 1.0 mm ≤ 0.7 + 0.5 mm; the
  neck reaches a legal via site within 1 mm and B.Cu is open) and
  `enclosed_pin_has_no_neck` (keepout ring on both layers: 5 rungs, `none`). The
  neck is floored at the board's minimum class width (D8) and capped at W.
  Remainder (B05.4): B05 has not landed, so `pin_min_width`, `pin_max_width` and
  `trace_neckdown_half_width` are computed in `neck.rs` from the padstack the
  way Java's `Pin.getMinWidth`/`getTraceNeckdownHalfwidth` do
  (`Pin.java:567-590`, with `getPadstackLayer`); replace them with B05's
  `board/pin_exits.rs` functions when they land, which also gives them the
  pin-exits probe check.
  *Done 2026-10-16:* the remainder landed with B05.4: `neck.rs`'s local
  `pin_min_width`, `pin_max_width`, `trace_neckdown_half_width` and
  `padstack_box_width` are deleted; `neck_ladder` calls
  `BasicBoard::trace_neckdown_half_width` and `feasibility/mod.rs` calls
  `BasicBoard::pin_min_width` (`board/pin_exits.rs`), which
  `board_probe_pin_exits` compares with Java on every pin and layer of 4
  boards. `cargo test -p freerouting --test quality_feasibility`: 6 passed,
  1 ignored (`qfn_neck_ladder`, `enclosed_pin_has_no_neck` among the passed).

- [x] **Q03.6 — `canary-pwr` and the synthetic fixtures.**
  Expected, from canary-09 (`canary-pwr-q03-cases`), in the 17:46 copy
  `~/code/canary/hardware/canary-pwr-rev1.0/freerouting-rs/canary-pwr.dsn`
  (Power_High and Switch_Node are 1.5 mm, clearance 0.25 mm):
  - U1 (QFN-38, 0.5 mm pitch, 0.25 mm pads): pins 22, 24, 30, 31, 34;
  - U2 (VQFN-HR): pins 1, 8, 10, 12;
  - U3 (0.5 mm pitch): pins 8, 10;
  - U7 (SOIC-14, 1.27 mm pitch, 0.6 mm pads): pins 3, 10, the boundary case:
    neighbour edge 0.97 mm from the centre against 1.0 mm needed;
  - Q2: pin 3; Q3: pins 1, 2, 3.

  That is 17 pins. U1 pin 21 (VCAP), U3 pin 9 and Q7 pin 3 are in the Default
  class in this copy, so they must **not** be infeasible at 1.5 mm; in DSNs
  exported after canary `f5d3989` U1 pin 21 is Power_High and joins the list.
  The board stays out of the repo (`owner.txt`).
  Commit two synthetic fixtures with the same geometry:
  `q03-qfn-0p5.dsn` (a 0.5 mm pitch QFN row and a 1.5 mm net) and
  `q03-soic-boundary.dsn` (1.27 mm pitch, 0.6 mm pads, the 1.5 mm net fails by
  0.06 mm of width, and a 1.40 mm class on a second net passes).
  - **Accept:**
    - all 17 pins are `infeasible`; the three Default-class pins are not;
    - U7 pin 3 reports `C` within 0.02 mm of the hand value
      `2·(0.97 − 0.25) = 1.44` mm, and one U1 pin within 0.02 mm of a hand
      value computed from its DSN pad geometry and clearance (the derivation
      goes in the done note);
    - every other pin flagged on the board is listed in the done note with a
      hand check of at least three of them (HANDOVER names small passives
      around U1, U2, U5 and U7);
    - the synthetic fixtures give the same verdicts in a committed test.
  *Done 2026-10-15:* fixtures `crates/freerouting/tests/data/quality/q03-qfn-0p5.dsn`
  and `q03-soic-boundary.dsn` (not `fixtures/quality/`: `tests/reference_load.rs`
  and `dsn_support` load every DSN under `fixtures/` and need a Java load
  reference for each, which only a C01/J-series run may add); tests in `crates/freerouting/tests/quality_feasibility.rs`
  (`qfn_pin_is_infeasible_with_the_hand_value`, `soic_boundary_case`, and the
  ignored `canary_pwr_infeasible_pins`, 60 s dev profile, run with
  `--include-ignored`). Synthetic: QFN U1-5 infeasible, C = 0.260 mm (hand
  2·(0.375 − 0.25) = 0.250, within δ = 0.014 mm + the raster's +s); SOIC U7-3
  infeasible, C = 1.440 (hand 1.44), the 1.40 mm net passable.
  **The start model decides the 17.** With `--start centre` (a trace must reach
  the pin centre, canary-09's model) all 17 pins are infeasible and U7-3 gives
  C = 1.440 mm = 2·(0.97 − 0.25). With the default `--start shape` (the trace
  may end anywhere on the pin, as the design above says) 15 of the 17 are
  infeasible: Q3-1 and Q3-2 are passable (C = 1.90 mm: a 1.5 mm trace fits at
  their outer ends beside Q3-2/Q3-1 of the same net), and U7-3 gives C = 1.466 mm
  against the shape hand value 2·(√(0.15² + 1.12²) − 0.15 − 0.25) = 1.460 mm
  (the trace end moves to the pad end, where the 0.15 mm corner rounding of the
  neighbours recedes). The centre model is not a proof against Java (Q03.7: 4
  hard false positives), so the default stays `shape`. U1-21, U3-9 and Q7-3 are
  not infeasible in either model (they are not candidates: Default class). U1
  hand check: U1-6 (`/Holdup/LTC_CAPFB`, Power class 0.5 mm, c = 0.2 mm; 0.25 mm
  pads at 0.5 mm pitch, neighbour edges 0.375 mm from the pad axis):
  2·(0.375 − 0.2) = 0.350 mm, reported 0.36 mm.
  Every other pin the default model flags (25): U6-2 (GND, W 0.5, C 0.46, U6-3 /
  trace 2006); R3-1 (+12V_PROT, 1.5, 0.60, traces 2040/1595); D5-2 (+12V_PROT,
  1.5, −0.08, D6-1 / trace 1567); D6-2 (+12V_BUS, 1.5, 0.90, trace 1567 / via
  2422); C27-1 (+12V_BUS, 1.5, 1.40, trace 1970 / C29-2); U3-7 (GND, 0.5, 0.26,
  U3-6/U3-8); C23-1 (+5V_BUCK, 1.5, 0.20, R9-2/C23-2); Q2-2 and Q2-1 (+12V_PROT,
  1.5, 1.32, Q2-5/C4-2); U2-3 (GND, 0.5, 0.42, U2-2/U2-4); C41-1 (+12V_PROT, 1.5,
  0.90, trace 2389); U4-4 (+5V_PI, 1.5, 0.07, U4-5 / trace 1865); C12-1
  (+12V_BUS, 1.5, −0.02, R8-1 / trace 1622); C5-2 (LTC_VOUTSP, 1.5, 0.65,
  C5-1/C4-1); C29-1 (+12V_BUS, 1.5, 1.40, trace 1970 / C29-2); R11-1 (+12V_PROT,
  1.5, 0.78, R11-2 / trace 1626); U1-16, U1-15 (LTC_CAP4/3, 0.5, 0.02, U1-17 or
  U1-16 / U1-39); U1-14 (LTC_CAP2, 0.5, 0.19, via 2426 / U1-13); U1-12
  (LTC_CAPRTN, 0.5, 0.21, via 2424 / trace 1672); U1-8 (GND, 0.5, 0.36,
  U1-9/U1-7); U1-6 (above); R25-1 and R22-1 (+12V_BUS/+5V_BUCK, 1.5, 0.16,
  C25-2/Q6-1); R9-1 (+12V_BUS, 1.5, 1.20, R9-2/R13-1). The board carries 899
  pre-routed wires, so many throats are against existing traces. Hand checks:
  U1-6 (above); U3-7 (TSSOP-10, 0.3 mm pads at 0.5 mm pitch, between U3-6 at
  c = 0.2 and U3-8 (+5V_PI, Power_High) at c = 0.25: edge-to-edge 0.7 mm, so
  C = 0.7 − 0.2 − 0.25 = 0.25 mm, reported 0.26); C23-1 (throat between R9-2,
  R0603 at (126.2, −63.9) rotated 90°, top edge y = −62.675, and C23-2, C0603 at
  (125.775, −61.5), bottom edge y = −61.975: 0.70 − 2·0.25 = 0.20 mm, reported
  0.20). Start model decided (orchestrator, under Dan's standing instruction to take
  the recommended answer, 2026-10-15): the default stays `--start shape` (sound against
  Java, Q03.7's 0 hard false positives); canary-09's "all 17" holds under
  `--start centre` (17/17, U7-3 = 1.440 mm) and the default flags 15/17 (Q3-1 and
  Q3-2 are passable at their outer ends beside a same-net pin). Every clause holds
  under the centre model.

- [x] **Q03.7 — Zero hard false positives against Java's routes.**
  For every fixture with a `fixtures/reference/route/<board>.ses`, run the
  check on the unrouted board, then import Java's session into a copy. A hard
  false positive is a pin reported `infeasible` at W on which Java's routed
  board has a trace of half-width ≥ W/2 whose end point lies inside the pin's
  shape on that layer, with no clearance violation involving that trace (Rust
  DRC). Also report the anti-vacuity counts:
  - H: the number of pins reported `infeasible`;
  - N: how many of those Java connected at all (with a narrower trace).
  The target of 0 hard false positives is preregistered (F03.4) before the run.
  - **Accept:** 0 hard false positives over all reference boards; H and N per
    board in the done note; the test is `#[ignore]`d in CI if it takes more
    than 60 s, and the done note gives the command and time.
  *Done 2026-10-15:* `crates/freerouting/tests/quality_feasibility_java.rs`
  (`no_hard_false_positives_against_java_routes`, `#[ignore]`d: 104 s in
  release for both start models). Command: `cargo test --release -p freerouting
  --test quality_feasibility_java -- --ignored --nocapture`. Default model
  (`shape`): 116 boards (Issue006 does not load), **0 hard false positives**;
  H and N are 0 on every board except `Issue684-Autorouter_PCB1_2026-5-8`: H = 33,
  N = 31. 56 Java trace ends lie inside those pins, 38 at the pin centre.
  Disclosure: F03.4's preregistration file does not exist yet; the target 0 is
  the one stated here. The first full run used the centre model, which I had
  made the default for Q03.6: H = 67, N = 63 on Issue684 and 4 hard false
  positives (u1-22e7, u1-19e21, u1-19e20, u1-19e17: full-width 0.4 mm traces
  ending 0.057–0.064 mm off the pin centre, no clearance violation). The default
  then went back to this checklist's design (the pin's shapes are the start);
  the test still prints the centre model's counts for comparison. The routes
  are Java at `cda540f` (`fd834c7`); rerun after a re-pin regenerates them.

- [x] **Q03.8 — Determinism and speed.**
  - **Accept:**
    - two runs on `canary-pwr` and on `Issue145-smoothieboard` give
      byte-identical JSON;
    - a test runs the per-pin checks with `parallel_map` and sequentially and
      gets equal reports;
    - release-build time on `canary-pwr` is recorded in the done note; above
      10 s, profile and record where the time goes before closing.
  *Done 2026-10-15:* byte-identical JSON: three runs of
  `freerouting-dev feasibility fixtures/canary-pwr/canary-pwr-2026-10-15-1747-unpoured.dsn --json …`
  (`cmp` equal) and two of `Issue145-smoothieboard` with and without
  `--all-pins` (`cmp` equal). `parallel_and_sequential_reports_are_equal`
  (both synthetic fixtures with `--all-pins`) and `canary_pwr_infeasible_pins`
  run the per-pin checks through `parallel_map` and sequentially and compare
  the reports. Release time on canary-pwr (75 candidates, default model):
  5.82, 6.24, 6.25 s at load 16–19 (not a quiet machine; 5.61 s at the first
  run), under 10 s, so no profile.
