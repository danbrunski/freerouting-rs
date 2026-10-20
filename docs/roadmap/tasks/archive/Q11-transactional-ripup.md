# Q11 — Transactional rip-up

Opened 2026-10-17. In pass 1, a rip whose victim does not re-land is permanent (73 of
109 on canary-pwr). Java's rip cost (`MazeRipupResolver.checkRipup`, about
`ripupCosts × halfWidth / detour` [V]) ignores whether the victim can be rerouted.

Prerequisites: board parity (rung 3); A10 (`ConnectionRouter` wrapper slot), A11 (rip events, `ConnectionOutcome::ripped`), B01 (snapshots); Q05.1 (`rip-stats`); Q10's sweep for the combined arms; F03.1, F03.4; measured after P04. Code may start before P04.

Source: research by freerouting-rs-f1 at the owner's request (2026-10-17), `docs/roadmap/research/2026-10-17-routing-algorithms.md` (candidate N2); the preregistration draft is `docs/roadmap/prereg/Q11.json`, marked `DRAFT` until the implementer registers it (sha256s pinned) before the first number.

Practice and literature:
- KRT's "custody" rips [V, `docs/rip-up-reroute.md` at `0486e799`];
- rjwalters/kicad-tools #6342/#6355: all-or-nothing rollback plus a cap of 2 rips per
  (victim, blocker set) [V];
- Large Neighbourhood Search's destroy, repair, accept (Shaw, CP-98) [V].

No peer-reviewed PCB numbers exist [V: none found]. Java's own rip-cost trend is the
evidence.

## What exists

- `ConnectionOutcome::ripped` gives the ripped items in Java's order.
- B01 `BoardSnapshot` / `restore`.
- `ConnectionParams::ripup_allowed`.
- The routing ledger's incomplete count.
- Q05.1's `rip-stats` diagnostic (the re-land share is its natural metric).

## Decisions made here

- A `ConnectionRouter` wrapper, `transactional-ripup`, around the configured router.
  The search and its rip costs stay Java's; only the outcome is judged.
- Procedure:
  1. Snapshot. Route.
  2. If the outcome ripped something, re-route each victim connection at once with
     `ripup_allowed = false`, in ascending item-id order of the victims' `from` items.
  3. Commit by the arm's rule; otherwise restore the snapshot.
  4. After a restore, retry the connection once with rip-up off. If that fails too,
     record the failure as Java would. Java's after-failure net rip (`after_failure`)
     is unchanged.
- Snapshot cost: take it only when the search could rip (`ripup_allowed`), or use
  the undo journal if B01 offers one. The implementer measures which.

## File ownership

- Creates `quality/transaction.rs`.
- Appends to `quality/mod.rs` and `quality/register.rs`.
- Adds one `rip_count` field to `rip_stats` if needed.

## Checklist

