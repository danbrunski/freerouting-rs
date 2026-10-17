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
| [P01](archive/P01-profiling-and-baselines.md) ✅ | Profiler recipes, a per-stage stats output, microbenchmarks with identical work counts, the first Rust-against-Java baseline, the release speed target | M / tooling | F02.3 (`bench`); the benchmark rows need the A-series board-parity gate |
| [P02](archive/P02-parity-safe-optimizations.md) ✅ | Speed-ups that leave every routed result byte-identical, starting with an incremental-against-rebuild audit | M–L / correctness guarded by `ses-diff` | P01; the A-series board-parity gate |
| [P03](archive/P03-parallelism.md) ✅ | Parallel work with an ordered merge where results can't depend on order | M / determinism | P01, O06 |
| [P04](P04-order-independent-rooms.md) | Room completion independent of tree visit order (a measured divergence), then a faster search tree | L / changes routes; Q-series rules | P01, P02, the A-series board-parity gate |
| [P05](P05-simd-and-allocator.md) | SIMD on hot data-parallel loops shown by P01; an allocator trial | M / portability | P01 |

## Order

Decided by Dan 2026-10-16 (the review's question below): **P01 → P02 → P04
→ P03 and P05 → the route-changing Q items.**

1. **P01** first. Its tooling items can start as soon as the A series has a
   routing CLI; the stats output can be built into the A series' search core
   (the sinks in A04). P01.5 sets the speed target.
2. **P02** next: every speed-up that keeps results byte-identical. Its
   numbers say how far the parity-shaped tree can go.
3. **P04** right after P02, and only after rung 3 (A14) and F03's gate. D5
   caps the two hottest paths, overlap queries and room completion, until
   the tree is freed, so P04 is the lever if P02 falls short of the target.
   It changes routes, so it lands before any route-changing Q item: each Q
   measurement is then paired against the freed tree once, not re-baselined
   when the tree changes later. The Java-shaped tree stays selectable (P04
   "Decisions"), so the parity suite keeps its exact Java reference.
4. **P03 and P05** after P04, measured against the freed tree. P03.1 (the
   statistics and analyses) may run earlier beside P02; it changes no route.
5. The speed target from P01.5 is stated for the default tree. The parity
   tree is measured too, but it is the reference, not the product.

Report-only Q items (Q03, Q01.4, Q02.1, Q06) do not wait for P04.

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
- **P04's position** (raised by the roadmap review of 2026-10-16). D5 ties
  the search tree's shape and the work meter to Java's `MinAreaTree`, so the
  two hottest paths, overlap queries and room completion, can only change
  their constants, not their structure, until P04 lands. P04 is last in the
  order above because it gives up exact parity. If P01's baseline shows P02's
  constant-factor work cannot reach the target on its own, P04 is the lever.
  *Recommendation:* keep P04 after P02, but run it before P05 and before any
  Q item that changes routes, so the tree is freed once and every later
  measurement is against the freed tree. P01.5 says which, from the numbers.
  *Decided by Dan 2026-10-16: as recommended, with the Java-shaped tree kept
  as a selectable mode. Recorded in "Order" above, in P04's decisions, in
  [performance.md](../performance.md) and in Q00's order.*
