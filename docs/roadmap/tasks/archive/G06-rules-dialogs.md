# G06 — Rules dialogs

Opened 2026-10-15. Java's Rules menu lets the user edit net classes, the
clearance matrix, vias, via padstacks and via rules, and its Settings menu
holds the routing settings that are board rules. These dialogs are part of
the Java feature set ("What 'complete' means", item 5). Every edit changes
board rules, and several must keep the search trees right (a clearance
change moves every item's tree shape), so the dialogs only call B08's
rules-edit facade. This is the report's GUI-6 (GUI, plugin and packaging
report §1.4, §1.5, §4).

**Prerequisites:**
- B08: the rules-edit facade (set a clearance, add or prune a class, set a
  net's class, set a net-class width or clearance class and reapply it to
  unfixed items as Java does, via info, padstack and via-rule edits). B08's
  own tests prove the facade keeps the search trees right; G06 proves the
  dialogs call it correctly.
- [G03](G03-ratsnest-drc-and-info-lists.md): DRC and ratsnest re-run on the
  session's background jobs after an edit.
- G06.6 needs nothing beyond G03: its settings are board rules with setters
  at HEAD.

## What exists and what this adds

| Exists | Where |
|---|---|
| Net classes | `NetClasses::{append, append_unnamed, remove}`, `NetClass::{name, set_trace_half_width, set_via_rule, set_shove_fixed, set_ignore_cycles_with_areas, set_minimum_trace_length, set_maximum_trace_length, set_active_routing_layer}`, `Net::{net_class, set_class}` in `crates/freerouting/src/rules/mod.rs`; `BoardRules::append_net_class` |
| Clearance matrix | `ClearanceMatrix::{set_value, set_value_on_layer, set_inner_value, append_class, remove_class, is_equal, class_count, get_name}` in `rules/clearance_matrix.rs`; `BoardRules::{change_clearance_class_index, remove_clearance_class_from_rules}`; `SearchTreeManager::{clearance_value_changed, clearance_class_removed}` in `board/search/mod.rs` |
| Vias and rules | `ViaInfos::remove`, `ViaRule::{append_via, remove_via, vias, swap}`, `ViaRules::remove` in `rules/mod.rs`; `Library::{add_via_padstack, remove_via_padstack}` in `library/mod.rs` |
| Routing settings that are board rules | `BoardRules::{set_trace_angle_restriction, set_ignore_conduction, set_pin_edge_to_turn_dist}` (`rules/mod.rs`), `BasicBoard::{set_clearance_compensation_used, generate_keepout_outside_outline}` (`board/basic_board.rs`); both board methods already update the search trees |
| Board-level wrapper that keeps trees and items right | **missing**: B08 |
| Java at `cda540f` | `gui/windows/routing/{WindowNetClasses,WindowAssignNetClass,WindowClearanceMatrix,WindowEditVias,WindowVia,WindowViaRule,WindowRouteParameter}.java`, their `_en.properties`, and `src/test/java/app/freerouting/gui/WindowNetClassesTest.java` |

What this adds: `src/dialogs/{mod,net_classes,assign_net_class,clearance_matrix,vias,via_rules,route_settings}.rs`
and `tests/ui_rules.rs`. `route_settings.rs` holds only the static part;
G09.2 adds the interactive part to the same dialog.

## Decisions made here

- **Dialogs are thin.** A dialog reads values from the board, shows them,
  and sends one `Command::EditRules(RulesEdit)` per user change. `App::execute`
  applies it through B08's facade on `Arc::make_mut(&mut session.board)`,
  calls `generate_snapshot()` first (so U undoes a rule edit, as in Java's
  board history), bumps `edit_generation`, and restarts the DRC and ratsnest
  jobs. Rule edits don't change `BasicBoard::revision()`, so G01's
  staleness tag `(revision, edit_generation)` is what discards old DRC
  results.
- **Values are shown in the display unit** (G04.6) and converted with
  `io::CoordinateTransform`. A width entered by the user becomes a half
  width exactly as Java's `WindowNetClasses.applyTraceWidth` does:
  `round_to_i32(user_to_board(0.5 * value))` (jmath), refusing values ≤ 0 or
  a half width ≤ 0. Text input accepts a comma as the decimal separator and
  trims blanks (`parseTraceWidthValue`). This order decides the stored rule
  value; reproduce it exactly.
- **Board rules are saved with the board, not in GUI settings.** Snap angle,
  ignore conduction, pad-to-turn gap, clearance compensation and the
  outline keepout are `BoardRules`/board state. They persist through G08's
  DSN and `.rules` writing, not through eframe storage.
- **Java's dialog texts and refusals are kept**, with their messages as
  status-line text (`WindowNetClasses_en.properties` and the others named
  below).

## File ownership

