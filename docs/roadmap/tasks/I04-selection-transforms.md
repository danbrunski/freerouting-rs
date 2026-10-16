# I04 — Move, rotate, mirror and copy a selection (optional)

Opened 2026-10-15. Java's GUI could once pick up the selected components or
items, move them with the mouse, turn them by 90° or any angle, flip them to
the other side, and drop them (`MoveItemState`), or place a copy
(`CopyItemState`). At `cda540f` both states have no callers:
`GuiBoardLegacyEditActions.moveSelectedItems` and `copySelectedItems` are
no-ops. The engine helper `ComponentItemTransform` is still tested in Java.
This checklist exists so the feature can be built if Dan wants it; **do not
start it until decision 1 in [I00](I00-interactive-README.md) says so**, and
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

- [ ] **I04.1 — `ComponentItemTransform`.**
  - Java: `ComponentItemTransform.apply` and helpers.
  - Rust: `interactive/transform.rs`, `apply_transform(board, detached,
    angle, mirror, pole) -> Result<Vec<ItemId>, TransformError>`.
  - Rules: detached items only; finite angle; transformed copies keep
    contacts, end points and DRC (`legalTraces`, `legalDirection` per angle
    restriction), else the originals are restored; `Math.toRadians` then
    `sin`/`cos` as Java (one ulp accepted, D6).
  - **Accept:** `ComponentItemTransformTest` (6) ported: shared junction
    via, undo/redo, conflict rejection, illegal angles.

- [ ] **I04.2 — Move, rotate and mirror of a selection.**
  - Java: `MoveItemState` (all engine parts: grid snapping of the move
    vector, 90°/45°/any-angle turns, side change, reset rotation, complete
    and cancel; the net-items bookkeeping that feeds the ratsnest).
  - Rust: `Command::{MoveSelection { to }, RotateSelection { degrees },
    MirrorSelection, ResetRotation}` arms in `interactive/editor.rs`, using
    `interactive/transform.rs`.
  - **Accept:** the transform cases of `Issue955PlacementLifecycleTest`
    listed above ported; rotating a component by 90° four times returns it
    to its placement (`state_hash` equal).

- [ ] **I04.3 — Copy of a selection.**
  - Java: `CopyItemState` (`changePadstackLayers` creates or reuses a
    padstack per layer pair; `insert` copies items with `copy(0)`, adds
    packages and components for copied component items, then inserts).
  - Rust: `interactive/copy.rs`, `Command::CopySelection { to }`.
  - **Accept:** `CopyItemPreviewTest` (the source stays untouched during the
    preview) and the copy cases of `Issue955PlacementLifecycleTest` ported;
    `copy_burns_ids_in_java_order` on a 3-item selection.

- [ ] **I04.4 — Replay, determinism and divergence notes.**
  - `examples/edit_script.rs` accepts the new commands; DIVERGENCES lines
    for "disabled in Java's GUI at the pin; enabled here by decision".
  - **Accept:** a committed script (select, move, rotate, mirror, copy,
    undo) on bm05 replays to identical output under
    `tools/determinism-check.sh`.
