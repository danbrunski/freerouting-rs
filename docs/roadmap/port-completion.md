# Completing the port

Recorded 2026-10-15. This is the plan for taking freerouting-rs from a board
viewer with DRC to a full replacement for the Java Freerouting: a headless
router the KiCad plugin can call, a desktop app with the Java feature set, and
release builds for Windows, macOS and Linux. The detailed implementation
checklists are in [tasks/](tasks/README.md). The module map and the original
porting order stay in [PORT_PLAN.md](../PORT_PLAN.md). This document replaces
PORT_PLAN's milestones 6 to 11 as the working plan.

**Owner steer (2026-10-15): the port should perform better than Java, and
it does not need to match the Java structure.** Java stays the reference for
*results*: what gets routed, where, with which vias, and what the DRC reports.
How the Rust code reaches those results is free: data layout, module
boundaries, ownership, caching and parallelism are chosen for speed, not to
mirror Java classes. See decision D1.

**Owner steer (2026-10-15): take the best of both worlds.** The engine
offers a choice of algorithms. It pulls them from Freerouting and from
[KiCadRoutingTools](https://github.com/drandyhaas/KiCadRoutingTools) (KRT,
MIT-licensed, at `../KiCadRoutingTools`). KRT is a grid-based A* router with
a Rust core. Freerouting lacks several of KRT's features: differential pairs,
length matching, power-net neck-down at fine-pitch pads, plane repair,
routing diagnostics and direct `.kicad_pcb` I/O. KRT lacks Freerouting's
push-and-shove and blind/buried vias. See decision D9 and the X series.

## Where the port stands

| Stage | State | Evidence |
|---|---|---|
| Geometry (`geometry.planar`) | done | Ported Java tests plus differential tests against compiled Java; all match |
| DSN/SES/rules files | done | 160 of 161 fixture DSN files parse and round-trip |
| Board model, rules, library, search trees, DSN → board | done for loading | `tests/reference_load.rs`: all 159 boards Java loads match on every compared `BoardStatistics` field |
| DRC, incompletes, airlines, SES import, `-drc` CLI | done, two loose ends | `tests/reference_load.rs`, `tests/reference_drc.rs`; Java DRC unit tests not all ported; copper islands not compared |
| Board viewer GUI: session model, GPU renderer, ratsnest, DRC markers and info lists | done (G01–G03) | `docs/development/gui.md`; the windows and dialogs are G04–G06 |
| KiCad plugin | hardened, waiting on routing (K01 done) | `docs/development/kicad-plugin.md` |
| Board editing operations used by the router (shove, forced vias and pads, drill moves, pull-tight) | not started | B and O series |
| Autorouter (`autoroute.*`, ~18k Java lines) | support classes done (A08: history, failure log, events, cancel token); search core not started | A series |
| Optimizer (`board.optimize`, `BatchOptimizer`, ~5k lines) | not started | O series |
| Interactive editing (`gui.interactive`, ~5k lines) | not started | I series |
| Settings model, settings applied to the board, Java reference dumps, SES writer | done (C01, C02, C03, C05) | `tests/settings_*.rs`, `tests/job_*.rs`, `tests/ses_writer.rs` |
| Rules files, DSN writer, CLI routing flags, plugin-facing CLI extras | not started | C04, C06, C07, C08 |
| Release builds and CI | CI workflow written, not installed (F01) | R series |

(Table updated 2026-10-16; `python3 tools/roadmap/checklists.py status` is
the live state.)

