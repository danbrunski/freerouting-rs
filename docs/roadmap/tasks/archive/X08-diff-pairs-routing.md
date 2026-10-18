# X08 — Differential pairs: pair model, centreline, P/N offset

Opened 2026-10-15. Java Freerouting has no differential pairs (the only
mention is a comment in `gui/workspace/progress/RatsNest.java`). KRT routes a
pair as one centreline and offsets it into P and N tracks
(`docs/differential-pairs.md`; `rust_router/src/pose_router.rs`
`PoseRouter::route_pose` with a Dubins heuristic in `dubins.rs`;
`py_router/diff_pair_routing.py`, `diff_pair_loop.py`,
`diff_pair_multipoint.py`, `route_diff.py`, about 11,200 Python lines, at
`0486e799`). KRT's own docs show that the terminal escapes (setbacks, the
"hybrid" fallback, polarity) are most of that code.

In a gridless router the natural port is different from KRT's: route the
centreline through Freerouting's room/door maze search at a compound
half-width, then offset it. KRT's pose search and Dubins heuristic are
grid-bound; only the idea of a heading-aware launch at the terminals carries
over, and it is deferred. This checklist builds the pair model, the
centreline, and the offset with its DRC check. X09 adds coupled vias, the
optimizer's handling and the measurement.

Prerequisites: board parity (rung 3); K05.3 and K07.5 (pair membership, width and
gap from the `.kicad_pro` net classes; the DSN has no pair construct and no
fixture DSN contains `(pair`); A04's width-policy hook (the same one Q02
uses); A06; B09; F04.

## What exists and what this adds

| Exists | State |
|---|---|
| freerouting-dsn parser | no `pair` construct; *unverified* whether KiCad's exporter could emit one |
| K05.3 (planned) | KiCad 8+ `.kicad_pro` `net_settings.classes` with `diff_pair_width` and `diff_pair_gap` |
| KRT pair detection by name: `_P/_N`, `_t/_c`, `DP/DM/DN`, `+/-`, `Net-(…)` peeled (`net_queries.find_differential_pairs`, `extract_diff_pair_base`) | the fallback |
| KRT offset: P and N at ±(w + g)/2 from the centreline, with miter compensation at corners | the offset |
| KRT hazard: `diff_pair_multipoint.py` uses scipy `linear_sum_assignment`, whose choice among equal-cost optima is implementation-defined | not copied: lowest index wins |

## Decisions made here

- **Pairs come from K05.3's net classes.** Name inference is a fallback behind
  `router.diff_pairs.infer_from_names` (default false). Pairs are listed in
  ascending P-net number.
- **Centreline through the maze search** at compound half-width
  `w + g/2` (each track w wide, edge-to-edge gap g), with the pair's
  clearance class, using A04's width policy. Start and end points are the
  midpoints between the P and N pads of each end.
- **Offset in integer geometry:** each centreline segment offset by
  ±(w + g)/2 along its normal; consecutive offset lines intersected for the
  corners (miter), which on 45° corners keeps the gap exact.
- **Terminal legs:** short single-ended traces from each pad to its offset
  end point, routed by the Freerouting connection router; if a leg fails,
  KRT's "hybrid" fallback: the coupled middle plus single-ended legs further
  out, tried at 1, 2 and 4 mm setbacks.
- **Stage id `krt-diffpair`**, selected by `router.special.diff_pairs`,
  routing pairs before other nets in pass 1, single layer in X08 (vias are
  X09).

## File ownership

| Item | New files | May edit |
|---|---|---|
| X08.1 | `crates/freerouting-krt/src/diffpair/mod.rs`, `diffpair/model.rs` | `crates/freerouting-krt/src/lib.rs` (append) |
| X08.2 | `diffpair/centreline.rs` | — |
| X08.3 | `diffpair/offset.rs` | — |
| X08.4 | `diffpair/legs.rs` | `crates/freerouting-krt/src/lib.rs` (register line) |
| X08.5 | `fixtures/krt/x08-*.dsn`, `crates/freerouting-krt/tests/diffpair.rs` | — |

## Checklist

