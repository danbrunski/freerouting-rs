# Q06 — Failure attribution and one verdict per incomplete connection

Opened 2026-10-15. After a run, an incomplete connection should say why it is
incomplete, and the reasons must not blur together. KRT keeps them as separate
verdicts (`py_router/routing_diagnostics.py`, `route.py`'s summary keys
`blockers` and `boxed_in`) and names the routed nets in the way from the
failed search's blocked frontier (`py_router/blocking_analysis.py`,
`analyze_frontier_blocking`, `rank_blockers`, at `0486e799`). This checklist
does the same from the Freerouting maze search's failure record, and joins it
with Q03's static `infeasible` verdict.

It is report-only. Q05 reads its blocker attribution to charge history costs.

Prerequisites: board parity (rung 3); A04 (the maze search's failure record,
see "Decisions"); A11 (per-connection outcomes and work-limit cut-offs in the
pass loop); Q03; C08 (the result JSON); the G series for the incompletes
list.

## What exists and what this adds

| Exists | State |
|---|---|
| Java `AutorouteUnroutedReport` (cda540f) | lists unrouted connections; ported by A10 as text |
| Java `RoutingFailureLog` | per-item failure counts; ported by A |
| A04 failure record (requested from the A series) | on a failed search: the doors and rooms that could not be expanded, each with the blocking `ItemId`, sorted `(net, item id)`, capped |
| A11 `ConnectionOutcome` with the work-limit flag (Java logs each WorkLimit cut-off, J01) | whether a budget cut the search off |
| Q03 `FeasibilityReport` | the static verdict per pin |
| KRT `BlockingInfo` fields `blocked_count`, `unique_cells`, `near_target_cells`/`near_source_cells` (within 3 mm), and the coverage line "N/M frontier cells attributed to routed nets; K static/unrippable" | the design |

## Decisions made here

- **One verdict per incomplete connection, by precedence:**
  1. `infeasible`: Q03 says an end pin can't be reached at the class width;
  2. `work_limit`: the last attempt was cut off by a work limit;
  3. `blocked_by`: the last attempt exhausted its search and at least one
     blocking item belongs to a routed (non-fixed) net;
  4. `unrouted`: the search exhausted with only fixed copper, keepouts or
     the outline in the way (KRT's "boxed in").
  The geometric proof comes first: a connection Q03 proves infeasible is
  `infeasible` even if a work limit also fired.
- **Attribution counts expansions, not area.** Per foreign net: `blocked` (the
  number of failure-record entries whose blocking item is on that net),
  `unique` (entries where it is the only net blocking that door), and
  `near_start` / `near_dest` (entries within R = 3 mm of a start or
  destination pin). The last analysis of a connection in the run wins.
- **Ranking** (KRT `rank_blockers` "count"): sole blockers first, then
  `unique + near_unique + 0.5·shared`, ties by net number; at most 10 nets.
- **One structure for every reader.** The CLI result JSON (C08) and the GUI
  incompletes list (G series) read the same `Verdict` values; nothing else
  computes them. The schema is documented in `docs/development/verdicts.md`.
- **The diagnostics stage.** Q06 registers an A10 diagnostics implementation,
  id `verdicts`, selected by `router.special.diagnostics`. It reads the
  pipeline's outcomes after the last pass and never feeds routing, except
  that Q05 may read the attribution.

## File ownership

| Item | New files | May edit |
|---|---|---|
| Q06.1 | `crates/freerouting/src/quality/attribution.rs` | `quality/mod.rs` (append) |
| Q06.2 | `fixtures/quality/q06-sole-wall.dsn`, `fixtures/quality/q06-fixed-cage.dsn`, `crates/freerouting/tests/quality_attribution.rs` | — |
| Q06.3 | `crates/freerouting/src/quality/verdict.rs` | `quality/mod.rs`, `quality/register.rs` (append) |
| Q06.4 | `docs/development/verdicts.md` | `crates/freerouting/tests/quality_attribution.rs` |
| Q06.5 | — | — (cross-check only) |

## Checklist

- [ ] **Q06.1 — Blocker attribution from the failure record.**
  Rust design: `quality::attribution::attribute(board, record, connection) ->
  Attribution { nets: Vec<NetBlocking>, coverage: Coverage }`. `NetBlocking {
  net, blocked, unique, near_start, near_dest }`; `Coverage { entries,
  routed, fixed }` where an item is fixed if its `fixed_state` is not
  unfixed, or it is a pin, keepout or outline.
  - **Orders that decide results:** entries in the record's order
    `(net, item id)`; output sorted by rank key descending, then net number;
    the cap of 10 applied after sorting.
  - **Accept:** unit tests on hand-built records: a single blocking net gets
    `unique == blocked`; two nets sharing every door get `unique == 0`; the
    cap keeps the 10 best-ranked nets.

- [ ] **Q06.2 — Fixtures for attribution.**
  `q06-sole-wall.dsn`: one connection whose only route is closed by a routed
  trace of one other net. `q06-fixed-cage.dsn`: a pin walled in by fixed pads
  of other nets.
  - **Accept:**
    - the sole-wall net is named first, with `unique == blocked`;
    - the caged pin shows 0 % routed coverage (`routed == 0`);
    - the SES is byte-identical with the stage on and off.

- [ ] **Q06.3 — Verdicts.**
  Rust design: `quality::verdict::Verdict { Infeasible(PinFeasibility),
  WorkLimit { pass, work }, BlockedBy(Attribution), Unrouted(Coverage) }`
  per incomplete connection, in ascending order of the connection's first
  item id. Q03's check runs once per job on the input board for the pins of
  incomplete connections only.
  - **Accept:** on `q03-qfn-0p5.dsn` the QFN connection is `infeasible`; on
    `q06-sole-wall.dsn` it is `blocked_by`; on `q06-fixed-cage.dsn` it is
    `unrouted`; a connection run with a tiny work limit is `work_limit`.

- [ ] **Q06.4 — Schema and output.**
  Document the JSON in `docs/development/verdicts.md` (schema
  `freerouting-verdicts/1`, one object per incomplete connection with its
  from/to items, verdict and evidence). Emit it under `verdicts` in the C08
  result JSON.
  - **Accept:** a golden-JSON test on the three fixtures; the doc lists every
    field; the CLI and the GUI incompletes list agree on 3 boards (the GUI
    part is `[~]` until the G series shows the list).

- [ ] **Q06.5 — Sanity on real boards.**
  - **Accept:** on `canary-pwr` and two `benchmark` boards, the done note
    gives the verdict counts per board, and for each `blocked_by` the top
    net. Every `infeasible` connection's pins are also in Q03's report for the
    board. This is the evidence Q05's decision rule reads.
