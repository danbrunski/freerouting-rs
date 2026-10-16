# F02 — Parity and benchmark harness

Opened 2026-10-15. The verification ladder in
[port-completion.md](../port-completion.md) needs four tools that don't exist
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

- [ ] **F02.1 — One-command Java probes.**
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

- [ ] **F02.2 — `freerouting-dev ses-diff`.**
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

- [ ] **F02.3 — `freerouting-dev bench`.**
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

- [ ] **F02.4 — Determinism check.**
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

- [ ] **F02.5 — Board sets.**
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
  - `owner.txt`: out-of-repo paths for `canary-pwr` and its variants, skipped
    when missing
  - **Accept:**
    - Every in-repo path exists.
    - `tools/bench/README.md` explains each set and how it was chosen, with
      each board's connection count and size.
    - `freerouting-dev bench --set connection-parity.txt` (Java only)
      completes.

- [ ] **F02.6 — KiCad DRC as an independent oracle.**
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
