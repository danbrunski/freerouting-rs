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
- K11.2 onward also needs a KiCad 11 release candidate or release date (question 2).

## What exists and what this adds

**Verified on a nightly** (K11.1, master `80521a1b94`; the details are in
`docs/development/kicad-plugin.md`, "Nightly probe"):
- **`ImportSpecctraSession` imports like `ImportSpecctraSES`.** It lands the same tracks and
  vias, 0.0 nm apart, keeps locked items, and makes one undo step.
- **`RunBoardJobExportSpecctraDsn` exports the file on disk, not the open document.**
  `SaveCopyOfDocument` (with `include_project`) and `SaveDocumentToString` do include unsaved
  edits, and `GetDocumentModifiedState` reports them. `kicad-cli pcb export specctra` exists
  on master and gives the job's DSN.
- **Master's DSN differs from 10.0.7's.**
  - Copper zones become fixed polygon wires instead of planes, with no option to keep planes.
    On `canary-pwr` that grades 57 unconnected against 68.
  - Footprint-owned rule areas are placed wrong, with the footprint position subtracted
    twice. This is a regression.
  - A B.Cu via-keepout window is now on the right layer.
  - Board-level rule areas are exported as in 10.0.7, so K01.2's fix-up is still needed.
- **kipy 0.8.0 has neither the wrappers nor the generated classes** for the 11.0 messages.
  The probe uses NNG with classes generated from the nightly's `api/proto`.
- **Plugins.** An IPC plugin is found by its `plugin.json`, in the user's
  `<Documents>/KiCad/<ver>/plugins/` (not `scripting/plugins`) or the PCM's `3rdparty`
  folder. It gets `KICAD_API_SOCKET` and `KICAD_API_TOKEN`.
- **The API is off by default** ("Enable KiCad API" on the Preferences Plugins page). The
  PCM offers to turn it on when it installs a package version with `"runtime": "ipc"`.
- **A PCM package version has one runtime,** `swig` or `ipc`, with `kicad_version` and
  `kicad_version_max`.

