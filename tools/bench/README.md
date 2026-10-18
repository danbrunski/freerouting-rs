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
- **Boards in parallel (P03.3).** `--jobs N` routes N boards at once, for
  correctness and parity sweeps only. Each run gets
  `--router.autorouter.max_threads=S --router.optimizer.max_threads=S` with
  `S = max(1, cores / N)` (its share of the cores; neither setting changes a
  route), the load is not checked, and the report says the wall times are
  not timings. The boards are handed out in set order and listed in set
  order, so `bench.json` and `bench.md` read as with `--jobs 1`. Timing runs
  keep the default `--jobs 1` and the quiet-machine rule.
- **Safety stop.** `--timeout` (seconds, default 1800) kills a run that takes
  longer; it is recorded as `timeout`. It only stops work.
- **Measurements.** Every SES is imported into the board and measured with
  `ses-diff`'s statistics: incomplete connections, clearance violations,
  vias, trace length in mm and bends. Java's `WORK_LIMIT_CUTOFF` log lines
  (from `a773a70ec` on) are counted per run.
- **Tidiness (Q15.2).** Every measured SES is also scored by
  `freerouting-dev tidy`'s rules (below) and kept as the run's `tidiness`
  object in `bench.json`; `bench.md` gets a second table with the connections
  scored and `median / p90` of each measure. A run without a score (the SES
  did not import) has no `tidiness` member.
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

## `freerouting-dev tidy`: how tidy a route is (Q15)

```sh
target/release/freerouting-dev tidy board.dsn routed.ses|routed.kicad_pcb \
  [--json out.json|-] [--connections]
```

Scores the routed copper of a board with five measures per **connection**
(a pair of pins joined by routed copper), then reports the **median** and
**90th percentile** over the board's connections. It reads a session (this
port's, Java's, KiCad's) or a `.kicad_pcb` (KiCadRoutingTools writes a board,
not a session), so one tool scores every router. Nothing is judged by eye and
nothing is a keep rule: the numbers are for reports and for X13's comparison.

| measure | definition |
|---|---|
| `detour` | length of the copper path over the distance between the two pin centres; 1 is a straight track |
| `bends_per_10mm` | direction changes of more than 10 degrees on the path, per 10 mm of copper |
| `acute` | bends sharper than 90 degrees (a turn of more than 90) on the path; a right angle is not acute |
| `jogs` | segments between two bends that are shorter than 2 track widths |
| `vias` | vias on the path, plus the through-hole pins it changes layer in |

- **Connections.** For each connected piece of copper the connections are the
  edges of a minimum spanning tree of its pins, the distance between two pins
  being the length of the shortest copper path between them. A two-pin net is
  one connection; a Y of three pins is its two shorter legs. Copper that
  reaches no second pin, pins joined only through a plane or pour, and pairs
  at one point (a via in a pad joining two stacked pads) make no connection
  (`skipped` counts the zero-length pairs). Details and tie-breaks in
  `crates/freerouting-dev/src/tidy_copper.rs`.
- **Bends, acute corners, jogs.** `tidy_geometry.rs`. 10 degrees is below any
  corner a router makes on purpose (22.5 and 45 degrees are the smallest) and
  above the 5.7 degrees between two chords of an approximated arc, so a smooth
  arc is not a row of bends. Vias and pads end a run: the turn where a path
  changes layer is not a bend.
- **Percentiles.** The median of an even count is the mean of the two middle
  values; the 90th percentile is the value at position `ceil(0.9 n)` of the
  sorted list.
- **`.kicad_pcb` input.** It must be the board the DSN was exported from: the
  reader's frame is KiCad's DSN frame (um, y negated). Segments, arcs and vias
  are read; a via whose padstack the DSN lacks takes the DSN's first via type
  (reported on stderr). Zones are ignored.
- **All copper counts**, including wiring the DSN already holds.

### The record (`freerouting-tidy-1`)

`--json` writes (and bench records embed, without `format` and
`connection_list`):

```json
{
  "format": "freerouting-tidy-1",
  "connections": 120,
  "skipped": 0,
  "nets": 87,
  "bend_degrees": 10.0000,
  "jog_widths": 2.0000,
  "measures": {
    "detour":         { "median": 1.0840, "p90": 1.6120 },
    "bends_per_10mm": { "median": 1.1000, "p90": 3.0000 },
    "acute":          { "median": 0.0000, "p90": 0.0000 },
    "jogs":           { "median": 0.0000, "p90": 1.0000 },
    "vias":           { "median": 0.0000, "p90": 2.0000 }
  }
}
```

Numbers have four decimals. A measure over no connections is left out.
`--connections` adds `connection_list`: one object per connection (`net`,
`from`, `to` as `component-pin`, `length_mm`, `straight_mm`, `detour`,
`bends`, `bends_per_10mm`, `acute`, `jogs`, `vias`).

### Where the measures appear

- `bench.json`: the run's `tidiness` object (above); `bench.md`: a table.
- `freerouting-dev gate collect`: the arm values `tidy_<measure>_median` and
  `tidy_<measure>_p90` (`tidy_detour_median`, `tidy_bends_per_10mm_p90`, ...)
  from the bench run's `tidiness`, or the oracle summary's when the bench has
  none. The gate prints them with its row lines (it compares the arms), but
  they are `report` metrics: not judged, not kept in a baseline.
- `tools/kicad-oracle/grade.sh` and `tools/krt-h2h/grade_routed.sh`: the
  `tidiness` object of `summary.json` (needs a built `freerouting-dev`).

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
- **`owner.txt`**: the owner's boards. The `canary-pwr` variants are in the
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
