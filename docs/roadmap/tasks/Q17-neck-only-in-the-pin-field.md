# Q17 — Neck down only through the pin field

Opened 2026-10-18 from N05.3's neck-down promotion. `enhanced` now sets
`router.neck_width_um = 300`: Java's necked retry. A connection that fails at its
class width is routed again at 0.3 mm **along its whole length**, not only where
the pins are close together. On canary-pwr with its Board Setup widths
(`canary-pwr-9511d9a-board-setup-widths.dsn`) the retry takes 49 unconnected to
35. But it ships 140.9 mm of Power_High (1.5 mm class) narrower than its class,
and the run takes 2.3× as long.

Q02 designed the better rule and built only its pure functions
(`quality/neckdown/bounded.rs`: `room_within`, `neck_distance`, `pitch`,
`neck_width`). Q02.3's width policy inside the maze search, Q02.4's fixture and
Q02.5's widen-back were never built (see their notes in
[archive/Q02](archive/Q02-neck-down-through-pin-fields.md)). This checklist
builds them. It measures them against today's `enhanced` (`neck_width_um = 300`),
not against Java.

Prerequisites: Q02.1–Q02.2 (the width report and the narrowing ledger), N02 (the
totals rule), N05 (the promotion recipe).

## Decisions made here

- **The parity path does not change.** The width hook's default policy returns
  the control's per-layer half-width, so `legacy` stays byte-identical to Java
  (A14).
- **The baseline is `enhanced` as it is.** A new stage is promoted only if it beats
  `neck_width_um = 300` under N02's totals rule on unconnected, or ties it and
  narrows less copper. If it is promoted, it replaces the retry in `enhanced`;
  the retry stays as an option.

## Checklist

