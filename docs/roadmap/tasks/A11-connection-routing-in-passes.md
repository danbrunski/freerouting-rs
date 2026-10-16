# A11 — Connection routing in the batch passes

Opened 2026-10-15. A06 routes one connection. The batch router routes every unrouted
item once per pass: it picks the via cost, aims plane nets at their planes, pulls the new
trace tight with the via optimizer, retries with a neck-down width, rejects new copper
that violates clearance from pass 3 on, counts failures and rips a net that keeps failing,
and removes tails at the end of the pass. This checklist ports that as the Freerouting
connection-router stage and the pass runner, adds the entry point the optimizer (O06)
calls, and compares a whole pass connection by connection with Java: the step from rung 2
to rung 3 ([A00](A00-autorouter-README.md); pipeline report P8 items 1–2).

**Prerequisites:**
- A06 (`route_connection`), A10 (traits, ordering, ripup policy), A08 (failure log,
  events, cancel token).
- O05: `opt_changed_area` with trace costs, which runs the via optimizer after every
  routed connection (`AutorouteConnectionRouter.java:122-128`).
- B01 (ids, snapshots for the strict-DRC rollback, the routing ledger for incomplete
  counts), C03 (routing load path).
- J01 (the re-pin to `30baad7e2`, where every limit is a work count) for the `pass`
  probe's acceptance; the code can start at `cda540f`.
- P02.1 for the audit in A11.8.
- Decisions D2, D4, D8, D9.

## What exists and what this adds

| Exists | State |
|---|---|
| A06 `route_connection`; A10 stage traits, `FreeroutingOrdering`, `FreeroutingRipupPolicy`; A08 `RoutingFailureLog` on the board | Prerequisites |
| `BasicBoard::power_plane_areas` (`board/basic_board.rs`), DRC per item (`drc/`) | Present |
| `pipeline/AutorouteConnectionRouter.java` (295 lines): `route` (`:30-180`), `retryConnectionNecked` (`:193-273`), `applyStrictDrcAfterRoute` (`:274-294`) | Missing |
| `pipeline/AutoroutePassRunner.java` `runSingleThread` (`:40-253`) | Missing |
| `pipeline/BatchAutorouter.java`: `enforceStrictDrc` (`:313-337`), `removeTails` (`:534-550`), `autoroutePassesForOptimizingItem` (`:253-289`) | Missing |

What this adds: `autoroute/batch/{connection.rs, strict_drc.rs, pass.rs}`, the `pass`
probe, and an example binary for the determinism check.

## Decisions made here

- **`FreeroutingConnectionRouter` implements A10's `ConnectionRouter`** with the plane-tap
  and neck-down policies as separate registered stages, so an X-series stage can replace
  one without the other.
