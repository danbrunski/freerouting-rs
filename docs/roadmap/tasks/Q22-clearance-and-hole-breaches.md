# Q22 — Routed output keeps the board's copper and hole clearance

Opened 2026-10-20 from the grades of N04.3 and Q16.5 ([archive/N04](archive/N04-contacts-inside-pads.md),
[archive/Q16](archive/Q16-no-dangling-stubs.md)). Decided 2026-10-20: routing never breaks the
board's manufacturing rules, graded under the board's own rules. Routing-caused `clearance` and
`hole_clearance` are 0 in every arm.

N04.3's `center` arm is the default `enhanced` (main `b98074e`, Board Setup minimums passed,
`grade.sh --keep-tracks`; grades in `target/orchestrator/n04q16/out/n04/center/<row>/`). Its
KiCad `drc_real_by_type` counted these routing-caused items:

| Row | clearance | hole_clearance | below_minimum |
|---|---|---|---|
| pcietest | 237 | 70 | 0 |
| cm4-edge-ai | 25 | 0 | 0 |
| nudac | 0 | 18 | 0 |
| ch32v003 | 5 | 0 | 0 |
| mydevboard | 1 | 0 | 0 |
| vna-usb-pd | 2 | 0 | 0 |
| picodebugger | 1 | 0 | 0 |
| canary-pwr | 0 | 0 | 4 (`annular_width`) |

Q16.5's base arm (the plugin path, `target/orchestrator/n04q16/out/q165/runs/q165-base/`) had 1
`clearance` on sonde-xilinx.

Q22.1 found that 41 of these 364 items involve copper the router placed. All 41 cross copper or a
rule that the DSN never gave the router. Another 31 are input tracks and vias that the DSN round
trip moved by less than 1 µm. The remaining 292 come from the grader. No routed item breaks a
rule that the DSN carries.

Q20's output check can only see what the router knows. Q22.4 and Q22.5 are what let it see these
items.

Prerequisites: Q18 (the floors and the minimums), Q19 (grading under the board's own rules), F02.6
(the KiCad oracle).

## Decisions made here

- **What counts as routing-caused.** A KiCad item is routing-caused when at least one of its items
  is a track or via that the input board doesn't have with the same coordinates (to the
  nanometre), width, layer and net.
  - Pads, footprint graphics, text, zones and unchanged input tracks and vias are input copper.
  - An item made only of input copper is pre-existing, whatever the input report listed. KiCad
    stops a type at 199 items and lists a pair once per copper layer, so the two reports don't
    list the same pairs.
  - Moved input copper counts as routing-caused (Q22.2).
- **Copper that was on the board comes back at its own coordinates** on the DSN path (Q22.3).
  K13.2 does the same for the `.kicad_pcb` path.
- **Whatever the DSN export drops becomes an obstacle or a rule, never an exception in the
  grader.** That covers:
  - footprint copper graphics, and board copper text and graphics (Q22.4);
  - `.kicad_dru` clearances between item types (Q22.5);
  - pad-local clearances on the grader's path (Q22.6).

  These are fixes to the input, so they apply in every profile, as Q18's floors do. The parity
  fixtures are bare DSNs and don't change. A connection that only fits through such copper stays
  open. Its cost in unconnected items is reported, not judged (as in Q18).
- **The grader can route the DSN the plugin writes** (Q22.6), so that a measurement and a plugin
  run see the same rules. The bare export stays selectable.
- **For Q20:** the engine DRC's `router.clearance_tolerance_um` defaults to 1.0 µm. That would let
  through the 0.5–0.8 µm breaches KiCad reported here, so Q20.1's check needs a tolerance of 0.

## File ownership

| Item | New files | May edit |
|---|---|---|
| Q22.2 | `tools/kicad-oracle/tests/test_routing_caused.py` | `tools/kicad-oracle/oracle_lib.py` (`subtract_preexisting`, `summarize`), `tools/kicad-oracle/kicad_steps.py` (`import_session`, a copper-geometry dump), `tools/kicad-oracle/grade.sh` (the summarize call), `tools/kicad-oracle/README.md` |
| Q22.3 | `kicad-plugin/freerouting_rs/restore_copper.py`, `kicad-plugin/tests/test_restore_copper.py` | `kicad-plugin/freerouting_rs/export_flow.py` (`after_import` only), `kicad-plugin/freerouting_rs/plugin.py` (`import_ses`: hand the detached originals to the restore), `kicad-plugin/headless_run.py` (`import`), `tools/kicad-oracle/kicad_steps.py` (`import_session`) |
| Q22.4 | `kicad-plugin/freerouting_rs/copper_graphics.py`, `kicad-plugin/tests/test_copper_graphics.py`, `crates/freerouting-kicad/tests/copper_graphics.rs`, a fixture under `crates/freerouting/tests/data/kicad/` | `kicad-plugin/freerouting_rs/kicad_board.py` (`collect`), `kicad-plugin/freerouting_rs/dsn_fixups.py` (`apply`), `kicad-plugin/freerouting_rs/export_flow.py` (`fix_dsn`), `crates/freerouting-kicad/src/{footprints.rs,read.rs}` |
| Q22.5 | `crates/freerouting-kicad/tests/dru_item_types.rs`, a fixture under `crates/freerouting/tests/data/kicad/` | `crates/freerouting-kicad/src/dru.rs`, `crates/freerouting/src/io/kicad.rs`, `crates/freerouting/src/job/prepare.rs`, `crates/freerouting-cli/src/{args.rs,help.txt}`, `kicad-plugin/freerouting_rs/export_flow.py` (`router_args`), `docs/DIVERGENCES.md` |
| Q22.6 | `tools/kicad-oracle/tests/run-plugin-export-test.sh` | `tools/kicad-oracle/grade.sh`, `tools/kicad-oracle/README.md`, `kicad-plugin/headless_run.py` |
| Q22.7–Q22.8 | `docs/roadmap/prereg/Q22.json`, `tools/bench/sets/q22-clearance.txt`, `crates/freerouting-dev/tests/prereg_q22.rs` | — |
| Q22.9 | — (runs only, under `target/orchestrator/q22/`) | — |
| Q22.10 | `crates/freerouting/src/job/clearance_floor.rs`, `crates/freerouting/tests/clearance_floor.rs`, `kicad-plugin/tests/test_min_clearance.py` | `crates/freerouting/src/settings/fields.rs` (`board_minimums`), `crates/freerouting/src/job/overrides.rs` (`apply_all`), `crates/freerouting/src/job/prepare.rs` (`board_minimums_patch`), `crates/freerouting-kicad/src/project.rs` (`BoardMinimums`), `kicad-plugin/freerouting_rs/options.py` (`BOARD_MINIMUM_SETTINGS`), `docs/DIVERGENCES.md` |

Sequencing:
- **Q22.2 and Q22.6 can start now.** They touch only `tools/kicad-oracle/` and
  `headless_run.py`.
- **Q22.3 and Q22.4 wait for Q18.5 and Q18.7.** Both edit `export_flow.py` (different functions),
  and Q22.4 also edits `dsn_fixups.py`, `kicad_board.py` and `crates/freerouting-kicad/src/`. Q18.5
  (Edge.Cuts and Margin keepouts) and Q18.7 (solder mask) are editing those files now. Q22.4 should
  reuse the keepout writer they add.
- **K13.1–K13.4 also own those files**, `crates/freerouting-kicad/src/**` and `export_flow.py`.
- **No Q22 item touches Q20's files** (`crates/freerouting/src/drc/`, `quality/`).

## Checklist

