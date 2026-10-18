# Q07 — First-pass net ordering

Opened 2026-10-15. Java's pass 1 routes in item order; later passes shuffle
with the D2 seed. KRT orders its nets by greedy rounds over a crossing-conflict
graph ("MPS", `net_queries._greedy_order_mps_units`,
`compute_mps_net_ordering`) and has an inside-out order for dense parts
(`net_ordering.order_nets_inside_out`), at `0486e799`. KRT's "MPS" is not the
exact maximum-planar-subset algorithm; it is a heuristic, and KRT has no A/B
of it against its original order ("MPS typically achieves higher success
rates" in `docs/net-ordering.md` is unmeasured). So both enter as experiments
against Java's order, for pass 1 only.

Prerequisites: board parity (rung 3); A10 (`TaskOrdering` trait,
`FreeroutingOrdering`); F03.1, F03.4.

A third candidate order comes from the Java fork's unimplemented plan
`docs/research/escape_congestion_research.md` (at `main`; a planning
document, no code): estimate escape congestion per component before
routing and route the pins under the most pressure first. It is written
for the fanout pre-pass (X10, X12), but its congestion estimate is also a
pass-1 ordering key. Preregister it as an arm only if the first two orders
leave congestion failures (Q06's verdicts say).

## What exists and what this adds

| Exists | State |
|---|---|
| Java `BatchAutorouter.getAutorouteItems` + `AutoroutePassRunner.reorderSingleThreadItems` (cda540f): plane-net tasks first, item order in pass 1, `JavaRandom` shuffle seeded `pass_no·1000003 + max_net_number` later | ported by A10 as `FreeroutingOrdering` |
| Rust `drc::incompletes::AirLine`, `NetIncompletes` | the connections and their pin positions |
| KRT crossing test (MST segments, CCW intersection, same-layer filter with `crossing_layer_check`); greedy rounds picking the fewest conflicts, ties by "routing-aware" distance then unit id; neighbours of a pick pushed to the next round | the design |

## Decisions made here

- **Pass 1 only.** `ConflictRounds` implements `TaskOrdering`: `select` and
  later passes delegate to `FreeroutingOrdering`; only pass 1's `order` is
  replaced. Plane-net tasks stay first (A10 invariant).
- **Integer geometry.** MST per net over pin centres (Prim from the lowest pin
  item id, edge weight squared distance in `i64`, ties by item id pair).
  Crossing by exact `i128` orientation tests. Lengths in board units. KRT's
  BGA-corner "routing-aware" distance is grid-specific and is dropped.
- **Tie-breaks:** within a round, pick the fewest conflicts, then the shorter
  MST, then the lower net number. Each round's output is sorted by
  `(MST length, net number)`.
- **Ids:** `conflict-rounds` and `inside-out`, selected by
  `router.autorouter.ordering`.

## File ownership

| Item | New files | May edit |
|---|---|---|
| Q07.1 | `crates/freerouting/src/quality/ordering/mod.rs`, `quality/ordering/conflicts.rs` | `quality/mod.rs` (append) |
| Q07.2 | `quality/ordering/conflict_rounds.rs` | `quality/register.rs` (append) |
| Q07.3 | `quality/ordering/inside_out.rs` | `quality/register.rs` (append) |
| Q07.4 | the Q07 preregistration and baseline files | `docs/DIVERGENCES.md` (append, only if kept) |

## Checklist

- [x] **Q07.1 — Conflict graph.**
  Rust design: `quality::ordering::conflicts::ConflictGraph::build(board,
  tasks) -> ConflictGraph` with, per net, its MST segments and the set of
  layers its pins are on (through-hole pins: all layers); two nets conflict
  if a segment of one properly crosses a segment of the other and their layer
  sets intersect. Adjacency as sorted `Vec<u32>` per net.
  - **Accept:** unit tests: two crossing two-pin nets on one layer conflict;
    the same on disjoint layers don't; segments sharing an end point don't;
    collinear overlap counts as no crossing (documented).

  *Done 2026-10-17:* `quality/ordering/conflicts.rs`: `ConflictGraph::build(board,
  tasks)` over the nets of the signal (non-plane) tasks, from every pin of those
  nets; `NetGeometry` per net (MST segments, layer mask of its pins' drill
  spans, MST length = sum of floor square roots); adjacency as sorted
  `Vec<u32>`; a sweep over the segments by `min_x` with a bounding-box and
  layer prefilter. Prim from the lowest pin id, `i64` squared distances
  (saturating beyond `CRIT_INT`), ties by `(tree id, new id)`; crossing by
  exact `i128` orientation signs, so shared end points, T contacts and
  collinear overlap are no crossing (module docs). Unit tests
  (`cargo test -p freerouting --lib quality::ordering`, 14 passed):
  `crossing_two_pin_nets_on_one_layer_conflict`,
  `crossing_nets_on_disjoint_layers_do_not_conflict` (and a through-hole net
  conflicts with either), `segments_sharing_an_end_point_do_not_cross`,
  `collinear_overlap_is_no_crossing`, `crossing_is_exact_at_the_coordinate_limits`
  (±(`CRIT_INT`−1), a parallel line one unit off misses, a half-unit offset is
  seen), `mst_starts_at_the_lowest_id_and_breaks_ties_by_id_pair`,
  `multi_pin_nets_conflict_through_any_segment`. On the 62 boards of
  `tools/bench/sets/q07.txt` the graph and both orders take at most 46 ms
  per board in release (`tests/quality_ordering.rs report_pass_one_orders`).
  The file was removed with Q07.2's rejection (Q07.4); it is kept, with
  the rest of the implementation, in the patch named in Q07.2's note.

- [x] **Q07.2 — Greedy conflict rounds (`conflict-rounds`).**
  Source: KRT `_greedy_order_mps_units`.
  Rust design: repeat until no net remains: within the current round, pick
  the remaining net with the fewest conflicts among those still in the round
  (ties as in the decisions), emit it, and push its conflicting neighbours to
  the next round. Pass-1 order: plane-net tasks first, then the rounds
  concatenated.
  - **Accept:**
    - unit tests on a 4-net example with a hand-derived order;
    - with `router.autorouter.ordering` at its default, `ses-diff` shows
      identical sessions on `benchmark`;
    - `tools/determinism-check.sh` passes on `canary-pwr` with
      `conflict-rounds`.

  *Done 2026-10-17 (rejected by Q07.4: the gate failed, the code is removed, the
  numbers stay in Q07.4's note):*
  - **What was built.** `quality/ordering/conflict_rounds.rs` (F04 header,
    KRT `_greedy_order_mps_units`): `rounds(&ConflictGraph)` with incremental
    in-round conflict counts, picks by `(conflicts in round, MST length, net)`,
    neighbours of a pick moved to the next round, each round sorted by
    `(MST length, net)`; `conflict_rounds_order(board, tasks) -> tasks` (the
    API Q12 asked for: a permutation of Java's pass-1 list, plane-net tasks
    first and unmoved, signal tasks moved net by net with each net's tasks in
    Java's order); `PassOneOrdering` (`quality/ordering/mod.rs`) wraps an order
    as a `NetOrdering`: pass 1 only, `order`/`on_pass_end`/`reset` delegate to
    `FreeroutingOrdering`, a restore does not bring pass 1 back; registered as
    `conflict-rounds` under `router.autorouter.ordering` (no settings-table
    change: the key exists since A10.2). The whole implementation (with
    `inside-out` and the tests) is parked on
    branch `rejected/q07-orderings` (379b413, on top of `c23d8c0`).
  - **Unit tests** (in that patch): `four_nets_give_the_hand_derived_order`
    (nets 1–4, conflicts 1–2 and 1–3: round 1 picks 4, then 3 over 2 by MST
    length, pushing 1; order 3, 4, 2, 1),
    `ties_fall_to_the_shorter_mst_then_the_lower_net`,
    `a_net_crossing_all_others_goes_last_and_rounds_cover_every_net`; on a
    loaded synthetic board `tests/quality_ordering.rs`
    `conflict_rounds_give_the_hand_derived_order_on_a_board`,
    `the_orderings_replace_pass_one_only`,
    `the_orderings_are_registered_and_selected_by_the_ordering_key`,
    `the_orders_permute_java_s_list_on_a_real_board` (CNH tester): all passed
    (`cargo test -p freerouting --lib quality::ordering`, 14 passed;
    `--test quality_ordering`, 6 passed, 1 ignored report).
  - **Default unchanged.** With the default ordering the 21 SES of the
    measurement's `java` arm (16 `benchmark` boards and the five canary-pwr
    variants, route bounds) are byte-identical to `fixtures/reference/route`
    (Java `a773a70ec`), and `freerouting-dev ses-diff <dsn> <java arm>
    <reference>` says `equivalent` on all 21
    (`target/orchestrator/q07/runs/ses-diff-default-vs-java.txt`).
  - **Determinism.** `tools/determinism-check.sh --out runs/det-cr -- bash -c
    '... freerouting-q07 -de fixtures/canary-pwr/canary-pwr-pours1.dsn -do
    $1/route.ses --router.autorouter.max_passes=3
    --router.optimizer.enabled=false --router.autorouter.ordering=conflict-rounds'`:
    `identical` (idle and beside 8 burners, load 5.6 → 13.8), and the session
    equals the bench run's for that board byte for byte.

- [x] **Q07.3 — Inside-out ordering (`inside-out`).**
  Source: KRT `order_nets_inside_out` (nets with pins inside a dense part's
  pin field, nearest the part centre first; the rest in input order).
  A dense part is a component with at least 16 pins whose smallest pin pitch
  is at most 0.65 mm (KRT's fine-pitch threshold). Distance is from the net's
  nearest pin to the part's pin bounding-box centre, in `i64` squared units;
  ties by net number.
  - **Accept:** unit tests on a synthetic QFN; `ses-diff` identical with the
    default.

  *Done 2026-10-17 (rejected by Q07.4: the gate failed, the code is removed, the
  numbers stay in Q07.4's note):*
  - **What was built** (on branch `rejected/q07-orderings`):
    `quality/ordering/inside_out.rs` (F04 header, KRT `order_nets_inside_out`):
    `dense_parts(board)`: components with at least 16 pins whose smallest
    non-zero pin-centre distance is at most 0.65 mm + 1 µm (KRT's `_DIST_TOL`,
    as X06/X10), pin field = bounding box of the pin centres;
    `dense_net_order`: nets with a pin inside (or on the edge of) a field,
    by the smallest squared distance to the field's centre in exact `i64`
    (doubled coordinates), ties by net number; `inside_out_order(board, tasks)
    -> tasks` with the other nets in Java's order; registered as `inside-out`.
  - **Tests:** `inside_out_routes_a_fine_pitch_qfn_from_its_centre` (a
    synthetic 16-pin QFN at 0.5 mm: NEAR_T and NEAR_B, 0.5 pitch along the
    edge, tie broken by net number, then FAR, 1.5 pitches along, then OUT in
    Java's place), `inside_out_keeps_java_order_without_a_dense_part` (the
    same QFN at 1.0 mm), `min_pitch_ignores_coincident_pins`,
    `distance_to_an_odd_field_centre_is_exact`: all passed (counts in Q07.2's
    note).
  - **Default unchanged:** the same 21 byte-identical SES and `equivalent`
    `ses-diff` results as Q07.2 (one default arm serves both).

- [x] **Q07.4 — Measurement.**
  Preregister (F03.4). Arms: Java order, `conflict-rounds`, `inside-out`.
  Boards: `benchmark` plus `canary-pwr` and its variants. Signal: unrouted
  after the full run, and after pass 1 (reported, not judged). Guards: vias,
  length, `drc_real`.
  - **Accept:** measured. An arm is kept only if it improves on N−1 boards
    and regresses on none; otherwise its code is removed and the numbers stay
    here.

  *Done 2026-10-17 (both orders rejected: the gate failed for each; their code
  is removed, the numbers stay here and in the baseline files):*
  - **Registration.** `docs/roadmap/prereg/Q07.json` (`conflict-rounds`,
    sha256 `b31fb555…205ecc`, 61 rows) and `Q07-inside-out.json` (`inside-out`,
    `b32f7baa…a26629`, 40 rows), both against the `java` arm
    (`freerouting-ordering`), written before the first number, 0 amendments,
    pinned by `crates/freerouting-dev/tests/prereg_q07.rs`. One smoke run before
    registration (bm01, 3 passes) is disclosed in the files' `notes`. Set
    `tools/bench/sets/q07.txt`: the 16 `benchmark` boards, the five
    in-repository canary-pwr variants (one board), the 11 PCBWorld-Bench D3-C
    boards and the 30 F03.3 corpus boards. Eligibility from the inputs: a row
    with no signal-net task (laser-backplane) is Java's order by construction
    for `conflict-rounds`; a row with no dense part (22 rows) is Java's order
    by construction for `inside-out`. Run: `freerouting-dev bench --rust
    <wrapper> --runs 1 --max-load off --timeout 7200` (release CLI with
    `--router.autorouter.ordering=<id> --stats json`, route bounds, else 5
    passes, optimizer off), the P04 rows and glasgow with `--jobs 1`, the
    other rows with `--jobs 2`; load 5–14 from other sessions, so wall times
    are not comparable. Every run exited 0, none timed out.
  - **Unrouted, full run → pass 1** (pass 1 is reported, not judged), `java →
    conflict-rounds`: 433-my-board 0→0/1→0; ecc83 0→0/0→0; bm02 0→0/0→1; sonde 11→11/11→11; bm07 0→0/4→6; bm01 24→33/49→55; bm10 0→0/15→20; interf_u 19→19/19→19; CNH 1→1/16→18; z10 37→37/37→37; coldfire 181→181/181→181; CPU-85 57→58/161→161; GP8B 61→69/220→228; smoothieboard 19→30/68→70; LogicBoard 285→300/285→300; contribution-board 57→42/172→165; canary pours1 120→114, pours2 116→120, pours3 115→113, pours-as-planes 71→73, unpoured 107→107 (sum 529→527; pass 1 equal to the full run on every variant); d3c-0400 2→2/4→5; d3c-0423 1→0/3→4; d3c-0448 2→1/16→10; d3c-0473 0→1/6→8; d3c-0496 16→13/36→28 (violations 9→7); d3c-0520 0→0/4→5; d3c-0546 0→0/4→6; d3c-0571 2→3/11→11; d3c-0581 4→4/4→4; d3c-0599 3→6/14→16; glasgow 162→173/162→173; picodebugger 0→0/0→4; pcietest 1→1/21→23 (violations 21→22); ch32v003 0→0/1→2; cc1101 2→2/2→2; rp2040dev 40→49/44→49; bread-slice 2→2/9→5; flight-computer 1→1/4→3; vfo 0→0/6→7; sx1262 0→0/0→0; eprom-card 25→16/82→76; rio-iceshield 14→28/37→35; diode-matrix 0→0/2→1; vna-usb-pd 0→0/0→0; picodvi 10→13/17→16; pcie-active-backplane 0→1/6→6; solarmeshtastic 0→0/2→2; openmuscle 3→1/13→17; wisweep 17→24/28→33; nudac 2→3/13→15 (violations 4→0); loratracker 34→35/34→35; m-fc-g474 4→6/19→23; moteus 39→43/39→43; pocketsdr 5→3/41→36; cm4-carrier 7→8/7→8; overlord 3→3/7→6; nescart 59→66/59→72; miniscope 41→43/46→55; microzed 2→3/18→47; healthypi 26→22/26→22. Totals over 61 rows: unrouted 1,810 → 1,884,
    pass 1 2,615 → 2,712, vias 8,401 → 8,397. Pass 1 better on 15 rows,
    worse on 32, equal on 14.
  - **`java → inside-out`**: bm01 24→40/49→54; bm10 0→0/15→22; z10 37→37/37→37; coldfire 181→181/181→181; CPU-85 57→65/161→141; GP8B 61→66/220→245; smoothieboard 19→34/68→71; LogicBoard 285→310/285→310; canary pours1 120→121, pours2 116→123, pours3 115→125, pours-as-planes 71→77, unpoured 107→107 (sum 529→553); d3c-0473 0→0/6→7; d3c-0496 16→19/36→27; d3c-0581 4→4/4→7; glasgow 162→175/162→175; pcietest 1→4/21→27 (violations 21→14); ch32v003 0→0/1→0; cc1101 2→3/2→3; rp2040dev 40→47/44→51; flight-computer 1→1/4→4; vfo 0→1/6→5; rio-iceshield 14→21/37→38; vna-usb-pd 0→0/0→1; picodvi 10→11/17→22 (violations 20→23); pcie-active-backplane 0→0/6→11; openmuscle 3→2/13→14; wisweep 17→24/28→28; nudac 2→2/13→17 (violations 4→0); loratracker 34→33/34→33; m-fc-g474 4→4/19→18; moteus 39→42/39→42; pocketsdr 5→6/41→50; cm4-carrier 7→9/7→9; overlord 3→5/7→10; nescart 59→62/59→81; miniscope 41→36/46→56; microzed 2→0/18→23; healthypi 26→27/26→27. Totals over 40 rows: unrouted 1,685 →
    1,824, pass 1 2,241 → 2,400, vias 6,843 → 6,821. Pass 1 better on 6
    rows, worse on 29, equal on 5.
  - **F03.1 gate** (`freerouting-dev gate --records … --prereg …`, logs
    `target/orchestrator/q07/runs/q07-{cr,io}-gate.txt`): both **FAIL**, exit 1,
    0 refusals. `conflict-rounds` improved 9 of 57 boards (contribution-board,
    d3c-0423, d3c-0448, d3c-0496, eprom-card, healthypi, nudac on the
    clearance guard, openmuscle, pocketsdr), regressed 24 (including
    canary-pwr, where pours2 and pours-as-planes regress although the sum
    falls 529 → 527, and coldfire on the HARD `via_in_pad` 6 that the `java`
    arm has too), neutral 24; luck rate at N=57: 1 in 1.4·10^17.
    `inside-out` improved 6 of 36 (loratracker, microzed, miniscope, nudac
    and pcietest on the clearance guard, openmuscle), regressed 21, neutral
    9; luck rate at N=36: 1 in 6.9·10^10. The gate marks a row by the first
    metric that differs, so a fall in clearance violations counts as an
    improvement even where unrouted rose (pcietest under `inside-out`). The
    `via_in_pad` HARD lines (smoothieboard 42, coldfire 6) are in the `java`
    arm as well, as in P04.
  - **KiCad oracle** (F02.6, KiCad 10.0.7, `grade.sh` per project with stdin
    from `/dev/null`; the engine wrapper grades the bench's own session when
    KiCad's export equals the bench input except its `pcb` name line, 36–37
    projects per arm, and routes at `max_passes=5` otherwise: the four demos,
    canary-pwr rev1.0 and d3c-0520/0571/0599): `conflict-rounds` on 44 graded
    projects: drc_real 5,826 → 5,492 in sum, lower on 22, higher on 16
    (canary-pwr 39 → 44, coldfire 235 → 241, interf_u 9 → 10, …), equal on 6;
    KiCad unconnected 981 → 1,038. `inside-out` on 28 graded projects:
    drc_real 5,027 → 4,780, lower on 17, higher on 10 (canary-pwr 39 → 42,
    coldfire 235 → 240, …); unconnected 855 → 918. A drc_real rise is a
    regression under the registration, so the oracle fails both too. Not
    graded: d3c-0581 (a123 BCM), whose KiCad DRC of the *unrouted* input hung
    for 18 minutes without CPU and was killed (the same input in every arm, so
    it was not retried). Tables:
    `target/orchestrator/q07/runs/oracle-compare-{cr,io}.md`.
  - **Board classes.** Neither order wins on a class one can name from the
    inputs. Medians, improved vs regressed rows, for `conflict-rounds`: signal
    layers 3 vs 4, plane scopes 3 vs 5, nets 77 vs 74, conflicts per net 3.2
    vs 2.3, dense parts 1 vs 2; the improvers span 2–6 layers, 0–6 planes,
    27–375 nets and 1.2–7.5 conflicts per net, and every such range also holds
    regressions (two-layer no-plane boards: contribution-board and eprom-card
    improve, bm01, pcietest and rio-iceshield regress). For `inside-out` the
    six improvers are corpus boards of 4–6 layers with 2–15 plane scopes, but
    so are cm4-carrier, overlord, wisweep, moteus and healthypi, which regress;
    dense parts 2.5 vs 2, nets 62 vs 83. KiCad's drc_real falls on more
    projects than it rises for both orders, while unconnected rises: the new
    orders leave more connections open and so fewer DRC items.
  - **Outcome.** Both orders, the conflict graph and the `PassOneOrdering`
    wrapper are removed (no DIVERGENCES or PROVENANCE line: no ported file
    remains); the implementation is parked on branch
    `rejected/q07-orderings` (379b413) for Q12's portfolio arms. The baseline files
    `tools/bench/baselines/Q07.json` (61 rows) and `Q07-inside-out.json` (40
    rows) keep every row with `"rejected": true`; the gate run on them reports
    `rejected: 61 row(s)` and `40 row(s)` and no DRIFT.
