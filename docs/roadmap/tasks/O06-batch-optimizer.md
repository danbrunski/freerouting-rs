# O06 — Batch optimizer

Opened 2026-10-15. After routing, Java runs `BatchOptimizer` by default: it
rips up one via or trace at a time, reroutes it with up to six autorouter
passes, and keeps the best improvement per pass. This checklist ports it as
the `freerouting-optimizer` stage of the D9 pipeline. It comes after the A
series, because each candidate is rerouted by the batch autorouter. See
[O00](O00-optimizer-README.md) for the series rules.

**Java's optimizer is not reproducible, at the pin or at `30baad7e2`.** Its
worker threads reuse one board per thread across candidates
(`WorkerBoardState`, `BatchOptimizer.java:979-1008`), and undo doesn't
restore the id generator, the trace half-width bounds
(`BasicBoard.java:218-219`), the normalize-suppressed nets or the routing
ledger. So a candidate's result depends on which candidates ran before it on
the same thread. The fork's `PORT_LEDGER.md` (at `main`, after `30baad7e2`)
says so, and says to compare optimizer output on aggregates only until a
later ledger line changes that. The Rust optimizer evaluates every candidate
on a copy-on-write copy of the pass baseline, never on a reused worker
board, so its result doesn't depend on thread count or scheduling, and
merges results in submission order. It is compared with Java on aggregates
until Java's fix lands (a ledger line saying so, adopted by a re-pin), then
exactly.

**bm01 is not evidence.** At `30baad7e2` the default V2 optimizer score
clamps to 0 on `Issue508-DAC2020_bm01`, so Java's optimizer rejects every
pass there and does nothing. The Java session is looking at it. A Java (or
Rust) optimizer run that changes nothing on bm01 says nothing about parity;
no acceptance here counts it as such. The Java session is fixing the clamp
(ranking by the unclamped score), and the fix lands with the commit J01
pins to. Also from the Java session: **the optimizer only runs on completely
routed boards**, so `canary-pwr` and other partly routed boards never reach it.
Pick optimizer reference boards from those Java routes to completion.

**Prerequisites:**
- O05 (`opt_changed_area` with trace costs);
- A08 (A08.3 `ItemRouteResult`, A08.4 router events, A08.5 the two-level
  `CancelToken`), A10 (the `Optimizer` stage trait and registry) and A11
  (A11.6 `route_passes_for_optimizer`, Java's
  `BatchAutorouter.autoroutePassesForOptimizingItem` `:253-289`, which ends
  with A11.5's `remove_tails`);
- B01 (B01.3 `snapshot`/`restore`/`deep_copy` with Java's restore
  semantics, B01.4 cheap snapshots, B01.5 `state_hash`, B01.6 the ledger's
  `incomplete_count`);
- B06 (`optimizer_score` driven by `RouterSettings`);
- C02 (`OptimizerSettings` and `RouterSettings`); C05 (SES writer, O06.8).

## What exists and what this adds

| Exists | Where |
|---|---|
| `BoardStatistics` incl. the cheap variant (`with_options`), `optimizer_score_v2`, `total_weighted_length` (`f32`, as Java) | `scoring/board_statistics.rs:275,387,84` |
| `connection_items(StopConnectionOption)`, `trace_start_contacts` / `trace_end_contacts`, `remove_items`, `combine_traces` | `board/basic_board.rs:1788`, `board/trace_ops.rs:580` |
| `ReferenceDump.java` modes `load`, `route` | `tools/java-reference/src/reference/ReferenceDump.java:87-95` |
| `ItemRouteResult` (with the integer-division quirk), router events, `CancelToken` | A08.3–A08.5 |

| Java at `cda540f` (`src/main/java/app/freerouting/`) | Lines |
|---|---|
| `autoroute/pipeline/BatchOptimizer.java`: `evaluatePreFlightGuards` `:156-224`, `areAllViasMandatoryLayerTransitions` `:226-276`, `runBatchLoop` `:279-517`, `optimizerCandidateRejectionReason` `:569-585`, `prepareCandidateItems` `:587-628`, `optRoutePass` `:630-822`, `optRouteItemOnBoard` `:852-942`, `WorkerBoardState` `:979-1008`, `OptimizeCandidateTask` `:1010-1111`, `ReadSortedRouteItems` `:1151-1248` | 1249 |
| `autoroute/ItemRouteResult.java` (integer division in `improvementPercentage` `:59-64`; `compareTo` ends on item id `:67-89`) | 145 |
| Tests: `autoroute/pipeline/BatchOptimizerCandidateGateTest` (4), `BatchOptimizerTraceMetricTest` (3), `fixtures/BatchOptimizerDeterminismTest` (2), `BatchOptimizerPreflightGuardsTest` (14), `BatchOptimizerStrategyTest` (1), `MaxFanoutOptimizerSettingsTest` (2) | `src/test/java/app/freerouting/` |

## Decisions made here

- **Candidates on copy-on-write copies.** Per pass, take `snap =
  baseline.snapshot()` once (B01.3). Each candidate gets `snap.restore()`
  followed by `clear_transient_autoroute_state()`, which is `deep_copy()`:
  trees rebuilt in item-list order, transient state reset, as Java's
  `deepCopy` does. That is exactly "a fresh `deepCopy()` per candidate", the
  semantics Java's fix would give. If B01.4's shared canonical tree hasn't
  landed and the rebuild dominates, restore once into `worker_base` and give
  candidates `worker_base.clone()`: a clone of a just-restored board is the
  same board (same items, same rebuilt tree, same reset state), so B01's
  "`Clone` is not a Java copy" caveat doesn't apply;
  `candidate_copy_equals_deep_copy` (O06.4) proves it. If Java instead
  fixes the leak by resetting fields after `undo`, its tree shapes still
  depend on earlier candidates on the thread (undo reinserts leaves; Java's
  own `WorkLimit` doc at `30baad7e2` warns about this), so runs where a
  work limit fires stay aggregate-only; the orchestrator should ask Java
  for the fresh-copy variant.
- **Explicit single-winner adoption.** `AdoptionPolicy::SingleWinner` (the
  only variant now): every candidate is evaluated against the pass baseline,
  and only the best improved result (by `ItemRouteResult` order, strict)
  becomes the next baseline. Java's "unchanged" signals elsewhere are
  explicit outcomes (O00).
- **Ordered merge.** Candidates run on `max_threads` scoped threads
  (`std::thread::scope`, as `drc/clearance.rs:122-140` already does; no new
  dependency), in chunks; results are consumed strictly in candidate order with
  Java's single-thread rules (failure counter, `max_items` accounting,
  winner rule). Results past the stop point are discarded. The chunk size
  doesn't change results; default `max(4 · threads, 8)` as Java.
