# P03 — Parallelism with an ordered merge

Opened 2026-10-15. The router's core is sequential by design. Each connection
changes the board the next one sees, and the search tree's shape decides
routes (D5). So the parallelism here is limited to work whose result can't
depend on order or thread count:
- independent analyses: DRC, statistics, airlines;
- candidate evaluation on copies (the optimizer);
- whole boards (the bench runner).
The DRC already runs its clearance and island checks on all cores with an
ordered merge.

Prerequisites: P01; O06 (the batch optimizer) for P03.2.

## What exists and what this adds

| Exists | State |
|---|---|
| DRC | Clearance violations and island fills computed on all cores, merged in item order (DIVERGENCES, "Design rule check") |
| O06 | The batch optimizer evaluates candidates on copy-on-write copies with an ordered merge |
| `bench` (F02.3) | Runs boards one after another |

## Decisions made here

- **Not allowed without a recorded divergence:**
  - routing two connections at once;
  - speculative searches for the next connection;
  - reusing rooms or drills across connections.
  Each of these changes the tree topology or the work counts (A-series
  research §6.4).
- **One thread pool.** Use one `rayon` pool (a `NEW DEP` for the engine
  crate if it isn't one already; check `Cargo.toml`). It is sized by
  `router.max_threads`, which defaults to the core count. Threads=1 must give
  the same bytes.

## File ownership

| Item | New files | May edit |
|---|---|---|
| P03.1 | — | `crates/freerouting/src/scoring/`, `crates/freerouting/src/drc/` |
| P03.2 | — | `crates/freerouting/src/optimize/` (O06's module) |
| P03.3 | — | `crates/freerouting-dev/src/bench.rs` |

## Checklist

- [ ] **P03.1 — Parallel statistics and analyses.**
  Run the per-pass statistics, airline computation and DRC inside the
  pipeline on the pool, with merges in item order.
  - **Accept:**
    - Threads=1 and threads=8 give byte-identical SES and statistics JSON on
      `benchmark`.
    - `tools/determinism-check.sh` passes.
    - The per-pass time on the 3 largest boards is in the note.

- [ ] **P03.2 — Tune the optimizer's parallel evaluation.**
  O06 makes candidate evaluation parallel and deterministic; this item
  measures and tunes it. Look at chunk size, copy cost (with P02.4) and the
  pool size against the core count.
  - **Accept:**
    - Threads=1 against threads=N byte-identical on `benchmark` with the
      optimizer on.
    - Optimizer wall time against thread count (1, 2, 4, 8) on the 3
      largest boards is in the note.
    - The default setting is the best measured one.

- [ ] **P03.3 — Boards in parallel in `bench`.**
  `bench --jobs N` routes N boards at once, each pinned to a share of the
  cores.
  - Timing runs keep `--jobs 1`; the quiet-machine rule still applies.
  - `--jobs` is for correctness and parity sweeps.
  - **Accept:**
    - A parity sweep of `connection-parity` with `--jobs 4` gives the same
      `ses-diff` results as `--jobs 1`.
    - The sweep's wall time is in the note.
