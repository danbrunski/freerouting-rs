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
- [G07](archive/G07-autorouter-run-and-live-redraw.md): the routing job and its
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

- [ ] **G08.1 — Save As: SES, DSN and the rules prompt.**
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

- [ ] **G08.2 — Unsaved-changes prompts.**
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

- [ ] **G08.3 — Import the `.rules` file found next to the design.**
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

- [ ] **G08.4 — Front-end mode: `-de` and `-do`.**
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

- [ ] **G08.5 — Auto-start countdown.**
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

- [ ] **G08.6 — End-to-end front-end check under Xvfb.**
  - `tools/gui/frontend-smoke.sh <fixture>` runs the release binary with
    `-de <fixture> -do <tmp>/out.ses -dct 0` under
    `xvfb-run -a -s '-screen 0 1280x800x24'` with `WGPU_BACKEND=vulkan`,
    waits for the exit (timeout 120 s), and checks the exit code and that
    `freerouting -de <fixture> <tmp>/out.ses -drc` imports the session
    with no "(N errors)" in its import line (`drc/runner.rs`).
  - **Accept:** the script exits 0 on `Issue159-setonix_2hp-pcb.dsn`; a
    screenshot taken during the run (implementer recipe) shows the board
    with routed traces, and the implementer describes it in the done note.

- [ ] **G08.7 — Docs.**
  - `gui.md` "What works" gains saving, the prompts and the front-end mode
    (with the flags); "Other gaps" loses "Saving". DIVERGENCES.md gets the
    line for the Save branch of the open/drop prompt (decisions).
  - **Accept:** both files updated; `git grep -n "no Save" docs/development/gui.md`
    finds nothing.
