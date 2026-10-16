# Q02 — Neck-down through pin fields

Opened 2026-10-15. On `canary-pwr`, 1.5 mm Power_High and Switch_Node traces
can't reach fine-pitch pins: Java narrows only inside the expansion room next
to the pin, so the trace must cross the neighbouring pins at full width. The
Java fork's attempt (retry the whole connection narrow, widen back where it
fits) didn't help and left 40–60 mm of 0.4 mm power trace. KRT has the same
weakness and handles it by reporting the shipped width, not bounding it.

This checklist adds, in order:
1. a report of the width each wide net actually got;
2. a rule that the engine never ratchets widths down;
3. a neck bounded by distance from the pin, inside the search;
4. a piecewise widen-back with a stepped taper;
5. the measurement against Java and the shelved variants.

Prerequisites: board parity (rung 3); A04 and A06 (a width-policy hook in the
maze search and per-segment widths in insertion; see "Decisions"); B05
(`Pin` neck-down half-width); B04 (trace shape checks); O01 (pull-tight);
Q03.5 (`neck_needed`, used to choose D); F03.1, F03.4, F02.6.

## What exists and what this adds

| Exists | State |
|---|---|
| Java `MazeSearchEngine` at cda540f, around line 456 (`ctrl.withNeckdown` and a `TargetItemExpansionDoor` on a start pin) and `checkNeckDownAtDestPin` (around line 1229) | room-local neck-down to the pin's width (`automatic_neckdown`, on by default); ported by A04 |
| Java `FoundConnectionInserter` lines 495–583 (`getTraceNeckdownHalfwidth` at start and end pins) | the inserted end segments get the neck width; ported by A06 |
| Java `Pin.getTraceNeckdownHalfwidth` (`Pin.java:586`) | `max(0.5·getMinWidth(layer) − 1, 1)`; ported by B05 |
| Java `AutorouteConnectionRouter.retryConnectionNecked` (lines 193–272) and `RouterSettings.neckWidthUm` (`neck_width_um`, default `DefaultSettings.DEFAULT_NECK_WIDTH_UM = 0.0`) | opt-in: after a FAILED or INSERT_ERROR result, retry once with every half-width clamped to the neck width, then `optChangedArea`; ported by A (the Freerouting neck-down stage) |
| Java fork `docs/development/shelved/neckdown/` (main): `neckdown-widening.patch` against `de7a4fd2c`, `NeckdownWideningTest.java`, two fixtures | the shelved widen-back; measured below |
| Rust | nothing yet: no neck-down code at HEAD |
| KRT `single_ended_routing.py`: `_route_main_connection`, `_neck_pass` (2.5 mm of path from the pad), `_widen_fitting_pieces` (0.5 mm pieces), `_apply_neckdown_widths` (4-step taper over 0.5 mm); `routing_common.power_width_report`; `fab_tiers.note_narrowing` | the report, the ratchet fix and the widen-back |

The Java fork's measurements on `canary-pwr` (HANDOVER, 3 passes, baseline 88
unrouted, 400 µm unless noted): necked retry only 91; plus widening 83 with
40–60 mm of 0.4 mm trace; plus a gate accepting only routes narrow within 2 mm
of a pad 95 (102 at 500 µm); plus no ripup for the retry 91 (98 at 500 µm).

## Decisions made here

- **Bound the neck in the search, not by a gate on the result.** The gate
  variant measured 95/102. The new width policy lets the maze search use the
  neck width only in rooms within distance D of a start or destination pin,
  and the class width everywhere else.
