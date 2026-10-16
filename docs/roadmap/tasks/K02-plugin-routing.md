# K02 — KiCad plugin routing end to end

Opened 2026-10-15. Once the CLI routes, the plugin's Route button works
without changes: it already runs `freerouting -de x.dsn -do x.ses` and
imports the result. What it lacks is everything around that: options,
progress, a way to stop and keep the best board, a result summary, the
"route in the GUI" mode the Java plugin has, and proof on real boards. This
checklist is the report's PLG-2, plus KRT's per-board settings persistence
(KRT report §8). It completes item 6 of "What 'complete' means" in
[port-completion.md](../port-completion.md): the plugin routes a real board
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

- [ ] **K02.1 — Route options, saved per board.**
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

- [ ] **K02.2 — Live progress.**
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

- [ ] **K02.3 — Stop and keep, or Abort.**
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

- [ ] **K02.4 — Result summary.**
  - Pass `--result-json <work dir>/<board>.result.json`. `summary.py` turns
    the manifest into the text shown after import: routed and incomplete
    connections, vias, total length, time, and a warning when incomplete
    is above 0.
  - **Accept:** `test_summary.py` on two recorded manifests (complete and
    incomplete) checks the exact text; the integration test checks that the
    manifest's incomplete count equals that of `freerouting -de <dsn> <ses>
    -drc` on the written SES.

- [ ] **K02.5 — Route in the GUI.**
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

- [ ] **K02.6 — Non-ASCII names round trip.**
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

- [ ] **K02.7 — Graded runs on real boards.**
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

- [ ] **K02.8 — Windows end to end with the native build.** *Manual, Dan.*
  - The orchestrator rebuilds the Windows exes natively (`kicad-plugin.md`,
    mingw `bin` on PATH, viewer closed) and reinstalls the plugin. Dan
    routes `canary-pwr` and one demo from inside KiCad: Route with
    progress, Stop, Abort, Route in GUI, and the summary.
  - **Accept:** Dan's results (what worked, routed and incomplete counts,
    times) are recorded with the date in `kicad-plugin.md` "Verified".
    Until then the item is partial with "needs Dan".
