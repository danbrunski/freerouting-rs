# A13 — The batch loop, restores and `run_pipeline`

Opened 2026-10-15. This checklist assembles the router: the fanout stage, then passes
until a stop rule fires, with a history of the best boards that the loop restores when a
pass made things worse, the last-mile blocker ripup, the final best-board restore and
tail cleanup, and the hand-off to the optimizer. It drives A10's pure pass controller with
A11's pass runner and A12's fanout, using B01's snapshots for every restore. The result is
`run_pipeline`, the routing job the CLI (C07), the GUI (G07) and the plugin call
([A00](../A00-autorouter-README.md); pipeline report P8 items 2–3, 5–7).

**Prerequisites:**
- A12 (fanout), A11 (pass runner), A10 (pass controller, registry), A08 (history,
  events, cancel token).
- B01: `deep_copy` with Java's restore semantics, `state_hash`, the routing ledger. B06:
  the router score with `RouterSettings`.
- C02 (settings), C03 (load path), C05 for A13.7's SES output.
- P02.1 (the incremental-against-rebuild audit) for A13.6.
- Decisions D2, D4, D7, D8, D9.

## What exists and what this adds

| Exists | State |
|---|---|
| A08 `BoardHistory`, `RouterEvent`, `CancelToken`; A10 `FreeroutingPassController`; A11 `run_pass`, `remove_tails`; A12 `FreeroutingFanout` | Prerequisites |
| `drc` airlines in a fixed order (Rust starts each net's group at the first remaining item in board order; DIVERGENCES) | Present |
| `pipeline/AutorouteBatchLoop.java` (713 lines), `pipeline/LastMileBlockerRipup.java` (93), `pipeline/RoutingPipeline.java` (151) | Missing |

What this adds: `autoroute/batch/{batch_loop.rs, last_mile.rs, snapshot.rs}`,
`autoroute/pipeline.rs` with `run_pipeline`, and an example binary that routes a board.

**Measured in Java (the Java session, 2026-10-15, `a773a70ec`, deterministic,
`canary-pwr` "pours1"):**
- **Pass 1 is the best pass at every ripup cost.** With `start_ripup_costs`
  100, 500 and 1500, pass 1 leaves 120, 109 and 100 connections unrouted.
  Every later pass is worse (161–185), and the loop's periodic restore puts the
  pass-1 board back.
- **The later passes are wasted time.** `-mp 1` gives the same SES as
  `-mp 10`, so passes 2–10 are about 60 % of the routing time for nothing.
- **Cause:** Java's ripup is greedy. Ripped blockers often can't be rerouted,
  and nothing remembers congestion between passes.

**Port the loop as Java has it, for parity, and record this as a known Java
weakness.** Changing it is a deliberate divergence (Q04, Q05) once pass-1
parity holds. Also seen on that board: `BasicBoard.normalizeTraces` hits its
2000-iteration oscillation guard 10 times, around fixed hand-laid copper (a
split/combine loop). Rust already has the guard and Java's per-board
suppression (`MAX_NORMALIZE_ITERATIONS`, `basic_board.rs`;
`BasicBoard::normalize_traces`, `trace_ops.rs`). Parity must reproduce the
same suppression on the same nets. Finding the oscillation's cause is a later
Q candidate.

## Decisions made here

- **A restore is a B01 snapshot restore, never a `Clone`.** The history stores B01
  snapshots. A restore brings back exactly what Java's serialized board holds: the items
  with their ids, the id generator, and the failure log (A08.2). Transient state resets as
  `BasicBoard.readObject` resets it: normalize suppression, the changed area, the autoroute
  info, the shove-failing obstacle, the ledger. The ordering stage's `reset()` runs after
  every restore (`resetAntiOscillationState`).
- **Statistics once per pass.** Java builds `BoardStatistics` three or four times per pass
  with the same result. The loop computes it once per board state (again only after a
  restore, a fanout recovery or a last-mile ripup) and passes it to the controller, the
  history and the events; P02.3 confirms the saving. Restore and stop decisions use the
  full statistics (clearance violations included) and the `f32` router score.
- **Last-mile airlines in input order.** At `cda540f` Java ordered them by identity hash
  (`NetIncompletes.calculateNetItems`). `30baad7e2` changed that to input order, which is
  what the Rust DRC already does, so the blocker ripup is reproducible and exact. Once
  A13.3's probe agrees, A13.3 rewrites the `NetIncompletes` sentence of the DIVERGENCES
  "Design rule check" entry to say the orders now match; the DRC's other `HashSet` orders
  stay listed.
- **`run_pipeline` returns the board; it writes nothing.** SES writing is C05's and the CLI
  flow is C07's. The final board is the one after the best-board restore and the optimizer
  (D8: the best board is the one saved). Every board swap, a restore during the passes or
  the final best-board restore, sends A08's `Replaced` event, so a GUI never keeps a stale
  board (Java `de7a4fd2c`, ledger "Port: yes").
- **Event cadence never touches the result.** Deltas are drained from B01's change journal
  after each connection or pin; `Progress` after each connection, pin and pass.

## File ownership

| Item | New files | May edit |
|---|---|---|
| A13.1 | `autoroute/batch/snapshot.rs` | `autoroute/batch/mod.rs` |
| A13.2 | `autoroute/batch/batch_loop.rs` | — |
| A13.3 | `autoroute/batch/last_mile.rs` | `docs/DIVERGENCES.md` (the `NetIncompletes` sentence only) |
| A13.4 | `autoroute/pipeline.rs` | `autoroute/mod.rs` (append) |
| A13.5, A13.6 | `crates/freerouting/tests/autoroute_pipeline.rs` | — |
| A13.7 | `crates/freerouting/examples/route_board.rs` | — |

## Checklist

- [x] **A13.1 — Snapshots for the history.** Implement A08's `HistoryEntry` for B01's
  snapshot: `state_hash`, the router score computed when the entry is added, restore by
  B01's restore. Measure the history's memory and the snapshot time on the three largest
  fixtures (board report Q12).
  - **Accept:** `restored_board_continues_identically`: route J2 for 3 passes, snapshot,
    route pass 4 twice from the snapshot, and the two results are equal item for item;
    `restore_rolls_back_failure_log_and_ids`; memory and time numbers in the done note.
  *Done 2026-10-16:* `autoroute/batch/snapshot.rs`: `HistorySnapshot` (B01 snapshot plus
  the state hash; `restore` is `BoardSnapshot::restore`), `ScoredBoard` (the
  `HistorySource` carrying the score of the statistics the loop already has),
  `BoardFigures`, `SnapshotHistory`. `tests/autoroute_pipeline.rs`:
  `restored_board_continues_identically` (J2 3 passes, snapshot, pass 4 twice: items,
  state hash and pass outcome equal; also bm01 with the fanout off, where pass 4 routes 25,
  fails 2 and rips 18) and `restore_rolls_back_failure_log_and_ids` (failure log, item ids,
  item contents and the next generated id come back) green. Numbers, release,
  `cargo run --release -p freerouting --example snapshot_bench` (median of 7, load 3.2 from
  other sessions): Issue190-processor.Z80 (10,885 items) snapshot 173 µs, restore 12.4 ms,
  state_hash 4.5 ms, 1.28 MB per snapshot; Issue054-tairakb (6,791) 63 µs, 7.9 ms, 6.5 ms,
  0.61 MB; Issue690-kit-dev-coldfire (5,551) 122 µs, 8.3 ms, 2.4 ms, 0.63 MB. A full
  history (30 boards) is 18-38 MB on these boards; the per-pass cost is one state hash
  and one snapshot (under 7 ms), a restore 8-12 ms.

- [x] **A13.2 — The batch loop.** Port `AutorouteBatchLoop.run` around A10's controller:
  - the routable-layer check (A10.7) first; an error ends the job;
  - the fanout stage when fanout is enabled and the board has SMD pins;
  - the pass loop (`:313-603`): `before_pass`; add the board to the history; `run_pass`;
    statistics and score once; `after_pass`, then apply the decision in Java's order:
    restore (swap in the restored board, send `Replaced`, reset the ordering, rescore;
    none restored means stop), fanout recovery (`remove_tails(NONE)`, rescore), last-mile ripup (A13.3,
    rescore when something was removed), stop; the pass number advances only while
    routing continues and no router stop was requested;
  - finish (`:607-642`): restore the best board when the history's best beats the final
    score, and send `Replaced`; the final `remove_tails(NONE)` only when the router was not stopped; clear the
    history;
  - the job state: a timed-out job (a stop-only deadline) asks for a router stop at the
    top of the next pass;
  - `RunSummary` (A08.4): start and final unrouted, passes, final score with incomplete
    and violations, and the resolved stage selection.
  - **Accept:** covered by A13.5 and A13.6; a unit test with a fake pass runner that feeds
    the score sequence of A10.3's restore case and checks the board actually swapped.
  *Done 2026-10-16:* `autoroute/batch/batch_loop.rs` (`run_batch_loop`, `PassExecutor`,
  `LoopStages`, `PassRecord`, `LoopOutcome`): routable-layer check, fanout, the pass loop
  driving `FreeroutingPassController` with restores (swapped in after the rank check,
  `Replaced` sent, `ordering.reset()`), fanout recovery, last-mile ripup, the finish with
  the best-board restore and the final `remove_tails(NONE)`, statistics once per board
  state. Fake-runner test: `reported_equals_saved` feeds A10.3's restore case (10 per pass
  to pass 7, then 50) through `run_pipeline_with`; the pass-8 restore swaps in the board
  stored before pass 8 (its hash is the one after pass 7, and the pass-8 line reports it at
  70), two `Replaced` events carry that board, `restores == 1`, `restored_best`. Real
  boards: `pass_scores_match_java` on CONGFLOCK (100 passes, natural stop at 30) runs three
  history restores, the one-time fanout recovery, two last-mile ripups (8 and 6 removed),
  the global-stagnation stop and the final best-board restore, and all 30 pass lines and
  the SES equal Java's byte for byte. A13.5 and A13.6 below.

