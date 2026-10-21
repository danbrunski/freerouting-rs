# K15 — Findings from the power-control-unit board

Opened 2026-10-21 from the owner's report
[`docs/development/pcu-rev2-board-report.md`](../../development/pcu-rev2-board-report.md) (commit
`2d38fdd`): a real board routed end to end through the `.kicad_pcb` input. The board is 180 x 120 mm,
4 layers, 500 footprints, 393 nets, KiCad 10.0.6, with a `.kicad_pro` (seven net classes) and a
`.kicad_dru` (a standard fab-minimum rule file). The report lists five bugs and some usability
points. This checklist fixes them in the order they cost the owner: one bug stopped every
connection, two broke the board's manufacturing minimums, one inflated the router's violations.

Prerequisites: K07 (the reader and its `.kicad_dru` subset), K08 (the `.kicad_pcb` writer), Q22 (the
item-type rules), N06 (`router.java_quirks`).

## What the work starts from

- **The board.** `../power-control-unit` (`hardware/rev2.0/Power_Control_Unit_24V.*`, read-only). The
  owner's repository, not a corpus board. Copy the files with `git -C ../power-control-unit archive`
  into `target/` for runs. Small cases cut from it go in `fixtures/` or a crate's `tests/data/`. The
  two repro boards of bug 1 are already in `docs/development/pcu-rev2/`.
- **Copies, logs and the binary** (2026-10-21).
  - The report's routing runs used `8c952e7` (2026-10-19). That was before Q18 (`b98074e`) made
    Board Setup's minimums router floors, and before pad clearances were read (`5926b61`).
  - Copies are in `target/orchestrator/pcu-rev2/` (gitignored), because the originals were in a
    session's temporary scratchpad:
    - `boards/placed_v7/`: the v7 input, 0 tracks; `.kicad_pcb` sha256 `948a4804…`;
    - `boards/final_v7/`: v7's output, `1e16823a…`;
    - `boards/committed-1b161c7/`: the committed board, `af63f202…`, with its `.kicad_pro` and
      `.kicad_dru`;
    - `logs/route_<run>.err.gz`: the `--progress json` logs of v4, v5, v7, v7c, v7c2, v8, v8n,
      v9, v9n, v9nc, hc and test;
    - `logs/route_<run>.json.gz`: their result manifests, with the settings.
- **Bug 1's cause** (orchestrator investigation `pcu1`, 2026-10-21, on main `383de4a`). The pads are
  not the cause. C65's F.Fab `fp_circle` (r 5 mm) is written as a polyline outline. The board builder
  enlarges it to a convex simplex (`polygon_path_to_board`, `Simplex::enlarge`). Then
  `PlacementTransform::shape` (`freerouting-geometry/src/placement_transform.rs`) adds the
  footprint's position in floating point, rounds each corner and calls
  `TileShape::get_instance_from_points`, which assumes a convex polygon. At y = 105.242039 mm
  (1052420.39 units) two 45° chamfer edges round to 1-unit steps, giving two reflex vertices.
  `Simplex::get_instance` keeps three parallel same-direction lines, and the box comes out with
  `ur = (i32::MAX, i32::MAX)`. Then `BasicBoard::expand_bounding_box_to_include_all_items` takes
  the union and `IntBox::offset(1000)` wraps (`wrapping_add`), so the board box is empty. Every room
  completion starts from the board box (`complete_shape.rs`), so every maze search on the board
  fails at `session/init.rs` ("the maze search algorithm could not be created").
  - The same happens on KiCad's own DSN export of the board, in both profiles.
  - Java at the pin has the same code (`PlacementTransform.java:54-68`, `IntBox.offset`). It aborts
    with `NegativeArraySizeException` in `DrillPageArray` during fanout.
  - Rust clamps the drill-page counts (`drills.rs`, `.max(0)`), so it runs on and fails every
    connection. DIVERGENCES does not list this.
- **Bug 3's cause.** `dru.rs` (`rule_status`) reports a rule without a condition as UNSUPPORTED,
  and so is every constraint kind other than `clearance`, `track_width`, `hole_clearance` and
  `hole_to_hole`. The pcu file's 12 rules are mostly unconditional board-wide minimums:
  - track width, clearance, hole size, hole clearance;
  - edge clearance (`(layer outer)` 0.254 mm, `(layer inner)` 0.381 mm);
  - annular ring (`A.Type == 'Via'`; plated pads over 0.4 mm drill);
  - disallow blind, buried and micro vias;
  - a `hole_size (max 999mm)` plated-slot rule.
