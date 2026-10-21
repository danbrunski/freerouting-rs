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
