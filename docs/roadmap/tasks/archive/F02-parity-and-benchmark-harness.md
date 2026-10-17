# F02 — Parity and benchmark harness

Opened 2026-10-15. The verification ladder in
[port-completion.md](../../port-completion.md) needs four tools that don't exist
yet:
- a one-line way to run a Java probe at the pin;
- a structural SES comparison;
- a benchmark runner that times Java and Rust on the same boards;
- a determinism check that routes twice, idle and under load.
Each is needed by many later checklists, so it is built once here, before the
router, and tested on Java's own outputs.

## What exists and what this adds

| Exists | Used for |
|---|---|
| `tools/java-reference/run.sh` | Builds Java at a checkout into `target/java-reference/` and dumps `load`/`route` references |
| The ECJ probe recipe (implementer agent, "The Java reference") | Compiling and running a `Probe.java` against the pinned classes, by hand, four commands |
| `freerouting::io::specctra::ses_reader::read_session` | Importing a SES into a board |
| `freerouting::scoring::BoardStatistics`, `freerouting::drc` | Completion, vias, lengths and clearance violations of a board |
| `docs/development/tools/compare.py` in the Java fork | Java jar-vs-jar comparison (unrouted, violations, vias, time). Its parsing of Java logs is reusable |

What this adds:
- `tools/java-reference/probe.sh`;
- a dev-only binary crate `freerouting-dev` with `ses-diff` and `bench`;
- `tools/determinism-check.sh`;
- committed board sets in `tools/bench/sets/`;
- a KiCad-DRC oracle for boards that have a `.kicad_pcb`.

## Decisions made here

- **A separate dev crate, not CLI flags.** `crates/freerouting-dev`
  (`publish = false`, binary `freerouting-dev`) holds developer tools, so the
  user-facing `freerouting` CLI stays Java-compatible. It depends only on
  `freerouting`. Clap or another argument parser is a `NEW DEP`. A hand-rolled
  parser like `freerouting-cli`'s is fine.
