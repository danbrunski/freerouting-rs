# A08 — Support classes: history, failure log, results, events, cancel

Opened 2026-10-15. The batch pipeline leans on a handful of small Java classes:
`BoardHistory` keeps the best boards and decides which one to restore,
`RoutingFailureLog` counts failures per item and rolls back with the board,
`ItemRouteResult` ranks optimizer candidates, and the events and the two-level stop
connect the router to the GUI and the CLI. They are pure logic, so this checklist can run
early, beside the search core ([A00](../A00-autorouter-README.md)). The event shape is fixed
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

- [x] **A08.1 — `BoardHistory`.** Port `BoardHistory.java` exactly:
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
  - *Partial 2026-10-15:* `crates/freerouting/src/autoroute/history.rs`:
    `BoardHistory<S>` with `add`, `restore`, `restore_best`, `rank`, `contains`,
    `remove`, `clear`, `len`, `max_score`, `MAX_HISTORY_SIZE = 30`; the board side is a
    `HistorySource` (hash, score, `to_entry`), the stored side a `HistoryEntry`
    (`state_hash`, `restore`). The score is computed once per `add`, the entry only made
    when the board is kept. `tests/autoroute_history.rs`, 13 tests with a fake entry, all
    pass: `tie_with_the_worst_at_capacity_is_dropped`,
    `strictly_better_evicts_the_first_worst`,
    `restore_3_returns_the_same_board_at_most_four_times`,
    `rank_is_insertion_order_until_a_restore_sorts`, `max_score_starts_at_zero`,
    `restore_zero_or_negative_is_unlimited`, `nan_scores_sort_first_as_in_java`,
    `signed_zero_scores_sort_positive_first` and five more. The sort uses
    `java_float_compare` (NaN made canonical, then `f32::total_cmp`), because
    `total_cmp` alone orders a NaN with the sign bit (x86's `0.0 / 0.0`) below
    `-inf`; unit tests `java_float_compare_matches_java` (81 pairs of `Float.compare`
    from a `probe.sh` run at `cda540f`) and
    `total_cmp_agrees_with_java_for_signed_zero_and_positive_nan`. Remaining: the port of
    `autoroute/BoardHistoryTest` (6 tests) needs B01.5's `state_hash` and B06.2's
    `RouterSettings` score, and the `HistoryEntry` impl for `BoardSnapshot`, which is
    A13.1's.
  - *Done 2026-10-16:* the remainder landed with B06.2. `tests/autoroute_history.rs`
    ports all six `autoroute/BoardHistoryTest` tests (`add_and_restore_board`,
    `restore_best_board_from_multiple`, `contains`, `clear`,
    `size_cap_never_exceeds_max_history_size`, `size_cap_evicts_worst_entry`) on
    `empty_board.dsn` and `Issue159-setonix_2hp-pcb.dsn`: restored boards have the
    original's `state_hash`, the empty board wins, cap 1 keeps the better board. The
    boards are scored with `BoardStatistics::router_score(Some(&settings))` on
    `resolve`d `DefaultSettings`; the entry is a minimal `HistoryEntry` for
    `BoardSnapshot` in the test file (A13.1 keeps the pipeline's own). `cargo test -p
    freerouting --test autoroute_history`: 19 passed (13 fake-board tests, 6 ports).

- [x] **A08.2 — `RoutingFailureLog`.** Port `RoutingFailureLog.java`: `record_failure(item,
  pass, state, reason)` creates the entry with the item's first net (or -1) and increments
  the count; `failure_count(item)`; `should_skip` at `>= 50`; `unroutable_items` sorted by
  id; `clear`. Put it on `RoutingBoard` as the decisions say.
  - **Accept:** unit tests; and, once B01 lands, `failure_log_rolls_back_with_snapshot`:
    record, snapshot, record again, restore, and the count is the snapshot's.
  - *Done 2026-10-15:* `crates/freerouting/src/autoroute/failure_log.rs`:
    `FailureLog` (`BTreeMap` by item id) with `record_failure`, `failure_count`, `info`,
    `should_skip` (`>= FAILURE_THRESHOLD = 50`), `unroutable_items` (ascending id),
    `has_unroutable_items`, `clear`; `RoutingBoard::record_routing_failure(item, pass,
    state, reason)` takes the item's first net or -1. The field, `failure_log()`,
    `failure_log_mut()` and the initialisation in `from_basic` are appended to
    `board/routing_board.rs`. `AttemptState` (Java `AutorouteAttemptState`) is in
    `autoroute/mod.rs` as A01.1 describes it. Unit tests, all pass:
    `first_failure_creates_the_entry_with_its_net`, `skip_starts_at_the_threshold`,
    `unroutable_items_are_sorted_by_ascending_id`, `clear_forgets_everything`,
    `board_records_with_the_first_net_of_the_item` (J2). The snapshot half landed with
    B01.3: `BoardSnapshot` carries the log and `restore`/`deep_copy` put it back;
    `tests/board_snapshot.rs::failure_log_rolls_back_with_snapshot` passes.

- [x] **A08.3 — `ItemRouteResult`.** Port `ItemRouteResult.java`: `improved` by incomplete,
  then vias, then length (`:23-58`); `compare` by incomplete after, vias after, length
  after, then item id (`:69-90`); `improved_over`. `improvement_percentage` keeps Java's
  **integer division** `via_after / via_before` inside the `f64` expression (`:55-58`),
  `// PARITY(quirk):`.
  - **Accept:** unit tests including a case where the integer division gives a different
    value from real division.
  - *Done 2026-10-15:* `crates/freerouting/src/autoroute/item_route_result.rs`, with
    `// PARITY(quirk):` on the integer division (`wrapping_div`, Java's `int` semantics).
    Unit tests against a `probe.sh` run of the Java class at `cda540f`:
    `improved_and_percentage_match_java` (8 cases, `improved()` and the raw `f32` bits of
    `improvementPercentage()`, including a negative via count and a zero length),
    `compare_matches_java` (25 `compareTo` pairs; the id tie-break is ascending by value,
    not `ItemId`'s descending order), `integer_division_differs_from_real_division`
    (2/3 vias: 0.55 against 0.2167 with real division), `unimproved_matches_java`,
    `reductions_and_update`; all pass.

- [x] **A08.4 — Router events.** `RouterEvent`, `Progress`, `RunSummary`, the `EventSink`
  wrapper over the bounded channel, and a `NullEvents` sink for the CLI and tests. Java's
  progress counters (`core/RouterCounters.java`) map onto `Progress` field by field; the
  done note lists the mapping. `RunSummary` holds what Java's
  `Auto-routing stage completed: ...` log line reports (start and final unrouted, passes,
  final score with incomplete and violations).
  - **Accept:** a test that a full channel drops `Progress` and blocks on `Delta` and
    `Replaced` (with a reader thread), and that `Finished` moves the board out.
  - *Done 2026-10-15:* `crates/freerouting/src/autoroute/events.rs`: `RouterEvent`
    (`Progress`, `Delta { removed, added }`, `Replaced { snapshot: Box<BoardSnapshot> }`,
    `Finished { board: Box<RoutingBoard>, summary }`), `Progress`, `Phase`, `RunSummary`
    (status, initial unrouted, passes, score, incomplete, violations, stage selection),
    `CompletionStatus`, the object-safe `RouterEvents` trait, `EventSink` over
    `sync_channel` and `NullEvents`. `ItemGeometry` is a placeholder alias for `ItemId`
    until B08.1. `finished` returns the board when nobody takes it. The
    `RouterCounters` mapping is a table in the module docs: `phase` -> `phase`,
    `passCount` -> `pass`, `queuedToBeRoutedCount` -> `items_to_go`, `routedCount` ->
    `routed`, `skippedCount` -> `skipped`, `failedToBeRoutedCount` -> `failed`,
    `rippedCount` -> `ripped`, `fanoutExtraViasCount` -> `extra_vias`,
    `incompleteCount` -> `incomplete`; `violations` and `score` come from the event's
    `BoardStatistics`, `airline` from `getAirLine`, `optimizer_position` from O06. Unit
    tests, all pass: `full_channel_drops_progress`, `delta_and_replaced_block_until_read`
    (bound 1, writer thread blocks in `delta` until the reader takes the progress
    event; a progress sent meanwhile is dropped), `finished_moves_the_board_to_the_listener`
    (and comes back when the receiver is gone), `null_events_drop_everything_but_return_the_board`,
    `sink_works_as_a_trait_object`.

- [x] **A08.5 — Two-level cancel token.** `CancelToken` as in the decisions.
  - **Accept:** port `core/StoppableThreadTest` (stop-all implies router stop; router stop
    does not imply stop-all; a full stop after a router stop gives the full stop; three
    tests).
  - *Done 2026-10-15:* `CancelToken` in `autoroute/events.rs` (one `AtomicU8`, no lock;
    `stop_router` is a compare-exchange from none). `tests/java_ports.rs` group
    `stoppable_thread`: `stop_request`, `stop_auto_router_request`,
    `stop_request_overrides_auto_router_request`, all pass. Unit tests
    `stoppable_has_the_full_stop_meaning` (the `Stoppable` impl is the full stop) and
    `token_is_shared_across_threads`.
