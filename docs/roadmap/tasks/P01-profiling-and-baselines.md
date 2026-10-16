# P01 — Profiling and baselines

Opened 2026-10-15. Nothing about the router's speed is known yet, and wall
time on this machine is noisy (8 cores shared with other sessions). This
checklist builds the instruments first:
- profiler recipes that work here;
- a stats output that counts work per stage;
- microbenchmarks whose work count is fixed;
- the first Rust-against-Java baseline.
Every later P item measures with these.

## What exists and what this adds

| Exists | State |
|---|---|
| `valgrind --tool=callgrind` | Installed, works on the release CLI: 670 M instructions for a release DRC run on `Issue178-KeebMaker_Sofle_Choc` (2026-10-15) |
| `perf_event_paranoid` | 2, so user-space sampling of your own processes is allowed. No `perf` binary for the WSL kernel |
| `freerouting-dev bench` (F02.3) | Times Java and Rust on board sets with a quiet-machine check |
| The A-series search core | Generic over sinks (A04), so counters compile out of the default build |

What this adds:
- documented profiler recipes;
- `--stats json` on the CLI;
- a microbenchmark harness;
- callgrind instruction-count baselines;
- the first parity baseline against Java, and a recommended speed target.

## Decisions made here

- **Two clocks.** Wall time on the `benchmark` set is the number that counts.
  Callgrind instruction counts are the steady second signal for
  regressions. Both are recorded.
- **No profiler dependency in the product.** Profiling uses external tools
  (callgrind; `samply` installed with `cargo install`, which is a developer
  tool, not a crate dependency). The stats output uses the A04 sink, so it
  costs nothing when it is off.
- **Microbenchmarks fix the work, not the time.** Each microbenchmark asserts
  that its work count is identical across builds, and only the time may
  change. This is KiCadRoutingTools' `microbench.py` rule ("the printed
  iterations must be IDENTICAL across binaries").

## File ownership

| Item | New files | May edit |
|---|---|---|
| P01.1 | `docs/development/profiling.md` | — |
| P01.2 | `crates/freerouting-cli/src/stats.rs` | `crates/freerouting-cli/src/main.rs` (one flag), the A04 sink module (append a stats sink) |
| P01.3 | `crates/freerouting/benches/` or `crates/freerouting-dev/src/microbench.rs` | `crates/freerouting-dev/src/main.rs` (append a subcommand) |
| P01.4 | `tools/bench/callgrind-baseline.json`, `tools/bench/callgrind.sh` | — |
| P01.5 | `docs/roadmap/performance.md` (baseline table) | `docs/roadmap/port-completion.md` (the target line, after Dan agrees) |

## Checklist

- [ ] **P01.1 — Profiler recipes that work on this machine.**
  Write `docs/development/profiling.md` covering:
  - callgrind on a release build
    (`valgrind --tool=callgrind --callgrind-out-file=… ./target/release/freerouting …`,
    then `callgrind_annotate`). Set `debug = 1` or `"line-tables-only"` in a
    `[profile.profiling]` that inherits `release`, for symbols.
  - a sampling profiler: try `samply record` (installed with
    `cargo install samply`). If WSL blocks it, say so and why.
  - how to read inlined Rust frames.
  - **Accept:** each recipe was run on `Issue178-KeebMaker_Sofle_Choc`'s DRC
    (until the router exists), and the doc shows its top five functions.
    `cargo build --profile profiling` works and is documented. A recipe that
    doesn't work here is listed with the error.

- [ ] **P01.2 — `--stats json`.**
  A CLI flag that writes per-stage counts and times to a JSON file through the
  A04 stats sink:
  - board load, DRC, fanout, each autorouter pass, each optimizer pass;
  - per stage: wall time, connections attempted and routed, rooms completed,
    doors expanded, maze queue pops and pushes, drop-on-equal skips, tree
    nodes visited (D5 units), overlap queries, inserts and ripups;
  - per-unit costs derived from these (µs per connection, per room, per
    overlap query).
  - **Accept:**
    - On 3 fixtures, routing with and without `--stats` gives byte-identical
      SES.
    - The work counts are identical across two runs.
    - Wall time with stats off is within noise of a build without the sink
      (median of 5).

- [ ] **P01.3 — Microbenchmarks with fixed work.**
  A harness (`freerouting-dev microbench`, or `cargo bench` with a
  hand-rolled timer; criterion is a `NEW DEP`) for the expected hot kernels:
  - overlap queries on a loaded board's default tree;
  - `complete_shape` (45°) for a fixed room list;
  - one maze search for a fixed connection;
  - one pull-tight of a fixed trace;
  - the DRC clearance pass.
  Each reports the minimum time over 9 runs and its work count.
  - **Accept:**
    - Every kernel prints a work count that a test asserts is unchanged.
    - Running the harness twice gives identical work counts and minimum
      times within 5 %.

- [ ] **P01.4 — Callgrind instruction-count baselines.**
  `tools/bench/callgrind.sh` runs each microbenchmark and a small routing run
  (3 boards from `connection-parity`) under callgrind, and writes the
  instruction counts to `tools/bench/callgrind-baseline.json`. A
  `--compare` mode reports each count against the baseline in percent.
  - **Accept:**
    - Two runs of the same build give identical counts.
    - A deliberately slower build (an extra allocation in the overlap-query
      loop) is reported with its percentage on the right kernel.

- [ ] **P01.5 — The first parity baseline against Java.**
  After the A-series board-parity gate passes, run `bench` on the `benchmark`
  set: Java (the pinned jar, optimizer off) against Rust, 3 runs each,
  quiet machine. Do the same with the optimizer on, once O06 lands. Put the
  table in performance.md:
  - per-board median wall time and spread
  - the ratio Java/Rust
  - routing results equal (by `ses-diff`) or the known reason they aren't
  Recommend the release target from these numbers.
  - **Accept:**
    - The table covers every `benchmark` board.
    - The recommendation names a median ratio and the slowest boards.
    - Dan's decision on the target is recorded in port-completion.md "What
      complete means". Until then, this item stays partial with "needs
      Dan".
