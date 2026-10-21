# K14 — The board snapshot applies the DSN fix-ups

Opened 2026-10-21 at the owner's request, after the owner's K13.7 run. `canary-pwr` opened on the
DSN export, not the new default. Its saved options turn on three K09 fix-ups (class widths, fixed
pour classes, dropped pours), and the board snapshot refused them at Route.

The four K09 fix-ups that rewrite the DSN (`kicad-plugin/freerouting_rs/dsn_fixups.py`) edit the
text of KiCad's export:
- `class_widths` writes a class's `(rule (width W))`;
- `fixed_pour_classes` in `wires` mode replaces each outer-layer pour of a listed class with fixed
  wires covering its fill (`fill_rects`, `_wire`);
- `drop_pour_nets` removes the outer-layer pours of the listed nets;
- `route_on_plane_layers` turns `(type power)` into `(type signal)`.

The snapshot (`board_input = kicad_pcb`, the default since K13.7) gives the router the `.kicad_pcb`,
so there is no DSN text to edit. Three places handle that today:
- `options.input_conflicts` refuses the four on the snapshot (K10.4);
- a saved file that has them and names no input loads on the DSN (K13.7);
- K13.1's table lists them as missing on the reader.

This checklist makes the `.kicad_pcb` path apply all four. Then `input_conflicts` is empty, and a
board's saved setup runs on the default input.

Prerequisites: K09 (the fix-ups), K10 (the snapshot), K12 (zones are refilled before both inputs),
K13 (the snapshot is the default).

## What the work starts from

- **The reader builds a DSN syntax tree.** `KicadRead::pcb` (`crates/freerouting-kicad/src/read.rs`)
  is the same structure KiCad's export parses to (`to_dsn()`, `copper_graphics.rs::the_written_dsn_keeps_the_areas`).
  So each rewrite has a direct target:
  - class rules: `classes.rs`;
  - planes: `zones.rs`;
  - layer types: `layers.rs`;
  - wires: `tracks.rs`.
- **Zone fills are already in the snapshot.** The fix-ups take each zone's fill from pcbnew
  (`kicad_board.py`, `GetFilledPolysList`). The snapshot's `.kicad_pcb` carries the same fills as
  `(filled_polygon ...)`, because the plugin refills zones before either input (K12).
- **Engine mode already works on both inputs.** `fixed_pour_mode = engine` is the router flag
  `--router.plane_obstacle_net_classes`. Only the `wires` mode and the `auto` lists need the DSN
  text.
- **The `auto` lists need only board data.** They come from the board and the classes of its nets
  (`dsn_fixups.resolve`: `auto_fixed_pour_classes`, `auto_drop_pour_nets`, `auto_class_widths`).
  The plugin has all of that in pcbnew without the DSN.

## Design (proposed 2026-10-21; an item may change it, giving its reason)

- **Router settings.** Each fix-up becomes a router setting that the `.kicad_pcb` path applies to
  the reader's tree. It lives in `crates/freerouting-kicad` behind `ReadOptions`, or in the
  engine's `job::prepare::set_kicad`. The plugin passes it as a flag, the way it passes the Board
  Setup minimums. The CLI can use the setting on any `.kicad_pcb`. No board or net names go in
  code; board values go in fixtures.
- **`auto` stays in the plugin.** The plugin resolves each `auto` list from the pcbnew board and
  passes explicit lists, so the router's settings take only names.
