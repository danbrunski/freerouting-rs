# Java reference harness

Runs the Java Freerouting engine on every board in `fixtures/` and stores what it
produced in `fixtures/reference/`, so the Rust port has something concrete to compare
against.

```sh
git -C ../freerouting worktree add --detach <scratch>/java-pin a773a70ec
tools/java-reference/run.sh <scratch>/java-pin load
tools/java-reference/run.sh <scratch>/java-pin route          # bounds from route-bounds.tsv
```

The first argument is a checkout of `danbrunski/freerouting` at the pin, `a773a70ec`
(J01 moved it there from `cda540f` on 2026-10-15). Use a detached worktree, never
`../freerouting` itself: another session edits it, and the script compiles whatever is
checked out. The script needs `python3`, `pip`, `curl` and the Gradle wrapper in that
checkout, which it only uses to resolve dependencies. You don't need a JDK 25 install:
the script takes a Java 25 runtime from the `jdk4py` wheel and compiles with the Eclipse
compiler (ECJ). Set `JAVA=/path/to/java` to use your own Java 25+ instead. Build
products go to `target/java-reference/` (`REFERENCE_WORK_DIR` moves them); the compiled
Java classes are reused, so delete `target/java-reference/classes` after moving the pin.
`REFERENCE_FIXTURES_DIR` and `REFERENCE_OUT_DIR` default to `fixtures` and
`fixtures/reference`; regenerate into a scratch `REFERENCE_OUT_DIR` and copy the finished
set over, so nobody runs the tests against a half-written tree.

## Outputs

| Path | Contents |
| --- | --- |
| `fixtures/reference/load/<board>.json` | Result of `DsnReader.readBoard` (`success`, `outline_missing`, `parse_error`, or `exception`) and the board's `BoardStatistics`: layers, item counts by type, components, pads, nets, incomplete connections, clearance violations, bounds. |
| `fixtures/reference/route/<board>.json` | The batch autorouter's result with the optimizer off, bounded by work (below): `result` (the job state), `routeMillis`, `freeroutingVersion`, `javaRuntime`, `maxPasses`, `maxItems`, `safetyTimeout`, `settings` (the merged `RouterSettings`) and `effective` (per-layer costs, via and ripup costs), `passes` (each pass's board hash and score line), `workLimitCutoffs`, the `BoardStatistics` after routing and the router score. |
| `fixtures/reference/route/<board>.ses` | The SES session file the Java router wrote. |
| `fixtures/reference/optimize/<board>.json`, `<board>.t1.ses`, `<board>.t7.ses` | Mode `optimize` (O06.7, `run.sh <pin-worktree> optimize [NAME_FILTER]`): the batch optimizer (`BatchOptimizer.create(job).runBatchLoop()`, `max_passes` 3, `max_items` 200, SEQUENTIAL, no timeout, a no-op `job.thread`) on a Java-routed session with every trace and via unfixed (`SesReader` imports them user fixed, and no user-fixed item is a candidate), once with `max_threads` 1 and once with 7. Per run: the statistics before, at the start of each pass and after, the optimizer ranking and router scores, the board hash and the run time; the pin, the JDK and `workerFix` (false at the pin: worker boards are reused). Boards: `Issue508-DAC2020_bm08` (with `fixtures/Issue508-DAC2020_bm08-routed.ses`), `Issue026-J2_reference`, `Issue508-DAC2020_bm05` and the three smallest fully routed route references with at least two vias, `Issue954-circle`, `Issue508-SMD-routing-issue-demo` and `Issue269-min_fr_test/min_fr_test_no_quotes` (each with its route-reference session). At `a773a70ec` the t1 and t7 sessions are equal on all six. Java accepts no pass on bm05 (14 incompletes, so guard G1 bypasses the optimizer), on `Issue508-SMD-routing-issue-demo` or on `min_fr_test_no_quotes`: those three give no evidence of the optimizer's moves. Compared by `crates/freerouting/tests/reference_optimize.rs`. |
| `fixtures/reference/optimize/candidates/<board>.jsonl` | Probe `optimize-candidates` (`probes/board/OptimizeCandidatesProbe.java`, O06.2 and O06.3; see Probes). |

