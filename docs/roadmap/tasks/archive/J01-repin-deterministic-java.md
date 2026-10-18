# J01 — Re-pin to the deterministic Java router

Opened 2026-10-15. The Java fork is replacing every wall-clock limit in the
router with a work-count limit. The affected cuts are the fanout per-pin
limit, the 1 s pull-tight guard after each connection and in `removeTails`,
the maze search, `TraceShover`, `DrillItemMover`, and the interactive
`Route`/`MoveComponent` limits. After that commit, the same jar, DSN and
settings give a byte-identical SES at any machine load. This checklist moves
the port's pin to that commit and regenerates the reference data, so the
autorouter port ([A series](../A00-autorouter-README.md)) is checked against exact
Java routes instead of a noisy baseline. Decision D3 in
[port-completion.md](../../port-completion.md) makes this a prerequisite for
route-level parity work.

**The Java commit has landed:** `30baad7e2`, "Make routing independent of
machine load", on 2026-10-15. Its ledger line in
`../freerouting/docs/development/PORT_LEDGER.md` (read it with
`git -C ../freerouting show main:docs/development/PORT_LEDGER.md`) says
"Port: yes: use the same budgets and count the same units (tree nodes
visited)". The change:
- every former `TimeLimit` site is a `WorkLimit`, counting 64,000 search-tree
  node visits per nominal millisecond on a per-thread `WorkMeter`;
- the polygon split and the Delaunay triangulation use a local seeded
  `Random`;
- `NetIncompletes` groups items in input order.
Its A/B against `60d5caa06` (5 passes, optimizer off) is in the commit
message. bm01, canary-pwr and CPU-85 gave byte-identical results.
smoothieboard went from 7 to 8 unrouted, from 490 to 495 vias, and from 326
to 285 CPU s.

**Not covered yet:**
- Java's optimizer worker boards still reuse state across candidates, so the
  ledger says to compare optimizer output on aggregates only until a later
  line changes that.
- The default V2 optimizer score clamps to 0 on bm01, so Java's optimizer
  rejects every pass there. Optimizer references may show the optimizer
  doing nothing for that reason.

**The pin is `a773a70ec`** (main of `../freerouting`, committed locally and not
pushed when this was written; a cloud session needs it pushed first). It
adds three commits on top of `30baad7e2`, each with a ledger line:
- `68a9efed0` "Rank optimizer passes by the unclamped score". On fully
  routed bm01 the optimizer now accepts its passes: vias 142 → 127, bends
  1237 → 1171, length +0.2 %, 0 violations. bm02 is identical.
- `7bef8995f` "Write DSN output headlessly instead of an empty file".
- `a773a70ec` "Log a machine-readable line when a work limit cuts off".
  Routing is unchanged: bm01 at 5 passes gives the same SES as `30baad7e2`.
The Java session keeps a frozen jar at
`~/code/freerouting/logs/jars/freerouting-a773a70ec.jar`.
Later ledger commits are bug fixes only (the Java fork's rule since
2026-10-15). If J01 runs after more of them land, pin to the latest one
that the ledger marks as not changing routing, and list them in J01.1.
At the time of writing the newest is `46bf9e2ef`, "Add router.min_trace_width_um to
stop neckdowns below the board minimum". It includes `07c970db9` and is
byte-identical to `a773a70ec` while the new setting is unset. `07c970db9`,
"Skip keepouts and planes whose shape cannot be read", is one such commit:
Java now skips such areas with a warning instead of throwing, as the port
already does. Its ledger line says "Port: no". J01.4 also needs
[F02.4](F02-parity-and-benchmark-harness.md)'s determinism script.

## What exists and what this adds

| Exists | State at `cda540f` |
|---|---|
| `tools/java-reference/run.sh` + `ReferenceDump.java` | `load` is deterministic. `route` runs the batch autorouter **and** the optimizer, bounded by 100 passes and a 45 s wall-clock timeout |
| `fixtures/reference/route/*.json` + `*.ses` | 118 boards: 69 `completed`, 47 `timed_out`, 1 `exception`, 1 `terminated`. The timed-out ones depend on machine load |
| `fixtures/reference/load/*.json` | Loaded-board statistics for every fixture |
| `crates/freerouting/tests/data/java_drc_digest.tsv` | Java DRC report digests at `cda540f` (see `java_drc_digest.py`) |
| Docs naming the pin | `PORT_PLAN.md`, `DIVERGENCES.md`, `tools/java-reference/README.md`, `run.sh`, `docs/development/engine.md` |

