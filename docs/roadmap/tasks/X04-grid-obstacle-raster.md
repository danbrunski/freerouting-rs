# X04 — Obstacle raster from the board model

Opened 2026-10-15. KRT's grid A* (vendored in X03) searches cells and knows
nothing about geometry. In KRT, about 22,000 lines of Python build its raster:
every foreign item inflated by clearance and the routed half-width
(`obstacle_cache.precompute_net_obstacles`,
`obstacle_map.build_base_obstacle_map`, `routing_utils.py`), a separate via
map, keepouts, and terminal exemptions. This checklist writes the
freerouting-rs equivalent: board items → a conservative raster for one
connection, in a new GPL crate, `crates/freerouting-krt`.

"Conservative" is the property everything else rests on: a move between two
adjacent free cells never comes closer to a foreign item than the clearance
plus the half-width. The exact DRC (X05) is still the judge; the raster only
has to never invite a violation.

Prerequisites: X03; F04. No board-model change is needed: item shapes
(`BasicBoard::tile_shape`), `Shape::distance` and the clearance matrix exist.

## What exists and what this adds

| Exists | Used for |
|---|---|
| `krt-grid-router::GridObstacleMap` (X03): per-layer refcounted blocked cells, a via-blocked map, static keep-out bitmap, source/target and allowed cells, batch add/remove from slices | the raster store |
| `BasicBoard::items`, `tile_shape`, `overlapping_objects` (`board/basic_board.rs`); `freerouting_geometry::Shape::distance(FloatPoint)` | item shapes and point-to-shape distance |
| `ClearanceMatrix::get_value(i, j, layer, add_safety_margin)`; `NetClass::trace_half_width`, `trace_clearance_class`, `via_rule`; `ViaInfos` | clearances, widths and via padstacks |
| `AreaKind::{Obstacle, ViaObstacle, ComponentObstacle, Conduction}` | keepouts |
| KRT: inflation by `own_half + clearance` for segments, `track_w/2 + clearance` for pads, via keep-out by `via_size/2 + clearance` plus half a grid step at pads; default grid 0.1 mm (`routing_defaults.GRID_STEP`), 0.05 mm for taps, 0.025 mm for rescue | the design |
| KRT #806: refcounts balanced while 30,862 of 47,448 committed cells were not blocked (stale cache entries) | why X04 rebuilds per connection |

## Decisions made here

- **Crate:** `crates/freerouting-krt`, `license = "GPL-3.0-only AND MIT"`,
  `publish = false`, edition and `rust-version` from the workspace;
  dependencies `freerouting` and `krt-grid-router`. `lib.rs` exposes
  `pub fn register(registry: &mut StageRegistry)`. X02.1 creates the crate if
  it runs first; then X04.1 only adds the `krt-grid-router` dependency and
  the `grid` module.
- **One raster per connection, built from scratch.** The window is the
  bounding box of the connection's start and destination items, enlarged by
  5 mm and clipped to the board outline's bounding box. Building per
  connection is correct by construction (no stale stamps). Incremental stamps
  come only if X06 measures raster building above 30 % of the stage's time
  (X04.6), and then with an audit.
- **Fixed grid origin:** cell (i, j) has its centre at `(i·s, j·s)` in board
  units, so windows agree where they overlap. Step `s` defaults to 0.1 mm in
  board units (setting `router.krt.grid_step_um`, default 100).
- **Conservative test.** With `h = ceil(s·√2 / 2)` board units, a cell is
  blocked on layer L for a trace if
  `item_shape.distance(centre) < c + hw + h`, where `c` is the clearance
  between the routed net's trace clearance class and the item's class on L
  (`add_safety_margin = true`, as the router uses) and `hw` the net's
  half-width on L. Any point of a move between adjacent free centres is
  within `h` of one of them, so it is at least `c + hw` from the item.
- **Vias.** A cell is via-blocked if, on any layer of the via's span,
  `distance(centre) < c_via + r_via(L) + h`; and it is via-blocked if its
  centre lies inside any pad of any net (D8: no via in a pad), with KRT's
  extra half step around pads.
- **Keepouts:** `Obstacle` blocks traces and vias; `ViaObstacle` blocks vias;
  `ComponentObstacle` blocks nothing; cells outside the board outline are
  static-blocked. Conduction areas of other nets are obstacles only where
  the board says so (`Item::is_obstacle_for_net`), and Q01's policy applies
  through the same call.
- **Own-net items are not obstacles.** Cells inside the connection's start
  items are sources; inside its destination items, targets. Cells inside
  other own-net items are free.

## File ownership

