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

Prior art in the Java fork (read at `main` through git,
`docs/development/tools/`): `Incompletes.java` (lists the incomplete
connections of a DSN + SES), `PowerFailures.java` (which unrouted
connections belong to the wide power classes and what sits in their way),
`GndAirlines.java` (airlines of the plane net) and `ViolDiff.java` (the
violations a session adds over the input board). Dan wrote these to read
`canary-pwr` runs by hand; the verdicts here replace them. Q06.4's report
format should let each of those four questions be answered from the result
JSON, so the tools are not needed against the Rust engine.

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

- [x] **Q06.1 — Blocker attribution from the failure record.**
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

  *Done 2026-10-17:* `quality/attribution.rs`: `attribute(board, record,
  ends) -> Attribution { nets, coverage }`, `NetBlocking`, `Coverage {
  entries, routed, fixed, dropped }`, the `RecordingSink` (a `SearchSink`
  that adds the popped step and its entry-line middle to each A04 `Blocked`)
  and `analyze_connection` (a ripup-off replay of the Freerouting maze search
  on a board copy). A "door" is the expansion step (popped element) that met
  the block; `unique` counts routed nets only (fixed copper is not a net one
  can rip). Record order `BlockEntry::key` = `(net, item id ascending, layer,
  reason, step, at)` (`ItemId`'s own `Ord` is Java's descending one); output
  by `rank_key` descending, then net; cap after the sort. `cargo test -p
  freerouting --lib quality::attribution`: 6 passed —
  `a_single_blocking_net_is_unique_everywhere` (unique == blocked == 3),
  `two_nets_sharing_every_door_have_no_unique_entries` (unique == 0, ties by
  net), `the_cap_keeps_the_ten_best_ranked_nets` (12 sole + 2 shared nets ->
  nets 12..3), `tier_one_score_counts_shared_entries_half`,
  `fixed_items_count_in_coverage_but_name_no_net`,
  `near_uses_the_exact_distance_to_the_pin_box` (i128 squares at
  `i32::MAX`/`MIN`).

- [x] **Q06.2 — Fixtures for attribution.**
  `q06-sole-wall.dsn`: one connection whose only route is closed by a routed
  trace of one other net. `q06-fixed-cage.dsn`: a pin walled in by fixed pads
  of other nets.
  - **Accept:**
    - the sole-wall net is named first, with `unique == blocked`;
    - the caged pin shows 0 % routed coverage (`routed == 0`);
    - the SES is byte-identical with the stage on and off.

  *Done 2026-10-17:* the fixtures are in
  `crates/freerouting/tests/data/quality/` beside Q01's and Q03's (not
  `fixtures/`, whose boards all need Java references):
  `q06-sole-wall.dsn` (A1-A2 must cross B's unfixed wire, B's pads 0.2 mm
  from the outline) and `q06-fixed-cage.dsn` (A1 inside eight single-pin-net
  pads 0.3 mm apart). `cargo test -p freerouting --test
  quality_attribution`: 8 passed —
  `sole_wall_on_the_input_board_names_the_wall_net` (top net B, the only
  net, `unique == blocked == 10`, coverage 12 entries / 10 routed / 2
  fixed), `fixed_cage_is_unrouted_with_no_routed_coverage` (`routed == 0`,
  `fixed == 6`, search exhausted, last attempt FAILED in pass 2, no work
  limit), `the_ses_is_byte_identical_with_the_verdicts_on_and_off` (both
  fixtures and the QFN board, 1 and 3 passes, `write_ses` equal),
  `analysing_a_connection_changes_nothing_on_the_board` (board meter and SES
  unchanged by `analyze_connection`). Also
  `sole_wall_after_a_pass_is_unrouted_but_routable`: after pass 1 (A routed
  by ripping B) B is incomplete, never attempted, and routable round A's
  ends (`search: found`, verdict `unrouted`, A met first).

- [x] **Q06.3 — Verdicts.**
  Rust design: `quality::verdict::Verdict { Infeasible(PinFeasibility),
  WorkLimit { pass, work }, BlockedBy(Attribution), Unrouted(Coverage) }`
  per incomplete connection, in ascending order of the connection's first
  item id. Q03's check runs once per job on the input board for the pins of
  incomplete connections only.
  - **Accept:** on `q03-qfn-0p5.dsn` the QFN connection is `infeasible`; on
    `q06-sole-wall.dsn` it is `blocked_by`; on `q06-fixed-cage.dsn` it is
    `unrouted`; a connection run with a tiny work limit is `work_limit`.

  *Done 2026-10-17:* `quality/verdict.rs`: `Verdict { Infeasible(Box<
  PinFeasibility>), WorkLimit { pass, work }, BlockedBy(Attribution),
  Unrouted(Coverage) }` in `ConnectionVerdict`s by ascending `(from, to)`
  item id, from `VerdictDiagnostics`, registered as `verdicts` in
  `quality/register.rs` (new; called from `StageRegistry::builtin`) under a
  new `Diagnostics` trait selected by `router.special.diagnostics`. Q03 runs
  once on a copy of the board before the fanout, for the candidate end pins
  of incomplete connections only. Tests in `tests/quality_attribution.rs`:
  `qfn_power_pin_is_infeasible` (PWR U1-5 `infeasible`, corridor 0.2602 mm),
  `sole_wall_on_the_input_board_names_the_wall_net` (`blocked_by`),
  `fixed_cage_is_unrouted_with_no_routed_coverage` (`unrouted`),
  `a_tiny_work_limit_gives_work_limit` (`FreeroutingConnectionRouter` with
  `work_budget = 0`: FAILED, `work_limit_hit`, verdict `WorkLimit { pass: 1
  }`).