Paths are under `crates/freerouting-gui/`.

| Item | New files | May edit |
|---|---|---|
| G06.1 | `src/dialogs/mod.rs`, `src/dialogs/net_classes.rs`, `tests/ui_rules.rs` | `src/commands.rs` (append `EditRules`), `src/lib.rs`, `src/text.rs` |
| G06.2 | `src/dialogs/assign_net_class.rs` | — |
| G06.3 | `src/dialogs/clearance_matrix.rs` | — |
| G06.4 | `src/dialogs/vias.rs` | — |
| G06.5 | `src/dialogs/via_rules.rs` | — |
| G06.6 | `src/dialogs/route_settings.rs` | — |
| G06.7 | — | `docs/development/gui.md` |

## Checklist

- [x] **G06.1 — Net classes table.**
  - Java: `WindowNetClasses`: columns `NAME`, `VIA_RULE`,
    `CLEARANCE_CLASS`, `TRACE_WIDTH`, `ON_LAYER` (summarised as
    `layers_all`, `layers_inner`, `layers_outer`, `layers_none`,
    `layers_custom`; `width_multiple` opens the `dialog_layer_rules_title`
    dialog), `SHOVE_FIXED`, `CYCLES_WITH_AREAS`, `MIN_TRACE_LENGTH`,
    `MAX_TRACE_LENGTH`, `IGNORED_BY_AUTOROUTER`. Buttons `add`, `remove`,
    `assign`, `select`, `show_nets`, `filter_incompletes`. Removal
    (`RemoveNetClassListener`): the last remaining class gives
    `default_net_class_not_removed`; a class used by a net gives
    `net_class_not_removed_in_use_message` naming the first such net in
    ascending net number; success gives `net_class_removed_message`.
  - Java's in-use loop runs `i = 1; i < nets.maxNetNumber()`. Whether that
    skips the last net is unverified; if it does, the port checks every net
    and records the difference in DIVERGENCES.md (no routing effect).
  - Rust: `dialogs::net_classes::NetClassesDialog` with an egui table; each
    cell edit sends one `RulesEdit` variant to B08. "Select" puts the items
    of the chosen classes' nets in the G03 selection; "Show Nets" opens
    G04.1's nets window filtered to those classes.
  - Port `WindowNetClassesTest`'s three tests as unit tests of the dialog's
    parsing helpers (`parse_trace_width_value`, `apply_trace_width` with a
    fake transform): `200`, `150.5`, `"0,35"` → 0.35, `"  120.0  "`,
    `"invalid"` and `""` → none; 0, −50 and a missing transform refuse
    without touching the class.
  - **Waits on B08.**
  - **Accept:** the three ported tests pass; `tests/ui_rules.rs`:
    - `edit_width_changes_engine_value`: on `Issue754-avionics_hub.dsn`,
      typing `0.3` (mm) into the default class's width sets its half width
      to the value the test computes with the same formula, and the DRC
      count shown afterwards equals `DesignRulesChecker::new(&board).check()`
      run by the test on the session's board.
    - `remove_default_class_refused` and `remove_used_class_refused` show
      Java's messages; `add_then_remove_unused_class` restores the class
      count.
  *Done 2026-10-16:* `dialogs/net_classes.rs`. The three `WindowNetClassesTest` tests are ported in
  `dialogs/input.rs` (`parse_trace_width_value_handles_numeric_and_string_inputs`,
  `apply_trace_width_applies_the_half_width`, `apply_trace_width_refuses_invalid_input`), with
  `DialogInteractionHandlersTest`'s `canRemoveNetClass`, `isLegalClassName`, `parseClearanceTableValue`
  and number normalizers (same file) and the autorouter checkbox handlers
  (`dialogs/autoroute_handlers.rs`). `tests/ui_rules.rs` (`cargo test -p freerouting-gui --test
  ui_rules`: 29 passed): `edit_width_changes_engine_value` (Issue754, typed `0.3` mm, half width =
  `round_to_i32(dsn_to_board(0.5 * 0.3 mm in design units))` on every signal layer; the shown DRC count
  equals `DesignRulesChecker::check()`), `remove_default_class_refused` (one-class board
  `tests/data/rules_small.dsn`), `remove_used_class_refused`, `add_then_remove_unused_class`, plus
  `net_class_cells_send_their_edits`, `select_takes_the_items_of_all_chosen_classes`,
  `layer_rules_dialog_sets_the_active_layers`, `filter_incompletes_keeps_the_airlines_of_the_chosen_classes`.
  The in-use loop does skip the last net in Java (`maxNetNumber()` is the net count), so the port
  checks every net (DIVERGENCES "Rules dialogs"; pinned with net N3, the last). Show Nets opens a
  "Contained Nets" window as Java does, not G04.1's nets window.

