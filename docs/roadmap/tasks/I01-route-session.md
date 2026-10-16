# I01 — Interactive route session

Opened 2026-10-15. Interactive routing is the core of Java's editor: click a
pin or trace, move the mouse, and a trace follows with push-and-shove,
neckdown, SMD snap and via layer changes. In Java that logic is spread over
`RouteState` (456 lines), `DynamicRouteState`, `StitchRouteState` and `Route`
(844 lines), mixed with Swing calls. This checklist ports the behaviour as an
engine `RouteSession` with no UI types. See [I00](I00-interactive-README.md)
for the API shape and the series rules.

**Prerequisites:**
- O01 (`opt_changed_area` with a keep point), O04
  (`insert_forced_trace_segment`, `forced_via`);
- B01 (undo snapshots, B01.7's change journal for `Response.delta`, B01.8's
  shove-failing obstacle and layer);
- B04 (`pick_nearest_routing_item`, filtered `pick_items`);
- B05 (`connect_to_trace`; `get_swappable_pins`; pin neckdown half width).
  If B00 places pin swap elsewhere, follow B00.

## What exists and what this adds

| Exists | Where |
|---|---|
| `generate_snapshot`, `undo`, `redo`, `pop_snapshot` | `board/basic_board.rs:1974-2005` |
| `get_trace_tail`, `remove_trace_tails`, `connection_items`, `unconnected_set` | `board/trace_ops.rs:704,767`; `board/basic_board.rs:1788,1625` |
| `reduce_nets_of_route_items` (returns the real value; Java always returns false, a listed divergence) | `board/routing_board.rs:123` |
| Rules: `trace_half_width(net, layer)`, net class `trace_clearance_class`, `via_rule`, `is_active_routing_layer`; `ViaInfo.attach_smd_allowed` | `rules/mod.rs` |
| Forced insertion and pull-tight | O01, O04 |

| Java at `cda540f` (`src/main/java/app/freerouting/`) | Lines |
|---|---|
| `gui/interactive/RouteState.java`: `getInstance` `:45-167`, `startOk` `:174-213`, `pickRoutingItem` `:215-226`, `getRouteNetNumbersAtTiePin` `:232-254`, `addCorner` `:304-331`, `cancel` `:334-363`, `changeLayerAction` `:365-441` | 456 |
| `gui/interactive/Route.java`: constructor `:85-132`, `nextCorner` `:139-253`, `changeLayer` `:260-317`, `snapToSmdCenter` `:319`, `connectToTarget` `:350`, `connect` `:378`, `nearestTargetLayer` `:421`, `calculateSwapPinInfos` `:437`, `draw` `:466-592`, `angledConnection` `:594`, `calculateTargetPointsAndAreas` `:618`, `calcNearestTargetPoint` `:648`, `tryNeckdownAtStart` `:708`, `tryNeckdownAtEnd` `:760` | 844 |
| `gui/interactive/DynamicRouteState.java` (move → corner; click ends; `s` snapshot), `StitchRouteState.java` (click → snapshot + corner; move → nearest target only) | 55, 77 |
| `gui/workspace/controllers/GuiBoardRoutingSettings.java`: `getTraceHalfwidth` `:64`, `isActiveRoutingLayer` `:71`, `getTraceClearanceClass` `:84`, `getViaRule` `:98` | — |

## Decisions made here

- **`RouteSession` owns Java's `Route` state** (previous corner, layer, nets,
  pen half widths, active layers, clearance class, via rule, depths, targets,
  swap-pin infos, nearest target, shove-failing obstacle). `EditSettings` is
  the engine subset of Java's `WorkspaceSettings` (layer, push, stitch,
  tidy width, accuracy, neckdown, via snap to SMD centre, highlight obstacle,
  select on all visible layers, manual rules, visible layers).
- **Step outcomes are explicit.** `nextCorner` compares `okPoint` with
  `prevCorner` and `currentCorner` by reference (`:184-205`), while `connect`
  compares by value (`:400-406`). Rust uses O04's `ForcedInsert` enum where
  Java uses identity and value equality where Java uses `equals`.
  `RouteStep::{Rejected, Blocked { obstacle }, Advanced, Completed,
  DatabaseDamaged}` is the session's result.
- **Kept quirk: a layer change can "succeed" without a via.** In
  `Route.changeLayer`, if no via of the rule spans both layers, the loop
  never runs, `result` stays true and the route's layer doesn't change
  (`:281-316`); `RouteState` then switches the workspace layer and takes a
  snapshot anyway. Kept, tested, DIVERGENCES line.
