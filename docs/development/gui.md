# freerouting-gui: handover notes

The desktop app in `crates/freerouting-gui` (binary `freerouting-gui`), built on
egui/eframe 0.32 with the wgpu renderer. The rule from the port plan is that the
GUI only draws and handles input. Board logic, including the future interactive
routing, drag and shove, belongs in the engine crate (`crates/freerouting`).

## What works

The app is a board viewer for now. Nothing in it edits or routes.

- It opens a `.dsn` board, optionally with a `.ses` session on top, from the
  command line (`freerouting-gui board.dsn [board.ses]`), from File > Open
  (a built-in file browser) or by drag and drop. Opening a `.ses` on its own
  looks for its `.dsn` next to it (by `base_design`, the file stem, then the
  session name). If none is found, the session goes on top of the board
  already open.
- It draws traces, pads, vias, conduction areas (planes and fills), keepouts
  (trace/via/placement), the board outline, footprint outlines, and a
  reference designator for every placed component.
- Wheel or pinch zooms around the cursor. Dragging with any mouse button pans.
  `F` or `Home` fits the board. The view fits again automatically when the
  window resizes, until the user pans or zooms.
- The layer panel shows or hides each layer and picks the active layer, which
  is drawn on top; keys `1`–`9` do the same. A separate checkbox list shows or
  hides each kind of item, plus the reference designators.
- The status bar shows the cursor position in millimetres, in file
  coordinates, so it matches the CAD tool. It also describes the item under
  the cursor: its kind, its owner (`U1-3` for a pad), its net and its layer.
- A warnings window lists what the DSN reader skipped.

Screenshots from the cloud sessions are in the project's shared folder under
`freerouting-rs/gui/`.

## Layout

| File | What it does |
| --- | --- |
| `main.rs` | Window options. Under WSL it forces X11 (see below). |
| `app.rs` | `App` (eframe state, panels, input). `load()` reads files on a background thread and builds the board and scene. |
| `scene.rs` | `Scene::from_board(&RoutingBoard)` flattens the engine board into drawable `Item`s (kind, slot, geometry in mm, net, owner) and `Label`s. `apply_session()` merges a SES into the DSN syntax tree. `Scene::pick()` does the hover hit-testing. |
| `view.rs` | `Tessellation` turns the scene into triangles once per load, as f32 mm relative to the board centre, one batch per (slot, kind). `Camera` handles pan and zoom. `paint()` transforms the vertices every frame and draws them. Also the colours and the label drawing. |
| `file_dialog.rs` | A minimal folder browser filtered to `.dsn`/`.ses`. |

The data flows like this: the file is read into a `dsn::Pcb`. If a session is
open, `apply_session` merges it in. `freerouting::io::specctra::build_board`
builds the `RoutingBoard`, `Scene::from_board` flattens it,
`Tessellation::new` triangulates it, and `paint` draws it every frame.

## Decisions worth knowing

- **The scene is built from the engine board, not the DSN tree.** An earlier
  version placed footprints from the DSN itself and got back-side parts
  wrong. The engine matches Java on placement, so trust it.
- **Sessions are merged at the syntax-tree level.** The engine has no SES
  import. Session coordinates are in steps of `1/value` of the resolution
  unit, while DSN coordinates are in whole units, so `apply_session` rescales
  them. The session's routes replace the design's wiring, because
  Freerouting writes every wire to the session. Padstacks from `library_out`
  are added when they are missing.
- **The built-in file browser is deliberate.** `rfd` needs GTK or an XDG
  portal on Linux, and WSLg has neither by default.
- **X11 under WSL** (`main.rs`, `window_system`): under WSLg a Wayland window
  draws its own flat title bar, while X11 windows get the standard Windows
  frame and buttons. `FREEROUTING_WAYLAND=1` overrides this. It needs the
  Linux-only `winit` dependency in `Cargo.toml`.
- **wgpu backends are enabled explicitly** in `Cargo.toml`. eframe's `wgpu`
  feature turns on no backend, and without them the app panics at startup on
  every platform. `gles` is the fallback where there is no Vulkan, Metal or
  DX12 driver.
- **Colours:** inactive layers are dimmed by blending toward the background,
  not by alpha. With alpha, the round joints of a trace overlap its segments
  and show as dark dots. Planes and keepouts stay translucent.
