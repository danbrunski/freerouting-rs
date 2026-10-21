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

- [ ] **K14.4 — Pours of listed classes as fixed wires.**
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

- [ ] **K14.5 — The plugin stops sending these boards to the DSN.** Once K14.1–K14.4 have landed:
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