- [x] **Q22.1 — Find the causes.** Classify every routing-caused `clearance`, `hole_clearance` and
  `below_minimum` item of N04.3's `center` arm and Q16.5's sonde-xilinx:
  - (i) a rule or copper that the DSN doesn't carry;
  - (ii) a geometry or rounding shortfall in the router;
  - (iii) a stage that skips its clearance check;
  - (iv) a drill that the router models differently;
  - (v) something else.

  Then answer canary-pwr's 18 → 26 from the saved records.
  - **Accept:**
    - counts per row and cause;
    - one worked example per cause, with coordinates, the rule KiCad applied and what the DSN
      says for that pair;
    - the canary answer, with what remains for Q22.9.
  - *Done 2026-10-20:* read-only, from saved grades. No route was run, no build was made and no
    KiCad run was needed.
    - **Method.** Scripts in `target/orchestrator/q22/scratch/`:
      - `classify.py ROW_DIR [verbose]` uses `oracle_lib.subtract_preexisting` and then
        `pcbcopper.py`, which matches each item's uuid in the routed board to the input board
        (to the nanometre, then to 1 µm).
      - `copper_graphics.py` counts copper graphics and text.
    - **Counts.** The causes below split the 364 items:

      | Row | Items | (i) DSN lacks it | (v-a) export moves input copper | (v-b) input violation counted again | (v-c) grader's import | (ii) / (iii) / (iv) |
      |---|---|---|---|---|---|---|
      | pcietest | clearance 237, hole 70 | 38 | 0 | 269 (199 + 70) | 0 | 0 |
      | cm4-edge-ai | clearance 25 | 0 | 25 | 0 | 0 | 0 |
      | nudac | hole 18 | 0 | 0 | 18 | 0 | 0 |
      | ch32v003 | clearance 5 | 2 | 3 | 0 | 0 | 0 |
      | mydevboard | clearance 1 | 0 | 1 | 0 | 0 | 0 |
      | vna-usb-pd | clearance 2 | 0 | 2 | 0 | 0 | 0 |
      | picodebugger | clearance 1 | 0 | 0 | 1 | 0 | 0 |
      | canary-pwr | annular_width 4 | 0 | 0 | 0 | 4 | 0 |
      | sonde-xilinx (Q16.5) | clearance 1 | 1 | 0 | 0 | 0 | 0 |
      | **total** | 364 | **41** | **31** | **288** | **4** | **0** |

      - Only the 41 items under (i) involve routed copper.
      - The 31 under (v-a) involve only input tracks and vias, each within 1 µm of where it was.
        The input grades had nothing for those pairs.
      - The 288 under (v-b) are made only of input copper and repeat the input's own breaches.
      - (ii), (iii) and (iv) are 0: no routed item breaks a clearance that the DSN gives the pair,
        and every `hole_clearance` item is input copper only.
    - **(i), footprint copper graphics (pcietest, 38).**
      - What was routed: `Track [unconnected-(J8-Pin_2-Pad2)]` on B.Cu, (143.18, 130.48)–(143.18,
        133.52), 0.1778 mm wide. It joins J8's two pads numbered 2 (footprint `matseng:WW37`,
        `duplicate_pad_numbers_are_jumpers no`).
      - What it crosses: the footprint's own `fp_line`, 0.5 mm wide on B.Cu between the same two
        pads. KiCad calls it `Segment [<no net>] of J8`.
      - KiCad's verdict: `Clearance violation ( clearance 0.1778 mm; actual 0.0000 mm)`. 0.1778 mm
        is the Default class; the line has no net.
      - What the DSN says: the net is `(pins J8-2 J8-2@1)`. The image `matseng:WW37` carries the
        37 lines only as `(outline (path signal 500 …))`. An outline is drawing only: the module
        docs of `crates/freerouting-kicad/src/image_outlines.rs` say so ("they do not influence
        routing").
      - All 38 items are the same case: 3.04 mm tracks between duplicate pads of J5, J6, J8 and
        J11–J14, laid over those lines.
    - **(i), a `.kicad_dru` item-type rule (ch32v003, 2).**
      - What was routed: `Track [unconnected-(J1-SHIELD-PadS1)]` on B.Cu, (145.9956,
        91.9703)–(152.1207, 91.9703), 0.2032 mm wide.
      - What it passes: the input GND vias (Ø0.6 mm) at (150.05, 92.56) and (148.05, 92.57), at
        0.1881 and 0.1981 mm.
      - KiCad's verdict: `rule 'Pad to Track clearance)' clearance 0.2000 mm`. The rule is
        `(condition "A.isPlated() && B.Type == 'track'")` in the project's `.kicad_dru`, and a via
        is plated.
      - What the DSN says: `(clearance 127)`, in the structure and in all six classes. The router
        kept at least 0.127 mm.
      - KiCad's export ignores the `.kicad_dru`. `dru.rs` (K07.5) reads it only for a `.kicad_pcb`
        input, and reports item-type conditions as unsupported.
    - **(i), copper text (sonde-xilinx, Q16.5 base, plugin path, 1).**
      - What was routed: `Track [/PWR_3,3-5V]` on F.Cu, (169.672, 80.137)–(162.952, 80.137),
        0.635 mm wide.
      - What it crosses: `gr_text "GND"` on F.Cu at (162.56, 81.28), 1.524 mm high with a
        0.381 mm stroke.
      - KiCad's verdict: `( clearance 0.2540 mm; actual 0.0000 mm)`, plus one `shorting_items`
        between `<no net>` and `/PWR_3,3-5V`.
      - What the DSN says: `(clearance 254)`, no keepout, and nothing for the text.
      - The same gap is latent on other rows. Copper text: sonde-xilinx 8, pic_programmer 19,
        complex_hierarchy 2. Copper graphics: vfo 3 `gr_poly`, flight-computer 1 `fp_poly`,
        slice 1 `fp_poly`. N04.3's routes happened to miss them.
    - **(v-a), the export rounds input copper (cm4-edge-ai 25, ch32v003 3, mydevboard 1, vna-usb-pd
      2).**
      - The input: the diff pair `/CM4 Core/PCIe_RX_N`/`_P` on F.Cu, 0.2 mm wide. N runs
        (194.175, 75.427512)–(192.322488, 73.575) and P runs (193.825, 75.572488)–(192.177512,
        73.925). The gap is 0.15000 mm, the PCIe class's `diff_pair_gap`.
      - The export: KiCad's `ExportSpecctraDSN` prints coordinates with `%.6g`
        (`crates/freerouting-kicad/src/fmt6g.rs`), which is 1 µm at 100 mm and beyond. The DSN wire
        reads `(path F.Cu 200 194175 -76142.9 194175 -75427.5 192322 -73575)`.
      - The round trip: the router passes the wire through unchanged (SES `1923220 -735750`, in
        0.1 µm units). `ImportSpecctraSES` rebuilds it at (192.322, 73.575) and (192.178, 73.925),
        and the gap becomes 0.14933 mm.
      - KiCad's verdict: `netclass 'PCIe' (diff pair) clearance 0.1500 mm; actual 0.1493 mm`.
      - What the DSN says: class `PCIe,Default` with `(clearance 200)`. It has no diff-pair gap.
      - All 31 are 0.5–0.8 µm short:
        - cm4: 23 Track–Track at 0.7 µm (diff pairs), Track–Via 0.5 µm, Pad–Via 0.6 µm;
        - ch32v003: 3 at 0.6 µm;
        - mydevboard: 1 at 0.5 µm (via (198.06689, 106.69344) → (198.067, 106.693));
        - vna-usb-pd: 2 at 0.8 µm (via (89.141384, 121.858616) → DSN `89141.4 -121859`).

      The plugin's default path does the same round trip. Only `fix_existing` locks the input
      copper so that `ImportSpecctraSES` keeps it.
    - **(v-b), input breaches counted again (pcietest 269, nudac 18, picodebugger 1).**
      - picodebugger: `PTH pad 35 [unconnected-(PicoProbe1-ADC_VREF-Pad35)]` and the input
        `Track [/PP_UART_Tx]` (65.151, 86.868)–(89.4795, 86.868) give `( clearance 0.2000 mm;
        actual 0.1730 mm)` in both grades. The routed report lists the pad first and the input
        report lists the track first. `subtract_preexisting` matches on the first item's
        position, and those positions are 13.1 mm apart.
      - pcietest: all 269 are pairs of a connector's `PTH pad N` and its own footprint line (actual
        0, or the line over the pad's hole). `shorting_items` is at KiCad's 199 cap in both
        grades, so each grade lists a different subset, under `shorting_items`, `clearance` or
        `hole_clearance`.
      - nudac: input vias and tracks sit 0.235 mm from a 0.25 mm hole clearance, 15 µm short, in
        the input's own design. The export moved them by less than 0.5 µm. KiCad lists a pair up
        to once per copper layer, and the two grades list different pairs.
        - Matched as unordered pairs, 57 of the 60 routed `hole_clearance` items have a twin in
          the input.
        - The rest measure 0.235 mm on the input geometry too. For example, `Track [I2C0_SDA]`
          (23.622, 10.80284)–(23.622, 11.747), 0.25 mm wide, against `Via [USB_DN]` (24.132,
          11.477515) with a 0.3 mm drill.
    - **(v-c), the grader's import (canary-pwr, 4).** The 4 `annular_width` items are vias the
      router placed.
      - Example: `Via [+12V_BUS]` at (144.0234, 55.2516), with the SES padstack
        `Via[0-3]_600:300_um` (a 0.15 mm ring).
      - `kicad_steps.py import_session` calls only `ImportSpecctraSES`. KiCad takes the drill
        from the Power_High class (`Via[0-3]_800:400_um`, 0.4 mm), so the via becomes 0.6/0.4.
      - KiCad's verdict: `min annular width 0.1500 mm; actual 0.1000 mm`.
      - The plugin's `export_flow.after_import` (`kicad_board.fix_via_drills`, K09) sets the drill
        back to 0.3. Q16.5's plugin-path grade has no `annular_width`.
    - **The `inside_pad` arm** has the same causes, plus one more (i) on canary-pwr. `Track [GND]`
      passes 0.5235 mm from FID3's no-net fiducial, against `pad clearance 0.6000 mm`. The plugin's
      `edge_and_pad_keepouts` carries pad-local clearances, but `grade.sh`'s bare export doesn't.
    - **Canary-pwr, 18 → 26.**
      - **The 18 has no saved session or grade.** The record is canary commit `d6199b48`'s
        message: freerouting-rs `b93bcd0`, "the operator's options", 18 at rip-up cost 1500 and at
        1400, "clean, both In2 islands whole". Searching `target/orchestrator` and
        `target/canary-handoff` for the board's sha256 `25395aeb…` finds only Q16.5's runs.
      - **By that record, the 18 broke no rule.** Canary's `hardware/tools/grade.sh` at
        `d6199b48` counts a result only when the only errors are unconnected items and starved
        thermals. The board's `.kicad_pro` has `clearance`, `hole_clearance`,
        `copper_edge_clearance`, `annular_width` and `track_width` at error. So the 26 is not the
        honest version of the 18. The two runs used different setups.
      - **The options differ.** The 18 used the preset in
        `../canary/hardware/canary-pwr-rev1.0/freerouting-rs/route-options.json`: `max_passes` 1,
        `start_ripup_costs` 1500, `sweep_passes` 2, class widths, Power_High and Switch_Node
        pours fixed, and GND's outer pours dropped. That file is dated 6 h after `d6199b48`;
        assumed to be the same preset. The 26 used `{"profile": "enhanced"}`: rip-up 100
        ("source: defaults"), 18 passes and then the sweep, and every pour exported as a plane.
        The code also differs by 56 commits (`b93bcd0..b98074e`).
      - **No floor explains the gap.**
        - The track floor of 150 µm is the 18-run's own `min_trace_width_um=150`.
        - Both via types in the DSN (600:300 and 800:400) meet 400/300/150, so no via is made at
          floor size and `route.log` has no via warning.
        - The plugin's 0.3 mm edge strips were in both runs.
        - Only the 250 µm hole clearance is new ("28 hole keepouts reclassified": 7 NPTH holes on
          4 layers). No unconnected end lies within 3 mm of one of them.
      - **11 of the 26 unconnected are pour islands.** These are zone–zone or zone–pad pairs on
        GND (3), +12V_BUS (2), +12V_PROT, +5V_PI, +5V_BUCK, LTC_SW, +12V_RAW and VCAP. Fixed pours
        and dropped GND pours avoid that kind.
      - **Verdict: the run setup, not a floor.** No run has checked this yet; Q22.9 runs it.

- [x] **Q22.2 — The grader counts what routing caused.** Two changes to the grader:
  - `oracle_lib` takes the routed and input copper (uuid → geometry, from both boards) and applies
    this checklist's definition. An item with a track or via that the input doesn't have, at the
    same nanometre geometry, is routing-caused. Any other item is pre-existing, whatever the input
    report listed.
  - `kicad_steps.py import_session` applies the plugin's via-drill correction (`ses_vias.vias`
    and `kicad_board.fix_via_drills`), loading those modules by path and never through the plugin
    package.
  - **Accept:**
    - unit tests, written from scratch:
      - an input pair listed in the other order is pre-existing;
      - an input pair the input report left out, as under the 199 cap, is pre-existing;
      - a moved input track is routing-caused;
      - a routed track is routing-caused;
    - the saved N04.3 `center` grades, summarized again with no new KiCad run:
      - pcietest: `clearance` 237 → 127 and `hole_clearance` 70 → 0 (amended 2026-10-20, was 38);
      - picodebugger: 1 → 0;
      - nudac `hole_clearance` 18 → 54 (amended 2026-10-20, was unchanged);
      - cm4, ch32v003, mydevboard and vna-usb-pd unchanged, because their copper moved
        (that is Q22.3);
    - canary-pwr's saved `center` session, imported again and graded with one DRC:
      `annular_width` 4 → 0;
    - `python3 -m unittest discover -s tools/kicad-oracle/tests -p 'test_*.py'` passes.
  - *Partial 2026-10-20:* the code, the tests and the canary and picodebugger clauses are in. Two
    numbers of the second Accept clause differ from the prediction, and the grader is right in both
    (the prediction came from Q22.1's classification of the old `real` set).
    - **Done.**
      - `oracle_lib.read_copper` reads the segments, arcs and vias of both `.kicad_pcb` files (uuid
        to nanometre geometry), and `split_routing_caused` applies the definition above. `grade.sh`
        passes both boards to `summarize` (`routing_caused_basis: copper`; five arguments keep the
        old match and say `report`).
      - `kicad_steps.py import` runs the plugin's `ses_vias.vias` and
        `kicad_board.fix_via_drills`, loaded by path (through `Tracks()`, which works in 10.0.5).
      - `starved_thermal` and `isolated_copper` items name a zone and no track, so they keep the old
        report match (`FILL_TYPES`, pinned by `FillTypesTest`): a thermal that routing starved still
        counts. This is the one place the definition was narrowed.
      - Tests: `tools/kicad-oracle/tests/test_routing_caused.py` (18: pair in the other order,
        pair absent from the input report, moved track at 1 nm either way, moved or re-drilled
        via, routed track, routed track over a footprint line with an input twin, arc, unknown
        uuid, FILL_TYPES, `summarize`, the `summarize` command) and `test_import_via_drill.py`
        (needs `pcbnew`; a `Via[0-1]_600:300_um` on a 0.8 / 0.4 mm class: 0.6 / 0.4 without the fix,
        0.6 / 0.3 with it, checked by removing the call). `python3 -m unittest discover -s
        tools/kicad-oracle/tests -p 'test_*.py'`: 35 tests OK.
      - Saved N04.3 `center` grades, summarized again with no KiCad run
        (`oracle_lib.py summarize` with the two boards, `drc_real_by_type` old to new):

        | Row | Before | After |
        |---|---|---|
        | picodebugger | clearance 1 | none (pair listed in the other order) |
        | pcietest | clearance 237, hole_clearance 70, shorting_items 68 | clearance **127**, hole_clearance 0, shorting_items 125 |
        | cm4-edge-ai | clearance 25 | clearance 25 |
        | ch32v003 | clearance 5, starved_thermal 1 | clearance 5, starved_thermal 1 |
        | mydevboard | clearance 1 | clearance 1 |
        | vna-usb-pd | clearance 2 | clearance 2 |
        | nudac | hole_clearance 18 | hole_clearance **54** |

      - canary-pwr's saved `center` session, imported again with `kicad_steps.py import` (the
        fix set 4 vias from drill 0.4 to 0.3: 123 + 4 vias at 0.6 / 0.3, none at 0.6 / 0.4) and
        graded with one `kicad-cli` DRC: `annular_width` 4 to 0 (`track_dangling` 1 and
        `via_dangling` 33 unchanged; KiCad unconnected 48 as before).
    - **The two differences.**
      - pcietest `clearance` is 127, not 38. Of the 127 routed items, 89 were hidden by the old
        match: the input report lists J8's pad 1 against the footprint line at the same spot, with
        the track's net (`[unconnected-(J8-Pin_1-Pad1)]`), and the old matcher called the routed
        track against that line its twin. All 127 involve a track that has no nanometre or 1 µm
        match in the input (checked with a throwaway script over the saved boards: 127 `new`, 0 `moved`). Q22.1's 38 counted only the items the old match
        left. `shorting_items` 68 to 125 is the same cause. All of it is Q22.4's.
      - nudac `hole_clearance` is 54, not 18. 54 items involve an input track or via that the DSN
        round trip moved by under 1 µm (39 with one moved item, 15 with two). The old match hid
        36 of them against the input's twin. They are routing-caused by this item's definition
        ("moved input copper counts"), and Q22.3 restores them.
    - **Other rows that the definition changes** (all moved input copper, to be restored by Q22.3):
      vfo-controller 0 to `copper_edge_clearance` 3, `track_dangling` 1, `via_dangling` 1;
      openmuscle-flexgrid 0 to `via_dangling` 1. flight-computer `track_dangling` 1 to 0 (an input
      track). pcietest `solder_mask_bridge` 103 and `track_dangling` stay pre-existing pad pairs,
      and `track_dangling` 1 appears (moved).
    - *Done 2026-10-20:* decided 2026-10-20: the second Accept clause is amended to the grader's
      numbers (pcietest `clearance` 127, nudac `hole_clearance` 54). The predicted 38 and
      "unchanged" came from Q22.1's classification of the old matcher's `real` set. The 127 are
      Q22.4's (routed tracks over footprint copper lines), the 54 Q22.3's (input copper moved
      under 1 µm). The `FILL_TYPES` narrowing stays. Every clause above now holds.

