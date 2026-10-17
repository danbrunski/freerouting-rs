# K11 — KiCad 11: a plugin on the IPC API

Opened 2026-10-17, from the same question as K10.
- **KiCad master has removed SWIG, wxPython and the Python integration** (commit
  `65a442b1`, 2026-03-22, verified in K01.5). The plugin therefore stops loading in the
  release cut from master, presumably KiCad 11.
- **K00 question 5** put off an IPC-API plugin until KiCad 11's release date is known.
  K08, the direct writer, is done.
- **This checklist is that plugin.** It keeps the DSN and SES flow, because the IPC API
  on master has both board operations.

Prerequisites:
- K02 (the dialog's behaviour, to match);
- K03 (the package);
- K10, for K11.6 only.
- K11.2 onward also needs a KiCad 11 release date or release candidate (question 2).

## What exists and what this adds

**Verified in K01.5,** in the protobuf files on KiCad `master`:
- `RunBoardJobExportSpecctraDsn` (`board_jobs.proto`, "Since: 11.0") exports a DSN.
- `ImportSpecctraSession` (`board_commands.proto`, "Since: 11.0") imports a session. It
  takes a `session_path` and returns a `skipped_count` and a report.
  - It repositions footprints, deletes unlocked tracks and vias, adds the routed ones,
    and skips locked ones.
- Neither message exists in the `10.0` branch.
- `kipy` (kicad-python 0.8.0) wraps neither, so the plugin sends the raw protobuf
  command.

Not verified there:
- whether master's implementation is complete;
- how the job exports rule areas.

**From the Java fork's research note** (`git -C ../freerouting show
main:docs/research/kicad_ipc_api_research.md`). None of this has been checked here, and
K11.1 checks it:
- **The plugin package:** an IPC plugin is a folder with a `plugin.json` (schema
  `https://go.kicad.org/api/schemas/v1`).
  - Its actions run out of process.
  - With `"runtime": {"type": "python"}`, they run in a virtual environment that KiCad
    builds from the plugin's `requirements.txt`.
  - With `"type": "exec"`, they run an executable.
  - KiCad passes `KICAD_API_SOCKET` and `KICAD_API_TOKEN` in the environment.
- **The API server is off by default** in KiCad 9 and 10. The note names two different
  places for the preference.
- **Calls run on KiCad's main loop.** They are synchronous, and they block while a
  KiCad modal dialog is open.
- **`kicad-cli api-server <board>`** (KiCad 10.99) serves one board without the GUI, for
  testing.
- **Edits made through a commit** (`begin_commit`, `create_items`, `push_commit`) are one
  undo step.
- **Live design rules are readable** (`BoardDesignRules`), including the copper-to-edge
  clearance that DSN doesn't carry.

**What carries over from today's plugin:**
- `options.py`, `routing.py`, `progress.py`, `summary.py`, `tools.py`, `version.py` and
  `rule_areas.py` import neither `pcbnew` nor `wx` (K00 rule), so they run in any Python 3.
- `plugin.py` is the SWIG entry point and holds the wx dialogs.
- `kicad_areas.py` imports nothing, but it reads the areas through the SWIG accessors of
  the `pcbnew` module it is given.

## Decisions made here

- **The SWIG plugin stays for KiCad 9 and 10.** The IPC plugin is for KiCad 11 and later.
  One package carries both, and each one registers only on the versions where it works.
  Whether the PCM allows both in one zip is checked in K11.1.
- **The transport stays DSN and SES:** `RunBoardJobExportSpecctraDsn`, the CLI, then
  `ImportSpecctraSession`.
  - K10's direct read is added in K11.6 if the API can save a copy of the open board.
  - Writing tracks through a commit is K10.6's decision, not this checklist's.
- **The plugin process never blocks on KiCad.** Every IPC call has a timeout. A
  timed-out call says that a KiCad dialog may be open, and offers retry or cancel.
- **The rule-area fix-up moves off SWIG.** It reads the area flags from the IPC board
  items, or, if the job exports areas by their flags, it is dropped. K11.1 decides which.

## File ownership

| Item | New files | May edit |
|---|---|---|
| K11.1 | `tools/kicad-ipc/probe.py` (no plugin code) | `docs/development/kicad-plugin.md` (K01.5 section) |
| K11.2–K11.4 | `kicad-plugin/ipc/plugin.json`, `kicad-plugin/ipc/requirements.txt`, `kicad-plugin/ipc/freerouting_rs_ipc/**`, `kicad-plugin/tests/test_ipc_*.py` | `kicad-plugin/freerouting_rs/{options,routing,summary}.py` (only to share code, not to change behaviour) |
| K11.3 | as decided by question 1: `crates/freerouting-gui/src/plugin_mode.rs` and its tests, or `kicad-plugin/ipc/freerouting_rs_ipc/ui_*.py` | — |
| K11.5 | — | `kicad-plugin/install.py`, `kicad-plugin/package.py`, `tools/install_plugin.py`, PCM metadata and their tests |
| K11.6–K11.7 | — | `docs/development/kicad-plugin.md`, `kicad-plugin/README.md`, `README.md` |

## Checklist

- [ ] **K11.1 — Probe a KiCad nightly.** Install a nightly with both messages (10.99 or
  later) beside the existing KiCads, never over them, and never touch a running KiCad
  (K00 rules). Using `kicad-cli api-server` on copies of three boards (`Issue558-dev-board`,
  `canary-pwr`, `rule-areas`):
  - **Export.** Send `RunBoardJobExportSpecctraDsn` raw through `kipy`'s client.
    `ses-diff`-style compare the DSN with 10.0.7's `ExportSpecctraDSN` of the same file,
    rule areas first.
  - **Import.** Import a Rust session with `ImportSpecctraSession`, and compare the
    result with `ImportSpecctraSES` on 10.0.7: track, arc and via counts, and locked items
    skipped.
  - **The facts to confirm or correct:**
    - the `plugin.json` fields;
    - the API preference and its default;
    - whether the PCM takes a SWIG plugin and an IPC plugin in one zip;
    - whether the API can save a copy of the open document (for K11.6);
    - whether the user's own Python UI can run in the plugin's environment
      (for question 1).
  - **Accept:**
    - the done note gives the nightly's version and the diffs;
    - every "unverified" above is marked verified or corrected in `kicad-plugin.md`'s
      K01.5 section.
- [ ] **K11.2 — The IPC plugin's core.**
  - It has `plugin.json` with one PCB-editor action, and a `requirements.txt` that pins
    `kicad-python`.
  - The entry point connects with the socket and token, finds the board and its folder,
    exports, runs the CLI with K02's options, and imports. It writes the same
    `freerouting-rs/` work folder and `route-options.json` as the SWIG plugin.
  - The IPC calls sit behind one small client interface, so tests can use a fake.
  - **Accept:**
    - unit tests with a fake client cover export, route, import, a refused connection
      and a timed-out call;
    - a headless run on the nightly routes `Issue558-dev-board` to 0 unrouted, and
      KiCad's DRC grades the imported board like the SWIG path on 10.0.7.
- [ ] **K11.3 — The dialog without wx.** It has options, live progress, Stop and keep
  result, Abort, and the result summary, as in K02, built the way question 1 decides.
  - **Accept:**
    - the same behaviour tests as K02's dialog, run against the new UI;
    - Stop keeps the best board and Abort changes nothing, shown on the nightly.
- [ ] **K11.4 — When the API is off or the board isn't ready.** These cases each give one
  clear message, never a traceback:
  - the API server disabled (it names the preference K11.1 found);
  - no board open;
  - a board never saved;
  - an older KiCad without the two messages, which points to the SWIG plugin.
  - **Accept:** unit tests with the fake client, one for each case.
- [ ] **K11.5 — Packaging and install.**
  - The PCM zip carries both plugins, or two zips if K11.1 found one zip can't.
  - `install.py` and `tools/install_plugin.py` install the IPC plugin for KiCad 11 and
    later, and the SWIG plugin for 9 and 10, chosen by the version folder.
  - **Accept:**
    - install tests for both version folders;
    - on 10.0.7 and on the nightly, exactly one freerouting-rs button appears.
- [ ] **K11.6 — K10's direct read on KiCad 11.** If K11.1 found the API can save a copy of
  the open board, `board_input = kicad_pcb` works through the IPC plugin too. If it
  can't, the option is hidden on 11 and the done note says why.
  - **Accept:**
    - with a copy, K10.1's in-memory-edit test passes through IPC;
    - otherwise, the dialog test shows the option hidden.
- [ ] **K11.7 — Documentation.** Covers:
  - in `kicad-plugin.md`: the IPC plugin, enabling the API, the nightly probe results;
  - which plugin each KiCad version gets;
  - the plugin README's install section;
  - and the top-level README's KiCad versions line.
  - **Accept:** each fact names its test or item.

## Open questions for Dan

1. **Where the dialog lives.** KiCad 11's Python has no wxPython.
   - *Recommendation:* the IPC plugin starts `freerouting-gui` in a plugin mode that
     shows the options, the progress, Stop and Abort, and the summary. It writes the
     session for the plugin to import.
     - The app already routes with live progress ("Route in GUI").
     - It needs no extra Python UI library in KiCad's environment.
     - It gives one dialog for every KiCad version later.
   - *Alternative:* a Python UI inside the plugin's environment, wxPython from pip or
     tkinter. Its availability on each OS is what K11.1 checks.
2. **When to start.**
   - *Recommendation:* run K11.1 now on a nightly. It is cheap and settles the open
     facts.
   - Start K11.2 onward when KiCad 11 has a release candidate or a release date, so the
     plugin is ready on the day 11.0 ships.
