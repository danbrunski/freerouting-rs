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

- [x] **Q08.2 — Off means identical; on is deterministic.**
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
  *Partial 2026-10-20:* the clause "`ses-diff` shows identical sessions on `benchmark`" (the
  default router unchanged) is met by the small parity check above (decided 2026-10-20: no full
  A14; Java-identity clauses are met by `PARITY_SET=tools/bench/sets/connection-parity.txt`);
  `tools/determinism-check.sh` runs on `canary-pwr-pours1` and on `Issue093-interf_u` in place of
  `Issue145-smoothieboard` (226 s per route), and the per-board extensions come from the quick
  benchmark rows (Q08.json amendment 2). Remainder: Q08.3's steps 3 and 4.
  *Partial 2026-10-20 (run):* Q08.3's run (release CLI `af3b05ee3577`, its note below).
  Per-board extensions and extra work: `maze_budget_extensions` 0 and `maze_budget_extra_units` 0
  (0 % of `maze_work_units` and of `tree_nodes_visited`) on all 8 quick benchmark rows and on
  `canary-pwr-pours1`, and every `earned-budget` session is byte-identical (`cmp`) to the default
  arm's. `tools/determinism-check.sh` with the `earned-budget` command: `Issue093-interf_u`
  identical (both exit 0); `canary-pwr-pours1` **incomplete**: the idle run exit 0, the loaded
  run (8 burners, load 3.67 before, 11.62 after) hit the registered `timeout -k 10 600` (exit 124;
  the CLI's SIGTERM handler cancels and saves the best board so far), so the script reports
  "different: exit status 0 idle, 124 loaded". Its saved SES is byte-identical to the idle run's,
  and the idle run's to both arms' registered sessions (three earned-budget runs at different
  loads, one session), but a cancelled run does not prove the full route. Remainder: a
  `canary-pwr-pours1` determinism run that fits its timeout (needs a Q08.json amendment for the
  loaded run's timeout: an unlimited-pass route takes 520 to 570 s here, not the 100 s of the
  5-pass bench runs amendment 2 quoted); moot if Q08.3's removal lands.
  *Done 2026-10-20:* the only remainder, `canary-pwr-pours1`'s loaded determinism run, is moot
  (decided 2026-10-20): Q08.3's keep rule failed and `earned-budget` is removed (Q08.3's note),
  so no option is left to be deterministic. The off-means-identical clause held throughout (small
  parity check above) and holds after the removal (Q08.3's done note: the same small parity
  check, 7/7 exact).

