# G01 — App session, crate structure and test harness

Opened 2026-10-15. The app builds the engine board on load, flattens it into a
`Scene`, and then drops it (`app.rs::load`). Ratsnest, DRC, item info,
editing, saving and routing all need the board, so every later G checklist
starts from a `Session` that keeps it. The crate is also binary-only, so
integration tests can't reach `App`, and nothing measures frame time. This
checklist adds the session, a library target, the egui_kittest harness, the
frame statistics and bench mode that later acceptance clauses use, and the
Java input model. It is the report's GUI-1.

**Prerequisites:** none. G02–G09 build on it.

## What exists and what this adds

| Exists (Rust at `70389b7`, Java at `cda540f`) | State |
|---|---|
| `crates/freerouting-gui/src/main.rs` (47 lines) | Window options; WSL forces X11 (`window_system`). `mod` declarations; no `lib.rs` |
| `src/app.rs` (577 lines) | `App` (menus, side panel, canvas, input), `Loaded { pcb, design_path, session_path, scene, tess }`. `load()` builds the board with `build_board`, calls `Scene::from_board(&pcb.name, board)` and lets the board go out of scope. Loading runs on a background thread |
| `src/scene.rs` (911 lines) | `Scene::from_board`, `Scene::pick`, `apply_session` (merges a SES into the DSN syntax tree, with its own coordinate rescaling), unit tests |
| `src/view.rs` (479 lines) | `Tessellation`, `Camera`, `paint` (per-frame CPU transform). Test `tessellates_fixture_boards` covers fixtures under 50 KB |
| `crates/freerouting-gui/Cargo.toml` | eframe 0.32 with `default-features = false` and `default_fonts, wgpu, wayland, x11`: no `persistence`, no `accesskit`. `Cargo.lock` has egui/eframe/egui-wgpu 0.32.3 and wgpu 25.0.2 |
| Engine | `freerouting::io::specctra::{build_board, import_session, read_session}`; `BoardReadResult::{Success { board: Box<RoutingBoard>, .. }, OutlineMissing { board: Option<Box<..>>, .. }, ParseError}`; `RoutingBoard: Clone + Deref/DerefMut<Target = BasicBoard>`; `BasicBoard::revision()` counts inserts and removes only; `freerouting::scoring::BoardStatistics::new` |
| Java input model | `gui/board/BoardPanel.java` javadoc: left button selects and acts, middle button pans, right button opens popups. Keys: `gui/interactive/InteractiveState.java` `keyTyped` (a, c, f, o, z, `,`, Enter/Space), `gui/interactive/MenuState.java` (b, d, e, g, i, p, r, s, t, u, v, w, `+`, `-`) |
| Java persistence | `gui/support/GuiDefaultsFile.java` (window layout, `gui_defaults.par`), `settings/GuiApplicationSettings.java` |
| Tools on this machine | `xvfb-run`, `Xvfb`, `xdotool`, ImageMagick `import` and `compare`; lavapipe (`/usr/share/vulkan/icd.d/lvp_icd.json`); `egui_kittest` 0.32.3 in the local cargo registry |

What this adds: `lib.rs`; `Session`; SES loading through the engine; a
command enum and one key map; `tests/` with egui_kittest; `--bench`,
`--screenshot`, `--exit` and the stats overlay; `tools/gui/smoke.sh`; Java's
mouse and key model; persisted settings; AccessKit.

## Decisions made here

- **`Session` holds `Arc<RoutingBoard>`.** Background jobs (DRC, ratsnest,
  later routing) take an `Arc` clone, which costs nothing. An edit goes
  through `Arc::make_mut`, which copies the board only while a job still
  holds the old one. No lock is shared between threads.
