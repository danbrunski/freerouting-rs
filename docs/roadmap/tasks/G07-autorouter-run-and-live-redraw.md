# G07 — Autorouter run and live redraw

Opened 2026-10-15. The Java app's main job is to start the autorouter and
optimizer, show progress, and redraw the board as routes land. This
checklist wires the engine's routing job into the GUI: the autoroute
settings dialog, Start and Cancel, the progress panel, live redraw from
board deltas, the routing overlay, and the routing summary. It is the
report's GUI-7 (GUI, plugin and packaging report §1.6, §3.2 items 3–4, §4),
and where frame targets FT4–FT6 are met during a real run.

**Prerequisites:**
- [G02](G02-gpu-board-renderer.md): chunked buffers patched by `ItemId`
  (G02.4).
- A08: router events and the two-level cancel token (`autoroute/events.rs`).
- A13: the batch loop and `run_pipeline`, the routing job the GUI starts.
- B01: the board change journal; B08: `ItemGeometry`, drawable geometry per
  item in board units.
- C02: `RouterSettings` (serde, Java names and defaults); C03: settings
  applied to the board.
- C05: the SES writer, for the GUI-against-CLI equality test.
- G07.7 only: A12 (fanout) and O01 (pull-tight), plus an operator decision.
- The optimization stage control: O06 (batch optimizer). Until O06 lands,
  the "Optimization" stage checkbox is disabled with a tooltip.

## What exists and what this adds

| Exists | Where |
|---|---|
| Board statistics | `freerouting::scoring::BoardStatistics::new(&BasicBoard)` (`items.via_count`, `traces.total_length_mm`, `connections.incomplete_count`, `clearance_violations`) |
| Session, background jobs, `Command`, stats counters | G01 (`src/session.rs`, `src/commands.rs`, `src/stats.rs`) |
| Chunk patching, culling, upload counters | G02 (`src/canvas/chunks.rs`) |
| Router, events, cancel | **missing** at HEAD: A08, A13 |
| `RouterSettings` | **missing** at HEAD: C02 |
| Java at `cda540f` | `gui/windows/routing/{WindowAutorouteParameter,WindowAutorouteParameterState}.java`, `gui/workspace/progress/{GuiRoutingJobWorker,ScreenMessages,RouteProgress,BatchProgress,RoutingSummaryData}.java`, `gui/windows/board/WindowRoutingSummary.java`, `gui/board/BoardToolbar.java`, `gui/workspace/session/InteractiveActionThread.java`; tests `src/test/java/app/freerouting/gui/WindowAutorouteParameterStateTest.java` |

What this adds: `src/routing.rs` (the job bridge),
`src/dialogs/autoroute_settings.rs`, `src/panels/progress.rs`,
`src/panels/routing_summary.rs`, `src/overlays/routing.rs`, and
`tests/ui_routing.rs`.

## Decisions made here

- **The job owns its board.** Start takes the board out of the session:
  `Arc::try_unwrap(session.board)`, or a clone when a DRC job still holds a
  reference. The session enters `SessionState::Routing { job }`. While
  routing, the canvas draws the chunks patched from deltas, editing
  commands are refused with a status message, and DRC/ratsnest panels show
  the counts from `Progress` events. `Finished { board, summary }` puts the
  board back and leaves `Routing`. The GUI never locks a board the router
  holds (no `Arc<RwLock<_>>`).
