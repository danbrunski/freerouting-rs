# P00 — Performance

Opened 2026-10-15. These checklists make the port faster than Java without
changing what it routes. The rules, the expected hot spots and the stages are
in [performance.md](../performance.md). In short: data-oriented design while
porting (D1), real measurements at parity, then optimizations that keep
results byte-identical. One deliberate divergence (P04) frees the search
tree.

## Task index

| Task | Outcome | Size / risk | Prerequisite |
|---|---|---|---|
| [P01](P01-profiling-and-baselines.md) | Profiler recipes, a per-stage stats output, microbenchmarks with identical work counts, the first Rust-against-Java baseline, the release speed target | M / tooling | F02.3 (`bench`); the benchmark rows need the A-series board-parity gate |
| [P02](P02-parity-safe-optimizations.md) | Speed-ups that leave every routed result byte-identical, starting with an incremental-against-rebuild audit | M–L / correctness guarded by `ses-diff` | P01; the A-series board-parity gate |
| [P03](P03-parallelism.md) | Parallel work with an ordered merge where results can't depend on order | M / determinism | P01, O06 |
| [P04](P04-order-independent-rooms.md) | Room completion independent of tree visit order (a measured divergence), then a faster search tree | L / changes routes; Q-series rules | P01, P02, the A-series board-parity gate |
| [P05](P05-simd-and-allocator.md) | SIMD on hot data-parallel loops shown by P01; an allocator trial | M / portability | P01 |

## Order

P01 comes first. Its tooling items can start as soon as the A series has a
routing CLI; the stats output can be built into the A series' search core
(the sinks in A04). P02 and P05 follow P01's profiles. P03 can run alongside
P02. P04 is last, because it gives up exact parity with Java on purpose.

## Series rules

- **Every timing:**
  - release build, on a quiet machine: `bench` refuses to time a run while
    the 1-minute load average is above 1.0
  - several runs, reporting the median and the spread
  - before and after numbers in the checklist's done note
- **Work counts and instruction counts must not move** for a change that
  claims to keep results identical. Work counts are D5's work units;
  instruction counts come from callgrind. If they do move, explain
  why in the done note.
- **A rejected optimization leaves no code behind.** Its numbers stay in the
  done note.
- **A new dependency** (an allocator, a SIMD crate, a profiler crate) is a
  `NEW DEP` line, measured before it is accepted.

## Open questions for Dan

- **The release speed target.** port-completion.md proposes at least 3×
  lower median wall time than Java at equal or better results. P01.5
  measures the first parity build and recommends the final number.
