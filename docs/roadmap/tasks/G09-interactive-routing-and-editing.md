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
| Interactive engine | **missing** at HEAD: no `freerouting::interactive` module (I01–I04) |
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

- [ ] **G09.1 — Route mode.**
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

- [ ] **G09.2 — Interactive routing settings.**
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

- [ ] **G09.3 — Drag mode.**
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

- [ ] **G09.4 — Pin swap.**
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

- [ ] **G09.5 — Route overlays: trace in progress and obstacle highlight.**
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

- [ ] **G09.6 — Move, copy, rotate and cutout (optional).**
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

- [ ] **G09.7 — Frame time while dragging (FT7).**
  - `--bench drag` (added to G01.5's bench scripts) drags one component of
    `Issue190-processor.Z80` along a fixed path for 600 frames in a release
    build, with patching only.
  - **Waits on I02 and G02.4.**
  - **Accept:** on a quiet machine (load average ≤ 1.0, recorded), frame CPU
    p95 ≤ 8 ms and `full_rebuilds == 0` over the run; numbers in the done
    note and in `gui.md`, with a "pending" line for Dan's Windows GPU.
