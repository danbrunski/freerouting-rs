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

- [x] **Q17.1 — A width policy in the maze search** (Q02.3's remainder, step 1).
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
  *Done 2026-10-18:* full A14 on origin/main 81fcef7 (release, `--test-threads=1`, then the 21 subdirectory references with `PARITY_SUBDIRS=1`; logs `target/orchestrator/heavy/a14-81fcef7*.log`): 1 passed, 0 failed, 138 boards (120 exact, 15 cutoff, 3 Java failures), every SES byte-identical except the known Issue684 and Issue721 (expected), Issue110-RelayModule (non-ASCII name only) and the Java-failure boards.
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
- [~] **Q17.4 — Measure under N02.** Register first, on the boards of
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
  *Partial 2026-10-20:* registered before any number: `docs/roadmap/prereg/Q17.4.json`
  (sha256 `b3d4ad492fb09fccb150bf142448f40ed21d041c049d35b086c1aa566a6bc2d9`, no amendment), set
  `tools/bench/sets/q17-4.txt`, pinned by `crates/freerouting-dev/tests/prereg_q17_4.rs`
  (`cargo test -p freerouting-dev --test prereg_q17_4`: 5 passed). Engine main a25c82f,
  `--router.profile=enhanced` on every command. Arms (decided 2026-10-20: measurement runs stay
  short, never an hour in total, so three arms): `enhanced` (the retry at 300); `widen-back`
  (`--router.neck_down.widen_back=true`); `bounded-neck-widen-back`
  (`--router.special.neck_down=bounded-neck --router.neck_down.widen_back=true`). The list's
  `bounded-neck` without the widen-back is cut (it differs from bounded-neck-widen-back only inside
  the neck zones; named in the registration's `not_registered`). Rows: canary-plugin45 (the stored
  plugin export `plugin-b1994423.dsn`, CLI with the plugin's flags), canary-9511d9a-mp1sweep (the
  Board Setup widths DSN, CLI), interf_u and pic_programmer (K09's stripped demos through
  `kicad-plugin/headless_run.py`), and two corpus boards with a class wider than 300 µm that no
  neck-down measurement used, routed and graded by `tools/kicad-oracle/grade.sh`:
  flight-computer-core-board (Ignitor 400 µm; 0.34–0.51 s in Q10.4 and Q07) and cm4-edge-ai-carrier
  (Power 500 µm; 22.4 s in Q07). CM5_MINIMA_3 is dropped (85.5–86.1 s per route in N05.3, no class
  wider than 300 µm); eprom-card-80bus (65–86 s), moteus-c1 (148 s) and microzed-carrier-board
  (302 s) are the slower wide-class corpus boards left out. N02 totals, `accept: no-regress`; hard
  guards via in pad, below minimum width, KiCad clearance-type errors and power-layer traces, each
  "no new breach versus baseline" (arm minus `enhanced`, floored at 0; raw counts reported);
  `drc_real` and inner tracks as guards; keep rule: the gate passes and either the unconnected total
  beats `enhanced` by more than the set noise or `under_board_setup_mm` (the width report's
  narrowed copper of the wide nets) is lower. Expected route time about 9.5 min per arm, 29 min
  for the three. heavy run pending, one route at a time, every route under `ulimit -v 8000000` and
  `timeout -k 10 1800` (runner `target/orchestrator/q174/q174run.sh`, adapted from N05.3's
  `neckrun.sh`):
  1. `target/orchestrator/q174/q174run.sh build` (`cargo build --release -p freerouting-cli -p
     freerouting-dev` in the worktree, binaries to `target/orchestrator/q174/run/bin/`, `SHA256SUMS`
     written), then `q174run.sh hashes` and compare with `boards.files` and `boards.project_files`.
  2. `q174run.sh batch`: for each arm in the order enhanced, widen-back, bounded-neck-widen-back:
     - canary rows: `freerouting -de <DSN> -do route.ses --gui.enabled=false
       --router.profile=enhanced <ROW_FLAGS> <ARM_FLAGS>`, then `freerouting-dev power-traces <DSN>
       route.ses` and `freerouting-dev widths <DSN> route.ses --json widths.json`;
     - demo rows (the plugin-row export): copy the stripped board, `python3 -I
       kicad-plugin/headless_run.py export <pcb> <work> opts.json` with `{"schema": 4}`, route with
       the exported `args` plus `--gui.enabled=false --router.profile=enhanced <ARM_FLAGS>`,
       power-traces and widths on the exported DSN, then (unless the session equals an earlier
       arm's) `headless_run.py import <pcb> <ses> <pcb> opts.json`, `kicad-cli pcb drc
       --refill-zones --save-board --severity-all --format json`, canary's `via_in_pad.py` and
       `inner_tracks.py`;
     - corpus rows: `tools/kicad-oracle/grade.sh --out <dir> <project> -- <capped freerouting> -de
       {DSN} -do {SES} --gui.enabled=false --router.profile=enhanced <canary-9511d9a ROW_FLAGS>
       <ARM_FLAGS>`, then power-traces, widths, `via_in_pad.py` and `inner_tracks.py` on
       `<dir>/routed/<stem>.kicad_pcb`;

     then each distinct canary session once: `GRADE_REV=<b1994423 | e21ee05e> KICAD_BIN=<N05.3's
     kshim> bash ../canary/hardware/tools/grade.sh ../canary/hardware/canary-pwr-rev1.0 route.ses
     <dir>`.
  3. Write `records.json` from the graded numbers (as N05.3's `analyze.py`), then `freerouting-dev
     gate --records records.json --baseline baseline.json --prereg docs/roadmap/prereg/Q17.4.json
     --write-baseline` and the same with `--all-pairs`; the per-class narrowed copper of the wide
     classes and the wall times go in the done note.
  4. The canary sessions to `target/canary-handoff/2026-10-20-q17-4/` for canary-7a's grade.
  *Partial 2026-10-20 (run):* the registered run, no amendment (registration sha256 `b3d4ad49…`
  re-checked by the gate: 0 amendments, 0 refusals). Engine main a25c82f in a detached worktree, one
  release build (`freerouting` sha256 `11d806d262c5…`, `freerouting-dev` `dd1dc02d2dc9…`); every
  input hash matched `boards.files` and `project_files` before the first route. 18 routes, one at a
  time, 10:24–10:36, all exit 0, none aborted; summed route time 196.5 s (`enhanced`), 205.9 s
  (`widen-back`), 193.5 s (`bounded-neck-widen-back`). Runner fixes, no registered setting changed:
  build in the detached engine worktree; route exit codes recorded; KiCad steps wait for any other
  `kicad-cli`; the canary grade under `timeout 900` without the route's `ulimit`; the corpus rows'
  `via_in_pad.py` re-run on the routed boards after the batch (the runner kept a font warning as the
  first line; all 0). Raw logs, `records.json`, `summary.json`, the gate outputs and the runner:
  `target/orchestrator/q174/run/`. Per row, `enhanced` / `widen-back` / `bounded-neck-widen-back`
  (KiCad unconnected; KiCad clearance-type errors raw; narrowed copper `under_board_setup_mm`; route
  s). Via in pad, below minimum width, power-layer traces and inner tracks are 0 in every arm on every
  row; no new hard breach in either candidate.

  | row | KiCad unconnected | clearance raw | under mm | route s |
  |---|---|---|---|---|
  | canary-plugin45 | 39 / 39 (same SES) / 42 | 0 / 0 / 0 | 48.9 / 48.9 / 65.5 | 78.5 / 78.9 / 93.0 |
  | canary-9511d9a-mp1sweep | 33 / 38 / 37 | 2 / 0 / 0 | 354.3 / 293.9 / 154.5 | 96.8 / 106.0 / 71.7 |
  | interf_u | 12 / 11 / 10 | 0 / 0 / 0 | 1.8 / 1.8 / 1.8 | 9.9 / 9.4 / 12.4 |
  | pic_programmer | 2 / 2 / 2 | 2 / 2 / 2 | 0.6 / 0.6 / 0.6 | 0.3 / 0.3 / 0.3 |
  | flight-computer-core-board | 5 / 5 / 5 (same SES) | 0 / 0 / 0 | 0 / 0 / 0 | 0.36 / 0.43 / 0.36 |
  | cm4-edge-ai-carrier | 73 / 73 / 75 | 5 / 5 / 1 | 56.2 / 55.3 / 53.4 | 10.7 / 10.9 / 15.7 |
  | total | 164 / 168 / 171 | | 461.8 / 400.6 / 275.8 | 196.5 / 205.9 / 193.5 |

  Narrowed copper per wide class, delivered / under the class width in mm (`freerouting-dev widths`):
  canary-9511d9a Power_High 300.1/237.3, 317.9/176.7, 147.2/73.7; Power 374.2/79.6, 412.5/96.7,
  335.3/43.7; GND 171.5/13.0, 166.7/11.2, 182.1/21.0; Switch_Node 13.9/11.8, 11.5/9.3, 7.7/5.6 (largest:
  `+5V_BUCK` 122.8 of 125.7 mm under in `enhanced`, 58.5 of 98.1 in `widen-back`; `GND` 21.0 of 182.1
  in `bounded-neck-widen-back`). canary-plugin45 Power 406.8/23.6, =, 398.9/33.9; Power_High
  311.1/13.6, =, 133.4/9.6; GND 186.2/5.9, =, 199.1/17.3. interf_u Power 333.6/1.8, 333.4/1.8,
  332.5/1.8; pic_programmer POWER 151.4/0.6 in every arm; flight-computer Ignitor 42.1/0 in every arm;
  cm4 Power 162.6/28.7, 162.2/27.9, 159.0/25.8 (`+3V3` 23.6, 22.7, 22.7 mm under). Gate
  (`freerouting-dev gate --records records.json --baseline baseline.json --prereg
  docs/roadmap/prereg/Q17.4.json`): `--write-baseline` wrote `baseline.json`; the primary pair against
  it `gate: PASS`; `--all-pairs`: `enhanced -> widen-back` total 164 -> 168 (+4), set noise 15.42,
  canary-pwr +5 (band 10), PASS; `enhanced -> bounded-neck-widen-back` 164 -> 171 (+7), canary-pwr +7
  (band 10), cm4 +2 (band 6), PASS; `gate: PASS`. pic_programmer's two clearance-type items are a
  copper text the router does not see (`1=>>` on top_layer) crossing `/DATA-RB7` in every arm (KiCad
  calls one of them `shorting_items` in `widen-back`); the input has none. Keep rule: neither candidate
  beats `enhanced` on unconnected by more than 15.42; both narrow less copper (400.6 and 275.8 against
  461.8 mm), so both are accepted, and the rule picks the lower unconnected total: **`widen-back`
  (168 against 171) is accepted**. Per the decision table N05 adds `router.neck_down.widen_back =
  true` to ENHANCED with a DIVERGENCES "Profile enhanced" line; the retry at 300 stays. Canary
  sessions with SHA256SUMS and README (flags, self-grade, widths):
  `target/canary-handoff/2026-10-20-q17-4/`. Remains: the promotion (N05) and canary-7a's grade of
  those sessions.
