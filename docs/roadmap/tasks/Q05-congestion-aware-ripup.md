# Q05 — Congestion-aware ripup

Opened 2026-10-15. The Java fork's HANDOVER names smarter ripup between passes
(history costs in the style of PathFinder negotiated congestion) as the only
idea likely to raise completion on dense boards. KRT has the closest
implementation (`py_router/history_congestion.py`, #590, at `0486e799`), and
its evidence is weak:
- −47 unrouted nets on corpus sets 1–10 (20 boards better, 11 worse,
  p = 0.075), nothing on sets 11–20, −15 on sets 21–27 with two boards
  supplying 113 % of the gain;
- a preregistered confirmation on sets 21–27 failed its own thresholds
  (≥ 3 % and p < 0.05); KRT shipped it on the congested-corpus result;
- escalation (doubling per repeat) walled off fine-pitch BGA escape fields,
  so it shipped flat;
- churn is dominated by large nets: on one board an 87-pad rail was ripped
  47–67 times and was 40 % of all torn copper.

So this checklist instruments first, and builds costs only if the instrument
and Q06 show congestion. Every cost is an experiment behind a setting.

Prerequisites: board parity (rung 3); A04 (a cost hook in maze expansion,
zero and not consulted in the parity path); A11 (rip events from the pass
loop); A10 (`RipupPolicy` trait); Q06 (blocker attribution); B01 (snapshots);
F03.1, F03.4.

**Fixtures:** `fixtures/canary-pwr/canary-pwr-pours1..3.dsn`, the variant set, with Java sessions at `a773a70ec`; pass 1 is the best pass on all of them.

## What exists and what this adds

| Exists | State |
|---|---|
| Java ripup inside the maze search: a foreign trace can be crossed at a ripup cost `start_ripup_costs × pass` (fanout `× (pass + 1)`); ripped connections become incomplete for the next pass (`MazeRipupResolver`, `AutorouteControl`, cda540f) | ported by A04 and A10's default `RipupPolicy` |
| Java fork finding (HANDOVER): `start_ripup_costs` 1500 instead of 100 routes `canary-pwr` much better (154 → 132 unrouted) but adds vias elsewhere (CPU-85 +60 %) | the existing ripup knob; Dan's `freerouting.json` sets 1500 |
| A04 cost hook `extra_cost(layer, door or room)` (requested) | where history is read |
| KRT `HistoryField` (sorted parallel arrays, lexsort eviction), charges: contest (frontier ∩ routed copper) at full increment, rip footprint weight 1.0, raw failed frontier 0.25; shipped cost 0.1 mm-equivalent, cap 0.5, no escalation, no decay | the design |
| KRT `obstacle_costs.compute_ripped_route_costs`: +0.1 mm-equivalent within 1 mm of a ripped net's former copper until it reroutes | the corridor cost |
| KRT `net_cost.py`: `sqrt(pads)·sqrt(span)`; P(ripped again \| ripped k times) ≈ 0.9 for every k | the victim cost; KRT never turned it on by default |

**Measured in Java (the Java session, 2026-10-15, `a773a70ec`, deterministic,
`canary-pwr` "pours1"):**
- **Pass 1 is the best pass at every ripup cost.** With `start_ripup_costs`
  100, 500 and 1500, pass 1 leaves 120, 109 and 100 connections unrouted.
  Every later pass is worse (161–185), and the loop's periodic restore puts the
  pass-1 board back.
- **The later passes are wasted time.** `-mp 1` gives the same SES as
  `-mp 10`, so passes 2–10 are about 60 % of the routing time for nothing.
- **Cause:** Java's ripup is greedy. Ripped blockers often can't be rerouted,
  and nothing remembers congestion between passes.

So the case for Q05 rests on more than KRT's weak evidence: on Dan's board,
Java's memoryless greedy ripup makes every pass after the first worse. That
makes history costs (negotiated congestion) the most promising divergence for
completion. It is still measured under the Q rules, starting from a pass-1
board that matches Java.

## Decisions made here

- **Instrument first (Q05.1).** The decision to build Q05.2–Q05.4 is
  preregistered: build them only if, on at least three boards, more than half
  of the unrouted connections are `blocked_by` routed nets (Q06), or one net
  accounts for more than 25 % of torn length (Q05.1).
- **History lives on a lattice, not on rooms.** Freerouting rebuilds rooms
  between passes. The lattice has a fixed origin at board (0, 0), a bin size
  of 0.5 mm in board units (preregistered), one dense `Vec<u32>` per layer
  over the board's bounding box, fixed-point costs in 1/1024 of the via cost
  unit. Integer accumulation makes summation order irrelevant. No eviction.
- **Scope: one batch run.** History survives board restores within the run:
  it is about where failures happen, not about board items. It is keyed by
  location, so A10's "reset state keyed by `ItemId`" rule doesn't apply.
- **Flat increment with a cap, no escalation, no decay** (KRT's measured
  choice).
- **The policy** is an A10 `RipupPolicy` implementation, id `history-cost`,
  selected by `router.autorouter.ripup_policy`, plus a `CostHook` for the maze
  search. The victim cost (Q05.4) is a separate id, `victim-cost`, so the two
  can be measured apart.

## File ownership

| Item | New files | May edit |
|---|---|---|
| Q05.1 | `crates/freerouting/src/quality/congestion/mod.rs`, `quality/congestion/rip_stats.rs` | `quality/mod.rs` (append) |
| Q05.2 | `quality/congestion/lattice.rs` | — |
| Q05.3 | `quality/congestion/history.rs`, `fixtures/quality/q05-bottleneck.dsn`, `crates/freerouting/tests/quality_history.rs` | `quality/register.rs` (append) |
| Q05.4 | `quality/congestion/victim.rs` | `quality/register.rs` (append) |
| Q05.5 | `tools/bench/sets/q05-congested.txt`, the Q05 preregistration and baseline files | `docs/DIVERGENCES.md` (append, only if kept) |

## Checklist

- [ ] **Q05.1 — Rip instrumentation (report only).**
  Rust design: `quality::congestion::rip_stats::RipStats` subscribes to A11's
  rip events and records per net, in a `BTreeMap<i32, NetRipStats>`: rips,
  torn length (sum of ripped trace lengths), reroutes, and
  `sqrt(pins)·sqrt(span)` (span = the diagonal of the net's pin bounding box,
  in mm). Emitted in the C08 result JSON and in `bench` records, with the top
  five nets by torn length and their shares.
  - **Accept:**
    - the SES is byte-identical with the instrument on and off;
    - on `canary-pwr` and two `benchmark` boards the done note lists the top
      five nets by torn length with their shares, and the Q06 verdict counts;
    - the decision rule above is evaluated and its outcome recorded.

- [ ] **Q05.2 — Cost lattice.**
  Rust design: `quality::congestion::lattice::Lattice { origin, bin, nx, ny,
  layers, cells: Vec<Vec<u32>> }` with `add_shape(layer, &TileShape, amount)`
  (every bin whose square intersects the shape gains `amount`, saturating at
  the cap), and `cost_along(layer, &TileShape) -> u32` (the maximum bin value
  the shape touches). Pure integer geometry on the bins' integer boxes.
  - **Accept:** unit tests: a shape exactly on a bin edge touches both bins;
    the cap saturates; adding the same shapes in two different orders gives
    equal lattices.

- [ ] **Q05.3 — History and corridor costs (`history-cost`).**
  Source: KRT `history_congestion.py` (three charges, flat, capped);
  `obstacle_costs.compute_ripped_route_costs` (corridor).
  Rust design, `quality::congestion::history`:
  - after each pass, in this order: charge each ripped item's shapes at
    weight 1.0; charge the blocking items of each failed connection (Q06's
    attribution) at weight 1.0; nothing for raw frontier unless the
    preregistration adds the 0.25 arm;
  - the corridor: ripped items' shapes enlarged by 1 mm cost +0.1 mm
    equivalent until their net reroutes; kept in a separate lattice that is
    rebuilt each pass from the open rips;
  - the `CostHook` adds `history + corridor` along the door's shape to the
    expansion cost, converted to the maze search's cost unit.
  - **Orders that decide results:** charges in ascending item id within each
    kind; the hook is a pure function of the lattice and the shape.
  - **Accept:**
    - with the policy off, `ses-diff` shows identical sessions on
      `benchmark`;
    - on `q05-bottleneck.dsn` (two nets competing for one corridor with a
      longer detour available), the history arm routes both, and the
      default arm's result is recorded beside it;
    - `tools/determinism-check.sh` passes on `canary-pwr`.

- [ ] **Q05.4 — Victim-cost ripup (`victim-cost`).**
  Rust design: a `RipupPolicy` that multiplies an item's ripup cost by its
  net's factor `min(cap, 1 + isqrt(pins · span_µm) / d)`, all in integers
  (`isqrt` is the integer square root, so no libm call can differ across
  platforms, D6). `cap` and `d` are preregistered (defaults 4 and 1000). The
  factor is computed once per job.
  - **Accept:** unit tests of the scale; with the policy on, Q05.1's torn
    length on the top net is reported for both arms on `canary-pwr`.

- [ ] **Q05.5 — Measurement.**
  Preregister (F03.4) with the prior stated: KRT's result was not
  significant. Arms: default; `history-cost`; `history-cost` with the corridor
  off; `victim-cost`. Boards: those with at least 10 unrouted connections in
  the default arm, at least three, plus `canary-pwr` and its variants
  (`tools/bench/sets/q05-congested.txt`). Signal: unrouted. Guards: vias,
  `drc_real`, time.
  - **Accept:** measured. An arm is kept only if it improves on N−1 boards
    and regresses on none. Each rejected arm's code is removed and its numbers
    stay in this done note. If every arm is rejected, Q05.2's lattice stays
    only if Q01.6 uses it.