- **Work is per candidate.** The meter belongs to each board's
  `SearchTreeManager` (B02.7) and `restore()` zeroes it, so every candidate
  counts its own work on its own board; no thread's work can leak into
  another candidate's count.
- **No clock in a result.** Java forces "not improved" when the deadline has
  passed (`:936-940`). Rust doesn't; `optimizer.timeout` is a stop checked
  before each chunk and each pass (O00 open question 2). DIVERGENCES line.
- **Kept quirks** (DIVERGENCES, "Java bugs kept on purpose"): a first pass
  with no winner (or a winner whose `improvement_percentage` is 0, which
  A08.3's integer division makes common) clears `useIncreasedRipupCosts`
  inside `optRoutePass`, so the loop sees 0 % < threshold and stops after
  one pass;
  the equal-key skip in candidate order; the threshold auto-scale for values
  in (0, 0.1).

## File ownership

| Item | New files | May edit |
|---|---|---|
| O06.1 | `optimize/batch/{mod.rs, guards.rs}` | `optimize/mod.rs` (append `pub mod batch;`) |
| O06.2 | `optimize/batch/candidates.rs`, `tools/java-reference/probes/board/OptimizeCandidatesProbe.java` | — |
| O06.3 | `optimize/batch/item.rs` | — |
| O06.4 | `optimize/batch/worker.rs` | — |
| O06.5 | `tests/batch_optimizer.rs` | `optimize/batch/mod.rs`, `docs/DIVERGENCES.md` |
| O06.6 | — | `optimize/batch/mod.rs`, the A10 registry file (append one registration) |
| O06.7 | `tests/reference_optimize.rs`, `fixtures/reference/optimize/*` | `tools/java-reference/src/reference/ReferenceDump.java` (append the `optimize` mode only), `tools/java-reference/README.md` |
| O06.8 | `examples/optimize_board.rs` | — |

`ReferenceDump.java` is shared: C01 adds `settings`/`ses`/`dsn`/`rules`
modes and J01.3 changes `route`. The `optimize` mode is O06's own: add a
`case "optimize"` arm and new private methods, touch nothing else.

## Checklist

- [ ] **O06.1 — Gate and preflight guards.**
  - Java: `optimizerCandidateRejectionReason` (A08.3 ports `ItemRouteResult`),
    `evaluatePreFlightGuards` (G1 incompletes > 0; G2 no vias and score
    ≥ 950 or length ≤ 1.05 × minimum; G3 score ≥ 995, or length ≤ 1.02 ×
    minimum and vias ≤ minimum via count; G4 all unfixed vias are mandatory
    SMD layer transitions), `areAllViasMandatoryLayerTransitions`.
  - Rust: `optimize/batch/guards.rs`.
  - **Accept:** `BatchOptimizerCandidateGateTest` (4) and
    `BatchOptimizerPreflightGuardsTest` (14) ported and green.

- [ ] **O06.2 — Candidate order.**
  - Java: `ReadSortedRouteItems.next` (`:1161-1243`), `prepareCandidateItems`
    (`:587-628`), the `maxItems` truncation (`:641-654`).
  - Rust: `optimize/batch/candidates.rs`, O(n log n) with the same result:
    - vias: not user-fixed, no ignored net; key (centre x, centre y, first
      layer) as `f64`;
    - traces: not shove-fixed, no ignored net, no unfixed via among normal
      contacts; key (the lexicographically larger end corner, layer);
    - sort by (key, vias before traces, item-list order = id descending)
      and keep the first item of each distinct key (Java needs a strictly
      greater key for every next pick, so every other item with that key
      is skipped for the whole pass);
    - PRIORITIZED with a previous result map: items with a result first, in
      ascending `ItemRouteResult` order, then the rest in key order; then
      clear the map.
  - **Accept:** `OptimizeCandidatesProbe` mode `order` on 10 fully routed
    boards (O06.7's six plus four more from `fixtures/reference/route`):
    the SEQUENTIAL id list is equal; `equal_key_skips_rest_of_pass` on a
    synthetic board (two vias and one trace sharing a key; only the higher-id
    via is a candidate).

- [ ] **O06.3 — One candidate.**
  - Java: `optRouteItemOnBoard` (`:852-942`).
  - Rust: `optimize/batch/item.rs`, `opt_route_item(board, item, baseline_len,
    with_preferred_directions, use_increased_ripup, settings) ->
    ItemRouteResult` (work counts on the candidate board's own meter).
  - Orders (exact): cheap statistics before; an ignored net → unimproved;
    incompletes from the ledger; rip set = the item plus its start and end
    contacts when they are only unfixed traces, expanded to connection items
    (stop option NONE), all in id order; any user-fixed or ignored item →
    unimproved; remove; `combine_traces` per net of the item; ripup cost =
    `start_ripup_costs` (× `additional_ripup_cost_factor_at_start` in `i32`
    while increased), then for a trace `round(trace_ripup_cost_factor ×
    cost)` as `f64` then `i32`; A11.6's `route_passes_for_optimizer`
    with `max_autoroute_passes` and the pull-tight accuracy; cheap
    statistics after; the result.
  - **Accept:**
    - `BatchOptimizerTraceMetricTest` (3) ported;
    - `OptimizeCandidatesProbe` mode `items`: the first 20 candidates of
      pass 1 on O06.7's six boards, each on a fresh `deepCopy()` of the
      baseline, dump the `ItemRouteResult` fields. Equal for every
      candidate once A14's board-parity gate passes on these boards; before
      that the item is `[~]` and the note gives the match rate per field.

- [ ] **O06.4 — Parallel evaluation with an ordered merge.**
  - Java: `optRoutePass` (`:630-822`), `OptimizeCandidateTask`,
    `WorkerBoardState`.
  - Rust: `optimize/batch/worker.rs` (see the decisions above).
  - Consumption rules (exact): per consumed result `total_items += 1` and
    record it in the result map; improved → reset the failure counter, and
    replace the winner if strictly better; otherwise count a failure and
    stop the pass at 12 (pass 1) or 50 (configurable). Cancel or timeout
    before a chunk ends the pass without adopting a winner.
  - **Accept:**
    - `optimizer_is_thread_count_independent`: threads 1, 2, 4, 8 on
      `Issue508-DAC2020_bm08` + `-routed.ses`, `Issue026-J2_reference` +
      SES, `Issue508-DAC2020_bm05` + SES; the per-candidate result log, each
      pass's winner and the final `state_hash` are equal;
    - `chunk_size_does_not_change_results` (1, 8, 64);
    - `candidate_copy_equals_deep_copy`: the board each candidate starts
      from has the same `state_hash` and tree fingerprint (B02.1) as
      `baseline.deep_copy()`;
    - `candidates_leave_the_baseline_untouched`: the baseline's
      `state_hash` is the same before and after a pass's evaluation.

- [ ] **O06.5 — The pass loop.**
  - Java: `runBatchLoop` (`:279-517`), `restoreIncumbentBoard`.
  - Rust: `optimize/batch/mod.rs`.
  - Orders (exact): baseline statistics and guards; incumbent = deep copy;
    threshold: NaN, infinite or negative → 2.5, (0, 0.1) → × 100; loop while
    `pass < max_passes`, `items < max_items`, not cancelled; odd passes use
    preferred directions; after the pass, adopt the winner, then
    `use_increased && route_improved == 0.0` → clear the flag; gate against
    the incumbent (accept → incumbent = deep copy; reject → restore a deep
    copy of it); improvement % = `(after − before) / before × 100` in `f64`
    from `f32` scores (0 when `before ≤ 0`); continue if the flag was still
    set and the score didn't rise (clearing it), else stop below the
    threshold; at the end restore the incumbent if the final score is lower
    (D8: the best board is the one kept).
  - **Accept:** `BatchOptimizerDeterminismTest` (2: bm01 routed by the Rust
    autorouter, 1 vs 4 threads with 1 pass and 6 items gives the same hash
    and statistics; `max_passes = 0` keeps the board; the done note gives
    bm01's optimizer score and whether any pass was accepted, because a
    clamped score of 0 makes the first case trivially true),
    `BatchOptimizerStrategyTest` (SEQUENTIAL and PRIORITIZED × threads on
    bm08-routed: no new violations or incompletes), and the optimizer half of
    `MaxFanoutOptimizerSettingsTest` (2) ported and green;
    `first_pass_without_winner_stops` pins the quirk.

- [ ] **O06.6 — Stage registration and events.**
  - Implement A10's `Optimizer` trait as id `freerouting-optimizer` (Java's
    `BatchOptimizer.getId`), selected by `router.optimizer.algorithm`.
  - Events through A08.4's sink: `Progress` with phase `optimize` after each
    pass and after each adoption (statistics, scores, the current item
    position: via centre or trace first corner), `Delta` from the change
    journal when a winner is adopted, and the summary at the end. A08.5's
    `stop_router()` lets the optimizer run; stop-all ends it before the next
    chunk.
  - **Accept:** `optimizer_event_order` unit test; the registry resolves
    `freerouting-optimizer` to this stage.

- [ ] **O06.7 — Optimizer references (`fixtures/reference/optimize/`).**
  - Java: `ReferenceDump` mode `optimize` (O06's own; append only). For each
    board: load the DSN with `HeadlessBoardManager.loadFromSpecctraDsn`,
    import the Java-routed SES from `fixtures/reference/route/` (J01: work
    bounds, optimizer off), set a non-null `job.thread` (the loop calls
    `thread.isStopRequested()` unguarded; `BatchOptimizerDeterminismTest`
    uses a no-op `StoppableThread`), run `BatchOptimizer.create(job).runBatchLoop()`
    with `max_passes = 3`, `max_items = 200`, SEQUENTIAL, no timeout, once
    with `max_threads = 1` and once with 7. Write `<board>.t1.ses`,
    `<board>.t7.ses` and `<board>.json` (per-pass statistics from the
    board-updated events, final statistics and optimizer score, the pin,
    the JDK, whether the pin has Java's worker fix).
  - Boards: `Issue508-DAC2020_bm08` (+ `-routed.ses`),
    `Issue026-J2_reference`, `Issue508-DAC2020_bm05`, and the three
    smallest `completed` route references with zero incompletes and at
    least two vias (names in the README). Not bm01 (see above); any board
    whose Java run accepts no pass is reported as such in the README and
    gives no parity evidence.
  - Rust: `tests/reference_optimize.rs`.
  - **Accept:**
    - before Java's fix: on every board, Rust has incompletes and clearance
      violations ≤ Java's (t1), and via count and weighted length within
      Java's own t1-vs-t7 spread plus 2 %;
    - after the fix is re-pinned: Java t1 ≡ t7 (checked by the test), and
      the Rust SES equals Java's by F02.2 `ses-diff`. Until then this
      item is `[~]` with the aggregate table in the note.

- [ ] **O06.8 — Determinism under threads and load.**
  - `examples/optimize_board.rs <dsn> <ses> <out.ses> --threads N
    --max-passes P --max-items I` writes the SES with C05's writer.
  - **Accept:**
    - on O06.7's six boards, `--threads 1` and `--threads N` (N =
      available parallelism, at least 4) give byte-identical SES files;
    - `tools/determinism-check.sh` reports `identical` for
      `optimize_board --threads N` on bm08 and J2, idle versus one burner
      per core;
    - once C07 wires the optimizer into the CLI, the same pair through
      `freerouting -de … -do …` is byte-identical (until then `[~]`).