In release, loading plus a full DRC already takes 0.04–0.5 s on the largest
fixtures (0.09 s on the owner's `canary-pwr` board).

## What "complete" means

The port is complete when all of these hold:

1. **The CLI routes.** `freerouting -de board.dsn -do board.ses` with the Java
   flags the KiCad plugin and the owner use (`-mp`, `-mt`, `-oit`, `-inc`, `-dr`,
   `--router.*`) produces a session KiCad imports. Exit codes and output match
   what the plugin expects.
2. **Results are at least as good as Java's** on every fixture and on the owner's
   boards. That means no new clearance violations, no via in a pad, completion
   at or above Java's, and via count and length within a stated tolerance or
   better. Every place where the Rust result differs on purpose is listed in
   [DIVERGENCES.md](../DIVERGENCES.md).
3. **Routing is deterministic.** The same input and settings give a
   byte-identical SES on every run, at any machine load and thread count.
4. **It is faster than Java.** The goal is at least 3× lower median wall time
   than Java on the reference set at equal or better results. This is a
   proposal; set the final number after the first parity measurement
   (P series).
5. **The desktop app has the Java feature set:**
   - the views, layer and net controls, and the rule and parameter
     dialogs
   - the autorouter with live progress
   - the DRC, incompletes and violation lists
   - interactive routing and editing with undo/redo
   - file open, save and export
6. **The KiCad plugin routes** a real board end to end, on Windows through the
   native build.
7. **Release builds** exist for Windows, macOS (universal) and Linux, and CI
   runs fmt, clippy and the tests on all three.

Later, and outside "complete": the KiCad `.kicad_pcb` importer (K04–K08),
and the REST/MCP server and job management (PORT_PLAN step 11;
[S00](tasks/S00-server-README.md) lists the Java classes and the seams the
port leaves for them).

## Decisions

Implement these; don't reopen them without the owner. Each checklist restates the ones
it depends on.

- **D1 — Match Java's results, not its structure (owner, 2026-10-15).** Rust code
  is organised for speed and clarity. That means arenas and indexes instead of
  object graphs, no class-per-module mirroring, fewer allocations, cheap
  snapshots, and parallelism with an ordered merge. What must match Java is
  observable behaviour: board contents, search order where it decides a route,
  tie-breaks, rounding and scores. Where Java's structure *does* decide a result
  (an iteration or tie-break order, the order of floating-point operations in a
  cost), the checklist names it and the Rust code reproduces the order, not the
  structure.
- **D2 — Deterministic by construction.**
  - Randomness uses `freerouting_geometry::jmath::JavaRandom` with Java's
    seeds: the maze search reseeded with the ripup cost before each
    connection, and the pass shuffle seeded
    `pass_no * 1000003 + max_net_number`. `BatchAutorouter`'s `Random(0)` is
    never read, at the pin or at `30baad7e2`.
  - No iteration over `HashMap`/`HashSet` reaches a result.
  - Parallel work is merged in submission order.
  - Budgets count work, never wall-clock time. A timeout may only stop a run,
    never change how work is done.
  - Every routing checklist proves this by routing the same board twice, once
    idle and once under load, and diffing the SES.
- **D3 — Re-pin before the router.** The Java fork is replacing its
  wall-clock limits with work-count limits ([J01](tasks/archive/J01-repin-deterministic-java.md)).
  That covers the fanout per-pin limit, the pull-tight guard, the maze search,
  the shover and the drill mover. After that commit, Java routing is
  reproducible, and its route outputs become exact targets instead of a
  baseline. The autorouter port starts from that commit. Until it lands, work
  on what doesn't depend on it: board gaps, writers, settings, GUI panels and
  infrastructure.
- **D4 — Parity is a phase, not a constraint forever.** The ladder is in
  [Verification](#verification-ladder). Routes must match deterministic Java on
  the connections where no Java work limit fired; Java logs each cut-off.
  After that, speed-ups must leave results byte-identical. Deliberate quality
  changes come last. Each is measured on the benchmark set the way the Java
  fork measures its changes, kept only if it helps, and listed in
  DIVERGENCES.md.
- **D5 — The search tree matches Java's shape, so work counts match too.**
  The 45° and 90° room completion visits obstacles in the search tree's
  depth-first order (`ShapeSearchTree45Degree.completeShape`). Java's work
  limits count every tree node visited (`MinAreaTree`, through `WorkMeter`).
  So the tree's *shape*, not only its query results, decides routes. The
  Rust tree already follows `MinAreaTree`'s insertion strategy. The B series
  makes the shape exact:
  - Port Java's leaf-reuse operations (`changeEntries`,
    `mergeEntriesInFront/AtEnd`, `reuseEntriesAfterCutout`) in place of
    today's remove-and-reinsert. That removes the DIVERGENCES entry for
    combining traces.
  - Burn item ids wherever Java constructs a temporary item.
  - Rebuild deep copies in item-list order, as Java's serialization does.
  - Prove all three with tree fingerprints on every fixture.
  The Rust work meter then counts the same visits as Java, and parity
  covers runs where a limit fires. A path where shape parity can't be
  reached is compared on aggregates, and its checklist names it. This is
  one of the places D1 doesn't free the structure. A faster tree after
  parity first needs room completion made independent of visit order,
  which is a measured divergence (P series).
- **D6 — Floating point.** In the parity phase, costs and distances that
  decide routes keep Java's operation order. That means no reassociation, and
  no FMA where it would change a value. `Math.sin`/`cos` may differ by one ulp;
  that is accepted and documented.
- **D7 — Engine owns behaviour, GUI owns input.** Routing, optimizing and
  interactive editing are engine code (`freerouting::autoroute`,
  `::optimize`, `::interactive`) with progress events over a channel and a
  cancel token. The GUI maps input to engine commands and draws engine state.
- **D8 — Owner rules carried from the Java fork are behaviour, not options.**
  These are already in the pinned Java and must stay true in Rust:
  - never put a via in a pad; honour `(attach off)`
  - plane-net pads get their own via to the plane
  - signal-layer pours are ignored for nets that have a power-layer plane
  - the flat cost settings work
  - the best board is the one saved
  - **the board's design rules are hard limits (owner, 2026-10-15).** No
    trace narrower than the minimum track width, no via or drill smaller
    than the board allows, no clearance below the rules. No trace narrower
    than its own net class's width either, because class widths carry
    current. The one exception is a short neck-down at a pin that can't take
    the full width; Q02 bounds its length, and it never goes below the
    board minimum. KiCad's DRC doesn't check class widths, so the oracle
    measures them separately (F02.6). These limits come
    from the manufacturer's capabilities or from physics. Completion is
    never bought by relaxing them, and a router never rewrites the board's
    rules. A connection that can't be routed within them is reported as
    unrouted, with the reason (Q03), not routed narrower. The 2026-10-15
    head-to-head (X01) showed why this needs saying: KiCadRoutingTools'
    default `--fab-tier auto` escalated to 0.089–0.127 mm tracks and
    0.25/0.15 mm vias and lowered the board's minimums to match.
- **D9 — One engine, interchangeable algorithms (owner, 2026-10-15).**
  - **A pipeline of stages.** A routing job runs fanout, net ordering,
    connection routing, the ripup policy, the optimizer, and special stages
    (plane taps and repair, neck-down, diagnostics, differential pairs,
    length matching). Each stage is a Rust trait with registered
    implementations selected by id. The ids use Java's existing settings keys
    `router.autorouter.algorithm` and `router.optimizer.algorithm`; at the
    pin Java accepts only `freerouting-router`. Per-stage keys are added
    under `router.*`.
  - **One board model.** Every implementation works on the one board model
    and inserts traces and vias through the board API, so DRC, undo, SES
    writing, the GUI and the plugin work the same whichever algorithm ran.
    Algorithms can be combined: for example a grid A* connection router
    whose result Freerouting's pull-tight optimizer then cleans up.
  - **The Freerouting implementations are the parity baseline.** Adding an
    algorithm never changes their results. They were also the defaults
    until D10; now they are profile `legacy`.
  - **The same gates for every implementation:** deterministic (D2), no new
    clearance violations, no via in a pad. It becomes a default only by the
    measurement rules in [routing-quality.md](routing-quality.md): at least
    three boards, paired, better on N−1 and worse on none, graded by the
    KiCad oracle where it applies.
  - **Licensing.** KRT is MIT and freerouting-rs GPL-3.0, so KRT code may be
    ported in. Each ported file keeps KRT's copyright line and names its
    source path and commit. The MIT text goes in
    `LICENSES/MIT-KiCadRoutingTools.txt`. Ported ideas without code are
    credited in the module docs.
  - **Sequencing.** The A series defines the stage traits when it ports the
    batch pipeline, so Freerouting's stages are their first implementations.
    KRT algorithms come after board parity, in the X series. They start
    with the ones that fill Freerouting's gaps on the owner's boards: neck-down
    at fine-pitch pads, plane handling and diagnostics. Then the grid A*
    connection router, then differential pairs and length matching. A
    head-to-head of Java Freerouting against KRT, graded by KiCad's DRC,
    sets the order within the X series.
- **D10 — Beyond Java (owner, 2026-10-17).** "Let's move beyond java
  freerouting now, and make this better." Board parity is reached (A14),
  so the goal becomes the best routed board, not Java's board.
  - **Two profiles.** `router.profile = legacy` keeps every Java behaviour
    and is what the parity gates and probes pin. The default, `enhanced`, is
    the set of accepted improvements; CLI, GUI and plugin use it.
  - **Defaults may diverge from Java** when a measurement accepts the
    change. Changes to `enhanced` are judged by the totals-within-noise rule
    (N02) instead of the per-board paired rule. The hard guards stay:
    D2, D8, no new clearance violations, the KiCad oracle,
    preregistration, and no code left behind by a rejected candidate.
  - **D4 and D5 relax for `enhanced` only.** The search tree may stop
    matching Java's shape there (N03); `legacy` keeps the Java tree.
  - The work is the N series ([N00](tasks/N00-beyond-java-README.md)).
- **D11 — `legacy` is Java, frozen, on the small parity set (decided 2026-10-20, N07.2).**
  `legacy` stays byte-identical to Java at `a773a70ec` on the connection-parity set
  (`tools/bench/sets/connection-parity.txt`) and the probe tests. Nothing new is ported from
  Java, the pin does not move, and the full A14 runs only on request. Java compatibility ranks
  below `enhanced`'s routing quality, and N07.3–N07.5 trim the rules and tests to this
  ([N07](tasks/archive/N07-lighten-java-parity.md)): a change that only touches `enhanced` needs
  no Java probe, differential test or Java-derived expected value (AGENTS.md, "Matching
  Java"), and `tools/parity-check.sh` runs the probe tests and the connection-parity set in
  one command.

## Verification ladder

Each rung uses the reference data and harness in
[docs/development/engine.md](../development/engine.md) and the Rust tests.

1. **Component parity.** Each ported piece has ported Java tests plus
   differential probes against Java at the pin, run through
   `tools/java-reference`. Examples: rooms completed for one connection, a
   maze search's found path, a pull-tight result.
2. **Connection parity.** On small fixtures, routing one connection gives the
   same traces and vias as Java.
3. **Board parity.** On every fixture and with the reference pass/item bounds,
   the routed SES matches Java's. Runs where Java logged a work-limit cut-off
   join the exact comparison once tree-shape parity is proven (D5). Until
   then, and on boards where a known Java non-determinism fires, they are
   compared on completion, violations, vias and length.
4. **Owner boards and the KiCad oracle.** `canary-pwr` and its variants route
   at least as well as the Java fork. Its results move by about ±10 unrouted
   with small input changes, so compare across several board variants, not
   repeat runs. Boards with a `.kicad_pcb` (the owner's, KiCad's bundled demos)
   are also graded by KiCad's own DRC after importing the session (F02.6).
   That is an oracle independent of the router's own model.
5. **Speed.** Release-build wall time against Java on the same set, same
   machine, quiet. After this rung, optimizations must keep results
   byte-identical.
6. **Quality beyond Java.** Deliberate changes, measured and logged
   ([routing-quality.md](routing-quality.md)), collected in profile
   `enhanced` (D10).

## Work series and order

| Series | Roadmap | Checklists | Starts |
|---|---|---|---|
| **F** Foundations: dev loop, CI, benchmark and parity harness | this file | [F00](tasks/F00-foundations-README.md) | now |
| **J** Java reference tracking: re-pin, deterministic references, ledger | this file | [J01](tasks/archive/J01-repin-deterministic-java.md) | when the Java determinism commit lands |
| **B** Board model gaps the router and editor need | [autorouter.md](autorouter.md) | [B00](tasks/B00-board-gaps-README.md) | now |
| **C** Settings, CLI and writers | [cli-settings-io.md](cli-settings-io.md) | [C00](tasks/C00-cli-settings-io-README.md) | now (settings, writers); routing flags after A |
| **O** Board operations and optimizer | [optimizer-and-editing.md](optimizer-and-editing.md) | [O00](tasks/O00-optimizer-README.md) | shared operations before A; optimizer after A |
| **A** Autorouter | [autorouter.md](autorouter.md) | [A00](tasks/A00-autorouter-README.md) | after J01 and the B/O prerequisites |
| **I** Interactive editing | [optimizer-and-editing.md](optimizer-and-editing.md) | [I00](tasks/I00-interactive-README.md) | after O's shared operations |
| **G** Desktop app | [desktop-app.md](desktop-app.md) | [G00](tasks/G00-desktop-app-README.md) | now (ratsnest, DRC panel); router UI after A |
| **K** KiCad plugin and importer | [desktop-app.md](desktop-app.md) | [K00](tasks/K00-kicad-README.md) | after the CLI routes |
| **P** Performance | [performance.md](performance.md) | [P00](tasks/P00-performance-README.md) | profiling harness now; optimization after parity |
| **Q** Routing quality beyond Java | [routing-quality.md](routing-quality.md) | [Q00](tasks/Q00-routing-quality-README.md) | after board parity |
| **X** Algorithms from KiCadRoutingTools | [algorithms.md](algorithms.md) | [X00](tasks/X00-krt-algorithms-README.md) | head-to-head now; ports after board parity |
| **R** Release builds | [desktop-app.md](desktop-app.md) | [R01](tasks/archive/R01-release-builds.md) | after the CLI routes |
| **S** Server and job management | this file ("Later") | [S00](tasks/S00-server-README.md) | after "complete", when the owner asks |

The critical path is J01 → B (router prerequisites) → O (shared board
operations) → A (search core → path insertion → batch pipeline) → C (routing
CLI) → K (plugin routes). G, the non-routing parts of C, F and P's harness run
alongside. Each series README gives its own order and file ownership, so
parallel implementers can be given disjoint areas.

## Parallel lanes

The ownership table in [engine.md](../development/engine.md) holds: engine
sessions own the four engine crates, `tools/java-reference/` and
`fixtures/reference/`. GUI sessions own `crates/freerouting-gui`, and
plugin sessions own `kicad-plugin/`. Inside the engine, the router (`autoroute`),
the optimizer and board operations (`optimize`, board operations) and
interactive editing (`interactive`) are separate modules. Each series README
names the files each checklist creates and the shared files it may only
append to (`lib.rs` module lists, `DIVERGENCES.md`).

## Relationship to other plans and sessions

- **Development focus is freerouting-rs** (owner, 2026-10-15, as relayed by the
  Java session). The Java fork now gets only bug fixes and cleanup, no
  large features. Its determinism commit `30baad7e2` and three fixes after it
  (the optimizer score clamp, headless DSN output, a machine-readable
  work-limit cut-off line) make up `a773a70ec`, the re-pin target (J01).
- **The Java fork** (`../freerouting`) keeps `docs/development/PORT_LEDGER.md`:
  one line per Java commit after the pin, with its measurements and
  "Port: yes/no/later". The J series adopts ledger entries by re-pinning or by
  a DIVERGENCES line. Read it at `main` through git, never the working tree.
- **Board lessons from `canary-pwr`** (the CANary power board, laid out in
  `../canary/hardware/canary-pwr-rev1.0`) feed the Q series: planes that block
  other nets, neck-down at pins, a "cannot route at this width" diagnostic,
  KiCad rule-area flags, and benchmarking across board variants.
- **PORT_PLAN.md** keeps the module map, crate layout and engine design
  decisions. Where this document and PORT_PLAN disagree, this one is newer.
  The difference is mainly D1: Java structure is no longer the template.

## Risks

- **The maze search and the shover are most of the behaviour**, and small
  ordering differences cascade. Connection-level probes (rung 2) catch these
  before board-level noise hides them.
- **Load-bearing Java bugs.** Some good results depend on Java bugs. These
  stay ported as they are ("Java bugs kept on purpose") until rung 6 measures
  the fix.
- **Parity versus D1.** A restructure that changes an iteration or tie-break
  order changes routes. Each checklist names the orders that decide results,
  and rung 3 catches the rest.
- **Reference drift.** The Java fork keeps improving. Re-pin deliberately
  (J series), never mid-checklist, and only on a clean reference
  regeneration.
- **Large boards in the GUI.** Per-frame CPU vertex transforms and full scene
  rebuilds won't survive live routing on big boards (G and P series).
