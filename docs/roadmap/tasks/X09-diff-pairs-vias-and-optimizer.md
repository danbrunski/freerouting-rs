# X09 — Differential pairs: coupled vias, the optimizer, measurement

Opened 2026-10-15. X08 routes a pair on one layer. This checklist adds layer
changes with coupled via pairs (KRT: collinear via pairs with approach and
exit tracks, `diff_pair_via_spacing` in `pose_router.rs`; GND return vias),
keeps Freerouting's optimizer from pulling the pair apart, adds intra-pair
length matching through X07, and measures the stage.

Prerequisites: X08; O01, O06 (pull-tight and the batch optimizer); X07 (for
X09.4); F03.1, F03.4, F02.6.

## What exists and what this adds

| Exists | State |
|---|---|
| X08 `krt-diffpair` | single-layer pairs |
| Java `BatchOptimizer`, `TraceTightener*` (cda540f) | ported by O01, O06; they treat every trace independently |
| KRT: a via needs room for both P and N barrels; GND return vias at each layer change, side chosen by the pose search; polarity fixed by pad swap in the schematic (`polarity_swap.py`) | the design; the schematic swap is out of scope |

## Decisions made here

- **Via pairs** are placed perpendicular to the centreline at the layer
  change, centres `max(w + g, via diameter + via-to-via clearance)` apart,
  with the net's via padstack for each. The centreline search treats a layer
  change as a via of compound radius (half that spacing plus the via radius).
- **GND return vias** are optional (`router.diff_pairs.return_vias`, default
  off) and only for nets named in a setting; D8 applies (never in a pad).
- **Polarity:** a pair whose P and N would cross is reported, not fixed;
  KRT's fix edits the schematic, which freerouting-rs doesn't touch.
- **The optimizer skips pair traces** in v1: O06 is given an exclusion set
  (pair nets), read through a stage-to-optimizer hint that A10 passes. A
  pair-aware pull-tight is a later checklist if the measurement asks for it.

## File ownership

| Item | New files | May edit |
|---|---|---|
| X09.1 | `crates/freerouting-krt/src/diffpair/vias.rs`, `fixtures/krt/x09-*.dsn`, `crates/freerouting-krt/tests/diffpair_vias.rs` | `diffpair/centreline.rs` |
| X09.2 | — | `diffpair/legs.rs` (polarity report) |
| X09.3 | `diffpair/optimizer.rs` | — |
| X09.4 | `diffpair/skew.rs` | — |
| X09.5 | `tools/bench/sets/x09-pairs.txt`, the X09 preregistration and baseline files | `docs/DIVERGENCES.md` (append, only if kept) |

## Checklist

- [ ] **X09.1 — Coupled via pairs.**
  - **Accept:** on `x09-two-layer.dsn` (a pair forced to change layer), the
    result has exactly two vias per layer change, side by side across the
    centreline at the decided spacing, no DRC item and no via in a pad.

- [ ] **X09.2 — Polarity report.**
  - **Accept:** `x09-swapped.dsn`, whose P and N pads are swapped at one
    end, reports the pair as crossing and routes nothing for it.

- [ ] **X09.3 — Optimizer exclusion.**
  - **Accept:** after the full pipeline with the optimizer on, every pair's
    coupled share within ±10 % of g is the same as before the optimizer;
    other nets are still optimized (`ses-diff` against an optimizer-off run
    shows changes only on non-pair nets).

- [ ] **X09.4 — Intra-pair skew.**
  Uses X07's meander loop on the shorter leg near the mismatched end, with a
  tolerance `router.diff_pairs.skew_um` (default 25).
  - **Accept:** on `x09-skew.dsn`, with a 0.4 mm built-in skew, the pair
    ends within the tolerance with no DRC item.

- [ ] **X09.5 — Measurement.**
  Preregister (F03.4). Boards: at least three with differential pairs in
  their KiCad net classes (from F03.3's corpus; `x09-pairs.txt`). Arms: no
  pair stage (pairs routed as single nets) and `krt-diffpair`. Signal: the
  share of coupled length within ±10 % of the gap. Guards: single-ended
  unrouted, `drc_real`, vias in pads.
  - **Accept:** on every board, at least 90 % of coupled length within ±10 %
    of g; no new violations; single-ended completion regresses on no board.
    Otherwise the stage stays off by default and the numbers are recorded.
