# K14 — The board snapshot applies the DSN fix-ups

Opened 2026-10-21 at the owner's request, after the owner's K13.7 run. `canary-pwr` opened on the
DSN export, not the new default. Its saved options turn on three K09 fix-ups (class widths, fixed
pour classes, dropped pours), and the board snapshot refused them at Route.

The four K09 fix-ups that rewrite the DSN (`kicad-plugin/freerouting_rs/dsn_fixups.py`) edit the
text of KiCad's export:
- `class_widths` writes a class's `(rule (width W))`;
- `fixed_pour_classes` in `wires` mode replaces each outer-layer pour of a listed class with fixed
  wires covering its fill (`fill_rects`, `_wire`);
- `drop_pour_nets` removes the outer-layer pours of the listed nets;
- `route_on_plane_layers` turns `(type power)` into `(type signal)`.

The snapshot (`board_input = kicad_pcb`, the default since K13.7) gives the router the `.kicad_pcb`,
so there is no DSN text to edit. Three places handle that today:
- `options.input_conflicts` refuses the four on the snapshot (K10.4);
- a saved file that has them and names no input loads on the DSN (K13.7);
- K13.1's table lists them as missing on the reader.

This checklist makes the `.kicad_pcb` path apply all four. Then `input_conflicts` is empty, and a
board's saved setup runs on the default input.

Prerequisites: K09 (the fix-ups), K10 (the snapshot), K12 (zones are refilled before both inputs),
K13 (the snapshot is the default).

## What the work starts from

- **The reader builds a DSN syntax tree.** `KicadRead::pcb` (`crates/freerouting-kicad/src/read.rs`)
  is the same structure KiCad's export parses to (`to_dsn()`, `copper_graphics.rs::the_written_dsn_keeps_the_areas`).
  So each rewrite has a direct target:
  - class rules: `classes.rs`;
  - planes: `zones.rs`;
  - layer types: `layers.rs`;
  - wires: `tracks.rs`.
- **Zone fills are already in the snapshot.** The fix-ups take each zone's fill from pcbnew
  (`kicad_board.py`, `GetFilledPolysList`). The snapshot's `.kicad_pcb` carries the same fills as
  `(filled_polygon ...)`, because the plugin refills zones before either input (K12).
- **Engine mode already works on both inputs.** `fixed_pour_mode = engine` is the router flag
  `--router.plane_obstacle_net_classes`. Only the `wires` mode and the `auto` lists need the DSN
  text.
- **The `auto` lists need only board data.** They come from the board and the classes of its nets
  (`dsn_fixups.resolve`: `auto_fixed_pour_classes`, `auto_drop_pour_nets`, `auto_class_widths`).
  The plugin has all of that in pcbnew without the DSN.

## Design (proposed 2026-10-21; an item may change it, giving its reason)

- **Router settings.** Each fix-up becomes a router setting that the `.kicad_pcb` path applies to
  the reader's tree. It lives in `crates/freerouting-kicad` behind `ReadOptions`, or in the
  engine's `job::prepare::set_kicad`. The plugin passes it as a flag, the way it passes the Board
  Setup minimums. The CLI can use the setting on any `.kicad_pcb`. No board or net names go in
  code; board values go in fixtures.
- **`auto` stays in the plugin.** The plugin resolves each `auto` list from the pcbnew board and
  passes explicit lists, so the router's settings take only names.