- [x] **X08.1 — Pair model.**
  Rust design: `diffpair::model::Pair { p: i32, n: i32, width: [i32; L],
  gap: [i32; L], class }` built from K05.3's classes, or from names when the
  fallback is on (KRT's suffix rules, applied in net-number order; a net
  matching two partners is reported and skipped).
  - **Accept:** unit tests for each naming convention; a board with
    `USB_D+`/`USB_D-` pairs them only with the fallback on; ambiguous names
    are reported.
  *Done 2026-10-17:* `crates/freerouting-krt/src/diffpair/model.rs` (`Pair { p, n, width: Vec<i32>,
  gap: Vec<i32>, class, base }` per board layer, `PairClasses::from_project(&Project)`, `name_key`
  (KRT `extract_diff_pair_base`, ported by hand without regular expressions), `pair_by_names`,
  `build_pairs(board, classes, infer_from_names)`). Class source: a K05.3 project class other than
  `Default` that sets `diff_pair_width` or `diff_pair_gap` itself is a pair class; its members
  (effective class, or a member of the aggregate) pair by name inside the class, width and gap the
  class's (missing one from `Default`, else KiCad's 0.2/0.25 mm); a member without a partner is
  reported. The project's classes don't say which net pairs with which, so the names decide inside a
  class (see *Operator decisions* in the X08 report). Fallback (`router.diff_pairs.infer_from_names`,
  new section `diff_pairs`, unset = off): every unpaired net, width = the net's trace width, gap = its
  class's trace-to-trace clearance per layer. Pairs in ascending P-net number; a key with two nets of
  one polarity is reported and skipped (KRT keeps the last). Unit tests (`cargo test -p
  freerouting-krt --lib diffpair`, 7 pass): `each_naming_convention_gives_its_key` (`_P/_N`,
  `_P0/_N0`, `P/N` after digit/`_`/capital and not after lower case, `+/-` but not `--`, `+_L`/`-_L`,
  `DP/DM/DN`, `DPLUS/DMINUS` with the letter guard, `_t/_c`, `_t_A`, `_TA`, `TARGET_C_SENSE+`,
  `Net-(…)` peeled to the leaf, `Net-(R1-Pad1)` none), `names_pair_in_net_number_order_and_ambiguity_is_reported`
  (`A_P` twice: "ambiguous names for pair A (UnderscorePn): positive 9 (A_P), 10 (A_P); negative 11
  (A_N); skipped"). Board test (`cargo test -p freerouting-krt --test diffpair`):
  `usb_d_plus_minus_pair_only_with_the_fallback_on` (`x08-connector.dsn`: no pair with the fallback
  off and no project; with it on, P = `USB_D+`, N = `USB_D-`, width 200 µm, gap 150 µm, no note;
  with a project class `DP` (pattern `USB_*`, 0.2/0.1 mm) and the fallback off, the pair comes from
  the class with gap 100 µm; a class matching only `USB_D+` reports "net 3 (USB_D+) is in pair class
  DP but has no partner by name").

- [x] **X08.2 — Centreline search.**
  - **Accept:** on `x08-straight.dsn` (one pair, open board) the centreline
    routes; its clearance to foreign items is at least `w + g/2 + c` (checked
    by the exact distance); with the stage off, `ses-diff` is identical on
    `benchmark`.
  *Done 2026-10-17:* `crates/freerouting-krt/src/diffpair/centreline.rs` (`pair_ends` with the
  lowest-index P/N end assignment, `End::{midpoint, escape, terminal}`, `route_centreline`,
  `min_foreign_slack`). On a scratch clone of the board N's items are reassigned to P, each terminal
  is a user-fixed 2-unit stub of P at the search half width, and Freerouting's `route_connection`
  routes P's control with the compound half width `hw + D/2` (`D = 2·hw + g`) plus one unit for the
  offset's rounding, one layer, no vias, no ripup, no neck-down, only the far stub as destination;
  the routed traces are chained and snapped to the terminals; the board is untouched. Test
  `the_centreline_routes_and_keeps_the_compound_clearance` (`x08-straight.dsn`, w 0.2 mm, g = c =
  0.2 mm): routes, 4 corners (detour around the 1 mm foreign pad), 45°-legal, compound half width
  300 µm = w + g/2; exact segment-to-shape distance to every foreign item minus (w + g/2) minus the
  class clearance: minimum slack 504 units (50.4 µm) ≥ 0; the board's SES unchanged. Stage off,
  sessions byte-identical: release CLI at the base `3203b9f` against this tree (`freerouting -de
  <board> -do <ses>`, default settings, `cmp`): `connection-parity` 10/10 and `benchmark`'s
  Issue433-my-board, Issue649 ecc83, Issue508 bm02, Issue690 sonde and Issue508 bm07 identical (14
  boards; the CLI doesn't register the krt stages, C07). In process, registered but unselected equals
  the built-in registry (`the_stage_is_selected_by_its_key_and_off_by_default`).

- [x] **X08.3 — Offset with miter compensation.**
  - **Accept:** unit tests: on a centreline with a 45° and a 90° corner, the
    edge-to-edge gap of the offset P and N polylines equals g within 1 board
    unit everywhere (measured at 100 points per segment).
  *Done 2026-10-17:* `crates/freerouting-krt/src/diffpair/offset.rs` (`offset_pair`): every segment
  moved by an integer vector (axis: `D/2` down and the rest; 45°: `(a, b)` axis steps with `a + b =
  k`, the two `k` adding to `ceil(D·√2)`, so the lines are never closer than `D` and less than
  `1/√2` unit farther; other directions: the rounded normal lengthened until it reaches the offset),
  corners from consecutive offset lines (miter, exact, possibly rational between two diagonals),
  ends on the normal through the centreline ends; a segment that would run backwards or a miter over
  3 offsets (KRT's cap) is refused. Unit tests (7 pass with X08.1's): `a_45_and_a_90_degree_corner_keep_the_gap`
  (east → 45° → 90° diagonal turn, and east → 90° → 45°; 100 samples per segment on each track, both
  directions: gap to the partner's corresponding segment within 1 unit of g everywhere, nearest gap
  never below g − 1), `the_inner_track_sees_the_outer_at_the_gap_and_the_miter_is_outside` (the
  inner corner sees the outer track at g; the outer miter point is `D/cos 22.5° − 2hw` from the inner
  track, within 1 unit: the miter, not a gap change), `ends_and_axis_offsets_are_integer_and_exact`,
  `a_too_short_segment_is_refused`.

- [x] **X08.4 — Terminal legs and insertion.**
  P, N and their legs are inserted in one B09 transaction and kept only if
  the exact DRC adds nothing.
  - **Accept:** on `x08-connector.dsn` (a pair between two 0.5 mm pitch
    connectors) both nets are connected, the coupled part keeps the gap, and
    there is no DRC item; a forced leg failure shows the 1, 2, 4 mm setback
    attempts in the log.
  *Done 2026-10-17:* `crates/freerouting-krt/src/diffpair/legs.rs` (`route_pair`, `PairOptions`,
  `PairStatus`) and `diffpair/mod.rs` (`KrtDiffPair`, a `BoardStage` at `StagePhase::BeforeFanout`,
  registered as `krt-diffpair`, selected by `router.special.diff_pairs`, which
  `StageSelection::resolve` appends after `router.special.stages`). `AfterFanout` was too late: the
  fanout's searches complete simple connections outright. Per attempt (direct, then 1, 2, 4 mm
  setbacks along each end's escape direction): the centreline leaves each terminal straight along the
  escape direction past the pins' pads (a heading run, the gridless stand-in for KRT's pose heading),
  is offset, and P and N go in with their joins in one `try_change(.., CheckPolicy::NewItemsOnly)`:
  first KRT's straight connectors to the pin centres, then, if the DRC refuses them, legs routed by
  Freerouting's connection router (KRT's hybrid legs); dead ends removed, the new traces of P and N
  user fixed; kept only if every pin is connected and the DRC adds nothing. Tests:
  `the_connector_pair_is_connected_with_its_gap_and_no_drc_item` (`x08-connector.dsn`: routed
  direct, `USB_D+` and `USB_D-` 0 incompletes, 0 clearance violations, the coupled tracks' gap within
  1 unit of 150 µm at 100 samples per segment, log "coupled length 20.0000 mm; 99.4 % of the pair's
  40.3000 mm of track within ±10 % of the gap 0.1500 mm"); `a_forced_leg_failure_tries_the_1_2_4_mm_setbacks`
  (`PairOptions::forced_leg_failures = 3`: the log has "direct: leg failed (forced)", "setback 1 mm:
  …", "setback 2 mm: …" and "setback 4 mm: inserted, centreline 12.0000 mm", both nets connected, 0
  violations, gap kept; with 4 forced failures the pair is not routed and the SES is the input's);
  `the_stage_is_selected_by_its_key_and_off_by_default` (through `run_pipeline`: unselected, inference
  alone, or selected without inference on a DSN, the SES equals the built-in registry's; selected with
  inference, the log has `router.special.diff_pairs=krt-diffpair` and "routed (setback 0 mm)", 0
  incompletes, 0 violations). Deviation from "Decisions made here": the routed legs are the second
  join, not the first; on both fixtures they fail to insert ("the new connection could not be
  inserted", seen with the coupled tracks unfixed and user fixed), so every committed pair here uses
  the straight connectors. Debugging the routed legs is left to X09 (see the X08 report).

- [x] **X08.5 — Determinism and the coupled-length report.**
  - **Accept:** `tools/determinism-check.sh` passes on both fixtures; the
    report gives, per pair, coupled length and the share within ±10 % of g.
  *Done 2026-10-17:* `crates/freerouting-krt/src/diffpair/report.rs` (`coupling`: the pair's copper
  on its layer, P and N, sampled at 100 points per segment, and the length within ±10 % of g of the
  partner's copper) and `DiffPairReport::log` (per pair: attempts, then "routed (setback S mm);
  coupled length L mm; X % of the pair's T mm of track within ±10 % of the gap G mm", or why not).
  `the_report_gives_coupled_length_and_gap_share_per_pair`: `x08-straight.dsn` coupled 30.8702 mm,
  99.4 % of 61.9404 mm; `x08-connector.dsn` 20.0000 mm, 99.4 % of 40.3000 mm; 0 violations.
  Determinism: `tools/determinism-check.sh --out <x08>/det -- env X08_SES_OUT={OUT} <diffpair test
  binary> write_diffpair_sessions --ignored --exact` (both boards: stage alone with 0 and 3 forced
  failures, and the full pipeline with the stage, SES and logs): `identical` (idle load 5.50, 8
  burners 6.42). Measurement: no X08 item owns one (X09 does); preregistered as
  `docs/roadmap/prereg/X08.json` (F03.4 format, set `tools/bench/sets/x08-diffpairs.txt`, harness
  `tests/diffpair.rs::measurement_records_on_a_set`, ignored), waiting for P04 (P00 "Order"). Smoke
  of the harness on the two synthetic rows (debug, pre-P04, not the registered result): (via_in_pad,
  clearance_violations, unrouted, pairs_coupled) off → on: x08-straight (0, 0, 0, 0) → (0, 0, 0, 1),
  x08-connector (0, 0, 0, 0) → (0, 0, 0, 1), coupled share 0.994 with the default optimizer on.
  Corpus smoke (release, same caveats): `mydevboard-rp2040dev` (0, 0, 34, 0) → (0, 0, 31, 1): the
  short `Net-(U1-USB_DP)`/`Net-(U1-USB_DM)` pair routed direct (coupled 2.5558 mm, 91.1 %), the
  3-pin `USB_D+`/`USB_D-` skipped (two-pin nets only); `picodvi` routed none of its 9 pairs (5
  skipped, their pins share no signal layer: vias are X09; 4 refused, the centreline at compound
  width finds no way out of the pin field). The straight-line case (a search that succeeds without
  a new trace when the terminals' stubs touch) was added for the short pair.
  *Bias note 2026-10-17:* both smokes above built the `on` arm through `TestingProfile::default()`
  (Java's fixture-test copper-to-edge clearance 0) and the `off` arm with the defaults, so the
  `on` rows were not default settings. The registered X08 run, unbiased, is in X09.5's done note.
