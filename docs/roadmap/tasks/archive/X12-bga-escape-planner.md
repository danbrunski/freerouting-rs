# X12 — Dense-BGA escape planner

Opened 2026-10-15. Java's fanout pre-pass escapes pins one at a time. KRT has
a full BGA escape planner (`py_router/bga_fanout/`, 14,527 lines:
`__init__.py` `generate_bga_fanout`, `underpad.py`, `reroute.py`; README in
that directory; at `0486e799`): channel-based escape planning with layer
assignment and balancing, via size chosen by pitch (#158), plane-ball drops
(#424), an under-pad engine for fully populated arrays, coupled escapes for
pairs, and parts solved in their own frame. This checklist ports the core of
it as a fanout stage that runs before batch routing and emits ordinary traces
and vias: channel planning, dog-bone vias, layer balancing, plane-ball drops.
The under-pad engine and coupled pair escapes are follow-ups. It is the
largest X item and the last in the default order.

Prerequisites: board parity (rung 3); A10 (fanout stage slot); K02 (pad
arrays in the footprint frame, rotation and side); X10.1 (the part frame);
B09; F04; F03.1, F03.4.

Also read the Java fork's `docs/research/escape_congestion_research.md`
(at `main`; a plan, never implemented) for its escape-congestion estimate
and its survey of why pin-by-pin fanout fails on dense arrays; X12's channel
planner is the answer to the problem it describes.

## What exists and what this adds

| Exists | State |
|---|---|
| Java `BatchFanout` (cda540f) | the default fanout; parity |
| X10.1 `PartFrame` | board ↔ package frame |
| KRT channel planning and layer balancing; via size by pitch; plane balls dropped to the plane with their own via; mirror handling by flipping the board in memory | the design |
| KRT hazard: about 100 `set(...)` uses over ids and tuples | replaced by ordered collections |
| D8 | no via in a pad: KRT's via-in-pad option is excluded |

## Decisions made here

- **Stage id `krt-bga-escape`**, selected by `router.fanout.algorithm`;
  applies to parts whose pads form a grid of at least 6 × 6 positions in the
  package frame; other parts go to the next fanout (X10 or Freerouting's).
- **Dog-bone vias** at the centre of the square between four balls, so a via
  is never in a pad; its padstack is the smallest via in the net's via rule
  whose diameter plus clearance fits that square. If none fits, the ball is
  reported as not escapable by vias.
- **Channel capacity** between two ball rows on a layer:
  `floor((pitch − pad − c) / (w + c))` traces of width w and clearance c
  (n traces need `n·w + (n + 1)·c` of the gap), in integer units. Rings
  are escaped from the outside in; each ring's nets are assigned to the
  layer with the most remaining capacity, ties by layer index.
- **Plane balls** (nets with a plane layer) get a dog-bone via only (D8: their
  own via); signal balls get a via plus a stub to the channel.
- **Order:** components by name, rings outside in, balls in ring order
  clockwise from the package frame's top-left.

## File ownership

| Item | New files | May edit |
|---|---|---|
| X12.1 | `crates/freerouting-krt/src/fanout/bga/mod.rs`, `fanout/bga/array.rs` | `crates/freerouting-krt/src/fanout/mod.rs` (append) |
| X12.2 | `fanout/bga/channels.rs` | — |
| X12.3 | `fanout/bga/vias.rs` | — |
| X12.4 | `fanout/bga/emit.rs` | `crates/freerouting-krt/src/lib.rs` (register line) |
| X12.5 | `fixtures/krt/x12-bga-*.dsn`, `crates/freerouting-krt/tests/bga_escape.rs` | — |
| X12.6 | the X12 preregistration and baseline files | `docs/DIVERGENCES.md` (append, only if kept) |

## Checklist

- [x] **X12.1 — Array model.**
  Rust design: `fanout::bga::array::BallArray { rows, cols, pitch,
  populated: Vec<bool>, ball_of: BTreeMap<(u16, u16), PinId> }` in the
  package frame.
  - **Accept:** a full 10 × 10 array, a depopulated-centre array and a
    staggered pattern (rejected, reported) are recognised as such.
  *Done 2026-10-17:* `crates/freerouting-krt/src/fanout/bga/array.rs` (ports KRT
  `analyze_bga_grid`, `_cluster_positions` and `staggered_lattice_diagnosis` from
  `py_router/bga_fanout/grid.py` at `0486e799`: SPDX header and a `docs/PROVENANCE.md` row):
  `BallArray { component, name, frame, rows, cols, pitch, origin, populated, ball_of:
  BTreeMap<(u16, u16), ItemId>, pad_layer, field, outline }` in X10.1's part frame (row 0 the
  top, column 0 the left), `analyze_array -> Result<BallArray, Rejection>`, `ring`, `side`
  (pinwheel: each side owns the corner it starts at clockwise from the top-left),
  `ring_positions`, `escape_order` (rings outside in, clockwise from the top-left). KRT's rule
  in integer units: clusters within 1 µm at their exact mean, per axis the dominant gap
  (rounded to 10 µm, held by at least half the gaps; the pitch the exact mean of those gaps),
  every centre within 1 µm of the lattice. Arrays also need: all pins SMD on one layer, one
  pitch on both axes, at least 6 × 6 positions (the X12 decision), and each of the two outer
  rings at least half populated (DAC2020 bm01's 0.5 mm QFP-100 fits a 33 × 33 lattice).
  Staggered (adapted): fewer than half the balls have an orthogonal lattice neighbour (KRT:
  nearest neighbour > 1.4 steps and occupancy < 35 %, which lets a 50 % checkerboard through).
  The stage reports every rejected component with at least 16 pins (`EscapeReport::rejected`,
  `log`). Tests (`cargo test -p freerouting-krt --test bga_escape`, 11 passed, 3 ignored):
  `a_full_a_depopulated_and_a_staggered_array_are_recognised` (`tests/data/x12-arrays.dsn`:
  B1 10 × 10 at 0.8 mm `Full`, 100 balls; B2 the same with the centre 4 × 4 missing
  `Depopulated`, 84 balls; B3 a checkerboard on that lattice `Err(Staggered)`; B4 8 × 8 at
  0.5 mm; the QFN-24 Q1 rejected; `arrays()` finds B1, B2, B4 and reports B3 (staggered) and
  Q1), `rings_sides_and_the_escape_order_follow_the_pinwheel` (5 rings, 9 − 2k balls per side
  of ring k, the clockwise order, 100 distinct via sites). Fixtures are in
  `crates/freerouting-krt/tests/data/` as X10's are, not `fixtures/krt/` (every DSN under
  `fixtures/` needs a Java reference).

- [x] **X12.2 — Channel planning and layer balancing.**
  - **Accept:** unit tests: capacity formula on 0.8, 0.65 and 0.5 mm pitch
    with typical rules; on a 10 × 10 array with 4 signal layers, each ring's
    assignment fits capacity and the per-layer counts differ by at most one
    where capacity allows.
  *Done 2026-10-17:* `crates/freerouting-krt/src/fanout/bga/channels.rs` (written from the
  checklist; KRT's `calculate_channels`, #158 budget and `rebalance_layers` as ideas):
  `capacity(pitch, obstacle, w, c) = floor((pitch − obstacle − c) / (w + c))`, `assign` and
  `slot_offset`. Made concrete: a ball escapes along its row (W, E) or column (N, S), its
  *band*; the dog-bone vias sit on the band's edges on every layer, so the obstacle is the
  array's largest via there; on the pad layer only ring 0 escapes (balls, dog-bones and vias
  fill the field). Each demand (escape order) takes the allowed layer with a free slot in its
  band and the most remaining capacity (free slots over all bands), ties by index in the
  part's own layer order (pad layer first: ascending on the front, descending on the back).
  Slots are fixed positions across the band, the first at the via edge, outer rings first,
  so a deeper ring's jog never crosses an outer ring's trace. Tests:
  `the_capacity_formula_at_0p8_0p65_and_0p5_mm_pitch` (9 cases, e.g. 0.8 mm / 0.45 via /
  0.1 rules → 1, 0.8 / 0.3 → 2, 0.65 / 0.3 ball → 1, 0.5 / 0.25 ball / 0.1 → 0, 0.5 / 0.25 /
  0.075 → 1; each checks `n·w + (n+1)·c ≤ gap < (n+1)·w + (n+2)·c`),
  `a_10x10_array_on_4_signal_layers_balances_each_ring` (2 slots per band: all 100 placed,
  no band over capacity, slots in order, ring 0 9/9/9/9 and every ring's eligible layers
  within one of each other; 1 slot per band: ≥ 80 placed, the middle bands full, ring 0
  9/9/9/9), `slots_start_at_the_via_edge_and_stay_inside_the_band`.

- [x] **X12.3 — Dog-bone vias.**
  - **Accept:** no via overlaps any pad (checked on every ball); at 0.5 mm
    pitch with a 0.45 mm smallest via the balls are reported as not
    escapable.
  *Done 2026-10-17:* `crates/freerouting-krt/src/fanout/bga/vias.rs` (written from the
  checklist; KRT's #128 dog-bone and #158 sizing as ideas; KRT's via-in-pad fallback not
  ported, D8): the site is half a pitch out through the ball's side and half a pitch across
  (toward the nearer end of the side; middle row and column count as upper and left), so ring
  k's vias fill the square half a pitch outside it, one per position; `via_options` (the
  net's via rule, smallest radius first), `fits` (on the pad layer each populated corner
  ball's pad `distance(site) ≥ via radius + clearance + 1 unit`), `choose` (the smallest via
  that spans the pad layer and a layer the ball escapes to, and fits; else `NoViaFits`).
  Test `no_via_overlaps_a_pad_and_a_0p5_mm_array_is_not_escapable_by_vias` (`x12-arrays.dsn`):
  for every ball of B1 and B2 the 0.45 mm via is chosen and clears every pad of the board by
  its radius plus 0.1 mm; all 64 balls of the 0.5 mm B4 are `Skipped(NoViaFits)`; after the
  escape no via is in a pad and `all_clearance_violations` is empty. Note: Java's default via
  rule keeps only the smaller via of one layer span (`createDefaultViaRule`), so a DSN's
  `(via VIA450 VIA600)` gives one candidate; a class `use_via` list gives several.

- [x] **X12.4 — Emission through the board API.**
  Each ball's via and stub go in one B09 transaction; a rejected one is
  reported and skipped.
  - **Accept:** on `x12-bga-0p8.dsn` every signal ball has a via and a stub
    and every plane ball a via; no DRC item; `tools/determinism-check.sh`
    passes.
  *Done 2026-10-17:* `crates/freerouting-krt/src/fanout/bga/emit.rs` and `bga/mod.rs`: the
  `krt-bga-escape` fanout stage (`KrtBgaEscape`, registered by `freerouting_krt::register`
  through `fanout::register`, selected by `router.fanout.algorithm`). Per ball, one
  `try_change` transaction (`CheckPolicy::NewItemsOnly`, plus X10's via-in-pad, pin-side and
  connectivity refusals): the dog-bone trace (pad layer, ball centre to via), the via, and for
  a signal ball the escape on its layer: a 45° jog from the via to its slot, then straight out
  to `max(escape-box edge + c + h, outer ring's vias + 1.5 pitches)` ("a stub to the channel" made
  concrete as the escape out of the array along it). A plane ball (a conduction area of its
  net on another layer, X10's rule) gets the dog-bone and a via spanning its plane. Widths
  `min(class, neck-down)`, never below the minimum. A refused ball is `Skipped(Rejected)`.
  Tests: `every_signal_ball_escapes_and_every_plane_ball_gets_a_via`
  (`tests/data/x12-bga-0p8.dsn`: 84 escapes, 16 plane vias, every outcome exactly one via,
  signal balls 3 items, plane balls 2, all connected to their ball; escapes per layer
  F.Cu 9, In2.Cu 25, In3.Cu 25, B.Cu 25 at 1 slot per band; afterwards
  `all_clearance_violations` empty, 100 vias, none in a pad),
  `the_stage_escapes_the_bga_and_hands_the_rest_to_freerouting` (through `FanoutStage::run`),
  `the_registry_selects_krt_bga_escape_in_the_fanout_slot`,
  `two_escapes_of_the_same_board_write_the_same_session`. Determinism:
  `tools/determinism-check.sh --out <x12>/det-x12 -- env X12_SES_OUT={OUT} <bga_escape test
  binary> write_escape_sessions --ignored --exact` (the three X12 boards' SES and logs after
  the escape): `identical` (idle, then 8 burners; load 3.05 → 3.69). Checked by eye on a
  render of the SES: dog-bones on the diagonals, escapes in the row and column bands.

- [x] **X12.5 — Rotated and back-side copies.**
  - **Accept:** the same BGA at 0°, 90° and on the back side gets the same
    escape counts per layer, and the escapes map onto each other through the
    part frame.
  *Done 2026-10-17:* `rotated_and_back_side_copies_get_the_same_escapes`
  (`tests/data/x12-bga-rotated.dsn`: U1 at 0°, U2 at 90°, U3 on the back, each ball's net
  joining that ball of every copy, GND centre on the In1.Cu plane): escapes per layer in each
  copy's own order {0: 9, 1: 25, 2: 25, 3: 25} on all three (front F.Cu, In2.Cu, In3.Cu,
  B.Cu; back B.Cu, In3.Cu, In2.Cu, F.Cu), the same result per ball, and every trace corner and
  via centre mapped to its copy's part frame within 1 board unit of U1's, with equal widths and
  own-order layers; afterwards no clearance violation, 300 vias, none in a pad.

- [~] **X12.6 — Measurement.**
  Preregister (F03.4). Arms: Freerouting fanout and `krt-bga-escape`, both
  followed by the full pipeline. Boards: at least three BGA boards from
  F03.3's corpus. Signal: escape completion and final unrouted. Guards:
  `drc_real`, vias in pads.
  - **Accept:** escape completion at least the Java pre-pass's on every
    board, and unrouted improving on N−1 and regressing on none, for it to be
    kept as a selectable stage.
  *Partial 2026-10-17:* registered, not run: the measurement changes routes, so it waits for
  P04's default-tree decision (P00 README "Order"; owner P04, then this item), and F03.3's
  corpus has only two BGA boards where the pass rule needs three (owner F03.3's corpus
  manifest, an operator decision). Preregistration `docs/roadmap/prereg/X12.json` (F03.4
  format): arms `freerouting-fanout` and `krt-bga-escape`, both with `fanout.enabled = true`
  and the defaults; boards `tools/bench/sets/x12-bga.txt` (input sha256 in the registration):
  pocketsdr-v3-0 (U7 11 × 11, 121 balls, 0.8 mm) and miniscope-v4 (U6 8 × 11, 67 balls,
  0.5 mm), the only arrays `arrays_on_a_set` finds over F03.3's 30 prepared boards and the 139
  `fixtures/` boards; `x12-bga-0p8.dsn` informational. Signal: escape completion `fanned`
  (balls whose copper holds a via after the fanout stage alone; guard, higher, never below
  Freerouting's) and final `unrouted` (score); hard `via_in_pad`, `drc_real`/`drc_new`. Harness
  `escape_arms_on_a_set` (`X12_SET` or `X12_DSN`, `X12_ARMS`, `X12_SES_DIR`, `X12_FANOUT_ONLY`)
  and `arrays_on_a_set` in `crates/freerouting-krt/tests/bga_escape.rs`, since the CLI does not
  register the crate's stages before C07. **Pre-P04 dry run** (informational, not the
  registered result; this tree, release, Rust DRC only, two other agents building): fanout
  stage alone, fanned / escapable: pocketsdr Freerouting 53 / 101, `krt-bga-escape` 64 / 101
  (U7: 68 escapes, 6 no channel, 1 rejected, 20 alone, 26 already connected to the F.Cu GND
  pour); miniscope 26 / 50 both (U6: every ball `NoViaFits` at 0.5 mm, so the array goes to
  Freerouting's fanout whole); x12-bga-0p8 51 / 100 and 100 / 100. Full pipeline on pocketsdr:
  Freerouting fanout 1 unrouted, drc_new 0, via_in_pad 0, 182 vias, 165 s;
  `krt-bga-escape` 0 unrouted, drc_new 0, via_in_pad 0, 189 vias, 964 s (the batch router
  took about 8 times longer after the escapes; not investigated). **Default-off gate:** X05's
  `default_off_sessions_on_a_set` (built-in registry against `freerouting_krt::register` with
  the stage, default settings), release: `identical` on `connection-parity` (10) and
  `benchmark`'s first five (Issue433, ecc83, bm02, sonde, bm07), `cmp` 0 on all 14 distinct
  SES pairs. Found by this dry run and
  kept in the stage: handing an array with planned copper to Freerouting's fanout for its
  left-over balls ripped planned escapes up (pocketsdr fanned 64 → 43), so only arrays with no
  planned copper are handed over. Remainder: a third BGA board in F03.3's corpus, the
  registered run after P04, KiCad grading where the corpus project allows, the keep or reject
  decision, and (if kept) the `docs/DIVERGENCES.md` line; owners F03.3 (corpus) and P04, then
  X12.6.