- **Same input on both paths.** The same board with the same option gives the router the same
  input on both paths. Tests compare the reader's tree with the plugin's fixed-up DSN, parsed, not
  routing results. Where the two cannot match exactly (the fill read from the file against pcbnew's
  polygons, the reader's 0.1 µm rounding), the item names the difference and its bound.
- **Profiles and defaults.** The settings work under both profiles. The defaults don't change: all
  four stay off, as K09.9 left them.
- **Fixture.** A new `.kicad_pcb` fixture covers all four: two classes with widths, an outer pour
  of a listed class, an outer pour of a listed net, an inner plane layer, and filled zones. Its
  KiCad DSN export is made once with pcbnew and recorded beside it. `canary-pwr`'s own values stay
  in `fixtures/canary-pwr/`.

## File ownership

| Item | New files | May edit |
|---|---|---|
| K14.1–K14.4 | reader tests and the fixture under `crates/freerouting-kicad/tests/` | `crates/freerouting-kicad/src/**`, `crates/freerouting/src/job/prepare.rs` and its settings, `kicad-plugin/freerouting_rs/{export_flow,dsn_fixups,options}.py` |
| K14.5 | — | `kicad-plugin/freerouting_rs/{options,presets,plugin}.py`, plugin tests, `docs/development/kicad-plugin.md`, `kicad-plugin/README.md`, `docs/DIVERGENCES.md` |
| K14.6 | `docs/roadmap/prereg/K14.json`, `tools/bench/sets/k14-fixups.txt` | `tools/kicad-reader/plugin-input-ab.sh` |

## Checklist

- [x] **K14.1 — Class widths on the snapshot.**
  - **Change:**
    - A router setting sets the listed classes' track width in the reader's class rules, as
      `dsn_fixups` writes `(rule (width W))`.
    - An unknown class is an error that names it, as the DSN fix-up raises one.
    - The plugin passes the setting for `class_widths`, explicit or `auto` resolved from the
      board, on the snapshot.
  - **Accept:**
    - a reader test: on the fixture, the tree's class rules with the setting equal the fixed-up
      export's, parsed;
    - `auto` resolved from the board on the snapshot gives the DSN path's list on the fixture;
    - plugin tests for the arguments; `input_conflicts` no longer names `class_widths`;
    - a DIVERGENCES line (freerouting-kicad, or the engine if the setting lives there).

    *Done 2026-10-21:* setting `router.input_fixups.class_widths` (`<class>=<um>` entries).
    - *Where and why (K14.1-K14.3):* the tree transform is `crates/freerouting-kicad/src/fixups.rs`
      (`InputFixups`, `apply`); the engine applies it in `job::prepare::apply_input_fixups`, in
      `prepare_job` after the first settings merge and before the board is built, from the new
      settings section `router.input_fixups.*` (`settings/fields.rs`, JSON `inputFixups`). Not
      `ReadOptions`: the CLI reads the `.kicad_pcb` before the settings are merged, so a reader
      option could only see the command line, not a settings file or the environment. A DSN input
      ignores the settings with one warning (`kicad_pcb` trees only, `JobInput::kicad_source`).
      Unset under both profiles. CLI help: `crates/freerouting-cli/src/help.txt`; GUI: three
      `CLI_ONLY` reasons (`autoroute_settings.rs`). `legacy` reaches only an empty-list check;
      `PARITY_SET=tools/bench/sets/connection-parity.txt tools/parity-check.sh --no-probes`: 7 of 7
      exact, PARITY OK. `cargo test --workspace --no-fail-fast`: 2810 passed, 1 failed (help text,
      fixed; `cargo test -p freerouting-cli` 178 passed after). After the review: `cargo test
      --workspace --no-fail-fast` 2814 passed, 0 failed; parity again 7 of 7 exact.
    - *Fixture:* `crates/freerouting-kicad/tests/data/k14-fixups.{kicad_pcb,kicad_pro}`, made with
      KiCad 10.0.7's pcbnew by `tools/kicad-reader/k14_fixture.py` (rerun:
      `python3 -I tools/kicad-reader/k14_fixture.py`), with its export `k14-fixups.kicad10.dsn`,
      the plugin's board facts `k14-fixups.board.json` and the export after `dsn_fixups.apply`
      with the three fix-ups on, `k14-fixups.fixed.dsn`. Four layers, In1.Cu and In2.Cu power;
      classes Power (1.0 mm) and Switch (1.5 mm); filled zones GND on F.Cu, B.Cu and In1.Cu, a
      VBUS pour over GND on F.Cu (priority 1, for K14.4), VBUS and V3V3 halves on In2.Cu.
    - *Reader test:* `cargo test -p freerouting-kicad --test k14_fixups`, 13 passed (16 after the review below):
      `class_widths_equal_the_fixed_up_export` (the class rules, `Vec<Rule>` per class, equal the
      parsed `fixed.dsn` exactly: Power 1000 -> 600 um, Switch 1500 -> 800 um),
      `the_unfixed_tree_equals_the_export_in_classes_layers_and_planes`,
      `an_unknown_class_is_an_error_that_names_it`, `the_setting_lists_parse_and_bad_entries_are_errors`,
      `engine::a_kicad_pcb_job_applies_the_settings_to_the_tree`,
      `engine::a_fixup_the_board_cannot_take_fails_the_job_naming_it`,
      `engine::without_the_settings_a_kicad_pcb_job_is_unchanged` (unset under default, `legacy`
      and `enhanced`), `engine::a_dsn_job_ignores_the_settings_with_a_warning`. RED: with
      `apply` returning before its changes, the four equality tests failed (5 passed, 4 failed);
      GREEN 9 of 9, then 13 with the engine tests (written after the code).
    - *`auto`:* `kicad-plugin/tests/test_snapshot_fixups.py::test_auto_on_the_snapshot_gives_the_dsn_paths_lists`
      (`dsn_fixups.resolve_on_board` on the board facts gives `{Power: 0.6, Switch: 0.8}`, the
      DSN path's `resolve` list; the classes from `export_flow.board_classes` equal the export's
      names, default width and widths), and
      `test_board_classes_on_the_fixture_through_pcbnew_gives_the_recorded_facts` (pcbnew, ran).
    - *Plugin:* `export_flow.snapshot_fixups` passes `--router.input_fixups.class_widths=Power=600,Switch=800`
      (`test_the_flags_carry_the_settings_as_the_dsn_fixup_writes_them`,
      `test_the_snapshot_passes_the_flags_explicit_or_auto`, `test_prepare_snapshot_adds_the_flags`,
      `test_route_dialog.py::test_a_snapshot_route_passes_the_reader_fixups_to_the_router`, and
      against the router: `test_the_router_applies_the_flags_to_the_snapshot`,
      `test_the_router_refuses_a_class_or_layer_the_board_does_not_have`). `input_conflicts` names only
      `fixed_pour_classes` (`test_snapshot.py::test_the_reader_fixups_no_longer_refuse_the_snapshot`);
      K13.7's two DSN rules keep the old list as `options.dsn_text_fixups` (`from_dict`,
      `presets.apply`), unchanged for K14.5. `pytest kicad-plugin/tests`: 608 passed, 6 skipped
      (610 passed, 6 skipped after the review).
    - *The board's `.kicad_dru` wins on both inputs* (orchestrator review 2026-10-21; routing
      never breaks the board's rules, decided 2026-10-20): the DSN path applies `--kicad-dru`
      after the text fix-up, so an applied `track_width` rule sets the class's width last. The
      reader applies the rules first, so `dru::track_width` records each class it set with the
      rule (`DruReport::track_widths`, through `JobInput::rules_track_widths`), and
      `fixups::apply_with_rules` skips it with one line, `class Power: width 900 um kept, set by
      the .kicad_dru rule 'power tracks' (class_widths skipped)`. Tests:
      `k14_fixups.rs::a_class_the_kicad_dru_track_width_sets_keeps_the_rules_width` (the tree's
      class rules equal `fixed.dsn` parsed and then `dru::apply_to_pcb`; Power 900, Switch still
      800) and `engine::a_kicad_pcb_job_keeps_the_kicad_dru_track_width`. RED with the API stubbed
      (12 passed, 4 failed with the K14.3 tests below), GREEN 16 passed.
    - *Difference between the paths:* none on the fixture (exact), with or without the rule. A name
      with a comma (an aggregate class `Power,Default`) cannot go through the comma list: the
      plugin leaves it out with a WARNING line
      (`test_a_name_a_comma_list_cannot_carry_is_left_out_with_a_warning`).
    - *DIVERGENCES:* "Class widths on a `.kicad_pcb` input" (freerouting-kicad).