**From the Java fork's research note** (`git -C ../freerouting show
main:docs/research/kicad_ipc_api_research.md`), not checked here:
- calls run on KiCad's main loop, synchronously, and block while a KiCad modal dialog is
  open;
- live design rules are readable (`BoardDesignRules`).

**What carries over from today's plugin:**
- `options.py`, `routing.py`, `progress.py`, `summary.py`, `tools.py`, `version.py` and
  `rule_areas.py` import neither `pcbnew` nor `wx` (K00 rule), so they run in any Python 3.
- `plugin.py` is the SWIG entry point and holds the wx dialogs.
- `kicad_areas.py` imports nothing, but it reads the areas through the SWIG accessors of
  the `pcbnew` module it is given.

## Decisions made here

- **The SWIG plugin stays for KiCad 9 and 10. The IPC plugin is for KiCad 11 and later.**
  They ship as two versions of the package, two zips, because a package version has one
  `runtime` (K11.1). Each version's `kicad_version` and `kicad_version_max` keep it to its
  KiCads.
- **The transport stays DSN and SES, starting from a saved copy.** The export job reads the
  file on disk (K11.1), so the plugin:
  1. calls `SaveCopyOfDocument` (with `include_project`) into the `freerouting-rs/` work
     folder;
  2. exports the copy with `kicad-cli pcb export specctra`, finding the binary with
     `GetKiCadBinaryPath`;
  3. routes it with the CLI;
  4. imports with `ImportSpecctraSession`.

  This never saves the user's board.
  - K10's direct read is K11.6: it routes the same copy without the export.
  - Writing tracks through a commit is K10.6's decision, not this checklist's.
- **The dialog lives in `freerouting-gui`** (Dan, 2026-10-17, question 1).
  - The IPC plugin starts the app in a plugin mode that shows the options, the progress,
    Stop and Abort, and the summary.
  - The app writes the session, and the plugin imports it.
  - No Python UI library is needed in KiCad's environment.
- **The plugin process never blocks on KiCad.** Every IPC call has a timeout. A
  timed-out call says that a KiCad dialog may be open, and offers retry or cancel.
- **The rule-area fix-up moves off SWIG.** Master exports board-level rule areas as
  10.0.7 does (K11.1), so K01.2's rewrite stays. It reads the area flags from the IPC
  board items instead of `pcbnew`.

## File ownership

| Item | New files | May edit |
|---|---|---|
| K11.1 | `tools/kicad-ipc/{probe.py,swig_side.py,run-probe.sh,Dockerfile,README.md}` (no plugin code) | `docs/development/kicad-plugin.md` (K01.5 section) |
| K11.2–K11.4 | `kicad-plugin/ipc/plugin.json`, `kicad-plugin/ipc/requirements.txt`, `kicad-plugin/ipc/freerouting_rs_ipc/**`, `kicad-plugin/tests/test_ipc_*.py` | `kicad-plugin/freerouting_rs/{options,routing,summary,rule_areas}.py` (only to share code, not to change behaviour) |
| K11.3 | `crates/freerouting-gui/src/plugin_mode.rs` and its tests | `crates/freerouting-gui/src/{lib,main}.rs` (the mode's flag) |
| K11.5 | — | `kicad-plugin/install.py`, `kicad-plugin/package.py`, `kicad-plugin/metadata.json`, `tools/install_plugin.py`, and their tests |
| K11.6–K11.7 | — | `docs/development/kicad-plugin.md`, `kicad-plugin/README.md`, `README.md` |

## Checklist

- [x] **K11.1 — Probe a KiCad nightly.** Install a nightly with both messages (10.99 or
  later) beside the existing KiCads, never over them, and never touch a running KiCad
  (K00 rules). Using `kicad-cli api-server` on copies of three boards (`Issue558-dev-board`,
  `canary-pwr`, `rule-areas`):
  - **Export.** Send `RunBoardJobExportSpecctraDsn` raw. Compare the DSN with 10.0.7's
    `ExportSpecctraDSN` of the same file, rule areas first.
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

  *Done 2026-10-17:*

  **Setup.**
  - The nightly is KiCad `10.99.0-unknown-80521a1b94~190~ubuntu24.04.1`
    (`kicad-dev-nightly` PPA build 202610172028, master `80521a1b94`, 2026-10-17), installed
    in Docker from `tools/kicad-ipc/Dockerfile`, run with `--memory=3g`.
    - Docker Hub's `kicad/kicad:nightly` was stale: `10.0.0-rc1`, February, no `api-server`.
  - The comparison is KiCad 10.0.7 from WSL's packages.
  - Neither Windows KiCad nor a KiCad GUI was started or touched (`pgrep -x kicad`,
    `pgrep -x pcbnew` empty before and after).
  - The run: `tools/kicad-ipc/run-probe.sh WORK target/release/freerouting <boards>`, with
    `PY` a kicad-python 0.8.0 venv (pynng, protobuf 5.29.6) and `PROTO_GEN` the classes
    generated by grpcio-tools 1.71.2 from the nightly's 24 `api/proto` files.
  - The sessions are Rust `a7a164a`, `-mp 3 --threads 2`, routed from 10.0.7's DSN.

  **Deviation from the item text.** The export went raw through `tools/kicad-ipc/probe.py`'s
  NNG client rather than kipy's client. kipy 0.8.0 has no generated classes for the 11.0
  messages, and loading newer ones beside kipy's would clash in protobuf's descriptor pool.

  **Import results.** `ImportSpecctraSession` against `ImportSpecctraSES` (`frame_check.py
  compare` on both saved boards):
  - `dev-board`: 440 tracks and 45 vias;
  - `rule-areas`: 47 items;
  - `canary-pwr`: 973 items;
  - all three 0.0 nm apart, with `skipped_count` 0 and an empty report.
  - **Locked items:** 10 tracks and a via locked, then the same session imported again.
    Both KiCads keep the 11 locked items and give the same boards (496, 58 and 984 items,
    0.0 nm apart).

  **Export results** (`probe.py dsn-summary`):
  - `dev-board`: identical except for the footprint-owned antenna keepout. It is at
    `(-136000 61050)-(-118000 67000)` on master and `(-9000 9800)-(9000 15750)` on 10.0.7.
    U2 sits at `(127000 -51250)`, so master has subtracted the footprint position twice and
    put the keepout off the board.
  - `rule-areas`: 92 lines differ.
    - The 6 planes become 6 `(wire (polygon ...) (type protect))`.
    - The 3 rotated footprints with rule areas get their own image each (5 images become
      8), with misplaced keepouts.
    - The B.Cu `via_keepout` window moves from F.Cu (10.0.7, wrong) to B.Cu.
    - Board-level keepouts are unchanged.
  - `canary-pwr`: no keepout or image differences. The 20 planes become 29 protect wires
    with 400 windows.
  - `kicad-cli-nightly pcb export specctra` gives the job's DSN, the file name aside.

  **Routing effect on `canary-pwr`** (`-mp 1 --threads 2`, imported with 10.0.7, graded with
  `kicad-cli pcb drc --severity-all --refill-zones`):
  - from 10.0.7's DSN: 68 unconnected, 31 `via_dangling`, 10 `track_width`; 93
    router-unrouted, pass 10.8 s;
  - from master's DSN: 57 unconnected, 15 `via_dangling`, 4 `track_width`; 82
    router-unrouted, pass 38.8 s.

  **Unsaved edits.**
  - A track added through `BeginCommit`, `CreateItems`, `EndCommit` turns
    `GetDocumentModifiedState` from `DMS_UNMODIFIED` to `DMS_MODIFIED`.
  - It is in `SaveDocumentToString` and in `SaveCopyOfDocument`; the file on disk keeps 0
    tracks.
  - It is missing from `RunBoardJobExportSpecctraDsn`'s DSN, which is byte-identical to the
    one before the edit apart from the name line. The handler passes `GetCurrentFileName()`
    to `KIWAY::ProcessJob` (`pcbnew/api/api_handler_pcb.cpp`).
  - `SaveCopyOfDocument` with `include_project` writes `.kicad_pcb`, `.kicad_pro` and
    `.kicad_prl`.

  **The facts.**
  - `plugin.json` fields: from `schemas/api.v1.schema.json`.
  - The API preference: `api.enable_server`, default `false`, labelled "Enable KiCad API"
    (`common_settings.cpp`, `panel_plugin_settings`). The PCM offers to enable it for an
    `ipc` package (`pcm.cpp`).
  - One zip with both plugins: **no.** `pcm.v2.schema.json` gives one `runtime` per package
    version, so it takes two versions.
  - Saving a copy: **yes.**
  - A Python UI in the plugin's environment: **not probed.** It was moot after Dan's answer
    to question 1.
  - Plugin folders and environment variables: from `common/paths.cpp` and
    `api_plugin_manager.cpp`.

  All of it is written in `kicad-plugin.md`'s K01.5 section, "Nightly probe". The K07
  section's "unverified" note on pours as wires is corrected there.
- [ ] **K11.2 — The IPC plugin's core.**
  - It has `plugin.json` with one PCB-editor action (`scopes: ["pcb"]`), and a
    `requirements.txt` that pins `kicad-python` and `pynng`. It ships the classes generated
    for the 11.0 messages until kipy wraps them.
  - The entry point:
    - connects with `KICAD_API_SOCKET` and `KICAD_API_TOKEN`, and finds the board and its
      folder;
    - saves a copy with `SaveCopyOfDocument` (`include_project`) into `freerouting-rs/`;
    - exports the copy with `kicad-cli pcb export specctra`;
    - applies K01.2's rule-area fix-up from the IPC zone items;
    - runs the CLI with K02's options;
    - imports with `ImportSpecctraSession`.

    It writes the same `freerouting-rs/` work folder and `route-options.json` as the SWIG
    plugin.
  - The IPC calls sit behind one small client interface, so tests can use a fake.
  - **Accept:**
    - unit tests with a fake client cover the copy, the export, the route, the import, a
      refused connection and a timed-out call;
    - a headless run on the nightly (`tools/kicad-ipc/run-probe.sh`'s setup) routes
      `Issue558-dev-board` to 0 unrouted, and KiCad's DRC grades the imported board like
      the SWIG path on 10.0.7;
    - an unsaved track before the run is in the routed result.
- [ ] **K11.3 — The dialog in `freerouting-gui`.** A plugin mode (a flag the IPC plugin
  passes) shows the options, live progress, Stop and keep result, Abort, and the result
  summary, as K02's wx dialogs do.
  - It reads and writes `route-options.json`.
  - It writes the session and a result file, then exits. The plugin imports only when the
    result says so.
  - **Accept:**
    - GUI harness tests (egui_kittest) for the options, Stop, Abort and the summary;
    - Stop keeps the best board and Abort changes nothing, shown on the nightly.
- [ ] **K11.4 — When the API is off or the board isn't ready.** These cases each give one
  clear message, never a traceback:
  - the API server disabled: it says to turn on "Enable KiCad API" on the Preferences
    Plugins page;
  - no board open;
  - a board never saved;
  - an older KiCad without the two messages, which points to the SWIG plugin.
  - **Accept:** unit tests with the fake client, one for each case.
- [ ] **K11.5 — Packaging and install.**
  - **Two package versions.**
    - The SWIG zip keeps `"runtime": "swig"` and gains a `kicad_version_max` below 11.
    - The IPC zip has `"runtime": "ipc"` and `kicad_version` 11.0, or 10.99 for nightly
      tests.
    - How the PCM compares those versions is unverified: check it on the nightly.
  - **Install.** `install.py` and `tools/install_plugin.py` put the IPC plugin in
    `<Documents>/KiCad/<ver>/plugins/` for KiCad 11 and later, and the SWIG plugin in
    `scripting/plugins` for 9 and 10, chosen by the version folder.
  - **Accept:**
    - install tests for both version folders;
    - on 10.0.7 and on the nightly, exactly one freerouting-rs button appears.
- [ ] **K11.6 — K10's direct read on KiCad 11.** K11.1 found the API can save a copy of the
  open board, unsaved edits included. So `board_input = kicad_pcb` routes the copy of
  K11.2 directly, without `kicad-cli pcb export specctra`.
  - This also avoids master's misplaced footprint keepouts (K11.1), which bears on K10's
    question 1 for KiCad 11.
  - **Accept:** K10.1's in-memory-edit test passes through IPC.
- [ ] **K11.8 — Correct master's misplaced footprint keepouts ourselves.** No upstream
  report (Dan, question 3). When the DSN comes from KiCad 11 or later (`kicad-cli` or
  `RunBoardJobExportSpecctraDsn`), the rule-area fix-up (`rule_areas.py`, K01.2) rewrites each
  footprint-owned keepout from the board's own rule areas (`kicad_areas`, read through the
  API), not from the exported coordinates. Detect the offset by comparing the two rather than
  by version alone, so a fixed KiCad needs no change here.
  - **Accept:**
    - on the nightly, `Issue558-dev-board`'s antenna keepout lands where 10.0.7 puts it
      (K11.1's repro);
    - the 10.0.7 DSN passes through unchanged;
    - a unit test with an offset keepout.
- [ ] **K11.7 — Documentation.** Covers:
  - in `kicad-plugin.md`: the IPC plugin, enabling the API, the nightly probe results;
  - which plugin each KiCad version gets;
  - the plugin README's install section;
  - and the top-level README's KiCad versions line.
  - **Accept:** each fact names its test or item.

## Open questions for Dan

1. **Where the dialog lives.** *Answered 2026-10-17:* in `freerouting-gui`, as recommended.
   Recorded under "Decisions made here".
2. **When to start.** *Answered 2026-10-17:* K11.1 now, which is done. K11.2 onward waits
   for a KiCad 11 release candidate or a release date.
3. **Report the footprint keepout regression to KiCad?**
   - Master's Specctra export puts footprint-owned rule areas off by the footprint's
     position (K11.1, with a two-line repro on `Issue558-dev-board`).
   - If 11.0 ships with it, every DSN from KiCad 11 has misplaced footprint keepouts.
   - Filing an issue on KiCad's GitLab is publishing, so it is Dan's call.
   - *Answered 2026-10-17 (Dan):* don't report upstream; fix what we can in our own repos.
     K11.8 corrects the keepouts in the plugin's DSN fix-up, and K11.6's direct read avoids
     the export altogether.
