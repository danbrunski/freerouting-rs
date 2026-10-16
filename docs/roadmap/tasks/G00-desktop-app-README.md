# G00 — Desktop app

Opened 2026-10-15. The G series takes `freerouting-gui` from a board viewer
to the Java desktop app's feature set (item 5 of "What 'complete' means" in
[port-completion.md](../port-completion.md)): views and layer controls, the
info lists, the rule and parameter dialogs, the autorouter with live
progress, interactive editing with undo and redo, and file save. It also
makes the app fast on large boards, which is Dan's "faster than Java" steer
applied to drawing. The narrative is in [desktop-app.md](../desktop-app.md);
the research behind it is the GUI, plugin and packaging report of
2026-10-15 (sections 1–4 and 8).

## Task index

| Task | Outcome | Size / risk | Prerequisite |
|---|---|---|---|
| [G01](archive/G01-session-and-test-harness.md) ✅ | The app keeps the board in a `Session`; `lib.rs`; SES through the engine; egui_kittest harness; `--bench`, `--screenshot`; Xvfb smoke script; Java input model; persistence and AccessKit | M / low | none |
| [G02](archive/G02-gpu-board-renderer.md) ✅ | GPU-resident board geometry: wgpu paint callback, shader hairlines, chunked and culled batches, patching by `ItemId`, synthetic 16× board, faster load, CPU fallback | L / high: the GUI's speed critical path | G01 |
| [G03](archive/G03-ratsnest-drc-and-info-lists.md) ✅ | Selection model, ratsnest, DRC markers, the violation and incomplete lists, delete all traces with undo, DRC report export | M / low | G01; B08 for G03.7's unfill only |
| [G04](archive/G04-board-windows-and-display.md) ✅ | Nets, components and library windows, item info, visibility sliders, virtual layers, units, rotation and mirroring, colours, status bar, log, About | M / low | G03; B08 (item info) for G04.3 only |
| [G05](archive/G05-inspect-and-safe-edits.md) ✅ | Inspect mode, selection settings, extend selection, selection toolbar, undo and redo with scene patching, context menus, keepout construction | M / medium | G02 (G02.4 patching), G03, B01 (change journal), B04 (selection filter), I03 (editor commands, for G05.4 and G05.7) |
| [G06](archive/G06-rules-dialogs.md) ✅ | Net classes, assign net class, clearance matrix, vias and padstacks, via rules, the static routing settings | M / medium | G03, B08 (rules-edit facade) |
| [G07](G07-autorouter-run-and-live-redraw.md) | Autoroute settings dialog, routing job, progress panel, live redraw from deltas, routing overlay, routing summary, selection-scoped routing | L / high | G02, A08 (router events, cancel token), A13 (batch loop, `run_pipeline`), B01, B08 (`ItemGeometry`), C02, C03, C05; O06 for the optimizer stage control |
| [G08](G08-save-and-front-end-mode.md) | Save SES/DSN/rules, unsaved-changes prompts, rules import, the `-de/-do` front-end mode with the auto-start countdown | M / medium | G07, C04, C05, C06; B01 (`state_hash`) for G08.2 |
| [G09](G09-interactive-routing-and-editing.md) | Route mode, interactive settings, drag, move and copy components, pin swap, cutout, make space, frame time while dragging | L / high | G05, I01, I02, I03, I04 (optional parts) |

Ids follow `series-map.md`: G01 is the report's GUI-1, G02 is GUI-2, and
G03–G09 are GUI-3 to GUI-9 in order.

## Prerequisites from other series

The GUI only draws and maps input (decision D7). Everything it shows or
changes comes from the engine, so most of the series waits on engine work.

