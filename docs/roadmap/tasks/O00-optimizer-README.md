# O00 — Board operations and optimizer

Opened 2026-10-15. The O series ports Java's board-changing algorithms:
pull-tight (`board.optimize.TraceTightener*`), push-and-shove
(`TraceShover`, `ShapeTraceEntries`, `ShapeEntrySide`), forced trace, via and
pad insertion (`board.actions.ForcedPadRouter`, `ForcedViaInserter`,
`DrillItemMover`), the via optimizer (`ViaOptimizer`) and the batch optimizer
(`autoroute.pipeline.BatchOptimizer`). Nothing in scope exists in Rust yet;
`board/routing_board.rs` still carries the `TODO(router)` in
`clear_changed_area`. The narrative is in
[optimizer-and-editing.md](../optimizer-and-editing.md). The plan these
checklists implement is [port-completion.md](../port-completion.md) (D1–D9).

**O01–O05 come before the autorouter.** The maze search (A04) calls the
read-only shove checks while it expands rooms, even with ripup off (O03).
Path insertion (A06) inserts every found connection through the forced-trace
code (O04) and pull-tightens it (O01). The post-connection pull-tight with via
moves (O05) is needed for board parity. Only the batch optimizer (O06) comes
after the A series, because it reroutes items with the batch autorouter.

## Task index

| Task | Outcome | Size / risk | Prerequisite |
|---|---|---|---|
| [O01](archive/O01-pull-tight-45-90.md) ✅ | 45° and 90° pull-tight, pin-exit recursion, `opt_changed_area` without vias, `remove_items_and_pull_tight`; closes the `TODO(router)` | L / high | B01, B02 (`LineReuse`, work meter), B04, B05; J01 (re-pin to `30baad7e2`) for O01.6's limit probe only |
| [O02](archive/O02-pull-tight-any-angle.md) ✅ | Any-angle pull-tight | M / high (numerics) | O01 |
| [O03](archive/O03-shove-checks.md) ✅ | Shove checks without item changes: entry sides, `ShapeTraceEntries` (incl. cutouts), `TraceShover.check` (both), spring-over, drill/pad/via checks, `check_forced_trace_polyline` | L / high | B01 (incl. B01.6 ledger, B01.8 shove-failing state), B02 (incl. B02.6 `reuse_entries_after_cutout`), B04; J01 for O03.7's limit probe only |
| [O04](archive/O04-shove-insert.md) ✅ | Shove insert: `TraceShover.insert`, drill moves, forced pads and vias, `insert_forced_trace_polyline/segment`, `forced_via`, `move_drill_item`; `Issue954CircularKeepoutInsertionTest` | L / high | O01, O03, B05; O02 for the any-angle cases of O04.6 |
| [O05](O05-via-optimizer.md) | `ViaOptimizer` and the full `opt_changed_area` with trace costs | M / medium | O04, A01 (`ExpansionCostFactor`) |
| [O06](O06-batch-optimizer.md) | Deterministic parallel batch optimizer, ordered merge, Java-identical candidate order and gates; `optimize` reference mode | L / high | O05, A08, A10, A11, B01, B06, C02, C05 |

## Order and critical path

```
B01 B02 B04 B05 ─> O01 ─┬─> O04 ─> O05 ─> O06   (O06 also waits for A08, A10, A11)
                        └─> O02                 (O04.6's any-angle cases wait for O02)
B01 B02 B04 ─────> O03 ─┬─> O04
                        └─> A04 (maze search)
O01 + O04 ─> A06 (path insertion)      O05 ─> A11, A12 (post-connection pull-tight)
O04 ─> I01 ─> I02 ─> I03 ─> I04        (see I00)
```

The critical path to the router is O03 → A04, and O01 + O03 → O04 → A06.
O05 joins before A's post-connection pull-tight is compared at board level.
O02 is off the critical path: no fixture uses any-angle routing (all 118
routed fixtures use the 45° tree; the 3 with a `snap_angle` say
`fortyfive_degree`), but `Issue954CircularKeepoutInsertionTest` runs every
angle mode.

A large checklist may be split across two dispatches by named item subsets
(tasks README). Suggested splits: O01.1–O01.4 then O01.5–O01.8;
O03.1–O03.4 then O03.5–O03.8; O04.1–O04.4 then O04.5–O04.8.

## Parallel lanes and file ownership