- **Dirty tracking is the session's, not the board's.** `revision()` counts
  inserts and removes only, so it misses rule edits and fixed-state
  changes. `Session::edit_generation` is bumped by every command that
  mutates the session; `is_dirty()` compares it with `saved_generation`.
  Job results carry the `(revision, edit_generation)` they were computed
  on, and stale results are dropped. (`delete_all_traces_and_vias` doesn't
  bump `revision()` at all, so the generation is what catches it.) Java
  decides "changed" by comparing a checksum of the board written as DSN,
  so undoing back to the saved state counts as unchanged; G08.2 switches
  `is_dirty()` to B01's `state_hash` for that.
- **One command path.** `enum Command` in `src/commands.rs`; menus, the
  toolbar, keys and tests all call `App::execute(Command)`. Tests then drive
  the app the way a user does without pixel coordinates.
- **SES loading follows Java.** Java's GUI reads the session on top of the
  design board with `SesReader.read` (`gui/board/GuiManager.java`, around
  line 401) and keeps the design's own wiring. The engine's
  `import_session` is the port of that, already checked against Java in
  `tests/reference_drc.rs`. The AST merge (`scene::apply_session`), which
  replaces the design's wiring, goes.
- **egui_kittest pinned to `=0.32.3`**, the egui version in `Cargo.lock`,
  with features `eframe, wgpu, snapshot`. Upgrading egui upgrades kittest in
  the same commit and regenerates the snapshots.
- **Rendering tests run on Linux** with lavapipe (`WGPU_BACKEND=vulkan`).
  On Linux a missing Vulkan adapter fails the test with a message naming
  `mesa-vulkan-drivers`. On Windows and macOS rendering tests are skipped
  unless `FREEROUTING_GUI_SNAPSHOTS=1`; interaction tests run everywhere.
- **Screenshots come from the app itself** (`ViewportCommand::Screenshot`,
  answered by `egui::Event::Screenshot`), written as PNG. That avoids X
  server races and the 125 % DPI capture problem on Windows
  (`kicad-plugin.md`). PNG encoding needs the `png` crate (`NEW DEP`).
- **Persisted settings carry a schema version** and restore defensively:
  unknown keys and values this version doesn't know fall back to defaults,
  never to a panic. That is KRT's lesson (`kicad_routing_plugin/settings_persistence.py`):
  a removed control once crashed on close and lost the user's settings.

## File ownership

Paths are under `crates/freerouting-gui/`.

| Item | New files | May edit |
|---|---|---|
| G01.1 | `src/lib.rs`, `src/commands.rs`, `src/input.rs`, `src/panels/{mod,layers,board_stats,warnings}.rs` | `src/main.rs`, `src/app.rs` |
| G01.2 | `src/session.rs` | `src/app.rs`, `src/scene.rs` |
| G01.3 | — | `src/session.rs`, `src/scene.rs`, `docs/development/gui.md` ("Decisions") |
| G01.4 | `tests/common/mod.rs`, `tests/ui_viewer.rs`, `tests/snapshots/` | `Cargo.toml` (dev-dependencies), `.gitignore` |
| G01.5 | `src/stats.rs` | `src/main.rs`, `src/app.rs`, `Cargo.toml`, `docs/development/gui.md` |
| G01.6 | `tools/gui/smoke.sh`, `tools/gui/golden/*.png` | `docs/development/gui.md` ("Running and testing") |
| G01.7 | `tests/ui_input.rs` | `src/input.rs`, `src/app.rs`, `src/view.rs` (`Camera` only) |
| G01.8 | `src/settings.rs`, `tests/ui_settings.rs` | `Cargo.toml` (eframe features), `src/app.rs` |

## Checklist

