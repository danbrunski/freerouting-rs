# KiCad plugin: handover notes

Source: [`kicad-plugin/`](../../kicad-plugin/README.md). It is a SWIG `pcbnew.ActionPlugin`
(KiCad 10.0 still ships the SWIG bindings) that adds a toolbar button to the PCB editor.

## How it works

- `freerouting_rs/plugin.py`: the dialog. **Route** exports the board with
  `pcbnew.ExportSpecctraDSN`, runs `freerouting -de board.dsn -do board.ses` and imports the
  SES with `pcbnew.ImportSpecctraSES`. **Open in viewer** exports and launches
  `freerouting-gui board.dsn [board.ses]`. **Import SES...** imports any session file. Files go
  in a `freerouting-rs/` folder next to the `.kicad_pcb`.
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
- Route only reports "routing is not implemented yet" (exit 2 from the CLI) until the
  autorouter is ported. When the CLI gains `-de/-do`, the plugin needs no change.

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
- Native viewer renders CM5_MINIMA_3 fitted, and ecc83-pp with reference designators.

## What's left

- **K01.6, Dan:** after the orchestrator reinstalls the plugin (`install.py --bin-dir target\release`,
  then Tools > External Plugins > Refresh Plugins, no restart), check in KiCad 10.0.5: the toolbar
  button, the dialog with both versions ("Router version: unknown" until C08 adds `--version`),
  Open in viewer, Import SES, and on `canary-pwr` that `freerouting-rs/plugin.log` names the
  fan inlet (`keepout -> place_keepout`). Record the date and what he saw under "Verified".
  Nothing in K01.2/K01.3 has run inside KiCad yet; only headless (pcbnew in a plain Python,
  fake pcbnew in tests).
- Dan to refresh plugins in KiCad and confirm the button and dialog work in the editor.
- Route becomes useful when the autorouter and CLI `-de/-do` land. Then exercise SES import
  on a real routed board, and consider passing options (passes, threads) through.
- Real silkscreen in the viewer would need the plugin to export extra data from the board
  alongside the DSN (offered to Dan, not requested).
