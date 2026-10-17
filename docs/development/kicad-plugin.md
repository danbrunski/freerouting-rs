# KiCad plugin: handover notes

Source: [`kicad-plugin/`](../../kicad-plugin/README.md). It is a SWIG `pcbnew.ActionPlugin`
(KiCad 10.0 still ships the SWIG bindings) that adds a toolbar button to the PCB editor.

## How it works

- `freerouting_rs/plugin.py`: the dialog. **Route** saves the options, exports the board with
  `pcbnew.ExportSpecctraDSN`, runs `freerouting -de board.dsn -do board.ses --progress json
  --result-json board.result.json <options>` and imports the SES with `pcbnew.ImportSpecctraSES`
  (K02). A progress dialog shows the phase, pass and routed/failed/incomplete counts live, the
  router's log lines, **Stop and keep result** and **Abort**; a summary follows the import.
  **Route in GUI** runs `freerouting-gui -de board.dsn -do board.ses <options>` and imports the
  session the window writes. **Open in viewer** exports and launches `freerouting-gui board.dsn
  [board.ses]`. **Import SES...** imports any session file. Files go in a `freerouting-rs/` folder
  next to the `.kicad_pcb`.
- K02 modules (all pure Python except `plugin.py`): `options.py` (the route options, per board in
  `freerouting-rs/route-options.json`, `"schema": 1`, defensive restore; `to_args()` builds the
  flags in a fixed order and always passes `--router.min_trace_width_um` from Board Setup's
  minimum track width, D8), `progress.py` (`parse_line` for C08's JSON lines, `ProgressState`,
  `pump_lines` = the reader thread's body), `summary.py` (the text after the import, from the
  `--result-json` manifest), `routing.py` (`RouteRun`: process, reader thread, queue, `stop`,
  `abort`; `GuiRun`: the front-end GUI process, `should_import`).
- **Stop keeps, Abort discards.** Stop writes `stop\n` to the router's stdin (the router ends the
  step it is in, writes the best board, exits 0). Abort kills the process; the CLI writes outputs
  to a temporary file and renames it, so an aborted run leaves no `.ses`. Only C08's JSON lines
  are parsed; any other line is shown in the log pane as it is. pcbnew and wx are called on the
  UI thread only: the reader thread fills a `queue.Queue`, a `wx.Timer` in the dialog drains it.
- `freerouting_rs/tools.py`: finds the executables (no pcbnew/wx imports). Order: `bin_dir`
  in `settings.json`, `FREEROUTING_RS_BIN`, PATH, `~/.cargo/bin`, then on Windows
  `wsl_bin_dir` (a Linux build run through `wsl.exe`, paths translated to `/mnt/c/...`).
  Routing runs in a thread behind a cancellable progress dialog.
- `freerouting_rs/rule_areas.py` + `kicad_areas.py` (K01.2): after `ExportSpecctraDSN`, the
  plugin reads the board's rule areas with pcbnew (UI thread) and rewrites the exported DSN
  text. KiCad picks the keepout type from the tracks/vias flags only, so a footprints-only
  area (the `canary-pwr` fan inlet) becomes a full `keepout`. The rewrite turns it into
  `place_keepout`, drops pads/pour-only areas, and leaves the rest byte for byte (mapping table
  in the `rule_areas.py` docstring). On by default, switch in the dialog (saved per board in
  `freerouting-rs/plugin.json`), one log line per area in `freerouting-rs/plugin.log`.
  Footprint-owned areas (written into the `(image ...)`) are only listed in a warning.
- `freerouting_rs/version.py` (K01.3): `__version__` (equals the workspace version) and the
  check against `freerouting --version` (`tools.query_version`, 10 s timeout). A major.minor
  mismatch disables Route; an unparsable answer is "unknown" (allowed, with a warning). The
  CLI has no `--version` until C08, so today every router shows "unknown".
- `install.py`: copies `freerouting_rs/` into KiCad's user `scripting/plugins` folder and
  writes `settings.json` there. It first moves any other copy of this plugin (PCM install in
  `3rdparty/plugins`, or a folder with our identifier) to `<KiCad user dir>/disabled_pcm_plugins/<version>/`
  (`--keep-pcm` refuses instead). The Java plugin (`app.freerouting.kicad-plugin`) is never
  touched. `OUR_IDENTIFIERS` is a placeholder until K03 fixes the PCM identifier.
