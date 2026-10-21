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
| K13.8 | plugin tests and fixtures under `kicad-plugin/tests/` | `kicad-plugin/freerouting_rs/{restore_copper,export_flow,dsn_fixups}.py` and a new fix-up module, `kicad-plugin/headless_run.py`, `tools/kicad-oracle/kicad_steps.py`, `docs/DIVERGENCES.md`, `docs/development/kicad-plugin.md` |

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

- [x] **K13.6 — Measure.** Run K13.5 as registered, after K13.1, K13.2 and K13.4 are on main
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

  *Amendment 2026-10-21:* `docs/roadmap/prereg/K13.json` amendment 1 (before any registered row) names commit `daa32c1`, the binary, the script and grader hashes, K13.1's option meaning, the smoke run and the gate's `--all-pairs --baseline` form.
  Amendment 2 (after the rows, nothing rerun) records that the gate writes no baseline for a run with aborted rows, and the judged-rows gate run used for its checks.

  *Done 2026-10-21:* K13.json run as registered on main `daa32c1` (`freerouting-rs 0.1.0 (git daa32c1e6d47)`, sha256 `fd575683…`), WSL KiCad 10.0.7, `plugin-input-ab.sh` `17af015a…` from the detached tree `target/orchestrator/k136/tree`, driver `target/orchestrator/k136/run.sh`, rows in set order, dsn then kicad_pcb, one run each, every router call under `ulimit -v 8000000` and `timeout -k 10` of the row's limit. Smoke run first on pic_programmer (outside the set): both arms exit 0, graded, no script fix. Rows ran 02:47:30–04:21:49 (1 h 34 min). Outputs: `target/orchestrator/k136/out/<arm>/<row>/summary.json`, `records.json`, `table.json`, `items.json` (every routing-caused item), `gate-*.txt`, ledger `progress.md`.

  Per row and arm (unconnected = kicad-cli's on the routed board, input's in brackets; errors = every DRC error of the routed board, pre-existing included; caused = routing-caused by type; mfg / below / vip = manufacturing_breaches, below_minimum, via_in_pad; p+l = prepare_s + load_s; every grade `rules: project`, basis copper, graded fresh: no two sessions were byte-identical, so no grade was shared):

  | Row | Arm | Unconnected | Errors by type | Caused by type | mfg | below | vip | p+l s | route s | aborted | session |
  |---|---|---|---|---|---|---|---|---|---|---|---|
  | ecc83-pp | dsn | 0 (0) | – | – | 0 | 0 | 0 | 0.32 | 0.2 | 0 | `432e2d38` |
  | ecc83-pp | kicad_pcb | 0 (0) | – | – | 0 | 0 | 0 | 0.33 | 0.2 | 0 | `ca95a2f1` |
  | interf_u | dsn | 0 (0) | starved_thermal 2 | – | 0 | 0 | 0 | 0.58 | 1.1 | 0 | `2643010f` |
  | interf_u | kicad_pcb | 0 (0) | starved_thermal 2 | – | 0 | 0 | 0 | 0.56 | 1.1 | 0 | `52994e66` |
  | complex_hierarchy | dsn | 0 (0) | – | – | 0 | 0 | 0 | 0.79 | 0.3 | 0 | `e6e4999d` |
  | complex_hierarchy | kicad_pcb | 0 (0) | – | – | 0 | 0 | 0 | 0.73 | 0.2 | 0 | `4c3b1505` |
  | kit-dev-coldfire-xilinx_5213 | dsn | 0 (0) | – | – | 0 | 0 | 0 | 1.17 | 51.6 | 0 | `b63314ce` |
  | kit-dev-coldfire-xilinx_5213 | kicad_pcb | 0 (0) | – | – | 0 | 0 | 0 | 0.91 | 56.0 | 0 | `0c0bef58` |
  | CM5_MINIMA_3 | dsn | 0 (0) | clearance 172, copper_edge_clearance 2, courtyards_overlap 2, holes_co_located 2 | clearance 172, silk_edge_clearance 3, silk_overlap 18 | **172** | 0 | 0 | 1.59 | 265.9 | 0 | `807c3dd2` |
  | CM5_MINIMA_3 | kicad_pcb | 0 (0) | copper_edge_clearance 2, courtyards_overlap 2, holes_co_located 2 | silk_edge_clearance 3, silk_overlap 18 | 0 | 0 | 0 | 1.60 | 180.9 | 0 | `976b9df2` |
  | canary-pwr-1a7df77 | dsn | 48 (153) | – | – | 0 | 0 | 0 | 2.97 | 672.2 | 0 | `abac11b2` |
  | canary-pwr-1a7df77 | kicad_pcb | 45 (153) | – | – | 0 | 0 | 0 | 0.85 | 629.4 | 0 | `47597cde` |
  | One-Air-Max | dsn | 2 (0) | clearance 12, drill_out_of_range 199, padstack_invalid 4, solder_mask_bridge 10, zones_intersect 1 | silk_edge_clearance 2, silk_over_copper 1, silk_overlap 4 | 0 | 0 | 0 | 1.28 | 213.3 | 0 | `8f061a88` |
  | One-Air-Max | kicad_pcb | 2 (0) | the same as dsn | the same as dsn | 0 | 0 | 0 | 1.31 | 203.8 | 0 | `a9a466eb` |
  | video | dsn | 0 (0) | – | – | 0 | 0 | 0 | 3.39 | 139.5 | 0 | `02748ddc` |
  | video | kicad_pcb | 0 (0) | – | – | 0 | 0 | 0 | 3.05 | 140.0 | 0 | `8a420e46` |
  | vme-wren | dsn | not graded | | | | | | 43.22 | 603.3 | **1** | `5265aee6` |
  | vme-wren | kicad_pcb | not graded | | | | | | 40.18 | 608.5 | **1** | none |
  | jetson-agx-thor-baseboard | dsn | not graded | | | | | | 60.45 | 608.9 | **1** | none |
  | jetson-agx-thor-baseboard | kicad_pcb | not graded | | | | | | 18.85 | 608.5 | **1** | none |
  | canary-pwr-plugin-2a7e070b | dsn | 23 (133) | – | track_dangling 3 | 0 | 0 | 0 | 2.95 | 145.3 | 0 | `fd6b64a3` |
  | canary-pwr-plugin-2a7e070b | kicad_pcb | 21 (133) | – | track_dangling 3 | 0 | 0 | 0 | 0.85 | 187.3 | 0 | `f6ba8b6f` |

  The four aborted arms hit the 600 s timeout in the route call (timeout exit 124; the load calls exited 0), all four still in the fanout stage except jetson kicad_pcb (fanout done at 87.2 %, routing pass 1 took 169 s, then the sweep). vme-wren dsn's router caught the signal, kept its best board and wrote a session (exit 0 inside `timeout`, so still aborted); the other three logged `CANCELLED` and wrote none. Not graded, not rerun. Their prepare times are the plugin's export or snapshot in pcbnew (vme-wren 41 s / 36 s, jetson 58 s / 13 s); the router's load is 2–6 s.

  Totals of kicad_unconnected over the nine judged rows: dsn 73, kicad_pcb 68. Routing-caused items also on the input board at the same place and type (named, not subtracted), identical in both arms: CM5_MINIMA_3, all 21 of its routing-caused items: 18 silk_overlap between the B.Silkscreen arcs and segments of Module301 and Module302 (at (127.25, 77.6), (94.25, 77.6), (94.25, 29.6), (127.25, 29.6), (90.75, 29.6)) and 3 silk_edge_clearance (J1 F.Silkscreen, Module301 and Module302 against Edge.Cuts, at (89.28, 28.44) and (126.42, 81.07)); One-Air-Max, all 7: silk_overlap C11/PM1 (147.09, 65.85), C12/CO2 (147.34, 59.87), C45/C48's reference (183.56, 85.23), C43/C42's reference (189.19, 85.08), silk_over_copper U8's arc on D7 pad 1 (139.66, 82.00), silk_edge_clearance BAT1 and LED2 against Edge.Cuts (128.18, 120.4), (141.91, 114.64). They count as routing-caused only because `oracle_lib.routed_item` takes any item described `Arc …` that is not routed copper for a routed arc, and footprint silkscreen arcs are described `Arc of <ref> on <layer>`; none is a manufacturing type. canary-pwr-plugin's 3 track_dangling are one +12V_PROT F.Cu stub of 0.15 mm at (141.7713, 74.8329), the same in both arms and not on the input.

  Rows where kicad_pcb grades worse: none. On every judged row kicad_pcb has at most dsn's unconnected (lower on canary-pwr-1a7df77, 48 → 45, and canary-pwr-plugin-2a7e070b, 23 → 21) and at most dsn's count of every routing-caused type (lower on CM5_MINIMA_3: clearance 172 → 0). One-Air-Max leaves 2 unconnected in both arms on an input with 0 (both: C62 pad 2 to the /VOUT4 zone on B.Cu; dsn also two /VOUT5 zone islands at (130.56, 99.07), kicad_pcb that zone to U17 pad 3); the count is the same, so the row is not worse. The gate's `WORSE than 2 x its band` lines name only the reported pair (dsn worse on the two canary rows), none in the verdict pair.

  dsn breach, a defect of the current default: CM5_MINIMA_3, 172 routing-caused `clearance` items, all between the two tracks of one of five differential pairs (`/CM5/ETH_PI.TRD0` 49, `TRD1` 41, `TRD2` 16, `/CM5/PCIE_PI.RX` 37, `TX` 29; netclass `100ohm` 0.2 mm, actual 0.1381–0.1988 mm, 106 items; `90ohm` 0.154 mm, actual 0.0678–0.1421 mm, 66 items; e.g. `/CM5/PCIE_PI.TX_P`/`TX_N` on F.Cu at (116.10, 41.01)), every one within 0.5 mm of one of the input's 200 track arcs. KiCad's DSN export lays each arc as one chord; the import brings the chords back, and a chord cuts inside its partner's clearance. The snapshot path reads the arcs as polylines (K13.1's table) and has 0.

  Gate: the registered `--write-baseline` prints 4 UNGRADED (the aborted arms), 0 REFUSED, writes no baseline, exit 1; the `--all-pairs` step (with `--baseline`, amendment 1) exits 1 on the missing baseline (amendment 2). The judged-rows check (`--all-pairs --rows` of the nine, baseline written from those rows): `0 refusal(s)` in both pairs; verdict pair: no HARD line, no WORSE line, totals 73 → 68, `totals verdict: accept` (its DRIFT/INVERTED lines are artefacts of that baseline's default metric table); reported pair: `HARD CM5_MINIMA_3.dsn.manufacturing_breaches = 172`.

  Verdict under keep_rule_by_hand: (1) 9 of 11 rows judged; vme-wren and jetson-agx-thor-baseboard are not (both arms aborted at the 600 s timeout); (2) holds: kicad_pcb has manufacturing_breaches, below_minimum and via_in_pad 0 on all nine; (3) holds: 68 ≤ 73; (4) no row to explain. (5) **keep dsn**, clause (1) failed on vme-wren and jetson-agx-thor-baseboard: "not met on the full set", which K13.7 takes only with the owner's acceptance of those two rows.

- [~] **K13.7 — Switch the default.** Only if K13.6 passes the switch rule, or the owner
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
  - *Waits 2026-10-21:* K13.6's verdict is "keep dsn" (clause (1): vme-wren and
    jetson-agx-thor-baseboard aborted at 600 s in both arms and are not judged). On the nine
    judged rows `kicad_pcb` has 0 breaches and 68 unconnected against `dsn`'s 73. K13.7 needs the
    owner to accept those two rows, or a new registration that judges them, and then the
    owner's run inside KiCad.
  - *Partial 2026-10-21:* the sub-bullet "the dialog accepts `kicad_pcb` with the default options" is
    done; the default, the schema bump, `apply_kicad_rules`, the "experimental" label and the DIVERGENCES
    line still wait on the owner. `options.input_conflicts` no longer lists `edge_and_pad_keepouts`;
    new `options.input_ignored(opts)` (the dialog labels of the options on but ignored on `kicad_pcb`:
    that one) and `input_ignored_note(opts)` (the line with the reason). The reader covers what the DSN
    fix-up wrote: the outline's copper-to-edge clearance (`min_copper_edge_clearance` of the `.kicad_pro` ->
    `router.copper_to_edge_clearance_um`, `read.rs` / `job::prepare::board_minimums_patch`, K13.1's table
    row for Q18.5), Edge.Cuts and Margin shapes off the outline (`edge_keepouts::edge_rects`,
    `edge_keepouts::edge_keepouts`) and the pads' own clearances (`pad_clearance::read_pad_clearances`,
    `pad_clearance::keepouts`; `k13_inputs.rs::pads_own_clearances_become_circle_keepouts`). The dialog shows
    the note in `Dialog.input_note` (refreshed on the choice, the box and `show_options`), `snapshot.notes`
    puts it in `plugin.log` (dropped when the run falls back to the DSN, where the box applies), and
    `headless_run.py route` prints `Note: ...`; none blocks. Tests (RED first: 14 failed before the code,
    which included every snapshot dialog test once their `keepouts.SetValue(False)` workaround was removed):
    `test_snapshot.py::test_the_defaults_are_accepted_on_the_snapshot_and_the_keepouts_are_ignored` (renamed
    from `test_the_defaults_refuse_the_snapshot_until_the_keepouts_are_off`),
    `::test_the_other_four_dsn_rewrites_still_refuse_the_default_snapshot_by_name`,
    `::test_the_snapshot_run_log_carries_the_ignored_note_once`,
    `::test_the_form_refuses_a_snapshot_with_a_dsn_option_on_and_names_it` (defaults accepted, `class_widths`
    refused), `::test_headless_route_takes_the_default_options_on_a_snapshot_and_prints_the_note`,
    `::test_headless_route_still_refuses_a_dsn_option_on_a_snapshot`,
    `::test_headless_route_on_the_dsn_prints_no_note`,
    `test_input_choice.py::test_the_defaults_route_the_snapshot_and_the_dialog_says_the_keepouts_are_ignored`,
    `::test_the_note_follows_the_input_and_the_keepouts_box`, and
    `test_route_dialog.py::test_a_snapshot_with_a_dsn_rewrite_on_is_refused_before_anything_is_saved`
    (`class_widths` refused, the keepouts not named). `python3 -m pytest kicad-plugin/tests` (router binary
    of 2f961b5 in `FREEROUTING_RS_BIN`): 531 passed, 6 skipped, 0 failed. Real wx (wxPython 4.2.1, GTK3,
    `xvfb-run`, stub `pcbnew`, the package loaded without its `__init__`): the note is 18 px high and empty on
    the DSN input, and 54 px high with its text after switching to the snapshot. `show_input_note` lays the
    Board page out again (`Layout`, `FitInside`); without that the text grew to 486 x 54 px while its grid row
    stayed 18 px, over the rows below. The note's look inside KiCad is part of the owner's run.
  - *Partial 2026-10-21:* decided 2026-10-21: the default switches to `kicad_pcb`; vme-wren and
    jetson-agx-thor-baseboard accepted unjudged. Done (plugin Python and docs only; no router change):
    - **Default.** `options.PROFILE_DEFAULTS["enhanced"]` gains `board_input = kicad_pcb`, with the comment naming K13.6,
      `docs/roadmap/prereg/K13.json`, the nine judged rows (0 manufacturing breaches, 68 unconnected against `dsn`'s 73)
      and the acceptance of the two unjudged rows. `legacy` keeps `dsn` (the dataclass default; Java's plugin reads a
      DSN, D11). The profile choice in the dialog moves no other field.
    - **Schema 6.** `SCHEMA = 6`, `READABLE_SCHEMAS = (1..6)`; `apply_kicad_rules` is dropped from `RouteOptions`,
      `_restore_k09`, `_form_k10`, the dialog (`saved_kicad_rules`, `form_values`, `show_options`) and the presets
      (a preset that still names it applies quietly, `presets._RETIRED`). The greyed ".kicad_dru" check box is removed,
      not kept (it read and wrote nothing; its text moved into the board input's tooltip). The module docstring's
      `"schema": 2` is fixed. A file that has the field loads and is saved again as schema 6 without it.
    - **Saved choice.** `to_dict` leaves defaults out, so a file without `board_input` takes the profile's input; a file
      without it that turns on a DSN-only fix-up (`input_conflicts`: `class_widths`, `fixed_pour_classes` as wires or `auto`,
      `drop_pour_nets`, `route_on_plane_layers`) takes `dsn` (`RouteOptions.from_dict`). `presets.apply` does the same for a
      preset that names no input and adds a warning line. `headless_run.py`: `export` always writes the DSN and `snapshot`
      always takes the snapshot, whatever `board_input` says; `route` and `import` follow it. `tools/kicad-oracle/grade.sh
      --export plugin` writes an options copy with `board_input: dsn` and uses it for `export` and `import`;
      `tools/kicad-reader/plugin-input-ab.sh` already sets `board_input` per arm (confirmed, unchanged);
      `kicad_steps.py` names `restore.DSN` itself (unchanged).
    - **Wording.** `BOARD_INPUT_LABELS` ("Board snapshot"), the tooltip and `plugin.py`'s and `snapshot.py`'s docstrings no
      longer say "experimental" (stage labels unchanged); `BOARD_INPUTS` keeps the order (dsn, kicad_pcb).
    - **Docs.** `docs/development/kicad-plugin.md` (which input runs, how to pick the other, `legacy`, the saved-options rules,
      schema 6), `kicad-plugin/README.md`, `README.md`, `tools/kicad-oracle/README.md`; `docs/DIVERGENCES.md`, kicad-plugin
      section: one new line (the plugin reads the open board by default, Java's exports a DSN) and the `apply_kicad_rules` line
      updated. `docs/roadmap/prereg/K13.json` (schema 5) untouched; `crates/freerouting-dev/tests/prereg_k13.rs` does not read the plugin.
    - **Tests** (new `kicad-plugin/tests/test_board_input_default.py`, 50 tests; RED first: 20 of the file's tests failed before the
      code): the default (`test_the_enhanced_profile_reads_the_board_and_legacy_the_dsn`, `test_no_saved_file_gives_the_snapshot`), the
      kept choice (`test_enhanced_with_the_dsn_writes_the_choice_and_reads_it_back`, `test_enhanced_with_the_snapshot_writes_nothing`,
      `test_a_schema_6_file_saying_dsn_loads_dsn`), the migration (`test_a_schema_5_file_without_a_choice_takes_the_profiles_default`,
      `test_a_schema_5_file_with_apply_kicad_rules_loads_and_saves_without_it`, `test_a_saved_dsn_fixup_without_a_choice_keeps_the_dsn`
      over six fix-ups and schemas 3 to 6, `test_schema_6_is_written_and_every_older_schema_still_reads`), presets (six fix-ups move
      to `dsn` with a warning; a preset naming `kicad_pcb` keeps it; the canary preset on the new default gives `dsn`;
      `test_the_canary_preset_file_reproduces_dsn_export_on_the_fixture` unchanged and green). In `test_input_choice.py`:
      `test_a_board_with_no_saved_file_opens_on_the_snapshot_with_only_the_keepouts_note`, `test_a_route_with_the_defaults_runs_the_snapshot`,
      `test_a_saved_dsn_choice_keeps_the_dsn`, `test_the_legacy_profile_opens_on_the_dsn`, `test_the_profile_choice_does_not_move_the_input`,
      `test_a_schema_5_board_with_class_widths_and_no_choice_still_routes_on_the_dsn`; the DSN fallback tests now run on the default and
      still say so. `test_snapshot.py`: the headless `route`, `export` and `snapshot` modes. Existing tests that meant the DSN now say
      `board_input = dsn` (`test_route_dialog.py`, `test_gui_mode.py`, `test_restore_copper.py`, `test_scripted_flow.py`,
      `test_kicad_dru_args.py`, `test_board_rules_both_inputs.py`, `test_options.py`); none was loosened. `python3 -m pytest
      kicad-plugin/tests` (router binary of main, `FREEROUTING_RS_BIN`): 574 passed, 22 skipped, 0 failed (22 skipped before the
      change; the skips need a Python with pcbnew or a jar). `python3 -m unittest discover -s tests` in `tools/kicad-oracle`: 44 OK.
    - **Open.** `load` returns the dataclass defaults, not `profile_defaults("enhanced")`, for a board with no
      `route-options.json`; so a fresh board starts with the completion sweep off while a saved enhanced file gets it (K09.9). Not
      changed here; the operator decides.
    - **Remains: the owner's confirmation from a run inside KiCad is recorded.** The owner should check: the Board page's input choice
      opens on "Board snapshot" and its note (the keepouts line, no other text); a Route on the snapshot imports the result; Undo after a
      Route that brought track arcs back (K13.8, untested inside KiCad).

- [x] **K13.8 — The DSN path returns input track arcs as arcs** (added 2026-10-21, from K13.6;
  decided 2026-10-20: routing never breaks the board's manufacturing rules, and `dsn` stays the
  fallback whatever K13.7 decides). On CM5_MINIMA_3 the `dsn` arm has 172 routing-caused
  `clearance` items, all between the two tracks of five differential pairs (netclass `100ohm`
  0.2 mm: actual 0.1381–0.1988 mm; `90ohm` 0.154 mm: 0.0678–0.1421 mm), each within 0.5 mm of
  one of the input's 200 track arcs. K13.6's reading: KiCad's `ExportSpecctraDSN` lays each
  unlocked arc as one chord, the router keeps it, `ImportSpecctraSES` replaces the arc with the
  chord, and `restore_copper` skips arcs ("the session has none"). The `kicad_pcb` arm reads arcs
  as polylines and has 0.
  - First confirm the cause on `target/orchestrator/k136/out/dsn/CM5_MINIMA_3` (the DSN's wires
    against the board's arcs, and the routed board's tracks at the breach points).
  - Fix so that input copper the router did not move comes back as it was, arcs included, and
    routed copper is checked against the arcs' true shape, not their chords. For example, write
    each unlocked arc into the DSN as a polyline at the reader's tolerance (as the `kicad_pcb`
    path reads it), and let `restore_copper` put an original arc back when the imported segments
    match it within its bound. Both the plugin's import and the oracle's `kicad_steps.py import`
    use `restore_copper`.
  - **Accept:**
    - a plugin test on a fixture with a pair of arcs at exactly the class clearance: after the
      export, the fix-ups and the import, the arcs come back as arcs at their own coordinates and
      KiCad's DRC (where pcbnew is available) has no clearance item between them;
    - CM5_MINIMA_3, one `tools/kicad-reader/plugin-input-ab.sh dsn` run with K13.6's options
      and limit (`ROUTE_TIMEOUT_S=300`), graded under its own rules: 0 routing-caused
      `clearance` and every manufacturing type 0, unconnected beside K13.6's 0;
    - the same run in the `kicad_pcb` arm stays at 0 routing-caused manufacturing items;
    - a DIVERGENCES line (kicad-plugin), and `python3 -m pytest kicad-plugin/tests` passes;
      `legacy` keeps connection-parity 7 of 7 if engine code changes.

  *Done 2026-10-21:* the fix is in and clauses 1, 3 and 4 hold. Clause 2: the dsn run at the
  300 s limit was cut by `timeout` (exit 124, route 300.9 s), and the script marks such a run
  aborted and ungraded. Its session (`3cd2afa5…`) is byte-identical to the 600 s run's, which
  finished in 438.7 s and was graded: 0 routing-caused `clearance`, every manufacturing type 0,
  unconnected 0. Decided 2026-10-21: clause 2 is met on that session, because the 300 s limit
  produced the same copper. The route-time cost of the polylines is K13.9.
  - **Cause, confirmed** on `target/orchestrator/k136/out/dsn/CM5_MINIMA_3`. KiCad 10.0.7's
    `ExportSpecctraDSN` writes each of the 200 unlocked track arcs as one chord, start to end. 192
    are two-point wires and 8 are one leg of a 3-point path that merges two arcs, for example
    `(path B.Cu 140 119505 -29387 119288 -29604.7 119070 -29822.5 ...)`. The chords have sagitta
    0.0205–0.1508 mm. The session returns them as `(type protect)` wires, and the routed board
    has 0 arcs: 196 arcs are now a chord segment. `restore_copper._kind` gave `None` for
    `PCB_ARC`, so no arc was put back. Of the 172 `clearance` items, 123 are between two chord
    segments and the other 49 between a chord and another track.
  - **Fix.** `kicad-plugin/freerouting_rs/track_arcs.py` is new: `export_flow.fix_track_arcs` is
    always on, after the thermal-spoke step. It replaces the chord of each unlocked arc in the
    DSN's wiring with the reader's polyline (`tracks.rs::arc_points`, ported). The ends stay as
    KiCad wrote them; no chord lies over 1 µm from the arc or spans over 0.1 rad. The port matches
    the Rust reader exactly on 2200 arcs: CM5's 200 plus 2000 random ones, 289027 points, checked
    by a scratch probe binary. `restore_copper` now records unlocked arcs. After tracks and vias it
    puts back an arc when a chain of unmatched imported tracks lays it unmoved:
    - same net and layer, width within 0.1 µm;
    - ends within the input's bound;
    - every point and every chord midpoint within the bound plus 1 µm of the arc, each point
      further along it.

    The arc comes back with its own start, mid, end, width and uuid, through
    `restore_copper.PcbnewEditor(board, pcbnew)`, which takes pcbnew as an argument and creates
    the `PCB_ARC`. A bare chord or a moved polyline stays as imported. Locked arcs stay chords.
    `export_flow.after_import` and `tools/kicad-oracle/kicad_steps.py import` pass the editor.
    The kicad_pcb path gets its arcs back the same way.
  - **Tests**, written first. At HEAD `test_arc_round_trip.py` failed, and the same steps by hand
    gave 2 `clearance` items (actual 0.0828 mm) and 0 arcs. Now:
    - `kicad-plugin/tests/test_arc_round_trip.py::test_two_arcs_at_the_class_clearance_come_back_as_arcs_with_no_clearance_item`
      uses real pcbnew and kicad-cli on the new fixture `tests/data/arc_pair.kicad_pcb`: arcs of
      radius 1.0 and 1.4 mm, 0.2 mm wide, gap 0.2 mm (the Default class clearance). It runs the
      `headless_run.py` export with the fix-ups, a session of the unmoved wiring, and the
      `headless_run.py` import. Both arcs come back at their own start, mid and end, every track
      equals the input with uuids, and DRC shows no `clearance` item;
    - `test_track_arcs.py`, 7 tests, including `test_arc_points_are_the_readers_points` and
      `test_a_chord_in_a_merged_path_run_end_to_start_gets_its_points_in_path_order`;
    - `test_restore_copper.py`, 6 new tests, including
      `test_an_arc_whose_polyline_comes_back_unmoved_is_put_back_as_the_arc`,
      `test_a_reversed_polyline_split_at_a_junction_is_still_the_arc` and
      `test_a_chord_or_a_moved_polyline_stays_as_imported`.
  - **Runs.** Binary `target/orchestrator/k138/bin/freerouting-rs-k138` (`freerouting-rs 0.1.0
    (git 21972677a983)`, sha256 `e545fc6c…`, no Rust change). Script
    `tools/kicad-reader/plugin-input-ab.sh` run from the worktree, options
    `{"schema": 5, "profile": "enhanced"}`, `ROUTE_MEMORY_KB=8000000`, one kicad-cli at a time,
    board copy `target/orchestrator/k138/boards/cm5_minima`, outputs under
    `target/orchestrator/k138/out/`. Every grade is `rules: project`.

    | Arm, limit | rc | aborted | route s | p+l s | unconnected (input) | caused by type | mfg / below / vip | arcs back | session |
    |---|---|---|---|---|---|---|---|---|---|
    | dsn, 300 s | 124 | **1** | 300.9 | 1.98 | not graded | | | | `3cd2afa5` |
    | dsn, 600 s | 0 | 0 | 438.7 | 1.96 | 0 (0) | silk_edge_clearance 3, silk_overlap 18 | 0 / 0 / 0 | 200 | `3cd2afa5` |
    | kicad_pcb, 300 s | 0 | 0 | 180.5 | 1.65 | 0 (0) | silk_edge_clearance 3, silk_overlap 18 | 0 / 0 / 0 | 200 | `976b9df2` |

    - The DRC errors of the dsn 600 s board are those of K13.6's kicad_pcb arm:
      copper_edge_clearance 2, courtyards_overlap 2, holes_co_located 2. Routing-caused
      `clearance` fell from 172 to 0. The 21 silk items are on the input board (K13.6).
    - All 200 arcs on the routed dsn board equal the input's: start, mid, end, width, layer, net
      and uuid.
    - Import line: `1835 track(s) and 444 via(s) back at their own coordinates, 6 routed end(s)
      moved onto them, 49 input item(s) left as routed (Q22.3), 200 track arc(s) back as arcs
      (K13.8)`.
    - The kicad_pcb session is byte-identical to K13.6's (`976b9df2`); only the arcs changed on
      import.
  - **Route time.** The dsn arm is slower than K13.6's 265.9 s. Each pass takes about 20.5 s
    instead of 12.1 s. The engine stops after 18 passes without improvement (20 unrouted in the
    router's count, 925.11, 993 violations against 1148 with chords) and then runs 2 sweeps.
    Running the same binary on the two DSNs for 80 s gave 12.0 s per pass on K13.6's chord DSN and
    20.4–20.9 s on the polyline DSN. That DSN is byte-identical to `track_arcs.apply` run on
    K13.6's DSN. A coarser polyline (0.5 rad, 10 µm, 600 points instead of 3000) took 13.6 s per
    pass, so the pass time grows with the number of points. K13.6's kicad_pcb arm reads the same
    polylines at 8.4 s per pass with 14 unrouted. Not profiled: `perf` is missing and ptrace is
    restricted.
  - **Gate.** `cargo fmt --all --check` passes; there is no Rust change, so clippy and the parity
    check were not needed. `FREEROUTING_RS_BIN=target/orchestrator/k138/target/release
    target/orchestrator/k09/venv/bin/pytest kicad-plugin/tests`: 525 passed, 6 skipped (pcbnew
    not importable in the venv: 3; canary-pwr board: 1; `KICAD_METADATA_REPO`: 2).
    `python3 -I -m unittest discover -s tools/kicad-oracle/tests`: 44 tests OK.

- [x] **K13.9 — Polyline arcs don't slow the DSN path's passes** (added 2026-10-21, from K13.8).
  With K13.8's polylines, CM5_MINIMA_3's dsn route takes 438.7 s against K13.6's 265.9 s with
  chords. Each pass takes about 20.5 s instead of 12.1 s, and the engine still stops after 18
  passes without improvement and then runs 2 sweeps. Probes of the same binary for 80 s each gave
  12.0 s per pass on K13.6's chord DSN, 20.4–20.9 s on the polyline DSN, and 13.6 s on a coarser
  polyline (0.5 rad, 10 µm: 600 points instead of 3000). The `.kicad_pcb` reader lays the same
  polylines, and K13.6's kicad_pcb arm runs 8.4 s per pass. So the cost is in how the router
  handles the DSN's 3000-point input wires, not in the points themselves.
  - Find where the pass time goes. `perf` is not installed and ptrace is restricted, so use the
    engine's own timers or a counting build. Compare the two inputs' boards after load: how the
    input wires become items (one polyline trace or many, fixed state, tree entries), and what
    each pass does with them (the optimizer, rip-up and the search tree).
  - **Accept:**
    - the cause named, with the numbers that show it;
    - CM5_MINIMA_3, one `tools/kicad-reader/plugin-input-ab.sh dsn` run with K13.6's options and
      `ROUTE_TIMEOUT_S=300`: exit 0 inside the limit, graded under its own rules, every
      manufacturing type 0 and the 200 arcs back as arcs, unconnected beside 0;
    - a test that pins the fix. If engine code changes, `legacy` keeps connection-parity 7 of 7
      and the gate's tests pass. An `enhanced`-only change gets its DIVERGENCES line.
  - *Done 2026-10-21:* the passes now take 12.8 s on the dsn path, against 20.5 s. The route
    takes 278.0 s, inside the 300 s limit, and gives the same session as K13.8, byte for byte.
  - **Cause.** The polylines cost time per room, not as board items. Pass 2 of the main
    `91b2f38` binary was compared on the polyline DSN and on K13.6's chord DSN, with counting
    and timing builds; `-mp 3`, scratch files under `target/orchestrator/k139/probe/`.
    - The 3000-point arcs add trace segments as holes to every drill page near them. The page
      split (`PolylineArea::split_to_convex_counted`) took 2.47 s against 0.34 s, for the same
      3060 pages. Drills went from 60964 to 84648.
    - `check_drill_layer` took 5.55 s against 2.36 s, with 804k calls against 578k. Within
      it, `drill_breaks_hole_to_hole` took 2.86 s for 295k calls, of which 68k were distinct.
    - The clearance queries return more leaves: 113.8M against 69.8M per pass. Per leaf,
      `Simplex::enlarge` took 1.77 s against 0.35 s and the intersection test 1.15 s against
      0.21 s. Item and tree-entry lookups by id reached 115.8M per pass in `BTreeMap`s.
    - The progress statistics (a full clearance scan every 10 connections) took 0.78 s against
      0.18 s.
    - The `.kicad_pcb` arm is not cheaper per room: 347 µs per room against 338 µs for the
      polylines and 201 µs for the chords. It searches 23657 rooms against 60286, because it
      starts with 24 unrouted nets instead of 31 and tries 22 connections per pass instead of
      31, 4 of them failing instead of 8.
  - **Fix.** Every change is an exact speed-up in the layer where the time went. Each step was
    checked: the same SES and the same events, with `elapsed_ms` left out. Times are per pass,
    polyline and chord DSN:
    | step | polyline | chord |
    |---|---|---|
    | baseline | 20.4 s | 12.1 s |
    | split skip and hole-to-hole memo | 17.4 s | 11.2 s |
    | enlarge memo and box pre-test | 15.4 s | 10.5 s |
    | dense keys in `UndoableObjects` | 13.5 s | 9.25 s |
    | dense keys for tree entries | 13.2 s | 8.98 s |
    | statistics memo, with `--progress json` | 12.66 s | 8.8 s |
    - Geometry:
      - `PolylineArea::split_to_convex_counted` skips the cutout when a piece and a hole piece
        are both bounded and their boxes are apart. It uses `IntBox::is_apart_from`, which
        needs a gap of more than 1 unit on each side, computed in i64. The skipped piece
        becomes `TileShape::cutout_apart`, the representation `cutout_counted` returns there.
        A one-line simplex (dimension 2, unbounded) is never skipped: Java cuts pieces far
        from it into several, as on Issue756-tomu-fpga9.
      - `Simplex::enlarge` keeps its first result by the offset's bits; the original is
        `enlarge_uncached`.
    - Engine:
      - `ShapeSearchTree::clearance_search_entries_where` drops a leaf whose enlarged box is
        apart from the query's before the exact intersection test.
      - `MazeSearch::hole_to_hole` memoizes `drill_breaks_hole_to_hole` per drill and ripped
        via. That query is not metered, so the work units are unchanged.
      - `UndoableObjects` mirrors its `BTreeMap` in a vector for ids below 4Mi
        (`DenseKey`, ItemId only). `ShapeSearchTree.entries` is the new `ItemIdMap`
        (`board/search/entry_map.rs`), a vector below 1Mi ids. Both keep the map's order for
        iteration.
      - `PassRunner` keeps the progress events' `BoardStatistics` by `state_hash`, up to 64
        states (`StatisticsMemo`).
    - No polyline is coarsened and no result changes, so DIVERGENCES has no line.
  - **Tests.** They were written after the profiling-driven fixes. RED was shown by breaking
    each fix:
    - `crates/freerouting-geometry/tests/split_skip.rs`, 7 tests:
      - the split against `split_to_convex_reference` (pieces and `SplitReach`) on a 300-segment
        arc page, 150 seeded random pages and the one-line simplex hole;
      - `cutout_apart` against `cutout_counted`, over 1000 or more apart pairs;
      - `enlarge` against `enlarge_uncached`;
      - `enlarge_keeps_its_first_result`, which checks pointer identity;
      - `is_apart_from` at its margin and at `i32::MIN`/`i32::MAX`.

      With the enlarge cache off and `simplify` dropped from `cutout_apart`, 3 of 6 failed.
      Without the `is_bounded` guard, `a_one_line_simplex_hole_is_cut_out_of_pieces_far_from_it`
      failed, and so did `board_probe_scores::scores_match_java`: Issue756-tomu-fpga9 had
      incomplete 24 against Java's 35.
    - `datastructures::undoable_objects::tests::dense_lookups_answer_as_the_map_does` runs
      20000 random operations against a map-only key type.
    - `board::search::entry_map::tests::iterates_in_item_id_order_across_the_vector_and_the_map`
      covers negative ids.
    - `autoroute::batch::pass::statistics_memo_tests::a_board_state_seen_before_reuses_its_statistics`.
    - A workspace run with a cross-check in the skip branch printed 0 mismatches.
  - **Runs.** Binary `target/orchestrator/k139/bin/freerouting-rs-k139` (`freerouting-rs 0.1.0
    (git 91b2f38d5891)` plus this diff, sha256 `3a7ccd2a…`). Script
    `tools/kicad-reader/plugin-input-ab.sh`, options `{"schema": 5, "profile": "enhanced"}`,
    `ROUTE_TIMEOUT_S=300 ROUTE_MEMORY_KB=8000000`, one kicad-cli at a time. Board copy
    `target/orchestrator/k139/boards/cm5_minima`, outputs under `target/orchestrator/k139/out/`.
    The kicad_pcb arm ran with `SHARE_GRADE_WITH` the dsn arm; the sessions differ, so both
    were graded.
    | arm | rc | aborted | route s | p+l s | pass s (mean, 18 passes) | unconnected (input) | caused by type | mfg / below / vip | arcs back | session |
    |---|---|---|---|---|---|---|---|---|---|---|
    | dsn | 0 | 0 | 278.0 | 1.91 | 12.84 (pass 1 13.39) | 0 (0) | silk_edge_clearance 3, silk_overlap 18 | 0 / 0 / 0 | 200 | `3cd2afa5121b012e8519e7f561f8d21d726d617e60917d6771d5d0fc067739c7` |
    | kicad_pcb | 0 | 0 | 111.3 | 1.59 | 5.16 (pass 1 5.76) | 0 (0) | silk_edge_clearance 3, silk_overlap 18 | 0 / 0 / 0 | 200 | `976b9df2fb0e4f1d0baf408f2910cfb032099854dbc6e71bad75144ff68f23be` |
    - Both arms are graded under `rules: project`. The DRC errors are those of K13.8:
      copper_edge_clearance 2, courtyards_overlap 2, holes_co_located 2.
    - Both sessions equal K13.8's byte for byte. The dsn arm took 438.7 s in K13.8 and the
      kicad_pcb arm 180.5 s.
  - **Gate.**
    - `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings`
      are clean.
    - `cargo test --workspace --no-fail-fast`: 2798 passed, 0 failed.
    - `PARITY_SET=tools/bench/sets/connection-parity.txt tools/parity-check.sh --no-probes`:
      PASS, 7 of 7 exact.
    - `pytest kicad-plugin/tests`: 525 passed, 6 skipped.
