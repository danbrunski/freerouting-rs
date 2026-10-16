# I00 — Interactive editing

Opened 2026-10-15. The I series ports Java's interactive editing
(`gui.interactive`, 27 classes, 5,321 lines at `cda540f`) as engine code:
`freerouting::interactive`, with a lean command/response API (D7: the engine
owns behaviour, the GUI owns input). It does not mirror Java's state classes.
The engine takes commands in board coordinates and returns a response with the
board changes, a typed status message and overlay data; the GUI maps keys and
the mouse to commands and draws. The G series wires it into the desktop app
later ([G09](G09-interactive-routing-and-editing.md)) and references I01–I04. The
narrative is in [optimizer-and-editing.md](../optimizer-and-editing.md).

## Task index

| Task | Outcome | Size / risk | Prerequisite |
|---|---|---|---|
| [I01](I01-route-session.md) | `RouteSession`: route start, dynamic and stitch routing with push-and-shove, neckdown, SMD snap, layer change with vias, cancel; parameter resolution; overlay data | L / medium-high | O01, O04; B01 (snapshots, change journal), B04 (`pick_nearest_routing_item`, filters), B05 (`connect_to_trace`, swappable pins, neckdown half width) |
| [I02](I02-drag-and-make-space.md) | `MoveComponent`, drag of drill items and components, make-space | M / medium | I01; O04 (`move_drill_item`); B04 (`check_move_item`); B05 (item moves) |
| [I03](I03-editor-commands.md) | `Editor::apply` dispatch, selection, pin swap, keepout construction, undo/redo with deltas; delete and fix/unfix by decision 1 | M–L / medium | I01, I02; B04, B05 (pin swap) |
| [I04](I04-selection-transforms.md) | Move, rotate, mirror and copy of selected items (optional; decision 1) | M / medium | I03; B05 (transforms, `copy(0)`), B08 (`detach_item_for_move`) |

## Order and critical path

```
O01 + O04 ─> I01 ─> I02 ─> I03 ─> I04 (only if decision 1 says so)
```

The I series is not on the router's critical path. I01 can start as soon as
O04 lands, in parallel with O05, O06 and the A series. Run I01–I03 in order:
I02 and I03 build on I01's session types.

## Parallel lanes and file ownership

| Checklist | Creates (under `crates/freerouting/`) | May edit |
|---|---|---|
| I01 | `src/interactive/{mod.rs, settings.rs, route.rs, overlay.rs}`, `tests/interactive_route.rs`, `examples/edit_script.rs`, probe `RouteScriptProbe` | `src/lib.rs` (append `pub mod interactive;`), `docs/DIVERGENCES.md`, `tools/java-reference/README.md` |
| I02 | `src/interactive/{move_component.rs, drag.rs}`, `tests/interactive_drag.rs`, probe `DragScriptProbe` | `src/interactive/mod.rs` (append), `src/rules/` (one constant, I02.3), `examples/edit_script.rs` (append commands), `docs/DIVERGENCES.md` |
| I03 | `src/interactive/{editor.rs, select.rs, pin_swap.rs, construct.rs, history.rs, edit_ops.rs}`, `tests/interactive_editor.rs`, probe `EditorProbe` | `src/interactive/mod.rs` (append), `examples/edit_script.rs` (append), `docs/DIVERGENCES.md` |
| I04 | `src/interactive/{transform.rs, copy.rs}`, `tests/interactive_transform.rs` | `src/interactive/editor.rs` (new command arms), `src/interactive/mod.rs` (append), `examples/edit_script.rs` (append), `docs/DIVERGENCES.md` |

## The API the G series uses

