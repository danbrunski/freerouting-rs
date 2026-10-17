# Engine port: handover notes

The routing engine side of freerouting-rs: `crates/freerouting-geometry`,
`crates/freerouting-dsn`, `crates/freerouting` (engine library),
`crates/freerouting-cli`, the Java reference tooling in `tools/java-reference/`
and the reference data in `fixtures/reference/`.

Other handovers: [gui.md](gui.md) for `crates/freerouting-gui`,
[kicad-plugin.md](kicad-plugin.md) for `kicad-plugin/`. The plan and the module
map are in [../PORT_PLAN.md](../PORT_PLAN.md); deliberate behaviour changes from
Java are in [../DIVERGENCES.md](../DIVERGENCES.md).

## Who owns what

| Area | Owner |
| --- | --- |
| `crates/freerouting-geometry`, `crates/freerouting-dsn`, `crates/freerouting`, `crates/freerouting-cli`, `tools/java-reference/`, `fixtures/reference/` | engine sessions (this file) |
| `crates/freerouting-gui` | GUI sessions ([gui.md](gui.md)) |
| `kicad-plugin/` | KiCad plugin sessions ([kicad-plugin.md](kicad-plugin.md)) |

Everyone commits and pushes straight to `main`, no PRs. Pull with `--rebase`
before every push, keep each commit green (fmt, clippy, tests).

## The Java reference

The port follows Dan's Java repository, `danbrunski/freerouting`, and tracks its
latest `main` rather than a fixed upstream release. It is currently pinned to
`a773a70ec` (J01, 2026-10-15; before that `cda540f`), where routing is
deterministic: every cut-off inside the router is a work budget (`WorkLimit`,
search-tree nodes visited), not wall-clock time, and each one that fires logs a
`WORK_LIMIT_CUTOFF` line. Java commits after the pin and whether the port takes
them are in the Java side's ledger
(`git -C ../freerouting show main:docs/development/PORT_LEDGER.md`). When that
repository moves on (only in a J-series checklist):

1. Check what changed: `git log -p <old>..<new> -- src/main`, read through git,
   never the working tree.
2. Port the changes that touch already-ported code (geometry, DSN, board, rules,
   DRC) and note anything still to port for the router.
3. Regenerate the reference (below) from a detached worktree of the new pin into
   a scratch `REFERENCE_OUT_DIR`, then copy it over `fixtures/reference/`.
   Regenerate `crates/freerouting/tests/data/java_drc_digest.tsv` too
   (`java_drc_digest.py`), and update the pin in `PORT_PLAN.md`,
   `DIVERGENCES.md`, `tools/java-reference/README.md`, `run.sh`, this file and
   `.claude/agents/implementer.md`. The board probes
   (`tools/java-reference/probes/board/generate.sh`) need a rerun at the new pin.

Java is the regression baseline. On routes without a work-limit cut-off it is an
exact target (D4). A difference from Java is either a bug in the port or a
deliberate fix, and every deliberate one is listed in `DIVERGENCES.md`.

### Regenerating the reference

```sh
git -C ../freerouting worktree add --detach <scratch>/java-pin a773a70ec
tools/java-reference/run.sh <scratch>/java-pin load       # ~3 min
tools/java-reference/run.sh <scratch>/java-pin route      # bounds from route-bounds.tsv
```

No JDK 25 install is needed: the script uses a Java 25 runtime from the
`jdk4py` wheel and compiles with the Eclipse compiler. Gradle is only used to
resolve dependencies; Maven Central sometimes answers 429, and the script
retries. `route` runs the autorouter with the optimizer off, bounded by
`max_passes`/`max_items` per board (`tools/java-reference/route-bounds.tsv`);
its 30-minute timeout is only a safety stop, and a board it stops gets no
reference. `REFERENCE_SHARD=k/n` splits the set over parallel runs. `route`
resumes an interrupted run; `REFERENCE_REDO=1` reruns all.
Details in [tools/java-reference/README.md](../../tools/java-reference/README.md).

Besides `load` and `route`, six deterministic modes dump what the settings, writer and CLI work
(C01) compares against: `settings`, `rules`, `ses`, `dsn`, `argv` and `clirun`. One command
regenerates all of them (about 7 minutes) from a detached worktree of the pin:

```sh
tools/java-reference/regenerate-dumps.sh <pin-worktree>
```

`ses` imports `fixtures/reference/route/*.ses`, so **rerun it after every `route` regeneration**
(J01.5). `REFERENCE_FULL=1 REFERENCE_OUT_DIR=target/java-reference/full` dumps every board
outside the repo for the ignored full-set Rust tests. The modes, scenarios and the subset rule are
in the README.