- **Structural comparison, not text diff.** Two sessions for the same DSN are
  compared net by net: wires as sets of (layer, width, normalized point list),
  and vias as sets of (padstack, position, net). Text order and number
  formatting don't count. Board statistics are compared after importing each
  session (decision D4's aggregate fields).
- **Quiet machine or no number.** The benchmark runner refuses to time a run
  while the 1-minute load average is above 1.0 (configurable). It records the
  load average next to every timing. This follows the Java fork's
  measurement rule: never benchmark while builds or tests run.
- **Owner boards stay out of the repo** until Dan says otherwise. Board sets can
  name a path outside the repo (for example a copy of `canary-pwr`); such
  entries are skipped with a note when the file is missing.

## File ownership

| Item | New files | May edit |
|---|---|---|
| F02.1 | `tools/java-reference/probe.sh`, `tools/java-reference/probes/JavaRandomProbe.java` | `tools/java-reference/README.md`, `.claude/agents/implementer.md` (probe recipe → script) |
| F02.2 | `crates/freerouting-dev/**` | `Cargo.toml` (workspace members are `crates/*`, so nothing to add) |
| F02.3 | `crates/freerouting-dev/src/bench.rs`, `tools/bench/README.md` | `crates/freerouting-dev/src/main.rs` (append a subcommand) |
| F02.4 | `tools/determinism-check.sh` | `docs/roadmap/tasks/J01-repin-deterministic-java.md` (point J01.4 at it) |
| F02.5 | `tools/bench/sets/*.txt` | `tools/bench/README.md` |
| F02.6 | `tools/kicad-oracle/**`, `tools/bench/sets/kicad.txt` | `tools/bench/README.md` |

## Checklist

- [x] **F02.1 — One-command Java probes.**
  `tools/java-reference/probe.sh <pin-checkout> <Probe.java> [args...]`:
  - builds the pinned classes through `run.sh` if
    `target/java-reference/classes` is missing
  - compiles the probe with ECJ into a temp dir and runs it with the jdk4py
    runtime and the same classpath `run.sh` uses
  - prints the probe's stdout, and fails with the compiler's message on a
    compile error
  `probes/JavaRandomProbe.java` prints `new Random(42).nextInt(100)` five
  times and `Collections.shuffle(0..9, new Random(99))`.
  - **Accept:**
    - Running the sample probe prints exactly the values asserted in
      `freerouting_geometry::jmath`'s tests (`30 63 48 84 70`, `[8, 1, 4, 3,
      9, 6, 0, 2, 5, 7]`).
    - A probe with a syntax error fails with a non-zero exit and ECJ's message.
    - The implementer agent's four-command recipe is replaced by the script,
      with the worktree add/remove steps kept.
  *Done 2026-10-15:* `tools/java-reference/probe.sh` and
  `probes/JavaRandomProbe.java`. With an empty `REFERENCE_WORK_DIR` and a
  fresh `cda540f` worktree, `probe.sh <pin> probes/JavaRandomProbe.java`
  built the classes through `run.sh` (fixture filter that matches nothing, no
  reference file written, `git status --short fixtures/reference` clean) and
  printed exactly `30 63 48 84 70` and `[8, 1, 4, 3, 9, 6, 0, 2, 5, 7]` in
  10 s; on the shared `target/java-reference` cache it reuses the classes.
  `freerouting-dev`'s `tests/probe_values.rs::java_random_probe_values_match_jmath`
  asserts the same values on `jmath::JavaRandom` (jmath's own tests assert
  `30, 63` and the shuffle). A probe with `int x = ;` exits 1 and prints ECJ's
  `Syntax error on token "=", VariableInitializer expected after this token`.
  A packaged probe with arguments passes them and its exit status (3)
  through. A class cache from another commit than the worktree's HEAD is
  refused with exit 2. `.claude/agents/implementer.md`'s four commands are
  replaced by the script, with the worktree add/remove steps kept;
  `tools/java-reference/README.md` has a "Probes" section.

- [x] **F02.2 — `freerouting-dev ses-diff`.**
  `freerouting-dev ses-diff board.dsn a.ses b.ses [--json out.json]` loads the
  board, imports each session into its own copy, and compares:
  - per net: wires and vias as described under the decisions
  - statistics: incomplete connections, clearance violations (full DRC),
    via count, total trace length, and bend counts
  It prints the first differing nets (up to 10) with the differing wires, plus
  the statistics deltas. It exits 0 when the sessions are equivalent and 1 when
  they differ.
  - **Accept:** tests in `crates/freerouting-dev/tests/`:
    - a Java reference SES against itself is equivalent
    - the same SES with wires reordered and coordinates reformatted
      (`1000` vs `1000.0`) is equivalent
    - one via moved by one unit is reported with its net and position
    - one wire removed is reported, and the incomplete count goes up by the
      right number
    - on `Issue508-DAC2020_bm01`, the statistics of an import of Java's
      reference SES match `fixtures/reference/route/Issue508-DAC2020_bm01.json`
      on the compared fields
  *Done 2026-10-15:* new crate `crates/freerouting-dev` (`publish = false`,
  depends only on `freerouting`, hand-rolled argument parsing, no new
  dependency): `session_geometry.rs` (wires and vias keyed in board
  coordinates converted as `read_session` does, multisets per net, paths
  direction-normalized), `ses_diff.rs` (import into a clone of the loaded
  board per session, statistics, text and `--json` report), `json_read.rs`.
  `cargo test -p freerouting-dev`: 11 passed. `tests/ses_diff.rs`:
  `java_session_is_equivalent_to_itself`;
  `reordered_and_reformatted_session_is_equivalent` (nets, items and path
  points reversed, every path number rewritten `N` -> `N.0`);
  `via_moved_by_one_unit_is_reported_with_net_and_position` (net and both
  positions in the text); `removed_wire_is_reported_and_incomplete_count_rises`
  (net `N$2`: its own count 0 -> 1, total 18 -> 19; Java's import of the same
  edited SES, `probes/SesImportStats.java` at `cda540f`, gives 19);
  `imported_java_session_matches_java_route_statistics` (incomplete 18,
  violations 0, vias 126, length 3778.74 mm equal to the route JSON; bends
  839 against the JSON's 841, which Java counted on its routed board in
  memory: Java's own import of the SES also gives 839, so bends are compared
  with Java's import); `json_output_lists_differing_nets`. CLI exit codes:
  `freerouting-dev ses-diff` on the same SES twice exits 0 (0.18 s release),
  with a wire removed exits 1.

- [x] **F02.3 — `freerouting-dev bench`.**
  `freerouting-dev bench --set <file> --java <jar> [--rust <bin>] --runs N
  --out <dir>`:
  - routes each board with each engine, using the per-board bounds in
    `tools/java-reference/route-bounds.tsv` (J01.3); until that exists, a
    `--max-passes` default
  - always passes `--router.optimizer.enabled=false` unless `--optimize`
  - records wall time, the 1-minute load average before and after, the
    `ses-diff` statistics of each result, and the engine's exit status
  - writes `bench.json` and a markdown table (median and spread per board and
    engine)
  The Java side launches `java -jar` (Java 25 required; check `java -version`).
  The Rust side is skipped until the CLI routes.
  - **Accept:**
    - A Java-only run of 3 small fixtures, 2 runs each, writes both files.
    - A run started while a CPU burner holds the load above the threshold
      waits and then refuses with a clear message, without timing anything.
    - The markdown table renders with one row per board and engine.
  *Done 2026-10-15:* `crates/freerouting-dev/src/bench.rs` (set and
  `route-bounds.tsv` reading, load wait and refusal, `java -version` check,
  timed runs with a safety timeout, SES statistics, `WORK_LIMIT_CUTOFF`
  count) and `bench_report.rs` (`bench.json`, `bench.md`); `--max-passes`
  default 5 until J01.3's bounds file exists. A Java-only run of 3 small
  fixtures (`min_fr_test`, `Issue368-CorneyIslandWireless_input_design`,
  `Issue110-testProjectFromFreeroutingBugTest`) with the `a773a70ec` jar,
  `--runs 2`, wrote `bench.json` and `bench.md` (medians 3.23 / 3.51 /
  3.18 s, spreads 0.19 / 0.64 / 0.04 s); other agents held the load at
  9–10 the whole time, so that run used `--max-load 12`, and its table
  records it. With 8 CPU burners and the default limit 1.0,
  `--load-wait 20` printed `waiting up to 20 s`, then `refusing to time: the
  1-minute load average is 8.30, above the limit 1.00, after waiting 20 s`,
  exit 3, no output directory and no Java process started. Tests
  (`cargo test -p freerouting-dev --test bench`, 9 passed):
  `markdown_has_one_row_per_board_and_engine` (4 rows for 2 boards x 2
  engines, same cell count as the header),
  `busy_machine_waits_then_refuses_without_timing`,
  `quiet_machine_is_timed_once_the_load_drops`,
  `json_records_every_run_with_its_load`, set, bounds, version, median and
  cut-off parsing. With `--rust` pointing at today's CLI the Rust rows read
  `skipped: the Rust CLI does not route yet (exit 2, no SES)`; timing Rust
  routes waits for the router (A13/A14).

- [x] **F02.4 — Determinism check.**
  `tools/determinism-check.sh --out <dir> -- <command with {OUT} placeholder>`:
  - runs the command once on an idle machine and once beside one busy-loop
    process per core (started 5 s before, killed after)
  - substitutes `{OUT}` with a separate output path each time
  - compares the two outputs byte for byte
  - prints both load averages and `identical` or the first differing line;
    exits 1 on a difference
  Point J01.4 at this script instead of its own.
  - **Accept:**
    - The script reports `identical` for a Java `load` reference dump.
    - It reports a difference, and exits 1, for a command that writes the
      current time.
    - No burner process is left running after either run (check with `pgrep`
      in the test), including after Ctrl-C.
  *Done 2026-10-15:* `tools/determinism-check.sh` (options `--out`,
  `--ignore REGEX` for timing lines, `--warmup`; the command runs in its own
  process group, so the traps stop it and its children). Its test,
  `REFERENCE_WORK_DIR=<scratch>/jwork tools/test-determinism-check.sh <pin
  worktree at cda540f> <scratch>/det-test`, prints `determinism-check tests
  passed`: a Java `load` dump of `Issue508-DAC2020_bm01` through
  `reference.ReferenceDump` into `{OUT}` reports `identical` (idle run at load
  3.45, loaded run at 3.38 with 8 burners); `date +%s.%N > {OUT}` reports
  `different:` with the first line and exits 1; `pgrep -f
  determinism-check-burner` is empty after both, and after a SIGINT to the
  script's process group in the middle of the loaded run (exit 130, `sleep`
  child gone too). J01.4 now says how to call the dump into `{OUT}`.

- [x] **F02.5 — Board sets.**
  Commit `tools/bench/sets/`, one board path per line with `#` comments:
  - `connection-parity.txt`: the smallest fixtures with at least two
    connections, for rung 2 of the verification ladder (choose by
    `fixtures/reference/load` connection counts)
  - `benchmark.txt`: about 15 boards spanning sizes, including the four
    inner-plane boards from the Java fork's HANDOVER
    (`Issue145-smoothieboard`, `Issue753-CPU-85_r104`,
    `Issue555-CNH_Functional_Tester_1`, `Issue269-z10_module`) and the two
    watch-list boards from `engine.md` (`Issue066-Project_GP8B`,
    `Issue145-smoothieboard`)
  - `owner.txt`: Dan's boards. `canary-pwr` is in the repo since 2026-10-15
    (`fixtures/canary-pwr/`: three variants pours1–3, the planes variant and
    the unpoured board; Dan approved committing them). Any further owner
    boards are out-of-repo paths, skipped when missing
  - **Accept:**
    - Every in-repo path exists.
    - `tools/bench/README.md` explains each set and how it was chosen, with
      each board's connection count and size.
    - `freerouting-dev bench --set connection-parity.txt` (Java only)
      completes.
  *Done 2026-10-15:* `tools/bench/sets/connection-parity.txt` (10 boards, 3–10
  connections), `benchmark.txt` (16 boards, 9–1125 connections, with the four
  inner-plane and two watch-list boards), `owner.txt` (the five in-repo
  `canary-pwr` variants plus one out-of-repo snapshot). Every listed path
  exists (checked with a script over all three sets; the out-of-repo one
  exists on this machine too). `tools/bench/README.md` explains each set and
  how it was chosen, with a table of connections, pins, layers, nets and DSN
  size per board from `fixtures/reference/load`. `freerouting-dev bench --set
  tools/bench/sets/connection-parity.txt --java freerouting-a773a70ec.jar
  --runs 1 --max-load 16` completed all 10 boards in 34 s, exit 0 (load 5.4–6.6
  from other agents). Its statistics for `Issue413-test`, `TestSensel` and
  `PCBench-retroreflectors-CONGFLOCK` (8 / 16 / 9 violations) equal Java's own
  import of the same SES files (`probes/SesImportStats.java`).