- [~] **Q06.4 — Schema and output.**
  Document the JSON in `docs/development/verdicts.md` (schema
  `freerouting-verdicts/1`, one object per incomplete connection with its
  from/to items, verdict and evidence). Emit it under `verdicts` in the C08
  result JSON.
  - **Accept:** a golden-JSON test on the three fixtures; the doc lists every
    field; the CLI and the GUI incompletes list agree on 3 boards (the GUI
    part is `[~]` until the G series shows the list).

  *Partial 2026-10-17:* done: `docs/development/verdicts.md` lists every
  field of `freerouting-verdicts/1`; `RunSummary::verdicts` carries the
  report; `crates/freerouting-cli/src/manifest.rs` adds it under
  `verdicts` after Java's keys only when the diagnostic ran;
  `verdict_json_matches_golden_and_is_deterministic` (the three fixtures
  against `tests/data/quality/q06-verdicts.golden.json`, two runs equal);
  `cargo test -p freerouting-cli --test result_json_verdicts --test
  result_json`: 4 passed (`the_manifest_carries_the_verdicts_only_with_the_diagnostic`:
  the key is there with the diagnostic and absent without, same SES).
  Remains: the GUI incompletes list (G03's `panels/incompletes.rs`, archived)
  does not read `RunSummary::verdicts` yet, so the CLI/GUI agreement on 3
  boards is not checked; no G checklist owns that yet (operator decision).

- [x] **Q06.5 — Sanity on real boards.**
  - **Accept:** on `canary-pwr` and two `benchmark` boards, the done note
    gives the verdict counts per board, and for each `blocked_by` the top
    net. Every `infeasible` connection's pins are also in Q03's report for the
    board. This is the evidence Q05's decision rule reads.

  *Done 2026-10-17:* release CLI, `--router.special.diagnostics=verdicts
  --router.optimizer.enabled=false`, each run also without the diagnostic
  (`cmp` of the SES: identical on all three boards):
  - `Issue508-DAC2020_bm01` `-mp 5`: 15 incomplete: `blocked_by` 4,
    `unrouted` 11 (all `search: found`, ripped after their last attempt),
    `infeasible` 0, `work_limit` 0. Top nets: ADC10 U12-3 -> U56-87 `+5V`;
    GND U24-P$2 -> U6-1 `SDA`; RXD1 U56-45 -> U13-3 `TXD1`; PG1 U51-14 ->
    U56-52 `+5V`. 37.5 s with, 32.5 s without.
  - `Issue690-sonde_xilinx` `-mp 5`: 11 incomplete, all `unrouted`
    (`found`, last attempt ROUTED in pass 5); no `blocked_by`.
  - `canary-pwr-9511d9a` with Q01.7's flags (`-mp 1 --gui.enabled=false
    --router.scoring.start_ripup_costs=1500 --router.min_trace_width_um=150`):
    109 incomplete (Q01.7's 109): `infeasible` 8, `blocked_by` 14,
    `unrouted` 87 (83 `found`: 73 with last attempt ROUTED, i.e. ripped later
    in the pass), `work_limit` 0. Top nets of the 14 `blocked_by`: `GND` 7
    (PRIV_SDA U1-2, /Holdup/LTC_BST C14-1, /Holdup/LTC_CAP4 R20-1,
    /Holdup/LTC_CAP3 R19-1, /Holdup/LTC_CAP2 R18-1, LTC_SMBALERT U5-1,
    /Supervisor/CAN_FAULT_N U5-29), `/Holdup/LTC_INFET` 2 (/Holdup/LTC_VCC2P5
    U1-35, +12V_BUS D6-2), `/Holdup/LTC_VCAPP5` (/Holdup/LTC_CFN U1-18),
    `/Holdup/LTC_BST` (/Holdup/LTC_INTVCC U1-29), `/Pi_Supply/LM_VCC`
    (+5V_BUCK U2-1), `+3V3_AON` (+5V_PI U3-8), `/Pi_Supply/+5V_SHUNT`
    (/Pi_Supply/OR_GATE U4-5). The `infeasible` pins U1-8, U1-21, U1-22,
    U1-30, U1-34, U3-7 are all in Q03's report (`freerouting-dev feasibility
    canary-pwr-9511d9a.dsn`: 19 candidates, 7 infeasible, the six plus
    U1-24). 67.7 s with, 59.0 s without; a second run gave the same SES and
    equal `verdicts`.
  - Reading for Q05: on these runs few incomplete connections are walled in
    by routed nets (4/15, 0/11, 14/109); most are routable on the final
    board after being ripped (`search: found`), so a pass-count or retry
    effect, not congestion, dominates at these pass limits.