- [x] **Q22.3 — Input copper comes back at its own coordinates.** After `ImportSpecctraSES`, each
  imported track or via that matches an unlocked original of the same net, layer and width (or
  size and drill) gets the original back. "Matches" means within the export's `%.6g` bound: 0.5 µm
  per coordinate under 1 m, plus the session's 0.1 µm.
  - A routed track end within that bound of a restored end moves onto it.
  - Anything moved further stays as imported.
  - The plugin (`after_import`), `headless_run.py import` and the grader's `import_session` all
    run the same code.
  - **Accept:**
    - plugin tests with the stub `pcbnew`:
      - an input track that `%.6g` rounds comes back at its nanometre coordinates;
      - a routed track that ends on it follows;
      - a track the router moved 2 µm stays as imported;
    - the saved N04.3 `center` sessions of cm4, ch32v003, mydevboard, vna-usb-pd and nudac, each
      imported again with one KiCad DRC:
      - routing-caused `clearance`: cm4 25 → 0, ch32v003 5 → 2 (the Q22.5 pair), mydevboard 1 → 0,
        vna-usb-pd 2 → 0;
      - nudac's `hole_clearance` 18 → 0;
      - KiCad unconnected unchanged on all five;
    - `python3 -m pytest kicad-plugin/tests` passes.
  - *Done 2026-10-20:* new `kicad-plugin/freerouting_rs/restore_copper.py` (`originals`, `restore`,
    `bound_nm`, `log_line`; pure Python, pcbnew items passed in).
    - **What it does.** The originals are the board's unlocked tracks and vias, read as plain tuples
      before `ImportSpecctraSES` (the same set `plugin.detach_unlocked_tracks` takes off; with
      `fix_existing` that set is empty, because the K09 locks keep the copper). A match has the same
      kind, net and layer (a via: its layer pair), the same width within the session's 0.1 µm (a
      via: the same size and drill, after `fix_via_drills`), and every coordinate within 600 nm
      (`%.6g` under 1 m plus the session) on the DSN path or 200 nm on the `kicad_pcb` path, a
      track in either direction. A restored item gets the original's coordinates, width and uuid
      (the uuid is K13.2's). Then the ends of the unrestored imported items within the bound of a
      restored end of their net move onto it, unless that leaves a track of length 0.
    - **Determinism.** The index is keyed by (kind, net, layer, grid cell) and built in board order.
      Every candidate pair is sorted by (squared distance, the original's order, the imported item's
      order) and taken greedily, each item once.
    - **Call sites.** `export_flow.after_import(board, ses, pcbnew, originals, board_input)` runs it
      after the drill fix and before the refill. `plugin.Dialog.finish_import` reads the originals
      from the board just before `import_ses` detaches them. `import_ses` keeps its signature, because
      `test_threads.py` plants code at `def import_ses(board, ses):`. `headless_run.py import` reads
      them after the `fix_existing` locks. `tools/kicad-oracle/kicad_steps.py import` loads the module
      by path and uses the DSN bound. `plugin.log` gets `input copper: N track(s) and M via(s) back at
      their own coordinates, K routed end(s) moved onto them, U input item(s) left as routed (Q22.3)`.
    - **Stub-`pcbnew` tests** (`kicad-plugin/tests/test_restore_copper.py`, 15):
      - `test_an_input_track_that_the_export_rounds_comes_back_at_its_nanometre_coordinates`: cm4's
        `PCIe_RX_N` end 192.322488 mm, rounded to 192.322 by `%.6g`, comes back to the nanometre;
      - `test_a_routed_track_that_ends_on_a_restored_track_follows_it`: another net's track does
        not follow;
      - `test_a_track_the_router_moved_2_um_stays_as_imported`: and so does a routed track ending on
        it;
      - the bound at 600/601/200 nm; vias with the same and with another drill; a reversed track
        that gets the original's direction and a 158.75 µm width; another width, layer or net; nearest
        first, then board order; locked tracks and arcs; the import order not changing the result;
        uuids; a 0.5 µm router stub that would shrink to nothing (cm4-edge-ai, below);
      - through the stub `wx`/`pcbnew` Route flow: `test_route_puts_the_input_copper_back_after_the_import`
        (log line checked), `test_a_fully_routed_board_on_the_kicad_pcb_path_comes_back_unchanged`, and
        `test_fix_existing_keeps_the_input_copper_and_restores_nothing`.
      - With `restore` replaced by a no-op, 7 of the 10 unit tests written by then failed. The 3
        that passed check items that must stay as imported.
    - **Real pcbnew:** `tools/kicad-oracle/tests/test_import_restore.py` (KiCad 10.0.7, `kicad_steps.py
      import` on `make_legacy_board.py`'s board). A track moved 0.4 / 0.3 µm and a via moved 0.2 / 0.4 µm
      come back with their uuids, a routed end follows, and a track moved 2 µm stays.
    - **The saved N04.3 `center` sessions**, imported again
      (`target/orchestrator/q223/scratch/regrade.sh ROW`: a copy of
      `n04q16/out/n04/center/ROW/`, `kicad_steps.py import` of its `unrouted` board and `noplace.ses`,
      the original `.kicad_pro` put back, one `kicad-cli pcb drc --severity-all --refill-zones`
      (KiCad 10.0.7), `kicad_steps.py stats`, `oracle_lib.py summarize` with both boards). "Before" is
      the saved routed board summarized by this checkout's `oracle_lib` (Q22.2's numbers). Outputs are
      in `target/orchestrator/q223/regrade/ROW/`.

      | Row | Routing-caused before | Routing-caused after | KiCad unconnected | Restored tracks / vias, ends moved, originals left as routed |
      |---|---|---|---|---|
      | cm4-edge-ai | clearance 25 | none | 0 → 0 | 705 / 136, 101, 13 |
      | ch32v003 | clearance 5, starved_thermal 1 | clearance **2** in three runs, **1** in the last (`rule 'Pad to Track clearance)'`, the routed `J1-SHIELD` track against the GND vias at 0.1881 and 0.1981 mm: Q22.5's pair) | 0 → 0 | 252 / 29, 44, 3 |
      | mydevboard | clearance 1 | none | 0 → 0 | 479 / 41, 3, 0 |
      | vna-usb-pd | clearance 2 | none | 0 → 0 | 149 / 43, 0, 0 |
      | nudac | hole_clearance 54 | none (61 `hole_clearance` on the routed board, 61 on the input, all pre-existing) | 0 → 0 | 539 / 194, 1, 5 |

      - nudac is 54 → 0, not 18 → 0: Q22.2 amended its count to 54.
      - ch32v003's 2 or 1 is KiCad's reporting, not the copper. All four imports restored the same
        items (252 / 29 / 44 / 3), and the routed track is the same.
        - The last run listed only the 0.1981 mm via. KiCad's DRC reports one clearance item per
          track unless "report all track errors" is on, and which via it names follows the order of
          the board's items.
        - The routed items get new uuids on every import, and so a new order. K13.2 found the same
          effect.
        - Q22.5 removes both.
      - ch32v003's `starved_thermal` also went (U1 pad 1 against the GND zone now has 2 spokes, as on
        the input).
      - The first cm4 run gave one `tracks_crossing`. The router had joined a pad centre (179.2875 mm)
        to the rounded end of an input track (179.288) with a 0.5 µm stub. Following both ends left a
        zero-length track, which KiCad reports as `tracks_crossing`. The fix is that a follow never
        leaves a zero-length track
        (`test_a_routed_stub_that_would_shrink_to_nothing_stays_as_imported`). All five rows were
        imported and graded again after that fix, after the uuid restore and after the last code
        change (restored ends indexed by grid cell). The table is the last run.
    - `python3 -m pytest kicad-plugin/tests` (venv `target/orchestrator/k02/venv`): 436 passed, 19
      skipped. `python3 -m unittest discover -s tools/kicad-oracle/tests -p 'test_*.py'`: 36 tests OK.

- [x] **Q22.4 — Copper graphics and copper text are obstacles.** On copper layers:
  - footprint lines, rectangles, polygons, arcs, circles and visible text;
  - board-level graphics and text.

  Each one reaches the router as a keepout on its layer. Its clearance is the one KiCad applies:
  the larger of the net's class clearance and the Default class's. A copper shape that KiCad gives
  a net becomes fixed copper of that net instead. Net-tie footprints keep their ties. This lives in
  the plugin's DSN fix-ups and in the `.kicad_pcb` reader, with K13.1's table row "copper text".
  - **Accept:**
    - a plugin test and a reader test on one fixture written for the purpose: a B.Cu `fp_line`
      between two same-numbered pads, and an F.Cu `gr_text` on the only short path. Both become
      keepouts, the same in both inputs;
    - an engine test on that fixture: the route goes round them, or the connection stays open;
    - pcietest routed with its `center` flags through the fixed export: 0 routing-caused items
      against footprint graphics (38 before);
    - sonde-xilinx on the plugin path: 0 (1 before);
    - KiCad unconnected reported for both.
  - *Partial 2026-10-20:* shapes of no net and copper text are keepouts; a shape of a net the
    DSN lists is left out (decided 2026-10-20, option (c)). What landed:
    - Fixture `crates/freerouting/tests/data/kicad/q22-copper-graphics.kicad_pcb`: J1's two pads
      "2" (net N2) at (12, 10) and (18, 10) mm, a 0.5 mm B.Cu `fp_line` of no net between them, a
      1.5 mm `gr_text "GND"` on F.Cu at (15, 10), and a filled F.Cu `gr_poly` of net FG under P1;
      `kicad-plugin/tests/data/q22-copper-graphics.dsn` is KiCad 10.0.7's export of it.
    - Plugin (`copper_graphics.py`, `kicad_board.copper_graphics`, `export_flow.fix_copper_graphics`
      after `fix_edge_graphics`): `kicad-plugin/tests/test_copper_graphics.py`, 11 tests in the
      venv and the pcbnew-gated one (KiCad 10.0.7's glyph strokes, 12.842..17.158 x
      9.021..10.836 mm), run with the system `python3`. The line's keepout is (12950,
      -10250)..(17050, -9750) µm on B.Cu; the text's keepouts are the glyphs' outer outlines on
      F.Cu. The FG polygon is not written, and the log says
      `copper graphics: 1 shape(s) of a net left out (Q22.4 remainder)`
      (`test_a_shape_of_a_net_is_left_out_and_logged`,
      `test_a_net_line_is_left_out_beside_the_keepouts`).
    - Reader (`crates/freerouting-kicad/src/copper_graphics.rs`, called from `read.rs`):
      `tests/copper_graphics.rs`, 4 tests: `a_footprint_copper_line_becomes_a_keepout_on_its_layer`
      (the plugin's corners, to 1e-6 µm), `a_copper_text_becomes_a_keepout_box_that_holds_its_glyphs`
      (12037.5..17962.5 x 8500..11500 µm, which holds the plugin's glyphs),
      `a_shape_of_a_net_is_left_out_and_logged` (no keepout, no fixed wire, the same warning as
      the plugin's log line) and `net_tie_shapes_and_shapes_of_a_net_without_pins_are_keepouts`.
      "The same in both inputs": the line's keepout is identical. The text differs by design: the
      reader has no font engine, so it keeps out a box that holds the glyphs (module docs and
      DIVERGENCES).
    - Engine (`crates/freerouting/tests/copper_graphics.rs`):
      `without_the_obstacles_the_route_crosses_the_copper_graphics` (the control crosses one of
      them) and `the_route_goes_round_the_copper_line_and_the_copper_text` (legacy and enhanced:
      incomplete 0, gap to the line and to the text box >= 1999 board units, i.e. the 0.2 mm
      clearance less 1 unit of rounding).
    - Layer quoting (a bug on main too, fixed here; decided 2026-10-20 to land it with Q22.4).
      vfo's DSN names its copper layers `"Top Layer, Top"` and `"Bottom Layer, Bottom"`, and
      every plugin fix-up wrote the layer unquoted, so the router dropped those shapes ("wire
      has no shape"), among them main's Q18.5/Q18.7 edge and mask keepouts (54 lines).
      `Dsn.layer_token` is now used by `dsn_fixups` (edge, pad and pour keepouts and wires),
      `edge_graphics` and `copper_graphics`; `kicad-plugin/tests/test_layer_tokens.py` has 5
      tests. Of the measured rows, only vfo has such layer names.
    - Re-measure (two arms, `grade.sh --export plugin {"schema": 4}`, `enhanced`, each row's
      Board Setup flags from the plugin, release CLIs of main `a976e02` and of this change,
      `ulimit -v 8000000`, `timeout 120`; `target/orchestrator/q22k/out/{before,q22k}`):
      - pcietest (N04.3 `center` grade, `--keep-tracks`): routing-caused `clearance` 130 → **0**,
        `shorting_items` 122 → 0, `hole_clearance`/`edge`/`solder_mask_bridge`/below-minimum 0 →
        0; KiCad unconnected 4 → 4 (input 256). The plugin's DSN carried 259 `copper_graphic`
        keepouts. Both arms stop at the 120 s limit (the CLI saves on SIGTERM).
      - sonde-xilinx (plugin path, routed whole): `clearance` 2 → **0** (the 2 were one track
        against "PCB text 'GND'"); KiCad unconnected 2 → 2 (input 48); 61 `copper_text` keepouts
        from 8 texts.
      - mydevboard: 0 → 0 in every class; unconnected 57 → 57.
      - vfo, with this landing's code (net shapes left out, layers quoted; release CLI of the
        branch on main `ba38e3f`, `timeout 300`, `target/orchestrator/q22k/out/q22k-optc/vfo`):
        route 1.19 s → **17.02 s**, engine unrouted 0 → **5** (all of net FG), input check clean,
        output check 0 breaches; KiCad unconnected 11 → **13** (input 87); routing-caused copper
        errors 0 of every type (`clearance`, `hole_clearance`, `copper_edge_clearance`,
        `shorting_items`, `solder_mask_bridge`, below-minimum); `drc_real` is `starved_thermal`
        2 → 5. The DSN has 0 `copper_graphic` keepouts: the plugin left out vfo's 3 FG polygons
        ("3 shape(s) of a net left out"). The 0 → 5 unrouted come from the Q18 edge and mask
        keepouts that now load (the same DSN without the FG wires gave 16.95 s and 5 unrouted
        before this change); their KiCad cost is graded again in Q22.8.
    - The "38 before" in the third clause was Q22.1's count on the bare export; this re-measure
      grades the plugin export, where the before arm has 130.
  - *Partial 2026-10-20 (why net shapes are left out):* the branch first wrote a shape of a
    listed net as fixed wires of that net (its stroke pieces, and a filled body in 50 µm strips
    covered by 0.1 mm bands plus a 0.1 mm boundary wire). With the layers quoted, vfo's 3 FG
    polygons became 665 fixed wires; the route was cut at 120 s with 7 unrouted and 940 router
    violations and gave no SES, and the input check reported 1604 input violations
    (`track_width` 665, bands under the 0.2 mm minimum; `copper_edge_clearance` 173; `keepout`
    766, FG copper in the mask apertures). That code is not landed and not kept (decided
    2026-10-20: option (a) does not reuse the strip covering).
    - **Remains (option (a)):** one conduction polygon of the net for each such shape, plus a
      keepout of its outline that holds only for other nets, in the plugin and the reader; then
      vfo, miniscope (6 net shapes) and openair (2) graded again. Until then a track of another
      net can cross a net shape of a listed net (KiCad reports it as `clearance` or
      `shorting_items`).
  - *Done 2026-10-20:* option (a). A shape of a net the DSN lists becomes one conduction-area
    wire of that net per polygon of the keepout outline code (filled body; one rectangle per
    stroke piece; a circle as a polygon; KiCad's shapes have no holes):
    `(wire (polygon <layer> 0 ...) (net <net>) (type fix) (attr obstacle))` at the end of the
    wiring, in the plugin (`copper_graphics.py`) and the reader (`copper_graphics::net_wires`,
    `read.rs`). No separate keepout is needed: the engine reads `(attr obstacle)`
    (`ast::WIRE_ATTR_OBSTACLE`, new `Wire::attr`) as a conduction area that is an obstacle to
    other nets (`BasicBoard::insert_fixed_copper_area`); `change_plane_as_obstacle` never clears
    it (without that, `router.plane_as_obstacle = false`, both profiles' value, would let other
    nets cross); two such areas of a net that meet are in contact, so the 34 chords of an arc
    are one copper (without it miniscope's engine ledger had 162 unrouted against main's 55).
    - Fixture extended: P2 (FG) beside P1 under the FG polygon, J2's two F.Cu pads of N3 at
      (24, 11.5) and (24, 16.5), a 1 mm F.Cu `gr_line` of FG from (20, 14) to (28, 14) across
      N3's straight path; `kicad-plugin/tests/data/q22-copper-graphics.dsn` re-exported with
      KiCad 10.0.7's `pcbnew.ExportSpecctraDSN`.
    - Plugin (`test_copper_graphics.py`, 16 passed with KiCad 10.0.7's pcbnew, venv 15 passed
      1 skipped): `test_a_filled_shape_of_a_net_becomes_a_conduction_area_of_its_net`,
      `test_a_stroked_shape_of_a_net_becomes_the_area_of_its_stroke` (19.5..28.5 x
      13.5..14.5 mm), `test_a_circle_of_a_net_becomes_a_polygon_area`,
      `test_a_net_line_becomes_an_area_beside_the_keepouts`,
      `test_net_and_layer_names_keep_the_dsn_s_quoting`,
      `test_a_wiring_on_one_line_or_none_takes_the_areas`, and
      `test_the_fixture_through_pcbnew_gives_the_reader_s_obstacles` (pcbnew gives both FG
      shapes; the areas equal the reader's).
    - Reader (`crates/freerouting-kicad/tests/copper_graphics.rs`, 5 passed):
      `shapes_of_a_net_become_conduction_areas_of_their_net` (the plugin's two polygons, to
      1e-6 µm, net FG, `fix`, last in the wiring, no warning) and
      `the_written_dsn_keeps_the_areas` (DSN text round trip).
    - Engine (`crates/freerouting/tests/copper_graphics.rs`, 8 passed):
      `other_nets_keep_clear_of_a_net_s_copper_and_the_net_connects_through_it` (legacy and
      enhanced: P1 and P2 each touch only the FG polygon, nothing without the areas; incomplete
      0; N3's F.Cu gap to the FG line >= 1999 units; the control
      `without_the_obstacles_the_route_crosses_the_copper_graphics` crosses it),
      `the_session_and_the_kicad_output_leave_the_net_shapes_out` (no `polygon` in the SES;
      `final_copper` has the traces and vias only; no `gr_`/`zone`/`fp_poly`/`fp_line` added to
      the `.kicad_pcb`), `the_output_check_counts_a_crossing_as_routed_and_keeps_the_area`
      (an N3 trace over the FG line is a routed `Clearance` breach whose removable items are the
      trace only; the areas stay), `plane_as_obstacle_never_clears_a_net_shape`,
      `the_dsn_text_gives_the_same_areas`, `the_pieces_of_a_stroked_shape_are_one_copper` (a
      0.8 mm `gr_arc` between two pads: incomplete 0; 33 without the contact rule).
    - Legacy: `tools/parity-check.sh --no-probes` PARITY OK, connection parity 7 of 7 exact
      (38 s). A DSN without `(attr obstacle)` reads as before.
    - Graded once per arm with KiCad 10.0.7 (`grade.sh --export plugin {"schema": 4}`,
      enhanced, `ulimit -v 8000000`, `timeout 300`; release CLIs of main `a36a5c0` and of this
      change on it; `target/orchestrator/q224a/out/{mine,main}-{final,t270}`). Routing-caused
      copper errors (`clearance`, `shorting_items`, `hole_clearance`, `copper_edge_clearance`,
      `solder_mask_bridge`, below-minimum) are 0 in every row and both arms, against the net
      shapes too:
      | board | areas | route this / main | engine unrouted this / main | KiCad unconnected this / main (input) |
      |---|---|---|---|---|
      | vfo | 3 | 1.12 s / 17.76 s | 4 / 5 | 14 / 13 (87) |
      | miniscope (`--timeout 270`) | 111 | 270 s / 270 s | 62 / 57 | 67 / 61 (294) |
      | openair (`--timeout 270`) | 10 | 270 s / 270 s | 57 / 62 | 58 / 63 (316) |

      vfo's `drc_real` is `starved_thermal` 6 / 5 (GND zone thermals), its open FG connections
      are the Q18 edge and mask keepouts' (see above). miniscope and openair need more than
      300 s in both arms, so they were cut by the router's own `--timeout 270` to get a session;
      their per-pass times are the same in both arms (miniscope 10.0..25.4 s against
      9.9..28.4 s). openair's `isolated_copper` 2 / 2 and silk items are not copper rules. On
      main `039a859`, before Q22.11 and Q22.12, the uncapped runs gave openair 31 / 59 KiCad
      unconnected (120 s against the 300 s cap) and miniscope 59 / 59.

- [x] **Q22.5 — `.kicad_dru` clearances between item types.**
  - `dru.rs` learns the conditions `A.Type` (`'track'`, `'via'`, `'pad'`), `A.isPlated()` and
    `A.Pad_Type`. It maps each onto the DSN's typed clearances: `via_wire`, `pin_wire`,
    `smd_wire`, `via_via` and so on.
  - For a DSN input, the router reads the board's `.kicad_dru` given on the command line, and the
    plugin passes it.
  - A rule the router can't express:
    - if it asks for more than the DSN gives, it is applied to every pair its item types allow,
      with a warning naming it;
    - if it asks for the same or less, it is reported and left out. KiCad lets a matching
      custom rule lower a class clearance, and the router keeping the larger value is safe.
  - **Accept:**
    - `crates/freerouting-kicad/tests/dru_item_types.rs`, one test per condition, including a
      net class with its own clearance (`wire` is clearance class 1 in the DSN reader);
    - ch32v003 routed with its `center` flags: 0 routing-caused items under `'Pad to Track
      clearance)'` (2 before);
    - a DIVERGENCES line (Java's plugin never applies the `.kicad_dru`), module docs and a
      pinning test;
    - `cargo fmt`, `clippy` and the touched crates' tests pass.
  - *Done 2026-10-20:* on main `5c79e31` (Q22.3, Q22.4 and Q22.10 in).
    - **Conditions.** `dru.rs` parses `X.Type == 't'`, `B.Type == A.Type`, `X.isPlated()`,
      `X.Pad_Type == 'v'` and `A.Net ==`/`!= B.Net`; `dru/item_types.rs` (new) evaluates a rule
      for every pair of item types (track, via, through-hole, SMD, edge-connector and NPTH pad)
      and net classes and writes the DSN's typed clearances: the default class's in the
      structure (so its routed vias, input vias and pins take them), every other class's after
      its own clearance, and every class pair as a typed `class_class` block. A pad-pad rule, a
      zone rule and a same-net rule are left out; several rules on a kind pair keep the largest.
    - **Hole clearance** (decided here, module docs of `item_types`): a `hole_clearance` rule that
      matches a hole and copper becomes a floor of the board-wide `router.hole_clearance_um`,
      applied in `prepare_job` after every merge (over a smaller command-line value, never
      lowering a larger one). The typed matrix is copper to copper, and a per-padstack
      conversion would need a clearance class per padstack; the global is conservative.
    - **Inputs.** New CLI flag `--kicad-dru FILE` for a DSN input (`args.rs`, `help.txt`,
      `cli.rs`; `JobInput::kicad_dru`); `--apply-design-rules` stays the `.kicad_pcb` switch and
      now applies the same item-type rules. The plugin passes `--kicad-dru <board>.kicad_dru` for
      every DSN route when the file exists (`export_flow.kicad_dru_args`, every profile; not for
      the GUI or the snapshot input).
    - **Tests.** `crates/freerouting-kicad/tests/dru_item_types.rs` (13) on the new fixture
      `crates/freerouting/tests/data/kicad/q225-item-types.dsn` (`kicad_default` 150 µm and
      `Power` 250 µm with its own clearance class), measuring the clearance classes the built
      board's wires, routed vias, input vias, pins and SMD pads really have:
      `type_via_and_track_sets_via_wire`, `type_pad_sets_pin_and_smd_wire`,
      `plated_sets_via_and_pin_wire_in_every_class` (Power pins and vias against default
      wires and back), `plated_is_exact_with_one_class` (nothing else changes),
      `pad_type_sets_smd_wire_with_a_warning`, `same_type_sets_wire_wire`,
      `net_class_term_is_applied_to_every_class_with_a_warning`,
      `rules_that_change_nothing_are_left_out`, `hole_clearance_is_a_floor`,
      `the_job_applies_the_rules_and_the_hole_floor` (250 → 330 µm, 400 kept, unchanged without
      a file), `ch32v003_rules_report`, `item_terms_parse`,
      `the_fixture_has_two_clearance_classes`. Plugin: `kicad-plugin/tests/test_kicad_dru_args.py`
      (2). CLI: `args_java.rs::kicad_dru_takes_a_file`.
    - **ch32v003** (`center` flags of N04.3, `tools/kicad-oracle/grade.sh --keep-tracks --export
      plugin` with `{"schema": 5}`, one KiCad 10.0.7 grade per arm; runs in
      `target/orchestrator/q225/runs/`):

      | Arm | routing-caused by type | KiCad unconnected | route |
      |---|---|---|---|
      | before (main's plugin, no `--kicad-dru`) | `clearance` 2: `'Pad to Track clearance)'`, the `J1-SHIELD` B.Cu track against the GND vias at 0.1881 and 0.1981 mm | 0 (input 3) | 3.29 s |
      | after (`--kicad-dru` passed) | none: `clearance` 0, `hole_clearance` 0, `below_minimum` 0 | 0 (input 3) | 13.22 s |

      The router log applies `'Pad to Track clearance)'` (2 targets: via-wire, pin-wire) and
      raises `router.hole_clearance_um` from 250 to 330 (`'via to track clearance'`, `'NPTH to
      Track clearance)'`, `'PTH to Track clearance)'`); `'Minimum Trace Spacing (outer layer)'`
      is left out (no more than the DSN), the inner-layer rules are left out (no inner layer),
      the pad-pad rule is left out. Pre-existing items 30 in both. The engine's own count of
      unrouted connections went 31 → 43, which KiCad's grade does not see (0 unconnected in
      both, the board's zones connect them).
    - **DIVERGENCES**: freerouting-kicad ("`.kicad_dru` clearances between item types reach
      the router"), freerouting-cli (`--kicad-dru`), kicad-plugin ("The project's `.kicad_dru`
      is passed for a DSN route").
    - **Gate**: `cargo fmt --all --check` clean; `cargo clippy --workspace --all-targets -- -D
      warnings` clean; `cargo test -p freerouting-kicad -p freerouting -p freerouting-cli`
      passes (CLI rerun after updating `EXTRA_FLAGS.len()` to 13); `pytest kicad-plugin/tests`
      456 passed, 20 skipped; `tools/parity-check.sh --no-probes` 7 of 7 exact.

- [x] **Q22.6 — The grader can route the plugin's export.**
  - `grade.sh --export plugin OPTIONS.json` exports with `headless_run.py export` (the plugin's
    fix-ups, and its router flags ahead of the engine command's), and imports with
    `headless_run.py import`.
  - Without the flag, nothing changes.
  - **Accept:**
    - `tools/kicad-oracle/tests/run-plugin-export-test.sh`: the graded DSN is byte-identical to
      `headless_run.py export`'s;
    - canary-pwr (N04.3's `ab786fcc`) routed with the `inside_pad` flags on the plugin export: 0
      routing-caused `clearance` (1 before, FID3);
    - the README documents the flag.
  - *Done 2026-10-20:*
    - `grade.sh --export plugin OPTIONS.json`: the DSN comes from `headless_run.py export` on the
      unrouted board (`OUT/work/BASE.dsn` and `BASE.args.json`), the plugin's router flags go into
      the engine command at `{PLUGIN_ARGS}` or right after its `{SES}` argument (the command's own
      flags come later and win; a command with neither is not graded), and the SES is imported by
      `headless_run.py import`. `meta.json` records `export` and `plugin_options`. Without the flag
      the script is unchanged apart from Q22.2's `summarize` arguments. `headless_run.py` needed no
      change.
    - `tools/kicad-oracle/tests/run-plugin-export-test.sh WORK_DIR` (legacy board of
      `make_legacy_board.py`, a recording engine): 7 `ok`, no `FAIL`. The graded DSN is
      byte-identical to the one `headless_run.py export` writes to the same path (the DSN's first
      line names its own path, so the test exports again to it and compares); it carries the 8
      edge-strip keepouts that the bare export lacks; the engine got `--max-passes 3 --seed 7` and
      the board minimums after `{SES}` and before its own `--engine-flag`; `{PLUGIN_ARGS}` in a
      `bash -c` string; no place for the flags gives exit 2 with `graded: false`; without the flag the
      engine got only `DSN SES --engine-flag` and `export` is `bare`.
    - canary-pwr (`n04q16/canary-ab786fcc`, `--keep-tracks`, `{"schema": 4}` options, release CLI
      built from main `91b540d`, `--router.pad_contacts=inside_pad`, `ulimit -v 8000000`):
      routing-caused `clearance` **0** through the plugin export (`drc_real_by_type`
      `starved_thermal` 1, `track_dangling` 9; KiCad unconnected 58; 227 s; the plugin's DSN had
      128 edge strips and 6 pad circles). The control, same binary and flags on the bare export:
      `clearance` 1, FID3 (`pad clearance 0.6000 mm; actual 0.5235 mm`), `track_dangling` 15,
      unconnected 74, 212 s. The plugin arm routes 16 fewer unconnected items; the arms also differ
      in `--router.autorouter.sweep=free`, `--router.min_trace_width_um=150` and the plugin's 128 edge strips and 6 pad circles, so the 16 is not a measurement of the flag.
    - README: "`--export plugin OPTIONS.json` (Q22.6)", the summary fields and the files table.

- [ ] **Q22.7 — Register the re-measurement.** Write `docs/roadmap/prereg/Q22.json` and
  `tools/bench/sets/q22-clearance.txt` before any number.
  - **Boards:**
    - N04.3's rows with a breach: pcietest, cm4-edge-ai, nudac, ch32v003, mydevboard, vna-usb-pd
      and picodebugger;
    - canary-pwr at `ab786fcc`;
    - the demos with copper text: sonde-xilinx and pic_programmer;
    - two clean rows: flight-computer and laser-backplane.

    That is a few minutes in all.
  - **Arms:** `before` (main) and `q22` (Q22.2–Q22.6), with each row's Board Setup minimums and
    `--export plugin` in both.
  - **Grading:** one KiCad grade per distinct session.
  - **Guards:** routing-caused `clearance`, `hole_clearance` and `below_minimum` (including
    `annular_width`) are 0 in `q22` on every row.
  - **Report:** KiCad unconnected and route time.
  - **Limit** (added 2026-10-20, from Q22.4/Q22.10's re-measure): pcietest and pocketsdr-v3-0 get
    at least 300 s, or a work budget. At 120 s both are cut by wall clock (the CLI saves on
    SIGTERM). pocketsdr-v3-0 needs 217.8 s to complete with the board minimum clearance.
  - **Accept:** the registration and its pin test (`prereg_q22.rs`) are in, with no number yet.

- [ ] **Q22.8 — Re-measure.** Run Q22.7 as registered.
  - **Accept:**
    - the per-row table is in the done note;
    - every guard is 0 in `q22`;
    - the change in KiCad unconnected is given per row, with the rows that Q22.4's obstacles cost
      named.

- [x] **Q22.9 — Canary-pwr: which change costs 18 → 26.** Two runs, one at a time, with main's
  release CLI. Each route is about 2 min, under `ulimit -v 8000000` and `timeout 600`.
  - **Arm (a), floors off.** Route Q16.5's base DSN
    (`target/orchestrator/n04q16/out/q165/runs/q165-base/canary-pwr/work/canary-pwr.dsn`) with
    `--router.min_trace_width_um=150 --router.autorouter.sweep=free`, without the five Q18 flags.
    Its difference from the 26 is the floors' cost.
  - **Arm (b), the preset.** Route the `d6199b48` board
    (`target/orchestrator/q165/scratch/canary-d6199b48/`) through `headless_run.py export` with
    a copy of the preset `route-options.json`. Its difference from the 26 is the options' cost.

  Grade both as `target/orchestrator/n04q16/scratch/q165r-arm.sh`'s grade stage does, on a fresh
  copy of the `d6199b48` project. If (b) gives about 18, the 56 commits don't cost it either.
  - **Accept:**
    - both rows' KiCad unconnected and copper errors are in the done note;
    - the 18 → 26 change is attributed to floors, options or code, each with its number.
  - *Done 2026-10-20:* four routes and four KiCad grades, one at a time.
    - **Setup.**
      - Binary: main `5a701e2` release CLI (`freerouting-rs 0.1.0 (git 5a701e274f34)`), sha256
        `94da5875191a8bfec24b4a951d58187cdf1fd5dfc1a72a78d5b2e7b5b2184c52`. The plugin and
        oracle scripts come from the same checkout.
      - Script: `target/orchestrator/q229/scratch/q229-arm.sh` (sha256 prefix `07975a8f`). It
        is `q165r-arm.sh` for canary-pwr only, with the binary renamed `freerouting-rs`. It
        adds a `DSN=` mode, which routes a saved DSN with only the given flags, and a
        `NOFLOORS=1` mode, which drops the Q18 flags from the export's args.
      - Every row starts from a fresh copy of `q165/scratch/canary-d6199b48/` (sha256
        `25395aeb…`). Routes ran under `ulimit -v 8000000` and `timeout 600`. None timed out
        and every rc was 0.
      - The grade is `q165r-arm.sh`'s grade stage: `headless_run.py import` with the row's
        options, `oracle_lib.py force-errors`, then one `kicad-cli pcb drc
        --schematic-parity --refill-zones --severity-all` (KiCad 10.0.7). Outputs are in
        `target/orchestrator/q229/runs/<row>/`.
    - **The reference row.** The 26 is Q16.5's `q165-base` canary-pwr row (b98074e, options
      `{"schema": 4, "profile": "enhanced"}`).
      - Its DSN (`…/q165-base/canary-pwr/work/canary-pwr.dsn`, sha256 `30d1adde…`) and its
        flags are on record. Its binary (`n04q16/target/release/freerouting`) is gone.
      - So row (c) reruns that DSN with those 8 flags on main's binary.
      - The "five Q18 flags" are six: the four `--router.board_minimums.*`,
        `--router.copper_to_edge_clearance_um=300` and `--router.hole_clearance_um=250`.
        Arm (a) drops all six.
    - **The preset.** `q229/scratch/preset-route-options.json` is a copy of canary's current
      `freerouting-rs/route-options.json` (sha256 `1ffe579f…`).
      - The copy at `d6199b48` (`4f828997…`) differs only by an explicit `"sweep": "free"`, and
        the export writes `--router.autorouter.sweep=free` either way.
      - The router args match the 18-era record (canary's `canary-pwr.args.json`): `--max-passes
        1 --router.min_trace_width_um=150 --router.scoring.start_ripup_costs=1500
        --router.autorouter.sweep=free --router.autorouter.sweep_passes=2`.
      - Main's export adds the six Q18 flags and Q18.7's
        `--router.solder_mask_expansions=FID3:@1=500,0;FID2:@1=500,0;FID1:@1=500,0;FID4:@1=0,500;FID5:@1=0,500;FID6:@1=0,500`.
      - Row (d) drops all seven. That makes it the 18's own setup on main's code.
    - **Rows.** Copper errors are 0 of every type in every row, so 0 are routing-caused:
      `clearance`, `hole_clearance`, `copper_edge_clearance`, `track_width`, `annular_width`,
      `via_diameter`, `drill_out_of_range`, `shorting_items` and `solder_mask_bridge` are all
      absent from `drc.json`. The other items are `starved_thermal` (error; the input has 0) and
      `lib_footprint_mismatch` 6 (warning; the input has 6). Every row has `via_in_pad` 0 and
      inner tracks 0.

      | Row | DSN (sha256) | Router flags | Route s | KiCad unconnected (zone-involved) | starved_thermal | via_dangling | SES sha256 |
      |---|---|---|---|---|---|---|---|
      | 26 (record, b98074e) | q165-base `30d1adde…` | min_trace 150, sweep free, the six Q18 flags | 107.8 | 26 (11) | 1 | 8 | `669662cf…` |
      | (c) the 26 on main | q165-base `30d1adde…` | the same 8 flags | 121.2 | **26** (11) | 1 | 0 | `6f6afb3a…` |
      | (a) floors off | q165-base `30d1adde…` | `--router.min_trace_width_um=150 --router.autorouter.sweep=free` | 95.2 | **23** (5) | 4 | 0 | `da680def…` |
      | (b) the preset | main's export with the preset, `8feb7b97…` | the preset's 5 flags + the six Q18 flags + solder_mask_expansions | 54.1 | **16** (3) | 3 | 0 | `fadfd110…` |
      | (d) the preset, floors off | main's export with the preset, `71ec9316…` (differs from (b) only in the DSN's path line) | the preset's 5 flags only | 33.5 | **19** (4) | 7 | 0 | `9eb64a1e…` |

      Commands: `DSN=scratch/q165-base.dsn q229-arm.sh route c-26-main
      scratch/default-options.json -- <8 flags>`; the same for `a-floors-off` with 2 flags;
      `q229-arm.sh route b-preset scratch/preset-route-options.json`; `NOFLOORS=1 q229-arm.sh
      route d-preset-floors-off scratch/preset-route-options.json`. Each was then graded with
      `q229-arm.sh grade <row> x`.
      - Router summaries: (c) 34 unrouted, 117 violations, score 853.62, the same as the
        record. (a) 36 / 97 / 845.93. (b) 76 / 138 / 664.55. (d) 81 / 118 / 643.55.
      - Row (c) has the record's 26 unconnected items, net by net. Its 0 dangling vias come
        from `cleanup.remove_stubs` (N05.3).
    - **Attribution of 18 → 26 (+8).** 18 → (d) 19 → (b) 16 → (c) 26.
      - **Options: +10.** (b) 16 → (c) 26 is the same code, the same floors and the same board,
        with the preset swapped for `{"profile": "enhanced"}`. 8 of the 10 are pour islands
        (zone-involved: 3 → 11). The preset fixes the Power_High and Switch_Node pours and drops
        GND's outer pours.
      - **Floors: −3 on the preset, +3 on the defaults.** With the preset, (d) 19 → (b) 16. With
        the defaults, (a) 23 → (c) 26. On the 26's path, floors cost 3, not 8.
      - **Code: +1.** The 18 (b93bcd0, plugin path, canary's `grade.sh`, KiCad 10.0.5) → (d)
        19 (5a701e2, this grade, 10.0.7). The 18 has no saved session, so this +1 also holds
        any difference in export and grading. From b98074e to main the code adds 0: the
        record's 26 → (c) 26.
      - Verdict: the options. The preset on main with the floors gives 16, under the 18.

- [x] **Q22.10 — The board's minimum clearance is a floor** (added 2026-10-20). pocketsdr-v3-0's
  Q18 grades (`target/orchestrator/q18b/out/grade/mp1/pocketsdr-v3-0`) have 446 routing-caused
  `clearance` items, every one "board minimum clearance 0.1270 mm" (e.g. `Track [+3.3V]` on F.Cu
  at (155.1411, 97.54) against `Via [/PLL_CLK_B]` at (154.522, 97.9331), actual 0.0919 mm). Its
  `.kicad_pro` has `min_clearance` 0.127 mm and one net class, Default, at 0.09 mm; KiCad's DSN
  export carries only the class value, and KiCad's DRC takes the larger.
  - `router.board_minimums.clearance_um` raises every clearance-matrix entry below it to it
    (the copper-to-edge and hole classes keep their own rules). The plugin passes
    `m_MinClearance`; a `.kicad_pcb` job reads `min_clearance` from the `.kicad_pro` (KiCad's
    default is 0, no floor). `legacy` with nothing supplied stays Java.
  - The other Board Setup constraints Q18 left out are listed with their KiCad key and whether
    they apply to routed copper; the ones that do are floored or named as follow-ups.
  - **Accept:**
    - pinning tests for the engine, the reader and the plugin, written first; DIVERGENCES lines
      and module docs;
    - pocketsdr-v3-0 routed through the plugin's export with its Board Setup minimums: 0
      routing-caused `clearance` (446 before), KiCad unconnected beside it;
    - the list of the other constraints.
  - *Done 2026-10-20:*
    - Evidence checked first: the before arm's pocketsdr grade has 499 routing-caused `clearance`
      items, each "board minimum clearance 0.1270 mm"; its `.kicad_pro` has `min_clearance` 0.127
      and Default 0.09. Corpus scan: `min_clearance` above every class clearance only on
      pocketsdr-v3-0 (0.127 vs 0.09) and overlord (0.175 vs 0.1).
    - Engine: `crates/freerouting/src/job/clearance_floor.rs`, called first in
      `overrides::apply_all`. It raises every entry between non-null classes on every layer
      (except the board-edge and hole classes) to the floor, then reinserts the search-tree items,
      and logs "Board minimum clearance 350.0 um (... board units): N class clearance(s) raised to
      it." `crates/freerouting/tests/clearance_floor.rs`, 4 tests:
      `without_a_minimum_clearance_the_matrix_is_the_dsns`,
      `the_board_minimum_clearance_raises_every_clearance_below_it` (350 µm floor; the edge class
      stays 2500), `routed_copper_keeps_the_board_minimum_clearance` (legacy without a floor
      routes at 0.2 mm; legacy and enhanced with a 400 µm floor keep >= 3999 units), and
      `a_kicad_pcb_input_passes_its_minimum_clearance` (the reader: missing key and 0 give None,
      0.127 gives 127000 nm and `clearance_um` 127.0).
    - Plugin: `options.BOARD_MINIMUM_SETTINGS` maps `m_MinClearance` to
      `router.board_minimums.clearance_um`; `kicad-plugin/tests/test_min_clearance.py` 2 tests. With
      KiCad's default of 0 nothing is passed, so `legacy` with nothing supplied stays Java
      (`settings_defaults` pins `board_minimums.clearance_um` None).
    - DIVERGENCES: an engine line (`board_minimums.clearance_um`), a freerouting-kicad line
      (`min_clearance`) and a kicad-plugin line (`m_MinClearance`).
    - pocketsdr-v3-0 through the plugin export with its Board Setup minimums (same arms as
      Q22.4): routing-caused `clearance` 499 → **0**, `hole_to_hole` 2 → 0, hole/edge/smb/below-minimum
      0 → 0. KiCad unconnected 40 → 48 at the 120 s limit (114 s complete before, cut at 120 s
      after); with 300 s (`out/q22k-300s`) the run completes in 217.8 s with unconnected **39**,
      `clearance` 0, engine "2 unrouted and 0 violations".
    - The other Board Setup constraints (pcbnew defaults read from `BOARD_DESIGN_SETTINGS`):

      | Constraint | KiCad key (`.kicad_pro` rules / pcbnew) | Applies to routed copper | State |
      |---|---|---|---|
      | Hole to hole | `min_hole_to_hole` / `m_HoleToHoleMin` (default 0.25 mm) | yes, via drill to via or pad drill | follow-up: different-net vias are kept apart by clearance plus annular rings (pocketsdr 2 → 0); same-net vias are not floored |
      | Copper to mask aperture | `solder_mask_to_copper_clearance` / `m_SolderMaskToCopperClearance` (default 0) | yes, routed copper against another net's mask opening | follow-up: not added to Q18.7's mask keepouts; nonzero on mydevboard 0.005, fc-rom 0.05, solarmeshtastic 0.005 mm |
      | Thermal spokes | `min_resolved_spokes` / `m_MinResolvedSpokes` (default 2) | indirectly: zone fill against routed copper (`starved_thermal`) | not floored |
      | Microvia size and drill | `min_microvia_diameter`, `min_microvia_drill` / `m_MicroViasMinSize`, `m_MicroViasMinDrill` | no: the router places through vias only | n/a |
      | Minimum connection width | `min_connection` / `m_MinConn` (default 0) | no: zone fill only | n/a |
      | Silk and text | `min_silk_clearance`, `min_text_height`, `min_text_thickness` | no | n/a |

- [x] **Q22.11 — Vias of the same net keep the board's hole-to-hole distance** (added 2026-10-20;
  decided 2026-10-20: routing never breaks the board's manufacturing rules, graded under the
  board's own rules). KiCad's `min_hole_to_hole` (`m_HoleToHoleMin`, default 0.25 mm) applies
  between any two drills, of any net. Q22.10 keeps vias of different nets apart (clearance plus
  annular rings; pocketsdr-v3-0 `hole_to_hole` 2 → 0). Two vias of one net, and a via next to a
  pad drill of its own net, have no floor.
  - `router.board_minimums.hole_to_hole_um`: the plugin passes `m_HoleToHoleMin`; a `.kicad_pcb`
    job reads `min_hole_to_hole` (KiCad's default when the key is missing). The router places no
    via whose drill comes closer than that to another drill. `legacy` with nothing supplied stays
    Java.
  - **Accept:**
    - pinning tests written first: an engine test where two vias of one net would sit closer
      than the minimum; a reader test; a plugin test. DIVERGENCES lines;
    - routing-caused `hole_to_hole` 0 on the Q22.7 rows, with KiCad unconnected beside it.
  - *Done 2026-10-20:*
    - Engine: `BoardRules::hole_to_hole` (board units), set by `job/hole_to_hole.rs`
      (`apply_hole_to_hole`, called in `overrides::apply_all` after the clearance floor; logs
      "Board minimum hole to hole 250.0 um (2500 board units): ..."). The rule is
      `board/hole_spacing.rs` (`BasicBoard::hole_to_hole_breaches`, `hole_to_hole_breach`,
      `hole_radius`): gap = centre distance less both drill radii, any net, vias and multi-layer
      pins, no tolerance; the query is `ShapeSearchTree::items_near_unmetered`, so the work
      meter counts the same. Asked by the maze search (`session/layers.rs`,
      `drill_breaks_hole_to_hole` in `expand_to_other_layers`, with the largest drill of the
      rule's vias), the via insertion (`TraceShover::check_via`) and every via move
      (`TraceShover::check_drill_item_move`: the via optimizer and shoved vias). Q20's output
      check has the rule `hole_to_hole` (`BoardRule::HoleToHole`, `drc/board_rules.rs`), so
      `repair` removes a routed via that breaks it. The KRT stages that insert vias directly
      (QFN/BGA fanout, diff-pair legs) do not ask; the output check covers them.
    - Tests: `crates/freerouting/tests/hole_to_hole.rs`, fixture
      `tests/data/quality/q22-same-net-vias.dsn` (net A needs one via each side of a B.Cu wall).
      The fixture was probed on the unchanged path first: the two vias of net A sit 1.0 mm apart
      hole to hole. 4 tests: `without_a_minimum_the_vias_are_javas` (legacy, unset: vias at
      (93500, 98287) and (106500, 98287), gap 10000 units, no log line);
      `vias_of_one_net_keep_the_board_hole_to_hole_distance` (legacy and enhanced, 1500 um:
      complete, 2 vias, gap >= 15000; measured 26631);
      `the_output_check_reports_holes_closer_than_the_minimum` (one `hole_to_hole`, both vias
      routed items, required 15000, actual 10000; none at 10000);
      `a_kicad_pcb_input_passes_its_hole_to_hole_minimum` (the reader: missing key 0.25 mm,
      0 is none, 0.3 gives 300000 nm and `hole_to_hole_um` 300.0). Plugin:
      `kicad-plugin/tests/test_hole_to_hole.py`, 2 tests (`m_HoleToHoleMin` 254000 passes
      `--router.board_minimums.hole_to_hole_um=254`; 0 passes nothing). `settings_defaults`
      pins the field unset; the GUI lists it in `CLI_ONLY`.
    - DIVERGENCES: an engine line (`board_minimums.hole_to_hole_um`), a freerouting-kicad line
      (`min_hole_to_hole`) and a kicad-plugin line (`m_HoleToHoleMin`).
    - Routing check on main `039a859` (with Q22.5) against this change, `grade.sh --export
      plugin` (`{"schema": 4}`, enhanced, each row's Board Setup minimums passed by the plugin,
      rows routed whole), one KiCad 10.0.7 grade each
      (`target/orchestrator/q2211/out/{before,q2211}/`):

      | Row | limit | routing-caused `hole_to_hole` before → after | KiCad unconnected before → after | route time |
      |---|---|---|---|---|
      | ch32v003 (`min_hole_to_hole` 0.25) | 300 s | 0 → 0 | 38 → 38 | 11.0 → 11.3 s |
      | pocketsdr-v3-0 (0.254) | 600 s | 0 → 0 | 39 → 32 | 194.7 → 288.9 s (24 → 29 passes) |

      Both arms have no routing-caused `clearance`, `hole_clearance` or `below_minimum`
      (ch32v003's only routing-caused type is `isolated_copper` 12 in both). On the pre-Q22.5
      main `6229b29` (`out-pre-rebase-6229b29/`) ch32v003 had 1 routing-caused `hole_to_hole` in
      both arms: a PD6 via against a VBUS via, 0.4147 mm, under the `.kicad_dru` rule "hole to
      hole clearance (different nets)" (0.5 mm), not Board Setup's 0.25 mm; the router logs
      that constraint as UNSUPPORTED (no DSN form), so a `.kicad_dru` `hole_to_hole` constraint
      is not floored yet (follow-up).
    - The full Q22.7 rows are graded by Q22.8's registered run, not here.

- [x] **Q22.12 — Copper keeps the board's solder-mask-to-copper clearance** (added 2026-10-20;
  decided 2026-10-20: routing never breaks the board's manufacturing rules). KiCad's
  `solder_mask_to_copper_clearance` (`m_SolderMaskToCopperClearance`, default 0) keeps copper of
  another net that far from a mask aperture. Q18.7's mask keepouts cover only the aperture.
  Nonzero on mydevboard (0.005 mm), fc-rom (0.05 mm) and solarmeshtastic (0.005 mm).
  - The plugin and the `.kicad_pcb` reader grow Q18.7's mask keepouts (pad apertures and mask
    graphics) by it.
  - **Accept:**
    - plugin and reader tests written first on a fixture with a nonzero value: the keepout is the
      aperture grown by the clearance; with 0, nothing changes;
    - fc-rom routed through the plugin's export: 0 routing-caused `solder_mask_bridge`, with KiCad
      unconnected beside it.
    - *Amended 2026-10-20* (decided 2026-10-20): fc-rom's clause moves to Q22.16. Its 7
      routing-caused `solder_mask_bridge` items come from a footprint-level mask aperture,
      J1's `fp_poly`, which this item's clearance doesn't reach: its pads open 0.05 mm, plus
      0.05 mm makes 0.1 mm, still under the 0.15 mm Default clearance. The plugin export is
      byte-identical with and without Q22.12 on fc-rom, mydevboard and solarmeshtastic. This
      item is met by the tests and by that measurement.

    *Done 2026-10-20* (first written as a Partial note, before the amendment above): built and
    tested; the fc-rom clause moved to Q22.16, as amended above. Carried by folding, no new setting: the reader (`Project::solder_mask_to_copper` from
    the `.kicad_pro`'s `board.design_settings.rules.solder_mask_to_copper_clearance`, 0 when
    missing, a negative value as 0) and the plugin (`kicad_board.mask_to_copper_nm`, from
    `GetDesignSettings().m_SolderMaskToCopperClearance`) add it to each pad side's expansion
    (`mask::expansion`, `kicad_board.mask_expansions`: margin + clearance, at least 0, before the
    smallest-class-clearance filter), so `router.solder_mask_expansions`, `job::solder_mask` and
    Q20's output check all use the grown aperture; and they take each board F.Mask / B.Mask shape
    with a stroke twice the clearance wider (`edge_keepouts::mask_polygons`,
    `edge_graphics.mask_polygons`). The engine is unchanged. Tests, written first (each failed on
    main 039a859): `crates/freerouting-kicad/tests/edge_keepouts.rs::the_mask_apertures_grow_by_the_projects_mask_to_copper_clearance`
    (0 and no key give Q18.7's keepouts; 0.05 mm: square 9.95..20.05 mm, line 4.75..15.25 mm,
    circle covered at 2.0499 mm), `crates/freerouting/tests/solder_mask.rs::the_projects_mask_to_copper_clearance_grows_the_pad_apertures`
    (TP1 500 000 nm with no project and with 0, 550 000 with 0.05 mm, P1/P2 250 000 and TP1
    700 000 with 0.2 mm; the job's setting `TP1:1=550,0` and the routed gap ≥ 0.5499 mm),
    plugin `test_solder_mask.py` (4 new) and `test_edge_graphics.py` (2 new; 0 gives the same DSN
    text). fc-rom (`tools/kicad-oracle/grade.sh --export plugin {"schema": 5}`, main's release
    binary in both arms, the engine being unchanged; `--timeout 270`, since a 300 s kill left no
    SES; `target/orchestrator/q2212/runs/out-{before,after}`): before (main) and after both
    routing-caused `solder_mask_bridge` 7, `via_dangling` 1, KiCad unconnected 79 (input 424),
    route 270.73 s / 272.64 s. The exported DSN differs only in its `(pcb "<path>")` line and the
    router flags are identical: fc-rom's pads open 0.05 mm, + 0.05 = 0.1 mm stays under the
    0.15 mm Default clearance, and it has no board-level mask shape. mydevboard and
    solarmeshtastic plugin exports are likewise identical on main and here. Remainder: the 7 are
    Track–Polygon bridges with J1's footprint `fp_poly` aperture on F.Mask and B.Mask
    (`cartridge_edge_72`, 93.5 x 11.3 mm over the 72 fingers, whose pads have no mask layer of
    their own), which Q18.7 leaves to the pads; KiCad caps the type at 199 (192 pre-existing
    Pad–Polygon + 7), and by geometry 51 routed track segments (48 nets) touch that aperture,
    against 57 in the designer's own board. Needs a new item for footprint-level mask apertures
    (operator decision: keepouts that still let each finger's net reach its pad, or accept edge
    connectors as inherent); parity and gate in the dispatch report.

- [x] **Q22.13 — The grader matches copper on boards that name nets by number** (added
  2026-10-20). In a KiCad 6–9 file (e.g. One-Air-Max, version 20241229), items name their net by
  number. A board KiCad 10 saves names it by name. `oracle_lib.read_copper` returns whatever the file holds, so on the
  input board as copied no copper key matches the routed board's. `plugin-input-ab.sh` then counts
  every item touching input copper as routing-caused: on One-Air-Max that was 199
  `drill_out_of_range` and 3 `clearance`, where 1 is routing's (found in K13.2). `grade.sh` is not
  affected, since both of its boards are saved by KiCad 10.
  - `read_copper` maps net numbers to names through the file's `(net N "name")` table, or through
    the footprint pads' nets on KiCad 10 files without a table, so both boards' keys use names.
  - **Accept:**
    - a test in `tools/kicad-oracle/tests/` reads one copper item written both ways and gets the
      same key;
    - One-Air-Max's K13.2 run (`target/orchestrator/q223/runs/`) summarized again: 1
      routing-caused copper error, the `VBAT`–F2 track;
    - the oracle's tests pass.

    *Done 2026-10-20:* `oracle_lib.read_copper` maps an unquoted `(net N)` through the file's
    top-level `(net N "name")` table (`net_table`; net 0 is the empty name); a quoted name is kept as
    written, so a name like `"12"` stays a name. KiCad 10 writes names, so no pad-net fallback was
    needed. Numbers with no table, or a number missing from it, raise `ValueError`. Tests (written
    first, 7 of the 8 failed before the change): `NumberedNetsTest` in
    `tools/kicad-oracle/tests/test_routing_caused.py` (`test_a_segment_written_both_ways_has_one_key`,
    `test_an_arc_and_a_via_written_both_ways_have_one_key`, `test_net_zero_is_the_empty_name`,
    `test_a_quoted_name_that_looks_like_a_number_stays_a_name`,
    `test_numbers_without_a_net_table_are_an_error`,
    `test_a_number_missing_from_the_table_is_an_error`,
    `test_a_footprint_pad_net_is_not_taken_for_the_net_table`,
    `test_an_old_input_track_is_pre_existing_next_to_a_new_routed_board`). One-Air-Max's K13.2 run
    (`target/orchestrator/q223/runs/kicad_pcb/One-Air-Max`) summarized again, no routing and no new
    DRC: `python3 -I tools/kicad-oracle/oracle_lib.py summarize routed/drc.json input/drc.json
    routed/copper.json grade-meta.json grade.json routed/One-Air-Max.kicad_pcb
    input/One-Air-Max.kicad_pcb` gives `drc_real` 8 = `clearance` 1, `silk_edge_clearance` 2,
    `silk_over_copper` 1, `silk_overlap` 4 (before: `clearance` 3, `drill_out_of_range` 199 and the same
    silk items). The one copper error is `Track [/Solar_module/VBAT]` on B.Cu against `Segment [<no net>]
    of F2` on B.Cu (clearance 0.1 mm, actual 0.0 mm). `below_minimum` computed with the same call as
    `plugin-input-ab.sh` goes from 410 (`via_drill`) to 0. Output in `target/orchestrator/q2213/out/`.
    `python3 -m unittest discover -s tools/kicad-oracle/tests -p 'test_*.py'`: 44 tests OK;
    `kicad-plugin/tests` (k02 venv): 454 passed, 20 skipped. `plugin-input-ab.sh` is unchanged.

- [x] **Q22.14 — `.kicad_dru` hole-to-hole rules reach the router** (added 2026-10-20; decided
  2026-10-20: routing never breaks the board's manufacturing rules). The reader logs
  `constraint hole_to_hole: UNSUPPORTED`. On main 6229b29 a ch32v003 PD6 via sat 0.4147 mm from
  a VBUS via, under its rule "hole to hole clearance (different nets)" (0.5 mm) (found in
  Q22.11). Option (a), decided 2026-10-20: the dru reader raises
  `router.board_minimums.hole_to_hole_um` to the largest `hole_to_hole` minimum among the rules
  that can apply. This is conservative, since it also holds a different-net rule between holes of
  one net, and it is logged with the rule's name. Both inputs: `--kicad-dru` on the DSN path and
  the `.kicad_pcb` reader.
  - **Accept:**
    - a reader test written first: a `.kicad_dru` with a 0.5 mm `hole_to_hole` rule raises the
      minimum to 500 µm, a smaller rule leaves Board Setup's value, and the log names the rule;
    - ch32v003 through the plugin's export, one KiCad grade: routing-caused `hole_to_hole` 0,
      with KiCad unconnected beside it.
  - *Done 2026-10-20:* on main `a36a5c0` (written on `e49b89a`, rebased; the grades below were
    rerun on `a36a5c0` and match the `e49b89a` runs in `target/orchestrator/q2214/runs-e49b89a/`).
    - **Reader.** `dru.rs` accepts the constraint `hole_to_hole`; `dru/item_types.rs::hole_to_hole`
      evaluates the condition for every pair of holes (via, plated pad, NPTH pad) with at least
      one via, each pair both of one net and of two (`Cond::with_same_net`: `A.Net == B.Net`
      taken true and false), and raises `DruReport::hole_to_hole` (largest `min`, the applied
      rules' names) for a rule that matches any; a rule between pads only is left out ("covers
      only pad pairs, which routing never moves"), one naming no hole is ignored. A rule that
      misses some pair (a different-nets rule) warns that it becomes a floor of
      `router.board_minimums.hole_to_hole_um` for every drill, of any net.
    - **Both inputs.** `--kicad-dru` on a DSN (`prepare_job` → `apply_kicad_dru`) and the
      `.kicad_pcb` reader with `apply_design_rules` (`JobInput::rules_hole_to_hole`, set by
      `set_kicad`). `raise_hole_to_hole` (a shared `raise_floor` with `raise_hole_clearance`)
      raises the setting after every merge, over Board Setup's `min_hole_to_hole` (the plugin's
      `--router.board_minimums.hole_to_hole_um`, or the `.kicad_pro` source) and the command line:
      the larger wins, and the log names the rules ("router.board_minimums.hole_to_hole_um raised
      from 250 to 500 by the .kicad_dru rules 'hole to hole clearance (different nets)', 'via to
      via clearance (same nets)'").
    - **Tests, written first** (`crates/freerouting-kicad/tests/dru_holes.rs` did not compile
      before the change: no `DruReport::hole_to_hole`): `a_hole_to_hole_rule_is_a_floor` (0.5 mm
      → 500000 nm, named, one warning, tree unchanged), `the_largest_rule_that_can_apply_wins`
      (ch32v003's three rules: applied, applied, pad pairs ignored; 500000),
      `a_rule_without_holes_is_left_out`, `the_job_raises_board_setups_hole_to_hole` (Board Setup
      250 → 5000 board units = 500 µm with the log line naming the rule; a 0.2 mm rule leaves 250
      with "is 250, no less"; Board Setup 600 kept; no Board Setup value → 500; no file →
      unchanged), `the_kicad_pcb_reader_reads_the_hole_to_hole_rules` (500000 with
      `apply_design_rules`, ignored without), `a_kicad_pcb_job_takes_the_larger_of_board_setup_and_the_rules`
      (`classes.kicad_pcb`, `.kicad_pro` default 0.25 mm, `.kicad_dru` 0.5 mm → setting 500.0
      and "Board minimum hole to hole 500.0 um").
    - **ch32v003** through the plugin's export (`tools/kicad-oracle/grade.sh --export plugin`,
      `{"schema": 4}`, one KiCad 10.0.7 grade; board copied to `target/orchestrator/q2214/board/`,
      runs in `target/orchestrator/q2214/runs/`, routes under `ulimit -v 8000000` and
      `timeout 300`): with Q22.14 and Q22.15 (`after`) routing-caused `hole_to_hole` **0**
      (`clearance` 0, `hole_clearance` 0, below-minimum 0; only `isolated_copper` 16), KiCad
      unconnected **34** (input 57), route 14.1 s. Q22.14 alone on main (`q2214-only`: main's
      binary with `--router.board_minimums.hole_to_hole_um=500` after the plugin's flags):
      `hole_to_hole` 0, unconnected 39 (main 38). Q22.15 alone (`q2215-only`: this build with the
      three `hole_to_hole` rules taken out of the `.kicad_dru` the router reads; KiCad grades
      with the original): `hole_to_hole` **3**, unconnected 30, so the floor is what keeps them
      out.
    - DIVERGENCES: freerouting-kicad, "`.kicad_dru` `hole_to_hole` rules reach the router".
      `--kicad-dru` in `help.txt` names the setting.

- [x] **Q22.15 — What Q22.5 costs on ch32v003** (added 2026-10-20). With the plugin's default
  options (`grade.sh --export plugin`, `{"schema": 4}`), ch32v003's KiCad unconnected went
  18 → 38 between main 6229b29 and 039a859, the Q22.5 merge, in both of Q22.11's arms. Q22.5's
  mapping is conservative in two places:
  - its hole rules become one board-wide `router.hole_clearance_um` floor of 330 µm, which
    also holds for via holes, where the rule names plated pads;
  - a routed default-class via keeps the via-to-track value from other vias and pads.

  Find which part costs the 20, and make the mapping exact where the engine can express it,
  for example a hole clearance for plated pad holes only, with 0 routing-caused manufacturing
  breaches kept.
  - **Accept:**
    - before and after rows on ch32v003 with the plugin defaults, one KiCad grade each:
      KiCad unconnected and routing-caused items by type;
    - each part's cost named, from a run with that part alone changed;
    - 0 routing-caused `clearance`, `hole_clearance` and `hole_to_hole` in the after row. If the
      cost stays, say why the engine can't express the exact rule.
  - *Done 2026-10-20:* on main `a36a5c0`, with Q22.14 in. Every row: ch32v003 through
    `tools/kicad-oracle/grade.sh --export plugin` (`{"schema": 4}`, enhanced, the plugin's Board
    Setup flags), one KiCad 10.0.7 grade, routes under `ulimit -v 8000000` and `timeout 300`
    (`target/orchestrator/q2214/route.sh`, `engine.sh`; runs in `target/orchestrator/q2214/runs/`).
    A part is switched alone by changing only the `.kicad_dru` the router reads (`engine.sh`
    swaps the `--kicad-dru` path; variants in `target/orchestrator/q2214/dru/`); KiCad always
    grades with the board's own file.

    | Row | router binary and rules | KiCad unconnected (input 57) | routing-caused by type | route |
    |---|---|---|---|---|
    | no `--kicad-dru` (Q22.5 off; 6229b29 had 18, `clearance` 19, `hole_clearance` 4, `hole_to_hole` 1) | main | 18 | `clearance` 17, `hole_clearance` 5, `hole_to_hole` 1, `isolated_copper` 17 | 4.0 s |
    | **before**: Q22.5 as merged | main, the board's rules | **38** | `isolated_copper` 12 | 11.5 s |
    | part 1 alone back: no hole floor | main, the three `hole_clearance` rules out (`router.hole_clearance_um` stays Board Setup's 250) | 22 | `hole_clearance` 9 (all a routed via's hole against a track under `'PTH to Track clearance)'` 0.33 mm), `hole_to_hole` 1 | 5.0 s |
    | the typed rule alone back | main, `'Pad to Track clearance)'` out | 38 | `clearance` 8, `isolated_copper` 14 | 11.9 s |
    | part 1 exact for vias (emulated) | main, the via and PTH hole rules replaced by `A.Type == 'via' && B.Type == 'track'` clearance 0.2538 mm (0.33 less the via's 76.2 µm ring); NPTH floor 254 | 34 | `hole_to_hole` 1 | 13.4 s |
    | Q22.14 alone | main, `--router.board_minimums.hole_to_hole_um=500` after the plugin's flags | 39 | `isolated_copper` 12 | 10.8 s |
    | Q22.15 alone | this change, the three `hole_to_hole` rules out | 30 | `hole_to_hole` 3, `isolated_copper` 20 | 14.2 s |
    | **after**: Q22.14 + Q22.15 | this change, the board's rules | **34** | `isolated_copper` 16 (`clearance` 0, `hole_clearance` 0, `hole_to_hole` 0, below-minimum 0) | 14.1 s |

    - **Part 1, the 330 µm hole floor, costs 16 of the 20** (38 → 22 without it), but 12 of those
      16 are KiCad's own rule: `A.isPlated()` is true for a via, and `'PTH to Track clearance)'`
      comes after `'via to track clearance'` in the file (KiCad's last matching rule wins), so a
      via hole keeps 0.33 mm from tracks; without the floor KiCad finds 9 such breaches. The
      conservative part, the floor also keeping every routed via's copper 0.33 mm from pad and via
      drills (KiCad asks Board Setup's 0.25 mm) and NPTH holes 0.33 mm from copper (0.254 mm),
      costs **4** (38 → 34 with only the via part expressed exactly).
    - **Part 2 costs 0 on ch32v003**: it has one net class, so a routed via's class is not kept
      at the via-to-track value from other vias and pads. Measured on the routed DSN with the
      board's `.kicad_dru` (`build_board` after `apply_to_pcb`, layer 0, 0.1 µm units): `via`
      against `via`, `pin`, `smd` and `area` 1270, against `default` (wires) 2538; `pin` against
      wires 3300; without the rules `default` 1270 and `smd` 318 only. No run can switch it, as it
      does not arise (Q22.5's `plated_is_exact_with_one_class`).
    - **The typed rule** `'Pad to Track clearance)'` costs 0 beside the floor (38 → 38 without it,
      with 8 `clearance` breaches) and 4 without it (22 vs 18).
    - **The mapping made exact** (`dru/item_types.rs`, module docs "Hole clearance"): a
      `hole_clearance` rule between plated holes and tracks only becomes `via_wire` of its value
      less the smallest annular ring of the board's via padstacks, read from their names
      (`Via[a-b]_<diameter>:<drill>_um`, the name the engine reads a via's drill from; ch32v003
      0.33 − 0.0762 = 0.2538 mm), and `pin_wire` of its value (warned: the DSN gives no pad drill,
      so the pad's copper edge stands for its hole). Any other match (an NPTH hole, a hole against
      pad or via copper) and a via name without a drill stay floors of `router.hole_clearance_um`.
      ch32v003's floor is now the NPTH rule's 254 µm. A pad-pad hole pair is left out.
      Tests (`crates/freerouting-kicad/tests/dru_holes.rs`):
      `a_plated_hole_rule_becomes_via_and_pin_to_track_clearances` (fixture via 600:300: `via_wire`
      180, `pin_wire` 330, no floor, one warning naming the rule),
      `a_via_hole_rule_uses_the_via_annular_ring` (0.4 mm → `via_wire` 250; 0.254 mm asks no more
      than 150 µm and is left out), `npth_and_pad_copper_rules_stay_floors` (254000, 300000),
      `a_via_without_a_known_drill_keeps_the_floor` (330000),
      `the_job_keeps_board_setups_hole_clearance_for_a_plated_rule` (hole clearance 2500 units,
      `via`–wire 1800). Q22.5's `hole_clearance_is_a_floor`,
      `the_job_applies_the_rules_and_the_hole_floor` and `ch32v003_rules_report` now use an NPTH
      or pad-copper rule for the floor (`'via to track clearance'` is "asks for no more than the
      DSN gives", the floor 254000).
    - **After: 34, 0 routing-caused breaches.** The 4 the exact via mapping saves on main are
      eaten by Q22.14's 500 µm hole-to-hole floor (Q22.15 alone 30 with 3 `hole_to_hole`; Q22.14
      alone 39). The rest of the 18 → 34 is what the board's rules ask (via holes 0.33 mm from
      tracks, plated copper 0.2 mm from tracks, 0.5 mm between holes of different nets).
    - **What the engine cannot say yet** (follow-ups, not code here):
      1. a hole-to-hole distance per net relation: `BoardRules::hole_to_hole` is one value for
         every drill pair, so ch32v003's 0.5 mm rule for different nets also holds between two
         vias of one net (its rule 0.254 mm); `board/hole_spacing.rs` would take a same-net and a
         different-net distance (Q22.14's option (b));
      2. a plated pad's hole against tracks: the DSN gives no pad drill, so `pin_wire` takes the
         whole value; the plugin could name the drill in the pad's padstack, as KiCad names a
         via's, or the engine could keep a hole-to-track clearance apart from
         `router.hole_clearance_um` (in `drill_hole_clearance_delta` for the trace trees, and in
         `drc/board_rules.rs`'s hole check for traces), with the global for via copper;
      3. the NPTH floor (254 µm) is global, so it also keeps via copper 4 µm further from every
         drill than Board Setup asks; the same split as 2 would remove it.
    - DIVERGENCES: freerouting-kicad, the Q22.5 line ("unless it is between plated holes and
      tracks only (Q22.15) ..."). `--kicad-dru` in `help.txt` says how hole rules are applied.

- [ ] **Q22.16 — Footprint mask apertures keep other nets out** (added 2026-10-20; decided
  2026-10-20: routing never breaks the board's manufacturing rules, so no grader exception for
  edge connectors). fc-rom's J1 (`cartridge_edge_72`) has one `fp_poly` on F.Mask and one on
  B.Mask, an aperture of 93.5 × 11.3 mm over 72 fingers. The finger pads have no mask layer of
  their own. Q18.7 leaves footprint mask shapes to the pads, so tracks run through the aperture:
  7 routing-caused `solder_mask_bridge` items (Track–Polygon) on main e49b89a. KiCad caps the
  type at 199, so the real count is hidden. By geometry, 51 routed segments of 48 nets touch the
  aperture. Found in Q22.12.
  - Footprint mask shapes become keepouts on their copper side, in the plugin and the
    `.kicad_pcb` reader. A pad inside the aperture keeps a way in for its own net: a track
    reaches it from the part of the pad outside the aperture (J1's pads extend 2 mm beyond it),
    or the keepout excludes the pad's own copper. Pick whichever the engine supports. Q20's
    output check judges against the same shapes.
  - **Accept:**
    - plugin and reader tests written first on a fixture: a footprint mask polygon over two pads
      of different nets; a third net's route stays out; each pad's own net still connects;
    - fc-rom through the plugin's export, one KiCad grade: 0 routing-caused `solder_mask_bridge`,
      with KiCad unconnected and route time beside it (main: 79 unconnected, 271 s; use
      `--timeout 270` or a pass cap, the same in both arms).

- [ ] **Q22.17 — Net shapes cost less to route around** (added 2026-10-20). Q22.4 makes each
  copper shape of a listed net a fixed conduction area of its net. That is correct, but on
  miniscope its 111 area pieces, mostly arc chords at the 5 µm maximum error, leave 67 KiCad
  unconnected against main's 61 at an equal 270 s cap. Its best pass leaves 81 unrouted against
  69 (decided 2026-10-20: keep the areas and reduce their cost).
  - Merge each stroked shape's pieces into one outline polygon, and chord net-shape arcs at a
    coarser maximum error that still covers the copper, in the plugin and the reader alike.
  - Check the open question from Q22.4: a pin connects to an area only when its drill centre is
    inside it (Java's rule). Count the pads on miniscope, vfo and openair that only overlap a net
    shape's edge, and fix the connection test for `enhanced` if KiCad calls them connected.
  - **Accept:**
    - plugin and reader tests: a stroked arc shape becomes one area that covers the stroke;
    - miniscope through the plugin's export at the same cap, one KiCad grade: 0 routing-caused
      copper errors, with KiCad unconnected beside Q22.4's 67 and main's 61;
    - the pad count, and its fix or the reason it stays.
