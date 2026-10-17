# G09 — Interactive routing and editing

Opened 2026-10-15. Java's app is also an interactive router: route a trace
from a pin with push-and-shove, change layers mid-route, drag vias and
components, swap pins and push traces aside ("make space"). The logic is
the engine's `interactive` module (the I series, decision D7). This
checklist maps mouse and keys to I-series calls and draws what they return.
It is the report's GUI-9 (GUI, plugin and packaging report §1.7, §4) and
where frame target FT7 is met.

**Prerequisites:**
- [G05](G05-inspect-and-safe-edits.md): tools framework, context menus,
  undo/redo with patching.
- I01: the route session (Java `Route`/`RouteState` logic as an engine
  `RouteSession`).
- I02: drag, `MoveComponent`, make space.
- I03: editor commands (pin swap).
- I04 (optional, owner decision): selection transforms; only for G09.6.
- C02: the interactive settings struct (G09.2).
- B01 (journal) and B08 (`ItemGeometry`), through G05.5's patching.

## What exists and what this adds

| Exists | Where |
|---|---|
| Tools framework, popup menus, patching, Java key set | G01 (`src/input.rs`), G05 (`src/tools/`, `src/tools/popup.rs`), G02.4 (`src/canvas/chunks.rs`) |
| Low-level component moves | `Component::{turn_90_degree, rotate, change_side}` and the `Components` wrappers by `ComponentId` in `crates/freerouting/src/board/structure/component.rs` (used by I02 and I04, never by the GUI) |
| Interactive engine | `freerouting::interactive` (I01–I04): `RouteSession`, drag and make-space, `Editor::apply` with `Command`, `Response`, `Overlay`, pin swap, history, the selection transforms and copy. G09 added `Command::SetSelection`, `Editor::detached` and `Editor::apply_on` (the GUI keeps the board in its `Arc` and lends it to each command). |
| Java at `cda540f` | `gui/interactive/{MenuState,RouteMenuState,RouteState,DynamicRouteState,StitchRouteState,DragMenuState,DragState,DragItemState,MakeSpaceState,PinSwapState,MoveItemState,CopyItemState,CutoutRouteState}.java`, `gui/menus/{PopupMenuDynamicRoute,PopupMenuStitchRoute,PopupMenuChangeLayer,PopupMenuMove,PopupMenuCopy}.java`, `gui/windows/routing/{WindowRouteParameter,ManualRulesPanel,WindowMoveParameter}.java`, `board/actions/MoveComponent.java` |

What this adds: `src/tools/route.rs`, `src/tools/drag.rs`,
`src/tools/move_component.rs` (only with G09.6), the interactive part of
`src/dialogs/route_settings.rs`, overlays for the route in progress, and
`tests/ui_interactive.rs`.

**What Java can actually do at the pin.** Reachable from the GUI: route
(dynamic and stitch), drag (`DragItemState`, or `MakeSpaceState` when
nothing movable is under the cursor), pin swap, keepout construction (G05.7)
and the selection tools (G05). Not reachable: `MoveItemState`,
`CopyItemState` and `CutoutRouteState` have no `getInstance` caller outside
their own files (only `CopyItemPreviewTest` creates one), and
`GuiBoardManager.moveSelectedItems`, `copySelectedItems`,
`cutoutSelectedItems` and `assignSelectedToNewGroup` call the empty
`GuiBoardLegacyEditActions`. So the component grid and the wheel-rotate
setting (`WindowMoveParameter`) only affect a state nobody can enter.

## Decisions made here

- **The GUI holds no routing logic.** A tool turns input into I-series
  calls (`start`, `move_to`, `add_corner`, `change_layer`, `complete`,
  `cancel`) and draws what the session returns: the trace so far, the
  rubber band, and the obstacle when "highlight routing obstacle" is on.
  Which net, which rule, whether stitch mode is forced (Java forces it for
  nets whose class is shove-fixed or has pull-tight off,
  `RouteState.getInstance`) and which item a drag picks are I01/I02
  decisions.