### Route bounds

`route` runs the autorouter only (`optimizer.enabled = false`), the way the CLI does, and
bounds each board by work, never by time: `max_passes` and an optional `max_items` per board
come from `route-bounds.tsv` (tab separated, `#` comments, header `board max_passes
max_items`, board = path relative to `fixtures/` without `.dsn`, or the file stem;
`freerouting-dev bench` reads the same file). Every fixture needs a row; a board without one
fails. The run's third argument can instead be a number (that `max_passes` for every
board) or another bounds file, and the fourth a safety timeout (`hh:mm:ss`, default
`00:30:00`; `-` keeps the default), and the fifth a name filter (boards whose path contains
it, or any of its `|`-separated parts). The safety timeout only stops a runaway board: a board
it stops gets no reference (any old `route/<board>.*` is deleted), and the run exits with 1
after listing the failed boards. `REFERENCE_SHARD=k/n` routes every n-th board starting at
the k-th, so n runs with `k = 0..n-1` into the same output dir cover the set in parallel.
The output does not depend on the machine load or on parallel runs, only the timings do.

`workLimitCutoffs` holds `count` (every `WORK_LIMIT_CUTOFF` line Java logged for the board)
and up to 50 `lines`, each parsed into `site`, `nominalMs`, `units`, `context` (the
key=value pairs) and `pass` (the batch pass it happened in, from the "Auto-routing pass #N
... was completed" lines that follow it). A board with `count` 0 is an exact-parity target;
one with cut-offs is compared on aggregates until the B series proves search-tree shape
parity (decision D5).

`route` output is reproducible at a fixed JDK under these bounds: Java's maze search
reseeds its `Random` with the ripup cost before each connection, the pass shuffle is
seeded, and every cut-off inside the router counts work units (search-tree nodes
visited), not time. J01.4 checked this with `tools/determinism-check.sh` (one run idle,
one beside a busy loop per core; byte-identical SES and JSON except `routeMillis`).

`load` and `route` leave out `fixtures/kicad/` (KiCad boards and their DSN exports, which
K07 compares itself; `reference_load` skips them too).

`route` skips boards that already have a `route/<board>.json`, so an interrupted run
picks up where it stopped. Set `REFERENCE_REDO=1` to rerun everything.

### References at `7f56b062d` (Q01.7)

The port adopted Java's fix `7f56b062d` ("Route only a power layer's own plane nets on it",
after the pin; `docs/DIVERGENCES.md`, "Maze search"), which changes Java's own routes on
boards with a power layer. On 2026-10-16 `route` was rerun at `7f56b062d` (a detached
worktree, this harness from the repository's `HEAD`, `-Xmx3g`, `NAME_FILTER` set to the 21
route references whose DSN has a `(type power)` layer). Four came out different and were
replaced; their JSON says `"freeroutingVersion": "7f56b062d"`:

- `route/Issue145-smoothieboard.*` (5 foreign-net wires on `Route2` before, none after),
- `route/Issue753-CPU-85_r104.*` (2 on `In1.Cu` before; it now has 2 work-limit cut-offs, so
  A14 classes it `cutoff`),
- `route/canary-pwr/canary-pwr-9511d9a-in2-open-wirekeepout.*` (1 on `Gnd.Cu` before),
- `route/canary-pwr/canary-pwr-pours-as-planes.*` (1 on `Gnd.Cu` before).

