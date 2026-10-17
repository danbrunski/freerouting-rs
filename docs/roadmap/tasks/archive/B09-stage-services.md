# B09 — Board services for algorithm stages

Opened 2026-10-15. Decision D9 in [port-completion.md](../../port-completion.md)
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

- [x] **B09.1 — Insert, check, commit or revert.**
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
  *Done 2026-10-16:* `board/transaction.rs`: `RoutingBoard::try_change(change,
  CheckPolicy) -> TransactionResult<T, E>` with `Committed(T)`, `Reverted { value,
  violations }` and `Aborted(E)` (the closure's `Err` reverts unchecked; the
  checklist's one-parameter `TransactionResult<T>` had no slot for the closure's
  error). `NewAndChangedItems` journals the closure with a fresh journal and hands
  a running caller journal its changes plus the revert's records. Tests
  (`cargo test --release -p freerouting --test board_transaction`, 7 passed, on
  Issue026-J2_reference):
    - `a_clean_insert_commits_like_a_snapshot_and_pop`: `state_hash` and the item
      list equal the plain `generate_snapshot`/insert/`pop_snapshot` board;
    - `a_violating_insert_reverts_to_the_before_state` (both policies): items equal
      the before-state, `max_generated_id` advanced; Java's undo leaves redo data,
      and after the next `generate_snapshot`/`pop_snapshot` drops it the full
      `state_hash` (undo structure included) equals the before hash;
    - `a_revert_inside_an_outer_snapshot_keeps_the_outer_undo`,
      `a_commit_inside_an_outer_snapshot_is_undone_with_it`;
    - `new_and_changed_items_checks_an_item_changed_in_place` (an in-place
      `assign_net_no` onto a foreign pad commits under `NewItemsOnly`, reverts
      under `NewAndChangedItems`; the caller's journal reports the item);
    - `the_violation_order_is_stable` (two runs equal), and
      `tools/determinism-check.sh --out <scratch>/det-b091 -- env
      BOARD_TRANSACTION_OUT={OUT} <board_transaction binary>
      the_violation_order_is_stable`: identical (idle vs 8 burners).

- [x] **B09.2 — Per-net clearance-inflated obstacles in a window.**
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
  *Done 2026-10-16:* `board/obstacle_view.rs`: `RoutingBoard::net_obstacles(net,
  layer, window, half_width) -> Vec<ObstacleShape { item, shape_index, shape,
  is_via_only }>` (`shape_index` added for the deduplication key). The query box
  is the window enlarged by `half_width`, so every inflated shape that reaches the
  window is found; `half_width` is Java's compensated trace half width (trace half
  width plus the class's compensation value), documented in the module. Tests
  (`cargo test --release -p freerouting --test obstacle_view`, 2 passed):
  `j2_view_holds_every_blocker` (6 nets, 864 probes, 684 blocked, 41 view shapes)
  and `smoothieboard_view_holds_every_blocker` (6 nets, 864 probes, 379 blocked,
  202 view shapes). For each probe segment `check_trace_shape` fails exactly when
  the clearance query finds a trace obstacle, every such item is in the view with a
  view shape that reaches the probe's centreline, no view item has the net, two
  calls are equal, the four quadrants merged by (id, shape index) equal the whole,
  and the walk of every tree that existed before is unchanged.

- [x] **B09.3 — Fill model and own-net connectivity.**
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
  *Done 2026-10-16:* `drc/fill_model.rs`: `conduction_fill(board, area: ItemId) ->
  Vec<FillIsland { island, outline: Vec<FillPolygon>, anchors }>` (the DRC's
  `Island` kept beside the outline; `FillPolygon` is a float ring, CCW outer, CW
  hole, since the fill is float), `island_outline(&Island)` (trapezoid sides
  cancelled by exact chain arithmetic, then traced into rings), `net_components(board,
  net, stop_at_plane)`. `zone_islands.rs` is unchanged: `Island.trapezoids` was
  already public. The optional minimum-thickness opening is not built (Q01.4 has not
  asked for it). Tests (`cargo test --release -p freerouting --test fill_model`, 6
  passed, 1 ignored report):
    - the four zone cases: `a_continuous_pour_is_one_island_without_anchors` (1
      island, one 4-corner ring), `a_severed_pour_is_two_islands_with_one_via_each`
      (left then right, each via its island's anchor, 4 spoke holes each),
      `a_cut_off_piece_is_an_island_without_anchors` (the anchorless island is the
      DRC's dead-copper finding), `a_hole_is_a_clockwise_ring_of_the_one_island`;
      in each the outline's signed area equals the trapezoids' area (1e-6 relative)
      and the anchors equal the DRC's `items_in_island`;
    - `fills_of_real_boards_close_and_match_the_drc` (smoothieboard, J2, hw48na: 3
      areas, 58 islands, 1597 hole rings, 35 DRC findings with equal anchors);
    - `net_components_equal_connected_sets_on_every_fixture`: 164 boards, 15776
      nets equal to `connected_sets`; with `stop_at_plane` a partition of the
      connectable items on every net (102 nets split further at planes).
  KiCad 10.0.7 (WSL) refill (`pcbnew.ZONE_FILLER`, outline count per zone layer)
  beside the model on the exported DSN (`report_islands_of_a_dsn`), as islands /
  islands with an anchor / islands with a bounding box of at least 1 mm²:

  | Board, zone | KiCad | model |
  |---|---|---|
  | StickHub GND F.Cu | 5 | 39 / 3 / 10 |
  | StickHub +5V B.Cu | 1 | 2 / 2 / 1 |
  | StickHub GND B.Cu | 3 | 50 / 7 / 11 |
  | StickHub +5V B.Cu | 1 | 4 / 4 / 3 |
  | StickHub +1V8 B.Cu | 1 | 6 / 6 / 2 |
  | pic_programmer GND bottom | 3 | 10 / 5 / 9 |

  Why they differ: every zone has KiCad's island removal "always" (islands without
  a pad are dropped; the model keeps them, and Java's DRC reports those of 1 mm² or
  more as dead copper), and a minimum thickness of 0.15 or 0.3 mm that removes the
  slivers the model keeps. StickHub's zones connect pads solidly (KiCad
  `FULL`), but the DSN carries no zone-connection setting, and Java's fill puts
  thermal spokes on every same-net pad; on square pads they end inside the
  clearance (DIVERGENCES, "Design rule check"), so pads become islands of their
  own: the +1V8 pour's 6 anchored islands against KiCad's 1. pic_programmer uses
  thermals (0.635 mm spokes, KiCad) against the DSN's rule-derived spokes.

- [x] **B09.4 — A per-net obstacle hook for conduction areas.**
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
  *Done 2026-10-16:* `rules/area_policy.rs` (trait, `Debug` for `dyn
  AreaObstaclePolicy`, the two `BasicBoard` helpers), `BoardRules::area_obstacle_policy`
  (`None` in `BoardRules::new`). In `basic_board.rs` the four conduction-area
  branches consult it: `is_trace_obstacle`, and in `is_obstacle` the via and trace
  arms (the other item is the area) and the area arm (the item is the area). In
  `is_obstacle` the policy applies to traces and vias only; pins keep Java's rule.
  Tests:
    - with `None`: `cargo test --release -p freerouting --test reference_load` (1
      passed), `--test reference_drc` (4 passed), `--test board_probe_tree` (1
      passed, 77 s);
    - `cargo test --release -p freerouting --test area_obstacle_policy`, 4 passed:
      `a_policy_blocks_only_its_net_in_check_trace_shape` (smoothieboard, a segment
      inside a non-obstacle plane: `check_trace_shape` fails for the blocked net,
      passes for another net, the area's own net and a policy naming another area),
      `the_policy_is_never_asked_for_the_areas_own_net`,
      `a_blocked_trace_inside_the_area_is_a_clearance_violation` (both directions of
      `is_obstacle` and both items' clearance checks),
      `a_board_with_a_policy_stays_sync_and_clones_share_it` (`BasicBoard` and
      `RoutingBoard` are `Send + Sync`; `deep_copy` shares the `Arc`);
    - DRC wall time (`DesignRulesChecker::check` on Issue145-smoothieboard, release,
      1 warm-up then the median of 25, three alternations of this tree against the
      same tree with `basic_board.rs` at HEAD, load average 4-5 from other agents):
      with the hook 14.0 / 14.1 / 13.8 ms, before 13.6 / 14.3 / 13.9 ms; median of
      the medians 14.0 against 13.9 ms (+0.7%), fastest runs 13.3-13.5 ms on both.
