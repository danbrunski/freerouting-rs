# Board probes

Java probes for the B series (board model parity), run through `../../probe.sh`. Each probe
writes JSONL that `crates/freerouting/tests/board_probe/mod.rs` reproduces line for line on the
Rust side; the format and rules are in `docs/roadmap/tasks/B00-board-gaps-README.md`
("Board probes").

```sh
git -C ../freerouting worktree add --detach <scratch>/java-pin cda540f
TMPDIR=<scratch> tools/java-reference/probes/board/generate.sh <scratch>/java-pin ids   # or: all
```

- `fixtures.tsv`: one row per fixture whose `fixtures/reference/load/**/*.json` result is
  `success` or `outline_missing`: the DSN path and the session path (or `-`). The comment at its
  top explains the session choice.
- `generate.sh <checkout> <kind|all>` runs `probe.sh <checkout> <Kind>Probe.java <dsn> <ses|->
  <out>` for every row and writes `fixtures/reference/probe/<kind>/<board>.jsonl`, where
  `<board>` is the DSN path below `fixtures/` without `.dsn`. A probe that exits 3 skips the
  board. Output above 1 MiB becomes `<board>.digest`. Two runs are byte-identical.
- Every probe starts with the header line (kind, board, Java commit, JDK, format version) and
  carries a verbatim copy of the `Boards` class between `// BEGIN Boards` and `// END Boards`
  from `IdsProbe.java`: the loader (as `cli/DrcRunner` loads, then `awaitPostLoad`), the session
  import (`SesReader.read`) and the JSONL writer. `board_probe_ids.rs::probe_loader_copies_identical`
  fails if a copy differs. A Java exception is written as `{"exception":…,"at":…}`.

