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

- [ ] **Q03.1 — Window raster with clearance groups.**
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

- [ ] **Q03.2 — Exact squared EDT with nearest-item labels.**
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

- [ ] **Q03.3 — Margin field and Kruskal widest path with via edges.**
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

- [ ] **Q03.4 — Candidates, verdicts and the report.**
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

- [ ] **Q03.5 — Neck ladder: how long a neck would make it routable.**
  For each `infeasible` pin, recompute with `req(p)` equal to the neck width
  `2·trace_neckdown_half_width(layer)` (B05's port of
  `Pin.getTraceNeckdownHalfwidth`) within radius R of the pin centre, and `W`
  outside. R runs through 0.25, 0.5, 1, 2 and 4 mm. Report `neck_needed`: the
  smallest R that gives `passable`, or `none`. This is the input Q02 needs to
  choose its distance D.
  - **Accept:** on `q03-qfn-0p5.dsn`, the QFN pin needs a neck of at most the
    pad length plus 0.5 mm; on an enclosed pin `neck_needed` is `none`. If B05
    has not landed, mark this item `[~]` with that remainder.

- [ ] **Q03.6 — `canary-pwr` and the synthetic fixtures.**
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

- [ ] **Q03.7 — Zero hard false positives against Java's routes.**
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

- [ ] **Q03.8 — Determinism and speed.**
  - **Accept:**
    - two runs on `canary-pwr` and on `Issue145-smoothieboard` give
      byte-identical JSON;
    - a test runs the per-pin checks with `parallel_map` and sequentially and
      gets equal reports;
    - release-build time on `canary-pwr` is recorded in the done note; above
      10 s, profile and record where the time goes before closing.
