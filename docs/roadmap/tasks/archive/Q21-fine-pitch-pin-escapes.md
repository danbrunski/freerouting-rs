# Q21 — Fine-pitch pin escapes within the board minimums (an option)

Opened 2026-10-20 at the owner's request. Q18 stopped every neck-down at the board's minimum
track width. That cost connections: KiCad unconnected 130 → 151 on Q18.6's four rows, 22 of them
on mydevboard, whose RP2040 QFN pins were reached only with 0.15 mm traces. Many such pins can be
reached at the minimum by leaving the pad straight along its long axis and turning only once
past the pin field.

X10 built that kind of stub for QFN, QFP and SOIC parts: the stage `krt-qfn-escape`, selected by
`router.fanout.algorithm` ([archive/X10](X10-qfn-escape.md)). Its stubs start narrow:
`min(class half-width, the pin's neck-down half-width)`, floored at the `min_half_width` its caller
passes (`crates/freerouting-krt/src/fanout/qfn.rs`, `plan_pin`). Its measurement (X10.4) never
finished. This checklist makes sure that floor is Q18's board minimum, keeps a stub that is no
wider than the pad straight along the pad's axis, makes the stage a preference a user turns on
or off, and measures what it wins back.

Prerequisites: X10 (the stage, its part frame and its rotated/mirrored tests), Q18 (the board
minimums).

## Decisions made here

- **An optional preference** (decided 2026-10-20). There is one user-facing switch:
  - the setting is `router.fanout.pin_escape = off | straight`;
  - it is off by default in both profiles, and off gives byte-identical output to today's;
  - the plugin dialog shows it as a checkbox and saves it per board;
  - the desktop app shows it in the autoroute settings, and the CLI takes it as
    `--router.fanout.pin_escape`.

  Turning it on by default in `enhanced` would be a separate decision, after Q21.4.
- **The stub width is never below the board minimum.** The narrow width is `max(board minimum,
  min(class width, neck-down width))`. The board minimum is `control::min_trace_half_width`,
  which includes `router.board_minimums.track_width_um` since b98074e. A pin where that stub
  doesn't fit with clearance to its neighbours gets no stub.
- **Only parts X10 qualifies**: edge rows in the package frame at pitch ≤ 0.65 mm. BGAs stay
  with X12's planner.

## File ownership

