# I02 — Drag, MoveComponent and make-space

Opened 2026-10-15. In Java's drag mode a press on a via, pin or component
starts dragging it, shoving traces aside (`DragItemState` with
`board.actions.MoveComponent`); a press on empty space starts "make space",
which pushes copper away with an invisible trace on a hidden net
(`MakeSpaceState`). Both are reachable at `cda540f`. This checklist ports
them into `freerouting::interactive`. See [I00](../I00-interactive-README.md)
for the API and the series rules.

**Prerequisites:** I01 (`RouteSession`, settings, the replay example); O04
(`move_drill_item`, `DrillItemMover`); B04 (`check_move_item`, filtered
`pick_items`); B05 (`move_by` for items, `DrillItem.min_width`).

## What exists and what this adds

| Exists | Where |
|---|---|
| `Components::move_by` and friends | `board/structure/component.rs:349-368` |
| `ObstacleArea` / `ComponentOutline` transforms | `board/items/mod.rs:313-474` |
| `move_drill_item` | O04: `shove/forced.rs` |
| `RouteSession` | I01: `interactive/route.rs` |

| Java at `cda540f` (`src/main/java/app/freerouting/`) | Lines |
|---|---|
| `board/actions/MoveComponent.java`: constructor `:41-111`, `getMinDrillItemWidthWithTraces` `:131`, `check` `:146-207`, `insert` `:214-260`, `SortedItem` `:267-273`; limits `CHECK_TIME_LIMIT = 3000`, `PULL_TIGHT_TIME_LIMIT = 1000` `:27-28` | 274 |
| `gui/interactive/DragState.java`: `getInstance` `:29-47`, `findItemToMove` `:49-77`, `selectItemToMove` `:79-99`, `mouseDragged` `:110-124` | 133 |
| `gui/interactive/DragItemState.java`: `moveTo` (two tries, `MoveComponent(item, rel, 99, 5)`) | 182 |
| `gui/interactive/MakeSpaceState.java`: constructor, `moveTo`, `buttonReleased` | 98 |
| `rules/Nets.java`: `hidden_net_number = 10000001` `:19` | — |
| `src/test/java/app/freerouting/board/actions/MoveComponentTest.java` | 8 tests |

## Decisions made here

