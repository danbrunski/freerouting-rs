# C01 — Java reference dumps for settings, writers and the CLI

Opened 2026-10-15. The rest of the C series needs exact Java answers that
`tools/java-reference` cannot give today:
- the settings Java resolves for a board, after every source, the rules file
  and the board-specific cost tuning;
- the text `SesWriter`, `DsnWriter` and `RulesWriter` write;
- what Java's argument parsers make of a command line;
- the exit codes and output files of a real headless CLI run.

`ReferenceDump.java` has two modes, `load` and `route`. This checklist adds
six deterministic modes next to them. C02–C07 compare against their output;
expected values never come from reading Java code.

**Prerequisites:** none for C01.1–C01.7. The routed SES dumps (C01.3) import
`fixtures/reference/route/*.ses`, so they are regenerated after J01.5 moves the
pin. F02.4's `tools/determinism-check.sh` can replace the plain run-twice diff
once it exists.

## What exists and what this adds

| Exists | State |
|---|---|
| `tools/java-reference/src/reference/ReferenceDump.java` (223 lines) | Modes `load` (plain `DsnReader.readBoard`, `BoardStatistics`) and `route` (DefaultSettings + DsnFileSettings + a priority-80 `Overrides` source, run through the real `RoutingJobScheduler`, which also reads an adjacent `<design>.rules`) |
| `tools/java-reference/run.sh` | Builds the classes with ECJ and runs `ReferenceDump MODE $ROOT/fixtures $ROOT/fixtures/reference "$@"`. Extra arguments pass through. New modes need no script change |
| `fixtures/reference/load/` (860 KB), `fixtures/reference/route/` (13 MB, 160 SES) | Loaded-board statistics; routed SES written by Java's `SesWriter` |
| Java at `cda540f` | `RoutingJobScheduler.java:92-191` (load, second merge, rules, cost tuning, SES import); `CliRunner.initializeCli` (first merge: prototype + `DsnFileSettings` [+ `RulesFileSettings` for `-dr`]); `SettingsMerger.merge` (calls `validate()`); `HeadlessBoardManager.applyRouterSettingsForLoadedBoard` (:845); `io/specctra/{SesWriter,DsnWriter,RulesWriter}.java`; `GlobalSettings.applyCommandLineArguments` (:579); `settings/sources/CliSettings`; `FRLogger.getLogEntries()` (used by `GlobalSettingsTest` to count warnings) |

What this adds:
- modes `settings`, `argv`, `ses`, `dsn`, `rules` and `clirun`;
- small synthetic boards for settings quirks no fixture shows, in
  `tools/java-reference/cases/`;
- committed dumps under `fixtures/reference/`, plus an on-demand full set.

## Decisions made here

- **Replicate the scheduler's load sequence; don't run the scheduler.** A rules
  file's `autoroute_settings` switch the router and optimizer back on
  (`AutorouteSettings.readScope` defaults `autoroute` and `postroute` to on), so
  running the real scheduler would route the rules boards. The harness method
  `prepareLikeScheduler` copies `CliRunner`'s first merge and
  `RoutingJobScheduler.java:92-191` call for call, with a comment citing both.
  One change: it calls `job.board.awaitPostLoad()` right after
  `loadFromSpecctraDsn`. That stops the deferred post-load DRC thread from
  racing with the rules and SES import (the race DIVERGENCES describes for the
  DRC runner).
- **Four named settings scenarios.** C02, C03 and C04 tests refer to them by
  name:
  - `default`: prototype `[DefaultSettings]`. This is
    `freerouting -de b.dsn -do b.ses` with no user config, environment or flags:
    edge clearance 250 µm (capped at the minimum pin gap), optimizer on.
  - `fixture`: `default` plus a priority-80 source that sets only
    `copperToEdgeClearanceUm = 0.0`. This is Java's `TestingSettings`
    (`src/test/java/app/freerouting/settings/sources/TestingSettings.java`).
    Java's fixture-test numbers were measured under it: edge clearance 0 and
    the optimizer on.
  - `cli`: prototype `[DefaultSettings, new CliSettings(ARGS)]` with
    `ARGS = -de <b>.dsn -do <b>.ses -mp 7 -mt 3 -oit 0.035 -inc <cls>
    --router.via_costs=77 --router.scoring.plane_via_costs=7
    --router.optimizer.enabled=false
    --router.default_undesired_direction_trace_cost=1.5`.
    `<cls>` is the board's first net class other than `default`, in
    `board.rules.netClasses` order, or `POWER` when there is none. The JSON
    records it as `incClass`. On a board with an adjacent rules file this
    scenario shows the rules overriding the CLI.
  - `owner-config`: prototype `[DefaultSettings, new JsonFileSettings(P)]` with
    `P = tools/java-reference/cases/owner-freerouting.json`. That file has the
    shape of Dan's `%APPDATA%\freerouting\freerouting.json`: every router
    section empty except `"scoring": {"start_ripup_costs": 1500}`.