- **Java's default mode returns.** Java starts in route mode
  (`InteractiveStateController.bootstrapInitialState`, and
  `GuiManager` calls `setRouteMenuState()` unless the `inspectionMode`
  feature flag is set). Before G09 the port starts in inspect mode; once
  G09.1 lands it starts in route mode like Java.
- **Keys follow `MenuState.keyTyped` and `RouteState.keyTyped`**: R route
  mode, D drag mode, S inspect mode, T start a route at the cursor, P
  toggle push-and-shove, W pin swap, I select at the cursor, digits and
  `+`/`-` change the layer (mid-route this asks I01 to change layer, which
  places a via), Enter or Space completes, Esc cancels. `E`
  (`ExpandTestState`, a debug state) is not ported.
- **Every completed interactive action is one undo step** (I01–I03 call
  `generate_snapshot()` as Java does), patched through G05.5.
- **Status texts are Java's** (`InteractiveState_en.properties`):
  `routing_net_message`, `layer_changed_to_message`,
  `layer_not_changed_inactive_net_message`,
  `layer_not_changed_obstacle_message`, `dragging_item`, `no_pin_selected`,
  `no_swappable_pin_found`, `please_click_second_pin_with_the_left_mouse_button`,
  `pin_not_swapped_because_it_is_already_connected`,
  `pin_not_swapped_because_second_pin_is_already_connected`,
  `pin_swap_completed`, `insertion_failed_because_of_obstacles`.

## File ownership

Paths are under `crates/freerouting-gui/`.

| Item | New files | May edit |
|---|---|---|
| G09.1 | `src/tools/route.rs`, `src/overlays/route.rs`, `tests/ui_interactive.rs` | `src/input.rs`, `src/commands.rs`, `src/tools/popup.rs` |
| G09.2 | — | `src/dialogs/route_settings.rs` (interactive section), `src/settings.rs` |
| G09.3 | `src/tools/drag.rs` | `src/input.rs` |
| G09.4 | — | `src/tools/inspect.rs` (W in menu states), `src/commands.rs` |
| G09.5 | — | `src/overlays/route.rs` |
| G09.6 | `src/tools/move_component.rs` | `src/tools/popup.rs` |
| G09.7 | — | `src/stats.rs`, `docs/development/gui.md` |

## Checklist

- [x] **G09.1 — Route mode.**
  - Java: `RouteMenuState.leftButtonClicked` → `RouteState.getInstance`
    (picks the start item, `generateSnapshot()`, chooses
    `DynamicRouteState` or `StitchRouteState`), `RouteState.addCorner`,
    `RouteState.keyTyped` (layer keys), `RouteState.cancel`,
    `PopupMenuDynamicRoute`, `PopupMenuStitchRoute`,
    `PopupMenuChangeLayer`.
  - Rust: `tools::route::RouteTool` holds an I01 `RouteSession` handle.
    Mouse move calls `move_to` (dynamic mode) or only updates the rubber
    band (stitch mode); left click calls `add_corner`; layer keys call
    `change_layer` and show the returned status; Enter or Space calls
    `complete`, Esc calls `cancel`. The context menu offers the layer list
    and end/cancel, as Java's route popups do.
  - **Waits on I01.**
  - **Accept:** `tests/ui_interactive.rs`:
    - `route_pin_to_pin_two_layer`: on a two-layer fixture, a scripted
      route from pin A to pin B (raw pointer events, then Enter) leaves A
      and B connected (`connection_items` of A contains B) and the ratsnest
      count is one lower.
    - `escape_cancels_route`: the board's statistics are unchanged after
      Esc.
    - `layer_key_mid_route_adds_via`: pressing `2` mid-route on the same
      fixture adds one via, and the status line shows
      `Layer changed to <name>.`
    - A Xvfb screenshot of a route in progress, looked at by the
      implementer and described in the done note.
  *Done 2026-10-16:* `tools/{host,route}.rs` drive the engine's `Editor` (new `Editor::apply_on`
  lends it the session board for one command, `Editor::detached` makes one without a board; the
  response's journal patches the scene). `crates/freerouting-gui/tests/ui_interactive.rs`, raw pointer
  events on J2 (two layers; the pin pair is found by running the same commands on an `Editor` that
  owns a copy, with the pointer positions the screen gives): `route_pin_to_pin_two_layer` (A's
  connection items reach B, incomplete count 33 -> 32), `escape_cancels_route` (item set, statistics and
  boxes identical after Esc), `layer_key_mid_route_adds_via` (one via, status `Layer changed to B.Cu.`).
  Also `the_app_starts_in_route_mode_like_java`, the route popup (`popup::tests`) and
  `analysis_waits_for_the_route`. Xvfb screenshot of a route in progress (release binary, J2, click on a
  U1 pad, two pointer moves; looked at with Read): the Route button of the new toolbar is lit and Inspect
  and Drag are dimmed, the status bar says `Routing net /MIPI_CSI_CLK_P.`, a thick red trace leaves the
  pad horizontally and turns 45 degrees, a thin yellow rubber band runs on to the target pad on J2, which
  has a cyan outline; "Checks" says `Checking...` because the DRC waits while a route is under way.
  Engine side: `tests/interactive_host.rs` (3 tests).