- [x] **G06.2 — Assign net class.**
  - Java: `WindowAssignNetClass` (columns `net_name`, `class_name`), opened
    from the net classes dialog's `assign` button.
  - Rust: `dialogs::assign_net_class` lists nets in net-number order with a
    class combo box per row; a change sends `RulesEdit::SetNetClass { net,
    class }` to B08, which also reapplies the class to unfixed items.
  - **Waits on B08.**
  - **Accept:** `tests/ui_rules.rs::assign_net_class_changes_net`: assigning
    net `GND` of Issue754 to a newly added class changes
    `Net::net_class()` to that class id, and the ratsnest job re-runs (its
    result tag carries the new `edit_generation`).
  *Done 2026-10-16:* `dialogs/assign_net_class.rs`. `assign_net_class_changes_net` (small board, the
  Assign button opens the window, a combo choice changes `Net::net_class()`, the analysis ran again and
  its ratsnest tag is `session.edit_generation`) and `assign_net_class_on_the_avionics_hub` (Issue754 has
  no net named `GND`; `GTN1_GND` is used). Rows are in net-name order, ignoring case, as Java's
  `Arrays.sort` does, not net-number order.

- [x] **G06.3 — Clearance matrix.**
  - Java: `WindowClearanceMatrix`: a layer combo (all layers, inner layers,
    or one layer); a cell edit is applied symmetrically by
    `applyClearanceValue` (`set_value` both ways, `set_inner_value` both
    ways, or `set_value_on_layer` both ways); `add_class` prompts
    `new_name`, ignores an existing name, appends the class and turns
    clearance compensation off; `prune` runs `pruneClearanceMatrix`.
  - Order that decides the result: prune walks `i` from the last class down
    to 2 and, for each `i`, `j` from the last class down to 0 (skipping
    `i == j`); the first `j` with `is_equal(i, j)` asks
    `confirm_remove_class_message`, and on Yes moves every item of class
    `i` to `j`, removes class `i`, and calls `clearance_class_removed(i)`.
    Reproduce this order, so the same class survives as in Java.
  - Rust: `dialogs::clearance_matrix`. The prune loop's order lives in the
    dialog (it is GUI flow with confirmations), but each confirmed step is
    one B08 call (`RulesEdit::MergeClearanceClass { from: i, to: j }`).
  - **Waits on B08.**
  - **Accept:** `tests/ui_rules.rs`:
    - `clearance_edit_is_symmetric`: setting cell (1, 2) on "all layers"
      makes `get_value(1, 2, l, false) == get_value(2, 1, l, false)` on
      every layer.
    - `prune_merges_equal_classes`: on a board where the test first adds a
      class equal to `default`, Prune (auto-confirmed in the test) drops
      the class count by one, and the DRC finding count is unchanged.
    - `add_class_turns_off_compensation`.
  *Done 2026-10-16:* `dialogs/clearance_matrix.rs`. `clearance_edit_is_symmetric`,
  `prune_merges_equal_classes` (class count -1, the DRC findings unchanged), `prune_asked_no_keeps_the_class`,
  `add_class_turns_off_compensation` (also the illegal-name reprompt), `clearance_cell_asks_when_items_use_both_classes`,
  and `prune_candidates_equal_the_engines_prune`: the dialog's one-candidate walk plus
  `MergeClearanceClass` ends with the same removed classes, in the same order, as B08's
  `prune_clearance_matrix` answering Yes (Issue754 and Issue159 with two added twin classes).

- [x] **G06.4 — Vias list.**
  - Java: `WindowEditVias`: columns `NAME`, `PADSTACK`, `CLEARANCE_CLASS`,
    `ATTACH_SMD`. `AddViaListener` names the new via `new_via_<n>` with the
    smallest `n ≥ 1` not in use, padstack = via padstack 0, clearance class
    = the default net class's via clearance class, attach SMD off.
    `RemoveViaListener`: the last via gives `last_via_not_removed`; a via
    used by a rule gives `via_not_removed_in_rule_message` with the first
    rule in rule order; success gives `via_removed_message`.
  - Rust: `dialogs::vias`; the name search and the refusals are dialog
    logic, the add and remove are B08 calls.
  - **Waits on B08.**
  - **Accept:** `tests/ui_rules.rs`:
    - `add_via_names_follow_java`: two adds on a board with no `new_via_*`
      give `new_via_1` and `new_via_2`.
    - `remove_via_used_by_rule_refused`: the status line names the rule.
    - `remove_last_via_refused`.
  *Done 2026-10-16:* `dialogs/vias.rs`. `add_via_names_follow_java` (`new_via_1`, `new_via_2`, smallest free
  number), `remove_via_used_by_rule_refused` (names the rule), `remove_last_via_refused`,
  `via_cells_send_their_edits`. B08's `add_via_info` names the via `newVia<n>`, but Java's
  `getText("newVia")` snake-cases to the bundle key `new_via` = `new_via_`, so `rules_edit::add_via_info`
  renames the new via (see the report: the B08 name is wrong).

