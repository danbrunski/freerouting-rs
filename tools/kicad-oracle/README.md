# KiCad DRC oracle

Grades a routed session with KiCad's own design rules check instead of
Freerouting's board model, so a router can't pass by agreeing with itself.
This follows KiCadRoutingTools' practice of grading with KiCad as the oracle,
and starts from the scripts of the 2026-10-15 head-to-head
([X01](../../docs/roadmap/tasks/archive/X01-krt-head-to-head.md): `prep.py`,
`import_ses.py`, `strip_placement.py`, `classwidths.py`, `stats.py`,
`summarize.py`, `run_board.sh`).

```sh
tools/kicad-oracle/grade.sh [--out DIR] [--keep-tracks] <project-dir or .kicad_pcb> -- \
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
   `pcbnew.ImportSpecctraSES` into a copy of the unrouted board.
5. Copies the board's **original** `.kicad_pro`/`.kicad_dru` over both staged
   boards (a router must not grade itself under rules it wrote) and forces
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
7. Subtracts what the unrouted input already reports: a routed item is
   pre-existing when an input item has the same type and net pair (from the
   `[net]` names in the item descriptions) and lies within 0.5 mm. Writes
   `OUT/summary.json` and prints one line.

`summary.json`: `graded`, `kicad_version`, `board`, `engine_command`,
`engine_exit`, `engine_seconds`, `keepouts_removed`,
`input_tracks_and_vias_removed`, `drc_real` and `drc_real_by_type`
(router-attributable), `preexisting`, `unconnected_items` and
`input_unconnected_items`, `routed_by_type`, `input_by_type`,
`capped_types`, `copper`, `originals_unchanged`, `exit_code`; or `graded:
false` with a `reason`.

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

## KiCad installs

`KICAD_CLI` (default `kicad-cli`) and `KICAD_PYTHON` (default `python3`, which
must import `pcbnew`) pick the install, and both must report the same version;
the version goes into every summary. Prefer the WSL-native KiCad 10.0.7
(`/usr/bin/kicad-cli`, `pcbnew` in the system `python3`): no path translation
and no Windows process start-up. The Windows install (10.0.5, the version Dan
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
| `kicad_steps.py` | KiCad's python (`pcbnew`) | strip, export, import, copper statistics, pad list (tests) |
| `oracle_lib.py` | plain python3 | keepout fix, SES placement strip, class widths, severity forcing, pre-existing subtraction, summary |
| `tests/test_oracle_lib.py` | plain python3 | unit tests: `python3 -m unittest discover -s tools/kicad-oracle/tests -p 'test_*.py'` |
| `tests/run-oracle-tests.sh` | bash + KiCad + Java | end-to-end checks (via in pad, graze under a warning-level clearance, pre-existing edge item, missing `kicad-cli`, KiCad processes and checksums) |
| `tests/inject.py`, `tests/make_edge_board.py` | python3 / KiCad's python | session edits and the synthetic edge board for those checks |