- Route in GUI needs the GUI's front-end mode (G08.4): the window counts down 5 s, routes, and
  writes `-do` when routing ends or the window closes. The router flags (`--max-passes`,
  `--threads`, `--router.*`, ...) are the CLI's (K02.5, `settings::cli_flags`); `--seed` is not
  passed to the GUI.

## Dan's machine (Windows 11, KiCad 10.0)

- Installed at `C:\Users\thebo\Documents\KiCad\10.0\scripting\plugins\freerouting_rs`, with
  `settings.json` = `bin_dir: C:\Users\thebo\Documents\code\freerouting-rs\target\release`
  (plus a WSL fallback, `wsl_bin_dir: /home/thebo/.cache/freerouting-rs-target/release`, only
  used if the Windows exes are missing).
- Reinstall after changing the plugin source, from Windows:
  `& "C:\Program Files\KiCad\10.0\bin\python.exe" kicad-plugin\install.py --bin-dir target\release --wsl-bin-dir /home/thebo/.cache/freerouting-rs-target/release`
  then Tools > External Plugins > Refresh Plugins in the PCB editor (no restart).
- Dan had not yet confirmed the button appears in KiCad when these notes were written.

## Native Windows toolchain

- rustup with the `x86_64-pc-windows-gnu` host toolchain (no Visual Studio needed), in
  `%USERPROFILE%\.cargo` / `.rustup`.
- WinLibs mingw-w64 (`winget install BrechtSanders.WinLibs.POSIX.MSVCRT`, user scope). Needed
  because `windows-sys`/`parking_lot_core` use raw-dylib, and the `dlltool.exe` bundled with
  Rust's gnu toolchain calls an assembler (`as`) it doesn't ship. The mingw `bin` must be on
  PATH for the build:
  `C:\Users\thebo\AppData\Local\Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.MSVCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin`
- The resulting exes only import system DLLs.

**KiCad launches the Windows exes in `target\release` of the Windows checkout.** A build in
WSL (Linux binaries, or a different checkout) does not update them. After any change to
`freerouting-gui` or `freerouting-cli`, rebuild natively. From WSL, for example:

```
cmd.exe /c "cd /d C:\Users\thebo\Documents\code\freerouting-rs && git pull && set PATH=C:\Users\thebo\AppData\Local\Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.MSVCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin;%USERPROFILE%\.cargo\bin;%PATH% && cargo build --release -p freerouting-gui -p freerouting-cli"
```

The build fails if the viewer is open (Windows locks the running exe), so close it first.

## Tests

`python3 -m pytest kicad-plugin/tests` (plain Python 3 + pytest, no pcbnew or wx; pytest is a
dev-only dependency). `conftest.py` puts `freerouting_rs/` on `sys.path`, so `tools`,
`rule_areas`, `version` and `kicad_areas` import as top-level modules and the package
`__init__` (which registers the plugin) never runs. Fake routers live in `tests/fake_bin/`.
`test_threads.py` fails if a `threading.Thread` target reaches `pcbnew` or `wx` (except
`wx.CallAfter`). `test_rule_areas.py::test_canary_pwr_fresh_kicad_export` runs KiCad's Python
(`FREEROUTING_RS_KICAD_PYTHON`, or a system `python3` that imports `pcbnew`) on a copy of
`../canary/hardware/canary-pwr-rev1.0/canary-pwr.kicad_pcb` (`CANARY_PCB` overrides) and skips
when either is missing. GitHub CI is disabled (Dan, 2026-10-15); run the suite by hand.

