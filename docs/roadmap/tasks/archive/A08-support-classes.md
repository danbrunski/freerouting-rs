# A08 — Support classes: history, failure log, results, events, cancel

Opened 2026-10-15. The batch pipeline leans on a handful of small Java classes:
`BoardHistory` keeps the best boards and decides which one to restore,
`RoutingFailureLog` counts failures per item and rolls back with the board,
`ItemRouteResult` ranks optimizer candidates, and the events and the two-level stop
connect the router to the GUI and the CLI. They are pure logic, so this checklist can run
early, beside the search core ([A00](A00-autorouter-README.md)). The event shape is fixed
here so the GUI (G07) can be written against it before the batch loop (A13) exists.

**Prerequisites:**
- None for A08.1's logic, A08.3, A08.4 and A08.5.
- B01.3 (snapshots, `deep_copy`), B01.5 (`state_hash`) and B06 (router score with
  `RouterSettings`) for A08.1's real-board test, and B01.3 for A08.2's rollback test.
- B08 (`ItemGeometry`) for the `Delta` payload in A08.4; until then `Delta` carries
  `ItemId`s and a placeholder type alias.
- Decisions D2, D7, D8.

## What exists and what this adds

| Exists | State |
|---|---|
| `freerouting_geometry::Stoppable` (`request_stop`, `is_stop_requested`) | Present; one level only |
| `scoring::BoardStatistics` with `router_score_v2` | Present; B06 adds the `RouterSettings` dispatch |
| `autoroute/BoardHistory.java` (221 lines; `MAX_HISTORY_SIZE = 30`) | Missing |
| `autoroute/RoutingFailureLog.java` (161; `FAILURE_THRESHOLD = 50`; a `RoutingBoard` field serialized with the board, `RoutingBoard.java:64`) | Missing |
| `autoroute/ItemRouteResult.java` (145) | Missing; O06 uses it |
| `autoroute/events/*` (83 lines), `core/StoppableThread.java` two-level stop | Missing |
| `autoroute/BoardHistoryEntry.java` (top level, unused), `PerformanceProfiler`, `AutorouteDiagnostic` | Not ported: unused, timing-only, and replaced by A04's sinks respectively |

## Decisions made here

- **`BoardHistory<S: HistoryEntry>`** is generic over what it stores: `S` gives
  `state_hash() -> u128` and is restorable. The score is computed once when the entry is
  added and stored as `f32`, as Java stores it. The pipeline instantiates it with B01's
  snapshot; tests use a fake.
- **The failure log lives on `RoutingBoard`** (an appended field), so B01's snapshot and
  restore roll it back exactly as Java's serialization does. It is a
  `BTreeMap<ItemId, FailureInfo>`; Java's `ConcurrentHashMap` is only looked up on the
  routing path, but sorted iteration costs nothing and removes the question.
- **Events.** `RouterEvent` is `Progress(Progress)`, `Delta { removed: Vec<ItemId>, added:
  Vec<ItemGeometry> }`, `Replaced { snapshot }` and `Finished { board, summary }`. The first,
  second and last are the shape G00 and G07 ask for. `Replaced` carries a B01 snapshot
  (cheap after B01.4) whenever the router swaps in another board: a history restore or the
  final best-board restore. Java's `de7a4fd2c` fixed exactly this: the GUI kept the board
  from before the first restore and saved a stale one (95 unrouted reported, 147 saved).
  Listeners rebuild from `Replaced` and never assume the board object stays the same.
  `Progress` carries: phase (fanout, autoroute, optimize), pass, items to go, routed,
  failed, ripped, extra vias, incomplete, violations, score, the current airline's two
  endpoints (Java's `BatchAutorouter.getAirLine`), and an optional optimizer position (O06
  fills it). Events go over a bounded `std::sync::mpsc::sync_channel`. `Progress` is sent
  with `try_send` and dropped when the channel is full; `Delta` and `Finished` block. The
  router emits at work boundaries only (after a connection, a pin or a pass), so the
  cadence never depends on time and never changes a result.
- **`CancelToken`** has two levels, as `StoppableThread`: `stop_router()` sets
  router-only unless a full stop is already set; `stop_all()` sets the full stop.
  `is_router_stop_requested()` is true for either; `is_stop_requested()` only for the full
  stop. It implements `Stoppable` with the full-stop meaning, which is what Java's maze
  search and pull-tight check. Atomics, no locks.

