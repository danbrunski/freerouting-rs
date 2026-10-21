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
  `hole_to_hole`. The pcu file's 13 rules are mostly unconditional board-wide minimums:
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

- [ ] **K15.1 — One item with an unbounded box no longer empties the board's bounding box.**
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

- [ ] **K15.3 — Board-wide `.kicad_dru` rules apply.**
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
  - **Accept:**
    - dru tests on a copy of the pcu `.kicad_dru` (copied into `crates/freerouting-kicad/tests/data/`
      with its source named): every rule is APPLIED or reported with its reason, and none says
      "rule without a condition";
    - each mapped setting has the rule's value on a small board, on both inputs;
    - a rule below Board Setup's value leaves Board Setup's;
    - module docs and a DIVERGENCES line if KiCad's semantics are narrowed (one edge clearance for
      all layers).

- [ ] **K15.4 — No track below the board's minimum on the pcu board.**
  - **Change:** the report's v5 output had 495 segments under the 0.2 mm Default width: 0.15 mm
    (476), 0.12 (11), 0.10 (5). KiCad flagged 16 below 0.127 mm, the `.kicad_dru` minimum that bug
    3 lost. With K15.3, no routed track goes below the largest of Board Setup's minimum and the
    `.kicad_dru`'s. Find where each width under the class width comes from (neck-down at a pad,
    the optimizer, existing copper) and where the odd widths come from (0.2498, 0.2502, 0.2538,
    0.2792, 0.3554 mm). Neck-down below the class width but at or above the minimum is allowed;
    say where the board's own copper had such widths before the route.
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
  - **Accept:** a CLI test with stdin closed and with a pipe; the help text; the CLI docs.

- [ ] **K15.9 — A pad's own clearance is kept on a `.kicad_pcb` input.**
  - **Change:** the report saw a fiducial's 0.6 mm local clearance broken by a GND trace and called
    pad-local clearances "not read". The reader writes pads' own clearances as keepouts since
    K13.7 (`pad_clearance::read_pad_clearances`, `read.rs`). Find which clearance the fiducial
    carries (pad, footprint, or the solder-mask opening) and whether the reader at main keeps it.
    Fix what is missing.
  - **Accept:**
    - the pcu fiducial cut into a small fixture: the keepout is there;
    - a route keeps 0.6 mm from it;
    - the doc line the report read says what is kept.
