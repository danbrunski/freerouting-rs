# Q12 — Pass-1 portfolio and restarts

Opened 2026-10-17. Pass 1 is the best pass on canary-pwr, yet passes 2–10 are about
60 % of routing time (Q05 file). Commercial practice spends that time on several
whole solutions and keeps the best:
- TopoR optimises several variants and drops the worst [V, Wikipedia];
- Cadence US6957408B1 "generates several solutions based on different net orders and
  returns the best" [V];
- tscircuit races sub-solvers with different parameters [V].

Prerequisites: board parity (rung 3); A10 (`NetOrdering`); B01; O06 (parallel evaluation pattern); **Q07.1–Q07.3 built** (its two orders are arms 1 and 2; Q07.4 rejected both as defaults, so the code is kept under tag `archive/q07-orderings`, 379b413 (the branch was deleted). Re-apply it here with its PROVENANCE rows, and change the engine crate's `license` to `GPL-3.0-only AND MIT`, because the files carry KRT's MIT portions); Q10 and Q11 for the combined arm; F03.1, F03.4; measured after P04.

Source: research by freerouting-rs-f1 at Dan's request (2026-10-17), `docs/roadmap/research/2026-10-17-routing-algorithms.md` (candidate N3); the preregistration draft is `docs/roadmap/prereg/Q12.json`, marked `DRAFT` until the implementer registers it (sha256s pinned) before the first number.

Evidence of order variance:
- KRT #1230: placebo perturbations moved open nets by about ±10 % [V];
- freerouting PR #793: on 70 boards, 24 better, 22 worse, 24 the same [V];
- kicad-tools #6342: a net pair routes in one order and not the other [V].

Squeaky-wheel restarts (Joslin & Clements, JAIR 1999) move failed elements to the
front and rebuild. KRT's naive "hot nets first" was **7 % worse** (#1230) [V], so it is
an arm, never the default.

## What exists

- The post-fanout board snapshot (B01).
- `NetOrdering` (A10), with Java's item order and the seeded shuffle with
  `JavaRandom`.
- Q07's conflict-round and inside-out orders, once Q07.1–Q07.3 are built.
- The board score (`BoardScore`).
- O06's parallel evaluation pattern: each candidate on its own copy, merged in
  submission order.

## Decisions made here

- A pipeline-level stage (`router.autorouter.portfolio = K`):
  1. Clone the post-fanout board K times.
  2. Run arm i's pass 1 (and Q10's sweep, if on) on clone i. Use up to
     `max_threads` threads.
  3. Keep the best by (Freerouting incomplete count, board score, arm index).
  4. Then run the optimizer once on the winner.
- **Arm 0 is the configured ordering.** The portfolio can therefore never be worse
  than the baseline by the engine's own score: a "best of" includes the baseline.
  KiCad's count can still disagree (plane nets), so measurement is still required.
- Arm list, fixed in the settings and in this order:
  - Java item order;
  - Q07 `conflict-rounds`;
  - Q07 `inside-out`;
  - seeded shuffles with seeds 1…K−3.
- `restarts = R`: a squeaky-wheel generation. Its order is the winner's order,
  stable-sorted with that run's failed connections and ripped victims first. It runs
  sequentially after the portfolio, and the best of all is kept.
- Results are independent of thread scheduling: each arm is a pure function of
  (board, arm). They are merged by arm index.

## File ownership

- Creates `quality/portfolio/{mod,arms}.rs`.
- Appends to `quality/mod.rs`, `quality/register.rs`, the C02 settings struct and the
  pipeline's stage list (one call).

## Checklist

