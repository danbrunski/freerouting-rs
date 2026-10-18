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
  button, or Space and the left button, pans. The left button is the inspect
  tool (G05, below); the right button opens the context menu. `A` or `Home`
  fits the board, `Z` / `O` zoom in / out by 2 at the cursor, `C` centres on
  the cursor, `F` then a left-drag zooms to the dragged rectangle, `,` toggles
  a crosshair, `Esc` cancels (a menu, a keepout, `F`, then the selection). `G`
  toggles the ratsnest, `V` the violation markers (or, with items selected,
  their clearance violations), `W` zooms to the selection, `U`/`Ctrl+Z` undo and
  `B`/`Ctrl+Y` redo. `S R D` choose the tool of the left button (G09, below).
  The view fits again automatically when the window resizes, until the user
  pans or zooms. (Open question for the owner, G00 question 1: this is the
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
- The status bar (G04.8, `panels/status.rs`) shows `Errors: n` and `Warnings: n`
  (a click opens the log window), the status line (an error, what is loading, the last
  notice, or what is under the cursor: its kind, its owner such as `U1-3` for a pad, its
  net and its layer), `Current layer: name`, and the cursor position in the chosen unit
  (`X` and `Y`, two fraction digits, padded as Java pads them) with the unit's label. The
  position is in the design file's frame, so it matches the CAD tool.
- A warnings window lists what the DSN reader skipped.
- **Board windows and display options (G04).** Info > Nets, Placed Components, Library
  Packages and Library Padstacks open lists over the board's own data on the same
  `ListWindow` the G03 lists use (rows built when the window opens, on Recalculate and
  when the board or the unit changes; Select selects and zooms). The nets window has the
  filter, "Incomplete connections only", the net count and `Net.printInfo` of the chosen
  nets. Display > Layer and Object Visibility has a 0 to 100 % slider (steps of 10) per
  copper layer and per kind of object, Display > Miscellaneous the crosshair (small or
  large; `,`), the board rotation (0, 90, 180, -90) and mirroring (left to right, top to
  bottom) and the automatic layer dimming slider, and Display > Color Manager the colour of
  each kind of item on each layer and the other colours. The toolbar has the unit selector
  (mil, in, mm, um). The side panel has six virtual layers (F/B silkscreen, courtyard and
  fabrication: component outlines are split by `is_courtyard`, `is_fabrication` and side;
  part numbers are drawn on the fabrication layers). Help > About shows the version, the
  commit (`unknown` until R01.2), the license and the links. The levels, the colours, the
  virtual layers, the unit and the crosshair are persisted (`settings.rs`; layer levels and
  layer colours by layer name). The item info window (G04.3) is wired by G05.
- **Ratsnest, DRC and the Info lists (G03).** After every load and every edit
  a worker thread (`analysis.rs`) runs the engine's `DesignRulesChecker` over
  an `Arc` clone of the session board: first the ratsnest (airlines, the
  incomplete count), then the clearance violations, unconnected items and zone
  islands. A result carries the `Tag` (load, board revision, edit generation)
  it was computed for and is hidden once the session has moved on; one worker
  runs at a time. The side panel's "Checks" section shows "Incomplete
  connections: n" and "Clearance violations: n" (clicking opens the list). The
  overlays (`overlays/`) are egui shapes drawn after the canvas: airlines as
  1 px lines (`G`, toolbar "Ratsnest", on by default), violation markers as the
  violation's shape with a ring (`V`, toolbar "Violations", off by default,
  as in Java) and the selection highlight. Info > Incomplete Connections,
  Length Violations, Clearance Violations, Unconnected traces and vias and
  Zone Islands open list windows (`panels/object_list.rs` is the generic one:
  Select, Invert, Info (the item info window), Recalculate and a filter). Select
  sets the selection and zooms to it. Run DRC forces a new run. Other > Delete
  All Traces and Vias (also a toolbar button) asks first, takes a snapshot and
  deletes (and unfills the conduction areas, as Java does); Undo/Redo
  (`U`/`B`, `Ctrl+Z`/`Ctrl+Y`, toolbar) restore the snapshots, and the scene is
  patched from the board's change journal (G05.5, below).
  File > Export DRC report writes the JSON report of the engine's
  `generate_report` (the same serializer as `-drc`, coordinates in mm). Row
  texts follow Java's bundles (`text.rs`); `docs/DIVERGENCES.md` lists where
  the lists differ from Java's.

