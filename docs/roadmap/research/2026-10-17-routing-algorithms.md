# Routing algorithms beyond Freerouting and KRT: proposals

Written 2026-10-17 by freerouting-rs-f1 for the orchestrator (freerouting-rs-11), at
Dan's request ("research other routing algorithms we could try … have them added as
checklists and implemented if anything good comes up"). This is research, not a
checklist. Ids below (N1–N11) are placeholders: the orchestrator picks real ids
(Q10+ or a new series), the order against Q04/Q07/Q08/P04, and what to drop.

Sources: four web-research passes (autorouter survey, rip-up and congestion
strategies, exact solvers, topological routing and DFM), plus this repo's own
measurements (Q05.1, Q06.5, the canary-pwr fixtures README, the Java fork's HANDOVER).
Labels: **[V]** read in the source, **[S]** search snippet or abstract only,
**[I]** inference. Prereg drafts for N1–N3 are in `prereg-drafts/` next to this file
(format `freerouting-ab-prereg-1`, `"registered": "DRAFT"`; not registered, no
sha256 pinned).

## 1. What the engine's own numbers say (read this first)

The literature offers dozens of strategies; our measurements say which failure to
target. On canary-pwr-9511d9a (Q01.7's flags, `-mp 1`), Q06.5 found 109 incomplete
connections:
- **83 `found`**: routable on the final, optimized board with rip-up off. **73 of
  them were routed and then ripped by a later connection in the same pass.** With
  `-mp 1` nothing retries them, so each such rip is a permanent loss.
- 14 `blocked_by` routed nets, 8 `infeasible` at their width (Q03), 0 `work_limit`.

Java points the same way:
- Raising `start_ripup_costs` from 100 to 500 to 1500 improves pass 1: 120, 109, then 100
  unrouted (Q05 file).
- Every later rip-up pass is worse (161–185), and the loop restores pass 1.
- The add-only second run (wiring frozen, `-mp 2`) takes KiCad's count from 45 to 40,
  which is the result to beat.
- LESSONS.md item 5: "add-only beats rip-up … freeze and re-route 43→39".

Two conclusions:
- **Rip-up in pass 1 destroys more than it gains**, and the gain is in not losing the
  victims, not in congestion avoidance. The cheapest candidates that stop the loss
  come first: N1 to N3.
- **Congestion and global methods target `blocked_by`, which is 13–29 % of the
  incomplete connections** (Q05.1). Negotiated congestion proper, global routing and
  topological planners stay gated behind N1 to N3 (N8 to N11).

A board-level noise caveat for every measurement here: on canary-pwr a small input
change moves completion by ±10. N3's portfolio measures the per-board **order
spread**. That is a noise floor every Q and X keep rule could use as its quantum
(see N3.4).

## 2. Ranked shortlist

| # | Candidate | Changes routes | Size | Code before P04? | Gate |
|---|---|---|---|---|---|
| N1 | Add-only completion sweep (in-engine "freeze and add"), also after the optimizer | yes, when on | S | yes; measure after P04 | none |
| N2 | Transactional rip-up: re-route the victims at once, roll back if completion falls; plus a no-code rip-cost sweep | yes, when on | S–M | yes; measure after P04 | none |
| N3 | Pass-1 portfolio: K deterministic orders, in parallel, keep the best; squeaky-wheel restarts | yes, when on | S | yes, after Q07.1–Q07.3 are built; measure after P04 | none |
| N4 | Shove before rip (Mighty's weak modification, Situs/Xpedition "Hug") | yes, when on | M | after P04.1 lands (insertion and room code) | N2 measured |
| N5 | Acute-angle and acid-trap cleanup (mitering), an optimizer stage | geometry only, no connectivity | S | yes | N5.1 count shows acute corners exist |
| N6 | Spread and centre pass (DFM) | geometry only | S–M | yes | Dan wants DFM work |
| N7 | Exact assignment in the escape planners (min-cost flow for BGA, DP/matching for QFN) | yes, when on | S–M | after X10/X12 are measured | X10/X12 measured post-P04 |
| N8 | SAT window repair of the last unrouted connections | yes, when on | M + NEW DEP | after P04 | N1–N4 leave ≥ 5 `blocked_by`/`found` per board on 3 boards |
| N9 | Specctra-style conflict-tolerant passes (true negotiated congestion) | yes, when on | L | after P04 | N1–N4 recover under half of the `found` failures |
| N10 | Soft layer pre-assignment from a crossing graph (max-cut) | yes, when on | S | yes | experiment only; evidence mixed |
| N11 | CDT topological feasibility diagnostic, then a corridor planner | report, then yes | S then M | yes | the diagnostic shows ≥ 30 % of `unrouted` feasible-but-failed on 3 boards |

Not recommended (section 5): a full topological router, coarse global routing or LP
global routing, RL/ML routers, GPU lattice routers, teardrops/stitching (KiCad does
them).

Suggested order: N1, then N2 (its rip-cost sweep first, which is a settings run), then N3
(after Q07's orders exist). Measure the three together post-P04 as one prereg family, since they compose:
portfolio of (transactional pass 1), then sweep, then optimizer, then sweep. N5 is
independent and cheap. Everything else waits on a gate.

## 3. Draft checklists for N1–N3

House rules apply unchanged: off means byte-identical (`ses-diff` on
`connection-parity` and `benchmark`), D2 via `tools/determinism-check.sh`, D8, the Q
measurement rules, and "a rejected candidate leaves no code".

### N1 — Add-only completion sweep

Opened 2026-10-17 (research by freerouting-rs-f1). Java's best canary-pwr result (40)
comes from a second run with pass-1 wiring frozen (`dsn_export --fix-existing`,
`-mp 2`). Q06.5 shows 83 of 109 incomplete connections are routable on the final
board with rip-up off. That is the measured case for adding without ripping. Altium
Situs has the same idea as its "Completion" pass, which re-runs the main pass with
different costs such as cheaper vias [V, Altium docs].

**What exists.**
- `ConnectionParams::ripup_allowed` exists: fanout uses it, and Q06's analysis search
  runs with rip-up off.
- `PassController` and `RipupPolicy` are A10 traits. The batch loop restores the best
  board (A13), and B01 snapshots exist.
- Java has no in-engine sweep: the two-run flow goes through KiCad's exporter.

**Decisions.**
- A sweep pass is Java's pass with `ripup_allowed = false` for the autoroute phase.
  Same ordering stage, same budgets.
- It starts from the board the loop would output: the best board after restores, not
  the last pass.
- Sweeps repeat until one routes nothing, capped at `sweep_passes` (default 2).
  Q04's quantum rule can replace the cap later.
- The `fixed` arm marks pre-sweep wiring as user-fixed for the sweep and unfixes it
  afterwards. That reproduces Java's two-run flow. The `free` arm leaves it unfixed, so
  pull-tight inside the sweep may move it.

**File ownership.**
- Creates `quality/sweep.rs`.
- Appends to `quality/mod.rs`, `quality/register.rs`, and the C02 settings struct
  (`router.autorouter.sweep = off|fixed|free`, `sweep_passes`, `sweep_after_optimizer`).

**Checklist.**
- [ ] **N1.1: the sweep pass controller.** `SweepController` wraps the configured
  pass controller. When it finishes, it restores the best board and runs sweep passes
  with rip-up off.
  - **Accept:**
    - unit test: a fixture where pass 1 rips a connection that is routable afterwards
      (q06-sole-wall style) ends complete with the sweep and incomplete without;
    - off is byte-identical on `connection-parity` and `benchmark`;
    - determinism check on bm01.
- [ ] **N1.2: the `fixed` arm.** Freeze wiring during the sweep, then unfreeze it. Compare
  with Java `7f56b062d`'s two-run flow on canary-pwr-9511d9a, item by item where
  possible.
  - **Accept:** the done note lists the differences and their causes. Byte equality
    is not required, because the Java flow goes through the KiCad exporter.
- [ ] **N1.3: `sweep_after_optimizer`.** Optimizer, sweep, optimizer again: the
  optimized board has more room (Q06's 83 were found after the optimizer).
  - **Accept:** off is byte-identical; determinism.
- [ ] **N1.4: measurement** (prereg draft `prereg-drafts/N1.json`). The arms:
  - `mp1`: `-mp 1`, the baseline;
  - `mpdef`: the default passes;
  - `mp1+sweep-fixed`;
  - `mp1+sweep-free`;
  - `mp1+opt+sweep+opt`;
  - `mpdef+sweep`.

  The cheaper-via completion arm (`sweep_via_cost_factor` 0.5) goes in only if the
  prereg lists it before the first number.
  - **Accept:** measured. On canary-pwr the hypothesis is ≤ 40 KiCad-unconnected,
    clean (canary-23 grades).

**Expected:** a sweep only adds, so the router's incomplete count cannot rise. The
real guards are:
- pours cut by added traces, so KiCad's count can rise after refill (watch
  `pour_lost_pins` with Q01's pour guard on);
- vias and length;
- time.

### N2 — Transactional rip-up

Opened 2026-10-17. In pass 1, a rip whose victim does not re-land is permanent (73 of
109 on canary-pwr). Java's rip cost (`MazeRipupResolver.checkRipup`, about
`ripupCosts × halfWidth / detour` [V]) ignores whether the victim can be rerouted.

Practice and literature:
- KRT's "custody" rips [V, `docs/rip-up-reroute.md` at `0486e799`];
- rjwalters/kicad-tools #6342/#6355: all-or-nothing rollback plus a cap of 2 rips per
  (victim, blocker set) [V];
- Large Neighbourhood Search's destroy, repair, accept (Shaw, CP-98) [V].

No peer-reviewed PCB numbers exist [V: none found]. Java's own rip-cost trend is the
evidence.

**What exists.**
- `ConnectionOutcome::ripped` gives the ripped items in Java's order.
- B01 `BoardSnapshot` / `restore`.
- `ConnectionParams::ripup_allowed`.
- The routing ledger's incomplete count.
- Q05.1's `rip-stats` diagnostic (the re-land share is its natural metric).

**Decisions.**
- A `ConnectionRouter` wrapper, `transactional-ripup`, around the configured router.
  The search and its rip costs stay Java's; only the outcome is judged.
- Procedure:
  1. Snapshot. Route.
  2. If the outcome ripped something, re-route each victim connection at once with
     `ripup_allowed = false`, in ascending item-id order of the victims' `from` items.
  3. Commit by the arm's rule; otherwise restore the snapshot.
  4. After a restore, retry the connection once with rip-up off. If that fails too,
     record the failure as Java would. Java's after-failure net rip (`after_failure`)
     is unchanged.
- Snapshot cost: take it only when the search could rip (`ripup_allowed`), or use
  the undo journal if B01 offers one. The implementer measures which.

**File ownership.**
- Creates `quality/transaction.rs`.
- Appends to `quality/mod.rs` and `quality/register.rs`.
- Adds one `rip_count` field to `rip_stats` if needed.

**Checklist.**
- [ ] **N2.1: rip-cost sweep, no code.** `start_ripup_costs` ∈ {1500, 5000, 20000}
  and pass 1 with rip-up off, on the eligible boards. This bounds what N2 can gain.
  - **Accept:** a table in the done note, with Q05.1 rip stats per arm.
- [ ] **N2.2: the wrapper, arm `strict`.** Commit only if every victim connection
  re-lands.
  - **Accept:**
    - a fixture where A rips B and B cannot re-land: A is retried without rip-up, and
      B keeps its copper;
    - a fixture where B can re-land: both are complete;
    - off is byte-identical;
    - determinism.
- [ ] **N2.3: arm `count`.** Commit if the ledger's incomplete count strictly
  decreases. A ripped multi-pin net may re-land along a different topology.
  - **Accept:** as for N2.2.
- [ ] **N2.4: arm `depth1`** (optional). Victims may rip at depth 1, under the same
  commit rule, with no victim ripped twice in one transaction.
  - **Accept:** as for N2.2, plus a termination test on a 3-net cycle fixture.
- [ ] **N2.5: rip-count protection** (optional; needs a one-line engine hook). The rip
  cost of an item is multiplied by (1 + 2k), where k is the number of times its
  connection was ripped this run. KRT measured P(ripped again | ripped k times) ≈ 0.9.
  - The hook: `RipupPolicy::ripup_factor(item) -> i32`, 1 by default and not
    consulted in the parity path.
  - **Accept:** off is byte-identical; determinism.
- [ ] **N2.6: measurement** (prereg draft `prereg-drafts/N2.json`).
  - Arms: `mp1` (baseline), `N2.1-best`, `strict`, `count`, `depth1`, `strict+N2.5`,
    each also followed by an N1 sweep.
  - **Accept:** measured.

**Expected:** the upper bound on canary-pwr is the 73 routed-then-ripped
connections. Fewer will survive, because a rolled-back connection may then fail. No
literature number exists; a guess of 5–15 % fewer unconnected is [I].

### N3 — Pass-1 portfolio and restarts

Opened 2026-10-17. Pass 1 is the best pass on canary-pwr, yet passes 2–10 are about
60 % of routing time (Q05 file). Commercial practice spends that time on several
whole solutions and keeps the best:
- TopoR optimises several variants and drops the worst [V, Wikipedia];
- Cadence US6957408B1 "generates several solutions based on different net orders and
  returns the best" [V];
- tscircuit races sub-solvers with different parameters [V].

Evidence of order variance:
- KRT #1230: placebo perturbations moved open nets by about ±10 % [V];
- freerouting PR #793: on 70 boards, 24 better, 22 worse, 24 the same [V];
- kicad-tools #6342: a net pair routes in one order and not the other [V].

Squeaky-wheel restarts (Joslin & Clements, JAIR 1999) move failed elements to the
front and rebuild. KRT's naive "hot nets first" was **7 % worse** (#1230) [V], so it is
an arm, never the default.

**What exists.**
- The post-fanout board snapshot (B01).
- `NetOrdering` (A10), with Java's item order and the seeded shuffle with
  `JavaRandom`.
- Q07's conflict-round and inside-out orders, once Q07.1–Q07.3 are built.
- The board score (`BoardScore`).
- O06's parallel evaluation pattern: each candidate on its own copy, merged in
  submission order.

**Decisions.**
- A pipeline-level stage (`router.autorouter.portfolio = K`):
  1. Clone the post-fanout board K times.
  2. Run arm i's pass 1 (and N1's sweep, if on) on clone i. Use up to
     `max_threads` threads.
  3. Keep the best by (Freerouting incomplete count, board score, arm index).
  4. Then run the optimizer once on the winner.
- **Arm 0 is the configured ordering.** The portfolio can therefore never be worse
  than the baseline by the engine's own score: a "best of" includes the baseline.
  KiCad's count can still disagree (plane nets), so measurement is still required.
- Arm list, fixed in the settings and in this order:
  - Java item order;
  - Q07 `conflict-rounds`;
  - Q07 `inside-out`;
  - seeded shuffles with seeds 1…K−3.
- `restarts = R`: a squeaky-wheel generation. Its order is the winner's order,
  stable-sorted with that run's failed connections and ripped victims first. It runs
  sequentially after the portfolio, and the best of all is kept.
- Results are independent of thread scheduling: each arm is a pure function of
  (board, arm). They are merged by arm index.

**File ownership.**
- Creates `quality/portfolio/{mod,arms}.rs`.
- Appends to `quality/mod.rs`, `quality/register.rs`, the C02 settings struct and the
  pipeline's stage list (one call).

**Checklist.**
- [ ] **N3.1: the portfolio stage.** K clones, the fixed arm list, parallel threads,
  selection by key.
  - **Accept:**
    - with K = 1 the SES is byte-identical to the baseline;
    - a test where arm 2 beats arm 0 picks arm 2;
    - runs with `max_threads` 1 and 4 give the same SES;
    - determinism.
- [ ] **N3.2: squeaky-wheel restarts.**
  - **Accept:** a fixture where the failed connection routes when moved first ends
    complete; off is byte-identical; determinism.
- [ ] **N3.3: memory and time guard.** A board clone's peak memory times K must stay
  under a setting (WSL limits: see the memory notes).
  - **Accept:** canary-pwr and LogicBoard_smt with K = 4; peak resident memory in the
    done note.
- [ ] **N3.4: order-spread report.** Per board: min, median and max incomplete count
  (and KiCad count where graded) over the K arms.
  - **Accept:** a table on the eligible boards in the done note. It is proposed as the
    per-board noise quantum for later keep rules (an F03 amendment, the
    orchestrator's call).
- [ ] **N3.5: measurement** (prereg draft `prereg-drafts/N3.json`).
  - Arms: `mp1`, `mpdef`, `portfolio4`, `portfolio4+restart2`, and `portfolio4` with
    N1 and N2 on.
  - Work is reported against `mpdef`'s, since the portfolio is meant to replace
    passes 2–10 at about the same total work.
  - **Accept:** measured.

**Expected [I]:** with the ±10 spread on canary-pwr, a best-of-4 should recover a few
connections. Upstream's clone-and-shuffle threads were deleted, but they shuffled
later passes, not pass 1.

## 4. Sketches for N4–N11

**N4 — Shove before rip.**
- **Sources:**
  - Shin & Sangiovanni-Vincentelli, "Mighty", IEEE TCAD 6(6), 1987: try a weak
    modification (move wires, which stay connected) and rip only if it fails [V
    abstract];
  - Dees & Karger, DAC 1982, on shove-aside versus rip-up [S];
  - Altium Situs routes a topological path through the push-and-shove engine [V];
  - Xpedition's Hug router "pushes, shoves and squeezes" in the last nets [vendor];
  - Cadence US7752590B1, "space tile push" [V].
  - No published PCB completion numbers.
- **Design:** when the found path would rip, restore the snapshot and insert the path
  with the shove insert (O04 / I01), so crossed traces are pushed aside. Commit if
  exact DRC is clean and no pushed net loses a connection; otherwise insert Java's
  way.
  - This complements the in-search `shove_room` (A04.7), which only shoves traces of
    the path's own width from a door's outer section.
- **Hazards:** shoved vias into pads (D8: run the via-in-pad check per commit), fixed
  items, neck-down regions.
- **Hypothesis:** with N2 on, fewer victims lost (rip stats) and fewer
  KiCad-unconnected on N−1 boards.
- Changes routes. Code waits for P04.1, which touches room completion.

**N5 — Acute-angle and acid-trap cleanup.**
- KiCad 10 DRC only checks `track_angle`; it fixes nothing [V, KiCad 10 manual].
- KiCad's PNS optimizer (GPL-3.0-or-later, portable) has `REQUIRE_OBTUSE_ANGLES` and
  `dragFixCorner` [V, doxygen]. pcb-rnd `djopt` mitres (GPL-2+) [V].
- N5.1 is S and report-only: count corners under 90° (and under 135° for the 45°
  style) and same-net T-junction acid traps on routed benchmark SES files, graded with
  a `.kicad_dru` rule `(constraint track_angle (min 90))`.
- Build N5.2 only if the counts are non-trivial: an `Optimizer` stage after the batch
  optimizer that chamfers each acute corner if exact DRC passes.
- **Accept:** acute-angle violations −90 % on N−1 boards, with no new DRC, unrouted
  and vias unchanged, and length up by at most 0.5 %.
- Geometry only, no P04 dependency.

**N6 — Spread and centre.**
- Xpedition's Spread moves traces apart up to a per-layer target and centres them
  when the target is at least half the gap [V, Siemens blog 2014]. Altium Spread is
  "very slow" [V]. KiCad has none.
- **Design:** re-pull each trace at clearance c+Δ, stepping Δ down to 0. Accept a move
  only under exact DRC at the real rules, within a length tolerance. Go in ascending
  item id, and leave fixed and neck-down copper alone.
- **Metric:** KiCad DRC on a copy of the board with clearance raised by 0.05 mm; slack
  violations −30 % on N−1 boards; real DRC clean.
- Ask Dan whether DFM spacing matters to him before building it.

**N7 — Exact escape assignment.**
- **Sources:**
  - Yan & Wong, DAC 2009 and TCAD 31(2) 2012: a corrected node-split min-cost-flow
    model for unordered escape, with optimal results; earlier models mis-modelled
    diagonal capacity [S];
  - Luo & Wong, ASP-DAC 2008/2009: ordered escape by SAT, about 10×10 arrays in
    minutes, larger with partitioning [V];
  - Liu et al., DAC 2023: matching-based escape with variable widths and neck mode,
    100 % on industrial boards [S];
  - Kahng/Wang/Xu, "Tao of PAO", DAC 2020: DP pin-access selection [S].
- **Design:** inside X12, replace the greedy channel assignment with integer min-cost
  flow. Inside X10, a DP or matching over per-pad candidates (exit side, neck, fanout
  via outside the pad, layer) by where each connection goes.
- **Crate:** `ac-library-rs` (CC0, pure Rust, integer min-cost flow), or a hand-written
  successive-shortest-path solver of about 200 lines.
- Gate: X10/X12 measured post-P04, with the planners kept or near misses.

**N8 — SAT window repair.**
- **Sources:**
  - OptRouter (Han/Kahng/Lee, DAC 2015): ILP on 7×10-track × 8-layer clips, solved in
    842–1340 s with CPLEX [V];
  - MCFRoute (ICCAD 2014): −80 % DRC on 9×9 clips [S];
  - Nam/Sakallah/Rutenbar's FPGA SAT router: routable cases fast, but unroutability
    proofs up to 1442 s, and two did not finish in 24 h [V];
  - CoRe-ECO (ICCD 2021): fixed 58.6 % of the remaining violations [S].
- **Design:**
  - For each connection still unrouted after N1–N4, take a window (its bounding box
    + 1–3 mm, at most about 40×40 nodes × 2 layers).
  - Rip up to 10 crossing connections. Where they cross the window edge, they become
    terminals.
  - Build a graph at the class pitch, with nodes at pad access points. Conflict pairs
    come from the board's own clearance checks.
  - Use SAT with a conflict-limit budget (work, not time). Decode, run DRC on a scratch
    copy, and re-solve with a blocking clause on a violation (up to 5 rounds).
  - Commit only if completion improves.
- **NEW DEP:** `rustsat` + `rustsat-batsat` (MIT, pure Rust, deterministic with a
  fixed CNF order). Avoid `coin_cbc` (EPL-2.0) and C++ solvers (FMA hazards across
  platforms).
- **Gate:** after N1–N4, at least 5 residual `blocked_by` or `found` connections per
  board on 3 boards.

**N9 — Conflict-tolerant passes (negotiated congestion, Specctra style).**
- **Sources:**
  - McMurchie & Ebeling, FPGA '95 [V];
  - Specctra passes per the Pulsonix Specctra wizard: "each pass increases the conflict
    cost and in the last pass conflicts are prohibited" [V];
  - Situs "Globally Optimised Main" ignores contentions first, then reroutes with
    rising conflict cost [V];
  - Synopsys US6324675B1: a gridless router with soft overlapping wires [V];
  - Ma/Yan/Wong, ISQED 2010: negotiated congestion for PCB escape, 7/14 routed, the
    same as Allegro, and complementary to it (11/14 together) [S].
- **The difference from Java:** conflicting copper stays on the board, so no
  connection is lost because of order. Freerouting rips at once.
- **Needs:**
  - the board must hold conflicting copper (the clearance-violation items exist);
  - a per-pass conflict-cost schedule;
  - history on Q05.2's lattice;
  - a final pass that forbids conflicts, then a deterministic sacrifice.
- This is the principled fix and the industry norm, but it is L and touches the
  search's obstacle model. Gate: N1–N4 recover under half of the `found` failures.

**N10 — Soft layer pre-assignment.**
- **Source:** Liers/Nieberg/Pardella, planar max-cut via minimisation: vias −9 to
  −33 % on 4 chips, but **+20 to +35 %** on the 4 with many blockages and pin
  pre-assignments [V]. On a 2-layer SMD board every pad pre-assigns F.Cu.
- **Design:** a crossing graph over connections (ratsnest or a first pass), weighted
  max-cut with a B.Cu fanout-via penalty, solved exactly for components of at most
  60 nodes and by deterministic 1-flip local search otherwise. The result is a
  preferred-layer cost through the A04 cost hook.
- Experiment only.

**N11 — CDT feasibility, then corridors.**
- **N11.1** (S, report-only): a per-layer CDT (`spade`, Apache-2.0, exact predicates)
  over pads, keepouts and fixed copper. Edge capacities are (free length − clearances)
  / (width + clearance). For each Q06 `unrouted` connection, apply the
  Leiserson–Maley / Dai cut condition: does any homotopy path fit?
- **N11.2** (M): a negotiated planner on the CDT whose triangle corridors bias the
  maze search through the A04 cost hook (a soft cost, never a restriction). This is
  Situs's pattern and Chen/Chang ICCAD 2022's "plan as guides" [S].
- Gate: N11.1 shows at least 30 % of `unrouted` connections feasible but failed, on 3
  boards.

## 5. Looked at and not recommended

- **Full topological (rubber-band) router as a stage** (SURF, Dayan's 1997 thesis,
  TopoR, the gEDA toporouter, Salewski's RBR):
  - Every open attempt stalled at DRC-correct geometric embedding.
  - The toporouter (about 8k lines of GPL C) hangs in its CDT and was declared
    "redundant (and abandoned)" in 2015 [V, Launchpad #846789].
  - Salewski calls RBR "fully experimental … no real use" and estimates 3,000 more
    hours [V].
  - TopoR routes through violations and fixes them later, which conflicts with D8 [V].
  - Arc output does not fit the polyline model.
  - Watch item: **Topola** (codeberg.org/topola/topola, MIT, Rust, NLnet-funded) is
    an interactive topological router with an autorouter at v0.1.0 (June 2026), with
    no benchmarks [V].
- **Coarse global routing (PathFinder lattice, LP / multi-commodity flow):**
  - The failures are local.
  - OrthoRoute (PathFinder on a GPU lattice, MIT) scores 0.01–0.02 clean-pass on
    PCBWorld-Bench D2 and D3-A, against Freerouting's 1.00 and 0.80 [V, arXiv
    2607.05915].
  - LP needs floating point and adds no determinism-safe benefit at PCB scale.
- **RL/ML routers** (DeepPCB, Quilter, PcbAutoRouter, GPCB, MCTS):
  - The evidence is vendor claims only.
  - PCBWorld's PPO policy lost to Freerouting off its training distribution [V].
  - They are non-deterministic and need heavy dependencies.
- **Teardrops, via stitching and copper thieving:** KiCad does them (teardrops are
  zones, refilled at DRC) [V, KiCad 10 manual].
- **Arc corners:** low value on these boards, and they need arc clearance in the DRC
  kernel.

## 6. Benchmarks and corpus notes

- **DAC2020 bm01–bm11**: github.com/DAC-2020-Submission-1703/PCB-Benchmarks.
  - It is an anonymous companion to DAC 2020 submission 1703, with no licence file and
    no paper identified [V].
  - The only published numbers are in freerouting issue #508 [V], and the most recent
    there is upstream v2.5.0.
  - Upstream v2.5.0 (`5238f4407`) and the PCBench work (`7adef7c08`) are ancestors of
    the J01 pin `a773a70ec`. I checked this, so there is no upstream gap to port.
- **PCBWorld-Bench** (arXiv 2607.05915, LG AI; code BSD-3-Clause, engine GPL-3.0):
  - 679 KiCad 9 boards, clean-pass best of 5 [V]:

    | Router | D2 | D3-A | D3-B |
    |---|---|---|---|
    | Freerouting 2.1.0 | 1.00 | 0.80 | 0.78 |
    | KRT | 1.00 | 0.74 | 0.20 |

  - On D3-B, KRT used 11.8 vias to Freerouting's 3.48.
  - **D3-C** (6–451 nets, 2–8 layers) is released but not evaluated. It is the public
    set closest to Dan's board type, and a candidate for F03.3's corpus once its
    boards' licences are checked.
- No academic paper reports completion on the DAC2020 boards [V: none found].

## 7. Crate notes (only if a gated item is built)

| Use | Crate | Licence | Note |
|---|---|---|---|
| SAT (N8) | `rustsat` 0.7.5 + `rustsat-batsat` 0.7.5 | MIT | pure Rust; conflict limits; recommended |
| SAT alternative | `splr` 0.19 | MPL-2.0 | pure Rust, maintained |
| Min-cost flow (N7) | `ac-library-rs` 0.2 | CC0 | integer, pure Rust |
| CDT (N11) | `spade` | Apache-2.0 | exact predicates; determinism with a fixed insertion order is [I] |
| Avoid | `coin_cbc` / good_lp's default backend | EPL-2.0 (CBC) | licence |
| Avoid for exact results | HiGHS, clarabel, microlp | MIT / Apache-2.0 | f64 LP, platform-dependent |