- **D** defaults to the pin's component courtyard (its package outline
  bounding box, when the DSN gives one) plus one pin pitch on that
  component; with no outline, the pin's own bounding box plus one pitch. The
  neck width is `max(2·trace_neckdown_half_width(layer), class minimum,
  board minimum track width)`. The class minimum is the narrowest width any
  net class on the board allows on that layer. The board minimum is the
  board's `min_track_width`, from the `.kicad_pcb`/`.kicad_pro` reader (K05.3,
  K07.5), or `router.min_trace_width_um` (C02; Java fork `46bf9e2ef`, opt-in) when
  the input is a DSN, which doesn't carry it. Never
  lower (D8: the board's design rules are hard limits). Freerouting at the
  pin necks below it: on StickHub in the head-to-head, 147 tracks were
  0.112 mm against a 0.15 mm minimum. The preregistration also carries KRT's 2.5 mm path
  length as an arm.
- **Hooks needed from the A series.** A04's maze search takes a width policy:
  `fn half_width(&self, room: &RoomShape, layer: i32) -> i32`, with the
  Freerouting policy reproducing Java's room-local rule exactly. A06's
  inserter accepts a half-width per path segment. Q02 implements a second
  policy; it doesn't edit A files.
- **The stage.** Q02's bounded neck is an alternative implementation of the
  A10 neck-down stage, id `bounded-neck`, selected by
  `router.special.neck_down`. Like Java's retry it runs only after the
  class-width attempt fails.
- **Widen-back is an optimizer post-pass** that only widens pieces where the
  exact clearance check passes, so it can't add violations.
- **Integer geometry.** Pieces are cut at exact integer fractions of a
  segment in board units, from the pin end. Tapers use integer half-widths.

## File ownership

| Item | New files | May edit |
|---|---|---|
| Q02.1 | `crates/freerouting/src/quality/neckdown/mod.rs`, `quality/neckdown/width_report.rs`, `tools/quality/ses_widths.py` | `quality/mod.rs` (append) |
| Q02.2 | `quality/neckdown/ledger.rs`, `crates/freerouting/tests/quality_no_ratchet.rs` | the C02 settings struct (one field) |
| Q02.3 | `quality/neckdown/bounded.rs` | `quality/register.rs` (append) |
| Q02.4 | `fixtures/quality/q02-qfn-power.dsn`, `crates/freerouting/tests/quality_neckdown.rs` | — |
| Q02.5 | `quality/neckdown/widen_back.rs` | `crates/freerouting/tests/quality_neckdown.rs` |
| Q02.6 | `tools/bench/sets/q02-wide-nets.txt`, the Q02 preregistration and baseline files | `docs/DIVERGENCES.md` (append, only if kept) |

## Checklist

- [ ] **Q02.1 — Delivered-width report (report only).**
  Source: KRT `routing_common.power_width_report` (requested_mm, length_mm,
  under_mm, under_share, min_mm, by_width_mm; taper steps count as narrow).
  Rust design: `quality::neckdown::width_report::report(board) ->
  Vec<NetWidthReport>` for each net whose class width on some layer is above
  the board's narrowest class width, in ascending net number: requested width
  per layer, total length, length under the requested width and its share,
  minimum width, and length per distinct width (sorted by width). Emitted in
  the C08 result JSON and in `bench` records.
  - **Accept:**
    - on a fixture with one forced 0.4 mm pinch in a 1.5 mm net, the
      under-width length matches `tools/quality/ses_widths.py` (an independent
      SES parser) within 0.01 mm;
    - the SES is byte-identical with the report on and off;
    - the report on Java's `canary-pwr` session (imported) is in the done
      note, as the baseline for Q02.6.

- [ ] **Q02.2 — No width ratchet, and a narrowing ledger.**
  Source: KRT #842 (a write-back made a 0.127 mm neck the board's width on the
  next run), `fab_tiers.note_narrowing`, `--strict-sizes` (exit 3).
  Rust design: `quality::neckdown::ledger::NarrowingLedger` records every
  narrowing (net, site item id, requested, delivered, stage id) in insertion
  order; it goes into the result JSON. Setting `router.neck_down.strict`
  (default false): the neck-down stages refuse to narrow, and the run exits
  non-zero when any shipped copper is under its requested width. The ratchet
  test covers the writers owned by C05/C06: it doesn't change them.
  - **Accept:**
    - routing a fixture, writing DSN (C06) and SES (C05), re-reading and
      routing again leaves every net class width unchanged;
    - strict mode exits non-zero exactly when Q02.1 reports under-width
      length, on one fixture where it does and one where it doesn't.