## File ownership

| Item | New files | May edit |
|---|---|---|
| A08.1 | `autoroute/history.rs`, `crates/freerouting/tests/autoroute_history.rs` | `autoroute/mod.rs` (append) |
| A08.2 | `autoroute/failure_log.rs` | `board/routing_board.rs` (append the `failure_log` field, its accessors, and its initialisation in the constructors), `board/snapshot.rs` (add the field to B01.3's `BoardSnapshot` and its `restore`, as B01.3 asks) |
| A08.3 | `autoroute/item_route_result.rs` | — |
| A08.4, A08.5 | `autoroute/events.rs` | — |

## Checklist

- [ ] **A08.1 — `BoardHistory`.** Port `BoardHistory.java` exactly:
  - `add` (`:60-94`): skip when an entry has the same hash; when full, compute the new
    score, find the worst entry by a linear scan keeping the **first** of equal worst
    scores, and drop the newcomer if its score is `<=` the worst; otherwise remove the
    worst and append;
  - `max_score` starts at **0** (`:131-145`), so negative scores report 0;
  - `restore(max_restore_count)` (`:151-178`): `<= 0` means unlimited; a **stable** sort
    by score descending (`Float.compare` semantics: use `f32::total_cmp` reversed and
    check it agrees with `Float.compare` for `-0.0` and NaN in a unit test); the first
    entry with `restore_count <= max` has its count incremented and is restored;
  - `restore_best = restore(0)`; `rank(hash)` is the 1-based list position, which is
    insertion order until a restore has sorted the list; `contains`, `remove`, `clear`,
    `size`.
  - **Accept:**
    - unit tests with a fake entry: tie at capacity dropped, strictly better evicts the
      first worst, `restore(3)` returns the same board at most four times, rank before and
      after a restore, `max_score` with all-negative scores;
    - port `autoroute/BoardHistoryTest` (6 tests, fixtures `empty_board.dsn` and
      `Issue159-setonix_2hp-pcb.dsn`) once B01 and B06 land: restored boards have equal
      `state_hash`; the empty board wins; cap 1 keeps the better board.

- [ ] **A08.2 — `RoutingFailureLog`.** Port `RoutingFailureLog.java`: `record_failure(item,
  pass, state, reason)` creates the entry with the item's first net (or -1) and increments
  the count; `failure_count(item)`; `should_skip` at `>= 50`; `unroutable_items` sorted by
  id; `clear`. Put it on `RoutingBoard` as the decisions say.
  - **Accept:** unit tests; and, once B01 lands, `failure_log_rolls_back_with_snapshot`:
    record, snapshot, record again, restore, and the count is the snapshot's.

- [ ] **A08.3 — `ItemRouteResult`.** Port `ItemRouteResult.java`: `improved` by incomplete,
  then vias, then length (`:23-58`); `compare` by incomplete after, vias after, length
  after, then item id (`:69-90`); `improved_over`. `improvement_percentage` keeps Java's
  **integer division** `via_after / via_before` inside the `f64` expression (`:55-58`),
  `// PARITY(quirk):`.
  - **Accept:** unit tests including a case where the integer division gives a different
    value from real division.

- [ ] **A08.4 — Router events.** `RouterEvent`, `Progress`, `RunSummary`, the `EventSink`
  wrapper over the bounded channel, and a `NullEvents` sink for the CLI and tests. Java's
  progress counters (`core/RouterCounters.java`) map onto `Progress` field by field; the
  done note lists the mapping. `RunSummary` holds what Java's
  `Auto-routing stage completed: ...` log line reports (start and final unrouted, passes,
  final score with incomplete and violations).
  - **Accept:** a test that a full channel drops `Progress` and blocks on `Delta` and
    `Replaced` (with a reader thread), and that `Finished` moves the board out.

- [ ] **A08.5 — Two-level cancel token.** `CancelToken` as in the decisions.
  - **Accept:** port `core/StoppableThreadTest` (stop-all implies router stop; router stop
    does not imply stop-all; a full stop after a router stop gives the full stop; three
    tests).