| Checklist | Creates | May edit (shared files: append only) |
|---|---|---|
| O01 | `optimize/{mod.rs, params.rs, pull_tight.rs}`, `optimize/tighten/{mod.rs, diag45.rs, ortho.rs}`, `tests/reference_pull_tight.rs`, `tests/support/item_dump.rs`, `examples/pull_tight_dump.rs`, `tools/java-reference/probes/board/PullTightProbe.java` | `lib.rs`, `board/routing_board.rs` (`clear_changed_area` only), `freerouting-geometry/src/polyline.rs` (append `Polyline::new_mapped`), `docs/DIVERGENCES.md`, `tools/java-reference/README.md` |
| O02 | `optimize/tighten/any_angle.rs`, `tests/pull_tight_any_angle_budget.rs` | `optimize/tighten/mod.rs` (dispatch arm), `tests/reference_pull_tight.rs`, `PullTightProbe.java` (append the `none` mode), `examples/pull_tight_dump.rs`, `tests/issue954_keepout_insertion.rs` (un-ignore), `docs/DIVERGENCES.md` |
| O03 | `shove/{mod.rs, entry_side.rs, trace_entries.rs, cutout.rs, trace_check.rs, spring.rs, drill_check.rs, via_check.rs, forced.rs}`, `tests/support/shove_sampler.rs`, `tests/reference_shove_check.rs`, `examples/shove_check_dump.rs`, probes `EntrySideProbe`, `TraceEntriesProbe`, `ShoveCheckProbe` | `lib.rs`, `freerouting-geometry/src/shapes/tile_shape.rs` (append `cutout_polyline_outcome`), `docs/DIVERGENCES.md` |
| O04 | `shove/insert.rs`, `tests/issue954_keepout_insertion.rs`, `tests/reference_shove_insert.rs`, `examples/shove_insert_dump.rs`, probe `ShoveInsertProbe` | `shove/forced.rs` (append), `shove/mod.rs` (re-exports), `tests/support/shove_sampler.rs` (append), `docs/DIVERGENCES.md` |
| O05 | `optimize/via.rs`, `tests/reference_opt_vias.rs` | `optimize/params.rs`, `optimize/tighten/mod.rs` (via branch), `optimize/pull_tight.rs`, `examples/pull_tight_dump.rs` (`--trace-costs`), `PullTightProbe.java` (append mode) |
| O06 | `optimize/batch/{mod.rs, guards.rs, candidates.rs, item.rs, worker.rs}`, `tests/batch_optimizer.rs`, `tests/reference_optimize.rs`, `examples/optimize_board.rs`, `fixtures/reference/optimize/*`, probe `OptimizeCandidatesProbe` | `optimize/mod.rs` (append), the A10 stage registry (one registration), `tools/java-reference/src/reference/ReferenceDump.java` (append the `optimize` mode only), `tools/java-reference/README.md`, `docs/DIVERGENCES.md` |

Paths without a prefix are under `crates/freerouting/src/` (code) or
`crates/freerouting/` (`tests/`, `examples/`). O01 ∥ O03 run in parallel on
disjoint files. After O01, O02 ∥ O04. O05 follows O04. I01 may start once
O04 lands and runs alongside O05 and O06.

**Module placement (decided here).** Shove code is a top-level
`freerouting::shove`, because the autorouter is its main caller. Pull-tight
and the via optimizer are `freerouting::optimize`; the batch optimizer is
`freerouting::optimize::batch`. Board entry points (`opt_changed_area`,
`insert_forced_trace_polyline`, …) are `impl RoutingBoard` blocks inside
those modules, so the O series doesn't edit the B series' board files.
PORT_PLAN's module map still says `board.actions` → `freerouting::board`; the
orchestrator updates it.

## Rules for this series

These add to the shared rules in the [tasks README](README.md).

- **Differential probes use B01.1's board-probe support.** Each probe is one
  Java file in `tools/java-reference/probes/board/`, with B01.1's `Boards`
  loader block copied verbatim (so `probe_loader_copies_identical` covers
  it), run with F02.1's `probe.sh` against the current pin, one board at a
  time. It writes `fixtures/reference/probe/<kind>/<board>.jsonl` in B01.1's
  line format (fixed key order, doubles as raw-bit hex). The Rust side uses
  `tests/board_probe/mod.rs` (loader, writer, `compare`). A probe that needs
  package-private Java (for example `ForcedPadRouter.forcedPad`) declares
  that package; the Java repo has no `module-info.java`, so split packages
  work on the classpath. Every output records the pin, the JDK version and
  the seed. Java time limits are off in every probe (`timeLimit ≤ 0`,
  `null` `TimeLimit`); Java at `cda540f` accepts that everywhere in scope.
- **Canonical item dump.** Probes and Rust tests dump boards in one format,
  defined in `tests/support/item_dump.rs` (O01, written with B01.1's line
  writer) and mirrored by a nested class in each Java probe: traces as
  `(id, nets, layer, half_width, clearance_class, fixed_state, corners)`,
  vias as `(id, nets, padstack, centre, fixed_state)`, highest id first,
  plus `maxGeneratedId`. Tests compare geometry first and ids second, so a
  failure says which one diverged.
