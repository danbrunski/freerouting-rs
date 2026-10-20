# Q12 — Pass-1 portfolio and restarts

Opened 2026-10-17. Pass 1 is the best pass on canary-pwr, yet passes 2–10 are about
60 % of routing time (Q05 file). Commercial practice spends that time on several
whole solutions and keeps the best:
- TopoR optimises several variants and drops the worst [V, Wikipedia];
- Cadence US6957408B1 "generates several solutions based on different net orders and
  returns the best" [V];
- tscircuit races sub-solvers with different parameters [V].

Prerequisites: board parity (rung 3); A10 (`NetOrdering`); B01; O06 (parallel evaluation pattern); **Q07.1–Q07.3 built** (its two orders are arms 1 and 2; Q07.4 rejected both as defaults, so the code is kept under tag `archive/q07-orderings`, 379b413 (the branch was deleted). Re-apply it here with its PROVENANCE rows, and change the engine crate's `license` to `GPL-3.0-only AND MIT`, because the files carry KRT's MIT portions); Q10 and Q11 for the combined arm; F03.1, F03.4; measured after P04.

Source: research by freerouting-rs-f1 at the owner's request (2026-10-17), `docs/roadmap/research/2026-10-17-routing-algorithms.md` (candidate N3); the preregistration draft is `docs/roadmap/prereg/Q12.json`, marked `DRAFT` until the implementer registers it (sha256s pinned) before the first number.

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
    --test-threads=1` ok (and the full A14 on origin/main 7a4f73b (Q08, Q12, N06.3 in the tree; release, `--test-threads=1`, `PARITY_SUBDIRS=1`; log `target/orchestrator/heavy/a14-7a4f73b.log`): 1 passed, 138 boards (120 exact, 15 cutoff, 3 Java failures), every SES byte-identical except the known Issue684 and Issue721 (expected), Issue110-RelayModule (non-ASCII name only) and the Java-failure boards). DIVERGENCES line
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
- [x] **Q12.3: memory and time guard.** A board clone's peak memory times K must stay
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
  *Partial 2026-10-20:* the LogicBoard_smt clause is dropped (decided 2026-10-20: measurement runs
  stay a few minutes; every board but canary-pwr must route in well under a minute, and
  LogicBoard_smt's fanout alone outlasts 170 s). The canary-pwr clause is met by the measurement
  above (K = 4: 109636 KB at 1 thread, 175556 KB at 2). Q12.5's amended `portfolio4` run records
  the peak resident set of every quick row and of canary-pwr-9511d9a (`tools/bench/quick-arm.sh`'s
  `.time` file, `/usr/bin/time` `%M`). Proposed close note once those numbers are in: "*Done
  <date>:* canary-pwr K = 4 peak RSS 109636 KB (1 thread) and 175556 KB (2 threads); Q12.5's
  portfolio4 rows <table>; LogicBoard_smt dropped (decided 2026-10-20)." No new run is needed
  beyond Q12.5's.
  - *Done 2026-10-20:* canary-pwr K = 4 peak RSS 109636 KB (1 thread) and 175556 KB (2 threads)
    (the 2026-10-18 measurement above); Q12.5's registered `portfolio4` run (`-mp 1`, K = 4, the
    host's 8 threads, the guard ran "4 at a time" on every row) and a 1-thread diagnostic run of
    the same command plus `--router.autorouter.max_threads=1` (not a registered run; every SES
    byte-identical to the registered run's, `cmp` on all 15 rows), `/usr/bin/time` `%M` from
    `target/orchestrator/q1112/out/{portfolio4,diag-portfolio4-t1}/<board>.time`:

    | row | peak RSS kB, 4 at a time (registered) | 1 at a time (diagnostic) |
    |---|---|---|
    | Issue433-my-board | 12672 | 11392 |
    | Issue649-kicad_ecc83-pp_input_board_v1 | 12160 | 11136 |
    | Issue508-DAC2020_bm02 | 18048 | 18368 |
    | Issue690-sonde_xilinx | 15744 | 12644 |
    | Issue508-DAC2020_bm07 | 32304 | 21720 |
    | Issue508-DAC2020_bm10 | 270748 | 101796 |
    | Issue093-interf_u | 28540 | 19824 |
    | Issue555-CNH_Functional_Tester_1 | 239616 | 122128 |
    | canary-pwr-9511d9a | 361236 | 152592 |
    | mydevboard-rp2040dev | 69552 | 32072 |
    | slice-dcmt-bread-slice | 32220 | 20000 |
    | flight-computer-core-board | 28908 | 19120 |
    | laser-backplane-dvi-laserbackplane-dvi | 17280 | 13428 |
    | vfo-controller-catvfoctrl | 42404 | 23888 |
    | mccx11-hardware-sx1262-433mhz-reverse-engineering | 11136 | 10752 |

    The largest, canary-pwr-9511d9a at 4 at a time, is 353 MiB, under the 2048 MiB default limit
    and the 8000000 kB `ulimit -v`. LogicBoard_smt dropped (decided 2026-10-20).
- [x] **Q12.4: order-spread report.** Per board: min, median and max incomplete count
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
  *Partial 2026-10-20:* the table comes from Q12.5's amended run (15 quick rows,
  `tools/bench/sets/q10-sweep-quick.txt`): the "Portfolio:" line of each `portfolio4` log in
  `target/q11-q12/portfolio4/<board>.log`, and KiCad counts where Q12.5 grades a row. No run of
  its own.
  - *Done 2026-10-20:* from Q12.5's registered `portfolio4` run (the job logs'
    "Portfolio arm i" and "Portfolio:" lines, `target/orchestrator/q1112/out/portfolio4/<board>.log`),
    on the 11 eligible rows (Q12.json amendment 3). Router incomplete count per arm (0 configured,
    1 conflict-rounds, 2 inside-out, 3 shuffle-1); KiCad unconnected only for the kept arm's
    session (the other arms write no session):

    | board | arms 0-3 incomplete | min / median / max | kept arm | kept arm KiCad |
    |---|---|---|---|---|
    | Issue433-my-board | 2, 0, 2, 2 | 0 / 2 / 2 | 1 conflict-rounds | - |
    | Issue690-sonde_xilinx | 11, 11, 11, 11 | 11 / 11 / 11 | 0 configured | - |
    | Issue508-DAC2020_bm10 | 2, 1, 3, 1 | 1 / 1.5 / 3 | 1 conflict-rounds | - |
    | Issue093-interf_u | 19, 19, 19, 19 | 19 / 19 / 19 | 0 configured | - |
    | Issue555-CNH_Functional_Tester_1 | 1, 2, 1, 2 | 1 / 1.5 / 2 | 0 configured | - |
    | canary-pwr-9511d9a | 115, 107, 108, 112 | 107 / 110 / 115 | 1 conflict-rounds | 44 |
    | mydevboard-rp2040dev | 37, 31, 37, 32 | 31 / 34.5 / 37 | 1 conflict-rounds | 29 |
    | slice-dcmt-bread-slice | 4, 3, 4, 3 | 3 / 3.5 / 4 | 1 conflict-rounds | 9 |
    | flight-computer-core-board | 1, 1, 1, 1 | 1 / 1 / 1 | 0 configured | 5 |
    | laser-backplane-dvi-laserbackplane-dvi | 45, 45, 45, 45 | 45 / 45 / 45 | 0 configured | 70 |
    | vfo-controller-catvfoctrl | 1, 4, 2, 3 | 1 / 2.5 / 4 | 0 configured | 14 |

    The spread is 0 on four boards; where it is not, `conflict-rounds` wins 5 times and the
    configured order 2 (CNH, vfo); max minus min is 8 on canary-pwr, 6 on mydevboard, 3 on vfo, 2
    on Issue433 and bm10, 1 on CNH and slice. Proposed to F03 as the per-board noise quantum (the
    orchestrator's call, F03 amendment).
- [x] **Q12.5: measurement** (prereg `docs/roadmap/prereg/Q12.json`, registered).
  - Arms: `mp1`, `mpdef`, `portfolio4`, `portfolio4+restart2`, and `portfolio4` with
    Q10 and Q11 on.
  - Work is reported against `mpdef`'s, since the portfolio is meant to replace
    passes 2–10 at about the same total work.
  - **Accept:** measured.
  - Registered 2026-10-18 (before any measured number) as `docs/roadmap/prereg/Q12.json`,
    sha256 8467d77dcd989648691a0a29a4095aceb650bb6e3fcd2a20d707da2a2e0b5a98, pinned by
    `crates/freerouting-dev/tests/prereg_q12.rs`; N02 totals rule, Q11's boards
    (`tools/bench/sets/q10-sweep.txt`), `eligible` still to be filled as an amendment.
  - *Partial 2026-10-20:* not run; cut to minutes (decided 2026-10-20: measurement runs stay a
    few minutes). Amendment 1 of `docs/roadmap/prereg/Q12.json` (before any registered run;
    sha256 `342635f960aba7a644f97cceb009c9274dd2be62b62fd80b1bc4a7f065b2e9ab`, re-pinned in
    `crates/freerouting-dev/tests/prereg_q12.rs`, 4 passed): the 15 quick rows of
    `tools/bench/sets/q10-sweep-quick.txt` (route times and sources in that file), arms `mpdef`
    (baseline), `portfolio4` (candidate) and `mp1` (Q11.json's `mp1` command, its runs shared);
    `portfolio4+restart2` and `portfolio4+Q10+Q11` dropped; one KiCad grade per distinct session.
    heavy run pending (the build and variables of Q11.1's note: `B`, `O=target/q11-q12`, `S`, `C`,
    `K`, and `G="--gui.enabled=false --router.scoring.start_ripup_costs=1500
    --router.min_trace_width_um=150 --stats json"`; `--stats json` is output only, for
    `work_units` (`maze_work_units` of `<board>.ses.stats.json`); one route at a time, `ulimit -v 8000000`, `timeout 600` for
    canary-pwr and `timeout 120` otherwise through `tools/bench/quick-arm.sh`; about 14 minutes, of
    which canary-pwr-9511d9a's `mpdef` route takes up to about 7.5 (414 to 452 s in n06-13 and
    n06-g, under the 600 s limit; if it times out the measurement is incomplete) and its
    `portfolio4` route about 1.5):
    1. `mp1`: Q11.1's step 1 (shared; if Q11 has not run, `tools/bench/quick-arm.sh $B $O mp1 $S
       -- $F` first, about 2 minutes), then Q12.json's amendment 2 writes `eligible`;
    2. `tools/bench/quick-arm.sh $B $O mpdef $S -- $G` and `tools/bench/quick-arm.sh $B $O
       portfolio4 $S -- $G -mp 1 --router.autorouter.portfolio=4` (the `.time` files hold Q12.3's
       peak resident set);
    3. `for b in $C; do tools/bench/grade-distinct.sh $O $b target/corpus/projects/$b mp1 mpdef
       portfolio4; done; tools/bench/grade-distinct.sh $O canary-pwr-9511d9a $K mp1 mpdef
       portfolio4`;
    4. records as Q12.json's `grading` and `metric_definitions` say (`work_units` from the
       `--stats` totals), then `freerouting-dev gate --records <records> --prereg
       docs/roadmap/prereg/Q12.json --eligible` and `guard_extra` (work at most 1.5x `mpdef`'s)
       per row; Q12.4's spread table from the `portfolio4` logs.
  - *Done 2026-10-20:* measured under Q12.json amendments 1-3 (sha256 now
    `840f008bc17a5e1b3e1d1414cdc6cfe3f87a3e9e59490ed8d792ae81d871764d`, re-pinned in
    `crates/freerouting-dev/tests/prereg_q12.rs`, 6 passed): engine main `3330f36` (Q11.1's
    build), `mp1` shared with Q11.1, `eligible` the same 11 rows. **canary-pwr-9511d9a's `mpdef`
    route hit its 600 s limit** (`TIMEOUT mpdef canary-pwr-9511d9a after 600s`, interrupted
    after pass 17, 115 unrouted; other sessions' routes ran at the same time, load average about
    6), so by amendment 1 that row is incomplete and not graded; the stop rule allows no rerun.
    Per row: router unrouted (KiCad unconnected where graded) / vias; work `maze_work_units` of
    the `--stats` total against `mpdef`'s:

    | row | mp1 | mpdef | portfolio4 | work p4/mpdef: registered (4 threads) / 1-thread diagnostic | wall s mpdef / p4 |
    |---|---|---|---|---|---|
    | Issue433-my-board | 2 / 7 | 1 / 7 | 0 / 7 | 1.14 / 0.36 | 1.28 / 0.04 |
    | Issue649-kicad_ecc83-pp_input_board_v1 (not eligible) | 0 / 0 | 0 / 0 | 0 / 0 | 15.37 / 3.94 | 0.10 / 0.01 |
    | Issue508-DAC2020_bm02 (not eligible) | 0 / 14 | 0 / 10 | 0 / 14 | 1.25 / 0.42 | 0.93 / 0.16 |
    | Issue690-sonde_xilinx | 11 / 3 | 11 / 3 | 11 / 3 | 1.51 / 0.41 | 1.72 / 0.05 |
    | Issue508-DAC2020_bm07 (not eligible) | 0 / 32 | 0 / 32 | 0 / 32 | 1.78 / 0.52 | 3.31 / 0.60 |
    | Issue508-DAC2020_bm10 | 2 / 161 | 0 / 155 | 1 / 160 | 4.20 / 1.24 | 23.99 / 7.73 |
    | Issue093-interf_u | 19 / 84 | 19 / 84 | 19 / 84 | 1.63 / 0.41 | 3.64 / 0.11 |
    | Issue555-CNH_Functional_Tester_1 | 1 / 32 | 1 / 32 | 1 / 32 | 14.53 / 3.65 | 10.32 / 3.11 |
    | canary-pwr-9511d9a | 115 (50) / 165 | timeout at 600 s | 107 (44) / 162 | n/a | 600.05 / 59.49 |
    | mydevboard-rp2040dev | 37 (34) / 29 | 37 (34) / 29 | 31 (29) / 36 | 1.01 / 0.32 | 53.38 / 7.34 |
    | slice-dcmt-bread-slice | 4 (12) / 14 | 2 (11) / 13 | 3 (9) / 8 | 9.59 / 2.84 | 0.44 / 0.39 |
    | flight-computer-core-board | 1 (5) / 28 | 1 (6) / 24 | 1 (5) / 28 | 7.97 / 2.50 | 0.36 / 0.39 |
    | laser-backplane-dvi-laserbackplane-dvi | 45 (70) / 3 | 45 (70) / 3 | 45 (70) / 3 | 1.00 / 1.00 | 0.32 / 0.32 |
    | vfo-controller-catvfoctrl | 1 (14) / 33 | 0 (10) / 27 | 1 (14) / 33 | 1.22 / 0.39 | 2.29 / 0.87 |
    | mccx11-hardware-sx1262-433mhz-reverse-engineering (not eligible) | 0 (0) / 0 | 0 (0) / 0 | 0 (0) / 0 | 1.00 / 1.00 | 0.02 / 0.02 |

    Gate (records `target/orchestrator/q1112/rec-q12-mpdef-p4.json`, `rec-q12-mp1-p4.json`):
    - `freerouting-dev gate --records rec-q12-mpdef-p4.json --prereg docs/roadmap/prereg/Q12.json
      --write-baseline ...` refuses: "UNGRADED fixtures/canary-pwr/canary-pwr-9511d9a.dsn.mpdef:
      not graded: timeout (amendment 1: the measurement is incomplete). A board that can't be
      graded fails the run", "gate: FAIL".
    - Trial on the 10 other eligible rows (not the registered verdict; `--rule totals --accept
      improve` on a records copy without the canary row): total 163 -> 159 (change -4), set noise
      25.75; "FAILS: hard guard: fixtures/Issue508-DAC2020_bm10.dsn.below_minimum = 7",
      "...flight-computer-core-board.dsn.drc_real 1 -> 6", "...vfo-controller-catvfoctrl.dsn.below_minimum
      = 3", "...vfo-controller-catvfoctrl.dsn.drc_real 75 -> 85", "FAILS: the improvement 4 does
      not exceed the set noise 25.75"; "totals verdict: reject". No canary value can turn it: the
      hard guards fail on rows that ran.
    - `guard_extra` (work at most 1.5x `mpdef`'s on every board) fails on 6 eligible rows with the
      registered numbers (sonde 1.51, bm10 4.20, interf_u 1.63, CNH 14.53, slice 9.59, flight 7.97)
      and on 3 with the 1-thread numbers (CNH 3.65, slice 2.84, flight 2.50).
    - Reported pair `--arms mp1,portfolio4 --eligible`: total 220 -> 203 (change -17), noise 27.62,
      hard guard bm10 below_minimum = 8, "the improvement 17 does not exceed the set noise 27.62",
      "totals verdict: reject"; canary-pwr 50 -> 44 KiCad-unconnected, clean.
    - **Verdict: `portfolio4` fails (UNGRADED baseline row, hard guards, noise, guard_extra); the
      keep rule does not keep it.** The decision table's fail branch (remove
      `quality/portfolio/`) is not carried out here: it is reported for the orchestrator.
    - Work counter defect found here: with arms running at once, each arm's per-stage counters
      take in the other arms' work, so `--stats` totals of a parallel portfolio run are about 3.3
      to 4x the 1-thread run's for the same session (canary 10389292397 vs 3098294240, CNH
      2466277992 vs 619374061). `work_units` here is the registered (4-thread) figure; the
      1-thread figures are a diagnostic. Not fixed here (engine work).

## Expected [I]

 with the ±10 spread on canary-pwr, a best-of-4 should recover a few
connections. Upstream's clone-and-shuffle threads were deleted, but they shuffled
later passes, not pass 1.
