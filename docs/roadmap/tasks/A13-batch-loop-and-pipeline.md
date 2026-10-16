# A13 — The batch loop, restores and `run_pipeline`

Opened 2026-10-15. This checklist assembles the router: the fanout stage, then passes
until a stop rule fires, with a history of the best boards that the loop restores when a
pass made things worse, the last-mile blocker ripup, the final best-board restore and
tail cleanup, and the hand-off to the optimizer. It drives A10's pure pass controller with
A11's pass runner and A12's fanout, using B01's snapshots for every restore. The result is
`run_pipeline`, the routing job the CLI (C07), the GUI (G07) and the plugin call
([A00](A00-autorouter-README.md); pipeline report P8 items 2–3, 5–7).

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

- [ ] **A13.1 — Snapshots for the history.** Implement A08's `HistoryEntry` for B01's
  snapshot: `state_hash`, the router score computed when the entry is added, restore by
  B01's restore. Measure the history's memory and the snapshot time on the three largest
  fixtures (board report Q12).
  - **Accept:** `restored_board_continues_identically`: route J2 for 3 passes, snapshot,
    route pass 4 twice from the snapshot, and the two results are equal item for item;
    `restore_rolls_back_failure_log_and_ids`; memory and time numbers in the done note.

- [ ] **A13.2 — The batch loop.** Port `AutorouteBatchLoop.run` around A10's controller:
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

- [ ] **A13.3 — Last-mile blocker ripup.** Port `LastMileBlockerRipup.ripBlockers`
  (`:28-63`): the DRC's airlines in order; for each, the corridor (A10.7) and
  `overlapping_objects(corridor, all layers)` on the default tree in `TreeObject` order;
  removable blockers (A10.7) go into an insertion-ordered set until it holds 24
  (`MAX_ITEMS`); one `remove_items` call with them in that order; return the count.
  - **Accept:** probe kind `last-mile` at `30baad7e2`: on `PCBench-retroreflectors-CONGFLOCK`
    and `Issue508-DAC2020_bm01`, each after importing Java's routed reference SES, the
    airline order and the removed ids in order are equal to Java's; at most 24 removed.

- [ ] **A13.4 — `run_pipeline`.** Port `RoutingPipeline.runRoutingStage` and the hand-off
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

- [ ] **A13.5 — Ported pipeline tests.** With `TestingSettings` semantics (edge clearance
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

- [ ] **A13.6 — Determinism with the audit and the sinks.**
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

- [ ] **A13.7 — Pass log against Java.** The per-pass score line Java logs (`Auto-routing
  pass #N ... with score S (U unrouted and V violations)`) gives, per pass, the score,
  unrouted and violation counts. Read them from the Java route reference's log (J01.3) and
  compare with Rust's `Progress` at the end of each pass on `Issue026-J2_reference` and
  `Issue508-DAC2020_bm08`, bounded as in `route-bounds.tsv`.
  - **Accept:** equal per pass on both boards; if J01.3 does not record the lines, the item
    is `[~]` with that request named (A00 open question 5).
