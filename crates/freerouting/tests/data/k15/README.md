# K15 test boards

Bug 1 of `docs/development/pcu-rev2-board-report.md` (K15.1): C65's F.Fab circle,
at y = 105.242039 mm, places to an unbounded simplex, and before K15.1 its box
emptied the board's bounding box, so no connection on the board routed.

- `far-nm.kicad_pcb`: `docs/development/pcu-rev2/c65-nm-position.kicad_pcb`
  with four more 1206 capacitors, C901 to C904, on nets `/FAR_A` to `/FAR_D`,
  75 to 95 mm from C65 (6 nets). Made by
  `python3 -I tools/kicad-reader/k15_far_fixture.py`.
- `c65-nm-position.kicad10.dsn`, `c65-snapped.kicad10.dsn`, `far-nm.kicad10.dsn`:
  KiCad 10.0.7's own Specctra export (`pcbnew.ExportSpecctraDSN`) of
  `docs/development/pcu-rev2/c65-nm-position.kicad_pcb`,
  `docs/development/pcu-rev2/c65-snapped.kicad_pcb` and `far-nm.kicad_pcb`
  (orchestrator investigation `pcu1`, 2026-10-21). The only edit: the `pcb`
  name on line 1, which held the exporting machine's path, is the file name.

The tests are `crates/freerouting/tests/unbounded_item_box.rs` (the board box)
and `crates/freerouting-cli/tests/cli_unbounded_outline.rs` (the routes).