- [x] **Q12.1: the portfolio stage.** K clones, the fixed arm list, parallel threads,
  selection by key.
  - **Accept:**
    - with K = 1 the SES is byte-identical to the baseline;
    - a test where arm 2 beats arm 0 picks arm 2;
    - runs with `max_threads` 1 and 4 give the same SES;
    - determinism.
  - *Done 2026-10-18:* `quality::portfolio` (`router.autorouter.portfolio = K`, one call in
    `run_pipeline`: `quality::portfolio::run_routing`, which is `run_batch_loop` when off). The
    batch loop is split into `start_batch_loop` (layer check, fanout) and `continue_batch_loop`
    (passes, finish, Q10 sweep); each arm continues it on its own clone of the post-fanout
    board with stages built afresh, its own `PassRunner` and cancel token, on up to
    `max_threads` scoped threads in waves, merged in arm order by (incomplete, higher score,
    lower index); the optimizer runs once on the winner. Arms (`quality::portfolio::arms`):
    configured, Q07 `conflict-rounds`, Q07 `inside-out` (re-applied with
    `git cherry-pick -n archive/q07-orderings`, not registered as orderings: arms only;
    PROVENANCE rows added, engine crate `license = "GPL-3.0-only AND MIT"`), shuffles seeds
    1, 2, …. Evidence: `tests/quality_portfolio.rs`
    `portfolio_one_is_byte_identical_to_the_job_without_it` (Issue433 `-mp 1`, Issue433 default
    passes with the optimizer, `q12-channel.dsn`; also `portfolio = 0` equals unset);
    `quality::portfolio::tests::the_key_is_incomplete_then_score_then_index` (arm 2 with fewer
    incomplete beats arms 0 and 1 and is kept) and
    `the_kept_arm_is_the_best_by_incomplete_then_score_then_index` (Issue433, K = 4: arm 0
    leaves 1 connection, arm 1 `conflict-rounds` 0, arm 1 kept);
    `one_and_four_threads_give_the_same_ses`, and on canary-pwr with the prereg flags, K = 4,
    `-mp 1`: `max_threads` 1 and 2 give byte-identical SES (`cmp`); determinism:
    `the_portfolio_and_restarts_are_deterministic` and `tools/determinism-check.sh` on
    Issue433 (K = 4, restarts 2, 2 threads) and on `q12-channel.dsn`: "identical". Off is
    unchanged: `PARITY_SET=tools/bench/sets/connection-parity.txt cargo test --release -p
    freerouting --test autoroute_board_parity -- --ignored autoroute_board_parity
    --test-threads=1` ok (the full A14 is pending, the orchestrator's run). DIVERGENCES line
    under the settings fields.
- [x] **Q12.2: squeaky-wheel restarts.**
  - **Accept:** a fixture where the failed connection routes when moved first ends
    complete; off is byte-identical; determinism.
  - *Done 2026-10-18:* `router.autorouter.restarts = R` (with `portfolio` unset it runs arm 0
    alone first). Each generation routes pass 1 in the current best's recorded pass-1 order
    with its failed nets first, then the other nets its pass 1 ripped (`arms::squeaky_order`;
    failed before victims, since the victim first repeats the run), on a clone of the
    post-fanout board; best of all kept by the same key. Generations stop once the best is
    complete or still the arm the last one started from (same order, same board). Fixture
    `tests/data/quality/q12-channel.dsn` (synthetic, one layer, fanout off): Java's pass 1
    leaves B incomplete; restart 1 (B failed, A ripped) ends complete:
    `a_restart_routes_the_failed_net_first_and_the_channel_ends_complete` (also: `restarts = 0`
    equals unset byte for byte, and a rerun gives the same SES);
    `a_restart_that_keeps_no_new_best_ends_the_generations`;
    `squeaky_order_puts_failed_then_victim_nets_first_and_keeps_planes_first`. Determinism:
    `tools/determinism-check.sh` on `q12-channel.dsn` (restarts 2, 2 threads) and on Issue433
    (restarts 2): "identical".
- [~] **Q12.3: memory and time guard.** A board clone's peak memory times K must stay
  under a setting (WSL limits: see the memory notes).
  - **Accept:** canary-pwr and LogicBoard_smt with K = 4; peak resident memory in the
    done note.
  - *Partial 2026-10-18:* `router.autorouter.portfolio_memory_mb` (unset 2048 MiB): the guard
    estimates a clone (`clone_estimate_bytes`: 8 MiB + 8 KiB per item), counts 3 clones per
    running arm plus the post-fanout and best boards, and runs only as many arms at once as
    fit, never fewer arms (at least one at a time, logged "Portfolio memory guard: ..."), so it
    changes no result: `the_memory_guard_runs_arms_one_at_a_time_and_keeps_every_arm`
    (limit 1 MiB: concurrency 1, same SES and arms). Time: each arm runs the job's passes and
    work budgets; the job deadline and cancel reach every arm before each connection.
    canary-pwr-9511d9a, K = 4, `-mp 1`, prereg flags, release, `/usr/bin/time`: 1 thread
    93.71 s, peak RSS 109636 KB; 2 threads 59.68 s, 175556 KB (about 64 MiB per extra running
    arm, which set the 8 KiB per item; the estimate is 24 MiB per clone there). Remainder:
    LogicBoard_smt with K = 4: its fanout alone outlasts the 170 s short-run limit (a K = 1
    run was cut at 170.39 s, still in the fanout, 31952 KB), so it needs the heavy slot; its
    peak RSS can come from Q12.5's `portfolio4` run (`/usr/bin/time -f %M`); owner Q12.5, run in Q00's Q10-Q12 measurement family after P04.
- [~] **Q12.4: order-spread report.** Per board: min, median and max incomplete count
  (and KiCad count where graded) over the K arms.
  - **Accept:** a table on the eligible boards in the done note. It is proposed as the
    per-board noise quantum for later keep rules (an F03 amendment, the
    orchestrator's call).
  - *Partial 2026-10-18:* the report is built: every arm's incomplete count, score and
    violations in the job log ("Portfolio arm i (label): ...") and in
    `PipelineOutcome::portfolio` (`PortfolioOutcome::arms`), and the min, median (mean of the
    two middle values for even K) and max incomplete count over the K portfolio arms in
    `PortfolioOutcome::spread` and the "Portfolio: ... Incomplete over the portfolio arms: min
    a, median b, max c." line (`the_spread_has_min_median_and_max`). Rows so far, from the
    portfolio arm's spot checks: canary-pwr-9511d9a (prereg flags, `-mp 1`) 104 / 108 / 111
    (arms 104, 110, 106, 111); Issue433-my-board 0 / 1 / 1. Remainder: the table on every
    eligible board, with KiCad counts where graded, from Q12.5's registered `portfolio4` run
    (the "Portfolio:" line of each job log); owner Q12.5, run in Q00's Q10-Q12 measurement family after P04.
- [ ] **Q12.5: measurement** (prereg `docs/roadmap/prereg/Q12.json`, registered).
  - Arms: `mp1`, `mpdef`, `portfolio4`, `portfolio4+restart2`, and `portfolio4` with
    Q10 and Q11 on.
  - Work is reported against `mpdef`'s, since the portfolio is meant to replace
    passes 2–10 at about the same total work.
  - **Accept:** measured.
  - Registered 2026-10-18 (before any measured number) as `docs/roadmap/prereg/Q12.json`,
    sha256 8467d77dcd989648691a0a29a4095aceb650bb6e3fcd2a20d707da2a2e0b5a98, pinned by
    `crates/freerouting-dev/tests/prereg_q12.rs`; N02 totals rule, Q11's boards
    (`tools/bench/sets/q10-sweep.txt`), `eligible` still to be filled as an amendment.

## Expected [I]

 with the ±10 spread on canary-pwr, a best-of-4 should recover a few
connections. Upstream's clone-and-shuffle threads were deleted, but they shuffled
later passes, not pass 1.
