# B01 — Board identity: ids, snapshots, hash, ledger, journal

Opened 2026-10-15. The batch router copies, compares and restores whole
boards, and Java does all of it through Java serialization:
- `BoardHistory` keeps the best boards and restores one
  (`autoroute/BoardHistory.java:108-216`);
- strict DRC rolls a connection back (`AutorouteConnectionRouter.java:94,290`);
- every optimizer candidate runs on a deep copy (`BatchOptimizer.java:321,424,558,986`);
- `BatchFanout` stops when the board hash did not change over a pass
  (`BatchFanout.java:117,160`).

A Java restore rebuilds the search tree by inserting the items in item-list
order and resets every transient field (`BasicBoard.readObject`,
`board/facade/BasicBoard.java:1508-1520`). Item ids order every
`TreeSet<Item>`, and Java takes an id for every item it constructs, including
temporaries. Decision D5 in [port-completion.md](../../port-completion.md) makes
both part of the parity target: ids burned where Java burns them, and deep
copies rebuilt in item-list order. This checklist gives the Rust board those
semantics. It also adds the probe support every B checklist uses, the routing
ledger that Java's pass control and statistics read, and the change journal the
GUI needs (G05, G07).

**Prerequisites:** F02.1 (`probe.sh`) for B01.1 and every probe-based Accept.
Nothing else. Series rules: [B00](../B00-board-gaps-README.md).

Suggested split for two dispatches: B01.1–B01.4, then B01.5–B01.8.

## What exists and what this adds

Rust paths are under `crates/freerouting/src/` at `70389b7`. Java paths are
under `src/main/java/app/freerouting/` at `cda540f`.

| Rust today | Java at the pin |
|---|---|
| `BasicBoard` and `RoutingBoard` derive `Clone` (`board/basic_board.rs:39`, `board/routing_board.rs:19`). A clone keeps `normalize_suppressed_nets`, `changed_area`, `revision` and every search tree as it is | `BasicBoard.serialize/deserialize/clone` (`board/facade/BasicBoard.java:148-182`, `BoardSnapshotManager.java:25-60`); `readObject` (`BasicBoard.java:1508-1520`); `RoutingBoardUndoFacade.deepCopy` (`:41-73`) adds `clearAllItemTemporaryAutorouteData` and `finishAutoroute` |
| Ids: `BasicBoard::new_item_id` (`basic_board.rs:177`), `ItemIdGenerator` (`datastructures/id_generator.rs`), called right before an insert | `Item` constructor takes `idGenerator.newId()` whenever `id <= 0` (`board/model/items/Item.java:70-92`) |
| `insert_trace_without_cleaning` rejects a zero-length unfixed trace **before** taking an id (`basic_board.rs:248-287`) | builds the `PolylineTrace` (id taken) **then** rejects it (`BasicBoard.java:198-222`) |
| No board hash | `getHash`: MD5 of `serialize(true)` = `getTraces()`, `getVias()`, `itemList` with its whole undo structure (`BoardSnapshotManager.java:62-84`) |
| No ledger. `DesignRulesChecker` recounts the whole board; `tests/drc.rs:565` checks the full count after insert and remove | `drc/NetRoutingLedger.java` (186 lines), fed by `BoardItemRepository.insertItem/removeItem` (`:162-167,205-210`); read by `BatchAutorouter.java:586,591`, `BatchOptimizer.java:1148`, `BoardStatistics.java:297-299` |
| Undo, redo, snapshots: `basic_board.rs:1974-2035` on `datastructures/undoable_objects.rs` | `UndoableObjects.java`, `RoutingBoardUndoFacade.applyUndoRedoSideEffects` |
| No journal, no graphics update box | observers and `getGraphicsUpdateBox/joinGraphicsUpdateBox` (`BasicBoard.java:1255-1295`) |
| No shove-failing state | `RoutingBoard.java:1407-1426`; `clearTransientAutorouteState` `:911-917` |
| Harness: `tools/java-reference/run.sh`, `ReferenceDump.java` (`load`, `route`); a test-local JSON parser in `tests/reference_load.rs`; FNV-1a 64 in `tests/reference_drc.rs:69` | — |

What this adds: board-probe support and an `ids` probe; ids taken where Java
takes them; `BoardSnapshot` with Java's restore semantics; copy-on-write
snapshots; `state_hash`; an exact incremental ledger; the change journal; the
transient routing state.

## Decisions made here

