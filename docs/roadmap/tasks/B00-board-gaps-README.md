# B00 — Board model gaps

Opened 2026-10-15. The Rust board is complete for loading, DRC and scoring: all
159 boards Java loads match on every compared statistic, and the DRC reports
match. The router, the shover, pull-tight and the editor need more:
- board copies, hashes and restores with Java's semantics;
- a search tree with Java's exact shape (decision D5), with rooms in it, a work
  meter and room completion;
- the read-only checks the tightener and the maze search call before every
  change;
- item moves, pin exits and pin-connection fixes;
- scores driven by the router settings;
- a few engine services for the GUI and for algorithm stages.

The B series ports these. Its narrative is in [autorouter.md](../autorouter.md)
("What the board must provide first"); the decisions it implements are D1–D9 in
[port-completion.md](../port-completion.md). The research behind it is the
board-gap report, plus the board parts of the autorouter, optimizer, GUI and
KiCadRoutingTools reports (2026-10-15).

The Java reference is `cda540f`. J01 re-pins to `30baad7e2` ("Make routing
independent of machine load"), which has landed. B02.7 and B03.7 read their
counting points at `30baad7e2` already. Every other B probe is regenerated at
the new pin by J01.5.

## Task index

| Task | Outcome | Size / risk | Prerequisite |
|---|---|---|---|
| [B01](archive/B01-board-identity.md) ✅ | Board-probe support and an `ids` probe; ids burned where Java burns them; `BoardSnapshot`/`deep_copy` with Java's restore semantics; copy-on-write snapshots; `state_hash`; an exact incremental ledger; the change journal; shove-failing state | L / medium | F02.1 |
| [B02](B02-search-tree-parity.md) | Search-tree parity (D5): `TreeObject` keys (rooms first, ids descending), Java's leaf reuse for combine, change and cutout, tree fingerprints equal to Java's on every loadable fixture, `WorkMeter`/`WorkLimit` (64,000 units per nominal ms), tie-pin reduction, side table; removes the DIVERGENCES entry "Combining traces removes and reinserts" | L / high | B01.1, B01.3, F02.1; Java `30baad7e2` for B02.7 |
| [B03](B03-room-completion.md) | `complete_shape` plain, 45° and 90°, with restrain helpers and work counting; the `complete-shape` probe equal | L / high | B02.1, B02.2, B01.1; B02.7 for B03.7 |
| [B04](B04-checks-and-picks.md) | `ItemFilter` and filtered picks, `check_shape`, `check_trace_shape`, `check_polyline_trace` (burns an id), `check_trace_segment`, nearest-item pick, move and net-change checks, item helpers; the `board-queries` probe equal | M / medium | B01.1, B01.2; B02.6 for B04.7 |
| [B05](B05-item-operations.md) | Item moves and transforms, copies, pin exit restrictions and widths, the three pin-connection fixes, `connect_to_trace`, pin swap, placement tests | L / medium | B01.2, B02.4, B04.4 |
| [B06](archive/B06-scores.md) ✅ | Router and optimizer scores dispatched on `RouterSettings`, the legacy score with Java's float/double split; optional benchmark classes | S–M / low | C02; B01.6 |
| [B07](B07-drc-loose-ends.md) | The unported Java DRC tests, exact Issue575 counts, copper islands compared with Java, plane owner-rule tests | M / low | B01.1 for B07.3 |
| [B08](B08-gui-engine-api.md) | `ItemGeometry`, `ItemInfo`, route stubs, the rules-edit facade, interactive leftovers, rule-dialog queries | M / low | B02.5 for B08.4; none for the rest |
| [B09](B09-stage-services.md) | Services for algorithm stages: insert-check-revert transaction, per-net obstacle view, fill model and connectivity, conduction-area obstacle hook | M / medium | B01.3, B01.5, B02.1, B04.3; runs when X or Q needs it |

The series map's ids are kept. The old board-gap proposal's "shove support"
(entry sides, `ShapeTraceEntries`, cutouts) is O03. It uses B02.6's
`reuse_entries_after_cutout`. B01 also took the board-probe runner, which the
series map listed under B04, because every B checklist needs it.

## Order and critical path

```
F02.1 ─> B01.1 ─> B01.2 ─> B01.3 ─> B02.1 ─> B02.2 ─┬─> B02.3 ─> B02.4 ─> B02.5 ─> B02.6 ─> B02.7 ─> A04 (meter)
                    │                                │                  │
                    │                                └─> B03.1–B03.6 ───┼──────────────────> A02 (rooms)
                    │                                                   │          B03.7 (after B02.7)
                    └─> B04 ─────────────────────────────────> O01, O03, A04
                          └─ B04.4 + B02.4 ─> B05 ─────────────> O01, A06
B01.4–B01.8: any order after B01.3.        B07: any time (B07.3 after B01.1).
B06: after C02 and B01.6.                  B08: B08.4 after B02.5; the rest any time.
B09: when the first X or Q checklist needs it.
```

- **Critical path to the router:** F02.1 → B01.1–B01.3 → B02.1–B02.2 → B03.1–B03.6
  → A02, and B02.3–B02.7 → A04. B04 and B05 sit on the O series' path
  (O00: B01, B02, B04, B05 → O01; B01, B02, B04 → O03).
- B02.3–B02.5 are the D5 proof. B02 isn't done until its tree probe's
  mismatch list is empty.
- Once B02.2 passes on `load` and `autoroute`, B03 can start beside B02.3–B02.6.
  B03's probes use freshly loaded boards, whose trees already match.

## Parallel lanes and file ownership

Lanes that can run at once:
- **core**: B01, then B02;
- **rooms**: B03, after B02.2;
- **checks**: B04 after B01.2, then B05;
- **DRC tests**: B07;
- **scores**: B06, after C02;
- **GUI API**: B08.

Paths are under `crates/freerouting/src/` unless they start with `crates/`,
`tools/`, `fixtures/` or `docs/`.

| Shared file | Owner | Others |
|---|---|---|
| `board/basic_board.rs` | B01, then B02 (in that order) | B04, B05, B08, B09 put their code in new files with `impl BasicBoard` blocks. Named edits only: B08.5 makes `insert_item` public; B09.4 changes the conduction-area branches of `is_trace_obstacle` and `is_obstacle` |
| `board/trace_ops.rs` | B02 (`combine_trace_at`, `change_trace`) | B01.6 and B01.7 add ledger marks and journal records only |
| `board/search/mod.rs`, `datastructures/shape_tree.rs` | B02 | B01.3 (the `SearchTreeManager` constructor); B03 appends `mod` lines |
| `board/search/complete_shape.rs` | B03 | P04 later |
| `board/mod.rs`, `drc/mod.rs`, `rules/mod.rs`, `scoring/mod.rs` | append only | every checklist |
| `crates/freerouting/tests/board_probe/mod.rs` | B01.1 | others append helpers only |
| `tools/java-reference/probes/board/{fixtures.tsv, generate.sh, README.md}` | B01.1 | others append one README line per probe; each probe is its own `*Probe.java` |
| `docs/DIVERGENCES.md` | append | B01, B02 and B06 remove the lines they close |

## Rules for this series

These add to the shared rules in [README.md](README.md).

- **Board probes.** Each probe is one Java file in
  `tools/java-reference/probes/board/`, run with F02.1's `probe.sh`.
  `probes/board/generate.sh <checkout> <kind|all>` writes
  `fixtures/reference/probe/<kind>/<board>.jsonl`. The format is A00's:
  - one JSON object per line, keys in a fixed order, integers in decimal, every
    `double` as `"0x"` plus 16 hex digits of its raw bits;
  - the first line is a header with the probe kind, board, Java commit, JDK and
    format version;
  - a Java exception is a record (`{"exception": …, "at": …}`);
  - a board whose output passes 1 MiB is committed as `<board>.digest`, one
    FNV-1a 64 line per section.
  Every B probe carries a verbatim copy of the `Boards` loader from
  `IdsProbe.java` (B01.1), which loads as `cli/DrcRunner` does and waits for the
  post-load processing. A test fails if any copy differs. Rust writes the same
  lines through `tests/board_probe/mod.rs`, so a comparison is line equality.
  Hashes are hand-written FNV-1a on both sides (no new dependency).
- **Re-pin.** When J01 moves the pin, rerun `generate.sh <checkout> all` before
  any other B work, and fix whatever changes. J01.5 does this.
- **Tree mutations go through B02's operations, in Java's order.** No
  remove-and-reinsert shortcut. A new code path that changes the trees adds a
  stage to the `tree-ops` probe (B02.4–B02.5). After B02, every B checklist keeps
  `board_probe_tree` green.
- **Ids.** Burn an id wherever Java constructs a temporary item (B01.2's table).
  Every probe that runs board operations logs `maxGeneratedId`.
- **Copies.** Routing, history and optimizer code copies boards with
  `deep_copy()` or `BoardSnapshot` (B01.3), never `Clone`. A restore resets
  exactly the fields B01.3 lists.
- **Work.** The board's search trees own its `WorkMeter`, and a `WorkLimit` is
  passed explicitly to the code that checks it (B02.7). One nominal ms is 64,000
  units of nodes visited, as at `30baad7e2`.
- **"Unchanged" is explicit.** `change_trace` takes `LineReuse`. Never infer
  Java's reference identity from value equality (O00's rule).
- **Iteration.** Java's item list iterates highest id first and is weakly
  consistent: items removed ahead of the cursor are skipped, and new items, which
  have higher ids, are never seen. A loop that changes the board takes a snapshot
  of ids and re-checks presence before each one.
- **Determinism.** A checklist that changes the board (B01, B02, B05, B08.4,
  B09.1) runs its probe test under `tools/determinism-check.sh` with
  `BOARD_PROBE_OUT={OUT}` (B01.1) and records "identical" in its done note.

## Open questions for Dan

1. **Ledger staleness (board-gap Q1).** Java's `NetRoutingLedger` isn't told
   about undo and redo, fast-cutout pieces, in-place trace changes or net
   changes. The incomplete counts that the pass loop, the optimizer and the
   statistics read can therefore go stale.
   *Recommendation:* count exactly in Rust (B01.6). B01.6's `ledger` probe shows
   where Java's ledger differs from a fresh count. If it never differs on the
   route references, parity costs nothing. If it does, ask the Java session to
   invalidate the ledger on undo, redo and trace change and to note cutout
   pieces, then re-pin. Don't reproduce staleness in Rust.
2. **The state hash and `smallestClearance` (board-gap Q4).** Java's `getHash`
   includes a DRC cache field that `clearanceViolations()` writes as a side
   effect, so computing statistics changes Java's hash.
   *Recommendation:* exclude it and add a DIVERGENCES line (B01.5). The
   `state-hash` probe lists the only steps where that changes an equality
   decision.
3. **Restores drop clearance compensation (board-gap Q8).** Every Java restore
   and deep copy rebuilds the default tree uncompensated, even when the GUI turned
   compensation on. That affects GUI routing only.
   *Recommendation:* reproduce it during the parity phase (B01.3), list it under
   "Java bugs kept on purpose", and fix it after board parity as a measured
   divergence, or in Java first.

**Asked of the Java session (asked 2026-10-15):**

4. **Optimizer worker-board leaks.** Worker threads reuse one board copy per
   thread across candidates. Undo restores neither the id generator, the trace
   half-width bounds, normalize suppression, the ledger nor the tree shape, so
   results depend on scheduling. *Status at `30baad7e2`:* not fixed; the fork's
   ledger says to compare optimizer output on aggregates.
   *Recommendation:* Java evaluates each candidate on a fresh copy of the pass
   baseline. Rust does that by construction (O06).
5. **Identity-hash airline order.** `NetIncompletes.calculateNetItems` grouped
   items in identity-hash order, which reached `LastMileBlockerRipup`. *Answered
   by `30baad7e2`:* a `LinkedHashSet` keeps the input order. J01.2 checks that
   Rust's group start order matches the new Java order.
6. **The 2000 ms limit in `removeItemsAndPullTight`.** *Answered by
   `30baad7e2`:* it is now `PULL_TIGHT_LIMIT_NOMINAL_MS = 2000`, a work limit
   (`RoutingBoardOperations.java:18,118`). O01 ports it.

**Decided here; say if you disagree:**
- D5 settles the board-gap report's Q2, Q3 and Q5: port Java's leaf reuse,
  rebuild deep copies in item-list order, and burn ids for temporaries.
- Room geometry lives in `board::search::rooms` (Q9); A wraps it.
- `delete_all_traces_and_vias` keeps removing tree entries, which Java forgets
  (board report Q11). It is GUI-only; there is a DIVERGENCES line and a test (B08.7).
- The work meter is per board, not per thread. It counts the same units at every
  Java limit site, and it can't leak between rayon tasks (B02 "Decisions").
- No retained autoroute database (A00), so the change log (B02.8) is off the
  critical path.

## Hand-offs to other series

- **Routing operations on `RoutingBoard`** (board-gap section 6):
  - `optChangedArea` and `removeItemsAndPullTight` → O01;
  - `checkForcedTracePolyline` → O03;
  - `insertForcedTracePolyline`, `insertForcedTraceSegment`, `moveDrillItem` and
    `forcedVia` → O04;
  - `initAutoroute`, `finishAutoroute` and the autoroute session → A02–A06;
  - `fanout` → A12;
  - `MoveComponentTest` → I02.
  `connectToTrace` stayed here (B05.6).
- **KiCadRoutingTools board services** (KRT report §11.1) that aren't B09's:
  - the search failure record, the expansion cost hook, the width policy and
    the budget hook → A04 (the width override also A06);
  - the fixed-origin cost lattice → Q05;
  - the rules the DSN lacks (pair membership, stackup) → the K series'
    `.kicad_pcb` reader;
  - snapshots → B01.
- **J01.5** regenerates `fixtures/reference/probe/*` with
  `tools/java-reference/probes/board/generate.sh`. J01.2's text says "the A series
  designs" work counting; the series map and this README give it to B02.7.
- **A08** adds `RoutingBoard`'s failure log to `BoardSnapshot` (B01.3's note).
- **P02.1** can compare a live tree with a rebuild only by `content_digest()`.
  The live shape depends on history, in Java as in Rust (B02 "Decisions").
- **O00** expects "`WorkMeter`/`WorkLimit`, passed explicitly": the limit is
  passed; the meter is reached through the board.
