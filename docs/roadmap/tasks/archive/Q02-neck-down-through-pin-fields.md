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

- [x] **Q02.1 — Delivered-width report (report only).**
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
  *Done 2026-10-17:* `quality/neckdown/width_report.rs`: `report(board) ->
  WidthReport` (one `NetWidthReport` per net whose class width on some
  signal layer is above the narrowest class width of the board's nets,
  ascending net number: requested width per layer, length, under-width
  length and share, minimum width, length per width ascending; lengths
  summed in ascending item id; system-fixed traces, which the SES writer
  leaves out, are not delivered copper). The `widths` diagnostic
  (`quality/neckdown/mod.rs`, `router.special.diagnostics=widths`)
  puts it with Q02.2's ledger in `RunSummary::widths`; the CLI result JSON
  carries it as `widths` (`freerouting-widths/1`, after `rip_stats`), and
  `freerouting-dev bench` records carry it per run (`widths`, from the SES
  imported into the board); `freerouting-dev widths board.dsn [ses]`
  prints it for any session. `tools/quality/ses_widths.py` is the
  independent SES/DSN parser.
  - Pinch: fixture `crates/freerouting/tests/data/quality/q02-pinch.dsn`
    (PWR, Power_High 1.5 mm, pre-routed with a 2 mm stretch at 0.4 mm):
    `tests/quality_widths.rs::the_forced_pinch_is_the_under_width_length`
    gets 2.0000 mm under on the board and from `ses_widths.py` on the
    written SES, and every wide net's under and total length agree within
    0.01 mm; `the_router_made_neck_at_a_small_pad_is_in_the_ledger` does
    the same for the router's own neck (PWR2 into a 0.25 mm pad between
    two foreign pads at 0.65 mm pitch: 1.3502 mm at 0.2498 mm).
  - SES byte-identical with the report on and off:
    `the_ses_is_byte_identical_with_the_report_on_and_off` (q02-pinch,
    q03-qfn-0p5), `crates/freerouting-cli/tests/result_json_widths.rs::
    the_manifest_carries_the_widths_only_with_the_diagnostic`, and the
    release CLI with and without `--router.special.diagnostics=widths`:
    `Issue093-interf_u` (`-mp 100`, optimizer off) SES sha256
    `3bcdcfb0686cec17…` both, `Issue555-CNH_Functional_Tester_1` (24 wide
    nets, `-mp 100`) `19ac64ee56af91ae…` both, `canary-pwr-9511d9a` with
    the Q01.7 flags (`-mp 1 --gui.enabled=false
    --router.scoring.start_ripup_costs=1500
    --router.min_trace_width_um=150`) `502226b0a0e54bd3…` both (57 s and
    58 s). That canary run's own report: 30 wide nets, 12 under width,
    45.5017 mm under, 45 narrowings (28 `freerouting-connection`, 17
    `freerouting-fanout`).
  - Java's `canary-pwr` session, imported (`freerouting-dev widths
    fixtures/canary-pwr/canary-pwr-9511d9a.dsn
    fixtures/canary-pwr/canary-pwr-9511d9a.46bf9e2ef.ses`), the Q02.6
    baseline: narrowest class 0.2 mm; 30 wide nets (Power_High,
    Switch_Node, Power and CAN at 0.3 mm, GND at 0.4 mm, as exported); 14
    with under-width copper, 50.2082 mm under in total. Largest: `+3V3_AON`
    10.9245 of 130.8403 mm (min 0.18), `/Holdup/LTC_INTVCC` 9.0035 of
    39.8145, GND 8.0565 of 168.9878 (min 0.2), `+5V_PI` 4.7145 of 18.6793
    (min 0.18), `/Holdup/VCAP` 3.9367 of 96.5973, `/Pi_Interface/PI_3V3`
    2.6750, `/Holdup/LTC_SW` 2.5299 of 4.0856 (min 0.18),
    `/Pi_Supply/+5V_SHUNT` 2.4404 of 7.3893, `+12V_BUS` 2.1240 of 21.5924,
    `+12V_PROT` 1.3570, `/Holdup/LTC_CAP2` 0.7551, `+5V_BUCK` 0.7106, CANH
    0.6874, CANL 0.2931. `ses_widths.py` on the same SES agrees on every
    net to 4.9e-5 mm.

- [x] **Q02.2 — No width ratchet, and a narrowing ledger.**
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
  *Done 2026-10-17:* `quality/neckdown/ledger.rs`: `NarrowingLedger`
  (net, item id, layer, requested, delivered, length, stage) for every
  shipped under-width trace of the reported nets, ascending item id; the
  stage comes from item-id marks the `widths` diagnostic records (`input`,
  the fanout id, the connection-router id, the neck-down policy id when the
  half width is the neck retry's, `optimizer`). In the result JSON as
  `widths.narrowings`. Setting `router.neck_down.strict` (new section
  `neck_down`, unset by default = off, so the Java settings dumps are
  unchanged; `tests/settings_defaults.rs`): the board
  settings turn `automatic_neckdown` off and `neck_width_um` to 0, the
  `widths` diagnostic is added (`job::prepare`), and the CLI exits 3 when
  some reported net has under-width length (`exit.rs`, `cli.rs`
  `apply_strict_sizes`; the manifest's `exit_code` says 3).
  - Ratchet: `tests/quality_no_ratchet.rs::
    round_trips_leave_every_class_width_as_loaded` routes q02-pinch and
    q03-qfn-0p5, writes DSN (C06) and SES (C05), reads the DSN back and
    the input DSN plus SES, routes both again, and compares every class's
    per-layer half widths, every net's class and the rules' minimum half
    width with the input after each step: equal.
    `the_round_trip_board_really_carries_narrow_copper` checks that the
    pinch and the router's neck went through the writers.
  - Strict: `result_json_widths.rs::
    strict_sizes_exit_3_exactly_when_copper_ships_under_width`: q02-pinch
    (the pinch ships under width) exits 3 with `widths.under_mm` > 0;
    q03-qfn-0p5 (the 1.5 mm net can't leave its pad unnecked, so nothing
    ships under width) exits 0 with `under_mm` 0.
    `quality_widths.rs::strict_sizes_turn_the_neck_downs_off_and_add_the_report`.
  - `tools/determinism-check.sh` (8 burners) on the release CLI, q02-pinch
    `-mp 3`: with `--router.neck_down.strict=true` (exit 3 both runs) and
    with `--router.special.diagnostics=widths`: identical.

- [~] **Q02.3 — Distance-bounded neck policy.**
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
  *Partial 2026-10-17:* built: `quality/neckdown/bounded.rs`, the rule as
  pure integer functions: `room_within(room, pin, d)` (room box meets the
  pin box enlarged by D, sums in `i64`), `neck_distance(pin, courtyard,
  pitch)` (far courtyard side plus one pitch), `pitch(center, others)`
  (exact `i128` squared distance, integer ceiling root) and
  `neck_width(pin_neck_half, class_min, board_min)` (never below the
  rules, D8). Unit tests: `room_predicate_at_d_minus_one_d_and_d_plus_one`
  (gaps D − 1 and D inside, D + 1 outside, on all four sides and the
  corner), `room_predicate_is_exact_at_the_coordinate_limits`,
  `neck_distance_saturates_at_the_coordinate_limits` and four more
  (`cargo test -p freerouting --lib quality::neckdown`: 7 passed). Nothing
  route-changing is built, so `router.special.neck_down` at its default is
  untouched: the A14 parity gate on the rebased tree is byte-identical to
  Java on `connection-parity` (7 of 7 boards with a reference, SES
  byte-identical) and on `benchmark` (16 of 16: 10 exact and 6 cutoff boards, every SES
  byte-identical, `cargo test --release -p freerouting --test
  autoroute_board_parity -- --ignored` with `PARITY_SET`, 3270 s of Rust
  routing).
  Not built, by choice (the dispatch allowed it for a large, risky item
  whose value rests on a measurement that waits for P04): the width policy
  inside the maze search, the `bounded-neck` `NeckDownPolicy`
  registration and the insertion with per-segment widths. The A04 search
  has no per-room width hook: `AutorouteControl` carries one
  `trace_half_width`/`compensated_trace_half_width` per layer, read by room
  completion, the door checks (`door_is_small`), the destination and start
  neck-downs in `session/search.rs`, the path locator and
  `path/insert.rs`. The design for the remainder: (1) a
  `WidthPolicy` trait in the A04 search (`fn half_width(&self, room,
  layer) -> i32`, the Freerouting policy returning the control's value, so
  the parity path is unchanged), read at each of those sites; (2)
  `BoundedNeck::retry` builds the policy from the connection's start-set
  and destination-set pins in that order (D and neck width per pin from
  the functions above, courtyard = the component outline's box), reruns
  `attempt` at the point and with the ripup cost of
  `FreeroutingNeckDown::retry`, and inserts each located segment with the
  policy's width of its room; (3) registration as `bounded-neck` in
  `quality/register.rs` and a `--router.neck_down.path_mm` key for the
  2.5 mm arm. Remainder owner: Q02 (needs an A04 width-policy hook first;
  measured in Q02.6 after P04).

- [~] **Q02.4 — Bounded-neck fixture.**
  `q02-qfn-power.dsn`: a 0.5 mm pitch QFN with one 1.5 mm power pin, open
  board beyond the pin field.
  - **Accept:**
    - with `bounded-neck`, the connection routes; a test walking the SES
      finds no stretch narrower than the class width further than D plus the
      taper length from the pin;
    - without it, Java's default leaves it unrouted (Q03 says `infeasible`);
    - no new clearance violation and no via in a pad;
    - `tools/determinism-check.sh` passes.
  *Partial 2026-10-17:* not started: the fixture's acceptance needs the
  `bounded-neck` stage, which Q02.3 has not built (see its note). Q03's
  `q03-qfn-0p5.dsn` already shows the default (Java-parity) router leaving the 1.5 mm QFN
  pin unrouted (`quality_attribution.rs`, verdict `infeasible`), so the new
  fixture only needs the open board beyond the pin field. Remainder: all of
  Q02.4, owned by Q02 after Q02.3.

- [~] **Q02.5 — Piecewise widen-back with taper.**
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
  *Partial 2026-10-17:* not started, by choice: its value depends on Q02.3
  and on the Q02.6 measurement, which waits for P04's default-tree decision
  (Q00 README). Design kept as written above; the pieces it needs from this
  dispatch exist: the ledger (`quality::neckdown::ledger`, the traces to
  widen, in insertion order) and the width report (to show the tapers count
  as narrow). Remainder: all of Q02.5, owned by Q02 after Q02.3.

- [~] **Q02.6 — Measurement.**
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
  *Partial 2026-10-17:* preregistered before any number:
  `docs/roadmap/prereg/Q02.json` (`Q02.6-bounded-neck`, sha256
  `bd65dddcd71428197506b47e5eb8963696a31c772db9c7c18ad30c0eaf1e3e89`,
  pinned by `crates/freerouting-dev/tests/prereg_q02.rs`): the four arms,
  the hard metrics (`via_in_pad`, `below_minimum`), the guards (`drc_real`,
  median under-width length per wide net), the signal (unrouted), the keep
  rule (N−1 boards, none worse, median under width below 40 mm), the stop
  rule and the decision table, with Q02.1's Java baseline. Board set
  `tools/bench/sets/q02-wide-nets.txt`: the five canary-pwr variants (one
  board) and four boards eligible from the inputs by Q03's candidate rule
  (`freerouting-dev feasibility` over every `fixtures/*.dsn`):
  coldfire (55 pins), z10_module (5), Issue029-hw48na (104),
  Issue684-Autorouter_PCB1 (110). interf_u and CNH have wide classes but no
  pad narrower than them, so they are out. Remainder: the measurement,
  which waits for P04's default-tree decision and for Q02.3–Q02.5 (owner
  Q02, after P04).