K02 tests: `test_options.py` (golden argument lists, save/load round trip, damaged files),
`test_progress.py` and `test_summary.py` (replay `tests/data/progress-{j2,bm01}.jsonl` and
`result-{j2,bm01}.json`, recorded from the real CLI with `--progress json --result-json`),
`test_routing.py` (`RouteRun` and `GuiRun` against `fake_bin/fake_freerouting.py` modes `route`,
`route-slow`, `route-fail`, `route-noise` and `fake_bin/fake_gui.py`), `test_route_dialog.py` and
`test_gui_mode.py` (plugin.py's Route and Route in GUI flows with the stub `wx`/`pcbnew` of
`tests/stub_ui.py`: real dialog code, modal loops driven by their timers), and
`test_cli_integration.py` (the real CLI: all options, Stop, Abort, summary against `-drc`).
The last one uses `FREEROUTING_RS_BIN` (a directory), else `target/release` or `target/debug`, and
skips with the reason when no `freerouting` with `--capabilities` is found.

## SWIG, IPC API and KiCad versions (K01.5)

Checked 2026-10-15 against KiCad's GitLab (`gitlab.com/kicad/code/kicad`).

- **KiCad master has removed SWIG, wxPython and the Python integration.** Verified: commit
  [`65a442b1`](https://gitlab.com/kicad/code/kicad/-/commit/65a442b1d2bf153be7979de8926ab71fd095f4bd),
  "REMOVED: SWIG, wxPython, and Python integration", 2026-03-22, "The IPC API will be used going
  forward" (fixes work item 9078). The commit is on `master` only (refs query), so the
  KiCad 10.0 series keeps SWIG (10.0.5 on Dan's machine and 10.0.7 here both import `pcbnew`).
  Verified: the plugin works through 10.x; it stops loading in the release cut from master
  (presumably 11.0; the version number itself is unverified).
- **IPC API, Specctra.** Verified in the protobuf files on `master`: `RunBoardJobExportSpecctraDsn`
  (`board_jobs.proto`, "Since: 11.0", returns `RunJobResponse`) exports DSN, and
  `ImportSpecctraSession` (`board_commands.proto`, "Since: 11.0"; `session_path`, returns
  `skipped_count` and a report) imports a `.ses`: it repositions footprints, deletes unlocked
  tracks and vias, adds the routed ones, and skips locked ones. Verified: neither message
  exists in the `10.0` branch protos. Verified: `kipy` (kicad-python `main`, PyPI 0.8.0) has no
  wrapper for either (`board.py` and `board_jobs.py` do not mention Specctra), so a plugin would
  send the raw protobuf command. Unverified: whether the master implementation is complete and
  how rule areas are exported by the job (not probed). Where the IPC calls do not exist (KiCad
  10), SWIG `ExportSpecctraDSN` / `ImportSpecctraSES` stay. If KiCad ever drops both, K08's
  direct `.kicad_pcb` writer and K04-K07's reader replace them.
- **Copper-pour flag rename.** Verified in `pcbnew/zone.h`: the `9.0` branch has
  `GetDoNotAllowCopperPour`, the `10.0` branch and `master` have `GetDoNotAllowZoneFills`.
  Verified by probe on 10.0.7: `GetDoNotAllowZoneFills` exists, `GetDoNotAllowCopperPour` does
  not; the track, via, pad and footprint accessors (`GetDoNotAllowTracks`, `...Vias`, `...Pads`,
  `...Footprints`) exist unchanged. `kicad_areas._pour_flag` tries both names (`getattr`), so
  KiCad 9 needs no other fallback for this flag (not run on 9).
- **Probe result (KiCad 10.0.7, WSL, `pcbnew.LoadBoard` on a copy of `canary-pwr.kicad_pcb`):**
  one rule area, "Fan inlet keep-out (ASSUMED position - verify on the Pi)", layer B.Cu,
  tracks False, vias False, pads True, footprints True, pour False, outline
  (136.5, 53)-(162.5, 79) mm, not footprint-owned. Its DSN export is
  `(keepout "" (polygon B.Cu 0 136500 -53000 162500 -53000 162500 -79000 136500 -79000 ...))`.
  Not probed on Windows KiCad 10.0.5.

## Gotchas

- **Never close, restart or kill KiCad, and don't touch the Java Freerouting plugin**
  (`3rdparty\plugins\app_freerouting_kicad-plugin`) or its Java processes; Dan runs it
  alongside this one.
- **Don't import the plugin package in a standalone KiCad Python.** `register()` outside the
  editor raises a wxWidgets "PgmOrNull() failed" debug dialog on Dan's desktop. Test
  `tools.py` and `export_dsn` directly instead (`pcbnew.LoadBoard` + `ExportSpecctraDSN`
  works headless).
- **Dan's display is at 125% (120 DPI).** Screen captures of the viewer from a DPI-unaware
  process (plain PowerShell) get only the top-left part of the window and look wrongly
  fitted. Call `SetProcessDPIAware()` before `GetWindowRect`/`PrintWindow`.
- KiCad's DSN export carries no silkscreen. The viewer's reference designators come from
  the DSN placement data.
- `git` on Windows has no identity configured; commits use
  `git -c user.name=Claude -c user.email=noreply@anthropic.com`.

## Verified

- Headless (KiCad's Python): DSN export, router call, viewer launch, both native and via WSL.
- Five KiCad 10 demo boards (ecc83-pp, interf_u, complex_hierarchy, kit-dev-coldfire,
  CM5_MINIMA_3) exported to DSN and loaded by `build_board` without parse errors.
- 2026-10-16 (K02, headless, WSL KiCad 10.0.7): the plugin's Route path against the real CLI (all
  options, Stop, Abort, summary: `test_cli_integration.py`), non-ASCII net and reference names
  through export, route and import (K02.6), and the five demos graded against Java (K02.7). Not
  yet run inside KiCad (K02.8).
- Native viewer renders CM5_MINIMA_3 fitted, and ecc83-pp with reference designators.

## What's left

- **K01.6, Dan:** after the orchestrator reinstalls the plugin (`install.py --bin-dir target\release`,
  then Tools > External Plugins > Refresh Plugins, no restart), check in KiCad 10.0.5: the toolbar
  button, the dialog with both versions, Open in viewer, Import SES, and on `canary-pwr` that
  `freerouting-rs/plugin.log` names the fan inlet (`keepout -> place_keepout`). Record the date and
  what he saw under "Verified". Nothing in K01.2/K01.3 has run inside KiCad yet; only headless
  (pcbnew in a plain Python, fake pcbnew in tests).
- **K02.8, Dan (Windows, inside KiCad):** see "K02.8: what Dan checks" below.
- Real silkscreen in the viewer would need the plugin to export extra data from the board
  alongside the DSN (offered to Dan, not requested).

## K02.6: non-ASCII names round trip

`tools/kicad-oracle/roundtrip_names.py --router target/release/freerouting` (run with a python3 that
imports `pcbnew`; KiCad 10.0.7 here). For each case it strips the tracks of a copy, exports the DSN
with `ExportSpecctraDSN`, routes it with the CLI (`-de x.dsn -do x.ses`, nothing sanitised),
imports the SES with `ImportSpecctraSES` into the stripped copy and compares: every track and via of
the routed board has a net name that is a net name of the original, the non-ASCII names are in the
DSN as they are, every imported non-ASCII net is in the SES as it is, no footprint reference
changed, and `kicad-cli pcb drc` reports no `shorting_items`.

KiCad 10's `interf_u` demo has plain ASCII names (the older board behind fixture
`Issue093-interf_u` has Chinese text, not Omega), so both cases rename: `interf_u` gets Omega on
8 nets and on 11 references (`R`/`C` become `RΩ`/`CΩ`), `ecc83-pp` gets mu and Phi on 3
nets and 6 references.

Result, 2026-10-16, `--max-passes 3`, release CLI at this tree, KiCad 10.0.7 (WSL):

| case | tracks and vias imported | non-ASCII nets routed | name differences | `shorting_items` |
|---|---|---|---|---|
| `interf_u` (Omega) | 789 | 8 (`/8MH-OUTΩ`, `/BIT0Ω`, ...) | 0 | 0 |
| `ecc83-pp` (mu, Phi) | 33 | 2 (`Net-(P3-P1)Φ`, `Net-(U1A-G)µ`) | 0 | 0 |

`GNDµ` has no wires (a zone net), so it is in the DSN and not in the SES. Not run in Windows
KiCad 10.0.5; `pgrep -x kicad; pgrep -x pcbnew` printed nothing before and after.

## K02.7: graded runs on real boards

`tools/bench/k02-graded-runs.sh tools/bench/sets/kicad-demos.txt OUT target/release/freerouting
~/code/freerouting/logs/jars/freerouting-a773a70ec.jar` (set file: the five demos and `canary-pwr`,
out of the repo, skipped when missing). Each engine gets the same `grade.sh` export (tracks and
vias stripped, DSN exported by KiCad 10.0.7, footprint-only rule areas removed, which is what the
K01.2 rewrite achieves) and the same `--router.min_trace_width_um` = the board's Board Setup
minimum track width (D8; the DSN does not carry it). Rust runs the plugin's default options and
nothing else; Java runs `a773a70ec` with `--gui.enabled=false --router.optimizer.enabled=false`
(its optimizer still depends on thread scheduling, J01). Both end by their own stop rule (no pass
limit). Graded by KiCad's own DRC (`grade.sh`, items the unrouted input does not already have),
vias inside pads counted by `tools/kicad-oracle/via_in_pad.py`. 2026-10-16, WSL, KiCad 10.0.7,
release CLI of this tree (with other sessions' uncommitted engine work in it), `pgrep -x kicad;
pgrep -x pcbnew` empty before and after:

| board | engine | DRC violations caused by the router | unconnected / input | vias | tracks | length mm | vias in pads | below board min. | engine s |
|---|---|---|---|---|---|---|---|---|---|
| ecc83-pp | rust | none | 0 / 14 | 0 | 33 | 161.8 | 0 | 0 | 0.02 |
| ecc83-pp | java | none | 0 / 14 | 0 | 33 | 161.8 | 0 | 0 | 3.2 |
| interf_u | rust | starved_thermal 5 | 10 / 164 | 43 | 1021 | 4478.5 | 0 | 0 | 15.4 |
| interf_u | java | starved_thermal 4 | 11 / 164 | 45 | 1029 | 4470.2 | 0 | 0 | 14.7 |
| complex_hierarchy | rust | none | 80 / 87 | 1 | 28 | 112.6 | 0 | 0 | 0.07 |
| complex_hierarchy | java | none | 80 / 87 | 1 | 28 | 112.6 | 0 | 0 | 3.3 |
| kit-dev-coldfire-xilinx_5213 | rust | clearance 1, shorting_items 1, via_dangling 48 | 87 / 478 | 240 | 1650 | 5072.6 | 0 | 0 | 446 |
| kit-dev-coldfire-xilinx_5213 | java | shorting_items 1, track_dangling 1, track_width 190, via_dangling 43 | 85 / 478 | 236 | 1748 | 4876.5 | 0 | 190 | 591 |
| CM5_MINIMA_3 | rust | clearance 1, copper_edge_clearance 24, hole_clearance 2, isolated_copper 10, track_dangling 1 | 22 / 208 | 187 | 1319 | 2646.0 | 0 | 0 | 99 |
| CM5_MINIMA_3 | java | the same | 22 / 208 | 187 | 1319 | 2646.0 | 0 | 0 | 252 |
| canary-pwr | rust | copper_edge_clearance 2, starved_thermal 2, via_dangling 36 | 70 / 174 | 124 | 734 | 1056.9 | 0 | 0 | 199 |
| canary-pwr | java | copper_edge_clearance 2, starved_thermal 2, track_dangling 1, track_width 2, via_dangling 32 | 71 / 174 | 119 | 787 | 1049.2 | 0 | 2 | 213 |

Differences explained:

- `CM5_MINIMA_3`: the two sessions have the same 187 wires and vias, line for line (the CM5 board
  has no Board Setup minimum track width, so 0 was passed).
- `kit-dev`: Rust's one `clearance` item is a via 0.04 mm from the copper text `MOD0` on
  `Top_layer`. KiCad's DSN export has no copper text, so neither router can see it; Java's route
  happened to miss it. Both engines have the same `shorting_items` (a track through the copper
  text `JTAG_EN`). Java has 190 tracks narrower than the board minimum (0.198 mm), Rust none (D8).
  `via_dangling` 48 against 43 and 87 against 85 unconnected are the two routes ending in
  different places after 18 passes each.
- `interf_u`: one more `starved_thermal` (a pour effect, not a clearance) in Rust.
- Vias in pads: 0 in every graded run.

- `canary-pwr`: the same two `copper_edge_clearance` and two `starved_thermal` items in both
  engines; Java has 2 tracks under the board minimum (0.15 mm) and one `track_dangling`, Rust none;
  `via_dangling` 36 against 32 and 70 against 71 unconnected are the two routes ending in
  different places. The Rust row was graded at `813663c` (2026-10-17, `ENGINES=rust`), after the
  fix of a panic in `normalize_pass` (`8fb7e21`: a trace removed by a drill split was read after
  its removal; Java carries on with the stale object, which the fix matches). This export
  (KiCad 10.0.7, pours as 20 planes, no wiring) is a different board from
  `fixtures/canary-pwr/canary-pwr-9511d9a.dsn`, so its numbers do not compare with the canary
  grades in `fixtures/canary-pwr/README.md`.

## K02.8: what Dan checks (Windows, inside KiCad)

Nothing of K02 has run inside KiCad yet; the dialogs are tested against a stub `wx`/`pcbnew`
(`tests/stub_ui.py`) and the router against the real CLI. Needs a native Windows rebuild first
(the orchestrator does that, "Native Windows toolchain" above: mingw `bin` on PATH, viewer closed,
`cargo build --release -p freerouting-gui -p freerouting-cli`) and a reinstall
(`install.py --bin-dir target\release`, then Tools > External Plugins > Refresh Plugins).

In KiCad 10.0.5 open `canary-pwr` (save it once if KiCad asks), start the plugin, and check:

1. **Options.** Change Maximum passes to 5 and the time limit to `10m`, press Route, close the
   dialog after it ends and open it again: the values are still there.
   `freerouting-rs\route-options.json` exists next to the board. The "Minimum trace width" box
   shows the Board Setup value as its hint.
2. **Route with progress.** The progress dialog shows `Loading`, `Fanout`, `Routing, pass N: x
   routed, y failed, z incomplete (t s)` changing about every second, and the router's log lines.
   When it ends the board has the new tracks and a message shows routed/incomplete connections,
   vias, length and time, with a WARNING line when connections are left.
3. **Stop.** Route again; press "Stop and keep result" after the first pass appears. The dialog
   ends within a pass or two, the board gets the tracks so far and the message starts "Stopped
   early".
4. **Abort.** Route again; press Abort (or the window's close button) after the first pass. The
   board is unchanged, `freerouting-rs\canary-pwr.ses` does not exist.
5. **Route in GUI.** Press "Route in GUI". The freerouting-gui window opens with the board, counts
   down 5 s and routes; close it after routing (or let it finish). The plugin imports the session.
   Run it once more and press Terminate in the wait dialog: the window closes, nothing is imported.
6. **A demo board.** Repeat 2 on one demo (for example `ecc83-pp`, which routes in about a second).

Record the date, KiCad version, what worked, the routed/incomplete counts and times for
`canary-pwr` and the demo, and anything odd (a frozen dialog, a Stop that did not return, a window
that stayed behind) under "Verified" above. The item stays partial until then.

## Direct `.kicad_pcb` input (K07)

`freerouting -de board.kicad_pcb`, the GUI's File > Open and `freerouting::io::read_design` read a KiCad
board without `ExportSpecctraDSN`, so a plugin can hand the CLI the board file. KiCad 11 (K00 question 5;
SWIG removal in KiCad master is unverified here) drops `ExportSpecctraDSN`; a master that exports pours as
`(wire (polygon ...))` instead of planes (GUI report 6.2, unverified) would not matter either. Bringing the
result back still needs `ImportSpecctraSES` or the optional writer (K08).

**What is read** (`crates/freerouting-kicad`, tests in its `tests/` and in `crates/freerouting/tests/kicad_*.rs`):
the board file (KiCad 6 to 10; older files are refused with a one-line message, `a_kicad_5_file_exits_1`), the
`<stem>.kicad_pro` next to it (net classes; KiCad's defaults when missing, `a_missing_project_file_is_not_an_error`)
and the `<stem>.kicad_dru` (K07.5, below). Layers, nets, net classes, the outline, footprints and pads, rule
areas, copper zones as planes, tracks and arcs, vias.

**Rule areas map by their flags** (K06.2, `rule_areas_map_by_flags`), where KiCad's export makes a full
`keepout` of any area that bans neither tracks nor vias:

| Area bans | Emitted |
|---|---|
| tracks and vias | `keepout` |
| vias only | `via_keepout` |
| tracks only | `wire_keepout`, built as a keepout that also blocks vias (as Java does, K00 question 1), one warning per area |
| footprints (any other flags) | the above plus a `place_keepout` |
| only pads, only copper pour, or nothing | nothing; a warning says so |

`canary-pwr`'s fan inlet bans pads and footprints, not tracks: the export makes it a full keepout (5 traces and
15 vias of the board then violate it), the reader a `place_keepout` (0 violations; `canary-pwr` row of the
corpus run, `canary_pwr_fan_inlet`).

**Expected differences from KiCad's export** (`crates/freerouting/tests/kicad_parity.rs`, `EXPECTED_DIFFERENCES`;
the test fails on any other difference, and on a listed one that no longer occurs):

- rule-area kinds as above (K06.2): the reader has `place_keepout`s and fewer full keepouts;
- track arcs are followed as polylines, the export writes one chord (K06.5);
- copper zones that footprints own stay planes, the export drops them (K06.4, K00 question 6);
- the order of the non-default net classes (project order against `std::unordered_map` order) and of the
  outline's holes (`tastexx-pcb`);
- coordinates keep KiCad's nanometres where the export prints `%.6g` (K00 question 2): areas, lengths and
  extents differ by under 2e-4 of themselves, and on routed boards the export's rounding leaves pads and
  tracks a micrometre apart, so its DRC sees more incomplete connections and dangling tracks than the
  reader's (`export_rounding_explains_connectivity`: round the reader's wires like the export and the counts
  are equal on `bm1`, `bm4`, `bm10`).

**`.kicad_dru`** (`dru.rs`): every rule is read and reported (`KicadInput::report_lines`, printed by the CLI and
shown in the GUI's warnings window). `ReadOptions::apply_design_rules` (default off, K00 question 7) applies
the class-pair clearance (`class_class`) and class track width (`opt`) rules; a condition term the module
cannot evaluate (`insideCourtyard`, `intersectsArea`, `inDiffPair`, item types) makes the rule UNSUPPORTED and
is named in both modes. `canary-pwr`'s ten `insideCourtyard` rules are all UNSUPPORTED
(`canary_pwr_rules_are_unsupported_by_name`). Board minimums of the `.kicad_pro` (track 0.15, annular 0.15, edge
clearance 0.3 for `canary-pwr`) and pad-local clearances are not carried into the DSN model yet.

**Regenerating the reference exports and running the corpus.** `tools/kicad-reader/export.sh <board> <out.dsn>`
exports a board with KiCad 10.0.7 (WSL; `--windows` for 10.0.5), `fetch-corpus.sh target/kicad-corpus` and
`export-corpus.sh` make the out-of-repo corpus (K04.7/K04.8, `fixtures/kicad/README.md`). Then
`cargo test --release -p freerouting --test kicad_parity -- --ignored --nocapture corpus_summary` prints one
row per board (`FR_KICAD_ONLY=stem,stem` limits it) and `corpus_timing` the read times
(`FR_KICAD_TIME`); `tools/kicad-reader/frame-check.sh` imports two sessions into KiCad and compares the tracks
(K07.4).

**Coordinate frame** (K00 question 2, K07.4; `crates/freerouting/tests/kicad_frame.rs`): the export declares
`(resolution um 10)` and writes micrometres; the session import computes `nm = KiROUND(1000 * v / resolution)`.
Java's session for `Issue558-dev-board` (`dev-board.kicad10.java.ses`, 231 wires, 35 vias) lands at identical
board coordinates on the board built from the reader and on the one built from the export, with no
incomplete connection left on either; KiCad 10.0.7 imports it and the session the Rust writer produces from the
reader's board to the same 494 tracks, vias and arcs, largest endpoint distance 0.0 nm. Loss: KiCad's
nanometres round to 0.1 um board units, at most 50 nm per coordinate; on this board every pad centre is on
the grid and the largest distance of a track end to its pad centre is 0 nm (171 ends). A finer resolution is
not worth a change until a board shows a connection lost to it.
