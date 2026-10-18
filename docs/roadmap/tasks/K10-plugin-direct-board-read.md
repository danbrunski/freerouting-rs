# K10 — The plugin reads the board directly

Opened 2026-10-17, after the owner asked whether DSN and SES are the right interface
for the plugin, or whether it should integrate with KiCad more closely, as
KiCadRoutingTools (KRT) does.
- **The answer:** keep DSN and SES as the default. Add a direct read of the board as an
  option, measure it, and let the owner decide whether it becomes the default.
- **The result still comes back through `ImportSpecctraSES`.**

Prerequisites:
- K02 (the routing dialog and options);
- K07 (`-de board.kicad_pcb`, the reader's parity);
- K09, for K10.4 only.

## What exists and what this adds

**How KRT integrates** (read at `0486e799`; ideas only, no code is copied):
- **It is a SWIG action plugin,** like this one (`kicad_routing_plugin/action_plugin.py`).
  It stops loading in KiCad 11 in the same way (K01.5).
- **It snapshots the open board** with `pcbnew.SaveBoard(tmp, board, aSkipSettings=True)`
  and parses the copy itself (`gui_utils.py`).
  - `aSkipSettings` avoids the settings save that aborted KiCad on worker threads for
    projects older than KiCad 10.
  - It also means no `.kicad_pro` is written. KRT re-authors that from the live board
    (`stage_live_project_rules`), because the file on disk lacks the rules changed in
    Board Setup and not yet saved (KRT #627).
- **It writes results onto the open board** with `board.Add(pcbnew.PCB_TRACK(...))`, the
  same for vias, then calls `pcbnew.Refresh()` (`fanout_gui.py`, `board_swaps.py`).
  - It does this because it produces what a session can't carry: teardrops, via tenting,
    footprint swaps and plane refills.
- **The current selection picks the nets to route** (`_get_selected_net_names`).

**What the DSN path loses**, with the item that measured each loss:

| Lost in KiCad's export | Today | With the reader |
|---|---|---|
| Board Setup minimum track width (D8) | the plugin passes `--router.min_trace_width_um` (`board_min_track_width_um`) | not in the reader either: still passed |
| `.kicad_dru` rules | not exported | K07.5 reads the expressible subset (`ReadOptions::apply_design_rules`, off by default; no CLI switch yet) |
| Rule-area kinds | full keepouts; K01.2 rewrites the DSN text (`rule_area_fixup`) | mapped by their flags (K06.2) |
| Footprint-owned copper zones | dropped | kept as planes (K00 question 6) |
| Net-class order | KiCad's `unordered_map` order, not reproducible | the project's order |
| Shape precision | `%.6g`, about 1 µm lost above 100 mm (K00 question 2) | KiCad's nm, rounded to 0.1 µm units |
| Track arcs | one chord per arc | followed as polylines (K06.5) |

**The pieces this checklist joins:**
- `freerouting -de board.kicad_pcb -do board.ses` routes a KiCad board. KiCad 10.0.7's
  `ImportSpecctraSES` imports the session it writes (K07.2, K07.4,
  `cli_kicad.rs::route_job_writes_a_session`).
- The reader reads `<stem>.kicad_pro` and `<stem>.kicad_dru` beside the board
  (`crates/freerouting/src/io/kicad.rs`, `sibling`). A missing one is not an error.
- The GUI opens a `.kicad_pcb` (K07.2), so "Route in GUI" and "Open in viewer" can take
  the snapshot too.

**The open differences from KiCad's export** (K07.6, 57 corpus boards). These are what
K10.5 has to show don't hurt:
- traces the reader merges differently (`video`, `vme-wren` and others);
- courtyards the export drops;
- keepout order;
- one board with an extra pin/via violation (`One-Air-Max`);
- the reader's tree builds into a board about 2× slower (`jetson`: 3.7 s against 1.6 s
  for the DSN).

## Decisions made here

- **DSN and SES stay the default.**
  - The DSN path is the Java comparison (A14) and what the plugin was graded with
    (K02, K09).
  - KiCad 11's IPC API keeps exactly these two board operations,
    `RunBoardJobExportSpecctraDsn` and `ImportSpecctraSession` (K01.5).
  - KiCad's import already does the merge: it skips locked tracks, deletes unlocked
    ones and places footprints.
- **One option, `board_input`, with the values `dsn` (the default) and `kicad_pcb`.**
  - It changes only what the router reads.
  - With `kicad_pcb`, `rule_area_fixup` does nothing, because the reader maps rule areas
    by flags. The run log says so once.
- **The snapshot is taken on KiCad's UI thread,** with `aSkipSettings=True`. It goes into
  the work folder as `freerouting-rs/<stem>.kicad_pcb`, beside `<stem>.dsn`, so the
  session's name is the same as on the DSN path.
- **Results keep coming back through `ImportSpecctraSES`.**
  - Writing to the open board (KRT's `board.Add`) is not part of this checklist.
  - K10.6 records when it becomes worth doing.
- **Making `kicad_pcb` the default is the owner's decision,** after K10.5's measurement. Until
  then the option is off and labelled as experimental in the dialog.

## File ownership

| Item | New files | May edit |
|---|---|---|
| K10.1–K10.4 | `kicad-plugin/freerouting_rs/snapshot.py`, `kicad-plugin/tests/test_snapshot.py` | `plugin.py` (the snapshot call and the input passed on), `options.py` (`board_input`, `apply_kicad_rules`, schema bump), `routing.py` (input path), the options dialog |
| K10.3 | — | `crates/freerouting-cli/src/args.rs`, `cli.rs`, `help.txt` (one switch for `apply_design_rules`), `crates/freerouting-cli/tests/cli_kicad.rs` |
| K10.5 | `tools/kicad-reader/plugin-input-ab.sh` (or a mode of `frame-check.sh`) | — |
| K10.6–K10.7 | — | `docs/development/kicad-plugin.md`, `kicad-plugin/README.md` |

## Checklist

- [x] **K10.1 — The option and the snapshot.** Add `board_input` to the options, with a
  dialog field. With `kicad_pcb`:
  - `plugin.py` calls `pcbnew.SaveBoard(path, board, aSkipSettings=True)` on the UI
    thread instead of `ExportSpecctraDSN`.
  - "Route", "Route in GUI" and "Open in viewer" pass the snapshot instead of the DSN.
  - The pure parts live in `snapshot.py`, with no `pcbnew` and no `wx` (K00 rule): the
    paths, the files to stage, and the arguments.
  - **Accept:**
    - unit tests give the argument lists for both values, and for a board path with
      non-ASCII characters;
    - in a headless pcbnew run (WSL KiCad 10.0.7, on a copy of `Issue558-dev-board`),
      a track added to the board in memory before the snapshot is on the reader's board;
    - `freerouting -de` on the snapshot writes a session that `ImportSpecctraSES`
      imports with 0 errors.

  *Done 2026-10-17:* `kicad-plugin/freerouting_rs/snapshot.py` (pure paths, staged files, arguments;
  `take` calls pcbnew passed in), `export_flow.prepare_snapshot`, option `board_input` (`dsn` default,
  `kicad_pcb`; schema 4, schema 3 files load as `dsn`), dialog "Board input" drop-down; "Route",
  "Route in GUI" and "Open in viewer" pass the snapshot (`plugin.Dialog.export` sets `self.design`).
  - Argument lists for both values and non-ASCII names: `test_snapshot.py::
    test_the_argument_lists_for_both_inputs_with_non_ascii_names` (4 stems, a non-ASCII folder),
    `test_the_design_file_is_the_dsn_or_the_snapshot_beside_it`; dialog:
    `test_route_dialog.py::test_route_with_the_snapshot_gives_the_router_the_kicad_pcb`,
    `test_the_default_input_still_exports_the_dsn`,
    `test_route_in_gui_and_the_viewer_take_the_snapshot_too`.
  - Headless pcbnew (WSL KiCad 10.0.7, copy of `Issue558-dev-board`), `test_snapshot_headless.py`
    (`headless_run.py snapshot`, new): `test_a_track_added_in_memory_is_on_the_readers_board`
    (reader report `wires: 0` for the board on disk, `wires: 1` for the snapshot);
    `test_the_router_session_of_a_snapshot_imports_with_no_error` (`freerouting -de` on the snapshot
    writes a session; `ImportSpecctraSES` returns true and the board gets segments and vias);
    `test_a_snapshot_of_an_unchanged_board_checks_like_the_board` (DRC reports equal as multisets).
  - The result goes back through `plugin.import_ses` (detach first, 9bc7ee6):
    `test_the_snapshot_result_comes_back_through_the_detaching_import`.
  - Not seen inside KiCad (owner, K02.8): the dialog's layout, and `SaveBoard`/`SaveProjectCopy` in the
    running editor.

- [x] **K10.2 — The project's rules come along.** Stage `<stem>.kicad_pro` and
  `<stem>.kicad_dru` beside the snapshot. Decide by test between two ways:
  - (a) copy the files on disk;
  - (b) re-author the net classes and clearances from the live board, as KRT does.

  The test changes one net class's clearance in memory, without saving: (a) misses it,
  (b) must not.
  - **Accept:**
    - the chosen way passes the test;
    - or, if (a) is chosen, the done note names what is missed, and the dialog tells
      the user to save first when the board has unsaved changes (whether pcbnew can
      tell is unverified: check it here).
  - On KiCad 11, K11.1 found that `SaveCopyOfDocument` with `include_project` writes the
    `.kicad_pro` with the copy, and that `GetDocumentModifiedState` reports unsaved
    changes. Whether that `.kicad_pro` carries unsaved Board Setup changes is still to
    test (K11.6).

  *Done 2026-10-17:* decided by test on KiCad 10.0.7: **(a) misses, (b) does not**
  (`test_snapshot_headless.py::test_an_unsaved_net_class_change_reaches_the_snapshot_only_through_the_live_project`:
  `Power` clearance 0.2 to 0.333 mm in memory, not saved. The live path puts 0.333 in the snapshot's
  `.kicad_pro` and leaves the file on disk at 0.2; the copy-from-disk fallback leaves 0.2 and logs
  `WARNING: project: ... not saved is not in the snapshot`). (b) is not a re-authored JSON but KiCad's
  own writer: `pcbnew.GetSettingsManager().SaveProjectCopy(path, board.GetProject())`, also on projects saved by
  older KiCad versions (`meta.version` 1); it writes a `.kicad_prl` too (deleted). The reader reads its
  output (net classes as arrays, `netclass_patterns`; the DRC equality test above). `.kicad_dru` is copied from
  disk (KiCad parses it from there; unsaved custom-rules edits unverified). `board.IsModified()` does not tell
  (False after an in-memory edit, headless), so there is no "save first" warning; (b) makes it unneeded for net
  classes. Unit tests: `test_snapshot.py::test_take_saves_the_board_without_settings_and_writes_the_live_project`,
  `test_a_missing_live_project_copy_falls_back_to_the_file_on_disk_and_says_what_is_missed`,
  `test_a_board_without_project_files_stages_none`. Unverified: (b) inside the running editor.

- [x] **K10.3 — Pass what the reader lacks.**
  - The minimum track width stays a CLI argument on both paths.
  - Add one CLI switch for `ReadOptions::apply_design_rules`, named by C02's settings
    conventions, and a plugin option `apply_kicad_rules`, off by default (K00 question
    7).
  - The run log lists the reader's report lines and warnings, as the CLI prints them.
  - **Accept:**
    - `cli_kicad.rs` shows the switch changes the class rules on the `classes` board
      exactly as `design_rules_reach_the_board` does;
    - a unit test shows the plugin passes the switch only when the option is on.

  *Done 2026-10-17:* CLI switch `--apply-design-rules` (`EXTRA_FLAGS`, `Args::apply_design_rules`, `cli.rs`
  builds `ReadOptions { apply_design_rules, .. }` for a route job; a DSN input gets a warning; `help.txt`);
  plugin option `apply_kicad_rules` (off by default; dialog check box), passed only with
  `board_input = kicad_pcb` and not to the GUI (which has no such switch; the log says so). The minimum track
  width stays `--router.min_trace_width_um` on both paths; the reader's report lines and warnings are in the
  router's log and copied to `plugin.log` (`snapshot.reader_lines`).
  - `cargo test -p freerouting-cli`: `cli_kicad.rs::the_apply_design_rules_switch_changes_the_class_rules`
    (on `classes` with `rules.kicad_dru`: off, both constraints `ignored (apply_design_rules is off)`; on,
    `applied (1 target(s))`, and the Fast wires in the session go from `path F.Cu 1000` to `3000`, as
    `design_rules_reach_the_board` shows for the board), `the_apply_design_rules_switch_on_a_dsn_is_a_warning`,
    `args_java.rs::apply_design_rules_is_a_switch`; `EXTRA_FLAGS.len()` 12 (`--capabilities` and `help.txt`
    list it: `tests/version.rs`).
  - Plugin: `test_snapshot.py::test_the_rules_switch_is_passed_only_when_the_option_is_on_and_the_input_is_the_snapshot`.

- [x] **K10.4 — K09's DSN rewrites and the direct read.** K09's options rewrite DSN text,
  so they can't apply to a `.kicad_pcb` input.
  - While any of them is on, the dialog refuses `board_input = kicad_pcb` and names the
    option, until that option has a reader-tree or engine form.
  - Options that already map to an engine setting are allowed, for example K09.2's arm
    (a) if it wins.
  - **Accept:** unit tests of the options check, for each K09 option that exists when
    this item runs.

  *Done 2026-10-17:* `options.input_conflicts` and the form check: with `board_input = kicad_pcb` the dialog refuses
  `class_widths`, `fixed_pour_classes` (as `wires`, or `auto`), `drop_pour_nets`, `edge_and_pad_keepouts` and
  `route_on_plane_layers`, naming each (`test_snapshot.py::
  test_each_dsn_rewriting_option_refuses_the_snapshot_by_name`, 5 parameters;
  `test_the_form_refuses_a_snapshot_with_a_dsn_option_on_and_names_it`;
  `test_route_dialog.py::test_a_snapshot_with_a_dsn_rewrite_on_is_refused_before_anything_is_saved`). Allowed
  because they are engine flags or board locks: `fixed_pour_mode = engine` with named classes (K09.2 arm a),
  `fix_existing`, `start_ripup_costs`, `sweep`, `sweep_passes`, `use_pass_hint`
  (`test_every_k09_option_is_either_checked_or_allowed` fails when a K09 option is added to neither list;
  `test_engine_pours_with_named_classes_work_on_a_snapshot_but_auto_does_not`;
  `test_route_dialog.py::test_fix_existing_locks_the_tracks_into_the_snapshot`). Note: `edge_and_pad_keepouts` is
  on by default (K09.9), so the user unticks it first
  (`test_the_defaults_refuse_the_snapshot_until_the_keepouts_are_off`).

- [~] **K10.5 — Measured against the DSN path.** Route each board both ways with the
  same options and grade both with KiCad's DRC (F02.6). Boards:
  - the five K02 demo boards;
  - `canary-pwr` with the options of the owner's plugin run in K09's header (68 unconnected on
    the DSN path);
  - the K07.6 corpus boards that have open differences, when the corpus is present.

  For each board, record:
  - unconnected items and violations by type;
  - the time to snapshot and read, against the time to export and parse.
  - **Accept:**
    - the table is in the done note;
    - every board where the direct read grades worse is explained, or tied to one of
      K07.6's open differences;
    - The owner's answer on the default is recorded (question 1).

  *Partial 2026-10-17:* `tools/kicad-reader/plugin-input-ab.sh` (new; one arm per call: the plugin's own export or
  snapshot, route, `ImportSpecctraSES`, `kicad-cli pcb drc`, `summary.json` with `prepare_s`, `load_s`,
  `route_s`, unconnected and violations by type). Smoke only (dev board `Issue558`, `--max-passes 1`, debug
  build, one run each): both arms 11 unconnected, 12 `copper_edge_clearance`, 4 `hole_clearance`; prepare 0.18 s
  (export) and 0.20 s (snapshot); load 0.02 s each. **Remaining, heavy run pending** (needs the heavy-job
  slot; options file `$O` = `{"schema":4,"threads":2,"edge_and_pad_keepouts":false}`, the same for both arms
  because the snapshot refuses the DSN rewrites; `$R` = a release `freerouting`):
  ```
  export FREEROUTING_BIN=$R
  for b in ecc83/ecc83-pp interf_u/interf_u complex_hierarchy/complex_hierarchy \
           kit-dev-coldfire-xilinx_5213/kit-dev-coldfire-xilinx_5213 cm5_minima/CM5_MINIMA_3; do
    for arm in dsn kicad_pcb; do
      tools/kicad-reader/plugin-input-ab.sh $arm /usr/share/kicad/demos/$b.kicad_pcb $O out/k10/$arm/$(basename $b)
    done
  done
  # canary-pwr, with the options of the owner's plugin run in K09's header (unlimited passes, 2 threads, optimizer on):
  for arm in dsn kicad_pcb; do tools/kicad-reader/plugin-input-ab.sh $arm <canary>/hardware/canary-pwr-rev1.0/canary-pwr.kicad_pcb $O out/k10/$arm/canary-pwr; done
  # K07.6 boards with open differences (video, vme-wren, One-Air-Max, jetson): the same loop on their .kicad_pcb
  ```
  Then fill the table (unconnected, violations by type, prepare+load time per arm), explain every board where
  `kicad_pcb` grades worse (tie to K07.6's open differences: merged traces, courtyards, keepout order,
  `One-Air-Max`'s pin/via violation) and record the owner's answer on the default (question 1).

- [x] **K10.6 — When to write to the open board.** Write down the trigger in
  `kicad-plugin.md`. A live write (KRT's `board.Add` plus `pcbnew.Refresh()`) is taken
  up when a stage first produces something a session can't carry, such as teardrops or
  zone fills, and not before.
  - That item must also cover undo. Whether KiCad's undo covers either path is
    unverified.
  - **Accept:** the paragraph exists and names the stages that would need it, with
    their checklist ids where they have one.

  *Done 2026-10-17:* `docs/development/kicad-plugin.md`, "The plugin reads the board directly (K10)", last paragraph:
  the trigger (a stage that first produces what a session cannot carry), the stages that would need it
  (teardrops, via tenting, footprint or pin swaps with I03/G09's editor pin swap as the one that exists, plane
  refills after added copper with Q10's note on pours cut by added traces), and that the item must decide undo
  (unverified for both paths; the detaching import lets the runner record removed tracks).

- [~] **K10.7 — Documentation.** `kicad-plugin.md` and the plugin README describe
  `board_input`, the staged project files, `apply_kicad_rules`, the K09 restriction and
  K10.5's numbers.
  - **Accept:** each fact names its test or item.

  *Partial 2026-10-17:* `kicad-plugin.md` (the K10 section: `board_input`, the staged project files,
  `apply_kicad_rules`, the K09 restriction, K10.6, each fact naming its test or item) and `kicad-plugin/README.md`
  describe everything but **K10.5's numbers**, which wait for the heavy run (the smoke result is in the section).
  Fill the table there when K10.5 is done.

## Open questions for the owner

1. **The default input after K10.5.** *Recommendation:* switch to `kicad_pcb` only if it
   grades the same or better on every K10.5 board. Otherwise keep `dsn` and list the
   boards that hold it back.
2. **Selection-aware routing**, which KRT has: route only the nets of the selected items.
   It needs a CLI way to route a subset of nets, which does not exist yet.
   *Recommendation:* not in K10. Open it as its own item if the owner wants it.
