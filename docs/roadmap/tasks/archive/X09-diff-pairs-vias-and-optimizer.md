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

- [x] **X09.1 — Coupled via pairs.**
  - **Accept:** on `x09-two-layer.dsn` (a pair forced to change layer), the
    result has exactly two vias per layer change, side by side across the
    centreline at the decided spacing, no DRC item and no via in a pad.
  *Done 2026-10-17:* `crates/freerouting-krt/src/diffpair/vias.rs` (`via_spec`, `via_pair`,
  `via_in_pad`, `ViaSpec::compound`), `centreline.rs` (`PairEnds::end_layer` when the four pins share
  no signal layer, `route_sections` with a compound via of radius `ceil(s/2) + r + 1` added to the
  scratch board's library and via rule only), `legs.rs` (one offset per section, the via pair at each
  layer change on the bisector normal, the section ends trimmed back by `s/2` so the tracks run to the
  vias, vias inserted in the pair's B09 transaction and user fixed with the tracks; with a layer change
  the heading runs also clear the pads by the compound via's radius plus the via clearance, since
  the search may change layer right at a terminal, and the scratch board gets via keepouts over the
  pair's own pads, which the search otherwise sees as its own net). Spacing
  `s = max(D, 2r + c_vv, 2(r + hw + c_tv) − D)`; the third term (via clear of the partner's track)
  is an addition to the decided `max(w + g, via diameter + via clearance)`, see the X09 report. Fixture
  `crates/freerouting-krt/tests/data/x09-two-layer.dsn` (in `tests/data/`, not `fixtures/krt/`: every
  DSN under `fixtures/` needs a Java reference, as X08 found): X08's connector pair with J2 on the back.
  Test `cargo test -p freerouting-krt --test diffpair_vias a_layer_change_gets_one_via_pair_across_the_centreline`
  passes: routed direct, layers `[1, 0]`, one via pair, exactly two vias on the board (one per net)
  at the planned centres, their distance within `[s, s + 2]` units of `via_spec`'s spacing, both
  nets 0 incompletes, 0 clearance violations, 0 vias in pads; log "routed (setback 0 mm); coupled
  length 21.0934 mm; 94.3 % of the pair's 42.6864 mm of track within ±10 % of the gap 0.1500 mm;
  1 coupled via pair(s)". Unit tests (`cargo test -p
  freerouting-krt --lib diffpair::vias`, 3 pass): straight change perpendicular to the centreline,
  diagonal change keeps `[s, s + 3]` and symmetry, U-turn refused. GND return vias
  (`router.diff_pairs.return_vias`) not added: optional and off by default in the decisions, no
  fixture asks for them (X09 report, TODO). Determinism: `tools/determinism-check.sh --out <sp>/det --
  env X09_SES_OUT={OUT} <diffpair_vias test binary> write_x09_sessions --ignored --exact` (four X09
  boards, stage and full pipeline with the optimizer, SES and logs, on the final code): `identical`
  (idle load 3.54, 8 burners 4.14). Off by default, sessions byte-identical: release CLI at the base `f463ea8` against
  this tree (`freerouting -de <board> -do <ses>`, default settings, `cmp`): `connection-parity` and
  `benchmark`'s first five, 14 boards, 14 identical (the CLI doesn't register the krt stages, C07;
  the engine change is the new `diff_pairs.skew_um` key, read only by the stage).

- [x] **X09.2 — Polarity report.**
  - **Accept:** `x09-swapped.dsn`, whose P and N pads are swapped at one
    end, reports the pair as crossing and routes nothing for it.
  *Done 2026-10-17:* `diffpair/legs.rs` `crossing(&PairEnds)` (P's side of the direction of travel at
  the start, along the start's escape direction, against at the end, along the end's reversed: the
  side rule the offset uses, since the heading runs fix those directions), checked before any
  attempt; new `PairStatus::Crossing(reason)`; log "crossing: P is on the left of the pair at the
  start and on the right at the end, so P and N would cross; not routed by the stage (the fix is a pad
  swap in the schematic, which freerouting-rs doesn't touch)". Fixture
  `crates/freerouting-krt/tests/data/x09-swapped.dsn` (X08's connector with `USB_D+`/`USB_D-` swapped
  at J2). Test `a_swapped_pair_is_reported_as_crossing_and_not_routed` passes: status `Crossing`, no
  attempt line, the board's SES after the stage equals the input's; the unswapped connector still
  routes. In a full pipeline the passes then route the two nets single-ended.

- [~] **X09.3 — Optimizer exclusion.**
  - **Accept:** after the full pipeline with the optimizer on, every pair's
    coupled share within ±10 % of g is the same as before the optimizer;
    other nets are still optimized (`ses-diff` against an optimizer-off run
    shows changes only on non-pair nets).
  *Partial 2026-10-17:* the exclusion set is the pair's user-fixed state (every trace and via the
  stage commits for a pair is user fixed: never an O06 candidate, never ripped or shoved by another
  candidate's reroute or by the passes); `diffpair/optimizer.rs` makes it explicit (`exclusion`,
  `PairCopper`). No engine change: the checklist's "stage-to-optimizer hint that A10 passes" would be
  a new pipeline channel, needed only once pair tracks stop being user fixed (X09 report). Test
  `cargo test -p freerouting-krt --test diffpair_vias the_optimizer_leaves_the_pairs_alone_and_optimizes_the_rest`
  passes on `x08-connector`, `x09-two-layer`, `x09-skew` and `x09-optimizer` (new: the connector
  pair with net ID pre-routed as an unfixed 5 mm detour), full pipeline optimizer off vs on: coupled
  share identical (0.9940, 0.8743, 0.9729, 0.9940), the pair's copper identical, no pair item among
  O06's candidates (`sorted_candidates`) and every pair item excluded (2, 6, 2, 2), the other nets'
  items are candidates (4, 7, 9, 4); 0 violations, 0 incompletes, 0 vias in pads on every board. The
  optimizer ran on `x09-skew` and `x09-optimizer` (preflight skips the other two). Remaining: "other
  nets are still optimized" is not shown by a change: the optimizer changes no net on any of the four
  boards, and on `x09-optimizer` it rejects its one pass (`OPTIMIZER_SCORE_NOT_IMPROVED`) with the
  pair stage and without it alike, so the ID detour stays. A board on which O06 changes a non-pair
  net (X09.5's corpus rows, once they run) closes it.

- [x] **X09.4 — Intra-pair skew.**
  Uses X07's meander loop on the shorter leg near the mismatched end, with a
  tolerance `router.diff_pairs.skew_um` (default 25).
  - **Accept:** on `x09-skew.dsn`, with a 0.4 mm built-in skew, the pair
    ends within the tolerance with no DRC item.
  *Done 2026-10-17:* `diffpair/skew.rs` (`compensate_skew`, `DEFAULT_SKEW_UM = 25`): X07's bump
  geometry (`meander::bump_points`, `amplitude_for`, X07's pitch rule and 1 mm amplitude cap) on the
  shorter net's straight runs from the mismatched end inwards (the end whose pins are staggered more
  along its escape direction), the first bump past that end's pads, on the side away from the
  partner, 45° bumps where the board allows, square ones for a skew too small for them; applied to
  the planned tracks before the pair's transaction, so the DRC judges the bumps with the pair. Not
  X07's `Matcher` loop itself: it lengthens only unfixed traces and picks the longest run, not the
  end. New setting `router.diff_pairs.skew_um` (engine `settings/fields.rs`, unset = 25). Fixture
  `crates/freerouting-krt/tests/data/x09-skew.dsn` (the connector pair with J2 turned by 90°, width
  0.1 mm, gap 0.141 mm). Test `the_shorter_track_is_bumped_to_within_the_skew_tolerance` passes:
  built-in skew 399.3 µm (X07's `measure_net`, asserted in [380, 420]); with the stage as a job builds
  it (`skew_um` unset → 25): 1 bump on `USB_D+`, skew after 0.14 µm, both nets connected, 0 clearance
  violations, log "skew 0.3993 mm → 0.0001 mm with 1 bump(s) on net 3", coupled share 97.3 % (99.0 %
  without the bump); a tolerance above the built-in skew leaves the pair alone.

- [~] **X09.5 — Measurement.**
  Preregister (F03.4). Boards: at least three with differential pairs in
  their KiCad net classes (from F03.3's corpus; `x09-pairs.txt`). Arms: no
  pair stage (pairs routed as single nets) and `krt-diffpair`. Signal: the
  share of coupled length within ±10 % of the gap. Guards: single-ended
  unrouted, `drc_real`, vias in pads.
  - **Accept:** on every board, at least 90 % of coupled length within ±10 %
    of g; no new violations; single-ended completion regresses on no board.
    Otherwise the stage stays off by default and the numbers are recorded.
  *Partial 2026-10-17:* preregistered as `docs/roadmap/prereg/X09.json` (F03.4 format; set
  `tools/bench/sets/x09-pairs.txt`: three synthetic boards and X08's three corpus boards, sha256s
  recorded; harness `crates/freerouting-krt/tests/diffpair_vias.rs::measurement_records_on_a_set`,
  ignored; metrics `via_in_pad` hard, `clearance_violations` and `unrouted_single_ended` guards,
  `coupled_share` score). The registered run waits for P04's default-tree decision (P00 "Order"),
  and `drc_real` waits for KiCad grading of the corpus rows (F02.6, C07 registering the stage in the
  CLI). Pre-P04 smoke, not the registered result (`X09_SET=<corpus rows> cargo test --release -p
  freerouting-krt --test diffpair_vias measurement_records_on_a_set -- --ignored --exact`, 709 s;
  synthetic rows in debug), (`coupled_share`, `unrouted_single_ended`, `clearance_violations`,
  `via_in_pad`) off → on: `x08-connector` (0, 0, 0, 0) → (0.9940, 0, 0, 0); `x09-two-layer`
  (0, 0, 0, 0) → (0.9285, 0, 0, 0); `x09-skew` (0, 0, 0, 0) → (0.9729, 0, 0, 0); `picodvi`
  (0, 3, 0, 0) → (0.0410, **4**, 0, 0): 10 pairs, 5 reported crossing (X09.2; they were X08's
  "no common signal layer" pairs), 4 centreline searches failed, 1 routed (72.9 %); `pcie-active-backplane`
  (0, 0, 0, 0) → (0.2858, 0, 0, 0): 33 pairs, 16 routed with coupled vias (7 direct at 83.8–89.3 %,
  9 at the 4 mm setback at 0–18.9 %: the straight connectors from the pins to terminals 4 mm out do
  not couple), 7 refused by the DRC, 1 skipped (4 P pins); `mydevboard-rp2040dev` (0, 33, 0, 0) →
  (0.9111, 31, 0, 0). Against the accept: ≥ 90 % holds on `x08-connector`, `x09-two-layer`,
  `x09-skew` and `rp2040dev`, fails on `picodvi` and `pcie`; no new violation and no via in a pad
  anywhere; single-ended completion regresses on `picodvi` (3 → 4). So the stage stays off by
  default (it is). Remaining: the registered run after P04, and KiCad grading.
