# Java reference harness

Runs the Java Freerouting engine on every board in `fixtures/` and stores what it
produced in `fixtures/reference/`, so the Rust port has something concrete to compare
against.

```sh
tools/java-reference/run.sh ../freerouting load
tools/java-reference/run.sh ../freerouting route 100 00:00:45
```

The first argument is a checkout of `danbrunski/freerouting` (the port is pinned to
`cda540f`). The script needs `python3`, `pip`, `curl` and the Gradle wrapper in that
checkout, which it only uses to resolve dependencies. You don't need a JDK 25 install:
the script takes a Java 25 runtime from the `jdk4py` wheel and compiles with the Eclipse
compiler (ECJ). Set `JAVA=/path/to/java` to use your own Java 25+ instead. Build
products go to `target/java-reference/`.

## Outputs

| Path | Contents |
| --- | --- |
| `fixtures/reference/load/<board>.json` | Result of `DsnReader.readBoard` (`success`, `outline_missing`, `parse_error`, or `exception`) and the board's `BoardStatistics`: layers, item counts by type, components, pads, nets, incomplete connections, clearance violations, bounds. |
| `fixtures/reference/route/<board>.json` | Final job state, wall-clock routing time, router score and `BoardStatistics` after the batch autorouter and optimizer ran with the given pass and time limits. |
| `fixtures/reference/route/<board>.ses` | The SES session file the Java router wrote. |

`load` output is deterministic and should be byte-identical on every run. `route`
output isn't: Java's maze search uses an unseeded `java.util.Random` and the router
runs against a wall-clock timeout, so treat routed results as a baseline for completion
rate, via count, length, score and speed, not as an exact target.

`route` skips boards that already have a `route/<board>.json`, so an interrupted run
picks up where it stopped. Set `REFERENCE_REDO=1` to rerun everything.

## Known Java failures

- `Issue006-LPC18XX_43XX_SCH.dsn` is not a PCB file (`parse_error`).
- `Issue179-Autorouter_PCB1_2023-3-24.dsn` throws a `NullPointerException` in Java
  (`exception`).

## Probes

`probe.sh` compiles and runs one Java probe against the Java classes of a pin checkout, for
differential tests whose expected values must come from Java:

```sh
git -C ../freerouting worktree add --detach <scratchpad>/java-pin <pin>
TMPDIR=<scratchpad> tools/java-reference/probe.sh <scratchpad>/java-pin tools/java-reference/probes/JavaRandomProbe.java
# 30 63 48 84 70
# [8, 1, 4, 3, 9, 6, 0, 2, 5, 7]
git -C ../freerouting worktree remove --force <scratchpad>/java-pin
```

- The probe is any `.java` file with a `main`; its class is the file name, prefixed by its
  `package` if it has one. Extra arguments go to `main`. Stdout and the exit status are the
  probe's. A compile error prints ECJ's message on stderr and exits 1.
