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

- [ ] **K13.1 — What each input carries.** Make a table of every rule and item the router
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
      it would affect.

- [ ] **K13.2 — Existing copper comes back unchanged.** Find which rounding moves the ends of
  existing tracks on `One-Air-Max`. The reader rounds to 0.1 µm and the export to `%.6g`. Make
  the `kicad_pcb` path return copper that was already on the board at KiCad's own coordinates.
  - **Accept:**
    - a test routes a board with nothing left to route through the `kicad_pcb` path, and the
      imported board's DRC report equals the original's;
    - on `One-Air-Max`, the pre-existing `/Solar_module/SYS`–`VBAT` violation counts against
      the same segment pairs as on the original board.

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

- [ ] **K13.4 — Load time.** The reader's board for `jetson-agx-thor-baseboard` builds in 3.7 s
  against 1.6 s for the DSN (K10). Profile it, then fix it or record why it stays.
  - **Accept:** before and after load times for `jetson` and `CM5_MINIMA_3` (arcs), release
    build, one run each.

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
  registration amendment before any registered row runs.
  - **Accept:**
    - the per-board table (unconnected, errors by type, prepare + load time, route time) is in
      the done note;
    - every board where `kicad_pcb` grades worse is explained;
    - the verdict under K13's switch rule is recorded.

- [ ] **K13.7 — Switch the default.** Only if K13.6 passes the switch rule, or the owner
  accepts the listed exceptions:
  - `board_input` defaults to `kicad_pcb`, with an options schema bump;
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
