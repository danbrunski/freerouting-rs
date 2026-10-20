# Q18 — Board minimums as hard floors, and strict rip-up's guards

Opened 2026-10-20 from Q11.6 ([archive/Q11](Q11-transactional-ripup.md)). Strict
transactional rip-up (`router.autorouter.connection_router=transactional-ripup`,
`quality/transaction.rs`) routed much more than Java's rip-up on Q11's 15 quick rows (total
220 -> 192 against `mp1`; canary-pwr-9511d9a 50 -> 31 KiCad unconnected), but failed hard
guards: `below_minimum` 4 on bm10 and 5 on vfo, and vfo's real DRC errors 85 -> 93. Q11's
fail row said remove it; decided 2026-10-20: keep it, fix the guard failures, re-measure.

Scope widened the same day (decided 2026-10-20): the default router breaks the board's
minimum track width too, not only strict rip-up (Q11.6's `mp1` grades: KiCad `track_width`
errors laser-backplane 10, mydevboard 74, slice 16, vfo 72; `copper_edge_clearance` mydevboard
18, vfo 4). **New rule: routing never breaks the board's minimum manufacturing rules.** Every
routed output has zero below-minimum copper (track width, via diameter, via drill) and zero
routing-caused copper-to-edge clearance errors, in every profile, whenever the minimum is known.
"No new breach versus baseline" no longer applies to these.

Widened again the same day (decided 2026-10-20): the same plumbing covers via sizes (annular ring
too) and hole-to-copper clearance (N03's grades of d3c-0496-kitspace-f-91w, `tree+nodes`: KiCad
`via_diameter` 59, min 0.5 mm, actual 0.46; `annular_width` 59; `copper_edge_clearance` 40;
`hole_clearance` 3, Pad–Via; `solder_mask_bridge` Pad–Track 7). The copper clearance shortfall
on d3c-0496/0599 (Track–Track 0.1865 against 0.2 mm) is Q19's; Q18 does not touch clearance
geometry or the DSN clearance import. Decided 2026-10-20, after Q18.6's runs: KiCad-graded rows
use only boards whose project has its own `.kicad_pro` (KiCad 6 or later); d3c-0496 (KiCad 4) is
reported as information only, because the rules KiCad invents on conversion contradict its own
design (its 0.46/0.3 mm class via has a 0.08 mm annular ring against the converted 0.1 mm
minimum). d3c-0599 is out of scope for the same reason.

Prerequisites: Q11 (the router), N02 (the totals rule), N05 (the promotion recipe).

## Decisions made here

- **Where the minimums come from.** The KiCad plugin and a `.kicad_pcb` input pass KiCad's Board
  Setup minimums (`m_TrackMinWidth`, `m_ViasMinSize`, `m_MinThroughDrill`,
  `m_ViasMinAnnularWidth`, `m_CopperEdgeClearance`, `m_HoleClearance`); the CLI takes them as
  `--router.board_minimums.*`, `--router.copper_to_edge_clearance_um` and
  `--router.hole_clearance_um` for a bare DSN.
- **A via type below the via minimums is routed at the floor size** (decided 2026-10-20): a net
  class's via smaller than the board allows becomes a via that meets the minimum, made at the
  floor size, with one warning naming the class. With nothing supplied, `enhanced`
  floors the track width at the narrowest class width of the DSN
  (`router.board_minimums.from_classes`); `legacy` keeps Java's behaviour (parity).
- **Correctness wins over completion**: a connection that only fits below the minimum stays
  open. The cost in unconnected items is measured and reported, not judged (Q18.6 pair A).

## Checklist

