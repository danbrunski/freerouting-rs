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
- `install.py`: copies `freerouting_rs/` into KiCad's user `scripting/plugins` folder and
  writes `settings.json` there.
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

- Dan to refresh plugins in KiCad and confirm the button and dialog work in the editor.
- Route becomes useful when the autorouter and CLI `-de/-do` land. Then exercise SES import
  on a real routed board, and consider passing options (passes, threads) through.
- Real silkscreen in the viewer would need the plugin to export extra data from the board
  alongside the DSN (offered to Dan, not requested).