`load` output is deterministic, and so is `route` output at a fixed JDK under
work bounds (J01.4: identical sessions idle and under load). Java's maze search
is reseeded with the ripup cost before each connection, the pass shuffle is
seeded, and the router's cut-offs count work. Each route JSON records its
`workLimitCutoffs`; the boards with none are the exact-parity set, the others
are compared on aggregates until the B series proves search-tree shape parity
(D5).

## What is done

| Stage | Where | Checked against Java |
| --- | --- | --- |
| Geometry (`geometry.planar`) | `freerouting-geometry` | Ported Java tests plus differential tests against the compiled Java classes; all match. |
| DSN/SES/rules files | `freerouting-dsn` | 160 of 161 fixture DSN files parse and round-trip (the other is an OrCAD binary Java rejects too); all SES and rules files round-trip. |
| Board model, rules, library, search trees, DSN → board | `freerouting` (`board`, `rules`, `library`, `io::specctra`) | `tests/reference_load.rs`: all 159 boards Java loads match on every compared `BoardStatistics` field. |
| `BoardStatistics` | `freerouting::scoring` | Same test. Router and optimizer scores still need `RouterSettings`. |
| Router settings model | `freerouting::settings` | One field table with Java's names, aliases and defaults; Java's merge and precedence; config file, environment, CLI-argument and `autoroute_settings` sources; validation. `tests/settings_*.rs` (Java unit tests ported; `settings_java_probe.rs` and `settings_text.rs` compare with Java 25 at the pin). The comparison with C01.1's dumps is `settings_reference.rs`. Does not apply settings to a board (C03). |
| DRC, incompletes, airlines, SES import | `freerouting::drc`, `io::specctra::ses_reader` | `tests/reference_load.rs` (connections and violations on every board) and `tests/reference_drc.rs` (the full Java DRC report, on each board as loaded and with Java's routed SES imported). Remaining differences are listed in those tests with reasons. |
| Session writer | `freerouting::io::specctra::ses_writer` (`write_ses`, `write_ses_to`, `session_from_board`), `freerouting_dsn::write::java_style` | Byte-identical to Java's `SesWriter` (C01.3's `ses` dumps) on the 19 subset boards, unrouted and with Java's routed session imported; `Issue029-hw48na`'s `library_out` lacks the via padstacks of its adjacent rules file (C04) and `Issue676`'s routed dump differs because Java imports none of its wires. `tests/ses_writer.rs`, `tests/ses_round_trip.rs`, `freerouting-dsn/tests/java_style_ses.rs` (116 Java route sessions round-trip to their own bytes). |
| Rules files | `freerouting::io::specctra::{rules_reader, rules_writer}` (`apply_rules_file`, `read_rules`, `rules_from_board`), `job::prepare` (discovery, second merge), `drc::runner` | `RulesReader.read` applied in file order through the DSN reader's own helpers. `tests/job_prepare_reference.rs` compares the six boards with an adjacent rules file in all four settings scenarios (clearance matrix, via infos, via rules, snap angle, padstacks, settings); `tests/reference_drc.rs` the DRC with `-dr` (C01.6 rows 7 and 8, digests from a Java run in `tests/data/java_drc_rules_digest.tsv`); `tests/rules_writer.rs` the written tree against all 164 Java `rules` dumps (5 listed differences). Java's format is lossy, see DIVERGENCES, "Rules files". |
| Design file writer | `freerouting::io::specctra::dsn_writer` (`pcb_from_board`, `write_dsn`, `write_dsn_to`) | Board to `Pcb` syntax tree (the plain layout of `Pcb::to_dsn`). Compared as parsed trees with Java's `DsnWriter` on the 19 boards of `fixtures/reference/dsn` (`tests/dsn_round_trip.rs`, every difference explained); the real check is the round trip on every loaded fixture: layers, outline, clearance matrix, areas, components, library, nets, net classes, via types and rules, traces and vias equal (`tests/dsn_writer.rs`), and the same statistics and DRC result (`tests/dsn_round_trip.rs`). Java's writer loses several things its reader needs (`docs/DIVERGENCES.md`, "Writing a design file"); `-do out.dsn` is registered by C07.4. |
| KiCad boards | `crates/freerouting-kicad` (the `.kicad_pcb` reader), `freerouting::io::{kicad, read_design}` | `read_design(path)` picks the reader by extension (`.dsn` or `.kicad_pcb`; a board reads its `.kicad_pro` and `.kicad_dru` too); the CLI (`-de board.kicad_pcb`), the DRC runner and the GUI session call it. The reader's board is compared with the board of KiCad 10.0.7's own DSN export of it, every difference listed with its reason (`tests/kicad_parity.rs`, 15 committed boards; `--ignored` runs the 57-board corpus); the frame against KiCad's session import is `tests/kicad_frame.rs`. See kicad-plugin.md, "Direct `.kicad_pcb` input". |
| `freerouting -de board.dsn [board.ses] -drc [report.json]` | `freerouting-cli` | Same JSON report as Java's DRC command. Routing isn't implemented yet. |
| `freerouting -de board.dsn -do out.ses [flags]` (the job flow) | `freerouting-cli` (`args`, `cli`, `log`, `help.txt`), `freerouting::job::{run, outputs, deadline, log}` | Java's flags (one parser for `GlobalSettings` and `CliSettings`), exit codes, outputs and log lines. `tests/args_java.rs` ports Java's argument tests and compares every `fixtures/reference/argv.json` entry; `tests/cli_outputs.rs` runs the binary against the `clirun` rows (SES byte-identical to Java's; `.dsn`, `.rules`, `.drc.json` written; `.frb`, `.scr`, `.json`, `.kicad_pcb`, `.xyz` refused before any work); `tests/settings_parity.rs` holds the KRT gates. Outputs are atomic and never 0 bytes, deadlines only stop work. `job::run::run_job` calls `freerouting::autoroute::pipeline::run_pipeline` (A13: fanout, batch passes with the board history, optimizer gate; `examples/route_board.rs` drives it) with a listening event sink that times the passes, rewrites the loop's pass lines to Java's, adds the stage summary, and writes the outputs from the final board; `tests/cli_route.rs` routes Issue413, J2 and DAC2020 bm01 (`-mp 2`: two pass lines, `compare.py`'s regex), `-inc 'Power\,Default'` end to end and the timeouts with routing. A router switched off still runs the fanout (A13, as Java's `RoutingPipeline`). `main.rs` only calls `freerouting_cli::cli::run`; the flow is `job::run::run_job`. Differences: DIVERGENCES, "Command line and job outputs". |

About 300 tests, all passing: `cargo test --workspace` takes about 26 s warm
(the workspace `dev` profile uses `opt-level = 1`; at `opt-level = 0` it took
181 s, mostly the reference comparisons). Overflow checks and debug assertions
stay on. For debugger stepping, build with `CARGO_PROFILE_DEV_OPT_LEVEL=0`.

### Loose ends from the DRC stage

- Not all Java DRC unit tests are ported yet (`src/test/java/app/freerouting/drc`).
- Copper-island (zone island) findings are computed with a different
  decomposition of the fill and are not compared with Java.
- `RoutingBoard::clear_changed_area` should pull-tighten like Java's
  `optChangedArea` (marked `TODO(router)`).
- `getRouterScore` / `getOptimizerScore` need `RouterSettings` (`TODO(router)`).

## What the GUI is waiting for

The GUI draws from the engine's public API and is about to add the ratsnest and
DRC panels. Both data sets are already exposed in `freerouting::drc`:

- `DesignRulesChecker::new(&board)` then `net_incompletes()` / `all_airlines()`:
  per net, each `AirLine` with both endpoint positions, the two `ItemId`s it
  connects and their layers. This is the ratsnest.
- `DesignRulesChecker::check()` → `DrcFindings`: `ClearanceViolation`s with the
  violation shape, the two items, the layer and expected vs actual clearance,
  plus unconnected items and zone islands. `generate_report` gives the Java JSON
  report.

`tests/drc.rs::drc_findings_are_drawable` exercises these on a fixture.

Engine API added for the desktop app (B08, 2026-10-16), all on `BasicBoard` (so on
`RoutingBoard` through `Deref`) unless noted:
- `item_geometry(id)`: the drawable shapes of an item in board units (`board::item_geometry`);
- `item_info(id)`: the fields of the info window in Java's order (`board::item_info`);
- `drc::route_stubs(&board)`: the route stubs list in the window's order;
- `detach_item_for_move`, `set_conduction_is_filled`, `unfill_conduction_areas`,
  `non_45_degree_trace_count`, `net_items` (`board::gui_ops`), and a public `insert_item`;
- the rule dialogs' edits (`rules::edit`: clearance values, classes and prune, net classes, via
  infos and rules), `ClearanceMatrix::get_row`/`row`, `BoardLibrary::is_used`, `Nets::new_net`.

