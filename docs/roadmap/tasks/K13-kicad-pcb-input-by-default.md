# K13 — Read the KiCad board directly by default

Opened 2026-10-20 at the owner's request. K10 added the plugin option `board_input = dsn |
kicad_pcb`, and K10.5 measured both inputs on ten boards. The direct read graded the same or
better on seven: `CM5_MINIMA_3` 195 → 4 errors (the export writes each of its 200 arcs as one
chord), `video` 1 → 0, and `canary-pwr` 42 → 38 unconnected. `dsn` stayed the default under
question 1's rule (decided 2026-10-18: switch only if `kicad_pcb` grades the same or better on
every K10.5 board). `One-Air-Max` held it back with one more clearance error, and `vme-wren`
and `jetson-agx-thor-baseboard` were not measured. `One-Air-Max`'s extra error is a violation
that was already on the board, counted against one more segment pair because the two paths round
existing copper differently ([archive/K10](archive/K10-plugin-direct-board-read.md), K10.5).

Since then, nearly every real rule breach found in routed output came from something the DSN
does not carry. The router now gets these as flags or from the `.kicad_pro`:
- Board Setup minimums (Q18.3);
- copper-to-edge and hole clearance (Q18);
- Margin-layer and inner Edge.Cuts lines (Q18.5);
- solder-mask expansion (Q18.7);
- the rules KiCad converts for old boards (Q19).

The board file carries all of them. This checklist makes `kicad_pcb` the plugin's default once
it meets K13's switch rule (below). `dsn` stays selectable.

Prerequisites: K10, Q18 (minimums read from the `.kicad_pro`), Q19 (grading under the board's own
rules).

## Decisions made here

- **The switch rule replaces question 1's rule for K13** (decided 2026-10-20; question 1's rule
  was "the same or better on every board"). The default changes once K13.6's measurement shows:
  - `kicad_pcb` has zero routing-caused manufacturing breaches on every board, graded under the
    board's own rules (`docs/roadmap/routing-quality.md`, order of importance item 1);
  - `kicad_pcb`'s total KiCad unconnected over the set is no higher than `dsn`'s.

  A board where `kicad_pcb` grades worse doesn't block the switch, but K13.6 explains it. If
  `kicad_pcb` has a breach that `dsn` doesn't, the switch waits for its fix.
- **Results still come back through `ImportSpecctraSES`** (K10's decision). Writing to the
  open board stays out of scope.
- **When the reader refuses a board, the plugin falls back to the DSN export** and says so.
  Examples are a pre-KiCad-6 file or a construct the reader doesn't support. A run never fails
  just because of the input choice.
- **Saved options keep an explicit choice.** A board whose saved options say `dsn` keeps `dsn`.
  Only boards with no saved choice get the new default.

## File ownership

| Item | New files | May edit |
|---|---|---|
| K13.1–K13.4 | reader tests and fixtures under `crates/freerouting-kicad/tests/` | `crates/freerouting-kicad/src/**`, `kicad-plugin/freerouting_rs/{snapshot,export_flow,plugin}.py` |
| K13.5–K13.6 | `tools/bench/sets/k13-input.txt`, `docs/roadmap/prereg/K13.json` | `tools/kicad-reader/plugin-input-ab.sh` |
| K13.7 | — | `kicad-plugin/freerouting_rs/options.py` (default, schema), the options dialog, `docs/development/kicad-plugin.md`, `kicad-plugin/README.md`, `docs/DIVERGENCES.md` |

## Checklist

- [x] **K13.1 — What each input carries.** Make a table of every rule and item the router
  reads, with a column per input: the plugin's DSN export and the reader on the snapshot. At
  least these rows:
  - net classes and clearances, and custom `.kicad_dru` rules;
  - the Board Setup minimums, copper-to-edge and hole clearance;
  - Margin-layer and inner Edge.Cuts lines, and solder-mask expansion;
  - arcs, rule areas and keepouts, copper text;
  - zones and their fill, locked tracks and vias;
  - footprint courtyards;
  - coordinate precision.
  - **Accept:**
    - the table is in the done note;
    - every row the reader carries has a test that names it;
    - every row the reader lacks is fixed in the reader (test first) or listed with the board
      it would affect;
    - the `.kicad_pcb` path applies the `.kicad_dru` item-type and hole rules by default, as the
      DSN path does since Q22.5. Today it needs `--apply-design-rules` (K10.3) (decided
      2026-10-20: the board's own rules reach the router on both inputs);
    - the two inputs filter mask expansions at the smallest class clearance the same way. Today
      the plugin zeroes each side at or under the floor, and the reader keeps both sides when
      either exceeds it (found in Q22.12).

  *Done 2026-10-20:* written on main `a18b970`, moved onto `c2cc058` (Q22.4's net shapes, which
  the table describes as landed). The DSN column is KiCad 10's export plus the plugin's
  fix-ups and flags (`export_flow.prepare`, `router_args`); the reader column is
  `crates/freerouting-kicad` on the snapshot plus `job::prepare::set_kicad`. On the plugin's
  snapshot path `export_flow.router_args` passes the same Board Setup flags as on the DSN path, and
  the command line wins over the `.kicad_pro`.

  | Row | DSN export + plugin | `.kicad_pcb` reader |
  |---|---|---|
  | Net classes: clearance, width, via | carried: KiCad's `(class ...)` | carried: `classes.rs` from the `.kicad_pro`, written from the live board (K10.2) (`structure.rs::net_classes_rules_and_vias_equal_the_export`, `synthetic_projects_give_kicad_classes`, `test_snapshot_headless.py::test_an_unsaved_net_class_change_reaches_the_snapshot_only_through_the_live_project`) |
  | Net-class and class-via order | KiCad's `unordered_map` order | carried differently: project order (`kicad_parity.rs` "order:net classes", `common/copper.rs::via_lists`) |
  | `.kicad_dru` class-pair clearances, class track widths (K07.5) | carried: `--kicad-dru` (`test_kicad_dru_args.py::test_a_dsn_route_gets_the_kicad_dru`) | carried, **by default since this item** (`cli_kicad.rs::the_apply_design_rules_switch_changes_the_class_rules`, `dru.rs::supported_rules_change_the_tree_exactly`, `kicad_read.rs::design_rules_reach_the_board`) |
  | `.kicad_dru` item-type clearances (Q22.5) | carried: `--kicad-dru` (`dru_item_types.rs`, 13 tests) | carried by default (new: `k13_inputs.rs::the_kicad_pcb_reader_applies_item_type_rules`, `cli_kicad.rs::a_kicad_pcb_route_applies_the_item_type_and_hole_rules_by_default`) |
  | `.kicad_dru` `hole_clearance` (Q22.5, Q22.15) | carried (`dru_item_types.rs::the_job_applies_the_rules_and_the_hole_floor`, `dru_holes.rs::a_plated_hole_rule_becomes_via_and_pin_to_track_clearances`) | carried by default (new: `cli_kicad.rs::a_kicad_pcb_route_applies_...`, `router.hole_clearance_um` 250 → 600) |
  | `.kicad_dru` `hole_to_hole` (Q22.14) | carried (`dru_holes.rs::the_job_raises_board_setups_hole_to_hole`) | carried by default (`dru_holes.rs::a_kicad_pcb_job_takes_the_larger_of_board_setup_and_the_rules`; new: `cli_kicad.rs` 250 → 500) |
  | `.kicad_dru` terms the router cannot evaluate (`insideCourtyard`, `intersectsArea`, zone rules, pad pairs) | missing: reported UNSUPPORTED or left out, same code | missing, same (`dru.rs::canary_pwr_rules_are_unsupported_by_name`). Boards: canary-pwr (10 `insideCourtyard`), canary-hat (`insideCourtyard`, RF-to-zone), vme-wren (`intersectsArea`), jetson-agx-thor-baseboard (zone) |
  | Board Setup minimum track width, via diameter, drill, annular ring (Q18.3, Q18.4) | carried: plugin flags from the live board (`test_options.py::test_board_minimums_become_hard_floors`, `test_router_args_carry_the_board_minimums`) | carried: `.kicad_pro` (`board_minimums.rs::a_kicad_pcb_input_passes_its_board_setup_minimums`, `kicad_board_minimums_default_to_kicads_and_zero_is_none`) |
  | Board minimum clearance (Q22.10) | carried (`test_min_clearance.py`) | carried (`clearance_floor.rs::a_kicad_pcb_input_passes_its_minimum_clearance`) |
  | Hole-to-hole minimum (Q22.11) | carried (`test_hole_to_hole.py`) | carried (`hole_to_hole.rs::a_kicad_pcb_input_passes_its_hole_to_hole_minimum`) |
  | Copper-to-edge clearance (Q18.5) | carried: `--router.copper_to_edge_clearance_um`, plus K09.5's edge strips with `edge_and_pad_keepouts` (`test_dsn_fixups.py::test_edge_and_pad_keepouts_on_a_two_layer_board`) | carried: `min_copper_edge_clearance` (`board_minimums.rs::a_kicad_pcb_input_passes_its_board_setup_minimums`); no strips, the router's edge class keeps it |
  | Hole clearance (Q18.5) | carried: `--router.hole_clearance_um`; NPTH holes as keepouts of drill + 2 × hole clearance (KiCad's export) | carried: `min_hole_clearance` (same test); NPTH holes the same (`pads.rs::make_pads`, `structure.rs::pads_equal_the_export`) |
  | Margin-layer and inner Edge.Cuts lines (Q18.5) | carried: `edge_graphics.py` (`test_edge_graphics.py::test_a_margin_line_becomes_a_keepout_on_every_copper_layer`) | carried: `edge_keepouts.rs` (`edge_keepouts.rs::margin_and_loose_edge_cuts_shapes_become_keepouts_on_every_copper_layer`, `the_keepouts_follow_the_projects_edge_clearance`) |
  | Pads' solder-mask expansions (Q18.7) | carried: `--router.solder_mask_expansions` (`test_solder_mask.py::test_only_apertures_past_the_clearance_are_listed_per_side`) | carried: `Extras::solder_mask` (`solder_mask.rs::a_kicad_pcb_input_carries_the_pads_mask_expansions`); the floor filter is now the plugin's, per side (below) |
  | Solder-mask-to-copper clearance (Q22.12) | carried (`test_solder_mask.py::test_the_mask_to_copper_clearance_grows_every_aperture_with_a_mask`, `test_edge_graphics.py::test_the_mask_to_copper_clearance_grows_each_mask_shape`) | carried (`edge_keepouts.rs::the_mask_apertures_grow_by_the_projects_mask_to_copper_clearance`, `solder_mask.rs::the_projects_mask_to_copper_clearance_grows_the_pad_apertures`) |
  | Board-level F.Mask / B.Mask shapes (Q18.7) | carried: `edge_graphics.mask_polygons` (`test_edge_graphics.py::test_a_filled_front_mask_polygon_keeps_the_top_copper_out_of_it`) | carried (`edge_keepouts.rs::the_boards_mask_apertures_become_keepouts_on_their_copper_side`) |
  | Footprint mask shapes | missing on main: left to the pads (fc-rom J1, 7 routing-caused `solder_mask_bridge`); in flight in Q22.16, not touched here | missing on main, the same; in flight in Q22.16 |
  | Pads' own (local) clearance | carried: K09.5 circles with `edge_and_pad_keepouts`, on by default (`test_dsn_fixups.py::test_edge_and_pad_keepouts_on_a_two_layer_board`); not on a bare export (Q22.6: canary-pwr FID3, 0.5235 mm against 0.6 mm) | was missing, **fixed here**: `pad_clearance.rs`, the same circles (new: `k13_inputs.rs::pads_own_clearances_become_circle_keepouts`; plugin `test_mask_floor.py::test_pad_clearance_circles_match_the_readers` gives the same circles for the same pads). Corpus boards with such pads: canary-pwr (6 × 0.6 mm), tinytapeout-demo (3 × 0.5 mm), CM5_MINIMA_3 (4 NPTH, 1.5 and 1.7 mm), all without a net |
  | Track arcs | carried differently: one chord per arc (CM5_MINIMA_3: 195 errors against 4 in K10.5) | carried: polylines (`copper.rs::arcs_follow_the_curve`, `cm5_trace_length_matches_kicad`) |
  | Rule areas and keepouts | carried differently: KiCad's export makes every area a full keepout; `rule_area_fixup` (K01.2) rewrites them by flags (`test_rule_areas.py::test_five_board_level_areas`) | carried: by flags (`copper.rs::rule_areas_map_by_flags`, `rule_area_flag_defaults_follow_kicad`, `footprint_areas_keep_the_files_coordinates`, `wire_keepout.rs::issue754_ground_area_still_blocks_vias`) |
  | Copper graphics of no net, net ties (Q22.4) | carried: `copper_graphics.py` (`test_copper_graphics.py::test_a_footprint_line_on_copper_becomes_a_keepout_on_its_layer`, `test_net_tie_shapes_are_logged`) | carried (`copper_graphics.rs::a_footprint_copper_line_becomes_a_keepout_on_its_layer`, `net_tie_shapes_and_shapes_of_a_net_without_pins_are_keepouts`; engine `copper_graphics.rs::the_route_goes_round_the_copper_line_and_the_copper_text`) |
  | Copper text (Q22.4) | carried: the glyph outlines (`test_copper_graphics.py::test_a_copper_text_becomes_keepouts_of_its_glyph_outlines`) | carried differently: a box that holds the glyphs, no font engine (`copper_graphics.rs::a_copper_text_becomes_a_keepout_box_that_holds_its_glyphs`) |
  | Copper shapes of a net with pins (Q22.4, main `c2cc058`) | carried: conduction-area wires of the net with `(attr obstacle)` at the end of the wiring (`test_copper_graphics.py::test_a_filled_shape_of_a_net_becomes_a_conduction_area_of_its_net`, `test_a_stroked_shape_of_a_net_becomes_the_area_of_its_stroke`, `test_the_fixture_through_pcbnew_gives_the_reader_s_obstacles`) | carried, the same polygons (`copper_graphics.rs::shapes_of_a_net_become_conduction_areas_of_their_net`, `the_written_dsn_keeps_the_areas`; engine `copper_graphics.rs::other_nets_keep_clear_of_a_net_s_copper_and_the_net_connects_through_it`, `plane_as_obstacle_never_clears_a_net_shape`) |
  | Zones and their fill | carried: the zone outline as a plane, never the fill; K09's `fixed_pour_classes` and `drop_pour_nets` rewrite them (`test_dsn_fixups.py::test_fixed_pour_classes_cover_the_fill_with_fixed_wires_and_drop_its_plane`) | carried: the outline as a plane (`copper.rs::planes_and_netless_zones`, `zone_outline_arcs_are_polylines`); footprint-owned zones kept as planes, which the export drops; K09's wire rewrites missing (refused, K10.4); `fixed_pour_mode = engine` works on both |
  | Locked tracks and vias | carried: KiCad writes them fixed; `fix_existing` locks all (`test_route_dialog.py::test_fix_existing_keeps_the_tracks_locked_through_the_import_only`) | carried: `fix` wires and vias (`copper.rs::issue180_wires_equal_the_export`, 15 locked wires; `issue180_vias_are_fixed`; `test_route_dialog.py::test_fix_existing_locks_the_tracks_into_the_snapshot`) |
  | Footprint courtyards | carried as image outlines, drawing only | carried (`structure.rs::image_outlines_equal_the_export`), with the courtyards the export drops (K07.6); no routing effect (`image_outlines.rs` docs) |
  | Coordinate precision | carried differently: `%.6g`, about 1 µm lost above 100 mm; input copper put back within 600 nm after the import (Q22.3, `test_restore_copper.py::test_an_input_track_that_the_export_rounds_comes_back_at_its_nanometre_coordinates`) | carried: nanometres rounded to 0.1 µm (`numbers.rs::nm_rounds_like_kicad_not_like_frommm`, `copper.rs::copper_equals_the_export`); input copper put back within 200 nm (K13.2, `test_restore_copper.py::test_a_fully_routed_board_on_the_kicad_pcb_path_comes_back_unchanged`) |
  | Board outline, malformed outline as its bounding box | carried | carried (`structure.rs::outlines_equal_the_export`, `notched_outline_and_slot`) |
  | K09 DSN rewrites (`class_widths`, `fixed_pour_classes` as wires or auto, `drop_pour_nets`, `route_on_plane_layers`): plugin options, not board rules | carried (`test_dsn_fixups.py`) | missing: refused (K10.4, `options.input_conflicts`). Board: canary-pwr's preset (`test_dsn_fixups.py::test_the_canary_preset_file_reproduces_dsn_export_on_the_fixture`); K13.7 decides |

  - **Clause 2** (every carried row has a test that names it): the table names one per row. Added
    where none named the reader: `k13_inputs.rs::the_kicad_pcb_reader_applies_item_type_rules`, the
    two CLI tests above, and the mask and pad-clearance tests below.
  - **Clause 3** (rows the reader lacks): the pads' own clearance is fixed in the reader. The
    others are listed with their boards above: footprint mask shapes (fc-rom, Q22.16), the unsupported `.kicad_dru` terms (both inputs, same
    code), and the K09 rewrites (canary-pwr's preset, K13.7).
  - **Clause 4, the `.kicad_dru` by default on the `.kicad_pcb` path.**
    - What `--apply-design-rules` gated: only `ReadOptions::apply_design_rules` in `cli.rs`'s route
      job, which `dru::apply` uses for every rule kind (class pairs, widths, item types, hole
      clearance, hole-to-hole). The CLI's route now reads with it on unless the new
      `--no-design-rules`; `--apply-design-rules` stays and only says the default; both together
      are a usage error (exit 2). K09/K10 had no off switch (off was the default); this is it.
    - Not changed: the reader's own `ReadOptions::default()` (off, the export-parity mode the
      reader and parity tests use), the `-drc` mode and the GUI. Neither of those gets
      `--kicad-dru` on a DSN either, so both inputs still agree there.
    - Plugin: `export_flow.kicad_dru_args` passes `--apply-design-rules` for every snapshot Route,
      as it passes `--kicad-dru` for a DSN Route; `options.to_args` no longer adds it. The K10.3
      option `apply_kicad_rules` decides nothing now. The dialog's check box is ticked and greyed,
      the saved value is kept as it was, and K13.7's schema bump can drop the field. Route in GUI
      logs that it reads the rules only, on any snapshot.
    - Tests, written before the code (`--no-design-rules` did not parse on main):
      `cli_kicad.rs::a_kicad_pcb_route_applies_the_item_type_and_hole_rules_by_default` (default:
      "constraint clearance: applied", hole clearance 250 → 600 and hole-to-hole 250 → 500 by the
      named rules; `--no-design-rules`: "ignored (apply_design_rules is off)", nothing raised),
      `the_apply_design_rules_switch_changes_the_class_rules` (rewritten: the default session equals
      the `--apply-design-rules` one, and Fast wires are 3000 against 1000 with
      `--no-design-rules`), `the_two_design_rules_switches_together_are_a_usage_error`,
      `args_java.rs::no_design_rules_is_a_switch` (`EXTRA_FLAGS.len()` 14). Plugin:
      `test_board_rules_both_inputs.py` (5: the snapshot gets the switch with the option off or
      on, every profile on both inputs, the GUI on neither, `to_args` adds none, the log note);
      `test_snapshot.py` and `test_route_dialog.py` updated to the new rule.
  - **Clause 5, the mask floor.**
    - Rule picked: each side alone; a side at or under the smallest class clearance is 0, and a pad
      with no side left is not listed. That is the plugin's rule.
    - Why it is safe: `job::solder_mask` raises the pad's clearances on a side to max(clearance,
      expansion), and every class clearance is at least the floor. So a side within the floor
      would raise nothing, and zeroing it loses nothing. Keeping both sides when either exceeds
      (the reader's old rule) protects no more in that case.
    - The exception: a `.kicad_dru` class-pair rule below the floor, which K07.5 applies as
      given. Neither rule covers it, because both drop a pad whose sides are both within the
      floor. No `.kicad_dru` in the corpus or in canary has an applied class-pair clearance (7
      files scanned: their clearance rules are zone, area or courtyard rules), so it is listed
      here and not fixed.
    - The reader's expansions are equal on both sides of a pad (one margin), so its output does not
      change on any board. Only the plugin's per-layer expansions (KiCad 9+) can differ by side.
    - Code: `mask::keep_above_floor` (new), called by `read.rs`. Tests: reader
      `k13_inputs.rs::mask_expansions_are_filtered_per_side_at_the_smallest_class_clearance` (fixture
      `tests/data/k13-mask-floor.kicad_pcb`: F1 (300, 0), T1 (250, 250), T2 (300, 0), B1 (0, 300) µm;
      F2 at the floor and T3 under it not listed) and `a_side_at_or_under_the_floor_is_zeroed_and_the_other_kept`
      ((300, 100) → (300, 0); (200, 200.001) → (0, 200.001)). Plugin `test_mask_floor.py`: the same
      pads give the same list, and the same per-side pairs.
  - DIVERGENCES: freerouting-kicad (the mask line amended; "Pads' own larger clearances are copper
    keepouts"), freerouting-cli ("A `.kicad_pcb` route applies its `.kicad_dru` rules by default"),
    kicad-plugin ("A snapshot route applies the `.kicad_dru` too"). Docs: `io/kicad.rs` and
    `read.rs` (`ReadOptions`) module docs, `help.txt`, `kicad-plugin/README.md`, and
    `docs/development/kicad-plugin.md` (the `apply_kicad_rules` bullet).
  - Gate on `c2cc058` plus this change: `cargo fmt --all --check` clean; `cargo clippy --workspace
    --all-targets -- -D warnings` clean; `cargo test --workspace --no-fail-fast` 286 suites, 2737
    passed, 0 failed (on the `a18b970` base, `ui_routing.rs::live_redraw_patches_before_the_job_ends`
    hit its 120 s limit once under load and passed alone in 110.5 s); `pytest kicad-plugin/tests` 476
    passed, 20 skipped; `tools/parity-check.sh --no-probes` connection parity PASS, 7 of 7 exact.

- [x] **K13.2 — Existing copper comes back unchanged.** Find which rounding moves the ends of
  existing tracks on `One-Air-Max`. The reader rounds to 0.1 µm and the export to `%.6g`. Make
  the `kicad_pcb` path return copper that was already on the board at KiCad's own coordinates.
  - **Accept:**
    - a test routes a board with nothing left to route through the `kicad_pcb` path, and the
      imported board's DRC report equals the original's;
    - on `One-Air-Max`, the pre-existing `/Solar_module/SYS`–`VBAT` violation counts against
      the same segment pairs as on the original board.
    - *Amended 2026-10-20* (decided 2026-10-20): the second clause is met when the violation has
      the original's item count and distance, and every `SYS` and `VBAT` segment comes back with
      the original's nanometre geometry and uuid. Which of two equally close segments KiCad names
      follows the board's item order, which neither `ImportSpecctraSES` nor the pcbnew API can
      rebuild. The note below shows both hold.

  *Done 2026-10-20* (first written as a Partial note, before the amendment above): the copper comes back unchanged, and the first clause holds. On the second
  clause, the item count and the distance now match the original, but KiCad names a different
  segment in one of the two items.
  - **The rounding.** The `kicad_pcb` path rounds twice: the reader to 0.1 µm, then the session to
    0.1 µm. `ImportSpecctraSES` replaces every unlocked track and via with the session's copy.
    554 of One-Air-Max's 2064 input tracks, arcs and vias are off the 0.1 µm grid (the export's
    `%.6g` would move 560). The fix is Q22.3's `restore_copper.py`, with the 200 nm bound on this
    path (`options.board_input`, passed by `plugin.finish_import` and `headless_run.py import`). A
    restored item also gets the original's uuid, because KiCad's attribution can follow the item
    order (below).
  - **Clause 1, the board with nothing left to route.**
    - The stub test is
      `test_restore_copper.py::test_a_fully_routed_board_on_the_kicad_pcb_path_comes_back_unchanged`:
      the Route flow on the `kicad_pcb` path, every input track and via back with its geometry and
      uuid.
    - The real run is `ecc83-pp`, a KiCad 10.0.7 demo (`target/orchestrator/k135/boards/ecc83`).
      Command: `tools/kicad-reader/plugin-input-ab.sh kicad_pcb` with `{"schema": 5, "profile":
      "enhanced"}`, the release `freerouting-rs` built from `ba38e3f`, and KiCad 10.0.7.
      - The input's DRC and the routed board's DRC are equal as multisets: 17 items each, compared
        by type, severity, description and item descriptions and positions, with uuids left out
        (`target/orchestrator/q223/scratch/drc_equal.py`).
      - All 59 input copper items are on the routed board to the nanometre. The router added 11
        segments, stubs to pad centres.
      - The control is the same session imported by `main`'s `headless_run.py` (a8b4ae7, no
        restore). Its DRC is also equal (17 = 17), but 23 of the 59 input items came back moved.
        So on this board the copper comparison tells the two imports apart, and the DRC comparison
        does not.
  - **Clause 2, One-Air-Max** (`target/orchestrator/k135/boards/openair-max`, same command and
    options). The route took 210 s with no pass cap and gave KiCad unconnected 2. This run used the
    code before the last change, which only indexes the restored ends by grid cell (same matches;
    `test_restore_copper.py` and the five Q22.3 rows were run again after it). The import log:
    `input copper: 1635 track(s) and 410 via(s) back at their own coordinates, 45 routed end(s) moved
    onto them, 3 input item(s) left as routed`.

    | Board | `SYS`–`VBAT` items on `In1.Cu` (PWR) | Actual | All `clearance` errors |
    |---|---|---|---|
    | original | `VBAT` 4.9243 / `SYS` 8.0070; `SYS` 2.8057 / `VBAT` 1.5429 | 0.0910 mm | 12 |
    | K10.5's `kicad_pcb` arm (c8b91b7) | 3 pairs | — | 15 |
    | control (`main`'s import, same session `758f17c9…`) | `SYS` 8.0070 / `VBAT` 4.9243; `SYS` **2.8058** / `VBAT` 4.9243 | **0.0911** mm (554 input items moved) | 13 |
    | restored | `VBAT` 4.9243 / `SYS` **2.8057**; `SYS` 2.8057 / `VBAT` 1.5429 | 0.0910 mm | 13 |

    - Restored, every `SYS` and `VBAT` segment has the original's nanometre geometry and uuid
      (2045 of the routed board's 2839 tracks and vias carry an input uuid, and none of them differs
      in geometry). The count (2) and the distance (0.0910 mm) match the original.
    - The difference is which of the two `SYS` segments KiCad names for `VBAT` 4.9243. The two
      segments meet at (169.84, 63.64705), the closest point, and both are 0.0910 mm from it.
    - **What decides it.** It is not the copper:
      - three DRC runs of each file are identical;
      - the original board, loaded and saved by pcbnew, still names `SYS` 8.0070;
      - the routed board, loaded and saved, still names `SYS` 2.8057;
      - an experiment that took the earlier routed board (the same geometry with the session's
        uuids), set the input uuids in pcbnew and saved it named `SYS` 8.0070, as the original does
        (`target/orchestrator/q223/runs/reorder/oam-uuid/`).

      The choice follows the order of the board's items (scripts `scratch/reorder.py` and
      `scratch/uuids.py`).
  - **KiCad's naming.** Matching it too would need the import to rebuild the original's item
    order. Neither the plugin's `ImportSpecctraSES` path nor KiCad's API offers that. Restoring
    the uuids is the most this item can do in the copper itself, and the amendment above accepts
    it.
  - The 199 routing-caused `drill_out_of_range` and the 3 routing-caused `clearance` in this run's
    `summary.json` are a grader artefact, not routing.
    - One-Air-Max's file (version 20241229) names nets by number in its items, so
      `oracle_lib.read_copper` gives the input copper net codes and the routed board net names, and
      no key matches.
    - With the numbers mapped to names, 1 routing-caused error is left: a routed `VBAT` track on B.Cu
      across footprint F2's copper segment, Q22.4's cause.
    - The grader fix is Q22.13. It lands before K13.6 runs.

- [x] **K13.3 — Fall back to the DSN export.** When the reader refuses the snapshot, the plugin
  exports the DSN instead and routes that. The run log and the result summary name the input
  that ran and why.
  - **Accept:** plugin tests with the stub `pcbnew` (`kicad-plugin/tests/stub_ui.py`) for a
    KiCad 5 board, for an unsupported construct, and for the normal case.

  *Done 2026-10-20:* new `kicad-plugin/freerouting_rs/input_choice.py` (`prepare`, `snapshot_refusal`,
  `load_refusal`, `input_line`), `summary.merge_input`/`input_line`, and `plugin.py` (`export` calls
  `input_choice.prepare`; `on_route` runs the DSN once when `route` returns a load refusal). Two refusals:
  - the snapshot's `(version N)` below 20201115 (the reader's `FIRST_SUPPORTED_VERSION`), checked before routing;
  - the router's error line `There was a parse error while reading board file at '<name>': <reason>` with
    exit 1 and no session. Run by hand against a debug `freerouting-rs` on a KiCad 5 file and on an unclosed
    list: both gave that line in the JSON log and exit 1, and `load_refusal` read the reason from both.

  The run log gets `input: kicad_pcb (the board snapshot, read directly)`, `input: dsn (KiCad's DSN
  export, as chosen)` or `input: dsn (KiCad's DSN export; fallback from kicad_pcb because <reason>)`.
  `<stem>.result.json` gets `input` and `input_fallback_reason` (null when none), and the summary a
  line `Input: ...`. Tests in `kicad-plugin/tests/test_input_choice.py` (stub pcbnew and fake router mode
  `route-refuse-kicad`):
  - `test_a_kicad_5_board_is_exported_as_a_dsn_without_routing_the_snapshot`: KiCad 5 board, one
    SaveBoard and one DSN export, the router gets the `.dsn` and no `--apply-design-rules`, the log names the
    version;
  - `test_a_kicad_5_board_result_names_the_dsn_and_why`: the same through the fake router, result.json
    `input` is `dsn` with the reason, the summary shows it;
  - `test_a_construct_the_router_refuses_exports_the_dsn_and_routes_once_more`: unsupported construct, the
    first run refused, the DSN exported and routed once, no error box, reason `line 7, column 3: ...` in
    result.json, summary and log;
  - `test_the_normal_case_routes_the_snapshot_and_says_so`: normal case, no export, `input` `kicad_pcb`,
    reason null;
  - `test_a_failure_that_is_not_a_load_refusal_is_not_retried`, `test_the_dsn_input_chosen_is_recorded_without_a_reason`,
    `test_fix_existing_locks_are_restored_around_the_fallback`, `test_view_with_a_kicad_5_snapshot_opens_the_dsn`,
    plus unit tests of `snapshot_refusal` (7 versions), `load_refusal`, `merge_input`.

  `python3 -m pytest kicad-plugin/tests`: 421 passed, 19 skipped. Not covered: `headless_run.py` has no
  fallback (its `kicad_pcb` runs stay strict for graded runs), and Route in GUI cannot see a refusal inside
    the GUI process (only the version pre-check applies). A `SaveBoard` failure still ends the run with the
    snapshot error, as before.

- [x] **K13.4 — Load time.** The reader's board for `jetson-agx-thor-baseboard` builds in 3.7 s
  against 1.6 s for the DSN (K10). Profile it, then fix it or record why it stays.
  - **Accept:** before and after load times for `jetson` and `CM5_MINIMA_3` (arcs), release
    build, one run each.

  *Done 2026-10-20:* boards are the KiCad 10 demos with KiCad 10.0.7's exports from `target/kicad-corpus/demos`
  (`<stem>.kicad10.dsn`, the files `kicad_parity.rs::corpus_timing` uses), copied to `target/orchestrator/k134/boards`.
  **Method:** a scratch harness, not committed, that times the calls `corpus_timing` times:
  `read_kicad_input` + `build_board` for the `.kicad_pcb` and `fs::read_to_string` + `Pcb::parse` + `build_board`
  for the DSN. Release build, one run each, before (`825540f`) and after built side by side and run alternately.
  Load and DRC through the CLI (`freerouting-rs -de FILE -drc out.json`, wall time, as K10.5 timed load) is in the
  last column.

  | Board | Input | before: read + build (ms) | after: read + build (ms) | CLI `-drc` before → after (s) |
  |---|---|---|---|---|
  | `jetson-agx-thor-baseboard` | `.kicad_pcb` | 1116 + 2099 = 3215 | 714 + 1623 = 2338 | 6.18 → 5.40 |
  | `jetson-agx-thor-baseboard` | DSN export | 39 + 1316 = 1355 | 39 + 888 = 927 | 2.93 → 2.47 |
  | `CM5_MINIMA_3` | `.kicad_pcb` | 22 + 94 = 115 | 20 + 90 = 110 | 0.42 → 0.40 |
  | `CM5_MINIMA_3` | DSN export | 2 + 45 = 47 | 2 + 39 = 41 | 0.15 → 0.14 |

  The base numbers are lower than K07.6's 3698 / 1636 ms, which were medians on a loaded machine.
  - **Profile** (`valgrind --tool=callgrind` on the harness, `jetson`, instructions, inclusive), before:
    - the reader: `fmt6g::c_fmt_g` 7.2 G, called from `pads::shape_key` for the image keys. It ran two exact
      float conversions per number. `NodeRef::line` took 3.9 G: `line_col` counted newlines from the start of the
      85 MB file for every rule area it reported;
    - the builder, in both arms: `wiring::subnets` took 9.8 G. `Nets::get_all_by_name` scanned all 25,615 nets
      with `equals_ignore_case` for each of the 24,319 wires and vias. `insert_component`'s per-pin
      `get_by_name` took 0.6 G more;
    - the builder, `.kicad_pcb` only: `normalize_all_traces` 12.5 G against 2.4 G for the DSN.
  - **Fixes** (pure speed-ups; nothing the board holds changes):
    - `crates/freerouting-kicad/src/fmt6g.rs`: `push_c_fmt_g` / `push_c_fmt_6g` take the fixed style's digits
      from the one `{:e}` conversion and append in place. `c_fmt_g` wraps them.
      `fmt6g::tests::matches_the_two_conversion_form` compares them with the old two-conversion form on more than
      800,000 values: random bit patterns and coordinates, the rounding boundaries `k·10^e ± 0.5·10^(e-p)` and
      their neighbours, precisions 1 to 8, 17, 30 and 60, and the NaN, infinity and zero cases, also appended
      after other text. `tests/numbers.rs::c_fmt_6g_matches_printf_on_200_values` (the printf vectors) and
      `c_fmt_6g_named_cases` still pass;
    - `crates/freerouting-kicad/src/pads.rs`: `shape_key` appends the numbers with `push_c_fmt_6g`;
    - `crates/freerouting-kicad/src/sexpr/tree.rs`: `Tree` finds the newline offsets once (a `OnceLock`, on
      the first `position`), and `position` binary-searches them.
      `sexpr::tree::position_tests::indexed_positions_equal_line_col_at_every_offset` compares it with `line_col`
      at every offset of a text with multi-byte characters and CRLF;
    - `crates/freerouting/src/io/specctra/parser/wiring.rs`: `NetNames` is a `BTreeMap<CaseInsensitive, Vec<i32>>`
      of net numbers, built once in `read_wiring` and `insert_components`, which add no nets. `subnets` and
      `insert_component` (`network.rs`) look up there. `CaseInsensitive` equality is `equals_ignore_case`, both
      comparing `lower(upper(c))` per character. `wiring::net_names_tests::net_names_find_what_the_scans_find`
      checks that the index finds what both scans find, in the same order, for `GND`/`gnd`, `ß`/`SS`,
      `İ`/`i`/`ı`, `Σ`/`σ`/`ς` and `ǅ`/`ǆ`/`Ǆ`, subnets 0 to 3. Java's reader scans the same way, so this is
      structure only (D1).
  - **Identical output:** before and after, on both boards and both inputs, the harness hashed (FNV-1a 64) the
    reader's `Pcb` (`{:?}`), its warnings and its `ReadReport`, the built board's `write_dsn`, the default search
    tree's `fingerprint` and `content_digest`, the item count and the builder's warnings. All equal (`jetson`
    `.kicad_pcb`: AST `7709b2478a2a594d`, DSN `d8dc191a355f5b64` 21,051,860 bytes, tree `c4ea9ae4b95b6682` /
    `db67d5072bdf2f40`, 358,982 items; DSN export: `4dfe1a7afd94f986`, 359,002 items; `CM5` `.kicad_pcb`
    `b3d04c7ff0e8285e`, DSN `024bdd5e81f91809`). The CLI's DRC reports differ only in `date`.
    `tools/parity-check.sh --no-probes`: connection parity PASS, 7 exact of 7.
  - **Why the rest of the gap stays** (`jetson` 2338 against 927 ms; callgrind after the fixes, 42.5 G against
    60.9 G before):
    - read 714 ms against 39: the reader parses the 85 MB board instead of the 7 MB export. `Tree::parse` takes
      1.8 G, `read_footprints` 1.5 G, and `push_c_fmt_g` 3.1 G: 1.81 million numbers printed in `%.6g` so that
      images group as KiCad's export groups them (`read.rs::image_key`, left alone because it is Q22.4's file);
    - build 1623 ms against 888, the arcs: 2,303 of the 21,082 wires are flattened track arcs, so the wires have
      77,973 path points against the export's 43,233 (one chord per arc, K06.5). `normalize_all_traces` takes
      12.5 G against 2.4 G. `split_trace` makes 58,988 overlap queries against 24,248 and 1,018,574 segment
      intersections against 90,606. `insert_shapes` takes 4.9 G against 2.3 G. That is geometry the DSN does not
      carry, and `CM5_MINIMA_3`'s +50 ms build is the same cause.

- [x] **K13.5 — Register the measurement.** Write `docs/roadmap/prereg/K13.json` and
  `tools/bench/sets/k13-input.txt` before any number:
  - **Boards:** K10.5's ten boards, plus the canary-pwr plugin row.
  - **Arms:** `dsn` and `kicad_pcb`, with the same options, through
    `tools/kicad-reader/plugin-input-ab.sh`.
  - **Few-minute rule:** `vme-wren` and `jetson` run with a pass cap that keeps each route to a
    few minutes, the same cap in both arms.
  - **Grading:** one KiCad grade per distinct session, under the board's own rules.
  - **Guards:** zero routing-caused breaches of the manufacturing rules in both arms.
  - **Keep rule:** K13's switch rule (zero breaches, total unconnected no higher).
  - **Accept:** the registration and its pin test (`crates/freerouting-dev/tests/prereg_k13.rs`)
    are in, with no number yet.

  *Done 2026-10-20:* registered at main `b78845e`, **no number yet**: nothing was routed or graded.
  - **Files.** `docs/roadmap/prereg/K13.json` (id `K13.6-board-input`, sha256
    `a470fa311beffa3f80af1663c251fdf3234445b3d213c34bbc00668ee7408770`),
    `tools/bench/sets/k13-input.txt` (11 rows),
    `crates/freerouting-dev/tests/prereg_k13.rs`, and `tools/kicad-reader/plugin-input-ab.sh`.
    The registration pins the script's sha256, `17af015a…`.
  - **Boards.** K10.5's ten boards and N06.13's canary-pwr plugin row (canary `2a7e070b`), copied
    to `target/orchestrator/k135/boards/`. The demos are `cp -r` of WSL KiCad 10.0.7's
    `/usr/share/kicad/demos` folders. The canary rows are `git archive` of
    `hardware/canary-pwr-rev1.0` at `1a7df77` (K10.5's) and `2a7e070b`. All 11 are KiCad 6+
    (version 20241229 or later) and have their own `.kicad_pro`. Each `.kicad_pcb` and its
    `.kicad_pro`/`.kicad_dru` are hashed in `boards.files` and `boards.rules_files`.
  - **Arms.** `dsn` against `kicad_pcb`, both with the options `{"schema": 5, "profile":
    "enhanced"}` (`profile_defaults('enhanced')`). The plugin reads the Board Setup minimums from
    the board on both paths. The two commands differ only in the arm's name.
  - **Pass cap** (the few-minute rule): the plugin's `max_passes: 1` plus
    `--router.fanout.max_passes=1`, the same in both arms. It applies to `vme-wren` and
    `jetson-agx-thor-baseboard`, and also to `kit-dev-coldfire-xilinx_5213` and `video`, whose
    K10.5 routes took 468 s and 1040 s.
    - Source: K10.5's route logs (`target/orchestrator/k105/out/<arm>/<board>/route.log`). Video's
      fanout passes took 19–37 s each and its routing passes 30–47 s each; kit-dev's fanout pass 2
      alone took 298 s. That gives about 130 s for video and 25 s for kit-dev.
    - `vme-wren` and `jetson` have no per-pass record (K10.5: "over 3 min"), so their 600 s
      timeout bounds them.
  - **Limits.** `ulimit -v 8000000` and `timeout -k 10` on every router call: 300 s, 600 s for
    the capped rows, 900 s for canary-pwr. An aborted route is recorded and not rerun.
  - **Grading.** One grade per distinct session (`SHARE_GRADE_WITH`), under the project's own
    rules. Routing-caused items are counted on grade.sh's copper basis (Q22.2).
  - **Guards and keep rule.** The guards (zero routing-caused manufacturing breaches, below-minimum
    copper and vias in pads, in both arms) and K13's switch rule are copied into the
    registration. The gate runs with `"rule": "totals"`, noise 0 on every row and the pair
    `[kicad_pcb, dsn]` reported. Because the gate's per-board clause is stricter, K13.6 applies the
    switch rule by hand (`keep_rule_by_hand`).
  - **Test and gate.** `cargo test -p freerouting-dev --test prereg_k13`: 6 passed. The board
    hashes were checked against the copies, and a changed hash fails the test. `cargo fmt --all
    --check` and `cargo clippy -p freerouting-dev --tests -- -D warnings` are clean.

- [ ] **K13.6 — Measure.** Run K13.5 as registered, after K13.1, K13.2 and K13.4 are on main
  (decided 2026-10-20: both arms re-lay unlocked input copper, so until K13.2 a violation already
  on the board counts as routing-caused in both). It starts with one smoke run of
  `plugin-input-ab.sh` on a board outside the set. A script fix that smoke run needs is a
  registration amendment before any registered row runs. The first amendment, which names the
  commit and the binary, also records that K13.1 changed what the options mean. The registration
  says `apply_kicad_rules` off, but since K13.1 every snapshot route applies the `.kicad_dru`,
  and the reader adds the pads' own clearances, as the DSN arm already did (decided 2026-10-20).
  - **Accept:**
    - the per-board table (unconnected, errors by type, prepare + load time, route time) is in
      the done note;
    - every board where `kicad_pcb` grades worse is explained;
    - the verdict under K13's switch rule is recorded.

- [ ] **K13.7 — Switch the default.** Only if K13.6 passes the switch rule, or the owner
  accepts the listed exceptions:
  - `board_input` defaults to `kicad_pcb`, with an options schema bump. The same bump drops
    `apply_kicad_rules`, which does nothing since K13.1 (saved files that have it still load);
  - saved `dsn` choices are kept;
  - the dialog stops calling it experimental;
  - the dialog accepts `kicad_pcb` with the default options. Today it refuses the pair because
    `edge_and_pad_keepouts` is on by default (K10.4), while `headless_run.py snapshot` accepts it
    and the option does nothing there (found in K13.5). The option is ignored, and the dialog
    says so, on `kicad_pcb`;
  - the plugin docs and README say which input runs and how to pick the other;
  - a DIVERGENCES line notes that Java's plugin exports a DSN.
  - **Accept:**
    - plugin tests for the default, the kept saved choice and the schema migration;
    - `python3 -m pytest kicad-plugin/tests` passes;
    - the owner's confirmation from a run inside KiCad is recorded.
