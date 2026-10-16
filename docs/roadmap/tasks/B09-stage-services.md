# B09 — Board services for algorithm stages

Opened 2026-10-15. Decision D9 in [port-completion.md](../port-completion.md)
makes the engine a pipeline of interchangeable stages. Every implementation
works on the one board model, and the X series ports algorithms from
KiCadRoutingTools (KRT) as extra stages. KRT's stages lean on services its grid
gives it for free. On a shape-based board each is a deliberate API (KRT report
§11.1). Most of those services belong to other series:
- the search failure record and the cost hook are maze-search design (A04);
- snapshots are B01;
- the rules the DSN lacks come from the KiCad reader (K05–K07).

This checklist holds the four that are board-model work: an insert, check,
commit-or-revert transaction; a per-net view of clearance-inflated obstacles;
a fill model for conduction areas with its connectivity; and a per-net
obstacle hook for conduction areas. None of them changes a Freerouting result:
each is unused, or `None`, on the parity path. They run when the first X or Q checklist needs
them. B00 lists the services left to other series.

**Prerequisites:** B01.3 and B01.5 (snapshots and the state hash) for B09.1;
B02.1 for B09.2; B04.3 for B09.4's test. The consumers are the X series (grid A*,
plane taps, region repair, meanders, diff pairs, escapes: X02, X05, X07, X08,
X10, X12) and the Q series (Q01 pour guard and audit, Q02 widen-back, Q03
diagnostics).

## What exists and what this adds

| Rust today (`crates/freerouting/src/`, `70389b7`) | Source |
|---|---|
| `BasicBoard::clearance_violations(id)` is public (`drc/clearance.rs:157`): Java's `Item.clearanceViolations` with the escape-via override | Java `Item.clearanceViolations` (`board/model/items/Item.java:371-505`) |
| `generate_snapshot`, `undo`, `pop_snapshot` (`board/basic_board.rs:1974-2005`) | Java's strict DRC takes a serialized snapshot and restores it when a new item has a violation (`autoroute/pipeline/AutorouteConnectionRouter.java:88-94,281-290`) |
| `get_autoroute_tree(class)` (`board/search/mod.rs:736`): trees compensated for a clearance class; tree shapes per item | KRT `obstacle_cache.precompute_net_obstacles`, `obstacle_map.build_base_obstacle_map` (Python C-space inflation) |
| `connected_sets(net)` (`board/basic_board.rs:1658`), `connected_set(id, net, stop_at_plane)` (`:1535`) | KRT `plane_component_oracle.py` (union-find over pins, traces, vias, areas) |
| `drc::zone_islands::fill_islands(board, ca)` (`drc/zone_islands.rs:899`): the fill as trapezoids per island; `ZoneIslandViolation.items_in_island` | KRT `plane_fill_model.py` (raster fill, min-thickness opening, islands without an anchor dropped) |

## Decisions made here

- **Services, not stages.** Each is a board query or a board transaction. No
  algorithm code lives here.
- **Determinism (D2) by construction:** every output is ordered by item id
  (highest first) or by (net, id). No hash order reaches a result.
- **A reverted transaction keeps Java's undo semantics.** The id generator and
  the trace half-width bounds are not rolled back, exactly as Java's undo
  (B01.8). Ids taken inside a reverted attempt stay taken. A stage that needs
  untouched ids works on a `deep_copy` instead.
- **The fill model reuses the DRC fill.** There is one fill computation
  (`zone_islands.rs`) with no fallback that depends on the environment. KRT's
  output depends on whether KiCad is installed, and that is the mistake to avoid.

## File ownership

| Item | New files | May edit |
|---|---|---|
| B09.1 | `board/transaction.rs`, `tests/board_transaction.rs` | `board/mod.rs` (append) |
| B09.2 | `board/obstacle_view.rs`, `tests/obstacle_view.rs` | `board/mod.rs` (append) |
| B09.3 | `drc/fill_model.rs`, `tests/fill_model.rs` | `drc/mod.rs` (append), `drc/zone_islands.rs` (expose island outlines only) |
| B09.4 | `rules/area_policy.rs`, `tests/area_obstacle_policy.rs` | `rules/mod.rs` (the policy field), `board/basic_board.rs` (the conduction-area branches of `is_trace_obstacle` and `is_obstacle` only) |

## Checklist

