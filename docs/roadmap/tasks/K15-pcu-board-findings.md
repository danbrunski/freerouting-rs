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

- [ ] **K15.25 — The router's search reaches a `.kicad_pcb` signal pour where it is filled.**
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

- [~] **K15.26 — The cleanup judges a DSN input's signal-layer pour by the fill model.**
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
      clearance of the board's signal-layer zones (K15.27);
    - `USB_GND`'s trace: trimming its dangling ends (K15.28);
    - `U24-OUT`'s via: the fill model fills over own-net pads (K15.29).
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

- [ ] **K15.27 — The plugin's DSN path passes its signal-layer zones' thickness and clearance.**
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

- [ ] **K15.28 — The cleanup trims a trace's dangling end segment.**
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

- [ ] **K15.29 — A via in a pour island KiCad drops is no contact in the cleanup.**
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
      the `.kicad_pcb` arm (K15.23's command): unconnected not up (47), 0 routing-caused;
    - `legacy` unchanged.