The other 17 were byte-identical and stay at `a773a70ec`. Their dependants were regenerated
at `7f56b062d` the same day: the `route` rows of these four boards in
`crates/freerouting/tests/data/java_drc_digest.tsv` (Java's `FreeroutingCli -drc` on the new
sessions; the `load` rows and every other board are unchanged, and a control rerun of
`Issue269-z10_module` gave its old rows), and `ses/Issue145-smoothieboard.json` with
`ses/routed/Issue145-smoothieboard.ses` (`ses` mode; `ses/unrouted` was byte-identical).
The probe references (`fixtures/reference/probe/`) were rerun at `7f56b062d` the same day with
`probe.sh` (`REFERENCE_WORK_DIR` private): the autoroute kinds `pass`, `insert`, `maze-log`,
`insert-routed` and `maze-routed` on all their boards, and every board-probe kind that has a
reference for `Issue145-smoothieboard` or `Issue753-CPU-85_r104` (they load the route
session). Of 76 runs, 59 came out identical below the header and stay at `a773a70ec`; 17 were
replaced and say `"javaCommit":"7f56b062d"`: `pass`, `insert` and `maze-log` of
`Issue269-min_fr_test/min_fr_test` (the maze search's work units change on its power layer),
and `ids`, `islands`, `scores`, `tree`, `tree-ops` of both boards plus `item-info`,
`pin-connection`, `rules-edit` and `tree-visits` of smoothieboard (the new sessions).
A re-pin past `7f56b062d` makes this note obsolete.

## Known Java failures

- `Issue006-LPC18XX_43XX_SCH.dsn` is not a PCB file (`parse_error` in `load`; its `route`
  job ends `terminated`).
- `Issue179-Autorouter_PCB1_2023-3-24.dsn` throws a `NullPointerException` in Java
  (`exception`; at `a773a70ec`: `Cannot read field "edgeLines"`, in the Delaunay
  triangulation of the incomplete connections).

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
- `probes/board/` holds the board probes (B series): `generate.sh <pin-worktree> <kind|all>` runs
  each `<Kind>Probe.java` on every `fixtures.tsv` row and writes
  `fixtures/reference/probe/<kind>/<board>.jsonl`; see `probes/board/README.md`.
  - `ids` (`IdsProbe.java`, B01.1): `maxGeneratedId`, item count and an id/type hash after load
    and after the session import.
  - `state-hash` (`StateHashProbe.java`, B01.5): which of eight board states `getHash` calls equal.
  - `ledger` (`LedgerProbe.java`, B01.6): `NetRoutingLedger` against a fresh DRC count, step by step.
  - `tree`, `tree-ops`, `tree-visits` (`TreeProbe.java`, `TreeOpsProbe.java`, `TreeVisitsProbe.java`,
    B02): search-tree shape fingerprints after load, session, undo, copies and board operations, and
    the work units of overlap queries.
  - `pull-tight` (`PullTightProbe.java`, O01): the 45° and forced 90° pull-tight steps on seeded
    polylines, `optChangedArea` of whole boards, `pullTight` with pin exits,
    `removeItemsAndPullTight` and a work-limit cut-off. One command:
    `TMPDIR=<scratch> tools/java-reference/probes/board/generate.sh <pin-worktree> pull-tight`.
  - `pull-tight-any` (`PullTightAnyProbe.java`, O02): the any-angle pull-tight (restriction forced
    to `NONE`): each step alone and the fixpoint on the seeded polylines of `pull-tight`, both end
    smoothings, `optChangedArea` of whole boards (without `Issue756-minimal-hang`, which never
    returns), and a work-limit cut-off on a board built in the probe. One command:
    `TMPDIR=<scratch> tools/java-reference/probes/board/generate.sh <pin-worktree> pull-tight-any`.
  - `shove-insert` (`ShoveInsertProbe.java`, O04): seeded forced trace segments, forced vias,
    drill item moves and the `(attach off)` check, with undo after a failure and the canonical
    item dump every 10th step. One command:
    `TMPDIR=<scratch> tools/java-reference/probes/board/generate.sh <pin-worktree> shove-insert`
    (it leaves empty `fixtures/reference/probe/shove-insert/<dir>/` directories for nested
    fixture names; `find fixtures/reference/probe/shove-insert -type d -empty -delete`).
  - `opt-vias` (`OptViasProbe.java`, O05): the via optimizer with the batch autorouter's default
    trace costs (written into the output): `optViaLocation` of every via, then `optChangedArea`
    of whole boards with the costs, on the `pull-tight` board set. One command:
    `TMPDIR=<scratch> tools/java-reference/probes/board/generate.sh <pin-worktree> opt-vias`
    (then `find fixtures/reference/probe/opt-vias -type d -empty -delete`).
  - `optimize-candidates` (`OptimizeCandidatesProbe.java`, O06.2, O06.3): on the board loaded as
    the job does with its routed session imported and every trace and via unfixed, the batch
    optimizer's SEQUENTIAL candidate ids of pass 1 (ten boards), and on O06.7's six boards the
    first 20 candidates each rerouted by `optRouteItemOnBoard` with pass 1's parameters on a fresh
    `deepCopy()` of the baseline (every `ItemRouteResult` field). Its output lives in
    `fixtures/reference/optimize/candidates/`, not `probe/`, and bm08 takes
    `fixtures/Issue508-DAC2020_bm08-routed.ses`, so it is run per board rather than through
    `generate.sh` (which would write `probe/optimize-candidates/`):
    `TMPDIR=<scratch> tools/java-reference/probe.sh <pin-worktree> tools/java-reference/probes/board/OptimizeCandidatesProbe.java fixtures/<board>.dsn <session> fixtures/reference/optimize/candidates/<board>.jsonl`
    with `<session>` = `fixtures/reference/route/<board>.ses` (bm05 takes about 3 minutes).
  - `route-script` (`RouteScriptProbe.java`, I01): interactive routing through
    `gui.interactive.Route` headless, with `RouteState`'s board calls transcribed: seeded starts
    at pins and traces, twelve corner steps towards the nearest target pin with a layer change
    before the ninth, a cancel every fifth route; push on and off, dynamic and stitch. One
    command:
    `TMPDIR=<scratch> tools/java-reference/probes/board/generate.sh <pin-worktree> route-script`
    (then `find fixtures/reference/probe/route-script -type d -empty -delete`).
  - `drag-script` (`DragScriptProbe.java`, I02): drags of seeded vias and pins (a pin drags its
    component) through `board.actions.MoveComponent` and make-space through `gui.interactive.Route`
    on the hidden net, with the drag states' board calls transcribed, on five boards with their
    wiring unfixed. One command:
    `TMPDIR=<scratch> tools/java-reference/probes/board/generate.sh <pin-worktree> drag-script`
    (then `find fixtures/reference/probe/drag-script -type d -empty -delete`).
  - `editor` (`EditorProbe.java`, I03): the board calls of the editor states transcribed
    headless: seeded single-item selections picked and extended four ways, region selections,
    pin swaps (`checkChangeNet` both ways, `Pin.swap`) on rpi_splitter with a logical part added
    to its DSN text, and tiles, polygons and circles (with holes) under each angle restriction
    through `checkShape` + `insertObstacle`; on J2, DAC2020_bm05 and the pin-swap board. One
    command:
    `TMPDIR=<scratch> tools/java-reference/probes/board/generate.sh <pin-worktree> editor`
    (then `find fixtures/reference/probe/editor -type d -empty -delete`).
  - `transform` (`TransformProbe.java`, I04): the move, turn, mirror and copy of a selection with
    `MoveItemState`'s and `CopyItemState`'s board calls transcribed headless and the real
    `ComponentItemTransform`; 12 seeded move trials and 8 copy trials per stage on J2 and
    DAC2020_bm05 (as loaded and with their session) and Issue955-rotated-pad-contacts (as
    loaded). One command:
    `TMPDIR=<scratch> tools/java-reference/probes/board/generate.sh <pin-worktree> transform`
    (then `find fixtures/reference/probe/transform -type d -empty -delete`).

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
