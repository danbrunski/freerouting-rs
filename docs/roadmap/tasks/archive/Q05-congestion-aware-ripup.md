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
| Java fork finding (HANDOVER): `start_ripup_costs` 1500 instead of 100 routes `canary-pwr` much better (154 → 132 unrouted) but adds vias elsewhere (CPU-85 +60 %) | the existing ripup knob; the owner's `freerouting.json` sets 1500 |
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

So the case for Q05 rests on more than KRT's weak evidence: on the owner's board,
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

- [x] **Q05.1 — Rip instrumentation (report only).**
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

  *Done 2026-10-17:* `quality/congestion/{mod,rip_stats}.rs` (new):
  `RipStats`, the `rip-stats` diagnostic on Q06's `Diagnostics` mechanism
  (`router.special.diagnostics=rip-stats`, registered in
  `quality/register.rs`), records per net in `BTreeMap<i32, NetRipStats>`
  rips, torn length, removed items, reroutes, pins and span (pin-centre
  bounding-box diagonal) with `size_factor = sqrt(pins)·sqrt(span_mm)`, plus
  per-pass totals; `RipReport::to_json` is `freerouting-rip-stats/1` with
  `top_nets` (top five by torn length, ties by net number, with `share`) and
  `top_share`. No A11 rip event existed, so the engine got one hook:
  `Diagnostics::before_connection` (default no-op, `autoroute/stages.rs`),
  called by the observing router (`stages/registry.rs`); a rip is the diff
  of the ripped nets' copper before and after the connection-router call
  (removed old traces minus traces the call added, which nets out the
  pull-tight's replacements; see the module docs). Emitted as
  `RunSummary::rip_stats`, under `rip_stats` in the C08 result manifest
  (`freerouting-cli/src/manifest.rs`), and in `bench` run records with
  `freerouting-dev bench --rip-stats` (the report without its per-net list).
  Tests: `cargo test -p freerouting --lib rip_stats` (5: order and ties,
  shares, null share, torn-length subtraction, size factor, JSON);
  `--test quality_rip_stats` (5): `routing_through_a_wall_rips_the_wall_net`
  (q06-sole-wall: B torn once, 18.6 mm = its whole wire, share 1.0),
  `later_passes_count_reroutes` (pass 2 reroutes B),
  `nothing_ripped_reports_no_rips`,
  `the_ses_is_byte_identical_with_the_rip_stats_on_and_off` (2 fixtures × 1
  and 3 passes, rip-stats alone and with verdicts),
  `the_report_is_deterministic`; `-p freerouting-cli --test
  result_json_rip_stats` (key present only with the diagnostic, same SES);
  `-p freerouting-dev --test bench`
  (`rip_stats_records_keep_the_report_without_the_net_list`).
  - **SES byte-identical on and off** (release CLI, `cmp`): `bm01` and
    `Issue690-sonde_xilinx` at `-mp 5 --router.optimizer.enabled=false`,
    `canary-pwr-9511d9a` with Q01.7's flags (`-mp 1 --gui.enabled=false
    --router.scoring.start_ripup_costs=1500 --router.min_trace_width_um=150`)
    and the optimizer off: off = `rip-stats` = `verdicts,rip-stats`; the
    same three boards with exactly Q01.7's flags (optimizer on): off =
    `rip-stats`; `bm02`, `bm07`, `bm10`, `Issue093-interf_u` (`-mp 5`, optimizer
    off): off = `verdicts,rip-stats`. Determinism: a second
    `verdicts,rip-stats` run on bm01, sonde and canary gave the same SES and
    equal `rip_stats` and `verdicts`; `tools/determinism-check.sh` on bm01
    (`-mp 5`, both diagnostics, SES plus both reports): `identical`. Cost
    (load average 6–10, other agents running): bm01 31.6 s off, 32.3 s with
    `rip-stats`; canary 53.7 s off, 53.7 s with it.
  - **Top five by torn length** (share of the run's torn length):
    - `Issue508-DAC2020_bm01` (`-mp 5`): 222 rips in 180 attempts, 6976.5 mm
      torn, 185 reroutes; ADC9 4.6 %, ADC12 4.1 %, ADC10 3.9 %, SCL 3.7 %,
      RXD1 3.5 %. Pass 1 tears 3788.7 mm, passes 2–5 1216.8, 843.9, 689.9,
      437.2. With Q01.7's flags (`-mp 1`): 42 rips, 1435.6 mm; ADC9 9.0 %,
      ADC12 8.6 %, ADC13 7.3 %, PH6 6.8 %, PC1 6.0 %.
    - `Issue508-DAC2020_bm10` (`-mp 5`, routes completely): 39 rips,
      1618.6 mm; /E1-A 8.6 %, /E1-BUT 8.6 %, /RF 7.2 %, /MOSI 6.7 %, /YAW
      6.5 %.
    - `Issue508-DAC2020_bm07` (complete): 13 rips, 206.2 mm; /SCK 22.2 %,
      /MOSI 21.8 %, /A7 10.9 %, /RST 9.3 %, /TXO 9.2 %.
    - `Issue508-DAC2020_bm02` (complete): one rip, RESET 13.4 mm, 100 %.
    - `Issue690-sonde_xilinx` and `Issue093-interf_u` (`-mp 5`): no rip.
    - `canary-pwr-9511d9a`: 63 rips in 45 attempts, 1618.5 mm, 28 reroutes;
      /Supervisor/BUTTON 9.2 %, BUCK_PG 7.6 %, /Supervisor/LED_STATUS 6.5 %,
      /Holdup/VCAP 6.4 % (12 pins, the largest net in the top five),
      LTC_SMBALERT 6.3 %.
    - `canary-pwr-pours1`: 69 rips, 1921.5 mm; /Supervisor/LED_BANK 10.5 %,
      +12V_PROT 6.0 % (15 pins), PRIV_SDA 5.9 %, /Holdup/LTC_INTVCC 5.8 %,
      LTC_SMBALERT 5.4 %. `pours2`: 77 rips, 1814.6 mm; PI_3V3_SENSE 9.9 %,
      BUCK_PG 7.5 %, /Holdup/LTC_CAPFB 7.5 %, LTC_SMBALERT 5.7 %,
      /Supervisor/LED_STATUS 5.4 %. `pours3`: 90 rips, 2041.0 mm;
      /Holdup/LTC_CAPFB 10.9 %, /Supervisor/LED_BANK 7.5 %,
      /Supervisor/BUTTON 6.6 %, /Holdup/VCAP 5.7 %, SWCLK_BOOT0 5.0 %.
  - **Q06 verdict counts** (same runs): bm01 15 incomplete: `blocked_by` 4,
    `unrouted` 11; sonde 11: `unrouted` 11; interf_u 19: `unrouted` 19;
    bm02, bm07, bm10: 0 incomplete; canary-pwr-9511d9a 109: `infeasible` 8,
    `blocked_by` 14, `unrouted` 87; pours1 105: 7 / 30 / 68; pours2 111: 6 /
    28 / 77; pours3 109: 0 / 20 / 89 (`infeasible` / `blocked_by` /
    `unrouted`; `work_limit` 0 everywhere).
  - **Decision rule: not met.** `blocked_by` share of the unrouted: bm01
    27 %, sonde 0 %, interf_u 0 %, canary-pwr 12.8 %, pours1 28.6 %, pours2
    25.2 %, pours3 18.3 %: none above 50 %. Top net's share of torn length:
    at most 10.9 % on every board with unrouted connections; above 25 % only
    on bm02 (one rip on a board that routes completely, 100 %); bm07 22.2 %.
    So no board with unrouted connections meets either arm, and at most one
    board meets either under the most literal reading: fewer than three.
    No large net dominates the churn (KRT's 87-pad rail case): the top nets
    are 2–15-pin nets. Q05.2–Q05.4 are not built.

- [~] **Q05.2 — Cost lattice.**
  Rust design: `quality::congestion::lattice::Lattice { origin, bin, nx, ny,
  layers, cells: Vec<Vec<u32>> }` with `add_shape(layer, &TileShape, amount)`
  (every bin whose square intersects the shape gains `amount`, saturating at
  the cap), and `cost_along(layer, &TileShape) -> u32` (the maximum bin value
  the shape touches). Pure integer geometry on the bins' integer boxes.
  - **Accept:** unit tests: a shape exactly on a bin edge touches both bins;
    the cap saturates; adding the same shapes in two different orders gives
    equal lattices.

  *Partial 2026-10-17:* not triggered: Q05.1's decision rule is not met
  (`blocked_by` at most 28.6 % of the unrouted on every board, top net at
  most 10.9 % of torn length on every board with unrouted connections; the
  numbers are in Q05.1's note). Not built. Reopen if a later Q05.1 + Q06
  run meets the rule on three boards; any build and its measurement then
  wait for P04's default-tree decision.

- [~] **Q05.3 — History and corridor costs (`history-cost`).**
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

  *Partial 2026-10-17:* not triggered: Q05.1's decision rule is not met
  (`blocked_by` at most 28.6 % of the unrouted on every board, top net at
  most 10.9 % of torn length on every board with unrouted connections; the
  numbers are in Q05.1's note). Not built. Reopen if a later Q05.1 + Q06
  run meets the rule on three boards; any build and its measurement then
  wait for P04's default-tree decision.

- [~] **Q05.4 — Victim-cost ripup (`victim-cost`).**
  Rust design: a `RipupPolicy` that multiplies an item's ripup cost by its
  net's factor `min(cap, 1 + isqrt(pins · span_µm) / d)`, all in integers
  (`isqrt` is the integer square root, so no libm call can differ across
  platforms, D6). `cap` and `d` are preregistered (defaults 4 and 1000). The
  factor is computed once per job.
  - **Accept:** unit tests of the scale; with the policy on, Q05.1's torn
    length on the top net is reported for both arms on `canary-pwr`.

  *Partial 2026-10-17:* not triggered: Q05.1's decision rule is not met
  (`blocked_by` at most 28.6 % of the unrouted on every board, top net at
  most 10.9 % of torn length on every board with unrouted connections; the
  numbers are in Q05.1's note). Not built. Reopen if a later Q05.1 + Q06
  run meets the rule on three boards; any build and its measurement then
  wait for P04's default-tree decision.

- [~] **Q05.5 — Measurement.**
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

  *Partial 2026-10-17:* nothing to measure: Q05.2–Q05.4 are not triggered
  (Q05.1's decision rule is not met), so no arm exists, no preregistration
  or `tools/bench/sets/q05-congested.txt` was written and no
  `DIVERGENCES.md` line is needed. If the rule is met later, preregister
  (F03.4) and measure after P04's default-tree decision, as the Q series
  orders. Q01.6 decides whether it builds its own lattice.