- **Re-pin.** If J01 re-pins during the series, rerun every O probe at the new
  pin before ticking a box. `tools/java-reference/README.md` lists one
  command per probe.
- **"Unchanged" is an explicit outcome.** Java signals "nothing changed" by
  returning the same object (`newLines != lines`, `newResult != prevResult`,
  `okPoint == insertPolyline.firstCorner()`, `counterClockWiseResult ==
  polyline`). Rust returns `Outcome::Unchanged` / `Outcome::Changed(..)` and
  reports `Changed` exactly where Java built a new object, even when the new
  value equals the old one. Never infer "unchanged" from value equality.
  The same holds for single lines: `PolylineTrace.change` keeps tree leaves
  for the leading and trailing lines that are the same Java `Line` objects,
  so callers of B02's `change_trace` pass `LineReuse { prefix, suffix }`
  computed from tracked line origins (O01.1), not from values.
- **Ids.** Burn an item id (B01's `burn_item_id`) wherever Java constructs a
  temporary item: `checkPolylineTrace`, every
  `ShapeTraceEntries.nextSubstituteTracePiece` (also on check paths),
  `copy(0)`. Probes compare `maxGeneratedId`.
- **Iteration.** Java item sets iterate highest id first. Loops that change
  the board take a snapshot of ids and re-check `is_on_the_board` before each
  one, and keep Java's `break` points.
- **Work budgets.** The fork's `30baad7e2` ("Make routing independent of
  machine load", J01's re-pin target) replaced every `TimeLimit` in scope
  with a `WorkLimit` at the same call sites: `TraceTightener` (built from
  `workLimitMs`, checked in `isStopRequested`), `TraceShover.check` and
  `DrillItemMover.check` at entry, `ForcedPadRouter.checkForcedPad` (passes
  it on), `MoveComponent` (`CHECK_LIMIT_NOMINAL_MS = 3000`,
  `PULL_TIGHT_LIMIT_NOMINAL_MS = 1000`), `RoutingBoardOperations`
  (`PULL_TIGHT_LIMIT_NOMINAL_MS = 2000`), `Route` (3000 check, a fresh limit
  per call site; 2000 pull-tight), and the 1000 guard after each connection
  and in `removeTails`. Java's semantics (`WorkLimit.java`, `WorkMeter.java`
  at that commit): one meter per thread counting search-tree nodes visited
  in `MinAreaTree` and the three `completeShape` traversals; a limit records
  the meter at construction and is exceeded when `used > nominal_ms ×
  64,000` (strict). Rust uses B02.7: the board's `SearchTreeManager` owns
  the meter, and `WorkLimit::start(&meter, budget)` is created where Java
  constructs its limit and checked where Java checks it. A per-board meter
  equals Java's per-thread meter for every limit in this series, because
  each limit lives only while one board is worked on by one thread. Parity
  probes run unlimited; a separate test per checklist proves a limit fires
  at the same visit count as Java once J01 has re-pinned.
- **Angle modes.** 45° is the default and the only mode fixtures use. Probes
  also force 90° and none through `rules.setTraceAngleRestriction`. The
  variants test the clip differently (45°: `isOutside`; 90° and any-angle:
  `contains`); keep each variant's predicate.
- **Exceptions as control flow.** Where Java catches an exception and carries
  on (`insertForcedTracePolyline`'s normalize, `TraceTightener`'s
  `normalizeTraces`), Rust returns an error value at the same point and takes
  the same branch. Never panic there.
- **Determinism.** Each checklist adds an example binary that dumps its
  results, and proves determinism with `tools/determinism-check.sh` (F02.4).
- **Kept Java quirks** get a test, a module-doc note and a DIVERGENCES line
  under "Java bugs kept on purpose".

## Open questions for Dan

1. **One winner per optimizer pass.** Java evaluates every candidate against
   the pass baseline and adopts only the single best one. That decides
   results and is slow on big boards. *Recommendation:* keep it as the default
   (an explicit `AdoptionPolicy::SingleWinner`). Measure a "non-overlapping
   winners" mode later in the Q series as an opt-in divergence.
2. **`optimizer.timeout`.** Java checks the wall clock before each pass,
   before each chunk, as a task early-out, and inside the per-item "improved"
   decision (`BatchOptimizer.java:936-940`). *Recommendation:* the
   reproducible bounds are `max_passes` and `max_items`. Honour `timeout` only
   as a stop between chunks (the current pass's winner is discarded, as Java
   does), never inside the improved decision; list that as a divergence.
3. **Optimizer threads.** Java defaults to CPUs − 1. *Recommendation:* keep
   it. Rust results don't depend on the thread count by construction (O06),
   and a test proves it.
