# Board operations, optimizer and interactive editing

Recorded 2026-10-15. This is the roadmap area for two series:

- **O, board operations and optimizer** ([O00](tasks/O00-optimizer-README.md)):
  pull-tight, push-and-shove, forced trace, via and pad insertion, the via
  optimizer and the batch optimizer.
- **I, interactive editing** ([I00](tasks/I00-interactive-README.md)): routing,
  dragging and editing driven by a user, as engine code.

The plan these series implement is [port-completion.md](port-completion.md).
Its decisions D1–D9 apply throughout; this document explains how they shape
this area.

## What the area is

In Java the code lives in four places, all at the pin `cda540f`:

| Java | Lines | What it does |
|---|---:|---|
| `board/optimize` | 4,026 | `TraceTightener` and its 45°, 90° and any-angle variants (pull-tight); `TraceShover` (push-and-shove); `ViaOptimizer` |
| `board/actions` | 2,180 | `ForcedPadRouter`, `ForcedViaInserter`, `DrillItemMover` (shove pads, vias and drill items); `MoveComponent`, `ComponentItemTransform` |
| `board/searchtree/ShapeTraceEntries`, `board/model/structure/ShapeEntrySide`, `ShapeAndEntrySide` | 1,091 | Where foreign traces enter a shove shape, and the substitute traces around it |
| `autoroute/pipeline/BatchOptimizer`, `autoroute/ItemRouteResult` | 1,394 | The optimizer passes after routing |
| `gui/interactive` | 5,321 | 27 editor state classes, including `Route` (interactive routing) |

None of it is in Rust yet. `board/routing_board.rs` marks a changed area and
has a `TODO(router)` where Java pull-tightens it. Everything these algorithms
need from the geometry crate exists (entrance points, cutouts, offset shapes,
border projections, 45°/90° projections). The board-level pieces they need
(checks, picks, item moves, pin exits, deep copies, a work meter) come from
the B series.

Three layers sit on top of each other:

1. **Board operations the router uses.** Pull-tight, shove checks, shove
   insertion, forced vias and pads, via moves. The autorouter calls about 85%
   of `board.optimize` and `board.actions`: the maze search asks shove
   questions while expanding rooms, path insertion inserts every connection
   through the forced-trace code, and every routed connection is
   pull-tightened with via moves. These are O01–O05.
2. **The batch optimizer** (O06). After routing, it rips up one item at a
   time, reroutes it with the batch autorouter, and keeps the best
   improvement per pass.
3. **The interactive editor** (I01–I04). Routing with the mouse, dragging,
   make-space, selection, pin swap, keepouts, undo and redo. It uses layer 1
   for every board change.

## Order

```
B01 B02 B04 B05 ─> O01 pull-tight ─┬──────────> O04 shove insert ─> O05 via optimizer ─> O06 batch optimizer
                                   └─> O02 any-angle                   │                  (after A10, A11)
B01 B02 B04 ────> O03 shove checks ─> A04 maze search                 └─> A06 path insertion
                                       O04 ─> I01 route ─> I02 drag ─> I03 editor ─> I04 (optional)
```

O01–O05 come before the autorouter, not after it as PORT_PLAN's milestone 8
had it: A04 needs O03, and A06 needs O04 and O01. O05 completes the
post-connection pull-tight that board parity needs. O06 needs the batch
autorouter, so it comes after the A series. The I series needs O04 and
otherwise runs beside A.

## How the design is shaped

**Results match Java, structure doesn't (D1).** One `Tightener` struct with an
angle mode replaces three classes; entry lists are indexed `Vec`s instead of
a circular linked list; shove code is a top-level `freerouting::shove`
module, pull-tight and the via optimizer are `freerouting::optimize`, the
batch optimizer is `freerouting::optimize::batch`, and the editor is
`freerouting::interactive`. Board entry points are `impl RoutingBoard` blocks
inside those modules, so the O series and the B series edit disjoint files.

Java's structure does decide results in a few places, and each checklist
names them:

- **"Unchanged" is reference identity.** Pull-tight's fixpoint loops stop
  when a step returns the same object; `springOverObstacles`,
  `insertForcedTraceSegment` and `Route.nextCorner` compare points by
  reference; a new object that is value-equal still counts as a change.
  Rust reports `Unchanged`/`Changed` explicitly at exactly Java's points.
- **One winner per optimizer pass.** Every candidate is evaluated against the
  pass baseline and only the best becomes the next baseline. Rust keeps that
  as an explicit `AdoptionPolicy::SingleWinner`.
- **Orders.** Item sets iterate highest id first; loops that change the
  board scan a snapshot and stop at Java's `break` points; the optimizer's
  candidate order skips every item that shares a key with an earlier one.
- **Ids.** Java burns an item id for every temporary item it builds,
  including the substitute traces of a pure shove check. Ids decide every
  sorted order, so Rust burns them at the same points.
- **Kept Java bugs.** The `inFrontOfPad` typo, `ItemRouteResult`'s integer
  division, the optimizer stopping after one pass when the first pass finds
  no winner, a layer change that "succeeds" with no spanning via, and a drag
  that never pull-tightens with a finite tidy width. Each is kept during the
  parity phase, pinned by a test and listed in DIVERGENCES.md.

**Determinism (D2).** The fork's `30baad7e2` (included in `a773a70ec`, J01's re-pin target) turned
every wall-clock limit in this area into a WorkLimit at the same call site:
the pull-tight guard after each connection and in `removeTails`, the shover
and drill-mover checks, `removeItemsAndPullTight`, and the interactive Route
and MoveComponent limits. A limit is exceeded after `nominal ms × 64,000`
search-tree node visits on its thread. Rust counts the same visits with
B02's `WorkMeter`, which belongs to the board, and checks the limits at
Java's check points. Whether a limit fires at the same point as in Java
depends on the search trees having Java's shape, which the B series makes
exact (D5).