- **Inspect mode, selection and safe edits (G05, `tools/`).** The left button is Java's
  `InspectMenuState`: a click selects what the engine's filtered pick finds at the cursor
  (`pick_items_filtered`, on the active layer, then on the other visible layers when
  Settings > Select says "All visible"), a click with items selected toggles one item (Ctrl+click
  also with none), a left drag draws a rectangle and selects the filtered `overlapping_items`
  of its box, added to the selection. Esc or the toolbar's Cancel clears it. A toolbar floats
  over the top of the canvas while items are selected: Cancel, Info, the four "extend to"
  buttons (Nets `N`, Connected Sets `S`, Connections `E`, Components), Violations (`V`), the
  three zoom buttons, and Delete (`Delete`), Fix and Unfix (decision G00 question 6; Java's
  toolbar has the first ten only). The right button opens the context menu of the mode (Select
  item, Start route and Swap pin (G09), Create keepout > Tile, Circle,
  Polygon, Hole; with items selected the extend entries; the display rows act on the click
  location). A keepout is built from clicked corners by the engine's `interactive::Construction`
  (I03.4: snapped to the board's angle restriction, concave tile corners dropped, as in Java), a
  rubber band follows the cursor, Enter completes, Esc cancels; an overlap says `Keepout canceled
  because it overlaps existing items.` A hole starts on a keepout, via keepout or conduction area
  of the active layer, takes the corners inside it and replaces the area with one that has the
  hole (Java's `HoleConstructionState`). Delete, Fix and Unfix call the engine's I03.6
  `delete_selection` / `fix_selection`; Fix and Unfix are undoable. Settings > Select is `WindowSelectParameter` ("General Settings":
  selection layers and the ten selectable classes, persisted). The item info window
  (`panels/item_info.rs`, the toolbar's Info, `I`, the Info buttons of the lists) prints the
  engine's `item_info` fields with Java's words. Every edit goes through
  `Loaded::sync_scene`: it drains the board's change journal (`take_changes`) and calls
  `Tessellation::patch`, so an edit, a delete all, an undo or a redo rebuilds only the chunks it
  touches; only a journal overflow rebuilds in full (`stats.full_rebuilds` stays 0, `stats.patches`
  counts the patches). FT4: a real undo of 200 removed plus 200 added items on Issue190 takes a
  median of 1.35 ms (max 2.15 ms) in `Loaded::sync_scene` in a release build (load average 21,
  not quiet; `tests/ui_edit.rs::ft4_undo_of_200_and_200_items`).

- **Interactive routing and editing (G09, `tools/{host,route,drag,move_component}.rs`,
  `overlays/route.rs`).** The app starts in route mode like Java. `S` (inspect), `R` (route) and
  `D` (drag) choose the tool of the left button, also from the toolbar; the three keys work
  without a selection and outside a session. The GUI holds no routing logic: a tool turns input
  into a command of the engine's `interactive::Editor` and `tools/host.rs` runs it with
  `Editor::apply_on`, which lends the editor the session board for one command (the board stays
  in the `Arc` the rest of the app reads). The response's change journal patches the scene
  (`Loaded::sync_scene_with`, `stats.patches`, no full rebuild), its status goes to the status
  line in Java's words (`text::interactive_status`), its layer becomes the active layer, and its
  `Overlay` is drawn by `overlays/route.rs`: the rubber band to the nearest target and the
  targets, the swappable pins, the length-matching ellipses, the layer-change marker, the
  obstacle a shove failed at (with "highlight routing obstacle"), the floating selection of a
  move and the clearance violations it causes. In route mode a click on an item starts a route
  (`T` starts one at the cursor); moving the pointer routes in dynamic mode and only moves the
  rubber band in stitch mode; a click adds a corner; `1`-`9`, `+`, `-` and the popup's Change
  layer change the layer with a via; Enter completes, Esc cancels (the tail is removed). In drag
  mode a press drags the item under the cursor (a component by its pads when "drag components"
  is on) or makes space when nothing is there; each pointer step is one editor command and one
  patch, and the release ends it. `W` starts a pin swap (a second click completes it), `P`
  toggles push-and-shove. The selection stays the GUI's (G05); the editor is told about it only
  for a move (`M`: the pointer carries the selection, `+` `*` `-` turn it by 90, 180 and 270
  degrees, `/` changes the side, a click puts it down, Esc puts it back) or a copy (`Ctrl+D`: a
  click inserts a copy). Undo and redo run through `Editor::apply` too; the engine refuses
  history while it holds a selection, as Java does, so the host clears the editor's selection
  first and the GUI keeps allowing undo with a GUI selection. The background DRC waits while a
  session is under way (`Analysis::set_held`), because its clone of the board would make every
  pointer step copy the whole board. Settings > Routing has the interactive fields
  (`settings::InteractiveSettings`, stored with the other settings, passed to every editor
  call) and the manual rules; Settings > Component Movement has the placement grid of a move.
  What differs from Java is in DIVERGENCES, "Interactive routing and editing".

- **Routing profile and Algorithms (N01.3, N01.4, `dialogs/autoroute_settings/algorithms.rs`).** The
  autoroute settings dialog opens with a Profile selector (Legacy / Enhanced, default Enhanced; switching
  moves the values that still have the old profile's value, `profile::rebase`) and an "Algorithms"
  section: one row per stage slot of the engine's catalog (`autoroute::stages::catalog`) with the
  registered ids to pick from (a list slot is one checkbox per id), then the slot's options. A value that
  differs from the profile's is marked "changed" with a Reset button (accessible label "Reset <row>")
  that returns it to the profile's value. The profile and the Algorithms rows are controls, so they are
  kept between runs like the others (`control_paths`); a front-end `--profile` moves the draft first.
  Tests: `ui_routing.rs::{profile_selector_covers_both_profiles, an_algorithm_stage_changes_and_resets,
  an_option_reset_returns_the_profile_value, the_algorithms_section_offers_the_krt_stages}`. The
  GUI routes and lists stages with `freerouting_stages::registry()` (N05.1: Freerouting's stages plus
  the KRT ones; the GUI does not depend on `freerouting-krt`). Not yet looked at by the owner: the layout and the look of the
  new rows.
- **Autorouter (G07, `routing.rs`, `app_routing.rs`, `dialogs/autoroute_settings.rs`,
  `panels/{progress,routing_summary}.rs`, `overlays/routing.rs`).** The toolbar's Autoroute button
  (`Command::StartRouting`) starts the engine's `run_pipeline` on its own thread with the settings of
  Settings > Autorouter and nothing else, so a GUI run and a CLI run with equal settings write the same
  SES (`tests/ui_routing.rs::gui_and_cli_write_identical_ses`). The job routes a copy of the session board
  (`prepare_board_for_routing`: the board half of the CLI's load sequence) and sends `RouterEvent`s over a
  bounded channel; each frame `App::poll_routing` drains it without blocking, merges the frame's deltas
  (an item added and removed in the same frame is dropped), patches the chunks once
  (`Loaded::patch_scene`, `stats.full_rebuilds` stays 0) and keeps the newest `Progress`. The router
  sends a delta after each connection and each fanout pin (`events::flush_journal`; the geometry of the
  added items is in the event, B08.1's `ItemGeometry`), so traces appear while a pass runs. A `Replaced`
  board and the `Finished` board are compared with the drawn scene item by item (`routing::reconcile`),
  so a missed delta can't leave the picture wrong. While a job runs the side panel shows Java's footer
  texts (`panels::progress`: "To route / routed", "Ripped up / failed", "Failed / extra vias" for the
  fanout, the score line) and the router's incomplete and violation counts instead of the stale checks,
  the canvas shows the airline being routed (`overlays::routing`), edits are refused with a status
  message (`Command::edits_board`), and Cancel, Esc or a click on the canvas stop the job (a full stop;
  the best board comes back). When it ends the returned board replaces the session board, and the
  summary window (`panels::routing_summary`: nets, incomplete connections, violations, vias, trace length,
  score, time; "show after routing" is kept) opens. The dialog edits the engine's `SettingsPatch`
  (`Draft`): layers (active, preferred direction, trace and bend costs), the three stages, vias,
  the costs, passes, threads and the job timeout (number and unit, capped at four weeks); a power layer's
  row is disabled and the optimization stage is disabled until O06. `CLI_ONLY` lists every other engine
  setting with its reason, and `every_router_setting_has_a_control` fails when the engine grows one that is in
  neither list. Only the values that differ from the board's own are kept between runs.

- **Saving, the questions and the front-end mode (G08, `files.rs`, `app_files.rs`,
  `dialogs/autostart.rs`, `file_dialog.rs`).** File > Save as... (`Ctrl+S`; Java's File menu has no plain
  Save) opens the file dialog in save mode: a file name field, the filters `Specctra session file (*.ses)`
  and `Specctra design file (*.dsn)`, and a question before an existing file is replaced. The default is
  the last output, else `<design dir>/<design stem>.ses`. A session is written by the CLI's writer (C05,
  `SesStyle::Java`) and then Java's question `Would you like to save the current design rules to a separate
  .rules file?` (default No) writes `<output stem>.rules` (C04); a design is written by C06. Every write
  goes to a temporary file in the target folder and is renamed over it (`job::outputs::write_atomic`), so
  a failed write leaves the old file and an empty result is an error. The session is named after the
  design file (`board.dsn` gives `board.ses`, as Java's Save As does). "Changed" is Java's
  `isBoardChanged`: the edit counter says dirty, and then the CRC32 of the board written as DSN
  (`files::content_hash`) must differ from the one taken at load or at the last save, so an edit that
  was undone is clean. Closing the window with a changed board (or a running job) asks `confirm_cancel`;
  opening or dropping a file over a changed board asks `confirm_save_changes` (Save, Don't Save, Cancel;
  Save runs Save As first, see DIVERGENCES). At start the `<design>.rules` file next to a DSN is offered
  with `confirm_import_rules` (`-dr` reads one without asking); File > Open does not look for it, as in
  Java.
  **Front-end mode** is what the KiCad plugin's "Route in GUI" (K02.5) starts, with Java's flags:
  ```
  freerouting-gui -de board.dsn [board.ses] [board.rules] -do out.ses [-dct seconds] [-dr board.rules] [-mp passes]
  freerouting-gui board.dsn [board.ses]            # as before: no output, no auto-start
  ```
  `-de` files are sorted by extension (`a.dsn+b.ses` is split); `-do` is the output (`.ses` or `.dsn`; the
  first one counts); `-dct` is the countdown in seconds (default 5; 0 or less starts at once, no dialog);
  `--autoroute` is `-dct 0`; `-mp` is the pass limit, the one router flag read. With `-de` and `-do` a
  stale output is deleted at start, the dialog `The Autorouter is about to start` counts down in
  `Start Now (n)` (GUI time; Cancel leaves the app idle and open), the job runs with the settings the
  open board would have, and when it ends, completed or stopped, the output is written. After Start Now (or
  `-dct 0`) the app then exits: 0 after a written file, 1 when the write failed or the design did not
  load. Closing the window stops the job and writes the output if the board changed or has traces,
  without a question. `tools/gui/frontend-smoke.sh <fixture.dsn>` checks all of it under Xvfb in a
  release build (the GUI exits 0, and the CLI imports the session with `-drc` with no import errors).
  Tests: `tests/ui_files.rs`.

Screenshots from the cloud sessions are in the project's shared folder under
`freerouting-rs/gui/`.

## Layout

| File | What it does |
| --- | --- |
| `lib.rs` | The library target (`freerouting_gui`), so integration tests can reach `App`. |
| `main.rs` | Argument collection, window options, the WSL X11 switch (see below). |
| `args.rs` | The command line: files, `--bench`, `--bench-synthetic 4x4 board.dsn`, `--frames`, `--screenshot`, `--exit`, `--control-dir`, and Java's front-end flags `-de`, `-do`, `-dr`, `-dct`, `-mp`, `-di`, `--autoroute` (G08). |
| `app.rs` | `App`: state, `execute(Command)` (the one path for menus, keys and tests), background loading, `save`. `layout.rs` lays out one frame (menu, status bar, side panel, canvas). |
| `commands.rs` | `enum Command`, every user action. |
| `input.rs` | The key table (Java's keys). `viewport.rs` has the mouse model, `ViewState` (camera, visibility, active layer) and the hover status. |
| `session.rs` | `Session` (the engine board as `Arc<RoutingBoard>`, the design, paths, `edit_generation`/`saved_generation`, warnings, SES summary), `load()` and `find_design()`. |
| `scene.rs` | `Scene::from_board(&RoutingBoard)` flattens the engine board into drawable `Item`s (`ItemId`, kind, slot, geometry in mm, net, owner) and `Label`s; `Flattener` does one engine item at a time, for patches. `Scene::pick()` does the hover hit-testing through a grid (`build_index`). |
| `view.rs` | `tessellate_item` triangulates one scene item; `Tessellation` holds the board's chunks (`canvas::chunks::ChunkSet`) behind a mutex that the GPU callback shares. `Camera` handles pan and zoom. `paint()` makes the draw plan and draws it on the GPU path or, as the fallback, with `paint_cpu`; `Renderer` picks the path. Also the colours and the label drawing. |
| `canvas/` | The GPU board canvas (G02): `chunks.rs` (chunks, grid, patching), `gpu.rs` (pipelines, buffers, paint callback), `board.wgsl`, `hairline.wgsl`. See "Renderer" below. |
| `bench_synthetic.rs` | The 4x4 stress board of `--bench-synthetic`. |
| `panels/`, `menu.rs`, `chrome.rs` | The side panel sections (`layers`, `board_stats`) and the warnings window, each a function `ui(ui, ..) -> Vec<Command>`; the menu bar; the look of the menu bar and the toolbar (icons, the coloured buttons, segmented groups, the board chip). |
| `settings.rs` | `PersistedSettings`, stored under one key with a schema version. |
| `analysis.rs`, `lists.rs`, `selection.rs`, `text.rs`, `toolbar.rs`, `drc_export.rs`, `app_analysis.rs` | G03: the background ratsnest and DRC with their result tags; the Info list windows and how their rows are built; `Selection` (a `BTreeSet<ItemId>`); the Java texts; the toolbar; the DRC report export; `App`'s G03 commands (selection, zoom to selection, delete all, undo and redo). |
| `overlays/` | Ratsnest, violation markers and selection highlight, drawn with egui shapes after the canvas. `Frame` converts board coordinates to the millimetres of the scene. `draw` returns what it drew (`stats.overlay`). |
| `panels/{object_list,violations,incompletes,length,unconnected,islands}.rs` | The generic list window and one module per list: its rows (text, items) from the engine results. |
| `look.rs`, `windows.rs` | G04: `Look` (the palette, the visibility levels, the dimming and the six virtual layers; `draw_color` is the one place a colour is decided) and the display windows that are plain flags (visibility, display options, colours, log, About). |
| `panels/{nets,components,library}.rs` | G04: the rows of the Nets, Placed Components, Library Packages and Library Padstacks lists. |
| `panels/{display,colors,status,log,about}.rs`, `panels/layers.rs` | G04: the window contents (each a function that returns commands), the status bar, the log (`Log::entries` composes the loader warnings, the engine's messages and the events) and the visibility sliders. |
| `tools/{host,route,drag,move_component}.rs`, `overlays/route.rs`, `bench_drag.rs` | G09: the editor host (`Editor::apply_on` on the session board, scene patching, status, layer), route mode with pin swap and the interactive keys, drag mode, move and copy of the selection, the editor overlay as egui shapes, and the `--bench drag` script. |
| `tools/{inspect,popup,keepout,edit}.rs`, `tools/mod.rs` | G05: `Tools` (the selection settings, the keepout under construction, the context menu, the shown violations, the item info windows); `App::execute_tool` runs the canvas commands (`CanvasClick`, `SelectRegion`, `ExtendSelection`, `DeleteSelection`, `OpenPopup`, `StartKeepout`, ...). `inspect.rs` has the pick, region and extend rules on the engine's queries; `popup.rs` the menu rows by mode; `keepout.rs` maps clicks onto the engine's `interactive::Construction` (I03.4); `edit.rs` maps Delete, Fix and Unfix onto `interactive::{delete_selection, fix_selection}` (I03.6). |
| `panels/{select_settings,selection_toolbar,item_info}.rs`, `overlays/construction.rs` | G05: Settings > Select ("General Settings", `SelectionSettings`), the toolbar over the canvas while items are selected, the item info window (G04.3), the keepout's rubber band. |
| `routing.rs`, `app_routing.rs`, `dialogs/autoroute_settings.rs`, `panels/{progress,routing_summary}.rs`, `overlays/routing.rs` | G07: the router thread and its events (`RoutingJobHandle`, `FrameEvents`, `GeometryMapper`, `reconcile`), `App`'s start, stop and per-frame poll, the settings dialog (`Draft`, `CONTROLS`, `CLI_ONLY`, `timeout`), the progress and summary panels and the airline overlay. |
| `stats.rs`, `bench.rs`, `capture.rs` | Frame statistics and the `FrameClock`, the `--bench` script, the in-app screenshot. |
| `file_dialog.rs` | A minimal folder browser filtered to `.dsn`/`.ses`/`.kicad_pcb`; in save mode (G08) a file name field, the two filters and the replace question. |
| `files.rs`, `app_files.rs`, `dialogs/autostart.rs` | G08: the writers (Save As, `.rules`, the front-end output, the content checksum), `App`'s side (Save As, the questions, the start-up steps, the front-end mode) and the auto-start countdown. |

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
  egui_kittest tests in `tests/`: `ui_viewer.rs` (loading, the side panel,
  snapshots of the window, the File menu and the toolbar idle and routing), `ui_input.rs` (Java's mouse and keys), `ui_settings.rs`
  (persistence, Open Recent), `ui_windows.rs` (G04: the nets, component and library
  lists, the visibility sliders, virtual layers, hover under rotation and mirroring,
  units, colours, the status bar counters, the log and About; four snapshots), `ui_drc.rs` (G03: selection and zoom, the
  ratsnest and violation counts against `fixtures/reference/load/*.json`, the Info
  lists, delete all with undo, the DRC export against `run_drc`; the harness
  waits for the first analysis, so snapshots show settled counts). The two
  small boards in `tests/data/` (`length_violation.dsn`, `zone_islands.dsn`) exist
  because no fixture has length rules or copper islands.
  `viewer_issue159.png` is the G01 golden that `ui_render.rs` compares the GPU canvas
  with; the full window has its own `viewer_issue159_g03.png`. `tests/common/mod.rs` has the harness helpers.
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
(G02 added keys; see "Renderer".) "Frame CPU" here, as FT0 was first
recorded, is the time on the GUI thread inside `App::update`, which includes
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
saves. G02 did that: see "Renderer" for the re-baselined FT0 with egui's
tessellation and upload included.

### FT7: dragging a component (G09.7)

`freerouting-gui --bench drag --frames 600 board.dsn` runs the drag tool instead of the camera
script: the camera stays on the fitted board, the first pad of a placed component whose drag moves
the component is pressed (chosen on a copy of the board, so the script measures drags the board
accepts), and every frame is one pointer step along `bench::drag_offset` (out to 0.3 mm along x and
back, a period of 120 frames, at most 0.016 mm per frame). Each step is one editor command and one
scene patch from the change journal; the last frame releases. The report adds `full_rebuilds` and
`patches` counted over the counted frames to the usual fields.

**FT7, recorded 2026-10-16** (release build, Xvfb with lavapipe, Issue190, 600 frames, frame CPU
in ms). The machine was not quiet (other sessions were building and routing; 1-minute load average
7 to 12 on 8 cores, the target needs at most 1.0), so these are upper bounds:

| Run | frame p50 | frame p95 | frame max | update p95 | patches | full rebuilds | load 1 min at the end |
|---|---|---|---|---|---|---|---|
| 1 | 3.270 | 5.839 | 9.278 | 5.426 | 598 | 0 | 8.3 |
| 2 | 3.268 | 5.538 | 7.464 | 5.159 | 598 | 0 | 6.7 |

Target: frame CPU p95 at most 8 ms and 0 full rebuilds. Both runs are under 8 ms even on the busy
machine and rebuild nothing (598 patches in 600 frames). Pending: the same run on a quiet machine
(load average at most 1.0) and on the owner's Windows GPU. The checklist box stays partial until then.

### FT4 to FT6: during an autoroute (G07.4, G07.8)

`freerouting-gui --bench route board.dsn` starts the autorouter on the first counted frame and runs until the
job ends, then prints the usual JSON plus `route_ms`, `job_status`, `passes`, the event counts
(`progress_events`, `delta_events`, `items_added`, `replaced`), `reconcile_fixes`, `max_apply_ms` (the slowest
frame's event handling) and the GPU buffer bytes at the first counted frame and the largest since.
`FREEROUTING_BENCH_PASSES=n` limits the passes; `FREEROUTING_BENCH_CANCEL_MS=n` presses Cancel after that long
and reports `cancel_to_finished_ms` (the press to the `Finished` event).

**Recorded 2026-10-16** at `d21d5f1` plus the G07 working tree (release build, Xvfb with lavapipe, Issue190,
`FREEROUTING_BENCH_PASSES=10`). The machine was **not quiet**: another session was running a release test, so
the 1-minute load average was 2.8 to 4.5 on 8 cores (the targets need at most 1.0). Read the numbers as upper
bounds; the box stays partial until a quiet run and the owner's Windows GPU.

| Run | route | passes | frame p50 / p95 / max (ms) | update p95 (ms) | `max_apply_ms` | patches | full rebuilds | GPU buffers first to max | load 1 min |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 5.89 s | 10 | 0.430 / 1.518 / 1.581 | 1.306 | 1.129 | 46 deltas | 0 | 7,797,692 to 7,808,068 B | 2.83 |
| 2 | 5.66 s | 10 | 0.428 / 1.488 / 1.676 | 1.295 | 1.175 | 46 deltas | 0 | 7,797,692 to 7,808,068 B | 2.85 |
| 3 | 5.71 s | 10 | 0.426 / 1.462 / 1.525 | 1.256 | 1.083 | 46 deltas | 0 | 7,797,692 to 7,808,068 B | 4.06 |

Cancel (`FREEROUTING_BENCH_CANCEL_MS`; frame p95 stayed 1.48 to 1.56 ms, 0 full rebuilds):

| Cancel pressed at | pass at the press | Cancel to `Finished` | load 1 min |
|---|---|---|---|
| 0.7 s | 1 | 213.7 ms | 3.87 |
| 1.5 s | 3 | 243.7 ms | 4.52 |
| 3.0 s | 5 | 87.0 ms | 4.48 |

Targets: frame CPU p95 at most 4 ms (met, 1.5 ms), Cancel to `Finished` at most 250 ms (met, 244 ms at most, but
close, and under load: the time is the router finishing the connection it is in, the best-board restore and
the statistics of the returned board), GPU buffers at most 64 MB (7.4 MiB) and growth at most 10 % across the
passes (0.13 % over 10 passes), 0 full rebuilds (met). Issue190 is mostly routed when it loads, so a job adds
only 34 to 35 items (46 delta events, one per connection that changed the board) and a pass is short; the
busier Issue684 test `live_redraw_patches_before_the_job_ends` has 310 deltas and 4 `Replaced` boards. The
longest single connection was not measured: Cancel is checked between connections and inside the maze search
(full stop), and the 244 ms above is the worst Cancel seen.

The owner's Windows GPU: pending (run `freerouting-gui.exe --bench route Issue190-processor.Z80.dsn` with
`FREEROUTING_BENCH_PASSES=10`, and once with `FREEROUTING_BENCH_CANCEL_MS=1500`, and put the numbers here).

A mid-run screenshot under Xvfb (`Issue269-caniot-tiny-arm.dsn`, `--bench route`, about 6 s into the job,
pass 1): the side panel shows "Autorouter is running. Click anywhere on the canvas to cancel. Autorouter pass
#1", "To route: 24; routed: 24", "Ripped up: 68; failed: 5" and the score line "Score: 604.17 (57 incomplete
connections, 0 clearance violations)"; the canvas has the board with the F.Cu traces in red and a mesh of new
B.Cu traces in blue that the earlier shot taken 3.5 s into the job does not have (it shows the fanout pass:
the first vias and short traces); the stale airlines are not drawn, and one yellow airline of the connection
being routed crosses the top right of the board.

## Renderer (G02)

The board is drawn by one wgpu paint callback (`canvas/gpu.rs`); the CPU painter
is the fallback.

- **Chunks.** `canvas/chunks.rs` groups the scene items by (slot, kind), then by
  the cell of a uniform grid over the board (cell size chosen so the median
  occupied cell holds about 1,000 items), and cuts each group into chunks of at
  most 4,096 items. A chunk keeps its items in ascending `ItemId` order, its
  triangles, its hairline segments and the bounding box of its items. Issue190
  has 96 chunks, the 4x4 board 1,514.
- **GPU.** `BoardGpu` (in egui's `callback_resources`) has two pipelines
  (`board.wgsl` for triangles, `hairline.wgsl` for lines), one vertex, index and
  segment buffer per chunk, and a camera uniform. The colour of a draw comes from a
  dynamic-offset uniform table indexed by (slot, kind), rewritten only when the
  active layer changes. Colours are premultiplied gamma values, converted for an
  sRGB target the way egui's shader does, so the board blends like egui's own
  meshes. The vertex shader maps points to clip space exactly as egui's does,
  over the whole screen (the callback widens the viewport; egui's scissor
  rectangle still clips to the canvas), so the board lands on the same pixels as
  the CPU painter's (`cpu_and_gpu_match`: 0.0000 % of the canvas differs on
  Issue159 and Issue690).
- **Hairlines** (board and footprint outlines, zero-width paths) are instanced
  quads expanded in the vertex shader: one point wide at every zoom, square caps.
  A point is `pixels_per_point` pixels, so `hairline_is_one_pixel` finds exactly
  `round(pixels_per_point)` lit rows.
- **Culling.** Each frame `canvas::plan` lists, in draw order, the chunks of the
  visible slots and kinds whose box meets the view (plus a 2-point margin). The
  draw order is the first canvas's: copper layers from the bottom up skipping the
  active one, the active layer, back silk, front silk, board; kinds in
  `Kind::ALL` order; per kind the triangles of all chunks, then the hairlines.
  `chunks_drawn` and `chunks_total` count them (Issue190 at 20x: 15 of 96).
- **Patching.** `Tessellation::patch(removed, added)` (`ChunkSet::patch`) finds
  the chunks that own a removed `ItemId` through a `BTreeMap<i32, Vec<chunk>>`,
  rebuilds just those and the ones that receive an added item (the chunk of its
  group whose `lo_id` range holds the id), and marks them dirty; the next
  `prepare` re-uploads exactly those buffers. A chunk over 8,192 items splits. A
  patched chunk is bit-identical to one built fresh from the same items
  (`patch_equals_fresh_build`). `Scene::item(board, id)` (or
  `Flattener::item(id)` for many) makes the scene items of one engine item (B08's `ItemGeometry` replaces that in G07).
  `Loaded::sync_scene` (G05.5) calls `patch` after every edit, undo and redo;
  `Loaded::rebuild_scene` is the fallback for a journal overflow. G07.4 patches
  from the router's deltas the same way.
- **Fallback.** `FREEROUTING_GUI_RENDERER=cpu`, or no wgpu render state, draws the
  same plan as egui meshes (`view::paint_cpu`; hairlines become quads). Only the
  environment variable is read at start; `ViewState::renderer.force_cpu` does the
  same for tests. `FREEROUTING_GUI_CULL=0` turns the culling off, to measure it.
- **Counters** (`stats.rs`, `FrameStats`): `draws`, `triangles`, `chunks_drawn`,
  `chunks_total`, `uploaded_bytes` (last frame; the callback's `prepare` runs
  after the update that made it, so a frame reports the previous one's
  upload), `uploaded_total`, `gpu_buffer_bytes`, `full_rebuilds`.
  A panning frame uploads 48 bytes (`no_upload_after_first_frame`; 32 before G04.6 added the orientation's basis to the camera uniform).
- **Two frame CPU definitions.** *Update CPU* (G01's, FT0 as first recorded) is the time
  inside `App::update`. *Frame CPU* (FT1 and later) adds what eframe does between
  the update and the board's callback (finishing egui's pass, tessellating its
  shapes, uploading its meshes) and the callback's `prepare` and `paint`;
  `FrameClock` measures it, with an empty marker callback on the CPU path. It still
  excludes queue submission and presentation. `--bench` prints both: `p50_ms`,
  `p95_ms`, `max_ms` are update CPU, `frame_*` are frame CPU (one frame fewer,
  since a frame completes after its update), plus `wall_ms` (mean per frame,
  llvmpipe included), `chunks_drawn`, `chunks_total`, `gpu_buffer_bytes`,
  `first_frame_ms` and `load_ms` (the load phases).
- **Synthetic board.** `--bench-synthetic 4x4 board.dsn` tiles the loaded scene
  4 by 4 with a gap of 10 % of the board size and ids offset per tile (on the load
  thread), then runs `--bench pan-zoom`. Issue190 gives 174,160 engine items
  (12,473 scene items per tile, 199,568 in all) and 5,600,896 triangles.

### Renderer numbers (this machine, lavapipe, 2026-10-15)

Release build under Xvfb, `--bench pan-zoom --frames 600`, three runs each. The
machine was not quiet (other sessions were building; load average 9 to 15 on 8
cores, target is at most 1.0), so read them as upper bounds on the new renderer;
ratios hold because the old and new runs were interleaved.

FT0 re-baselined with the old renderer under the *frame CPU* definition (the old
code plus a marker callback; ms):

| Board | update p50 / p95 (G01 recorded) | frame p50 | frame p95 | frame max |
|---|---|---|---|---|
| Issue190 | 2.6 / 3.1 to 3.6 (quiet-ish); 2.9 / 3.3 to 8.1 (these runs) | 11.9, 7.9, 7.9 | 26.4, 19.5, 9.8 | 51.3, 59.7, 13.5 |
| Issue690 (coldfire) | 0.67 / 0.86 to 0.95 | 3.5, 3.3, 3.3 | 4.9, 4.3, 4.5 | 18.4, 8.3, 7.2 |

The new renderer (frame CPU, ms):

| Board | Run | update p50 / p95 | frame p50 | frame p95 | frame max | draws | load 1 min |
|---|---|---|---|---|---|---|---|
| Issue190 (FT1) | 1 | 0.23 / 0.33 | 0.333 | 0.461 | 3.58 | 55 | 11.2 |
| | 2 | 0.18 / 0.33 | 0.260 | 0.444 | 1.07 | 55 | 13.2 |
| | 3 | 0.21 / 0.34 | 0.299 | 0.468 | 4.59 | 55 | 15.2 |
| Issue690 | 1 | 0.18 / 0.28 | 0.268 | 0.433 | 0.69 | 81 | 11.7 |
| | 2 | 0.18 / 0.29 | 0.270 | 0.436 | 1.11 | 81 | 11.5 |
| | 3 | 0.18 / 0.28 | 0.272 | 0.430 | 0.85 | 81 | 10.3 |
| 4x4 Issue190 (FT2) | 1 | 1.09 / 1.46 | 1.286 | 1.796 | 6.87 | 386 | 15.3 |
| | 2 | 1.07 / 1.30 | 1.254 | 1.550 | 6.18 | 386 | 13.2 |
| | 3 | 1.05 / 1.35 | 1.222 | 1.582 | 5.47 | 386 | 8.8 |

- **FT1** (p95 at most 2 ms and at least 5x better than FT0): met. Frame CPU p95
  is 0.44 to 0.47 ms against 9.8 to 26.4 ms (at least 21x) under the same
  definition; against G01's recorded update p95 (3.07 to 3.57) it is at least
  6.5x.
- **FT2 CPU part** (p95 at most 4 ms): met, 1.55 to 1.80 ms. The 4x4 board has
  `gpu_buffer_bytes` 124,442,080 (118.7 MiB). The wall-clock part (60 fps on the owner's
  GPU) is manual: needs the Windows run. The 1 ms update p50 on this board is the
  hover test and the label loop, not the board.
- **FT3** (Issue190 file to first frame at most 1.5 s, tessellation plus chunking at
  most 200 ms): met. `first_frame_ms` is the time from the start of the load to the
  end of the first board paint (CPU, presentation excluded): 264, 411 and 736 ms in
  the three runs above (load-dependent). Load phases (ms, release, load 12 to 20):

  | Phase | Before (G01 renderer) | After |
  |---|---|---|
  | read | 19 (read plus parse) | 1.7 |
  | parse | | 15.6 |
  | `build_board` | 422 | 169 |
  | scene | 4.1 | 2.2 |
  | tessellation | 6.8 | 4.6 |
  | chunking | | 0.8 |
  | total | 452 | 194 |

  `build_board` is the only phase that matters and it is the engine's. Tessellation
  was never the problem, so nothing was parallelised (no `rayon`). The load runs on
  a background thread, and the first `prepare` uploads the board (7.8 MB for Issue190).
- **FT4 CPU part** (200 removed plus 200 added items, at most 2 ms): `delta_200_200`
  on Issue190, release, median of 20: 0.44 ms; the bytes uploaded are exactly the
  dirty chunks' (64 of 96 chunks, 7.55 MB, because the 200 random items touch most
  of the large trace chunks), plus the camera (32 bytes then, 48 since G04.6). 0 full rebuilds.
- **FT6 static** (at most 64 MB): Issue190 has 7.4 MiB of GPU buffers. The growth
  across routing passes is G07.4's.
- Not done: the Windows run of `freerouting-gui.exe --bench pan-zoom --frames 600`
  on Issue190 and on the 4x4 board (DX12, and `WGPU_BACKEND=gl`). The owner runs it; the
  numbers go here.

## Rules dialogs (G06)

`src/dialogs/`. A dialog reads the board (`RuleView`), shows values in the display unit
(`RuleUnits`: display unit, then the design file's unit, then `CoordinateTransform`), and returns one
`Command::EditRules(RulesEdit)` per user change; `App::execute_rules_edit` runs it with
`dialogs::rules_edit::apply` through B08's facade (`freerouting::rules::edit`) on
`Session::edit`, which counts the edit, so the DRC and the ratsnest run again (the analysis tag
carries `edit_generation`). A refusal goes to the status line (`Command::Notice`, `App::notice`).
Rule edits are not in the board history: `U` restores items only, as in Java.
Menus: Rules > Clearance Matrix, Vias, Net Classes; Settings > Routing.

| Dialog (module) | Java class | Calls (`RulesEdit` variant: B08 call) |
|---|---|---|
| Net Classes (`net_classes`) | `WindowNetClasses` | `AddNetClass`: `add_net_class`; `RemoveNetClass`: `remove_net_class` after the all-nets check; `RenameNetClass`: `rename_net_class`; `SetNetClassViaRule`: `set_net_class_via_rule`; `SetNetClassClearanceClass`: `set_net_class_clearance_class`; `SetNetClassHalfWidth`: `set_net_class_trace_half_width`; `SetNetClassShoveFixed`; `SetNetClassIgnoreCyclesWithAreas`; `SetNetClassMinLength` / `MaxLength`; `SetNetClassIgnoredByAutorouter`; `SetNetClassLayers`: `NetClass::set_active_routing_layer`. Select sends `Select` + `ZoomSelection`; Show Nets and Filter Incomplete Connections are display only |
| Assign Net Class (`assign_net_class`) | `WindowAssignNetClass` | `SetNetClass`: `assign_net_class` |
| Clearance Matrix (`clearance_matrix`) | `WindowClearanceMatrix` | `SetClearance`: `apply_clearance_value`; `AddClearanceClass`: `add_clearance_class`; `MergeClearanceClass` (one confirmed prune step, in `pruneClearanceMatrix` order): `change_clearance_class_index`, `remove_clearance_class`, `clearance_class_removed` |
| Edit Vias (`vias`) | `WindowEditVias` | `AddViaInfo`: `add_via_info` (renamed `new_via_<n>`); `RemoveViaInfo`: `remove_via_info`; `RenameViaInfo`; `SetViaInfoPadstack`; `SetViaInfoClearanceClass`; `SetViaInfoAttachSmd` |
| Via Rules (`via_rules`) | `WindowVia`, `WindowViaRule` | `CreateViaPadstack`: `Padstacks::add` + `add_via_padstack`; `RemoveViaPadstack`: `remove_via_padstack`; `AddViaRule`: `add_via_rule`; `RemoveViaRule`: `remove_via_rule`; `AppendViaToRule`; `RemoveViaFromRule`; `SwapViasInRule`: `swap_vias_in_rule` (Move Up / Down) |
| Routing Settings, board rules (`route_settings`) | `WindowRouteParameter` | `SetSnapAngle`: `set_trace_angle_restriction` (asks first when traces would not fit); `SetIgnoreConduction`: `change_plane_as_obstacle(!v)`; `SetPinEdgeToTurnDist`: `set_pin_edge_to_turn_dist`, with Java's unfixing of shove-fixed two-corner pin traces (done in `rules_edit`, no B08 facade); `SetClearanceCompensation`: `set_clearance_compensation_used`; `SetKeepoutOutsideOutline`: `generate_keepout_outside_outline` |

`input` has Java's static helpers (`parseTraceWidthValue`, `applyTraceWidth`, `isLegalClassName`,
`parseClearanceTableValue`, `canRemoveNetClass`, the autorouter number and timeout normalizers) and
`autoroute_handlers` the autorouter checkbox handlers, for the G07 dialog to call.
`route_settings::static_section` is where G09.2 adds the interactive fields. Tests:
`tests/ui_rules.rs` (drives each dialog through egui_kittest, reads the engine board back) and the
unit tests of `dialogs/input.rs`. Lessons: `ScrollArea::both()` swallows clicks of the cells under
it (use `vertical`, or `scroll_source` without `drag`); a click needs a second frame to take
effect, so the tests step at least twice; a drop-down is found by its accessible name
(`name_drop_down`). `docs/DIVERGENCES.md` has the "Rules dialogs" list.

## Opening `.kicad_pcb` (K07.2)

File > Open, drag and drop and the command line accept a `.kicad_pcb`. `session::load` reads it through
`freerouting::io::kicad::read_kicad_input` (the direct reader of `crates/freerouting-kicad`, with the
`.kicad_pro` and `.kicad_dru` next to it) and builds the board from the tree like a DSN's; the reader's
warnings (`wire_keepout` areas, kept footprint zones, renamed references) and its read report (rule areas
with their flags and the keepouts emitted, planes, arcs followed, `.kicad_dru` rules and their outcome) go
into `Session::warnings` as "KiCad reader: ..." and "KiCad read report: ..." lines, for the warnings window.
`find_design` also looks for `<base_design>.kicad_pcb` when a `.ses` is opened alone. The file browser lists
`.kicad_pcb`. Tests: `tests/ui_kicad.rs` (Issue368 opens, its item count equals `build_board` of the reader's
tree) and `file_dialog::tests::lists_kicad_boards`. Writing a `.kicad_pcb` is K08.

## What's next

In plan order. Each item needs engine work first, so check what has landed in
`crates/freerouting` before starting one.

1. **Ratsnest and DRC panel:** done in G03 (see "What works"). Open: Route
   Stubs waits for B08.3. G04 (windows, display options, colours, status bar) is
   done, including the item info window.
2. **Selection by mouse, safe edits and keepouts:** done in G05 (see "What works").
   The Hole keepout and the engine forms of keepouts, Delete, Fix and Unfix landed with
   I03 (2026-10-16). Route, drag, pin swap, move and undo run on `interactive::Editor::apply`
   since G09; the G05 tools (select, keepout, Delete, Fix) still call the engine's I03 functions
   directly. Delete still takes the push-off path (plain removal).
3. **Autorouter run:** done in G07 (see "What works"). Open: the FT4 to FT6 numbers on a quiet machine
   and on the owner's Windows GPU (see "FT4 to FT6: during an autoroute"), routing a selection (G07.7, an operator
   decision) and the optimizer's cross and stage (O06).
4. **Parameter dialogs:** the autorouter's (G07) and the rules dialogs (G06) are done; the remaining
   Java windows are listed in G08.
5. **Interactive routing and editing:** done in G09 (see "What works"). Open: the
   FT7 numbers on a quiet machine and on the owner's Windows GPU (see "Frame numbers").

Other gaps:

- Saving: done in G08 (SES, DSN, `.rules`). Not offered, by decision (G00 question 2): `.frb`, the
  Autodesk Fusion `.scr` and the KiCad session JSON.
- Screen readers: eframe's `accesskit` feature is on, so the widgets are in
  the accessibility tree (egui_kittest finds them by label through it). Nobody
  has tried a screen reader on it.
- Performance: done in G02 (see "Renderer"). The vertices stay on the GPU;
  a frame uploads the 48-byte camera.
- Widget style: the panels use egui's default dark style. The owner asked about
  native Windows looks, which turned out to be about the title bar only (now
  fixed). A Windows-like light theme is an open offer, not a request.
- The menu bar and the toolbar have a look of their own (`chrome.rs`, asked
  for by the owner on 2026-10-17): the app's icon at the left, an icon column and
  right-aligned shortcuts in the File and View menus, a green Autoroute (red
  Cancel while routing), Mode and Unit as segmented groups, toggles filled while
  on, a red Delete All icon, and the open board in a chip at the right. The icons
  are glyphs of egui's default fonts painted into space the button keeps
  (`Atom::custom`), so the accessible labels the tests look up stay the texts;
  pick a new glyph from those fonts (check the font's charset) or it shows as a box.

## Working rules

- The owner is the sole developer. Commit and push straight to main, with no PRs.
  Pull with `--rebase` before pushing. Keep fmt, clippy and tests green.
- The GUI work owns `crates/freerouting-gui`. Engine changes go into
  `crates/freerouting` in coordination with the engine work; don't change
  engine behaviour from the GUI side.
- Deliberate differences from Java go in `docs/DIVERGENCES.md`. The GUI has
  none so far; it is rebuilt with egui rather than translated from Swing.