- **Full-precision numbers.** Dumps use ReferenceDump's own plain `GSON`.
  `GsonProvider.GSON` writes doubles with 2 decimals, which would hide cost
  differences such as 1.7 against 1.70000001.
- **Machine-dependent values are normalized.** Any `max_threads` value
  (top level, `autorouter`, `optimizer`) equal to `Math.max(1, cores - 1)` is
  written as the string `"<cores-1>"`, and one equal to `cores` as
  `"<cores>"`. The dumps must not depend on the machine.
- **Design name** is the DSN file name without its last extension. That is
  `RoutingJob.setInput` → `input.getFilenameWithoutExtension()`, which the CLI
  passes to `SesWriter.write`. The Rust side uses the same rule.
- **What gets committed.** The `settings`, `rules`, `argv` and `clirun` dumps
  are committed for every board. The `ses` and `dsn` dumps are committed only
  for the boards listed in `tools/java-reference/dump-subset.txt`, because the
  route SES alone are 13 MB and the DSN fixtures 17 MB. `REFERENCE_FULL=1`
  dumps every board, and `REFERENCE_OUT_DIR=target/java-reference/full` sends
  that run outside the repo. The Rust full-set tests are `#[ignore]` and read
  from there.
- **Every mode is deterministic.** Two runs give byte-identical output.
  A difference is a harness bug, not a Java fact.
- **Shared file, separate modes.** `ReferenceDump.java` is shared:
  - C01 owns the six modes it adds;
  - J01.3 owns changes to the `route` mode;
  - O06 adds an `optimize` mode.
  Every edit is append-only. C01 may only:
  - append imports;
  - add `case` arms to the mode `switch`;
  - add one early `if` in `main` that dispatches the non-board modes
    (`argv`, `clirun`) before the fixture walk;
  - append new methods and nested classes after the last existing member.

## File ownership

| Item | New files | May edit |
|---|---|---|
| C01.1 | `tools/java-reference/cases/{aspect-4layer.dsn, explicit-one-layer-cost.dsn, explicit-cost-rules.dsn, explicit-cost-rules.rules, owner-freerouting.json}`, `fixtures/reference/settings/**`, `fixtures/reference/cases/settings/**` | `ReferenceDump.java` (append-only, as above) |
| C01.2 | `tools/java-reference/cases/argv.txt`, `fixtures/reference/argv.json` | `ReferenceDump.java` |
| C01.3 | `tools/java-reference/dump-subset.txt`, `fixtures/reference/ses/**` | `ReferenceDump.java` |
| C01.4 | `fixtures/reference/dsn/**` | `ReferenceDump.java` |
| C01.5 | `fixtures/reference/rules/**` | `ReferenceDump.java` |
| C01.6 | `tools/java-reference/cases/clirun.tsv`, `fixtures/reference/clirun/**` | `ReferenceDump.java` |
| C01.7 | — | `tools/java-reference/README.md` (new sections), `tools/java-reference/run.sh` (the final command line only: `REFERENCE_FIXTURES_DIR` and `REFERENCE_OUT_DIR` defaults; J01.3 also edits this file, so whichever lands second rebases), `docs/development/engine.md` (append to the regeneration list) |

## Checklist