- [x] **K14.2 — Plane layers opened to routing.**
  - **Change:** a router setting turns the listed inner plane layers into signal layers in the
    reader's tree, as the export's `(type power)` becomes `(type signal)`. Their zones stay planes.
    An unknown layer, or one that is not a plane, is an error that names it.
  - **Accept:**
    - a reader test: on the fixture, layer types and planes equal the fixed-up export's;
    - plugin tests; `input_conflicts` no longer names `route_on_plane_layers`;
    - a DIVERGENCES line.

    *Done 2026-10-21:* setting `router.input_fixups.route_on_plane_layers` (layer names), in
    `fixups.rs` and `apply_input_fixups` as K14.1 says.
    - *Reader test:* `k14_fixups.rs::opened_plane_layers_equal_the_fixed_up_export_and_keep_their_planes`
      (layer names and types equal the parsed `fixed.dsn` exactly, In2.Cu signal, In1.Cu still
      power; the planes, In2.Cu's VBUS and V3V3 halves included, equal the export's:
      `copper::planes` empty), `all_three_together_equal_the_fixed_up_export`,
      `an_unknown_layer_or_a_signal_layer_is_an_error_that_names_it` (In7.Cu unknown, F.Cu "not a
      plane"), and in the engine `engine::a_kicad_pcb_job_applies_the_settings_to_the_tree` (the
      built board's In2.Cu `is_signal`). RED/GREEN as K14.1 (16 passed).
    - *Plugin:* `--router.input_fixups.route_on_plane_layers=In2.Cu`
      (`test_snapshot_fixups.py::test_the_flags_carry_the_settings_as_the_dsn_fixup_writes_them`,
      `test_the_router_applies_the_flags_to_the_snapshot`: the router logs
      `input fix-up: layer In2.Cu: power -> signal`); `input_conflicts` no longer names it
      (`test_snapshot.py::test_the_reader_fixups_no_longer_refuse_the_snapshot[route_on_plane_layers]`).
    - *Difference between the paths:* none (layer names and types are exact).
    - *DIVERGENCES:* "Plane layers opened to routing on a `.kicad_pcb` input".

- [x] **K14.3 — Pours of listed nets dropped.**
  - **Change:**
    - A router setting leaves the outer-layer zones of the listed nets out of the reader's
      planes, as `dsn_fixups` drops their `(plane ...)`.
    - Inner-layer zones of those nets stay.
    - The plugin resolves `auto` (`auto_drop_pour_nets`) from the board.
  - **Accept:**
    - a reader test: on the fixture, the planes equal the fixed-up export's;
    - `auto` gives the DSN path's list;
    - plugin tests; `input_conflicts` no longer names `drop_pour_nets`;
    - a DIVERGENCES line.

    *Done 2026-10-21:* setting `router.input_fixups.drop_pour_nets` (net names), in `fixups.rs`
    and `apply_input_fixups` as K14.1 says. Outer layers are the first and last layer of the
    tree's structure (the plugin's set is the outer zones' layers plus F.Cu and B.Cu by name; a
    plane lies on a layer with a zone, so the two agree, renamed layers included).
    - *Reader test:* `k14_fixups.rs::dropped_pours_equal_the_fixed_up_export_and_inner_planes_stay`
      (the planes, net and layer in order, equal the parsed `fixed.dsn`; `copper::planes` empty;
      GND keeps In1.Cu and loses F.Cu and B.Cu, "2 outer plane(s) removed"),
      `an_unknown_net_is_a_warning_that_names_it` (orchestrator review 2026-10-21: the DSN fix-up
      passes an unknown net silently and a run never fails because of the input choice, so the
      setting warns `router.input_fixups.drop_pour_nets: the board has no net NOPE; left out` and
      goes on), `engine::an_unknown_net_warns_and_the_job_goes_on`, and against the router
      `test_snapshot_fixups.py::test_the_router_warns_about_an_unknown_net_and_routes`. RED/GREEN
      as K14.1 (16 passed). The router's `input fix-up:` lines reach the run log
      (`snapshot.READER_PREFIXES`; `test_the_reader_lines_keep_the_fixup_lines` and
      `test_the_router_applies_the_flags_to_the_snapshot`, RED 3 failed, GREEN 12 passed).
    - *`auto`:* `test_snapshot_fixups.py::test_auto_on_the_snapshot_gives_the_dsn_paths_lists`
      (`auto_drop_pour_nets` on the board facts gives `["GND"]` on both paths).
    - *Plugin:* `--router.input_fixups.drop_pour_nets=GND` (`test_the_flags_carry_the_settings_as_the_dsn_fixup_writes_them`,
      `test_the_snapshot_passes_the_flags_explicit_or_auto`,
      `test_route_dialog.py::test_a_snapshot_route_passes_the_reader_fixups_to_the_router`,
      `test_the_router_applies_the_flags_to_the_snapshot`); `input_conflicts` no longer names it.
    - *Difference between the paths:* none on the fixture. The reader keeps footprint-owned
      zones as planes (K06.4), which the export drops; an inner one of a listed net stays on the
      reader only, as without the fix-up.
    - *DIVERGENCES:* "Pours of listed nets dropped on a `.kicad_pcb` input" (it also states what
      a DSN input does with the three settings).