- [x] **Q11.1: rip-cost sweep, no code.** `start_ripup_costs` ∈ {1500, 5000, 20000}
  and pass 1 with rip-up off, on the eligible boards. This bounds what Q11 can gain.
  - **Accept:** a table in the done note, with Q05.1 rip stats per arm.
  - *Not run 2026-10-18 (heavy run pending, the heavy slot is another agent's):* per eligible board
    of `tools/bench/sets/q10-sweep.txt` (Q11.json's rule) and per cost `C` in 1500, 5000,
    20000: `target/release/freerouting -de {DSN} -do {SES} --gui.enabled=false --router.scoring.start_ripup_costs=C --router.min_trace_width_um=150 -mp 1
    --router.special.diagnostics=rip-stats` (the result manifest's `rip_stats` is the table's
    per-arm rip stats), graded as Q11.json says. The rip-up-off arm has no switch: the batch
    passes always allow rip-up (`router.ripup_allowed` is the fanout's), and `-mp 0` with a
    one-pass sweep is not pass 1 (`-mp 0` means no pass limit; probed on `Issue093-interf_u`).
    Operator decision (2026-10-18): the arm runs as `start_ripup_costs=1000000`, labelled
    "near-off" in the table (each item's rip cost is capped at `i32::MAX/100`, so rip-up is
    priced out, not switched off); Q11.1 stays no-code. The code here supports every other arm.
  - *Partial 2026-10-20:* not run; cut to minutes (decided 2026-10-20: measurement runs stay a
    few minutes). Amendment 1 of `docs/roadmap/prereg/Q11.json` (before any registered run;
    sha256 `9a1c2d795c92c3849b745807debf2585397fad55f9729470e257c9ef4c0e783c`, re-pinned in
    `crates/freerouting-dev/tests/prereg_q11.rs`, 4 passed) registers the sweep as arms `mp1`
    (1500), `ripcost-5000`, `ripcost-20000` and `ripcost-near-off` (1000000) on the 15 quick rows
    of `tools/bench/sets/q10-sweep-quick.txt` (route times and their sources in that file), every
    command with `--router.special.diagnostics=rip-stats`; `ripcost-best` reuses the best arm's
    runs. heavy run pending (one release build of main, `CARGO_BUILD_JOBS=2 cargo build --release
    -p freerouting-cli`; from the repository root; one route at a time, every route under
    `ulimit -v 8000000` and `timeout 600` for canary-pwr or `timeout 120` otherwise, which
    `tools/bench/quick-arm.sh` applies; about 12 minutes: 4 canary-pwr pass-1 routes of about 44 s,
    the quick rows' routes, and at most 4 KiCad grades of canary-pwr and 24 of the corpus rows),
    with `B=target/release/freerouting; O=target/q11-q12; S=tools/bench/sets/q10-sweep-quick.txt;
  F="--gui.enabled=false --router.scoring.start_ripup_costs=1500 --router.min_trace_width_um=150 -mp 1
  --router.special.diagnostics=rip-stats --stats json"; C="mydevboard-rp2040dev slice-dcmt-bread-slice
  flight-computer-core-board laser-backplane-dvi-laserbackplane-dvi vfo-controller-catvfoctrl
  mccx11-hardware-sx1262-433mhz-reverse-engineering"; K=/home/thebo/code/canary/hardware/canary-pwr-rev1.0`:
    1. `tools/bench/quick-arm.sh $B $O mp1 $S -- $F`, then for `c` in 5000, 20000:
       `tools/bench/quick-arm.sh $B $O ripcost-$c $S -- $F --router.scoring.start_ripup_costs=$c`,
       and `tools/bench/quick-arm.sh $B $O ripcost-near-off $S -- $F
       --router.scoring.start_ripup_costs=1000000`;
    2. one KiCad grade per distinct session: `for b in $C; do tools/bench/grade-distinct.sh $O $b
       target/corpus/projects/$b mp1 ripcost-5000 ripcost-20000 ripcost-near-off; done;
       tools/bench/grade-distinct.sh $O canary-pwr-9511d9a $K mp1 ripcost-5000 ripcost-20000
       ripcost-near-off` (the 8 benchmark rows have no `.kicad_pcb`: the router's unrouted count);
    3. the table: per arm and row, unrouted (`$O/<arm>/<board>.result.json` `board_statistics`),
       KiCad unconnected, vias and the manifest's `rip_stats`; then Q11.json's amendment 2 writes
       `eligible` (rows where `mp1` leaves an incomplete connection) and names `ripcost-best`.
    Owner: this item, run in Q00's Q10-Q12 measurement family with Q11.6 and Q12.5 (Q12.5 reuses
    the `mp1` runs).
  - *Done 2026-10-20:* run under Q11.json amendments 1-3 (sha256 now
    `e57691f5b225400dd07e92d6a2b6ee24f4da87c1ab35f63fd255da9bc9b87139`, re-pinned in
    `crates/freerouting-dev/tests/prereg_q11.rs`, 6 passed). Amendment 2 (before any run): engine
    main `3330f36` (`freerouting --version`: git `3330f369a2ab`; one release build from a detached
    worktree, CLI sha256 `a6bc2d82…`, default profile `enhanced`), runner and metric sources made
    precise, hard guards "no new breach versus baseline". Amendment 3 (after `mp1`, before any
    candidate): `eligible` = the 11 rows `mp1` leaves incomplete (not Issue649, bm02, bm07,
    mccx11) and `ripcost-best` = `ripcost-5000`. Routes: `tools/bench/quick-arm.sh` (each under
    `ulimit -v 8000000`, `timeout 600`/`120`; no timeout), outputs in
    `target/orchestrator/q1112/out/<arm>/`; KiCad: `tools/bench/grade-distinct.sh`, 15 grades for
    the 24 corpus and canary sessions. Per row: router unrouted (KiCad unconnected where graded) /
    vias / rip stats `ripping_attempts-rips-reroutes` (the manifest's `rip_stats`):

    | row | mp1 | ripcost-5000 | ripcost-20000 | ripcost-near-off |
    |---|---|---|---|---|
    | Issue433-my-board | 2 / 7 / 0-0-0 | 2 / 7 / 0-0-0 | 2 / 7 / 0-0-0 | 2 / 7 / 0-0-0 |
    | Issue649-kicad_ecc83-pp_input_board_v1 | 0 / 0 / 0-0-0 | 0 / 0 / 0-0-0 | 0 / 0 / 0-0-0 | 0 / 0 / 0-0-0 |
    | Issue508-DAC2020_bm02 | 0 / 14 / 0-0-0 | 0 / 14 / 0-0-0 | 0 / 20 / 0-0-0 | 0 / 20 / 0-0-0 |
    | Issue690-sonde_xilinx | 11 / 3 / 0-0-0 | 11 / 3 / 0-0-0 | 11 / 3 / 0-0-0 | 11 / 3 / 0-0-0 |
    | Issue508-DAC2020_bm07 | 0 / 32 / 2-2-2 | 0 / 32 / 2-2-2 | 0 / 32 / 2-2-2 | 0 / 32 / 2-2-2 |
    | Issue508-DAC2020_bm10 | 2 / 161 / 10-12-10 | 2 / 161 / 10-12-10 | 2 / 161 / 11-12-10 | 0 / 165 / 8-8-8 |
    | Issue093-interf_u | 19 / 84 / 0-0-0 | 19 / 84 / 0-0-0 | 19 / 84 / 0-0-0 | 19 / 84 / 0-0-0 |
    | Issue555-CNH_Functional_Tester_1 | 1 / 32 / 1-1-1 | 1 / 32 / 1-1-1 | 1 / 32 / 1-1-1 | 1 / 32 / 1-1-1 |
    | canary-pwr-9511d9a | 115 (50) / 165 / 42-60-29 | 106 (42) / 177 / 39-54-23 | 105 (42) / 178 / 38-51-25 | 110 (44) / 163 / 47-59-26 |
    | mydevboard-rp2040dev | 37 (34) / 29 / 35-40-11 | 32 (29) / 36 / 34-46-14 | 39 (33) / 28 / 33-47-15 | 35 (31) / 43 / 26-36-16 |
    | slice-dcmt-bread-slice | 4 (12) / 14 / 3-3-1 | 4 (12) / 14 / 3-3-1 | 4 (12) / 14 / 3-3-1 | 4 (12) / 14 / 3-3-1 |
    | flight-computer-core-board | 1 (5) / 28 / 0-0-0 | 1 (5) / 28 / 0-0-0 | 1 (5) / 28 / 0-0-0 | 1 (5) / 28 / 0-0-0 |
    | laser-backplane-dvi-laserbackplane-dvi | 45 (70) / 3 / 0-0-0 | 45 (70) / 3 / 0-0-0 | 45 (70) / 3 / 0-0-0 | 45 (70) / 3 / 0-0-0 |
    | vfo-controller-catvfoctrl | 1 (14) / 33 / 6-7-4 | 1 (14) / 31 / 4-6-3 | 0 (13) / 24 / 2-2-1 | 0 (13) / 24 / 2-2-1 |
    | mccx11-hardware-sx1262-433mhz-reverse-engineering | 0 (0) / 0 / 0-0-0 | 0 (0) / 0 / 0-0-0 | 0 (0) / 0 / 0-0-0 | 0 (0) / 0 / 0-0-0 |
    | sum | 238 (185) / 559 / rips 123, reroutes 56 | 224 (172) / 576 / rips 122, reroutes 52 | 229 (175) / 562 / rips 116, reroutes 53 | 228 (175) / 566 / rips 109, reroutes 53 |

    Sums over the 11 eligible rows. KiCad unconnected over the six graded eligible rows: 185 /
    172 / 175 / 175, so `ripcost-best` is `ripcost-5000` (amendment 3). canary-pwr-9511d9a: 50
    / 42 / 42 / 44 KiCad-unconnected, every arm clean (no error but unconnected and starved
    thermals, 0 via-in-pad, 0 inner-layer tracks). "near-off" still rips (canary: 47 attempts,
    59 rips): the capped cost does not price rip-up out. Re-land share (reroutes / rips) on
    canary: 29/60, 23/54, 25/51, 26/59. Wall times overlapped other sessions' routes: reported,
    not compared.
- [x] **Q11.2: the wrapper, arm `strict`.** Commit only if every victim connection
  re-lands.
  - **Accept:**
    - a fixture where A rips B and B cannot re-land: A is retried without rip-up, and
      B keeps its copper;
    - a fixture where B can re-land: both are complete;
    - off is byte-identical;
    - determinism.
  - *Done 2026-10-18:* `quality::transaction::TransactionalRipup`, registered as
    `transactional-ripup` (`router.autorouter.connection_router`), settings
    `router.transaction.{rule, depth, rip_factor}` (unset strict, 0, 0; in no profile), an
    invalid value refused before routing
    (`invalid_transaction_settings_are_refused_before_routing`). Tests in
    `crates/freerouting/tests/quality_transaction.rs` (14 green):
    - B cannot re-land (`tests/data/quality/q11-locked-cross.dsn`):
      `a_victim_that_cannot_reland_keeps_its_copper_and_a_is_retried_without_ripup`: B keeps
      its wire, A fails after its rip-up-free retry, tally "2 transactions, 0 committed, 2
      rolled back (0 retries routed)" (A1 and A2 are both pass items); Java's pass 1 rips B
      there (`without_the_router_a_rips_b_in_pass_1_on_both_fixtures`);
    - B re-lands (`q06-sole-wall.dsn`): `a_victim_that_relands_completes_both_nets_in_pass_1`,
      incomplete 0 after pass 1 (Java: B incomplete until pass 2);
    - off: `off_is_byte_identical` (SES and job log, 4 fixtures × 1 and 3 passes, with the
      transaction settings set but the router unset, and with `freerouting-connection`
      named), `without_a_rip_the_router_routes_as_java`; the fast parity subset
      `PARITY_SET=tools/bench/sets/connection-parity.txt cargo test --release -p freerouting
      --test autoroute_board_parity -- --ignored autoroute_board_parity --test-threads=1`:
      "exact: 7 boards, 7 pass, 0 expected, 0 fail, 7 SES byte-identical"; full A14 on origin/main 81fcef7 (release, `--test-threads=1`, then the 21 subdirectory references with `PARITY_SUBDIRS=1`; logs `target/orchestrator/heavy/a14-81fcef7*.log`): 1 passed, 0 failed, 138 boards (120 exact, 15 cutoff, 3 Java failures), every SES byte-identical except the known Issue684 and Issue721 (expected), Issue110-RelayModule (non-ASCII name only) and the Java-failure boards;
    - determinism: `every_arm_is_deterministic` (SES and log, two fixtures, 3 passes, every
      arm); canary-pwr-9511d9a pass 1 with `rule=strict` twice: the same SES (sha256
      `062a1baeb0d0…`).
    - Snapshot cost, measured: a `Clone` of the routed canary-pwr board (2318 items) takes
      1.42 ms in release (a `BoardSnapshot` of the input 0.08 ms but its restore rebuilds the
      tree, changing later work counts), against 42.8 s for Java's pass 1 there; the clone is
      taken for every connection with rip-up allowed. Release spot checks on canary-pwr-9511d9a
      (`-de fixtures/canary-pwr/canary-pwr-9511d9a.dsn --gui.enabled=false --router.scoring.start_ripup_costs=1500 --router.min_trace_width_um=150 -mp 1`, router's own unrouted
      count, not a measurement): Java 104 unrouted in 42.8 s; strict 94 in 87.3 s (54
      transactions, 21 committed, 33 rolled back, 102 victim nets, 56 re-landed);
      `Issue093-interf_u` rips nothing in pass 1, every arm's SES is Java's.
- [x] **Q11.3: arm `count`.** Commit if the ledger's incomplete count strictly
  decreases. A ripped multi-pin net may re-land along a different topology.
  - **Accept:** as for Q11.2.
  - *Done 2026-10-18:* `CommitRule::Count` (`router.transaction.rule=count`): the board is kept
    when the routing ledger's incomplete count is strictly below the count before the
    transaction. The same tests cover it (`a_victim_that_cannot_reland_...`,
    `a_victim_that_relands_...`, `off_is_byte_identical`, `every_arm_is_deterministic`,
    `the_rule_decides_on_the_ledger`). Spot check on canary-pwr-9511d9a pass 1: 96 unrouted
    in 85.8 s (53 transactions, 17 committed, 36 rolled back).
- [x] **Q11.4: arm `depth1`** (optional). Victims may rip at depth 1, under the same
  commit rule, with no victim ripped twice in one transaction.
  - **Accept:** as for Q11.2, plus a termination test on a 3-net cycle fixture.
  - *Done 2026-10-18:* `router.transaction.depth=1` (0 or 1, anything else refused): wave-0
    victims route with rip-up on; nets they rip are a second wave routed with rip-up off; a
    victim attempt that rips the routed net or a net of the transaction again is undone and
    routed without rip-up. Termination on `tests/data/quality/q11-cycle.dsn` (nets A, B, C)
    with a scripted inner router that rips A→B→C→A: `depth_1_ends_on_a_three_net_cycle`
    (one transaction, two waves, 2 victim nets, rolled back, every wire back),
    `depth_1_never_rips_a_net_twice` (A rips B and C; both re-rips refused),
    `depth_0_routes_victims_without_ripup`, and the real router on the cycle board
    (`the_real_router_ends_on_the_cycle_board_with_every_arm`). The Q11.2 clauses hold for
    the `depth1` arm in the same tests. Spot check on canary-pwr-9511d9a pass 1 (`count`,
    depth 1): 89 unrouted in 109.4 s (47 transactions, 19 committed, 28 rolled back, 139
    victim nets, 113 re-landed, 43 re-rips refused).
- [x] **Q11.5: rip-count protection** (optional; needs a one-line engine hook). The rip
  cost of an item is multiplied by (1 + 2k), where k is the number of times its
  connection was ripped this run. KRT measured P(ripped again | ripped k times) ≈ 0.9.
  - The hook: `RipupPolicy::ripup_factor(item) -> i32`, 1 by default and not
    consulted in the parity path.
  - **Accept:** off is byte-identical; determinism.
  - *Done 2026-10-18:* `router.transaction.rip_factor=f`: the transactional router counts per net
    the committed transactions that ripped it (`k`) and hands the search the factors
    `1 + f·k`. The hook is `ConnectionParams::ripup_factors` (copied into
    `AutorouteControl::ripup_factors`, read in `check_ripup`), not a `RipupPolicy` method:
    the ripup policy does not reach the maze search, and the router that counts the rips is
    the one that builds the parameters. `None` everywhere else, so Java's search never reads
    it. Tests: `the_rip_factor_multiplies_the_rip_cost_of_the_ripped_net_only` (B's rip cost
    ×5 with factor 5 for B; a factor for A or of 1 changes nothing),
    `the_factors_are_one_plus_f_times_k` (unit), `a_rip_factor_of_0_is_unset`, and the
    `strict+ripfactor` arm in `off_is_byte_identical` and `every_arm_is_deterministic`. Spot
    check on canary-pwr-9511d9a pass 1 (strict, f=2): 95 unrouted in 91.6 s.
- [x] **Q11.6: measurement** (prereg draft `docs/roadmap/prereg/Q11.json (draft)`).
  - Arms: `mp1` (baseline), `Q11.1-best`, `strict`, `count`, `depth1`, `strict+Q11.5`,
    each also followed by an Q10 sweep.
  - **Accept:** measured.
  - *Not run 2026-10-18 (heavy run pending):* registered first as `docs/roadmap/prereg/Q11.json`
    (sha256 `d1757b3d…`, pinned by `crates/freerouting-dev/tests/prereg_q11.rs`; N02's totals
    rule, Q10's board set and eligibility). Per eligible board and arm, the arm's `command`
    from Q11.json, e.g. strict: `target/release/freerouting -de {DSN} -do {SES} --gui.enabled=false --router.scoring.start_ripup_costs=1500 --router.min_trace_width_um=150 -mp 1
    --router.autorouter.connection_router=transactional-ripup --router.transaction.rule=strict`,
    each also with `--router.autorouter.sweep=free --router.autorouter.sweep_passes=2`, plus
    `--router.special.diagnostics=rip-stats` for the rip table; graded as Q11.json says, then
    `freerouting-dev gate --prereg docs/roadmap/prereg/Q11.json` per pair. Q11.1's
    `ripcost-best` value and the `eligible` list go into its `amendments` first.
  - *Partial 2026-10-20:* not run; cut to minutes (decided 2026-10-20: measurement runs stay a
    few minutes) by amendment 1 of Q11.json (sha256 `9a1c2d79…`, see Q11.1): 15 quick rows, arms
    `mp1`, `ripcost-best` and `strict` only (`count`, `depth1`, `strict+ripfactor` and every
    `+sweep` arm dropped, so the question's second half, after Q10's sweep, is not measured),
    pairs (`mp1`, `strict`) and (`ripcost-best`, `strict`), one KiCad grade per distinct session.
    Its `mp1` command is Q12.json's `mp1` (Q12.5 reuses the runs). heavy run pending, after
    Q11.1's steps (same build and variables), about 5 minutes (one canary-pwr pass-1 route of
    about 90 s with the wrapper, the quick rows, at most 7 KiCad grades):
    1. `tools/bench/quick-arm.sh $B $O strict $S -- $F
       --router.autorouter.connection_router=transactional-ripup --router.transaction.rule=strict`;
    2. `for b in $C; do tools/bench/grade-distinct.sh $O $b target/corpus/projects/$b mp1 strict;
       done; tools/bench/grade-distinct.sh $O canary-pwr-9511d9a $K mp1 strict` (a session equal
       to `mp1`'s shares its grade; `ripcost-best`'s grades are Q11.1's);
    3. records as Q11.json's `grading` says, then `freerouting-dev gate --records <records>
       --prereg docs/roadmap/prereg/Q11.json --eligible` (the pair `mp1`, `strict`) and the same
       with `--arms ripcost-best,strict`; the job log's "Transactional rip-up" tally per row.
  - *Done 2026-10-20:* measured under Q11.json amendments 1-3 (Q11.1's note: engine main
    `3330f36`, the same build and runner). `strict` routed every row inside its limit
    (canary-pwr-9511d9a 126.4 s). Per row: router unrouted (KiCad unconnected where graded) /
    vias / raw `below_minimum` (KiCad `track_width` errors on graded rows, else engine-model
    traces under the narrowest class width); the tally is the job log's "Transactional rip-up"
    line (transactions, committed, rolled back (retries routed), victim nets, re-landed; 0 re-rips
    refused everywhere):

    | row | mp1 | ripcost-best (5000) | strict | strict tally |
    |---|---|---|---|---|
    | Issue433-my-board | 2 / 7 / 0 | 2 / 7 / 0 | 2 / 7 / 0 | 0, 0, 0 (0), 0, 0 |
    | Issue649-kicad_ecc83-pp_input_board_v1 (not eligible) | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 0 | 0, 0, 0 (0), 0, 0 |
    | Issue508-DAC2020_bm02 (not eligible) | 0 / 14 / 19 | 0 / 14 / 19 | 0 / 14 / 19 | 0, 0, 0 (0), 0, 0 |
    | Issue690-sonde_xilinx | 11 / 3 / 0 | 11 / 3 / 0 | 11 / 3 / 0 | 0, 0, 0 (0), 0, 0 |
    | Issue508-DAC2020_bm07 (not eligible) | 0 / 32 / 45 | 0 / 32 / 45 | 0 / 32 / 45 | 2, 2, 0 (0), 2, 2 |
    | Issue508-DAC2020_bm10 | 2 / 161 / 60 | 2 / 161 / 60 | 0 / 157 / 64 | 9, 9, 0 (0), 10, 10 |
    | Issue093-interf_u | 19 / 84 / 0 | 19 / 84 / 0 | 19 / 84 / 0 | 0, 0, 0 (0), 0, 0 |
    | Issue555-CNH_Functional_Tester_1 | 1 / 32 / 30 | 1 / 32 / 30 | 1 / 32 / 29 | 1, 1, 0 (0), 1, 1 |
    | canary-pwr-9511d9a | 115 (50) / 165 / 0 | 106 (42) / 177 / 0 | 92 (31) / 208 / 0 | 52, 30, 22 (2), 98, 72 |
    | mydevboard-rp2040dev | 37 (34) / 29 / 74 | 32 (29) / 36 / 57 | 31 (29) / 33 / 72 | 43, 22, 21 (0), 86, 65 |
    | slice-dcmt-bread-slice | 4 (12) / 14 / 16 | 4 (12) / 14 / 16 | 2 (11) / 15 / 16 | 3, 3, 0 (0), 3, 3 |
    | flight-computer-core-board | 1 (5) / 28 / 0 | 1 (5) / 28 / 0 | 1 (5) / 28 / 0 | 0, 0, 0 (0), 0, 0 |
    | laser-backplane-dvi-laserbackplane-dvi | 45 (70) / 3 / 10 | 45 (70) / 3 / 10 | 45 (70) / 3 / 10 | 0, 0, 0 (0), 0, 0 |
    | vfo-controller-catvfoctrl | 1 (14) / 33 / 72 | 1 (14) / 31 / 56 | 1 (13) / 35 / 77 | 5, 3, 2 (0), 7, 4 |
    | mccx11-hardware-sx1262-433mhz-reverse-engineering (not eligible) | 0 (0) / 0 / 0 | 0 (0) / 0 / 0 | 0 (0) / 0 / 0 | 0, 0, 0 (0), 0, 0 |

    Records: `target/orchestrator/q1112/rec-q11-mp1-strict.json`, `rec-q11-best-strict.json`
    (option (b) against each pair's baseline). Gate:
    - `freerouting-dev gate --records rec-q11-mp1-strict.json --baseline base-q11.json --prereg
      docs/roadmap/prereg/Q11.json --eligible`: "0 refusal(s)", "judged 11 of 15 row(s)", total
      220 -> 192 (change -28), set noise 27.62; "FAILS: hard guard:
      fixtures/Issue508-DAC2020_bm10.dsn.below_minimum = 4", "... vfo-controller-catvfoctrl.dsn.below_minimum
      = 5", "... vfo-controller-catvfoctrl.dsn.drc_real 85 -> 93"; "totals verdict: reject", "gate:
      FAIL" (exit 1).
    - `... --records rec-q11-best-strict.json --eligible --arms ripcost-best,strict`: total 207 ->
      192 (change -15), noise 27.62; hard guard below_minimum on bm10 (4), mydevboard (15), vfo
      (21), drc_real mydevboard 65 -> 88 and vfo 70 -> 93; "FAILS: the improvement 15 does not
      exceed the set noise 27.62"; "totals verdict: reject", "gate: FAIL".
    - **Verdict: `strict` fails both pairs, so the keep rule does not keep it.** canary-pwr gains
      19 KiCad-unconnected against `mp1` (50 -> 31) and 11 against `ripcost-best` (42 -> 31), with
      43 more vias than `mp1`; the rejection comes from new narrow traces (the commands' 150 um
      floor with `enhanced`'s neck-down, on the extra connections) and, against `ripcost-best`,
      from the summed improvement. The decision table's fail branch (remove `quality/transaction.rs`
      and the rip-factor hook) is not carried out here: it is reported for the orchestrator.
    - decided 2026-10-20: kept, not removed; follow-up [Q18](../Q18-board-minimums-and-strict-ripup.md).

## Expected

 the upper bound on canary-pwr is the 73 routed-then-ripped
connections. Fewer will survive, because a rolled-back connection may then fail. No
literature number exists; a guess of 5–15 % fewer unconnected is [I].