- **The router already has the targets:**
  - `router.board_minimums.{track_width_um,via_diameter_um,via_drill_um,via_annular_um,hole_to_hole_um}`;
  - `router.copper_to_edge_clearance_um`, `router.hole_clearance_um`;
  - the class-pair clearance rules.
  Board Setup's minimums already reach them on a `.kicad_pcb` input (`project.rs`, `job::prepare::board_minimums_patch`).

## Checklist

- [x] **K15.1 — One item with an unbounded box no longer empties the board's bounding box.**
  - **Change:**
    - `expand_bounding_box_to_include_all_items` leaves out an item box that is not bounded (a
      coordinate at `i32::MAX` or `i32::MIN`), and `IntBox::offset` saturates instead of wrapping.
    - A WARNING names the item: the component's reference and its outline, or the pad, with its
      position.
    - The doc of `freerouting-kicad/src/image_outlines.rs` stops saying outlines "do not influence
      routing": they reach the board box.
    - The drill-page clamp (`drills.rs`) gets its DIVERGENCES line: Java aborts on such a board.
  - **Accept:**
    - `docs/development/pcu-rev2/c65-nm-position.kicad_pcb` routes 2 of 2 under both profiles, with 0
      "could not be created" lines and the WARNING. `c65-snapped.kicad_pcb` is unchanged.
    - A synthetic board with nets far from C65 routes them all (the investigation's `far-nm`: 6 of 6).
    - Its KiCad DSN export routes the same way, from a committed `.dsn` fixture.
    - Unit tests: `IntBox::offset` at the limits; the board box with one unbounded item.
    - A DIVERGENCES line (Java aborts with `NegativeArraySizeException`), and connection-parity 7
      of 7 exact.
  - *Done 2026-10-21:* `IntBox::offset` saturates and `IntBox::has_infinite_coordinate` tells a
    box with a coordinate at `i32::MIN`/`MAX`; `expand_bounding_box_to_include_all_items` skips
    such an item and returns one warning per item (`board::unbounded_items`), which the job logs
    at WARN (`job::prepare`, both paths; the DRC runner adds it to the read warnings). The CLI
    prints `warning: The fabrication outline of component C65 at (57251.006, -105242.039) um has
    an unbounded shape after placement; it is left out of the board's bounding box.`
    - RED at main: `cli_unbounded_outline` 3 of 4 failed (the "could not be created" assert),
      `int_box_limits` 2 of 5 failed with the wrapping offset. GREEN: all pass.
    - c65-nm-position routes 2 of 2 in `legacy` and `enhanced`, 0 "could not be created" lines,
      the warning once: `cli_unbounded_outline::the_c65_repro_routes_in_both_profiles`.
      c65-snapped: `the_snapped_repro_routes_as_before` (2 of 2, no warning), and its
      `.kicad_pcb` and `.ses` outputs (from the `.kicad_pcb` and from KiCad's DSN, both profiles)
      are byte-identical before and after the change.
    - far-nm (`crates/freerouting/tests/data/k15/far-nm.kicad_pcb`, made by
      `python3 -I tools/kicad-reader/k15_far_fixture.py`) routes 6 of 6 in both profiles:
      `nets_far_from_c65_route`.
    - KiCad 10.0.7's DSN export (`tests/data/k15/{c65-nm-position,far-nm,c65-snapped}.kicad10.dsn`)
      routes 2 of 2 and 6 of 6 with the warning, snapped 2 of 2 without:
      `the_kicad_dsn_exports_route_the_same_way`.
    - Unit tests: `freerouting-geometry/tests/int_box_limits.rs` (5: offset at `i32::MAX` and
      `i32::MIN`, inside the limits, `has_infinite_coordinate`, an unbounded simplex's box);
      `freerouting/tests/unbounded_item_box.rs` (3: the outline left out and named and every
      other item inside the box; the snapped board's box unchanged; the far board);
      `drills.rs::an_empty_board_box_gives_no_pages`.
    - DIVERGENCES: geometry (`IntBox::offset`), board model (the left-out item; Java aborts with
      `NegativeArraySizeException`), maze search (the drill-page clamp, no longer reached by a read
      board). `PARITY_SET=tools/bench/sets/connection-parity.txt tools/parity-check.sh
      --no-probes`: 7 of 7 exact, PARITY OK (37 s).
    - Full-board check (copy of the pcu board with C65 at y = 105.242039, release, `--max-passes 2
      --timeout 240` and the report's flags except `plane_obstacle_net_classes`, whose classes the
      copy lacks): board box (428000, -1521000)-(2232375, -317000), the same as with C65 snapped;
      1 unbounded item of 6819 (C65's outline); 0 "could not be created"; 20 unrouted, 663
      violations, score 953.78 in 26.6 s, the same as the snapped copy (before: 128 unrouted,
      2790 "could not be created").

- [ ] **K15.2 — A placed outline or pad stays convex after rounding.**
  - **Change:** `PlacementTransform::shape` gives the convex hull of the rounded corners, so no
    shape built from a placed polygon has reflex vertices. Pads go through the same function, and
    Java cuts such a pad along the reflex edge, so the fix sits behind
    `router.java_quirks.placed_shape_convex` (unset or `true` is Java's), marked
    `// PARITY(quirk, java_quirks.placed_shape_convex):`. It joins `ENHANCED` only after a
    preregistered measurement; that measurement is not part of this item.
  - **Accept:**
    - with the quirk off, C65's outline at y = 105.242039 gives a bounded simplex, and a pad at a
      position that rounds to a reflex vertex keeps its convex hull (unit tests in
      `freerouting-geometry`);
    - with the quirk unset, both are Java's (the K15.1 board still routes, through K15.1's fix);
    - the settings test, the help text, a DIVERGENCES line, and connection-parity 7 of 7.

- [x] **K15.3 — Board-wide `.kicad_dru` rules apply.**
  - **Change:**
    - A rule with no condition applies to every class pair, layer and item it can.
    - A constraint kind the router has a setting for applies as that setting, never lowering a
      value Board Setup or another rule set:
      - `track_width` → `router.board_minimums.track_width_um`;
      - `clearance` → every class pair;
      - `hole_clearance` → `router.hole_clearance_um`;
      - `hole_size` (min) → `router.board_minimums.via_drill_um`;
      - `annular_width` → `router.board_minimums.via_annular_um` for vias;
      - `edge_clearance` → `router.copper_to_edge_clearance_um` (the largest of `outer` and
        `inner` if the router keeps one value; say so).
    - `disallow blind_via|buried_via|micro_via` is met when the board offers only through vias;
      otherwise it removes those via kinds from the router's choices. A rule the router cannot use
      (the plated-slot `hole_size (max ...)`, a pad annular ring) is reported by name with why it is
      left out.
    - The same on both inputs (the reader and `--kicad-dru` on a DSN).
  - **Found 2026-10-21** (`-drc` at `084f3a9` on the committed board). The file has 12 rules:
    - 9 are UNSUPPORTED as "rule without a condition": track width, clearance, hole size, hole
      clearance, edge clearance outer and inner, and the three `disallow` rules;
    - line 16 (via annular ring, `A.Type == 'Via'`): "constraint 'annular_width' has no DSN form";
    - line 21 (pad annular ring): "condition term 'A.Hole_Size_X > 0.4mm'";
    - line 59 (plated slots): "condition term 'A.Hole_Size_X != A.Hole_Size_Y'".

    A min-only `track_width` is refused even with a condition ("track_width without an opt
    value"). On this board, Board Setup is stricter than the file except for hole clearance (0.25
    against 0.254 mm).
  - **Accept:**
    - dru tests on a copy of the pcu `.kicad_dru` (copied into `crates/freerouting-kicad/tests/data/`
      with its source named): every rule is APPLIED or reported with its reason, and none says
      "rule without a condition";
    - each mapped setting has the rule's value on a small board, on both inputs;
    - a rule below Board Setup's value leaves Board Setup's;
    - module docs and a DIVERGENCES line if KiCad's semantics are narrowed (one edge clearance for
      all layers).
  - *Done 2026-10-21:* `dru::floors` (new) and `dru.rs`: a rule with no condition is no longer
    UNSUPPORTED; `track_width`, `clearance` (no condition), `hole_size`, `annular_width` and
    `edge_clearance` `min`s are `DruReport::floors` (`BoardFloors`); `job::prepare::raise_board_floors`
    raises `router.board_minimums.{track_width,clearance,via_drill,via_annular}_um` and
    `router.copper_to_edge_clearance_um` after every merge, over the command line, the larger
    winning, on both inputs (the reader's `JobInput::set_kicad`, and `--kicad-dru` on a DSN).
    `disallow blind_via|buried_via|micro_via` is "met" or removes those padstacks (`floors::disallow`).
    KiCad's semantics of a rule without a condition and the narrowing (one edge clearance for all
    layers, the largest, with a warning; a `min` limited to some net classes is never a board-wide
    floor: `track_width` raises those classes' width rules, the other kinds are UNSUPPORTED) are in
    `dru/floors.rs`'s module docs and a DIVERGENCES line.
    - Accept 1: `tests/data/pcu-rev2.kicad_dru` (commit 1b161c7, source named in its header);
      `dru_floors.rs::every_rule_of_the_pcu_file_is_applied_or_reported_with_its_reason`: 12
      constraints; on the two-layer test board 6 applied, `inner` ignored (no inner layer), the three
      `disallow` rules ignored as "met", the two pad-hole rules UNSUPPORTED with the term and "the
      router places no pads"; none says "rule without a condition"; the DSN and the `.kicad_pcb`
      agree. On the real four-layer board (`freerouting-rs -de Power_Control_Unit_24V.kicad_pcb -do
      ...`, log): 7 applied, 3 met, 2 UNSUPPORTED (line 21, line 59).
    - Accept 2: `each_mapped_setting_has_the_rules_value_on_both_inputs` (track 250, clearance 300,
      via drill 400, annular 150, edge 600, hole clearance 300 um, from a DSN job and a `.kicad_pcb`
      job), `the_floors_are_the_rules_values` (pcu values 127/127/254/127 um, edge 381 um on six
      layers), `disallow_removes_the_via_kinds_a_board_offers` (vias.kicad_pcb: only `Via[0-5]` left).
    - Accept 3: `a_rule_below_board_setup_leaves_board_setups_value` (DSN job with the command line's
      200/150/300/150/500/300 um keeps them, log "is 200, no less than ..."; `.kicad_pcb` job keeps
      Board Setup's track 200, drill 300, edge 500 um and raises annular 100 to 127 and hole clearance
      250 to 254). The real board's log: hole clearance 250 -> 254, clearance floor 0 -> 127, annular
      100 -> 127; track, drill, edge unchanged.
    - Accept 4: DIVERGENCES "Board-wide `.kicad_dru` rules without a condition apply (K15.3)".
    - Class-limited `min` (review 2026-10-21): `a_class_limited_track_width_min_raises_only_that_class_on_a_dsn`
      (q225: `Power` 300 -> 500 um, `kicad_default` 200 and the command line's
      `board_minimums.track_width_um` unchanged; Default selected: its and the structure's width
      rise; below the class's width: ignored), `..._on_a_kicad_pcb` (`hasNetclass('Fast')`: the two
      100 um classes -> 150, 250 and 600 unchanged, Board Setup's 200 stays),
      `a_class_limited_hole_size_min_is_unsupported` (`hole_size`, `annular_width`,
      `edge_clearance`, both inputs: UNSUPPORTED "limited to some net classes; the router keeps one
      board-wide value", no floor).
    - Also: `crates/freerouting-kicad/tests/dru.rs::conditions_choose_class_pairs` updated (a rule
      without a condition and `hole_size` are no longer UNSUPPORTED; `length` stands in).
    - Gate: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
      `cargo test -p freerouting-kicad` and `-p freerouting-cli` green, `-p freerouting` lib (309) and
      the KiCad, board-minimum and job tests green;
      `PARITY_SET=tools/bench/sets/connection-parity.txt tools/parity-check.sh --no-probes`: 7 exact.
    - Also: the `--kicad-dru` paragraph of `crates/freerouting-cli/src/help.txt` lists the kinds now
      applied. The plugin's `edge_and_pad_keepouts` is unchanged (decided: the DIVERGENCES note covers it).

- [ ] **K15.4 — No track below the board's minimum on the pcu board.**
  - **Change:** the report's v5 output had 495 segments under the 0.2 mm Default width: 0.15 mm
    (476), 0.12 (11), 0.10 (5). KiCad flagged 16 below 0.127 mm, the `.kicad_dru` minimum that bug
    3 lost. With K15.3, no routed track goes below the largest of Board Setup's minimum and the
    `.kicad_dru`'s. Find where each width under the class width comes from (neck-down at a pad,
    the optimizer, existing copper) and where the odd widths come from (0.2498, 0.2502, 0.2538,
    0.2792, 0.3554 mm). Neck-down below the class width but at or above the minimum is allowed;
    say where the board's own copper had such widths before the route.
  - **Found 2026-10-21** (code reading, and `--dump-settings` at `084f3a9`):
    - The report's runs used `8c952e7`, before Q18 (`b98074e`) made Board Setup's minimums
      router floors. v5 and v7 had no `min_trace_width_um`, so nothing set the floor.
    - 0.15, 0.12 and 0.10 mm are exactly the micro neck-down's candidates on a 0.2 mm class
      (`autoroute/path/insert.rs`, `micro_neckdown_half_widths`): 3/4, 3/5 and 1/2 of the class
      width. A candidate below `AutorouteControl::min_trace_half_width` is skipped.
    - On main, `--dump-settings` on the committed board shows
      `board_minimums.track_width_um = 200.0 (kicad_pro board minimums)`, so all three are
      skipped. No route has confirmed it yet.
    - The DIVERGENCES `board_minimums` entry credits the 0.15 mm tracks to "the pin neck-down";
      they are the micro neck-down's.
    - 0.2498, 0.2538, 0.2792 and 0.3554 mm are pad widths of 0.25, 0.254, 0.2794 and 0.3556 mm
      less 2 board units. That is Java's pin neck-down, `board/pin_exits.rs::trace_neckdown_half_width`
      (`max(0.5·pin_min_width − 1, 1)`), on nets whose class is wider than the pad. 0.2502 mm fits
      the same formula; its pad is not identified.
  - **Accept:**
    - a run on a copy of the pcu board, a few minutes at most (`--max-passes` and `--timeout` to
      fit, report flags), graded by KiCad under the board's own rules: 0 routing-caused
      `track_width`, and 0 below-minimum in the grader's summary;
    - a test that pins the minimum on a small board with an unconditional `track_width` rule;
    - each odd width named with its source.

- [ ] **K15.5 — The DRC report and the router's violations count what KiCad counts.**
  - **Change:**
    - `-drc` against KiCad 10 on the committed pcu board (`--router.hole_clearance_um=260`):
      | Type | freerouting-rs | KiCad |
      |---|---|---|
      | holeClearance | 165 (121 same net) | 8 |
      | via_dangling | 117 | 0 |
      | track_dangling | 16 | 2 |
      | clearance | 4 | 0 |
    - Hole clearance applies between different nets only, as KiCad's does.
    - A via that joins a pad on one layer to its net's plane on another is connected, not dangling.
    - Name the 16 `track_dangling` and 4 `clearance` items and fix or explain each.
    - Where the router's own violation count (its score) uses the same checks, and Java counts
      them, the fix sits behind a `router.java_quirks.<id>` (legacy keeps Java's count). Where
      only the report uses them, it changes under both profiles.
  - **Found 2026-10-21:**
    - `-drc` ignores router settings: `DrcRunOptions` carries only the design, session, rules
      file and unit. The table's `--router.hole_clearance_um=260` had no effect.
    - Without it, at `084f3a9`, the counts are holeClearance 165 (121 same net),
      via_dangling 117, track_dangling 15, clearance 4, isolated_island_dead_copper 20,
      unconnected 77, in 0.28 s.
    - **Same net.** The reader writes `(attach off)` on every padstack (`pads.rs`, `read.rs`),
      so `BasicBoard::is_obstacle`'s via branch makes a same-net via inside an SMD pin an
      obstacle. The search uses the same test, and changing it would let the router put vias in
      pads (owner rule, D8). So the fix belongs in the report and the score, not in
      `is_obstacle`. Q20's `hole_clearance` (`drc/board_rules.rs`) already skips same-net pairs.
    - **Dangling vias.** `is_tail` counts contacts by Java's centre rule: a pin counts only when
      its centre is the via's. Q16's stub cleanup counts copper overlap
      (`quality/stub_contacts.rs::via_contact_layers`).
    - **The score.** `autoroute/batch/snapshot.rs` reads `clearance_violations` from the same
      list. The 142 -> 217 rise is `job/overrides.rs::apply_hole_clearance` growing the drill
      items' tree shapes.
  - **Accept:**
    - on the pcu board, each of the four types equals KiCad's or differs by named items;
    - tests per case (same-net hole clearance, a via to a plane, a stub);
    - connection-parity 7 of 7 if engine code `legacy` runs changes; DIVERGENCES lines.

- [ ] **K15.6 — The run stops when nothing routes.**
  - **Change:** with bug 1 present, the zoned pcu board ran pass after pass at `routed 0`, `failed
    1484`, score 71.03, until its 40-minute timeout. The same placement without zones stopped
    after 18 passes. The investigation could not reproduce it on main `383de4a`: it stopped at pass
    18. Its copy lacked the report's `Pour1A`/`Pour3A` classes and `plane_obstacle_net_classes`.
    Reproduce it on the zoned variant, with the report's flags and the bad C65, on main before
    K15.1, a few minutes at most. If it doesn't stop, make the stop rule end a run whose passes
    route nothing and leave the score unchanged (`autoroute/batch/control.rs`).
  - **Found 2026-10-21** in the report's own logs (`target/orchestrator/pcu-rev2/logs/`):
    - `route_v4` is the zoned board with the bad C65: 20 planes, 151 vias, 1484 failures a
      pass, `job_timeout` 40 min, without `plane_obstacle_net_classes`, on `8c952e7`. It stopped
      itself after 18 passes of about 1 s each, with "has not improved enough" (score 71.03).
    - The report's 40-minute timeout is `route_v5`, which routed normally: 8 passes of
      160–319 s, 185 unrouted.
    - Every timeout in the logs (v5, v7, v8, v8n, v9, v9n) had passes of 80–338 s, so the
      18-pass stop came after the limit.
    - What remains is the same board with `plane_obstacle_net_classes` on main.
  - **Accept:**
    - the reproduction with its pass log, or the note that it stops, with the pass count;
    - a test of the stop on a board where nothing can route, if the rule changes;
    - parity 7 of 7 if `legacy`'s stop rule changes. Java's rule is kept for `legacy` behind a quirk
      if they differ.

- [ ] **K15.7 — Failure messages and the edge-clearance cap name board items.**
  - **Change:**
    - "Failed to route connection between pin #9 of component #385" names the reference designator
      and the pad number (`U12 pad 9`), in the log and in the result JSON.
    - "Copper-to-edge clearance capped from 2500 to 0 board units because a pin is closer to the
      board outline" names the pin. A footprint wholly outside the outline (a fiducial left at
      (0, 0)) is left out of the cap, with a WARNING naming it.
  - **Found 2026-10-21:**
    - The text comes from `autoroute/connection.rs` (`describe_connection`, `java_item_text`).
    - Pin index 0 prints "pin of component #2" with no number. The component number follows the
      DSN placement order, which the reader groups by image.
    - Each side lists every item of the connected set. In `route_v4` and `route_v7`, 9310 of
      27330 failure lines are over 1000 characters and the longest is 10416 ("via, via, ...").
    - `autoroute/batch/report.rs::describe_item` already gives `Ref-Pad`.
    - The edge cap (`job/overrides.rs::apply_edge_clearance`) logs at Debug, and only when the
      setting is exactly the 250 µm default. On main a `.kicad_pcb` input takes the value from the
      `.kicad_pro` or KiCad's 500 µm, so the cap now shows mostly on DSN inputs.
      `BasicBoard::minimum_pin_gap` drops the item ids, and a pin outside the outline gives 0.
  - **Accept:**
    - tests on small boards for both messages;
    - the pcu board's first failure lines quoted before and after;
    - Java's text stays in any output that parity compares (check `ses-diff` and the parity
      check, 7 of 7).

- [ ] **K15.8 — The CLI's standard input and `--threads` are documented.**
  - **Change:**
    - The CLI reads stop requests from standard input, so in a shell `while read` loop it eats the
      loop's input. Add a flag that ignores standard input (or reads it only from a terminal), and
      a sentence in `--help`.
    - `--threads` help says which stages use the threads: the report saw the autorouter on one
      core and only the optimizer using 24.
  - **Found 2026-10-21:**
    - `crates/freerouting-cli/src/stop.rs::spawn_stdin_reader` starts for every route
      (`cli.rs::route_job`). The plugin relies on it (`tools.py`: `stdin=PIPE`, writes
      `stop`), so it stays on by default.
    - `autorouter.max_threads` only sizes the analyses' `ThreadBudget` (`job/run.rs`: DRC and
      statistics). `parallel.rs` says the router stays sequential (D5).
    - The DIVERGENCES `--threads` line is already right; `help.txt` isn't.
  - **Accept:** a CLI test with stdin closed and with a pipe; the help text; the CLI docs.

- [x] **K15.9 — A pad's own clearance is kept on a `.kicad_pcb` input.**
  - **Change:** the report saw a fiducial's 0.6 mm local clearance broken by a GND trace and called
    pad-local clearances "not read". The reader writes pads' own clearances as keepouts since
    K13.7 (`pad_clearance::read_pad_clearances`, `read.rs`). Find which clearance the fiducial
    carries (pad, footprint, or the solder-mask opening) and whether the reader at main keeps it.
    Fix what is missing.
  - **Found 2026-10-21:**
    - FID1–FID3 carry `(clearance 0.6)` on their pads (`F.Cu`, `F.Mask`), at (52, 33),
      (194.48, 99.96) and (80, 147.5) mm.
    - The reader has read pad clearances since `5926b61` (after the report's binary).
    - A footprint-level `(clearance)` is not read (`pad_clearance.rs` module doc).
  - **Accept:**
    - the pcu fiducial cut into a small fixture: the keepout is there;
    - a route keeps 0.6 mm from it;
    - the doc line the report read says what is kept.
  - *Done 2026-10-21:* the reader at main already read a pad's own `(clearance)` (the fiducials'
    pads carry `(clearance 0.6)`, `(solder_mask_margin 0.5)`; the footprint has none, and the
    solder-mask margin is not a clearance). On the real board `read` gives three `pad_clearance`
    circles on F.Cu, 1.9 mm across (1 + 2 * (0.6 - the Default class's 0.15)), at (52, 33),
    (194.48, 99.96) and (80, 147.5) mm. What was missing: a footprint-level `(clearance)`.
    `pad_clearance::read_pad_clearances` now gives a pad with no positive clearance of its own the
    footprint's (KiCad 10.0.7 `PAD.GetLocalClearance` on a three-pad footprint: pad 1 and pad 3
    `(clearance 0)` inherit 0.6, pad 2 keeps 0.3).
    - Accept 1: `tests/data/k15-fiducial.kicad_pcb` (FID1's footprint verbatim from commit
      1b161c7 at (52, 33) mm, two GND pads across it); `crates/freerouting/tests/k15_pad_clearance.rs::the_fiducials_keepout_is_there`
      and `freerouting-kicad/tests/k15_pad_clearance.rs::the_pcu_fiducial_has_its_circle`
      (1.8 mm circle on F.Cu at the default 0.2 mm), `a_footprint_clearance_reaches_the_pads_without_their_own`,
      `the_pads_own_clearance_wins_over_the_footprints`.
    - Accept 2: `a_route_keeps_the_pads_own_clearance` routes the fixture on `legacy` and `enhanced`:
      the GND trace on F.Cu is 0.652 mm from the pad's edge with the keepout and 0.245 mm without it.
    - Accept 3: `docs/development/kicad-plugin.md` (the line the report read, "pad-local clearances are
      not carried into the DSN model yet") and `pcu-rev2-board-report.md` now say what is kept; the
      `pad_clearance.rs` module docs state the pad-then-footprint rule; DIVERGENCES "A footprint's own
      clearance reaches its pads (K15.9)". The plugin's DSN fix-up reads the pad's own value only
      (kicad-plugin lane, not changed here).

- [ ] **K15.10 — The desktop app routes a `.kicad_pcb` with the board's own minimums.**
  - **Change:** `freerouting-gui/src/session.rs::read_design_timed` reads a `.kicad_pcb` with
    `ReadOptions::default()` (`apply_design_rules` off) and builds its job without
    `JobInput::set_kicad`'s sources. So a desktop-app route of a KiCad board gets neither Board
    Setup's minimums (Q18's floors, `board_minimums_patch`) nor the `.kicad_dru` (K13.1, K15.3),
    which the CLI and the plugin both apply. Found 2026-10-21 by the orchestrator's K15.3 agent, by
    code reading. Read the board as the CLI does (the same `ReadOptions` and the same extra
    sources in the job), so the desktop app and the CLI route the same board with the same settings.
  - **Accept:**
    - a GUI test (the session harness, no window) on a small `.kicad_pcb` with a `.kicad_pro`
      minimum and a `.kicad_dru` floor: the settings the session routes with equal the CLI's
      `--dump-settings` for the same board, values and sources;
    - the GUI's settings dialog shows where those values came from, if it shows provenance;
    - no window opens on the desktop (`xvfb-run` for anything that draws).