What this adds:
- a pin at the deterministic commit;
- route references bounded by work (passes and items), never by time, with the
  optimizer off;
- a record of every Java work-limit cut-off per board, so parity tests know
  which runs to compare exactly;
- a determinism check of the Java references themselves;
- corrected docs: three of them still say Java's maze search is unseeded. It
  isn't. It is reseeded with the ripup cost before every connection.

## Decisions made here

- **Route references run the autorouter only, with the optimizer off.** The A
  series needs autorouter parity first. Optimizer references are a separate
  output that the O series adds (`fixtures/reference/optimize/`), so one doesn't
  mask the other.
- **Work bounds, not time.** Each board routes with a fixed `max_passes`, plus
  `max_items` on boards that would otherwise take too long. The wall-clock
  timeout stays only as a safety stop, set far above the expected time. A
  board whose safety timeout fires is a harness failure, not a reference. Pick
  the bounds so the whole `route` regeneration takes at most about 2 h on this
  machine, and record them per board in its JSON.
- **Cut-offs are data.** The Java fork logs every WorkLimit cut-off. The
  harness stores the count and the log lines per board (`workLimitCutoffs`). A
  board with zero cut-offs is an exact-parity target from the start. One with
  cut-offs becomes one once the B series proves tree-shape parity (decision
  D5); until then it is compared on aggregates.
- **Same JDK for references and probes.** Use the `jdk4py` runtime that
  `run.sh` installs, and record its version in each JSON. `Math.sin`/`cos` can
  differ by one ulp across JVMs and CPUs (D6).
- **Never run against the Java working tree.** The harness builds from a
  detached worktree of the new pin in the scratchpad. Another session edits
  `../freerouting`, and `run.sh` compiles whatever is checked out.

## File ownership