- **A snapshot holds what Java serializes, and no search trees.** `BoardSnapshot`
  is the Java byte array's counterpart. `restore()` builds a board the way
  `readObject` does. `RoutingBoard::deep_copy()` is `snapshot().restore()`.
  Snapshots without trees are cheaper, and every Java restore rebuilds the tree
  anyway, so nothing is lost.
- **A restore rebuilds the default tree in item-list order** (highest id first,
  all shapes of an item in index order), with a fresh `SearchTreeManager` holding
  only the default tree, uncompensated. That is Java's `readObject`
  (`BasicBoard.java:1511-1519`, `SearchTreeManager.java:30-36`). Java drops a
  GUI-set clearance compensation on every restore. The port reproduces that
  during the parity phase (open question Q8 in B00 asks the owner about fixing it later).
- **`Clone` stays for tests and the viewer**, documented as "not a Java copy".
  Routing, optimizer and history code uses `deep_copy()` or `BoardSnapshot`
  (A00's board-history rule says the same).
- **`state_hash` is computed on demand.** Java serializes the whole board for
  every `getHash` call. Callers use it once per pass or per history entry, so a
  full scan (FNV-1a 128, no new dependency) costs little. Incremental upkeep
  would add a drift risk for no measurable gain.
- **The ledger counts exactly** (open question Q1 in B00). It marks nets dirty on
  every change that can change a count, including the ones Java's ledger misses.
  Its probe records where Java's ledger is stale, as evidence for Q1.
- **The journal is off by default and costs one branch when off.** It records at
  the same choke points as the board's own bookkeeping, so it can't miss a change.
- **Probe format and layout are the A series' format** (A00, "Probes"), so one
  set of habits covers both series. Details are in B00 "Board probes".

## File ownership

| Item | New files | May edit |
|---|---|---|
| B01.1 | `tools/java-reference/probes/board/{IdsProbe.java, fixtures.tsv, generate.sh, README.md}`, `crates/freerouting/tests/board_probe/mod.rs`, `crates/freerouting/tests/board_probe_ids.rs`, `fixtures/reference/probe/ids/*` | `tools/java-reference/README.md` (one line per probe) |
| B01.2 | — | `board/basic_board.rs` (`insert_trace_without_cleaning`, `burn_item_id`), `fixtures/reference/probe/ids/*` (regenerate only if the fix shows Java data was wrong, which it shouldn't) |
| B01.3 | `board/snapshot.rs`, `tests/board_snapshot.rs` | `board/mod.rs` (append), `board/basic_board.rs`, `board/routing_board.rs`, `board/search/mod.rs` (`SearchTreeManager` constructor only) |
| B01.4 | `crates/freerouting/examples/snapshot_bench.rs` | `board/snapshot.rs`, `datastructures/undoable_objects.rs`, `board/basic_board.rs` (field types) |
| B01.5 | `board/state_hash.rs`, `tools/java-reference/probes/board/StateHashProbe.java`, `tests/board_probe_state_hash.rs`, `fixtures/reference/probe/state-hash/*` | `board/mod.rs` (append) |
| B01.6 | `drc/ledger.rs`, `tools/java-reference/probes/board/LedgerProbe.java`, `fixtures/reference/probe/ledger/*` | `drc/mod.rs` (append), `board/basic_board.rs`, `board/trace_ops.rs` (dirty marks only), `scoring/board_statistics.rs` (connection counts), `tests/drc.rs` (append) |
| B01.7 | `board/journal.rs`, `tests/board_journal.rs` | `board/mod.rs` (append), `board/basic_board.rs`, `board/trace_ops.rs` (record calls only) |
| B01.8 | — | `board/routing_board.rs`, `tests/board_snapshot.rs` |

Every item may append to `docs/DIVERGENCES.md`. The board-model bullet that says
"the routing ledger, Java serialization, `getHash` and the graphics update box
are not ported" is rewritten by the item that ports each piece.

## Checklist

- [x] **B01.1 — Board-probe support and the `ids` probe.**
  Every later B probe loads boards the same way on both sides, so this comes
  first. The `ids` probe also proves the precondition of all of them: the Java
  and Rust loaders produce the same item ids.
  - Java loader (a nested class `Boards`, between `// BEGIN Boards` and
    `// END Boards`, copied verbatim into every B probe):
    - load as `cli/DrcRunner.java` does: a `RoutingJob`, `setInput`, then
      `BoardLoader.loadBoardIfNeeded(job)` (find the session setup in
      `DrcRunner.java` at the pin);
    - wait for `board.awaitPostLoad()` (`BasicBoard.java:1601`), so the
      background DRC can't race the session import (the Issue289 race in
      `docs/DIVERGENCES.md`);
    - import a session with `SesReader.read(stream, board)`
      (`io/specctra/SesReader.java`).
  - Rust loader: `freerouting::drc::load_design` (`drc/runner.rs:53`), which ports
    `HeadlessBoardManager.loadFromSpecctraDsn` plus
    `applyImmediatePostLoadProcessing`, then `freerouting::io::read_session`.
  - `fixtures.tsv`: one row per fixture whose `fixtures/reference/load/*.json`
    result is `success` or `outline_missing`. Columns: DSN path, session path or
    `-`. The session is `fixtures/reference/route/<board>.ses`, else a `.ses`
    beside the DSN in `fixtures/`, else `-`. A comment line explains the choice.
  - `generate.sh <checkout> <kind|all>` runs `probe.sh` for each row and kind and
    writes `fixtures/reference/probe/<kind>/<board>.jsonl` (format: B00 "Board
    probes").
  - `tests/board_probe/mod.rs`: fixture list reader, loader, a JSONL writer that
    formats exactly like the Java side (fixed key order, decimal integers, doubles
    as `"0x"` + 16 hex digits of the raw bits), `fnv1a64`, and
    `compare(kind, board, rust_lines)`. On a difference, `compare` prints the
    first differing line, the 5 lines before it and the `probe.sh` command for
    that board, and writes the Rust output under `target/probe/<kind>/`. When
    `BOARD_PROBE_OUT` is set, it always writes the Rust lines to
    `$BOARD_PROBE_OUT/<kind>/<board>.jsonl`, so `tools/determinism-check.sh`
    (F02.4) can compare two runs (B00 rule "Determinism").
  - The `ids` probe writes, per fixture, one line per stage (`load`, then `ses`
    when the row has a session): `maxGeneratedId`, the item count, and FNV-1a 64
    over the lines `"<id> <type>"` in item-list order (highest id first), where
    `<type>` is the Java simple class name (`Pin`, `Via`, `PolylineTrace`,
    `ObstacleArea`, `ViaObstacleArea`, `ComponentObstacleArea`,
    `ConductionArea`, `ComponentOutline`, `BoardOutline`).
  - **Accept:**
    - `generate.sh <pin> ids` writes one file per row; a second run is
      byte-identical (`cmp`).
    - `cargo test --test board_probe_ids` passes. Its `EXPECTED_MISMATCHES` holds
      only `Issue676-ch32v-tx118s` stage `ses` (Java's `SesReader` fails on all
      48 wires, `docs/DIVERGENCES.md`) and any fixture with a zero-length wire,
      each with its reason. B01.2 removes the second kind.
    - A test `probe_loader_copies_identical` reads every
      `tools/java-reference/probes/board/*Probe.java` and fails if any `Boards`
      block differs from `IdsProbe.java`'s.
  - *Done 2026-10-15:* `tools/java-reference/probes/board/{IdsProbe.java, fixtures.tsv,
    generate.sh, README.md}`, `tests/board_probe/mod.rs`, `tests/board_probe_ids.rs`.
    `fixtures.tsv` has 164 rows (159 with `fixtures/reference/route/**.ses`, 5 canary-pwr
    rows without a session); boards are the DSN path below `fixtures/` without `.dsn`, so
    nested fixtures keep their directories. `generate.sh <cda540f worktree> ids` wrote 164
    files; a second full run was byte-identical (`diff -r` exit 0). `cargo test --test
    board_probe_ids`: 4 passed (`ids_match_java`, `probe_loader_copies_identical`, the two
    B01.2 tests); `EXPECTED_MISMATCHES` holds only Issue676 `ses` (Java: maxGeneratedId 166,
    118 items; Rust: 166 items) and a listed entry that stops occurring fails the test. No
    zero-length entry was needed (B01.2). `tools/determinism-check.sh --out <scratch>/det --
    env BOARD_PROBE_OUT={OUT} cargo test --release --test board_probe_ids --test
    board_probe_state_hash`: identical (167 files). A probe that exits 3 skips a board.

- [x] **B01.2 — Take item ids where Java does.**
  - Java: `Item` constructor (`Item.java:86-91`); `insertTraceWithoutCleaning`
    (`BasicBoard.java:198-222`) constructs the trace, taking an id, and only then
    returns `null` for a zero-length trace below `USER_FIXED`.
  - Rust:
    - In `insert_trace_without_cleaning`, call `new_item_id()` after the
      `corner_count() < 2` check and before the zero-length check.
    - Add `pub(crate) fn burn_item_id(&mut self) -> ItemId`: one counter step,
      returned for logging. Its doc comment holds the table below.
  - Order: one id per Java `Item` construction with `id <= 0`, in Java's order.
    The Java sites at the pin and the checklist that calls `burn_item_id` or
    takes the id:

    | Java site | Temporary? | Rust owner |
    |---|---|---|
    | `BasicBoard.insertTraceWithoutCleaning` (`:209`) | rejected zero-length trace | B01.2 |
    | `BasicBoard.checkPolylineTrace` (`:1169`) | yes, every call | B04.4 |
    | `ShapeTraceEntries.nextSubstituteTracePiece` (`:276`) | yes, also on check paths | O03 |
    | `ShapeTraceEntries.fastCutoutTrace` (`:116,131`) | no: start piece, then end piece | O03 |
    | `Item.copy(0)` in every item class (`PolylineTrace.java:63`, `Via.java:71`, `Pin.java:122`, area items) | id taken only for `copy(0)` | B05.2 |
    | `BasicBoard.makeConductive` (`:1312`) | no; id before the removal | already matches (`basic_board.rs:806`) |

  - **Accept:**
    - `zero_length_unfixed_trace_consumes_an_id`: `max_generated_id` grows by 1 and
      no item is inserted.
    - `zero_length_user_fixed_trace_is_inserted`.
    - `board_probe_ids` green with the zero-length entries gone from
      `EXPECTED_MISMATCHES`. The done note names the fixtures that contain
      zero-length wires (or says there are none).
    - `reference_load.rs` and `reference_drc.rs` green.
  - *Done 2026-10-15:* `insert_trace_without_cleaning` takes the id after the corner check
    and before the zero-length check; `burn_item_id` carries the site table.
    `zero_length_unfixed_trace_consumes_an_id` and `zero_length_user_fixed_trace_is_inserted`
    pass (closed triangle polylines: two equal points alone give fewer than 2 corners, in
    Java too). Zero-length wires: the DSNs of Issue015, Issue022, Issue029, Issue054, Issue190
    (9), Issue269-z10_module, Issue367-UltraFlactyl (14), Issue575-drc (43), Issue754,
    Issue756-tomu-fpga{,7,8,9,11} and canary-pwr-2026-10-15-1747-unpoured have wires whose
    two points are equal; both loaders drop those before constructing a trace (fewer than 2
    corners), so no fixture reaches the zero-length branch: `board_probe_ids` was green
    without B01.2 (checked in a HEAD worktree) and stays green with it. `reference_load.rs`
    and `reference_drc.rs` green (release, 5 tests, on a copy of the tree without the
    untracked foreign `fixtures/quality/`).

- [x] **B01.3 — Snapshots and deep copies with Java's restore semantics.**
  - Java: `BasicBoard.readObject` (`:1508-1520`), the `transient` fields of
    `BasicBoard` (`:98-112`) and `RoutingBoard` (`:67-76`),
    `RoutingBoardUndoFacade.deepCopy` (`:41-73`).
  - Rust: `board/snapshot.rs`.
    - `RoutingBoard::snapshot(&self) -> BoardSnapshot`. It holds the item list
      with its whole undo structure, the components with their undo list, rules,
      library, layer structure, `communication` (with the id generator), bounding
      box, `max/min_trace_half_width` and `pre_existing_clearance_violations_count`.
      These are Java's non-transient fields. Java's `RoutingBoard.failureLog` is
      non-transient too; A08 adds that field and must add it here.
    - `BoardSnapshot::restore(&self) -> RoutingBoard`. It resets
      `normalize_suppressed_nets` (empty), `revision` (0), the ledger (unbuilt,
      B01.6), `changed_area` (none), the shove-failing state (B01.8), the
      edge-pin-net cache, the journal (off) and the work meter (B02.7, zero).
      `log` starts empty. It builds the search trees as described next.
    - The trees: a new `SearchTreeManager` with only the default tree (`Plain`,
      class 0, compensation off). Insert every item of `items()`, highest id first,
      with all its shapes in index order. Never clone the source tree.
    - `RoutingBoard::deep_copy(&self) -> RoutingBoard` = `snapshot().restore()`
      followed by `clear_transient_autoroute_state()` (B01.8). Java's copy also
      drops item autoroute info and the engine; in Rust both live in the A series'
      session, outside the board (doc note).
  - Orders that decide results: the rebuild order (item-list order, shape
    index order). It decides the tree shape and therefore the work counts (D5).
  - **Accept:** `tests/board_snapshot.rs`:
    - `restore_resets_transient_state`: one assertion per reset field.
    - `deep_copy_resets_normalize_suppression`: a net suppressed on the original
      is normalized again on the copy.
    - `deep_copy_keeps_ids_generator_and_undo`: after `generate_snapshot`, an
      insert and `deep_copy`, an `undo` on the copy gives the same item ids as on
      the original, and the next new id is equal on both.
    - `deep_copy_drops_compensated_trees`, and
      `deep_copy_drops_clearance_compensation` (Java behaviour, Q8).
    - `deep_copy_tree_equals_ordered_rebuild` on Issue026-J2_reference with its
      SES: the copy's default tree has the same leaf sequence (`to_array`),
      `distance_to_root` per leaf and leaf bounds as a tree built by inserting
      `items()` in order. B02.2 later adds the Java comparison (`deep-copy` stage).
    - The gate is green.
  - *Done 2026-10-15:* `board/snapshot.rs` (`BoardSnapshot`, `RoutingBoard::snapshot`,
    `restore`, `deep_copy`), `BasicBoard::from_snapshot` (fresh transient state, new
    `SearchTreeManager` holding only the default tree, items inserted highest id first).
    `tests/board_snapshot.rs`: `restore_resets_transient_state` (normalize suppression,
    revision, changed area, shove-failing obstacle and layer, edge-pin-net cache, ledger
    unbuilt, journal off, log; the work meter is B02.7's), `deep_copy_resets_normalize_suppression`,
    `deep_copy_keeps_ids_generator_and_undo`, `deep_copy_drops_compensated_trees`,
    `deep_copy_drops_clearance_compensation`, `deep_copy_tree_equals_ordered_rebuild` (J2 +
    SES: leaf order, depth and bounds equal), `deep_copy_items_equal_original`, and
    `failure_log_rolls_back_with_snapshot` (the snapshot carries A08's `failure_log`, as
    Java serializes it; closes A08.2's remainder). Gate: see the report; the only failures
    are foreign.

- [~] **B01.4 — Make snapshots and copies cheap.**
  Measure first (`docs/roadmap/performance.md`): release build, quiet machine.
  - Rust:
    - `examples/snapshot_bench.rs` times `snapshot()`, `restore()`, `deep_copy()`
      and today's `Clone`, and estimates heap bytes per snapshot, on
      Issue190-processor.Z80, Issue054-tairakb and
      Issue690-kit-dev-coldfire-xilinx_5213 (the largest by item count in
      `fixtures/reference/load`).
    - Put the parts that don't change while routing behind `Arc`: library, layer
      structure, rules (with `Arc::make_mut` where they change:
      `change_plane_as_obstacle`, rule edits), components.
    - Store item payloads as `Arc<Item>` in `UndoableObjects` nodes, with
      `Arc::make_mut` in `get_mut` and `save_for_undo`. A snapshot then copies the
      key map and pointers, not geometry. A persistent map would be a `NEW DEP`;
      don't add one without the orchestrator.
    - Optional, measured: a board keeps a `tree_is_canonical` flag, set by
      `restore()` and cleared by every tree mutation. A snapshot of a canonical
      board carries an `Arc` of its default tree, and `restore()` clones it instead
      of rebuilding. B02's fingerprints prove the two equal.
  - **Accept:**
    - The done note has the before/after table: time and bytes per operation and
      fixture.
    - `snapshot()` on Issue190-processor.Z80 is at least 10× cheaper than today's
      `Clone`, or the note says why not.
    - B01.3's tests, `reference_load.rs`, `reference_drc.rs` and
      `board_probe_ids` stay green.
  - *Partial 2026-10-15:* `examples/snapshot_bench.rs` (median of 7, counting allocator for
    bytes; release; the machine was not quiet, load average 8-14). Item payloads are
    `Arc<V>` in `UndoableObjects` nodes (`Arc::make_mut` in `get_mut`, undo and redo;
    `save_for_undo` shares the version). Before/after on today's code:

    | Board (items) | op | before | after |
    |---|---|---|---|
    | Issue190-processor.Z80 (10885) | snapshot | 813 µs, 3.28 MB | 189 µs, 1.28 MB |
    | | Clone | 932 µs, 6.39 MB | 483 µs, 4.39 MB |
    | | restore / deep_copy | 14.1 / 14.4 ms | 13.0 / 13.4 ms |
    | Issue054-tairakb (6791) | snapshot | 117 µs, 1.78 MB | 64 µs, 0.61 MB |
    | | Clone | 230 µs, 3.16 MB | 169 µs, 1.99 MB |
    | | restore / deep_copy | 8.5 / 8.9 ms | 8.3 / 8.5 ms |
    | Issue690-kit-dev-coldfire (5551) | snapshot | 192 µs, 1.71 MB | 102 µs, 0.63 MB |
    | | Clone | 453 µs, 3.80 MB | 297 µs, 2.72 MB |
    | | restore / deep_copy | 8.6 / 8.5 ms | 7.9 / 8.3 ms |

    `snapshot()` on Issue190 is 4.9x cheaper than the old `Clone` (932 / 189 µs), not 10x:
    what is left is the key map and node arena (about 0.8 MB) and deep copies of rules,
    library, layer structure and components (about 0.5 MB). Putting those behind `Arc`
    changes `BasicBoard`'s public field types, and the mutation sites include
    `settings/board.rs` and `job/overrides.rs`, held by the concurrent C03 agent (tried;
    reverted). Remainder: the `Arc` fields with `Arc::make_mut` at their mutation sites,
    and the optional canonical-tree flag (`restore()` is the tree rebuild, about 13 ms on
    Issue190), owned here (B01.4) after C03 lands. B01.3's tests, `reference_load.rs`,
    `reference_drc.rs` and `board_probe_ids` stay green.

- [x] **B01.5 — `state_hash` with Java's equality.**
  - Java: `BoardSnapshotManager.getHash` (`:62-84`) hashes `serialize(true)`:
    `getTraces()`, `getVias()` and `itemList`. No field of `UndoableObjects` or
    its nodes is transient (`UndoableObjects.java:21-32,328-333`), so the hash
    covers every version node, the deleted-objects stack, `stackLevel` and
    `redoPossible`. Items contribute every non-transient field, including
    `onTheBoard` and the DRC cache `smallestClearance` (`Item.java:48`), which
    `clearanceViolations()` writes as a side effect.
  - Rust: `board/state_hash.rs`, `RoutingBoard::state_hash(&self) -> u128`:
    FNV-1a 128 (hand-written) over a canonical byte encoding.
    - Live items in item-list order: id, kind, nets, clearance class, component
      id, fixed state, `on_the_board`, and geometry (trace: layer, half width,
      corners; via: padstack, center, attach, escape flags; pin: component and pin
      index; areas and outlines: their shapes and flags).
    - Then the undo structure: stack level, `redo_possible`, every version node
      reachable from the key map (key order, then the undo chain), each with its
      level and object encoding, and the deleted-objects stack, level by level, in
      list order.
    - Excluded: `smallestClearance` (no Rust counterpart; open question Q4). Add
      a DIVERGENCES line.
  - Equality is what must match, not bytes: two board states hash equal in Rust
    exactly when they hash equal in Java (except for `smallestClearance`).
  - **Accept:**
    - Unit tests: the hash changes on insert, remove, a trace change, a fixed-state
      change and `generate_snapshot`; it survives `deep_copy`; insert then undo
      differs from the state before the insert (redo is now possible, as in Java).
    - Probe `state-hash` on Issue026-J2_reference, Issue508-DAC2020_bm08 and
      Issue413-test. Steps: load (h0); `generateSnapshot` (h1); import the SES
      (h2); `generateSnapshot`, then remove the 5 highest-id traces (h3); undo
      (h4); redo (h5); `deepCopy`, hashing the copy (h6); `new
      BoardStatistics(board)` on the copy (h7). Java writes, per step, which
      earlier hashes it equals. The Rust test asserts the same equality matrix.
      Entries that differ only through `smallestClearance` (h7) are listed in the
      test with that reason.
    - `state_hash` on Issue190-processor.Z80 takes under 5 ms in release (done
      note).
  - *Done 2026-10-15:* `board/state_hash.rs`, `UndoableObjects::walk_state` (nodes with
    handles for back references). Undo and redo now clear `on_the_board` on the cancelled
    versions, as Java's `SearchTreeManager.remove` does. Tests in
    `tests/board_probe_state_hash.rs`: `state_hash_tracks_changes` (insert, fixed-state
    change, trace change, remove, `generate_snapshot`; survives `deep_copy`),
    `insert_then_undo_differs_from_before`, `state_hash_equality_matches_java`.
    `StateHashProbe.java` on Issue026-J2_reference, Issue508-DAC2020_bm08, Issue413-test:
    Java's matrix is the same on all three (h6 = h5; h7 = h5 = h6; all else distinct) and
    Rust's equals it. The session's wires are user fixed, so the probe unfixes the 5 traces
    before removing them (on both sides). No step differs through `smallestClearance`
    (`EXPECTED_MISMATCHES` empty). DIVERGENCES line added. Release on Issue190: 4.66 ms
    (`snapshot_bench`, median of 7; 9.7 ms before dropping Java's redundant
    `getTraces/getVias` prefix and digesting `Debug` text with FNV-1a 64).

- [x] **B01.6 — Exact incremental routing ledger.**
  - Java: `drc/NetRoutingLedger.java`: per-net item lists, a `TreeSet` of dirty
    nets recounted on read, `incompleteCount`, `incompleteNetNumbers` (ascending),
    `maximumConnections` (pins and conduction areas per net, minus one). It is fed
    only by `BoardItemRepository.insertItem/removeItem`, invalidated by
    `deleteAllTracksAndVias` and rebuilt after a restore. It misses undo and redo,
    `fastCutoutTrace`'s pieces, in-place trace changes and net changes
    (board-gap report 2.4 item 7).
  - Rust: `drc/ledger.rs`, a `RoutingLedger` owned by `BasicBoard` behind a
    `Mutex` (reads recount dirty nets; the board must stay `Sync`, see
    `drc/clearance.rs:405-410`).
    - Per-net `BTreeSet<ItemId>` (no identity staleness), dirty nets in a
      `BTreeSet<i32>`, the same per-net count `DesignRulesChecker` uses
      (`drc/incompletes.rs`).
    - Marks the item's nets dirty on: insert, remove, undo and redo (cancelled and
      restored items), `change_trace` and combine, `assign_net_no` (old and new
      net), `remove_from_net`, and a `pub(crate) fn note_changed(id)` that O03's
      fast cutout calls.
    - Invalidated by `delete_all_traces_and_vias` and `restore()`.
    - `BoardStatistics` reads `maximum_connections` and `incomplete_count` from it
      (`BoardStatistics.java:297-299`).
  - Probe `ledger` (Java only; evidence for Q1) on the B01.5 fixtures. After each
    step it logs `routingLedger().incompleteCount()` and a fresh full count
    (`DesignRulesChecker`'s, as `BoardStatistics` uses without the ledger; find
    the call at the pin). Steps: load (first read builds); SES import;
    `generateSnapshot`, remove 10 traces, undo; `assignNetNo` on 5 vias;
    `ShapeTraceEntries.cutoutTrace` on 20 seeded (trace, octagon) pairs.
  - **Accept:**
    - `drc/NetRoutingLedgerTest` ported (69 Java lines).
    - `ledger_matches_full_count`: on every `board_probe/fixtures.tsv` row, as
      loaded and with the session imported, the ledger equals
      `DesignRulesChecker`'s count. The same holds after a scripted undo, redo and
      net change on 3 fixtures.
    - The `ledger` probe output is committed. The done note lists every step where
      Java's ledger differs from Java's fresh count, with the counts.
    - `reference_load.rs` and `reference_drc.rs` green, so the statistics didn't
      change.
    - An insert plus a read costs under 1 ms on Issue190-processor.Z80 (done note).
  - *Done 2026-10-15:* `drc/ledger.rs` (`RoutingLedger` behind a `Mutex`, per-net
    `BTreeSet<ItemId>`, dirty nets, per-net incomplete and maximum counts), marked at insert,
    remove, undo and redo (cancelled and restored versions), `change_trace` and combine,
    `assign_net_no`, `remove_from_net`, fixed-state and conduction changes and
    `note_changed`; invalidated by `delete_all_traces_and_vias` and `restore()`.
    `BoardStatistics` reads `maximum_connections` and `incomplete_count` from it.
    `tests/drc_ledger.rs` (not `tests/drc.rs`, which the orchestrator kept for B07; the Java
    port is not in `java_ports.rs`, which another session was editing):
    `incomplete_count_matches_full_scan_across_insert_and_remove` (port of
    `NetRoutingLedgerTest`), `ledger_matches_full_count` (all 164 rows, loaded and with the
    session), `ledger_matches_full_count_after_undo_redo_and_net_change` (3 fixtures),
    `ledger_restarts_after_delete_all_and_restore`, all green. `ledger` probe committed
    (`fixtures/reference/probe/ledger/`). Java's ledger differs from Java's fresh count at:
    J2 `undo` 3 vs 0, `cutout-0..19` from 3/3 to 1/4 (13 stale steps); DAC2020_bm08 `undo`
    4 vs 0, `assign-net-5-vias` 4 vs 1 and the cutouts (18 stale steps); Issue413 `undo` 3
    vs 0, `assign-net-5-vias` 3 vs 0 and the cutouts (16 stale steps). `reference_load.rs`
    and `reference_drc.rs` green (release). Insert + read on Issue190: 0.46 ms release.

- [x] **B01.7 — Board change journal.**
  This replaces Java's observers and graphics update box (`BasicBoard.java:1255-1295`).
  G05.5, G07.4 and G09 patch their scene from it; O01 joins its dirty region
  where Java calls `joinGraphicsUpdateBox`.
  - Rust: `board/journal.rs`.
    - `BasicBoard::set_journaling(bool)` and
      `take_changes(&mut self) -> BoardChanges { inserted, removed, changed:
      Vec<ItemId>, changed_nets: BTreeSet<i32>, dirty: Option<IntBox>, overflow:
      bool }`.
    - Recorded at the board's choke points: `insert_item`; `remove_item` and
      `delete_all_traces_and_vias`; every `save_for_undo` call site (`changed`);
      undo and redo (cancelled ids are removed, restored ids inserted);
      `assign_net_no`, `remove_from_net`, `change_clearance_class`,
      `set_fixed_state`. `dirty` is the union of the bounding boxes of affected
      items, before and after the change.
    - `take_changes` reduces the log to the final state per id (inserted then
      removed means neither), each list highest id first.
    - Past a cap (default 100 000 records) the log is dropped and `overflow` set:
      the consumer rebuilds.
  - **Accept:** `tests/board_journal.rs`:
    - one test each for insert, remove, change, undo, redo and a net change,
      checking ids, nets and the dirty box;
    - `journal_overflow_signals_rebuild`;
    - `journal_has_no_effect`: a scripted sequence gives the same `state_hash`
      (B01.5) with journaling on and off;
    - with journaling off, `reference_load.rs` time is unchanged within noise
      (done note: before and after).
  - *Done 2026-10-15:* `board/journal.rs` (`set_journaling`, `set_journaling_with_cap`,
    `take_changes`, `BoardChanges`, cap 100 000), recorded at insert, remove, delete all,
    every `save_for_undo` site, undo and redo, net, clearance-class, fixed-state and
    conduction changes and the outline keepout switch. `tests/board_journal.rs`, 9 green:
    insert, remove, change (dirty covers before and after), undo, redo, net change (old and
    new net), insert-then-remove reduces to nothing, `journal_overflow_signals_rebuild`,
    `journal_has_no_effect` (J2 + SES script, equal `state_hash`). `reference_load` release,
    journaling off, alternating runs: HEAD 14.5 / 14.8 / 6.8 s, B01 10.4 / 9.4 / 7.6 s (load
    average 8-14, so noise dominates; no slowdown visible).

- [x] **B01.8 — Transient routing state, and the fields Java's undo keeps.**
  - Java: `RoutingBoard.get/setShoveFailingObstacle`,
    `get/setShoveFailingLayer`, `clearShoveFailingObstacle` (`:1407-1426`);
    `clearTransientAutorouteState` (`:911-917`). Undo doesn't restore the id
    generator, `max/min_trace_half_width` (`BasicBoard.java:218-219`) or
    `normalizeSuppressedNetNos`. Rust already matches; nothing pins it.
  - Rust: on `RoutingBoard`, `shove_failing_obstacle: Option<ItemId>` and
    `shove_failing_layer: i32` (default -1) with getters, setters and
    `clear_shove_failing_obstacle()`. `clear_transient_autoroute_state()` drops
    the changed area and the shove-failing state. The callers that clear them
    (`moveDrillItem`, `forcedVia`, `insertForcedTracePolyline`) are O04's.
  - **Accept:** `undo_keeps_trace_half_width_bounds`, `undo_keeps_id_generator`,
    `undo_keeps_normalize_suppression`, `restore_resets_shove_failing_state`,
    `clear_transient_autoroute_state_resets_changed_area`.
  - *Done 2026-10-15:* `RoutingBoard` shove-failing obstacle and layer with getters,
    setters, `clear_shove_failing_obstacle`, `clear_transient_autoroute_state`. Tests in
    `tests/board_snapshot.rs`: `undo_keeps_trace_half_width_bounds`, `undo_keeps_id_generator`,
    `undo_keeps_normalize_suppression`, `restore_resets_shove_failing_state`,
    `clear_transient_autoroute_state_resets_changed_area`, all green.
