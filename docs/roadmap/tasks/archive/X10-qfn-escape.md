# X10 — QFN and QFP escape

Opened 2026-10-15. Java has a fanout pre-pass (`BatchFanout.fanoutBoard` →
`RoutingBoard.fanout` per SMD pin, cda540f), which is parity work for the A
series. KRT has a dedicated QFN/QFP escape (`py_router/qfn_fanout/`, 2,469
lines, entry `generate_qfn_fanout`, at `0486e799`): short stubs outward from
edge pads, in the part's own frame. On the owner's boards the escapes are exactly
where wide power nets have to narrow, so this stage pairs with Q02: each stub
starts at the pin's width and widens back past the pin field.

Prerequisites: board parity (rung 3); A10 (the fanout stage slot and
Freerouting's fanout as the default); B05 (pin exits and neck-down width);
B09; Q02 (widen-back with taper, reused); F04; F03.1, F03.4.

Also read the Java fork's `docs/research/escape_congestion_research.md`
(at `main`; a plan, never implemented): pre-route escape-congestion
estimation and congestion-aware pin ordering for the fanout. Its congestion
measure is the natural order for X10's stubs, and its "detect escape
bottlenecks before routing" idea overlaps Q03.

## What exists and what this adds

| Exists | State |
|---|---|
| Java `BatchFanout`, `RoutingBoard.fanout` | the default fanout stage (A10/A11) |
| Rust component and package model (`board/structure/component.rs`, `library/package.rs`): placement, rotation, side, pin offsets | the part frame |
| KRT `qfn_fanout`: side classification by pad position in the part frame; stubs perpendicular to the side; vias only where a pad must change layer | the design |
| KRT rotated and back-side handling (`rotate_frame.py`, `flip_frame.py`; cut 18 of 51 mirror differences to 0 in its BGA code) | the test discipline |

## Decisions made here

- **Stage id `krt-qfn-escape`**, selected by `router.fanout.algorithm`. Parts
  qualify when their pins form four (QFN, QFP) or two (SOIC, SOP) edge rows in
  the package frame with a pitch of at most 0.65 mm; others go to the
  Freerouting fanout.
- **Stubs** run outward perpendicular to the pin's side, to the package
  outline plus the net's clearance plus half its class width. Width at the
  pin: `min(class width, 2·neckdown half-width)` (B05); past the pin field the
  stub widens with Q02's taper.
- **No vias at the stub end by default**; a via is added only for a plane-net
  pin (D8: its own via, never in a pad), placed at the stub end.
- **Order:** components by name, pins by pin number within a component,
  sides in the order W, S, E, N of the package frame.

## File ownership

| Item | New files | May edit |
|---|---|---|
| X10.1 | `crates/freerouting-krt/src/fanout/mod.rs`, `fanout/frame.rs` | `crates/freerouting-krt/src/lib.rs` (append) |
| X10.2 | `fanout/qfn.rs` | `crates/freerouting-krt/src/lib.rs` (register line) |
| X10.3 | `fixtures/krt/x10-qfn-*.dsn`, `crates/freerouting-krt/tests/qfn_escape.rs` | — |
| X10.4 | the X10 preregistration and baseline files | `docs/DIVERGENCES.md` (append, only if kept) |

## Checklist

- [x] **X10.1 — Part frame and sides.**
  Rust design: `fanout::frame::PartFrame` maps board points to the package
  frame (undoing placement, rotation and the back-side mirror) and back, in
  integer units; `sides(component) -> Option<[Vec<PinId>; 4]>`.
  - **Accept:** unit tests: a QFN placed at 0°, 90°, 37° and mirrored gives
    the same side assignment for every pin; a BGA returns `None`.
  *Done 2026-10-17:* `crates/freerouting-krt/src/fanout/frame.rs` (ports KRT
  `analyze_qfn_layout`, `analyze_pad`, `single_line_reason` from
  `py_router/qfn_fanout/layout.py` at `0486e799`: SPDX header and a
  `docs/PROVENANCE.md` row): `PartFrame::{new, of_component, to_board, to_part,
  box_to_part}` (the component's `PlacementTransform` and its exact inverse,
  rounded to board units; exact at multiples of 90°), `sides(board, component)
  -> Option<[Vec<ItemId>; 4]>` (W, S, E, N, each by pin number) and `analyze`
  (the `PartLayout`: pins in the part frame, pitch, pin field, outline box).
  KRT's rule in integer units: centre box of the mapped pin centres, edge
  tolerance `10·d < min side`, ties N, S, W, E (KRT's top, bottom, left, right in
  the DSN's y-up frame), pitch the smallest gap above 0.1 mm. Qualifying (the
  X10 decision made concrete): 4 sides or 2 opposite ones, at least 8 side
  pins (KRT's perimeter-package floor), pitch ≤ 0.65 mm + 1 µm, interior pins
  only exposed pads (smaller side ≥ 2 pitches) or inside one. Not in KRT: an
  edge tie goes to the side the pad's long axis points out of first (KRT puts
  a SOIC's end pads on its top and bottom rows). Tests (`cargo test -p
  freerouting-krt --test qfn_escape`, 7 passed, 2 ignored):
  `the_frame_round_trips_at_0_90_37_degrees_and_mirrored` (both flip styles,
  0°, 90°, 37°, mirrored 0° and 127°: exact at multiples of 90°, within 1 unit
  otherwise), `a_qfn_at_0_90_37_degrees_and_mirrored_has_the_same_sides`
  (`tests/data/x10-frame.dsn`: the 24-pin 0.5 mm QFN at 0°, 90°, 37°, back 0°
  and back 127° has sides W 1–6, S 7–12, E 13–18, N 19–24 on every copy, the
  EP interior, every mapped centre within 1 unit of the package pin's relative
  location, pitch 0.5 mm ± 1 unit), `a_bga_and_a_coarse_soic_return_none_and_a_tssop_has_two_sides`
  (5 × 5 0.5 mm BGA `None`, 1.27 mm SOIC-8 `None`, 0.65 mm TSSOP-8 W 1–4 and
  E 5–8). The fixtures are in `crates/freerouting-krt/tests/data/` as X05–X07's
  are, not `fixtures/krt/` (every DSN under `fixtures/` needs a Java reference).

- [x] **X10.2 — Stubs.**
  Each stub is inserted in a B09 transaction; a stub the exact DRC rejects is
  shortened by halves to the outline and then skipped.
  - **Accept:** on `x10-qfn-power.dsn` (a 0.5 mm pitch QFN with one 1.5 mm
    power pin and signal pins), every connected pin gets a stub, the power
    stub widens to 1.5 mm past the pin field, and there is no DRC item and
    no via in a pad.
  *Done 2026-10-17:* `crates/freerouting-krt/src/fanout/qfn.rs` (written
  from the checklist's design; KRT's straight lead escape and taper widths as
  ideas) and `fanout/mod.rs`: the `krt-qfn-escape` fanout stage
  (`KrtQfnEscape`, registered by `freerouting_krt::register`, selected by
  `router.fanout.algorithm`). Parts by component name, sides W, S, E, N, pins
  by pin number; a pin is skipped when not SMD, not on exactly one net, on an
  ignored net, alone on its net, or already connected. Stub on the pin's layer
  outward from the pin centre: narrow half-width `min(class, neck-down)` (never
  below the board minimum); a plain pin ends at escape box (pin field ∪
  outline) + the class's largest clearance + class half-width; a wider class
  or a plane-net pin (a conduction area of its net on another layer) runs
  narrow to `edge + c + r` (r the class half-width or the via radius), moved
  past every same-side neighbour stub it would face, then a 4-step taper over
  0.5 mm at `w + (n − w)·k/5`, k = 4..1, and one step at class width, or its
  own via. Each stub is one B09 `try_change(NewItemsOnly)` that also refuses
  a via in a pad, a via a nearby pin's check reports (X06's pin-side gap) and
  a stub that misses its pin; a rejected stub is retried narrow with the
  overshoot past the escape box halved while ≥ n, then at the box, then
  skipped. Then Freerouting's fanout on the other components
  (`FanoutRun::retain_components`, a new filter in the engine's
  `batch/fanout.rs`, unused by the Freerouting stage). Tests (`cargo test -p
  freerouting-krt --test qfn_escape`): `every_connected_pin_of_the_power_qfn_gets_a_stub`
  (`tests/data/x10-qfn-power.dsn`, 4 layers, GND plane on In1.Cu: the input
  has 0 clearance violations; U1 the only qualifying part; 24 side pins, 23
  stubs, pin 20 (alone on its net) `Unconnected`; PWR pin 3 `Widened` at the
  first try: 6 pieces with half-widths 124.9, 250, 375, 500, 625, 750 µm, the
  1.5 mm piece wholly outside the W edge of the pin field; GND pin 10 `Via`;
  every other stub narrow at 0.2 mm; afterwards `all_clearance_violations`
  empty, 1 via on the board and `via_in_pad` false; every stub item in its
  pin's connected set), `the_stage_escapes_the_qfn_and_hands_the_rest_to_freerouting`
  (the stage through `FanoutStage::run`: 23 stubs, then Freerouting's fanout
  on the header and J1, 5 passes, 23 vias on the board, no clearance violation,
  no via in a pad), `the_registry_selects_krt_qfn_escape_in_the_fanout_slot`.

- [x] **X10.3 — Rotated and mirrored copies.**
  - **Accept:** the same part placed at 0°, 90°, 180° and on the back side
    gets the same number of stubs, and the stubs map onto each other through
    the part frame within 1 board unit.
  *Done 2026-10-17:* `rotated_and_mirrored_copies_get_the_same_stubs`
  (`cargo test -p freerouting-krt --test qfn_escape`;
  `tests/data/x10-qfn-rotated.dsn`: the X10.2 part at 0° (U1), 90° (U2), 180°
  (U3) and on the back (U4, pads on B.Cu), each pin's net joining that pin of
  every copy, GND plane on In1.Cu): 23 stubs on every copy, the same result
  (kind and try) for every pin, and every trace corner and via centre, mapped
  to its copy's part frame, within 1 board unit of U1's, with equal widths;
  afterwards no clearance violation, 4 vias (one plane via per copy), none in
  a pad.

- [~] **X10.4 — Measurement.**
  Preregister (F03.4). Arms: Freerouting fanout and `krt-qfn-escape`, both
  followed by the full pipeline. Boards: at least three with fine-pitch
  QFN/QFP parts, plus `canary-pwr` and its variants. Signal: unrouted.
  Guards: `drc_real`, vias in pads, Q02.1 under-width length.
  - **Accept:** measured; kept as a selectable stage only if unrouted
    improves on N−1 boards and regresses on none.
  *Partial 2026-10-17:* registered, not run: the measurement changes routes,
  so it waits for P04's default-tree decision (P00 README "Order"); owner P04,
  then this item. Preregistration `docs/roadmap/prereg/X10.json` (F03.4
  format, `freerouting-dev gate`'s global rules: ≥ 3 boards, N−1, no
  regression, `via_in_pad` hard): arms `freerouting-fanout` and
  `krt-qfn-escape`, both with `fanout.enabled = true` and the defaults
  (optimizer on, 20 passes); boards `tools/bench/sets/x10-qfn.txt` (input
  sha256 in the registration): Issue066-GP8B, DAC2020 bm01 and bm10,
  kit-dev-coldfire (KiCad demo, graded by `tools/kicad-oracle/grade.sh`),
  smoothieboard, and canary-pwr with its four variants as one board (graded by
  canary-23); each has at least one qualifying part
  (`qualifying_parts_on_a_set`: e.g. GP8B IC1 100 pins at 0.5 mm, coldfire
  U102 and U301 at 0.5 mm, canary-pwr U1 38 pins at 0.5 mm, U3, U6, U8);
  signal unrouted; guards drc_real/drc_new, via_in_pad, Q02.1's
  under-width length; harness `escape_arms_on_a_set` (`X10_SET` or
  `X10_DSN`, `X10_ARMS`, `X10_SES_DIR`) in
  `crates/freerouting-krt/tests/qfn_escape.rs`, since the CLI does not
  register the crate's stages before C07. **Default-off gate:** X05's
  `default_off_sessions_on_a_set` (built-in registry against
  `freerouting_krt::register` with the stage, default settings), release:
  `identical` on `connection-parity` (10) and `benchmark`'s first five
  (Issue433, ecc83, bm02, sonde, bm07), `cmp` 0 on all 14 SES pairs.
  **Pre-P04 dry run** (informational, not the registered result; this tree,
  release, Rust DRC only, machine load 3–5), canary-pwr-9511d9a:
  `freerouting-fanout` 123 unrouted, drc_new 0, via_in_pad 0, 142 vias,
  70.9 mm under class width, 598 s; `krt-qfn-escape` 122 unrouted, drc_new 0,
  via_in_pad 0, 135 vias, 45.3 mm under class width, 515 s. The other boards
  were not run: one arm on GP8B had not finished after 11 minutes (the
  Freerouting fanout's per-pin budget dominates), so the full set takes hours
  and is left to the registered run. Remainder: the registered run on all
  boards after P04, KiCad grading of coldfire and canary-pwr, the keep or
  reject decision, and (if kept) the `docs/DIVERGENCES.md` line; owner P04,
  then X10.4.
  *Bias note 2026-10-17:* the dry run above built both arms through `TestingProfile::default()`
  (`prepare` with the fanout keys), so both ran with Java's fixture-test copper-to-edge clearance
  0, not the defaults. `tests/qfn_escape.rs` now starts from an empty patch (`default_profile()`);
  rerun those rows before quoting them.
  *Partial 2026-10-17 (registered run started, then stopped by the operator's priority change to
  K09; nothing decided, no code removed):* `docs/roadmap/prereg/X10.json` was amended before the
  first run (kit-dev-coldfire registered as the prepared X01 demo that grade.sh routes, canary-pwr
  9511d9a graded by canary-23's tools on WSL KiCad 10.0.7 and its four variants by Rust's DRC, the
  harness fix, a 45 min timeout per route). One route ran: DAC2020 bm01 `freerouting-fanout`
  timed out at 2700 s (not graded; log `target/orchestrator/meas/runs/x10-engine.log`), so bm01
  cannot count as a pass, and its `krt-qfn-escape` arm was dropped. **Remaining** (owner: this item,
  after K09): both arms on bm10, smoothieboard, GP8B (expect long runs; the pre-P04 GP8B arm was
  still running after 11 min), the five canary-pwr rows (9511d9a via `ses_import.py` +
  `tools/krt-h2h/grade_routed.sh`, the variants by Rust's DRC) and kit-dev-coldfire through
  `tools/kicad-oracle/grade.sh`; then the gate and the keep or reject decision. The exact job
  lines are in `target/orchestrator/meas/jobs-not-run.txt` (engine wrapper `meas/engine.sh x10`).
  With bm01 not graded at 45 min, the keep rule (improve on N−1 of 6) needs every other board to
  improve; consider a longer timeout for bm01 through another amendment before rerunning.