- [x] **F02.6 — KiCad DRC as an independent oracle.**
  For a board with a `.kicad_pcb`, grade a routed session with KiCad's own DRC
  rather than Freerouting's model. This follows KiCadRoutingTools' practice of
  grading with KiCad as the oracle. Two KiCad 10 installs are available:
  - **WSL native, 10.0.7** (`/usr/bin/kicad-cli`, `pcbnew` importable from
    `python3`). Prefer it: no path translation, no Windows process start-up.
  - **Windows, 10.0.5**, the version Dan saves his boards with:
    `/mnt/c/Program Files/KiCad/10.0/bin/kicad-cli.exe` and
    `/mnt/c/Program Files/KiCad/10.0/bin/python.exe`, with paths passed
    through `wslpath -w`. The 2026-10-15 head-to-head used this one, at
    about 8 s of start-up per call.
  Record the KiCad version in every summary, and never mix versions inside
  one comparison.
  `tools/kicad-oracle/grade.sh <project-dir> <engine-command with {DSN} {SES}>`
  does the following:
  1. Copies the `.kicad_pcb`, `.kicad_pro` and `.kicad_dru` into a scratch
     dir. `kicad-cli` may rewrite the project file, so it never works on
     the original.
  2. Exports the DSN with `pcbnew.ExportSpecctraDSN`.
  3. Runs the engine command.
  4. Imports the SES with `pcbnew.ImportSpecctraSES` and saves the copy.
  5. Runs `kicad-cli pcb drc --refill-zones --format json`. Pours are
     refilled, so a trace that cuts a pour shows up, which is canary-09's
     lesson.
  6. Writes a summary JSON with violations by type, unconnected items, and
     the KiCad version.
  Board sets gain a `kicad.txt`: KiCad's bundled demos
  (`C:\Program Files\KiCad\10.0\share\kicad\demos`, about 15 boards) and Dan's
  boards, all out of the repo.
  - **Grading rules** (from KiCadRoutingTools' `awx/baseline_freerouting.py`
    and its CLAUDE.md, plus the 2026-10-15 head-to-head):
    - Grade every engine's result under the board's **original** project
      rules: copy the original `.kicad_pro`/`.kicad_dru` over the result's
      before DRC. KiCadRoutingTools' `route.py` rewrites the routed
      project's DRC settings "to match the routed floors"; on `ecc83-pp` it
      set `min_copper_edge_clearance` 0.01 → 0.2 mm and `min_via_drill` to
      0.6 mm. A router must not grade itself under rules it wrote.
    - On the staged copy, force the copper classes (`clearance`,
      `shorting_items`, `tracks_crossing`, the hole classes,
      `copper_edge_clearance`) to `error`, so a board whose author set
      `clearance` to `warning` still counts a graze.
    - Run with `--severity-all --refill-zones`.
    - Subtract what the unrouted input already reports: match by type, then
      by net pair within 0.5 mm. Report `drc_real`, `preexisting` and
      `unconnected_items` separately.
    - Exit codes: 0 graded clean, 1 router-attributable items, 2 not graded.
      "Not graded" (no `kicad-cli`, a failed import) never counts as a pass.
    - Strip the SES `(placement ...)` scope before `ImportSpecctraSES`.
    - **Measure copper narrower than its net class.** KiCad's DRC checks a
      track against the board minimum, never against its net class's width,
      so a 1.5 mm power net routed at 0.089 mm passes. That is a
      current-capacity (physics) violation under D8. Report, per engine:
      tracks and length narrower than their net class, the worst shortfall,
      and the five nets with the most under-width length. Take net → class
      width from the `(class ...)` scopes of the DSN KiCad exported (their
      `(rule (width …))`, in the DSN's unit). KiCad 10.0.5's Python returns
      an unwrapped object from `GetEffectiveNetClass()`, so it can't be used
      for this. In the head-to-head on `canary-pwr`: Freerouting 175 mm
      (short pin neck-downs; worst +5V_SHUNT at 0.5 mm against 1.5 mm); KRT
      at its default tier 672 mm (worst +12V_BUS at 0.089 mm against
      1.5 mm).
    - **Don't trust capped counts.** KiCad 10's DRC reported at most 199
      `track_width` items in the 2026-10-15 head-to-head. coldfire's
      Freerouting result had 305 tracks below the minimum width and reported
      199. interf_u's KRT result had 594 and reported 199. Clearance items
      weren't capped at 199 (424 reported). Count widths, via sizes and drills
      from the board's geometry, and treat any DRC count of exactly 199 as
      "≥ 199".
    - **DSN keepouts:** remove the keepouts that KiCad's DSN export makes
      from rule areas that ban only footprints or pads. The `.kicad_pcb`
      says which they are, by `(tracks allowed)(vias allowed)`. Otherwise
      the DSN-based engine is graded on a board with less room than the
      `.kicad_pcb`-based one. On `canary-pwr` that is the 26 × 26 mm fan
      inlet on B.Cu.
  - **Gotchas** (from the CANary KiCad notes and the head-to-head):
    - Never close, restart or kill a running KiCad, and never touch the
      Java Freerouting plugin.
    - Don't import the freerouting-rs plugin package in KiCad's Python.
    - `BOARD.GetTracks()` fails in 10.0.5; `BOARD.Tracks()` works.
    - Files copied from `Program Files` keep read-only modes; `chmod u+w`
      the staged copies.
    - `kicad-cli` takes rules from the `.kicad_pro`/`.kicad_dru` with the
      board's own base name. Stage each variant in its own directory under
      the original file name, or DRC silently uses default rules.
  - **Accept:**
    - On two demo boards and `canary-pwr`, the Java-routed session is graded
      and the summary JSON is written.
    - A session edited to put one via inside an SMD pad of another net
      reports a clearance violation that the unedited session doesn't.
    - A board whose author set `clearance` to `warning` still reports an
      injected 0.05 mm graze.
    - An untouched session on an input with one pre-existing edge item
      reports `preexisting=1, drc_real=0`.
    - A missing `kicad-cli` gives exit 2 and "not graded".
    - `pgrep -x kicad` (not `-f`, which also matches paths) shows the same
      KiCad GUI processes before and after. A Linux KiCad GUI may be
      running in WSL as well as the Windows one.
    - The originals' checksums are unchanged.
    - The scripts written for the 2026-10-15 head-to-head
      (`scratchpad/h2h/scripts/`, summarized in
      [X01](X01-krt-head-to-head.md)) are the starting point, not a separate
      tool.
  *Done 2026-10-15:* `tools/kicad-oracle/` (`grade.sh`, `grade-set.sh`,
  `kicad_steps.py` for pcbnew, `oracle_lib.py` pure Python, `README.md`,
  `tests/`) and `tools/bench/sets/kicad.txt` (16 bundled demos of the
  WSL-native KiCad 10.0.7 plus Dan's `canary-pwr` and `canary-hat`, all out of
  the repo). Built from the head-to-head's `prep.py`, `import_ses.py`,
  `strip_placement.py`, `classwidths.py`, `stats.py`, `summarize.py` and
  `run_board.sh`. Uses the WSL KiCad 10.0.7 (`kicad-cli` and `pcbnew` must
  report the same version, which goes into every summary). With the
  `a773a70ec` jar at 5 passes, optimizer off: `ecc83-pp` graded clean (exit 0,
  drc_real 0, 14 input unconnected items -> 0), `pic_programmer` exit 1
  (drc_real 2: copper against PCB text, which KiCad's DSN export leaves out),
  `canary-pwr` (copy of `~/code/canary/hardware/canary-pwr-rev1.0`) exit 1
  (drc_real 39, mostly `via_dangling`; 71 unconnected; 1 keepout removed, the
  fan inlet; 134 tracks / 80.2 mm narrower than their net class, worst
  `/Pi_Supply/LM_SW` 0.401 mm against 1.5 mm; 2 tracks below the 0.15 mm
  minimum), each with `summary.json` written. `tests/run-oracle-tests.sh
  <jar> <scratch>` prints `kicad-oracle tests passed`: a via on SMD pad
  `JP1-1` (net VCC) of `pic_programmer`, joined to net `/pic_sockets/VCC_PIC`,
  adds 2 new `shorting_items` (KiCad's copper-overlap class) that the unedited
  Java session doesn't have, exit 1; on `ecc83-pp` with the project's
  `clearance` set to `warning`, a via 0.05 mm inside the clearance of pad
  `P6-1` gives 2 new `clearance` items at severity `error` (unedited drc_real
  0); an empty session on a synthetic board with one pad across the edge gives
  `preexisting=1, drc_real=0`, exit 0; `KICAD_CLI=<missing>` gives exit 2 and
  `not graded`; `pgrep -x kicad` is empty before and after; the 49 original
  files' checksums are unchanged (`grade.sh` also checks its own originals and
  records `originals_unchanged`). `python3 -m unittest discover -s
  tools/kicad-oracle/tests -p 'test_*.py'`: 7 tests OK (keepout removal,
  class widths, placement strip, severity forcing, pre-existing matching,
  exit codes, 199 cap). Found on the way: KiCad's `ImportSpecctraSES` gives a
  via or wire that touches only a pad the pad's net, whatever net the session
  says, so a lone via in a pad of another net is invisible to KiCad's DRC; the
  test joins it to its own net's copper. The Windows 10.0.5 install is not
  wired up (needs `wslpath -w`); grading Rust-routed sessions waits for the
  router (A13/A14).
