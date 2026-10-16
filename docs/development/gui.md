# freerouting-gui: handover notes

The desktop app in `crates/freerouting-gui` (binary `freerouting-gui`), built on
egui/eframe 0.32 with the wgpu renderer. The rule from the port plan is that the
GUI only draws and handles input. Board logic, including the future interactive
routing, drag and shove, belongs in the engine crate (`crates/freerouting`).

## What works

The app is a board viewer for now. Nothing in it edits or routes.

- It opens a `.dsn` board, optionally with a `.ses` session on top, from the
  command line (`freerouting-gui board.dsn [board.ses]`), from File > Open
  (a built-in file browser), File > Open Recent (the last 10 files, newest
  first; a file that is gone stays listed, disabled) or by drag and drop.
  Files are read on a background thread. Opening a `.ses` on its own looks for
  its `.dsn` next to it (by `base_design`, the file stem, then the session
  name; a trailing `.dsn` in those names is ignored). If none is found, the
  session goes on top of the board already open. The engine imports the
  session (`import_session`); if it could not import some nets, wires or
  vias, the warnings window says how many.
- It draws traces, pads, vias, conduction areas (planes and fills), keepouts
  (trace/via/placement), the board outline, footprint outlines, and a
  reference designator for every placed component.
- Mouse and keys follow Java (`gui.board.BoardPanel`, `InteractiveState`,
  `MenuState`). The wheel or a pinch zooms around the cursor. The middle
  button, or Space and the left button, pans. The left button is reserved for
  tools (inspect and region select come later); the right button is reserved
  for context menus. `A` or `Home` fits the board, `Z` / `O` zoom in / out by
  2 at the cursor, `C` centres on the cursor, `F` then a left-drag zooms to
  the dragged rectangle, `,` toggles a crosshair, `Esc` cancels `F`. `G V U B
  S R D W` are reserved for the checklists that add them (`input::RESERVED`).
  The view fits again automatically when the window resizes, until the user
  pans or zooms. (Open question for Dan, G00 question 1: this is the
  recommended set; the change from "any button pans, `F` fits" awaits his
  confirmation.)
- The layer panel shows or hides each layer and picks the active layer, which
  is drawn on top; keys `1`–`9` do the same, `+` / `-` step to the next or
  previous signal layer. A separate checkbox list shows or hides each kind of
  item, plus the reference designators.
- Settings are kept between runs (window size by eframe, the rest in
  `settings.rs`): layer visibility by layer name, which kinds are shown,
  labels, the last folder, the recent files and the side panel width. The
  stored value carries a schema version; anything this version does not
  understand falls back to the defaults.
- The status bar shows the cursor position in millimetres, in file
  coordinates, so it matches the CAD tool. It also describes the item under
  the cursor: its kind, its owner (`U1-3` for a pad), its net and its layer.
- A warnings window lists what the DSN reader skipped.

Screenshots from the cloud sessions are in the project's shared folder under
`freerouting-rs/gui/`.

## Layout