- [ ] **C01.1 — `settings` mode.**
  For every fixture DSN, and for every DSN in `tools/java-reference/cases/`,
  run `prepareLikeScheduler` once per scenario. Write one JSON per board, under
  `settings/` or `cases/settings/`. Each scenario object holds:
  - `settings`: the final `job.routerSettings` serialized with plain Gson,
    with `layers` and the transient cost arrays written out explicitly;
  - `effective`: per layer `{name, isSignal, active (getLayerActive),
    preferredHorizontal, bendCost (getBendCost), preferredCost,
    againstCost, horizontalCost, verticalCost (getTraceCosts)}`, plus
    `boardSpecificTraceCostsApplied`, `viaCosts`, `planeViaCosts`,
    `startRipupCosts` (the getters), `runRouter`, `runOptimizer`,
    `runFanout`, `validationWarnings`;
  - `board`:
    - the full clearance matrix (class names, then values per class pair
      and layer) and the outline's clearance class name;
    - `minimumPinGap` (`Double.toString` text) and `holeClearance`;
    - `clearanceToleranceUm` and `ignoreConduction`;
    - the names of the nets with `containsPlane()`;
    - per net class: name, via rule name, trace clearance class, trace
      half-width per layer, `isIgnoredByAutorouter`, and the remaining
      flags;
    - via infos (name, padstack, clearance class, attach SMD), via rules
      (name, via names), snap angle and the padstack names;
  - `postLoadViolations`: the size of
    `new DesignRulesChecker(board, null).getAllClearanceViolations()`, taken
    after the full sequence.

  Synthetic cases, all hand-written and under 80 lines each:
  - `aspect-4layer.dsn`: a 100 × 50 mm board with 4 signal layers, two
    components and three 2-pin nets, and no `autoroute_settings`. It
    shows the aspect add and the outer-layer add.
  - `explicit-one-layer-cost.dsn`: the same board plus
    `(autoroute_settings (layer_rule In1.Cu
    (preferred_direction_trace_costs 2.0)))`.
  - `explicit-cost-rules.dsn` and `explicit-cost-rules.rules`: the board
    without the scope, with the same layer rule in an adjacent rules file.
  - **Accept:**
    - `run.sh <pin> settings` writes one JSON per fixture and per case. A
      second run is byte-identical (`diff -r`).
    - In every `fixture` scenario the edge clearance resolves to `0.0`, and
      in every `default` scenario to `250.0`.
    - Every `max_threads` value is normalized. `grep -r` for the
      machine's core count finds nothing.
    - Board totals and run time are in the done note.

- [ ] **C01.2 — `argv` mode.**
  `run.sh <pin> argv tools/java-reference/cases/argv.txt` reads one command line
  per line (`#` comments; arguments separated by tabs, so names with spaces
  survive). For each line:
  - run `new GlobalSettings().applyCommandLineArguments(args)` and
    `new CliSettings(args)`;
  - count warnings through `FRLogger.getLogEntries()`, as `GlobalSettingsTest`
    does;
  - dump `initialInputFile`, `designSessionFilename`, `initialRulesFile`,
    `initialOutputFile`, `additionalOutputFiles`, `drcReportFile`, the non-null
    fields of `CliSettings.getSettings()`, `getParsedArguments()` (sorted by
    key), and the warning count and texts.
  `argv.txt` covers at least:
  - every case in `GlobalSettingsCommandLineTest` and `GlobalSettingsTest`;
  - the Java KiCad plugin's command line (`router_dsn.py._build_command` at
    the pin: `-de in.dsn -do out.ses -host <host> --gui.enabled=false
    --api_server.enabled=false --mcp_server.enabled=false
    --logging.file.location=<dir>`);
  - `-mp 10000`, `-mt 3`, `-oit 0.035`, `-oit 4.0`;
  - `-inc "Power,Default"` and `-inc POWER,GND`;
  - `--router.via_costs=25`, `--router.scoring.via_costs=30`;
  - `--scoring-version=v1`, `--router-scoring-version=v2`;
  - `--router.layers.routable=false,true`;
  - `-de a.dsn -do b.ses` with and without `--router.enabled=false`;
  - `-dex`, `-unknownFlag`, `--someflag`, a bare `-de`.
  - **Accept:**
    - `fixtures/reference/argv.json` has one entry per line, in file order.
    - Two runs are byte-identical.
    - The `-mp 10` entry shows exactly 1 warning, as `GlobalSettingsTest`
      asserts. That is the harness's self-check.