| Item | New files | May edit |
|---|---|---|
| Q21.1 | — | — (analysis of `target/orchestrator/q18/out/`) |
| Q21.2 | tests under `crates/freerouting-krt/tests/` | `crates/freerouting-krt/src/fanout/qfn.rs` and `fanout/mod.rs` (X10's stage) |
| Q21.3 | — | the settings table, `crates/freerouting-stages`, the GUI's autoroute settings dialog, `kicad-plugin/freerouting_rs/options.py` and the dialog, `docs/DIVERGENCES.md` |
| Q21.4 | `docs/roadmap/prereg/Q21.json`, `tools/bench/sets/q21-pin-escape.txt` | — |

## Checklist

- [x] **Q21.1 — Which connections the floor cost.** From Q18.6's pair-A sessions
  (`target/orchestrator/q18/out/`), list every connection that was routed before the floor and is
  open after it. For each, give:
  - the part, pin, pitch and pad size;
  - the width it was routed at before;
  - whether a straight stub at the board minimum fits with clearance to the neighbouring pads.
  - **Accept:** the table is in the done note, with the count that fits and the count that
    doesn't.
  *Done 2026-10-20:* from Q18.6's KiCad grades of the pair-A sessions
  (`target/orchestrator/q18/out/grade/{before,mp1}/<row>/routed/drc.json`; pads and widths from
  `before`'s routed `.kicad_pcb`; scratch scripts `diffu.py`, `lost.py`). A pad counts when it is
  in `mp1`'s KiCad unconnected items and not in `before`'s; its width before is the width of the
  `before` tracks ending in it. **mydevboard** (unconnected 34 → 56): 29 SMD pads, and their
  other ends (11 header pins of J2-J4, the 0402s C12, C17, R3, R5, R7); by net +3V3 7 → 10, +1V1
  1 → 3, and one each on GPIO2, 5, 11, 12, 14, 16, 17, 18, 20, USB_DM, USB_DP, QSPI_SD0, SD1,
  SD3, SS, SWD, XOUT. Board: one net class, track 0.2 mm, clearance 0.2 mm; Board Setup
  `min_track_width` 0.2 mm.

  | part, package | pitch | pad | pins (net) | width before | 0.2 mm stub, gap to neighbour pad |
  |---|---|---|---|---|---|
  | U1, QFN-56 7 × 7 mm (RP2040) | 0.4 mm | 0.875 × 0.2 mm | 1 (+3V3), 4 (GPIO2), 7 (GPIO5), 14 (GPIO11), 15 (GPIO12), 17 (GPIO14), 21 (XOUT), 23 (+1V1), 25 (SWD), 27-29 (GPIO16-18), 31 (GPIO20), 33 (+3V3), 45 (+1V1), 46-47 (USB_DM, USB_DP), 48-49 (+3V3), 50 (+1V1), 51 (QSPI_SD3), 53 (QSPI_SD0), 55 (QSPI_SD1), 56 (QSPI_SS) | 0.15 mm | 0.2 mm = clearance: fits, no slack |
  | U1 | 0.4 mm | 0.875 × 0.2 mm | 42 (+3V3) | 0.15 and 0.2 mm | fits, no slack |
  | U1 | 0.4 mm | 0.875 × 0.2 mm | 43 (+3V3) | 0.2 mm (open after the floor through its net's other changes) | fits, no slack |
  | U3, USON-8 3 × 2 mm (Winbond) | 0.5 mm | 0.9 × 0.25 mm | 5 (QSPI_SD0), 6 (QSPI_SCLK) | 0.2 mm | 0.275 mm: fits |
  | U3 | 0.5 mm | 0.9 × 0.25 mm | 7 (QSPI_SD3) | 0.15 mm | 0.275 mm: fits |

  **Fits: 29, does not fit: 0.** The 26 U1 pins fit with no slack (pitch 0.4 − pad 0.1 − stub
  0.1 = 0.2 mm, exactly the clearance); the 3 U3 pins with 0.075 mm to spare, but U3 is not a
  part X10 qualifies (8 side pins and an exposed pad 0.2 mm wide, under its 2-pitch rule;
  `qualifying_parts_on_a_set` lists mydevboard's U1, 56 pins at 0.4 mm, and J1, 20 at 0.5 mm), so
  the escape can act on 26 of the 29. **slice** (12 → 11): its two newly open connections are
  through-hole (/BR1 A1-15 to U2-4, /PWM2 A1-9 to U3-5), not a fine-pitch pin, and slice has no
  qualifying part. **laser-backplane** (70 → 70) and **vfo** (14 → 14): no change.

- [x] **Q21.2 — Stubs at the board minimum.** Make the `min_half_width` that every caller of
  `krt-qfn-escape` passes include Q18's board minimum, keep the narrow stub straight along the
  pad's axis through the pin field, and skip pins where the stub doesn't fit.
  - **Accept:**
    - fixture tests, written from scratch, for a 0.4 mm-pitch QFN with 0.2 mm pads and a
      0.2 mm minimum: the stubs are 0.2 mm, run straight out past the outline, and leave the
      neighbours their clearance;
    - a pitch where nothing fits gets no stubs;
    - X10.3's rotated and mirrored copies give the same stubs in the part frame;
    - no stub is ever below the board minimum (`tests/board_minimums.rs` style check).
  *Done 2026-10-20:* the floor was already in place: `fanout::qfn::escape` (the only planner
  both callers use, the `krt-qfn-escape` fanout stage and Q21.3's `krt-pin-escape` board stage)
  takes `min_half_width = control::min_trace_half_width`, which since b98074e includes
  `router.board_minimums.track_width_um`; the narrow stub runs from the pin centre along the
  side's outward axis (`plan_pin`, `tries`), and a stub that fails the transaction's exact check
  at every try is `Skip::Rejected` with no copper. No planner code changed; the module docs of
  `fanout/qfn.rs` and `fanout/mod.rs` now say so. Fixture `crates/freerouting-krt/tests/data/
  q21-pin-escape.dsn`, written from scratch (two 16-pin QFNs with 0.8 × 0.2 mm pads, class 0.15 mm,
  clearance 0.2 mm; 0.4 mm pitch at 0° U1, 90° U3, 180° U4, back U5; 0.35 mm pitch U2, U6).
  `cargo test -p freerouting-krt --test pin_escape` (9 passed):
  `stubs_at_a_0p2_minimum_on_a_0p4_qfn_run_straight_out_and_keep_their_clearance` (minimum
  0.2 mm: U1's 16 pins each one `Narrow` stub at the first try, 0.2 mm wide, both ends within 1
  unit of the pin's lateral line, starting at the pin centre, ending past the outline and past
  escape box + clearance + half-width; the board has no clearance violation; gap to the
  neighbour pad 0.2 mm = the clearance), `a_0p35_pitch_where_no_0p2_stub_fits_gets_no_stubs` (U2
  and U6: all 32 pins `Rejected`, no items; 64 stubs on the 0.4 mm copies),
  `a_0p25_minimum_fits_nowhere_on_the_0p4_qfn` (0 stubs, every pin `Rejected`),
  `without_a_board_minimum_the_stubs_follow_the_narrowest_class` (0.15 mm stubs: the 0.2 mm above
  is the floor), `rotated_and_mirrored_copies_get_the_same_stubs_at_the_minimum` (U3, U4, U5:
  the same result per pin and every trace end within 1 unit of U1's in the part frame, equal
  widths), `no_stub_is_ever_below_the_board_minimum` (this board and X10's `x10-qfn-power.dsn`
  and `x10-qfn-rotated.dsn` at minimums 150, 200, 250 and 300 µm: the floor equals the minimum and
  every stub trace's half-width is at least it; no clearance violation afterwards).

- [x] **Q21.3 — The option.** Add `router.fanout.pin_escape`, routed through the stage registry
  (`freerouting-stages`, so the CLI and GUI get it without depending on `freerouting-krt`), and
  show it in the plugin dialog and the desktop app.
  - **Accept:**
    - tests for the setting's default and parse;
    - plugin tests for the checkbox, its saved value and the options schema bump;
    - GUI test that the dialog sets the key;
    - with the option off, connection-parity stays 7 of 7 exact under `legacy`, and the Q18
      set's sessions are byte-identical under `enhanced`;
    - a DIVERGENCES line (Java has no such escape).
  *Done 2026-10-20:* settings field `fanout.pin_escape` (`settings/fields.rs`, unset = off in
  both profiles); `autoroute::stages::pin_escape` (new: `PinEscape::{parse, of, stage}`,
  `check_settings`, `SETTINGS_KEY`, `PIN_ESCAPE_STAGE = "krt-pin-escape"`); `straight` adds that
  board stage to `StageSelection::stages` after `special.diff_pairs` (`stages/registry.rs`), an
  unknown value fails `run_pipeline` before routing, and the option is listed under the fanout
  slot of the catalog (`stages/catalog.rs`, so `--list-stages` and the GUI show it).
  `freerouting-krt` registers `KrtPinEscape` (`fanout/mod.rs`: `BeforeFanout`, X10's `escape`, its
  log lines) and `freerouting-stages` hands it to the CLI and GUI unchanged; the CLI takes
  `--router.fanout.pin_escape=straight` through the settings table. GUI: a choice option
  (`CHOICE_OPTIONS`, `Draft::set_choice`) in the Algorithms section's Fanout row, `off` clearing
  the key. Plugin: `RouteOptions.pin_escape` (route-options schema 4 → 5, 4 still read), the
  checkbox "Escape fine-pitch pins straight (QFN, QFP, SOIC)" in the Advanced tab's Flow group,
  `--router.fanout.pin_escape=straight` when on. Tests:
  - default and parse: `cargo test -p freerouting --test pin_escape_setting` (3 passed:
    `pin_escape_is_off_by_default_in_both_profiles`,
    `pin_escape_parses_off_and_straight_and_refuses_the_rest`,
    `the_cli_and_a_json_text_set_pin_escape`), `settings_defaults.rs` (`fanout.pin_escape` unset);
    `freerouting-krt/tests/pin_escape.rs`: `straight_selects_the_pin_escape_board_stage` (the
    built-in registry refuses it as an unknown stage), `an_unknown_pin_escape_value_is_refused_before_routing`,
    `off_is_byte_identical_to_unset_and_straight_logs_its_stubs`;
  - plugin (`target/orchestrator/k02/venv/bin/python -m pytest kicad-plugin/tests`: 396 passed,
    19 skipped): `test_options.py::{test_pin_escape_is_off_under_both_profiles_and_passes_its_flag_when_on,
    test_pin_escape_is_saved_per_board_and_restored, test_the_form_sets_pin_escape}`,
    `test_route_dialog.py::test_the_pin_escape_checkbox_is_saved_and_reaches_the_router`, the
    schema asserts moved to 5, `test_snapshot.py`'s allowed list;
  - GUI: `cargo test -p freerouting-gui --test ui_routing the_pin_escape_choice_sets_the_key`
    (the combo box sets `straight`, Apply keeps it, `off` clears it);
  - option off: `tools/parity-check.sh --no-probes` PARITY OK, connection-parity 7 of 7 exact
    under `legacy`; the Q18 set (`tools/bench/sets/q18-board-minimums.txt`, 12 rows, Q18.6's `mp1`
    flags with `ROW_FLAGS=1 tools/bench/quick-arm.sh`) gives 12 of 12 sessions byte-identical
    (`cmp`) between main 6b59998's release CLI and this one, with the key unset and with `off`
    (`target/orchestrator/q21/ident.sh`, `ident.log`);
  - DIVERGENCES: the settings line under `freerouting (board model)`, the "Straight pin escapes"
    paragraph, and the plugin's checkbox line.

- [x] **Q21.4 — Measure what it wins back.** Register first (`docs/roadmap/prereg/Q21.json`,
  `tools/bench/sets/q21-pin-escape.txt`):
  - **Arms:** off and `straight`.
  - **Boards:** the KiCad 6+ rows that lost connections in Q18.6 pair A, plus canary-pwr (U1,
    38 pins at 0.5 mm), each with its Board Setup minimums as the plugin passes them.
  - **Time:** a few minutes in all, one KiCad grade per distinct session.
  - **Signal:** KiCad unconnected.
  - **Guards:** zero routing-caused manufacturing breaches and zero vias in pads, in both arms.
  - **Accept:**
    - the per-board table, with how many of Q21.1's connections came back;
    - the option is kept unless it routes worse on every board it acts on, in which case the
      owner decides whether it stays.
  *Done 2026-10-20:* registered before any route (`docs/roadmap/prereg/Q21.json`, sha256
  8431ffbc…, pinned by `crates/freerouting-dev/tests/prereg_q21.rs`; set
  `tools/bench/sets/q21-pin-escape.txt`: mydevboard, the only KiCad 6+ pair-A row whose KiCad
  unconnected rose with the floor, and canary-pwr's project at canary 9511d9a3, extracted to
  `target/orchestrator/q21/canary-pwr-9511d9a3`; slice left out, see Q21.1). One release build
  of land/q21 (`target/orchestrator/q21/target/release/freerouting`, sha256 b153084b…, `git
  6b5999803f2f` plus the uncommitted Q21 diff). Every row routed and graded inside
  `tools/kicad-oracle/grade.sh --export plugin` (the plugin's export, fix-ups, flags with the
  Board Setup minimums, and import; KiCad 10.0.7) with `-mp 1
  --router.scoring.start_ripup_costs=1500`, arm options `{"schema": 5, "profile": "enhanced"}`
  and the same plus `"pin_escape": true`; `ulimit -v 8000000`, timeout 120 s / 600 s, none hit;
  one KiCad grade per session, kicad-cli waited for (`target/orchestrator/q21/run-q21.sh`,
  outputs `target/orchestrator/q21/out/`). 3.4 min in all (17, 16, 84, 87 s).
  Manufacturing breaches as registered (routing-caused clearance, edge, hole, width, via,
  annular, drill, short, crossing, mask bridge, not-allowed items).

  | row | arm | KiCad unconnected | manufacturing breaches | below minimum | vias in pads | drc_real (routing-caused, by type) | Q21.1 pads open | stubs | tracks / vias / length mm |
  |---|---|---|---|---|---|---|---|---|---|
  | mydevboard | off | 57 | 0 | 0 | 0 | 0 | 28 of 29 | – | 201 / 17 / 324.3 |
  | mydevboard | straight | **44** | 0 | 0 | 0 | 0 | 19 of 29 | U1 56, J1 6 | 260 / 21 / 462.1 |
  | canary-pwr 9511d9a3 | off | 52 | 0 | 0 | 0 | 5 (starved_thermal 5) | – | – | 1368 / 120 / 1887.4 |
  | canary-pwr 9511d9a3 | straight | **51** | 0 | 0 | 0 | 6 (starved_thermal 6) | – | U1 37 (4 widened), U3 5 (4 rejected), U6 5, U8 8 | 1556 / 123 / 1877.3 |

  Q21.1's connections: of its 29 pads, the off arm reaches 1 (U1-43) and straight 10 (U1-14,
  15, 25, 31, 43, 46, 47, 49, 51 and U3-7); the 19 still open include U3-5 and U3-6, which the
  stage does not act on. **Keep rule:** straight routes better on both boards it acts on
  (unconnected −13 and −1), with zero manufacturing breaches, zero copper below the minimums and
  zero vias in pads in both arms: the option is kept, off by default in both profiles.
  starved_thermal (a zone's thermal spokes, reported, not judged by the registration) is 5 → 6 on
  canary-pwr.

  *N05.3 judged the promotion 2026-10-20* (`docs/roadmap/prereg/N05-pin-escape.json`, seven boards): total
  156 -> 147 KiCad unconnected, but the gate failed one hard clause (ch32v003: 30 -> 32 manufacturing
  breaches of custom rules the DSN lacks), so `pin_escape` stays off in both profiles. See N05.3.
