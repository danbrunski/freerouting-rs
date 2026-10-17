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
- **Open in viewer**: exports the board and opens it (plus the last `.ses`, if any) in
  `freerouting-gui`.
- **Import SES...**: imports any Specctra session file into the board.

The DSN and SES files go in a `freerouting-rs` folder next to the `.kicad_pcb`. The board
must be saved once so that folder has somewhere to live.

## Install

Build the executables, then run the installer with any Python 3 (KiCad's own works):

```
cargo build --release -p freerouting-cli -p freerouting-gui
python kicad-plugin/install.py --bin-dir target/release
```

It copies `freerouting_rs/` into the newest KiCad version's user `scripting/plugins` folder
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
