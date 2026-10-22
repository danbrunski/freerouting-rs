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

- [x] **K15.2 — A placed outline or pad stays convex after rounding.**
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
  - *Done 2026-10-21:* `router.java_quirks.placed_shape_convex` (`settings/java_quirks.rs`, owner
    `K15.2`; `settings/fields.rs`, JSON `placedShapeConvex`) reaches the geometry as the
    `java_corners` argument of `PlacementTransform::shape_with` / `area_with` (`shape` / `area` keep
    Java's), as `rational_perp_projection` reaches it: no global. `false` takes
    `PolygonShape::convex_hull` of the rounded corners, marked
    `// PARITY(quirk, java_quirks.placed_shape_convex):` in `placement_transform.rs`. Every caller
    passes `BasicBoard::java_quirks().placed_shape_convex`: pins (`basic_board.rs::pin_drill_data`),
    component areas (`absolute_area`, `item_ops.rs::refresh_derived_data`), component outlines
    (`refresh_derived_data`), auxiliary pad shapes (`io/specctra/parser/network.rs`,
    `interactive/copy.rs`). A switch changed after reading places the component items again and
    reinserts those whose shapes change (`board/placed_shapes.rs`, from `set_java_quirks`). Not in
    `ENHANCED`: both profiles keep Java's shape.
    - Accept 1 (`freerouting-geometry/tests/placed_shape_convex.rs`; RED on a stub that ignored the
      flag: 3 of 3 failed, GREEN 3 of 3):
      `c65s_outline_is_bounded_with_the_switch_off` (the 77-line simplex from
      `tests/data/k15/c65-nm-position.kicad10.dsn` at (572510.06, -1052420.39): unset box ur
      `(i32::MAX, i32::MAX)`, off box (522059, -1102908)-(623010, -1001932), the box of its rounded
      corners, every corner inside, also through `area_with`);
      `a_pad_rounded_to_a_reflex_vertex_keeps_its_hull_with_the_switch_off` (a 24-gon pad r 2500
      enlarged by 75, component at (100000, -50000), pin offset (0, 0.13), found by a search over
      fractional positions: unset cuts 8 rounded corners off, off is the hull, mirrored too);
      `convex_placements_are_the_same_under_both_values` (120 placements: where the rounded corners
      are convex both values give the same shape).
    - Accept 2: unset is Java's in both tests above; K15.1's tests pass unchanged
      (`cli_unbounded_outline` 4 of 4, `unbounded_item_box` 3 of 3). Switch off:
      `tests/k15_placed_shape_convex.rs` (no unbounded item and no warning on the three KiCad DSN
      exports; only C65's outline changes; a board switched after reading has the boxes of one read
      with the switch, and back); `freerouting-cli/tests/cli_placed_shape_convex.rs`:
      `c65-nm-position.kicad_pcb` and the KiCad DSN exports route 2 of 2 / 6 of 6 / 2 of 2 in
      `legacy` and `enhanced` with `--router.java_quirks.placed_shape_convex=false`, 0 "could not be
      created", no "unbounded shape" line.
    - Accept 3: `tests/settings_java_quirks.rs` (42 switches with K15.5's; `placed_shape_convex` owned by K15.2
      and `true` in both profiles; every key round-trips, has its marker);
      `settings_defaults.rs` lists every `java_quirks` field from the table (unchanged, passes); the
      help text is the field's doc, printed by `--dump-settings` (the GUI has no quirk controls,
      `dialogs/autoroute_settings.rs`). DIVERGENCES "K15.2 placed shapes convex (in neither
      profile)" under "Java quirk switches, opt-in", and the K15.1 board-model line points to it.
      `PARITY_SET=tools/bench/sets/connection-parity.txt tools/parity-check.sh --no-probes`: 7 of 7
      exact, PARITY OK (63 s).
    - Also: `quality/neckdown/bounded_neck.rs::courtyard` leaves an outline with an unbounded box out
      (both the courtyard and the all-outlines fallback), unconditionally:
      `bounded_neck::tests::the_courtyard_fallback_leaves_an_unbounded_outline_out` (RED without the
      filter: the fallback's box reached `i32::MAX`). It logs nothing of its own: the stage has no
      log channel, and the job's K15.1 warning already names the item. No route in the test suite
      reaches it: with temporary instrumentation over `cargo test --workspace`, `courtyard` ran 59
      times (42 components without outlines, 14 with a courtyard, 3 from the unit test) and met an
      unbounded outline only in the unit test; C65 has a courtyard, so its routes never use the
      fallback.
    - Gate: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`
      clean; `cargo test --workspace` 2874 passed, 0 failed.

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

- [x] **K15.4 — No track below the board's minimum on the pcu board.**
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
  - *Done 2026-10-21:* no change to routing was needed: on main `41cf105` the micro neck-down's
    candidates under the floor are skipped (`insert_fanout_micro_neckdown`,
    `half_width < self.ctrl.min_trace_half_width`), and the pin neck-downs go through
    `AutorouteControl::neckdown_half_width` (floored the same way). The board:
    `../power-control-unit` at `76163c7`, `hardware/rev2.0` (one net class at this revision,
    Default 0.2 mm; Board Setup minimum 0.2 mm, `.kicad_dru` 0.127 mm), tracks and vias stripped
    (2807 removed, zones kept: `kicad_steps.py strip`), release binary built at `41cf105`, every
    route under `ulimit -v 8000000`, stdin `/dev/null`.
    - Accept 1, the plugin's `.kicad_pcb` path (`{"schema": 6}`, default options, `enhanced`):
      `FREEROUTING_BIN=... ROUTE_TIMEOUT_S=300 tools/kicad-reader/plugin-input-ab.sh kicad_pcb
      stripped/Power_Control_Unit_24V.kicad_pcb opts-default.json run-main-pcb -- --max-passes 2
      --timeout 240` (171 s): 1730 new segments, **all 0.2 mm**, 262 vias; KiCad 10.0.7 under the
      board's own `.kicad_pro`/`.kicad_dru`: 0 `track_width` items (routed and input board),
      `below_minimum` 0, `manufacturing_breaches` 0, routing-caused `track_dangling` 3,
      `via_dangling` 3; 96 unconnected (input 499). The DSN path as well:
      `tools/kicad-oracle/grade.sh --export plugin opts-default.json Power_Control_Unit_24V.kicad_pcb
      -- bash -c "ulimit -v 8000000; exec timeout -k 5 300 freerouting-rs -de {DSN} -do {SES}
      {PLUGIN_ARGS} --max-passes 2 --timeout 240"` (183.6 s): 2272 new segments, all 0.2 mm, 343
      vias, 0 `track_width`, copper `below_minimum` 0, `drc_real` 4 (`via_dangling`), 55
      unconnected. Before (the report's v5 at `8c952e7`): 0.15 mm 476, 0.12 mm 11, 0.10 mm 5
      segments, 16 KiCad `track_width`.
    - Accept 2: `crates/freerouting/tests/k15_track_width_floor.rs`: q18-neck-floor.dsn with a
      `.kicad_dru` holding only `(constraint track_width (min X))`, no condition, under `legacy`
      and `enhanced`: the setting is the rule's, and every routed trace is at or above it (0.127
      and 0.15 mm: routed; 0.2 mm, the class width: the connection stays open); the control
      without a rule necks to 0.1198 mm in `legacy`. Green at main (nothing to fix); it bites:
      with the micro neck-down's floor check removed, `legacy` at 0.2 mm ships a 0.15 mm trace
      (`[1500, 2000, 2000, 2000, 2000]`).
    - Accept 3, the widths under the class width and their sources. On this revision no routed
      track is under 0.2 mm, because the class width is the board minimum. A copy with the
      Default class at 0.36 mm (one pass, `--max-passes 1 --timeout 240`, `.kicad_pcb` input)
      shows every source, each at or above 0.2 mm: `enhanced` 0.216 (12), 0.225 (14), 0.2498 (12),
      0.252, 0.2538 (63), 0.27 (57), 0.2752, 0.279, 0.2792 (7), 0.2964, 0.2998 (11), 0.3 (32),
      0.3004 (112), 0.3046 (7), 0.306, 0.312, 0.3176, 0.324, 0.336, 0.3388, 0.348, 0.3554 (26),
      0.36 (1601); `legacy` 0.216 (3), 0.2498 (9), 0.2538 (58), 0.27 (60), 0.2792 (9), 0.2998
      (11), 0.3004 (85), 0.3046 (7), 0.3554 (25), 0.36 (1543).
      - Micro neck-down (`path/insert.rs::micro_neckdown_half_widths`): 0.27 and 0.216 (3/4 and
        3/5 of 0.36; 1/2, 0.18 mm, is skipped under the floor); 0.225 (3/4 of the necked
        retry's 0.3).
      - Pin neck-down (`board/pin_exits.rs::trace_neckdown_half_width`, pad min width less 2
        units, ends hit-tested in pcbnew): 0.2498 at the 0.25 mm pads (`DGS0010A_L` oval
        0.249999 x 1.15, `QFN-22_MPQ4242_MNP` rect 0.95 x 0.25), 0.2538 at 0.254 mm
        (`VQFN36_RRV_TEX`, `VQFN-FCRLF24_RXA-A_TEX`), 0.2792 at 0.2794 mm (`MSOP-10_MS_LIT`,
        `DGS10`), 0.3554 at 0.3556 mm (`HTSSOP20_PWP_TEX`), 0.2998 at 0.3 mm rects
        (`QFN-22_MPQ4242_MNP`), 0.3046 at 0.3048 mm (`USB4110-GF-A_GCT`), 0.3004 at the
        `TSSOP-10_3x3mm_P0.5mm` 1.45 x 0.3 roundrects, whose polygon (KiCad's outside-error arc
        approximation) is 0.3006 mm wide.
      - 0.2502: the same formula on a pad 0.2504-0.2505 mm across, a 0.25 mm roundrect: the
        padstack `RoundRect[T]Pad_250.000000x700.000000_62.738000_um` (canary-pwr's plugin DSN,
        `tests/data/canary-pwr-n05/plugin-b1994423.dsn`) is 0.250476 mm wide, neck 0.2502 mm. The
        pcu board at `76163c7` has no 0.25 mm roundrect pad (its pad history has one commit,
        `08bcd3f`), so the v5 board's 0.2502 pad is not identified by name.
      - `enhanced` only: 0.3 is the necked retry (`neck_width_um` 300); 0.252, 0.2752, 0.279,
        0.2964, 0.306, 0.312, 0.3176, 0.324, 0.336, 0.3388, 0.348 are widen-back's taper steps
        (`quality/neckdown/widen_back.rs`, `wide - (wide - narrow)·k/5` from 0.3, 0.2538 and
        0.225 mm up to 0.36).
      - The optimizer made no width: it is on but skipped every run ("Skipping optimization
        stage: the board has N unrouted connection(s)"); the input copper is stripped.
    - The board's own copper before the route (the committed, routed board): 1985 segments at
      0.2 mm, 36 at 0.5, 6 at 0.6, 5 at 0.7, 2 at 1.0 mm; none under its class width.
    - DIVERGENCES `board_minimums.*`: the 0.15 mm tracks are the micro neck-down's 3/4
      candidate, not the pin neck-down's. Checked on Q11.6's `mp1` sessions
      (`target/orchestrator/q1112/out/mp1`): vfo and mydevboard have 47 wires at 0.15 mm each;
      their smallest pads are 0.25 and 0.2 mm, whose pin neck-downs are 0.2498 and 0.1998 mm,
      and the run's necked retry (300 um) narrows nothing on a 0.2 mm class.

- [x] **K15.5 — The DRC report and the router's violations count what KiCad counts.**
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

    *Done 2026-10-21:* the report counts as KiCad (`drc/kicad_report.rs`, `ReportRules::Kicad`, the
    default of `DesignRulesChecker::generate_report`: `-drc`, a route's `.drc` output, the GUI's
    export); `ReportRules::Java` keeps Java's report, which `tests/reference_drc.rs` now pins.
    - *Changes:* `clearance`/`holeClearance` leave out a same-net pair unless one item is routed
      copper (a trace or via neither user nor system fixed), so a via the run puts in a pad stays
      reported; `BasicBoard::is_obstacle` is unchanged (D8). `via_dangling` is a via meeting its net
      on fewer than two layers (`stub_contacts::via_contact_layers`); `track_dangling` an end point
      on no copper of its net (`board_rules::end_on_copper`, now `pub(crate)`). The split nets,
      the scores, `check()` and `all_unconnected_items()` (GUI lists) stay Java's.
    - *The score:* new `router.java_quirks.same_net_via_pad_violation` (unset/`true` = Java's
      count; marker in `DesignRulesChecker::all_clearance_violations`, which the statistics and
      `record_pre_existing_violations` read). `false` drops the same pairs, routed vias still
      counted. In neither profile: promoting it needs a preregistered measurement.
    - *`-drc` settings:* warn only. One WARN per router setting from the command line or the
      environment ("The DRC (-drc) ignores router.hole_clearance_um, set by the command line: ...").
      Not applied under `enhanced`: that needs `prepare_job`'s board steps (`job/prepare.rs`), and
      `router.hole_clearance_um` grows the drill items' tree shapes, which would add the octagon
      hole pairs (the 142 -> 217 rise) to the report with no KiCad-style hole check.
    - *pcu board* (committed `1b161c7` copy, `-de Power_Control_Unit_24V.kicad_pcb -drc`, 0.36 s;
      KiCad: `kicad-cli pcb drc --format json --severity-all`, KiCad 10.0.7):

      | Type | before (`aacdfc2`) | after | KiCad 10.0.7 |
      |---|---|---|---|
      | holeClearance | 165 (121 same net) | 44 | `hole_clearance` 8 |
      | via_dangling | 117 | 17 | 0 |
      | track_dangling | 15 | 2 | 3 |
      | clearance | 4 | 4 | 0 |

      - holeClearance 44 and clearance 4: 39 pad pairs inside one footprint (J17, U11, U23, U26,
        J13, U34, U33, U28, U2), 5 trace-pad pairs (INA_ALERT_2 at RN3.1, BUS_GND at R110.2) and 4
        trace pairs (CH1_NO 1.8 mm against Net-(U12-PGTH)). All are 0.1467 mm or more apart. The
        router checks the net-class clearances (0.15-0.25 mm). KiCad applies the `.kicad_dru`'s
        unconditional `Minimum copper-to-copper spacing (5 mil)` instead. A kicad-cli run on a copy
        with a duplicated U23 reported that rule's 0.1270 mm against a Pour3A pad (class 0.25).
        Also, 3 of the 5 trace-pad pairs and all 4 trace pairs pass with round track ends (exact
        0.1598, 0.1604 and 0.2806 mm). The board's octagonal trace caps make them short.
      - KiCad's 8 `hole_clearance`: J17 A1/B12/A12/B1 and J13 A1-B12/A4-B9/A12-B1/A9-B4 against
        their own footprint's NPTH (0.1733-0.2063 < 0.254 mm). Java's report has no
        copper-to-hole check.
      - via_dangling 17: V_BUS_24V vias at (173.54, 94.80), (172.44, 94.80), (172.44, 95.90),
        (195.94, 94.80), (195.94, 95.90), (197.04, 94.80), (101.44, 107.30), (102.54, 107.30),
        (83.44, 107.30), (84.54, 107.30), (119.44, 107.30), (120.54, 107.30), (64.44, 96.40),
        (65.54, 95.30), (90.95, 86.17), (92.15, 86.17), (93.35, 86.17). Each sits in an F.Cu pour
        of its net (U12-U16, U18 IN, 24V entry) and in the In2.Cu plane. The DSN structure reader
        drops a signal-layer pour of a net with a plane layer (`insert_planes`), so the board has
        the via on In2.Cu only. The other 100 now count their pad or trace on F.Cu or B.Cu and
        their plane.
      - track_dangling: the 2 are VCC_3V3 #7153 (197.478, 43.2503) and USB_VBUS_SENSE #12034
        (194.6262, 51.6541), both in KiCad's list. KiCad's third is FE_HGATE's 0.3 mm segment
        (94.0442 -> 93.9193, 45.8749). Both of its ends lie on the 0.5 mm FE_HGATE segment's
        copper (0.1249 mm from that segment's end, within its 0.25 mm half width), so the anchor
        test connects it. The other 12 of the 15 end on another trace of their net, mid-segment
        or at an inner corner: #11833, #11292, #10907, #10885, #10606, #10511, #10121, #9637,
        #9627, #9032, #7286, #6959.
    - *Tests:* `tests/drc_kicad_report.rs` (5, one synthetic DSN):
      `kicad_report_counts_different_nets_and_copper_overlap` covers the same-net input via
      dropped, the routed via kept, the via joining pad C1-1 off its centre to the Gnd.Cu plane,
      the T junction and the stub. Also `java_report_counts_same_net_pairs_and_centre_contacts`,
      `score_switch_leaves_out_only_the_inputs_same_net_pairs`,
      `the_default_report_is_kicads_and_the_board_findings_stay_javas` and
      `the_fixture_has_an_input_via_and_a_routed_via_in_pads_of_their_net` (`is_obstacle`
      unchanged). `tests/drc.rs::issue575_unconnected_items` pins Java 109/4/44 and KiCad 0/4/44.
      `freerouting-cli/tests/drc_settings.rs`. `tests/settings_java_quirks.rs` has 41 switches.
    - *Gate:* `cargo fmt --all --check` 0; `cargo clippy --workspace --all-targets -- -D warnings`
      clean; `cargo test -p freerouting` all pass; `-p freerouting-cli` 181 passed;
      `-p freerouting-gui --test ui_drc` 24 passed. The connection-parity check
      (`PARITY_SET=tools/bench/sets/connection-parity.txt tools/parity-check.sh`) passed: probes
      PASS in 375 s, 7 of 7 exact, 7 SES byte-identical. No SES changes under either profile:
      the switch is unset in both, and `enhanced`'s set is unchanged
      (`legacy_keeps_every_java_bug_and_enhanced_fixes_exactly_the_promoted_set`), so no
      ses-diff was run.
    - *DIVERGENCES:* "The report counts what KiCad counts" and the `-drc` settings line under
      Design rule check, plus "K15.5 same-net vias in pads" under Java quirk switches.

- [x] **K15.6 — The run stops when nothing routes.**
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

  *Done 2026-10-21:* it stops; the stop rule does not change.
  - **Board.** The committed pcu board (`committed-1b161c7`), made with pcbnew 10.0.7 into an
    unrouted zoned board: its 6609 tracks removed, its 1022 vias kept, C65 moved to
    (57.251006, 105.242039) mm, and its zones refilled. Saved as
    `target/orchestrator/pcu-rev2/k156/zoned.kicad_pcb`, beside the committed `.kicad_pro` and
    `.kicad_dru`.
  - **Binary.** `084f3a9` (`target/orchestrator/pcu-rev2/k156/freerouting-rs-084f3a9`). Main
    `aacdfc2` has no change under `autoroute/`, `quality/`, `board/` or `freerouting-geometry`
    since then, so this is main before K15.1.
  - **Command.** `freerouting-rs-084f3a9 -de zoned.kicad_pcb -do outzz.kicad_pcb
    --router.plane_obstacle_net_classes=Pour1A,Pour3A --router.cleanup.remove_stubs=true
    --router.copper_to_edge_clearance_um=400 --router.hole_clearance_um=260
    --router.min_trace_width_um=130 --timeout 20m --progress json`.
  - **Result.**
    - The load: `planes: 22`, `wires: 0, vias: 1022`, baseline 1208 unrouted, 213 violations.
    - 18 passes of 1.5–1.7 s, each `routed 0` with 1208 unrouted; 38178 "could not be
      created" lines.
    - It stopped itself with "has not improved enough" after 29.5 s (256 MB).
  - **The report's own log agrees.** `route_v4`, on `8c952e7`, also stopped after 18 passes;
    see the "Found" note above. The report's 40-minute run was `route_v5`, a normal route.
  - No test is added, because the rule is unchanged.

- [x] **K15.7 — Failure messages and the edge-clearance cap name board items.**
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
  - *Done 2026-10-21:*
    - Text. A failed connection's log line keeps Java's frame and reason and writes each side as
      the pins' `Ref-Pad` names (`describe_item`; pin index 0 too), at most three pins per side,
      then a bracketed count of the rest with the net: `between U12-9 and C66-1 (+212 vias and
      traces of net BUS_GND), because ...` (`autoroute/connection.rs`, `describe_connection_named`).
      `AttemptResult` has a second field, `log_details`; `details` keeps Java's whole text,
      because `fixtures/reference/probe/insert/*.jsonl` holds Java's `Failed to route connection
      ...` lines and `autoroute_probe_insert` compares them (the dispatch assumed probes do not
      compare it). `pass.rs` and the failure log take `log_text()`. The result JSON already names
      `Ref-Pad` (`quality::verdict` uses `describe_item`); no change there. No consumer parses the
      line: `kicad-plugin/freerouting_rs` and `tools/` never match it (the plugin test data
      `progress-bm01.jsonl` is a recorded log), the `Autorouter ` prefix and the frame are kept.
    - Edge cap. `BasicBoard::pin_gap(leave_out)` returns the gap and the pin that sets it;
      `minimum_pin_gap()` is unchanged (`minimum_pin_gap_with`, `is_wholly_outside` on
      `BoardOutline`). The cap line is Info and names the pin and the gap in um. New setting
      `edge_cap_skips_off_board_pins` (opt-in, unset in both profiles; decided 2026-10-21 at
      review: an ENHANCED entry needs a preregistered measurement, and it changes the edge
      clearance of corpus boards such as Issue367-Charger, 0 -> 250 um): a pin whose pad shapes all lie wholly outside the outline is left out,
      with one WARNING per pin.
    - Accept 1: `autoroute::connection::tests::{a_failed_connection_names_its_pins_by_reference_and_pad_in_the_log,
      a_large_side_is_counted_in_brackets_and_the_line_stays_short (300 vias: `U1-1 (+300 vias of
      net A)`, Java's text 1524 chars),
      a_side_without_pins_names_its_first_item_and_lists_at_most_three_pins}`; the edge cap on a
      board with a fiducial at (0, 0) outside the outline and a pad 0.1 mm inside:
      `tests/k15_edge_cap.rs` (`legacy_keeps_javas_cap_and_names_the_off_board_pin`: capped to 0,
      names FID1-1; `an_off_board_pin_is_left_out_of_the_edge_cap_when_the_setting_is_on`: one WARNING,
      capped to 100.0 um, names U2-1; `legacy_leaves_the_setting_unset_and_enhanced_sets_it`).
    - Accept 2, pcu board (`-mp 1 --timeout 300`, 80 failure lines, 3 min in an opt-level 1
      build). Before (`route_v7`, first line, cut): `Autorouter Failed to route connection between
      pin #8 of component #330 and polylinetrace, polylinetrace, polylinetrace, polylinetrace,
      via, polylinetrace, via, ... pin #1 of component #333, pin of component #332, ...` (over
      1000 characters). After: `Autorouter Failed to route connection between U26-1@1, U26-1 (+2
      traces of net V_BUS_24V) and C37-1, C36-1, C49-1 (+224 pins, vias, traces and other items
      of net V_BUS_24V), because no connection was found between their nets.`; `... between C88-1
      and C37-1, C36-1, C49-1 (+227 pins, vias, traces and other items of net V_BUS_24V),
      because no connection was found between their nets.` The edge cap did not show on this
      board: the `.kicad_dru` raises `copper_to_edge_clearance_um` to 381, so the default cap does
      not run (a DSN input of it would).
    - Accept 3: `PARITY_SET=tools/bench/sets/connection-parity.txt tools/parity-check.sh
      --no-probes`: 7 of 7 exact, SES byte-identical, 79 s; `autoroute_probe_insert` (compares
      Java's `details`) passes; `rules_writer.rs` and `job_prepare_reference.rs` pin `legacy`
      (the first needed the pin, because `enhanced` changes `board_edge` on Issue367-Charger).

- [x] **K15.8 — The CLI's standard input and `--threads` are documented.**
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
  - *Done 2026-10-21:*
    - `--stdin-commands on|off` (default `on`; `args.rs`, `cli.rs::route_job`, listed by
      `--capabilities` through `EXTRA_FLAGS`; a bad value is exit 2). With `off`
      `spawn_stdin_reader` is not started; signals are unchanged.
    - Tests (`crates/freerouting-cli/tests/stdin_commands.rs`): the binary runs under `sh` with
      `cat` after it on one standard input of three lines: `with_stdin_commands_off_three_lines_stay_unread`
      (and the `=off` form) leaves all three for `cat`, `by_default_the_route_reads_standard_input`
      leaves none; `with_stdin_commands_off_and_stdin_closed_the_route_completes` (stdin null);
      `stop_still_stops_with_stdin_commands_on` (`stop` after the first route event: at most one
      pass, exit 0, SES written); `a_bad_value_is_a_usage_error`. `version.rs` and `args_java.rs`
      (`EXTRA_FLAGS.len() == 15`) agree.
    - Help: `STOPPING A RUN` says the route reads and consumes standard input, names the
      `while read` loop and the two ways out (the flag, `< /dev/null`); `--threads` and `-mt` say
      the autorouter's passes are sequential (D5), that the threads go to the optimizer and the
      analyses (DRC, statistics), and that the result is the same at any count. `args.rs` and
      `stop.rs` module docs, the DIVERGENCES `--threads` and stopping lines and `engine.md` say
      the same.

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

- [x] **K15.10 — The desktop app routes a `.kicad_pcb` with the board's own minimums.**
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
  - *Done 2026-10-21:* the app reads a `.kicad_pcb` as a CLI route does.
    - Engine: `io::kicad::read_kicad_for_route(path, apply_design_rules)` (one read for the CLI's
      route, the CLI's `--dump-settings` and the app); `job::prepare::KicadSources`
      (`from_read`: the read log, the Board Setup and mask-aperture sources, the `.kicad_dru`
      floors; `raise_floors`), which `JobInput::set_kicad` now uses (same lines, same order);
      `prepare_job_settings_with` (the app's merge: the sources join at the DSN's priority, the
      floors raise the result, the provenance comes back; `prepare_job_settings` calls it).
    - GUI: `session.rs` reads with `read_kicad_for_route(path, true)` (no switch in the app for
      `--no-design-rules`, so on, as the CLI's default) and keeps `Session::kicad`;
      `App::autoroute_patch` merges with `prepare_job_settings_with`, raises the user's patch again
      and logs the floors' lines; `Autoroute::provenance` keeps the sources. A `.kicad_dru` that
      does not parse is the reader's warning "KiCad reader: the .kicad_dru file could not be read",
      as on the CLI, in `Session::warnings` (log window).
    - Accept 1: `crates/freerouting-gui/tests/ui_kicad_settings.rs` on `classes.kicad_pcb` with a
      `.kicad_pro` `min_track_width` 0.15 mm and a `.kicad_dru` `track_width (min 0.25mm)`:
      `the_app_routes_with_the_settings_of_the_cli` (the app's baseline, its job patch and its
      provenance equal `prepare_job`'s on a `JobInput` filled by `set_kicad`, which is what the CLI
      route and `--dump-settings` build; `board_minimums.track_width_um` 250 from "kicad_pro board
      minimums"), `a_rule_below_board_setup_leaves_it` (150 stays),
      `the_log_and_warnings_are_the_clis` (the "raised from 150 to 250" line in the app's log;
      the broken-file warning among the session warnings). RED first with the app's old read
      (`ReadOptions::default()`, no sources): all 3 failed (`left: None, right: Some(250.0)`). The
      real binary on the same board: `freerouting-rs -de classes.kicad_pcb --dump-settings=json`
      gives `board_minimums.track_width_um` 250.0, source "kicad_pro board minimums".
    - Also: `--dump-settings` read a `.kicad_pcb` with `ReadOptions::default()` (the `.kicad_dru`
      floors ignored, so 150 here while the route used 250). It now reads with the route's options
      (`!--no-design-rules`): `cli_kicad.rs::dump_settings_shows_the_dru_floors_a_route_applies`.
    - Accept 2: the settings window shows no provenance (it edits a `SettingsPatch`; Java's has no
      source column), so no provenance UI was added.
    - Accept 3: the harness has no renderer or window; `frontend-smoke.sh` (DSN fixtures only) not run.
    - Gate: `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`;
      `xvfb-run cargo test -p freerouting-gui -p freerouting-cli`: 506 passed, 0 failed;
      `cargo test -p freerouting --lib` (312) and the `autoroute_deltas`, `board_minimums`,
      `job_overrides`, `job_prepare_reference`, `kicad_read`, `kicad_write` tests green.

- [x] **K15.11 — The DRC report's last differences from KiCad on the pcu board.**
  - **Change:** after K15.5 (`1defadd`, `drc/kicad_report.rs`), the report on the pcu board still
    differs from KiCad 10.0.7 by named items (K15.5's note). Close each:
    - **via_dangling 17.** Each V_BUS_24V via sits in an F.Cu pour of its net and in the In2.Cu
      plane. `Structure::insert_planes` drops a signal-layer pour of a net with a plane layer, so
      the board has the via's copper on In2.Cu only. Record the dropped pour (no board item, kept
      by snapshots) and count it as copper of its net in `via_dangling` and `track_dangling`:
      KiCad fills it. An orchestrator branch did this on 2026-10-21 (`BasicBoard::dropped_pours`,
      recorded in `io/specctra/parser/structure.rs`): 0 dangling vias on the board at `76163c7`.
      Its patch is `target/orchestrator/k155-duplicate.patch` (not on main; written against
      `41cf105`, a different report design). Check whether Q16's stub cleanup
      (`quality/stub_contacts.rs`) needs the same.
    - **clearance and holeClearance: the octagons.** Java measures the search-tree shapes, which
      put a circle in its bounding octagon and give a trace octagonal ends; 3 of the 5 trace-pad
      pairs and all 4 trace pairs pass on the items' own copper (round ends, circles). Measure a
      pair the report keeps again on its own copper before calling it a violation (the patch's
      `exact_clearance`). KiCad reports a pair once: keep one finding per pair of items (its
      smallest clearance over the layers), with KiCad's type `clearance`.
    - **clearance: the rule's value.** The 39 pad pairs inside one footprint and the rest are
      0.1467 mm or more apart, under the class clearances (0.15–0.25 mm) and over the `.kicad_dru`'s
      unconditional `Minimum copper-to-copper spacing (5 mil)` (`clearance (min 0.127mm)`), which
      replaces the class clearance in KiCad. The router keeps the larger value (K15.3, decided
      2026-10-21: no DRC breach, possibly less room). The report counts with the rule's value
      where an unconditional custom clearance rule applies; the router's settings and routes do not
      change.
    - **hole_clearance 8.** J17 A1/B12/A12/B1 and J13 A1-B12/A4-B9/A12-B1/A9-B4 against their own
      footprint's NPTH (0.1733–0.2063 < 0.254 mm). The report has no copper-to-hole check for
      pads; Q20's `hole_clearance` (`drc/board_rules.rs`) checks traces and vias. Add pads of
      another net against a hole, as KiCad's `hole_clearance` does.
    - **track_dangling 2 against KiCad's 3.** FE_HGATE's 0.3 mm segment (94.0442 -> 93.9193,
      45.8749): both ends lie on the 0.5 mm FE_HGATE segment's copper, which KiCad still reports.
      Find KiCad's rule (`TestTrackEndpointDangling`) and match it, or name why not.
  - **Accept:**
    - on the pcu board copy (`../power-control-unit` `76163c7`, and the `1b161c7` copy if on disk),
      each type equals KiCad's or differs by named items, with the before/after table;
    - tests per case on small boards: a via in a dropped pour and a plane, a pair that fails as
      octagons and passes as copper, an unconditional clearance rule under the class clearance (the
      report takes the rule's, `--dump-settings` the larger), a pad of another net near an NPTH;
    - `ReportRules::Java` and the router's score unchanged (`tests/reference_drc.rs`), parity 7 of
      7 if shared code changes (the dropped-pour record is in the DSN reader).
  - *Done 2026-10-21:* `-drc` on the board copy (`../power-control-unit` `76163c7`,
    `Power_Control_Unit_24V.kicad_pcb`, sha256 `dde7bb00…`; the `1b161c7` copy is not on disk),
    release, 0.13-0.14 s, against `kicad-cli pcb drc --severity-all --format json` (KiCad 10.0.7):

    | Type | before (`71ed14b`) | after | KiCad 10.0.7 |
    |---|---|---|---|
    | clearance | 78 | 0 | 0 |
    | holeClearance | 276 | 0 | - |
    | hole_clearance | - | 4 | 4 |
    | via_dangling | 248 | 0 | 0 |
    | track_dangling | 1 | 0 | 0 |
    | isolated_island_dead_copper | 4 | 4 | 0 |
    | unconnected | 101 | 101 | 0 |

    - hole_clearance 4: the same J13/J17 pads against their footprint's NPTH as KiCad's 4, exact
      0.1733-0.2063 mm at 0.2540 mm. KiCad's other findings are 60 `lib_footprint_issues`.
    - Named remainders, outside this item's change list: unconnected 101 are 31 split nets (Java's
      connectivity, kept by K15.5's design) and 70 islands of KiCad's fill (V_BUS_24V 26,
      /BAT_VP 10, ...) that KiCad's zone fill joins; isolated_island_dead_copper 4 are U24-SW2,
      /BAT_VP, D11-C and /BUCK_6V. Not closed here.
    - Dropped pours (`BasicBoard::dropped_pours`, recorded in `Structure::insert_planes`, kept by
      snapshots) count in `via_dangling` and `track_dangling`. Q16's stub cleanup
      (`quality/stub_contacts.rs`) needs no change: it removes on the router's connectivity, which
      lacks the pour, so it only keeps more than KiCad would; no test showed a wrong removal.
    - A kept pair is measured again on its own copper (`drc/kicad_copper.rs`, `exact_clearance`)
      and typed `clearance`. Not one finding per pair: KiCad 10.0.7 reports a through-via pair once
      per layer (case f-norule: two vias 0.1673 mm apart under 0.2 mm give 4 `clearance` on 4
      layers), so the report keeps Java's per (pair, layer) dedup, as KiCad does.
    - The unconditional `.kicad_dru` clearance rule is the report's value (`drc/kicad_rules.rs`,
      `KicadReportRules::clearance_um`); `-drc` on a `.kicad_pcb` reads the sibling `.kicad_dru`
      itself (evaluated again on a copy of the tree, as `-drc` reads without applying it).
      `--dump-settings` on case f is byte-identical before and after (`cmp`). On a DSN input `-drc`
      ignores `--kicad-dru` (exit 0, no message), before and after: not changed here.
    - FE_HGATE: KiCad's `TestTrackEndpointDangling` (KiCad 10.0 `connectivity_data.cpp`) tests each
      segment's ends with an accuracy of half its width; a zone, pad or via hitting both ends
      connects it, another segment hitting both ends credits only the end nearer to that segment's
      ends. Matched per design segment (the reader's dropped collinear wire corners are kept in
      `BasicBoard::wire_joints`). Case g gives KiCad's 1 and Issue575 KiCad 9.0.1's 5
      (`[1697, 1858, 1860, 2265, 2372]`).
    - Tests, RED on `71ed14b`'s binary, GREEN now, `crates/freerouting/tests/drc_kicad_pcu.rs`
      (9 pass), boards `tests/data/kicad/pcu-k155/{a,b,c,d}` and `pcu-k1511/{e,f,g}`:
      `a_via_in_a_dropped_pour_and_its_plane_is_connected` (b: via_dangling 6 -> 0, KiCad 0),
      `dropped_pours_and_the_report_rules_survive_a_deep_copy`,
      `a_pair_that_fails_as_octagons_passes_as_copper` (d: holeClearance 4 -> 0, KiCad 0),
      `a_true_violation_is_kept_once_per_layer_with_its_own_distance` (f-norule: holeClearance 4
      -> clearance 4, KiCad clearance 4),
      `an_unconditional_rule_under_the_class_clearance_is_what_the_report_counts` (f-rule:
      holeClearance 4 -> 0, KiCad 0; the router's matrix keeps 0.2 mm),
      `a_pad_of_another_net_near_an_npth_is_a_hole_clearance` (e: 0 -> hole_clearance 4, KiCad 4),
      `a_short_segment_on_one_trace_dangles_and_a_junction_on_a_joined_trace_does_not` (g: 0 -> 1,
      KiCad 1), `same_net_vias_in_a_pad_are_no_finding`,
      `a_stub_that_represents_a_split_group_is_reported` (c: 0 -> 1, KiCad 1).
      `tests/kicad_read.rs::dev_board_drc_equals_the_exports` and
      `cli_kicad.rs::dev_board_report_equals_the_dsn_export` now expect KiCad's 4 J1
      hole_clearance on the `.kicad_pcb` (0.2349/0.2001 at 0.2500 mm).
    - `ReportRules::Java` unchanged: `tests/reference_drc.rs` passes;
      `tests/drc.rs::issue575_unconnected_items` keeps Java's counts and score (only its KiCad-mode
      track_dangling list changed, to KiCad's 5). Parity:
      `PARITY_SET=tools/bench/sets/connection-parity.txt tools/parity-check.sh --no-probes`:
      `PARITY OK`, 7 of 7 `[exact]` with `bytes true` (SES unchanged).
    - DIVERGENCES: the "The report counts what KiCad counts" entry is extended with K15.11.
    - The desktop app (orchestrator, after K15.10): `session.rs` puts the same report rules on the
      app's board from K15.10's `KicadSources` (`kicad_report`), so its DRC export counts as
      `-drc` does. `freerouting-gui/tests/ui_kicad_settings.rs::the_app_board_has_the_clis_report_rules`
      (`f-rule-clearance`, `e-npth-pads`): RED without it (`clearance_um: None` against
      `Some(127.0)`), GREEN with it.

- [x] **K15.12 — A sweep pass that adds a violation keeps the connections that don't.**
  - **Change:** the add-only sweep (`quality/sweep.rs`, `SweepController::after_pass`) rolls a
    whole pass back when the violation count rises. On K15.4's `.kicad_pcb` run of the pcu board
    (main `41cf105`, plugin default options, `--max-passes 2 --timeout 240`, files in
    `target/orchestrator/k154/run-main-pcb/`), sweep pass 1 routed 53 and took incomplete from 65
    to 16 with violations 4 -> 5, and the whole pass was undone: the run ended at 65 (KiCad 96
    unconnected). The DSN run on the same board kept its sweep (59 -> 12, violations 4 -> 4).
    Undo only the connections whose routing added a violation (one at a time in the pass's order,
    or each connection checked as it is routed), keep the rest, and say which the board's
    violation was.
  - **Found 2026-10-21:** in both runs pass 2 took the board from 65 (59) to 256 (250)
    incomplete and the best-board restore brought pass 1's back; that is Q23's subject, not this
    item's.
  - **Accept:**
    - a test on a small board where one swept connection adds a violation and another doesn't:
      the second is kept, the first undone, and the board's violations do not rise;
    - the pcu `.kicad_pcb` run again (a few minutes), with KiCad's grade: unconnected before and
      after, 0 below-minimum and 0 new routing-caused DRC errors;
    - the sweep is opt-in in `legacy` (unset there), so `legacy` is untouched; a DIVERGENCES line
      if `enhanced`'s default path changes.
  - *Done 2026-10-21:* on main `bad4171` (branch `k1512`).
    - The change (`quality/sweep.rs`): before, `AddOnlyRouter::route` only turned rip-up off and
      `SweepController::after_pass` rolled the whole pass back when `after.violations >
      before.violations`. Now `AddOnlyRouter::route` clones the board before each sweep connection,
      routes it, and `added_violations` counts the router's clearance violations
      (`drc::counts_clearance_violation`, the filter `DesignRulesChecker::all_clearance_violations`
      uses, now shared) on the items the connection inserted or changed (a clone shares every
      unchanged item, so a pointer test finds them) against those of the items it removed or
      changed on the board before. When the count rises, the board before comes back
      (`swap_in`), the connection counts as failed, and the job log says which connection and where:
      `Add-only sweep pass #1 (free): undid the connection of S2-1 (net A): it added 1 clearance
      violation(s), the first polylinetrace (net A) - S5-1 (net X) on F.Cu at (20.000, 5.000) mm.`;
      the pass line adds `, undone N` (`SweepPassRecord::undone`). The pass-level rules are
      unchanged: a pass that raises the incomplete count, or the violations through a pair the
      guard's count misses, is still undone whole.
    - Accept 1: `a_sweep_connection_that_adds_a_violation_is_undone_and_the_others_kept`
      (`tests/quality_sweep.rs`, new fixture `tests/data/quality/k15-sweep-guard.dsn`: nets A and B
      to route, a lone pad of net X; a test router routes nothing in the batch loop and, in the
      sweep, "routes" A by a detour over X's pad). RED at main: `free: B kept, A undone` left
      `[1, 2]` right `[1]` (the whole pass undone). GREEN, `free` and `fixed`: B routed, A's two
      pin connections undone (pass 1 `routed 1, failed 2, undone 2; incomplete 2 -> 1, violations
      0 -> 0`), no trace of net A left, `DesignRulesChecker` violations 0.
    - Accept 2, the pcu `.kicad_pcb` run (`tools/kicad-reader/plugin-input-ab.sh kicad_pcb
      target/orchestrator/k154/stripped/Power_Control_Unit_24V.kicad_pcb
      target/orchestrator/k154/opts-default.json <out> -- --max-passes 2 --timeout 240 --stats
      json`, `ROUTE_TIMEOUT_S=300`, `ROUTE_MEMORY_KB=8000000`, outputs in
      `target/orchestrator/k1512/runs/`). On main `bad4171` the sweep no longer rolls back (K15.15
      changed the planes): pass 1 routed 52, incomplete 65 -> 14, violations 4 -> 4; pass 2 14 ->
      11. No corpus row of `k15-all-pours.txt` in K15.14's runs (`target/orchestrator/k1514/out/`)
      rolls a pass back for a violation either (their roll-backs are passes that routed nothing),
      so the test builds the case. With the guard the guard undoes nothing on pcu and the session
      is byte-identical (sha256 `a2d99014…`). KiCad 10.0.7 under the board's own rules, before
      (main) and after: unconnected 47 and 47 (input 499), `below_minimum` 0 and 0,
      `manufacturing_breaches` 0 and 0, routing-caused `track_dangling` 1 and 1 (the same
      session: no new routing-caused item). Cost (`--stats json`): sweep pass 1 21.85 s -> 18.93 s,
      pass 2 7.93 s -> 5.10 s (wall time, machine noise), search-tree queries 6802257 -> 6805704 and
      1046255 -> 1046615 (+0.05% and +0.03%, the guard's checks); route_s 198.3 -> 168.1.
    - Accept 3: `legacy` and the engine's own `enhanced` leave `router.autorouter.sweep` unset
      (`freerouting-rs --profile legacy --dump-settings`: `autorouter.sweep = (unset)`), so
      `run_sweep` and the guard are never reached; only the plugin's default `enhanced` run passes
      `sweep=free`. Shared code changed only by moving the checker's filter into
      `counts_clearance_violation` (same result): `PARITY_SET=tools/bench/sets/connection-parity.txt
      tools/parity-check.sh --no-probes` PARITY OK, 7 of 7 `[exact]`. DIVERGENCES: a K15.12 line
      under "Add-only completion sweep".

- [x] **K15.13 — Routing keeps a pour whole, or counts it split.**
  - **Change:** on K15.4's DSN run of the pcu board (`target/orchestrator/k154/grade-main-dsn/`,
    plugin default options), KiCad 10.0.7 reports 55 unconnected where the router reports 12
    incomplete. 39 of the 55 are Zone-Zone items: 27 zones are split into islands after the route,
    9 before it (`unrouted/drc.json`). On the `.kicad_pcb` run, 96 against the router's 65, with
    22 split zones. The split zones are outer-layer signal pours: `EFUSE_NO_1`-`_8`,
    `CH1_SENSE`-`CH8_SENSE`, `USB_GND_CAPS`, `USB_PORT_GND`, `USB_SW2`, `Q3S`, `Q3d`,
    `V_INRUSH`, `BUCK_V`, `BAT_FETS`, `12V_BUCK_SW1`. Other nets' new traces cross them, KiCad's
    refill cuts the pour, and the router still counts the pour as one connected item.
    - Neither run had a pour fix-up: the board has one net class (Default), so K14.4's
      `auto_fixed_pour_classes` (pours above the lowest priority on their layer, by class) finds
      none, and `plane_obstacle_net_classes` is unset.
    - Find how the router models these pours on each input (conduction area, obstacle or not, and
      whether its connectivity sees the cut), then make the default path either route around a
      pour it would split or count the split as KiCad does, so the router's incomplete equals
      KiCad's unconnected (or differs by named items). Say whether the answer is a default (both
      inputs, `enhanced`) or a per-board option, and why.
  - **Accept:**
    - a test on a small board with a signal pour that a crossing trace would split;
    - the pcu board routed again on both inputs (a few minutes each), graded by KiCad: Zone-Zone
      unconnected items no more than the input's 9, the router's incomplete against KiCad's
      unconnected, 0 below-minimum and 0 routing-caused DRC errors;
    - `legacy` unchanged unless Java at the pin models pours the same way (say which), parity 7
      of 7 if shared code changes.
  - *Partial 2026-10-22:* met with `router.plane_obstacle_all_pours`, a per-board option and not a
    default (K15.14: not kept, KiCad unconnected 368 -> 493 over 16 rows; the plugin offers it as
    a checkbox). On the `.kicad_pcb` input every Accept 2 clause holds (K15.14's amendment 3
    `pcb-on` arm of `pcu-76163c7`, engine `4916e6d`: KiCad 20, router incomplete 20, Zone-Zone 6,
    0 below-minimum, 0 routing-caused). On the DSN input it does not: K15.14's `dsn-on` arm has 4
    `via_dangling` routing-caused and router 26 against KiCad 31, items not matched (K15.15 cuts
    planes on the `.kicad_pcb` path only; a DSN carries no priorities). K15.13's own runs had 3-4
    `via_dangling` in both arms (below).
    - *Found:* both inputs give each pour the same item. The DSN reader inserts every `(plane ...)`
      as a conduction area that is no obstacle (`io/specctra/parser/structure.rs::insert_planes`,
      `insert_conduction_area(.., false, SystemFixed)`). Java does the same at the pin
      (`Structure.java` 1141, `insertConductionArea(.., false, ..)`). The `.kicad_pcb` reader writes
      the same plane from the zone's outline, not its fill (`freerouting-kicad/src/zones.rs`,
      `make_plane`). `BasicBoard::is_obstacle` (Conduction branch) and `is_trace_obstacle` block a
      foreign net only if `is_obstacle` is set or an area policy says so. The router's
      connectivity (`conduction_area_normal_contacts`, Java `ConductionArea.getNormalContacts`)
      connects every pin and via centred in the outline, and every trace ending there, however
      foreign copper crosses it. So a crossing trace costs the router nothing, and KiCad's refill
      cuts the pour into islands. Neither run had a pour fix-up. The board has one class
      (`default` on the `.kicad_pcb`, `kicad_default` in KiCad's DSN), so K14.4's
      `auto_fixed_pour_classes` finds none, and `plane_obstacle_net_classes` is unset.
    - *Crossings* on the DSN run at main (`k154/grade-main-dsn`, new copper of other nets
      overlapping each zone's outline, `shapely`):
      - `CH1_SENSE` (F.Cu, priority 44): BUS_GND 3 segments and 2 vias, VCC_3V3 2 segments;
      - `EFUSE_NO_1` (`/CH1_NO`, 43): the same BUS_GND and VCC_3V3 copper;
      - `Q3S` (`Net-(Q4-D)`, 17): BUS_GND, `/CHG_EN`, `Net-(Q4-G)`, `/Charger Circuit/DRV_SUP`,
        `/HYST_OVR`;
      - `V_INRUSH` (6, 986 mm2): ten nets, 53 segments and 10 vias.
    - *Fix:* the setting `router.plane_obstacle_all_pours` (`settings/fields.rs`, JSON
      `planeObstacleAllPours`, `opt` bool, unset in both profiles; `tests/settings_defaults.rs`).
      With it, Q01's pour guard (`quality/pour_guard.rs`, `ExemptionTable::build_all`) lists every
      signal-layer pour whatever its class, with Q01's exemptions (a foreign net with a pin or input
      via inside the outline is not blocked). No class names are needed, so the same flag works on
      both inputs. Candidate (b), the router seeing the cut, would need a refill of each pour
      around foreign copper on the board (polygon clipping the geometry crate lacks), and it would
      still split the pours. With one class the setting equals `plane_obstacle_net_classes=default`:
      the `.kicad_pcb` session is byte-identical (`e296697d…`) to a run with that list.
    - *Accept 1:* `crates/freerouting/tests/quality_pour_guard.rs` (K15.13 section, on Q01's
      `q01-pour-between-pads.dsn`: Z's pour lies on S's shortest path):
      `with_every_pour_listed_s_goes_around_and_the_pour_stays_whole` (unset: S crosses and
      `pour_audit` shows more islands; set: S routes, 0 traces on the pour, the audit is clean,
      the session equals the `Pwr` class list's), `every_signal_pour_is_listed_without_a_class_name`,
      `every_pour_lists_the_signal_pours_and_no_plane` (canary-pwr-pours-as-planes: every
      signal-layer pour of several classes, no power-layer plane, no D8-dropped pour),
      `the_all_pours_key_installs_the_policy_only_when_true`. RED: E0599 (`build_all`
      missing). GREEN: 17 passed, 1 ignored. They fail when `install` ignores the key: 2 failed.
    - *Accept 2*: release binary at `2d88784` plus this change. Each run is
      `tools/kicad-reader/plugin-input-ab.sh <input> k154/stripped/Power_Control_Unit_24V.kicad_pcb
      k154/opts-default.json <out> -- --max-passes 2 --timeout 240 [--router.plane_obstacle_all_pours=true]`
      (`ROUTE_TIMEOUT_S=300`, `ulimit -v 8000000`), graded by KiCad 10.0.7 under the board's own rules.
      Runs are in `target/orchestrator/k1513/runs/pcu-*`. The input board has 11 Zone-Zone items
      (5 nets, `input/drc.json`). The off arms reproduce K15.4's runs: `.kicad_pcb` session
      `03a14dd7…`, the same as K15.4's.

      | arm | KiCad unconnected | Zone-Zone (zones) | router incomplete | below-minimum | routing-caused | route s |
      |---|---|---|---|---|---|---|
      | `.kicad_pcb` off | 96 | 28 (23) | 65 | 0 | track_dangling 3, via_dangling 3 | 164.7 |
      | `.kicad_pcb` on | 26 | 6 (6) | 23 | 0 | via_dangling 3 | 146.6 |
      | DSN off | 55 | 38 (25) | 12 | 0 | via_dangling 4 | 164.3 |
      | DSN on | 31 | 7 (6) | 26 | 0 | via_dangling 4 | 157.6 |

      - With the setting on, KiCad's count exceeds the router's by 3 (`.kicad_pcb`) and 5 (DSN);
        with it off, by 31 and 43.
      - The Zone-Zone items left with it on:
        - `V_INRUSH`, crossed by BUS_GND, `Net-(U11-ACN/ACP/ACUV/ACOV)`, `Net-(Q1-D)`,
          `Net-(Q8-G)` and `/HYST_OVR`;
        - `D11_C`, crossed by `Net-(D11-A)`;
        - `BAT_FETS`, crossed by `Net-(Q10-G)`.

        All of these are nets exempt under Q01's rule, with a pin inside the outline. Three more
        zones have no new foreign copper over them: `USB_GND` and `U24-OUT` were split in the
        input as well, and `U24-SW2` is split for a reason not found.
      - The other items (20 on `.kicad_pcb`, 24 on DSN, against the router's 23 and 26) are not
        matched item by item.
      - The `via_dangling` items are V_BUS_12V vias, at the same places in both arms
        (165.71, 44.32; 169.05, 47.44; 173.72, 40.53). Each lies where In2.Cu's V_BUS_24V zone
        (priority 41) overlaps V_BUS_12V's (priority 1): the router counts the via in V_BUS_12V's
        plane, and KiCad fills V_BUS_24V there. The setting does not cause them. They remain for a
        new item (overlapping plane-layer zones of different priority).
      - A third board: canary-pwr (`k146/boards/canary-95882f00`, tracks and vias stripped, 1606
        removed), the same commands (runs `canary-*`).
        - `.kicad_pcb`: KiCad 40 -> 36, Zone-Zone 18 -> 7, router incomplete 27 -> 38;
        - DSN: 38 -> 36, 17 -> 7, 24 -> 39;
        - 0 below-minimum and 0 routing-caused in all four arms.

        The router routes fewer connections there (the setting's cost in room), but KiCad's count
        still falls.
    - *Accept 3:* `legacy` is unchanged. Java at the pin models these pours the same way: a
      conduction area that is no obstacle and is one connected item. The setting is unset in
      `legacy`, and `install` returns before building a table when both keys are unset.
      `PARITY_SET=tools/bench/sets/connection-parity.txt tools/parity-check.sh --no-probes`:
      PARITY OK, 7 of 7 exact.
    - *Remains:*
      - the DSN input's 4 V_BUS_12V `via_dangling` and its router-against-KiCad gap of 5
        (`dsn-on` of K15.14: 26 incomplete, 31 unconnected): the plugin's DSN fix-up could cut the
        planes from pcbnew's priorities (K15.24; K15.15's last note). The
        plugin's default input is `.kicad_pcb`, so the `legacy` DSN path is the one left.
    - *Closed 2026-10-22:* the default: K15.14 decided "not kept, no gain", so no `ENHANCED`
      entry; the plugin offers the flag (`RouteOptions.plane_obstacle_all_pours`, the Advanced
      tab's "Keep other nets out of every pour"). The V_BUS_12V `via_dangling` on the
      `.kicad_pcb` input: K15.15.
    - *Done 2026-10-22:* the DSN remainder closed by K15.24 (the plugin's DSN export cuts the
      planes by zone priority). K15.24's `dsn-on` cut arm of the pcu board
      (`target/orchestrator/k1524/runs/pcu-dsn-on-cut`, release CLI at `1a5f1b8`, graded by KiCad
      10.0.7): KiCad 27 unconnected, Zone-Zone 6, router incomplete 26 (the gap 5 -> 1, not
      matched item by item), 0 below-minimum, 0 manufacturing breaches, 0 routing-caused (was
      `via_dangling` 4). With the `.kicad_pcb` `pcb-on` arm above, Accept 2 holds on both inputs
      under the option; Accept 1 and 3 as above.

- [x] **K15.14 — Measure `plane_obstacle_all_pours` under `enhanced`.**
  - **Change:** K15.13's runs are exploratory (two passes, one run per arm). Register a measurement
    before its numbers exist, run it, and apply its decision table: `router.plane_obstacle_all_pours`
    joins `ENHANCED` (`settings/profile.rs`) or stays opt-in.
    - Prereg: `docs/roadmap/prereg/K15.14.json`, `tools/bench/sets/k15-all-pours.txt`, a pin test
      in `crates/freerouting-dev/tests/`.
    - Boards: the pcu board (`../power-control-unit` `76163c7`, tracks and vias stripped, by
      absolute path, skipped when missing), canary-pwr `95882f00` stripped, `tools/bench/sets/q01-pours.txt`
      (interf_u, pic_programmer, kit-dev-coldfire, complex_hierarchy), and the 4-layer corpus
      boards with signal pours. Name the runs K15.13 already made on two of them.
    - Inputs: `.kicad_pcb` on every board (the plugin's default); DSN on the pcu board and
      canary-pwr only (decided 2026-10-21: no hour-long runs).
    - Arms: `enhanced` against `enhanced` plus `router.plane_obstacle_all_pours=true`, plugin
      default options otherwise.
    - Signal: KiCad's unconnected after refill, summed over the boards. Secondary: Zone-Zone
      items, and KiCad's unconnected less the router's incomplete.
    - Guards: 0 below-minimum and 0 manufacturing breaches in every arm, routing-caused DRC not
      up, no board worse than its own noise band (decided 2026-10-21: the totals rule's band,
      canary-pwr 10, others max(2, ceil(5 % of connections))), route time at most 25 % more.
    - A few minutes per run (`--max-passes` and `--timeout` registered); the whole set is a heavy
      run, queued through the orchestrator.
  - **Accept:**
    - the prereg pin test passes with the hashes;
    - the per-board table and the decision in the note;
    - if kept: the `ENHANCED` entry, its test, DIVERGENCES, and `legacy` unchanged (parity 7 of 7);
      if not: the plugin offers the setting as an option (`options.py`, the dialog, a test).
  - *Partial 2026-10-21:* registered 2026-10-21 at main `8b37947`: `prereg/K15.14.json` (id
    `K15.14-all-pours`) sha256 `4b5cd80a32fe2a8ee6915b39d44bb117034b9eefd808f7e339ea1d74ffe8232f`,
    `tools/bench/sets/k15-all-pours.txt` (17 rows) sha256
    `900c81f41e26f6f688909e4001f3156c182d0712d464e07b8df2efd062f3d408`, 0 amendments (edited in
    place before any number after the orchestrator's review, decided 2026-10-21: DSN arms on two
    rows, guard (3) per-row band, rows that don't complete left out by name). **No number
    of this measurement exists**: nothing was routed or graded, no kicad-cli ran.
    - *Arms:* `pcb-off` (baseline) and `pcb-on` (candidate) on the `.kicad_pcb` path, `dsn-off` and
      `dsn-on` on the DSN path (secondary, on `pcu-76163c7` and `canary-pwr-95882f00` only: 38
      calls, about 35-50 min estimated, 3.5 h if every call hit its 300 s cap), each `tools/kicad-reader/plugin-input-ab.sh` (K13.5's,
      `17af015a…`) with the plugin's default options (`tools/bench/k15-all-pours.options.json`,
      `{"schema": 6}`), `-- --max-passes 2 --timeout 240`, `ROUTE_TIMEOUT_S=300`,
      `ROUTE_MEMORY_KB=8000000`; the `-on` arms add `--router.plane_obstacle_all_pours=true` and
      share the `-off` arm's grade when the sessions are byte-identical. Engine: the release CLI of
      the commit this registration lands in, built in `target/orchestrator/k1514/tree`; amendment 1
      names it before any arm runs.
    - *Boards* (`target/orchestrator/k1514/boards/<row>/`, stripped with `kicad_steps.py strip`
      beside the source's own `.kicad_pro`/`.kicad_dru`, made by `tools/bench/k15_all_pours.py
      prepare`): `pcu-76163c7` and `canary-pwr-95882f00` (byte-identical to K15.4's and K15.13's
      stripped boards, whose exploratory runs are listed under `seen_before_registration`); Q01's
      four demos; and the 11 `4 layers` rows of `kicad-corpus.txt` whose stripped board has a
      netted, non-keepout zone on a copper layer typed other than `power`/`jumper`
      (`k15_all_pours.py pours`; `picodvi-picodvi` has none). Three of them (`moteus-moteus-c1`,
      `nudac-nudac`, `vna-usb-pd`, sources at version 20221018 or older) don't strip to the same
      bytes twice (KiCad gives old-format items new UUIDs on load), so keep
      `target/orchestrator/k1514/boards/` until the run is done.
    - *Rule:* totals, `accept: improve`, on the `.kicad_pcb` arms: KiCad's unconnected summed over the
      judged rows falls by more than the set noise (canary-pwr's band 10, every other board
      max(2, 5 % of its connections)); guards: 0 manufacturing breaches, below-minimum and vias in
      pads in every arm that runs; `drc_real` total not up; no row worse than its own band; route
      time total at most 1.25×. A row whose arms don't all complete (aborted, not graded, not
      COMPLETED) is left out and named; the result is "not measured" only when fewer than 3 rows
      are judged or the pcu row is not judged. Secondary: Zone-Zone items and KiCad's unconnected less the router's
      incomplete, per arm. The decision table maps each outcome to one row (not measured, breach,
      not kept no gain, not kept guard, kept).
    - *Pin test:* `crates/freerouting-dev/tests/prereg_k15_14.rs`: 8 passed (the JSON's and the
      set's hashes, every present board and rules file against its hash, the 38 commands against
      their arm templates, the script and helper hashes, 0 amendments).
    - heavy run pending: `cd /home/thebo/code/freerouting-rs && git worktree add --detach
      target/orchestrator/k1514/tree <K15_14_COMMIT> && cd target/orchestrator/k1514/tree &&
      CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
      CARGO_TARGET_DIR=/home/thebo/code/freerouting-rs/target/orchestrator/k1514/target-release
      cargo build --release -p freerouting-cli -p freerouting-dev`; then amendment 1 (with the pin
      updated); then `cd /home/thebo/code/freerouting-rs && mkdir -p target/orchestrator/k1514/out
      && python3 -I target/orchestrator/k1514/tree/tools/bench/k15_all_pours.py run
      target/orchestrator/k1514/tree/docs/roadmap/prereg/K15.14.json >
      target/orchestrator/k1514/out/run.log 2>&1; echo "EXIT $?" >>
      target/orchestrator/k1514/out/run.log` (38 calls, the registration's `commands`), then
      `collect` and the gate as `heavy_run` spells out.
    - *Amendments:* 1 (`1de9a20`): the engine at `f687b76`. 2: that run was stopped unread
      (K15.15 changes the `.kicad_pcb` planes the setting acts on); the engine is `b8bbe78` (K15.15,
      K15.17), binary sha256 `7e0f18ee…`, pin test 8 passed in the tree, the seven script and plugin
      hashes unchanged, all 17 boards present as registered. All 38 calls run again.
    - *Run 2026-10-21 (amendment 2, engine `b8bbe78`, `target/orchestrator/k1514/out/`):* the
      registered `run` (38 calls, every one `EXIT 0`), `collect`, then the gate calls. Outcome:
      **breach** (the decision table's second row). The setting stays opt-in, unset in both
      profiles, and the plugin does not offer it; the breaches are fixed in K15.19, and the run
      repeats at the fixed commit under an amendment that also names K15.20's fix.
      - *Validity:* (c) holds (amendment 2 before the run, pin test 8 passed in TREE). Left out
        by (b): `pocketsdr-v3-0`, both arms TERMINATED with no session: the plugin's snapshot
        wrote `v3.kicad_pcb` for the board `v3.0.kicad_pcb` (K15.20). 16 rows judged, the pcu row
        among them. (a): no `-off` arm logs the pour-guard line, and every `-on` arm that ran
        does; `loratracker-hardware-loratracker-hat` guards 0 pours (judged as it is).
      - *Guard (1) fails* on `wisweep-driver-board`, both arms (routing-caused, on top of the
        input's own pads JP3 1-2 and J1 1-2 within the edge clearance):
        - `pcb-off` (the plugin's default today): 1 `copper_edge_clearance`, a /TMC_AIN track on
          B.Cu (64.6891, 47.6156) -> (62.2652, 50.0395) that crosses the Edge.Cuts line x = 64.5
          (y 39.25-50.03) next to J1 pad 2;
        - `pcb-on`: 5 `copper_edge_clearance` (/TMC_REFL and /TMC_AIN tracks on F.Cu and B.Cu)
          and 2 `hole_clearance` (Net-(U5-VCP) tracks on F.Cu 0.2148 and 0.2247 mm from J10's
          NPTH at the board's 0.25 mm).

        Every other arm has 0 manufacturing breaches. All 36 graded arms have 0 below-minimum
        and 0 vias in pads.
      - *The gate:* `freerouting-dev gate` refuses this run. `--write-baseline` stops at
        pocketsdr's ungraded hard limits, refuses `--rows`, and refuses a records copy without
        the row ("registered but missing from the run"), so the registered pcb calls have no
        verdict (K15.21). The DSN calls ran: `dsn-off -> dsn-on` pcu 55 -> 31, canary-pwr
        38 -> 36 (the 3-board rule refuses N = 2; they decide nothing beyond guard (1), which
        holds there).
      - *For the record only* (by hand from `records.json` without pocketsdr, bands as the gate
        rounds them; "breach" decides before these): KiCad unconnected 368 -> 490 over the 16
        judged rows against a set noise of 67.09, so S fails. Guard (2) drc_real 17 -> 14 holds.
        Guard (3) fails on 4 rows (moteus 40 -> 67 against a band of 22, openmuscle 7 -> 28
        against 8, pcie-active-backplane 5 -> 83 against 14, wisweep 25 -> 42 against 11).
        Guard (4) route time 600.0 -> 698.9 s (x1.165) holds.

        | row | KiCad unc. off -> on | band | router inc. | drc_real | route s | breaches | pours |
        |---|---|---|---|---|---|---|---|
        | pcu-76163c7 | 47 -> 20 | 43 | 11 -> 20 | 1 -> 0 | 163.4 -> 158.0 | 0 -> 0 | 54 |
        | canary-pwr-95882f00 | 40 -> 36 | 10 | 27 -> 38 | 0 -> 0 | 92.1 -> 149.8 | 0 -> 0 | 16 |
        | interf_u | 17 -> 17 | 11 | 2 -> 2 | 1 -> 1 | 6.3 -> 5.8 | 0 -> 0 | 1 |
        | pic_programmer | 3 -> 3 | 7 | 0 -> 0 | 0 -> 0 | 0.6 -> 0.6 | 0 -> 0 | 1 |
        | kit-dev-coldfire-xilinx_5213 | 25 -> 21 | 28 | 4 -> 3 | 3 -> 0 | 122.1 -> 123.0 | 0 -> 0 | 3 |
        | complex_hierarchy | 81 -> 81 | 14 | 81 -> 81 | 0 -> 0 | 0.1 -> 0.2 | 0 -> 0 | 143 |
        | loratracker-hardware-loratracker-hat | 44 -> 44 | 12 | 46 -> 46 | 0 -> 0 | 57.2 -> 58.0 | 0 -> 0 | 0 |
        | m-fc-g474-rev-a-m-fc | 5 -> 15 | 14 | 3 -> 17 | 1 -> 2 | 24.4 -> 51.7 | 0 -> 0 | 3 |
        | macroprocessor-diode-matrix-rom-dip | 6 -> 6 | 8 | 5 -> 5 | 0 -> 0 | 0.7 -> 0.7 | 0 -> 0 | 1 |
        | moteus-moteus-c1 | 40 -> 67 | 22 | 30 -> 76 | 4 -> 1 | 61.8 -> 25.6 | 0 -> 0 | 51 |
        | nudac-nudac | 11 -> 16 | 9 | 5 -> 11 | 1 -> 0 | 10.3 -> 10.8 | 0 -> 0 | 2 |
        | openmuscle-flexgrid-om-flexgrid-rigid-pcb | 7 -> 28 | 8 | 1 -> 27 | 1 -> 0 | 7.2 -> 8.9 | 0 -> 0 | 2 |
        | pcie-active-backplane-pcie-active-backplane-x16-4layer | 5 -> 83 | 14 | 3 -> 83 | 0 -> 0 | 24.1 -> 87.6 | 0 -> 0 | 3 |
        | solarmeshtasticnode-meshtasticnode-bq25185 | 10 -> 9 | 6 | 10 -> 9 | 0 -> 0 | 6.0 -> 7.6 | 0 -> 0 | 2 |
        | vna-usb-pd | 2 -> 2 | 4 | 0 -> 2 | 0 -> 0 | 2.2 -> 0.9 | 0 -> 0 | 5 |
        | wisweep-driver-board | 25 -> 42 | 11 | 18 -> 42 | 5 -> 10 | 21.5 -> 9.7 | 1 -> 7 | 16 |

        The pcu row against `seen_before_registration`: off 47 and on 20, as K15.15's runs gave
        (router incomplete 11 and 20 there too). TREE's registration copy is amendment 1's
        (`e17e383a…`, as the gate prints): the commands are the same bytes.
    - *Amendment 3 (2026-10-21):* the engine is `4916e6d` (K15.19, with K15.20 and K15.21 before
      it), binary sha256 `5e78d925…`, pin test 8 passed in the tree, the seven hashes unchanged
      and K15.20's `snapshot.py` named, all 17 boards present as registered. The registration
      gains `must_judge: [pcu-76163c7]`, and the gate step reads the amended copy and takes
      `--left-out` (K15.21). Seen before the run: amendment 2's run, K15.20's pocketsdr run,
      K15.19's wisweep runs and K15.12's pcu runs.
    - *Run 2026-10-22 (amendment 3, engine `4916e6d`, `target/orchestrator/k1514/out/`):* the
      registered `run` (38 calls, every one `EXIT 0`), `collect`, then the gate calls as amended
      (`--prereg docs/roadmap/prereg/K15.14.json`, `25e8f26f…`, 3 amendments, 0 refusals).
      Outcome: **not kept, no gain** (the decision table's third row). The setting stays opt-in,
      unset in both profiles, and the plugin offers it as an option.
      - *Validity:* (c) holds. (b) leaves out `pocketsdr-v3-0` (`--left-out`): its `pcb-off` arm
        ended TIMED_OUT at the `--timeout 240` cap (KiCad 50 unconnected), `pcb-on` COMPLETED (115);
        both graded with hard limits 0. 16 rows judged, `pcu-76163c7` among them (`must_judge`).
        (a): no `-off` arm logs the pour-guard line and every `-on` arm does
        (`loratracker-hardware-loratracker-hat` guards 0 pours, judged as it is).
      - *Guard (1) holds:* 0 manufacturing breaches, 0 below-minimum and 0 vias in pads in all 38
        graded arms (K15.19's fixes: wisweep 1 and 7 breaches before).
      - *S fails* (`freerouting-dev gate`, `gate-verdict.log`): KiCad unconnected 368 -> 493
        (+125) over the 16 judged rows against a set noise of 67.09; `totals verdict: reject`;
        openmuscle (+21, band 8) and pcie-active-backplane (+78, band 14) worse than twice their
        band. `--all-pairs`: `pcb-off -> pcb-on` FAIL, the reported pair `pcb-on -> pcb-off` PASS.
        For the record (decided before them by the S row): guard (2) drc_real 16 -> 7 holds;
        guard (3) fails on 4 rows (moteus 40 -> 67 against 22, openmuscle, pcie-active-backplane,
        wisweep 25 -> 45 against 11); guard (4) route time 595.7 -> 688.8 s (x1.156) holds.
      - *DSN pair* (decides nothing beyond guard (1), which holds): pcu 55 -> 31, canary-pwr
        38 -> 36; the 3-board rule refuses N = 2.

        | row | KiCad unc. off -> on | band | router inc. | drc_real | route s | Zone-Zone | pours |
        |---|---|---|---|---|---|---|---|
        | pcu-76163c7 | 47 -> 20 | 43 | 11 -> 20 | 1 -> 0 | 163.3 -> 156.6 | 34 -> 6 | 54 |
        | canary-pwr-95882f00 | 40 -> 36 | 10 | 27 -> 38 | 0 -> 0 | 96.6 -> 153.8 | 18 -> 7 | 16 |
        | interf_u | 17 -> 17 | 11 | 2 -> 2 | 1 -> 1 | 5.7 -> 5.7 | 0 -> 0 | 1 |
        | pic_programmer | 3 -> 3 | 7 | 0 -> 0 | 0 -> 0 | 0.6 -> 0.6 | 0 -> 0 | 1 |
        | kit-dev-coldfire-xilinx_5213 | 25 -> 21 | 28 | 4 -> 3 | 3 -> 0 | 113.3 -> 113.8 | 0 -> 1 | 3 |
        | complex_hierarchy | 81 -> 81 | 14 | 81 -> 81 | 0 -> 0 | 0.1 -> 0.1 | 0 -> 0 | 143 |
        | loratracker-hardware-loratracker-hat | 44 -> 44 | 12 | 46 -> 46 | 0 -> 0 | 56.7 -> 56.9 | 0 -> 0 | 0 |
        | m-fc-g474-rev-a-m-fc | 5 -> 15 | 14 | 3 -> 17 | 1 -> 2 | 24.1 -> 51.5 | 2 -> 2 | 3 |
        | macroprocessor-diode-matrix-rom-dip | 6 -> 6 | 8 | 5 -> 5 | 0 -> 0 | 0.7 -> 0.7 | 0 -> 0 | 1 |
        | moteus-moteus-c1 | 40 -> 67 | 22 | 30 -> 76 | 4 -> 1 | 61.6 -> 25.5 | 8 -> 2 | 51 |
        | nudac-nudac | 11 -> 16 | 9 | 5 -> 11 | 1 -> 0 | 10.4 -> 10.8 | 0 -> 0 | 2 |
        | openmuscle-flexgrid-om-flexgrid-rigid-pcb | 7 -> 28 | 8 | 1 -> 27 | 1 -> 0 | 7.2 -> 8.9 | 0 -> 0 | 2 |
        | pcie-active-backplane-pcie-active-backplane-x16-4layer | 5 -> 83 | 14 | 3 -> 83 | 0 -> 0 | 24.2 -> 85.8 | 0 -> 0 | 3 |
        | pocketsdr-v3-0 | 50 -> 115 | left out | 42 -> 103 | 0 -> 0 | 240.1 -> 60.2 | 2 -> 6 | 4 |
        | solarmeshtasticnode-meshtasticnode-bq25185 | 10 -> 9 | 6 | 10 -> 9 | 0 -> 0 | 6.1 -> 7.8 | 0 -> 0 | 2 |
        | vna-usb-pd | 2 -> 2 | 4 | 0 -> 2 | 0 -> 0 | 2.3 -> 0.9 | 2 -> 0 | 5 |
        | wisweep-driver-board | 25 -> 45 | 11 | 20 -> 43 | 4 -> 3 | 22.8 -> 9.4 | 4 -> 4 | 16 |

      - The gain is the owner boards': pcu 47 -> 20 (Zone-Zone 34 -> 6) and canary-pwr 40 -> 36.
        On corpus boards whose pours are large (pcie-active-backplane's three, openmuscle's two),
        keeping every foreign net out of them leaves those nets unrouted.

  - *Done 2026-10-22:* the third Accept clause's "not kept" branch: the plugin offers the setting
    as an option. The decision table's row ("not kept, no gain") is in the run note above, with
    the per-board table; the pin test passes.
    - *Accept 1:* `cargo test -p freerouting-dev --test prereg_k15_14`: 8 passed (the registration's
      and the set's hashes, the commands, the scripts), at main `f7b0928` plus this change.
    - *Accept 2:* the per-board table and the decision are in the 2026-10-22 run note (16 rows
      judged, KiCad unconnected 368 -> 493 against a noise of 67.09, `totals verdict: reject`).
      DIVERGENCES' K15.13 entry carries the numbers.
    - *Accept 3 (not kept):* option `plane_obstacle_all_pours` in `kicad-plugin/freerouting_rs/
      options.py` (`RouteOptions` field, default off in both profiles; `to_args` adds
      `--router.plane_obstacle_all_pours=true` only when on; `from_dict`, `from_form`; no schema
      bump, schema stays 6, because `to_dict` leaves a default out and a file without the key
      means off) and a checkbox in `plugin.py` (Advanced tab, Flow group, "Keep other nets out of
      every pour", tooltip: "Can help boards with many pours on signal layers (the measured
      power-control board: KiCad unconnected 47 -> 20). Can leave nets unrouted on boards with
      large pours (pcie-active-backplane 5 -> 83)."). Tests: `test_options.py`
      `test_all_pours_is_off_under_both_profiles_and_passes_its_flag_only_when_on`,
      `test_all_pours_is_saved_per_board_and_restored_without_a_schema_bump` (every readable
      schema loads with it off), `test_the_form_sets_all_pours`; `test_route_dialog.py`
      `test_the_all_pours_checkbox_is_saved_and_reaches_the_router`; `test_snapshot.py`
      `test_every_k09_option_is_either_checked_or_allowed` lists it. RED: 5 failed (the field was
      missing); GREEN: the same 5 pass. `python3 -m pytest kicad-plugin/tests`: 681 passed, 6
      skipped (pcbnew x3, canary-pwr board, `KICAD_METADATA_REPO` x2), with the debug CLI of this
      change as `FREEROUTING_RS_BIN`. The registration's plugin-defaults test
      (`the_options_are_the_plugin_defaults`, `{"schema": 6}`) still passes: the plugin's default
      arguments are unchanged. `legacy` and `enhanced` leave the setting unset in the engine
      (no crate changed).

- [x] **K15.15 — A plane-layer zone under a higher-priority zone is not counted there.**
  - **Change:** on every pcu arm of K15.13 (`target/orchestrator/k1513/runs/`), KiCad reports 3
    V_BUS_12V vias dangling at (165.71, 44.32), (169.05, 47.44) and (173.72, 40.53) mm. Each lies
    where In2.Cu's V_BUS_24V zone (priority 41) overlaps V_BUS_12V's (priority 1). KiCad fills
    V_BUS_24V there; the router builds each plane from its zone's outline
    (`freerouting-kicad/src/zones.rs`, `make_plane`; the DSN export the same), so it counts the via
    as on V_BUS_12V's plane. Find what each input gives the router for overlapping zones of one
    layer (outline, fill, priority), and make a plane cover only what its zone's fill covers
    (`.kicad_pcb`: the zone's `filled_polygon`, which K14.4's `read_pour_fills` reads; or the
    outline minus every higher-priority zone of another net on the layer), on both inputs if
    the DSN path can know it.
  - **Accept:**
    - a test on a small board with two overlapping zones of different nets and priorities on an
      inner layer and a via of the lower one's net inside the overlap: the router counts it
      unconnected (or routes it), as KiCad does;
    - the pcu board again (a few minutes), graded by KiCad: 0 routing-caused `via_dangling`;
    - `legacy` unchanged unless Java at the pin does the same (say which); parity 7 of 7 if shared
      code changes.
  - *Done 2026-10-21:* the `.kicad_pcb` reader cuts each plane where a higher-priority zone of
    another net fills; the DSN path cannot know priorities (KiCad's export writes none).
    - *Cause:* the reader builds each plane from its zone's whole outline
      (`freerouting-kicad/src/zones.rs::make_plane`, `read.rs` plane loop), as KiCad's export does
      (`specctra_export.cpp` 1190-1280, `zone->IterateWithHoles()`, no priority written). The DSN
      reader inserts each plane on its own (`io/specctra/parser/structure.rs::insert_planes`), and
      `BasicBoard::conduction_area_normal_contacts` counts a drill centred in a tile as a contact;
      Java does the same at the pin (`Structure.java` 1141, `ConductionArea.getNormalContacts` 341).
      Probe on K15.13's `.kicad_pcb` off arm (routed board read back): each of the 3 vias has the
      contacts {its F.Cu trace, V_BUS_12V's plane on In2.Cu (outline, 0 windows)}; V_BUS_24V's
      plane covers the same points. KiCad's fill (`zone_filler.cpp`
      `buildDifferentNetZoneClearances`) knocks out every higher zone of another net
      (`ZONE::HigherPriority`: teardrop, then priority, then UUID) by its fill plus the clearance;
      in the input board V_BUS_12V's `filled_polygon` covers none of the 3 points.
    - *Fix:* `freerouting-kicad/src/zone_priority.rs` (new): each plane gets one convex window per
      pair of convex pieces of its outline and of a higher zone's outline (grown by the larger of
      the two zones' `connect_pads` clearances) that overlap, on a 100 nm grid. `Zone` reads
      `priority`, `uuid`, the teardrop `attr` and the clearance. The read report lists each cut
      plane. `ReadOptions::uncut_planes` keeps KiCad's whole outlines (used by the export
      comparisons in `tests/{copper,k14_fixups}.rs`). DIVERGENCES ("A plane is cut where a
      higher-priority zone of another net fills"). Engine code is unchanged.
    - *Accept 1:* fixture `freerouting-kicad/tests/data/k15-overlapping-planes.kicad_pcb` (the
      pcu's two In2.Cu zones verbatim without fills; J1, a V_BUS_12V pad on F.Cu with a track to a
      via at (165.71, 44.32) mm; J2, a V_BUS_12V THT pad at (200, 60) mm). KiCad 10.0.7
      `pcb drc --refill-zones`: 1 unconnected (J1's track and J2), the via dangling.
      `crates/freerouting/tests/k15_overlapping_planes.rs`:
      `the_via_in_the_overlap_is_not_on_the_lower_plane` (both profiles: no plane contact,
      incomplete 1), `the_via_in_the_overlap_stays_open` (routed, both profiles: incomplete 1),
      `the_router_reaches_the_lower_plane_where_it_is_filled` (J1's pad alone: `legacy` 0
      incomplete, its In2.Cu trace ends at x 178.37 mm, past V_BUS_24V's edge plus 0.8 mm;
      `enhanced` 1). `freerouting-kicad/tests/k15_zone_priority.rs` (6 tests) and the
      `zone_priority` unit tests (4, one at the 2 m coordinate limit). RED: 4 of the 6 reader
      tests and both engine tests failed ("legacy: the via counts on V_BUS_12V's plane"). GREEN:
      all pass.
    - *Accept 2:* release binary at `8b37947` plus this change,
      `target/orchestrator/k1515/scripts/arms.sh` (K15.13's command:
      `tools/kicad-reader/plugin-input-ab.sh kicad_pcb k154/stripped/... k154/opts-default.json
      <out> -- --max-passes 2 --timeout 240 [--router.plane_obstacle_all_pours=true]`,
      `ROUTE_TIMEOUT_S=300`, `ROUTE_MEMORY_KB=8000000`), graded by KiCad 10.0.7; runs in
      `target/orchestrator/k1515/runs/pcu-kicad_pcb-*`.

      | arm | KiCad unconnected | Zone-Zone (zones) | router incomplete | below-minimum | routing-caused | route s |
      |---|---|---|---|---|---|---|
      | off, K15.13 | 96 | 28 (23) | 65 | 0 | track_dangling 3, via_dangling 3 | 164.7 |
      | off, K15.15 | 47 | 36 (26) | 11 | 0 | track_dangling 1 | 174.9 |
      | on, K15.13 | 26 | 6 (6) | 23 | 0 | via_dangling 3 | 146.6 |
      | on, K15.15 | 20 | 6 (6) | 20 | 0 | none | 160.8 |

      0 `via_dangling` and 0 manufacturing breaches in both arms. The off arm's one
      `track_dangling` is `/CH6_NO` on F.Cu at (124.59, 108.10) mm (not V_BUS_12V). The pcu's read
      report cuts 28 planes, 27 of them outer-layer pours cut by neighbours' clearance or overlap.
      DSN arms not rerun: the DSN path is unchanged.
    - *Accept 3:* engine and DSN code are unchanged, so `legacy` on a DSN (Java's input) is
      unchanged:
      `PARITY_SET=tools/bench/sets/connection-parity.txt tools/parity-check.sh --no-probes`:
      PARITY OK, 7 of 7 exact. On a `.kicad_pcb` input the cut applies under both profiles; Java
      reads KiCad's export, which has whole outlines, so `legacy` on a `.kicad_pcb` with
      overlapping zones of different nets and priorities now differs from Java on KiCad's DSN.
    - *Found:* neither profile routes a connection that starts inside a plane's window or a
      zone's own hole (the fixture with V_BUS_24V's part drawn as a hole of V_BUS_12V's zone fails
      the same at this change, where the reader path is unchanged); `enhanced` cannot reach the
      plane from inside the window with no trace on a power layer.
    - *Open, outside this item:* whether `legacy` on a `.kicad_pcb` keeps the cut (as K15.9's
      reader additions do) or gets whole outlines, an operator decision; the plugin's DSN fix-up
      could cut the planes from pcbnew's priorities (kicad-plugin lane, no item yet).

- [x] **K15.16 — The report's split nets and islands as KiCad's zone fill joins them.**
  - **Change:** after K15.11 the KiCad-mode `-drc` report on the pcu board (`76163c7`) has 101
    `unconnected` and 4 `isolated_island_dead_copper` where KiCad 10.0.7 has 0 and 0. The 101 are
    31 split nets of Java's count (kept by K15.5's design) and 70 islands that KiCad's zone fill
    joins (V_BUS_24V 26, /BAT_VP 10, …); the 4 islands are U24-SW2, /BAT_VP, D11-C and /BUCK_6V,
    islands of the router's fill model (outline minus obstacles), where KiCad removes isolated
    islands (93 of 94 zones have `island_removal_mode 0`). Under `ReportRules::Kicad`, count
    connectivity through a pour as KiCad's refilled zone gives it (the `.kicad_pcb`'s
    `filled_polygon`, which K14.4 reads, where it is current; else the router's fill model), and
    report an island only where KiCad would keep it. Say what the DSN input can do without the
    fills.
  - **Accept:**
    - on the pcu board copy, `unconnected` and the island findings equal KiCad's or differ by named
      items, with the before/after table;
    - tests on small boards: two pads joined only by a pour's fill (connected), a pour cut in two
      by another net's trace (two pieces, as KiCad);
    - `ReportRules::Java` and the router's score unchanged; parity 7 of 7 if shared code changes.
  - *Done 2026-10-21:* under `ReportRules::Kicad` the split nets and zone islands are KiCad 10's
    connectivity (`drc/kicad_connectivity.rs`, new); `ReportRules::Java`, `check()`,
    `all_unconnected_items()` and the scores are unchanged.
    - *Design:* zone copper is the fill, one node per island. The `.kicad_pcb`'s `filled_polygon`s
      (read with the board: `freerouting-kicad/src/zone_fills.rs`, new, `Extras::zone_fills`;
      `KicadReportRules::zones`) count while they are current: every zone filled (`fill yes` and at
      least one `filled_polygon`) and the board's `state_hash` equal to the one recorded when the
      rules were put on it (`KicadReportRules::fills_current`), so `-drc` on a `.kicad_pcb`. K14.4's
      `read_pour_fills` is not used: its hash check guards the file, not the board. Otherwise
      (a DSN, a route's `.drc`, an edited board) the router's fill model: each conduction area's
      detailed fill (`fill_islands`), a pad or via within the clearance of an island joining it
      (the model leaves that gap and its spokes may not reach the fill), and a dropped pour's
      outline. A DSN carries no fill, so the fill model is what a DSN input counts with, and its
      zones take KiCad's default `island_removal_mode` 0 (module docs). Items join where their own
      copper touches (`CN_VISITOR`), a pad or via joins an island it overlaps, a trace one that
      contains an end of one of its segments. An island whose group has no pin is isolated and
      removed per `island_removal_mode` (0 always, 1 never, 2 below `island_area_min`) unless every
      island of its zone on that layer is isolated: KiCad 10.0.7 then keeps them all (boards a and
      d2 below). A kept one is an `isolated_island_dead_copper` (Java's type; KiCad's
      `isolated_copper`); no `isolated_island_unconnected`. A net of `n` groups gives `n - 1`
      `unconnectedItems` (KiCad's ratsnest edges), a kept isolated island alone being no group.
    - *pcu board* (copy of `../power-control-unit` `76163c7`, sha256 `dde7bb00…`, in
      `target/orchestrator/k1516/board/`; release `-de Power_Control_Unit_24V.kicad_pcb -drc`;
      KiCad 10.0.7 from K15.11's note; runs in `target/orchestrator/k1516/runs/`):

      | Type | before (main `b8bbe78`) | after | KiCad 10.0.7 |
      |---|---|---|---|
      | unconnected | 74 (31 `unconnectedItems`, 43 `isolated_island_unconnected`) | 0 | 0 |
      | isolated_island_dead_copper | 5 | 0 | `isolated_copper` 0 |
      | hole_clearance | 4 | 4 | 4 |
      | same board, its 86 `filled_polygon`s stripped (the fill model) | 74 / 5 | 0 / 0 | - |

      - Before, after K15.15 (not K15.11's 101 and 4): the 31 split nets are 25 nets with no zone
        (SCL, VCC_3V3, /FLT_INT, Net-(D5-A), ...: traces that end on another trace's middle or in
        a pad off its centre, which Java's contacts do not join), 3 nets whose F.Cu pour the DSN
        reader drops (BUS_GND, V_BUS_12V, V_BUS_24V) and 3 with signal-layer pours (/BAT_VP,
        Net-(Q1-D), USB_GND); the 43 islands are fill-model islands (/BAT_VP 10, USB_GND 4,
        Net-(U24-SW2) 3, Net-(D11-C) 3, ...); the 5 dead islands U24-SW2, USB_GND, /BAT_VP,
        /CH2_NO, /BUCK_6V. After: no remaining difference.
      - Time: 0.14-0.15 s before, 0.20-0.21 s after (3 runs each, with the file's fills and
        without).
    - *Two more boards against KiCad:* `fixtures/kicad/Issue558-dev-board-autoroute-demo`'s
      unrouted `dev-board.kicad_pcb`: `unconnectedItems` 45 -> 94, KiCad 10.0.7 `kicad-cli pcb drc`
      94 (its DSN export gives 94 too); Issue575 Natural Tone Preamp (DSN, fill model): 44 -> 7,
      the 7 nets of KiCad 9.0.1's report in Java's fixtures.
    - *Tests:* `crates/freerouting/tests/drc_kicad_fills.rs` (5 pass), boards
      `tests/data/kicad/pcu-k1516/` written by `make_boards.py` there on the pcu board's 4-layer
      stack, each as written (`-unfilled`, the fill model) and as `kicad-cli pcb drc --severity-all
      --format json --refill-zones --save-board` (KiCad 10.0.7) saved it (KiCad's fill); every
      expected count is that KiCad run's:
      `two_pads_joined_only_by_a_pour_are_connected` (a: 0 unconnected, 1 island, the In2.Cu
      plane; Java 1 split net), `a_pour_cut_in_two_is_one_unconnected` (b: 1 `unconnectedItems`
      Pin [N1] to Pin [N1], no island; Java 0 split, 1 `isolated_island_unconnected`, 2 dead),
      `an_isolated_island_is_reported_as_its_zone_keeps_it` (c: mode 0 -> 0, mode 1 -> 1, mode 2
      with `island_area_min` 10 -> 1 and 200 -> 0), `a_zone_whose_islands_are_all_isolated_keeps_them`
      (d2: 2; d3: 0), `the_files_fill_counts_only_while_the_board_is_unchanged` (b with N2's trace
      removed: the fill model, N1 in one piece, N2 split). RED: main's release binary gives a 1
      unconnected / 0 islands, b 1 `isolated_island_unconnected` + 2 dead, c mode 0, 1 and both
      mode-2 boards 2 dead each, d2 1 split + 2 dead, d3 3 dead, with fills and without; GREEN:
      all equal KiCad's on both variants.
    - *Changed expectations:* `tests/drc.rs::issue575_unconnected_items` (KiCad-mode split nets 44
      -> the 7 KiCad nets; Java's 44, 109, 4 unchanged); `tests/drc_kicad_report.rs::kicad_report_counts_different_nets_and_copper_overlap`
      (Java's 2 split nets GND and SIG, KiCad's 0); `freerouting-cli/tests/cli_kicad.rs::dev_board_report_equals_the_dsn_export`
      (4 + 94 findings).
    - *Java and score unchanged:* `tests/reference_drc.rs` and `tests/drc.rs` pass (Java's report
      and scores). Parity: `PARITY_SET=tools/bench/sets/connection-parity.txt
      tools/parity-check.sh --no-probes`: PARITY OK, 7 of 7 `[exact]`, `bytes true` (37 s).
    - *Gate:* `cargo fmt --all --check` clean; `cargo clippy --workspace --all-targets -- -D
      warnings` clean; `cargo test --workspace --no-fail-fast`: 2945 passed, 1 failed (the
      dev-board count above, then updated: `cli_kicad` 10 passed); plugin `pytest -rs
      kicad-plugin/tests` with the new binary: 674 passed, 6 skipped (pcbnew, canary-pwr,
      KICAD_METADATA_REPO only).
    - DIVERGENCES: the "The report counts what KiCad counts" entry is extended with K15.16;
      `docs/development/engine.md` (the `-drc` row).
    - *Not modelled:* KiCad's thermal relief and pad zone-connection settings, zones that
      footprints own, via flashing, the fill model's minimum-thickness opening.

- [x] **K15.17 — `-drc` on a DSN takes `--kicad-dru`.**
  - **Change:** `-drc` on a DSN input ignores `--kicad-dru` with exit 0 and no message (found by
    K15.11's agent, 2026-10-21). Apply the rules to the report as a routing job's DRC does
    (`drc::KicadReportInputs::apply_rules`), or refuse the flag with an error naming it; say which
    and why.
  - **Accept:** a CLI test on a DSN with `--kicad-dru`: the report counts with the rule's clearance
    (K15.11's `f-rule-clearance` case through KiCad's DSN export, or a DSN made for it), or the
    run fails with the message; the help text says what `-drc` takes.
  - *Done 2026-10-21:* applied, not refused. `DrcRunOptions::kicad_dru` (`drc/runner.rs`,
    `kicad_dru_rules`) evaluates the file on a copy of the DSN's tree (`dru::apply_to_pcb`, as
    `prepare_job`) and gives the result to the report alone (`KicadReportInputs::apply_rules`, then
    `on_board`, `set_kicad_report_rules`). It changes no clearance matrix or item, so the board
    `-drc` checks is the DSN's and nothing needed refusing. `DrcRun::warnings` is new (the CLI
    prints it as `warning:` lines).
    - Tests, `crates/freerouting-cli/tests/cli_drc_kicad_dru.rs` (5 pass; RED before the change:
      all 5 failed, the flag ignored with exit 0 and no message). The DSN is hand-written
      (two 0.8 mm pads of different nets 0.1670 mm apart, class clearance 0.2 mm; KiCad's DSN
      export of `f-rule-clearance` needs KiCad and was not used), the `.kicad_dru` is the pcu
      board's `clearance (min 0.127mm)`:
      `the_rules_clearance_is_what_the_dsn_report_counts` (without the flag 1 `clearance`,
      "expected: 0.2000 mm, actual: 0.1670 mm"; with it 0; `unconnectedItems` equal),
      `a_rule_above_the_class_clearance_is_what_the_report_counts` (rule 0.3 mm: 1 `clearance`
      at 0.3000 mm), `a_kicad_dru_that_does_not_parse_is_the_routing_jobs_warning` (one
      `warning: the .kicad_dru file could not be read: ...`, equal to the route's line for the
      same file; report as without the flag), `a_missing_kicad_dru_is_refused_as_the_route_refuses_it`
      (exit 1, `Couldn't read the --kicad-dru file 'nothing.kicad_dru'`),
      `on_a_kicad_pcb_the_flag_is_a_warning_and_the_boards_own_rules_count` (`f-rule-clearance`:
      `warning: --kicad-dru only applies to a DSN input; a .kicad_pcb reads the .kicad_dru next to
      it`; 0 `clearance` with and without the flag, the board's own rule).
    - Help text (`help.txt`, `-drc` and `--kicad-dru`), `docs/development/engine.md` (the `-drc`
      row) and DIVERGENCES (the K15.11 entry names `-drc`) say what `-drc` takes.
    - Gate: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`
      clean; `cargo test -p freerouting-cli -p freerouting`: all pass. The plugin calls no `-drc`,
      so no pytest run.

- [x] **K15.18 — Under `enhanced` a via to a cut plane lands where the plane is filled.**
  - **Change:** after K15.15 a plane has windows where a higher-priority zone of another net fills.
    On K15.15's fixture (`crates/freerouting-kicad/tests/data/k15-overlapping-planes.kicad_pcb`, J1's
    pad alone), `legacy` reaches V_BUS_12V's own part of In2.Cu with a trace on the power layer
    (Java's rule) and routes 0 open; `enhanced` lays no trace on a power layer
    (`router.power_layer_traces=none`, Q13), its fan-out via lands in the window, and the
    connection stays open (`tests/k15_overlapping_planes.rs::the_router_reaches_the_lower_plane_where_it_is_filled`).
    Find where `enhanced` chooses the via's place for a plane connection, and make it a place
    where the via's own plane is filled (outside every window), or route the trace on a signal
    layer to such a place.
  - **Accept:**
    - the fixture routes 0 open under `enhanced`, with the via outside V_BUS_24V's window, and the
      test's expected value changes with a note;
    - the pcu board's `.kicad_pcb` route (K15.15's command), graded by KiCad: unconnected not up,
      0 below-minimum, 0 routing-caused DRC;
    - `legacy` unchanged (parity 7 of 7 if shared code changes).
  - *Partial 2026-10-21:* accept 1 and 3 hold; accept 2 holds except "0 routing-caused DRC": the
    pcu route is byte-identical to main's and keeps K15.15's one `track_dangling` (`/CH6_NO`, not
    a plane via; remainder below).
    - *Cause:* not the via's place. `enhanced` routes J1 on F.Cu to a via at (177.389, 44.9141) mm
      in the window, on B.Cu to a via at (180.347, 44.9141) mm in V_BUS_12V's plane (a zero-length
      hop there, `trace_stays_in_own_plane`, `autoroute/session/search.rs`), and the insert counts
      as routed. The pull-tight after the connection (`pull_tight_after_connection`,
      `autoroute/batch/connection.rs`; `opt_changed_area`, `optimize/pull_tight.rs`) then moves the
      first via, a via between two traces (`opt_via_location`, `optimize/via.rs`), along the B.Cu
      trace to (179.5151, 44.9141) mm, inside V_BUS_12V's plane. The drill move inserts the B.Cu
      stub first (`move_drill_item`, `board/item_ops.rs`, layers descending), which closes a cycle
      via - B.Cu - via - plane, and `remove_if_cycle` (`board/trace_ops.rs`) removes the cycle's
      whole connection, both vias and the B.Cu trace; the F.Cu trace is left dangling. The same
      happens in all 3 passes; the stub cleanup removes the leftovers, and the fanout's via in the
      window (165.489, 44.914 mm, `fanout_pin.rs`'s escape) is a stub too. `legacy` lays a trace on
      In2.Cu instead and never gets there.
    - *Fix:* under `router.power_layer_traces = own_plane | none` (unset in `legacy`; `none` in
      `enhanced`'s table, which is unchanged) the via optimizer does not move a via between two
      traces onto a conduction area of its net that it does not touch, on a layer neither trace is
      on (`RoutingBoard::opt_via_location_keeping_planes`, `via_would_join_a_plane`;
      `PullTightParams::keep_vias_off_new_planes`, set by `pull_tight_after_connection` (now given
      the mode), the pass's tail removal (`PassRunner::power_layer_traces`), the fanout and the
      bounded neck-down; `PowerLayerTraces::keeps_vias_off_new_planes`). A plane on a trace's layer
      still takes the via (Q13's fixtures: the move replaces the In1.Cu trace into PB's island; a
      first version without that condition failed
      `tests/power_layer_traces.rs::{own_plane_keeps_every_trace_inside_its_island,
      none_lays_no_trace_on_the_power_layer_and_still_lands_the_via}`, "PB on In1.Cu").
      DIVERGENCES (Autorouter, `power_layer_traces`, the K15.18 lines); module docs of
      `optimize/via.rs` and `autoroute/power_layer_traces.rs`.
    - *Accept 1:* `crates/freerouting/tests/k15_overlapping_planes.rs`:
      `the_router_reaches_the_lower_plane_where_it_is_filled` now expects `enhanced` 0 open (was 1,
      with the note why), no trace on In2.Cu, one via on V_BUS_12V's plane at (180.347, 44.9141)
      mm, x >= 177.575 + 0.8 (V_BUS_24V's right edge grown by the 0.8 mm clearance) and y in
      31.9-52.45; `legacy` 0 open, its In2.Cu trace ending past that edge and its via at
      (166.4755, 44.5428) mm as before. `the_via_in_the_overlap_stays_open` is now
      `the_via_in_the_overlap_stays_open_under_legacy`: `legacy` 1 open as before, `enhanced` 0
      (was 1): J1 reaches the plane right of the window the same way, the input via at (165.71,
      44.32) mm stays off the plane. RED before the fix: the first test failed at `enhanced`'s
      count (left 1, right 0); GREEN after: 3 passed. Release CLI on the fixture without the track
      and via (`-de k15-j1-alone.kicad_pcb --max-passes 3`, runs in
      `target/orchestrator/k1518/runs/fixture/`): main `a91e46b` "1 unrouted", this change "0
      unrouted".
    - *Accept 2:* `target/orchestrator/k1518/scripts/arms.sh` (K15.15's off-arm command:
      `tools/kicad-reader/plugin-input-ab.sh kicad_pcb k154/stripped/Power_Control_Unit_24V.kicad_pcb
      k154/opts-default.json <out> -- --max-passes 2 --timeout 240`, `ROUTE_TIMEOUT_S=300`,
      `ROUTE_MEMORY_KB=8000000`), graded by KiCad 10.0.7; runs in `target/orchestrator/k1518/runs/`:

      | arm | KiCad unconnected | Zone-Zone (zones) | router incomplete | below-minimum | manufacturing | routing-caused | route s | session |
      |---|---|---|---|---|---|---|---|---|
      | off, K15.15 | 47 | 36 (26) | 11 | 0 | 0 | track_dangling 1 | 174.9 | `a2d99014…` |
      | off, main `a91e46b` | 47 | 35 (25) | 11 | 0 | 0 | track_dangling 1 | 163.2 | `a2d99014…` |
      | off, K15.18 | 47 | 34 (24) | 11 | 0 | 0 | track_dangling 1 | 165.5 | `a2d99014…` |

      The session is byte-identical in the three runs (the guard never acts on this board; the
      Zone-Zone count differs between KiCad gradings of the same session). *Remains:* the
      `track_dangling` is `/CH6_NO` on F.Cu, a 0.5439 mm track from a via at (125.0442, 107.7107)
      mm ending at (124.59, 108.0953) mm inside zone EFUSE_NO_6's outline (F.Cu, priority 43) but
      outside its three `filled_polygon`s (the nearest fill ends at x 123.8264 mm there): the
      router counts the end as touching the pour's outline where KiCad's fill keeps clear of other
      copper. Not a plane via and not K15.18's change; it needs its own item (a trace end on a pour
      lands where KiCad's fill reaches; no checklist owns it yet).
    - *Accept 3:* the change acts only under `own_plane | none`, which `legacy` leaves unset
      (`layer`): `PARITY_SET=tools/bench/sets/connection-parity.txt tools/parity-check.sh
      --no-probes`: PARITY OK, 7 of 7 `[exact]`, `bytes true`; the fixture's `legacy` route is
      unchanged (asserted above).
    - *Gate:* `cargo fmt --all --check` clean; `cargo clippy --workspace --all-targets -- -D
      warnings` clean; `cargo test --workspace --no-fail-fast`: 2946 passed, 0 failed, 56 ignored.

  - *Done 2026-10-22:* accept 2's last clause holds on K15.23's run
    (`target/orchestrator/k1523/runs/pcu-k1523-kicad_pcb-off`: K15.18's off-arm command with
    K15.23's release binary, which carries this change; graded by KiCad 10.0.7): 47 unconnected,
    0 below-minimum, 0 manufacturing breaches, 0 routing-caused DRC (the `/CH6_NO`
    `track_dangling` is gone: K15.23's cleanup removes the stub and the output check's retry
    routes the connection into EFUSE_NO_6's fill). Accept 1 and 3 as above.

- [x] **K15.19 — No routed track crosses the board edge or an NPTH's hole clearance on wisweep.**
  - **Change:** K15.14's run (`target/orchestrator/k1514/out/wisweep-driver-board/`, engine
    `b8bbe78`, the corpus board `wisweep-driver-board` stripped, plugin default options,
    `--max-passes 2 --timeout 240`) breaks the board's rules with routed copper. Under the
    plugin's default (`pcb-off`), a /TMC_AIN track on B.Cu, (64.6891, 47.6156) -> (62.2652,
    50.0395), crosses the Edge.Cuts line x = 64.5 (y 39.25-50.03) beside J1 pad 2 (65.7956,
    47.6156). With `--router.plane_obstacle_all_pours=true` (`pcb-on`) there are 5
    `copper_edge_clearance` (/TMC_REFL, /TMC_AIN) and 2 `hole_clearance` (Net-(U5-VCP) on F.Cu
    0.2148 and 0.2247 mm from J10's NPTH, against `min_hole_clearance` 0.25 mm). Find why the
    router's board-edge and hole keep-outs let these tracks through. Check whether J1's and JP3's
    pads lie outside the outline (KiCad flags the pads themselves too), and whether the router
    sees that edge segment. A connection that cannot be made inside the rules stays open (decided
    2026-10-20: routing never breaks the board's manufacturing minimums).
  - **Accept:**
    - a test on a small board cut from wisweep (named in the test): the track crossing the
      outline, then no track crossing it (the connection routed inside the edge clearance, or
      left open);
    - a test for the NPTH hole clearance: no routed track nearer than `min_hole_clearance` to an
      NPTH;
    - wisweep both arms again (the K15.14 commands, a minute each), graded by KiCad: 0
      routing-caused `copper_edge_clearance` and `hole_clearance`, 0 below-minimum;
    - `legacy` unchanged unless Java does the same (say which); parity 7 of 7 if shared code
      changes.
  - *Done 2026-10-21* (branch from main `a91e46b`). Two causes:
    - *The edge.* J1's pads (4 x 1.4 mm, centres x = 65.7956) and JP3's lie across the outline's
      notch line x = 64.5 (Edge.Cuts y 39.25-50.03; the board's outline goes round the notch,
      the router's outline has it). J1's pad centres are outside the board, so KiCad flags the
      pads themselves (the input's 4 `copper_edge_clearance`). `min_copper_edge_clearance` is 0
      (unknown), so the 250 µm default is capped to 0 ("pin J1-2 is 0.0 um from the board
      outline"), but the outline was still an obstacle. Java at the pin (`BoardOutline.blocksNets`,
      `isTraceObstacle`, from `e79ed32ea`) makes the outline no obstacle for every trace of a net
      with a pin on or outside it, anywhere on the board: /TMC_AIN (J1-2) and /TMC_REFL (JP3-2)
      crossed it, also at the bottom chamfer and the arc (`pcb-on`). Q20's output check found 0
      because it asks the same exemption. Fix: `router.java_quirks.edge_pin_net_crosses_outline`
      (`basic_board.rs`, `is_trace_obstacle` and `outline_blocks_nets`; `true` unset and in
      `legacy` is Java), `false` in `ENHANCED` as a correctness rule (decided 2026-10-20). A
      trace ends at a pin's centre, so J1-1 and J1-2 can't be reached inside the board and stay
      open.
    - *The hole.* J10's NPTH at (45.4516, 63.6636) is a copper-less oval slot (pad and drill
      0.95 x 0.65 mm). KiCad's export, and the reader, make it a pin whose pad is the slot, and
      a padstack carries no pad drill: `Padstack::get_drill_radius` estimates 0.45 x 0.325 =
      0.146 mm, so `drillHoleClearanceDelta` added nothing to the 0.2 mm copper clearance, and
      Q20's `hole_clearance` measured the same estimate. Fix: a `.kicad_pcb` job gives each
      pin on more than one layer its real hole (`board/pad_holes.rs`,
      `BasicBoard::set_pad_holes`, `DrillData::hole`, matched by centre, a slot as a segment),
      used by the search trees' hole inflation and Q20's check (`drc/board_rules.rs`, exact for
      slots), under both profiles; a DSN board is unchanged. Also the KiCad-mode report counted
      that pin's own pad against its own hole (4 findings of 0 mm): `drc/kicad_report.rs` skips
      the pin at an NPTH's centre.
    - *Accept 1:* `crates/freerouting/tests/k15_wisweep_edge_hole.rs::no_routed_trace_crosses_the_outline_beside_a_pin_on_the_edge`
      on `tests/data/kicad/k1519/wisweep-j1-edge.kicad_pcb` (wisweep's outline with J1, JP3,
      U5, C24, R6, R8 and J2): `legacy` crosses with /TMC_AIN and GND and routes all;
      `enhanced` crosses nowhere and leaves 2 open (J1-1, J1-2); `enhanced` with the switch
      `true` crosses again. RED at `a91e46b` (the test without the switch's third part):
      `enhanced` gave `[("GND", -0.125), ("/TMC_AIN", -0.125)]`.
    - *Accept 2:* `no_routed_track_comes_nearer_than_the_hole_clearance_to_an_npth` on
      `wisweep-j10-slot.kicad_pcb` (wisweep's outline, J10's slot, test points on SIG at x =
      44.6316): RED at `a91e46b` "legacy: SIG is 0.2200 mm from J10's slot"; GREEN in both
      profiles (0.2919 mm by the CLI route, `--router.hole_clearance_um=250`).
      `an_npth_slots_own_pad_is_no_hole_clearance`: RED 4 findings, GREEN 0.
    - *Accept 3:* the K15.14 commands with this tree's `plugin-input-ab.sh` and its release
      binary (sha256 `126fcc67…`), the board copied byte for byte to
      `target/orchestrator/k1519/boards/`, output `target/orchestrator/k1519/runs/wisweep-pcb-{off,on}/`,
      KiCad 10.0.7. These numbers are seen before K15.14's repeat run.

      | arm | KiCad unc. (K15.14 -> now) | routing-caused edge / hole | manufacturing breaches | below-minimum | route s |
      |---|---|---|---|---|---|
      | `pcb-off` | 25 -> 27 | 1 / 0 -> 0 / 0 | 1 -> 0 | 0 -> 0 | 21.5 -> 22.8 |
      | `pcb-on` | 42 -> 45 | 5 / 2 -> 0 / 0 | 7 -> 0 | 0 -> 0 | 9.7 -> 9.3 |

      The remaining 4 `copper_edge_clearance` in both arms are the input's J1 and JP3 pads.
      J1-2 is among `pcb-off`'s new unconnected items; the routes differ elsewhere too (the real
      holes change the tree shapes). Routing-caused items left: `silk_edge_clearance` 2 and
      `track_dangling` 2 / 1, as in K15.14. Q20's output check found 0 in both arms.
    - *Accept 4:* the edge exemption is Java's (`legacy` keeps it, `true` unset); the real holes
      change only `.kicad_pcb` jobs, which Java has no reader for (on a DSN Java and the port
      both estimate). Parity:
      `PARITY_SET=tools/bench/sets/connection-parity.txt tools/parity-check.sh --no-probes`:
      `PARITY OK`, 7 of 7 `[exact]`, `bytes true`.
    - DIVERGENCES: "A pin's hole clearance is kept from its real hole" (freerouting-kicad), the
      report's NPTH own pad (K15.11's entry), "K15.19 edge-pin nets cross the outline" and its
      `enhanced` line. The promoted-switch counts in `settings_java_quirks.rs`,
      `settings_profile.rs`, `freerouting-cli/tests/profiles.rs` and
      `freerouting-gui/tests/ui_load_quirks.rs` are 38; `freerouting-dev/tests/prereg_n06_13.rs`
      has the switch in `LATER_ROUNDS` (the orchestrator applied the implementer's patch: 5 passed).
    - The desktop app gives the pins the same holes: `KicadReportInputs::set_pad_holes`
      (`drc/kicad_rules.rs`) is what `prepare_job` and `freerouting-gui/src/session.rs` both call.
      `freerouting-gui/tests/ui_kicad_settings.rs::the_app_board_has_the_clis_pad_holes`
      (`wisweep-j10-slot`): RED before (the app's pins had no hole, the CLI's had J10's slot at
      radius 3250 and half 1500 board units), GREEN after.
    - The joining of `edge_pin_net_crosses_outline = false` to `ENHANCED` was decided 2026-10-21
      as a correctness rule (as `output_check` and `board_minimums.from_classes` were), its cost
      measured by K15.14's repeat run, where both arms carry it.

- [x] **K15.20 — The plugin's snapshot keeps a board name that has a dot in it.**
  - **Change:** `snapshot.py::snapshot_files` builds each file name with
    `(work / stem).with_suffix(...)`, so for the board `v3.0.kicad_pcb` (stem `v3.0`) it writes
    `v3.kicad_pcb`, `v3.kicad_pro` and `v3.kicad_dru`. The rest of the flow looks for
    `v3.0.kicad_pcb` (`design_path`, `headless_run.py::snapshot_mode`, `save_arguments`) and the
    route fails with no session (K15.14's `pocketsdr-v3-0` row, both arms, `prepare.log`). Every
    `.kicad_pcb` route of such a board fails, and `.kicad_pcb` is the plugin's default input.
    Name the files `stem + suffix`.
  - **Accept:**
    - a plugin test with a dotted stem: the snapshot's files are `<stem>.kicad_pcb`,
      `.kicad_pro`, `.kicad_dru` and `.kicad_prl`, and `design_path` names the same board;
    - `pocketsdr-v3-0`'s `pcb-off` command again: a session, graded;
    - `python3 -m pytest kicad-plugin/tests` with the router dir: no new skip.
  - *Done 2026-10-21:* `snapshot.py::snapshot_files` appends each suffix to the stem
    (`work / (stem + ".kicad_pcb")` and so on); the other `with_suffix` calls in the plugin
    (`export_flow.py` `kicad_dru_args`, `save_arguments`) act on a full file name and were right.
    - Test `kicad-plugin/tests/test_snapshot.py::test_a_dot_in_the_board_name_stays_in_the_snapshots_files`
      (stems `v3.0`, `ça va.ok`, `board.rev2.1`: the four files are `<stem>.<suffix>` and the board
      is `design_path`'s): RED before the change (3 failed), GREEN after.
    - `pocketsdr-v3-0`'s `pcb-off` command again (this tree's `plugin-input-ab.sh` and plugin, the
      `b8bbe78` release engine, output `target/orchestrator/k1520/pocketsdr-pcb-off/`): `EXIT 0`,
      work files `v3.0.kicad_pcb`, `.kicad_pro`, `.kicad_dru`, `.args.json`, `.ses`; graded: KiCad
      unconnected 50 (input 322), 0 routing-caused items, 0 manufacturing breaches, 0
      below-minimum, 0 vias in pads, route 240.1 s. K15.14's amendment 3 lists this run as seen.
    - `python3 -m pytest kicad-plugin/tests` with `FREEROUTING_RS_BIN` the debug build dir: 677
      passed, 6 skipped (pcbnew 3, the canary-pwr board 1, `KICAD_METADATA_REPO` 2), as before.

- [x] **K15.21 — The gate judges a registered run with a row left out by validity.**
  - **Change:** K15.14's validity rule (b) leaves out a row whose arms don't all complete and
    judges the rest, and its `heavy_run` adds `--rows` to the gate calls for that. But
    `freerouting-dev gate` refuses such a run whichever way it is called: with the row,
    `--write-baseline` stops at its ungraded hard limits (`UNGRADED … null, but a hard limit must
    be measured`); `--rows` with `--write-baseline` is refused (`refuses a partial run`); and a
    records copy without the row is refused (`board … is registered but missing from the run`).
    Give the gate a way to judge the rows a registration's validity rule leaves in: for example
    `--left-out ROW=REASON`, recorded in the baseline and printed with the verdict, where every
    other registered row must still be present and graded.
  - **Accept:**
    - gate tests (`freerouting-dev`): a run with one row's arms TERMINATED, judged over the rest
      with `--left-out`, and the row and its reason in the output and the baseline; a missing row
      not named is still refused, and a left-out row can't make fewer rows than the
      registration's `min_boards`;
    - K15.14's `heavy_run` gate calls work as written with the flag. Amend the registration only
      when its repeat run needs it (amendment 3).
  - *Done 2026-10-21:* `freerouting-dev gate --left-out ROW=REASON` (repeatable; `gate_cli.rs`,
    `gate.rs`, `gate_prereg.rs`, `gate_baseline.rs`; usage text, `tools/ab/README.md`,
    `routing-quality.md`).
    - *Rules:* needs `--prereg` and a registration with a non-empty `validity` text (else exit 2);
      the text is only checked to exist, the operator names the rows and reasons; the row must be
      one of `boards.files`, with a reason, given once, and not in `--rows`/`--eligible`. Every
      other registered row must be present and graded. A left-out row may be missing, ungraded
      or null; a measured hard limit above 0 in any of its arms is still `HARD`. Judged rows
      below the registration's `min_boards` (caused by the left-out rows), or a left-out row of
      the new optional `must_judge` array, give `gate: NOT MEASURED` (exit 1), before `FAIL`.
      `--write-baseline` records the judged rows plus `left_out` (row to reason) and refuses
      where the verdict would be NOT MEASURED; a baseline with other left-out rows gives `DRIFT
      left_out`. The verdict, each `--all-pairs` pair and the pair calls print the row, reason
      and what the gate saw per arm.
    - *RED:* `cargo test -p freerouting-dev --test gate_left_out` before the flag: 6 failed
      (`unknown option --left-out`). *GREEN:* 7 passed:
      `a_terminated_row_is_judged_over_the_rest_and_named_in_output_and_baseline`,
      `a_missing_row_that_is_not_named_is_still_refused`,
      `left_out_rows_cannot_take_the_run_below_min_boards`,
      `a_left_out_row_the_registration_requires_judged_is_not_measured`,
      `a_graded_left_out_row_with_a_breach_still_fails`,
      `the_flag_is_refused_where_it_cannot_apply`,
      `the_totals_rule_sums_noise_over_the_judged_rows_only`; the gate's self-test has 4 more
      checks (`freerouting-dev gate --self-test`: ok). `cargo test -p freerouting-dev`: 342
      passed, 0 failed (every `prereg_*` pin test, `prereg_k15_14` 8 passed).
    - *K15.14's records* (copy of `target/orchestrator/k1514/out/records.json`, this tree's gate,
      TREE's registration `e17e383a…`), `--left-out 'pocketsdr-v3-0=both arms TERMINATED …'`:
      `--write-baseline` wrote 16 rows plus `left_out`; the verdict and `--all-pairs` judged 16 of
      17 rows, total 368 -> 490 against set noise 67.09 (S fails), `HARD
      wisweep-driver-board.pcb-on.manufacturing_breaches = 7` (and `pcb-off … = 1` in the
      reported pair), exit 1. The two DSN calls need no flag (pocketsdr has no DSN arms) and
      give the N = 2 refusal as before. Output: `target/orchestrator/k1521/out/`.
    - Amendment 3 is the orchestrator's: it should add `--left-out` to `heavy_run`'s baseline and
      `--all-pairs` calls in place of "adds --rows", and `"must_judge": ["pcu-76163c7"]` (the
      registration's `validity` text already satisfies the flag; without `must_judge` the gate
      can't refuse a run that leaves pcu out, only a shortfall of boards).

- [x] **K15.22 — A via keeps the hole-to-hole distance from a pin's real hole.**
  - **Change:** after K15.19 a `.kicad_pcb` pin carries its drilled hole (`DrillData::hole`,
    `board/pad_holes.rs`) for the hole clearance, but the hole-to-hole distance between drills
    (Q22.11, `board/hole_spacing.rs`) still takes the padstack's estimated radius. An NPTH slot
    such as wisweep's J10 (0.95 x 0.65 mm, estimated as a 0.146 mm radius circle) lets a via's
    drill come nearer its wall than `min_hole_to_hole` (found by K15.19's implementer,
    2026-10-21; not yet seen on a routed board). Measure hole to hole from the real hole, slots
    included, in the router and in Q20's output check.
  - **Accept:**
    - a test on a small board with an NPTH slot and a via forced beside it: the via's drill is
      at least `min_hole_to_hole` from the slot's wall, and the check finds a via placed nearer;
    - wisweep's two K15.14 arms (a minute each), graded by KiCad: 0 routing-caused
      `hole_to_hole`, 0 manufacturing breaches;
    - a DSN board unchanged (`legacy`, parity 7 of 7 if shared code changes).
  - *Done 2026-10-21* (branch from main `8689999`). Cause: `BasicBoard::hole_to_hole_breaches`
    (`board/hole_spacing.rs`), the one query the maze search (`autoroute/session/layers.rs`,
    `drill_breaks_hole_to_hole`), the via insertion (`shove/via_check.rs`, `check_via`), a via
    move (`shove/drill_check.rs`) and Q20's check (`drc/board_rules.rs`, `hole_to_hole`) ask,
    measured every other drill through `hole_radius`, the padstack's estimate (J10's slot: a
    0.146 mm radius round its centre). Fix: a pin with its real hole (K15.19's `DrillData::hole`)
    is measured from the hole's centre segment less its radius (a slot's half width); the query
    reaches the largest real hole (`BasicBoard::pad_hole_reach`, set by `set_pad_holes`, kept by a
    snapshot). Both profiles; a DSN board has no real holes.
    - *Accept 1:* `crates/freerouting/tests/k15_wisweep_hole_to_hole.rs` on the new
      `tests/data/kicad/k1519/wisweep-j10-via.kicad_pcb` (K15.19's slot cut, hand-written: SIG's
      F.Cu TP1 and B.Cu TP2 centred 1.5 mm right of J10's slot, so one via goes beside it). The
      board's 0.25 mm is not binding there (the 0.25 mm hole clearance from the via's 0.4 mm
      copper radius keeps the drills further apart), so the tests set
      `router.board_minimums.hole_to_hole_um=1400`.
      `a_via_keeps_the_hole_to_hole_distance_from_an_npth_slot`: RED at `8689999` "legacy: the via
      at (469516, -624910) is 1.2632 mm from J10's slot" (1.442 mm from the estimate); GREEN in
      both profiles, the via at (46.9516, 62.2712) mm, 1.4144 mm from the wall.
      `the_output_check_finds_a_via_nearer_than_the_hole_to_hole_distance_to_a_slot` (the via
      routed under the board's 0.25 mm, then the minimum raised to 1.4 mm): RED at `8689999`
      (0 findings), GREEN 1 finding, actual 1.2632 mm (within 0.1 µm of the slot gap), a pin
      among its items.
    - *Accept 2:* the K15.14 commands (`plugin-input-ab.sh kicad_pcb`,
      `tools/bench/k15-all-pours.options.json`, `--max-passes 2 --timeout 240`, `pcb-on` adding
      `--router.plane_obstacle_all_pours=true`) with this tree's script and its release binary
      (sha256 `bc7accf4…`), the board copied byte for byte to
      `target/orchestrator/k1522/boards/`, output `target/orchestrator/k1522/runs/wisweep-pcb-{off,on}/`,
      KiCad graded (kicad-cli one at a time). These numbers are seen before any later K15.14
      amendment.

      | arm | KiCad unc. (K15.19 -> now) | routing-caused `hole_to_hole` | manufacturing breaches | below-minimum | route s |
      |---|---|---|---|---|---|
      | `pcb-off` | 27 -> 27 | 0 -> 0 | 0 -> 0 | 0 -> 0 | 22.8 -> 23.3 |
      | `pcb-on` | 45 -> 45 | 0 -> 0 | 0 -> 0 | 0 -> 0 | 9.3 -> 9.7 |

      KiCad reports no `hole_to_hole` at all in either routed board. Routing-caused items left:
      `silk_edge_clearance` 2 and `track_dangling` 2 / 1, as in K15.19. Q20's output check: 0
      breaches (225 and 97 routed items checked). The sessions differ from K15.19's (`36e80009…`
      -> `571cb3d0…`, `8e4454b5…` -> `906d85af…`); main between `4916e6d` and `8689999` changed
      the route too (K15.12, K15.18), and no `8689999` baseline was routed.
    - *Accept 3:* `PARITY_SET=tools/bench/sets/connection-parity.txt tools/parity-check.sh
      --no-probes`: `PARITY OK`, 7 of 7 `[exact]`, `bytes true`.
    - DIVERGENCES: "A via keeps the hole-to-hole distance from a pin's real hole" (K15.22); the
      K15.19 entry now points to it.

- [x] **K15.23 — A trace end inside a pour's outline but outside its fill is not a contact.**
  - **Change:** on the pcu board's `.kicad_pcb` route (plugin default options, main `a91e46b`
    and K15.18's branch, byte-identical sessions, `target/orchestrator/k1518/runs/`), KiCad
    reports one routing-caused `track_dangling`: /CH6_NO on F.Cu, a 0.5439 mm track from a via at
    (125.0442, 107.7107) mm to (124.59, 108.0953). That end lies inside zone EFUSE_NO_6's outline
    (F.Cu, priority 43) but outside all three of its `filled_polygon`s (the fill ends at x
    123.8264 there). The router counts the end as touching the pour (its outline) and keeps the
    stub; KiCad's fill doesn't reach it. Make the router's contact with a pour on a
    `.kicad_pcb` follow the fill where K14.4's fills are current (or the fill model where not),
    so the route reaches the fill or the stub is removed (Q16's stub cleanup).
  - **Accept:**
    - a test on a small board cut from the pcu board: a trace that ends on the outline outside
      the fill is not a contact, and the route reaches the fill or leaves no stub;
    - the pcu `.kicad_pcb` route again, graded by KiCad: 0 routing-caused `track_dangling`,
      unconnected not up (47), 0 below-minimum;
    - `legacy` unchanged (parity 7 of 7 if shared code changes).
  - *Done 2026-10-22* (branch from main `f7b0928`): the stub cleanup judges a signal-layer pour of
    a `.kicad_pcb` by the router's fill model opened by the zone's `min_thickness`; the router's
    own contacts stay Java's.
    - *Cause:* EFUSE_NO_6 is a conduction area built from the zone's outline
      (`freerouting-kicad/src/zones.rs::make_plane`; K15.15's windows only cut higher-priority
      zones of other nets). The area is no obstacle (`plane_obstacle_all_pours` unset), so the
      route lays `Net-(U15-PGTH)` on F.Cu at x 124.0539 mm through it. Right of that track the
      outline leaves a strip 0.347 mm wide (to x 124.6279, KiCad's clearance 0.127 mm), under the
      zone's 0.5 mm `min_thickness`, so KiCad's refill opening removes it. The `/CH6_NO` end at
      (124.59, 108.0953) lies in that strip. Three things count it as a contact:
      - the router's contact, `BasicBoard::conduction_area_normal_contacts` (`basic_board.rs`,
        Java `ConductionArea.getNormalContacts`: a trace end in a tile of the outline);
      - Q16.4's cleanup contacts, `quality/stub_contacts.rs::pieces` (a conduction area's pieces
        are its outline tiles; "Known limit" in `quality/stubs.rs`);
      - the fill model, `drc::fill_islands`, which keeps the strip: it has no minimum-thickness
        opening (`drc/fill_model.rs`, "not modelled").

      K14.4's fills (`io/kicad.rs::read_pour_fills`) and K15.16's (`KicadReportRules::zones`)
      are the board as read. Any routed item changes `state_hash`, so `fills_current` is false
      whenever the cleanup has router copper to judge. Java at the pin has the same outline
      contact and no cleanup.
    - *Fix:* `quality/pour_fill.rs` (new) holds `PourFills`, the cleanup's per-area copper for a
      conduction area on a signal layer that is a zone of the file (`drc::file_zone_of`, made
      `pub(crate)`). That copper is the fill model eroded by `r` = `min_thickness`/2 - 1 µm
      (`drc::eroded_fill_islands`, `zone_islands.rs`): each foreign obstacle is grown by its
      clearance plus `r`, `edge_strips` cut `r` along the outline and its holes, and the net's own
      pads are filled over. Each trapezoid is rounded outwards and grown back by `r`
      (`TileShape::enlarge`). The fills are cached and forgotten near each removal
      (`forget_near`). `quality/stub_contacts.rs` has `*_with` variants that take the fills;
      `quality/stubs.rs` (`remove_stubs`, `is_removable_stub`, `contacts_are_connected`) uses
      them. The output check and the KiCad report keep the outline functions. `ZoneFill` and
      `BoardZone` carry `min_thickness` (default 0.25 mm, `ZONE_SETTINGS`). With erosion 0 the
      operands are `fill_operands`' exactly. Not changed: power layers, a DSN input (it has no
      fill and no minimum thickness, so the outline stays) and the router's own contacts. The
      route does not aim for the fill. It reached the fill on the pcu board only through the
      output check's retry (below); on the fixture it leaves the connection open. Module docs:
      `quality/pour_fill.rs`, `quality/stub_contacts.rs`, `quality/stubs.rs`,
      `drc/zone_islands.rs`, `freerouting-kicad/src/zone_fills.rs`. DIVERGENCES: the
      `cleanup.remove_stubs` entry, "K15.23 lifts it ...".
    - *Accept 1:* `crates/freerouting/tests/k15_pour_fill_stub.rs` (4 tests) on
      `tests/data/kicad/pcu-k1523/`, written by `make_boards.py` with every coordinate taken from
      the pcu route: zone EFUSE_NO_6, the `/CH6_NO` stub with its via and B.Cu trace, the PGTH
      track, and the pcu `.kicad_dru`. The boards: `a-stub-beside-a-crossing` and `b-no-crossing`
      (no PGTH track), each as written (`-unfilled`) and as KiCad 10.0.7 `pcb drc --refill-zones
      --save-board` saved it.
      - KiCad on a: `track_dangling` "Track [/CH6_NO] on F.Cu, length 0.5439 mm", plus 1
        unconnected (the track and the zone).
      - KiCad on b: no `/CH6_NO` item.
      - `the_end_in_the_outline_touches_the_opened_fill_only_without_the_crossing`: on all 4
        boards the router's contact and Q16.4's contact are true, and the opened fill gives
        false on a, true on b.
      - `the_cleanup_removes_the_stub_beside_the_crossing_and_keeps_it_without`: on a, 2 traces
        and 1 via are removed and no `/CH6_NO` copper is left; on b, 0 are removed.
      - `the_fill_eroded_by_nothing_is_the_fill_model`.
      - `the_route_from_j2_leaves_no_stub_under_enhanced`: a without the stub, routed. Both
        profiles lay one F.Cu trace to (124.5279, 109.2002) mm, in the strip again. `legacy`
        keeps it (0 incomplete); `enhanced` without the cleanup keeps it too; `enhanced`
        removes it, so the route leaves no stub (0 `/CH6_NO` copper, 1 incomplete).

      RED: with `PourFills::pieces` forced to the outline, 3 of the 4 tests fail. GREEN: all 4
      pass.
    - *Accept 2:* release binary of this branch,
      `target/orchestrator/k1523/scripts/arms.sh` (K15.18's off-arm command, `ROUTE_TIMEOUT_S=300`,
      `ROUTE_MEMORY_KB=8000000`), graded by KiCad 10.0.7. Runs are in
      `target/orchestrator/k1523/runs/`.

      | arm | KiCad unconnected | router incomplete | below-minimum | manufacturing | routing-caused | cleanup (vias, traces) | route s | session |
      |---|---|---|---|---|---|---|---|---|
      | off, K15.18 | 47 | 11 | 0 | 0 | track_dangling 1 | 22, 5 | 165.5 | `a2d99014…` |
      | off, K15.23 | 47 | 11 | 0 | 0 | none | 24, 7 | 181.1 | `bd0a87fd…` |
      | off, K15.23, `--router.output_check=report` | 47 | 11 | 0 | 0 | track_dangling 1 | 24, 7 | 162.3 | `b0b5367e…` |

      - The cleanup now removes the `/CH6_NO` chain: the F.Cu stub, the vias at (125.0442,
        107.7107) and (126.9387, 105.6351), and the B.Cu trace.
      - It keeps a 0.193 mm F.Cu trace from (126.9387, 105.6351) to (126.9387, 105.8281). Its
        open end's cap (half width 0.1 mm) reaches a neighbouring `/CH6_NO` trace 0.093 mm away.
        That is Q16.4's end cap, which is more generous than KiCad's point test.
      - The output check (`repair`) finds that trace as `track_dangling` 1, removes it and
        retries the connection. The retry routes `/CH6_NO` to a via at (127.811, 111.2185) mm,
        inside EFUSE_NO_6's fill. The report-only run keeps the trace, and KiCad flags it.
      - Probe on K15.18's routed board as read back: of 2121 traces and 343 vias, only that
        stub's end changes its contact (outline true, opened fill false), and judging all of
        them took 40 ms. On the owner's committed pcu board (K15.16's copy, 1909 traces, 773
        vias) nothing changes.
    - *Accept 3:* `legacy` does not run the cleanup (`cleanup.remove_stubs` unset), and the shared
      DRC fill is unchanged (`fill_operands` = the eroded operands with erosion 0, pinned by the
      test above): `PARITY_SET=tools/bench/sets/connection-parity.txt tools/parity-check.sh
      --no-probes`: PARITY OK, 7 of 7 `[exact]`, `bytes true`.
    - *Gate:* `cargo fmt --all --check` clean; `cargo clippy --workspace --all-targets -- -D
      warnings` clean; `cargo test --workspace --no-fail-fast`: 2962 passed, 0 failed, 56
      ignored; `pytest -rs kicad-plugin/tests` (`FREEROUTING_RS_BIN` = this branch's debug
      directory): 677 passed, 6 skipped (pcbnew, canary-pwr, KICAD_METADATA_REPO only).

- [x] **K15.24 — The plugin's DSN input cuts a plane where a higher-priority zone fills.**
  - **Change:** K15.15 cuts each plane of a `.kicad_pcb` input where a higher-priority zone of
    another net fills (`freerouting-kicad/src/zone_priority.rs`). The DSN input can't: KiCad's
    export writes no zone priority, so each `(plane ...)` covers its whole outline. On the pcu
    board's DSN arms (K15.14 amendment 3, `target/orchestrator/k1514/out/pcu-76163c7/dsn-off` and
    `dsn-on`), KiCad reports 4 routing-caused `via_dangling`: V_BUS_12V vias where In2.Cu's
    V_BUS_24V zone (priority 41) overlaps V_BUS_12V's (priority 1). The router counts them in
    V_BUS_12V's plane, but KiCad fills V_BUS_24V there. The DSN is the plugin's own export
    (pcbnew in hand), and K14's fix-ups already rewrite it. So write each plane's polygon as
    KiCad fills it: the zone's filled polygons on that layer, or the outline minus every
    higher-priority zone of another net. Use K15.15's rule, ZONE::HigherPriority, for the order.
  - **Accept:**
    - a plugin test with the stub `pcbnew`: two overlapping zones of different nets and
      priorities on an inner layer, and the DSN's lower plane leaves out the higher zone's area;
    - the pcu board's DSN arm (the K15.14 `dsn-off` command, a few minutes), graded by KiCad:
      0 routing-caused `via_dangling`, unconnected not up (55), 0 below-minimum;
    - a DSN with no overlapping zones is byte-identical; `legacy`'s connection-parity is
      unaffected (it reads committed DSNs).
  - *Done 2026-10-22:* the plugin cuts the planes; every clause holds as read here. Decided
    2026-10-22 (the clause's premise was wrong): the item took K15.14's 4 `dsn-off`
    `via_dangling` for V_BUS_12V vias, but they are 3 V_BUS_12V vias and one `Net-(U24-OUT)` via
    that no higher zone covers. Accept 2's "0 routing-caused `via_dangling`" is read as the
    priority-caused ones: 3 -> 0. The `dsn-off` cut arm keeps 2 `via_dangling` and has 3
    `track_dangling` (5 routing-caused items against the base arm's 4), all K15.13's
    pour-crossing class under the default options; K15.14 decided that class's answer is the
    per-board option, and the `dsn-on` cut arm has 0 routing-caused (below).
    - *Form:* the outline minus the higher zones, not the zone's fill. A fill has a relief or
      clearance hole round every pad, so the router (contact = a drill centred in the plane)
      would count every pad as off its plane. The reader takes it as written:
      `(plane NET (polygon ...) (window ...)...)` is a border with holes
      (`io/specctra/parser/shape.rs::DsnArea::resolve`, `PolylineArea`), several planes per net
      are several conduction areas, and nothing in `crates/` changed.
    - *Change:* `kicad_board.plane_cuts` gives one fact per exported plane, in KiCad's export
      order (`specctra_export.cpp`: board zones that are no rule area, each on its copper
      layers, a netless zone as `@:no_net_N`). It uses pcbnew's `SHAPE_POLY_SET`. The base is
      the zone's `Outline()`. For each copper zone (board and footprint) of another net on that
      layer for which `other.HigherPriority(zone)` (KiCad's own `ZONE::HigherPriority`) holds,
      the plane loses that zone's outlines (not its holes, K15.15's rule), grown by the larger
      of the two zones' `GetLocalClearance()` (0.5 mm when unset, as the reader) plus
      `m_MaxError`, round corners (`ZONE::TransformShapeToPolygon`'s ERROR_OUTSIDE). A plane
      counts as cut when it loses more than 1 um2.
      - `plane_priority.apply` (new, pure) matches the DSN's planes to the facts by layer and
        net in order. A mismatch logs a WARNING and cuts nothing. It writes each cut plane as
        one `(plane ...)` per piece left, holes as `(window ...)`, and drops a plane with
        nothing left.
      - `export_flow.fix_plane_priorities` runs it right after the export (always on, both
        profiles) and logs one `plane NET on LAYER: cut where N higher-priority zone(s) of
        another net fill (...), A -> B mm2, P plane(s), W window(s)` line per cut plane.
        Without a cut it does not write the file.
      - DIVERGENCES (kicad-plugin section, "A plane is cut where a higher-priority zone of
        another net fills"); `docs/development/kicad-plugin.md`.
    - *Accept 1:* `kicad-plugin/tests/test_plane_priority.py` (14 tests). The fake pcbnew does
      KiCad's arithmetic for rectangles:
      - `test_the_lower_plane_leaves_out_the_higher_zones_area`: In2.Cu, LOW priority 1 under
        HIGH priority 41 with 0.5 mm clearance. The lower plane's border is the U round the
        grown zone, 200.00 -> 172.42 mm2; HIGH's plane is KiCad's text.
      - window, split and dropped planes; the order (priority, then UUID); one layer only;
        footprint zones cut and rule areas don't; the export's netless names; a mismatch.
      - `test_kicads_export_of_two_overlapping_zones_through_pcbnew`: KiCad 10.0.7's own pcbnew
        and `ExportSpecctraDSN`. It checks the round grown corner, and that zones moved apart
        export byte-identically. It is skipped without pcbnew. It passes under the system
        `python3` (pcbnew) with the venv's pytest on `PYTHONPATH`: 23 passed with
        `test_export_flow.py`.
      - RED: `ModuleNotFoundError: No module named 'plane_priority'`, at main `1a5f1b8`.
        GREEN: all pass.
    - *Accept 2:* release CLI at main `1a5f1b8` (`crates/` unchanged). Script:
      `target/orchestrator/k1524/runs/arms.sh` (K15.14's `dsn-off` command,
      `plugin-input-ab.sh dsn <pcu 76163c7 stripped> tools/bench/k15-all-pours.options.json <out>
      -- --max-passes 2 --timeout 240`, `ROUTE_TIMEOUT_S=300`, `ROUTE_MEMORY_KB=8000000`), and
      `arms-on.sh` (the same plus `--router.plane_obstacle_all_pours=true`). Arms: `base` with
      main's plugin (worktree `k1524/base`), `cut` with this tree's. Graded by KiCad 10.0.7;
      runs in `target/orchestrator/k1524/runs/pcu-dsn-*`. These are the first numbers of this
      change, seen before any later measurement. The base arms reproduce K15.14 amendment 3's
      sessions byte for byte (`1af813ec…`, `7771480f…`).

      | arm | KiCad unconnected | Zone-Zone (zones) | router incomplete | below-minimum | manufacturing | routing-caused | route s |
      |---|---|---|---|---|---|---|---|
      | `dsn-off` base | 55 | 39 (28) | 12 | 0 | 0 | via_dangling 4 | 176.2 |
      | `dsn-off` cut | 47 | 28 (25) | 13 | 0 | 0 | via_dangling 2, track_dangling 3 | 167.7 |
      | `dsn-on` base | 31 | 7 (9) | 26 | 0 | 0 | via_dangling 4 | 161.6 |
      | `dsn-on` cut | 27 | 6 (9) | 26 | 0 | 0 | none | 175.3 |

      - The plugin log of both cut arms has 28 `plane ...` lines. One is
        `plane V_BUS_12V on V_BUS: cut where 1 higher-priority zone(s) of another net fill
        (V_BUS_24V), 11346.00 -> 8585.16 mm2`; the other 27 are outer-layer pours, the same
        count as K15.15's `.kicad_pcb` report. The cut takes 0.11 s.
      - `dsn-off` base's 4 `via_dangling` are the 3 V_BUS_12V vias and a `Net-(U24-OUT)` via at
        (61.22, 118.92) mm. The cut arm keeps the U24-OUT via and has a `/CH5_NO` via at
        (112.91, 92.19), plus `track_dangling` on USB_GND (2, 0.1 mm stubs) and `/CH4_NO`. Each
        of the 5 ends inside its own net's F.Cu pour outline, where the input board's fill
        reaches (within 0.05 mm, pcbnew probe `k1524/probe_any.py`) and the routed board's
        does not (more than 0.8 mm): other nets' new copper splits the pour (K15.13). They are
        not under a higher zone.
      - Router incomplete against KiCad's unconnected: `dsn-off` cut 13 against 47 (28 of the
        47 are Zone-Zone from the split pours); `dsn-on` cut 26 against 27.
    - *Accept 3:* 19 boards through `k1524/dry.py` (KiCad's refill and `ExportSpecctraDSN`,
      then the fix-up): the 18 KiCad 10.0.7 demos in `/usr/share/kicad/demos` and canary-pwr
      `95882f00`. The 10 without such an overlap give 0 lines and a byte-identical DSN
      (ecc83-pp, ecc83-pp_v2, interf_u, kit-dev-coldfire, microwave,
      multichannel_mixer-unrouted, pic_programmer, sonde xilinx, tinytapeout-demo, video). The
      9 that are cut (CM5_MINIMA_3 3, complex_hierarchy 1, jetson 44,
      multichannel_mixer 1, One-Air-Max 9, RoyalBlue54L 5, StickHub 1, vme-wren 13,
      canary-pwr 6) all load in the router (`-de -drc`, rc 0). `crates/` is unchanged, so
      `legacy` on the committed DSNs (connection-parity) is unchanged; no parity run made.

- [x] **K15.25 — The router's search reaches a `.kicad_pcb` signal pour where it is filled.**
  - **Change:** K15.23 made the stub cleanup judge a signal-layer pour of a `.kicad_pcb` by the
    router's fill model opened by the zone's `min_thickness` (`quality/pour_fill.rs`). The
    router's own contacts and the maze search's destination are still the pour's outline
    (`BasicBoard::conduction_area_normal_contacts`, Java's `ConductionArea.getNormalContacts`).
    On K15.23's fixture (`crates/freerouting/tests/data/kicad/pcu-k1523/a-stub-beside-a-crossing`,
    `tests/k15_pour_fill_stub.rs::the_route_from_j2_leaves_no_stub_under_enhanced`) both
    profiles route J2's `/CH6_NO` to (124.5279, 109.2002) mm, inside the outline where the
    opened fill does not reach; `enhanced`'s cleanup removes it and the connection stays open
    (1 incomplete). On the pcu board the connection reached the fill only through the output
    check's retry (`router.output_check=repair`). Make `enhanced`'s search aim for the pour
    where its opened fill reaches (the destination or the contact test from `PourFills`), with
    `legacy` unchanged.
  - **Accept:**
    - the fixture's J2 connection routes 0 open under `enhanced`, its end inside the opened
      fill; the test's expected value changes with a note;
    - the pcu `.kicad_pcb` route (K15.18's off-arm command, a few minutes), graded by KiCad, with
      the default options and with `--router.output_check=report`: 0 routing-caused
      `track_dangling`, unconnected not up (47), 0 below-minimum, 0 manufacturing breaches, the
      route time against K15.23's (181.1 s, 162.3 s);
    - `legacy` unchanged (parity 7 of 7 if shared code changes).
  - *Partial 2026-10-22* (second pass, on the first pass's WIP `f029319`): accept 1 and 3 hold
    with the setting set; accept 2 does not: with the setting the pcu route has KiCad unconnected
    49 > 47 (0 routing-caused, 0 below-minimum, 0 manufacturing breaches under `repair` and
    `report`). By the orchestrator's rule (decided 2026-10-22: the setting stays in `ENHANCED` as
    a correctness rule only if neither the pcu board's KiCad unconnected nor a second board's goes
    up, with 0 routing-caused added) `router.pour_fill_target` is now **opt-in**, unset in both
    profiles. *Remains:* the pcu board's extra unconnected items (below; they come from the
    router's own pour contacts, the outline, which K15.25 left Java's) and the choice of making it
    a default, which needs a measurement over more boards: K15.30 measures it, K15.31 takes the
    router's own pour contacts from the opened fill (decided 2026-10-22: canary-pwr's 40 -> 31 is
    measured, not dropped).
    - *Setting:* `router.pour_fill_target` (bool; unset/`false` Java's rule; unset in `legacy`
      and `enhanced`, opt-in). No existing setting fits: the search's target and the board's cycle
      rule, not `pad_contacts` (pins) nor the cleanup. Documented in `settings/fields.rs`,
      `tests/settings_defaults.rs` (unset), the GUI's no-control list, the module docs of
      `autoroute/pour_target.rs` ("Not in `enhanced`", with the numbers), DIVERGENCES (board
      model, `pour_fill_target`: opt-in and why). The first pass's `ENHANCED` entry and its
      "Profile `enhanced`" line are removed (`settings/profile.rs`).
    - *Cause:* the search's destination on a pour is its outline tiles: a target door's
      connection point is the tile's nearest point (`expand_to_target_doors`,
      `autoroute/session/search.rs`), the path ends at the centre of gravity of tile within room
      (`calculate_starting_point`, `autoroute/path/locate.rs`). On the fixture that is (124.5279,
      109.2002) mm in the strip. Second cause, found on the pcu run: Java's cycle rule removes a
      trace piece whose two ends lie in one conduction area (`split_trace_item`'s
      `FoundKind::ConductionArea` branch, `board/trace_ops.rs`; `trace_is_overlap`,
      `board/basic_board.rs`). The insertion splits the found path at its corners, so a path that
      reaches the core keeps only its part up to the first corner inside the outline: `/CH2_NO`'s
      fanout reached EFUSE_NO_2's core at (138.049, 102.1244) mm, the piece from (139.644,
      102.1244) was removed, KiCad flagged the 0.1017 mm rest `track_dangling` (traced with a
      board-insert/remove hook and backtraces, `target/orchestrator/k1525/scratch/dbg/`).
    - *Fix:* `autoroute/pour_target.rs` (new). Per search, `Session::init` takes the core of each
      start or destination pour's opened fill (`quality/pour_fill.rs::opened_fill_core`, now a
      `FillCore` with the tiles and the radius: K15.23's eroded fill before the growth back;
      `PourFills`' selection unchanged, a `.kicad_pcb` only): a target door is expanded only where
      tile within room meets the core (connection point: nearest point of those parts), the
      located end is Java's point if a part holds it else the centre of gravity of the nearest
      part, a start door with no core part is not queued and the start's final target shape is
      the nearest part. The board's cycle rule (`BasicBoard::area_joins_trace_ends`, board flag
      set by `prepare_job`, kept in snapshots) keeps a trace with both ends in such a pour only as
      a bridge into the fill (second pass, below). A first version targeted the grown pieces: the
      fixture route ended at (124.6279, 110.42) mm, on the outline's edge (KiCad accepted it), so
      the core replaced them. A second version judged the cycle rule by the grown pieces: the same
      `/CH2_NO` stub (the fill reached the corner during the fanout, later routing cut it).
    - *Second pass, the lost connections* (item 1 of the dispatch). Same binary (`f029319`'s tree,
      `target/orchestrator/k1525/bin/v1`), the setting off against on: off is K15.23's route
      byte for byte (session `bd0a87fd…`, 47 unconnected, router 11); on is the first pass's
      (`ab2efa8a…`, 49, 14). Instrumented runs (a temporary `K1525D` trace of every insertion
      failure and every trace the cycle rule kept, and `K1525_OFF` switches for the parts; the
      patch is `target/orchestrator/k1525/scratch/debug-instrumentation.patch`, not committed;
      logs in `target/orchestrator/k1525/runs/dbg/`):
      - the first pass's cycle rule (`area_joins_trace_ends`, `autoroute/pour_target.rs`: Java's
        cycle only if the core holds both ends) kept 25 distinct traces Java removes, only one of
        them a bridge (`/CH2_NO`'s). One is `Net-(U24-SW2)`'s trace 5303 from (52.8133, 122.7795)
        to (52.8133, 122.3244) mm inside its F.Cu pour, neither end in the fill (one end a via,
        the other bare), kept at every normalization (7 times). `Net-(U24-BST1)` then routes
        around it at y 121.97 instead of 122.2, and `/V_PD_IN`'s C71-2 connection (net 10, from
        U24's pin at (55.1, 122.4) mm) fails to insert on F.Cu at (53.4463, 121.6225) mm
        (`ForcedInsert::Partial`, all 4 attempts): the "could not be inserted" failures;
      - `/Charger Circuit/DRV_SUP` (U11-23, no pour) fails to insert at (139.3033, 50.0811) mm
        with no kept trace near it: a knock-on of the changed board;
      - with the cycle rule switched off (`K1525_OFF=cycle`) the route is 12 incomplete,
        `/V_PD_IN` and `DRV_SUP` routed, `Net-(J2-Pin_1)` failed; with the start rule off
        (`K1525_OFF=start`) 13, `/V_PD_IN` routed, `DRV_SUP` failed; the core target itself
        changes the fanout (escaped pins 882 -> 884 in every arm with it on);
      - KiCad's 3 more items (v1, per net, off -> on): `/CH2_NO` 1 -> 2 (U4's pad 8 at
        (137.343, 96.3) mm and the track to the core: pad 8 lies in EFUSE_NO_2's outline, which
        the router counts as joining it, where KiCad's fill does not reach), `/CH2_SENSE` 1 -> 2,
        `/CH5_NO` 1 -> 2 (R11's pad 1 and a B.Cu track, the same kind), `Net-(Q2-D)` 0 -> 3 (zone
        islands), `DRV_SUP` 0 -> 1, `/V_INRUSH` 2 -> 3; fewer on `/BAT_VP`, `/CH6_NO`,
        `/CH7_NO`, `USB_GND`, `Net-(Q4-D)`.
    - *Second pass, the fix:* the cycle rule keeps a trace only as a bridge into the fill: the
      core holds exactly one end and the other end is not at a pin or via that the opened fill
      reaches (`FillCore::reaches`: the core grown back by the radius, as `PourFills::pieces`
      grows it). Both ends in the core, neither, or the far end at such a pin or via: Java's
      removal. On the pcu board it keeps 8 distinct traces (was 25), `/V_PD_IN` routes, and the
      route still has 49 unconnected and 14 incomplete (session `a1118e6c…`): `SCL` (U24-10)
      fails to insert and `DRV_SUP` finds no connection, two nets without a pour. Per net against
      off: `/CH2_NO` 1 -> 2 (two tracks from two U4 pads the router joins through EFUSE_NO_2's
      outline, not KiCad's fill), `/CH5_NO` 1 -> 2 (R11, the same kind), `Net-(F4-Pad2)` 0 -> 1
      (a SPARE_V island), `DRV_SUP` 0 -> 1, `SCL` 0 -> 1; `/CH6_NO` 3 -> 2, `USB_GND` 6 -> 5,
      `/V_PD_IN` 1 -> 0. Insertion failures over the run 135 -> 66, "could not be inserted" 47 ->
      36, "no connection was found" 10 -> 16 (all on `SDA`, `Net-(U11-ICHG)` and `DRV_SUP`).
      The remaining extra items are pads the router's contact rule (the outline, Java's
      `ConductionArea.getNormalContacts`) counts joined where KiCad's fill does not reach, and
      which pads stay so changes from route to route; K15.25 keeps the router's contacts Java's.
    - *Accept 1:* `tests/k15_pour_fill_stub.rs`, run with `pour_fill_target = true` on top of
      `enhanced` (the tests set it now): `the_route_from_j2_leaves_no_stub_with_the_fill_target`
      (renamed from `..._under_enhanced`) expects 0 incomplete with one `/CH6_NO` trace ending at
      (124.3789, 110.5231) mm, in a piece of the opened fill and in its core, with the cleanup
      and the output check and without them; `legacy` and `enhanced` (unset) still end at
      (124.5279, 109.2002) mm, and `enhanced` with its cleanup leaves 1 incomplete (K15.23's
      expected value, back). `a_trace_from_the_fill_to_the_strip_is_a_cycle_only_under_javas_rule`
      has three arms (`legacy`, `enhanced`: a cycle; `enhanced` with the setting: no cycle). New
      in the second pass: `the_cycle_rule_keeps_only_a_bridge_into_the_fill` (a trace between two
      points of the strip, neither in the core, and a trace from the core to a `/CH6_NO` via that
      the opened fill reaches outside the core are both Java's cycles under the setting). RED:
      with the first pass's rule it failed at the strip trace (`is_cycle_through_the_pour` false);
      with the bridge rule minus the pin-or-via clause it failed at the via trace (overlap false,
      cycle true). GREEN: 6 passed. First pass (kept): release CLI on the fixture without the
      stub, KiCad 10.0.7 `pcb drc --refill-zones`: `legacy` `track_dangling` 1 + 1 unconnected,
      the fill target none, 0 unconnected (`target/orchestrator/k1525/runs/fixture/`).
    - *Accept 2:* K15.18's off-arm command (`plugin-input-ab.sh kicad_pcb k154/stripped/...
      k154/opts-default.json <out> -- --max-passes 2 --timeout 240`, `ROUTE_TIMEOUT_S=300`,
      `ROUTE_MEMORY_KB=8000000`; `target/orchestrator/k1525/scripts/armsb.sh`), graded by KiCad
      10.0.7, release binaries of this tree; second-pass runs in `target/orchestrator/k1525/runs/b/`
      (first pass: `runs/`, `first-core-only/`, `second-pieces-cycle/`). Route times of the
      second-pass runs are with other builds on the machine (clippy and the test build ran beside
      the final arms):

      | arm | KiCad unconnected | router incomplete | below-min | mfg | routing-caused | route s | session |
      |---|---|---|---|---|---|---|---|
      | K15.23, default | 47 | 11 | 0 | 0 | none | 181.1 | `bd0a87fd…` |
      | K15.23, `output_check=report` | 47 | 12 | 0 | 0 | track_dangling 1 | 162.3 | `b0b5367e…` |
      | first pass: core target, Java cycle rule | 44 | 12 | 0 | 0 | track_dangling 1 (`/CH2_NO`) | 159.6 | `840e0575…` |
      | first pass: cycle rule by grown pieces | 45 | 12 | 0 | 0 | track_dangling 1 (`/CH2_NO`) | 157.5 | `d88528e2…` |
      | first pass: core cycle rule, default (`v1-true`) | 49 | 14 | 0 | 0 | none | 167.7 | `ab2efa8a…` |
      | same binary, `pour_fill_target=false` (`v1-false`) | 47 | 11 | 0 | 0 | none | 164.3 | `bd0a87fd…` |
      | **final, unset (`enhanced`'s default now)** | 47 | 11 | 0 | 0 | none | 166.4 | `bd0a87fd…` |
      | **final, `pour_fill_target=true`** | 49 | 14 | 0 | 0 | none | 179.0 | `a1118e6c…` |
      | **final, `true`, `output_check=report`** | 49 | 14 | 0 | 0 | none | 164.8 | `a1118e6c…` |

      With the setting the default and report runs are byte-identical (the output check finds
      nothing to repair). Unset, the route is K15.23's byte for byte. A fill core costs about
      0.3 ms per pour on this board (first pass).
    - *Second board* (item 3 of the dispatch): `canary-pwr-95882f00` of
      `tools/bench/sets/k15-all-pours.txt` (19 signal-layer pours of 12 nets, K15.14's `pcb-off`
      96.6 s; the owner's other board and the row with the most signal-layer pours per net among
      the rows under 3 minutes after the pcu board), K15.14's command
      (`plugin-input-ab.sh kicad_pcb <row> tools/bench/k15-all-pours.options.json <out> --
      --max-passes 2 --timeout 240`; `target/orchestrator/k1525/scripts/board2.sh`, runs in
      `target/orchestrator/k1525/runs/c/`), this tree's release binary, graded by KiCad 10.0.7:

      | arm | KiCad unconnected | router incomplete | routing-caused | below-min | mfg | route s | session |
      |---|---|---|---|---|---|---|---|
      | unset (`final-unset`; `r2-false` the same) | 40 | 27 | none | 0 | 0 | 95.9 | `f1d2da90…` |
      | `pour_fill_target=true` (`final-true`; `r2-true` the same) | 31 | 19 | none | 0 | 0 | 108.0 | `0b534572…` |

      (`r2-*`: 93.3 s and 93.1 s, beside two other routes; `final-*` beside a test build.)
    - *Decision:* pcu 47 -> 49 and canary-pwr 40 -> 31, 0 routing-caused added on both: the pcu
      clause fails, so the setting is opt-in (above). Numbers seen in this order: v1 off/on, the
      instrumented runs, the bridge rule's pcu run (49), then canary-pwr, then the decision, then
      the final arms (all byte-identical to the earlier runs of the same code).
    - *Accept 3:* shared code changes only under the setting (`legacy` and `enhanced` leave it
      unset; the cycle rule returns Java's answer when the board flag is off):
      `PARITY_SET=tools/bench/sets/connection-parity.txt tools/parity-check.sh --no-probes`:
      PARITY OK, 7 of 7 `[exact]`, `bytes true`.
    - *Gate:* `cargo fmt --all --check` clean; `cargo clippy --workspace --all-targets -- -D
      warnings` clean; `cargo test --workspace --no-fail-fast`: 2971 passed, 0 failed, 56 ignored.

  - *Done 2026-10-22* (main `dce4f86`, K15.31's release binary `target/orchestrator/k1531/bin/v3`,
    whose crates are main's): Accept 1 and 3 held in the Partial note above. Accept 2 holds with
    `router.pour_fill_target` and K15.31's `router.pour_fill_contacts` together, the pair whose
    default K15.37 measures (with `pour_fill_target` alone it stays 49 > 47, above). The pcu
    `.kicad_pcb` route (K15.18's off-arm command on `target/orchestrator/k154/stripped`, plugin
    default options, `--max-passes 2 --timeout 240`), graded by KiCad 10.0.7:

    | options | KiCad unconnected | routing-caused | below-minimum | manufacturing | route s | K15.23's s | session |
    |---|---|---|---|---|---|---|---|
    | default (`repair`), both settings (K15.31's run, `k1531/runs/rebased-pcu/contacts-target`) | 21 | none | 0 | 0 | 176.2 | 181.1 | `25cf997a...` |
    | `--router.output_check=report`, both settings (`target/orchestrator/k1525c/runs/both-report`) | 21 | none | 0 | 0 | 185.0 | 162.3 | `25cf997a...` |

    The two sessions are byte-identical: the output check repairs nothing on this route. Whether
    the pair becomes `enhanced`'s default is K15.37's.

- [x] **K15.26 — The cleanup judges a DSN input's signal-layer pour by the fill model.**
  - **Change:** K15.23's opened fill applies to a `.kicad_pcb` only (a zone of the file gives its
    `min_thickness`); on a DSN input the cleanup keeps the pour's outline. On K15.24's pcu
    `dsn-off` cut arm (`target/orchestrator/k1524/runs/pcu-dsn-off-cut`, the plugin's default
    options) KiCad reports 2 `via_dangling` (`Net-(U24-OUT)` at (61.22, 118.92) mm, `/CH5_NO` at
    (112.91, 92.19) mm) and 3 `track_dangling` (two 0.1 mm USB_GND stubs, one `/CH4_NO`). Each
    ends inside its own net's F.Cu pour outline more than 0.8 mm from the routed board's fill:
    other nets' copper splits the pour (K15.13's class). Let `enhanced`'s cleanup take a DSN
    signal-layer pour's copper from the fill model too. A DSN carries no fill and no
    `min_thickness`: say what opening it uses and why.
  - **Accept:**
    - a test on a small DSN with a signal pour that a crossing trace splits: a trace end in the
      cut-off part is no contact and the cleanup removes it; on the same pour without the
      crossing it stays;
    - the pcu board's DSN arm again (K15.24's `target/orchestrator/k1524/runs/arms.sh`, the
      plugin's default options, a few minutes), graded by KiCad: 0 routing-caused
      `track_dangling` and `via_dangling`, unconnected not up (47), 0 below-minimum, 0
      manufacturing breaches;
    - `legacy` unchanged (parity 7 of 7 if shared code changes).
  - *Partial 2026-10-22* (branch from main `a541495`): the engine side is done. A DSN signal-layer
    pour is the cleanup's fill model; it is opened by the zones' minimum thickness and kept the
    zones' clearance from other nets' copper when `router.cleanup.pour_min_thickness_um` and
    `pour_clearance_um` give them. These are new settings, unset in both profiles. The pcu arm
    keeps 3 of its 5 routing-caused items. Remains:
    - the plugin passing the two values on its DSN path: the smallest `min_thickness` and zone
      clearance of the board's signal-layer zones (K15.27, done: its pcu run is byte-identical
      to the `zones` arm);
    - `USB_GND`'s trace: trimming its dangling ends (K15.28);
    - `U24-OUT`'s via: the fill model fills over own-net pads (K15.29).
    - `/CH5_NO`'s via, back under the true clearance (K15.32 passes the `.kicad_dru`'s 127 µm,
      not the zones' 500): the fill model keeps a corridor to the via that KiCad's fill does not
      (K15.33).
    - *Cause per item*: the 5 items of K15.24's `dsn-off` cut arm, probed on its DSN and SES. The
      probe was `target/orchestrator/k1526/probe/zz_k1526_probe.rs`, a temporary test, not
      committed. KiCad's fill was read with pcbnew (`k1526/probe/probe_ends.py`, `zonefill.py`).
      The DSN's clearance is the net class's 0.15 mm. KiCad's F.Cu zones have clearance 0.5 mm
      and `min_thickness` 0.5 mm (0.4 mm on 2 of 79), which KiCad's export drops. With erosion 0
      the fill model (router clearances) reaches every one of the 5 ends: each lies inside an
      island, up to r = 0.249 mm for 4 of them.
      - `/CH4_NO` track end (149.4926, 94.289): no contact with min thickness 0.5 mm alone (the
        end is 1.22 mm off the eroded fill) or with clearance 0.5 mm alone (1.57 mm).
      - `/CH5_NO` via (112.9114, 92.1902): it needs both. With clearance 0.5 mm and r 0.124 mm
        the end is 1.28 mm off; with either value alone it stays in the fill.
      - `USB_GND`, 2 `track_dangling` (KiCad's 0.0997 mm and 0.0925 mm end segments): they are
        one router trace, from (71.6693, 139.6551) to (79.2555, 139.65). With clearance 0.5 mm
        neither end touches the fill (0.21 and 0.23 mm off, cap 0.1 mm). The body crosses the
        fill of both USB_GND F.Cu pours (`USB_PORT_GND`, priorities 71 and 70) and joins them.
        Removing it would split them, so Q16.5's guard (`contacts_are_connected`) keeps it, as
        it must. KiCad flags only the end segments.
      - `Net-(U24-OUT)` via (61.2182, 118.9218): with every value, its F.Cu island in the fill
        model (zone USB_OUT, 5.2 x 2.2 mm) also holds a pin of the net. On the routed board,
        KiCad's USB_OUT fill is one piece, x 56.4-58.9 mm and y 120.0-120.8 mm (1.92 of 7.8 mm2).
        Routed copper severs it: `Net-(J13-CC2)` diagonal across the zone, then a `BUS_GND`
        track and via along y 120.47. The neck the fill model keeps is not found exactly; the
        fill model fills over own-net pads (no thermal relief) where KiCad keeps a gap. Then the
        via's B.Cu island (USB_OUT_POUR, no pin) has no pad in its cluster, and KiCad's island
        removal drops it too.
    - *Fix*: `quality/pour_fill.rs::opening` picks the area by input:
      - On a `.kicad_pcb` (`KicadReportRules::kicad_pcb`, new, set by `KicadReportInputs::of_kicad`)
        it is `zone_of` and `opening_radius` as before.
      - On a DSN it is every conduction area of a net on a signal layer (`signal_pour`, the
        selection `zone_of` already used). The opening comes from
        `KicadReportRules::dsn_pours` (`DsnPourRules`, new, set in `job/prepare.rs::dsn_pour_rules`
        from the settings, µm to board units). The radius is thickness/2 - 1 µm.
      - `drc::opened_fill_islands` (new, `zone_islands.rs`) is `eroded_fill_islands` with every
        foreign obstacle's clearance raised to at least a floor. With floor 0 the operands are
        the same (`fill_operands_eroded`'s new argument; `fill_operands` passes 0, 0).
      - Power layers and the router's own contacts keep the outline. `legacy` does not run the
        cleanup.
      - Module docs: `quality/pour_fill.rs`, `quality/stubs.rs`, `quality/stub_contacts.rs`,
        `drc/kicad_rules.rs`.
      - The GUI's `CLI_ONLY` lists both keys (`dialogs/autoroute_settings.rs`), and
        `tests/settings_defaults.rs` lists them unset.
      - DIVERGENCES: the `cleanup.remove_stubs` entry, "K15.26 does the same ...".
    - *Opening*: unset gives no opening and the router's clearances. KiCad's zone clearance is
      never below the net class clearance the DSN carries, and its opening only removes copper.
      So that fill covers KiCad's, and the cleanup keeps every trace KiCad counts connected.
      None of the fixed defaults is safe on every board: 0.25 mm (KiCad's default
      `min_thickness`), the minimum track width, or KiCad's default 0.5 mm clearance. A zone with
      a smaller value would have fill there, and the cleanup would remove a trace KiCad counts
      connected. A DSN need not come from KiCad either. The plugin knows each zone's values, so
      it is the source. Board-wide values (the smallest of the signal-layer zones) keep the
      rule; per-zone values would be tighter (operator decision in the K15.26 report). Island
      removal is not modelled: a DSN does not say which zones keep their islands (one F.Cu zone
      of the pcu board is `island_removal_mode` 1).
    - *Accept 1*: `crates/freerouting/tests/k15_dsn_pour_fill_stub.rs` (3 tests) on
      `tests/data/quality/k1526-dsn-pour-crossing.dsn`. The board is hand-written with the pcu
      numbers: net class clearance 0.15 mm, track 0.2 mm, pour POUR on F.Cu over x 2-12 mm. A
      CROSS track at x 11.2 mm leaves a strip 0.55 mm wide at 0.15 mm clearance and 0.2 mm wide
      at 0.5 mm. POUR's stub ends at (11.65, -6) mm in that strip.
      - `the_end_beside_the_crossing_is_no_contact_and_the_cleanup_removes_it`: with 500 µm and
        500 µm the end is no contact (the outline's contact is true), and the cleanup removes 1
        trace. Without CROSS the end touches the fill and nothing is removed.
      - `without_the_zones_values_the_strip_stays_a_contact`: unset, the stub stays with and
        without CROSS.
      - `a_pour_on_a_power_layer_keeps_its_outline`.
      - RED: with `opening`'s DSN branch returning `None` (the outline, as at main), the first
        test fails (`left: (true, true)`, `right: (true, false)`). GREEN: 3 passed.
    - *Accept 2*: release CLI of this branch, `target/orchestrator/k1526/runs/arms.sh`. It runs
      K15.24's cut-arm command with this tree's `plugin-input-ab.sh` and plugin:
      `tools/bench/k15-all-pours.options.json`, `-- --max-passes 2 --timeout 240`,
      `ROUTE_TIMEOUT_S=300`, `ROUTE_MEMORY_KB=8000000`. The arms are `dflt` (the plugin's options
      as they are) and `zones` (plus `--router.cleanup.pour_min_thickness_um=400
      --router.cleanup.pour_clearance_um=500`, the pcu board's smallest signal-layer values, as
      the plugin would pass them). Graded by KiCad 10.0.7; runs in
      `target/orchestrator/k1526/runs/pcu-dsn-off-*`.

      | arm | KiCad unconnected | router incomplete | below-minimum | manufacturing | routing-caused | cleanup (vias, traces) | route s | session |
      |---|---|---|---|---|---|---|---|---|
      | K15.24 cut | 47 | 13 | 0 | 0 | via_dangling 2, track_dangling 3 | 20, 2 | 167.7 | `1cf4f61f…` |
      | `dflt` | 47 | 13 | 0 | 0 | via_dangling 2, track_dangling 3 | 20, 2 | 167.3 | `1cf4f61f…` |
      | `zones` | 47 | 16 | 0 | 0 | via_dangling 1 (U24-OUT), track_dangling 2 (USB_GND) | 23, 7 | 167.9 | `55618a37…` |

      `dflt`'s session is byte-identical to K15.24's: with no values the fill model changes no
      judgement on this board. `zones` removes `/CH4_NO`'s and `/CH5_NO`'s stubs. The router's
      incomplete count rises 13 -> 16: it had counted those connections complete through the
      pours' outlines. KiCad already counted them, and its count stays 47. The output
      check (`repair`) finds 0 breaches in both arms. Not met: 0 routing-caused (3 left, causes
      above), and the plugin's default options (they pass no values yet).
    - *Accept 3*: `PARITY_SET=tools/bench/sets/connection-parity.txt tools/parity-check.sh
      --no-probes`: PARITY OK, 7 of 7 `[exact]`, `bytes true`. The `.kicad_pcb` path is unchanged:
      `tests/k15_pour_fill_stub.rs` (K15.23, 4 tests) passes unchanged.
    - *Gate*: `cargo fmt --all --check` clean; `cargo clippy --workspace --all-targets -- -D
      warnings` clean; `cargo test --workspace --no-fail-fast`: 2967 passed, 0 failed, 56 ignored.
      The plugin is unchanged, so no pytest run.
  - *Done 2026-10-22* (K15.33's runs, branch from main `f0cac8f`): Accept 2 holds. The pcu DSN
    arm (K15.24's command as K15.32 runs it, `target/orchestrator/k1533/runs/arms.sh`, the
    plugin's default options, which now pass K15.27's, K15.29's, K15.32's and K15.33's flags),
    graded by KiCad 10.0.7: 0 routing-caused `track_dangling` and `via_dangling`, unconnected 45
    (not up from 47), 0 below-minimum, 0 manufacturing breaches; session `d7dcfd7b...`. Accept 1
    and 3 were met in the Partial note above; K15.33's parity run is 7 of 7 `[exact]` too.
    `USB_GND`'s end segments are no longer flagged on this route; K15.28 stays its own item.

- [x] **K15.27 — The plugin's DSN path passes its signal-layer zones' thickness and clearance.**
  - **Change:** K15.26's cleanup opens a DSN signal-layer pour's fill model by
    `router.cleanup.pour_min_thickness_um` and keeps `router.cleanup.pour_clearance_um` from
    other nets' copper; both are unset unless given, and KiCad's DSN export drops the zones'
    values (the pcu board: `min_thickness` 0.5 mm, 0.4 mm on 2 of 79 F.Cu zones, zone clearance
    0.5 mm, against the DSN's net-class 0.15 mm). The plugin has the zones in hand on its DSN
    path: pass the smallest `min_thickness` and the smallest zone clearance over the board's
    signal-layer copper zones (rule areas left out), as it passes `board_minimums`, and log
    them. Don't pass a value when the board has no signal-layer zone.
  - **Accept:**
    - a plugin test with the stub `pcbnew`: zones of 0.5 and 0.4 mm on F.Cu and one on an inner
      plane layer give `--router.cleanup.pour_min_thickness_um=400` and
      `--router.cleanup.pour_clearance_um=` the smallest clearance; a board without signal-layer
      zones adds neither; the `.kicad_pcb` input adds neither;
    - the pcu board's DSN arm (K15.26's `target/orchestrator/k1526/runs/arms.sh` `dflt` arm,
      the plugin's default options, a few minutes), graded by KiCad: the `/CH4_NO` and `/CH5_NO`
      items gone as in K15.26's `zones` arm, unconnected not up (47), 0 below-minimum, 0
      manufacturing breaches.
  - *Done 2026-10-22* (branch from main `537f1e8`; the numbers are the first seen, before any
    later measurement):
    - *Plugin*: `export_flow.pour_rule_args` (called last in `prepare`, after the fix-ups; the
      DSN input only) with `kicad_board.signal_pour_rules` and `options.pour_rule_args`. It takes
      the smallest `GetMinThickness()` and zone clearance (`GetLocalClearance()`; a zone without
      its own counts 0.5 mm, `ZONE_CLEARANCE_NM`, KiCad's `ZONE_SETTINGS` default, as
      K15.24's `_clearance` takes it) over the board's zones that are no rule area and lie on a
      layer of the exported DSN typed `signal`, `jumper` or none. Reading the DSN gives the layers
      the router takes as signal (`structure.rs`: `power` is a plane, a `mixed` layer is not
      loaded), after the fix-ups, so a power layer opened by `route_on_plane_layers` counts. Footprint zones are
      left out (the export writes the board's). A value of 0 passes no flag. The flags come before
      the stage choices; arguments after `--` in `headless_run.py` come later and win (the
      dialog has no free-form field, so no other board-derived flag has a user override to
      follow). One log line names the zones, layers and values.
    - *Accept 1*: `kicad-plugin/tests/test_pour_rules.py` (9 tests, stub pcbnew). F.Cu zones of
      0.5 and 0.4 mm plus an In1.Cu plane zone (0.2 mm) give `--router.cleanup.pour_min_thickness_um=400`
      and `--router.cleanup.pour_clearance_um=500` (the smallest clearance of the F.Cu/B.Cu
      zones); a board with only plane zones, a rule area, a `mixed`-layer zone or no zone adds
      neither; `prepare_snapshot` (the `.kicad_pcb` input) adds neither. RED: with the call in
      `prepare` replaced by `[], []`, 7 failed, 2 passed (the two negative tests). GREEN: 9 passed.
    - *Accept 2*: `target/orchestrator/k1527/runs/arms.sh` (K15.26's `dflt` command, this tree's
      script and plugin, a release CLI from main `537f1e8`, no extra router flags), graded by
      KiCad 10.0.7; `runs/pcu-dsn-off-dflt`. The plugin log line: `signal-layer zones: 83 on
      F.Cu, B.Cu; the cleanup judges their pours by min thickness 400 um and clearance 500 um
      (--router.cleanup.pour_min_thickness_um=400 --router.cleanup.pour_clearance_um=500)`.

      | arm | KiCad unconnected | router incomplete | below-minimum | manufacturing | routing-caused | route s | session |
      |---|---|---|---|---|---|---|---|
      | K15.26 `dflt` (no values) | 47 | 13 | 0 | 0 | via_dangling 2 (U24-OUT, /CH5_NO), track_dangling 3 (USB_GND x2, /CH4_NO) | 167.3 | `1cf4f61f...` |
      | K15.26 `zones` (flags by hand) | 47 | 16 | 0 | 0 | via_dangling 1, track_dangling 2 | 167.9 | `55618a37...` |
      | K15.27 (the plugin's flags) | 47 | 16 | 0 | 0 | via_dangling 1 (U24-OUT), track_dangling 2 (USB_GND) | 168.6 | `55618a37...` |

      The session is byte-identical to K15.26's `zones` arm: the `/CH4_NO` track and `/CH5_NO` via
      items are gone, and the remaining 3 are K15.28's (USB_GND) and K15.29's (U24-OUT).
    - *Gate*: `python3 -m pytest kicad-plugin/tests` (`FREEROUTING_RS_BIN` = the debug build dir):
      703 passed, 7 skipped (pcbnew x4, canary-pwr board, KICAD_METADATA_REPO x2). No `crates/`
      change, so no cargo gate.

- [x] **K15.28 — The cleanup trims a trace's dangling end segment.**
  - **Change:** on K15.26's pcu DSN `zones` arm (`target/orchestrator/k1526/runs/`) KiCad
    reports 2 `track_dangling` on USB_GND: the 0.0997 mm and 0.0925 mm end segments of one
    router trace from (71.6693, 139.6551) to (79.2555, 139.65) mm. Its body crosses the fill of
    both USB_GND F.Cu pours and joins them, so Q16.5's guard (`contacts_are_connected`) keeps
    the whole trace, as it must; only the ends dangle. Let the cleanup (`quality/stubs.rs`) cut
    a trace back from an end with no contact to its last corner or point that has one, when
    the rest keeps every contact it joins. Both inputs.
  - **Accept:**
    - a test on a small board: a trace whose body joins two pours and whose end overhangs the
      fill by less than a segment keeps its body and loses the overhang; a trace whose end has
      a contact is unchanged;
    - the pcu DSN `zones` arm again, graded by KiCad: 0 USB_GND `track_dangling`, unconnected not
      up (47), 0 below-minimum, 0 manufacturing breaches; the `.kicad_pcb` arm (K15.23's
      command): unconnected not up (47), 0 routing-caused;
    - `legacy` unchanged (the cleanup is `enhanced` only).
  - *Done 2026-10-22* (decided: the trim is not landed, its premise is gone). The trim was built on
    branch `k1528` (`quality/stub_trim.rs`: a trace the guard keeps is cut back from an end whose
    cap meets no contact to the first point whose cap lies in its net's copper;
    `tests/k15_trim_dangling_end.rs` on `tests/data/quality/k1528-trace-joins-two-pours.dsn`,
    passing). Rebased on main `4a3a864` (K15.33) and built in release
    (`target/orchestrator/k1528/runs/arms-r2.sh`), it cuts back 0 trace ends on every real board
    run, so it changes no route:
    - pcu DSN arm (K15.33's command, plugin defaults): "cut back 0 dangling trace end(s)",
      session `d7dcfd7b...`, byte-identical to main's (K15.33's run); KiCad 10.0.7: unconnected 45,
      0 routing-caused (no `USB_GND` `track_dangling`: K15.29 and K15.33's fill model changed the
      route that had them), 0 below-minimum, 0 manufacturing breaches;
    - pcu `.kicad_pcb` arm (K15.23's command): cut back 0, session `bd0a87fd...`, identical to
      main's; unconnected 47, 0 routing-caused;
    - canary-pwr `95882f00` `.kicad_pcb` (K15.14's `pcb-off` command): cut back 0; unconnected 40,
      0 routing-caused, 0 below-minimum, 0 manufacturing breaches.

    Accept 2's KiCad clauses hold on main without the trim, and `legacy` is untouched (nothing
    landed). A cleanup step that changes no measured route is left out. The rebased commit is kept
    as `target/orchestrator/k1528/k1528-trim-r2.patch` (`git am` on `4a3a864`), for a board
    whose route leaves a dangling end segment on a kept trace.

- [x] **K15.29 — A via in a pour island KiCad drops is no contact in the cleanup.**
  - **Change:** on K15.26's pcu DSN arms KiCad reports a `Net-(U24-OUT)` `via_dangling` at
    (61.2182, 118.9218) mm for every opening tried: the fill model's F.Cu island of zone USB_OUT
    there also holds a pin of the net, but KiCad's routed fill is one piece at x 56.4-58.9,
    y 120.0-120.8 mm (1.92 of 7.8 mm2), cut by a `Net-(J13-CC2)` diagonal and a `BUS_GND` track
    and via along y 120.47. The fill model fills over own-net pads where KiCad keeps a thermal
    relief gap (unconfirmed), and the via's B.Cu island (USB_OUT_POUR, no pin) is one KiCad's
    island removal drops. Find which neck the fill model keeps that KiCad's fill doesn't, and
    make the cleanup's fill model (both inputs) agree, keeping its rule that it keeps a trace
    KiCad might still call dangling, never the reverse.
  - **Accept:**
    - a test on a small board cut from the pcu route (named in the test) where the via's island
      is cut off as KiCad cuts it: the via is no contact and the cleanup removes it;
    - the pcu DSN arm (K15.26's `zones` arm, or K15.27's default), graded by KiCad: no
      U24-OUT `via_dangling`, unconnected not up (47), 0 below-minimum, 0 manufacturing breaches;
      no USB_GND `track_dangling` (added 2026-10-22 from K15.28);
      the `.kicad_pcb` arm (K15.23's command): unconnected not up (47), 0 routing-caused;
    - `legacy` unchanged.
  - *Done 2026-10-22* (branch from main `58e8ec7`; every number below is the first seen, before
    any later measurement):
    - *Cause:* KiCad's island removal, not a neck and not a thermal relief. Zone USB_OUT
      (`Net-(U24-OUT)`, F.Cu, priority 37) connects pads solid (`GetPadConnection()` 2, FULL), so
      U24's pad 3 has no relief gap. Refilled with `island_removal_mode` 1 (never), KiCad gives
      USB_OUT a second piece at x 59.0-61.6, y 118.6-120.24 mm, 4.066 mm2. That piece holds the
      via at (61.2182, 118.9218) and is `IsIsland` (no pad). It is cut from pad 3's piece (x
      56.4-58.9) by `BUS_GND`'s via at (59.2922, 120.4697) and its track along y 120.4697. With
      mode 0 (always) the filler removes it. USB_OUT_POUR (B.Cu) keeps its pieces: none has a
      pad, and KiCad keeps all the islands of a zone layer when every one is isolated (K15.16).
      So the via is connected on B.Cu only.
      - The cleanup's opened fill already split the two (the probe ran on K15.27's DSN and SES
        with r 199 µm, floor 500 µm). The via's island is x 59.35-61.40, y 118.80-119.67 before
        growth, the pin's island x 56.84-58.30, y 120.55-120.60.
      - K15.26's "the island also holds a pin" came from `drc::conduction_fill`, the unopened
        fill. With the router's clearance and no opening, the unopened fill does join them, near
        (59.0, 120.0).
      - The cleanup kept both islands, because no island removal was modelled
        (`quality/pour_fill.rs`, the module docs before this change: "Neither path removes the
        fill islands no pad reaches").
      - USB_GND (K15.28's two `track_dangling`; scope added 2026-10-22) has the same cause. On
        K15.27's routed board, KiCad's mode 1 refill shows a USB_PORT_GND F.Cu island under the
        trace (2.88 mm2, `IsIsland`, holding the end (71.6693, 139.6551) and the body at
        (72.5, 139.5846)). Mode 0 removes it, and the other pour has no fill at (79.0, 139.5846)
        in either mode. In KiCad's view the trace joins nothing.
      - Found on the way, not changed here: the pcu board's `.kicad_dru` has an unconditional
        `(constraint clearance (min 0.127mm))`, and KiCad 10.0.7's filler uses it in place of the
        zones' 0.5 mm. On `b-no-bus-gnd` the USB_OUT fill is 7.701 mm2 with the rule and 0.893
        mm2 without it; the fill keeps 0.127 mm from the `Net-(J13-CC2)` track. So K15.27's
        `pour_clearance_um=500` is larger than what KiCad applies here (see the report).
    - *Fix:* `quality/pour_islands.rs` (new). `PourFills::pieces` now leaves out the pieces of
      islands that KiCad's filler removes; `all_pieces` keeps every island. Which islands go is
      judged on an island model, the same opened fill with the router's clearances and no
      `pour_clearance_um` floor (`PourFills::island_model`), so a floor larger than KiCad's
      clearance cannot invent an island. An island of the model is dropped when all of these
      hold:
      - the zone removes islands: `island_removal_mode` 0 on a `.kicad_pcb`; on a DSN the new
        `router.cleanup.pour_island_removal`, unless the net is in the new
        `pour_islands_kept_nets`;
      - a search over the net's copper (pins, traces, vias, the other pours' model islands; shapes
        meet on a layer) reaches no pin;
      - another island of the area meets a pin's pad;
      - the net has no dropped pour.

      Islands are the connected components of the pieces. The cache is forgotten at each removal.
      The plugin (`kicad_board.signal_pour_rules`, `options.pour_rule_args`,
      `export_flow.pour_rule_args`) passes `--router.cleanup.pour_island_removal=true` and
      `--router.cleanup.pour_islands_kept_nets=` the nets of signal-layer zones whose mode is not
      0. A mode that cannot be read, or a net name the comma list cannot carry, passes neither.
      On the pcu board that is `Net-(U1-SW)` (BUCK_SW). Both settings are unset in both profiles;
      the GUI's `CLI_ONLY` and `tests/settings_defaults.rs` list them. A DSN has no mode, so the
      plugin is the source, as for K15.27's values. Module docs: `quality/pour_islands.rs`,
      `quality/pour_fill.rs`, `quality/stub_contacts.rs`, `drc/kicad_rules.rs`. DIVERGENCES: the
      `cleanup.remove_stubs` entry, "K15.29 leaves out ...". `kicad-plugin.md` is updated.
    - *Accept 1:* `crates/freerouting/tests/k15_pour_island_via.rs` (4 tests) on
      `tests/data/kicad/pcu-k1529/`, written by `make_boards.py` from the pcu route's coordinates:
      USB_OUT and USB_OUT_POUR verbatim, the via, `BUS_GND`'s track and via, and the
      `Net-(J13-CC2)` diagonal. Each board is there as KiCad 10.0.7 `pcb drc --refill-zones
      --save-board` saved it, plus KiCad's Specctra export (`.dsn`).
      - KiCad on `a-via-island-cut-off`: `via_dangling` "Via [Net-(U24-OUT)] on F.Cu - B.Cu" at
        (61.2182, 118.9218). On `b-no-bus-gnd`: none for the net.
      - `the_via_in_the_cut_off_island_is_no_contact_on_the_kicad_pcb`: the outline contact layers
        are {F.Cu, B.Cu} on both boards. The cleanup's are {B.Cu} on a, so it removes the via; on
        b they are {F.Cu, B.Cu} and nothing is removed.
      - `the_via_in_the_cut_off_island_is_no_contact_on_the_dsn`: the same on the DSNs, with 500,
        500 and island removal given.
      - `a_zone_that_keeps_its_islands_keeps_the_via` (mode 1) and
        `without_island_removal_the_dsn_keeps_the_via` (unset, or the net kept): the via stays.
      - A unit test in `pour_islands.rs`, `islands_are_the_pieces_that_meet`.
      - RED 1: with `removes_islands` forced false, the two `the_via_in_...` tests fail (`left:
        ({0, 3}, {0, 3})`, `right: ({0, 3}, {3})`).
      - RED 2: with the islands judged on the floored fill itself (the first version), the DSN
        test fails on b (`left: ({0, 3}, {3})`): the 0.5 mm floor splits USB_OUT where KiCad's
        fill is whole.
      - GREEN: 4 passed.
      - Plugin: `kicad-plugin/tests/test_pour_islands.py` (4 tests). RED with the removal forced
        off in `export_flow`: 2 failed. GREEN: 4 passed, and `test_pour_rules.py` (9) passes
        unchanged.
    - *Accept 2:* release CLI of this branch, graded by KiCad 10.0.7.
      - The DSN arm is `target/orchestrator/k1529/runs/arms.sh`: K15.27's `dflt` command with
        this tree's script and plugin, which passes `--router.cleanup.pour_island_removal=true
        --router.cleanup.pour_islands_kept_nets=Net-(U1-SW)` after K15.27's two flags.
      - The `.kicad_pcb` arm is `runs/arms-pcb.sh`: K15.23's `kicad_pcb:off` command, with
        `k154/opts-default.json`, `-- --max-passes 2 --timeout 240`, `ROUTE_TIMEOUT_S=300` and
        `ROUTE_MEMORY_KB=8000000`.

      | arm | KiCad unconnected | router incomplete | below-minimum | manufacturing | routing-caused | cleanup (vias, traces) | route s | session |
      |---|---|---|---|---|---|---|---|---|
      | DSN, K15.27 | 47 | 16 | 0 | 0 | via_dangling 1 (U24-OUT), track_dangling 2 (USB_GND) | 23, 7 | 168.6 | `55618a37…` |
      | DSN, K15.29 | 45 | 18 | 0 | 0 | none | 24, 8 | 182.6 | `4d183cdf…` |
      | `.kicad_pcb`, K15.23 | 47 | 11 | 0 | 0 | none | 24, 7 | 181.1 | `bd0a87fd…` |
      | `.kicad_pcb`, K15.29 | 47 | 11 | 0 | 0 | none | 24, 7 | 166.7 | `bd0a87fd…` |

      - The DSN route log is the same as K15.27's up to the cleanup (log texts diffed). The
        cleanup removes 2 more items:
        - the U24-OUT via at (612182, -1189218);
        - USB_GND's whole trace, (716693, -1396551) to (792555, -1396500). Its two pour contacts
          are islands the model drops, so its contacts are none.
      - The router's incomplete count rises 16 -> 18 because it counted through the outlines.
        KiCad's unconnected count falls 47 -> 45.
      - The `.kicad_pcb` session is byte-identical to K15.23's.
      - Route time: the DSN arm's earlier phases were already about 10 s slower than K15.27's on
        a busy machine. The cleanup itself, timed in a release probe on K15.27's routed board
        with its 1442 traces and vias unfixed, takes 34-39 ms without island removal and
        261-263 ms with it (3 runs each).
    - *Accept 3:* `PARITY_SET=tools/bench/sets/connection-parity.txt tools/parity-check.sh
      --no-probes`: PARITY OK, 7 of 7 `[exact]`, `bytes true`. `legacy` does not run the cleanup,
      and `fill_operands` is unchanged.
    - *Gate:* `cargo fmt --all --check` clean; `cargo clippy --workspace --all-targets -- -D
      warnings` clean; `cargo test --workspace --no-fail-fast`: 2972 passed, 0 failed, 56 ignored;
      `pytest -rs kicad-plugin/tests` (`FREEROUTING_RS_BIN` = this branch's release directory): 707
      passed, 7 skipped (pcbnew x4, canary-pwr board, KICAD_METADATA_REPO x2).

- [x] **K15.30 — Measure `pour_fill_target` under `enhanced`.**
  - **Change:** K15.25's `router.pour_fill_target` (the search aims for a `.kicad_pcb` signal
    pour's opened fill; the cycle rule keeps only a bridge into it) is opt-in. On the pcu board
    KiCad's unconnected goes 47 -> 49 with it, on canary-pwr (`95882f00`) 40 -> 31, with 0
    routing-caused items in each arm (K15.25's note). Preregister a measurement of `enhanced`
    with the setting unset against set on K15.14's set (`tools/bench/sets/k15-all-pours.txt`,
    the `.kicad_pcb` input; the setting does not act on a DSN), with K15.14's guards
    (manufacturing breaches, below-minimum, routing-caused DRC, route time) and a decision table:
    kept, it joins `ENHANCED`; not kept, the plugin offers it as an option (as K15.14). The run
    is a heavy run, through the orchestrator.
  - **Accept:**
    - the prereg (`docs/roadmap/prereg/K15.30.json`, its set and its pin test) committed before
      any run of the set;
    - the registered run's records and the gate's verdict in the note, and the decision table's
      outcome applied (an `ENHANCED` entry and its DIVERGENCES profile line, or the plugin option
      with a test);
    - `legacy` unchanged.
  - *Partial 2026-10-22:* Accept 1 is written (registered at main `da87319`, to be committed before
    any run of the set); Accept 2 and 3 remain here, in K15.30: the registered run (K13.5's
    `plugin-input-ab.sh` per call) is a heavy run, queued by the orchestrator in
    `target/orchestrator/pending-heavy-runs.md`. **No number of this measurement exists**: nothing was routed or graded, no
    kicad-cli ran.
    - *Files:* `docs/roadmap/prereg/K15.30.json` (id `K15.30-fill-target`) sha256
      `209d8c14ace2015f583591c93ae735cc08f2cc518b6b7c5f8481cb4a0df32a6b`, 0 amendments;
      `tools/bench/sets/k15-fill-target.txt` (K15.14's 17 rows verbatim under a header naming
      K15.30) sha256 `b7339dd3138342e9ba446b331dd89956901041c1e08ac7443486bcbdac464a6f`; the pin
      test `crates/freerouting-dev/tests/prereg_k15_30.rs`. The helper is K15.14's
      `tools/bench/k15_all_pours.py`, unchanged (`8de88dd8…`): `run` runs a registration's
      `commands` verbatim and `collect` reads the arms and metrics it lists, so nothing in it is
      K15.14's flag or arms.
    - *Registration:* K15.14's with the differences it lists (`differences`): arms `pcb-off`
      (baseline, setting unset) and `pcb-on` (`--router.pour_fill_target=true`), the `.kicad_pcb`
      input only (`no_dsn_why`: `fill_tiles` and `area_joins_trace_ends` give Java's rule unless
      `read_from_kicad_pcb`); every command is K15.14's `pcb-*` command of the row with
      `target/orchestrator/k1530/` for the tree, output and build, and this flag (`--max-passes 2
      --timeout 240`, `ROUTE_TIMEOUT_S=300`, `ROUTE_MEMORY_KB=8000000`, options `{"schema": 6}`,
      KiCad 10.0.7, the boards at K15.14's paths under `target/orchestrator/k1514/boards/`, all 17
      present with their hashes on 2026-10-22). Totals rule `improve`, K15.14's noise (canary-pwr
      10, others max(2, ceil(5 % of connections)): 67.09 over K15.14's 16 judged rows, 72.32 over
      all 17), its guards (0 manufacturing breaches, below-minimum and vias in pads in every arm;
      `drc_real` not up; no row worse than its band; route time at most 1.25x), `must_judge`
      pcu-76163c7 and canary-pwr-95882f00, validity (a) from the result manifests'
      `settings_snapshot.pour_fill_target`, `pour_guard_pours` dropped. Decision table: not
      measured, breach (opt-in, not offered until fixed), not kept no gain / not kept guard (unset
      in both profiles, the plugin offers it), kept (`ENHANCED` entry, its test, DIVERGENCES
      profile line, parity 7 of 7). `seen_before_registration`: K15.25's runs (pcu 47 -> 49,
      canary-pwr 40 -> 31, and its other runs of the setting), K15.14's amendment-3 run, and
      Zone-Zone pcu 36 -> 36, canary-pwr 18 -> 13 from a `collect` format check on K15.25's arms.
    - *Calls and time:* 34 calls (17 rows x 2). K15.14's amendment-3 run took about 38 min for 38
      calls (16 min `pcb-off`, 14 min `pcb-on`, 8 min DSN); with K15.25's -2 % to +13 % route time
      for the setting, about 32-36 min expected, at most 3.1 h if every call hit its 300 s cap.
    - *Pin test:* `cargo test -p freerouting-dev --test prereg_k15_30`: RED 9 failed before the
      registration existed (`docs/roadmap/prereg/K15.30.json: No such file`); GREEN 9 passed (the
      registration's and the set's hashes, the rows, boards and rules files equal to K15.14's and
      hashing as registered, the 34 commands against their templates and K15.14's `pcb-*`
      commands, the helper's `run --dry-run` printing exactly them, the script and helper hashes,
      the rule, noise, guards, validity, `must_judge` and decision table, 0 amendments).
      `--test prereg_k15_14`: 8 passed, unchanged.
    - heavy run pending: `cd /home/thebo/code/freerouting-rs && git worktree add --detach
      target/orchestrator/k1530/tree <K15_30_COMMIT> && cd target/orchestrator/k1530/tree &&
      CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
      CARGO_TARGET_DIR=/home/thebo/code/freerouting-rs/target/orchestrator/k1530/target-release
      cargo build --release -p freerouting-cli -p freerouting-dev` and `cargo test --release -p
      freerouting-dev --test prereg_k15_30` (same environment); then `cd
      /home/thebo/code/freerouting-rs && python3 -I
      target/orchestrator/k1530/tree/tools/bench/k15_all_pours.py prepare
      target/orchestrator/k1530/tree/docs/roadmap/prereg/K15.30.json` (every row `present`); then
      amendment 1 (the commit, the binary, the grader and plugin hashes) with the pin updated,
      committed to main; then `mkdir -p target/orchestrator/k1530/out && python3 -I
      target/orchestrator/k1530/tree/tools/bench/k15_all_pours.py run
      target/orchestrator/k1530/tree/docs/roadmap/prereg/K15.30.json >
      target/orchestrator/k1530/out/run.log 2>&1; echo "EXIT $?" >>
      target/orchestrator/k1530/out/run.log`; then `collect` and the two gate calls
      (`--write-baseline`, `--all-pairs`, `--prereg docs/roadmap/prereg/K15.30.json` as amended,
      `--left-out ROW=REASON` for a row validity (b) leaves out) as `heavy_run` spells out. Keep
      `target/orchestrator/k1514/boards/` until this run is done.
    - *Run 2026-10-22* (amendment 1, abcaf61: K15_30_COMMIT `fb59eb5`, binary `43f10fb7…`, all 17
      rows present): 34 calls, every one EXIT 0, 04:14-04:48 (34 min), into
      `target/orchestrator/k1530/out/` (`run.log`, `records.json`, `baseline.json`,
      `gate-baseline.txt`, `gate-all-pairs.txt`). Validity (a): every `pcb-on` manifest has
      `pour_fill_target` true, no `pcb-off` one has it. Validity (b) leaves out `pocketsdr-v3-0`
      (both arms TIMED_OUT at `--timeout 240`; graded, hard limits 0), passed to both gate calls as
      `--left-out`; 16 rows judged, pcu and canary-pwr among them. Guard (1): 0 manufacturing
      breaches and 0 below-minimum in all 34 graded arms. KiCad unconnected, `pcb-off` -> `pcb-on`
      (routing-caused `drc_real` in brackets where it changed):

      | row | off | on | change | band |
      |---|---|---|---|---|
      | pcu-76163c7 | 47 | 49 | +2 | 43 |
      | canary-pwr-95882f00 | 40 | 31 | -9 | 10 |
      | complex_hierarchy | 81 | 80 | -1 | 14 |
      | moteus-moteus-c1 (drc_real 4 -> 9) | 40 | 39 | -1 | 22 |
      | nudac-nudac (drc_real 1 -> 0) | 11 | 12 | +1 | 9 |
      | solarmeshtasticnode-meshtasticnode-bq25185 | 10 | 9 | -1 | 6 |
      | wisweep-driver-board (drc_real 4 -> 3) | 27 | 29 | +2 | 11 |
      | the 9 other judged rows | 114 | 114 | 0 | |

      Total 370 -> 363 (-7) against the set noise 67.09: `freerouting-dev gate --all-pairs`
      "FAILS: the improvement 7 does not exceed the set noise 67.09", totals verdict reject, pair
      `pcb-off -> pcb-on` FAIL. Decision table, in its order: not "not measured" (16 rows, both
      `must_judge` rows judged, amendment 1 before the first arm), not "breach" (guard (1) holds in
      every arm), so **"not kept, no gain"**: `router.pour_fill_target` stays opt-in, unset in both
      profiles, and the plugin offers it as an option (the outcome is applied in the next change).

    - *Outcome applied 2026-10-22:* the plugin option `RouteOptions.pour_fill_target` (off in both
      profiles; `to_args` adds `--router.pour_fill_target=true` only when on; `from_dict`,
      `from_form`; options file schema stays 6, a file without the key means off), the Advanced
      tab's Flow group checkbox "End traces where KiCad fills a pour" (below "Keep other nets out
      of every pour") with a tooltip giving the measurement without private board names (370 -> 363 over 16
      boards, one board 40 -> 31, the power-control board 47 -> 49) and that it applies to the
      board snapshot input only. DIVERGENCES' `pour_fill_target` entry and
      `autoroute/pour_target.rs`'s "Not in `enhanced`" docs carry the numbers; the plugin handover
      (`docs/development/kicad-plugin.md`) lists the option.
    - *Done 2026-10-22:* (1) the registration was committed before any run: `fb59eb5`, amendment 1
      `abcaf61`, the run at `fb59eb5`'s binary; `cargo test -p freerouting-dev --test prereg_k15_30`:
      9 passed. (2) The records and the verdict ("not kept, no gain": 370 -> 363 against the noise
      67.09, 16 rows) are in the *Run 2026-10-22* note above, and the outcome is applied (the plugin
      option above). Tests, RED first (5 failed: `test_options.py::test_fill_target_*` x3,
      `test_route_dialog.py::test_the_fill_target_checkbox_is_saved_and_reaches_the_router`,
      `test_snapshot.py::test_every_k09_option_is_either_checked_or_allowed`), then GREEN 141 passed
      in those three files; the option is off in both profiles and the arguments differ by exactly
      the flag when on, the options file round-trips (schema 6, every readable schema loads it off, a
      bad value keeps it off), the form sets it, the dialog shows the checkbox on the Advanced tab's
      Flow group, writes the field, and the flag reaches the router. `pytest -rs kicad-plugin/tests`
      (`FREEROUTING_RS_BIN` = the debug build directory): 711 passed, 7 skipped (pcbnew x4,
      canary-pwr board, KICAD_METADATA_REPO x2). (3) `legacy` unchanged: nothing under `crates/`
      but module docs changed; `PARITY_SET=tools/bench/sets/connection-parity.txt
      tools/parity-check.sh --no-probes`: connection parity PASS, 7 of 7 exact (55 s);
      `cargo test -p freerouting --test settings_defaults`: 9 passed. `cargo fmt --all --check`
      clean.

- [x] **K15.31 — The router counts a pad on a `.kicad_pcb` signal pour only where it is filled.**
  - **Change:** the router's pour contacts are Java's (`BasicBoard::conduction_area_normal_contacts`,
    `ConductionArea.getNormalContacts`): every pin and via centred in the outline and every trace
    ending there. On K15.25's pcu runs with `pour_fill_target` (`target/orchestrator/k1525/runs/b/`)
    KiCad reports items the router counts as joined through a pour's outline where KiCad's fill
    does not reach: `/CH2_NO` U4 pad 8 at (137.343, 96.3) mm and `/CH5_NO` R11 pad 1 (K15.25's
    note). Under `enhanced` on a `.kicad_pcb`, let the router's connectivity (the incomplete
    count and the connections it routes) take a signal-layer pour's contacts from its opened fill
    (`quality/pour_fill.rs`), so a pad the fill does not reach is routed to, with `legacy`
    unchanged. Behind a setting; its default needs a measurement (K15.30's set) unless it costs
    nothing on both boards.
  - **Accept:**
    - a test on a small board cut from the pcu route: a pad in the outline outside the opened fill
      is not connected through the pour, and the router routes it or counts it open;
    - the pcu and canary-pwr `.kicad_pcb` routes (K15.25's commands, a few minutes each) with the
      setting, alone and with `pour_fill_target`, graded by KiCad: unconnected, the router's
      incomplete against it, 0 routing-caused, 0 below-minimum, 0 manufacturing breaches;
    - `legacy` unchanged (parity 7 of 7).
  - *Partial 2026-10-22* (worktree branch `k1531`, rebased onto main `48fad8c`; every number
    below is the first seen, before any later measurement): accept 1 and 3 hold; accept 2 holds on
    every arm but one: canary-pwr with both settings has 1 routing-caused `track_dangling`
    (`/Supervisor/CAN_EN`, no pour net, below). *Remains:* that stub (K15.36), and the setting's
    default, which needs a measurement on K15.30's set (a prereg), with both settings together
    (K15.37; the setting alone is worse, below). Decided 2026-10-22: the two stay separate
    settings, documented as meant together (`router.pour_fill_contacts` does not switch on
    `pour_fill_target`), so each stays configurable on its own; K15.37 measures the pair.
    - *Setting:* `router.pour_fill_contacts` (bool; unset or `false` is Java's rule; unset in
      `legacy` and `enhanced`, not in `ENHANCED`). Documented in `settings/fields.rs`,
      `tests/settings_defaults.rs` (unset), the GUI's no-control list, the module docs of the new
      `board/pour_contacts.rs` (and `autoroute/pour_target.rs`, `quality/pour_fill.rs`), and
      DIVERGENCES (board model, `pour_fill_contacts`). `prepare_job` sets it on the board;
      snapshots keep it (`BoardSnapshot::pour_fill_contacts`); it acts only on a `.kicad_pcb`.
    - *Cause:* the router's connectivity joins a conduction area to every Java contact inside its
      outline: `BasicBoard::conduction_area_normal_contacts` (`board/basic_board.rs`, the pin/via
      and trace arms test `contains` on the outline) feeds `connected_set_recu`, which
      `NetIncompletes` (`drc/incompletes.rs`), the routing ledger (`drc/ledger.rs`) and the batch
      router's `route_sets` (`autoroute/batch/connection.rs`) use. On the pcu route (K15.25's
      `final-true`) `VCC_3V3`'s tracks cut EFUSE_NO_2 in three; U4 pad 8's track ends in the top
      left part and pad 10's in the right one, and the router counted `/CH2_NO` complete.
    - *Design:* the contacts change only in the connected sets (`connectivity_contacts` in
      `connected_set_recu`, unless `stop_at_plane`); `normal_contacts` stays Java's. Per judged
      area, `fill_geometry` takes `opened_fill_core` and grows each core tile back by the radius
      (as `PourFills::all_pieces`); the islands come from `pour_islands::island_numbers` (split
      out of `islands_of`, same output). A Java contact point (drill centre, trace end, inside the
      outline) belongs to the island whose piece holds it. **Islands:** each island is its own
      connected group. The area item stands for its main island (pin contacts first, then any
      contact, then the largest area) and its contacts are that island's members; the members of
      every other island are in contact with each other directly; a point in no island joins
      nothing. All islands count, not only K15.29's kept ones (`PourFills::pieces`): an island
      KiCad removes has a copper group with no pad, so counting it joins no pad to another, and
      K15.29's test is a search over the net's copper that would run at every change. **Cycle
      rule:** Java's insertion removed every trace the search found between two islands (both
      ends in the outline), and `Trace.isCycle` then removed the vias and the inner-layer trace
      between them (seen on the fixture with a temporary print of the located paths: "Routed"
      with no new copper). So `area_joins_trace_ends` (`autoroute/pour_target.rs`) joins two ends
      only in one island, and `BasicBoard::pour_cycle` (a search over the island-aware contacts)
      answers `isCycle` where it can differ from Java's (a Java cycle, or a trace with an end in
      a judged pour's outline). **Search:** with `pour_fill_target` too, `fill_tiles` gives the
      main island's core only.
    - *Keeping the fill current:* the fill and the contacts are cached on the board
      (`PourContacts`, behind a `Mutex` like the ledger). Every `note_*` hook (insert, remove,
      change, net change, undo/redo) and every search-tree insert/remove/reinsert forgets them for
      each judged area on the item's layers whose outline tiles, grown by `pour_fill::reach`,
      meet the item's box, and marks that area's nets dirty in the ledger. An item of the area's
      own net forgets only the contacts (the opening takes none of its own copper). `PourFills`'
      cache (forgotten only at removals) is not used. Cost: a fill model is about 25 ms per pour
      on canary-pwr, about 2000 of them per route (temporary counters). The first version took
      203 s on canary-pwr with both settings; after the own-net, tile-box and cycle changes it
      took 153 s with the same session (`cce88912…`). The tree hooks changed no session with both
      settings. The first version's canary-pwr arm with the setting alone (84 unconnected,
      87 incomplete, 4 `track_dangling`, session `428207a7…`) became 38/39/0 (`ca3becbe…`), so
      that first arm read a stale fill somewhere.
    - *Accept 1:* `crates/freerouting/tests/k15_pour_fill_contacts.rs` (3 tests) on
      `tests/data/kicad/pcu-k1531/`. `make_boards.py` takes zone EFUSE_NO_2 verbatim and the
      copper and pads around it from K15.25's `runs/b/final-true`; each board is as KiCad 10.0.7
      `pcb drc --refill-zones --save-board` saved it. KiCad: `a-pads-in-three-islands` has 2
      unconnected `/CH2_NO` items, `b-no-vcc-cut` none.
      - `a_pad_the_fill_does_not_reach_is_not_joined_through_the_pour`: unset or `false`, both
        boards 0 incomplete (recount and ledger) and U4 pad 8 joined to all 8 pins; with the
        setting, `b` 0 and `a` 2: {U4-8, U17-18..20, R85-2}, {C54-1}, {U4-10, R8-1};
      - `with_the_setting_the_router_routes_the_islands_together_or_counts_them_open` (the board
        without `/CH2_NO`'s tracks, `enhanced`, 3 passes): unset 0 incomplete, `false` the same
        copper; the setting alone the same copper and 2 open; with `pour_fill_target` 0 and all
        pins joined. The release CLI on the same board, graded by KiCad
        (`target/orchestrator/k1531/scratch/fx/`): unset 2 `/CH2_NO` unconnected, alone 2 (the
        router counts them), both 0;
      - `the_islands_are_recomputed_when_the_board_changes`: inserting the `VCC_3V3` tracks into
        `b` makes 3 islands, and the ledger (built before) counts 2.
      - RED: with islands off (`pour_islands` returning `None`) 3 failed; with no forgetting
        2 failed (islands 1 vs 3; `(1, 1)` vs `(0, 0)`); with Java's `isCycle` the routing test
        failed (`(2, 2)` vs `(0, 0)`). GREEN: 3 passed; `k15_pour_fill_stub` (6) and
        `k15_pour_island_via` (4) unchanged.
    - *Accept 2:* K15.25's commands (`plugin-input-ab.sh kicad_pcb ... -- --max-passes 2
      --timeout 240`, `ROUTE_TIMEOUT_S=300`, `ROUTE_MEMORY_KB=8000000`;
      `target/orchestrator/k1531/scripts/pcu3.sh`, `canary3.sh`), release binary of this tree on
      main `48fad8c` (`bin/v3`), graded by KiCad 10.0.7, runs in
      `target/orchestrator/k1531/runs/rebased-pcu/` and `rebased-canary/`. Every session is
      byte-identical to the same arm on base `c7c4e07` (`bin/v2`, `runs/final-*`, run first):

      | board, arm | KiCad unconnected | router incomplete | routing-caused | below-min | mfg | route s | session |
      |---|---|---|---|---|---|---|---|
      | pcu, unset | 47 | 11 | none | 0 | 0 | 162.9 | `bd0a87fd…` |
      | pcu, `pour_fill_target` | 49 | 14 | none | 0 | 0 | 160.0 | `a1118e6c…` |
      | pcu, `pour_fill_contacts` | 48 | 49 | none | 0 | 0 | 174.9 | `6a7350da…` |
      | pcu, both | **21** | 22 | none | 0 | 0 | 176.2 | `25cf997a…` |
      | canary-pwr, unset | 40 | 27 | none | 0 | 0 | 92.2 | `f1d2da90…` |
      | canary-pwr, `pour_fill_target` | 31 | 19 | none | 0 | 0 | 93.0 | `0b534572…` |
      | canary-pwr, `pour_fill_contacts` | 38 | 39 | none | 0 | 0 | 181.2 | `ca3becbe…` |
      | canary-pwr, both | **26** | 27 | track_dangling 1 | 0 | 0 | 152.5 | `cce88912…` |

      With the setting, the router's incomplete is within 1 of KiCad's count in every arm;
      without it, 11 against 47 and 19 against 31. The setting alone counts the open islands but
      does not route them: a search between two islands starts or ends in the outline, which
      holds both, and inserts nothing. The `track_dangling` is a 0.0725 mm B.Cu stub of
      `/Supervisor/CAN_EN` from (144.777, 79.0511) to (144.7257, 79.1024) mm at a junction of
      three of its traces, with no pad or via there. Its end lies within the cap of the junction,
      which N04.4's trace-to-trace contact counts and KiCad does not (not confirmed in the
      router).
    - *Accept 3:* `PARITY_SET=tools/bench/sets/connection-parity.txt tools/parity-check.sh
      --no-probes`: PARITY OK, 7 of 7 `[exact]`, `bytes true`. Every changed path returns Java's
      answer while the setting is off.
    - *Gate:* `cargo fmt --all --check` clean; `cargo clippy --workspace --all-targets -- -D
      warnings` clean; `cargo test --workspace --no-fail-fast`: 2993 passed, 0 failed, 56 ignored.

  - *Done 2026-10-22* (main `66c0b01`): the arm that failed accept 2 now holds. K15.36's runs
    (`target/orchestrator/k1536/runs/`, release binary `k1536/bin/v1`, K15.31's scripts), graded
    by KiCad 10.0.7: canary-pwr with both settings 26 unconnected, 0 routing-caused (the
    `/Supervisor/CAN_EN` stub is gone), 0 below-minimum, 0 manufacturing breaches, 154.1 s; pcu
    with both 21, 0 routing-caused, 176.3 s (session `25cf997a`, identical); unset 40 and 47, 0
    routing-caused. The other arms of the table above held already. Accept 1 and 3 held in the
    Partial note. The pair's default is K15.37's measurement.

- [x] **K15.32 — The plugin's pour clearance is the clearance KiCad's filler applies.**
  - **Change:** K15.27's plugin passes `--router.cleanup.pour_clearance_um` as the smallest zone
    clearance of the board's signal-layer zones (500 µm on the pcu board). KiCad's filler takes
    the clearance its rule engine resolves, and a `.kicad_dru` constraint wins over the zone's
    own: the pcu `.kicad_dru` has an unconditional `(constraint clearance (min 0.127mm))`, and
    KiCad 10.0.7 fills K15.29's `b-no-bus-gnd` board to 7.701 mm2 with the rule and 0.893 mm2
    without it, 0.128 mm from the `Net-(J13-CC2)` track (K15.29's note). A floor above KiCad's
    clearance makes the cleanup's fill model smaller than KiCad's fill, so it can remove a trace
    KiCad counts connected, against Q16.4's rule (it keeps a trace KiCad might call dangling,
    never the reverse). Pass the smallest clearance KiCad's filler can apply to those zones: the
    minimum of the zone clearances and of the `.kicad_dru` clearance constraints (and the net
    classes' where they are smaller), or no flag when that is not known.
  - **Accept:**
    - a plugin test with the stub `pcbnew`: zones of 0.5 mm and a `.kicad_dru` clearance of
      0.127 mm give `--router.cleanup.pour_clearance_um=127`; without a `.kicad_dru` rule, the
      zones' smallest as before;
    - the pcu DSN arm (K15.29's `target/orchestrator/k1529/runs/arms.sh`, a few minutes), graded
      by KiCad: unconnected not up (45), 0 routing-caused, 0 below-minimum, 0 manufacturing
      breaches; if K15.26's `/CH5_NO` `via_dangling` comes back, say why (its fix depended on the
      500 µm floor) and whether K15.26 then needs another item.
  - *Partial 2026-10-22* (branch from main `fb59eb5`; every number is the first seen, before any
    later measurement). The plugin side and the test are done; the DSN arm has 0 routing-caused
    items except `/CH5_NO`'s `via_dangling`, which comes back (below). Remains: a new item for
    the fill model (below), which no K15 item owns yet.
    - *Plugin*: `export_flow.filler_clearance_nm` (called by `pour_rule_args`, DSN input only)
      gives `--router.cleanup.pour_clearance_um` = the smallest of the signal-layer zones'
      clearances and every `(constraint clearance (min ...))` of `<board>.kicad_dru`
      (`kicad_rules.py`, new: a stdlib S-expression reader; conditional rules count; `mm`, `um`,
      `mil`, `in` and bare mm). An existing `.kicad_dru` that cannot be read (unbalanced
      parentheses, a non-numeric `min`) passes no clearance flag and logs a WARNING; a `min` of 0
      or a clearance constraint without `min` passes none either (a floor of 0 is none); no file
      is no rule. Thickness and island flags are unaffected.
    - *How KiCad 10.0.7 resolves it*, measured with `kicad-cli pcb drc --refill-zones --save-board`
      on K15.29's `b-no-bus-gnd-unfilled.kicad_pcb` (`target/orchestrator/k1532/exp/run.sh`,
      `measure.py`; zone USB_OUT, local clearance 0.5 mm, default class 0.2 mm; fill area in mm2):

      | variant | area | gap to the `Net-(J13-CC2)` track |
      |---|---|---|
      | no rule | 0.893 | |
      | `.kicad_dru` clearance 0.127 mm, unconditional | 7.701 | 0.127 mm |
      | same, 0.4 mm / 0.8 mm | 7.119 / 4.523 | 0.4 / 0.8 mm |
      | rule with condition on the track's net, 0.3 mm | 7.408 | 0.3 mm |
      | rule whose condition matches nothing | 0.893 | as no rule |
      | rule on `A.Type == 'Zone'`, 0.127 mm | 7.701 | 0.127 mm |
      | net class clearance 0.1 mm / 0.2 mm (default) | 0.893 / 0.893 | zone's 0.5 mm |
      | net class clearance 0.9 mm | 4.254 | 0.9 mm |

      A matching custom rule replaces the zone's own clearance, up or down; with none the zone's
      0.5 mm stands, a net class below it changes nothing, and one above it raises the gap. So
      the smallest the filler can apply is the minimum of the zones' and the rules' values;
      classes are not read. A zone without a local clearance reads as 0.5 mm in KiCad 10.0.7
      (`GetLocalClearance()` 0.5 after removing the line from the file). Asking pcbnew's DRC
      engine for the resolved clearance would be exact per pair but needs a DRC engine with the
      project loaded (the plugin's `board` has one, a headless `LoadBoard` does not apply the
      `.kicad_dru`), and the router takes one board-wide value, so the minimum is what it can use.
    - *Accept 1*: `kicad-plugin/tests/test_pour_clearance.py` (6 tests, stub pcbnew): zones of
      0.5 mm and a `.kicad_dru` of 0.127 mm give `--router.cleanup.pour_clearance_um=127`; no
      rule, a file without a clearance rule, and a rule above the zones give 500; four unreadable
      files give no clearance flag and a WARNING; a conditional 0.3 mm and an 8 mil rule give 203.2
      (other constraint types ignored); a 0 or max-only rule gives no flag; the reader's units,
      comments and quoted names. RED (the call replaced by the zones' value): 5 failed, 2 passed
      (of the first 7 tests; one was dropped with the class term). GREEN: 6 passed; `test_pour_rules.py`
      (9) and `test_pour_islands.py` (4) pass unchanged.
    - *Accept 2*: `target/orchestrator/k1532/runs/arms.sh` (K15.29's with this tree, release CLI from
      `fb59eb5`), graded by KiCad 10.0.7. The plugin passed `--router.cleanup.pour_clearance_um=127`
      (the board's `.kicad_dru` rule; the zones' 500). `arms-probe.sh` adds flags after `--`.

      | arm | KiCad unconnected | router incomplete | below-minimum | manufacturing | routing-caused | cleanup (vias, traces) | session |
      |---|---|---|---|---|---|---|---|
      | K15.29 (500) | 45 | 18 | 0 | 0 | none | 24, 8 | `4d183cdf...` |
      | `dflt`, K15.32 (127) | 45 | 16 | 0 | 0 | via_dangling 1 (`/CH5_NO`, (112.9114, 92.1902)) | 21, 5 | `aac8151f...` |
      | `t500`: K15.32 plus `pour_min_thickness_um=500` | 45 | 16 | 0 | 0 | via_dangling 1 (same) | 21, 5 | `aac8151f...` |
      | `c500`: K15.32 plus `pour_clearance_um=500` | 45 | 18 | 0 | 0 | none | 24, 8 | `4d183cdf...` |

      `c500` reproduces K15.29's session byte for byte, so nothing else in this tree changed the
      route. Unconnected, below-minimum and manufacturing hold; 0 routing-caused does not: the
      `/CH5_NO` `via_dangling` K15.26 removed is back, because its removal depended on the
      500 µm floor, which is above what KiCad applies on this board (127).
    - *Why it comes back* (probes in `target/orchestrator/k1532/probe/`): the via is in zone
      `EFUSE_NO_5` (F.Cu, `/CH5_NO`, `min_thickness` 0.5 mm, clearance 0.5 mm, island removal 0). On
      the routed board refilled by KiCad with the `.kicad_dru`, the zone's fill (2 pieces) is
      0.5-1.0 mm from the via (`Collide` at 0.5 mm false, at 1.0 mm true), not in an island
      KiCad drops (with `island_removal_mode` 1 the same 2 pieces, none at the via), and
      min thickness is not the cause (`t500` with 500 is the same session). The nearest foreign
      copper is a `VCC_3V3` track 0.47 mm from the via's edge, and the zone's outline passes about
      0.18 mm from the via. With the floor at the 127 µm KiCad applies, the cleanup's fill model
      still reaches the via; with 500 µm it does not, only because the track's 0.5 mm margin cuts
      the corridor. So the model is wider than KiCad's fill here by an amount the floor hid.
    - *Remains*: an item for the fill model (`quality/pour_fill.rs`, a K15.26 follow-up): find
      what KiCad's filler removes there at 127 µm that the model keeps (the corridor between the
      outline and the track; candidates: the outline edge handling of the opening, the zone's
      `smoothing fillet`), then fix it, so the floor can be the true clearance. Until then the
      pcu board has this 1 `via_dangling` under the plugin's default options. Decided 2026-10-22:
      keep the safe value (127 here), not 500. A floor above KiCad's gap can remove a trace KiCad
      counts connected, and a kept via KiCad calls dangling is the side Q16.4 allows. The fill
      model's fix is K15.33; K15.32 closes when K15.33's pcu arm has 0 routing-caused items.
    - *Gate*: `python3 -m pytest -rs kicad-plugin/tests` (`FREEROUTING_RS_BIN` = this branch's
      debug dir): 713 passed, 7 skipped (pcbnew x4, canary-pwr board, KICAD_METADATA_REPO x2). No
      `crates/` change, so no cargo gate.
  - *Done 2026-10-22* (K15.33's runs, branch from main `f0cac8f`): Accept 2 holds. K15.32's pcu
    DSN arm with K15.33's fill model (`target/orchestrator/k1533/runs/arms.sh`, the plugin's
    default options, `--router.cleanup.pour_clearance_um=127`), graded by KiCad 10.0.7: unconnected
    45, 0 routing-caused (the `/CH5_NO` `via_dangling` is gone), 0 below-minimum, 0 manufacturing
    breaches; session `d7dcfd7b...`. Accept 1 was met in the Partial note above.

- [x] **K15.33 — The cleanup's fill model is no wider than KiCad's fill at the true clearance.**
  - **Change:** K15.32's pcu DSN arm (`target/orchestrator/k1532/runs/arms.sh`, plugin from
    K15.32, `--router.cleanup.pour_clearance_um=127` from the board's `.kicad_dru`) brings back
    K15.26's `/CH5_NO` `via_dangling` at (112.9114, 92.1902) mm (session `aac8151f`). KiCad 10.0.7,
    refilling the routed board with the `.kicad_dru`, keeps zone `EFUSE_NO_5`'s fill (F.Cu,
    `min_thickness` 0.5 mm, clearance 0.5 mm, island removal 0; 2 pieces) 0.5-1.0 mm from the via.
    `quality/pour_fill.rs`'s model, opened by 0.5 mm with a 127 µm floor, still reaches it; with a
    500 µm floor it does not, because the `VCC_3V3` track 0.47 mm from the via cuts the corridor
    between it and the outline (about 0.18 mm from the via). Find what KiCad's filler removes there
    that the model keeps (candidates: how the opening treats the outline's edge, the zone's
    smoothing fillet, the clearance KiCad's filler takes to the via or the track), and fix the
    model on both inputs so the floor can be the true clearance.
  - **Accept:**
    - a test on a small board cut from the routed pcu board (named in the test, every coordinate
      from it): at a 127 µm floor the via is no contact of the fill model, as in KiCad's fill;
    - K15.32's pcu DSN arm again (the plugin's default options), graded by KiCad: unconnected not
      up (45), 0 routing-caused, 0 below-minimum, 0 manufacturing breaches; the `.kicad_pcb` arm
      (K15.23's command): unconnected not up (47), 0 routing-caused;
    - `legacy` unchanged (the cleanup is `enhanced` only; parity 7 of 7 if shared code changes).
  - *Done 2026-10-22* (branch from main `f0cac8f`; every number is the first seen, before any later
    measurement; runs and probes in `target/orchestrator/k1533/`):
    - *Cause*: not the clearance, the outline or smoothing, but KiCad's blob rule.
      `ZONE_FILLER::fillCopperZone` (KiCad source at tag 10.0.0,
      `target/orchestrator/k1524/kicad-src/zone_filler.cpp`, after the min-width `Deflate`)
      deletes every deflated polygon whose bounding box is smaller than the zone's `min_thickness`
      on both axes ("a blob min-thickness by min-thickness is not useful"), before the re-inflate.
      `SCL`'s routed F.Cu track (113.8454, 90.2934)-(110.8041, 93.3347) cuts off EFUSE_NO_5's
      bottom right corner, a triangle of 0.652 mm2 (legs 1.13 and 1.16 mm at 0.127 mm clearance)
      that holds the via. Its inradius 0.33 mm survives the opening by 0.249 mm (the model kept it),
      but the deflated triangle is 0.29 mm across, below 0.5 mm. `VCC_3V3`'s track clips only the
      corner's tip (0.131 mm from it). Measured with `kicad-cli pcb drc --refill-zones
      --save-board` (KiCad 10.0.7) on the routed board (`probe/refill.sh`, `probe/exp/`): removing
      the two `SCL` segments at (113.8454, 90.2934) brings the fill to the via; removing the
      `VCC_3V3` track, removing the via, or `island_removal_mode` 1 leaves it 0.908 mm off; zone
      `min_thickness` 0.4 and 0.42 mm keep the triangle (KiCad's piece is the triangle opened by
      0.199 mm to 7 µm), 0.44, 0.45 and 0.5 mm delete it. The blob rule (shapely, `probe/shp.py`)
      predicts all five: deflated sizes 0.465, 0.430, 0.395, 0.378, 0.291 mm. Two more causes of
      a model off KiCad's fill: the plugin passes the board's smallest `min_thickness` (400 µm, 2
      F.Cu zones of `BUS_GND` and `Net-(U26-SW1)`), and at 0.4 mm KiCad keeps the triangle too; and
      `fill_operands_eroded` took the larger of the floor and the router's clearance (the DSN's
      net class 0.15 mm on the pcu board, 0.2 mm on the cut board), while the `.kicad_dru` rule
      replaces the class clearance in KiCad's filler (K15.32), so the model was narrower than
      KiCad's 0.127 mm fill along every foreign track.
    - *Fix* (engine: `quality/pour_fill.rs`, `drc/zone_islands.rs`, `drc/kicad_rules.rs`,
      `job/prepare.rs`, `settings/fields.rs`; plugin: `kicad_board.py`, `options.py`,
      `export_flow.py`):
      - `pour_fill.rs::eroded_tiles` drops the islands of the eroded fill KiCad deletes
        (`kicad_keeps_blob`: bounding box plus a quarter of the radius on each side, for the
        model's octagonal erosion, below `min_thickness`), on both inputs, so also in K15.29's
        island model and K15.25's `opened_fill_core`;
      - `Opening` has the zone's `min_thickness` and `blob_radius`; on a DSN the blob test takes
        the net's own thickness (`DsnPourRules::min_thickness_of`), the opening keeps the board's
        smallest, and `without_blobs` drops an island only when it holds a piece of the fill
        eroded by the net's radius and all such pieces are blobs: the model never loses copper
        the board-wide opening keeps but where KiCad deletes it all;
      - a positive `pour_clearance_um` is the clearance to every foreign obstacle
        (`zone_islands.rs::fill_operands_eroded`), not the larger of it and the router's;
      - new setting `router.cleanup.pour_min_thickness_nets` (`<µm>:<net>` list, unset in both
        profiles; `tests/settings_defaults.rs`, GUI `CLI_ONLY`), parsed in
        `prepare.rs::dsn_pour_rules` with a warning per entry left out;
      - the plugin passes it for the nets whose zones' smallest `min_thickness` is above the
        board's (`signal_pour_rules`' `net_min_thickness_nm`; 44 nets at 500 µm on the pcu board).
      - Docs: `quality/pour_fill.rs` (Blobs), `quality/stubs.rs`, `drc/kicad_rules.rs`,
        DIVERGENCES (the `cleanup.remove_stubs` entry, K15.33, and the plugin's pour rules entry),
        `docs/development/kicad-plugin.md`.
    - *Accept 1*: `crates/freerouting/tests/k15_pour_blob.rs` (5 tests) on
      `tests/data/kicad/pcu-k1533/` (`make_boards.py`: zone EFUSE_NO_5, R11's pad, the via and its
      B.Cu track, `SCL`'s and `VCC_3V3`'s tracks, every coordinate from the routed pcu board;
      refilled by KiCad 10.0.7 with the pcu `.kicad_dru`: `a-corner-cut-off` reports
      `via_dangling` for the via, `b-thin-zone` (min_thickness 0.4) keeps a 0.538 mm2 triangle at
      it; both exported to DSN, the same board):
      - `the_via_in_the_cut_off_corner_is_no_contact_on_the_dsn`: 500 µm, 127 µm floor: the via's
        contacts by the fill model are B.Cu only (by the outline F.Cu and B.Cu), and the cleanup
        removes it;
      - `on_the_kicad_pcb_the_zones_own_thickness_decides`: `a` no contact, removed; `b` a
        contact, kept;
      - `told_400_um_the_triangle_stays_and_covers_kicads` (the near case where KiCad's fill does
        reach): a contact, kept, and each of KiCad's 19 triangle vertices (10 µm inwards) lies in
        the model;
      - `a_nets_own_thickness_wins_over_the_boards_smallest`: 400 with `500:/CH5_NO` no contact;
        500 with `400:/CH5_NO` a contact; `500:SCL` leaves 400;
      - `an_unreadable_entry_is_left_out_with_a_warning`: 3 warnings, the net keeps 400.
      - RED at main's model (before the fix): the first three failed (`left: ({0, 3}, {0, 3})`,
        `right: ({0, 3}, {3})`; the third with the router's 0.2 mm class clearance: KiCad's
        fill at (113.0097, 91.6732) mm outside the model). With `min_thickness_of` ignoring the
        map: `a_nets_own_thickness...` failed. GREEN: 5 passed; K15.23's, K15.26's and K15.29's
        tests pass unchanged. Plugin: `kicad-plugin/tests/test_pour_net_thickness.py` (3 tests);
        RED with the per-net values not passed: 2 failed, 1 passed; GREEN 3 passed.
    - *The model against KiCad's fill on the pcu board* (temporary probe test on K15.32's DSN and
      SES with the plugin's flags, the fill model's pieces against KiCad's refill of the routed
      board, shapely, slivers under 10 µm dropped; `probe/compare.py`, `regress.py`): at the via,
      the board-wide 400 µm model kept a 0.549 mm2 piece KiCad has not; with the per-net values it
      is gone and the via's contact layers are {B.Cu}. Against main's model (the same probe built
      at `f0cac8f`): KiCad copper this branch's model lacks and main's had: 0 pieces; KiCad copper
      it has and main's lacked: 364 pieces, 22.49 mm2 (the 127 µm clearance where main took the
      class's 150 µm). KiCad copper outside the model: main 577 pieces, 1322.91 mm2; this branch 87
      pieces, 1300.87 mm2, nearly all of it whole `BUS_GND` F.Cu areas the DSN's pours do not hold
      and planes the plugin cuts 0.5 mm from higher zones' fill where KiCad keeps 0.127 mm, both
      as at main. Model outside KiCad's copper: main 392 pieces, 83.53 mm2; this branch 596 pieces,
      86.15 mm2, of it 69.02 mm2 `Net-(U24-OUT)`'s B.Cu pour, which KiCad's refill leaves empty
      (as at main), the rest slivers of the mitred growth back.
    - *Accept 2*: release CLI of this branch; `target/orchestrator/k1533/runs/arms.sh` (K15.32's
      command with this tree's script and plugin, the plugin's default options; the plugin passed
      `--router.cleanup.pour_min_thickness_um=400 --router.cleanup.pour_clearance_um=127
      --router.cleanup.pour_min_thickness_nets=500:/BAT_VP,...` (44 nets) and the island flags)
      and `runs/arms-kicad.sh` (K15.23's `.kicad_pcb` off-arm command), graded by KiCad 10.0.7:

      | arm | KiCad unconnected | router incomplete | below-minimum | manufacturing | routing-caused | cleanup (vias, traces) | route s | session |
      |---|---|---|---|---|---|---|---|---|
      | DSN, K15.32 | 45 | 16 | 0 | 0 | via_dangling 1 (`/CH5_NO`) | 21, 5 | 166.2 | `aac8151f...` |
      | DSN, K15.33 | 45 | 17 | 0 | 0 | none | 23, 7 | 166.4 | `d7dcfd7b...` |
      | `.kicad_pcb`, K15.23 | 47 | 11 | 0 | 0 | none | 24, 7 | 181.1 | `bd0a87fd...` |
      | `.kicad_pcb`, K15.33 | 47 | 11 | 0 | 0 | none | 24, 7 | 163.2 | `bd0a87fd...` |

      The `.kicad_pcb` session is byte-identical to K15.23's. The DSN arm's router incomplete count
      rises 16 -> 17: it counted `/CH5_NO`'s connection complete through the triangle; KiCad's
      count stays 45.
    - *Accept 3*: `PARITY_SET=tools/bench/sets/connection-parity.txt tools/parity-check.sh
      --no-probes`: PARITY OK, 7 of 7 `[exact]`, `bytes true`. `legacy` runs no cleanup.
    - *Gate*: `cargo fmt --all --check` clean; `cargo clippy --workspace --all-targets -- -D
      warnings` clean; `cargo test --workspace --no-fail-fast`: 2990 passed, 0 failed, 56 ignored;
      `python3 -m pytest -rs kicad-plugin/tests` (`FREEROUTING_RS_BIN` = this branch's release
      dir): 716 passed, 7 skipped (pcbnew x4, canary-pwr board, KICAD_METADATA_REPO x2).

- [x] **K15.34 — The plugin's plane cuts keep the clearance KiCad's filler applies.**
  - **Change:** K15.24's `export_flow.fix_plane_priorities` (`plane_priority.py`) cuts a
    lower-priority plane where a higher-priority zone fills, keeping the zones' clearance
    (0.5 mm on the pcu board) from that zone's fill. KiCad 10.0.7's filler applies the
    `.kicad_dru`'s 0.127 mm there (K15.32's measurement: a matching custom rule replaces the
    zone's clearance). K15.33's comparison of the routed pcu board's model with KiCad's refill
    (`target/orchestrator/k1533/probe/compare.py`) found the DSN plane narrower than KiCad's fill
    because of it, for example `/CH7_NO` near (169.8, 109.9) mm, a 0.18 mm margin. Cut with the
    clearance K15.32's `export_flow.filler_clearance_nm` gives.
  - **Accept:**
    - a plugin test with the stub `pcbnew`: zones of 0.5 mm and a `.kicad_dru` clearance of
      0.127 mm cut the lower plane 0.127 mm from the higher zone's fill; with no `.kicad_dru`
      rule, the zones' clearance as before; an unreadable `.kicad_dru` keeps the zones' value;
    - the pcu DSN arm (K15.33's `target/orchestrator/k1533/runs/arms.sh` with this tree's plugin),
      graded by KiCad: unconnected not up (45), 0 routing-caused, 0 below-minimum, 0 manufacturing
      breaches; K15.33's comparison again: the `/CH7_NO` margin gone, no KiCad copper newly
      outside the model.
  - *Done 2026-10-22* (branch from main `48fad8c`; every number is the first seen, before any
    later measurement):
    - *Measured* (`target/orchestrator/k1534/exp/run.sh`, `gen.py`, `measure.py`: two 20 x 20 mm
      zones of nets NA, NB on F.Cu, NB (priority 2) over NA (1), 10 mm overlap, island removal
      never, refilled by `kicad-cli pcb drc --refill-zones --save-board` 10.0.7; gap from NA's
      fill to NB's left edge, mm):

      | variant | gap |
      |---|---|
      | zones 0.5 / 0.5, no `.kicad_dru` | 0.5055 |
      | zones 0.2 / 0.5 and 0.5 / 0.2, no rule | 0.5055 (the larger) |
      | no `(clearance)` line (default 0.5) | 0.5055 |
      | rule `(constraint clearance (min 0.127mm))` | 0.1325 |
      | rule 0.127 mm, zones 0.2 / 0.5 | 0.1325 |
      | rule 0.4 mm, zones 0.5 / 0.5 and 0.2 / 0.2 | 0.4055 |
      | rule `A.Type == 'Zone'`, 0.127 mm | 0.1325 |
      | rule on `A.NetName == 'NA'`, 0.3 mm / on a net that is not there | 0.3055 / 0.5055 |
      | net class clearance 0.9 mm | 0.9055 |

      So zone to zone resolves as zone to track (K15.32): a matching rule replaces the zones'
      value, up or down, the 5 um over is `m_MaxError`, which `kicad_board._knockout` already adds.
    - *Change*: `export_flow.dru_clearance_nm` (new; the smallest positive `clearance` constraint of
      `<board>.kicad_dru` through `kicad_rules`, None for no file, no rule, rules of 0, or an
      unreadable file, which logs `WARNING: ... unreadable (...); planes cut at the zones'
      clearance`) feeds `kicad_board.plane_cuts(board, pcbnew, rule_clearance_nm)`, whose
      `_gap(zone, other, rule_nm)` (`kicad_board.py`, used by `_cut_plane` and `_knockout`) is the
      smaller of the zones' larger own clearance and the rule value. `fix_plane_priorities`
      (`export_flow.py`) calls it first and logs the rule line with the cut lines. A rule above
      the zones' value does not widen the cut (as K15.32's min), nor does a rule of 0; net classes
      are not read; the cut stays wider than KiCad's fill only where a rule above the zones' value
      applies.
    - *Accept 1*: `kicad-plugin/tests/test_plane_clearance.py` (6 tests, the fake pcbnew of
      `test_plane_priority.py`, zones of 0.5 mm): a 0.127 mm rule cuts the lower plane's edge at
      14.868 mm (15 - 0.127 - 0.005) and not at 14.495; no rule or a file without a clearance rule
      14.495 (the zones' 0.5); zones of 0.2 / 0.5 mm 14.495 either way; three unreadable files
      (unbalanced parentheses, bad UTF-8, a non-numeric `min`) cut at 14.495 with a `WARNING ...
      b.kicad_dru unreadable ... zones' clearance`; a conditional 0.3 mm rule 14.695, a 0.8 mm rule
      and a 0 rule 14.495; nothing to cut writes nothing. RED (before the change): 3 failed, 3
      passed (the three that pass are the "as before" cases); GREEN: 6 passed.
      `test_plane_priority.py` and `test_pour_clearance.py` pass unchanged.
    - *Accept 2*: release CLI of this branch (built from `48fad8c` plus the plugin change);
      `target/orchestrator/k1534/runs/arms.sh` (K15.33's, this tree), plugin defaults, graded by
      KiCad 10.0.7: unconnected 44 (K15.33: 45; not up), routing-caused none, below-minimum 0,
      manufacturing 0; router incomplete 12 (17), cleanup 22 vias and 5 traces (23, 7), route
      163.9 s, session `6a90062c...` (K15.33's `d7dcfd7b`). The plugin cut 13 planes (K15.33's
      DSN: 28; the 15 left uncut lie within 0.5 mm of a higher zone but overlap it by less than
      0.127 mm) and logged `planes cut 127 um from a higher zone's fill`; `V_BUS_12V` on
      V_BUS 11346 -> 8698.42 mm2 (8585 at 0.5 mm).
    - *K15.33's comparison again*: its probe test (the fill model's pieces) is gone, so this
      compares the DSN's plane polygons with KiCad's refill of the routed board instead
      (`exp/fill_dump.py`, `plane_compare.py`, `newly.py`, shapely, slivers under 10 um dropped,
      the 52 zone-layer pairs that are both a plane and have a fill). KiCad copper outside the DSN
      plane: K15.33's DSN against its own refill 50 pieces, 165.15 mm2 (`/CH7_NO` F.Cu 0.67 mm2 at
      (169.89, 111.54): the margin K15.33 named at (169.8, 109.9)); this branch's DSN against its
      refill 22 pieces, 2.43 mm2, no `/CH7_NO` piece. Both DSNs against this branch's refill:
      K15.33's 54 pieces, 166.15 mm2, this branch's 22, 2.43 mm2. KiCad copper inside K15.33's
      plane but outside this branch's: 0 pieces (also at a 1 um sliver floor). The remaining 22
      pieces (largest 0.96 mm2 `Net-(Q2-D)` F.Cu at (145.91, 42.46), then 0.29 mm2 `/V_INRUSH`)
      lie outside both DSNs' planes, so they are not this item's: K15.35's. This is a plane
      comparison, not the fill model's, so the model's own numbers (K15.33's 87 pieces) are not
      repeated here.

- [x] **K15.35 — The DSN path's fill model covers KiCad's fill.**
  - **Change:** K15.33's comparison of the cleanup's fill model with KiCad 10.0.7's refill of the
    routed pcu board (DSN arm, session `d7dcfd7b`; `target/orchestrator/k1533/probe/compare.py`,
    `regress.py`) found KiCad copper outside the model, 87 pieces and 1300.87 mm2, most of it whole
    `BUS_GND` F.Cu areas near y 144 mm (several pieces of about 70 mm2), and model copper outside
    KiCad's, 596 pieces and 86.15 mm2, 69.02 mm2 of it the `Net-(U24-OUT)` B.Cu pour, which this
    refill leaves empty while K15.29's note has KiCad keeping it. A model narrower than KiCad's
    fill can let the cleanup remove a trace KiCad counts connected (Q16.4). Find each cause
    (candidates: the area being a plane rather than a signal pour in the DSN, so outside the
    model; K15.34's cut gap; KiCad's `connect_nearby_polys`; island removal) and fix the model
    where it is narrower on a pour the cleanup judges; reconcile the `Net-(U24-OUT)` pour with
    K15.29.
  - **Accept:**
    - every piece of KiCad copper outside the model above 1 mm2, and the `Net-(U24-OUT)` pour,
      named with its cause in the note;
    - per fixed cause, a test on a small board cut from the routed pcu board (named in the test,
      every coordinate from it);
    - the pcu DSN arm, graded by KiCad: unconnected not up (45), 0 routing-caused, 0
      below-minimum, 0 manufacturing breaches; the `.kicad_pcb` arm (K15.23's command):
      unconnected not up (47), 0 routing-caused;
    - `legacy` unchanged (parity 7 of 7 if shared code changes).
  - *Done 2026-10-22* (branch from main `af0c0de`; every number is the first seen, before any later
    measurement; runs, refills and probes in `target/orchestrator/k1535/`):
    - *Method*: main's release CLI on K15.34's arm (`runs/arms.sh`, `ARMS=base`): session
      `6a90062c...`, byte-identical to K15.34's, KiCad unconnected 44. `refill/` is that routed
      board after `kicad-cli pcb drc --refill-zones --save-board` (KiCad 10.0.7). A temporary probe
      test (`probe/zz_k1535_probe.rs`, not committed) dumped `PourFills::pieces` of every
      conduction area on the DSN and SES with the plugin's 14 flags (`probe/args-full.txt`), and
      variants without island removal, without the thickness, and with no floor either
      (`probe/model-*.txt`); `scripts/attrib.py`, `over.py` and `plane_over.py` compare them with
      KiCad's fill (`scripts/fill_dump.py`; shapely, slivers under 10 um dropped).
    - *KiCad copper outside the model* (F.Cu, B.Cu): 110 pieces, 1241.67 mm2; above 1 mm2 42
      pieces, 1238.41 mm2, each named:
      - 36 pieces, 1229.79 mm2: the F.Cu pours of `BUS_GND` (17 pieces, 780.69 mm2: Fans_GND,
        ETH_MODEM_GND, SPARE_GND, TELESCOPE_GND, ODROID_GND, ANTENNA_GND and B200_GND, the whole
        areas near y 144 mm, then BAT_GND_VIAS, MAIN_GND_Vias, CHG_GND_POUR, Q1_DRAIN, Q4S, CR1_GND,
        BAT_FETS, CR2_GND_POUR, V_BUS_24V and TVS_GND), `V_BUS_24V` (12, 274.95 mm2:
        12V_BUCK_DECOUPLING, VBUS_VIAS, EFUSE_VIN_1-4 and EFUSE_VIN_8) and `V_BUS_12V` (7, 174.15
        mm2: 12V_BUCK_OUT, EFUSE_VIN_5-7). Cause: the area is no conduction area at all. These nets
        have a plane on a power layer (GND, V_BUS), so the DSN reader drops their 29 signal-layer
        planes (Java's `Structure.insertPlanes` rule, `BasicBoard::dropped_pours`, K15.11), and the
        cleanup judges no copper there, by design. Each piece lies inside the DSN's plane (coverage
        0.99-1.00). None of KiCad's 44 unconnected involves them: its two `BUS_GND` items (U24's
        pads 17 and 21, and a track) are router failures (`Failed to route connection between
        U24-21 ...`, `U24-17 ...`).
      - 6 pieces, 8.61 mm2: strips 0.08-0.13 mm wide along five footprint rule areas (L1 twice:
        BUCK_SW `Net-(U1-SW)` 1.67 mm2 and BUCK_OUT `Net-(C7-Pad1)` 1.35; CR2: MAIN_V_NTC/TVS
        `Net-(D9-C)` 1.88; CR3: BAT_V_IN `/BAT_VP` 1.44; L2: Q3S `Net-(Q4-D)` 1.13) and round F1's
        NPTH (V_PD_IN `/V_PD_IN`, 1.14). Cause: the floor. K15.33 made `pour_clearance_um` the
        clearance to every foreign obstacle, keepouts included, while KiCad's filler cuts a rule
        area at its outline ("Keepouts use outline with no clearance",
        `ZONE_FILLER::buildCopperItemClearances`) and the export writes an NPTH as a keepout
        already grown by the hole clearance (F1: a 3.294 mm circle for the 2.794 mm hole; KiCad
        fills from 1.6566 mm off its centre, the model from about 1.774 mm). Fixed (below). Not
        island removal (the model without it covers 0.00 of each piece), not the opening (0.03-0.14
        without it).
      - Below 1 mm2, the largest, 0.957 mm2 `Net-(Q2-D)` F.Cu at (145.91, 42.46), is the DSN plane
        itself (K15.34's piece): the plugin cuts CHG_Q2 at Q1_DRAIN's outline, where KiCad 10.0.7
        knocks a lower zone out by the higher zone's fill (`buildDifferentNetZoneClearances` takes
        `ZONE::TransformShapeToPolygon`, which is the fill).
    - *`Net-(U24-OUT)`'s B.Cu pour*, reconciled with K15.29: KiCad fills USB_OUT_POUR here, 83.66
      mm2 in 1 piece (K15.33's refill, read again: 27.82 mm2 in 1 piece; not empty). The model has
      111.93 mm2 in 4 pieces; 28.27 mm2 lie outside KiCad's fill (21.90 mm2 at (67.34, 130.58),
      3.42, 2.75): strips between other nets' tracks routed across the pour. Cause: island removal.
      KiCad removes them because one piece is joined to U24's pads through the net's vias, so not
      every piece is an island (KiCad keeps them all only "if *all* the polygons are islands"); on
      K15.29's route every piece was isolated and KiCad kept them all. The model keeps them: its
      third condition (another island of the area meets a pin's pad itself, K15.29's conservative
      stand-in for KiCad's rule) fails on a B.Cu pour with no pad, and the model without island
      removal is the same 111.93 mm2. Wider than KiCad's fill, so the cleanup may keep a trace KiCad
      calls dangling, never the reverse; not changed. `Net-(U24-SW2)` B.Cu (9.85 mm2) and USB_GND
      B.Cu (8.75 mm2) are the same case.
    - *The DSN plane area KiCad leaves empty* (K15.34's 1142 pieces, 2355.5 mm2): most of it is the
      plane being an outline (the router keeps other nets' copper, the board edge and the NPTHs
      clear itself). Leaving out 0.3 mm round other nets' copper (every copper layer), 0.55 mm
      inside the edge, the rule areas and higher-priority zones' fill, 30 pieces of at least 1 mm2
      remain, 405.22 mm2 (92 pieces, 417.00 mm2, from 0.01 mm2; `probe/plane-over*.txt`). KiCad
      refills of the routed board with every zone's `island_removal_mode` 1 (`exp/isl1`) and with
      that and `min_thickness` 0.01 mm (`exp/open0`) fill them:
      - island removal, 29 pieces, 403.60 mm2: CHG_BAT_VP 92.62 (2 pieces), CH4_SENSE 94.17 (4),
        CH2_SENSE 68.34 (3), CH5_SENSE 48.42 (4), USB_OUT_POUR 22.19 (3), Q3S 19.12, VBUS_VIAS
        10.22, Q3d 9.28, V_INRUSH 9.05 (3), USB_GND_CAPS_BACK 7.58, USB_SW2_BACK 6.31,
        MAIN_V_NTC/TVS 5.19, CHG_Q2 4.35, EFUSE_VIN_3 2.86, EFUSE_NO_4 2.14, USB_PORT_GND 1.80;
      - the minimum-thickness opening, 1 piece, 1.62 mm2 (Q3d);
      - no zone is unfilled (each one's filled area is above 0), and no `connect_nearby_polys` or
        thermal-relief case was found; the export's planes are K15.24's cuts.

      KiCad's unconnected count: 44 as graded, 44 with no island removal, 42 with neither island
      removal nor the opening. So none of the 44 comes from an island KiCad removes (such an island
      holds no pad); 2 come from a neck KiCad's opening removes and the router counts joined
      through the plane's outline (USB_OUT_POUR B.Cu to USB_OUT F.Cu, `Net-(U24-OUT)`, and
      USB_SW2_BACK to USB_SW2, `Net-(U24-SW2)`); the other 42 are the 12 the router left
      incomplete and pours split by other nets' routed copper (K15.13's class, counted through the
      outline by the router).
    - *Fix* (`drc/zone_islands.rs`: new `KeepoutFill`, `keepout_fill`, `COPPER_KEEPOUT_NAMES`;
      `fill_operands_eroded` and `opened_fill_islands` take `kicad_keepouts`; `quality/pour_fill.rs`:
      new `Opening::kicad_keepouts`, true on a DSN input, false on a `.kicad_pcb`, read by
      `eroded_islands`, so by the cleanup's pieces and K15.29's island model): a `keepout` (a rule
      area that bans tracks and vias, or an NPTH) is cut at its outline with no clearance; a fitted
      mask-aperture or thermal-spoke keepout and a `via_keepout`, `wire_keepout` or `place_keepout`
      are left out (no copper, or the DSN does not say they ban the pour); a copper keepout
      (`copper_graphic`, `copper_text`, `net_tie`) keeps the floor. Java's fill (`fill_operands`)
      and the `.kicad_pcb` path are unchanged; the `.kicad_pcb` path has the same narrowing
      (K15.38). Docs: `quality/pour_fill.rs` (Keepouts), `drc/zone_islands.rs`, DIVERGENCES (the
      `cleanup.remove_stubs` entry, "K15.35, on a DSN input").
    - *The model against KiCad's fill after the fix* (the same probe and SES): KiCad copper outside
      the model 97 pieces, 1231.87 mm2, every piece above 1 mm2 a dropped pour; KiCad copper the
      fixed model lacks and main's had: 0 pieces; that it gained: 13 pieces, 9.79 mm2. Model
      outside KiCad's copper: 811 pieces, 57.60 mm2 before, 843 pieces, 57.91 mm2 after (the edge
      strips are cut at their outline too).
    - *Accept 1*: the two lists above name every piece above 1 mm2 and the pour.
    - *Accept 2*: `crates/freerouting/tests/k15_pour_keepout.rs` (2 tests) on
      `tests/data/kicad/pcu-k1535/` (`make_boards.py`: BUCK_SW and L1 with its rule area; V_PD_IN
      and F1 with its NPTH; every coordinate from the routed pcu board; refilled by KiCad 10.0.7
      with the pcu `.kicad_dru` and exported to DSN):
      - `the_model_reaches_a_rule_area_as_kicads_fill_does`: KiCad's fill edge at x 200.6472 mm
        (10 um inwards, every 0.5 mm from y 114.0 to 126.5) lies in the model, and 10 um into the
        rule area does not;
      - `the_model_reaches_an_npth_as_kicads_fill_does`: KiCad's 32 arc vertices 1.6566 mm from the
        NPTH's centre (10 um outwards) lie in the model, and nothing 1.5 mm from it does.
      - RED with `kicad_keepouts` false on a DSN (main's model): 2 failed ("KiCad's fill at
        (200.6372, 114.0000) mm is outside the model", "(95.9176, 122.8327)"). GREEN: 2 passed;
        `k15_pour_blob` (5), `k15_pour_island_via` (4), `k15_dsn_pour_fill_stub` (3) and
        `k15_pour_fill_stub` (6) pass unchanged.
    - *Accept 3*: release CLI of this branch; `runs/arms.sh` (`ARMS=fix`, K15.34's command, the
      plugin's default options) and `runs/arms-kicad.sh` (K15.23's `.kicad_pcb` off-arm command),
      graded by KiCad 10.0.7:

      | arm | KiCad unconnected | router incomplete | below-minimum | manufacturing | routing-caused | cleanup (vias, traces) | route s | session |
      |---|---|---|---|---|---|---|---|---|
      | DSN, main `af0c0de` | 44 | 12 | 0 | 0 | none | 22, 5 | 162.9 | `6a90062c...` |
      | DSN, K15.35 | 44 | 12 | 0 | 0 | none | 22, 5 | 164.1 | `6a90062c...` |
      | `.kicad_pcb`, K15.35 | 47 | 11 | 0 | 0 | none | 24, 7 | 162.7 | `bd0a87fd...` |

      Both sessions are main's (the DSN SES `cmp`-equal to the base arm's; the `.kicad_pcb` one is
      K15.23's and K15.33's): on this route no cleanup judgement changes.
    - *Accept 4*: `PARITY_SET=tools/bench/sets/connection-parity.txt tools/parity-check.sh
      --no-probes`: PARITY OK, 7 of 7 `[exact]`, `bytes true`. `legacy` runs no cleanup, and
      `fill_operands` (Java's fill, the DRC's) passes `kicad_keepouts` false.
    - *Gate*: `cargo fmt --all --check` clean; `cargo clippy --workspace --all-targets -- -D
      warnings` clean; `cargo test --workspace --no-fail-fast`: 2992 passed, 0 failed, 56 ignored.
      The plugin is unchanged.

- [x] **K15.36 — A trace stub at a junction of its own net's traces is not left dangling.**
  - **Change:** K15.31's canary-pwr arm with `router.pour_fill_contacts` and `pour_fill_target`
    (`95882f00`, K15.14's `pcb-off` command; `target/orchestrator/k1531/runs/rebased-canary/`,
    session `cce88912`) has 1 routing-caused item: KiCad 10.0.7 reports `track_dangling` on a
    0.0725 mm B.Cu stub of `/Supervisor/CAN_EN` from (144.777, 79.0511) to (144.7257, 79.1024) mm,
    at a junction of three of its traces, with no pad or via there. Its end lies within the cap of
    the junction, which N04.4's trace-to-trace contact counts and KiCad does not (inferred from
    the geometry, not traced in the router). Find why the router lays and keeps it, and make the
    route leave no such stub (the cleanup removes it, or the router does not lay it), on both
    inputs.
  - **Accept:**
    - a test on a small board cut from that route (named in the test, every coordinate from it):
      the stub is gone, the junction's connections stay;
    - the canary-pwr arm again with both settings, graded by KiCad: 0 routing-caused,
      unconnected not up (26), 0 below-minimum, 0 manufacturing breaches; the pcu `.kicad_pcb`
      arm with both settings (K15.31's): unconnected not up (21), 0 routing-caused;
    - `legacy` unchanged (parity 7 of 7 if shared code changes).
  - *Done 2026-10-22* (branch `k1536` from main `b1f429b`; every number below is the first seen,
    before any later measurement):
    - *Cause*: a probe (a temporary print with a backtrace in `BasicBoard::insert_item` and
      `stubs::is_removable_stub_with`, profiling build, same session `cce88912...`) shows the stub
      is its own router trace, made by pull-tight: `remove_tails` (`autoroute/batch/pass.rs`) ->
      `pull_tight_after_connection` -> `smoothen_end_corners_at_trace` -> `split_traces` ->
      `split_trace_at_line` (`board/trace_ops.rs`). The fanout's neckdown had put the junction at
      (144.7257, 79.1024); the smoothing moved it to (144.777, 79.0511) on the via's wire and left
      the piece between as a trace. The board's contact rule is `center` (Java's): no contact at
      the far end. The cleanup's test (Q16.4, `stub_contacts::end_has_contact`) found the via's
      wire's copper within the far end's cap (0.0725 mm from its end), so both ends had a
      contact and it stayed. KiCad 10.0.7's `CONNECTIVITY_DATA::TestTrackEndpointDangling`
      (`connectivity_data.cpp`) counts a track that meets both ends of a segment for the end
      nearer its own ends only (`getMinDist`); the via's wire, the neckdown and U7's wire each
      meet both ends and are nearer the junction, so the far end has no count.
    - *Fix*: `stub_contacts::trace_dangling(_with)` (new) counts a trace's end segments as
      `TestTrackEndpointDangling` does: a pad, via or pour (its opened fill) that meets both ends
      connects the segment; a track segment (another trace's or the trace's own) that meets both
      counts for the nearer end only (a tie counts for both); the end-cap test is the same.
      `stubs::is_removable_stub_with` uses it for traces. The cleanup is `enhanced` only; the
      router's contacts are unchanged.
    - *Accept 1*: `crates/freerouting/tests/k15_junction_stub.rs` (3 tests) on
      `tests/data/kicad/canary-k1536/` (`make_boards.py`: `/Supervisor/CAN_EN`'s B.Cu copper from
      U7 pad 6 to the via at (151.7716, 51.5288) from the route, every coordinate from it). KiCad
      10.0.7 `pcb drc --severity-all`: `a-stub-at-a-junction` reports the stub
      `track_dangling`; `b-stub-end-on-a-track` (plus a track from the far end to a test pad)
      none. `the_stub_s_far_end_is_dangling_as_kicad_counts_it`,
      `the_cleanup_removes_the_stub_and_keeps_the_junction` (1 trace removed, U7's connected set
      is the same less the stub), `a_stub_whose_far_end_meets_a_track_there_stays`. RED with the
      old rule: 2 failed (`trace_dangling` false; `is_removable_stub` false); GREEN: 3 passed.
    - *Accept 2*: K15.31's scripts (`target/orchestrator/k1536/scripts/canary3.sh`, `pcu3.sh`,
      `--max-passes 2 --timeout 240`), release binary of this tree (`k1536/bin/v1`), graded by
      KiCad 10.0.7, outputs in `target/orchestrator/k1536/runs/rebased-canary/` and
      `rebased-pcu/`:

      | board, arm | KiCad unconnected | router incomplete | routing-caused | below-min | mfg | route s | session |
      |---|---|---|---|---|---|---|---|
      | canary-pwr, both | 26 (was 26) | 27 | none (was track_dangling 1) | 0 | 0 | 154.1 | `7811ea5a...` |
      | pcu, both | 21 (21) | 22 | none | 0 | 0 | 176.3 | `25cf997a...` (same) |
      | canary-pwr, unset | 40 (40) | 27 | none | 0 | 0 | 92.6 | `f1d2da90...` (same) |
      | pcu, unset | 47 (47) | 12 (was 11) | none | 0 | 0 | 164.6 | `2e8bf394...` (was `bd0a87fd...`) |

      canary-pwr with both settings: the cleanup removes 2 trace stubs (was 1); the session
      differs from `cce88912...` by the `/Supervisor/CAN_EN` stub alone. pcu unset: the cleanup
      removes 8 traces (was 7); before, the output check found 1 `track_dangling` after it
      (its KiCad-rule report, `drc/kicad_report.rs::track_dangling`), removed it and retried
      `/CH6_NO`, whose retry added a via pair from (130.0807, 106.6024) to (127.811, 111.2185)
      that the router counts as joining and KiCad does not (47 both times). Now the output check
      finds no breach and retries nothing: router incomplete 11 -> 12, KiCad unchanged. That the
      cleanup now removes the trace the output check removed is inferred (the counts, and U8's
      pad 8 to pad 10 wire, one SES wire before, is two now, joined at (126.9387, 105.8281)), not
      traced.
    - *Accept 3*: `PARITY_SET=tools/bench/sets/connection-parity.txt tools/parity-check.sh
      --no-probes`: PARITY OK, 7 `[exact]`, `bytes true` (the change is the `enhanced` cleanup).
    - *Gate*: `cargo fmt --all --check` clean; `cargo clippy --workspace --all-targets -- -D
      warnings` clean; `cargo test --workspace --no-fail-fast`: 2996 passed, 0 failed, 56 ignored.

- [x] **K15.37 — Measure `pour_fill_contacts` with `pour_fill_target` under `enhanced`.**
  - **Change:** K15.31's first numbers on top of `enhanced` (main `48fad8c`, plugin default
    options, `--max-passes 2 --timeout 240`, KiCad 10.0.7): the pcu board's `.kicad_pcb` route
    goes from KiCad unconnected 47 to 21 with both settings, canary-pwr's from 40 to 26; route time
    163 s -> 176 s and 92 s -> 153 s. Register a measurement of the pair against `enhanced` on
    K15.30's set (`tools/bench/sets/k15-fill-target.txt`, its helper `tools/bench/k15_all_pours.py`,
    K15.30's registration as the model), after K15.36, and run it through the orchestrator's heavy
    queue.
  - **Accept:**
    - the registration (prereg JSON, its pin test, the set) committed before any run;
    - the records and the verdict in the note, and the outcome applied as its decision table says
      (both settings into `ENHANCED`, a plugin option, or neither);
    - `legacy` unchanged (the settings are unset there; parity 7 of 7).
  - *Partial 2026-10-22:* Accept 1 is written (registered at main `c4f74fb`, to be committed before
    any run of the set); Accept 2 and 3 remain here: the registered run (K13.5's
    `plugin-input-ab.sh` per call) is a heavy run, queued by the orchestrator in
    `target/orchestrator/pending-heavy-runs.md`. **No number of this measurement exists**: nothing
    was routed or graded, no kicad-cli ran (the seen numbers are K15.31's, K15.36's, K15.38's and the
    orchestrator's check on `c4f74fb`, listed in the registration's `seen_before_registration`).
    - *Files:* `docs/roadmap/prereg/K15.37.json` (id `K15.37-fill-pair`) sha256
      `73b58f138f06112c2b6c0d621456b3b625597d24509c406bd4f2900b61911fb5`, 0 amendments;
      `tools/bench/sets/k15-fill-pair.txt` (K15.30's 17 rows verbatim under a header naming K15.37)
      sha256 `fc34109cde3f899ecfdc40dd50e477a74dcc7e5b6bf50a4b74bf1413aa355adb`; the pin test
      `crates/freerouting-dev/tests/prereg_k15_37.rs`. The helper is K15.14's
      `tools/bench/k15_all_pours.py`, unchanged (`8de88dd8...`): `run` runs a registration's
      `commands` verbatim, so two flags in one command need nothing in it.
    - *Registration:* K15.30's with the differences it lists (`differences`): arms `pcb-off`
      (baseline, both settings unset) and `pcb-on` (`--router.pour_fill_contacts=true
      --router.pour_fill_target=true`), the `.kicad_pcb` input only (`no_dsn_why`: neither setting
      acts on a DSN); every command is K15.30's command of the row with `target/orchestrator/k1537/`
      for the tree, output and build, and the two flags (`--max-passes 2 --timeout 240`,
      `ROUTE_TIMEOUT_S=300`, `ROUTE_MEMORY_KB=8000000`, options `{"schema": 6}`, KiCad 10.0.7, the
      boards at K15.14's paths under `target/orchestrator/k1514/boards/`). Set, rule, noise,
      metrics, guards (1)-(3) and `must_judge` (pcu-76163c7, canary-pwr-95882f00) are K15.30's.
      Differences that matter: (a) `pocketsdr-v3-0` stays in the set (guard (1) reads every graded
      arm; validity (b) leaves it out again if both arms time out, as in K15.30); (b) the pair costs
      route time (pcu 162.5 s -> 191.6 s, canary-pwr 93.1 s -> 159.8 s on `c4f74fb`), both under
      the 240 s deadline, so the caps stay K15.30's; a row COMPLETED in `pcb-off` and TIMED_OUT in
      `pcb-on` (`kit-dev-coldfire-xilinx_5213`, 171 s in K15.30, reaches 240 s at +40 %) is left out
      of the judgement by validity (b) but named, and guard (4) counts its route time, so a
      timeout cannot hide a time cost; (c) validity (a) reads both `pour_fill_target` and
      `pour_fill_contacts` from the manifests; (d) the noise is K15.30's 67.09 over 16 rows (72.32
      over 17): `connections.maximum_count`, the band's base, is 852 (pcu) and 331 (canary-pwr) in
      both arms of the check on `c4f74fb`, equal to K15.30's records, and the gate printed 67.09
      on K15.30's records through this registration (format check below). Decision table: not
      measured, breach (`pour_fill_contacts` unoffered, K15.30's option stays), not kept no gain /
      not kept guard (both unset in both profiles; the plugin offers `pour_fill_contacts` beside
      K15.30's checkbox), kept (both into `ENHANCED` with DIVERGENCES profile lines, parity 7 of 7,
      and K15.30's plugin option for `pour_fill_target` removed: with the profile carrying the
      setting it would add an already-set flag and could not switch it off).
    - *Seen gains vs the noise:* pcu 47 -> 20 and canary-pwr 40 -> 26 are 41 together against a set
      noise of 67.09, so S holds only if the other 14 judged rows gain 27 more; K15.30's rows moved
      0 to 2 each with the target alone. 'Not kept, no gain' is the likely outcome and is
      registered as the keep rule has it; the owner-board numbers go into the plugin option's
      tooltip then.
    - *Calls and time:* 34 calls (17 rows x 2). K15.30's run took 34 min (673.7 s routing in
      `pcb-off`, 706.1 s in `pcb-on` over 16 rows, `pocketsdr-v3-0` 2 x 240 s, the rest snapshot,
      import and grading); with the pair about +30 s on pcu, +67 s on canary-pwr, up to +72 % on the
      other rows' 420 s, and more grading (fewer shared grades): about 40-55 min expected, at most
      3.1 h if every call hit its 300 s cap.
    - *Pin test:* `cargo test -p freerouting-dev --test prereg_k15_37`: RED 10 failed before the
      files existed (`docs/roadmap/prereg/K15.37.json: No such file or directory`); GREEN 10 passed
      (the registration's and the set's hashes, the rows, boards and rules files equal to K15.30's
      and hashing as registered, the 34 commands against their templates and K15.30's commands with
      `k1537` and the two flags, the helper's `run --dry-run` printing exactly them, the script and
      helper hashes, the rule, noise, guards, validity, `must_judge`, decision table, the seen
      sessions, 0 amendments). `--test prereg_k15_30`: 9 passed and `--test prereg_k15_14`: 8
      passed, unchanged; `cargo test -p freerouting-dev`: all passed. Format check, nothing routed:
      `k15_all_pours.py collect` and `freerouting-dev gate --write-baseline` / `--all-pairs
      --left-out pocketsdr-v3-0=...` of this registration on symlinks to K15.30's finished arms
      (scratch directory, not recorded as a result) printed K15.30's 370 -> 363 over 16 rows and the
      noise 67.09: the registration, `collect` and the gate agree.
    - *Heavy run (done 2026-10-22, below):* `cd /home/thebo/code/freerouting-rs && git worktree add
      --detach target/orchestrator/k1537/tree <K15_37_COMMIT> && cd target/orchestrator/k1537/tree &&
      CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
      CARGO_TARGET_DIR=/home/thebo/code/freerouting-rs/target/orchestrator/k1537/target-release
      cargo build --release -p freerouting-cli -p freerouting-dev` and `cargo test --release -p
      freerouting-dev --test prereg_k15_37` (same environment); then `cd
      /home/thebo/code/freerouting-rs && python3 -I
      target/orchestrator/k1537/tree/tools/bench/k15_all_pours.py prepare
      target/orchestrator/k1537/tree/docs/roadmap/prereg/K15.37.json` (every row `present`); then
      amendment 1 (the commit, the binary, the grader and plugin hashes) with the pin updated
      (`AMENDMENTS`, the registration's sha256), committed to main; then `mkdir -p
      target/orchestrator/k1537/out && python3 -I
      target/orchestrator/k1537/tree/tools/bench/k15_all_pours.py run
      target/orchestrator/k1537/tree/docs/roadmap/prereg/K15.37.json >
      target/orchestrator/k1537/out/run.log 2>&1; echo "EXIT $?" >>
      target/orchestrator/k1537/out/run.log`; then `collect` and the two gate calls
      (`--write-baseline`, `--all-pairs`, `--prereg docs/roadmap/prereg/K15.37.json` as amended,
      `--left-out ROW=REASON` for each row validity (b) leaves out) as `heavy_run` spells out. Keep
      `target/orchestrator/k1514/boards/` until this run is done.
    - *Run 2026-10-22* (amendment 1, `034b6e6`: K15_37_COMMIT `40c9f7f`, binary `ed27fe02…`,
      all 17 rows present): 34 calls, every one EXIT 0, 09:42:31-10:27:32 (45 min), into
      `target/orchestrator/k1537/out/` (`run.log`, `collect.log`, `records.json`, `baseline.json`,
      `gate-baseline.txt`, `gate-all-pairs.txt`). Validity (a): every `pcb-on` manifest has
      `pour_fill_target` and `pour_fill_contacts` true, no `pcb-off` one has either; two `pcb-on`
      sessions are byte-identical to their `pcb-off` ones (grade shared: loratracker `1fa9e1d7`,
      macroprocessor `ad5f72c8`; the pair changed no route there), judged as they are. Validity (b)
      leaves out 3 rows, passed to both gate calls as `--left-out` (each graded, hard limits 0):
      `pocketsdr-v3-0` (both arms TIMED_OUT at `--timeout 240`: 240.1 s, 254.6 s),
      `kit-dev-coldfire-xilinx_5213` (`pcb-off` COMPLETED 173.3 s, `pcb-on` TIMED_OUT 245.7 s with
      219 unconnected) and `pcie-active-backplane-pcie-active-backplane-x16-4layer` (`pcb-off`
      COMPLETED 40.9 s, `pcb-on` TIMED_OUT 280.2 s with 28 unconnected); 14 rows judged, pcu and
      canary-pwr among them. Guard (1): 0 manufacturing breaches, 0 below-minimum and 0 via-in-pad in
      all 34 graded arms. Judged rows, `pcb-off` -> `pcb-on`:

      | row | KiCad unconnected | band | drc_real (routing-caused types) | route s |
      |---|---|---|---|---|
      | pcu-76163c7 | 47 -> 20 | 43 | 0 -> 0 | 163.7 -> 189.6 |
      | canary-pwr-95882f00 | 40 -> 26 | 10 | 0 -> 0 | 92.8 -> 153.2 |
      | interf_u | 17 -> 13 | 11 | 1 -> 7 (`starved_thermal` 1 -> 5, `via_dangling` 0 -> 2) | 6.6 -> 20.5 |
      | moteus-moteus-c1 | 40 -> 30 | 22 | 4 -> 13 (`isolated_copper` 3 -> 12, `track_dangling` 1 -> 1) | 66.9 -> 117.9 |
      | openmuscle-flexgrid-om-flexgrid-rigid-pcb | 7 -> 3 | 8 | 1 -> 2 (`starved_thermal` 1 -> 2) | 10.8 -> 34.1 |
      | nudac-nudac | 11 -> 9 | 9 | 1 -> 0 (`track_dangling` 1 -> 0) | 10.9 -> 21.1 |
      | wisweep-driver-board | 25 -> 23 | 11 | 4 -> 4 | 22.8 -> 75.9 |
      | m-fc-g474-rev-a-m-fc | 5 -> 6 | 14 | 1 -> 1 | 24.5 -> 70.2 |
      | complex_hierarchy | 81 -> 80 | 14 | 0 -> 0 | 0.2 -> 44.3 |
      | pic_programmer | 3 -> 2 | 7 | 0 -> 0 | 0.6 -> 3.1 |
      | solarmeshtasticnode-meshtasticnode-bq25185 | 10 -> 9 | 6 | 0 -> 0 | 5.9 -> 5.6 |
      | vna-usb-pd | 2 -> 1 | 4 | 0 -> 0 | 2.3 -> 3.8 |
      | loratracker-hardware-loratracker-hat | 44 -> 44 | 12 | 0 -> 0 | 56.9 -> 57.0 |
      | macroprocessor-diode-matrix-rom-dip | 6 -> 6 | 8 | 0 -> 0 | 0.8 -> 0.9 |
      | total | 338 -> 272 | | 12 -> 27 | 465.7 -> 797.2 |

      `freerouting-dev gate --all-pairs`: "total 338 -> 272 (change -66); set noise sqrt(sum band^2)
      = 59.34; totals verdict: accept; pair pcb-off -> pcb-on: PASS" (the reverse pair rejects,
      +66). The gate prints no verdict for guards (2)-(4); summed from `records.json` against the
      registration's text: S holds (-66 against 59.34); guard (3) holds (no row above its band; the
      one rise is m-fc +1, band 14); **guard (2) fails**: drc_real over the judged rows 12 -> 27
      (interf_u +6, moteus +9, openmuscle +1, nudac -1); **guard (4) fails**: route time over the
      judged rows and the two rows TIMED_OUT only in `pcb-on` 679.9 s -> 1323.1 s, 1.95 x against
      the limit 1.25 x (over the judged rows alone 465.7 s -> 797.2 s, 1.71 x). The `must_judge`
      rows match `seen_before_registration`: pcu sessions `2e8bf394` (off) and `aa9c84fd` (on), 47
      and 20, as on `c4f74fb` (162.5 s and 191.6 s there); canary-pwr `f1d2da90` and `7811ea5a`, 40
      and 26, as on `c4f74fb` (93.1 s and 159.8 s there). Decision table, in its order: not "not
      measured" (14 rows, both `must_judge` rows judged, amendment 1 before the first arm), not
      "breach" (guard (1) holds in every arm), not "not kept, no gain" (S holds), so **"not kept,
      guard"** (guards (2) and (4)): both settings stay opt-in, unset in both profiles, and the
      plugin offers `router.pour_fill_contacts` as an option beside K15.30's `pour_fill_target`
      one (applied in the next change). The gain is kept for a fix (decided 2026-10-20: fix gains,
      don't remove them): K15.41 takes the route time, K15.42 the routing-caused DRC, and K15.43
      measures the pair again under a new registration. K15.30's target-alone arms (engine
      `fb59eb5`, before K15.35, K15.36 and K15.38) cost little time on the rows that slow here
      (interf_u 6.6 -> 6.7 s, complex_hierarchy 0.2 -> 0.3 s, kit-dev-coldfire 171.1 -> 182.7 s,
      pcie 40.8 -> 43.2 s) and left interf_u's and openmuscle's drc_real at 1, so those costs come
      with `pour_fill_contacts`; moteus's `isolated_copper` rose with the target alone too (3 -> 9).
    - *Outcome applied 2026-10-22:* the plugin option `RouteOptions.pour_fill_contacts` (off in
      both profiles; `to_args` adds `--router.pour_fill_contacts=true` only when on; `from_dict`,
      `from_form`; options file schema stays 6, a file without the key means off), the Advanced
      tab's Flow group checkbox "Count each pour island as its own connection" (below "End traces
      where KiCad fills a pour"), whose tooltip says it is meant to be used with that option
      (alone it finds the open islands but does not route them) and gives the measurement without
      private board names (338 -> 272 over 14 boards, one board 47 -> 20, another 40 -> 26,
      routing about 1.7 x as long, three boards with more DRC violations, 12 -> 27 in all; board
      snapshot input only). DIVERGENCES' `pour_fill_contacts` and `pour_fill_target` entries and
      the "Not in `enhanced`" docs of `board/pour_contacts.rs` (a new paragraph) and
      `autoroute/pour_target.rs` carry the numbers; the plugin handover
      (`docs/development/kicad-plugin.md`) lists the option. K15.41-K15.43 fix the costs and
      measure again.
    - *Done 2026-10-22:* (1) the registration was committed before any run: `40c9f7f`, amendment 1
      `034b6e6`; `cargo test -p freerouting-dev --test prereg_k15_37`: 10 passed. (2) The records
      and the verdict ("not kept, guard", guards (2) and (4)) are in the note above (`4f7866b`),
      and the outcome is applied: the option with its tests, RED 5 failed (`test_options.py`
      3, `test_route_dialog.py` 1, `test_snapshot.py::test_every_k09_option_is_either_checked_or_allowed`
      1) then GREEN 15 passed on the same selection, and the whole plugin suite 730 passed, 7
      skipped (pcbnew, canary-pwr board, `KICAD_METADATA_REPO` only). (3) `legacy` unchanged: the
      change touches no engine code (two module doc comments); `PARITY_SET=tools/bench/sets/connection-parity.txt
      tools/parity-check.sh --no-probes`: connection parity PASS, 7 of 7 `[exact]`, 56 s;
      `cargo test -p freerouting --test settings_defaults`: 9 passed. Gate: `cargo fmt --all
      --check` clean; `cargo clippy --workspace --all-targets -- -D warnings` clean.

- [x] **K15.38 — The `.kicad_pcb` path's fill model takes the keepouts as KiCad's filler does.**
  - **Change:** K15.35 changed the DSN path only (`Opening::kicad_keepouts` is false for a
    `.kicad_pcb`). Read from K15.35's `tests/data/kicad/pcu-k1535/a-l1-keepout.kicad_pcb` and
    `b-f1-npth.kicad_pcb` (probe 2026-10-22, enhanced defaults), the cleanup's fill model holds
    none of 6 points inside KiCad's fill: 3 along L1's rule area at x 200.6372 mm (10 um in), 3
    round F1's NPTH 1.6615-1.6645 mm from its centre (KiCad's fill starts at 1.6566 mm). The `.kicad_pcb` model keeps the router's clearance from
    a keepout, where KiCad's filler cuts a rule area at its outline and keeps an NPTH's hole
    clearance. That opened fill also feeds the router's pour target (`opened_fill_core`, K15.25,
    `router.pour_fill_target`) and K15.31's contacts, so change it after K15.31 lands: find how
    the `.kicad_pcb` reader writes rule areas and NPTHs (their kind, name and clearance class),
    then take them as K15.35 does.
  - **Accept:**
    - K15.35's two tests' checks on the `.kicad_pcb` boards (`kicad_board`), RED before the
      change;
    - the `.kicad_pcb` arm (K15.23's command), graded by KiCad: unconnected not up (47), 0
      routing-caused; K15.30's `pour_fill_target` arm not worse than its last row;
    - with both `router.pour_fill_target` and `router.pour_fill_contacts` (K15.31) on: the pcu
      `.kicad_pcb` arm not worse than 21, 0 routing-caused; canary-pwr's arms (unset 40, target 31,
      both 26) not worse, no routing-caused added; 0 below-minimum and 0 manufacturing breaches in
      every arm;
    - `legacy` unchanged (parity 7 of 7 if shared code changes).
  - *Done 2026-10-22* (branch from main `e5bc82b`; every number is the first seen, before any
    later measurement; runs, refills and probes in `target/orchestrator/k1538/`). Every clause
    holds but one: the pcu `pour_fill_target` arm is 51, above 49 (the NPTH cut moves that route:
    2 more router failures on signal nets, no copper misjudged, below). Decided 2026-10-22: accepted.
    The NPTH cut is KiCad's fill; `pour_fill_target` alone is opt-in and not kept (K15.30), and
    the pair K15.37 measures improves (21 -> 20). Keeping the NPTH's clearance would leave the
    model short of KiCad's fill by about 0.12 mm round every NPTH. The fitted mask-aperture and
    thermal-spoke keepouts keep Java's clearance on a `.kicad_pcb` (dropping them, as KiCad's
    filler does, moved canary-pwr's pair arm 26 -> 30; decided 2026-10-22: left until a measured
    item takes it up). The canary-pwr pair arm's CAN_EN stub is K15.36's (fixed on main after this
    branch).
    - *How the reader writes them*: a rule area becomes unnamed keepouts of its outline, one per
      copper layer (`rule_areas::rule_area_keepouts`: `keepout` for tracks and vias, `via_keepout`,
      `wire_keepout`, plus `place_keepout` for footprints; nothing for an area banning only pads
      or the pour), no clearance class; in a footprint's image the board builder names them
      `keepout_<n>`, `via_keepout_<n>`, `place_keepout_<n>` (Java
      `Library.generateMissingKeepoutNames`). A copper-less round hole is a footprint keepout
      circle of the drill plus twice the board's hole clearance (`pads::make_pads`), which the
      hole override puts in class `hole_edge`. The copper-pour flag reached nothing (the DSN AST
      has no field for it).
    - *Fix*: the reader now reports each rule area's copper layers, bounding box and pour flag
      (`freerouting_kicad::rule_areas::RuleAreaExtent`, `rule_area_extent`, `Extras::rule_areas`,
      footprints' as filed then the board's), carried in board coordinates per layer
      (`drc::BoardRuleArea`, `KicadReportInputs::rule_areas`, `KicadReportRules::rule_areas`).
      `drc/zone_islands.rs`: `keepout_fill` takes the board and the layer and, on a `.kicad_pcb`,
      `kicad_pcb_keepout_fill`: an unnamed (or builder-named, `is_generated_name`) keepout whose
      box is a rule area's on that layer within 1 µm (`RULE_AREA_SLACK_UM`) is cut at its outline
      if the area bans the pour and left out if it allows it, whatever its kind; a footprint's
      circle keepout (the NPTH) is cut at its outline; every keepout the reader names and an
      unnamed one of no rule area keep the clearance (Java's). `KeepoutFill::Copper` is now
      `Clearance`. `quality/pour_fill.rs`: `kicad_pcb_opening` sets `kicad_keepouts`, so the
      cleanup's pieces, K15.29's island model, `opened_fill_core` (the pour target) and K15.31's
      contacts all take it. A first version also left out the fitted `solder_mask_pads` and
      `thermal_spoke` keepouts, as K15.35 does on a DSN: canary-pwr's both-settings arm went from
      26 to 30 (session `e5521ac8`); with them kept (`K1538_NOMASK` diagnostic) it is main's
      session `cce88912` again, so they keep Java's clearance on a `.kicad_pcb` (module docs).
    - *Tests* (`crates/freerouting/tests/k15_pour_keepout.rs`, `kicad_board`):
      `the_kicad_pcb_model_reaches_a_rule_area_as_kicads_fill_does` and
      `the_kicad_pcb_model_reaches_an_npth_as_kicads_fill_does` (K15.35's checks on
      `a-l1-keepout.kicad_pcb` and `b-f1-npth.kicad_pcb`), and
      `a_rule_area_that_allows_the_pour_does_not_cut_the_kicad_pcb_model` on
      `tests/data/kicad/k1538-pour-allowed/` (`make_boards.py`: zone POUR, board rule area R1
      banning tracks and vias, footprint U1's (90 degrees) R2 banning tracks only, both allowing the
      pour, board rule area R3 banning the pour; refilled by KiCad 10.0.7, which fills through R1
      and R2 and cuts at R3's outline): 50 points inside R1 and R2 and 25 across R3, each checked
      against KiCad's fill read from the file and against the model; it also pins the extents
      (`[false, false, true]`, the three boxes). RED on main's model: 3 failed ("KiCad's fill at
      (200.6372, 114.0000) mm is outside the model", "(95.9176, 122.8327)", "the model at (5.01,
      5.01) mm"; R2 alone: "(17.01, 5.01)"). GREEN: 5 passed.
    - *Arms*: release CLI of this branch (`bin/v2`), `scripts/pcu3.sh` (K15.23's `.kicad_pcb`
      command on the stripped pcu board) and `scripts/canary3.sh` (K15.14's on canary-pwr
      `95882f00`), graded by KiCad 10.0.7; base rows are main's (`k1531/bin/v3`):

      | arm | KiCad unconnected | router incomplete | below-minimum | manufacturing | routing-caused | session |
      |---|---|---|---|---|---|---|
      | pcu unset, main | 47 | 11 | 0 | 0 | none | `bd0a87fd` |
      | pcu unset, K15.38 | 47 | 11 | 0 | 0 | none | `bd0a87fd` |
      | pcu target, main | 49 | 14 | 0 | 0 | none | `a1118e6c` |
      | pcu target, K15.38 | **51** | 16 | 0 | 0 | none | `2d96bc75` |
      | pcu both, main | 21 | 22 | 0 | 0 | none | `25cf997a` |
      | pcu both, K15.38 | 20 | 21 | 0 | 0 | none | `aa9c84fd` |
      | canary unset, main | 40 | 27 | 0 | 0 | none | `f1d2da90` |
      | canary unset, K15.38 | 40 | 27 | 0 | 0 | none | `f1d2da90` |
      | canary target, main | 31 | 19 | 0 | 0 | none | `0b534572` |
      | canary target, K15.38 | 31 | 19 | 0 | 0 | none | `0b534572` |
      | canary both, main | 26 | 27 | 0 | 0 | `track_dangling` 1 | `cce88912` |
      | canary both, K15.38 | 26 | 27 | 0 | 0 | `track_dangling` 1 | `cce88912` |

      Canary's three sessions are main's: its two In2.Cu rule areas (tracks banned, pour allowed)
      made main's model of the In2.Cu `+12V_BUS` and `+5V_PI` pours empty, and the model now has
      1111.78 and 38.21 mm2 of KiCad's 1112.22 and 38.39 (a temporary probe on the first
      version's routed both-settings board refilled by KiCad, 0.1 mm grid, `probe/`), but no
      route changes. K15.36's CAN_EN `track_dangling` is still in the
      both-settings arm. The pcu target arm: with the NPTH circle keeping its clearance
      (`K1538_NONPTH` diagnostic) it is main's `a1118e6c` (49); with the rule areas keeping theirs
      (`K1538_NORULE`) it stays `2d96bc75` (51). The 2 more unconnected are 2 more failed
      connections (router incomplete 14 to 16; failures on SDA, SCL, `/CH_SHDN_1`,
      `/Charger Circuit/REGN`, no routing-caused item), so the NPTH cut moves the route rather
      than misjudging copper; the both-settings arm improves by 1.
    - *`legacy`*: `PARITY_SET=tools/bench/sets/connection-parity.txt tools/parity-check.sh
      --no-probes`: PARITY OK, 7 of 7 `[exact]`, `bytes true`. `legacy` runs no cleanup, and Java's
      fill (`fill_operands`) passes `kicad_keepouts` false.
    - *Gate*: `cargo fmt --all --check` clean; `cargo clippy --workspace --all-targets -- -D
      warnings` clean; `cargo test --workspace --no-fail-fast`: 2998 passed, 0 failed, 56 ignored.
      Docs: `quality/pour_fill.rs` (Keepouts on a `.kicad_pcb`), `drc/zone_islands.rs`,
      `freerouting-kicad` `rule_areas.rs`, DIVERGENCES (the `cleanup.remove_stubs` entry, "K15.38,
      on a `.kicad_pcb` input").

- [x] **K15.39 — The DSN path's fill model keeps the plugin's pad and edge keepouts.**
  - **Change:** K15.38's reading of K15.35's DSN keepout rule (`drc/zone_islands.rs::keepout_fill`,
    DSN input): it leaves out `solder_mask_shared` keepouts although the pads under them are cut
    back (`pad_trim`; the plugin does the same), so the model can fill over real pad copper, and it
    cuts the plugin's edge and pad-clearance keepouts (K09's fix-ups, `kicad-plugin/freerouting_rs/`)
    at their outline, where they stand for a clearance KiCad's filler keeps. Both make the model
    wider than KiCad's fill (the side Q16.4 allows, but a kept via KiCad calls dangling is a
    routing-caused item). Neither is measured. Measure both on the pcu DSN arm against KiCad's
    refill, and take each keepout as the copper or clearance it stands for.
  - **Accept:**
    - a test per changed keepout kind on a small DSN cut from the routed pcu board (named in the
      test, every coordinate from it): the model holds KiCad's fill there and not the pad's copper
      or the clearance strip;
    - the pcu DSN arm (K15.34's command, plugin defaults), graded by KiCad: unconnected not up
      (44), 0 routing-caused, 0 below-minimum, 0 manufacturing breaches; the `.kicad_pcb` arm
      unchanged;
    - `legacy` unchanged (the cleanup is `enhanced` only).
  - *Done 2026-10-22* (branch from main `c4f74fb`; every number is the first seen, before any later
    measurement; runs, refill, probe and scripts in `target/orchestrator/k1539/`). Decided
    2026-10-22: nothing changed. The measurement finds no place on the pcu DSN route where
    the model is wider than KiCad's fill because of these keepouts (the dispatch's rule: say so with
    the numbers and change nothing). So no keepout kind changed, and no test was added.
    - *Method*: main's release CLI (`bin/base`, `c4f74fb`) on K15.34's arm (`runs/arms.sh`,
      `ARMS=base`, plugin defaults): session `6a90062c...` (K15.34's and K15.35's), DSN identical to
      K15.35's but for its path. `refill/` is the routed board after `kicad-cli pcb drc
      --refill-zones --save-board` (KiCad 10.0.7), dumped by `scripts/fill_dump.py`
      (`probe/kicad-fill.txt`). K15.35's probe (`probe/zz_k1539_probe.rs`, a temporary test, not
      committed) dumped the cleanup's model with the plugin's flags (`probe/model-base.txt`,
      byte-identical to K15.35's `model-full-fix.txt`). `scripts/keepouts.py` compares the model with
      KiCad's fill inside each structure-level keepout and within a distance of it, and
      `scripts/edge.py` does the same in a band along the board outline (shapely, slivers under 10 um
      dropped).
    - *The keepouts on the DSN*: 58 `solder_mask_pads` (fitted, left out), 8 unnamed edge strips
      (F.Cu and B.Cu, 0.35 mm inside the outline: 0.5 mm edge clearance less the 0.15 mm default
      clearance; the plugin log says "8 edge strip(s) (edge clearance 0.5 mm), 0 pad circle(s)"),
      1 `copper_graphic`, and the footprints' rule areas and NPTHs (K15.35's). No
      `solder_mask_shared` keepout (no footprint aperture lies over pads of two nets, so `pad_trim`
      cuts no pad), no `solder_mask` or `thermal_spoke` keepout and no pad-clearance circle.
    - *Mask apertures* (`solder_mask_pads`): 24 aperture-pour pairs overlap. Inside the apertures
      KiCad fills 80.35 mm2, and the model holds exactly that (0.0000 mm2 wider, 0.0000 narrower).
      Taking them as copper with the clearance would take those 80.35 mm2 out of the model. Within 0.5 mm
      of them (32 pairs) the model is 0.2395 mm2 wider, all outside the apertures. The largest piece,
      0.4210 mm2 of `Net-(Q10-S-Pad1)` F.Cu at (83.549, 68.391), lies 0.39 mm off Q10's pad 5, and the
      model's distance from that pad equals KiCad's (0.3925 mm), so the piece is not the keepout's.
    - *Edge strips*: no judged pour comes within 0.6 mm of the outline, so 0.0000 mm2 lies wider or
      narrower within 0.5 mm of the strips. The nearest pour, `Net-(Q1-D)` F.Cu, starts 0.95 mm from
      the edge in both the model and KiCad's fill (its zone outline), and the strips reach 0.35 mm in.
      Within 2 mm of the strips the model is 0.0179 mm2 wider: 2 pieces of 0.009 mm2 at the zone
      outline's corners, (126.81, 32.93) and (136.44, 32.93), 0.95 mm from the edge, which are not
      the strips. Taking a strip with the clearance it encodes (0.5 mm) would not be KiCad's
      either: KiCad's fill of the inner planes keeps 0.3815 mm from the edge, the pcu `.kicad_dru`'s
      inner `edge_clearance` of 0.381 mm, below Board Setup's 0.5 mm.
    - *Elsewhere, for a later item* (DSN text alone, `scripts/static_overlap.py`, not routed or
      refilled): fc-rom's 4 `solder_mask_shared` keepouts meet no plane, and canary-pwr's 6 pad
      circles and 12 edge strips meet only GND pours, which the reader drops (GND has a plane on the
      power layer Gnd.Cu). CM5_MINIMA_3's 8 pad circles (two mounting holes) meet its GND F.Cu and
      B.Cu planes over 211 mm2, and those pours are judged (every layer is a signal layer).
    - *Accept 1*: no keepout kind changed, so no test.
    - *Accept 2*: `runs/arms.sh` (`BIN=bin/base ARMS=base`), graded by KiCad 10.0.7: unconnected 44
      (not up), routing-caused none, below-minimum 0, manufacturing breaches 0, cleanup 22 vias and 5
      traces, route 164.8 s, session `6a90062c...`. The `.kicad_pcb` arm (`runs/arms-kicad.sh`,
      K15.23's command): unconnected 47, routing-caused none, below-minimum 0, manufacturing 0,
      session `2e8bf394...` (K15.36's row), so unchanged. No code changed.
    - *Accept 3*: no code changed, so `legacy` is unchanged. The parity check was not run, because
      the orchestrator's K15.37 heavy run forbade routes then.
    - *Gate* (docs-only diff): `cargo fmt --all --check` clean; `cargo clippy --workspace
      --all-targets -- -D warnings` clean; `cargo test --workspace --no-fail-fast`: 3001 passed, 0
      failed, 56 ignored. DIVERGENCES: the `cleanup.remove_stubs` entry, "K15.39, on a DSN input".

- [ ] **K15.40 — A pad-clearance keepout does not cut the fill model where KiCad's fill reaches.**
  - **Change:** K15.39's static check (`target/orchestrator/k1539/scripts/static_overlap.py` on the
    corpus DSNs, `progress-k1539.md`) found CM5_MINIMA_3's 8 pad-clearance circles (K09's
    `edge_and_pad_keepouts`, two mounting holes at (127.25, 77.6) and (127.25, 29.6) mm) meeting
    its judged GND F.Cu and B.Cu pours over 211 mm2 (K13.6's runs, `target/orchestrator/k136/out/`).
    The circles carry no net, so the fill model keeps the clearance from them around a pad of the
    pour's own net, where KiCad's filler joins that pad to the pour (the model narrower than
    KiCad's fill, against Q16.4), and takes the default clearance around another net's pad (wider).
    The `.kicad_pcb` reader writes the same keepouts (`pad_clearance`, which K15.38 keeps with
    their clearance). K15.39 also saw the pcu refill keep the inner planes 0.3815 mm from the edge,
    the `.kicad_dru`'s inner `edge_clearance` (0.381 mm, below Board Setup's 0.5 mm), against
    `freerouting-kicad/src/dru/floors.rs`'s note that a rule below Board Setup changes nothing.
    Measure CM5_MINIMA_3's route on both inputs against KiCad's refill there, and take a
    pad-clearance keepout as KiCad's filler takes the pad (joined to its own net's pour, the
    clearance from another net's), and the edge clearance as the filler applies it.
  - **Accept:**
    - a test on a small board cut from CM5_MINIMA_3's route (named in the test, every coordinate
      from it): the model holds KiCad's fill round a mounting-hole pad of the pour's net, and not
      the clearance round another net's;
    - CM5_MINIMA_3's `.kicad_pcb` and DSN arms (K13.6's commands), graded by KiCad: unconnected
      not up, 0 routing-caused, 0 below-minimum, 0 manufacturing breaches; the pcu and canary-pwr
      `.kicad_pcb` arms' sessions unchanged unless the edge clause changes them (then not up);
    - `legacy` unchanged (the cleanup is `enhanced` only).


- [ ] **K15.41 — The pour pair's route time.**
  - **Change:** K15.37's run (`target/orchestrator/k1537/out/`, engine `40c9f7f`) failed guard (4):
    with `router.pour_fill_contacts` and `router.pour_fill_target` both on, route time over the
    judged rows and the rows that time out only with the pair is 679.9 s -> 1323.1 s (1.95 x,
    limit 1.25 x). The slowest rows: complex_hierarchy 0.2 s -> 44.3 s, interf_u 6.6 s -> 20.5 s,
    m-fc 24.5 s -> 70.2 s, wisweep 22.8 s -> 75.9 s, canary-pwr 92.8 s -> 153.2 s, and two rows
    that complete without the pair and time out with it: kit-dev-coldfire-xilinx_5213 173.3 s ->
    240 s (219 unconnected at the deadline) and pcie-active-backplane 40.9 s -> 240 s (28). With the
    target alone K15.30's arms cost little on those rows (complex_hierarchy 0.2 s -> 0.3 s,
    kit-dev-coldfire 171.1 s -> 182.7 s, pcie 40.8 s -> 43.2 s), so the time comes with
    `pour_fill_contacts` (`board/pour_contacts.rs`: `PourContacts`, `pour_islands`,
    `pour_aware_contacts`, `pour_cycle`, `note_pour_change`; `connected_set_recu` through
    `connectivity_contacts`). Profile the pair on complex_hierarchy and pcie, find what costs, and
    make it cheaper without changing a route.
  - **Accept:**
    - the cause named with its profile numbers (calls and time per function, before and after);
    - with the pair, K15.37's command per row (`k15_all_pours.py`'s `pcb-on` command with this
      branch's binary): the sessions of complex_hierarchy, interf_u, pcu and canary-pwr
      byte-identical to K15.37's `pcb-on` ones (`c7a0e236`, `0257be2e`, `aa9c84fd`, `7811ea5a`),
      or each changed route named and graded by KiCad (not worse); pcie and kit-dev-coldfire
      COMPLETED within 240 s, and each named row's route time at most 1.25 x its K15.37 `pcb-off`
      time or within 5 s of it (complex_hierarchy's 0.2 s);
    - a test pinning that the contacts' cache gives the same connected sets as the uncached rule
      (`tests/k15_pour_fill_contacts.rs`);
    - `legacy` unchanged (the setting is unset there; parity 7 of 7 if shared code changes).

- [ ] **K15.42 — The pour pair's routing-caused DRC.**
  - **Change:** K15.37's run failed guard (2): routing-caused DRC over the judged rows 12 -> 27 with
    both settings on. interf_u 1 -> 7 (`starved_thermal` 1 -> 5, `via_dangling` 0 -> 2), moteus
    4 -> 13 (`isolated_copper` 3 -> 12; 3 -> 9 with the target alone in K15.30's run), openmuscle
    1 -> 2 (`starved_thermal`); nudac 1 -> 0. Attribute each added violation with KiCad's DRC report
    (`grade.json` under `target/orchestrator/k1537/out/<row>/pcb-on/`) to the routed items, find
    why the pair leaves it (a trace end in a pour that starves a pad's thermal spokes, a via left
    in a pour island, a pour island that the route leaves isolated), and fix it in the router or
    the cleanup.
  - **Accept:**
    - a test per fixed cause on a small board cut from the row's route (named in the test, every
      coordinate from it), RED before the fix;
    - with the pair, K15.37's command per row: interf_u, moteus and openmuscle drc_real at most
      their K15.37 `pcb-off` values (1, 4, 1), KiCad unconnected not above their K15.37 `pcb-on`
      values plus their bands (13 + 11, 30 + 22, 3 + 8); pcu and canary-pwr 0 routing-caused and
      unconnected not above 20 and 26 plus their bands; 0 below-minimum and 0 manufacturing
      breaches in every arm;
    - `legacy` unchanged (parity 7 of 7 if shared code changes).

- [ ] **K15.43 — Measure the pour pair again under `enhanced`.**
  - **Change:** after K15.40, K15.41 and K15.42, register the pair's measurement again on K15.37's
    set (`tools/bench/sets/k15-fill-pair.txt`), modelled on `docs/roadmap/prereg/K15.37.json`
    with its guards and decision table, and run it through the orchestrator's heavy queue.
  - **Accept:**
    - the registration (prereg JSON, its pin test, the set) committed before any run;
    - the records and the verdict in the note, and the outcome applied as its decision table says
      (both settings into `ENHANCED` and K15.30's and K15.37's plugin options removed, or the
      options kept);
    - `legacy` unchanged (the settings are unset there; parity 7 of 7).

- [ ] **K15.44 — An unconditional `.kicad_dru` minimum replaces Board Setup's, as in KiCad.**
  - **Why** (2026-10-22, the power-control-unit session's runs in its scratchpad `layout/`):
    - The board is `pw6.kicad_pcb`: Board Setup `min_track_width` 0.2 mm, and the fab
      `.kicad_dru` has `track_width (min 0.127mm)` with no condition.
    - Run S, the old build `8c952e7`, reached 82 unrouted. KiCad 10.0.6 grades it clean under
      the project's rules (`track_width 0`, `annular_width 0`), with 611 segments at 0.15 mm.
    - Runs X and Y, at `4f7866b`, reached 129 and 125, with every track at 0.2 mm or more.
    - KiCad's custom rules take precedence over Board Setup for the same constraint, so the
      board allows 0.127 mm. K15.11 found the same for clearance.
    - The router takes the larger value: K15.3's `raise_board_floors` only raises, over Board
      Setup's floor from Q18.
    - The plugin also passes Board Setup's minimums, and `min_trace_width_um` set to the board
      minimum, as command-line flags (`options.py`, `m_TrackMinWidth` and the others), which a
      rule cannot override.
  - **Decided 2026-10-22:** follow KiCad. The router's floor for a constraint is the value KiCad
    would check against: an unconditional `.kicad_dru` `min` replaces Board Setup's, larger or
    smaller. Several such rules for one constraint follow KiCad's own order. Where no rule
    applies, Board Setup's value stays. A value the user sets on the command line or in a
    settings file still wins.
  - **Change:**
    - Router: for `track_width`, `clearance`, `hole_size` (via drill), `annular_width` (via) and
      `edge_clearance`, the `.kicad_dru` floor replaces a value whose source is the `.kicad_pro`
      Board Setup (`board_minimums_patch`, DSN priority) instead of taking the larger. The log
      line and `--dump-settings` say which won and why.
    - The class-limited path (`class_track_width`) and K15.3's layer rule (one edge clearance,
      the largest over the layers) stay.
    - Hole clearance and hole-to-hole (Q22.5, Q22.14) take the same precedence if the change
      is the same code; otherwise say why not.
    - Plugin: Board Setup's minimums reach the router so that the board's `.kicad_dru` can
      replace them, on both inputs. For example, pass them at a lower priority than the command
      line, or leave a minimum out when an applied unconditional rule sets it.
    - The plugin's default `min_trace_width_um` follows the same effective minimum.
    - A user's explicit `min_trace_width_um` option still wins.
    - Update K15.3's note, the DIVERGENCES K15.3 entry and `help.txt`'s `--kicad-dru` paragraph.
  - **Accept:**
    - tests on both inputs:
      - Board Setup 0.2 with an unconditional dru 0.127 gives a track floor of 127 µm;
      - Board Setup 0.1 with a dru of 0.127 gives 127 µm;
      - no rule leaves Board Setup's value;
      - an explicit `--router.board_minimums.track_width_um=200` stays 200;
    - a plugin test: the arguments for a board with such a rule let the router take the rule's
      value;
    - on the pcu board (`target/orchestrator/pcu-rev2/boards/committed-1b161c7`),
      `--dump-settings` shows `board_minimums.track_width_um = 127` from `kicad_dru`;
    - KiCad's DRC still 0 `track_width` on a route of a small board with such a rule;
    - `tools/parity-check.sh --no-probes` 7 of 7 (no parity board has a `.kicad_dru`);
    - `python3 -m pytest kicad-plugin/tests`.
