# KiCad plugin

An action plugin for the KiCad PCB editor (tested with KiCad 10.0 on Windows).
Its toolbar button opens a dialog that:

- **Route**: exports the board to Specctra DSN, runs `freerouting -de board.dsn -do board.ses`,
  and imports the session back into the board. The Rust autorouter is not ported yet, so for
  now this reports "routing is not implemented yet".
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