| Item | New files | May edit |
|---|---|---|
| J01.1 | — | — (report only) |
| J01.2 | — | engine crates (only the ported counterparts of changed Java code), `docs/DIVERGENCES.md` |
| J01.3 | — | `tools/java-reference/src/reference/ReferenceDump.java`, `tools/java-reference/run.sh`, `tools/java-reference/README.md` |
| J01.4 | — (uses F02.4's `tools/determinism-check.sh`) | — |
| J01.5 | `fixtures/reference/route/*` (regenerated) | `fixtures/reference/load/*`, `crates/freerouting/tests/data/java_drc_digest.tsv`, the reference tests' `EXPECTED_MISMATCHES` |
| J01.6 | — | `docs/PORT_PLAN.md`, `docs/DIVERGENCES.md`, `docs/development/engine.md`, `tools/java-reference/README.md` |

## Checklist

- [x] **J01.1 — Inventory the Java changes since `cda540f`.**
  Read `git -C ../freerouting log --oneline cda540f..<new pin>` and
  `git -C ../freerouting log -p cda540f..<new pin> -- src/main` through git,
  never the working tree. Also read the ledger
  (`git -C ../freerouting show main:docs/development/PORT_LEDGER.md`). Sort
  every changed Java file into one of three groups:
  - **already ported:** geometry, DSN, board model, rules, search trees, DRC.
    These need a matching Rust change in J01.2.
  - **not ported yet:** autoroute, optimize, board operations, GUI. Note what
    the A, O and I series must take from the new code, such as the
    `WorkMeter` hooks in the search trees and the `WorkLimit` call sites.
  - **no effect:** docs, tests, tooling.
  Known so far:
  - `de7a4fd2c` changes the GUI batch loop so it saves the best board. The
    ledger says "Port: yes" for the Rust GUI session, so that goes to the A
    and G series.
  - `30baad7e2` touches 31 files, including:
    - `ShapeSearchTree`, its 45° and 90° variants, and `MinAreaTree`
      (work counting);
    - the new `datastructures/WorkMeter.java` and `WorkLimit.java`, with
      `TimeLimit.java` deleted;
    - `PolygonShape` and `PlanarDelaunayTriangulation` (a local seeded
      `Random`; the Rust ports already create one per call);
    - `NetIncompletes` (input order, which Rust already uses);
    - `TraceShover`, `TraceTightener*`, `DrillItemMover`, `ForcedPadRouter`,
      `MoveComponent`, `RoutingBoard`, `RoutingBoardOperations`, `Route` and
      `FanoutSettings`;
    - new tests `WorkLimitTest` and `WorkLimitDeterminismTest`. Port both
      in B02 and A11+.
  - `07c970db9` (if the pin includes it): Java skips keepouts and planes
    whose shape can't be read, instead of throwing a NullPointerException.
    In DIVERGENCES.md's board-model line "Where Java ends with an exception
    the port keeps going …", drop keepouts and planes; windows and the
    other cases in that line still diverge.
  - **Accept:** the inventory table (file, group, what Rust must do, which
    series) is in the checklist's done note. Every changed `src/main` file
    appears in it.
  *Done 2026-10-15:* pin `a773a70ec` (orchestrator, confirmed by the Java
  session). `git -C ../freerouting log --oneline cda540f..a773a70ec`: `de7a4fd2c`,
  `60d5caa06`, `c38e9e4fd`, `30baad7e2`, `68a9efed0`, `7bef8995f`, `a773a70ec`.
  After the pin, on `main`: `07c970db9` (ledger "Port: no", the port already drops
  unreadable keepouts and planes, so the DIVERGENCES board-model line stays as it is)
  and `46bf9e2ef` (`router.min_trace_width_um`, byte-identical while unset; C02 models
  the field as unset). `git diff --stat cda540f a773a70ec -- src/main` lists 34 files:

  | Java file (`src/main/java/app/freerouting/`) | Group | What Rust must do | Series |
  |---|---|---|---|
  | `geometry/planar/PolygonShape.java` | already ported | Nothing: `polygon_shape.rs` already makes a fresh `JavaRandom::new(99)` per `split_to_convex` and threads it through the recursion, the sequence Java now has | J01.2 (no change) |
  | `datastructures/PlanarDelaunayTriangulation.java` | already ported | Nothing: `planar_delaunay_triangulation.rs` shuffles with a fresh `JavaRandom::new(99)` per triangulation | J01.2 (no change) |
  | `drc/NetIncompletes.java` | already ported | `calculateNetItems` uses a `LinkedHashSet`: groups start in input order, as Rust already did; within a group the items follow the connected set's `TreeSet` (descending id), which Rust did not | J01.2 (ported) |
  | `core/scoring/BoardStatistics.java` | already ported | New `getOptimizerRankingScore` (V2 score without the clamp at 0). `getOptimizerScore` is unchanged, so `optimizer_score_v2` stays as it is; the ranking score is read only by `BatchOptimizer` | O06 |
  | `datastructures/MinAreaTree.java` | already ported (tree), not ported (counting) | `WorkMeter.tick()` per node popped in `overlaps` (`overlapsUnlocked`) | B02.7 |
  | `board/searchtree/ShapeSearchTree.java` | already ported (tree), not ported (counting) | tick per node popped in the general `completeShape` traversal | B03.7 |
  | `board/searchtree/ShapeSearchTree45Degree.java`, `ShapeSearchTree90Degree.java` | already ported (tree), not ported (counting) | tick per node popped in their `completeShape` traversals | B03.7 |
  | `settings/FanoutSettings.java` | already ported (settings) | Doc only: `max_milliseconds_per_pin` is a work budget in nominal ms | A12 (use it as work) |
  | `datastructures/WorkLimit.java`, `WorkMeter.java` (new), `TimeLimit.java` (deleted) | not ported | A per-thread work meter (64,000 units per nominal ms) and limits that log `WORK_LIMIT_CUTOFF site=... nominal_ms=... units=...` once; port both with `WorkLimitTest` | B02.7 (meter), A11 (limits, log line) |
  | `autoroute/maze/AutorouteEngine.java` | not ported | maze-search `WorkLimit` (site `maze_search`, context `net item ripup_pass`) | A04, A11 |
  | `autoroute/pipeline/AutorouteConnectionRouter.java` | not ported | 1 s pull-tight guard after each connection as a `WorkLimit` (`pull_tight`, `nets=[..]`) | A11, O01 |
  | `autoroute/pipeline/BatchAutorouter.java` | not ported | `ENDLESS_LOOP_GUARD_NOMINAL_MS = 1000` for `removeTails` (rename of the time limit) | A13 |
  | `autoroute/pipeline/BatchFanout.java` | not ported | per-pin `WorkLimit` (`fanout_pin`, `net item pin pass`), budget `maxMillisecondsPerPin * pass` | A12 |
  | `autoroute/pipeline/BatchOptimizer.java` | not ported | rank passes by `getOptimizerRankingScore`, improvement relative to the score's magnitude, final restore by it | O06 |
  | `board/actions/DrillItemMover.java`, `board/optimize/TraceShover.java` | not ported | take the caller's `WorkLimit` instead of a `TimeLimit` | O04 |
  | `board/actions/ForcedPadRouter.java` | not ported | same, under the fanout limit | A12 |
  | `board/actions/MoveComponent.java` | not ported | `move_component` limit | I02 |
  | `board/facade/RoutingBoard.java`, `RoutingBoardOperations.java` | partly ported (`RoutingBoard` facade), the changed methods not ported | `TimeLimit` → `WorkLimit` in the shove, pull-tight, fanout and autoroute entry points | O01, O04, A11, A12 |
  | `board/optimize/TraceTightener.java`, `TraceTightener45.java`, `TraceTightener90.java`, `TraceTightenerAnyAngle.java` | not ported | the pull-tight stop check reads a `WorkLimit` | O01, O02 |
  | `gui/interactive/Route.java` | not ported | `interactive_route` limit | I01 |
  | `gui/workspace/ports/BoardReplacementPort.java`, `WorkspacePortAdapter.java`, `gui/workspace/progress/GuiRoutingJobWorker.java` | not ported | `de7a4fd2c`: the GUI batch loop forwards every board replacement, so the saved board is the best one (ledger "Port: yes") | A13, G07 |
  | `cli/CliRunner.java` | not ported (CLI routing) | `7bef8995f`: an empty main output is an error, not a 0-byte file | C07 |
  | `management/HeadlessBoardManager.java`, `management/jobs/RoutingJobSchedulerActionThread.java` | not ported | `7bef8995f`: `-do board.dsn` writes the routed board through `DsnWriter`; formats that can't be written headless log an error | C06, C07 |

  No effect: `.claude/agents/implementer.md`, `docs/**` (handover, ledger, settings,
  shelved neckdown, measurement tools). Tests: `WorkLimitTest` (B02.7),
  `WorkLimitDeterminismTest` (A11+), `BoardStatisticsTest` and
  `BatchOptimizerCandidateGateTest` additions (O06), `GuiFinalBoardTest` (G07),
  `JobOutputSerializationTest` (C06/C07).

- [x] **J01.2 — Port the changes that touch already-ported code.**
  Apply each "already ported" change from J01.1 to its Rust counterpart, or
  record why it has no Rust effect. Search-tree work counting is **not** done
  here. B02.7 and B03.7 port it (decision D5), so J01.2 only notes
  where Java counts. A behaviour change in ported code gets a test that pins
  it. A change the port deliberately doesn't take gets a `DIVERGENCES.md` line.
  - **Accept:** `cargo test --workspace` green. Each ported change is named
    with its test, and each non-port with its DIVERGENCES line.
  *Done 2026-10-16:* one ported change: `NetIncompletes.calculateNetItems` now lists the
  items of each group in connected-set order (`BTreeSet<ItemId>`, descending id, as Java's
  `TreeSet`), groups still starting in input order (`crates/freerouting/src/drc/incompletes.rs`;
  test `drc::incompletes::tests::net_items_group_in_input_order_and_list_by_descending_id`;
  the DIVERGENCES DRC sentence about `NetIncompletes` now says it matches Java since
  `30baad7e2`). No Rust effect, so neither a port nor a divergence: `PolygonShape` and
  `PlanarDelaunayTriangulation` (Rust already seeds a fresh `JavaRandom::new(99)` per
  call), `getOptimizerRankingScore` (only `BatchOptimizer` reads it: O06),
  `FanoutSettings` (doc), and the search-tree `WorkMeter` ticks (B02.7, B03.7 by D5).
  `cargo test` of `freerouting` (lib and every test but the uncompilable in-flight
  `autoroute_probe_start_rooms`), `freerouting-geometry`, `-dsn`, `-dev` and `-cli`: all
  green except `board_probe_state_hash` (Issue413 `h7`, a B01 gap the new session exposes;
  see J01.5). `reference_drc`, with the change and the `cda540f` digest, was green before
  the re-pin too.

- [x] **J01.3 — Bound the route harness by work.**
  Change `ReferenceDump`'s `route` mode as follows:
  - turn the optimizer off (`settings.optimizer.enabled = false`)
  - take `max_passes` and an optional `max_items` per board from a committed
    file, `tools/java-reference/route-bounds.tsv`
  - keep a timeout only as a safety stop (default 30 min)
  - write these fields into each JSON: `maxPasses`, `maxItems`,
    `safetyTimeout`, `javaRuntime` (version string), the effective
    settings that change results (edge clearance, ripup costs), the
    per-pass score lines, `workLimitCutoffs` (count + the logged lines, capped at 50), and
    `result`
  - fail the board, without writing a reference, when the safety timeout
    fires
  The cut-off lines. The Java session implemented this format; it lands with
  the clamp-fix commit, which is the pin. Java logs one line per WorkLimit
  instance at INFO, after the usual `<date> <time> INFO   ` prefix. There is
  no `[<job>]` topic in these lines, so anchor on the marker, not on the line
  start:
  ```
  <date> <time> INFO   WORK_LIMIT_CUTOFF site=<site> nominal_ms=<n> units=<u>[ <key>=<value>...]
  WORK_LIMIT_CUTOFF site=(\S+) nominal_ms=(-?\d+) units=(\d+)(?: (.*))?$
  ```
  Example from bm01 at 5 passes, optimizer off. It has three lines of
  `WORK_LIMIT_CUTOFF site=pull_tight nominal_ms=1000 units=64455759 nets=[]`,
  so bm01 is **not** a zero-cut-off board.
  - `units` is the work done when the cut-off was detected. It can overshoot
    `nominal_ms × 64,000`, because the checks are coarse.
  - Sites and their context:
    - `maze_search`: `net=<n> item=<id> ripup_pass=<p>`;
    - `fanout_pin`: `net=<n> item=<pin id> pin=<Ref-Pad> pass=<fanout pass>`;
    - `pull_tight`: `nets=[a, b]` (`[]` = all nets). This covers both the
      guard after a connection and the `remove_tails` one;
    - `move_component`, `interactive_route`: no context, GUI only.
  - TraceShover, DrillItemMover and ForcedPadRouter never create a limit of
    their own. They report under the `maze_search` or `fanout_pin` limit they
    were handed.
  - The batch pass is not on the line. Lines for pass N come before that
    pass's `Auto-routing pass #N ... was completed` line, so the harness
    assigns each line to a pass by counting those.
  Store the parsed fields per board: site, nominal ms, units, the context
  map, and the pass.
  Choose the bounds:
  - Route each fixture once with a high pass limit and record the time.
  - Then set per-board bounds so the full run is about 2 h or less.
  - Keep `max_passes` at least 3 wherever that fits, so ripup and restore
    paths are exercised.
  - **Accept:** `run.sh <pin> route` on three small fixtures writes the new
    fields; a board forced past a tiny safety timeout is reported as a failure
    and leaves no `route/<board>.json`. `route-bounds.tsv` covers every fixture
    and has a header comment explaining the choice.
  *Done 2026-10-16:* `ReferenceDump` `route` takes `[BOUNDS|-] [SAFETY_TIMEOUT|-]
  [NAME_FILTER]`: BOUNDS is a `route-bounds.tsv` (default: the committed one, passed by
  `run.sh` as `-Dreference.routeBounds`) or one `max_passes` for every board; the
  optimizer is off (`optimizer.enabled = false` in the priority-80 harness source);
  `max_items` sets `autorouter.max_items`; the safety timeout defaults to `00:30:00`.
  Each JSON has `result`, `routeMillis`, `freeroutingVersion`, `javaRuntime`
  (`OpenJDK 64-Bit Server VM 25.0.2+10-LTS`), `maxPasses`, `maxItems`, `safetyTimeout`,
  `settings` (the merged `RouterSettings`, threads normalized as in C01), `effective`
  (per-layer costs, via, plane-via and start ripup costs), `passes` (pass, board hash,
  score line per "Auto-routing pass #N ... was completed"), `workLimitCutoffs`
  (`count` and up to 50 `lines`, each with `pass`, `site`, `nominalMs`, `units`,
  `context`, `line`), the statistics and the router score. A board without a bounds row,
  or whose safety timeout fires, gets no JSON or SES (stale ones are deleted), and the
  run lists it and exits 1. Extras: `REFERENCE_SHARD=k/n` for parallel runs, a
  `|`-separated NAME_FILTER, and `run.sh` recompiles the harness only when it is older
  than its source or the classes. Checked with `REFERENCE_OUT_DIR=<scratch>
  run.sh <a773a70ec worktree> route 5 - <board>` on `Issue508-SMD-routing-issue-demo`,
  `Issue368-CorneyIslandWireless_input_design` and `Issue954-circle` (all fields
  written; the first: 3 passes, `555.56 (2 unrouted ...)` then `1000.00`, 0 cut-offs),
  and `run.sh ... route 5 00:00:02 Issue508-DAC2020_bm01.dsn`: `route FAILED:
  Issue508-DAC2020_bm01.dsn: the safety timeout of 00:00:02 fired after 2314 ms`, exit
  1, no JSON. `tools/java-reference/route-bounds.tsv` has a row for every fixture DSN
  (170) and a header comment with the rule: 112 boards that stop on their own within
  90 s keep 100 passes, the others get the passes that fit in 45 s (at least 3; the 20
  whose pass 1 took over a minute get 3); no `max_items`. Its columns are `board
  max_passes max_items note`; `freerouting-dev bench`'s `bounds_for` now also matches
  the fixtures-relative name, and
  `freerouting-dev/tests/bench.rs::committed_route_bounds_cover_every_fixture` checks
  that every fixture DSN has a row.

- [x] **J01.4 — Prove the Java references are deterministic.**
  For each board, run the harness's `route` mode through
  [F02.4](F02-parity-and-benchmark-harness.md)'s
  `tools/determinism-check.sh`. That routes it twice into the scratchpad,
  once idle and once beside one CPU burner per core, and compares the SES
  byte for byte. Also compare the JSON fields except timing.
  The harness writes to `fixtures/reference`, so call `reference.ReferenceDump`
  directly with `{OUT}` as its output directory, and drop the timing lines with
  `--ignore '"routeMillis"'`; the script's header has the command line for a
  `load` dump, and `route` takes the same with `route` and the bounds.
  - **Accept:** on at least 8 fixtures spanning small to large, including
    `Issue145-smoothieboard`, `Issue753-CPU-85_r104`,
    `Issue555-CNH_Functional_Tester_1`, `Issue269-z10_module` and one board
    whose run records cut-offs, every pair is identical. Record the board list
    and load averages in the done note. A difference means the Java change is
    incomplete. Report it to the Java session, mark this item partial, and
    don't regenerate references.
  *Done 2026-10-16:* one `tools/determinism-check.sh --out <scratch>/det --ignore
  '"routeMillis"' -- env JAVA_TOOL_OPTIONS= <jdk4py java> -Xmx6g -Djava.awt.headless=true
  -cp <harness:classes:resources:deps> reference.ReferenceDump route <fixtures> {OUT}
  tools/java-reference/route-bounds.tsv - "<8 boards, |-separated>"` run, the 8 boards
  routed one after another in each run, with the committed bounds, at `a773a70ec`:
  `Issue034-Green14SegLED` (100 passes allowed, stops at 10, 0 cut-offs),
  `Issue508-SMD-routing-issue-demo` (stops at 3, 0), `Issue297-myboard` (12, 0),
  `Issue508-DAC2020_bm01` (3, 0), `Issue555-CNH_Functional_Tester_1` (stops at 18, 0),
  `Issue753-CPU-85_r104` (3, 0), `Issue269-z10_module` (3 passes, **11 cut-offs**) and
  `Issue145-smoothieboard` (3 passes, **8 cut-offs**, 927 s). Result `identical` (SES
  byte for byte, JSON except `routeMillis`), exit 0. Load averages (the machine was
  shared with other sessions all along): idle run 22.82 before, 23.85 after; loaded run
  (8 burners) 23.54 before, 34.49 after. The same 8 JSONs also equal those of the J01.5
  run, which routed 3 boards at a time beside this check.

- [x] **J01.5 — Regenerate the references at the new pin.**
  Run `load`, then `route` (as a long run; see the implementer's long-run
  pattern), then regenerate the DRC digest the way
  `crates/freerouting/tests/data/java_drc_digest.py` describes. Delete
  `target/java-reference/classes` first, because `run.sh` reuses that cache.
  - **Accept:**
    - every fixture has a `route/<board>.json` with `result` `completed`
      (or the Java failure recorded in the README's "Known Java failures")
    - `cargo test --workspace` green, with any new expected mismatch in the
      test that would fail, its reason, and a matching DIVERGENCES line
    - the done note gives total route time and the number of boards with zero
      work-limit cut-offs, which are the exact-parity set
  *Done 2026-10-16:* `board_probe_state_hash` (Issue413 `h7`) is the documented
  `smallestClearance` divergence: `BoardStatistics` runs Java's `clearanceViolations()`,
  which writes that cache into Java's hash. It is now an `EXPECTED_MISMATCHES` entry with
  that reason and the DIVERGENCES line names the step (orchestrator, 2026-10-16); the test
  passes (4 passed). From a
  detached `a773a70ec` worktree, with a private
  `REFERENCE_WORK_DIR` (the shared `target/java-reference/classes` stays at `cda540f` for
  the sessions using it) and a scratch `REFERENCE_OUT_DIR`, copied over
  `fixtures/reference/` in one step. `load`: 170 boards, byte-identical to the `cda540f`
  ones plus the 4 new canary-pwr boards. `route` (3 shards): 168 `completed`,
  `Issue006-LPC18XX_43XX_SCH` `terminated` and `Issue179-Autorouter_PCB1_2023-3-24`
  `exception` (both in "Known Java failures"). Four boards hit the 30 min safety timeout
  at load 25-30 and were rebounded and rerun with `02:00:00` (recorded in their JSON):
  `Issue289-...FHT-8086` and `-VGA` (2 passes), `Issue208-freerouting` and
  `Issue219-LogicBoard_smt` (1 pass). **Total route time:** 18,223 s summed over the
  boards, 8,084 s wall for the 3-shard run plus about 36 min of reruns, all at load 20-35
  from other sessions. **Zero work-limit cut-offs: 153 of the 168 completed boards** (the
  exact-parity set); 15 have cut-offs (Issue066 7, Issue070 1, Issue113 1, Issue145 8,
  Issue214 1, Issue219 74, Issue269-caniot 3, Issue269-z10 11, Issue420 128,
  DAC2020_bm01.unrouted 3, canary-pwr 9511d9a 2, pours-as-planes 11, pours1 1, pours2 7,
  pours3 2). C01 dumps rerun (`settings`, `rules`, `dsn`, `argv`, `clirun`, then `ses`):
  `settings`/`rules` gain the 4 canary boards, `dsn`, `argv` and `cases` are unchanged,
  `clirun/02-out-dsn.json` changes (`7bef8995f`: Java writes an 8,550-byte DSN and exits
  0), and 9 `ses` dumps change with their sessions. DRC digest regenerated with
  `FreeroutingCli` at the pin (2,022 lines; the load part equals the old one apart from
  10 boards new to it and `track_dangling` representatives). Board probes regenerated at
  the pin against the new sessions: `ids`, `state-hash`, `ledger`, `islands`,
  `board-queries`, `tree`, `tree-ops`, `tree-visits`, `scores`, `entry-side`,
  `trace-entries`, `shove-check` (B05's and B08's kinds left to them). Test updates:
  `reference_drc` drops the two `Issue289-...VGA` entries (the race no longer shows with
  the new session; DIVERGENCES line updated); `ses_round_trip` lists `Issue420` in
  `REIMPORT_FEWER_WIRES` (Java does the same: its import makes 3,933 traces of 3,934 wires,
  `SesImportStats` at the pin); `freerouting-dev/tests/ses_diff.rs` takes its bm01 numbers
  from `SesImportStats` at the pin (528 wires, incomplete 24/25, bends 705/704).
  `reference_load`, `reference_drc`, `settings_reference`, `job_prepare_reference`,
  `ses_writer`, `ses_round_trip`, `freerouting-dsn` (`java_style_ses`) green. Still red:
  `board_probe_state_hash` on Issue413 (Java's hash changes after `new BoardStatistics`
  on the deep copy with the new session, Rust's does not; with the old session at
  `a773a70ec` Java gives the old `[5,6]`), a B01 parity gap for B01 to fix.

- [x] **J01.6 — Move the pin in the docs and fix the stale claims.**
  - Update the pinned commit everywhere that names it: `PORT_PLAN.md`,
    `DIVERGENCES.md`, `tools/java-reference/README.md`, `run.sh` and
    `engine.md`.
  - Fix the claims that Java's maze search uses an unseeded `Random`
    (PORT_PLAN "Determinism", the reference README, engine.md "Regenerating
    the reference"). The search is reseeded with the ripup cost before each
    connection. At `cda540f` its random factor never fires anyway:
    `ripupPassNo` never exceeds 1 (`MazeRipupResolver.java:158`).
  - Fix the claim that route output isn't reproducible. It is, at a fixed
    JDK, under work bounds.
  - Update the implementer agent's pin (`.claude/agents/implementer.md`).
  - **Accept:** `git grep -n cda540f` shows only history: DIVERGENCES entries
    that describe Java at the old pin, and commit messages. The done note
    lists every file changed.
  *Done 2026-10-16:* pin `a773a70ec` in `docs/PORT_PLAN.md` (source line; the
  "Determinism" bullet now says Java routing is reproducible, its maze `Random` is
  reseeded with the ripup cost before each connection and its random factor never fires,
  and its cut-offs are `WorkLimit` work budgets; the "Cancellation" bullet no longer
  calls them time limits), `docs/DIVERGENCES.md` (header; the DRC `NetIncompletes`
  sentence), `tools/java-reference/README.md` (intro, outputs, "Route bounds", the
  reproducibility paragraph replacing "route output isn't", known Java failures),
  `tools/java-reference/run.sh` (header), `docs/development/engine.md` ("The Java
  reference": pin, re-pin steps incl. the DRC digest, the board probes and this agent
  file; "Regenerating the reference": worktree commands, work bounds, the paragraph that
  said route output is not reproducible; the router commit range `fc35bbd0..a773a70ec`),
  `.claude/agents/implementer.md` (six pin mentions; the ledger and determinism bullets),
  `crates/freerouting/tests/data/java_drc_digest.py` (docstring) and
  `fixtures/canary-pwr/README.md`. `git grep -n cda540f` in those files now finds only
  history: "before that `cda540f`" (engine.md, PORT_PLAN, the README), the DIVERGENCES
  header note, the `NetIncompletes` and `-do x.dsn` entries describing Java at
  `cda540f`, and engine.md's `fc4bd20`/`cda540f` comparison. Elsewhere `cda540f` stays
  where a checklist, a code comment or a test names the commit a fact was checked at
  (A00 says line numbers stay at `cda540f`); those files belong to other checklists.