| File | What it does |
| --- | --- |
| `lib.rs` | The library target (`freerouting_gui`), so integration tests can reach `App`. |
| `main.rs` | Argument collection, window options, the WSL X11 switch (see below). |
| `args.rs` | The command line: files, `--bench`, `--frames`, `--screenshot`, `--exit`, `--control-dir`. |
| `app.rs` | `App`: state, `execute(Command)` (the one path for menus, keys and tests), background loading, `save`. `layout.rs` lays out one frame (menu, status bar, side panel, canvas). |
| `commands.rs` | `enum Command`, every user action. |
| `input.rs` | The key table (Java's keys). `viewport.rs` has the mouse model, `ViewState` (camera, visibility, active layer) and the hover status. |
| `session.rs` | `Session` (the engine board as `Arc<RoutingBoard>`, the design, paths, `edit_generation`/`saved_generation`, warnings, SES summary), `load()` and `find_design()`. |
| `scene.rs` | `Scene::from_board(&RoutingBoard)` flattens the engine board into drawable `Item`s (kind, slot, geometry in mm, net, owner) and `Label`s. `Scene::pick()` does the hover hit-testing. |
| `view.rs` | `Tessellation` turns the scene into triangles once per load, as f32 mm relative to the board centre, one batch per (slot, kind). `Camera` handles pan and zoom. `paint()` transforms the vertices every frame, draws them and reports what it drew. Also the colours and the label drawing. |
| `panels/`, `menu.rs` | The side panel sections (`layers`, `board_stats`) and the warnings window, each a function `ui(ui, ..) -> Vec<Command>`; the menu bar. |
| `settings.rs` | `PersistedSettings`, stored under one key with a schema version. |
| `stats.rs`, `bench.rs`, `capture.rs` | Frame statistics, the `--bench` script, the in-app screenshot. |
| `file_dialog.rs` | A minimal folder browser filtered to `.dsn`/`.ses`. |

The data flows like this: the file is read into a `dsn::Pcb`.
`freerouting::io::specctra::build_board` builds the `RoutingBoard` (moved out
of the read result, not cloned); if a session is open,
`import_session` puts its wires and vias on top. The `Session` keeps the
board; `Scene::from_board` flattens it, `Tessellation::new` triangulates it,
and `paint` draws it every frame. Background jobs (DRC, ratsnest and later
routing) will take an `Arc` clone of the board, which is free; an edit goes
through `Session::board_mut()`, which copies the board only while a job still
holds the old one, and bumps `edit_generation`. `is_dirty()` compares that
with `saved_generation`, because `board.revision()` counts inserts and
removes only (and `delete_all_traces_and_vias` does not bump it at all).

## Decisions worth knowing

- **The scene is built from the engine board, not the DSN tree.** An earlier
  version placed footprints from the DSN itself and got back-side parts
  wrong. The engine matches Java on placement, so trust it.
- **The engine imports sessions, on top of the design's wiring, as Java's GUI
  does** (`gui/board/GuiManager.java` calls `SesReader.read` on the design
  board). `freerouting::io::specctra::import_session` is that port, and
  `tests/reference_drc.rs` already checks it against Java. An earlier version
  merged the session into the DSN syntax tree and replaced the design's
  wiring; that path (`scene::apply_session`) is gone. The difference shows on
  boards whose DSN already has wiring: the design's traces stay and the
  session's are added (G01.3 recorded the counts below).
- **Dirty tracking is the session's.** See "The data flows like this".
- **One command path.** Menus, keys, panels and tests all produce `Command`s
  and `App::execute` runs them, so tests drive the app without pixel
  coordinates.
- **A scripted run never touches the user's settings** (`--bench`,
  `--screenshot`, `--exit`, `--control-dir`): it starts from a throwaway
  persistence file and does not write one.
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
cargo test --release -p freerouting-gui --lib -- --include-ignored   # also the all-sessions comparison
UPDATE_SNAPSHOTS=1 cargo test -p freerouting-gui --test ui_viewer     # re-record a snapshot
cargo clippy -p freerouting-gui --all-targets
cargo fmt -p freerouting-gui
```

- The tests: unit tests in the modules (scene building, picking, labels,
  session loading against the engine, the bench script, argument parsing) and
  egui_kittest tests in `tests/`: `ui_viewer.rs` (loading, the side panel, a
  snapshot), `ui_input.rs` (Java's mouse and keys), `ui_settings.rs`
  (persistence, Open Recent). `tests/common/mod.rs` has the harness helpers.
  A full test run takes about 40 seconds in debug. `view::tests::tessellates_fixture_boards`
  covers every fixture under 50 KB; the larger fixtures are too slow to build
  in a debug build.
- **Rendering tests** (snapshots) run on Linux with lavapipe
  (`mesa-vulkan-drivers`; a missing Vulkan adapter fails the test with a
  message saying so). On Windows and macOS they are skipped unless
  `FREEROUTING_GUI_SNAPSHOTS=1`; interaction tests run everywhere.
  egui_kittest is pinned to `=0.32.3`, the egui version in `Cargo.lock`;
  upgrade them together and re-record the snapshots.
- Use `--release` when running real boards. A debug build is very slow at
  building the board.
- On WSL the app runs through WSLg. Plain Linux needs `libxkbcommon-x11` and a
  Vulkan or OpenGL driver. The native Windows build (windows-gnu with mingw)
  is what the KiCad plugin launches; see `kicad-plugin.md`.
- **Command-line flags for scripted runs:**
  `--screenshot out.png` saves the first frame after the load finished as a
  PNG (the app takes it itself, so there are no X server races and no DPI
  problem); `--exit` quits after it; `--bench pan-zoom --frames N` runs a
  camera script and prints one JSON object on stdout (below);
  `FREEROUTING_GUI_STATS=1` shows frame statistics in a corner of the window
  and logs them every 120 frames.
- **Headless smoke test.** Never launch the app on your own desktop; use
  Xvfb:
  ```
  sudo apt-get install -y xvfb libxkbcommon-x11-0 mesa-vulkan-drivers libegl1 xdotool imagemagick
  cargo build --release -p freerouting-gui
  tools/gui/smoke.sh fixtures/Issue690-kit-dev-coldfire-xilinx_5213.dsn [--keys "z"]
  ```
  `tools/gui/smoke.sh` runs the release binary under `xvfb-run` with
  `WGPU_BACKEND=vulkan`, waits for the window (`xdotool search --sync`) and
  for the board (a marker file from `--control-dir`), sends the keys with
  `xdotool`, takes the in-app screenshot and compares it with
  `tools/gui/golden/<fixture>.png` by `compare -metric AE -fuzz 10%`. It
  exits 0 when at most 1 % of the pixels differ, 1 when more do (the count is
  printed), 2 on a setup problem. `UPDATE_GOLDEN=1` writes the golden;
  `SMOKE_OUT=file.png` keeps the screenshot; `GOLDEN=` picks another golden.
  It refuses `WGPU_BACKEND=gl`, which panics with "Invalid surface" under
  Xvfb. Winit drops keys while the window has no focus, and Xvfb has no
  window manager, so the script focuses the window with `xdotool windowfocus`;
  `xdotool key --window` (synthetic events) does not work.
  Screen captures on Windows must be DPI-aware: at 125% scaling, a naive
  capture grabs only the top-left part of the window.

### Frame numbers (`--bench`)

`freerouting-gui --bench pan-zoom --frames N board.dsn` runs a camera script
(fit; 10 zoom-in steps of x1.25 at the board centre; pans of 20 % of the view
width right, down, left and up; 10 zoom-out steps; repeated), one step per
frame, ignores the first 30 frames and prints
`{"script","frames","p50_ms","p95_ms","max_ms","triangles","draws","uploaded_bytes","load_avg_1m"}`.
"Frame CPU" is the time on the GUI thread inside `App::update`, which includes
building and painting the board's shapes. It excludes egui's tessellation,
queue submission and presentation (under Xvfb that is llvmpipe rasterising,
not a usable metric). `triangles` is the tessellation's count; `draws` and
`uploaded_bytes` are those of the last frame (egui vertices are 20 bytes,
indices 4). Run it in a release build on a quiet machine (1-minute load
average at most 1.0) and record the load average next to the numbers.

**FT0, recorded 2026-10-15** (release build, Xvfb with lavapipe, `--bench
pan-zoom --frames 600`, three runs each, frame CPU in ms). The machine was
not quiet: other sessions were building and running Java, so the 1-minute
load average was 5–8 on 8 cores (the target is at most 1.0). Re-measure on a
quiet machine before comparing against FT1.

| Board | Run | p50 | p95 | max | triangles | draws | uploaded bytes (last frame) | load 1 min |
|---|---|---|---|---|---|---|---|---|
| Issue190-processor.Z80 | 1 | 2.792 | 3.568 | 18.835 | 350,056 | 3,316 | 11,797,412 | 7.85 |
| | 2 | 2.615 | 3.106 | 4.751 | 350,056 | 3,316 | 11,797,412 | 6.94 |
| | 3 | 2.615 | 3.071 | 4.168 | 350,056 | 3,316 | 11,797,412 | 5.84 |
| Issue690-kit-dev-coldfire-xilinx_5213 | 1 | 0.677 | 0.876 | 1.231 | 184,222 | 1,811 | 6,206,424 | 5.70 |
| | 2 | 0.670 | 0.856 | 1.476 | 184,222 | 1,811 | 6,206,424 | 5.44 |
| | 3 | 0.737 | 0.948 | 1.322 | 184,222 | 1,811 | 6,206,424 | 6.63 |

The triangle counts match the research pass (350,056 and 184,222). This
metric is the time inside `App::update` only: it does not include egui
tessellating the 3 to 3.3 thousand meshes and the renderer uploading them,
which is the larger part of the cost today (a bench run takes about 45 ms per
frame wall-clock under llvmpipe). So FT0 understates what a GPU renderer
saves; G02 should measure its own number the same way and compare, and may
want to add the egui tessellation time to the definition.

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
- Screen readers: eframe's `accesskit` feature is on, so the widgets are in
  the accessibility tree (egui_kittest finds them by label through it). Nobody
  has tried a screen reader on it.
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