- When `target/java-reference/classes` (or the checkout's `build/reference-classpath.txt`) is
  missing, the script builds it through `run.sh` with a fixture filter that matches no board,
  so no reference file is written.
- The cached classes must come from the checkout given: `run.sh` records the commit in the
  generated `Constants.java`, and `probe.sh` exits 2 when that differs from the checkout's
  HEAD. Delete `target/java-reference/classes` to rebuild for another pin.
- Never point it at `../freerouting` itself, which another session edits; use a detached
  worktree. `REFERENCE_WORK_DIR` and `JAVA` work as for `run.sh`; the probe is compiled into a
  temporary directory under `TMPDIR`.
- `probes/` holds probes worth keeping. `JavaRandomProbe.java` prints the `java.util.Random`
  values `freerouting_geometry::jmath` is tested against.

## Settings, writer and CLI dumps (C01)

Six deterministic modes next to `load` and `route`. Two runs give byte-identical output; a
difference is a harness bug, not a Java fact. Regenerate all committed dumps with one command
(about 7 minutes), then check `git status`:

```sh
tools/java-reference/regenerate-dumps.sh <pin-worktree>
```

It runs the six `run.sh` calls below in order. Use a detached worktree of the pin, never
`../freerouting` itself. After a `route` regeneration (J01.5) rerun it, because `ses` imports
`fixtures/reference/route/*.ses`. The jdk4py runtime has no execute bit on `lib/jspawnhelper`;
`clirun` fixes that in the runtime under `target/`.

The board modes take an optional name filter (`run.sh <pin> settings Issue413`); `REFERENCE_TMP_DIR`
sets where `clirun` makes its working directories (default: the system temp directory).

| Mode and command | Output under `fixtures/reference/` |
| --- | --- |
| `run.sh <pin> settings [FILTER]` | `settings/<board>.json` for every fixture DSN, `cases/settings/<case>.json` for every DSN in `tools/java-reference/cases/` |
| `run.sh <pin> rules [FILTER]` | `rules/<board>.rules` for every fixture (a `<board>.json` with the failure when the board does not load) |
| `run.sh <pin> ses [FILTER]` | `ses/unrouted/<board>.ses`, `ses/routed/<board>.ses` (when `route/<board>.ses` exists) and `ses/<board>.json` (import summary), for the boards in `dump-subset.txt` |
| `run.sh <pin> dsn [FILTER]` | `dsn/<board>.dsn` and `dsn/<board>.json` (byte count), for the boards in `dump-subset.txt` |
| `run.sh <pin> argv tools/java-reference/cases/argv.txt` | `argv.json` |
| `run.sh <pin> clirun tools/java-reference/cases/clirun.tsv [ID_FILTER]` | `clirun/<id>.json` |

**Subset rule.** `settings`, `rules`, `argv` and `clirun` are committed for every board. `ses` and
`dsn` are committed only for the boards in `dump-subset.txt` (one `<name><TAB><reason>` per line;
`cases/<name>` is a synthetic board). `REFERENCE_FULL=1` dumps every board for those two modes. The
full recipe writes nothing under `fixtures/`:

```sh
REFERENCE_FULL=1 REFERENCE_OUT_DIR=target/java-reference/full tools/java-reference/regenerate-dumps.sh <pin-worktree>
```

`REFERENCE_FIXTURES_DIR` and `REFERENCE_OUT_DIR` default to `fixtures` and `fixtures/reference`.
Inputs that live in the reference tree (`route/*.ses`) are always read from
`<fixtures>/reference`.

### `settings`

Replicates the scheduler's load sequence without running the router, once per scenario:
`CliRunner.initializeCli`'s first merge, then `RoutingJobScheduler` lines 92-191 call for call
(`loadFromSpecctraDsn`, the second merge with the adjacent `<design>.rules`, `RulesReader.read`,
`applyBoardSpecificOptimizations`, `applyNetClassExclusions`). Running the real scheduler would
route the rules boards, because a rules file's `autoroute_settings` switch the router back on. One
change: `awaitPostLoad()` right after the load, so the deferred DRC thread cannot race the rules
import.

| Scenario | Prototype merger |
| --- | --- |
| `default` | `[DefaultSettings]`: `freerouting -de b.dsn -do b.ses` with no user config |
| `fixture` | `default` plus a priority-80 source setting only `copperToEdgeClearanceUm = 0.0` (Java's `TestingSettings`) |
| `cli` | `[DefaultSettings, CliSettings(-de <b>.dsn -do <b>.ses -mp 7 -mt 3 -oit 0.035 -inc <cls> --router.via_costs=77 --router.scoring.plane_via_costs=7 --router.optimizer.enabled=false --router.default_undesired_direction_trace_cost=1.5)]`; `<cls>` (`incClass` in the JSON) is the board's first net class other than `default` right after the DSN load, or `POWER` |
| `owner-config` | `[DefaultSettings, JsonFileSettings(cases/owner-freerouting.json)]`: only `scoring.start_ripup_costs = 1500` |

Each scenario holds `settings` (the final `RouterSettings` with plain Gson, full-precision numbers,
`layers` and the transient cost arrays written out), `effective` (per-layer getters and the
scalar getters), `board` (clearance matrix by layer, net classes, via infos and rules, padstack
names, snap angle, ...), `adjacentRules` and `postLoadViolations`. A `max_threads` equal to
`cores - 1` or `cores` is written as `"<cores-1>"` or `"<cores>"` so the files do not depend on the
machine (the `cli` scenario's `-mt 3` would read as `"<cores-1>"` on a 4-core machine).
The synthetic boards in `cases/` (`aspect-4layer`, `explicit-one-layer-cost`,
`explicit-cost-rules` + `.rules`, `lock-position`) show quirks no fixture shows.

### `argv`

One command line per line of `cases/argv.txt` (tab-separated arguments, `#` comments). For each
line: `new GlobalSettings().applyCommandLineArguments(args)` (file names, `changedFields` against
a fresh `GlobalSettings`, the debug and strategy fields, and the warnings and errors it logged,
counted through `FRLogger.getLogEntries()`), and `new CliSettings(args)` (its non-null
`RouterSettings` fields, `getParsedArguments()` sorted by key, and its own log counts).

### `ses`, `dsn`, `rules`

`ses` loads the board as in the `default` scenario and writes `SesWriter.write(board, out,
designName)`; if `route/<b>.ses` exists it imports it into a fresh load with `SesReader.read` and
writes that too. `dsn` writes `DsnWriter.write(board, out, designName, false)` on the plain
`DsnReader` load (no settings overrides). `rules` writes `RulesWriter.write(board, settings, out,
designName)` with the `default` scenario's board and settings. `designName` is the DSN file name
without its last extension.

### `clirun`

Starts the real Java CLI (`app.freerouting.Freerouting`) once per row of `cases/clirun.tsv`, in a
fresh temporary directory with the listed fixture files copied in, always adding
`--user_data_path=<tmp>/userdata -da --gui.enabled=false --api_server.enabled=false
--mcp_server.enabled=false`. A run longer than 120 s aborts the harness. Each JSON has the exit
code, every file in the directory with its size (not `userdata/`), the text of output files under
64 KB, `Auto-routing stage` log lines with every number replaced by `<t>`, selected log lines and
the non-log output. A timestamp in a DRC report is replaced by `<t>` before the size is taken.