- **Labels:** DSN files have no silkscreen text, so each reference designator
  is centred on the bounding box of its pads and outline, sized to fit, and
  turned upright on tall parts. Back-side labels are drawn first, in
  blue-grey, and are not mirrored. Real silkscreen (values, logos) would need
  the KiCad plugin to export it; see `kicad-plugin.md`.
- **Oval pads look octagonal.** That is the engine's (and Java's) shape
  approximation, not a drawing bug.
- **Hover status:** the status bar is laid out before the canvas, so the
  canvas stores the hover text in egui temp data and asks for a repaint when
  it changes. The temp value's type must be `Option<String>` on both the read
  and the write, or the read silently returns nothing.

## Running and testing

```
cargo run --release -p freerouting-gui -- fixtures/Issue754-avionics_hub.dsn
cargo test -p freerouting-gui
cargo clippy -p freerouting-gui --all-targets
cargo fmt -p freerouting-gui
```

- The tests cover scene building (pad placement on both sides, vias, traces,
  session merge, labels, picking) and tessellate every fixture under 50 KB.
  The larger fixtures are too slow to build in a debug build. A full test run
  takes about 40 seconds in debug.
- Use `--release` when running real boards. A debug build is very slow at
  building the board.
- On WSL the app runs through WSLg. Plain Linux needs `libxkbcommon-x11` and a
  Vulkan or OpenGL driver. The native Windows build (windows-gnu with mingw)
  is what the KiCad plugin launches; see `kicad-plugin.md`.
- **Headless screenshots** (how the cloud sessions checked their work):
  ```
  sudo apt-get install -y xvfb libxkbcommon-x11-0 mesa-vulkan-drivers libegl1 xdotool imagemagick
  Xvfb :99 -screen 0 1280x800x24 &
  DISPLAY=:99 ./target/release/freerouting-gui fixtures/Issue690-kit-dev-coldfire-xilinx_5213.dsn &
  sleep 10; DISPLAY=:99 import -window root shot.png
  ```
  `xdotool mousemove/click 4/key 2` drives hover, zoom and layer keys.
  Screen captures on Windows must be DPI-aware: at 125% scaling, a naive
  capture grabs only the top-left part of the window.

## What's next

In plan order. Each item needs engine work first, so check what has landed in
`crates/freerouting` before starting one.

1. **Ratsnest (airlines):** lines for the connections still to be routed.
   It needs the engine's connectivity/unrouted data (Java `RatsNest`). Draw it
   as a hairline overlay with its own toggle, and count the unrouted
   connections in the side panel.
2. **DRC panel:** a list of violations, where clicking one zooms to it, plus
   markers on the canvas. It needs the engine DRC.
3. **Autorouter run:** start and stop the router from the GUI, with live
   progress. The board should redraw as routes land, which needs a way to
   rebuild or patch the `Scene`/`Tessellation` from engine change events
   (today they are rebuilt only on load).
4. **Parameter dialogs:** route and autoroute settings, nets, net classes,
   the clearance matrix and via rules, as in the Java `gui.windows.*`.
5. **Interactive routing and editing,** with the logic in the engine's
   `interactive` module and only input mapping in the GUI.

Other gaps:

- Saving: there is no Save, and no SES or DSN export from the GUI yet.
- Recent files and remembered settings: eframe's `persistence` feature is off.
- Performance: every frame transforms all vertices on the CPU. That is fine
  for the fixtures. For very large boards, consider caching the transformed
  meshes while the camera is still, or a wgpu paint callback that transforms
  on the GPU.
- Widget style: the panels use egui's default dark style. Dan asked about
  native Windows looks, which turned out to be about the title bar only (now
  fixed). A Windows-like light theme is an open offer, not a request.

## Working rules

- Dan is the sole developer. Commit and push straight to main, with no PRs.
  Pull with `--rebase` before pushing. Keep fmt, clippy and tests green.
- The GUI work owns `crates/freerouting-gui`. Engine changes go into
  `crates/freerouting` in coordination with the engine work; don't change
  engine behaviour from the GUI side.
- Deliberate differences from Java go in `docs/DIVERGENCES.md`. The GUI has
  none so far; it is rebuilt with egui rather than translated from Swing.