| Item | New files | May edit |
|---|---|---|
| X04.1 | `crates/freerouting-krt/{Cargo.toml, README.md, src/lib.rs}` if absent; `src/grid/mod.rs` | `crates/freerouting-krt/Cargo.toml`, `src/lib.rs` (if X02.1 created them) |
| X04.2 | `src/grid/frame.rs` | — |
| X04.3 | `src/grid/raster.rs` | — |
| X04.4 | — | `src/grid/raster.rs` |
| X04.5 | `src/grid/terminals.rs` | — |
| X04.6 | `src/grid/stamps.rs`, `src/grid/audit.rs` (only if triggered) | — |
| X04.7 | `crates/freerouting-krt/tests/raster.rs` | — |

## Checklist

- [ ] **X04.1 — Crate skeleton and provenance.**
  - **Accept:** the gate passes with the new crate; its `README.md` states the
    licence split (GPL-3.0-only for code written here, MIT for files carrying
    KRT code, which have the F04 header); F04's CI check passes.

- [ ] **X04.2 — Frame and window.**
  Rust design: `grid::frame::Frame { step, i0, j0, nx, ny, layers }` with
  `fn cell_of(Point) -> (i32, i32)`, `fn centre(i, j) -> Point`, all integer.
  `fn window_for(board, task) -> Frame`.
  - **Accept:** unit tests: windows of two overlapping connections give the
    same centre for a shared cell; a point exactly on a cell boundary maps to
    the cell with the lower index.

- [ ] **X04.3 — Trace obstacles, keepouts and outline.**
  Source: KRT `obstacle_map.build_base_obstacle_map`,
  `add_rule_area_keepout_obstacles`, `add_board_edge_obstacles`.
  Rust design: `grid::raster::build(board, task, frame) -> GridObstacleMap`.
  For each layer, items overlapping the window in ascending `ItemId`; for each
  item's tile shapes, test the cells of the shape's bounding box enlarged by
  `c + hw + h`; collect blocked cells into a sorted `Vec<[i32; 3]>` and add
  them with the vendored batch call.
  - **Orders that decide results:** items ascending by id, cells in
    `(layer, j, i)` order; the vendored map's result doesn't depend on insertion
    order (refcounts), but the slice is sorted anyway.
  - **Accept:**
    - unit tests on a hand-built board: a pad's blocked region has the
      expected cell count at 0.1 mm; a keepout blocks traces and vias; a via
      keepout blocks only vias; cells outside the outline are blocked;
    - `Shape::distance` agrees with a brute-force point-to-edge distance on
      1,000 random points around random tile shapes (fixed seed) to within
      1 board unit. It is unverified whether it is exact Euclidean for every
      shape kind; if it isn't, the raster uses the brute-force distance and
      the done note says so.

- [ ] **X04.4 — Via map.**
  As in the decisions, using the net's via rule: the first via info in the
  rule whose padstack spans the layers the path needs (for a two-layer
  board, the through via).
  - **Accept:** a via cell is never inside any pad (test over every pad of
    `Issue145-smoothieboard`); a cell next to a pad is via-blocked when the
    via ring would come within clearance.

- [ ] **X04.5 — Terminals.**
  Source: KRT `add_source_target_cell`, `add_allowed_rect`.
  Rust design: `grid::terminals::cells(board, frame, items) -> Vec<[i32; 3]>`:
  the free cells whose centre lies inside the item's shape on its layers. If
  an item has none at this step, the result is empty and X05 retries at a
  finer step.
  - **Accept:** a 0.25 mm QFN pad at 0.1 mm step has at least one source
    cell; a 0.05 mm sliver pad has none and is reported.

- [ ] **X04.6 — Incremental stamps (only if X06 measures the need).**
  Source: KRT `obstacle_cache.py` (`NetObstacleData`, refcounted per-net
  stamps) and its audits (`run_obstacle_audit`, `run_obstacle_content_audit`,
  #309 and #806).
  If triggered: a per-item stamp cache keyed by (item id, layer, clearance
  class, half-width), updated from B01's change journal, and an audit that
  compares the live raster with a fresh build **cell by cell**.
  - **Accept:** if not triggered, this item is closed with X06's timing
    numbers. If triggered: after 1,000 random insert/delete operations on a
    `benchmark` board, the audit finds no difference; a seeded fault (one
    skipped removal) is reported with its item id and layer.

- [ ] **X04.7 — The conservative property, checked exactly.**
  - **Accept:**
    - on `Issue145-smoothieboard` and `canary-pwr`, for 10,000 free cells
      sampled with a fixed `JavaRandom` seed, the exact `Shape::distance` to
      every foreign item on that layer is at least `c + hw`; for 10,000
      sampled adjacent free pairs, the segment between their centres is at
      least `c + hw` from every foreign item (computed with the geometry
      crate's segment-to-shape distance, or by sampling the segment every
      `s/8`);
    - build time per connection (median and maximum) on those two boards is
      in the done note.