| Kind | Probe | Boards | Rust test |
| --- | --- | --- | --- |
| `ids` | `IdsProbe.java` (B01.1) | all rows | `tests/board_probe_ids.rs` |
| `state-hash` | `StateHashProbe.java` (B01.5) | J2, DAC2020_bm08, Issue413 | `tests/board_probe_state_hash.rs` |
| `ledger` | `LedgerProbe.java` (B01.6, Java only: where `NetRoutingLedger` goes stale) | J2, DAC2020_bm08, Issue413 | `tests/drc_ledger.rs` (exact count) |
| `islands` | `IslandsProbe.java` (B07.3: Java's islands and findings, the hole-pairing evidence, and the same check with the pairing fixed) | rows with conduction areas | `tests/reference_islands.rs` (tolerances, allowlist) |
| `board-queries` | `BoardQueriesProbe.java` (B04: checks, filtered picks, nearest pick, move and net-change checks, item helpers, `offsetShapes` of the three tree kinds, on samples from `Random(20261015)`) | J2 (`ses`, `compensated`), DAC2020_bm08 (`ses`), Issue413, Issue957, Issue145, Issue676 (`load`) and the first row with a multi-net pin (`ses`, its row's session) | `tests/board_probe_queries.rs` (one test per method) |
| `tree` | `TreeProbe.java` (B02.2: per tree and stage the node and leaf counts, maximum depth, walk hash and content digest; `--walk STAGE TREE` prints one tree's walk) | all rows (`load`, `autoroute`; with a session `ses`, `deep-copy`, `undo`, `redo`) | `tests/board_probe_tree.rs` |
| `tree-ops` | `TreeOpsProbe.java` (B02.4-B02.6: the trees after `insertVia`, `removeItems`, undo/redo, compensation on/off, `changePlaneAsObstacle`, `makeConductive`, 50 seeded `PolylineTrace.change` calls, `reduceTraceShapeAtTiePin`) | rows with a session; compensation, plane and conductive stages on J2, Issue145, Issue413; `change` on J2, Issue413; `tie-pin` on rows with a multi-net pin | `tests/board_probe_tree_ops.rs` |
| `tree-visits` | `TreeVisitsProbe.java` (B02.7, needs `WorkMeter`, pin `a773a70ec`: work units of 1 000 seeded overlap queries per stage) | J2, DAC2020_bm08, Issue145, Issue190 | `tests/board_probe_tree_visits.rs` |
| `scores` | `ScoresProbe.java` (B06.3: `getRouterScore`, `getOptimizerScore` and `getOptimizerRankingScore` of `new BoardStatistics(board)` for 19 settings variants: `null`, defaults, V1, each V2 weight and each legacy cost doubled, `lengthFloor = 0`; the statistics they read first) | all rows (`load`, `ses`) | `tests/board_probe_scores.rs` |
| `complete-shape` | `CompleteShapeProbe.java` (B03, needs `WorkMeter`, pin `a773a70ec`: `completeShape` of the 45, 90 and plain trees on 500 seeded queries from `Random(20261015)` per variant, each with its result rooms and work units; stage `with-rooms`: 100 result rooms inserted, 200 queries with `ignoreObject`/`ignoreShape`, the tree's fingerprint after insertion and removal) | 45 and `with-rooms` on J2, DAC2020_bm08, Issue413, Issue145, Issue676; 90 and plain on J2, DAC2020_bm08 (`load`) | `tests/board_probe_complete_shape.rs` |
| `route-stubs` | `RouteStubsProbe.java` (B08.3: a stand-in copy of `WindowRouteStubs.fillList` and `RouteStubInfo.compareTo`, which need a GUI; the list with the routed session imported, and again after every 7th trace was removed) | J2, DAC2020_bm05, ecc83 (`ses`) | `tests/board_probe_route_stubs.rs` |
| `item-info` | `ItemInfoProbe.java` (B08.2: `Item.printInfo` of a selection of items per Java class into a recording `ItemInfoPrinter`; one line per printed value, labels skipped) | J2, Issue145, Issue039 (`ses`) | `tests/board_probe_item_info.rs` |
| `item-ops` | `ItemOpsProbe.java` (B05.1, B05.6: `move`, 20 via `moveBy` with the default tree after each, then undo; `transform`, `turn90Degree`, `changePlacementSide` and `rotateApprox` of a trace, a via and a pin; `connect`, 50 seeded `connectToTrace`) | DAC2020_bm05 (`move`), J2 (`connect`, `transform`), with sessions | `tests/item_transform.rs` |
| `pin-exits` | `PinExitsProbe.java` (B05.3, B05.4: per pin and layer `getTraceExitRestrictions`, `getMinWidth`, `getMaxWidth`, `getTraceNeckdownHalfwidth`, `nearestTraceExitCorner` and `calcNearestExitRestrictionDirection` for 5 seeded points; `--find-back` finds the back-side board) | Issue955, Issue145, J2 and the first back-side row, Issue022 (`load`) | `tests/board_probe_pin_exits.rs` |
| `pin-connection` | `PinConnectionProbe.java` (B05.5: `checkConnectionToPin`, `correctConnectionToPin`, `swapConnectionToPin` at every trace end at a pin, two passes, `pinEdgeToTurnDist` = 2 × the smallest trace half width; the default tree after each correction) | J2, DAC2020_bm08, Issue145, with sessions | `tests/board_probe_pin_connection.rs` |
| `rules-edit` | `RulesEditProbe.java` (B08.4: the trees after the clearance matrix window's calls: `setValue` + `clearanceValueChanged`, `appendClass` + `setClearanceCompensationUsed(false)`, `changeClearanceClassIndex` of items, the prune sequence with `clearanceClassRemoved`) | J2, Issue145 (`ses`) | `tests/rules_edit.rs` (`java::`) |
| `entry-side` | `EntrySideProbe.java` (O03.1: the four `ShapeEntrySide` constructors and `ShapeAndEntrySide` for the four (orthogonal, inShoveCheck) combinations on 300 samples of the shove sampler, `ShoveSampler` block shared with the two probes below) | J2 (`ses`), Issue413 (`load`) | `tests/board_probe_shove.rs` |
| `trace-entries` | `TraceEntriesProbe.java` (O03.2, O03.3: `ShapeTraceEntries.storeItems` and its results, every substitute piece and `maxGeneratedId`, then on a fresh load 100 `cutoutTraces` with the default tree's walk hash after every 10) | J2 (`ses`), Issue413, Issue676, Issue957 (`load`) | `tests/board_probe_shove.rs` |
| `shove-check` | `ShoveCheckProbe.java` (O03.4, O03.6-O03.8: on the shove sampler the instance `TraceShover.check` with depths (20, 8, 5) and (0, 0, 0), the static OK length for both sides with via depth 8 and 0, `checkForcedTracePolyline` with both depth sets; the first via on a 20 x 20 grid and at the first 100 pins, `ForcedViaInserter.check` and `checkLayer` per layer in seven kinds of rooms; 50 deep checks under a `WorkLimit` that fires a quarter, half or three quarters through; then the sampler and via stages again in forced 90 degree mode; each query with the shove-failing obstacle and layer and `maxGeneratedId`) | Issue413, Issue676, Issue957 (`load`), J2, DAC2020_bm08 (`ses`) | `tests/reference_shove_check.rs`; `examples/shove_check_dump.rs` |