- [x] **A13.3 — Last-mile blocker ripup.** Port `LastMileBlockerRipup.ripBlockers`
  (`:28-63`): the DRC's airlines in order; for each, the corridor (A10.7) and
  `overlapping_objects(corridor, all layers)` on the default tree in `TreeObject` order;
  removable blockers (A10.7) go into an insertion-ordered set until it holds 24
  (`MAX_ITEMS`); one `remove_items` call with them in that order; return the count.
  - **Accept:** probe kind `last-mile` at `30baad7e2`: on `PCBench-retroreflectors-CONGFLOCK`
    and `Issue508-DAC2020_bm01`, each after importing Java's routed reference SES, the
    airline order and the removed ids in order are equal to Java's; at most 24 removed.
  *Done 2026-10-16:* `autoroute/batch/last_mile.rs` (`plan_last_mile`, `rip_blockers`,
  `MAX_ITEMS = 24`). Probe kind `last-mile` appended to `AutorouteProbe.java` and
  `fixtures.tsv` and generated at `a773a70ec` (the pin; `30baad7e2`'s `NetIncompletes`
  change is in it) into `fixtures/reference/probe/last-mile/` with
  `REFERENCE_WORK_DIR=target/orchestrator/a13/java-ref generate.sh <pin> last-mile`: the
  reference SES is imported and its traces and vias unfixed (Java's `SesReader` inserts
  them user fixed, which the ripup never removes); Java's `replica_matches_real` is true on
  both boards. `last_mile_matches_java` green: CONGFLOCK 4 airlines, 11 removed;
  bm01 24 airlines, 24 removed (the cap). The DIVERGENCES `NetIncompletes` sentence now
  says the orders match.