- [x] **G09.2 — Interactive routing settings.**
  - Java: `WindowRouteParameter` (`route_mode` `dynamic`/`stitching`,
    `rule_selection` `automatic`/`manual`, `push&shove_enabled`,
    `drag_components_enabled`, `via_snap_to_smd_center`,
    `highlight_routing_obstacle`, `automatic_neckdown`,
    `restrict_pin_exit_directions`, `pull_tight_region` (maximum = whole
    board), `pull_tight_accuracy`), and `ManualRulesPanel` (manual via
    rule, clearance class and trace width per layer); P toggles
    push-and-shove (`MenuState.keyTyped`). The board-rule fields of the same
    dialog are G06.6.
  - Rust: the interactive section of `dialogs::route_settings` edits C02's
    interactive settings struct; the app passes it to every I-series call.
    It is persisted with G01.8's settings.
  - **Waits on C02 (and I01 for the behaviour tests).**
  - **Accept:** `tests/ui_interactive.rs`:
    - `interactive_settings_round_trip`: settings → dialog → settings is
      equal, and a save/restore keeps them.
    - `stitch_mode_does_not_follow_cursor`: in stitch mode, moving the
      pointer without clicking adds no trace segment; in dynamic mode it
      does (segment count from the route session).
    - `p_toggles_push_and_shove`.
  *Done 2026-10-16:* `settings::InteractiveSettings` (Java's defaults, stored under the GUI's settings
  key, `#[serde(default)]`) and the interactive section of Settings > Routing
  (`dialogs/route_settings.rs::interactive_section`: route mode, rule selection with the manual rules panel,
  push-and-shove, drag components, via snap, highlight obstacle, neckdown, pull-tight radius as "entire board"
  or a length, pull-tight accuracy). The app passes it to every editor call (`tools/host.rs::edit_settings`).
  `tests/ui_interactive.rs`: `interactive_settings_round_trip` (settings -> dialog clicks -> settings, then
  `save` and `PersistedSettings::restore` through an in-memory `eframe::Storage`),
  `stitch_mode_does_not_follow_cursor` (stitch: `corners_added` 0 and no trace after three moves, 1 corner
  after a click; dynamic: a trace after one move), `p_toggles_push_and_shove` (also: ignored mid-route, and the
  engine's settings follow). The manual rules are not stored (they belong to a board). Note: the checklist said
  "C02's interactive settings struct"; the engine's `interactive::EditSettings` (I01.1) is that struct.

- [x] **G09.3 — Drag mode.**
  - Java: `DragMenuState.mousePressed` → `DragState.getInstance`:
    `findItemToMove` picks on the current layer first, then on every other
    visible layer in ascending order (with "select on all visible layers");
    `selectItemToMove` skips traces, skips component items unless "drag
    components" is on, and stops at the first drill item, else keeps the
    last candidate; nothing movable under the cursor but something found
    gives no drag, nothing found at all gives `MakeSpaceState`.
    `DragItemState.moveTo` moves via `MoveComponent(item, offset, 99, 5)`.
  - Order: the pick order above decides which item moves. It belongs to
    I02; the GUI passes the location, the current layer and the
    visibility of each layer, and draws the result.
  - Rust: `tools::drag::DragTool` calls I02's `DragSession::start`,
    `drag_move_to`, `drag_end`, patching after each move from the journal.
  - **Waits on I02.**
  - **Accept:** `tests/ui_interactive.rs`:
    - `drag_via_moves_attached_traces`: dragging a via by a fixed offset on
      `Issue754-avionics_hub.dsn` moves it by that offset (rounded as I02
      rounds) and its traces stay connected to it; U restores the original
      position exactly.
    - `drag_empty_space_makes_space`: a drag from empty copper next to a
      trace pushes the trace (its geometry changes) and inserts nothing.
    - `drag_component_needs_setting`: with "drag components" off, pressing
      on a pad of a placed component drags nothing.
  *Partial 2026-10-16:* `tools/drag.rs` and `viewport.rs` (press, move and release become `Press`, `DragTo`
  and `Release`; each move patches the scene, `full_rebuilds` unchanged). `tests/ui_interactive.rs`:
  `drag_via_moves_attached_traces` (the via ends where an `Editor` on a copy puts it for the same pointer
  positions, every item the drag pushed too, the incomplete count is unchanged, one `U` restores the content
  exactly), `drag_empty_space_makes_space` (a trace is pushed as the engine pushes it, item count unchanged,
  no hidden-net item left), `drag_component_needs_setting` (off: nothing moves; on: the component moves).
  The test uses J2 with its session, not `Issue754-avionics_hub.dsn`: the four Issue754 vias that have
  traces are `SystemFixed` (DSN wiring), which `unfix` leaves alone and the engine's drag reports as
  `DragStep::Fixed`, as Java does, so the clause cannot be met on that board. *Done 2026-10-16:* the accept
  clause's board is amended to J2 with its session (operator decision, recommended option A, logged in
  `target/orchestrator/operator-decisions.md`).

