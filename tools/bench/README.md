# Benchmarks and board sets

`freerouting-dev bench` routes a set of boards with the Java and the Rust
engine and times them on a quiet machine. `freerouting-dev ses-diff` compares
two sessions for one board. Both live in the dev-only crate
`crates/freerouting-dev`, so the user-facing `freerouting` command line stays
Java-compatible.

## `freerouting-dev bench`

```sh
cargo build --release -p freerouting-dev
target/release/freerouting-dev bench --set tools/bench/sets/benchmark.txt \
  --java ~/code/freerouting/logs/jars/freerouting-a773a70ec.jar \
  [--rust target/release/freerouting] --runs 3 --out <scratch>/bench
```

- **Engines.** `--java <jar>` runs `java -jar <jar>` (Java 25 or newer:
  `java -version` is checked first; `--java-bin` picks another `java`).
  `--rust <bin>` runs the Rust CLI with the same arguments; while it answers
  exit 2 without writing a SES (it doesn't route yet), the Rust rows say
  `skipped: the Rust CLI does not route yet` and nothing is timed for it.
- **Command line.** Each run is
  `-de <board.dsn> -do <run.ses> --gui.enabled=false
  --router.autorouter.max_passes=<n> [--router.autorouter.max_items=<m>]
  --router.optimizer.enabled=false`. `--optimize` drops the last flag. The
  optimizer is off by default because Java's optimizer still depends on
  thread scheduling (J01); compare optimizer results on aggregates only.
- **Bounds.** Per-board `max_passes` and `max_items` come from
  `tools/java-reference/route-bounds.tsv` (J01.3) when it exists, or from
  `--bounds <tsv>`: a tab-separated file with a header row naming `board`,
  `max_passes` and optionally `max_items`; `board` is the file stem or the
  path as written in the set. Boards without a row use `--max-passes`
  (default 5).
- **Quiet machine or no number.** Before every run the 1-minute load
  average must be at most `--max-load` (default 1.0). The runner waits up to
  `--load-wait` seconds (default 120) for it, then refuses with exit 3 and
  `refusing to time: the 1-minute load average is ...`, without timing
  anything. If the machine gets busy in the middle, the runs so far are
  written and the report says `Stopped early`. The load before and after each
  run is recorded with its wall time. `--max-load off` times regardless.
  Never benchmark while builds or tests run.
- **Safety stop.** `--timeout` (seconds, default 1800) kills a run that takes
  longer; it is recorded as `timeout`. It only stops work.
- **Measurements.** Every SES is imported into the board and measured with
  `ses-diff`'s statistics: incomplete connections, clearance violations,
  vias, trace length in mm and bends. Java's `WORK_LIMIT_CUTOFF` log lines
  (from `a773a70ec` on) are counted per run.
- **Rip statistics (Q05.1).** `--rip-stats` adds
  `--router.special.diagnostics=rip-stats --result-json <run>.result.json`
  to every Rust run and keeps the manifest's `freerouting-rip-stats/1`
  report, without its per-net list, under `rip_stats` in the run's record
  in `bench.json` (totals, the top five nets by torn length with their
  shares, the passes). It changes no route but adds work, so don't compare
  its wall times with plain runs.
- **Output.** `<out>/bench.json` has every run (wall seconds, load before and
  after, exit status, cut-offs, statistics) plus the median and spread per
  board and engine; `<out>/bench.md` is a table with one row per board and
  engine: median wall time, spread (max - min), the highest load before a
  run, the statistics (a range when runs disagree) and the exit statuses.
  Each board's SES files and logs are in `<out>/<board>/`.
- **Exit status.** 0 when it ran, 3 when it refused or stopped because the
  machine was busy, 2 on an error.

## `freerouting-dev ses-diff`

```sh
target/release/freerouting-dev ses-diff board.dsn a.ses b.ses [--json out.json]
```

Loads the board, imports each session into its own copy and compares them
net by net: wires as multisets of (layer, half width, point list) and vias as
multisets of (padstack, position), in board coordinates converted exactly as
the session importer converts them. Text order, number format (`1000` vs
`1000.0`) and the direction a path is written in don't count. It prints the
first 10 differing nets with their differing wires and vias (positions in the
first session's units), each net's incomplete count before and after, and the
statistics of both imports. Exit 0 when equivalent, 1 when they differ, 2 on
an error.

## Board sets

`sets/*.txt` list one board path per line. `#` starts a comment. Relative
paths are relative to the repository root (the nearest ancestor of the set
file with a `Cargo.lock`), `~/` is the home directory, and a file that is
missing is skipped with a note, so sets can name owner boards outside the
repository. The counts below are `connections.maximum_count`,
`items.pin_count`, `layers` and `nets.total_count` from
`fixtures/reference/load/<board>.json` (Java at `cda540f`).

- **`connection-parity.txt`**, for rung 2 of the verification ladder: the ten
  smallest fixtures with at least two connections, taken in order of
  connection count. Copies of the same board under another name
  (`min_fr_test_no_quotes`, `corney_island_wireless`,
  `testProjectFromFreeroutingBugTest01`, `Issue270-non-ansi_bracket`,
  `TestSensel-KiCad6`) are left out.
- **`benchmark.txt`**, for rungs 3 and 5: 16 boards spanning 9 to 1125
  connections, 2 to 4 copper layers, with and without inner planes. It holds
  the four inner-plane boards from the Java fork's HANDOVER
  (`Issue145-smoothieboard`, `Issue753-CPU-85_r104`,
  `Issue555-CNH_Functional_Tester_1`, `Issue269-z10_module`), the two
  watch-list boards from `docs/development/engine.md`
  (`Issue066-Project_GP8B`, `Issue145-smoothieboard`), four KiCad demo boards
  (ecc83, sonde, interf_u, coldfire, which the KiCad oracle can also grade),
  four DAC2020 benchmarks, and the largest boards that Java loads quickly.
- **`owner.txt`**: Dan's boards. The `canary-pwr` variants are in the
  repository since 2026-10-15 (`fixtures/canary-pwr/README.md`); further
  owner boards are listed by out-of-repo path and skipped when missing.

### connection-parity

| board | connections | pins | layers (signal/total) | nets | DSN KB |
|---|---|---|---|---|---|
| `Issue269-min_fr_test/min_fr_test` | 3 | 4 | 3/4 | 2 | 2 |
| `Issue368-CorneyIslandWireless_input_design` | 3 | 8 | 2/2 | 5 | 3 |
| `Issue110-testProjectFromFreeroutingBugTest` | 4 | 24 | 2/2 | 20 | 8 |
| `Issue143-rpi_splitter` | 5 | 24 | 2/2 | 5 | 4 |
| `Issue413-test` | 5 | 10 | 2/2 | 5 | 8 |
| `Issue508-SMD-routing-issue-demo` | 6 | 14 | 2/2 | 6 | 2 |
| `Issue069-TestSensel/TestSensel` | 8 | 12 | 2/2 | 4 | 6 |
| `Issue269-NoViasOnPowerPlanes/Issue269-NoViasOnPowerPlanes` | 9 | 21 | 4/4 | 12 | 19 |
| `Issue433-my-board` | 9 | 32 | 2/2 | 24 | 8 |
| `PCBench-retroreflectors-CONGFLOCK` | 10 | 17 | 2/2 | 5 | 4 |

### benchmark

| board | connections | pins | layers (signal/total) | nets | DSN KB |
|---|---|---|---|---|---|
| `Issue433-my-board` | 9 | 32 | 2/2 | 24 | 8 |
| `Issue649-kicad_ecc83-pp_input_board_v1` | 21 | 33 | 2/2 | 13 | 34 |
| `Issue508-DAC2020_bm02` | 34 | 75 | 2/2 | 34 | 78 |
| `Issue690-sonde_xilinx` | 67 | 108 | 2/2 | 42 | 49 |
| `Issue508-DAC2020_bm07` | 86 | 140 | 2/2 | 52 | 14 |
| `Issue508-DAC2020_bm01` | 195 | 319 | 2/2 | 99 | 30 |
| `Issue508-DAC2020_bm10` | 199 | 312 | 4/4 | 63 | 31 |
| `Issue093-interf_u` | 201 | 379 | 2/2 | 173 | 145 |
| `Issue555-CNH_Functional_Tester_1` | 203 | 966 | 2/4 | 764 | 160 |
| `Issue269-z10_module` | 237 | 324 | 2/4 | 83 | 166 |
| `Issue690-kit-dev-coldfire-xilinx_5213` | 541 | 825 | 4/4 | 278 | 403 |
| `Issue753-CPU-85_r104` | 569 | 774 | 2/4 | 208 | 148 |
| `Issue066-Project_GP8B` | 759 | 1032 | 4/4 | 266 | 108 |
| `Issue145-smoothieboard` | 793 | 1055 | 3/4 | 245 | 129 |
| `Issue219-LogicBoard_smt` | 993 | 1360 | 4/4 | 343 | 105 |
| `Issue420-contribution-board` | 1125 | 1536 | 2/2 | 411 | 171 |

### owner

| board | connections | pins | layers (signal/total) | nets | DSN KB |
|---|---|---|---|---|---|
| `canary-pwr/canary-pwr-pours1` | 313 | 443 | 2/4 | 128 | 140 |
| `canary-pwr/canary-pwr-pours2` | 313 | 443 | 2/4 | 128 | 140 |
| `canary-pwr/canary-pwr-pours3` | 313 | 443 | 2/4 | 128 | 140 |
| `canary-pwr/canary-pwr-pours-as-planes` | 322 | 443 | 2/4 | 128 | 134 |
| `canary-pwr/canary-pwr-2026-10-15-1747-unpoured` | 312 | 443 | 2/4 | 128 | 212 |
| `~/code/freerouting/logs/canary-pwr/canary-pwr-2026-10-15-1747-stale.dsn` (out of the repository) | | | | | |


## Profiling and instruction-count baselines (P01)

Wall time is the number that counts, but it needs a quiet machine. For a steady
signal on a busy one there are callgrind instruction counts:

- `freerouting-dev microbench board.dsn` times five hot kernels (overlap
  queries, room completion, one maze search, a pull-tight, the DRC clearance
  pass) with a fixed amount of work and prints the work counts, which must not
  change between builds that keep results identical;
- `callgrind.sh` counts the instructions of each kernel and of three routing
  runs and compares them with `callgrind-baseline.json` in percent
  (`--compare`);
- `callgrind_top.py` sums a callgrind profile per function;
- `freerouting --stats json` writes work counts and times per stage.

All of them are described in
[docs/development/profiling.md](../../docs/development/profiling.md).

## `freerouting-dev gate`, `gate collect` and `failed-vs-human` (F03)

`bench` and the KiCad oracle produce per-board, per-engine numbers;
`gate collect` turns the `bench.json` of each arm (and, where there is one,
the oracle's `grade-set.sh` output directory) into a records file, and
`gate` compares it with a baseline and applies the pass rule: at least 3
distinct boards, improve on N-1, regress on none. See
[tools/ab/README.md](../ab/README.md) for the preregistration that has to be
committed before the first number, and the module docs of
`crates/freerouting-dev/src/gate.rs` for the problem classes (INVERTED, DRIFT,
ORPHAN, MISSING, NEW ROW, MALFORMED, UNGRADED, HARD, REFUSED).

`gate collect` adds a `via_in_pad` count to every arm by importing the saved
SES (`<out>/<board>/<engine>-run1.ses`) and counting the vias whose copper
overlaps an SMD pad, minus those already in the input. A via in a pad is a
hard failure in scoring, not a quality note.

`freerouting-dev failed-vs-human board.dsn engine.ses human.dsn` lists every
connection an engine leaves unrouted next to what the human-routed original
did (layers, vias, widths, clearance headroom), tagged `router gap`, `no human
route` or `infeasible` (the Q03 diagnostic). `tools/corpus/prep.sh` writes the
`<id>.human.dsn` it needs.

### kicad-corpus

`sets/kicad-corpus.txt` lists the 30 prepared boards of the out-of-repo corpus
(`tools/corpus/`, F03.3), at `target/corpus/prepped/<id>.dsn`. Fetch and prepare
them first (`tools/corpus/fetch.sh`, `tools/corpus/prep.sh`); missing boards are
skipped with a note. Use `target/corpus/grade-set.txt` with
`tools/kicad-oracle/grade-set.sh` to grade the same boards with KiCad's DRC.
