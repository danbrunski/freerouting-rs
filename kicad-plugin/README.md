# KiCad plugin

An action plugin for the KiCad PCB editor (tested with KiCad 10.0 on Windows).
Its toolbar button opens a dialog that:

- **Route**: exports the board to Specctra DSN, runs `freerouting -de board.dsn -do board.ses
  --progress json --result-json board.result.json` with the options of the dialog, shows the
  router's progress live, and imports the session back into the board. **Stop and keep result**
  finishes the current step and imports the best board so far; **Abort** kills the router and
  imports nothing. A summary (connections routed and incomplete, vias, length, time) follows the
  import, with a warning when connections are left.
- **Route in GUI**: the same export, then `freerouting-gui -de board.dsn -do board.ses` with the
  options; the window routes after its countdown and writes the session when routing ends or the
  window closes, and the plugin imports it. **Terminate** kills the window and imports nothing.
- **Options** (maximum passes, threads, optimizer on/off and its threshold, net classes to ignore,
  time limit, seed, minimum trace width) are saved per board in
  `freerouting-rs/route-options.json`. The minimum trace width defaults to the Board Setup
  minimum track width, which the DSN does not carry.
- **DSN fix-ups and flow** (K09, `dsn_fixups.py`, ported from canary's `dsn_export.py`): per
  class routed widths, outer pours of listed classes kept as fixed copper (or kept clear by the
  router), outer pours of listed nets dropped, the board-edge and pad clearances as keepouts (on by
  default: they are board rules), inner plane layers opened to routing, an add-only run that keeps
  the existing tracks, the start rip-up cost, Q10's completion sweep, and a remembered pass limit.
  The **Canary flow** button fills them with the values canary's graded flow uses. After the import
  the plugin sets each routed via's drill from the session (KiCad's import takes the net class's).
- **Open in viewer**: exports the board and opens it (plus the last `.ses`, if any) in
  `freerouting-gui`.
- **Import SES...**: imports any Specctra session file into the board.

The DSN and SES files go in a `freerouting-rs` folder next to the `.kicad_pcb`. The board
must be saved once so that folder has somewhere to live.

## Install

**From the plugin manager (a release).** KiCad > Plugin and Content Manager > Install from
File... with `freerouting-rs-<version>-pcm.zip`. The zip carries the plugin and the executables
for Windows (x86_64), macOS (universal) and Linux (x86_64); the plugin picks the ones for your
system (`plugins/bin/<os>-<arch>/`) after a `bin_dir` of `settings.json`. On another system, build
the executables as below and set `bin_dir`. Maintainers: `python3 kicad-plugin/package.py --bin
<dir> --out <dir>` builds the zip, `update_metadata.py` and `validate_pcm.py` finish the
release metadata (docs/development/kicad-plugin.md, "PCM package").

**From source, in one step.** From the repository root, with any Python 3.8+ (KiCad's own
works), on Windows, macOS, Linux or WSL:

```
python tools/install_plugin.py
```

It checks the build environment and offers to fix what is missing, asking first each time:
- Rust, and a linker (on Windows, MSVC or windows-gnu with WinLibs);
- on Linux, the viewer's libraries;
- KiCad's user folder.

Then it builds both executables and runs `install.py` below with `--bin-dir` pointing at them.
- `--check` only reports.
- `--yes` applies the fixes without asking.
- `--windows-kicad`, run from WSL, installs into the Windows KiCad and runs the Linux build
  through WSL.

The top-level [README](../README.md#install-the-kicad-plugin) lists every option.

**From source, step by step.** Build the executables, then run the installer:

```
cargo build --release -p freerouting-cli -p freerouting-gui
python kicad-plugin/install.py --bin-dir target/release
```

`install.py` copies `freerouting_rs/` into the newest KiCad version's user `scripting/plugins` folder
(`--kicad-version` or `--plugins-dir` to choose) and records where the executables are in
`settings.json` there. Without `--bin-dir` the plugin looks in `FREEROUTING_RS_BIN`, the PATH
and `~/.cargo/bin`.

On Windows with the `x86_64-pc-windows-gnu` toolchain, the build needs a full mingw-w64
(for example WinLibs, `winget install BrechtSanders.WinLibs.POSIX.MSVCRT`) on the PATH: the
`dlltool` that ships with Rust calls an assembler it doesn't bundle. The MSVC toolchain needs
nothing extra beyond the Visual Studio build tools.

The executables can instead be a Linux build inside WSL, shown through WSLg:

```
python kicad-plugin/install.py --wsl-bin-dir /home/me/freerouting-rs/target/release
```

In the PCB editor, Tools > External Plugins > Refresh Plugins loads the plugin (or a
reinstalled one) without restarting KiCad.

## Tests

```
python3 -m pytest kicad-plugin/tests
```

No KiCad needed (plain Python 3 and pytest; `pcbnew` and `wx` are never imported).
