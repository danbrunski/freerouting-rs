# KiCad plugin

An action plugin for the KiCad PCB editor (tested with KiCad 10.0 on Windows).
Its toolbar button opens a dialog that:

- **Route**: exports the board to Specctra DSN, runs `freerouting-rs -de board.dsn -do board.ses
  --progress json --result-json board.result.json` with the options of the dialog, shows the
  router's progress live, and imports the session back into the board. **Stop and keep result**
  finishes the current step and imports the best board so far; **Abort** kills the router and
  imports nothing. A summary (connections routed and incomplete, vias, length, time) follows the
  import, with a warning when connections are left. Under profile Enhanced the router checks
  its routed copper against the board's rules before it saves (`router.output_check=repair`,
  Q20), and the summary adds a **Rule check** line from the manifest's `output_check` object:
  `found` (routed breaches, per rule in `by_rule`: `clearance`, `copper_edge_clearance`,
  `keepout`, `track_width`, `via_diameter`, `drill_out_of_range`, `annular_width`,
  `hole_clearance`, `solder_mask_bridge`, `via_in_pad`, `track_dangling`, `via_dangling`),
  `removed`, `retried`, `repaired` and `left_open` (connections that only fit by breaking a rule
  stay unrouted); `input` counts the board's own violations, which are never changed, and a
  `remaining` above 0 (`report` mode) adds a warning.
- **Route in GUI**: the same export, then `freerouting-gui -de board.dsn -do board.ses` with the
  options; the window routes after its countdown and writes the session when routing ends or the
  window closes, and the plugin imports it. **Terminate** kills the window and imports nothing.
- **Options** sit on three tabs (Route, Board and DSN, Advanced) above the buttons. The Route tab
  (maximum passes, threads, optimizer on/off and its threshold, net classes to ignore,
  time limit, seed, minimum trace width) and the others are saved per board in
  `freerouting-rs/route-options.json`. The minimum trace width defaults to the Board Setup
  minimum track width, which the DSN does not carry.
- **DSN fix-ups and flow** (K09, `dsn_fixups.py`, ported from canary's `dsn_export.py`): per
  class routed widths, outer pours of listed classes kept as fixed copper (or kept clear by the
  router), outer pours of listed nets dropped, the board-edge and pad clearances as keepouts (on by
  default: they are board rules), inner plane layers opened to routing, an add-only run that keeps
  the existing tracks, the start rip-up cost, Q10's completion sweep, and a remembered pass limit.
  A **Preset** drop-down fills the dialog from option files in `freerouting-rs/presets/` next to the board
  (or the user's presets folder); **Save as preset...** writes one. The plugin ships none. After the import
  the plugin sets each routed via's drill from the session (KiCad's import takes the net class's).
- **Board input** (K10, the default since K13.7): the plugin saves a snapshot of the open board
  (`SaveBoard`, with the project's net classes from the live board) and routes that with the router's own
  `.kicad_pcb` reader, instead of KiCad's DSN export. When the reader refuses the board (a file older than
  KiCad 6, or a construct it does not support) the plugin exports the DSN instead and says so. The Board
  page's "Board input" picks the other input; the Legacy profile defaults to the DSN, as Java's plugin
  reads a DSN. A board's saved choice is kept; a saved file without one takes the profile's default, except
  that one with a DSN-only fix-up (below) keeps the DSN, and an older file's `apply_kicad_rules` is dropped
  when it is saved again. The snapshot is refused while an option that rewrites the
  DSN is on (class widths, fixed or dropped pours, opened plane layers); the note under "Board input" names
  them on either input, before Route. The board-edge and pad clearances box
  (on by default) does not refuse it: the reader keeps those clearances itself, so the box is ignored on the
  snapshot, and the dialog and the run log say so. The project's
  `.kicad_dru` rules reach the router on both inputs (`--kicad-dru` for the DSN, `--apply-design-rules` for the
  snapshot; K13.1). The result comes back through the same SES import.
  Measured on eight boards (K10.5, `docs/development/kicad-plugin.md`): the snapshot grades the same on four,
  better on three (`CM5_MINIMA_3` 4 errors against 195, because KiCad's DSN export turned track arcs into one
  chord each; `canary-pwr` 38 unconnected against 42; `video` 0 errors against 1) and one error worse on
  `One-Air-Max` (a violation the board already has, counted once more). K13.6 then judged nine boards: the
  snapshot had 0 manufacturing breaches and 68 unconnected against the DSN's 73, which made it the default.
- **Open in viewer**: exports the board and opens it (plus the last `.ses`, if any) in
  `freerouting-gui`.
- **Import SES...**: imports any Specctra session file into the board.

The DSN and SES files go in a `freerouting-rs` folder next to the `.kicad_pcb`. The board
must be saved once so that folder has somewhere to live.

## Use the same flow from the command line

From a source checkout, run with a Python that has KiCad's `pcbnew`:

```
python3 -I kicad-plugin/headless_run.py route board.kicad_pcb out route-options.json /absolute/path/to/freerouting
```

Use the board's `freerouting-rs/route-options.json` to reproduce its dialog choices.
This command and the plugin share zone refill, DSN/snapshot preparation, minimum
width, router arguments and process invocation. The command writes `out/board.ses`
and `out/board.result.json`; it leaves the source board on disk untouched. To
apply the session to a copy:

```
python3 -I kicad-plugin/headless_run.py import board.kicad_pcb out/board.ses routed/board.kicad_pcb route-options.json
```

Create the destination directory first and keep the matching `.kicad_pro` and
`.kicad_dru` alongside the copy when checking it with KiCad. Import corrects via
drills and refills zones, as the plugin does. The result summary is the router's
connection count; use KiCad DRC for its post-fill unconnected count.

Both frontends refill zones before export, stop if filling fails, and record
`board.args.json` with the exact router arguments, KiCad version and prepared
input SHA256. This avoids routing stale pour fills after an edit. Use the same
saved board, KiCad version, router binary, options and router configuration/environment
when comparing runs; save unsaved editor changes first.

Older external exporters, including CANary's `hardware/tools/dsn_export.py`,
remain separate implementations. Their DSNs can differ even with matching router
flags. This shared command replaces that preparation for plugin comparisons;
it retains the plugin's rounded-corner edge protection and placement keepouts.
The historical CANary 33-unconnected result has not been reproduced by this change.

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

Most tests use plain Python and stubs. Real KiCad/CLI integration tests skip when
their tools are unavailable; `FREEROUTING_RS_BIN` selects the router's directory.