- [x] **A13.4 — `run_pipeline`.** Port `RoutingPipeline.runRoutingStage` and the hand-off
  (`RoutingPipeline.java:95-150`):
  - the router runs when `autorouter.enabled && (max_passes is None || max_passes >= 0)`;
    else, with fanout enabled, a fanout-only run (Java temporarily sets `max_passes = 0`,
    and the loop then runs fanout alone);
  - the session is dropped (A06.6's `finish_autoroute` equivalent);
  - the optimizer stage from the registry runs unless a **full** stop was requested: it
    runs after a router-only stop (max passes, stagnation) and is skipped after
    `max_items`, a timeout or a cancel;
  - `Finished { board, summary }` hands the board back by move;
  - a job deadline (default 12 h in Java) only requests a full stop (D2).
  - **Accept:**
    - unit tests for the router-enabled rule (`max_passes` `None`, `0`, `2`, `-1`) and for
      the optimizer gate under each stop kind, using a fake optimizer stage;
    - `reported_equals_saved` (the engine half of Java's `fixtures/GuiFinalBoardTest`,
      `de7a4fd2c`): on a run that restores at least once (J2 with enough passes, or a fake
      pass runner that forces a restore), a listener that applies every `Replaced` event
      ends on the board that `Finished` returns, and that board's unrouted count equals the
      final unrouted count in `RunSummary`.
  *Done 2026-10-16:* `autoroute/pipeline.rs` (`run_pipeline`, `run_pipeline_with`,
  `PipelineJob`, `PipelineOutcome`, `PipelineError`/`PipelineFailure`, `router_enabled`).
  Tests: `router_enabled_rule` (`0`, `2` on; `-1` off; `enabled = false` off; `None` is not
  representable, the unset default is 0 and on), `disabled_router_with_fanout_runs_the_fanout_alone`,
  `negative_max_passes_with_fanout_routes_as_javas_fanout_only_run` (Java's quirk kept),
  `optimizer_gate_under_each_stop_kind` with a counting fake optimizer (runs after a
  finished run, after max passes and after a router stop request; skipped after
  `max_items`, a zero deadline and a cancel), `reported_equals_saved` (a listener that
  applies every `Replaced` ends on the `Finished` board; its incomplete and violation
  counts are the `RunSummary`'s). All green.

- [x] **A13.5 — Ported pipeline tests.** With `TestingSettings` semantics (edge clearance
  0, the optimizer off unless the test needs it):
  - `fixtures/MaxPassesSettingRoutingTest`: J2 with `max_passes = 2` ends at pass 2;
  - `fixtures/MaxItemsRoutingTest`, strengthened: J2 with `max_items = 20` routes exactly
    20 items, sets the full stop and skips the optimizer;
  - `fixtures/HeadlessCompleteRoutingTest`: `Issue508-DAC2020_bm01` bounded runs reach a
    terminal state; `empty_board.dsn` exits at once;
  - `fixtures/RoutingPipelineComparisonTest` as a determinism test: two runs of bm01 with
    one pass and two items give identical statistics;
  - `fixtures/Issue684MemoryLeakRoutingTest` test 1 (`MAX_HISTORY_SIZE == 30`).
  - **Accept:** all green; their wall time in the done note.
  *Done 2026-10-16:* `tests/autoroute_pipeline.rs`, `cargo test -p freerouting --test
  autoroute_pipeline` (dev profile, opt-level 1, load 4-7 from other sessions), each run
  alone: `max_passes_setting_is_respected` (J2, `summary.passes == 2`) 0.22 s;
  `max_items_limit` (J2, `max_items = 20`: `total_items_routed == 20`, the full stop set,
  the counting optimizer not run, Java's log line) 0.21 s; `headless_routing_completes`
  (bm01 3/50 and 2/20, `empty_board.dsn` in under 10 s with nothing routed) 3.17 s;
  `pipeline_runs_are_deterministic` (bm01 one pass, two items, twice: incomplete, vias,
  violations, state hash and summary equal) 2.32 s; `board_history_size_cap_is_bounded`
  0.00 s. All green; the whole file (14 tests) 23.7 s with 2 threads.
  `tests/autoroute_fanout.rs`'s `run_job` stand-in now runs `run_pipeline`; its 18 tests
  stay green (4.9 s).

- [~] **A13.6 — Determinism with the audit and the sinks.**
  - `examples/route_board.rs <board.dsn> <out> [--sink noop|counters]` routes with the
    reference bounds and writes the SES through C05 (or, until C05 lands, the canonical item
    lines of A06.7).
  - **Accept:**
    - `tools/determinism-check.sh` reports `identical` on every board of
      `connection-parity.txt` and on `Issue026-J2_reference`, `Issue508-DAC2020_bm08` and
      `Issue145-smoothieboard` from `benchmark.txt`, with P02.1's audit on
      (`router.debug.audit_incremental = true`) and zero audit differences;
    - the `noop` and `counters` sinks write identical output on every board of
      `connection-parity.txt` (A04's sink requirement at the SES level);
    - `autorouter.max_threads` 1 and 8 give identical output (Java's batch router never
      reads it; the setting must not change routing).
    Load averages in the done note. Without P02.1, mark the item `[~]` with that remainder.
  *Partial 2026-10-16:* done without the audit: `examples/route_board.rs` (`<board.dsn>
  <out.ses> [--sink noop|counters] [--max-threads N] [--no-bounds] [-- <Java CLI flags>]`,
  `route-bounds.tsv` bounds as J01.3's route mode, SES through C05's `write_ses`).
  `tools/determinism-check.sh` (release, a script routing each board into `{OUT}`) reports
  `identical` on the 10 boards of `connection-parity.txt` plus J2 and bm08 (load 8.47 ->
  8.04 idle, 8.75 -> 8.75 loaded, other sessions running) and on `Issue145-smoothieboard`
  (3 passes, 214 s a run; load 5.82 -> 2.16 idle, 2.71 -> 12.31 loaded). `--sink counters`
  (journal on, 47-344 progress events, deltas and `Replaced`) and `--max-threads 8` write
  SES files byte-identical to `--sink noop --max-threads 1` on those 12 boards. All 12 SES
  files are also byte-identical to `fixtures/reference/route`. Remainder: the same runs
  with `router.debug.audit_incremental = true` and zero audit differences need P02.1's
  audit, owned by P02 (P02.1).

- [x] **A13.7 — Pass log against Java.** The per-pass score line Java logs (`Auto-routing
  pass #N ... with score S (U unrouted and V violations)`) gives, per pass, the score,
  unrouted and violation counts. Read them from the Java route reference's log (J01.3) and
  compare with Rust's `Progress` at the end of each pass on `Issue026-J2_reference` and
  `Issue508-DAC2020_bm08`, bounded as in `route-bounds.tsv`.
  - **Accept:** equal per pass on both boards; if J01.3 does not record the lines, the item
    is `[~]` with that request named (A00 open question 5).
  *Done 2026-10-16:* J01.3 records the lines (`passes[].score` in
  `fixtures/reference/route/<board>.json`). `pass_scores_match_java` compares the batch
  loop's pass records (the figures of the pass-end `Progress` it sends, after the restore
  check, as Java logs them) with them: J2 (2 passes) and bm08 (2 passes) equal at their
  `route-bounds.tsv` bound of 100; CONGFLOCK (30 passes, three restores, two last-mile
  ripups) equal too; and the three routed boards' SES equal Java's byte for byte.
