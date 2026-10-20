# Q19 — Routed copper keeps the board's clearance

Opened 2026-10-20. Decided 2026-10-20: routing never breaks the board's minimum manufacturing
rules, and clearance between copper of different nets is the main one. Every arm's routed output
must have zero routing-caused KiCad `clearance` errors; "no new breach versus baseline" no
longer applies.

The baseline arm (`enhanced`, main `8918a1b`, N03's grades in
`target/orchestrator/n03/out/grade/tree+nodes/`) had KiCad DRC `clearance` errors in routed
copper on two corpus boards, all reported against a 0.2 mm clearance:

| Board | Clearance errors | Pairs |
|---|---|---|
| d3c-0496-kitspace-f-91w | 169 | Track–Via 58, Track–Track 54, Pad–Track 41, Pad–Pad 9, Pad–Via 8, Via–Via 8, PTH–Pad 1 |
| d3c-0599-rgbmatrixpanelcpld-photonbackpack-rgbmatrixpanel-cpld | 88 | |
| the other eight d3c boards and the q1112 boards | 0 | |

Prerequisites: F02.6 (the KiCad oracle), N03 (the grades above).

## Checklist

- [x] **Q19.1 — Where the 0.2 mm comes from.** For a sample of the violations, find the rule
  source (net class, `.kicad_dru` rule, zone clearance), the clearance the exported DSN gives
  the net pair, and the clearance the router kept; classify the cause as (i) the DSN does not
  carry KiCad's rule, (ii) a geometry or rounding shortfall in the router, (iii) a stage that
  skips the clearance check, or something else.
  - **Accept:** the cause with evidence for both boards: rule source, DSN value, router value,
    one example's geometry.

  *Done 2026-10-20:* **something else: the KiCad oracle graded legacy boards under KiCad's
  default rules, not the board's.** The router kept the board's clearance everywhere.
  - Rule source: both boards are KiCad 4 files (`(kicad_pcb (version 4)`) with no
    `.kicad_pro` and no `.kicad_dru` in `target/corpus-d3c/projects/<board>/` (none of the 11
    d3c projects has a `.kicad_pro`). Their net classes are in the board file:
    d3c-0496 `Default` and `PWR` both `(clearance 0.1524)`, d3c-0599 `Default` and `Power`
    both `(clearance 0.14986)`. KiCad 6+ moves these into the project when it loads the board.
    `grade.sh`'s strip step (`pcbnew.LoadBoard` + `SaveBoard`) wrote that migrated
    `.kicad_pro` next to `unrouted/` (reproduced by hand: its `net_settings.classes` are
    `Default 0.1524`, `PWR 0.1524`, `netclass_patterns` per net, `min_via_diameter 0.46`), the
    DSN export read it, and then `original_rules()` deleted it before the DRC because the
    project has no original `.kicad_pro`. `kicad-cli` then graded the KiCad 10 copy under
    KiCad's default class, clearance 0.2 mm (every one of the 179 + 88 items says
    `clearance 0.2000 mm`).
  - DSN value: `work/<board>.dsn` has `(clearance 152.4)` (d3c-0496) and `(clearance 149.86)`
    (d3c-0599) in the structure rule and in every class.
  - Router value: the smallest `actual` over all of KiCad's items is 0.1540 mm on d3c-0496
    (rule 0.1524) and 0.1517 mm on d3c-0599 (rule 0.14986); the largest 0.1992 and 0.1971.
    Example: `Track [/LCD_SEG1]` (class `kicad_default`) and `Track [GND]` (class `PWR`) on
    F.Cu near (52.85, 103.55) mm, `actual 0.1865 mm`, 34 µm more than the 152.4 µm both
    classes give the pair.
  - The only boards with errors are the only two whose classes are below 0.2 mm: per-board
    class clearances in the DSN are 200 (0400, 0448, 0473, 0546, 0571, 0581), 199/200 (0423),
    203.2 (0520), 152.4 (0496) and 149.86 (0599).
  - Check: `kicad-cli pcb drc --format json --severity-all --refill-zones` of N03's routed
    d3c-0496 board with the migrated `.kicad_pro` (copper classes forced to error) next to it:
    0 `clearance` items (was 179), 14 unconnected (as before).
  - The pair table above counts all 179 routed items; the unrouted grade's 10 `clearance` items
    are exactly the 9 Pad–Pad and 1 PTH–Pad (footprint copper, no routing; the grade subtracted
    them as pre-existing), and they are the same artifact: 0 under the board's rules (Q19.3).