- **Visible layers are input.** `pickRoutingItem` skips layers the GUI shows
  as invisible (`graphicsContext.getLayerVisibility <= 0`). The GUI passes a
  `LayerMask` in `EditSettings`.

## File ownership

| Item | New files | May edit |
|---|---|---|
| I01.1 | `interactive/mod.rs`, `interactive/settings.rs` | `lib.rs` (append) |
| I01.2 | `interactive/route.rs` | — |
| I01.3 | — | `interactive/route.rs` |
| I01.4 | — | `interactive/route.rs`, `docs/DIVERGENCES.md` |
| I01.5 | — | `interactive/route.rs` |
| I01.6 | `interactive/overlay.rs` | — |
| I01.7 | `tools/java-reference/probes/board/RouteScriptProbe.java`, `tests/interactive_route.rs`, `fixtures/reference/probe/route-script/*` | `tools/java-reference/README.md` |
| I01.8 | `examples/edit_script.rs` | — |

## Checklist

- [ ] **I01.1 — Settings and parameter resolution.**
  - Java: `GuiBoardRoutingSettings.getTraceHalfwidth/isActiveRoutingLayer/getTraceClearanceClass/getViaRule`
    (`:64-108`).
  - Rust: `interactive/settings.rs`: `EditSettings`, `ManualRules`,
    `resolve_route_params(board, settings, nets) -> RouteParams` (half width
    per layer from the first net; a layer is active if any route net may use
    it; the clearance class and via rule of the first net; manual rules
    override, and a missing manual via rule falls back to the net class's).
  - **Accept:** unit tests for net-class rules, manual rules, a net without
    a net class and a manual via-rule index out of range.

- [ ] **I01.2 — Route start.**
  - Java: `RouteState.getInstance` (`:45-167`), `startOk`, `pickRoutingItem`,
    `getRouteNetNumbersAtTiePin`.
  - Rust: `RouteSession::start(board, settings, at) -> Result<(Self,
    StartInfo), StartError>`.
  - Orders (exact):
    - pick on the current layer; if nothing and "select on all visible
      layers": layer 0, the last layer, inner signal layers ascending, then
      inner non-signal layers ascending, skipping the current layer and
      invisible layers; a pick on another layer moves the current layer to
      the item's first layer (reported in `Response.layer`);
    - no nets → no start; a tie pin routes its nets minus those of contacts
      on the current layer (ascending);
    - a picked trace: snap to its nearest end point if that is an integer
      point within 5 × half width; otherwise round the nearest point on the
      polyline and `connect_to_trace` (failure → no start); without manual
      rules take the trace's half width (on its layer) and clearance class;
    - a picked drill item: start at its integer centre;
    - stitch mode if the setting is on, or the net class is shove-fixed or
      has pull-tight off; snapshot; target set = `unconnected_set(item,
      -1)`.
  - **Accept:** I01.7's start outcomes (start point, nets, half widths,
    clearance class, mode, layer) equal.

- [ ] **I01.3 — Next corner, neckdown and connecting to the target.**
  - Java: `Route` constructor, `nextCorner`, `tryNeckdownAtStart/AtEnd`,
    `connectToTarget`, `connect`, `angledConnection`,
    `calculateTargetPointsAndAreas`, `calcNearestTargetPoint`,
    `nearestTargetLayer`, `calculateSwapPinInfos`.
  - Rust: `RouteSession::next_corner(board, p, budget) -> RouteStep`.
  - Orders (exact), from `nextCorner`: inactive layer, a corner off the
    board, a non-signal layer or an unchanged rounded corner → `Rejected`;
    previous corner already at a drill-item target → `Completed`; 90°/45°
    projection; `endRoutingItem` = nearest routing item at the previous
    corner, then the nearest target excluding items connected to it; a
    3000-nominal-ms check limit for the insert (none in stitch mode), and a
    fresh one inside each neckdown try and each `connect` corner pair
    (`Route.java:177,384,738,779` at `30baad7e2`); neckdown at start, then
    at end, only while
    nothing was inserted; `DatabaseDamaged` → `undo`, and end the route in
    dynamic mode; nothing inserted → record the shove-failing obstacle,
    `Blocked`; reaching the corner tries `connectToTarget`; tidy clip:
    `i32::MAX` → none, 0 → `Skip`, else the octagon around the reached
    point enlarged; on completion `reduce_nets_of_route_items` and
    `combine_traces` per net, else update the nearest target point; finally
    `opt_changed_area` with the keep point and Route's pull-tight budget.
  - Push on: depths 20/8/5; off: 0/0/0. Push enabled is read at start.
  - **Accept:** I01.7.