```rust
pub struct Editor { /* RoutingBoard, EditSettings, mode, selection, sessions */ }
impl Editor {
    pub fn new(board: RoutingBoard, settings: EditSettings) -> Self;
    pub fn apply(&mut self, cmd: Command) -> Response;   // synchronous, budgeted, deterministic
    pub fn board(&self) -> &RoutingBoard;
    pub fn into_board(self) -> RoutingBoard;
}
pub enum Command {   // board coordinates; the GUI converts from the screen
    Press(FloatPoint), Move(FloatPoint), DragTo(FloatPoint), Release, Click(FloatPoint),
    Complete, Cancel, SetTool(Tool), SetLayer(i32), UpdateSettings(EditSettings),
    Undo, Redo, Snapshot, StartRoute { at: FloatPoint }, StartPinSwap { at: FloatPoint },
    SelectAt(FloatPoint), SelectRegion(IntBox), ToggleSelect(FloatPoint),
    ExtendSelection(Extent), FilterSelection, InsertKeepout { shape: KeepoutShape, layer: i32 },
    AddHole { area: ItemId, hole: PolygonShape },
    // by decision 1: DeleteSelection, FixSelection(bool) (I03);
    // MoveSelection, RotateSelection, MirrorSelection, CopySelection (I04)
}
pub struct Response {
    pub mode: ModeKind,                 // Menu(Tool) | Routing { stitch } | Dragging | MakingSpace | Selecting | PinSwap | Constructing
    pub delta: BoardChanges,            // B01.7 take_changes(): ids, changed nets, dirty box
    pub status: Option<Status>,         // Java's message key + arguments; the GUI localizes
    pub overlay: Overlay,               // route targets, swap pins, failing obstacle, selection
    pub layer: i32,                     // the current layer (route start and layer change move it)
    pub warp_pointer: Option<FloatPoint>,
}
```

Names may change in I01–I03; the shape stays: one `apply`, one `Response`, no
UI types (no colours, screen coordinates or toolkit types).

What stays in the GUI: key and mouse bindings (Java: `u`/`b` undo/redo,
`d`/`r`/`s` tools, `t`, `w` pin swap, `p` push toggle, digits and `+`/`-` for
layers, Esc/Enter/Space), popup menus and toolbars, overlay drawing,
localized messages, the item-info window, the zoom region, coalescing pointer
moves to one `Move` per frame, and the expand-test debug view (dropped until
the A series exposes rooms as drawable data).

## Rules for this series

These add to the shared rules and to [O00](O00-optimizer-README.md)'s rules
(probes, dumps, explicit outcomes, ids), which apply here too.

- **Engine only.** `freerouting::interactive` depends on the board and the
  O series, never on a GUI crate.
- **Deterministic and replayable.** `Editor::apply` runs to completion.
  Every Java `TimeLimit` in this code (Route's 3000 check and 2000
  pull-tight, MoveComponent's 3000/1000) is a WorkLimit at the fork's
  `30baad7e2` (J01's re-pin target): nominal ms × 64,000 tree-node visits.
  Rust uses those budgets (O00's work-budget rule) and never the clock. `examples/edit_script.rs` replays a JSON
  command script on a DSN (+ SES) and writes the canonical dump; replaying
  twice, idle and under load, gives identical output
  (`tools/determinism-check.sh`).
- **Snapshots where Java takes them**: route start (after any
  `connectToTrace`, `RouteState.java:132`), each stitch corner, each via try
  in a layer change (undone on failure), after a successful layer change,
  `Snapshot` in dynamic routing, the first drag step, pin swap, each keepout
  construction. Undo and redo only in menu mode
  (`GuiBoardHistoryController.canChangeHistory`).
- **Probes drive Java headless.** `Route`, `MoveComponent`, `Pin.swap`,
  `checkChangeNet`, `checkShape` and `insertObstacle` need only a
  `RoutingBoard`. The `*State` classes need a `GuiBoardManager` (Swing);
  probes don't construct it. Where a state's logic must be compared, the
  probe transcribes that state's few board calls and says so in its header.
- **Typed status.** Status messages carry Java's resource keys (for example
  `pin_not_swapped_because_it_is_already_connected`) and arguments; the GUI
  owns the text.

## Open questions for Dan

1. **Java's disabled GUI edits.** At `cda540f`,
   `gui/workspace/controllers/GuiBoardLegacyEditActions.java` makes these
   no-ops: fix and unfix selected, assign to a new net or group, delete,
   cutout, assign clearance class, move and copy selected, optimize and
   autoroute selected. `MoveItemState`, `CopyItemState` and
   `CutoutRouteState` have no callers. G00's question 6 asks the same for
   the GUI buttons; this is the engine half of one decision.
   *Recommendation* (the same as G00's):
   - port **delete** and **fix/unfix** in I03 (I03.6), each with a
     DIVERGENCES line; they are small and their engine pieces exist (O01's
     `remove_items_and_pull_tight`, the board's fixed-state setter);
   - leave **cutout** of a selection out until you ask (O03's
     `cutout_trace` makes it a small addition later);
   - leave the assign-net/group/clearance-class edits to B08's rule-edit
     facade when the G series asks for them;
   - **I04** (move, rotate, mirror and copy of a selection) only once the
     GUI has selection, and only if you want it;
   - build "optimize/autoroute selection" later on the batch engine (A and
     O06 stages restricted to an item set) instead of reviving the dead Java
     paths.