- [x] **Q19.2 — Failing test, then the fix.** A small fixture written for the purpose that
  reproduces the shortfall, red before the fix and green after; the fix where the cause is.
  - **Accept:** the test fails at `main` and passes with the fix; the fix sits where Q19.1 found
    the cause; no board specifics copied.

  *Done 2026-10-20:* the cause is the grader, so the fix and the test are in
  `tools/kicad-oracle/`; no engine, plugin or DSN change, so `legacy` is untouched and there is
  no quirk, divergence or profile change.
  - Fix (`grade.sh`): right after the strip step, a project without a `.kicad_pro` keeps the
    one KiCad's `SaveBoard` wrote from the original board in `OUT/work/rules/`, and
    `original_rules()` copies it over both staged boards (then forces the copper classes to
    error, as for a project file). Written before the engine runs, so a router still never
    grades itself under rules it wrote. `meta.json`/`summary.json` gain `rules`: `project`,
    `board` or `defaults`. README step 5, the summary field list and the file table updated.
  - Test: `tools/kicad-oracle/tests/run-legacy-rules-test.sh WORK_DIR` (KiCad 10.0.7, no
    Java) with `tests/make_legacy_board.py`, a 20 x 20 mm KiCad 4 board written as text (nets
    A and B, net class clearance 0.15 mm, no `.kicad_pro`) and two sessions with 0.2 mm tracks
    0.17 mm and 0.12 mm apart. At `main`: `gap 0.17 mm: exit 1, routed clearance items 3`
    (`clearance 0.2000 mm; actual 0.1700 mm`), FAIL. With the fix: `gap 0.17 mm: exit 0,
    routed clearance items 0, rules board`; `gap 0.12 mm: exit 1, router-attributable
    clearance items 3`; originals unchanged; exit 0.
  - Regression: `run-oracle-tests.sh`'s check 3 (the pcbnew-made edge board, no `.kicad_pro`,
    `inject.py empty` engine; the other checks need a Freerouting jar and were not run) by
    hand with the fix: `graded: drc_real=0 preexisting=1`, exit 0, `rules board`, as before.
    `python3 -m unittest discover -s tools/kicad-oracle/tests -p 'test_*.py'`: 16 tests OK.

- [x] **Q19.3 — Re-measure.** d3c-0496 and d3c-0599 plus two clean boards (d3c-0400,
  d3c-0546), baseline against fixed, one KiCad grade per distinct session.
  - **Accept:** routed clearance errors 0 on all four; KiCad unconnected, `drc_real` by type
    and vias reported, with the cost.

  *Done 2026-10-20:* the router is unchanged, so the sessions are N03's baseline sessions
  (`target/orchestrator/n03/out/tree+nodes/<board>.ses`, main `8918a1b`, `enhanced`), copied
  to `target/orchestrator/q19/out/fixed/` and graded with the fixed grader by
  `tools/bench/grade-distinct.sh target/orchestrator/q19/out <board>
  target/corpus-d3c/projects/<board> fixed` (one KiCad run per board, KiCad 10.0.7). No
  re-route, so no `quick-arm.sh` run: the fix changes no route. Before is N03's grade of the
  same session.

  | Board | Grade | `rules` | real clearance | routed clearance | input clearance | unconnected | `drc_real` | vias | `drc_real` by type |
  |---|---|---|---|---|---|---|---|---|---|
  | d3c-0496 | before | – | 169 | 179 | 10 | 14 | 400 | 72 | annular_width 59, clearance 169, copper_edge_clearance 40, hole_clearance 3, isolated_copper 1, solder_mask_bridge 13, track_dangling 1, track_width 36, via_dangling 19, via_diameter 59 |
  | d3c-0496 | after | board | **0** | **0** | 0 | 14 | 208 | 72 | annular_width 59, copper_edge_clearance 40, hole_clearance 40, isolated_copper 1, solder_mask_bridge 12, track_dangling 1, track_width 36, via_dangling 19 |
  | d3c-0599 | before | – | 88 | 88 | 0 | 20 | 383 | 20 | clearance 88, solder_mask_bridge 89, starved_thermal 4, track_dangling 2, track_width 199, via_dangling 1 |
  | d3c-0599 | after | board | **0** | **0** | 0 | 20 | 166 | 20 | solder_mask_bridge 90, starved_thermal 4, track_dangling 2, track_width 69, via_dangling 1 |
  | d3c-0400 | before | – | 0 | 0 | 0 | 13 | 17 | 26 | starved_thermal 3, track_width 14 |
  | d3c-0400 | after | board | 0 | 0 | 0 | 13 | 17 | 26 | starved_thermal 3, track_width 14 |
  | d3c-0546 | before | – | 0 | 0 | 0 | 3 | 20 | 2 | starved_thermal 4, track_width 16 |
  | d3c-0546 | after | board | 0 | 0 | 0 | 3 | 20 | 2 | starved_thermal 4, track_width 16 |

  Cost: none in completion or vias (same sessions). The other rule types move with the rules
  and belong to Q18 (board-minimum floors): on d3c-0496 `via_diameter` 59 -> 0 (the board's
  `via_min_size 0.46`; before, `board setup constraints min diameter 0.5000 mm`, KiCad's
  default), `hole_clearance` 3 -> 40 (KiCad reports one item
  per pair; these hid behind clearance items before, and the migration's 0.25 mm hole
  clearance exceeds 0.1524 clearance + 0.08 annular ring of the 0.46/0.3 via); on d3c-0599
  `track_width` 199 (capped) -> 69.

- [x] **Q19.4 — Small parity check.** `PARITY_SET=tools/bench/sets/connection-parity.txt`;
  `legacy` stays exact.
  - **Accept:** the parity summary line.

  *Done 2026-10-20:* no engine change, run anyway:
  `PARITY_SET=tools/bench/sets/connection-parity.txt PARITY_OUT=target/orchestrator/q19/parity
  cargo test --release -p freerouting --test autoroute_board_parity -- --ignored
  autoroute_board_parity --test-threads=1 --nocapture` (worktree `land/q19` at `87128fb` plus
  this diff): 7 boards, all `[exact] rust completed eq true bytes true` (Issue368, Issue110,
  Issue433, PCBench-retroreflectors-CONGFLOCK, Issue413, Issue143, Issue508);
  `test result: ok. 1 passed; 0 failed`. `legacy` stays exact.