- **Same input on both paths.** The same board with the same option gives the router the same
  input on both paths. Tests compare the reader's tree with the plugin's fixed-up DSN, parsed, not
  routing results. Where the two cannot match exactly (the fill read from the file against pcbnew's
  polygons, the reader's 0.1 µm rounding), the item names the difference and its bound.
- **Profiles and defaults.** The settings work under both profiles. The defaults don't change: all
  four stay off, as K09.9 left them.
- **Fixture.** A new `.kicad_pcb` fixture covers all four: two classes with widths, an outer pour
  of a listed class, an outer pour of a listed net, an inner plane layer, and filled zones. Its
  KiCad DSN export is made once with pcbnew and recorded beside it. `canary-pwr`'s own values stay
  in `fixtures/canary-pwr/`.

## File ownership

| Item | New files | May edit |
|---|---|---|
| K14.1–K14.4 | reader tests and the fixture under `crates/freerouting-kicad/tests/` | `crates/freerouting-kicad/src/**`, `crates/freerouting/src/job/prepare.rs` and its settings, `kicad-plugin/freerouting_rs/{export_flow,dsn_fixups,options}.py` |
| K14.5 | — | `kicad-plugin/freerouting_rs/{options,presets,plugin}.py`, plugin tests, `docs/development/kicad-plugin.md`, `kicad-plugin/README.md`, `docs/DIVERGENCES.md` |
| K14.6 | `docs/roadmap/prereg/K14.json`, `tools/bench/sets/k14-fixups.txt` | `tools/kicad-reader/plugin-input-ab.sh` |

## Checklist

- [ ] **K14.1 — Class widths on the snapshot.**
  - **Change:**
    - A router setting sets the listed classes' track width in the reader's class rules, as
      `dsn_fixups` writes `(rule (width W))`.
    - An unknown class is an error that names it, as the DSN fix-up raises one.
    - The plugin passes the setting for `class_widths`, explicit or `auto` resolved from the
      board, on the snapshot.
  - **Accept:**
    - a reader test: on the fixture, the tree's class rules with the setting equal the fixed-up
      export's, parsed;
    - `auto` resolved from the board on the snapshot gives the DSN path's list on the fixture;
    - plugin tests for the arguments; `input_conflicts` no longer names `class_widths`;
    - a DIVERGENCES line (freerouting-kicad, or the engine if the setting lives there).

- [ ] **K14.2 — Plane layers opened to routing.**
  - **Change:** a router setting turns the listed inner plane layers into signal layers in the
    reader's tree, as the export's `(type power)` becomes `(type signal)`. Their zones stay planes.
    An unknown layer, or one that is not a plane, is an error that names it.
  - **Accept:**
    - a reader test: on the fixture, layer types and planes equal the fixed-up export's;
    - plugin tests; `input_conflicts` no longer names `route_on_plane_layers`;
    - a DIVERGENCES line.

- [ ] **K14.3 — Pours of listed nets dropped.**
  - **Change:**
    - A router setting leaves the outer-layer zones of the listed nets out of the reader's
      planes, as `dsn_fixups` drops their `(plane ...)`.
    - Inner-layer zones of those nets stay.
    - The plugin resolves `auto` (`auto_drop_pour_nets`) from the board.
  - **Accept:**
    - a reader test: on the fixture, the planes equal the fixed-up export's;
    - `auto` gives the DSN path's list;
    - plugin tests; `input_conflicts` no longer names `drop_pour_nets`;
    - a DIVERGENCES line.

- [ ] **K14.4 — Pours of listed classes as fixed wires.**
  - **Change:**
    - For each filled outer-layer zone whose net is in a listed class, the reader's plane is
      dropped.
    - Its fill, read from the snapshot's `filled_polygon`, is covered by fixed wires of that net.
    - The wires use `fill_rects`'s and `_wire`'s arithmetic, ported operation for operation, so
      the same fill gives the same rectangles.
    - The plugin resolves `auto` (`auto_fixed_pour_classes`) from the board.
  - **Accept:**
    - a port test: `fill_rects` on recorded fills gives the Python's rectangles exactly;
    - a reader test: on the fixture, the fixed wires equal the fixed-up export's (count, net,
      layer, width, and coordinates within the reader's 0.1 µm), and the dropped planes match;
    - a route test: the session written from a snapshot route holds none of the fixed wires (the
      DSN path's property: `ImportSpecctraSES` never sees them);
    - plugin tests; `input_conflicts` no longer names `fixed_pour_classes` in `wires` mode or as
      `auto`;
    - a DIVERGENCES line;
    - `legacy` keeps connection-parity 7 of 7 if engine code that `legacy` runs changes.

- [ ] **K14.5 — The plugin stops sending these boards to the DSN.** Once K14.1–K14.4 have landed:
  - **Change:**
    - `input_conflicts` returns nothing for any of the four; keep it, empty, if other refusals
      remain.
    - K13.7's two DSN rules go: the load rule (a file that names no input and has a fix-up loads on
      `dsn`) and `presets.apply`'s move to `dsn` with its warning. Such a file then takes the
      profile's default input, as K13's "only boards with no saved choice get the new default"
      says. A file that says `dsn` keeps it.
    - The dialog's note no longer says the fix-ups need the DSN export.
    - The docs and README say which fix-ups work on which input; the DIVERGENCES lines from K10.4
      and K13.7 change to match.
  - **Accept:**
    - plugin tests: a saved file and a preset with each fix-up route on the snapshot (stub UI);
      the dialog form accepts each on the snapshot; the note has no DSN line;
    - `python3 -m pytest kicad-plugin/tests` passes.

- [ ] **K14.6 — `canary-pwr` with its saved fix-ups, on both inputs.**
  - **Preregister first:** `docs/roadmap/prereg/K14.json` and `tools/bench/sets/k14-fixups.txt`.
    - Board: `canary-pwr` at the canary revision named in the registration.
    - Options: a copy of the board's saved `route-options.json` (the class widths, fixed pour
      classes and dropped pours it has today, with its pass and rip-up settings), with
      `board_input` set per arm.
    - Arms: `dsn` against `kicad_pcb`, one `tools/kicad-reader/plugin-input-ab.sh` run each.
    - Grading: under the board's own rules.
    - Machine: a heavy run, queued through the orchestrator (canary-pwr is exempt from the
      few-minutes rule, decided 2026-10-20).
  - **Accept:**
    - every routing-caused manufacturing type, below-minimum and via-in-pad is 0 in both arms
      (decided 2026-10-20: routing never breaks the board's manufacturing rules);
    - the `kicad_pcb` arm's KiCad unconnected is no higher than the `dsn` arm's, or within the
      noise the registration states;
    - both sessions' sha256s are recorded;
    - the owner's confirmation from a Route inside KiCad on `canary-pwr`, with its saved options,
      on the snapshot, is recorded.
