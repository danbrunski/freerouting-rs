# freerouting-rs

A Rust port of [Freerouting](https://github.com/freerouting/freerouting), the open-source
Specctra DSN/SES autorouter for printed circuit boards. The port has three parts:
- `freerouting`, a command line that routes a board;
- `freerouting-gui`, a desktop app for viewing, routing and editing;
- a KiCad plugin that routes the board open in KiCad's PCB editor.

It runs natively on Windows, macOS, Linux and WSL, without a Java runtime.

The port follows the Java code closely, so a board routes the same way. On the 140-board parity
gate, 135 boards produce a session byte-identical to Java's (Freerouting pinned at `a773a70ec`).
Every intended difference is listed in [docs/DIVERGENCES.md](docs/DIVERGENCES.md).

Status: version 0.1.0.
- The router, the desktop app and the KiCad plugin all work end to end.
- There is no published release yet; build from source as below.

## What it does better than the Java version

**Two routing profiles.** **Enhanced**, the default, routes with the improvements over Java that
a measurement has accepted; **Legacy** gives results identical to Java Freerouting. Below the
profile every stage (fanout, net ordering, connection router, rip-up policy, optimizer, the
special stages) and every option can still be set on its own. Pick one with `--profile legacy`
on the command line, the Profile selector in the desktop app's autoroute settings, or the
Profile choice in the KiCad plugin's dialog.

Enhanced changes three things so far, each accepted by a measurement on real boards
([docs/DIVERGENCES.md](docs/DIVERGENCES.md), "Profile `enhanced`"):
- **No traces on power layers.** Vias still land in their net's plane.
- **Neck-down when a connection fails.** A failed connection is retried with traces down to
  300 µm, so a board can keep its Board Setup class widths. Full width is kept wherever the
  first attempt fits.
- **Vias take their net class's clearance**, as KiCad checks them, where Java uses the via
  rule's.

**Java's bugs, fixed one switch at a time.** The port kept Java's bugs so that Legacy matches
Java. The ones no board reaches are now fixed outright. Each of the others has a fix behind
`router.java_quirks.<id>`; unset means Java's behaviour, and Legacy keeps every one. A fix
joins Enhanced once its measurement accepts it, including a fix that changes nothing on the
measured boards: 37 of them so far (the maze search, the fanout, shove, pull-tight, the batch
optimizer, the any-angle room graph, the geometry cutouts, the DSN and SES readers, the reports and interactive
editing). `--router.java_quirks.<id>=true` brings one
bug back. Options that won only on one kind of board are offered as **Experimental**, with that
board class in their help text.

**Speed**
- **No JVM.** Each tool is one native executable that starts in milliseconds. Java spends about
  3 s on start-up and class loading before routing begins. Small boards therefore finish 7× to
  160× sooner end to end: for example 0.04 s against 3.2 s, and 0.54 s against 3.9 s.
- **Large boards route at about Java's speed today.** On the 11 benchmark boards where Java takes
  at least 10 s:
  - the median board is 1.16× faster than Java, with interf_u up to 4.6× faster;
  - the largest boards, with many connections, run at 0.77× to 0.92× of Java's speed;
  - so in total Rust takes about 9 % longer.

  These timings were taken on a busy machine, so they are rough
  ([docs/roadmap/performance.md](docs/roadmap/performance.md)). Getting clearly ahead, with a
  target of 3× on large boards, is the performance roadmap's job.
- **A parallel optimizer that is also reproducible.** Java's multi-threaded optimizer gives
  different results from run to run. Here the thread count changes nothing in the output, and
  the work still scales: the optimizer on DAC2020 bm10 takes 53 s on one thread and 10 s on
  eight, with a byte-identical session on every run.

**For KiCad users**
- **Reads and writes `.kicad_pcb` directly.** Java only reads Specctra DSN; given a KiCad board it
  reports "Unknown file type".
- **A plugin with live progress.**
  - The options sit on three tabs (Route, Board and DSN, Advanced), with the buttons always in
    view.
  - "Stop and keep result" finishes the current step and imports the best board so far. "Abort"
    imports nothing.
  - "Route in GUI" watches the routing in the desktop app.
  - Rule areas are fixed up before routing. Optional DSN fix-ups (class widths, fixed or dropped
    pours, edge and pad clearances as keepouts) reproduce canary's export, and presets save a
    set of options for reuse.
  - Instead of the DSN export, it can route a snapshot of the open board through the router's
    own `.kicad_pcb` reader (experimental).
  - Net and part names outside ASCII round-trip intact (Java's plugin strips them).
  - A version check catches a mismatched router.
  - Its plugin-manager package, not yet published, carries the executables for Windows, macOS
    and Linux.

**For scripts and tools**
- **The CLI accepts Java's flags and adds:**
  - `--progress json` and `--result-json` for machine-readable progress and results;
  - `--version`, `--capabilities`, `--threads` and `--timeout`;
  - `--profile legacy|enhanced`, `--list-stages` (the stages, their ids and options) and
    `--dump-settings` (every effective setting and where it came from);
  - a stop request on standard input that keeps the best result so far;
  - distinct exit codes.
- **No half-written files.** An output file is written whole or not at all, and never as 0 bytes.
- **Diagnostics.** A report says why a connection cannot be routed, not only that it wasn't.

**The desktop app**
- **The board is drawn on the GPU** with wgpu. A synthetic board of 5.6 million triangles stays
  under 2 ms per frame (p95), and the view redraws live while the autorouter runs.
- **Interactive routing and editing**, undo and redo, the rules dialogs and the Info lists, all
  ported from Java's GUI.

**Still in progress.** More candidates are being measured for Enhanced. The stages ported from
KiCadRoutingTools can be picked in the command line, the app and the plugin, but none is on by
default: length matching (`krt-meander`), BGA and QFN fanout (`krt-bga-escape`,
`krt-qfn-escape`), and coupled differential pairs (`krt-diffpair`, Experimental: it helps boards
with one or a few pairs on a shared signal layer). The KiCadRoutingTools grid router and bus
routing were measured and removed. See [docs/roadmap/](docs/roadmap/).

## Install the KiCad plugin

One command builds the router and the app and installs the plugin into KiCad:

```
python tools/install_plugin.py
```

Any Python 3.8 or newer works, KiCad's own included. Before building, it checks the machine and
offers to fix what is missing, asking first each time:
- **Rust:**
  - Windows: rustup through `winget`;
  - Linux and macOS: the rustup.rs script;
  - any system: `rustup update` when the installed Rust is older than the workspace needs.
- **A linker:**
  - Windows: Visual Studio's C++ build tools, or, without them, the `windows-gnu` toolchain with
    WinLibs' mingw-w64 (it finds an existing install and puts it on the PATH for the build);
  - Linux: a C compiler, from the distribution's packages (apt, dnf, pacman, zypper);
  - macOS: the Xcode command line tools.
- **The viewer's libraries on Linux:** xkbcommon, and Vulkan or EGL.
- **Before the build starts:**
  - KiCad's user folder, where KiCad itself looks: under `KICAD_DOCUMENTS_HOME` when set, else
    Windows' Documents folder (asked of Windows, also from WSL, so one that OneDrive or a
    redirection moved to another drive is found), `~/Documents` on macOS, and `XDG_DATA_HOME` or
    the Flatpak's folder on Linux;
  - enough free disk space;
  - on Windows, a viewer or router still running, since Windows locks a running exe.

Useful options:

| Option | What it does |
|---|---|
| `--check` | Report what is there and what is missing; change nothing |
| `--yes` | Apply every fix offered without asking |
| `--no-setup` | Never install anything; stop on a problem |
| `--skip-build` | Install the executables already built |
| `--windows-kicad` | From WSL: install into the Windows KiCad, which then runs this Linux build through WSL |
| `--kicad-version 9.0` | Install into that KiCad version's folder instead of the newest |
| `--plugins-dir DIR` | Install into DIR; with `--windows-kicad`, a Windows path such as `D:\Documents\KiCad\10.0\scripting\plugins` works too |
| `--keep-pcm` | Passed to `kicad-plugin/install.py` |

Then, in KiCad's PCB editor: Tools > External Plugins > Refresh Plugins. No restart is needed.
The plugin's own documentation: [kicad-plugin/README.md](kicad-plugin/README.md).

## Build and run by hand

```
cargo build --release -p freerouting-cli -p freerouting-gui
target/release/freerouting -de board.dsn -do board.ses             # route
target/release/freerouting -de board.kicad_pcb -do routed.kicad_pcb  # a KiCad board, directly
target/release/freerouting-gui fixtures/Issue754-avionics_hub.dsn    # the desktop app
```

`freerouting --help` lists every option.

The app opens a `.dsn` (optionally with its `.ses` as a second argument) or a `.kicad_pcb`, from
the command line, from File > Open, or by dropping the file on the window. Scroll to zoom, drag to
pan, `F` to fit the board, `1`–`9` to bring a layer to the front.

On Linux the app needs the usual X11 or Wayland libraries (`libxkbcommon-x11`, plus a Vulkan or
OpenGL driver). Under WSL it runs through WSLg.

## Layout

| Crate | Purpose |
| --- | --- |
| `freerouting-geometry` | Planar geometry with exact integer and rational math |
| `freerouting-dsn` | Specctra DSN, SES and rules parsing and writing |
| `freerouting-kicad` | Direct `.kicad_pcb` reading and writing |
| `freerouting` | The routing engine: board, rules, DRC, autorouter, optimizer |
| `freerouting-krt` | Routing stages ported from KiCadRoutingTools |
| `freerouting-stages` | The full stage registry (engine plus KRT stages) the CLI and GUI route with |
| `freerouting-cli` | The `freerouting` command line |
| `freerouting-gui` | The `freerouting-gui` desktop app (egui/eframe, wgpu) |
| `freerouting-dev` | Developer tools: benchmarks, parity checks, the board corpus |

How the port maps the Java modules: [docs/PORT_PLAN.md](docs/PORT_PLAN.md).

## License

GPL-3.0, the same as upstream Freerouting, since this is a derivative work.

Portions are ported from KiCadRoutingTools (MIT, Copyright (c) 2026
drandyhaas). Its licence text is in `LICENSES/MIT-KiCadRoutingTools.txt` and
the sources are listed in `THIRD_PARTY_NOTICES.md`.
