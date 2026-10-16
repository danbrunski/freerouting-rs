# I04 — Move, rotate, mirror and copy a selection (optional)

Opened 2026-10-15. Java's GUI could once pick up the selected components or
items, move them with the mouse, turn them by 90° or any angle, flip them to
the other side, and drop them (`MoveItemState`), or place a copy
(`CopyItemState`). At `cda540f` both states have no callers:
`GuiBoardLegacyEditActions.moveSelectedItems` and `copySelectedItems` are
no-ops. The engine helper `ComponentItemTransform` is still tested in Java.
This checklist exists so the feature can be built if Dan wants it; **do not
start it until decision 1 in [I00](../I00-interactive-README.md) says so**, and
not before the G series has selection.

**Prerequisites:** decision 1; I03; B05 (item `translate_by`,
`turn_90_degree`, `rotate_approx`, `change_placement_side`, `copy(0)`
burning an id, `Components::copy`); B08 (`detach_item_for_move`).

## What exists and what this adds

| Exists | Where |
|---|---|
| Component transforms `Components::move_by/turn/rotate/change_side` | `board/structure/component.rs:349-368` |
| Padstack and package libraries (`add`) | `library/padstack.rs:269`, `library/package.rs:164` |
| `clearance_violations` of an item | `drc/clearance.rs:157` |
| `Editor`, selection, history | I03 |

| Java at `cda540f` (`src/main/java/app/freerouting/`) | Lines |
|---|---|
| `board/actions/ComponentItemTransform.java`: `apply` `:32-139`, `transformed` `:152`, `alignCopper` `:162`, `legalTraces` `:208`, `legalDirection` `:239` | 298 |
| `gui/interactive/MoveItemState.java`: `getInstance` `:85-196`, `move` `:270`, `adjustToPlacementGrid` `:294`, `turn90Degree` `:309`, `rotate` `:317`, `transformItems` `:323`, `turn45Degree` `:348`, `changePlacementSide` `:357`, `resetRotation` `:385`, `complete` `:225`, `cancel` `:251` | 459 |
| `gui/interactive/CopyItemState.java`: `changePadstackLayers` `:83`, `changePosition` `:131`, `changeLayerAction` `:145`, `insert` `:156-290` | 331 |
| Tests: `board/actions/ComponentItemTransformTest` (6), `fixtures/Issue955PlacementLifecycleTest` transform and copy cases (`quarterTurnPreservesContact`, `arbitraryTurnPreservesContact`, `mirrorPreservesContact`, `copyConstructionPreservesLocation`, `copyRebindingInvalidatesCachedPadGeometry`, `copyRebindingInvalidatesCachedKeepoutGeometry`, `backSideRotateFirstQuarterTurnUsesWorldDirection`, `backSideMirrorFirstReflectionUsesWorldDirection`), `gui/interactive/CopyItemPreviewTest` (1) | `src/test/java/app/freerouting/` |

## Decisions made here

- **Floating items are an editor-side preview**, not board items: the
  selection is detached (`detach_item_for_move`), transformed copies are
  kept in the session, and only `complete` inserts them. `cancel` reinserts
  the originals. Java does the same with detached items and
  search-tree-only insertion for its DRC check.
- **Order-independent hash maps stay maps.** `ComponentItemTransform` uses
  `HashMap`/`HashSet` only for membership and for results that don't depend
  on visit order (`alignCopper`'s group result). Rust may use any map there;
  no divergence.
- **Copy burns ids like Java.** `copy(0)` takes a new id per copied item in
  Java's order (`CopyItemState.java:56,63,237`).

## File ownership

| Item | New files | May edit |
|---|---|---|
| I04.1 | `interactive/transform.rs` | `interactive/mod.rs` (append) |
| I04.2 | — | `interactive/transform.rs`, `interactive/editor.rs` (command arms) |
| I04.3 | `interactive/copy.rs` | `interactive/editor.rs` (command arms) |
| I04.4 | `tests/interactive_transform.rs` | `examples/edit_script.rs` (append), `docs/DIVERGENCES.md` |

## Checklist

- [x] **I04.1 — `ComponentItemTransform`.**
  - Java: `ComponentItemTransform.apply` and helpers.
  - Rust: `interactive/transform.rs`, `apply_transform(board, detached,
    angle, mirror, pole) -> Result<Vec<ItemId>, TransformError>`.
  - Rules: detached items only; finite angle; transformed copies keep
    contacts, end points and DRC (`legalTraces`, `legalDirection` per angle
    restriction), else the originals are restored; `Math.toRadians` then
    `sin`/`cos` as Java (one ulp accepted, D6).
  - **Accept:** `ComponentItemTransformTest` (6) ported: shared junction
    via, undo/redo, conflict rejection, illegal angles.

  *Done 2026-10-16:* `interactive/transform.rs`, `apply_transform(board, detached: &[Item], angle, mirror, pole) -> Result<Vec<Item>, TransformError>`. It returns the transformed copies (not ids): detached items are not in the board's arena, so the copies are values the caller inserts; Java returns `List<Item>` too. A refusal names its reason (`Rejection`), Java returns `null`. The detached items go into the trees for a moment through `UndoableObjects::insert_untracked` (new, no undo bookkeeping, unit test `untracked_objects_leave_the_structure_as_it_was`). All six `ComponentItemTransformTest` tests are ported in `tests/java_ports.rs` `board_actions_component_item_transform` (8 tests: the four parameter sets of the shared-junction test, conflicting anchors -> `ConflictingAnchors`, new foreign-net clearance -> `NewViolations`, non-grid escape rotates rigidly, illegal trace angle -> `IllegalTrace`; `componentRotationApiUsesExactQuarterTurns` was B05's). Differential: `transform_matches_java_{j2,bm05,issue955}` compare 132, 132 and 80 probe lines (accept/refuse per operation, floating items, ids, component poses, default-tree fingerprints) with Java at `a773a70ec`. `cargo test -p freerouting --test java_ports --test interactive_transform`: 56 and 17 passed.