- [x] **K14.4 — Pours of listed classes as fixed wires.**
  - **Change:**
    - For each filled outer-layer zone whose net is in a listed class, the reader's plane is
      dropped.
    - Its fill, read from the snapshot's `filled_polygon`, is covered by fixed wires of that net.
    - The wires use `fill_rects`'s and `_wire`'s arithmetic, ported operation for operation, so
      the same fill gives the same rectangles.
    - The plugin resolves `auto` (`auto_fixed_pour_classes`) from the board.
  - **Accept:**
    - a port test: `fill_rects` on recorded fills gives the Python's rectangles exactly;
    - a reader test: on the fixture, the fixed wires equal the fixed-up export's (count, net,
      layer, width, and coordinates within the reader's 0.1 µm), and the dropped planes match;
    - a route test: the session written from a snapshot route holds none of the fixed wires (the
      DSN path's property: `ImportSpecctraSES` never sees them);
    - plugin tests; `input_conflicts` no longer names `fixed_pour_classes` in `wires` mode or as
      `auto`;
    - a DIVERGENCES line;
    - `legacy` keeps connection-parity 7 of 7 if engine code that `legacy` runs changes.

    *Done 2026-10-21:* setting `router.input_fixups.fixed_pour_classes` (class names), beside
    K14.1-K14.3's (`settings/fields.rs`, JSON `fixedPourClasses`; `tests/settings_defaults.rs`
    unset under both profiles; CLI help; GUI `CLI_ONLY`; the DSN-input warning names it).
    - *Where:* `fixups.rs` (`InputFixups::fixed_pour_classes`, `pour_fills`, `apply_with_fills`)
      runs `dsn_fixups.apply`'s pour loop on the tree: the board's own zones in file order (rule
      areas and footprint zones out, as `board.Zones()`), F.Cu then B.Cu (`GetLayerSet().Seq()`),
      filled as `parseZONE` sets `IsFilled` (`(fill yes ...)`, `zones::is_filled`), the class from
      the network's class lists (last wins, as `Dsn.net_class`), `drop_pour_nets` first (dropping
      wins); each `fill_rects` rectangle gives a `_wire` path (`fill_wires.rs`, ported in
      `4c1fd8e`), `(type fix)`, net and layer as named, put first in the wiring; the planes of each
      covered `(net, layer)` go. The fill is the file's `(filled_polygon ...)` (`zones::read_fills`,
      `parseOutlinePoints`' points, `nm / 1e6` as `pcbnew.ToMM`), read again in
      `job::prepare::apply_input_fixups` through `io::kicad::read_pour_fills` (hash-checked as the
      K08 writer is) only when the setting is set. An unknown class is an error naming it.
    - *Recorded expectation:* `tools/kicad-reader/k14_fixed_pours.py` (no pcbnew; rerun:
      `python3 -I tools/kicad-reader/k14_fixed_pours.py`) applies `dsn_fixups.apply`, loaded by
      path, to the committed `k14-fixups.kicad10.dsn` and `k14-fixups.board.json`:
      `k14-fixups.fixed-pours.dsn` (all four, 44 wires, 3 outer planes removed),
      `k14-fixups.fixed-pours-only.dsn` (44 wires, 1 plane) and `k14-fixups.fixed-pours-default.dsn`
      (`kicad_default` and Power: 317 wires for 3 pours). `auto_fixed_pour_classes` on the board
      facts gives `["Power"]` (VBUS, priority 1, over GND's priority-0 pour on F.Cu; GND is in
      `kicad_default` on this fixture), and the script checks the board's classes give the same.
      `k14-fixups.fixed.dsn` is unchanged.
    - *Port test:* `crates/freerouting-kicad/tests/fill_wires.rs` (landed in `4c1fd8e`), 6 passed.
    - *Reader test:* `cargo test -p freerouting-kicad --test k14_fixups`: 25 passed.
      `fixed_pours_equal_the_fixed_up_export` (the wiring equals the parsed `fixed-pours.dsn`
      exactly: 44 wires, net, layer, width and both points, in order; no difference, so no bound
      needed, the integers come from `um`; planes, classes and layer types equal too),
      `fixed_pours_alone_equal_the_fixed_up_export`, `several_fixed_pours_come_in_the_plugins_zone_order`
      (317 wires), `a_dropped_net_is_not_covered_by_fixed_wires`, `an_unfilled_pour_keeps_its_plane`,
      `the_fills_read_from_the_file_equal_the_plugins_board_facts` (every outer fill point equals
      the board facts' `f64`), `an_unknown_fixed_pour_class_is_an_error_that_names_it`,
      `engine::an_unknown_fixed_pour_class_fails_the_job_naming_it`,
      `engine::a_dsn_job_ignores_the_settings_with_a_warning` (now with the fourth setting).
      RED: with the pour loop returning nothing, 19 passed, 4 failed (the four export
      comparisons); GREEN 23, then 25 with the engine tests.
    - *Route test, every output:* the session writer already leaves `SYSTEM_FIXED` items out
      (`ses_writer::session_nets`), which is how the DSN path keeps the wires out of
      `ImportSpecctraSES`; the reader's `(type fix)` wires are `SYSTEM_FIXED` too. The `.kicad_pcb`
      writer did not: a release run of the fixture with the setting wrote 90 segments, 44 of them the
      fill wires. `io::kicad::final_copper` now skips `SYSTEM_FIXED` traces (a locked original, which
      the K08 diff keeps anyway, or a fill wire); the same run then writes 46 segments, none of the
      fill widths, and its session's 37 paths include none of the 44 (checked by value). Tests:
      `k14_fixups.rs::engine::the_fixed_pour_wires_are_on_the_board_and_in_no_output` (the board has
      VBUS `SYSTEM_FIXED` traces on F.Cu; the session has no wire and equals the one without the
      setting; the rendered `.kicad_pcb` equals the input; RED with the filter off: "a fill wire
      became a track") and `crates/freerouting-cli/tests/cli_k14_fixed_pours.rs` (2 passed:
      `-do fixed.ses+fixed.kicad_pcb+fixed.dsn`, the `.dsn` holds 44 `(type fix)` wires, the session
      none, the `.kicad_pcb` equals the input; the unknown class fails the run). The reader's track
      handling (`tracks.rs`) reads only `segment`/`arc`, and the plugin's `fix_existing` locks
      pcbnew's tracks before the snapshot; the fill wires are made from zones after both, so neither
      can count them, and `restore_copper` sees only pcbnew's tracks and the session's.
    - *Plugin:* `dsn_fixups.auto_fixed_pour_classes_of_board` (the board's `classes`),
      `resolve_on_board` resolves `fixed_pour_classes`; `snapshot_fixups` passes
      `--router.input_fixups.fixed_pour_classes=Power` in `wires` mode, named or `auto`;
      `snapshot_engine_args` resolves `auto` from the board in `engine` mode and passes
      `--router.plane_obstacle_net_classes=Power`; `input_conflicts` returns `[]` (kept for K14.5,
      as are K13.7's two DSN rules). Tests (`test_snapshot_fixups.py`):
      `test_the_recorded_fixed_pour_exports_are_the_dsn_fixups_of_the_recorded_export`,
      `test_auto_on_the_snapshot_gives_the_dsn_paths_lists` (board and DSN agree, `["Power"]`),
      `test_the_flags_carry_the_settings_as_the_dsn_fixup_writes_them`,
      `test_the_snapshot_passes_the_fixed_pour_classes_explicit_or_auto`,
      `test_engine_pours_on_the_snapshot_resolve_auto_from_the_board`,
      `test_the_router_lays_the_fixed_pour_wires_and_writes_none` (against the router); the refusal
      tests became acceptance tests (`test_snapshot.py::test_the_fixed_pour_wires_no_longer_refuse_the_snapshot`,
      `test_no_dsn_rewrite_refuses_the_default_snapshot`,
      `test_engine_and_wire_pours_named_or_auto_work_on_a_snapshot`,
      `test_headless_route_takes_the_fixed_pour_classes_on_a_snapshot`,
      `test_the_form_accepts_every_dsn_option_on_a_snapshot`,
      `test_route_dialog.py::test_a_snapshot_route_passes_the_fixed_pour_classes_to_the_router`,
      `test_board_input_default.py::test_the_note_names_no_fixup_that_keeps_the_snapshot_from_running`:
      `input_note` has no DSN line now, and
      `test_input_choice.py::test_a_board_kept_on_the_dsn_by_its_fixups_says_why_and_the_snapshot_says_what_blocks_it`).
      RED: these against main's plugin modules, 14 failed. GREEN: `pytest kicad-plugin/tests`
      with `FREEROUTING_RS_BIN` at this tree's release build, 614 passed, 6 skipped.
    - *Difference between the paths:* none on the fixture (exact). The plugin walks pcbnew's zones
      in memory, the reader the saved file's; they agree once the board was loaded from a file KiCad
      saved, and a zone drawn in the session before the snapshot could change only the wires' order
      (`fixups.rs` docs).
    - *DIVERGENCES:* "Pours of listed classes as fixed wires on a `.kicad_pcb` input"
      (freerouting-kicad); the K14.3 line now says all four settings are ignored on a DSN input.
    - *legacy:* the setting is unset under both profiles and `final_copper` runs only for a
      `.kicad_pcb` output; still, `PARITY_SET=tools/bench/sets/connection-parity.txt
      tools/parity-check.sh --no-probes`: 7 of 7 exact, PARITY OK. `cargo test --workspace
      --no-fail-fast`: 2838 passed, 0 failed.

- [x] **K14.5 — The plugin stops sending these boards to the DSN.** Once K14.1–K14.4 have landed:
  - **Change:**
    - `input_conflicts` returns nothing for any of the four; keep it, empty, if other refusals
      remain.
    - K13.7's two DSN rules go: the load rule (a file that names no input and has a fix-up loads on
      `dsn`) and `presets.apply`'s move to `dsn` with its warning. Such a file then takes the
      profile's default input, as K13's "only boards with no saved choice get the new default"
      says. A file that says `dsn` keeps it.
    - The dialog's note no longer says the fix-ups need the DSN export.
    - The docs and README say which fix-ups work on which input; the DIVERGENCES lines from K10.4
      and K13.7 change to match.
  - **Accept:**
    - plugin tests: a saved file and a preset with each fix-up route on the snapshot (stub UI);
      the dialog form accepts each on the snapshot; the note has no DSN line;
    - `python3 -m pytest kicad-plugin/tests` passes.

    *Done 2026-10-21:* plugin Python, tests and docs only; no engine code, so no parity run (the router
    binary of main `383de4a`, a debug build, was used for the tests that call it).
    - *Removed.* `options.input_conflicts` (nothing else refused: the form's refusal loop and
      `headless_run.py route`'s `Incompatible board input options` exit were its only callers),
      `options.dsn_text_fixups` and `DSN_REWRITE_LABELS` (kept only for K13.7's rules; the one label still
      used is now `KEEPOUTS_LABEL` for the ignored-keepouts line), `presets._dsn_for_fixups` (the preset
      move to `dsn` and its warning) and the load rule at the end of `RouteOptions.from_dict`. A file or
      preset without `board_input` takes the profile's input (the dialog's, for a preset); a file that says
      `dsn` keeps it. `options.input_note` stays, as `input_ignored_note` (the keepouts line, snapshot only);
      the text boxes and the pour mode no longer re-run it. Docstrings of `options.py` and `presets.py` say
      so. `plugin.py`: tooltips of class widths and pour mode and the board input's say "on either board
      input" (the board input's no longer says the snapshot cannot be combined with the fix-ups), and the
      group box is "Fix-ups" (was "DSN fix-ups").
    - *Tests, RED then GREEN.* RED: the new and rewritten tests against main's plugin modules, 41 failed,
      633 passed, 6 skipped. GREEN: `pytest kicad-plugin/tests` (`FREEROUTING_RS_BIN` = this build's
      `target/debug`): 674 passed, 6 skipped (pcbnew-only and the PCM validator, as before);
      `test_snapshot_fixups.py` and `test_snapshot_headless.py`: 21 passed, none skipped.
      - saved files: `test_board_input_default.py::test_a_saved_fixup_without_a_choice_takes_the_profiles_input`
        (7 fix-up shapes: `class_widths` named and `auto`, `fixed_pour_classes` in `wires`, `engine` and `auto`,
        `drop_pour_nets`, `route_on_plane_layers`; schemas 3 to 6; `legacy` stays `dsn`),
        `::test_a_saved_fixup_with_a_choice_keeps_the_choice` (`dsn` stays `dsn`),
        `::test_the_canary_preset_file_as_a_saved_options_file_takes_the_snapshot`,
        `::test_the_k13_7_rules_and_the_refusal_are_gone`;
      - routes there through the stub UI: `test_input_choice.py::test_a_board_saved_with_a_fixup_and_no_choice_opens_on_the_snapshot_and_routes_there`
        (7 shapes: opens on the snapshot, one snapshot, no export, no error box, `input` is `kicad_pcb`),
        `::test_a_board_saved_with_a_fixup_and_the_dsn_keeps_the_dsn` (7 shapes: one export, no snapshot),
        `test_snapshot.py::test_headless_route_follows_the_profile_when_the_options_name_no_input` (headless
        `route` takes the snapshot with a fix-up and no input, `export` with `dsn`);
      - presets: `test_board_input_default.py::test_a_preset_with_a_fixup_and_no_input_keeps_the_input_it_found_without_a_warning`
        (7 shapes, both inputs, no warning), `::test_the_canary_preset_on_the_new_default_stays_on_the_snapshot`;
      - the form: `::test_the_form_accepts_each_fixup_on_either_input` (7 shapes, both inputs, no error),
        `::test_the_form_accepts_all_the_fixups_together_on_the_snapshot`;
      - the note: `::test_the_note_has_no_dsn_line_for_any_fixup` (7 shapes, both inputs: equal to the ignored
        note, no "DSN"), `::test_the_note_with_the_canary_fixups_is_the_ignored_note_or_nothing`;
      - canary-pwr's saved shape (schema 4: class widths, fixed pour classes, dropped pours, no
        `board_input`; synthetic, `test_input_choice.py::CANARY_SAVED`; nothing read from `../canary`):
        `test_input_choice.py::test_canary_pwrs_saved_options_open_on_the_snapshot_and_route_there` (opens on the
        snapshot with only the keepouts note, Route goes through the snapshot, the saved file keeps the fix-ups
        and writes no `board_input`).
    - *K13.7 tests removed or rewritten.* Removed: `test_snapshot.py::test_the_fixed_pour_wires_no_longer_refuse_the_snapshot`,
      `::test_the_reader_fixups_no_longer_refuse_the_snapshot`, `::test_no_dsn_rewrite_refuses_the_default_snapshot`,
      `::test_engine_and_wire_pours_named_or_auto_work_on_a_snapshot` (all asserted `input_conflicts == []`; the
      form, route and note tests above replace them), the `input_conflicts` assertions in `test_snapshot.py`,
      `test_snapshot_fixups.py`. Rewritten: `test_board_input_default.py::test_a_saved_dsn_fixup_without_a_choice_keeps_the_dsn`
      (now takes the snapshot), `::test_a_preset_with_a_dsn_fixup_and_no_input_moves_to_the_dsn_with_a_warning`
      (now keeps the input, no warning), `::test_the_canary_preset_file_as_a_saved_options_file_keeps_the_dsn`,
      `::test_the_canary_preset_on_the_new_default_gives_its_dsn_export`, the two note tests;
      `test_input_choice.py::test_a_schema_5_board_with_class_widths_and_no_choice_still_routes_on_the_dsn` and
      `::test_a_board_kept_on_the_dsn_by_its_fixups_says_why_and_the_snapshot_says_what_blocks_it` (now the two
      "opens on the snapshot" tests); `test_presets.py` (the canary preset as an options file takes the
      profile's input); `test_route_dialog.py` (group title "Fix-ups"). None was loosened.
    - *Docs.* `docs/development/kicad-plugin.md` (a table of the fix-ups on each input; the saved-options rule),
      `kicad-plugin/README.md`, `README.md`, and the K13.7 line in `docs/DIVERGENCES.md` (the K10.4
      refusal had no DIVERGENCES line of its own). All four fix-ups work on both inputs; `edge_and_pad_keepouts`
      is done by the reader itself on the snapshot and ignored there.
    - *Not done:* nothing in KiCad ran (no Windows build, no Route inside KiCad); K14.6 grades canary-pwr.

- [~] **K14.6 — `canary-pwr` with its saved fix-ups, on both inputs.**
  - **Preregister first:** `docs/roadmap/prereg/K14.json` and `tools/bench/sets/k14-fixups.txt`.
    - Board: `canary-pwr` at the canary revision named in the registration.
    - Options: a copy of the board's saved `route-options.json` (the class widths, fixed pour
      classes and dropped pours it has today, with its pass and rip-up settings), with
      `board_input` set per arm.
    - Arms: `dsn` against `kicad_pcb`, one `tools/kicad-reader/plugin-input-ab.sh` run each.
    - Grading: under the board's own rules.
    - Machine: a heavy run, queued through the orchestrator (canary-pwr is exempt from the
      few-minutes rule, decided 2026-10-20).
  - **Accept:**
    - every routing-caused manufacturing type, below-minimum and via-in-pad is 0 in both arms
      (decided 2026-10-20: routing never breaks the board's manufacturing rules);
    - the `kicad_pcb` arm's KiCad unconnected is no higher than the `dsn` arm's, or within the
      noise the registration states;
    - both sessions' sha256s are recorded;
    - the owner's confirmation from a Route inside KiCad on `canary-pwr`, with its saved options,
      on the snapshot, is recorded.

  *Partial 2026-10-21:* the registration half is in, **no number yet**: nothing was routed or graded,
  and no kicad-cli or KiCad ran. Registered at main `4c1fd8e`, before K14.1–K14.3 and K14.5 landed and
  with K14.4 in part.
  - **Files.** `docs/roadmap/prereg/K14.json` (id `K14.6-saved-fixups`, sha256
    `81b0c6f3eaad13c082511075d2207de2c12635cff76b65b4e35f397eccf61501`), `tools/bench/sets/k14-fixups.txt`
    (1 row, sha256 `39434e90e576b35af82dba327f57f1b7dd6c5dcc80d79634cc9220100cc91d53`) and the pin test
    `crates/freerouting-dev/tests/prereg_k14.rs` (sha256
    `a76d080c234a8c451192412ee214e416696bbf5862d04e03df78ab99ef456855`). The script stays K13.5's
    `plugin-input-ab.sh` (`17af015a…`, pinned by K13.json too), so it is not edited.
  - **Board.** canary `95882f00` (HEAD at registration) `hardware/canary-pwr-rev1.0`, copied with `git
    archive` to `target/orchestrator/k146/boards/canary-95882f00/`. Its `.kicad_pcb` (sha256 `b290e4f7…`,
    version 20260206) last changed in `88e68eac`. The canary working tree's board, `.kicad_pro`,
    `.kicad_dru` and `route-options.json` equal HEAD's at registration.
  - **Options.** The copy's own `freerouting-rs/route-options.json` (schema 4, sha256 `1ffe579f…`), recorded
    verbatim in `options.saved_text`. `board_input` is set per arm by the script. Its class widths equal
    Board Setup's (GND 0.4, Power/Power_High/Switch_Node 0.3 mm).
  - **Rule.** Both arms must hold 0 for `manufacturing_breaches`, `below_minimum` and `via_in_pad`.
    `kicad_pcb`'s unconnected must be at most `dsn`'s + 10, where 10 is canary-pwr's band from K09.json
    (N02). Both session sha256s are recorded. The rule is applied by hand, because the gate needs 3
    boards. A breach blocks the item and gets fixed, not removed. The registration decides no default.
  - **Test and gate.** `cargo test -p freerouting-dev --test prereg_k14`: 7 passed, with the board,
    rules and options hashes checked against the copy. `cargo fmt --all --check` and `cargo clippy -p
    freerouting-dev --all-targets -- -D warnings` are clean.
  - **Remains.**
    1. K14.1–K14.5 land on main.
    2. The heavy run, from the main checkout, with `K14_COMMIT` = that main:
       - heavy run pending: `git worktree add --detach target/orchestrator/k146/tree <K14_COMMIT> && cd
         target/orchestrator/k146/tree && CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0
         CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
         CARGO_TARGET_DIR=/home/thebo/code/freerouting-rs/target/orchestrator/k146/target cargo build
         --release -p freerouting-cli`;
       - then K14.json amendment 1 (the commit, the binary, the hashes, K14's router flags, validity (a)),
         with the pin updated;
       - then, with `pgrep -a kicad-cli` empty, the dsn arm:
         heavy run pending: `mkdir -p target/orchestrator/k146/out &&
         FREEROUTING_BIN=/home/thebo/code/freerouting-rs/target/orchestrator/k146/target/release/freerouting-rs
         ROUTE_TIMEOUT_S=1800 ROUTE_MEMORY_KB=8000000
         SHARE_GRADE_WITH=target/orchestrator/k146/out/kicad_pcb/canary-pwr
         target/orchestrator/k146/tree/tools/kicad-reader/plugin-input-ab.sh dsn
         target/orchestrator/k146/boards/canary-95882f00/hardware/canary-pwr-rev1.0/canary-pwr.kicad_pcb
         target/orchestrator/k146/boards/canary-95882f00/hardware/canary-pwr-rev1.0/freerouting-rs/route-options.json
         target/orchestrator/k146/out/dsn/canary-pwr`;
       - then the same command with `dsn` and `kicad_pcb` swapped (the arm, `SHARE_GRADE_WITH=…/out/dsn/canary-pwr`
         and the outdir `…/out/kicad_pcb/canary-pwr`). K14.json's `heavy_run` spells out both commands with
         their logs.
    3. The owner's confirmation from a Route inside KiCad on canary-pwr, with its saved options, on
       the snapshot (recording that board's sha256).

  *Partial 2026-10-21 (amendment 1):* K14.1–K14.5 are on main; `K14_COMMIT` = `98f11e7`. TREE built
  (release binary sha256 `d37543af…`, `freerouting-rs 0.1.0 (git 98f11e7ae822)`; `prereg_k14` in TREE: 7
  passed). Amendment 1 is in K14.json (sha256 now `c554c5b0…`, pinned in `prereg_k14.rs`, which also
  expects one amendment): the commit, the binary, the grader and plugin hashes, WSL KiCad 10.0.7, K14's
  router flags and the exact `args` validity (b) expects on the `kicad_pcb` arm
  (`class_widths=GND=400,Power=300,Power_High=300,Switch_Node=300`, `drop_pour_nets=GND`,
  `fixed_pour_classes=Power_High,Switch_Node`). Validity (a) is recorded as its equivalent, because
  K14.5 removed `options.input_conflicts`: the function is gone, `saved` loads on `kicad_pcb` and the
  form adds no error. Still no number: the two arms run next, then the owner's Route.

  *Partial 2026-10-21 (the run):* K14.json run as registered and amended, from the main checkout with TREE
  `target/orchestrator/k146/tree` (`98f11e7`), binary `d37543af…`, WSL KiCad 10.0.7, `plugin-input-ab.sh`
  `17af015a…`, `ROUTE_TIMEOUT_S=1800`, `ROUTE_MEMORY_KB=8000000`, dsn first, then kicad_pcb with
  `SHARE_GRADE_WITH` (the sessions differ, so each arm was graded), `pgrep -x kicad-cli` empty before each.
  Outputs: `target/orchestrator/k146/out/<arm>/canary-pwr/summary.json`, `route.log`, `work/canary-pwr.args.json`.

  | | dsn | kicad_pcb |
  |---|---|---|
  | KiCad unconnected (input) | 0 (0) | 0 (0) |
  | errors by type | none | none |
  | routing-caused by type | none | none |
  | manufacturing breaches / below minimum / via in pad | 0 / 0 / 0 | 0 / 0 / 0 |
  | prepare + load, route (s) | 3.20, 38.2 | 1.01, 31.7 |
  | router: unrouted nets at start, final unrouted / violations | 146, 109 / 206 | 105, 86 / 206 |
  | aborted | 0 | 0 |
  | session sha256 | `73250af03ac4e5321aeadc45ab179b0f38b3d37e18b86cf58a0ea0e6afb1773e` | `27b2446a2e3850a2265a4fb6f7e201413abb3cc739406a47d27a1ac34eea26b4` |
  | design sha256 | `696472a3…` | `166bcbb9…` |
  | args sha256 | `0e890d65…` | `ef736990…` |

  - *Validity.* (a) as amendment 1 records. (b) the kicad_pcb `args` carry exactly the three flags amendment
    1 names; the args.json `log` reports them ("board snapshot fix-ups for the router: …"), and the router
    applied them (`route.log`: four `input fix-up: class … width` lines, "pours: 183 fixed wires for 12
    pour(s) of Power_High, Switch_Node; 14 outer plane(s) removed" and "pours: 2 outer plane(s) removed
    (dropped nets: GND)"). Amendment 1 put the router's lines in the args.json `log`; they are in
    `route.log`, which changes no number. (c) the dsn `log` has "pours: 183 fixed wires for 12 pour(s) of
    Power_High, Switch_Node; 16 outer plane(s) removed (dropped nets: GND)" and the four `class … width`
    lines. Both arms: 183 fixed wires for 12 pours, 16 outer planes removed.
  - *Rule.* Every routing-caused manufacturing type, below-minimum and via-in-pad is 0 in both arms.
    kicad_pcb's KiCad unconnected 0 ≤ dsn's 0 + 10. Both session sha256s are above. All three hold.
  - *What the numbers say.* This canary revision is already routed (1397 segments, 212 vias), and KiCad
    counts 0 unconnected on the input, so the grade cannot separate the arms on completion; it does show
    both inputs re-import the saved setup's routing with no breach. The router's own counts differ
    (146 against 105 unrouted nets at the start) because it does not count connections made through
    pours the same way on the two inputs (K13.1's table); KiCad's count decides here.
  - *Remains:* the owner's confirmation from a Route inside KiCad on canary-pwr, with its saved options,
    on the snapshot (recording that board's sha256). The same run confirms K13.7.