- [x] **G09.4 — Pin swap.**
  - Java: `MenuState.swapPins` (pick with a pins-only filter; none gives
    `no_pin_selected`) → `PinSwapState` (second click; the messages in the
    decisions). W starts it.
  - Rust: W or the context menu's `swap_pin` starts a two-click tool that
    calls I03's pin swap and shows its result message.
  - **Waits on I03.**
  - **Accept:** `tests/ui_interactive.rs`:
    - `pin_swap_refused_when_connected`: on a pin that has a trace, the
      status line is `Pin not swapped because it is already connected.`
    - `pin_swap_completes`: on two swappable unconnected pins of one
      component, the pins' nets are exchanged and the status line is
      `Pin swap completed.`
  *Done 2026-10-16:* `W` (and the popup's Swap pin, enabled) starts `Editor`'s `StartPinSwap`, the second
  click completes it, the Java message is shown (`text::interactive_status`), the first pin and the swappable
  pins are drawn (`overlays/route.rs`). `tests/ui_interactive.rs`, on rpi_splitter with the engine test's
  logical part for J3 written to a temporary DSN: `pin_swap_completes` (two swappable J3 pins with different
  nets exchange them; `Pin swap completed.`), `pin_swap_refused_when_connected` (with the session loaded;
  `Pin not swapped because it is already connected.`, nets unchanged), `pin_swap_without_a_pin_says_so`
  (`No pin selected.`).

- [x] **G09.5 — Route overlays: trace in progress and obstacle highlight.**
  - Java: the route states draw the trace so far and, with
    `highlight_routing_obstacle`, the item that stopped the route
    (`GraphicsContext` highlight colour).
  - Rust: `overlays::route` draws the session's pending trace and the
    obstacle id returned by I01 as egui shapes (G00: overlays are egui
    shapes). The committed part of a trace arrives through the journal, as
    any other board change.
  - **Waits on I01.**
  - **Accept:** `tests/ui_interactive.rs::obstacle_is_highlighted`: routing
    into a fixed pad of another net with the setting on puts that pad's id
    in the overlay's highlight list; with it off the list is empty. A
    snapshot `route_obstacle_highlight.png` passes on lavapipe.
  *Done 2026-10-16:* `overlays/route.rs` draws the engine's `Overlay` (rubber band, targets, swap pins,
  ellipses, layer-change marker, the obstacle, the floating selection and its violations); the committed
  trace arrives through the journal. `tests/ui_interactive.rs::obstacle_is_highlighted` (a pad of another net
  that stops a route, found with the engine on a copy: `highlights` is `[pad id]` with the setting on and empty
  with it off) and `snapshot_route_obstacle_highlight` -> `tests/snapshots/route_obstacle_highlight.png`
  (lavapipe; looked at: the pad is a brighter red than its neighbours, the airlines run to the right).

