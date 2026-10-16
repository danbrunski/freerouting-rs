# I01 — Interactive route session

Opened 2026-10-15. Interactive routing is the core of Java's editor: click a
pin or trace, move the mouse, and a trace follows with push-and-shove,
neckdown, SMD snap and via layer changes. In Java that logic is spread over
`RouteState` (456 lines), `DynamicRouteState`, `StitchRouteState` and `Route`
(844 lines), mixed with Swing calls. This checklist ports the behaviour as an
engine `RouteSession` with no UI types. See [I00](../I00-interactive-README.md)
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

- [x] **I01.1 — Settings and parameter resolution.**
  - Java: `GuiBoardRoutingSettings.getTraceHalfwidth/isActiveRoutingLayer/getTraceClearanceClass/getViaRule`
    (`:64-108`).
  - Rust: `interactive/settings.rs`: `EditSettings`, `ManualRules`,
    `resolve_route_params(board, settings, nets) -> RouteParams` (half width
    per layer from the first net; a layer is active if any route net may use
    it; the clearance class and via rule of the first net; manual rules
    override, and a missing manual via rule falls back to the net class's).
  - **Accept:** unit tests for net-class rules, manual rules, a net without
    a net class and a manual via-rule index out of range.
  - *Done 2026-10-16:* `interactive/settings.rs` (`EditSettings` with Java's
    `WorkspaceSettings` defaults, `ManualRules`, `LayerMask`,
    `resolve_route_params`/`resolve_with_rules`). Unit tests
    `interactive::settings::tests::{net_class_rules_of_the_first_net,
    a_layer_is_active_if_any_route_net_may_use_it,
    manual_rules_override_the_net_class,
    manual_via_rule_index_out_of_range_falls_back_to_the_net_class,
    a_net_without_a_net_class, layer_mask_and_defaults}`:
    `cargo test -p freerouting --lib interactive` → 6 passed. A net number
    without a net stands in for "a net without a net class" (every port net
    has one); its defined results and the out-of-range fallback are in
    DIVERGENCES, "Interactive routing".

- [x] **I01.2 — Route start.**
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
  - *Done 2026-10-16:* `interactive/route/start.rs` (`RouteSession::start`,
    `StartInfo`, `StartError`). Every start line of the `route-script` probe
    (item, point, nets, half widths, clearance class, via rule, stitch, layer,
    target count) equals Java on all 10 boards: `cargo test -p freerouting --test interactive_route route_script` → 10 passed (`route_script_matches_java_{j2,bm08,bm05,rpi_splitter,issue413,issue676,issue957,smd_demo,scoring_mixed_layer,issue110}`). The trace-start
    path runs on the 4 trace starts per variant; where `connectToTrace` splits
    the picked trace, Java's `getUnconnectedSet` on the removed trace is
    reproduced (bm05 route 21).

- [x] **I01.3 — Next corner, neckdown and connecting to the target.**
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
  - *Done 2026-10-16:* `interactive/route.rs` (`next_corner`, `RouteStep`,
    `RouteBudget::JAVA` = 3000/2000 nominal ms, fresh check limits per insert,
    neckdown try and `connect` pair). Every corner line (ended flag, layer, last
    corner, shove-failing obstacle, nearest target, `maxGeneratedId`) and every
    per-route canonical dump equal Java on the 10 boards, push on/off ×
    dynamic/stitch: `cargo test -p freerouting --test interactive_route route_script` → 10 passed (`route_script_matches_java_{j2,bm08,bm05,rpi_splitter,issue413,issue676,issue957,smd_demo,scoring_mixed_layer,issue110}`). Java's stale target objects are mirrored by a
    last-seen shape cache (rpi_splitter route 14, smd-demo route 22).

- [x] **I01.4 — Layer change and cancel.**
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
  - *Done 2026-10-16:* `interactive/route/layer.rs` (`change_layer` →
    `LayerChange` + `Status`, `cancel`). The probe's layer-change lines
    (changed/failed, failing layer) and cancel lines equal Java on the 10
    boards (in the 8 `.jsonl` outputs 104 changed, 211 failed, 120 cancels; no board
    reaches a plane): `cargo test -p freerouting --test interactive_route route_script` → 10 passed (`route_script_matches_java_{j2,bm08,bm05,rpi_splitter,issue413,issue676,issue957,smd_demo,scoring_mixed_layer,issue110}`). `layer_change_without_spanning_via_quirk`,
    `cancel_removes_tail_only` (push on and off), `layer_change_with_a_via`:
    `cargo test -p freerouting --test interactive_route -- --skip route_script`
    → 5 passed. DIVERGENCES, "Interactive routing", has the quirk.

- [x] **I01.5 — Dynamic and stitch modes.**
  - Dynamic: `Move` adds a corner; `Click` ends; `Snapshot` takes one;
    `Complete`/`Cancel` as Java's Enter/Space and Esc. Stitch: `Click`
    takes a snapshot then adds a corner; `Move` only updates the nearest
    target point.
  - Rust: a `RouteMode` policy in `interactive/route.rs`.
  - **Accept:** unit tests that replay the same pointer sequence in both
    modes on J2 and check snapshot counts (undo depth) and corner counts.
  - *Done 2026-10-16:* `RouteMode`, `RouteInput`, `RouteEvent`,
    `RouteSession::handle`. `dynamic_and_stitch_modes_replay_the_same_pointer_sequence`
    (J2, six moves, `Snapshot`, a click): dynamic 6 steps, snapshot, end, 2
    snapshots; stitch 6 target updates, `Snapshot` ignored, the click 1
    snapshot + 1 corner, 2 snapshots; the board's undo depth equals
    `snapshots_taken` in both (`interactive_route` → passed). The probe mirror
    drives stitch steps through `handle(Click)` and dynamic ones through
    `handle(Move)`. `Cancel` is `RouteSession::cancel` (I01.4).

- [x] **I01.6 — Overlay data.**
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
  - *Done 2026-10-16:* `interactive/overlay.rs` (`RouteOverlay::new(session,
    board, length_matching_visible)`, `RouteIncomplete`, `LengthEllipse`).
    `overlay_ellipses_and_targets_on_j2`: J2 has no length rules, so the test
    writes net 1's limits (none; max 100000; min 60000 + max 100000; max
    20000; min 50000 only, visible and not) and checks the 21 target ids in
    Java's order, the incomplete (from, to, width 100 or 300, violation flag)
    and the radii computed by hand (`0.5 L`, `0.5 sqrt(L² − 24000²)`), centre
    and rotation π/2: `cargo test -p freerouting --test interactive_route
    overlay` → 1 passed.