- [~] **Q17.1 — A width policy in the maze search** (Q02.3's remainder, step 1).
  A `WidthPolicy` trait in the A04 search, `fn half_width(&self, room, layer) -> i32`,
  read at every site that now reads `AutorouteControl::trace_half_width` or
  `compensated_trace_half_width`: room completion, `door_is_small`, the start and
  destination neck-downs in `session/search.rs`, the path locator and
  `path/insert.rs`. The Freerouting policy returns the control's value.
  - **Accept:** with the default policy, the A14 exact set is byte-identical; a unit
    test shows a two-width policy changes what `door_is_small` decides.
  *Partial 2026-10-18:* built: `autoroute/width_policy.rs` (`WidthPolicy::half_width(room:
  Option<&TileShape>, layer: usize) -> i32`, `FreeroutingWidths` = the control's value). The
  session holds `Option<&dyn WidthPolicy>`; `None` (every Java path) reads
  `trace_half_width`/`compensated_trace_half_width` directly, a `match` with no call and no
  arithmetic, and a policy's compensated width is its half width plus the control's
  compensation. Read at every site: `expand_to_room_doors` (the `door_is_small` and
  `next_room_is_thick` width and with it the start- and destination-pin neck-downs),
  `expand_to_target_doors`, `expand_to_door` (door sections), `room_shape_is_thick`,
  `expand_to_drill` and the via check (`layers.rs`), `enter_through_small_door`
  (`ripup.rs`), `check_shove_trace_line` (`shove_room.rs`), the locator (`locate.rs`,
  `locate45.rs`, `locate_any.rs`, with the room of each corner) and the inserter
  (`ResultItem::half_widths`, a segment at its room's width; empty = the control's). Entry
  point `route_connection_with_widths`, `attempt_with_widths`. Room completion itself reads
  no width (the tree shapes use the clearance class), so it has no site. Tests:
  `autoroute::session::tests::a_two_width_policy_changes_what_door_is_small_decides` (a door
  small at the control's width is not small when the policy narrows the room behind it; a far
  room keeps the control's decision), `the_freerouting_policy_is_the_controls_width`,
  `tests/autoroute_width_policy.rs::the_freerouting_policy_routes_like_no_policy` (every
  sampled connection of Issue143-rpi_splitter and Issue508-SMD-routing-issue-demo routed in
  turn with ripup, with and without `FreeroutingWidths`: same states, same SES). A14 fast
  subset, release: `PARITY_SET=tools/bench/sets/connection-parity.txt cargo test --release -p
  freerouting --test autoroute_board_parity -- --ignored autoroute_board_parity
  --test-threads=1`: 7 of 7 boards `[exact] ... eq true bytes true`. Timing (release,
  `legacy`, `-mp 100`, optimizer off, base `9c49979` and this tree interleaved, 3 runs each,
  load 3.6–8.9 from other agents), SES identical: Issue508-DAC2020_bm01 36.50/40.09/34.67 s
  → 37.72/34.94/34.66 s, Issue508-DAC2020_bm07 0.454/0.453/0.452 → 0.440/0.447/0.443 s,
  Issue093-interf_u 1.264/1.256/1.280 → 1.250/1.279/1.251 s: no difference above the noise.
  Remainder: the full A14 exact set, heavy run pending: `cargo test --release -p freerouting
  --test autoroute_board_parity -- --ignored autoroute_board_parity --test-threads=1`
  (owner Q17, in the heavy slot).
- [x] **Q17.2 — The `bounded-neck` stage** (Q02.3's remainder, steps 2–3).
  `BoundedNeck::retry` builds the policy from the connection's start-set and
  destination-set pins, in that order: the neck width within D of a pin, the class
  width elsewhere, with D = courtyard plus one pitch from `neck_distance`. It reruns
  the search at the point and with the ripup cost of `FreeroutingNeckDown::retry`,
  and inserts each segment at its room's width. Register it as a `NeckDownPolicy`
  with the id `bounded-neck`, and add `--router.neck_down.path_mm` for a fixed-length arm.
  - **Accept:** Q02.4's fixture (`q02-qfn-power.dsn`: a 0.5 mm pitch QFN with one
    1.5 mm power pin, open board beyond the pin field):
    - the connection routes;
    - a test walking the SES finds no stretch narrower than the class width
      further than D plus the taper length from the pin;
    - no new clearance violation and no via in a pad;
    - `tools/determinism-check.sh` passes.
  *Done 2026-10-18:* `quality/neckdown/bounded_neck.rs`: `BoundedNeck` (`NeckDownPolicy`,
  registered as `bounded-neck` in `quality/register.rs`, selected with
  `--router.special.neck_down=bounded-neck`; unset by default and in no profile).
  `BoundedWidths` (the `WidthPolicy`): one zone per pin of the start set, then of the
  destination set (each ascending id), the pin's box enlarged by D (`neck_distance` of the
  pin box, the component's courtyard outline box, `pitch` among its pins), the neck half width
  `ceil(neck_width(neckdown_half_width, class minimum, 2·min_trace_half_width) / 2)`, at most the
  class; a room meeting a zone (`room_within` on its box) gets the narrowest neck. `retry`
  takes a new control on the board as it is (as `FreeroutingNeckDown::retry`), reruns
  `attempt_with_widths` with the attempt's sets, work limit and ripped set, inserts each
  segment at its room's width, and returns `None` with `neck_down.strict`. New setting
  `router.neck_down.path_mm` (unset; D = that length). **Deviation from Q02.3's design:** a free
  room next to a pin field reaches far beyond D (on the fixture the room below the QFN spans the
  open board), so inserting at the room's width alone left copper narrow beyond D; after the
  insert the stage widens the new narrow traces' pieces outside every zone to the class width
  where the exact clearance check passes (`widen_back::widen_traces` with the zones), before
  the pull-tight. Fixture `crates/freerouting/tests/data/quality/q02-qfn-power.dsn` (QFN 0.5 mm
  pitch, staggered double row on the bottom, PWR 1.5 mm on inner pin U1-2 to J1 across an open
  board), tests `tests/quality_neckdown.rs`:
  - routes: `bounded_neck_routes_the_power_pin_that_java_leaves_unrouted` (`legacy`: PWR
    incomplete 1; `bounded-neck`: 0);
  - SES walk: `bounded_neck_is_narrow_only_within_d_of_the_pin` (D = 60934 units = 6.09 mm;
    every PWR wire under 1.5 mm has its corners in the pin box enlarged by D + 0.5 mm, with and
    without widen-back; Java's retry at 0.24 mm on the same board has narrow copper beyond it).
    Release CLI SES: 0.1872 mm into the pin (Java's micro neck-down), 0.2498 mm to
    (16.4184, 6.6565) mm, the zone's edge, then 1.5 mm to J1; Java's retry at 240 µm: 0.24 mm
    for all but the last 1.6 mm;
  - `bounded_neck_adds_no_clearance_violation_and_no_via_in_a_pad` (BoardStatistics 0, DRC
    clearance 0, vias in pads 0); `a_fixed_path_length_bounds_the_neck` (`path_mm = 2.5`);
  - `tools/determinism-check.sh` (8 burners) on the release CLI, `-mp 5 --router.profile=legacy
    --router.special.neck_down=bounded-neck`: identical (SES sha256 `450fbc512d93c735…`).
- [x] **Q17.3 — Widen-back with taper** (Q02.5 as designed there). After insertion and before
  pull-tight, retry each narrowed trace in ledger order at its class width, in pieces
  of at most 0.5 mm, using the exact clearance check. Add the 4-step taper at each
  narrow-to-wide junction, and drop wide islands no longer than two tapers. Run it
  after both `bounded-neck` and the plain retry.
  - **Accept:**
    - a 22 mm run with one 0.4 mm pinch is narrow only at the pinch and its tapers;
    - no new DRC items on any fixture;
    - pull-tight does not remove the tapers.
  *Done 2026-10-18:* `quality/neckdown/widen_back.rs`, switch `router.neck_down.widen_back`
  (unset: off). `widen_traces` takes the retry's new traces under the class width in ascending
  id, cuts each into pieces of at most 0.5 mm at exact lattice points (`a + k·d/gcd(d)`), tries
  the class width on each with `check_polyline_trace` (the inserted-trace check), drops wide
  islands no longer than two tapers, replaces the first 0.5 mm of each wide run at each
  junction with 4 steps `wide − (wide − narrow)·k/5`, and replaces the trace by one trace per
  run; runs after the insert and before the pull-tight in both `FreeroutingNeckDown::retry`
  (only with the switch; unset leaves Java's retry unchanged) and `BoundedNeck::retry`. Unit
  tests in the module (pieces, zones, exact negative-step ranges, taper widths, islands).
  Fixture `crates/freerouting/tests/data/quality/q17-pinch-22mm.dsn` (PWR 1.5 mm, 22 mm between
  two pads, one layer, a 1 mm gap between walls at x = 14.5–15.5 mm), tests in
  `tests/quality_neckdown.rs`:
  - `a_22mm_run_with_one_pinch_is_narrow_only_at_the_pinch_and_its_tapers`: Java's retry at
    400 µm ships one 0.4 mm wire over the whole run; with the widen-back the 0.4 mm wire is
    x = 13.5–16.5 mm (the walls plus the 1 mm a 1.5 mm trace keeps from them) and 8 taper steps
    (0.62/0.84/1.06/1.28 mm, 0.125 mm each) sum to 2 × 0.5 mm next to it, 1.5 mm elsewhere;
  - `widen_back_adds_no_drc_items_on_any_fixture`: q02-qfn-power, q17-pinch-22mm, q02-pinch and
    q03-qfn-0p5, each with the retry at 400 µm and with `bounded-neck`: DRC clearance violations
    and unconnected items with the widen-back ≤ without, vias in pads 0 (the test fixtures, as
    Q02.5 wrote it; the switch is off everywhere else);
  - `pull_tight_does_not_remove_the_tapers`: after the run, a pull-tight of the whole board
    (`mark_all_changed_area`, `opt_changed_area`) leaves the 9 under-width wires as they were;
  - `tools/determinism-check.sh`: q02-qfn-power with `bounded-neck` and the switch identical
    (`cc07d01fc2e16861…`), q17-pinch-22mm with the retry at 400 µm and the switch identical
    (`6b2e39c552478a54…`).
- [ ] **Q17.4 — Measure under N02.** Register first, on the boards of
  `N05-neck-down.json` (the canary-pwr plugin DSN, the canary-pwr 9511d9a DSN with
  its Board Setup widths, interf_u, pic_programmer, CM5_MINIMA_3) plus two
  corpus boards with wide classes it has not seen.
  - Arms:
    - `enhanced` (the retry at 300);
    - `bounded-neck`;
    - `bounded-neck + widen-back`;
    - `retry 300 + widen-back`.
  - Report the narrowed copper on the wide classes from the neck-down width report,
    and the wall time.
  - **Accept:**
    - every hard guard holds: clearance, width, via in pad, power-layer traces;
    - if an arm is accepted, N05 makes it `enhanced`'s neck-down, with a
      DIVERGENCES line;
    - canary-7a grades the canary sessions.