The batch optimizer is the one place where Java itself is not reproducible:
its worker threads reuse a board per thread, and undo doesn't restore the id
generator, the trace half-width bounds, the normalize-suppressed nets or the
routing ledger. Rust evaluates every candidate on a copy-on-write copy of a
per-pass deep copy of the baseline, so a candidate's result never depends on
the thread count or on which candidates ran before it. Results are merged in
candidate order with Java's single-thread rules. The fork's ledger says the
worker leak is not fixed at `30baad7e2` and asks for aggregate comparison
until a later line; after that, exactly. Separately, the default optimizer
score clamps to 0 on bm01 at that commit, so Java's optimizer does nothing
there; a no-op on bm01 is not parity evidence.
If Java fixes the leak by resetting fields after `undo` rather than by a
fresh copy, its tree shapes still depend on history, so runs where a work
limit fires stay aggregate-only; asking Java for the fresh-copy fix avoids
that.

**The editor is engine code (D7).** Java's 27 state classes mix board logic
with Swing. The Rust editor is one `Editor::apply(Command) -> Response`: the
GUI sends pointer and key commands in board coordinates and gets back the
board changes (from B01's change journal), a typed status message and
overlay data. Every command runs to completion under Java's work budgets, so
a recorded command script replays to the same board. The G series maps input
to commands and draws the response ([G09](tasks/G09-interactive-routing-and-editing.md)).

## How it is verified

Each checklist climbs the verification ladder in port-completion.md:

- **Ported Java tests:** `Issue954CircularKeepoutInsertionTest` (10 methods,
  75 cases: spring-over, forced insertion and cleanup around keepouts),
  `MoveComponentTest`, `ComponentItemTransformTest`, and the six optimizer
  tests (`BatchOptimizerCandidateGateTest`, `…TraceMetricTest`,
  `…DeterminismTest`, `…PreflightGuardsTest`, `…StrategyTest`,
  `MaxFanoutOptimizerSettingsTest`). Java has no unit tests for the
  tighteners, the shover, the via optimizer or `Route`.
- **Differential probes** at the pin through F02.1's `probe.sh`, each
  comparing the canonical board dump including item ids and the id
  generator: `pulltight` (43 routed boards, 45°, forced 90° and forced
  any-angle), `entry-side`, `trace-entries`, `shove-check` (every query
  must match, because the maze search depends on each answer),
  `shove-insert` (scripted forced inserts and vias, compared step by step),
  `optvias`, `optimize-candidates`, `route-script`, `drag-script` and
  `editor`.
- **Work limits:** after J01, a probe per operation runs with a limit that
  fires and compares the visit count at the cut-off.
- **Optimizer references:** a `ReferenceDump` mode `optimize` writes
  `fixtures/reference/optimize/` (Java-routed SES in, 3 passes, 1 and 7
  threads); Rust is compared on aggregates now and by `ses-diff` after
  Java's fix.
- **Determinism:** every checklist has an example binary that dumps its
  results, run through `tools/determinism-check.sh` idle and under load. The
  optimizer also proves threads=1 and threads=N give byte-identical SES.
- **Owner rules (D8):** forced vias honour `(attach off)`; the via optimizer
  never leaves a via in another net's pad; the optimizer keeps the best board.

## Risks

- **The shover is most of the behaviour.** A small ordering difference in
  `ShapeTraceEntries` or `TraceShover` changes which routes the maze search
  finds. The probes compare each query and each insertion step, so a
  divergence is caught where it starts, before board-level noise hides it.
- **Work-limit parity is borrowed.** It needs B02's tree-shape parity and
  the counting points of `30baad7e2`. Java also runs debug-only queries
  (`insertForcedTracePolyline`'s `dbgBefore`/`dbgAfter` picks), and its
  per-thread meter counts them, so Rust must count the same visits without
  running them.
- **Java's optimizer isn't reproducible** until its worker fix lands, so O06
  can only be compared on aggregates until then.
- **Any-angle numerics.** The any-angle tightener works in floating point with
  cosine thresholds and rounding. Its probe allows a few explained
  mismatches, each traced to its operands.
- **Interactive probes are weaker oracles.** Java's state classes need a
  Swing `GuiBoardManager`, so probes drive `Route`, `MoveComponent` and the
  board operations directly and transcribe the states' few board calls.
  `MoveComponent` also iterates identity-hash sets per the Java session; Rust
  uses item-id order and compares only order-independent cases.
- **Scope.** Much of Java's editing is switched off at the pin (delete,
  fix/unfix, cutout, move, copy, optimize or autoroute the selection). I00
  asks Dan which to build, together with G00's matching question; the
  recommendation is delete and fix/unfix now, the rest later.
- **Speed.** One winner per optimizer pass is slow on large boards. It stays
  the default for parity; a faster mode is a Q-series measurement.

## Corrections to the research reports

- The board-gap report's hazard H2 says an empty tidy region passed through
  `enlarge` makes pull-tight run with an empty clip. At `cda540f`
  `IntOctagon.offset` normalizes, and `normalize` returns the `EMPTY`
  instance, so pull-tight is skipped; `moveDrillItem` with a finite tidy
  width never pull-tightens. O04 keeps and pins that.
- `Issue954CircularKeepoutInsertionTest.insertsDetour` has 60 cases (3 angle
  modes × 2 diameters × 2 directions × 5 profiles), not 100; the class has 10
  test methods and 75 cases in all.
- Java's pin-exit branch of pull-tight only runs when `pinEdgeToTurnDist > 0`,
  which only the GUI sets, and path insertion sets it to −1 while inserting.
  It matters for interactive work, not for headless routing.