- [x] **Q08.3 — Measurement.**
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

  *Partial 2026-10-20:* not run; the run is cut to minutes (decided 2026-10-20: measurement
  runs stay a few minutes). Amendment 2 of `docs/roadmap/prereg/Q08.json` (before any Q08 run;
  sha256 `0b82473af2ceedbc244c2fbacf0340f803040f53ec2e3edbe1fab3bbf94b8125`, re-pinned in
  `crates/freerouting-dev/tests/prereg_q08.rs`, 5 passed): rows are the 8 benchmark boards whose
  longest recorded route took at most 25 s plus `canary-pwr-pours1`
  (`tools/bench/sets/quick-benchmark.txt`, each time with the `target/orchestrator/` directory it
  came from); the wall-time clause has no quiet-machine condition and is taken over the rows where
  `earned-budget` granted an extension; the determinism boards are `canary-pwr-pours1` and
  `Issue093-interf_u` (smoothieboard takes 226 s); no A14 (Q08.2's identity clause is met by its
  small parity check). Disclosed in the amendment: earlier registrations' `--stats` show
  `maze_work_limit_hits` 0 on every kept row, so fewer than 3 eligible boards are expected and
  the decision table's fail row would then apply. This replaces the heavy run list above.
  heavy run pending (one release build of main, `CARGO_BUILD_JOBS=2 cargo build --release -p
  freerouting-cli`; from the repository root; one route at a time; about 11 minutes, of which
  `canary-pwr-pours1` takes about 8: about 100 s per arm and about 5 minutes for its determinism
  check; every route under `ulimit -v 8000000` and `timeout 600` for canary-pwr or `timeout 120`
  otherwise, which `tools/bench/quick-arm.sh` applies):
  1. `B=target/release/freerouting; O=target/q08; tools/bench/quick-arm.sh $B $O default
     tools/bench/sets/quick-benchmark.txt -- --router.optimizer.enabled=false --stats json`;
  2. amendment 3 of Q08.json: `files` narrowed to the benchmark rows whose
     `$O/default/<board>.ses.stats.json` total `maze_work_limit_hits` is above 0, plus
     `canary-pwr-pours1`, quoting each count; re-pin `prereg_q08.rs`;
  3. `tools/bench/quick-arm.sh $B $O earned-budget tools/bench/sets/quick-benchmark.txt --
     --router.optimizer.enabled=false --stats json --router.autorouter.connection_router=earned-budget`
     (every quick row, for Q08.2's per-board extension counts; the gate reads the eligible rows);
  4. `tools/determinism-check.sh --out $O/det-pours1 -- bash -c "ulimit -v 8000000; mkdir -p {OUT}
     && timeout -k 10 600 $B -de fixtures/canary-pwr/canary-pwr-pours1.dsn -do {OUT}/out.ses
     --router.optimizer.enabled=false --router.autorouter.connection_router=earned-budget"`, and
     the same with `--out $O/det-interf_u`, `timeout -k 10 120` and `fixtures/Issue093-interf_u.dsn`;
  5. records from the runs as Q08.json's `grading` says (`freerouting-dev ses-diff` statistics,
     `via_in_pad`, the `.time` files' wall seconds, the `--stats` totals), then
     `freerouting-dev gate --records <records> --prereg docs/roadmap/prereg/Q08.json`, the
     wall-time ratio over the rows with an extension, and Q08.2's per-board
     `maze_budget_extensions` and `maze_budget_extra_units / tree_nodes_visited`.
  Operator decision taken with the amendment (recommended answer): the one board in the records
  with a work-limit hit, `canary-pwr-2026-10-15-1747-unpoured` (357 s per route), stays out.
  *Partial 2026-10-20 (run):* measured; **gate FAIL, removal pending.** Engine: release CLI and
  `freerouting-dev` of main `af3b05e` (`freerouting --version`: `git af3b05ee3577`), built in a
  detached worktree; run from that worktree's root, one route at a time, under `ulimit -v 8000000`
  and `timeout -k 10 120` (`600` for canary-pwr) as `tools/bench/quick-arm.sh` applies (no runner
  fix was needed); other agents routed at the same time (load average 3 to 8).
  1. Default arm (`quick-arm.sh $B $O default tools/bench/sets/quick-benchmark.txt --
     --router.optimizer.enabled=false --stats json`): 9 rows, all exit 0.
  2. Amendment 3 of `docs/roadmap/prereg/Q08.json` (after step 1, before any `earned-budget`
     route; sha256 `a53e55772fb2a14ed4637f91cedc7c499158913e4a376b2998d42103ebf25bb2`, re-pinned
     in `crates/freerouting-dev/tests/prereg_q08.rs`, 6 passed): total `maze_work_limit_hits` of
     the default arm is 0 on every row (Issue433 0, Issue649 0, bm02 0, sonde_xilinx 0, bm07 0,
     bm10 0, interf_u 0, CNH 0, canary-pwr-pours1 0), so no benchmark row is eligible and `files`
     keeps `canary-pwr-pours1` only.
  3. `earned-budget` arm (the same plus `--router.autorouter.connection_router=earned-budget`) on
     all 9 rows, all exit 0.

  | row | arm | unrouted | vias | clearance | wall s | `maze_work_limit_hits` | extensions |
  |---|---|---|---|---|---|---|---|
  | Issue433-my-board | default / earned | 0 / 0 | 4 / 4 | 0 / 0 | 0.04 / 0.04 | 0 / 0 | 0 / 0 |
  | Issue649-kicad_ecc83 | default / earned | 0 / 0 | 0 / 0 | 0 / 0 | 0.01 / 0.01 | 0 / 0 | 0 / 0 |
  | Issue508-DAC2020_bm02 | default / earned | 0 / 0 | 13 / 13 | 0 / 0 | 0.13 / 0.12 | 0 / 0 | 0 / 0 |
  | Issue690-sonde_xilinx | default / earned | 11 / 11 | 3 / 3 | 0 / 0 | 0.36 / 0.33 | 0 / 0 | 0 / 0 |
  | Issue508-DAC2020_bm07 | default / earned | 0 / 0 | 27 / 27 | 0 / 0 | 0.48 / 0.42 | 0 / 0 | 0 / 0 |
  | Issue508-DAC2020_bm10 | default / earned | 0 / 0 | 150 / 150 | 0 / 0 | 5.73 / 5.24 | 0 / 0 | 0 / 0 |
  | Issue093-interf_u | default / earned | 19 / 19 | 84 / 84 | 0 / 0 | 1.13 / 1.04 | 0 / 0 | 0 / 0 |
  | Issue555-CNH_Functional_Tester_1 | default / earned | 1 / 1 | 15 / 15 | 6 / 6 | 5.97 / 5.47 | 0 / 0 | 0 / 0 |
  | canary-pwr-pours1 (judged) | default / earned | 122 / 122 | 129 / 129 | 70 / 70 | 568.70 / 523.46 | 0 / 0 | 0 / 0 |

  Unrouted, vias and clearance are `freerouting-dev ses-diff` statistics of each arm's SES
  imported into its DSN (`connections.incomplete_count`, `vias.total_count`,
  `clearance_violations.total_count`); wall is `/usr/bin/time`; the counts are the `--stats` JSON
  totals. `maze_budget_extra_units` is 0 on every row; `tree_nodes_visited` is equal in both arms
  on every row (canary-pwr-pours1 29 510 522 037). Every `earned-budget` SES is byte-identical
  (`cmp`) to the default's and `ses-diff` finds them equivalent; via_in_pad 0 in both arms.
  4. Determinism (Q08.2's note above): `Issue093-interf_u` identical; `canary-pwr-pours1`
     incomplete (the loaded run hit the 600 s timeout, exit 124; its saved SES equals the idle
     run's).
  5. Records: a bench.json per arm built from these runs, `freerouting-dev gate collect --engine
     rust --group fixtures/canary-pwr/=canary-pwr` (via_in_pad), plus `connections` (313, the
     default run's `connections.maximum_count`) and the report counts; then `freerouting-dev gate
     --records records.json --baseline baseline.json --prereg docs/roadmap/prereg/Q08.json`
     (baseline written first with `--write-baseline`):
     ```
     prereg Q08.3-earned-budget (sha256 a53e55772fb2a14ed4637f91cedc7c499158913e4a376b2998d42103ebf25bb2, 3 amendment(s)): 0 refusal(s)
     pair default -> earned-budget; judged 1 of 1 row(s)
       fixtures/canary-pwr/canary-pwr-pours1.dsn [canary-pwr]: neutral  via_in_pad 0->0, vias 129->129, clearance_violations 70->70, unrouted 122->122, ...
     totals rule (accept: improve, >= 3 distinct boards):
       canary-pwr [unrouted]: 122 -> 122 (change 0, band 10)
       FAILS: REFUSED: a change is judged on >= 3 DISTINCT boards; this run has N=1. Rows of one board count once
       FAILS: the total does not improve (change 0) and the speed clause does not hold
       totals verdict: reject
     gate: FAIL
     ```
     Wall-time clause: no judged row granted an extension, so it holds by amendment 2 (3); the
     verdict is the gate's. Run files: `target/orchestrator/q08/run/` (deleted after the note).

  Keep rule: (1) fails, so the decision table's fail row applies: `earned-budget` is removed.
  Remainder (removal pending, not done here): remove `crates/freerouting/src/quality/budget.rs`
  (and `pub mod budget` in `quality/mod.rs`), the `earned-budget` entry in
  `quality/register.rs`, the session's budget hook (`autoroute/session/budget.rs`, `pub mod
  budget`, the `budget`/`first_estimate`/`best_estimate` fields, `set_budget_policy`,
  `has_budget_policy`, `observe_estimate` and the limit check's Q08 branch in
  `autoroute/session/mod.rs`, the `has_budget_policy` call in `autoroute/session/search.rs`),
  `route_with_budget` and the `budget` parameters in `autoroute/batch/connection.rs` and
  `autoroute/connection.rs`, `WorkLimit::extend` in `board/search/work.rs`, the
  `maze_budget_extensions` / `maze_budget_extra_units` counters in `run_stats.rs`,
  `crates/freerouting/tests/quality_budget.rs`, and the "Earned budget extension" block of
  `docs/DIVERGENCES.md`; this note keeps the numbers.
  *Done 2026-10-20:* measured (the run above); the keep rule failed, so per the decision table's
  fail row `earned-budget` is removed, and the removal commit is this change. Removed:
  `quality/budget.rs` and its `pub mod`, the `earned-budget` registration in
  `quality/register.rs`, `autoroute/session/budget.rs` and the session's hook (`budget`,
  `first_estimate`, `best_estimate`, `set_budget_policy`, `has_budget_policy`,
  `observe_estimate`, the Q08 branch of `is_stop_requested`, the estimate call in
  `session/search.rs`), `FreeroutingConnectionRouter::route_with_budget`, `attempt_budgeted`,
  `route_connection_hooked` and the `budget` parameters of `route_connection_counted` and
  `search_and_locate`, `WorkLimit::extend`, the `maze_budget_extensions` and
  `maze_budget_extra_units` counters (`COUNTERS` 55 -> 53), `tests/quality_budget.rs`, and the
  DIVERGENCES block. Kept as the record: `docs/roadmap/prereg/Q08.json` and
  `crates/freerouting-dev/tests/prereg_q08.rs` (now with the id and E as constants; 6 passed).
  `router.autorouter.connection_router=earned-budget` is now refused:
  `autoroute_batch_control::the_removed_earned_budget_router_is_refused_naming_the_known_routers`
  ("unknown stage id 'earned-budget' in router.autorouter.connection_router; known ids:
  freerouting-connection, transactional-ripup"). Default routing unchanged:
  `PARITY_SET=tools/bench/sets/connection-parity.txt cargo test --release -p freerouting --test
  autoroute_board_parity -- --ignored autoroute_board_parity --test-threads=1` (1 passed, 7 of 7
  boards exact, SES bytes equal); `cargo test --workspace --no-fail-fast` 2619 passed, 0 failed,
  56 ignored. Status lines: `docs/roadmap/routing-quality.md` §8 and the Q00 README's Q08 row.