If the GUI needs something else from the engine, add it to the engine crate. Don't
put board logic in the GUI.

## Next stages

1. **Autorouter** (~18k Java lines, `autoroute.*`): expansion rooms and doors,
   drill pages, maze search, path insertion, trace shoving, ripup, fanout, the
   batch pipeline. Done when small fixtures route to completion. Then compare
   completion, vias, length, score and time with `fixtures/reference/route`.
   Use a fixed seed everywhere (`freerouting_geometry::jmath` has a bit-exact
   `java.util.Random`), and take the router changes in Dan's commits since
   `fc35bbd0` (`git log fc35bbd0..a773a70ec -- src/main/java/app/freerouting/autoroute`).
2. **Optimizer** (`board.optimize`, `BatchOptimizer`): pull-tight, shove, via
   optimization.
3. **Interactive editing** (`gui.interactive` logic, as engine code the GUI calls).
4. **CLI routing parity**, performance, packaging.

### Watch list from comparing Dan's Java changes

A one-run comparison of the old (`fc4bd20`) and new (`cda540f`) Java router on
16 boards, 45 s each: the new version left fewer connections unrouted on 13,
was slightly worse on `Issue066-Project_GP8B` (486 → 500) and
`Issue145-smoothieboard` (350 → 385), and was the same on `Issue093` while
finishing in 6 s instead of 17 s. Dan accepted that as is; those two boards are
worth watching when the router is ported.