- [ ] **Q02.3 — Distance-bounded neck policy.**
  Rust design: `quality::neckdown::bounded::BoundedNeck` implements the A10
  neck-down trait. On a failed class-width connection it reruns the
  Freerouting maze search with a width policy: for a room whose shape lies
  within D of a start or destination pin of the connection, the neck
  half-width; elsewhere the class half-width. The distance test uses the
  room shape's bounding box against the pin's bounding box enlarged by D, in
  integer board units. It then inserts through A06 with per-segment widths.
  - **Orders that decide results:** the retry happens at the same point as
    Java's `retryConnectionNecked` (after FAILED or INSERT_ERROR), with the
    same ripup cost; pins are taken in the connection's start-set and
    destination-set order.
  - **Accept:**
    - unit tests of the room predicate at D − 1, D and D + 1 units;
    - with `router.special.neck_down` at its default, `ses-diff` shows
      identical sessions on `benchmark`.

- [ ] **Q02.4 — Bounded-neck fixture.**
  `q02-qfn-power.dsn`: a 0.5 mm pitch QFN with one 1.5 mm power pin, open
  board beyond the pin field.
  - **Accept:**
    - with `bounded-neck`, the connection routes; a test walking the SES
      finds no stretch narrower than the class width further than D plus the
      taper length from the pin;
    - without it, Java's default leaves it unrouted (Q03 says `infeasible`);
    - no new clearance violation and no via in a pad;
    - `tools/determinism-check.sh` passes.

- [ ] **Q02.5 — Piecewise widen-back with taper.**
  Source: KRT `_widen_fitting_pieces` (≈0.5 mm pieces, each tested with the
  search's own fit test), `_apply_neckdown_widths` (taper at widths
  `wide + (narrow − wide)·k/5`, k = 1..4, over 0.5 mm), and the rule that a
  wide island no longer than two tapers goes back to narrow.
  Rust design: `quality::neckdown::widen_back::widen(board, ledger)` runs
  after the connection is inserted and before pull-tight. For each narrowed
  trace in ledger order: cut into pieces of at most 0.5 mm from the pin end;
  try the class width on each piece with the exact clearance check (B04
  `check_trace_shape` on a copy, or B09's insert → DRC → revert transaction);
  merge accepted pieces; add the 4-step taper at each narrow → wide junction
  on the same layer; drop wide islands no longer than two tapers.
  - **Accept:**
    - a 22 mm run with one 0.4 mm pinch is narrow only at the pinch and its
      tapers (KRT's #1033 case);
    - no new DRC items on any test fixture;
    - pull-tight (O01) afterwards doesn't remove the tapers on the fixture.

- [ ] **Q02.6 — Measurement.**
  Preregister (F03.4). Arms:
  - `java-default` (room-local neck-down only);
  - `java-retry-400`: `router.neck_width_um = 400`;
  - `bounded-neck` with D = courtyard plus one pitch;
  - `bounded-neck-2.5mm` (KRT's constant as path length from the pin).
  Boards: `canary-pwr` and its variants (the 88-unrouted baseline is from the
  Java fork at `de7a4fd2c` and 3 passes; re-measure it in the same run), plus
  at least three boards where a net class is wider than the narrowest pad it
  reaches (`tools/bench/sets/q02-wide-nets.txt`). Signal: unrouted. Guards:
  `drc_real`, vias in pads, and Q02.1's under-width length.
  - **Accept:** measured. `bounded-neck` is kept only if it beats
    `java-default` on N−1 boards and loses on none, **and** its median
    under-width length per net is below the shelved variant's 40 mm. The done
    note gives all arms per board. If not kept, Q02.3–Q02.5's code is removed;
    Q02.1 and Q02.2 stay.
