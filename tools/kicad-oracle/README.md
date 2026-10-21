# KiCad DRC oracle

Grades a routed session with KiCad's own design rules check instead of
Freerouting's board model, so a router can't pass by agreeing with itself.
This follows KiCadRoutingTools' practice of grading with KiCad as the oracle,
and starts from the scripts of the 2026-10-15 head-to-head
([X01](../../docs/roadmap/tasks/archive/X01-krt-head-to-head.md): `prep.py`,
`import_ses.py`, `strip_placement.py`, `classwidths.py`, `stats.py`,
`summarize.py`, `run_board.sh`).

```sh
tools/kicad-oracle/grade.sh [--out DIR] [--keep-tracks] [--export plugin OPTIONS.json] \
  <project-dir or .kicad_pcb> -- \
  java -jar freerouting.jar -de {DSN} -do {SES} --gui.enabled=false \
  --router.autorouter.max_passes=5 --router.optimizer.enabled=false
tools/kicad-oracle/grade-set.sh tools/bench/sets/kicad.txt <out> -- <engine command>
```

## What `grade.sh` does

1. Copies the `.kicad_pcb`, `.kicad_pro` and `.kicad_dru` (same base name)
   into `OUT/source`, `OUT/unrouted` and later `OUT/routed`, one directory
   per variant under the original file name (`kicad-cli` takes the rules
   from the project files with the board's own name, or silently uses
   defaults), and makes them writable (copies from a read-only install keep
   read-only modes). The originals are checksummed before and after; a
   change makes the run "not graded".
2. Strips tracks and vias (zones kept) unless `--keep-tracks`, and exports
   the DSN with `pcbnew.ExportSpecctraDSN`. Then removes the structure-level
   `(keepout ...)` scopes KiCad exports for rule areas that allow both tracks
   and vias (they ban only footprints, pads or pours; matched by layer and
   outline within 1 um), so the DSN-based engine gets the same room as the
   `.kicad_pcb`. On `canary-pwr` that is the 26 x 26 mm fan inlet on B.Cu.
3. Runs the engine with `{DSN}` and `{SES}` replaced.
4. Strips the SES `(placement ...)` scope and imports it with
   `pcbnew.ImportSpecctraSES` into a copy of the unrouted board. The import
   then does what the plugin does (`fix_via_drills`, K09; Q22.2): KiCad takes a
   routed via's diameter from the session's padstack but its drill from the
   net class, so a `Via[0-3]_600:300_um` on a net of a 0.8 / 0.4 mm class came
   back 0.6 / 0.4 and KiCad's DRC reported an annular ring of 0.1 mm
   (canary-pwr, 4 `annular_width` items); the drill is set back to the
   padstack's 0.3 mm. Then the input copper goes back to its own coordinates
   and uuids (`restore_copper.py`, Q22.3): KiCad's DSN export prints `%.6g`, so
   the session gave back input tracks and vias up to 0.6 µm off, and the grade
   counted them as moved. `kicad_steps.py` loads `kicad-plugin/freerouting_rs/`'s
   `ses_vias.py`, `kicad_board.py` and `restore_copper.py` by path, never the
   plugin package.
5. Copies the board's **original** `.kicad_pro`/`.kicad_dru` over both staged
   boards (a router must not grade itself under rules it wrote). A project
   without a `.kicad_pro` gets the one KiCad wrote when step 2 first saved the
   original board (Q19): a KiCad 4 or 5 board keeps its net classes and setup
   in the board file, KiCad 6+ moves them to the project, and without that
   file `kicad-cli` grades under KiCad's defaults (0.2 mm clearance where
   `d3c-0496` says 0.1524). `summary.json`'s `rules` says which: `project`,
   `board` or `defaults`. Then forces
   `clearance`, `shorting_items`, `tracks_crossing`, `hole_clearance`,
   `hole_to_hole`, `holes_co_located` and `copper_edge_clearance` to `error`
   in the staged project, so a board whose author set `clearance` to
   `warning` still counts a graze. Runs
   `kicad-cli pcb drc --format json --severity-all --refill-zones` on the
   unrouted and the routed board; pours are refilled, so a trace that cuts a
   pour shows up.
6. Counts copper from the routed board's geometry
   (`OUT/routed/copper.json`): tracks, vias, length, and tracks, vias and
   drills below the board minimums (KiCad 10's DRC capped `track_width` at
   199 items in the head-to-head, so counts come from geometry, and a DRC
   count of exactly 199 is flagged in `capped_types` as "at least 199").
   It also measures **copper narrower than its net class**, which KiCad's DRC
   never checks (a current-capacity violation under D8): the number and
   length of such tracks, the worst shortfall and the five nets with the most
   under-width length. Net-to-class widths come from the `(class ...)` scopes
   of the DSN KiCad exported (`GetEffectiveNetClass()` returned an unwrapped
   object in 10.0.5).