- [x] **I01.7 — Probe `route-script`.**
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
  - *Done 2026-10-16:* `tools/java-reference/probes/board/RouteScriptProbe.java`,
    `fixtures/reference/probe/route-script/` (8 `.jsonl`, J2 and bm08 as
    `.digest`), `tests/interactive_route.rs`. Identical on 10 of 10 boards:
    `cargo test -p freerouting --test interactive_route route_script` → 10 passed (`route_script_matches_java_{j2,bm08,bm05,rpi_splitter,issue413,issue676,issue957,smd_demo,scoring_mixed_layer,issue110}`). Generated at `a773a70ec` with
    `JDK_JAVA_OPTIONS=-Xmx4g REFERENCE_WORK_DIR=… generate.sh <pin> route-script`
    (Java's own run of J2: 885 ms). The script adds 4 trace starts per variant
    and a cancel every fifth route to the checklist's 20 pin starts.

- [x] **I01.8 — Replay and determinism.**
  - `examples/edit_script.rs <dsn> [ses] <script.json> <out>` replays route
    commands (start, moves, clicks, layer changes, cancel) and writes the
    canonical dump.
  - **Accept:** a committed 50-step script on J2 replays to identical output
    under `tools/determinism-check.sh`; after J01, the same script with
    Java's Route budgets (3000 check, 2000 pull-tight, × 64,000 units)
    still matches the probe.
  - *Done 2026-10-16:* `examples/edit_script.rs` (`[--unlimited]` drops the
    budgets), `tests/support/edit_script.rs` (script parser and replay, shared
    with the test), `tests/data/edit_script_j2.json` (60 commands: the
    `load/push-dynamic` routes 0-4 of the J2 probe, 48 moves, 4 layer changes,
    a cancel). `tools/determinism-check.sh --out … -- target/release/examples/edit_script
    fixtures/Issue026-J2_reference.dsn crates/freerouting/tests/data/edit_script_j2.json {OUT}`
    → `identical` (70 lines; idle load 17.99, loaded 18.47 with 8 burners).
    J01 is the pin: `edit_script_replays_the_probe_routes` replays the script
    with `RouteBudget::JAVA` (3000/2000 nominal ms × 64,000 units) and matches
    every command state and the final dump of the probe mirror, which
    `route_script_matches_java_j2` (also with the Java budgets) proves equal to
    Java; `RouteBudget::UNLIMITED` gives the same output.