- [x] **G06.5 — Via padstacks and via rules.**
  - Java: `WindowVia` (`create` padstack: prompts
    `prompt_new_padstack_name`, refuses an existing name with
    `padstack_name_exists`, asks `select_start_layer` and
    `select_end_layer`, then `radius_on_layer_label` per layer with
    `prompt_default_radius`; `remove` padstack refuses with
    `padstack_not_removed_in_use_message`; via rules `create`
    (`prompt_new_via_rule_name`), `edit`, `remove`
    (`remove_via_rule_confirm`)), and `WindowViaRule` (`append`
    with `choose_via_to_append`, `remove` with `remove_via_from_rule_confirm`,
    `move_up`, `move_down`).
  - Rust: `dialogs::via_rules` with the padstack list, the via rule list and
    a rule editor. Move up and down call `ViaRule::swap` through B08; the
    rule's via order is the routing preference order, so it must be exactly
    what the user set.
  - **Waits on B08.**
  - **Accept:** `tests/ui_rules.rs`:
    - `padstack_create_use_remove`: create a padstack, create a via that
      uses it (G06.4), then try to remove the padstack: refused with the
      message naming the via.
    - `via_rule_order`: append three vias, move the last one up twice;
      `ViaRule::vias()` returns them in the shown order.
  *Done 2026-10-16:* `dialogs/via_rules.rs`. `padstack_create_use_remove` (create through the form,
  a via uses it, Remove is refused naming `new_via_1`, then works), `existing_padstack_name_is_refused_by_the_form`,
  `via_rule_order` (three vias appended, the last moved up twice, then down, `ViaRule::vias()` equals the
  shown order; Remove asks first), `via_rule_create_and_remove`.

- [~] **G06.6 — Routing settings: the board-rule part.**
  - Java: `WindowRouteParameter` (`WindowRouteParameter_en.properties`):
    `snap_angle` (`90_degree`, `45_degree`, `none`; changing it asks
    `change_snap_angle_45` or `change_snap_angle_90` first),
    `ignore_conduction_areas`, `pin_pad_to_turn_gap`,
    `clearance_compensation_checkbox`, `keepout_outside_outline`.
  - Rust: `dialogs::route_settings::static_section` calls
    `set_trace_angle_restriction`, `set_ignore_conduction`,
    `set_pin_edge_to_turn_dist`, `set_clearance_compensation_used` and
    `generate_keepout_outside_outline` through `Command::EditRules`. The
    interactive fields (route mode, push-and-shove, pull-tight radius and
    accuracy, neckdown, and the rest) are G09.2.
  - **Accept:** `tests/ui_rules.rs`:
    - `route_settings_change_engine_values`: each control changes the board
      value the test reads back, and U undoes it.
    - `snap_angle_change_asks_first`: choosing 90° shows Java's
      confirmation text; Cancel leaves the value unchanged.
    - `keepout_outside_outline_toggles_item`: turning it on and off changes
      the board outline's `keepout_outside_outline_generated()` and the DRC
      result equals a fresh `check()` on the same board.
  *Partial 2026-10-16:* `dialogs/route_settings.rs` (`static_section`). `route_settings_change_engine_values`
  (snap angle, ignore conduction, restrict pin exits and the typed pad-to-turn gap, clearance compensation),
  `snap_angle_change_asks_first` (Java's text, Cancel leaves 45 degrees, Yes sets 90),
  `keepout_outside_outline_toggles_item` (the outline's `keepout_outside_outline_generated()` toggles and the shown DRC
  equals a fresh `check()`) pass. **Remaining: "and U undoes it".** The engine's board history
  (`generate_snapshot`/`undo`) holds items and components only, so no rule is restored, and the edits that do
  change items (outline keepout, conduction obstacle flags, fixed state of pin traces) change them in place
  without a saved version, so an undo restores nothing (as in Java). Needs engine work: rules (and these
  item changes) in the snapshot; then `execute_rules_edit` takes a snapshot first. Owner: B08 follow-up
  (operator decision, see the report).

- [x] **G06.7 — Document the dialogs.**
  - `docs/development/gui.md` gains a "Rules dialogs" section: each dialog,
    the Java class it follows, and the B08 call each control makes.
  - **Accept:** the section lists every dialog in this checklist and every
    `RulesEdit` variant the GUI sends.
  *Done 2026-10-16:* `docs/development/gui.md` "Rules dialogs (G06)": every dialog, its Java class and the B08 call
  of each `RulesEdit` variant (all variants listed).