| GUI need | Owner | Needed by |
|---|---|---|
| Board change journal (`take_changes`: inserted and removed `ItemId`s per revision, overflow past a cap) | B01 | G05.5, G07.4, G09 |
| Selection filter (`ItemSelectionFilter`, filtered picks) | B04 | G05.1, G05.2 |
| `ItemGeometry` (drawable geometry per item, in board units), item info (`Item.printInfo`), route stubs, rules-edit facade that keeps the search trees right, and a port of `BasicBoard.unfillConductionAreas` (Java's delete-all calls it) | B08 | G07.4 (geometry), G04.3 (info), G03.6 (stubs, optional), G06 (facade), G03.7 (unfill) |
| Settings model (`RouterSettings` and the interactive settings, Java names and defaults; C02 uses no serde, and the GUI's own persisted preferences are G01's) | C02 | G06.6 (persisted parts), G07.1, G09.2 |
| Settings applied to the board | C03 | G07.2 |
| `.rules` reader and writer | C04 | G08.1, G08.3 |
| SES writer | C05 | G07.2 (CLI equality test), G08.1, G08.4 |
| DSN writer | C06 | G08.1 |
| Router events and the two-level cancel token | A08 | G07 |
| The batch loop and `run_pipeline` (the routing job the GUI starts) | A13 | G07 |
| Fanout and pull-tight of a selection | A12 (fanout), O01 (pull-tight) | G07.7 |
| Interactive engine: route session, drag and move, editor commands, selection transforms | I01, I02, I03, I04 | G05.4, G05.7, G09 |
| Board content hash (`state_hash`), so "unsaved changes" matches Java's content comparison | B01 | G08.2 |
| The batch optimizer, behind the settings dialog's optimization stage | O06 | G07.1, G07.2 |
| CI with a Linux GUI job | F01.2 | snapshot tests in CI (G01.4) |

**What G07 needs from the router's event stream.** A08 defines the events,
so the A series can shape them before the batch loop (A13) is written. This
is the GUI report's biggest risk:
- The routing job owns its board (moved in, or an `Arc` clone made unique)
  and runs on its own thread. The GUI never locks a board the router holds.
- Events go over a bounded channel: `Progress` (phase, pass, queued,
  routed, failed, ripped, extra vias, incomplete, violations, score),
  `Delta { removed: Vec<ItemId>, added: Vec<ItemGeometry> }` drained from
  the B01 journal, and `Finished { board, summary }`, which hands the board
  back by move.
- Deltas are emitted at work boundaries (after each inserted connection, or
  every N of them), never on a timer that the routing reads. Event cadence
  must not change the result: a GUI run and a CLI run with equal settings
  write byte-identical SES files (G07.2).

## Order and critical path

1. **G01 first.** It restructures the crate (`lib.rs`, `Session`, module
   split) that every later checklist edits, and it adds the test harness and
   the frame statistics every later acceptance uses.
2. **G02 and G03 in parallel** after G01. They touch disjoint files (see
   the lanes below). G04 can follow G03 at once; only G04.3 waits for B08.
3. **G05 and G06** when B01/B04 and B08 land. G05.5 needs G02.4's patching.
4. **G07, then G08**, when the router exists (A08, A13) and the writers
   (C04–C06) land.
5. **G09** when the I series lands.

The GUI's own critical path is G01 → G02 → G07: G02's chunked GPU buffers
are what make live redraw (FT4, FT5) possible. The series as a whole waits
on the port's critical path (J01 → B → O → A → C).

## Parallel lanes and file ownership

Paths are under `crates/freerouting-gui/` unless they say otherwise.

| Lane | Owns | Shared, append-only |
|---|---|---|
| G01 | `src/lib.rs`, `src/main.rs`, `src/app.rs` (split), `src/session.rs`, `src/commands.rs`, `src/input.rs`, `src/stats.rs`, `src/settings.rs`, `src/panels/{layers,board_stats,warnings}.rs`, `tests/common/`, `tests/ui_viewer.rs`, `tools/gui/` | — |
| G02 | `src/canvas/**`, `src/view.rs`, `benches/` | `src/stats.rs` (new counters), `docs/development/gui.md` (renderer section) |
| G03 | `src/selection.rs`, `src/overlays/**`, `src/panels/{violations,incompletes,length,unconnected,islands}.rs`, `tests/ui_drc.rs` | `src/commands.rs`, `src/input.rs`, `src/session.rs` (background jobs) |
| G04 | `src/panels/{nets,components,library,item_info,display,colors,status,log,about}.rs`, `src/text.rs`, `tests/ui_windows.rs` | `src/scene.rs` (virtual layers), `src/settings.rs` |
| G05 | `src/tools/{inspect,popup,keepout}.rs`, `src/panels/{select_settings,selection_toolbar}.rs`, `src/overlays/construction.rs`, `tests/ui_edit.rs` | `src/session.rs` (undo), `src/canvas/chunks.rs` (patch calls only) |
| G06 | `src/dialogs/**` except `autoroute_settings.rs`, `tests/ui_rules.rs` | `src/commands.rs` |
| G07 | `src/routing.rs`, `src/dialogs/autoroute_settings.rs`, `src/panels/{progress,routing_summary}.rs`, `src/overlays/routing.rs`, `tests/ui_routing.rs` | `src/session.rs`, `src/panels/selection_toolbar.rs` |
| G08 | `src/files.rs`, `src/dialogs/autostart.rs`, `tools/gui/frontend-smoke.sh`, `tests/ui_files.rs` | `src/main.rs` (flags), `src/commands.rs`, `src/session.rs` |
| G09 | `src/tools/{route,drag}.rs`, `src/tools/move_component.rs` (G09.6 only), `src/overlays/route.rs`, `src/dialogs/route_settings.rs` (interactive part), `tests/ui_interactive.rs` | `src/input.rs`, `src/commands.rs` |

**Overlays are drawn with egui shapes in `src/overlays/`, on top of the
canvas**, not inside G02's GPU pipeline. Airlines, violation markers and the
selection highlight are at most a few thousand shapes, so this costs little,
and it keeps G02 and G03 on disjoint files. G02 may move an overlay into the
GPU pipeline later only if a measurement shows it in the frame time.

Engine files are owned by the engine series. A G checklist may add a
read-only accessor or drawable data to the engine's public API, but never
changes engine behaviour (implementer agent rule; `gui.md` "Working rules").

## Rules for this series

These add to the shared rules in [README.md](README.md).

- **Acceptance is measured, never "feels fast" or "looks right".** Each item
  names one or more of:
  - an egui_kittest test by file and function name (`tests/ui_*.rs`);
  - a snapshot (`tests/snapshots/<name>.png`), generated and checked on
    Linux with lavapipe only (`WGPU_BACKEND=vulkan`). Anti-aliasing differs
    across backends, so other OSes run the interaction tests but skip
    rendering tests unless `FREEROUTING_GUI_SNAPSHOTS=1` is set;
  - a headless screenshot of the real app under Xvfb, taken with the recipe
    in `.claude/agents/implementer.md` ("No windows on the operator's
    desktop"), or with `--screenshot` once G01.5 lands. **The implementer
    opens the PNG with Read and says in the done note what it shows**;
  - frame numbers from `--bench`, in a release build, on a quiet machine
    (1-minute load average ≤ 1.0, recorded next to the numbers), against
    the targets below;
  - deterministic counters from `src/stats.rs` (triangles, draw calls,
    chunks drawn, bytes uploaded, full rebuilds), which tests may assert
    exactly.
- **No window on Dan's desktop.** Under WSLg a plain `cargo run` opens on
  his screen. Run the app only under `xvfb-run`.
- **Long work never blocks the GUI thread.** Loading, DRC, ratsnest and
  routing run on worker threads. The check is the frame statistics: the
  maximum frame CPU stays ≤ 50 ms while the worker runs.
- **Results come from the engine.** Counts in lists and overlays are checked
  against engine calls or `fixtures/reference/load/*.json`
  (`statistics.items.total_count`, `statistics.connections.incomplete_count`),
  never against numbers the GUI computed itself.
- **Java texts.** Messages and list texts follow the English Java bundles
  (`src/main/resources/app/freerouting/gui/**/*_en.properties` at
  `cda540f`), so users moving from Java recognise them. All user-visible
  strings live in `src/text.rs` (see question 3).
- **Windows check.** KiCad launches the Windows exes in the Windows
  checkout's `target\release`, not anything built in WSL. A change Dan
  should see in KiCad needs the native rebuild in `kicad-plugin.md`
  (mingw `bin` on PATH; close the viewer first, because Windows locks the
  running exe). The orchestrator does that after a commit; implementers
  don't.
- **New dependencies** (`egui_kittest`, `rayon`, `bytemuck`, and so on) each
  need a `NEW DEP` line in the report.

### Frame targets

All numbers are for release builds. "Frame CPU" is the time on the GUI
thread in `App::update` plus the paint callback's `prepare`,
`finish_prepare` and `paint`, measured with `std::time::Instant` in
`src/stats.rs`. It excludes queue submission and presentation, which under
Xvfb is llvmpipe rasterising and not a usable metric (report §3.1). CI
asserts only loose bounds (3× the target); the targets are checked on this
machine and recorded in `gui.md`, and the wall-clock frame rate is checked
by hand on Dan's Windows GPU.

| Id | Metric | Target | Checked in |
|---|---|---|---|
| FT0 | Baseline: `--bench pan-zoom` on `Issue190-processor.Z80` and `Issue690-kit-dev-coldfire-xilinx_5213` with today's renderer | numbers recorded in `gui.md` | G01.5 |
| FT1 | `Issue190`, `--bench pan-zoom --frames 600`: frame CPU | p95 ≤ 2 ms and ≥ 5× better than FT0 | G02.1, G02.3 |
| FT2 | Synthetic 16× board (4×4 tiling of Issue190: 174,160 items, about 5.6 M triangles): frame CPU | p95 ≤ 4 ms; ≥ 60 fps wall clock on Dan's GPU (manual) | G02.5 |
| FT3 | Load Issue190 from file to first frame | ≤ 1.5 s total, tessellation ≤ 200 ms | G02.6 |
| FT4 | Apply a delta of 200 removed plus 200 added items | ≤ 2 ms CPU; uploaded bytes ≤ 2× the dirty chunks' size; 0 full rebuilds | G02.4, G07.4 |
| FT5 | During an autoroute of Issue190 | frame CPU p95 ≤ 4 ms; progress shown at ≤ 30 Hz; Cancel to `Finished` ≤ 250 ms; 0 full rebuilds | G07.4 |
| FT6 | GPU buffer memory for Issue190 | ≤ 64 MB; growth ≤ 10 % across 10 routing passes | G02.4, G07.4 |
| FT7 | Dragging a component on Issue190 | frame CPU p95 ≤ 8 ms; patching only, 0 full rebuilds | G09.7 |

Issue190 has 10,885 items and 5,981 traces
(`fixtures/reference/load/Issue190-processor.Z80.json`) and tessellates to
350,056 triangles with today's renderer (measured by the research pass on
2026-10-15; G01.5 re-measures it). Issue690 tessellates to 184,222.

## Open questions for Dan

1. **Keys and mouse.** Java uses left-drag for region select and tools,
   the middle button to pan, the right button for context menus
   (`gui.board.BoardPanel` javadoc), and the keys A (zoom all), Z/O (zoom
   in/out), C (centre), F (zoom region), G (ratsnest), V (violations),
   U/B (undo/redo), S/R/D (inspect/route/drag modes), W (pin swap)
   (`gui.interactive.InteractiveState`, `MenuState`). Today any mouse
   button pans and F fits the board. *Recommendation:* Java's set, plus Home
   to fit, Ctrl+Z/Ctrl+Y as aliases for U/B, and Space+left-drag to pan for
   laptops. G01.7 implements this and stays partial until Dan confirms.
2. **Java features to drop.** `.frb` files (Java serialization:
   `board/facade/BoardSnapshotManager`, `util/SafeObjectInputStream`), the
   Autodesk Fusion `.scr` export (`io/specctra/parser/SessionToFusion`, 631
   lines, and `SessionToFusionTest`), the KiCad session JSON export
   (`io/kicad/KiCadJsonWriter`, `KiCadBoardJson`, `Issue733DsnJsonParityTest`),
   the tutorial board (`gui/rendering/TutorialBoardPalette`), Sample Designs,
   Router Demonstrations, and the telemetry, sponsor, e-mail and user-profile
   dialogs (`gui/windows/board/WindowUserSettings`, 375 lines: user id,
   e-mail, telemetry and contact consent, usage statistics).
   *Recommendation:* drop all of them, each with a DIVERGENCES line. DSN +
   SES + `.rules` restore everything `.frb` did. Open Recent replaces the
   tutorial. *Accepted by Dan 2026-10-15; the DIVERGENCES lines were added
   2026-10-16 ("Dropped Java features").*
3. **Languages.** Java ships 36 locales. *Recommendation:* English only,
   with every string in `src/text.rs`, so Fluent or gettext can come later.
   egui does no Arabic or Hebrew shaping, and CJK needs bundled fonts.
4. **Frame targets FT1–FT7** as written above. *Recommendation:* accept
   them, and re-baseline once G01.5 has recorded FT0.
5. **Router single-step controls** (Java's debug play, pause, step:
   `debug/DebugControl.java`, 342 lines, driven from `gui/board/BoardToolbar`
   and tested by `autoroute/BatchAutorouterDebugTest`, 521 lines).
   *Recommendation:* developer-only, behind an environment variable, after
   the A series has a step API (A04's `MazeSearch::step` and the batch
   loop's pass boundary). Not in G07; `DebugControl` is not ported as a
   class and `BatchAutorouterDebugTest` is not ported. *Accepted by Dan
   2026-10-15.*
6. **Java's disabled edits.** At `cda540f`, Delete, Fix, Unfix, New Net,
   New Component, Cutout, Assign Clearance Class, Autoroute and Pull Tight
   of a selection do nothing in Java: they call
   `gui/workspace/controllers/GuiBoardLegacyEditActions.java`, whose
   methods are empty. `MoveItemState`, `CopyItemState` and
   `CutoutRouteState` can't be reached from the GUI. Matching Java means
   leaving them out. *Recommendation:* add Delete, Fix and Unfix only
   (engine calls exist: `remove_items` with `is_deletion_forbidden`,
   `set_fixed_state`, `unfix`), each with a DIVERGENCES line; leave the
   rest out until Dan asks. G05.4, G07.7 and G09.6 refer to this question.
