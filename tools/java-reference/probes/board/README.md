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