7. Counts what routing caused (Q22.2). `oracle_lib.read_copper` reads every
   segment, arc and via of the routed and the unrouted `.kicad_pcb` (uuid to
   geometry in nanometres). A DRC item is **routing-caused** when at least one
   of its items is a track or via that the unrouted board does not have with
   the same coordinates (to the nanometre; the order of a track's ends does
   not matter), width, layer and net (a via: position, diameter, drill, layers
   and net; a net is its name, so a KiCad 6-9 file that names nets by number is
   read through its `(net N "name")` table, and one with numbers and no table is an
   error, Q22.13). Pads, footprint graphics, text, zones and unchanged input tracks
   and vias are input copper, and an item made only of input copper is
   pre-existing, whatever the input report listed: KiCad prints a pair in
   either order, once per copper layer, and stops a type at its cap, so the two
   reports do not list the same pairs. The old match (same type, net pair and
   position within 0.5 mm) counted a pair that the input report listed in the
   other order twice, and hid a routed track over a footprint line when the
   input listed the line against a same-net pad (pcietest). The exception:
   `starved_thermal` and `isolated_copper` items name only a zone and a pad,
   so no track tells input from routing there; they keep the old report match,
   and a thermal that routing starved still counts. Input copper that the
   DSN round trip moved by 1 nm or more is routing-caused until Q22.3 restores
   it. Writes `OUT/summary.json` and prints one line. Without the two boards
   (`oracle_lib.py summarize` with five arguments) the old match applies and
   `routing_caused_basis` says `report`.

`summary.json`: `graded`, `kicad_version`, `board`, `engine_command`,
`engine_exit`, `engine_seconds`, `keepouts_removed` (`plugin` with
`--export plugin`), `input_tracks_and_vias_removed`, `rules`, `export` (`bare` or
`plugin`, with `plugin_options`), `drc_real` and `drc_real_by_type`
(router-attributable), `preexisting`, `routing_caused_basis` (`copper`, or
`report` for the old match), `unconnected_items` and
`input_unconnected_items`, `routed_by_type`, `input_by_type`,
`capped_types`, `netclass_clearance`, `copper`, `originals_unchanged`,
`exit_code`; or `graded: false` with a `reason`.

`netclass_clearance` (N05.3, Q14) counts the routed board's clearance and
hole-clearance items whose rule is a net-class clearance. KiCad names the rule
it applied in the item description (`netclass 'Power_High' clearance 0.2500
mm`, against `board minimum clearance`, `board setup constraints hole
clearance`, `rule 'x'` or, for an item without a net class, an empty source),
and it applies the largest rule, so a net-class source means the class
clearance is at least the board minimum. `netclass_via_clearance_errors`: such
items with two items, one of them a via, on two different nets;
`netclass_clearance_errors`: all of them. `python3 oracle_lib.py
netclass-clearance DRC.json` prints both for any KiCad `drc.json` (canary's
grade, for one).