- [ ] **B09.1 — Insert, check, commit or revert.**
  - Rust: `board/transaction.rs`:
    `RoutingBoard::try_change(|board| -> Result<T, E>, check: CheckPolicy) ->
    TransactionResult<T>`. It runs `generate_snapshot()`, notes
    `max_generated_id`, runs the closure, then checks every item with an id above
    the noted one (Java's strict-DRC rule, `AutorouteConnectionRouter.java:88`)
    with `clearance_violations`. No violation: `pop_snapshot()` (commit). Else
    `undo()` (revert), returning the violations sorted by (first item, second
    item, layer). `CheckPolicy::{NewItemsOnly, NewAndChangedItems}`; the second
    also checks the ids B01.7's journal reports as changed.
  - **Accept:** `tests/board_transaction.rs`:
    - a clean insert commits, and the state hash equals that of the same insert
      without a transaction plus `generate_snapshot`/`pop_snapshot`;
    - an insert that violates clearance reverts, and the state equals the
      before-state except for the id generator;
    - a revert inside an outer snapshot leaves the outer undo intact;
    - the violation order is stable across two runs.

- [ ] **B09.2 — Per-net clearance-inflated obstacles in a window.**
  For a grid router's raster and for widest-path feasibility (Q03.1).
  - Rust: `board/obstacle_view.rs`: `RoutingBoard::net_obstacles(net, layer,
    window: IntBox, half_width) -> Vec<ObstacleShape { item: ItemId, shape:
    TileShape, is_via_only: bool }>`. Build it from the autoroute tree
    compensated for the net's trace clearance class (`get_autoroute_tree`), with
    the overlap query on the window, keeping items that `is_trace_obstacle(net)`,
    each shape enlarged by `half_width`. Ordered by item id (highest first), then
    shape index. Via keepouts are flagged `is_via_only`. The default tree and the
    router's trees are not modified, except that the compensated tree is created
    if missing, exactly as `get_autoroute_tree` does today.
  - **Accept:** `tests/obstacle_view.rs` on Issue026-J2_reference and
    Issue145-smoothieboard:
    - every shape of an item that `check_trace_shape` (B04.3) reports as an
      obstacle for a probe segment inside the window is in the view, and no
      own-net item is;
    - the output is identical across two runs and with the window split into four
      quadrants and merged (the quadrant union equals the whole, up to items
      spanning quadrants, which are deduplicated by (id, shape index)).

- [ ] **B09.3 — Fill model and own-net connectivity.**
  For plane-tap verification, the pour-cut audit (Q01.2) and region repair.
  - Rust: `drc/fill_model.rs`:
    - `conduction_fill(board, area) -> Vec<FillIsland { outline: Vec<Polygon>,
      anchors: Vec<ItemId> }>`. Island outlines come from the existing trapezoid
      decomposition (merged into boundary polygons). Anchors are the net's items
      whose centre (first corner for traces) lies on the island, highest id first,
      as `ZoneIslandViolation.items_in_island` computes them. Islands are ordered
      as `fill_islands` orders them (lowest y, then lowest x).
    - `net_components(board, net, stop_at_plane) -> Vec<Vec<ItemId>>`, over pins,
      traces, vias and conduction areas, built on `connected_set`, ordered by
      highest member id.
    - Optional, only when Q01's measurement (Q01.4) asks for it: opening islands
      at a minimum thickness (erode then dilate by half the thickness), as a
      separate function.
  - **Accept:** `tests/fill_model.rs`:
    - the four zone cases of `tests/drc.rs:375-452` (continuous pour, severed
      pour, dead copper, hole) give the expected islands and anchors;
    - `net_components` equals `connected_sets` on every `board_probe/fixtures.tsv`
      row for every net;
    - on two KiCad demo boards through F02.6's oracle, the island count per zone
      after refill is reported beside the model's count in the done note.
      Differences are explained, not required to be zero: KiCad's thermal and
      fill settings aren't in the DSN.

- [ ] **B09.4 — A per-net obstacle hook for conduction areas.**
  Q01 makes pours of listed net classes block nets that have no pin inside
  them. It needs one hook where the board decides whether a conduction area is
  an obstacle for a net, `None` on the parity path. Q01 implements the policy;
  this item only adds the hook.
  - Java: `ConductionArea.isTraceObstacle(net)` (`board/model/items/ConductionArea.java:407`)
    and `isObstacle(Item)` (`:389`). Rust: the conduction-area branches of
    `BasicBoard::is_trace_obstacle` (`board/basic_board.rs:1286-1288`) and
    `is_obstacle` (`:1173`). The maze search (via room completion), the checks
    (B04) and the shover call these at query time, so a policy never changes
    tree contents.
  - Rust: `rules/area_policy.rs` with `pub trait AreaObstaclePolicy: Send + Sync
    { fn blocks(&self, area: ItemId, net: i32) -> bool; }` and
    `BoardRules::area_obstacle_policy: Option<Arc<dyn AreaObstaclePolicy>>`
    (manual `Debug`, cloned by `Arc`). With a policy, an area is an obstacle for a
    net iff Java's rule says so **or** `blocks(area, net)`. Same-net items are
    never blocked: the policy is asked only for foreign nets. With `None`, the
    code path is Java's rule exactly.
  - **Accept:** `tests/area_obstacle_policy.rs`:
    - with `None`, `reference_load.rs`, `reference_drc.rs` and B02's tree probe
      stay green (results unchanged by construction);
    - a test policy that blocks one net makes `check_trace_shape` (B04.3) fail
      for that net inside the area and pass for another;
    - the board stays `Sync` (`drc/clearance.rs:405-410`);
    - DRC wall time on Issue145-smoothieboard with `None` is within 1% of before
      (release, median of 5, done note).
