# freerouting-rs

A Rust port of [Freerouting](https://github.com/freerouting/freerouting), the
Specctra DSN/SES PCB autorouter: a Rust routing engine plus a cross-platform
desktop GUI for Windows, macOS, Linux and WSL.

Work in progress: nothing routes yet. See [docs/PORT_PLAN.md](docs/PORT_PLAN.md)
for the module map, crate layout and porting order.

## Layout

| Crate | Purpose |
| --- | --- |
| `freerouting-geometry` | Planar geometry with exact integer/rational math |
| `freerouting-dsn` | Specctra DSN/SES/rules parsing and writing |
| `freerouting` | Routing engine library: board, rules, DRC, autorouter, optimizer |
| `freerouting-cli` | The `freerouting` command-line tool |
| `freerouting-gui` | The `freerouting-gui` desktop app (egui/eframe) |

## Install

There is no release yet: build from source as below. Releases will put builds for Windows (MSI
and zip), macOS (universal DMG and tar.gz) and Linux (AppImage and tar.gz) on the
[releases page](https://github.com/danbrunski/freerouting-rs/releases), with a `SHA256SUMS`
file. They will not be signed at first, so Windows shows a SmartScreen warning and macOS asks for
"Open Anyway" on the first launch. What each file is, the Linux requirements and how to verify
the download: [docs/development/release.md](docs/development/release.md).

## Board viewer

```
cargo run --release -p freerouting-gui -- fixtures/Issue754-avionics_hub.dsn
```

Opens a Specctra `.dsn` board (and optionally its `.ses` session as a second argument).
Boards can also be opened from File > Open or by dropping a file on the window.
Scroll to zoom, drag to pan, `F` to fit the board, `1`–`9` to bring a layer to the
front. Hovering over an item shows its net and layer in the status bar.

On Linux the app needs the usual X11/Wayland libraries (`libxkbcommon-x11`, a
Vulkan or OpenGL driver); under WSL it runs through WSLg.

## KiCad plugin

[kicad-plugin](kicad-plugin/README.md) adds a PCB editor toolbar button that exports the board
to DSN, routes it with the `freerouting` command line, imports the session back, and opens
boards in the viewer.

## License

GPL-3.0, the same as upstream Freerouting, since this is a derivative work.

Portions are ported from KiCadRoutingTools (MIT, Copyright (c) 2026
drandyhaas). Its licence text is in `LICENSES/MIT-KiCadRoutingTools.txt` and
the sources are listed in `THIRD_PARTY_NOTICES.md`.