- [x] **G01.1 — Library target and module split.**
  - `src/lib.rs` declares the modules and re-exports `App`. `src/main.rs`
    keeps only argument collection, the window options and the WSL
    `window_system` switch.
  - Split `app.rs`: the side panel into `src/panels/{layers,board_stats,warnings}.rs`,
    each a `fn ui(&mut self, ui: &mut egui::Ui, …)` so `Harness::new_ui_state`
    can test a panel alone; menu and key actions into `Command` variants
    (`Open(PathBuf)`, `FitBoard`, `SetActiveLayer(usize)`,
    `ToggleLayer(usize)`, `ToggleKind(Kind)`, `Quit`); the key table into
    `src/input.rs`, unchanged in behaviour for now.
  - **Accept:**
    - `cargo test -p freerouting-gui` passes with the existing tests
      unchanged.
    - `main.rs` ≤ 60 lines; `app.rs` ≤ 300 lines.
    - Xvfb screenshots of `fixtures/Issue690-kit-dev-coldfire-xilinx_5213.dsn`
      before and after the split (implementer recipe, 1280×800) differ in
      at most 1 % of the pixels by `compare -metric AE -fuzz 10%`. The
      implementer opens both PNGs and says what they show.
  *Done 2026-10-15:* `lib.rs` (library target `freerouting_gui`), `main.rs`
  60 lines, `app.rs` 285 lines (`wc -l`); the side panel is in
  `src/panels/{layers,board_stats,warnings}.rs` (each `ui(ui, ..) -> Vec<Command>`),
  `commands.rs` has `Command` (`Open`, `FitBoard`, `SetActiveLayer`,
  `ToggleLayer`, `ToggleKind`, `Quit`, plus the view commands G01.7 needs),
  `input.rs` the key table, `layout.rs` and `menu.rs` the frame layout.
  `cargo test -p freerouting-gui`: all the existing tests pass unchanged except
  `scene::tests::session_placement_and_routes_replace_the_design`, which went
  with `apply_session` in G01.3 (the item's own plan). Xvfb 1280x800 screenshots of
  `Issue690-kit-dev-coldfire-xilinx_5213` from the HEAD binary (before) and
  the final binary (after): `compare -metric AE -fuzz 10%` = 0 differing
  pixels of 1,024,000 (0 %); right after the split alone it was 104 pixels
  (0.01 %). Both PNGs show the same thing: the board fitted in the canvas with
  its 4 layers (Top_layer active), the layer and "Show" lists and the Board
  stats (157.5 x 91.4 mm, 160 components, 278 nets, 2674 trace segments,
  184,222 triangles) in the left panel, and the hover text for a trace on
  `B.Cu` in the status bar. (An interim change that let the side panel fill
  its width moved the panel edge by 6 px and gave 24 % differing pixels; the
  default width is now 203 px, which is the width the panel had when it
  followed its contents.)

- [x] **G01.2 — The session owns the board.**
  - `src/session.rs`: `Session { board: Arc<RoutingBoard>, design: Arc<Pcb>,
    paths: SessionPaths { design: PathBuf, session: Option<PathBuf>, rules:
    Option<PathBuf> }, edit_generation: u64, saved_generation: u64,
    warnings: Vec<String> }`, with `is_dirty()` and `board_mut()`
    (`Arc::make_mut` plus a generation bump).
  - `load()` returns `Loaded { session, scene, tess }`. The board comes out
    of `BoardReadResult` by value (match on `Success { board, .. }`), with no
    clone. `Scene::from_board` reads `session.board`.
  - **Accept:**
    - `session::tests::keeps_board_issue754` loads
      `fixtures/Issue754-avionics_hub.dsn` and checks
      `BoardStatistics::new(&session.board).items.total_count == 2384` and
      `connections.incomplete_count == 221` (values from
      `fixtures/reference/load/Issue754-avionics_hub.json`, named in a
      comment).
    - `session::tests::make_mut_copies_only_when_shared`: with a second
      `Arc` alive, `board_mut()` leaves the clone's `item_count()`
      unchanged after an edit; without one, `Arc::ptr_eq` holds before and
      after.
  *Done 2026-10-15:* `src/session.rs`: `Session { board: Arc<RoutingBoard>,
  design: Arc<Pcb>, paths: SessionPaths, edit_generation, saved_generation,
  warnings, ses_summary }`, `is_dirty()`, `board_mut()`; `load()` returns
  `Loaded { session, scene, tess }` and takes the board out of
  `BoardReadResult::Success { board, .. }` by value (no clone).
  `session::tests::keeps_board_issue754` (items 2384, incomplete 221, values
  from `fixtures/reference/load/Issue754-avionics_hub.json`) and
  `make_mut_copies_only_when_shared` (a held `Arc` keeps its `item_count()`
  through `delete_all_traces_and_vias`; unshared, `Arc::as_ptr` is the same
  before and after) pass in `cargo test -p freerouting-gui`.

- [x] **G01.3 — Sessions through the engine.**
  - Opening a `.ses` (with its `.dsn`, found as today by `find_design`)
    builds the board from the DSN, then calls
    `freerouting::io::specctra::import_session(&mut board, &session)`.
    Show `SesImportSummary` (wires, vias, errors) in the warnings window
    when `errors_encountered > 0`.
  - Delete `scene::apply_session` and its rescaling code and tests once the
    comparison below is recorded.
  - Update `gui.md` "Decisions worth knowing": the engine imports sessions,
    on top of the design's wiring, as Java's GUI does.
  - `find_design` strips a trailing `.dsn` from `base_design` before adding
    one: `Issue368-CorneyIslandWireless_output_session.ses` and
    `Issue733-kicad_complex_hierarchy_output_session.ses` name their design
    with the extension, so today it looks for `….dsn.dsn`.
  - **Accept:**
    - `session::tests::sessions_match_engine_path`: for each Java-routed
      session `fixtures/reference/route/<board>.ses` whose
      `fixtures/<board>.dsn` is under 50 KB (the release-only variant,
      `#[ignore]`, covers all of them), opening the DSN and the SES gives a
      board whose `BoardStatistics` (traces, vias, incomplete connections)
      equals `build_board` followed by `read_session` on the same pair.
      That is the path `tests/reference_drc.rs` compares with Java.
    - `session::tests::finds_design_by_base_design`: opening each of the 14
      top-level `fixtures/*.ses` alone finds a design for at least the
      ones whose `base_design` (with or without `.dsn`) or stem names an
      existing `fixtures/*.dsn`; the test lists the expected design per
      session.
    - The done note lists, for the 14 top-level sessions, the trace and via
      counts of the old AST-merge path against the new path, and explains
      each difference (expected: the design's wiring is kept under the
      session's).
  *Done 2026-10-15:* a `.ses` is opened by `build_board` plus
  `import_session`; `SesImportSummary` is kept in `Session::ses_summary` and
  the warnings window leads with a line when `errors_encountered > 0`.
  `scene::apply_session` and its rescaling code and test are deleted.
  `find_design` strips a trailing `.dsn`. `gui.md` "Decisions worth knowing"
  updated. Tests: `session::tests::sessions_match_engine_path` (small boards,
  5+ compared) and the `#[ignore]` release variant
  (`cargo test --release -p freerouting-gui --lib -- --include-ignored`: both
  pass, 55 s; it compares every `fixtures/reference/route/*.ses` whose DSN
  loads, so 50+ boards; `Issue006-LPC18XX_43XX_SCH.dsn` is an OLE2 document,
  not DSN text, and is skipped when the engine's own read also fails);
  `finds_design_by_base_design` lists the expected design for all 14 top-level
  sessions: 10 find one (Issue368 only after the `.dsn` fix, to
  `Issue368-CorneyIslandWireless_input_design.dsn`), 4 have none
  (`bm07-v190`, `bm08-routed`, `Issue555-...-v190`, and Issue733, whose
  base_design names `..._input_board.dsn`, which is not in `fixtures/`).
  Old AST-merge path against the new engine path (traces, vias, incomplete
  connections after the load; "design" is the DSN alone):

  | Session | Design wiring | Old merge | New (engine) | wires/vias imported |
  |---|---|---|---|---|
  | Issue026-J2_reference | 0 / 0 / 33 | 54 / 10 / 13 | 89 / 10 / 13 | 89 / 10 |
  | Issue313-FastTest | 127 / 20 / 3 | 129 / 21 / 3 | 256 / 41 / 3 | 129 / 21 |
  | Issue368 (output_session) | none found (old) | none found | (new finds the design) | |
  | Issue508-DAC2020_bm05 | 0 / 0 / 107 | 146 / 31 / 30 | 198 / 31 / 30 | 198 / 31 |
  | Issue508-DAC2020_bm07-current | 0 / 0 / 86 | 139 / 40 / 2 | 188 / 40 / 2 | 188 / 40 |
  | Issue555-BBD_Mars-64-current | 0 / 0 / 195 | 206 / 92 / 113 | 285 / 92 / 113 | 285 / 92 |
  | Issue593-BBD_Mars-64 | 0 / 0 / 106 | 120 / 51 / 47 | 215 / 51 / 47 | 215 / 51 |
  | Issue690-ecc83 | 55 / 0 / 10 | 61 / 0 / 6 | 116 / 0 / 6 | 61 / 0 |
  | Issue742-tastexx-pcb | 0 / 0 / 15 | 20 / 7 / 3 | 37 / 7 / 9 | 37 / 7 |
  | Issue951-corney_island_wireless | 299 / 42 / 145 | 510 / 79 / 31 | 900 / 89 / 61 | 601 / 47 (32 errors) |
  | bm07-v190, bm08-routed, Mars-v190, Issue733 | no design found by either path | | | |

  Differences: (a) where the DSN already has wiring (Issue313, Issue690-ecc83,
  Issue951) the design's traces stay under the session's, so new = design +
  imported exactly (127+129 = 256 traces and 20+21 = 41 vias for Issue313;
  55+61 = 116 for Issue690; 299+601 = 900 and 42+47 = 89 for Issue951), where
  the old path replaced the design's wiring. (b) Where the DSN has none
  (Issue026, bm05, bm07, Mars, Issue593, Issue742), the new trace count is
  the imported wire count (each session wire is one trace, the way Java's
  `SesReader` inserts them), while the old path fed the wires through the DSN
  reader, which ended with fewer traces (e.g. 89 against 54). I did not
  trace why the reader gives fewer (it merges or drops pieces); the new path
  is the one `tests/reference_drc.rs` compares with Java, so it is the
  reference. (c) Issue742 (3 incomplete in the old path, 9 in the new) and
  Issue951 (31 against 61, and 32 import errors: nets or wires the engine
  could not place) differ in connectivity, not only in counts. The 32 errors
  are what the new warnings line shows. I did not investigate whether the
  session in Issue951 was written for another version of the design.

- [x] **G01.4 — egui_kittest harness.**
  - `Cargo.toml` dev-dependency `egui_kittest = { version = "=0.32.3",
    features = ["eframe", "wgpu", "snapshot"] }` (`NEW DEP`).
  - `tests/common/mod.rs`: `fn app_harness(files: &[&str]) ->
    Harness<'static, App>` built with
    `Harness::builder().with_size([1280.0, 800.0]).wgpu().build_eframe(|cc|
    App::new(cc, paths))`, then `try_run_realtime()` until the background
    load finishes; `fn rendering_enabled() -> bool` implementing the
    platform rule in the decisions.
  - `.gitignore`: `crates/freerouting-gui/tests/snapshots/*.new.png` and
    `*.diff.png`.
  - **Accept:** `tests/ui_viewer.rs`:
    - `loads_issue159`: `fixtures/Issue159-setonix_2hp-pcb.dsn` (24 KB, fast
      in debug) loads, and `get_by_label("Traces")` finds the kind checkbox.
    - `kind_checkbox_toggles_state`: clicking "Vias" flips the app's
      visibility for `Kind::Via`.
    - `snapshot_issue159`: `harness.snapshot("viewer_issue159")` passes
      against the committed PNG (`OsThreshold` default; Linux only by the
      rule above). The implementer opens the PNG and describes it.
    - `UPDATE_SNAPSHOTS=1 cargo test -p freerouting-gui` regenerates it.
  *Done 2026-10-15:* `egui_kittest = { version = "=0.32.3", features =
  ["eframe", "wgpu", "snapshot"] }` as a dev-dependency (NEW DEP);
  `tests/common/mod.rs` (`app_harness`, `plain_harness`, `rendering_enabled`,
  `wait_for_load`); `.gitignore` has the `*.new.png`/`*.diff.png` lines.
  `tests/ui_viewer.rs`: `loads_issue159`, `kind_checkbox_toggles_state`,
  `snapshot_issue159` pass; run twice without `UPDATE_SNAPSHOTS` after the
  PNG was committed. `UPDATE_SNAPSHOTS=1 cargo test -p freerouting-gui --test
  ui_viewer` regenerates `tests/snapshots/viewer_issue159.png`. The PNG
  (1280x800) shows the narrow Issue159 board (9.6 x 108.5 mm) fitted upright in
  the canvas with its 15 footprints and pads, labels D1-D8 and J1-J4, the
  layer list (F.Cu active, B.Cu), the Show checkboxes and the Board stats
  (15 components, 9 nets, 0 trace segments, 1,132 triangles) in the left
  panel, and an empty status bar. The `libEGL`/`ZINK` warnings
  that appear on this machine come from wgpu probing the GL backend; the
  harness picks the lavapipe Vulkan adapter.

- [x] **G01.5 — Frame statistics, bench mode and in-app screenshots (records FT0).**
  - `src/stats.rs`: `FrameStats`, a ring of the last 600 frame CPU times as
    defined in G00 ("Frame targets"), plus counters `triangles`, `draws`,
    `chunks_drawn`, `uploaded_bytes`, `full_rebuilds`. Later checklists add
    their counters here. `FREEROUTING_GUI_STATS=1` shows an overlay and logs
    p50, p95 and max every 120 frames.
  - `src/main.rs` flags: `--bench <script> --frames N` runs a scripted
    camera path, prints one JSON object
    `{"script","frames","p50_ms","p95_ms","max_ms","triangles","draws","uploaded_bytes","load_avg_1m"}`
    on stdout and exits 0. Script `pan-zoom`: fit; 10 zoom-in steps of
    ×1.25 at the board centre; pans of 20 % of the view width right, down,
    left, up; 10 zoom-out steps; repeat until N frames. The first 30 frames
    are warm-up and not counted. `--screenshot <png>` saves the first frame
    after the load finishes; `--exit` quits after it.
  - **Accept:**
    - `xvfb-run -a target/release/freerouting-gui --bench pan-zoom --frames
      600 fixtures/Issue190-processor.Z80.dsn` prints the JSON and exits 0;
      `triangles` is the tessellation's count (350,056 by the research
      pass; record what it prints).
    - FT0 recorded in `gui.md`: the JSON for Issue190 and
      `Issue690-kit-dev-coldfire-xilinx_5213`, three runs each, release,
      with the load average.
    - `--screenshot` writes a PNG of Issue690 under `xvfb-run`; the
      implementer opens it and confirms the board is fitted and drawn.
  *Done 2026-10-15:* `src/stats.rs` (`FrameStats` ring of 600, counters,
  `FREEROUTING_GUI_STATS=1` overlay and a log line every 120 frames),
  `src/bench.rs` (the `pan-zoom` script, 30 warm-up frames, JSON report),
  `src/capture.rs` (`--screenshot`, `--exit`, in-app screenshot written with
  the `png` crate, NEW DEP: `png` 0.18, already in `Cargo.lock` through egui),
  `src/args.rs`. `xvfb-run -a ... --bench pan-zoom --frames 600
  fixtures/Issue190-processor.Z80.dsn` prints the JSON and exits 0:
  `triangles` 350,056 (as in the research pass). FT0 is in `gui.md` ("Frame
  numbers"): Issue190 p50 2.6-2.8 ms, p95 3.1-3.6 ms; Issue690 p50
  0.67-0.74 ms, p95 0.86-0.95 ms; three runs each with the load average, which
  was 5-8, not quiet (other sessions were running). `--screenshot` of
  Issue690 under Xvfb wrote a 1280x800 PNG that I opened: the board is
  fitted, all four layers drawn, labels on the footprints, the layer panel
  and stats on the left. Unit tests: `bench::tests`, `stats::tests`,
  `args::tests`, `capture::tests::writes_a_png`. The `draws` counter and
  `uploaded_bytes` come from `view::paint`, which now returns a `PaintStats`
  (additive).

- [x] **G01.6 — Xvfb smoke script.**
  - `tools/gui/smoke.sh <fixture> [--keys "<xdotool keys>"]`: runs the
    release binary under `xvfb-run -a -s '-screen 0 1280x800x24'` with
    `WGPU_BACKEND=vulkan`, sends the keys with `xdotool` after the window
    appears (`xdotool search --sync --onlyvisible --name .`), takes the
    in-app screenshot, and compares it with `tools/gui/golden/<fixture>.png`
    by `compare -metric AE -fuzz 10%`. It never uses `sleep` to wait for the
    X server. `WGPU_BACKEND=gl` under Xvfb panicked with "Invalid surface" in
    the research pass, so the script refuses it.
  - **Accept:**
    - `tools/gui/smoke.sh fixtures/Issue690-kit-dev-coldfire-xilinx_5213.dsn`
      exits 0, with AE ≤ 1 % of the pixels against the golden.
    - An edited golden (one layer hidden) makes it exit 1 and print the
      AE count.
    - Documented in `gui.md` "Running and testing", replacing the
      `sleep 10` recipe.
  *Done 2026-10-15:* `tools/gui/smoke.sh <fixture> [--keys "..."]` with
  the golden `tools/gui/golden/Issue690-kit-dev-coldfire-xilinx_5213.png`.
  `tools/gui/smoke.sh fixtures/Issue690-kit-dev-coldfire-xilinx_5213.dsn`
  exits 0 with `AE=0 of 1024000 pixels (limit 10240)`, three runs in a row (about 1.2 s
  each). An edited golden (In1.Cu hidden by a click, made with a scratch
  copy of the script) makes it exit 1 and print `AE=355356 of 1024000 pixels
  (limit 10240)`; `--keys "z"` changes the picture too (AE 612,689; the
  screenshot is the board zoomed x2 about the canvas centre, which I opened).
  The script never sleeps for a fixed time: it waits for the window with
  `xdotool search --sync`, for the board with a marker file the app writes
  (`--control-dir`), and polls with `timeout 0.1 tail -f /dev/null`. It
  refuses `WGPU_BACKEND=gl`. `xdotool key --window` (synthetic events) is
  ignored by winit and winit drops keys while it has no focus, so the script
  runs `xdotool windowfocus` first. `gui.md` "Running and testing" replaces
  the `sleep 10` recipe.

- [x] **G01.7 — Java mouse and key model.**
  *Operator decision:* G00 question 1. Implement the recommendation; the item
  stays partial until Dan confirms or changes it.
  - Mouse: the left button is reserved for tools (inspect and region select
    come in G05); middle-drag and Space+left-drag pan; right-click opens the
    context menu (G05.6); the wheel zooms at the cursor; pinch still zooms.
  - Keys (Java's set): A zoom all, Z zoom in and O zoom out at the cursor, C
    centre on the cursor, F zoom to a dragged rectangle, Home fit, digits and
    `+`/`-` change the active layer, `,` toggles the crosshair. G, V, U, B,
    S, R, D and W are reserved in `input.rs` for G03, G05 and G09.
  - Update the View menu labels (`Fit board  (Home)`).
  - **Accept:** `tests/ui_input.rs` (non-rendering, all OSes):
    - `middle_drag_pans`: pushing `PointerButton { button: Middle, pressed:
      true }`, `PointerMoved` by 100 px and the release moves the camera
      centre by 100 px in board units at the current zoom.
    - `left_drag_does_not_pan`: the same with `Primary` leaves the camera
      unchanged.
    - `keys_a_z_o_c`: each key changes the camera as specified (A equals the
      fitted camera; Z then O returns to the start zoom within 1e-6).
    - `f_zooms_to_rectangle`: F, then a left-drag from (100, 100) to
      (300, 250), gives a view that contains that rectangle's board area.
  *Done 2026-10-15:* Dan accepted G00 question 1's recommendation
  (2026-10-15), which is what was implemented. `tests/ui_input.rs` (no renderer, all OSes): `middle_drag_pans`,
  `left_drag_does_not_pan`, `keys_a_z_o_c`, `f_zooms_to_rectangle`, plus
  `space_and_left_drag_pans`, `a_left_drag_without_f_leaves_the_view_alone_and_escape_cancels_f`,
  `reserved_keys_do_nothing_yet`, `digits_and_plus_minus_change_the_active_layer`:
  8 pass. Mouse: middle-drag and Space+left-drag pan, the left button only
  serves `F`'s rectangle for now, the wheel and pinch zoom at the cursor (the
  right button is untouched until G05.6). Keys: A and Home fit, Z and O zoom
  x2 and /2 at the cursor (Java's `c_zoom_factor`), C centres, F arms a
  zoom rectangle, Esc cancels it, `,` toggles a crosshair, digits and `+`/`-`
  change the layer; G V U B S R D W are listed in `input::RESERVED`. View menu
  labels now read `Fit board  (Home)` and so on. Differences from Java kept
  for now: the digits select the layer with that index, as before (Java's
  select the n-th signal layer); `+`/`-` do step over plane layers like Java.
  Ctrl+Z/Ctrl+Y aliases are G05's, which adds undo.

- [x] **G01.8 — Persisted settings and AccessKit.**
  - eframe features `persistence` and `accesskit` on. `src/settings.rs`:
    `PersistedSettings { schema: u32, layer_visible_by_name:
    BTreeMap<String, bool>, kind_visible, show_labels, last_dir:
    Option<PathBuf>, recent_files: Vec<PathBuf> (at most 10, newest first),
    side_panel_width }`, all fields `#[serde(default)]`. `App::save` writes
    it under one storage key; `App::new` restores it.
  - File > Open Recent lists the recent files; a missing file is shown
    disabled, not removed silently.
  - Window size comes from eframe's own persistence.
  - **Accept:** `tests/ui_settings.rs`:
    - `settings_round_trip`: change visibility, open two files, call
      `App::save` into an in-memory `eframe::Storage`, build a fresh `App`
      from it, and the restored `PersistedSettings` are equal.
    - `unknown_keys_fall_back`: restoring a stored value with an unknown
      field, a `schema` of 999 and a layer name not on the board gives the
      defaults for what it can't use, with no panic.
    - `open_recent_lists_files`: after opening two fixtures, the menu shows
      both, newest first (`query_by_label_contains`).
  *Done 2026-10-15:* eframe features `persistence` and `accesskit` are on
  (`serde` with `derive` is a direct dependency now, NEW DEP: `serde`, already
  in `Cargo.lock`; the `Cargo.lock` diff grows by about 900 lines, mostly the
  accessibility stack). `src/settings.rs`: `PersistedSettings` with all
  fields `#[serde(default)]` and a schema version; `App::save` writes it under
  one key; `App::build` restores it. File > Open Recent lists the files,
  newest first, a missing file disabled and kept. `tests/ui_settings.rs`:
  `settings_round_trip`, `unknown_keys_fall_back`,
  `open_recent_lists_files`, plus `layer_visibility_comes_back_by_name` and
  `a_missing_recent_file_is_listed_disabled`: 5 pass. A schema other than 1
  or a value that is not RON restores the defaults; an unknown field, layer
  name or kind name is ignored. Scripted runs (`--bench`, `--screenshot`,
  `--exit`, `--control-dir`) use a throwaway persistence file and never write
  the user's settings; I found this out when a first screenshot run wrote
  `~/.local/share/freerouting/app.ron` and moved the next run's panel width,
  and deleted that file.