- **Budgets, as at `30baad7e2`.** The connection's `WorkLimit` is `min(100000 *
  2^(pass-1), i32::MAX)` nominal ms (`AutorouteConnectionRouter.java:83-85`), times 64,000
  units, started **before** the session is created and shared by the plane attempt and its
  fallback (`:100-109`). The pull-tight after a connection and in `remove_tails` gets its
  own limit of 1,000 nominal ms (`ENDLESS_LOOP_GUARD_NOMINAL_MS`). Each limit counts every
  node popped on B02.7's meter after the limit was created, so room completion, shove
  checks and drill computation draw on the connection's limit.
- **No limit spans a board replacement.** Java's meter belongs to the thread; B02.7's
  belongs to the board and B01.3's restore zeroes it. Java never checks a limit after a
  restore (the strict-DRC rollback comes after the search). The Rust code keeps it that
  way: every limit is created on the board it is checked against, after any restore that
  precedes it. A unit test routes a strict-DRC rejection with `strict_drc` on and then the
  next connection, and checks that the second connection's limit starts at zero on the
  restored board.
- **The strict-DRC rollback has two forms, as in Java.** With `strict_drc` off (the default)
  a rejection removes the new items (id above the watermark) of the routed net with one
  `remove_items` (`BatchAutorouter.enforceStrictDrc`). With `strict_drc` on, the board taken
  before the route is restored whole (`AutorouteConnectionRouter.java:94, 289-291`),
  using B01's snapshot. Both are kept.
- **Incomplete counts** come from B01.6's ledger, as Java reads
  `routingLedger().incompleteCount()`. Until B01.6 lands, the DRC's full count gives the
  same number.
- **The optimizer entry point** is a plain function, `route_passes_for_optimizer`, not a
  stage. O06 calls it the way `BatchOptimizer` calls
  `BatchAutorouter.autoroutePassesForOptimizingItem`.

## File ownership

| Item | New files | May edit |
|---|---|---|
| A11.1, A11.2 | `autoroute/batch/connection.rs` | `autoroute/batch/mod.rs`, `autoroute/stages.rs` (registry entries only) |
| A11.3 | `autoroute/batch/strict_drc.rs` | — |
| A11.4–A11.6 | `autoroute/batch/pass.rs`, `crates/freerouting/tests/autoroute_pass.rs` | — |
| A11.7 | `fixtures/reference/probe/pass/` | `AutorouteProbe.java`, `fixtures.tsv`, `tests/autoroute_pass.rs` |
| A11.8 | `crates/freerouting/examples/route_pass.rs` | — |

## Checklist

- [ ] **A11.1 — `FreeroutingConnectionRouter`.** Port `AutorouteConnectionRouter.route`
  (`:30-180`):
  - via cost: `plane_via_costs` when the net contains a plane, else `via_costs`; the
    control from A01 with `ripup_allowed = true`, `ripup_costs` from the ripup policy,
    `remove_unconnected_vias` from the pass;
  - an empty unconnected set gives `NoUnconnectedNets`;
  - start and destination: the plane-tap policy (A11.2) for plane nets, else start =
    unconnected set and destination = connected set;
  - the id watermark `max_generated_id` before routing; with `strict_drc`, a B01 snapshot;
  - `route_connection`; on `Failed` with a fallback, a new session, the fallback
    destination and the **same** budget;
  - on `Routed`: `opt_changed_area` with O05.4's `PullTightParams::after_connection(accuracy,
    trace_costs)`: no net filter, the pull-tight accuracy, the trace costs (so the via
    optimizer runs), and a `WorkLimit` of 1,000 nominal ms;
  - on `Failed` or `InsertError`, the neck-down policy (A11.2); a necked success goes
    through the strict check (A11.3) before it is returned;
  - on `Routed`, the strict check;
  - any panic-free error path that Java reaches through `catch (Exception)` returns
    `Failed`.
  - **Accept:** covered by A11.7; a unit test that the fallback reuses the budget.

- [ ] **A11.2 — Plane taps and neck-down as policies.**
  - `FreeroutingPlaneTaps` (`:59-77`): a plane net whose connected set holds a
    conduction area gives `ConnectedToPlane`; otherwise start = connected set, primary =
    `power_plane_areas(unconnected)` when not empty with fallback = unconnected, else
    primary = unconnected. This is the D8 rule that each plane-net pad gets its own via.
  - `FreeroutingNeckDown` (`retryConnectionNecked`, `:193-273`): only when
    `neck_width_um > 0`; `neck_half_width = max(1, neck_width / 2)` in board units; skip
    when no active layer's half width exceeds it; a new control with half widths clamped and
    the compensation kept (`compensated = clamped + (old compensated - old)`); a new
    session; pull-tight on success.
  - **Accept:** on `Issue269-min_fr_test/min_fr_test.dsn`, pass 1 gives one via per GND
    pad and no trace between the two GND pads (the routing half of `PlaneFanoutViaTest`;
    A12 adds the fanout half); a unit test of the neck-down width clamp.

- [ ] **A11.3 — Strict DRC.** `enforce_strict_drc(board, net, max_id_before) ->
  Option<AttemptResult>` (`BatchAutorouter.java:313-337`): collect the net's traces and
  vias with an id above the watermark in `connectable_items` order; if any has a clearance
  violation, remove all of them with one `remove_items` and return `Failed`. Then the
  snapshot restore when `strict_drc` is on (decisions).
  - **Accept:** port `autoroute/StrictDrcEnforcementTest` (fixture
    `Issue575-drc_BBD_Mars-64_6_track_1_hole_clearance_violations.dsn`: watermark 0
    rejects and the trace count drops; watermark `max_generated_id` returns `None` and
    the violation count is unchanged).

- [ ] **A11.4 — The pass runner.** Port `runSingleThread` (`:40-253`) as
  `run_pass(board, pass, stages, ctx) -> PassOutcome`:
  - `select`, and `order` when `pass > 1`; an empty list returns "nothing to route";
  - for each item, for each of its nets: stop on a router stop; `max_items` reached →
    **full stop** (`requestStop`, so the optimizer is skipped too); count the item;
    `start_marking_changed_area`; a fresh ripped set and cost list; route;
    `Routed` counts as routed; `AlreadyConnected`, `NoUnconnectedNets` and
    `ConnectedToPlane` as skipped; anything else records a failure in the board's failure
    log and calls the ripup policy's `after_failure` with the item's count;
  - at the end: `remove_tails` (A11.5) with `NONE` when the pass removes unconnected vias,
    else `FANOUT_VIA`; the incomplete count and nets; `on_pass_end` for the ordering;
  - events: `Progress` after each connection and at the end of the pass (A08);
  - return `routed > 0 || failed > 0`; a Java exception path returns false.
  - **Accept:** covered by A11.7; a unit test that `max_items` sets the full stop.

- [ ] **A11.5 — `remove_tails`.** `BatchAutorouter.removeTails` (`:534-550`):
  `start_marking_changed_area`, `remove_trace_tails(-1, option)`, then
  `opt_changed_area` with `PullTightParams::after_connection` (O05.4), the same parameters
  as after a routed connection.
  - **Accept:** covered by A11.7.

- [ ] **A11.6 — Optimizer entry point.** `route_passes_for_optimizer(board, max_passes,
  ripup_costs, accuracy, with_preferred_directions, settings, ctx) -> u32` =
  `autoroutePassesForOptimizingItem` (`:253-289`): a pass runner with
  `remove_unconnected_vias = true` and pass logging off; passes `1..=max_passes` while
  something is left and no router stop; then `remove_tails(NONE)`; return the pass count
  with Java's off-by-one (`--currentPassNo` when the board completed).
  - **Accept:** a unit test of the returned count for a board that completes in pass 1 and
    one that never completes; O06 is the consumer.

- [ ] **A11.7 — `pass` probe.** Java kind `pass`: load the board as J01's route mode does
  (fanout off, so the pass sees the loaded board), build the `BatchAutorouter`, and run
  pass 1 then pass 2 by replicating `runSingleThread`'s item loop around the real
  package-private `autorouteItem` (reflection), with Java's own `getAutorouteItems` and
  `reorderSingleThreadItems`. Per connection print: index, item id, net, state, ripped ids,
  `maxGeneratedId` before and after, an FNV-1a digest of the added items' canonical lines
  (as A06.7), the item's failure count, and the thread's `WorkMeter` units spent on the
  connection (`WorkMeter.current().units()` before and after; units above the budget mean
  the limit fired). Per pass: the incomplete count, the tail-removal result and its units. The probe also
  runs the real `autoroutePass(1)` on a second fresh load and prints whether the board
  equals the replica's (`replica_matches_real`), so the replica cannot drift from Java.
  - **Accept:** `pass_matches_java`: equal for passes 1 and 2 on every board in
    `tools/bench/sets/connection-parity.txt` and on `Issue026-J2_reference`, with
    `replica_matches_real` true on all of them, at `30baad7e2`. The work units must match
    too; a board where only the units differ is listed with its reason (tree shape, B02)
    until B02's proof lands.

- [ ] **A11.8 — Determinism with the audit.** `examples/route_pass.rs <board.dsn>
  <out.jsonl>` writes the `pass` probe lines for passes 1 and 2.
  - **Accept:** `tools/determinism-check.sh` reports `identical` on every board of
    `connection-parity.txt` and on J2, run with P02.1's audit on
    (`router.debug.audit_incremental = true`) and zero audit differences. Load averages in
    the done note. If P02.1 has not landed, mark the item `[~]` with that remainder.
