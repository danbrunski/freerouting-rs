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

- [x] **Q08.1 — The policy and the wrapper.**
  Rust design: `quality::budget::{EarnedBudgetPolicy, EarnedBudget}`. The
  policy keeps `estimate_at_start`, `extensions` and the base budget; integer
  arithmetic only (`2 %` as `estimate / 50`).
  - **Accept:** unit tests over synthetic progress sequences: steady progress
    extends up to E times; progress below the quantum stops; a zero estimate
    (target reached) never asks for more.

  *Done 2026-10-18:* A04 had no budget hook, so Q08.1 adds it:
  `autoroute::session::budget::{BudgetPolicy, BudgetStart, BudgetProgress, LimitDecision,
  StopAtLimit}` (the progress type is `BudgetProgress`, because `sink::SearchProgress` is the
  GUI's event). `Session::set_budget_policy` (None on every Java path) tells the policy the base
  budget and the trace width (twice the widest active-layer half width); the search records each
  popped element's `sorting_value - expansion_value`; `Session::is_stop_requested` asks the
  policy on the first check past the limit and `WorkLimit::extend` raises the limit on
  `Extend(units)`, else Java's cut-off line and stop. `FreeroutingConnectionRouter::route_with_budget`
  passes it to the connection's search and its plane fallback's (each starts the policy anew;
  the neck-down retry keeps Java's stop); `route` is `route_with_budget(.., None)`.
  `quality::budget::EarnedBudget` (id `earned-budget`, registered in `quality::register`) wraps it
  with `EarnedBudgetPolicy` (E = `DEFAULT_MAX_EXTENSIONS` = 4 per search; extend when the
  truncated estimate fell by more than 0 and by at least `max(2 * trace width, start / 50)`; a
  Stop latches until the next search). `run_stats` counts `maze_budget_extensions` and
  `maze_budget_extra_units`. Tests (`cargo test -p freerouting --lib quality::budget`, 7 passed):
  `steady_progress_extends_up_to_e_times` (4 extensions, then Stop), `progress_below_the_quantum_stops`
  (98 001 from 100 000 stops, 98 000 extends; the trace-width term rules small estimates),
  `a_zero_estimate_never_asks_for_more` (0 and 0.5), `the_quantum_is_measured_from_the_start_of_each_budget`,
  `no_progress_or_no_pop_stops`, `a_stop_latches_until_the_next_search`,
  `a_search_without_a_limit_is_never_extended`.

- [~] **Q08.2 — Off means identical; on is deterministic.**
  - **Accept:**
    - with the default connection router, `ses-diff` shows identical
      sessions on `benchmark`;
    - with `earned-budget`, `tools/determinism-check.sh` passes on
      `canary-pwr` and `Issue145-smoothieboard`;
    - the done note gives, per `benchmark` board, the number of extensions
      granted and the extra work as a share of total work.

  *Partial 2026-10-18:* done: the default router's path is unchanged where it can be checked
  without the heavy slot: `PARITY_SET=tools/bench/sets/connection-parity.txt cargo test --release
  -p freerouting --test autoroute_board_parity -- --ignored autoroute_board_parity
  --test-threads=1` passed (1 passed). `crates/freerouting/tests/quality_budget.rs` (4 passed):
  `a_policy_that_always_stops_routes_like_java` (`StopAtLimit` through the hook equals Java's
  router on `Issue143-rpi_splitter` with a 1 nominal ms budget: same state hash, SES, work units
  and limit hits), `earned_budget_extends_tight_searches_deterministically` (the same budget:
  9 limits fired, 8 extensions, 512 000 extra units, unrouted 2 -> 0; two runs give the same SES,
  hash and tally), `where_no_limit_fires_the_job_routes_like_java` (full jobs on
  `scoring-perfect-two-pin` and `Issue269-min_fr_test`: byte-identical SES with and without
  `earned-budget`, and twice with it), `earned_budget_is_registered_under_its_id`.
  `tools/determinism-check.sh` with `earned-budget` on `Issue143-rpi_splitter` (release CLI,
  optimizer off, 8 burners): identical (0 extensions at Java's budgets). Release spot runs before
  the Q08.3 registration, both arms, optimizer off, `--stats json`: `Issue433-my-board`,
  `Issue649-kicad_ecc83`, `Issue508-DAC2020_bm02`, `Issue690-sonde_xilinx`,
  `Issue508-DAC2020_bm07`: SES identical, `maze_work_limit_hits` 0, extensions 0 (0 % extra
  work); disclosed in `Q08.json`'s amendment 1 (operator decision). Full A14 done 2026-10-18: full A14 on origin/main 7a4f73b (Q08, Q12, N06.3 in the tree; release, `--test-threads=1`, `PARITY_SUBDIRS=1`; log `target/orchestrator/heavy/a14-7a4f73b.log`): 1 passed, 138 boards (120 exact, 15 cutoff, 3 Java failures), every SES byte-identical except the known Issue684 and Issue721 (expected), Issue110-RelayModule (non-ASCII name only) and the Java-failure boards. Remainder, in Q08.3's heavy run: `ses-diff` on `benchmark` with the
  default router, `determinism-check.sh` on `canary-pwr` and `Issue145-smoothieboard`, and the
  per-board extensions and extra-work share (Q08.3's report metrics `maze_budget_extensions`,
  `maze_budget_extra_units`, `tree_nodes_visited`).

- [ ] **Q08.3 — Measurement.**
  Preregister (F03.4). Arms: default and `earned-budget`. Boards:
  `benchmark` boards where J01 recorded work-limit cut-offs, plus
  `canary-pwr` and its variants. Signal: unrouted. Guards: vias, `drc_real`,
  and wall time (at most +25 % median).
  - **Accept:** measured. Kept only if it improves on N−1 boards and regresses
    on none; otherwise removed, with the numbers here.

  Registered 2026-10-18 before any number on its boards: `docs/roadmap/prereg/Q08.json`, sha256
  066c0433… as registered, `faa4188e…` after amendment 1 (disclosure), pinned by
  `crates/freerouting-dev/tests/prereg_q08.rs` (4 passed). N02's totals rule (`accept: improve`,
  signal `unrouted`, canary-pwr noise 10) by the orchestrator's instruction, in place of the
  paired wording above; guards `vias` and `clearance_violations` per row, `via_in_pad` hard, and
  a keep-rule clause: median wall-time ratio at most 1.25. Pool: the 16 `benchmark` boards and the
  5 in-repository canary-pwr variants. J01's references show no `maze_search` cut-off line on any
  benchmark board (pull_tight only), so eligibility is the default arm's own `--stats`
  `maze_work_limit_hits > 0`, fixed by an amendment before any `earned-budget` run.

  heavy run pending: one release build `B=target/release/freerouting` of the landing tree, quiet
  machine (load average <= 1.0 before each run), one run at a time.
  1. Default arm, every `boards.files` row of Q08.json:
     `$B -de {DSN} -do runs/default/{name}.ses --router.optimizer.enabled=false --stats json`.
  2. Amendment 1 of Q08.json: `files` narrowed to the benchmark rows with total
     `maze_work_limit_hits > 0` in `runs/default/{name}.ses.stats.json`, plus the 5 canary-pwr
     rows, quoting each count; update the hash in `prereg_q08.rs`.
  3. Candidate arm, every remaining row:
     `$B -de {DSN} -do runs/earned/{name}.ses --router.optimizer.enabled=false --stats json
     --router.autorouter.connection_router=earned-budget`.
  4. Determinism (Q08.2 and the stop rule): `tools/determinism-check.sh --out runs/det-{name} --
     bash -c "mkdir -p {OUT} && $B -de {DSN} -do {OUT}/out.ses --router.optimizer.enabled=false
     --router.autorouter.connection_router=earned-budget"` for
     `fixtures/canary-pwr/canary-pwr-pours1.dsn` and `fixtures/Issue145-smoothieboard.dsn`.
  5. Q08.2's identity: the full A14, `cargo test --release -p freerouting --test
     autoroute_board_parity -- --ignored autoroute_board_parity --test-threads=1` (no
     `PARITY_SET`), and `freerouting-dev ses-diff` of each `benchmark` row's `runs/default` SES
     against the same command's SES from a build of the commit before Q08 (identical expected).
  6. `freerouting-dev gate collect --arm default=runs/default ... --arm
     earned-budget=runs/earned ... --group fixtures/canary-pwr/=canary-pwr --out records.json`,
     then `freerouting-dev gate --records records.json --prereg docs/roadmap/prereg/Q08.json`,
     the wall-time median ratio by hand, and per benchmark board `maze_budget_extensions` and
     `maze_budget_extra_units / tree_nodes_visited` for Q08.2's note.
