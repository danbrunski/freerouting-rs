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

- [ ] **Q07.1 — Conflict graph.**
  Rust design: `quality::ordering::conflicts::ConflictGraph::build(board,
  tasks) -> ConflictGraph` with, per net, its MST segments and the set of
  layers its pins are on (through-hole pins: all layers); two nets conflict
  if a segment of one properly crosses a segment of the other and their layer
  sets intersect. Adjacency as sorted `Vec<u32>` per net.
  - **Accept:** unit tests: two crossing two-pin nets on one layer conflict;
    the same on disjoint layers don't; segments sharing an end point don't;
    collinear overlap counts as no crossing (documented).

- [ ] **Q07.2 — Greedy conflict rounds (`conflict-rounds`).**
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

- [ ] **Q07.3 — Inside-out ordering (`inside-out`).**
  Source: KRT `order_nets_inside_out` (nets with pins inside a dense part's
  pin field, nearest the part centre first; the rest in input order).
  A dense part is a component with at least 16 pins whose smallest pin pitch
  is at most 0.65 mm (KRT's fine-pitch threshold). Distance is from the net's
  nearest pin to the part's pin bounding-box centre, in `i64` squared units;
  ties by net number.
  - **Accept:** unit tests on a synthetic QFN; `ses-diff` identical with the
    default.

- [ ] **Q07.4 — Measurement.**
  Preregister (F03.4). Arms: Java order, `conflict-rounds`, `inside-out`.
  Boards: `benchmark` plus `canary-pwr` and its variants. Signal: unrouted
  after the full run, and after pass 1 (reported, not judged). Guards: vias,
  length, `drc_real`.
  - **Accept:** measured. An arm is kept only if it improves on N−1 boards
    and regresses on none; otherwise its code is removed and the numbers stay
    here.