- [x] **Q18.1 — Find the causes.** Which code paths make the narrow traces, the extra DRC errors
  and the edge-clearance errors on bm10, vfo and mydevboard; first from Q11.6's kept sessions
  (`target/orchestrator/q1112/out/`), then a small repro test.
  - **Accept:** each width source and the edge-clearance source named, with the evidence; a
    failing test for each before its fix.
  - *Done 2026-10-20:* one width source, three readers, no strict-specific bug:
    - **Width.** Every sub-class-width trace in the Q11.6 sessions is 1500 board units
      (0.15 mm) on a 0.2 mm board: bm10 `mp1` 60 / `strict` 64 paths, vfo 47 / 49, mydevboard
      47 / 51 (SES path widths; laser's 0.187 mm is a pin's own width, above 0.15). They are
      Java's pin neck-down (`automatic_neckdown`: `session/search.rs` at a start pin door and
      `path/insert.rs` `try_neck_down`, both through `AutorouteControl::neckdown_half_width`,
      the pin's width floored only by `router.min_trace_width_um`), which the measurement
      commands set to 150 µm, below the 0.2 mm Board Setup minimum. The necked retry
      (`neck_half_width`), the fanout's micro neck-down and `bounded-neck` read the same floor.
      The neck retry at `enhanced`'s 300 µm narrows nothing on these 0.2 mm classes. The width
      reaches no rollback: a transaction restores a `Clone` of the board (exact).
    - **Strict's extra breaches are the same source with more rip-ups.** bm10: the 0.15 mm
      paths differ on 15 nets between `mp1` and `strict` (17 only in `mp1`, 21 only in
      `strict`, all at pins, plus one 10 mm 0.15 mm trace on `/ROLL`, Route2, in a through-hole
      pin's neck room). vfo: 8 only in `mp1`, 10 only in `strict`, on 7 nets (LOCK and PTT new);
      its +8 real DRC errors are +5 KiCad `track_width` (`under_class_width_tracks` 72 -> 77)
      and +3 `via_dangling` (PTT, PTT_LED, BB_SCL; `mp1` has 5 of its own, so a router
      artifact, not a strict one). Which transaction made which path is not traced.
    - **Edge clearance.** KiCad's `min_copper_edge_clearance` (0.5 mm on mydevboard and vfo) is
      not in the DSN; the router keeps `router.copper_to_edge_clearance_um`'s default 250 µm
      (capped at the closest pin's gap), so tracks land 0.25-0.5 mm from the outline (mydevboard:
      18 track segments of GPIO2/5/16/18/22 at x ≈ 205.8 mm; vfo: 4 FG tracks at y ≈ 87.3 mm).
      The plugin's `edge_and_pad_keepouts` fix-up covers it on a plugin run; a bare DSN run has
      nothing.
    - **Repro, failing first:** `crates/freerouting/tests/data/quality/q18-neck-floor.dsn` (a
      0.12 mm pad between two pads at 0.35 mm pitch, one 0.2 mm class);
      `tests/board_minimums.rs::java_pin_neck_down_goes_under_the_class_width` pins Java's
      1198-unit neck (also with `min_trace_width_um=100`); `from_classes_floors_at_the_narrowest_class_width`
      failed (`[1198, 2000, ...]`) until `enhanced` carried the floor.
- [x] **Q18.2 — Hard floors in the router.** No trace narrower than the minimum track width
  from any source, no via below the minimum diameter or drill; bare DSN: `enhanced` floors at the
  smallest class width, `legacy` keeps Java's behaviour.
  - **Accept:** a test per width source; legacy unchanged (the small parity check, 7/7 exact);
    DIVERGENCES line, module docs, pinning test.
  - *Done 2026-10-20:* settings section `router.board_minimums.{track_width_um,
    via_diameter_um, via_drill_um, from_classes}` (`settings/fields.rs`,
    `settings::board::BoardMinimums`); `autoroute::control::min_trace_half_width` is the larger
    of Java's floor and `board_min_trace_half_width` (the known minimum, else with
    `from_classes` `narrowest_class_half_width`), so all four readers above and KRT's fanouts
    are floored; `init_net` raises a class width below a known minimum; `rebuild_via_info`
    drops via types below the via minimums (`via_meets_minimums`, via `merged_via_rule`).
    `enhanced` gets `board_minimums.from_classes = true` (ENHANCED entry, DIVERGENCES "Profile
    `enhanced`" line). Tests (`crates/freerouting/tests/board_minimums.rs`, 8 green):
    `the_board_minimum_floors_the_pin_neck_down` (0.15 mm: routed, no trace under 1500; 0.2 mm:
    nothing under 2000, the connection stays open), `the_necked_retry_stays_at_the_board_minimum`
    (retry at 0.1 mm: 1000 units; with a 0.15 mm minimum: 1500),
    `from_classes_floors_at_the_narrowest_class_width` (also under profile `enhanced`; a known
    0.1 mm minimum gives Java's 1198 back), `a_board_minimum_under_the_neck_changes_nothing`
    (SES byte-identical), `the_control_floor_is_the_larger_of_the_user_floor_and_the_board_minimum`,
    `vias_under_the_board_minimums_are_made_at_the_floor` (Q18.4). Parity: small check
    (`PARITY_SET=tools/bench/sets/connection-parity.txt cargo test --release -p freerouting --test
    autoroute_board_parity -- --ignored autoroute_board_parity`) 7/7 exact.
- [x] **Q18.3 — The minimums from KiCad.** The plugin and the `.kicad_pcb` input pass Board
  Setup's minimums; the CLI takes them for a bare DSN.
  - **Accept:** the plugin's arguments and the `.kicad_pcb` job's settings carry them, pinned
    by tests.
  - *Done 2026-10-20:* plugin: `options.board_minimum_args` and
    `export_flow.board_minimums_nm` (both frontends through `export_flow.router_args`) pass
    `--router.board_minimums.{track_width_um, via_diameter_um, via_drill_um}` and
    `--router.copper_to_edge_clearance_um` from `m_TrackMinWidth`, `m_ViasMinSize`,
    `m_MinThroughDrill`, `m_CopperEdgeClearance` (0 passes nothing);
    `test_options.py::test_board_minimums_become_hard_floors`,
    `test_router_args_carry_the_board_minimums`, `test_gui_mode.py` updated;
    `python3 -m pytest kicad-plugin/tests`: 366 passed, 19 skipped. `.kicad_pcb`:
    `freerouting_kicad::project::BoardMinimums` reads `min_track_width`, `min_via_diameter`,
    `min_through_hole_diameter`, `min_copper_edge_clearance` into `Extras::board_minimums`;
    `JobInput::set_kicad` adds them as a settings source at the DSN's priority
    (`job::prepare::board_minimums_patch`; the command line wins):
    `tests/board_minimums.rs::a_kicad_pcb_input_passes_its_board_setup_minimums` (Issue180-Test:
    254 / 381 / 254 / 254 µm; a CLI drill wins). Bare DSN: the generic `--router.board_minimums.*`
    flags.
- [x] **Q18.4 — Via floors.** Diameter at least `m_ViasMinSize`, drill at least
  `m_MinThroughDrill`, annular ring at least `m_ViasMinAnnularWidth`; a smaller net class via is
  routed as one at the floor size, with one warning naming the class.
  - **Accept:** a test that the via type is made at the floor, one that routed vias meet it, the
    warning pinned; DIVERGENCES line; 0 routing-caused `via_diameter`, `annular_width` and
    `drill_out_of_range` on Q18.6's graded rows in `mp1` and `strict`.
  - *Done 2026-10-20:* `job::via_minimums::apply_via_minimums` (called last in
    `job::overrides::apply_all`) makes a via type below `router.board_minimums.{via_diameter_um,
    via_drill_um, via_annular_um}` a round padstack `Via[<from>-<to>]_<d>:<drill>_um` at
    `max(drill, min drill)` and `max(d, min d, drill + 2 · min annular)`, carried by the
    session's `library_out` (the plugin's `ses_vias.py` sets size and drill from the name); one
    warning per net class names the class (d3c-0496: "Net class 'default': via
    'Via[0-1]_460:300_um' (460 um, drill 300 um) is below the board minimums (via 460 um, drill
    300 um, annular ring 100 um); routed vias of it are made as 'Via[0-1]_500:300_um'.");
    `AutorouteControl::via_meets_minimums` (annular too) is the backstop. Tests
    (`crates/freerouting/tests/board_minimums.rs`):
    `vias_under_the_board_minimums_are_made_at_the_floor` (the floor, the warning per class, at
    the floor already unchanged), `routed_vias_meet_the_board_minimums` (q14-hv-via, 0.7 mm
    floor: every routed via ≥ 7000 units, the padstack in the session). Plugin
    `m_ViasMinAnnularWidth` → `--router.board_minimums.via_annular_um`; `.kicad_pro`
    `min_via_annular_width` read. DIVERGENCES `board_minimums.*` line. Q18.6: 0 routing-caused
    `via_diameter`, `annular_width` and `drill_out_of_range` on every graded row of `mp1` and
    `strict` (and on the information row d3c-0496, `before` 71 `annular_width` → 0).
- [x] **Q18.5 — Copper keeps the board's copper-to-edge and copper-to-hole clearance.** With
  `m_CopperEdgeClearance` and `m_HoleClearance` supplied (the plugin, a `.kicad_pro`, or the CLI's
  `--router.copper_to_edge_clearance_um` and `--router.hole_clearance_um`), both kept as
  obstacles.
  - **Accept:** the plugin's and the `.kicad_pcb` job's settings carry both, pinned by tests;
    0 routing-caused `copper_edge_clearance` and `hole_clearance` on every graded row of Q18.6's
    `mp1` and `strict`.
  - *Partial 2026-10-20:* done and tested: the plugin passes `m_HoleClearance` as
    `--router.hole_clearance_um` and `m_CopperEdgeClearance` as
    `--router.copper_to_edge_clearance_um` (`test_options.py::test_board_minimums_become_hard_floors`;
    pytest 366 passed, 19 skipped); a `.kicad_pcb` job sets both from the `.kicad_pro`
    (`min_hole_clearance`, `min_copper_edge_clearance`), KiCad's defaults (0.25, 0.5 mm) where the
    file leaves them out (`a_kicad_pcb_input_passes_its_board_setup_minimums`,
    `kicad_board_minimums_default_to_kicads_and_zero_is_none`). Both are the router's existing
    obstacles (Java's `ShapeSearchTree.drillHoleClearanceDelta`, the edge keepout class); no
    clearance geometry or DSN clearance import changed (Q19's area). Q18.6: `hole_clearance` 0 on
    every graded row of `mp1` and `strict` (and on the information row d3c-0496, `before` 77 →
    0); `copper_edge_clearance` 0 on every judged row except slice, an edge the DSN does not
    carry, so the router never saw it:
    - slice-dcmt-bread-slice 2 (`mp1` and `strict`; `before` 0): a `+5V` via and track at 0 mm
      from a "Segment on Margin" (KiCad checks edge clearance against the Margin layer too;
      KiCad's DSN export has only Edge.Cuts).
    - Information only (amendment 2), the same kind: d3c-0496-kitspace-f-91w 24 (`mp1`), 57
      (`strict`), 43 (`before`): KiCad 4 board whose
      Edge.Cuts does not close, so the prepped DSN's boundary is the bounding rectangle
      45.66..70.16 × 84.88..109.36 mm and the segments inside it (17 places, e.g. (57.28,
      106.27), (49.30, 87.59)) are not in the DSN; copper 0.22-0.37 mm from them.

    Remains: the DSN input must carry them, as keepouts from the plugin or the `.kicad_pcb`
    reader (Margin-layer lines; for KiCad 6+ boards also Edge.Cuts segments outside the exported
    outline), then 0 on slice (`bash target/orchestrator/q18/runs.sh mp1 strict` and
    `BOARDS=slice-dcmt-bread-slice bash target/orchestrator/q18/grade.sh mp1 strict`).
  - *Done 2026-10-20:* the remainder. Every Margin shape and every Edge.Cuts shape off the
    outline's outer contours (outline holes, lines that close nothing, all of Edge.Cuts when the
    bounding box stands in for a malformed outline) becomes keepouts named `edge_clearance` on
    every copper layer, the rectangle around each straight piece grown by the edge clearance +
    half the Margin stroke − the DSN's default clearance (at least 1 µm; curves by the chord
    error + 1 µm). Plugin: `edge_graphics.py` (new), `kicad_board.edge_graphics`,
    `export_flow.fix_edge_graphics` (always on); tests `test_edge_graphics.py` (e.g.
    `test_a_margin_line_becomes_a_keepout_on_every_copper_layer`,
    `test_a_malformed_outline_exports_the_bounding_box_so_every_line_inside_is_kept`,
    `test_hole_contours_get_the_edge_clearance_too`). Reader: `crates/freerouting-kicad/src/edge_keepouts.rs`
    (new), wired in `read.rs`; tests `tests/edge_keepouts.rs` (fixture
    `tests/data/q18-edge-graphics.kicad_pcb`:
    `margin_and_loose_edge_cuts_shapes_become_keepouts_on_every_copper_layer`,
    `the_keepouts_follow_the_projects_edge_clearance`, `the_keepouts_round_trip_through_dsn_text`)
    and `kicad_parity.rs::edge_keepouts_are_the_only_extra_keepouts` (motorizedopener,
    tastexx-pcb, outline). DIVERGENCES lines in the freerouting-kicad and kicad-plugin sections.
    Re-measure (`target/orchestrator/q18b`: one release build of the land/q18b worktree on main
    b98074e, sha256 28cf6822…; the plugin's keepouts applied to the prepped DSNs by `edge_prep.py`,
    slice 21 shapes → 186 keepouts, d3c-0496 76 → 152; `runs.sh` = Q18's flags, `ulimit -v
    8000000`, `timeout 120`; KiCad 10.0.7 grades by `grade.sh`): routing-caused
    `copper_edge_clearance` slice 2 / 2 → **0 / 0** (`mp1` / `strict`), `hole_clearance` 0 / 0;
    d3c-0496 (information) edge 24 / 57 → 0 / 0. The other rows' inputs do not change:
    mydevboard, vfo, laser-backplane and flight-computer prepped DSNs byte-identical after
    `edge_prep.py` (`cmp`), canary-pwr-9511d9a gets 0 keepouts (8 Edge.Cuts shapes, all on the
    outline), so Q18.6's 0 stands for them.
- [x] **Q18.6 — Quick re-measurement** (`docs/roadmap/prereg/Q18.json`, set
  `tools/bench/sets/q18-board-minimums.txt`, pinned by
  `crates/freerouting-dev/tests/prereg_q18.rs`; decided 2026-10-20: a few minutes per run,
  canary-pwr fine, one grade per distinct session). Pair A: `before` (main's behaviour) against
  `mp1` (the floors with each row's Board Setup minimums) on laser-backplane, mydevboard, slice
  and vfo (d3c-0496 information only, Q18.json amendment 2); pair B: `mp1` against `strict` on Q11's 11 eligible rows.
  - **Accept:** registered before any number; hard guards raw in `mp1` and `strict` on every
    row: `below_minimum` (track_width, via_diameter, annular_width, drill_out_of_range) = 0,
    `edge_clearance` = 0, `hole_clearance` = 0; unconnected reported next to them; pair B judged
    by N02's totals rule.
  - *Partial 2026-10-20:* registered before any run (Q18.json rewritten twice before the first
    route, for the via/hole scope and Q19's grader fix; one amendment after the runs:
    `edge_clearance`/`hole_clearance` judged as guards by the gate, which refuses a null hard
    metric on the 5 ungraded fixture rows, the zero rule read from the table below; amendment 2:
    d3c-0496 information only; sha256 00dfd14d…, pinned by `prereg_q18.rs`). One release build of land/q18 on main 038e0f5
    (`target/orchestrator/q18/out-binaries.sha256`); every route under `ulimit -v 8000000` and
    `timeout` (600 s canary, 120 s others), none timed out; KiCad 10.0.7 grades by main's fixed
    `grade.sh` (`rules`: project, d3c-0496 `board`). Raw counts, routing-caused (KiCad
    `drc_real_by_type`): below = track_width + via_diameter + annular_width + drill_out_of_range.

    | row | arm | below | edge | hole | KiCad unconnected | unrouted | drc_real |
    |---|---|---|---|---|---|---|---|
    | mydevboard | before | 74 | 18 | 0 | 34 | 37 | 101 |
    | mydevboard | mp1 / strict | 0 / 0 | 0 / 0 | 0 / 0 | 56 / 55 | 62 / 58 | 7 / 6 |
    | slice | before | 16 | 0 | 0 | 12 | 4 | 20 |
    | slice | mp1 / strict | 0 / 0 | 2 / 2 | 0 / 0 | 11 / 9 | 6 / 4 | 9 / 9 |
    | laser-backplane | before | 10 | 0 | 0 | 70 | 45 | 44 |
    | laser-backplane | mp1 / strict | 0 / 0 | 0 / 0 | 0 / 0 | 70 / 70 | 45 / 45 | 33 / 33 |
    | vfo | before | 72 | 4 | 0 | 14 | 1 | 85 |
    | vfo | mp1 / strict | 0 / 0 | 0 / 0 | 0 / 0 | 14 / 14 | 1 / 1 | 15 / 15 |
    | d3c-0496 (information) | before | 107 (track 36, annular 71) | 43 | 77 | 16 | 21 | 274 |
    | d3c-0496 (information) | mp1 / strict | 0 / 0 | 24 / 57 | 0 / 0 | 23 / 17 | 26 / 18 | 59 / 107 |
    | flight-computer | mp1 / strict | 0 / 0 | 0 / 0 | 0 / 0 | 6 / 6 | 4 / 4 | 6 / 6 |
    | canary-pwr-9511d9a | mp1 / strict | 0 / 0 | 0 / 0 | 0 / 0 | 46 / 33 | 110 / 92 | 0 / 0 |

    The five fixture rows: q11m `below_minimum` 0 and `via_in_pad` 0 in both arms; unrouted
    `mp1` → `strict`: Issue433 2 → 2, Issue690 11 → 11, bm10 2 → 0, Issue093 19 → 19, Issue555
    2 → 1. `via_in_pad` 0 and `pour_lost_pins` 0 everywhere.

    Pair A (the floors' cost, reported) on its four rows: below_minimum 172 → 0, edge 22 → 2
    (slice's Margin layer, Q18.5), hole 0 → 0; KiCad unconnected 130 → 151 (+21: mydevboard +22,
    slice −1). Information row d3c-0496: below 107 → 0, hole 77 → 0, edge 43 → 24, unconnected
    16 → 23. Pair B
    (`freerouting-dev gate --records rec-all.json --baseline base-q18.json --prereg Q18.json
    --eligible --arms mp1,strict`, `target/orchestrator/q18/gate-mp1-strict.txt`): no hard
    breach, guards not worse, total 239 → 220 (−19; kicad_unconnected 203 → 187, unrouted 36 →
    33), set noise 27.62: **totals verdict: reject**. Gate of the default path: small parity
    (`PARITY_SET=tools/bench/sets/connection-parity.txt`) 7/7 exact. Remains: edge 0 on slice
    and d3c-0496 after Q18.5's remainder.
  - *Done 2026-10-20:* the remainder, after Q18.5's and Q18.7's keepouts (same build and scripts
    as Q18.5's note, `target/orchestrator/q18b/out`, KiCad 10.0.7; routing-caused counts;
    `smb` = `solder_mask_bridge`; below as above):

    | row | arm | below | edge | hole | smb | KiCad unconnected | unrouted | drc_real |
    |---|---|---|---|---|---|---|---|---|
    | slice | mp1 / strict (Q18.6) | 0 / 0 | 2 / 2 | 0 / 0 | – | 11 / 9 | 6 / 4 | 9 / 9 |
    | slice | mp1 / strict (edge keepouts) | 0 / 0 | 0 / 0 | 0 / 0 | 2 / 0 | 11 / 6 | 7 / 2 | 8 / 4 |
    | slice | mp1 / strict (+ mask keepouts) | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 10 / 6 | 7 / 2 | 5 / 4 |
    | mydevboard | mp1 / strict | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 56 / 55 | 62 / 58 | 7 / 6 |
    | vfo | mp1 = strict (one session) | 0 | 0 | 0 | 0 | 14 | 1 | 15 |
    | pocketsdr-v3-0 (Q18.7, mask flag) | mp1 / strict | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 54 / 76 | 25 / 44 | 472 / 534 |
    | d3c-0496 (information) | mp1 / strict | 1 / 1 | 0 / 0 | 0 / 0 | 13 / 12 | 28 / 24 | 31 / 26 | 43 / 43 |
    | d3c-0496 (information, mask flag) | mp1 / strict | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 31 / 24 | 35 / 25 | 28 / 21 |

    Hard guards 0 in both arms on every re-measured row; the rows whose input did not change
    (Q18.5's note) keep Q18.6's 0. slice's `mp1` smb 2 were Track /BR2 against its two filled
    F.Mask `gr_poly` (exposed motor-driver zones), gone with Q18.7's board mask keepouts.
    pocketsdr-v3-0's `strict` hit the 120 s timeout (exit 124; the session written by then was
    graded), and its 446 / 469 `clearance` items are the board's 0.127 mm minimum against its
    0.09 mm class (Q19's grader area, not a hard guard here). slice's
    second-pass cost: unconnected 11 → 10 (`mp1`), 9 → 6 (`strict`). Small parity
    (`PARITY_SET=tools/bench/sets/connection-parity.txt`): `autoroute_board_parity ... ok`.
- [x] **Q18.7 — Solder-mask bridges of tracks over foreign pads** (d3c-0496, information only
  since amendment 2: KiCad `solder_mask_bridge` Pad–Track 7; Q18.6: `before` 17, `mp1` 11,
  `strict` 22). Not cheap once Q18.4-Q18.5 are in: the router has no
  solder-mask geometry (a DSN carries no mask layers or expansion), so it needs the mask openings
  from the plugin or the `.kicad_pcb` reader as keepouts per copper layer.
  - **Accept:** a test where a track would cross a foreign pad's mask opening and does not;
    0 routing-caused `solder_mask_bridge` Pad–Track on a KiCad 6+ board that shows them.
  - *Done 2026-10-20:* count first: routing-caused `solder_mask_bridge` Track–Pad / Via–Pad
    between different nets in the Q18 grades of KiCad 6+ boards 0; in every saved KiCad 6+ grade
    6 (pocketsdr-v3-0 in q07's oracle-java, inside-out and conflict-rounds grades, 2 each: TP9
    /CLKOUT, B.Mask expansion 0.1016 mm over a 0.09 mm class clearance), so implemented.
    Pads: the expansion (pad `solder_mask_margin`, else footprint, else board
    `pad_to_mask_clearance`, per side with a mask layer) of every pad whose expansion exceeds the
    smallest class clearance goes to the router as the new `router.solder_mask_expansions`
    (`<component>:<pin>=<front um>,<back um>;…`); `job::solder_mask` gives each such pin a
    clearance class copy raised to the expansion on the top / bottom layer, so other nets keep
    max(clearance, expansion). Filled by the plugin (`kicad_board.mask_expansions`,
    `export_flow.solder_mask_args`, always on) and the `.kicad_pcb` reader (`mask.rs`,
    `Extras::solder_mask`, `job::prepare::solder_mask_patch`). Board-level F.Mask / B.Mask
    shapes (apertures no pad owns) become keepouts named `solder_mask` on their side's copper
    (plugin `edge_graphics.mask_polygons`, reader `edge_keepouts::mask_polygons`; KiCad 10's
    `IsAnyFill` for the removed `IsFilled`). Tests: `crates/freerouting/tests/solder_mask.rs`
    (fixture `q18-mask-bridge.dsn` / `.kicad_pcb`, a 0.2 mm trace past a foreign test point:
    `without_the_expansion_the_trace_passes_inside_the_mask_aperture` gap 0.2 mm,
    `a_front_expansion_keeps_other_nets_out_of_the_aperture` gap ≥ 0.4999 mm,
    `a_back_expansion_leaves_the_front_alone`, `an_expansion_within_the_clearance_changes_nothing`,
    `a_kicad_pcb_input_carries_the_pads_mask_expansions`, plus parse / warning tests);
    `tests/edge_keepouts.rs::the_boards_mask_apertures_become_keepouts_on_their_copper_side`;
    plugin `test_solder_mask.py` (5) and `test_edge_graphics.py`'s mask tests (7). DIVERGENCES
    lines in the freerouting, freerouting-kicad and kicad-plugin sections. Measured (Q18.6's
    table, routing-caused): d3c-0496 smb (Pad–Track 7, Pad–Via 1, the rest Pad–Pad of its
    27 U2 pads) 13 / 12 → **0 / 0** with the plugin's flag (27 pads, 0.2 mm on F.Mask), cost
    unconnected 28 → 31 (`mp1`), 24 → 24 (`strict`); slice (KiCad 6+) Track–mask-polygon smb 2
    → **0** (`mp1`); pocketsdr-v3-0 (KiCad 6+, 9 test points) 0 / 0 with the flag. Not shown:
    pocketsdr's own bridges no longer occur under the current router even without the flag
    (`nomask` arm: smb 0, unconnected 46), so on a KiCad 6+ board the Pad–Track clause rests on
    the d3c-0496 run (a KiCad 4 board) and the fixture test.
- [x] **Q18.8 — Strict's remainder.** Once the floors are in, anything strict still breaks.
  - **Accept:** strict's hard metrics and drc_real against `mp1` per row from Q18.6, each
    difference explained or fixed with a test.
  - *Done 2026-10-20:* from Q18.6's table: on Q11's 11 eligible rows strict's hard metrics equal
    `mp1`'s (0, except slice's edge 2 in both arms, the Margin layer of Q18.5) and drc_real is
    equal or lower (mydevboard 7 → 6). Q11.6's breaches (bm10 `below_minimum` 4, vfo 5 and
    drc_real 85 → 93) are gone: they were the pin neck-down under the board minimum (Q18.1),
    which the floors remove. On the information row d3c-0496: strict's edge 57 vs 24 and
    drc_real 107 vs 59 are the same missing edges (it routes 8 more connections, along the
    hidden Edge.Cuts segments) plus `solder_mask_bridge` 22 vs 11 (Q18.7); no strict-specific
    cause.
- [x] **Q18.9 — Promotion** (only if Q18.6's pair B passes). Strict into `enhanced` through N05:
  ENHANCED entry, DIVERGENCES "Profile `enhanced`" line, settings test, N05.3 entry.
  - **Accept:** the four pieces, the gate line quoted.
  - *Done 2026-10-20:* not promoted, the registration's fail row: "FAILS: the improvement 19
    does not exceed the set noise 27.62 / totals verdict: reject" (Q18.6). Strict stays a
    setting, off by default and in no profile (decided 2026-10-20: kept, not removed).
