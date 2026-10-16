# J01 — Re-pin to the deterministic Java router

Opened 2026-10-15. Dan's Java fork is replacing every wall-clock limit in the
router with a work-count limit. The affected cuts are the fanout per-pin
limit, the 1 s pull-tight guard after each connection and in `removeTails`,
the maze search, `TraceShover`, `DrillItemMover`, and the interactive
`Route`/`MoveComponent` limits. After that commit, the same jar, DSN and
settings give a byte-identical SES at any machine load. This checklist moves
the port's pin to that commit and regenerates the reference data, so the
autorouter port ([A series](A00-autorouter-README.md)) is checked against exact
Java routes instead of a noisy baseline. Decision D3 in
[port-completion.md](../port-completion.md) makes this a prerequisite for
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

- [ ] **J01.1 — Inventory the Java changes since `cda540f`.**
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

- [ ] **J01.2 — Port the changes that touch already-ported code.**
  Apply each "already ported" change from J01.1 to its Rust counterpart, or
  record why it has no Rust effect. Search-tree work counting is **not** done
  here. B02.7 and B03.7 port it (decision D5), so J01.2 only notes
  where Java counts. A behaviour change in ported code gets a test that pins
  it. A change the port deliberately doesn't take gets a `DIVERGENCES.md` line.
  - **Accept:** `cargo test --workspace` green. Each ported change is named
    with its test, and each non-port with its DIVERGENCES line.

- [ ] **J01.3 — Bound the route harness by work.**
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

- [ ] **J01.4 — Prove the Java references are deterministic.**
  For each board, run the harness's `route` mode through
  [F02.4](F02-parity-and-benchmark-harness.md)'s
  `tools/determinism-check.sh`. That routes it twice into the scratchpad,
  once idle and once beside one CPU burner per core, and compares the SES
  byte for byte. Also compare the JSON fields except timing.
  - **Accept:** on at least 8 fixtures spanning small to large, including
    `Issue145-smoothieboard`, `Issue753-CPU-85_r104`,
    `Issue555-CNH_Functional_Tester_1`, `Issue269-z10_module` and one board
    whose run records cut-offs, every pair is identical. Record the board list
    and load averages in the done note. A difference means the Java change is
    incomplete. Report it to the Java session, mark this item partial, and
    don't regenerate references.

- [ ] **J01.5 — Regenerate the references at the new pin.**
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

- [ ] **J01.6 — Move the pin in the docs and fix the stale claims.**
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