- **The GUI follows A08's event shape** (G00, "What G07 needs from the
  router's event stream"): `Progress`, `Delta { removed, added:
  Vec<ItemGeometry> }`, `Replaced { snapshot }` and `Finished`. `Replaced` comes when the
  batch loop restores an earlier board (and at the end, when the best board, not the
  last one, is kept: Java fork `de7a4fd2c`); the GUI rebuilds its scene from the snapshot
  instead of patching. The channel is bounded. Each frame the
  GUI drains it without blocking, merges all deltas of the frame into one
  removed set and one added list (an item added and removed in the same
  frame is dropped), patches chunks once, and shows the newest `Progress`.
  This coalescing is the only place progress is rate-limited (≤ 30 Hz by
  the frame rate).
- **Cancel is a graceful stop.** The Cancel button, Esc, and a click on the
  canvas (Java: `autorouter_started` "Click anywhere on the canvas to
  cancel") trigger A08's stop level, so A13 finishes the current step and
  hands back the board A13 chooses to keep (D8: the best board is the one
  saved). The abort level is used only when the app exits.
- **Same settings, same result.** The GUI starts `run_pipeline` with the
  `RouterSettings` from the dialog and nothing else. A GUI run and a CLI run
  with equal settings must write byte-identical SES files (G07.2). The job
  timeout in the dialog is a stop-only deadline (D2).
- **One settings description.** The dialog reads and writes C02's
  `RouterSettings`, never its own copy. A mechanical test ties every
  `RouterSettings` field to a control or to a listed CLI-only reason, so the
  CLI and the GUI can't drift (KRT's lesson, its CLAUDE.md "Keep CLI and GUI
  routing in sync").
- **No sponsor button.** Java's summary window has a sponsor message and
  button (`sponsor_message`, `sponsor_button`); the port drops them with the
  other telemetry and sponsor dialogs (G00 question 2).

## File ownership

Paths are under `crates/freerouting-gui/`.

| Item | New files | May edit |
|---|---|---|
| G07.1 | `src/dialogs/autoroute_settings.rs`, `tests/ui_routing.rs` | `src/settings.rs` (persist the last used settings), `src/text.rs` |
| G07.2 | `src/routing.rs` | `src/session.rs`, `src/commands.rs`, `src/lib.rs` |
| G07.3 | `src/panels/progress.rs` | `src/text.rs` |
| G07.4 | — | `src/routing.rs`, `src/canvas/chunks.rs` (calls only), `src/stats.rs` |
| G07.5 | `src/overlays/routing.rs` | — |
| G07.6 | `src/panels/routing_summary.rs` | `src/settings.rs` |
| G07.7 | — | `src/panels/selection_toolbar.rs` |
| G07.8 | — | `docs/development/gui.md` |

## Checklist

- [ ] **G07.1 — Autoroute settings dialog.**
  - Java: `WindowAutorouteParameter` (1,431 lines) and
    `WindowAutorouteParameterState`. Fields (`WindowAutorouteParameter_en.properties`):
    per layer `active` and `preferred_direction` (`horizontal`,
    `vertical`), disabled for non-signal layers with `power_layer_tooltip`
    (`setEnabled(currentLayer.isSignal)`, lines ~140–152);
    `routing_stages` (`fanout`, `autoroute`, `optimization`);
    `vias_allowed`, `via_costs`, `plane_via_costs`, `start_ripup_costs`,
    `start_pass`, `speed` (`fast`, `slow`), `max_passes`, `max_threads`,
    `job_timeout` with units `timeout_unit_minutes` to
    `timeout_unit_weeks`; advanced (`detail_autoroute_parameter`):
    `trace_costs_on_layer`, `in_preferred_direction`,
    `against_preferred_direction`, `bend_cost`.
  - Input rules from `WindowAutorouteParameterState`: integers clamped to
    their range, positive doubles fall back to the old value, timeouts
    capped at 4 weeks (`MAX_TIMEOUT_SECONDS = 2,419,200`). Port
    `WindowAutorouteParameterStateTest`'s six tests
    (`clampsIntegerAndPositiveDoubleInputs`, `parsesAndFormatsBoundedTimeouts`,
    `capsTimeoutAtFourWeeks`, `formatsTimeoutWithDaysHoursMinutesThresholds`,
    `decomposesAndBuildsTimeoutsWithUnits`, `normalizesTimeoutInput`) as unit
    tests of `dialogs::autoroute_settings::timeout` with the same expected
    strings (`"1 hours 2 minutes"`, `"672:00:00"`,
    `"28 days 0 hours 0 minutes"`, and the rest).
  - Rust: the dialog edits a `RouterSettings` value; Apply stores it in the
    app (persisted with G01.8's settings). It never writes board rules.
  - **Waits on C02.**
  - **Accept:**
    - The six ported tests pass.
    - `tests/ui_routing.rs::settings_round_trip`: `RouterSettings` → dialog
      → `RouterSettings` is equal for the defaults and for a settings value
      with every field changed.
    - `tests/ui_routing.rs::every_router_setting_has_a_control`: walks C02's
      field list (its serde names) and fails if a field has neither a
      dialog control nor an entry in a `CLI_ONLY: &[(&str, &str)]` table
      with a reason. A negative control (a fake extra field) makes it fail.
    - `power_layers_disabled`: on a board with a power plane layer, that
      layer's `active` checkbox is disabled (AccessKit state).

- [ ] **G07.2 — Routing job wiring.**
  - Java: `BoardToolbar` autoroute button, `GuiBoardManager.startAutorouterAndRouteOptimizer`,
    `stopAutorouterAndRouteOptimizer`, `GuiRoutingJobWorker`.
  - Rust: `routing::RoutingJobHandle { events: Receiver<RouterEvent>,
    cancel: CancelToken, thread: JoinHandle<()> }`. `Command::StartRouting`
    moves the board in (decisions) and spawns `run_pipeline` with the
    settings; `Command::StopRouting` triggers the stop level.
  - **Waits on A08, A13, C02, C03; the equality test also on C05.**
  - **Accept:** `tests/ui_routing.rs`:
    - `routes_small_fixture_to_completion`: on `Issue159-setonix_2hp-pcb.dsn`
      with `max_passes = 3`, `try_run_realtime` reaches `Finished`; the board
      has traces; frame CPU max ≤ 50 ms during the run (stats).
    - `gui_and_cli_write_identical_ses`: the GUI run's board written with
      C05's writer is byte-identical to the SES that `freerouting -de
      <fixture> -do <out> -mp 3` writes (C07's Java flag; the long form is
      `--router.autorouter.max_passes`), on two small fixtures. A difference is reported with `freerouting-dev ses-diff`
      (F02.2).
    - `cancel_hands_back_board`: Cancel during pass 1 leads to `Finished`
      and an editable session.

- [ ] **G07.3 — Progress panel and status messages.**
  - Java texts (`ScreenMessages_en.properties`): `batch_autoroute_add`
    ("To route: {{items_to_go}}; routed: {{routed_count}}"),
    `batch_autoroute_layer` ("Ripped up: …; failed: …"),
    `batch_fanout_layer` ("Failed: …; extra vias: +…"), `score`
    ("Score: … (… incomplete connections, … clearance violations)"),
    `autorouter_started` and `optimizer_started` with the pass number,
    `autoroute_end_message` and `optimization_end_message` with `completed`
    or `interrupted`.
  - Rust: `panels::progress` renders the newest `Progress` with these
    templates from `src/text.rs`. Labels carry AccessKit names so tests can
    read them.
  - **Waits on A08.**
  - **Accept:** `tests/ui_routing.rs::progress_texts_match_events`: feed a
    recorded sequence of `Progress` events (a fixture file
    `tests/data/progress_issue159.json`, recorded from a real run) through
    the panel; after each event the labels equal the Java templates filled
    with the event's values. A mid-run snapshot `routing_progress_issue159.png`
    passes on lavapipe.

- [ ] **G07.4 — Live redraw from deltas.**
  - Java: the GUI repaints from the shared board on each
    `BoardUpdatedEvent`. The port patches instead.
  - Rust: `routing::apply_frame_events` coalesces as in the decisions and
    calls `ChunkSet::patch` once per frame.
  - **Waits on A08, B01, B08.**
  - **Accept:**
    - `tests/ui_routing.rs::final_scene_equals_fresh_build`: after a full
      run on `Issue159-setonix_2hp-pcb.dsn`, the chunk draw list equals one
      built from scratch from the returned board, and `full_rebuilds == 0`.
    - FT4 with real deltas, FT5 and FT6 on `Issue190-processor.Z80` in
      release on a quiet machine: frame CPU p95 ≤ 4 ms during the route,
      Cancel to `Finished` ≤ 250 ms, GPU buffers ≤ 64 MB with ≤ 10 % growth
      over 10 passes, 0 full rebuilds. Numbers and load average in the done
      note. If Cancel takes longer because A13 checks the token only between
      connections, record the measured time and the longest connection, and
      leave the item partial with that remainder.
    - A Xvfb screenshot taken mid-run shows routed traces on the canvas;
      the implementer looks at it and describes it in the done note.

- [ ] **G07.5 — Routing overlay.**
  - Java: `GuiRoutingJobWorker.draw`: the airline of the connection being
    routed, drawn in the incomplete colour, width
    `min(3 mil in board units, 300)`; and the optimizer position as an X of
    radius `10 * defaultTraceHalfWidth(layer 0)`.
  - Rust: `overlays::routing` draws both as egui shapes from the newest
    `Progress`. It needs A08's `Progress` to carry the current airline's
    two endpoints and the optimizer position. If A08 doesn't, this item is
    partial with that request named.
  - **Waits on A08 (and O06 for the optimizer position).**
  - **Accept:** `tests/ui_routing.rs::overlay_draws_current_airline`: with a
    recorded `Progress` event, the overlay emits one line shape between the
    event's endpoints (checked on the overlay's shape list), and a snapshot
    shows it.

- [ ] **G07.6 — Routing summary window.**
  - Java: `WindowRoutingSummary`, `RoutingSummaryData` (total nets,
    incomplete count, violations, via count, total trace length, duration,
    score, interrupted). Texts: `header_completed`, `header_interrupted`,
    `nets_total`, `connections_incomplete`, `zero_incompletes`,
    `clearance_violations`, `vias_added`, `trace_length`, `score`,
    `execution_time`, `checkbox_show_summary` (persisted; Java
    `guiSettings.showRoutingSummary`).
  - Rust: `panels::routing_summary` from A13's run summary plus
    `BoardStatistics::new` of the returned board.
  - **Waits on A13.**
  - **Accept:** `tests/ui_routing.rs::summary_matches_board_statistics`:
    after a run, the shown incomplete count, via count and trace length
    equal `BoardStatistics::new(&board)` of the returned board;
    "show after routing" off survives a save/restore and suppresses the
    window on the next run.

- [ ] **G07.7 — Routing a selection (optional).**
  - Java at the pin: `GuiBoardManager.autorouteSelectedItems` and
    `optimizeSelectedItems` call `GuiBoardLegacyEditActions`, whose methods
    are empty; there is no fanout of a selection. The toolbar texts
    `autoroute`, `fanout`, `pull_tight` remain in
    `BoardToolbarInspectedItem_en.properties`.
  - *Operator decision* (G00, "Java's disabled edits"). Only if Dan says
    yes: Autoroute, Fanout and Pull Tight buttons in the selection toolbar
    start the engine's selection entry points (A13's pipeline limited to
    the selected nets, A12's fanout of the selected pins, O01's pull-tight
    of the selected traces), with the same job bridge as G07.2.
  - **Waits on the decision, A12, O01, and an engine entry point for a
    selection.**
  - **Accept:** `tests/ui_routing.rs::selection_route_changes_only_selected_nets`:
    every item of an unselected net is identical before and after (ids and
    geometry).

- [ ] **G07.8 — Record the numbers.**
  - `docs/development/gui.md` gains the FT4, FT5 and FT6 numbers on this
    machine (lavapipe, release, load average) and a line for Dan's Windows
    GPU, filled in by hand.
  - **Accept:** the numbers are in `gui.md` with the commit they were
    measured at; Dan's line is marked "pending" until he runs it.
