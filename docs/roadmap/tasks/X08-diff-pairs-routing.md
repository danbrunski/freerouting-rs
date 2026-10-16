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

- [ ] **X08.1 — Pair model.**
  Rust design: `diffpair::model::Pair { p: i32, n: i32, width: [i32; L],
  gap: [i32; L], class }` built from K05.3's classes, or from names when the
  fallback is on (KRT's suffix rules, applied in net-number order; a net
  matching two partners is reported and skipped).
  - **Accept:** unit tests for each naming convention; a board with
    `USB_D+`/`USB_D-` pairs them only with the fallback on; ambiguous names
    are reported.

- [ ] **X08.2 — Centreline search.**
  - **Accept:** on `x08-straight.dsn` (one pair, open board) the centreline
    routes; its clearance to foreign items is at least `w + g/2 + c` (checked
    by the exact distance); with the stage off, `ses-diff` is identical on
    `benchmark`.

- [ ] **X08.3 — Offset with miter compensation.**
  - **Accept:** unit tests: on a centreline with a 45° and a 90° corner, the
    edge-to-edge gap of the offset P and N polylines equals g within 1 board
    unit everywhere (measured at 100 points per segment).

- [ ] **X08.4 — Terminal legs and insertion.**
  P, N and their legs are inserted in one B09 transaction and kept only if
  the exact DRC adds nothing.
  - **Accept:** on `x08-connector.dsn` (a pair between two 0.5 mm pitch
    connectors) both nets are connected, the coupled part keeps the gap, and
    there is no DRC item; a forced leg failure shows the 1, 2, 4 mm setback
    attempts in the log.

- [ ] **X08.5 — Determinism and the coupled-length report.**
  - **Accept:** `tools/determinism-check.sh` passes on both fixtures; the
    report gives, per pair, coupled length and the share within ±10 % of g.
