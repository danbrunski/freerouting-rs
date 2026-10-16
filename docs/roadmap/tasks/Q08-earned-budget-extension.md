# Q08 — Earned budget extension

Opened 2026-10-15. After J01, Java's per-connection maze search stops at a work
limit counted in search-tree nodes (WorkMeter), doubling per pass. A search
that is still closing in on its target when the limit fires is cut off the
same way as one that is lost. KRT's rule (#529, `GridSearch::extend_budget` in
`rust_router/src/router.rs`, at `0486e799`): when the budget runs out, grant
one more base budget if the closest approach improved by at least
`max(2 cells, 2 % of best_h)` during the last one, up to a ceiling. KRT
reports −29 incomplete nets over 150 boards for it
(`single_ended_routing._dynamic_iterations`); its "grace" variant measured
worse (zynq7020 30 open plus 2 DRC; duodyne 3.6× CPU) and is not ported.

This changes where Java's limits cut, so it is a quality change measured under
the Q rules, never part of parity.

Prerequisites: board parity (rung 3); B02 (WorkMeter, WorkLimit); A04 (a
budget hook in the maze search that reports progress when the limit fires);
A10 (`ConnectionRouter` trait); F03.1, F03.4.

## What exists and what this adds

| Exists | State |
|---|---|
| Java WorkLimit on the maze search after the J01 re-pin; per-connection limit `×2` per pass (pipeline report §3.5(c)) | ported by B02 and A04 |
| Java `MazeSearchEngine` priority queue: the expansion value is the cost so far plus the destination-distance estimate (`DestinationDistance`) | ported by A04 |
| KRT `best_h` = the minimum of `f − g` over expanded nodes, tracked at pop time; tranche ceiling 1e7 | the progress signal |

## Decisions made here

- **Progress signal:** the smallest destination-distance estimate of any
  element popped during the current budget, in board units. A04's hook
  reports it with the work used when the limit fires:
  `fn on_limit(&mut self, progress: SearchProgress) -> LimitDecision`
  (`Stop` or `Extend(work)`); the Freerouting policy always returns `Stop`.
- **The rule:** extend by one base budget if the estimate fell by at least
  `max(2·trace width, 2 % of the estimate at the start of the budget)`.
  At most E extensions per connection attempt (preregistered, default 4).
- **A wrapper.** `EarnedBudget` implements `ConnectionRouter` by running the
  Freerouting connection router with this policy. Id `earned-budget`, selected
  by `router.autorouter.connection_router`.
- **Deterministic by construction:** the decision depends only on search state
  and work counts (D2, D5).

## File ownership

| Item | New files | May edit |
|---|---|---|
| Q08.1 | `crates/freerouting/src/quality/budget.rs` | `quality/mod.rs`, `quality/register.rs` (append) |
| Q08.2 | `crates/freerouting/tests/quality_budget.rs` | — |
| Q08.3 | the Q08 preregistration and baseline files | `docs/DIVERGENCES.md` (append, only if kept) |

## Checklist

- [ ] **Q08.1 — The policy and the wrapper.**
  Rust design: `quality::budget::{EarnedBudgetPolicy, EarnedBudget}`. The
  policy keeps `estimate_at_start`, `extensions` and the base budget; integer
  arithmetic only (`2 %` as `estimate / 50`).
  - **Accept:** unit tests over synthetic progress sequences: steady progress
    extends up to E times; progress below the quantum stops; a zero estimate
    (target reached) never asks for more.

- [ ] **Q08.2 — Off means identical; on is deterministic.**
  - **Accept:**
    - with the default connection router, `ses-diff` shows identical
      sessions on `benchmark`;
    - with `earned-budget`, `tools/determinism-check.sh` passes on
      `canary-pwr` and `Issue145-smoothieboard`;
    - the done note gives, per `benchmark` board, the number of extensions
      granted and the extra work as a share of total work.

- [ ] **Q08.3 — Measurement.**
  Preregister (F03.4). Arms: default and `earned-budget`. Boards:
  `benchmark` boards where J01 recorded work-limit cut-offs, plus
  `canary-pwr` and its variants. Signal: unrouted. Guards: vias, `drc_real`,
  and wall time (at most +25 % median).
  - **Accept:** measured. Kept only if it improves on N−1 boards and regresses
    on none; otherwise removed, with the numbers here.