- [~] **G09.6 — Move, copy, rotate and cutout (optional).**
  - Java at the pin: `MoveItemState` (rotate keys, change side, reset
    rotation, component grid, wheel rotate or zoom from
    `WindowMoveParameter`), `CopyItemState` and `CutoutRouteState` exist but
    cannot be entered from the GUI (see "What Java can actually do at the
    pin"). `PopupMenuMove_en.properties` still holds their menu texts.
  - *Operator decision* (G00, "Java's disabled edits", and I04's owner
    decision). Only if Dan says yes: `tools::move_component` and a cutout
    tool calling I04, with the component grid and wheel setting in a
    "Component Movement Settings" dialog (Java `WindowMoveParameter`
    texts).
  - **Waits on the decision and I04.**
  - **Accept:** `tests/ui_interactive.rs::rotate_four_times_is_identity`:
    rotating a component by 90° four times gives the same placement and
    pad positions; one undo per rotation.
  *Partial 2026-10-16:* `tools/move_component.rs` and `Command::{MoveSelection, RotateSelection,
  MirrorSelection, ResetRotation, CopySelection, SetComponentGrid}`: `M` picks the selection up, `+` `-`
  `*` (Shift+8) turn it, `/` mirrors, a click puts it down, Esc puts it back, `Ctrl+D` copies, the popup of a
  floating selection (`popup::tests`), the floating items and their violations are drawn, and Settings >
  Component Movement has the placement grid. `tests/ui_interactive.rs::rotate_four_times_is_identity` (a
  component that turns four times on a copy: placement, pad positions and item boxes equal after four turns,
  and one `U` per rotation walks back through the four states), `move_then_click_puts_the_selection_down`,
  `escape_puts_a_moving_selection_back`, `undo_clears_the_editors_selection_but_not_the_guis`,
  `component_movement_settings_set_the_placement_grid`. Remainder: the cutout tool (the engine has no
  `CutoutRouteState`; I04 does not include it) and the wheel setting of `WindowMoveParameter` (zoom or rotate).

- [~] **G09.7 — Frame time while dragging (FT7).**
  - `--bench drag` (added to G01.5's bench scripts) drags one component of
    `Issue190-processor.Z80` along a fixed path for 600 frames in a release
    build, with patching only.
  - **Waits on I02 and G02.4.**
  - **Accept:** on a quiet machine (load average ≤ 1.0, recorded), frame CPU
    p95 ≤ 8 ms and `full_rebuilds == 0` over the run; numbers in the done
    note and in `gui.md`, with a "pending" line for Dan's Windows GPU.
  *Partial 2026-10-16:* `--bench drag` (`bench_drag.rs`, `bench::drag_offset`): Issue190, release,
  600 frames, patching only. Frame CPU p95 5.84 and 5.54 ms, p50 3.27 ms, max 9.28 and 7.46 ms,
  `full_rebuilds` 0, `patches` 598 (two runs; numbers and the method in `gui.md`, "FT7"). The machine was
  not quiet (other agents routing and building: 1-minute load average 7 to 12, the clause needs at most
  1.0), so the timing clause is not met as written. Remainder: repeat on a quiet machine (owner: P01, the
  measurement runs) and on Dan's Windows GPU (owner: G00, "Frame targets"; pending line in `gui.md`).