- [x] **I04.2 — Move, rotate and mirror of a selection.**
  - Java: `MoveItemState` (all engine parts: grid snapping of the move
    vector, 90°/45°/any-angle turns, side change, reset rotation, complete
    and cancel; the net-items bookkeeping that feeds the ratsnest).
  - Rust: `Command::{MoveSelection { to }, RotateSelection { degrees },
    MirrorSelection, ResetRotation}` arms in `interactive/editor.rs`, using
    `interactive/transform.rs`.
  - **Accept:** the transform cases of `Issue955PlacementLifecycleTest`
    listed above ported; rotating a component by 90° four times returns it
    to its placement (`state_hash` equal).

  *Done 2026-10-16:* `interactive/move_items.rs` (`MoveSession`: `getInstance` with the component and connected-set extension and its refusals, the grid snap, move, `turn90Degree`, `rotate`, `changePlacementSide`, `resetRotation`, `complete`, `cancel`; new file, the checklist named `transform.rs`) and `Command::{MoveSelection { to }, RotateSelection { degrees }, MirrorSelection, ResetRotation}` in `interactive/editor.rs` (`ModeKind::Moving`, floating items and violations in `Overlay`). The transform cases of `Issue955PlacementLifecycleTest`: `quarter_turn_preserves_contact`, `arbitrary_turn_preserves_contact`, `mirror_preserves_contact` in `tests/interactive_transform.rs`; the other cases of that class are B05.8's in `tests/item_transform.rs` (`back_side_*`, `copy_*`, `shape_and_pin_use_same_anchor`, ...). `four_quarter_turns_leave_the_board_state_unchanged`: a cancelled move with four 90-degree turns has the same `state_hash`, components and item hash as one without; `four_quarter_turns_return_the_selection_to_its_placement`: the floating geometry equals the start after four turns and a completed move equals the original items but for their ids. Probe comparison as in I04.1 (12 move trials per stage with moves, turns, 45/-45/30/135/12.5 degree rotations, mirrors, resets, complete and cancel; 5 refusal reasons). DIVERGENCES: "Interactive move and copy of a selection".

- [x] **I04.3 — Copy of a selection.**
  - Java: `CopyItemState` (`changePadstackLayers` creates or reuses a
    padstack per layer pair; `insert` copies items with `copy(0)`, adds
    packages and components for copied component items, then inserts).
  - Rust: `interactive/copy.rs`, `Command::CopySelection { to }`.
  - **Accept:** `CopyItemPreviewTest` (the source stays untouched during the
    preview) and the copy cases of `Issue955PlacementLifecycleTest` ported;
    `copy_burns_ids_in_java_order` on a 3-item selection.

  *Done 2026-10-16:* `interactive/copy.rs` (`CopySession`, `change_padstack_layers`) and `Command::CopySelection { to }` (`ModeKind::Copying`; `Click` inserts, `SetLayer` is `changeLayerAction`). `CopyItemPreviewTest` (both parameters) is `copy_preview_follows_cursor_without_moving_source_pin` and `..._keepout`: the preview's exact geometry moves by the board offset, keeps its size, and the source component's exact location is unchanged. The copy cases of `Issue955PlacementLifecycleTest` (`copyConstructionPreservesLocation`, `copyRebindingInvalidatesCachedPadGeometry`, `copyRebindingInvalidatesCachedKeepoutGeometry`) were ported by B05.8 in `tests/item_transform.rs` (`copy_construction_preserves_location`, ...). `copy_burns_ids_in_java_order`: three preview ids from `max + 1` in selection order, then one id per inserted copy; the probe compares preview ids, inserted ids, padstack and package counts (layer changes), components and `maxGeneratedId` on 8 trials per stage and board.

- [x] **I04.4 — Replay, determinism and divergence notes.**
  - `examples/edit_script.rs` accepts the new commands; DIVERGENCES lines
    for "disabled in Java's GUI at the pin; enabled here by decision".
  - **Accept:** a committed script (select, move, rotate, mirror, copy,
    undo) on bm05 replays to identical output under
    `tools/determinism-check.sh`.

  *Done 2026-10-16:* `tests/support/editor_script.rs` (the editor script format used by `examples/edit_script.rs`) has `move_selection`, `rotate_selection`, `mirror_selection`, `reset_rotation`, `copy_selection` and the option `component_grid`; `crates/freerouting/tests/data/edit_script_i04_bm05.json` (22 commands: select, move, four turns, 45 degrees, mirror, reset, complete, undo, select, copy twice, cancel, undo twice) on bm05 without a session. `tools/determinism-check.sh --out <dir> -- target/.../examples/edit_script fixtures/Issue508-DAC2020_bm05.dsn crates/freerouting/tests/data/edit_script_i04_bm05.json {OUT}` (idle, then beside 8 burners) printed `identical`; `the_edit_script_replays_identically` replays it twice in the test. DIVERGENCES lines under "Interactive move and copy of a selection" (disabled in Java's GUI at the pin, enabled by decision 1; the plane-area and multi-net conduction area refusals; the command semantics).