- [ ] **I01.4 — Layer change and cancel.**
  - Java: `Route.changeLayer` (`:260-317`), `RouteState.changeLayerAction`
    (`:365-441`), `RouteState.cancel` (`:334-363`).
  - Rust: `RouteSession::change_layer(board, to, budget) -> LayerChange`,
    `RouteSession::cancel(self, board, settings)`.
  - Orders (exact): same layer → ok; out of range or inactive → fail; no
    via rule → fail; SMD snap to the target layer, else the current one;
    vias of the rule in order, only those spanning both layers: snapshot,
    `forced_via` with via depth 0, success stops, failure records the
    obstacle and undoes; after success, a via of the route nets at the last
    corner (VIAS filter) contacting a conduction area on a layer strictly
    between (old layer exclusive, new layer inclusive) ends the route
    (plane reached); otherwise switch the layer and snapshot. Cancel: the
    trace tail at the last corner on the current layer, its connection items
    stopping at vias, removed with `remove_items_and_pull_tight(tidy,
    accuracy)` when push is on, else `remove_items`.
  - **Accept:** I01.7 layer-change steps; `layer_change_without_spanning_via_quirk`
    (the kept quirk); `cancel_removes_tail_only`.

- [ ] **I01.5 — Dynamic and stitch modes.**
  - Dynamic: `Move` adds a corner; `Click` ends; `Snapshot` takes one;
    `Complete`/`Cancel` as Java's Enter/Space and Esc. Stitch: `Click`
    takes a snapshot then adds a corner; `Move` only updates the nearest
    target point.
  - Rust: a `RouteMode` policy in `interactive/route.rs`.
  - **Accept:** unit tests that replay the same pointer sequence in both
    modes on J2 and check snapshot counts (undo depth) and corner counts.

- [ ] **I01.6 — Overlay data.**
  - Java: the data `Route.draw` uses (`:466-592`): target items, swap pins
    and their incompletes, the incomplete from the last corner to the
    nearest target (flagged when the net's length limits are violated),
    the length-matching ellipse parameters from the net's min/max length,
    the layer-change marker, the shove-failing obstacle (only with
    "highlight obstacle").
  - Rust: `interactive/overlay.rs`, `RouteOverlay` built from the session;
    no colours or pixels.
  - **Accept:** unit tests on `Issue026-J2_reference` comparing ellipse
    parameters and target ids with values computed by hand from the DSN's
    net lengths (written in the test).

- [ ] **I01.7 — Probe `route-script`.**
  - Java: `RouteScriptProbe` drives `Route` headless (its constructor needs
    only a `RoutingBoard`; `draw` is never called). It transcribes the board
    calls of `RouteState.getInstance` with fixed settings (current layer 0,
    select on all visible layers, all layers visible, no manual rules) and
    says so in its header.
  - Boards: `Issue026-J2_reference`, `Issue508-DAC2020_bm08`,
    `Issue508-DAC2020_bm05`, `Issue143-rpi_splitter`, `Issue413-test`,
    `Issue676-ch32v-tx118s`, `Issue957-smoke-ldo-led`,
    `Issue508-SMD-routing-issue-demo`, `scoring-mixed-layer`,
    `Issue110-testProjectFromFreeroutingBugTest` (as loaded; J2 and
    bm08 also with their SES).
  - Script per board: 20 start pins (seeded); for each, 8 corner steps
    towards the nearest unconnected pin of the net, a layer change to the
    next signal layer, 4 more steps; run with push on and off, and dynamic
    and stitch. Dump after each step: the step outcome, the layer, the
    shove-failing obstacle id and `maxGeneratedId`; the canonical dump at
    the end of each route.
  - **Accept:** identical on at least 9 of 10 boards; on the remaining one
    the first differing step is printed and its cause named.

- [ ] **I01.8 — Replay and determinism.**
  - `examples/edit_script.rs <dsn> [ses] <script.json> <out>` replays route
    commands (start, moves, clicks, layer changes, cancel) and writes the
    canonical dump.
  - **Accept:** a committed 50-step script on J2 replays to identical output
    under `tools/determinism-check.sh`; after J01, the same script with
    Java's Route budgets (3000 check, 2000 pull-tight, × 64,000 units)
    still matches the probe.
