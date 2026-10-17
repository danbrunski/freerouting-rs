# G08 — Save, unsaved changes and the front-end mode

Opened 2026-10-15. The app can route after G07 but cannot write anything.
This checklist adds Java's file output: Save As to SES or DSN with the
optional `.rules` file, the unsaved-changes prompts, the `.rules` import on
load, and the front-end mode Java's KiCad plugin uses (`-de board.dsn -do
board.ses`: auto-start after a countdown, write the session when routing
ends, exit). It is the report's GUI-8 (GUI, plugin and packaging report
§1.2, §4). The KiCad plugin's "Route in GUI" button (K02.5) depends on
G08.4 and G08.5.

**Prerequisites:**
- [G07](G07-autorouter-run-and-live-redraw.md): the routing job and its
  `Finished` event (G08.4, G08.5).
- C05: SES writer; C06: DSN writer; C04: `.rules` reader and writer.
- B01's `state_hash` (board content equality), for G08.2's "undo back to
  the saved state is clean" rule. Without it G08.2 uses the generation
  counter only and is partial.

**Not in this checklist** (G00 question 2): `.frb` files, the Autodesk
Fusion `.scr` export and the KiCad session JSON export. Java's Save As
offers all three (`BoardExportActions.showSaveAsDialog` filters `ses`,
`scr`, `dsn`, `json`). They are added only if Dan asks.

## What exists and what this adds

| Exists | Where |
|---|---|
| File browser | `crates/freerouting-gui/src/file_dialog.rs` (open only, filtered to `.dsn`/`.ses`; deliberately not `rfd`, see `gui.md`) |
| Loading DSN + SES | G01's `Session` load through `build_board` and `import_session` |
| Command-line parsing | `src/main.rs` takes positional `board.dsn [board.ses]`; G01 adds `--bench`, `--screenshot`, `--exit` |
| Writers | **missing** at HEAD: C04, C05, C06 |
| Java at `cda540f` | `gui/board/{BoardFrameFileActions,BoardExportActions,BoardFrame,GuiManager}.java`, `gui/menus/BoardMenuFile.java`, `settings/{GlobalSettings,GuiApplicationSettings}.java`, `core/RoutingJob.java` (`getRulesFile`), `management/HeadlessBoardManager.java` (`calculateCrc32`) |

What this adds: `src/files.rs` (save, rules prompt, atomic writes), a save
mode in `file_dialog.rs`, the front-end flags in `src/main.rs`, and
`tests/ui_files.rs`.

## Decisions made here

- **Java's menu, not a new one.** Java's File menu has Open (Ctrl+O),
  Save as… (Ctrl+S) and Exit (Alt+F4) (`BoardMenuFile`,
  `BoardMenuBar`). There is no plain Save. The port keeps that: Ctrl+S
  opens Save As, pre-filled with the last output path or
  `<design dir>/<design stem>.ses`, SES as the default type.
- **After an SES save, ask about rules** with Java's
  `confirm_rules_save` (`BoardFrame_en.properties`), default No
  (`WindowMessage.confirm(..., NO_OPTION)`). Yes writes the `.rules` file
  next to the output, named like the output with `.rules`
  (`RoutingJob.getRulesFile`).
- **No partial or empty files.** Every write goes to a temporary file in
  the same directory, then is renamed over the target. Java skips writing
  when the output is 0 bytes (`GuiManager.exportBoardToFile`: "not
  overwriting"); the port does the same and reports it.
- **"Changed" means the content changed**, as in Java, where
  `isBoardChanged()` compares a CRC32 of the board written as DSN with the
  one taken at load (`HeadlessBoardManager.calculateCrc32`). The port
  checks `edit_generation != saved_generation` first (cheap) and, when that
  says dirty, compares B01's `state_hash` with the hash at the last save
  or load. Undoing back to the saved state therefore counts as clean, like
  Java.
- **The Save button in the open/drop prompt saves.** Java's
  `confirmSaveBeforeLoad` "Save" branch only logs and proceeds without
  saving (the comment says the save "would be handled by the file menu").
  The port opens Save As and loads the new file only after a successful
  save. This is GUI behaviour with no routing effect; it gets a
  DIVERGENCES line.
- **Front-end flags are Java's.** `-de <design>`, `-do <output>`, `-dct
  <seconds>` and `-dr <rules>` keep their Java meaning
  (`GlobalSettings.applyCommandLineArguments`). `--autoroute` is accepted
  as an alias of `-dct 0` for scripts. The positional
  `freerouting-gui board.dsn [board.ses]` form stays.

## File ownership

Paths are under `crates/freerouting-gui/`.

| Item | New files | May edit |
|---|---|---|
| G08.1 | `src/files.rs`, `tests/ui_files.rs` | `src/file_dialog.rs` (save mode), `src/commands.rs`, `src/session.rs` (`saved_generation`, saved hash), `src/text.rs`, `src/lib.rs` |
| G08.2 | — | `src/app.rs` (close request), `src/files.rs` |
| G08.3 | — | `src/session.rs` (load path), `src/files.rs` |
| G08.4 | — | `src/main.rs`, `src/routing.rs` (finish hook), `src/files.rs` |
| G08.5 | `src/dialogs/autostart.rs` | `src/main.rs` |
| G08.6 | `tools/gui/frontend-smoke.sh` | — |
| G08.7 | — | `docs/development/gui.md`, `docs/DIVERGENCES.md` (append) |

## Checklist

- [x] **G08.1 — Save As: SES, DSN and the rules prompt.**
  - Java: `BoardFrameFileActions.saveAs` (SES, then `confirm_rules_save`
    and `saveRulesAs(routingJob.getRulesFile(), designName)`; DSN through
    `saveAsSpecctraDesignDsn(file, designName, false)`),
    `BoardExportActions.showSaveAsDialog` (filters `Specctra session file
    (*.ses)`, `Specctra design file (*.dsn)`; default SES). Status texts:
    `message_specctra_ses_saved`, `message_specctra_ses_save_failed`
    (`BoardFrame_en.properties`), `save_message`,
    `error_design_save_failed` (`BoardMenuFile_en.properties`).
  - Rust: `files::save_as(session, path, kind)` calls C05 or C06 with the
    design name (the output's file stem, as `exportBoardToFile` derives it),
    then C04 for the rules on Yes. The file dialog gains a save mode with a
    file-name field, the two filters and an overwrite confirmation.
    On success `saved_generation = edit_generation` and the saved hash is
    stored.
  - **Waits on C04, C05, C06.**
  - **Accept:** `tests/ui_files.rs`:
    - `save_ses_then_reopen_has_equal_statistics`: on three fixtures with a
      `.ses` (one of them `Issue593-BBD_Mars-64`), load DSN + SES, Save As
      SES into a temp dir, reopen the DSN with the new SES; every compared
      `BoardStatistics` field is equal (the fields `tests/reference_load.rs`
      compares).
    - `save_dsn_round_trip`: Save As DSN, reopen it alone; statistics equal.
    - `rules_prompt_writes_rules_file`: Yes writes `<stem>.rules`, and
      loading it with C04's reader into a fresh board succeeds; No writes
      nothing.
    - `failed_write_leaves_old_file`: saving into a read-only directory
      shows the failure text and leaves the old file byte-identical.

  *Done 2026-10-16:* `files.rs` (`save_as`, `save_rules`, `export_output`, `SaveKind`, names), `app_files.rs`
  (`save_as_dialog`, `save_as`, the Rules question), `file_dialog.rs` save mode (`show_save`, file name field,
  filters, `Replace` question), `Command::{SaveAsDialog, SaveAs, SaveCancelled, AnswerPrompt}`, `Ctrl+S` and
  File > Save as.... Writes go through `job::outputs::write_atomic` (the CLI's atomic writer: a temporary file in the
  target folder, renamed; an empty result is an error). Success sets `saved_generation` and the saved checksum
  (`Session::mark_saved`). `tests/ui_files.rs` (all green, `cargo test -p freerouting-gui --test ui_files`):
  - `save_ses_then_reopen_has_equal_statistics`: Issue026-J2_reference, Issue508-DAC2020_bm05 and
    Issue593-BBD_Mars-64 (DSN + SES, Save As SES, reopen DSN + the new SES): the whole `BoardStatistics` equal,
    counts exactly and floats within `reference_load`'s `floats_match` (0.006 + 1e-5 of the value; a trace length
    sums in another order after the import: 2352.5637 against 2352.563), and the import has 0 errors.
    Issue313-FastTest is not used: its DSN already has wiring, which the saved session contains again.
  - `save_dsn_round_trip`: Issue026, Issue508-bm05 and Issue742-tastexx-pcb, reopened alone, statistics equal.
    Issue593's DSN has a pinless footprint `G***` that C06's writer leaves out, so it comes back with 79
    components instead of 80 (not a G08 matter; see the report).
  - `rules_prompt_writes_rules_file`: Yes writes `yes.rules` next to `yes.ses`, C04's `read_rules` takes it into a
    fresh board without error; No writes no `.rules`.
  - `failed_write_leaves_old_file`: a 0555 folder: the status says `Unable to save the Specctra session file`
    (Java's `message_specctra_ses_save_failed`), the old file is byte-identical, no temporary file is left, no
    question is asked. More: `save_refuses_other_extensions_and_writes_nothing`,
    `ctrl_s_opens_the_save_dialog_and_asks_before_replacing` (default `<stem>.ses`, then the last output; Replace),
    `a_failed_save_does_not_load_the_new_file`, unit tests `file_dialog::tests::save_target_follows_the_filter`,
    `files::tests::{crc32_is_java_crc32, names_follow_java}`.
  - Differs from the text above: Java's `saveAs` names the design after the *input* file (`board.dsn`, so the
    session is `board.ses`), not the output's stem (that is `exportBoardToFile`, G08.4); the port does both as Java
    does.

- [x] **G08.2 — Unsaved-changes prompts.**
  - Java: `BoardFrame.confirmSaveBeforeLoad` (open and drag-and-drop:
    `confirm_save_changes` with `confirm_save_yes`, `confirm_save_no`,
    `confirm_save_cancel`, title `confirm_save_title`) and the exit check
    (`confirm_cancel`: "The board has changed since you last saved it. Are
    you sure you want to exit?", `confirm_exit_yes`, `confirm_exit_no`).
  - Rust: `App::update` watches `ctx.input(|i| i.viewport().close_requested())`;
    when the session is dirty it sends `ViewportCommand::CancelClose` and
    shows the exit dialog. Open and drop go through the same check first.
    In front-end mode (G08.4) the close path writes `-do` instead (Java
    `windowClosing` listener), so no prompt appears there.
  - **Waits on B01 for the hash comparison.**
  - **Accept:** `tests/ui_files.rs`:
    - `close_with_changes_asks`: after one edit, a close request shows the
      dialog; "No" keeps the app running (the harness still steps).
    - `undo_to_saved_state_is_clean`: edit, then undo; a close request
      closes without a dialog.
    - `drop_with_changes_save_then_load`: "Save" opens Save As; after the
      save the dropped file is loaded; "Cancel" keeps the old board.

  *Done 2026-10-16:* `app_files.rs` (`close_request`, `Prompt::{Exit, UnsavedBeforeLoad}`, `open_checked`),
  `Session::{is_changed, mark_saved, saved_hash}`, `files::content_hash`. `Open` (menu, dialog, recent, drop)
  asks before it replaces a changed board; the close request asks `confirm_cancel`. `tests/ui_files.rs`:
  - `close_with_changes_asks`: unchanged, the close goes through; after Delete All, `CancelClose` is sent and
    the Exit question is shown (Java's text, found by AccessKit); No keeps the app running; asked again, Yes sends
    the close.
  - `undo_to_saved_state_is_clean`: edit, undo: `is_dirty()` is true, `is_changed()` false, and a close request
    closes without a question. `save_marks_the_state_clean` covers save, undo (changed), redo (clean).
  - `drop_with_changes_save_then_load`: Cancel keeps the old board (still changed); Don't Save loads; Save opens
    Save As, nothing loads before the save, the Rules question is answered, then the new file loads; closing the
    save dialog without a file keeps the old board; a failed save does not load (`a_failed_save_does_not_load_the_new_file`).
  - **Not B01's `state_hash`:** it covers the undo history (stack level, `redo_possible`, version nodes), so an
    edit followed by an undo would stay "changed". The port uses what Java uses, the CRC32 of the DSN text
    written with the name `N/A` (`HeadlessBoardManager.calculateCrc32`), taken at load (on the load thread) and at
    each save; it costs 4 to 11 ms on the small fixtures. The edit counter is still checked first.

- [x] **G08.3 — Import the `.rules` file found next to the design.**
  - Java: `GuiManager.createBoardFrame` (lines ~432–458 at `cda540f`): for a
    DSN input, if `<design name>.rules` exists in the DSN's directory, ask
    `confirm_import_rules` ("Rules imported from the host system may
    overwrite your current rules.") and read it; with `-dr <file>` read that
    file without asking. The design name is Java's `routingJob.name`
    (unverified that it is always the file stem). Whether File > Open does
    the same at the pin is unverified; check `BoardFrame.loadFromBytesAsync`
    and follow it.
  - Rust: the load path in `session.rs` checks for the file after
    `build_board`, and the prompt applies it through C04's reader as one
    undoable edit.
  - **Waits on C04.**
  - **Accept:** `tests/ui_files.rs::rules_next_to_dsn_imported_after_confirm`:
    loading `fixtures/Issue029-hw48na.dsn` (its `Issue029-hw48na.rules`
    sits beside it) asks; Yes changes the board rules exactly as C04's
    reader does on a fresh board (compare the clearance matrix and net
    classes); No leaves them as built from the DSN.

  *Done 2026-10-16:* `app_files.rs` (`startup_rules`, `import_rules`, `Prompt::ImportRules`). After the start-up
  load of a DSN, `-dr <file>` is read without a question, else `<stem>.rules` beside the design is offered with
  Java's `confirm_import_rules` when the DSN came from a host CAD (`dsn_file_generated_by_host`, Java's condition).
  `read_rules` (C04) runs on a copy of the board; the file's `autoroute_settings` go on top of the next job's
  settings. File > Open and a drop do not look for the file: at the pin `BoardFrameFileActions.open` and
  `loadFromBytesAsync` never read rules (`GuiManager.createBoardFrame` only; `file_open_does_not_look_for_rules`).
  `routingJob.name` is the file stem (`RoutingJob.setInput`, `getFilenameWithoutExtension`). Tests:
  `rules_next_to_dsn_imported_after_confirm` (Issue029-hw48na: the question appears, Yes gives exactly the
  clearance matrix and net classes of C04's `read_rules` on a fresh board (`Debug` of both), which differ from the
  DSN's; No leaves them as built), `dr_flag_imports_without_asking`. Not "one undoable edit": rule changes are not
  in the board's history here or in Java (G06, `RulesEdit::touches_items`); the import counts as an edit
  (`is_changed`) so closing asks. DIVERGENCES has the `-dct 0` and `-dr` cases.

- [x] **G08.4 — Front-end mode: `-de` and `-do`.**
  - Java: `GuiManager.createBoardFrame` lines ~182–245: with `-do`, delete
    an existing output file before starting; install a listener that writes
    the output on `autorouterFinished` and on `autorouterAborted`
    (`exportBoardToFile(frame, file, exitWhenFinished)`); on window close,
    stop the router and write the output if `isBoardChanged()` or the
    board has traces. `exportBoardToFile` accepts `.dsn` and `.ses` (and
    `.scr`, `.json`, which the port leaves out), derives the design name
    from the output's file stem, refuses a 0-byte result, and calls
    `System.exit(0)` when `exitWhenFinished`.
  - Rust: `main.rs` parses `-de`, `-do`, `-dct`, `-dr` and `--autoroute`
    into `FrontEndOptions`. `routing.rs` gets a finish hook that calls
    `files::export_output` on `Finished` (completed or stopped); the close
    path does the same. Exit uses `ViewportCommand::Close` after the write,
    and the process exit code is 0 on a written file, 1 on a failed write.
  - **Waits on G07, C05 (C06 for a `.dsn` output).**
  - **Accept:** `tests/ui_files.rs`:
    - `frontend_deletes_old_output`: a stale `out.ses` is gone right after
      start.
    - `frontend_writes_on_finish`: with `-dct 0` on
      `Issue159-setonix_2hp-pcb.dsn` and `max_passes = 2`, `out.ses` exists
      after `Finished` and imports with `read_session` without errors.
    - `frontend_writes_on_close_when_routed`: closing during routing stops
      the job and writes `out.ses`.

  *Done 2026-10-16:* `args.rs` (`-de`, `-do`, `-dr`, `-dct`, `-di`, `-mp`, `--autoroute`, `FrontEndOptions`, `+`
  splitting; unit tests `parses_the_front_end_flags`, `rejects_bad_front_end_flags`), `app_files.rs`
  (`init_files`, `frontend_job_ended`, `close_request`, `write_output`), `files::export_output`, `main.rs` (exit code
  through `freerouting_gui::exit_code`), a hook in `app_routing::poll_routing`. `-mp` was added (not in the
  checklist) so a test and a script can limit the passes; it is the one router flag read. `tests/ui_files.rs`:
  - `frontend_deletes_old_output`: a stale `out.ses` is gone after start.
  - `frontend_writes_on_finish`: `-dct 0 -mp 2` on Issue159: after `Finished` the app asks to close,
    `out.ses` exists, the exit code is 0, `read_session` imports it with 0 errors and some wires, no temporary
    file is left.
  - `frontend_writes_on_close_when_routed`: on Issue684 the close during routing is held back, the job stops,
    `out.ses` is written and imports, then the close goes through. Also
    `frontend_close_after_cancelled_countdown_writes_a_routed_board` (traces present), `..._of_an_untouched_empty_board_writes_nothing`,
    `frontend_failed_write_exits_with_1` (a missing folder: exit code 1), `without_do_nothing_is_written_or_started`.
  - Differs from Java (DIVERGENCES, "Saving, the questions and the front-end mode"): exit code 1 after a failed
    write or load (Java exits 0, or not at all); only `-mp` of the CLI's router flags; `-dct 0` reads the
    adjacent rules without the question.

- [x] **G08.5 — Auto-start countdown.**
  - Java: `GuiManager` lines ~245–330: when both `-de` and `-do` are given,
    a dialog `auto_start_routing_title` with `auto_start_routing_message`
    and the buttons `auto_start_routing_startnow_button` (shown as
    "Start Now (n)", counting down once a second from
    `dialogConfirmationTimeout`, default 5, `GuiApplicationSettings.java:23`)
    and `auto_start_routing_cancel_button`. At 0 it starts as if clicked.
    Start sets `exitWhenFinished = true`; Cancel sets it false and leaves
    routing idle. `-dct 0` (or a value ≤ 0) shows no dialog and starts at
    once.
  - Rust: `dialogs::autostart` counts down with `ctx.request_repaint_after`
    (one second); the countdown is GUI time and never reaches the router.
  - **Accept:** `tests/ui_files.rs`:
    - `countdown_starts_routing`: after stepping the harness 5 s of
      simulated time the job is running and the button text went
      `Start Now (5)` … `Start Now (1)`.
    - `countdown_cancel_stays_idle`: Cancel leaves no job, and a later
      Finished (started by hand) does not exit the app.
    - `dct_zero_starts_without_dialog`.

  *Done 2026-10-16:* `dialogs/autostart.rs` (`Autostart`, `Choice`, `seconds_left`), `app_files.rs`
  (`startup_autostart`, `autostart_step`, `autostart_chosen`). The countdown runs on egui's input time (a kittest
  step is 0.25 s), shows `Autorouter Confirmation` with Java's text and `Start Now (n)` / `Cancel`, and starts as
  Java's timer does (5 s: 5 to 1, start at 5.0 s). Start Now sets `exit_when_finished`; Cancel clears it. Tests:
  - `countdown_starts_routing`: the button text went `Start Now (5)` ... `Start Now (1)` (each once, in order),
    then the job runs and, at its end, the output is written and the app asks to close.
    `countdown_start_now_starts_at_once` (clicks the button).
  - `countdown_cancel_stays_idle`: no job after 10 simulated seconds; a job started by hand writes `out.ses` and
    does not exit the app.
  - `dct_zero_starts_without_dialog` and `autoroute_flag_is_dct_zero`; unit test
    `autostart::tests::counts_down_once_a_second_like_java`.
  - Seen under Xvfb (release): the dialog is centred over the board with `Start Now (3)` and Cancel
    (`target/orchestrator/g08/countdown.png`). Whether it looks right to Dan is his call.

- [x] **G08.6 — End-to-end front-end check under Xvfb.**
  - `tools/gui/frontend-smoke.sh <fixture>` runs the release binary with
    `-de <fixture> -do <tmp>/out.ses -dct 0` under
    `xvfb-run -a -s '-screen 0 1280x800x24'` with `WGPU_BACKEND=vulkan`,
    waits for the exit (timeout 120 s), and checks the exit code and that
    `freerouting -de <fixture> <tmp>/out.ses -drc` imports the session
    with no "(N errors)" in its import line (`drc/runner.rs`).
  - **Accept:** the script exits 0 on `Issue159-setonix_2hp-pcb.dsn`; a
    screenshot taken during the run (implementer recipe) shows the board
    with routed traces, and the implementer describes it in the done note.

  *Done 2026-10-16:* `tools/gui/frontend-smoke.sh <fixture.dsn> [max passes]` (release GUI under
  `xvfb-run -a -s '-screen 0 1280x800x24'` with `WGPU_BACKEND=vulkan`, `-de <fixture> -do <tmp>/out.ses -dct 0`,
  timeout 120 s, then the CLI `-de <fixture> <out.ses> -drc` and a check for `SES file loaded for DRC` without
  `(N errors)`). It takes screenshots in a loop while the GUI runs and keeps the last one (the window is not
  waited for with xdotool: Issue159 is routed and the app gone in under a second, so the wait timed out).
  Run on `fixtures/Issue159-setonix_2hp-pcb.dsn` with release binaries built from this tree: exit 0, "the GUI
  exited 0 and wrote 8972 bytes", "SES file loaded for DRC: 51 wires, 0 vias imported". The last screenshot
  (`target/orchestrator/g08/frontend-shot.png`) shows the 108 mm tall board with its routed traces on F.Cu and
  B.Cu, the Routing Summary window (`Auto-routing Completed`, 9 nets, 0 incomplete connections, 51 trace segments,
  score 1000.00) and the status line `Saved 8972 bytes to '/tmp/tmp.../out.ses'.`. Because the app exits at the end
  of the run, the screenshot is of the last frame before the exit, not of a frame in the middle of a pass.

- [x] **G08.7 — Docs.**
  - `gui.md` "What works" gains saving, the prompts and the front-end mode
    (with the flags); "Other gaps" loses "Saving". DIVERGENCES.md gets the
    line for the Save branch of the open/drop prompt (decisions).
  - **Accept:** both files updated; `git grep -n "no Save" docs/development/gui.md`
    finds nothing.

  *Done 2026-10-16:* `docs/development/gui.md`: "What works" has the G08 bullet (saving, the questions, the
  start-up rules and the front-end mode with its flags), the layout table has `files.rs`, `app_files.rs`,
  `dialogs/autostart.rs`, `args.rs` and `file_dialog.rs` rows, "Other gaps" says saving is done (and what is not
  offered by decision). `docs/DIVERGENCES.md` has a "Saving, the questions and the front-end mode" section (the
  Save branch of the prompt, the checksum, the rules cases, the flags and exit codes).
  `git grep -n "no Save" docs/development/gui.md` finds nothing.