- **`MoveComponent` lives in `interactive`**, not `shove`: only interactive
  code calls it (the autorouter doesn't).
- **Fixed order where Java uses an identity-hash set.** `MoveComponent` keeps
  `itemGroupSet`, `itemsToShove` and the check's ignore collection as
  `HashSet<Item>` (`:35,79,159`). `Item` has no `hashCode`, so any
  iteration follows identity hashes. Rust uses item-id order (sorted
  `Vec`/`BTreeSet`). Add a DIVERGENCES line only if an iteration over these sets can
  change a result: verify first, at the pin, which uses depend on order (I02.1's done note
  records the answer). At `cda540f` I found these
  sets used for membership, for `removeAll` (whose result doesn't depend on
  order) and for a copy into a `LinkedList` in `DrillItemMover.shoveVias`
  (`:222`); the Java session reports an order-dependent use, possibly on
  its main. Acceptance compares with Java only where the result can't
  depend on that order (I02.4: drags of a single drill item, or of a group
  whose shove checks pass for every order).
- **Drag sessions are engine state.** `DragSession::{Item(..),
  MakeSpace(RouteSession)}`; the GUI sends `Press`, `DragTo`, `Release`.

## File ownership

| Item | New files | May edit |
|---|---|---|
| I02.1 | `interactive/move_component.rs` | `interactive/mod.rs` (append), `docs/DIVERGENCES.md` |
| I02.2 | `interactive/drag.rs` | — |
| I02.3 | — | `interactive/drag.rs`, `rules/mod.rs` (append `HIDDEN_NET_NUMBER`) |
| I02.4 | `tests/interactive_drag.rs`, `tools/java-reference/probes/board/DragScriptProbe.java`, `fixtures/reference/probe/drag-script/*` | `tools/java-reference/README.md` |
| I02.5 | — | `examples/edit_script.rs` (append drag commands) |

## Checklist

- [x] **I02.1 — MoveComponent.**
  - Java: `MoveComponent` (all).
  - Rust: `interactive/move_component.rs`.
  - Orders (exact):
    - group: the component's items in item-list order (id descending), or
      the item alone; any user-fixed item, or one that isn't a drill item,
      obstacle area or component outline, makes the group immovable;
    - gravity point: mean of drill centres (else the component location,
      else bounding-box centres), each sum in `f64`, then `round`;
    - sort by `(gravity − centre) · vector` with Java's stable sort and
      `Signum` comparison;
    - `check`: one limit for the whole check (3000 nominal ms, a WorkLimit at `30baad7e2`);
      a drill item with traces or several nets uses `DrillItemMover.check`
      unless the move is at least its minimum width (then it fails); other
      drill items try `check_move_item` first, then `DrillItemMover.check`
      if the move is shorter than their minimum width;
    - `insert`: move the component first; per item, `move_drill_item` (the
      1000 pull-tight limit) or `move_by`; a failure translates the
      component back and returns false; finally invalidate the outline's
      edge-pin nets.
  - **Accept:** `MoveComponentTest` (8 tests) ported with behavioural asserts
    instead of Mockito spies; `move_component_order_is_item_id_order`
    (the DIVERGENCES line pinned).
  - *Done 2026-10-16:* `interactive/move_component.rs` (`MoveComponent::new/check/insert`,
    `min_drill_item_width_with_traces`, `requires_drill_item_mover`, `MoveBudget::JAVA` =
    3000/1000 nominal ms). Order check at `a773a70ec`: `itemGroupSet`, `itemsToShove` and the
    check's ignore set are used only by `contains`, `removeAll` (`DrillItemMover.shoveVias`) and
    the `LinkedList` copy of `:222`, which is itself only tested for membership; the ignore set
    starts as the whole group, so the drill items the check adds are already in it. No result
    depends on the set order, so no DIVERGENCES line (the decision's condition);
    `move_component_order_is_item_id_order` pins the item-list order (highest id first), the
    stable `Signum` sort and the tie order. `cargo test -p freerouting --test java_ports
    board_actions_move_component` → 10 passed (the 8 `MoveComponentTest` ports, the order test,
    `a_user_fixed_item_makes_the_group_immovable`).

- [x] **I02.2 — Drag of items.**
  - Java: `DragState.getInstance/findItemToMove/selectItemToMove`,
    `DragItemState.moveTo`, `DragState.mouseDragged`.
  - Rust: `interactive/drag.rs`, `DragSession::start(board, settings, at)`
    and `drag_to`, `release`.
  - Orders (exact): pick on the current layer, then, with "select on all
    visible layers", layers 0 … n−1 except the current and invisible ones,
    with the settings' item filter; in each pick traces are skipped,
    component items only when component dragging is on, and the first
    drill item wins (else the last non-trace item); nothing found at all →
    make-space; only traces found → no drag. `drag_to`: up to two tries
    with `MoveComponent(item, delta, 99, 5)`; before the second, shorten the
    move to `2 · min_trace_half_width` (or `max(1, min_drill_width − 1)`
    when that is smaller and positive); snapshot on the first successful
    step; an insert failure ends the drag and undoes, reporting changed
    nets.
  - **Accept:** I02.4.
  - *Done 2026-10-16:* `interactive/drag.rs`: `DragSession::start(board, settings,
    drag_settings, at)` (`find_item_to_move` transcribes Java's loop, including its layer
    order), `drag_to` (angle projection, `DragStep::Fixed` for a user-fixed item, the two
    `MoveComponent(item, delta, 99, 5)` tries with the shortened second move, snapshot on the
    first success, `DragStep::Failed { vector, changed_nets }` after the undo of a failed
    insert), `release` (`DragRelease::Item { changed_nets }`, Java's nets). `DragSettings`
    carries Java's `dragComponentsEnabled` and item filter (defaults on / `ItemFilter::default`)
    because `settings.rs` is I01's. Accept I02.4: `drag_script_matches_java_*` 5 passed; own
    tests `press_on_a_trace_starts_nothing_and_a_fixed_via_does_not_move`,
    `a_drag_is_undone_by_one_undo` (`cargo test -p freerouting --test interactive_drag` → 9
    passed).

- [x] **I02.3 — Make-space.**
  - Java: `MakeSpaceState`.
  - Rust: `interactive/drag.rs`; `rules::HIDDEN_NET_NUMBER = 10000001`.
  - Rules: half width `max(min(100, min_trace_half_width / 10), 5)` on
    every layer, all layers active, push on, no via rule, no start item or
    targets; snapshot on the first drag; each drag is a `next_corner`; in
    angle mode NONE, when the route end differs from the rounded pointer,
    `Response.warp_pointer` carries the route end; release removes every
    connectable item of the hidden net.
  - **Accept:** `make_space_leaves_no_hidden_net_items`; I02.4's make-space
    cases.
  - *Done 2026-10-16:* `RouteSession::make_space` (`interactive/route/make_space.rs`; the
    session's start item and target set became `Option`s for Java's `null`s, so
    `connectToTarget` keeps the nearest item as Java does) and `MakeSpaceDrag` in `drag.rs`;
    the hidden net is the existing `rules::Nets::HIDDEN_NET_NUMBER` (10000001), so no second
    constant was added. The warp point is `DragStep::MadeSpace { warp_pointer }` (I03's
    `Response` takes it over). `make_space_leaves_no_hidden_net_items` passes (hidden trace on
    the board during the drag, none after `release`); the probe's make-space cases match on all
    5 boards (15 presses: 9 make-space drags run, 5 found an item, 1 only a trace).

- [x] **I02.4 — Probe `drag-script`.**
  - Java: `DragScriptProbe` drives `MoveComponent` and `Route` headless.
  - Boards: `Issue026-J2_reference` + SES, `Issue508-DAC2020_bm05` + SES,
    `Issue508-DAC2020_bm08` + routed SES, `Issue143-rpi_splitter` + SES,
    `Issue957-smoke-ldo-led` as loaded (its DSN holds wiring).
  - Script: 10 seeded vias and 5 seeded single-pin drill items per board,
    each dragged in 5 steps of 200 units in a seeded 45° direction (with
    the I02.2 retry), undoing on failure; 3 make-space drags of 8 steps
    per board.
  - **Accept:** after each step, the outcome, `maxGeneratedId` and every
    10th canonical dump are equal on all 5 boards. Component drags (several
    items) are compared only where every order of the group passes the
    check (the probe reports that), per the decision above.
  - *Done 2026-10-16:* `tools/java-reference/probes/board/DragScriptProbe.java`,
    `fixtures/reference/probe/drag-script/` (5 `.jsonl`), `tests/interactive_drag.rs`:
    `cargo test -p freerouting --test interactive_drag drag_script` → 5 passed
    (`drag_script_matches_java_{j2,bm05,bm08,rpi_splitter,issue957}`), every step line
    (outcome, vector, centre, component location, `maxGeneratedId`) and every dump equal.
    Generated at `a773a70ec` with `JDK_JAVA_OPTIONS=-Xmx4g REFERENCE_WORK_DIR=… generate.sh
    <pin> drag-script` → 5 boards (byte-identical to the direct `probe.sh` runs; Java J2 1.0 s).
    Two choices beyond the checklist: the session import and Issue957's DSN user-fix all
    wiring, so the probe unfixes every user-fixed trace and via first (otherwise every via step
    is `fixed`); the 5 pins are drawn from all pins with a net, so a pin of a several-item
    component drags the component (all 25 pin drags: `group` 2-63). They are compared too: I02.1
    showed the check's ignore set is the whole group from the start and the check does not
    change the board, so no order of Java's sets can change a result. Outcomes over the 5
    boards: 320 moved, 21 blocked, 7 failed (undone), 0 exceptions.

- [x] **I02.5 — Replay and determinism.**
  - `examples/edit_script.rs` accepts `press`, `drag_to`, `release` in
    drag mode.
  - **Accept:** a committed 40-step drag script on bm05 replays to
    identical output under `tools/determinism-check.sh`.
  - *Done 2026-10-16:* `tests/support/drag_script.rs` (format and replay; a script with
    `"tool": "drag"` is a drag script, `"unfix": true` unfixes the wiring as the probe does),
    `examples/edit_script.rs` dispatches on it (`--unlimited` → `DragBudget::UNLIMITED`),
    `tests/data/edit_script_drag_bm05.json` (40 commands: the probe's via drags 0-2, two pin
    drags and three make-space steps). `tools/determinism-check.sh --out … --
    <target>/release/examples/edit_script fixtures/Issue508-DAC2020_bm05.dsn
    fixtures/reference/route/Issue508-DAC2020_bm05.ses
    crates/freerouting/tests/data/edit_script_drag_bm05.json {OUT}` → `identical` (327 lines;
    load 4.38 idle, 4.75 with 8 burners). `edit_script_replays_the_probe_drags`: the replay's
    first 21 lines equal the probe's via drags 0-2, and Java and unlimited budgets agree.
