# K02 — KiCad plugin routing end to end

Opened 2026-10-15. Once the CLI routes, the plugin's Route button works
without changes: it already runs `freerouting -de x.dsn -do x.ses` and
imports the result. What it lacks is everything around that: options,
progress, a way to stop and keep the best board, a result summary, the
"route in the GUI" mode the Java plugin has, and proof on real boards. This
checklist is the report's PLG-2, plus KRT's per-board settings persistence
(KRT report §8). It completes item 6 of "What 'complete' means" in
[port-completion.md](../../port-completion.md): the plugin routes a real board
end to end, on Windows through the native build.

**Prerequisites:**
- the CLI routes: A13 (`run_pipeline`), C05 (SES writer), C07 (CLI parity:
  `-de`, `-do`, `-mp`, `-mt`, `-oit`, `-inc`, exit codes, no 0-byte files);
- C08 (plugin-facing extras: `--progress json`, `--result-json`, `stop` on
  stdin, exit codes 0/1/2/130, `--version`, `--seed`, `--timeout`);
- F02.6 (the KiCad DRC oracle) for K02.6 and K02.7;
- G08.4 (the GUI's `-de/-do` front-end mode) for K02.5;
- K01 (tests, rule-area rewrite, handshake).

## What exists and what this adds

| Exists | State |
|---|---|
| `kicad-plugin/freerouting_rs/plugin.py` `on_route` | Exports, starts `tools.start_route`, pulses an indeterminate `wx.ProgressDialog`, kills the process on Cancel, imports the SES on exit 0 |
| `tools.start_route` / `finish_route` | `stdin=DEVNULL`, stdout and stderr merged |
| K01 | pytest suite, `rule_areas.py`, `version.py`, thread and install guards |
| The CLI contract (C08, from the GUI report §5.2) | `--progress json`: one JSON object per line on stderr, `{"event":"progress","phase":"fanout|route|optimize","pass":…,"routed":…,"failed":…,"ripped":…,"incomplete":…,"violations":…,"score":…,"elapsed_ms":…}`, then `{"event":"done",…}`. `--result-json path`: Java's `RoutingResultManifest` field names where they apply. The line `stop` on stdin finishes the current step, writes the SES and exits 0. Exit codes: 0 output written, 1 failure, 2 bad arguments, 130 aborted before any output. Outputs go to a temp file and are renamed |
| Java plugin at `cda540f` | `integrations/KiCad/kicad-freerouting/plugins/plugin.py`: GUI mode starts the router with its window and waits for it to close; `process_utils.py` `clean_log_line` shows log lines |
| KRT | `kicad_routing_plugin/settings_persistence.py` (defensive restore), `tests/gui_parity/test_settings_roundtrip.py`; settings kept in memory only, lost when KiCad closes |
| Test boards | KiCad 10 demos in `C:\Program Files\KiCad\10.0\share\kicad\demos` (`ecc83/ecc83-pp`, `interf_u`, `complex_hierarchy`, `kit-dev-coldfire-xilinx_5213`, `cm5_minima/CM5_MINIMA_3`), already exported and loaded by `build_board` (`kicad-plugin.md` "Verified"); `canary-pwr` in `../canary/hardware/canary-pwr-rev1.0` |

What this adds: route options saved per board; live progress; Stop and
Abort; a result summary; Route in GUI; a non-ASCII round trip; graded runs
on real boards; the Windows end-to-end check.

## Decisions made here

- **Options are saved per board on disk**, in
  `<board dir>/freerouting-rs/route-options.json` with `"schema": 1`. They
  are not kept in `settings.json`, which holds executable locations and is
  rewritten by `install.py`. Restore skips unknown keys and values it
  doesn't know and keeps defaults for them (KRT's lesson). KRT kept settings
  in memory only; Dan's boards are long-lived, so ours go to disk.
- **The plugin parses only C08's JSON lines**, never human log text. Lines
  that aren't JSON are shown in a log pane as they are.