- [ ] **C01.3 — `ses` mode.**
  For each board in `dump-subset.txt`, or every board under `REFERENCE_FULL=1`:
  - load with `prepareLikeScheduler` (`default` scenario);
  - write `ses/unrouted/<b>.ses` with `SesWriter.write(board, out, designName)`;
  - if `route/<b>.ses` exists, import it with `SesReader.read` into a fresh
    load and write `ses/routed/<b>.ses`;
  - write `ses/<b>.json` with the import summary (wires, vias, errors).

  `dump-subset.txt` holds about 20 boards, each with a one-line reason. It
  must include:
  - `Issue029-hw48na` (KiCad host, adjacent rules);
  - `Issue593-BBD_Mars-64` (`SesRoundTripTest`'s board, `(attach off)`);
  - `Issue742-tastexx-pcb` (rotation `338.5`, `(attach off)`);
  - `Issue676-ch32v-tx118s` (LibrePCB host, coordinates written in
    exponent form; Java's `SesReader` imports none of its wires);
  - `Issue508-DAC2020_bm06` and `Issue872-single-layer-repro`
    (`(attach off)` padstacks);
  - one `Issue110-*` board (Cyrillic names);
  - `Issue143-rpi_splitter` (CadSoft host);
  - the largest fixture by item count, `Issue190-processor.Z80` (timing);
  - a board whose Java output contains `(lock_type position)`, found by
    grepping a full run;
  - a board with signal-layer conduction areas in `network_out`;
  - a board with USER_FIXED wires (`(type protect)`).
  - **Accept:**
    - Two runs are byte-identical.
    - The done note gives the size of the committed subset and of a full
      run, and lists which subset entry exercises each listed feature.

- [ ] **C01.4 — `dsn` mode.**
  `DsnWriter.write(board, out, designName, false)` on the plain `DsnReader`
  load. This mode uses no overrides, so the dump tests the writer and not the
  settings. Output: `dsn/<b>.dsn` for the subset (or all under
  `REFERENCE_FULL=1`), plus `dsn/<b>.json` with the byte count. Java's
  `DsnWriter` passes `null` settings, so its output never contains
  `autoroute_settings`. It writes `snapAngle` and `viaRule`, which Java's own
  reader skips (see `docs/DIVERGENCES.md`, freerouting-dsn).
  - **Accept:**
    - Two runs are byte-identical.
    - Every committed file starts with `(pcb` and contains `(structure`,
      as `DsnWriterTest` asserts.

- [ ] **C01.5 — `rules` mode.**
  `RulesWriter.write(board, settings, out, designName)` with the board and
  settings from the `default` scenario. Write `rules/<b>.rules` for every
  fixture. Rules files are small.
  - **Accept:**
    - Two runs are byte-identical.
    - Every file contains `(rules PCB` and `(autoroute_settings`.
    - The total size is in the done note.

- [ ] **C01.6 — `clirun` mode.**
  `run.sh <pin> clirun tools/java-reference/cases/clirun.tsv` starts the real
  Java CLI once per row. It uses `ProcessBuilder` with the running JVM's
  binary (`ProcessHandle.current().info().command()`), the harness classpath
  and `app.freerouting.Freerouting`. Each run:
  - works in a fresh temporary directory;
  - copies the input files in;
  - always adds `--user_data_path=<tmp>/userdata`, `-da`,
    `--gui.enabled=false`, `--api_server.enabled=false` and
    `--mcp_server.enabled=false`, so no developer config, analytics or GUI
    is involved;
  - has a 120 s safety limit, and a run that hits it is a harness failure.

  It records the exit code, every file in the directory with its size, the
  text of output files under 64 KB, and log lines matching `Auto-routing
  stage` with the seconds and resource tail replaced by `<t>`. Rows (all on
  `Issue413-test.dsn`, 8.7 KB, no rules file), each with
  `--router.enabled=false --router.fanout.enabled=false
  --router.optimizer.enabled=false` so nothing routes:
  1. `-do out.ses`;
  2. `-do out.dsn` (HANDOVER's 0-byte case);
  3. `-do out.ses+out.drc.json`;
  4. `-do out.xyz`;
  5. no `-do`;
  6. a missing input file;
  7. `-de Issue029-hw48na.dsn -dr Issue029-hw48na_valid.rules -drc out.json`;
  8. the same with `_invalid.rules`;
  9. `-help`.
  - **Accept:**
    - Two runs give identical JSON.
    - The done note states Java's exit code and output size for rows 1, 2
      and 4. Reading `CliRunner.computeCliExitCode` says row 2 writes a
      0-byte file, logs no error and exits 1; this run settles it.

- [ ] **C01.7 — Docs, run script, regeneration.**
  - Add a section per mode to `tools/java-reference/README.md`: the command,
    the output paths, the scenarios, the subset rule and the full-run
    recipe.
  - Change only the final command line of `run.sh` to use
    `${REFERENCE_FIXTURES_DIR:-$ROOT/fixtures}` and
    `${REFERENCE_OUT_DIR:-$ROOT/fixtures/reference}`.
  - List the new modes in `docs/development/engine.md` "Regenerating the
    reference".
  - State there that `ses` must be rerun after every `route` regeneration
    (J01.5).
  - **Accept:**
    - A fresh clone can regenerate every committed dump with the README's
      commands, and `git status` is clean afterwards.
    - The full-set recipe writes nothing under `fixtures/`.
