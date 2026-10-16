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