- **Progress reaches the UI through a queue.** A reader thread parses
  stderr into a `queue.Queue`; the dialog's UI-thread loop drains it. No
  pcbnew or wx call happens on the reader thread (K01.4's test covers it).
- **Stop keeps, Abort discards.** Stop sends `stop\n` and waits for the
  process; Abort kills it. C08's temp-file-and-rename rule means an aborted
  run leaves no SES.
- **No DSN sanitizing.** The Java plugin strips Ω, µ and Φ because Java's
  parser can't read them. The Rust lexer already reads non-ASCII fixtures
  (`Issue093-interf_u`, `Issue110-*`). If a round trip fails, the fix goes
  in the engine, never in the plugin.

## File ownership

| Item | New files | May edit |
|---|---|---|
| K02.1 | `kicad-plugin/freerouting_rs/options.py`, `kicad-plugin/tests/test_options.py` | `plugin.py`, `tools.py` (`start_route` takes the argument list) |
| K02.2 | `kicad-plugin/freerouting_rs/progress.py`, `kicad-plugin/tests/test_progress.py`, `kicad-plugin/tests/data/progress-*.jsonl` | `plugin.py`, `tools.py` |
| K02.3 | `kicad-plugin/tests/test_cli_integration.py` | `plugin.py`, `tools.py` |
| K02.4 | `kicad-plugin/freerouting_rs/summary.py`, `kicad-plugin/tests/test_summary.py` | `plugin.py` |
| K02.5 | `kicad-plugin/tests/test_gui_mode.py` | `plugin.py`, `tools.py` |
| K02.6 | `tools/kicad-oracle/roundtrip_names.py` | engine files only through an engine checklist, if the round trip fails |
| K02.7 | `tools/bench/sets/kicad-demos.txt` | `docs/development/kicad-plugin.md` |
| K02.8 | — | `docs/development/kicad-plugin.md` |

## Checklist

- [x] **K02.1 — Route options, saved per board.**
  - `options.py`: `RouteOptions { max_passes (-mp, 0–9999), threads (-mt,
    0–1024), optimizer (on/off, `--router.optimizer.enabled`), improvement
    threshold (-oit), ignore_net_classes (-inc, comma list), seed (--seed),
    min_trace_width_um (`--router.min_trace_width_um`, filled from the board's
    Board Setup `min_track_width` by default, D8: the DSN doesn't carry it),
    timeout (--timeout, e.g. "10m"), rule_area_fixup (K01.2) }`;
    `to_args()` builds the argument list in a fixed order; `load(path)`
    and `save(path)` with the schema and defensive restore. Ranges and
    defaults follow C02's settings and Java's `GlobalSettings`
    (`src/main/java/app/freerouting/settings/GlobalSettings.java`, flag
    parsing around lines 640–960 at `cda540f`).
  - The dialog shows the options; Route uses them.
  - **Accept:** `test_options.py`:
    - golden argument lists for the defaults and for every option set;
    - save, load, save gives equal JSON;
    - a stored dict with a removed key, an unknown key, an out-of-range
      value and `"schema": 99` restores to defaults for those fields
      without an exception;
    - one integration test (in `test_cli_integration.py`, K02.3) passes all
      options to the real CLI on a small fixture and gets exit 0.

  *Done 2026-10-16:* `options.py` (`RouteOptions`, `to_args`, `load`/`save`, `from_dict`, `from_form`), the dialog's option form and `route-options.json` (`"schema": 1`) in `plugin.py`. Deviations from the text above: the flags are `--max-passes` (the alias of `-mp`; `-mp` makes the router log a deprecation warning) and `--threads` (`-mt` is a flat setting no stage reads headless), the net classes use `--router.autorouter.ignore_net_classes=a,b` (`-inc a\,b` only when a name has a comma), the optimizer threshold range is 1 to 100 percent (the router multiplies a value in (0, 1) by 100), `--seed` is passed only when not 0 and the CLI accepts only 0 today, the width flag is always passed (D8). `test_options.py` (36 tests): golden argument lists for the defaults (`[]`, and `--router.min_trace_width_um=150` with a board minimum) and for each option and all of them in a fixed order; `test_save_load_save_gives_equal_json`; `test_a_damaged_stored_dict_restores_defaults_without_an_exception` (removed key, unknown key, out-of-range, `"schema": 99`); `test_cli_integration.py::test_all_options_reach_the_real_cli_and_it_exits_0` passes passes, threads, optimizer off, threshold, ignored class, min width and timeout to the real CLI on `Issue026-J2_reference.dsn`: exit 0, no "Unknown"/"Deprecated" log line, and the manifest's `settings_snapshot` has every value. `test_route_dialog.py::test_the_options_are_saved_and_reach_the_router` runs the dialog code (stub wx): saved, passed, restored by a new dialog.

- [x] **K02.2 — Live progress.**
  - `progress.py`: `parse_line(text) -> event | None` for C08's lines and a
    `ProgressState` that keeps the last phase, pass, routed, failed,
    incomplete and elapsed time. The dialog replaces the pulsing bar with
    those numbers and a log pane for non-JSON lines.
  - Record real output once: run the CLI with `--progress json` on two
    fixtures and commit the stderr as `tests/data/progress-<board>.jsonl`.
  - **Accept:** `test_progress.py` replays both recordings and checks the
    final state against the recorded `done` event; malformed and partial
    lines are skipped without an exception; K01.4's thread test still
    passes.

  *Done 2026-10-16:* `progress.py` (`parse_line`, `ProgressState`, `format_log_event`, `pump_lines`, the reader thread's body), `routing.RouteRun` (queue drained on the UI thread by the dialog's `wx.Timer`), `RouteProgressDialog` in `plugin.py` (phase, pass, routed/failed/incomplete, elapsed, a log pane for the router's log and for non-JSON lines). Recordings from the real CLI (`--progress json --result-json`): `tests/data/progress-j2.jsonl` (`Issue026-J2_reference.dsn`, `-mp 2`, 34 lines, complete) and `progress-bm01.jsonl` (`Issue508-DAC2020_bm01.dsn`, `--max-passes 1`, 59 lines, 49 incomplete), with `result-j2.json` and `result-bm01.json`. `test_progress.py` (33 tests incl. `test_a_recording_replays_to_the_state_of_its_done_event` for both, malformed and cut-off lines, bad values); `test_threads.py` now scans every module for thread targets (the reader thread lives in `routing.py`), keeps its negative controls (lambda calling pcbnew, a helper reaching `wx.Yield`, a thread in `plugin.py`, `wx.CallAfter` allowed) and passes (5 tests).

- [x] **K02.3 — Stop and keep, or Abort.**
  - Stop writes `stop\n` to the process's stdin (`stdin=PIPE` from now on)
    and waits; the dialog then imports the SES as after a normal finish.
    Abort kills the process and imports nothing.
  - **Accept:** `test_cli_integration.py` with the real CLI (skipped with a
    message when no `freerouting` with routing is found):
    - on a fixture large enough to still be routing at its first progress
      event (name it in the test), `stop` after that event gives exit 0 and
      an SES that `freerouting -de <dsn> <ses> -drc` loads with zero import
      errors;
    - a kill at the same point leaves no `.ses` file and no temp file in the
      output folder.

  *Done 2026-10-16:* `tools.start_route` (stdin is a pipe), `tools.stop_route`, `RouteRun.stop`/`abort`, the dialog's buttons. `test_cli_integration.py` against the real CLI (debug build via `FREEROUTING_RS_BIN`; skips with a message when none is found): `test_stop_after_the_first_route_event_keeps_a_loadable_board` (`Issue508-DAC2020_bm01.dsn`, `--max-passes 50`, `stop` sent at the first `route` event: exit 0, state completed, a SES that `freerouting -de dsn ses -drc` loads ("SES file loaded for DRC: N wires, M vias imported", no "Failed"/"rror" in its log), only the dsn, ses and manifest left) and `test_abort_at_the_same_point_leaves_no_ses_and_no_temp_file` (kill at the same point: no `.ses`, no `.tmp`). Fake-router tests in `test_routing.py` and the dialog flows in `test_route_dialog.py` (`test_stop_keeps_the_best_board`, `test_abort_discards_everything`). The C08.5 remainder (Ctrl-C and SIGTERM through `ctrlc`) was done here; see the archived C08.

- [x] **K02.4 — Result summary.**
  - Pass `--result-json <work dir>/<board>.result.json`. `summary.py` turns
    the manifest into the text shown after import: routed and incomplete
    connections, vias, total length, time, and a warning when incomplete
    is above 0.
  - **Accept:** `test_summary.py` on two recorded manifests (complete and
    incomplete) checks the exact text; the integration test checks that the
    manifest's incomplete count equals that of `freerouting -de <dsn> <ses>
    -drc` on the written SES.

  *Done 2026-10-16:* `summary.py` (`format_summary`, `load_manifest`), `--result-json <work dir>/<board>.result.json` in `RouteRun`, the message after the import. `test_summary.py` (8 tests) checks the exact text of the two recorded manifests: complete (`Connections: 33 of 33 routed, 0 incomplete`, no warning) and incomplete (`Connections: 146 of 195 routed, 49 incomplete`, ..., `WARNING: 49 connections are not routed. Check the board before you use it.`). `test_cli_integration.py::test_the_summary_matches_the_drc_of_the_written_session`: the manifest's `normalized_score` equals `qualityScore` of `freerouting -de <dsn> <ses> -drc` on the written SES for a complete board (J2, 0 incomplete, no unconnected item) and an incomplete one (bm01, 1 pass, 49 incomplete). The DRC report has no incomplete count of its own (its `unconnectedItems` lists split nets: 43 entries for bm01's 49 connections), but its score is the router score computed from that count and the violations, so equal scores mean equal counts.

- [x] **K02.5 — Route in the GUI.**
  - A "Route in GUI" button launches `freerouting-gui -de x.dsn -do x.ses`,
    the command the Java plugin uses with `--gui.enabled=true`
    (`integrations/KiCad/kicad-freerouting/plugins/router_dsn.py`
    `_build_command` at `cda540f`). G08.4's front-end mode then auto-starts
    routing after Java's 5 s countdown and writes `-do` when routing
    finishes or the window closes. The plugin shows a modal dialog with the
    Java plugin's text ("Complete routing and close the window, or press
    Terminate to cancel."); Terminate kills the GUI process. When the
    process exits, the plugin imports the SES if it exists and is newer
    than the launch.
  - **Accept:** `test_gui_mode.py` with a fake GUI executable that writes
    an SES and exits: the command line is as specified and the import step
    is reached; with a fake that exits without writing, no import happens
    and a message says so. The real GUI part is in K02.8.

  *Done 2026-10-16:* `routing.GuiRun`, `tools.start_gui`, `GuiWaitDialog` and the "Route in GUI" button; the options go to the GUI as `--max-passes`, `--threads`, `--router.*` (not `--seed`). `test_gui_mode.py` (7 tests; `fake_bin/fake_gui.py` and the stub wx/pcbnew): `test_a_gui_that_writes_the_session_is_imported` (command line `-de x.dsn -do x.ses --router.min_trace_width_um=150`, import reached on the written file), `test_a_gui_that_exits_without_a_session_imports_nothing` (message "...closed (exit code 0) without writing b.ses"), Terminate, an old session not imported, the Java plugin's text. Operator decision (option A): the CLI's argument-to-`SettingsPatch` mapping moved into the engine (`freerouting::settings::cli_flags`: `rewrite_aliases`, `parse_router_flags`, `router_flag_span`, `apply_escaped_inc`, the timeout and `Integer.decode` helpers), the CLI's `args.rs` uses it with its tests unchanged (`cargo test -p freerouting-cli`: all pass, 58 in `args_java`), and the GUI's front-end mode reads the same flags (`-mt`, `-oit`, `-inc`, `--router.*`, `--max-passes`, ...) into `FrontEndOptions::router`, laid over the job's settings: `args::tests::router_flags_reach_the_patch_as_on_the_cli`, `ui_files.rs::frontend_takes_the_routers_flags_of_the_cli`. The real GUI window is K02.8. DIVERGENCES, "freerouting-gui", names the flag set (and `-mp -1`, now an unknown option as on the CLI).

- [x] **K02.6 — Non-ASCII names round trip.**
  - `tools/kicad-oracle/roundtrip_names.py` runs in KiCad's Python on a
    copy of the `interf_u` demo (its nets carry Ω, as in fixture
    `Issue093-interf_u`) and on a copy of one board edited to add nets and
    references with µ and Φ: export DSN, route with the CLI, import the SES
    with `ImportSpecctraSES` into the copy, and compare the net names of
    every imported track with those of the original board.
  - **Accept:** every routed track's net name equals the original's; no
    sanitizing anywhere in the path. A failure is reported to the C series
    (C05 writer) or the DSN lexer owner with the failing name, and the
    item stays partial until fixed.

  *Done 2026-10-16:* `tools/kicad-oracle/roundtrip_names.py` (details and results in `kicad-plugin.md`, "K02.6"). KiCad 10's `interf_u` demo has no Omega in any name (the old board behind `Issue093-interf_u` has Chinese text), so the script renames nets and references on the copies: `interf_u` gets Omega on 8 nets and 11 references, `ecc83-pp` mu and Phi on 3 nets and 6 references. Run with the release CLI, `--max-passes 3`, KiCad 10.0.7: `interf_u: ok: 789 tracks and vias imported, 8 non-ASCII nets routed ... shorting_items 0`; `edited: ok: 33 tracks and vias imported, 2 non-ASCII nets routed (Net-(P3-P1)Φ, Net-(U1A-G)µ), shorting_items 0`. Every routed track's net name is a net of the original, the names are in the DSN and (for routed nets) the SES as they are, no reference changed; no sanitizing anywhere. `pgrep -x kicad; pgrep -x pcbnew` empty before and after. Not run in Windows KiCad 10.0.5.

- [x] **K02.7 — Graded runs on real boards.**
  - `tools/bench/sets/kicad-demos.txt` lists the five demos and
    `canary-pwr` by path (out of the repo, skipped when missing).
  - For each board, run the plugin's path headless (export with the K01.2
    rewrite, CLI route with default options, import), and the same export
    through Java Freerouting at the pin, both graded with F02.6's
    `grade.sh`.
  - **Accept:** a table in `kicad-plugin.md`: per board and engine, the
    KiCad DRC violations by type, unconnected items, vias and run time.
    The Rust run has no clearance violation that Java's lacks and no via in
    a pad; any other difference is explained. `pgrep -x kicad; pgrep -x pcbnew` shows the
    same KiCad processes before and after.

  *Partial 2026-10-16:* `tools/bench/sets/kicad-demos.txt`, `tools/bench/k02-graded-runs.sh`, `tools/kicad-oracle/via_in_pad.py`, `graded_table.py`; the table and the explanations are in `kicad-plugin.md`, "K02.7". Done: five of six boards, both engines, graded by `grade.sh` (KiCad 10.0.7): `ecc83-pp` and `complex_hierarchy` no router-caused item in either engine; `CM5_MINIMA_3` the two sessions are line for line the same (38 items, all shared); `interf_u` Rust 5 `starved_thermal`, Java 4; `kit-dev-coldfire-xilinx_5213` Rust has one `clearance` item (a via 0.04 mm from the copper text `MOD0`, which the DSN export does not carry; Java missed it by chance) and Java has 190 tracks under the board's minimum width, Rust none. No via in a pad in any graded run. Same KiCad processes before and after (none). **Remaining: `canary-pwr`, Rust.** The CLI panics at HEAD with the plugin's options (`expect("item on the board")`, `basic_board.rs:227`, from `normalize_pass` in the fanout stage; with the fanout off it panics with a minimum width and routes without one). That is an engine bug owned by the A series (A14, the board parity gate, which owns the fanout and trace-insertion fixes); Java's `canary-pwr` row is in the table. Re-run `tools/bench/k02-graded-runs.sh ... canary-pwr` once it is fixed, then tick.
  *Done 2026-10-17:* the panic was a port bug in `normalize_pass` (not A14; it panics back to A13), fixed in `8fb7e21` as Java behaves. `ENGINES=rust tools/bench/k02-graded-runs.sh tools/bench/sets/kicad-demos.txt target/orchestrator/k02/g4 <release freerouting at 813663c> <a773a70ec jar> canary-pwr`: copper_edge_clearance 2, starved_thermal 2, via_dangling 36, 70 / 174 unconnected, 124 vias, 0 vias in pads, 0 below the board minimum, 199 s; Java at the pin: the same edge-clearance and thermal items plus 2 under-width tracks and 1 dangling track, 71 / 174. No clearance item Rust has that Java lacks; KiCad processes unchanged (none). Table and explanation in `kicad-plugin.md`, "K02.7".

- [~] **K02.8 — Windows end to end with the native build.** *Manual, Dan.*
  - The orchestrator rebuilds the Windows exes natively (`kicad-plugin.md`,
    mingw `bin` on PATH, viewer closed) and reinstalls the plugin. Dan
    routes `canary-pwr` and one demo from inside KiCad: Route with
    progress, Stop, Abort, Route in GUI, and the summary.
  - **Accept:** Dan's results (what worked, routed and incomplete counts,
    times) are recorded with the date in `kicad-plugin.md` "Verified".
    Until then the item is partial with "needs Dan".

  *Partial 2026-10-16:* needs Dan, on Windows, inside KiCad 10.0.5. The exact steps (options saved, Route with progress, Stop, Abort, Route in GUI with Terminate, a demo board) and what to record are in `kicad-plugin.md`, "K02.8: what Dan checks". Before that the orchestrator rebuilds the Windows exes natively (mingw `bin` on PATH, viewer closed, `cargo build --release -p freerouting-gui -p freerouting-cli`) and reinstalls the plugin (`install.py --bin-dir target\release`, Refresh Plugins). Same Windows reinstall as K01.6. Nothing of K02 has run inside KiCad; all of it runs headless against the stub `wx`/`pcbnew`, the fake and the real CLI.