`tidiness` (Q15.2): when `freerouting-dev` is built (`FREEROUTING_DEV`, default
`target/release/freerouting-dev`), `grade.sh` also scores the SES the engine wrote
with `freerouting-dev tidy` and adds `connections`, `skipped`, `nets` and
`measures` (median and p90 of `detour`, `bends_per_10mm`, `acute`, `jogs`, `vias`
per connection) to the summary. It is reported, never part of the verdict or the
exit status, and costs no KiCad call. `tools/krt-h2h/grade_routed.sh` does the same
for the routed `.kicad_pcb` of an engine that writes a board. The definitions are in
[tools/bench/README.md](../bench/README.md#freerouting-dev-tidy-how-tidy-a-route-is-q15).

Exit status: **0** graded clean (no router-attributable DRC item and no copper
below the board minimums), **1** router-attributable items, **2** not graded
(no `kicad-cli`, no `pcbnew`, `kicad-cli` and `pcbnew` of different versions,
a failed export or import, no SES). "Not graded" never counts as a pass.
Copper narrower than its net class is reported, not part of the exit status:
D8 allows a short neck-down at a pin, whose bound is Q02's.

## `--export plugin OPTIONS.json` (Q22.6)

By default the grader routes KiCad's bare `ExportSpecctraDSN` plus the rule-area
keepout fix of step 2. A KiCad user routes the DSN the **plugin** writes, with
its fix-ups (edge strips, pad-local clearances, class widths, pours) and its
router flags, so a measurement that routes the bare export can pass where a
plugin run fails (canary-pwr's FID3 pad clearance: 1 `clearance` item). With
`--export plugin OPTIONS.json` (a `route-options.json`, what the plugin's dialog
saves; `{"schema": 4}` is the defaults):

- the DSN is exported by `python3 -I kicad-plugin/headless_run.py export` on the
  unrouted board into `OUT/work/`, together with `BASE.args.json`, the router
  flags `RouteOptions.to_args` gives plus the board's minimums; the rule-area fix
  of step 2 is the plugin's own;
- the engine command gets those flags at `{PLUGIN_ARGS}` (replaced by one
  shell-quoted string, for `bash -c "... {PLUGIN_ARGS} ..."`), or else right
  after its `{SES}` argument, so the command's own flags come later and win. A
  command with neither is not graded (exit 2);
- the SES is imported by `headless_run.py import` (the via-drill fix, the zone
  refill, and the input copper locked for `fix_existing`).

Without the flag nothing changes. `tests/run-plugin-export-test.sh WORK_DIR`
checks that the graded DSN is byte-identical to `headless_run.py export`'s.

## KiCad installs

`KICAD_CLI` (default `kicad-cli`) and `KICAD_PYTHON` (default `python3`, which
must import `pcbnew`) pick the install, and both must report the same version;
the version goes into every summary. Prefer the WSL-native KiCad 10.0.7
(`/usr/bin/kicad-cli`, `pcbnew` in the system `python3`): no path translation
and no Windows process start-up. The Windows install (10.0.5, the version the owner
saves his boards with, about 8 s of start-up per call) would need `wslpath -w`
path translation, which these scripts don't do yet. Never mix versions in one
comparison.

## Gotchas

- Never close, restart or kill a running KiCad, never touch the Java
  Freerouting plugin, and never import the freerouting-rs plugin package in
  KiCad's Python. The scripts start no GUI.
- `BOARD.GetTracks()` fails in 10.0.5; the scripts use `BOARD.Tracks()`.
- In 10.0.7, exporting the DSN right after removing tracks in the same process
  crashed, and a second `LoadBoard` in one process returned an unwrapped
  `SwigPyObject`; stripping and exporting are separate processes. SWIG's
  memory-leak messages at exit mix with stdout, so counts go through files.

## Files

| File | Runs with | Purpose |
|---|---|---|
| `grade.sh` | bash | the steps above |
| `grade-set.sh` | bash | `grade.sh` over a set file, `grades.tsv` |
| `kicad_steps.py` | KiCad's python (`pcbnew`) | strip, export, import (with the via-drill fix and the input copper restored), copper statistics, pad list (tests) |
| `oracle_lib.py` | plain python3 | keepout fix, SES placement strip, class widths, severity forcing, `.kicad_pcb` copper reader, routing-caused split (Q22.2), summary |
| `dangling_visible.py` | KiCad's python (`pcbnew`) | splits KiCad's `via_dangling`/`track_dangling` into what the DSN shows and the plane-fill limit (a via or track end inside a DSN plane of its net, outside KiCad's fill), Q16.4 |
| `tests/test_dangling_visible.py` | plain python3 | unit tests of its DSN plane reader and point-in-polygon test |
| `tests/test_oracle_lib.py`, `tests/test_routing_caused.py` | plain python3 | unit tests: `python3 -m unittest discover -s tools/kicad-oracle/tests -p 'test_*.py'` |
| `tests/test_import_via_drill.py` | KiCad's python (skipped without `pcbnew`) | the import sets a `Via[0-1]_600:300_um` on a 0.8 / 0.4 mm class to drill 0.3 mm |
| `tests/test_import_restore.py` | KiCad's python (skipped without `pcbnew`) | Q22.3: the import puts an input track and via moved under 0.6 µm back at their nanometre coordinates and uuids, a routed end follows, a track moved 2 µm stays |
| `tests/run-plugin-export-test.sh` | bash + KiCad, no Java | Q22.6: `--export plugin` routes the plugin's DSN (byte-identical to `headless_run.py export`'s), passes its flags, `{PLUGIN_ARGS}`, not graded without a place for them |
| `tests/run-oracle-tests.sh` | bash + KiCad + Java | end-to-end checks (via in pad, graze under a warning-level clearance, pre-existing edge item, missing `kicad-cli`, KiCad processes and checksums) |
| `tests/inject.py`, `tests/make_edge_board.py` | python3 / KiCad's python | session edits and the synthetic edge board for those checks |
| `tests/run-legacy-rules-test.sh` | bash + KiCad, no Java | Q19.2: a KiCad 4 board without a `.kicad_pro` grades under its own 0.15 mm net-class clearance (`rules: board`), not KiCad's 0.2 mm default; a 0.12 mm gap is still reported |
| `tests/make_legacy_board.py` | plain python3 | that check's board (written as KiCad 4 text) and its two sessions |