## Working practices that paid off

- Port one Java package at a time and compare it with the Java reference on all
  fixtures before moving on.
- Keep Java's iteration order where it affects results (no hash-order
  iteration); Java `HashMap` order is simulated where needed.
- Every expected mismatch with Java lives in the test that would otherwise
  fail, with the reason next to it.
- CI is prepared in [ci-workflow.yml](ci-workflow.yml) (fmt, `clippy -D
  warnings` and `cargo test --workspace` on Linux, Windows and macOS, the same
  three commands as the local gate, with read-only permissions, a 30-minute
  timeout and `rust-cache`). It is disabled at Dan's request (2026-10-15):
  it stays in `docs/development/` so GitHub never runs it. To enable it, move
  it to `.github/workflows/ci.yml`; pushing that directory needs Dan's own git
  credentials (the cloud sessions' GitHub token can't).

## Parity and benchmark tools (F02)

Engine sessions own these too. Details in each tool's README or header.

| Tool | What it does |
| --- | --- |
| `tools/java-reference/probe.sh <pin worktree> <Probe.java> [args]` | Compiles and runs one Java probe against the pin's classes (builds them through `run.sh` if missing; refuses a class cache from another commit). `probes/JavaRandomProbe.java`, `probes/SesImportStats.java` (Java's statistics after importing a SES). |
| `crates/freerouting-dev`, binary `freerouting-dev` | Dev-only (`publish = false`). `ses-diff board.dsn a.ses b.ses [--json f]`: structural session comparison (wires and vias per net, in board coordinates) plus the statistics after importing each. `bench --set <file> --java <jar> [--rust <bin>] --runs N --out <dir>`: times both engines on a quiet machine (refuses above load 1.0 by default), optimizer off unless `--optimize`; writes `bench.json` and `bench.md`. See [tools/bench/README.md](../../tools/bench/README.md). |
| `tools/determinism-check.sh --out <dir> [--ignore RE] -- <cmd with {OUT}>` | Runs a command idle and beside one busy loop per core, compares the outputs byte for byte. Test: `tools/test-determinism-check.sh`. |
| `tools/bench/sets/*.txt` | Board sets: `connection-parity` (rung 2), `benchmark` (rungs 3 and 5), `owner` (`canary-pwr`), `kicad` (projects for the oracle). |
| `tools/kicad-oracle/grade.sh <project> -- <engine with {DSN} {SES}>` | Grades a routed session with KiCad's own DRC (WSL KiCad 10.0.7) under the board's original rules, minus what the unrouted input reports, plus copper narrower than its net class. See [tools/kicad-oracle/README.md](../../tools/kicad-oracle/README.md). |

Two findings from building them:
- Java's route statistics in `fixtures/reference/route/*.json` are taken on
  the routed board in memory. Importing the SES Java wrote gives the same
  incomplete count, violations, vias and length, but bends can differ: on
  `Issue508-DAC2020_bm01`, 705 in the JSON and 704 after an import (841 and 839
  with the `cda540f` session), in Java
  (`probes/SesImportStats.java`) as in Rust.
- KiCad's `ImportSpecctraSES` gives a via or wire that touches only a pad the
  pad's net, whatever net the session names, so KiCad's DRC can't see a lone
  via in a pad of another net.
