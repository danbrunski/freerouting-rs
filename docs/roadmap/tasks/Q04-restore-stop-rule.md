# Q04 — Stop when restores stop helping

Opened 2026-10-15. On `canary-pwr`, later passes never beat pass 1. Java's
batch loop restores the best board every few passes and stops only after
about 18 passes, which costs 3–5 minutes per run (Java fork HANDOVER, open
idea 1; on `Issue269-z10_module` the passes after a restore repeat exactly).
This checklist adds a work-based stop rule as an alternative pass controller,
and KRT's two-axis output guard, which never writes a board worse than the
input.

Prerequisites: board parity (rung 3); A10 (the `PassController` trait and
registry) and A13 (the pass loop) and A08 (board history); B01 (snapshots);
F03.1, F03.4.

**Fixtures:** `fixtures/canary-pwr/canary-pwr-pours1..3.dsn`, the variant set, with Java sessions at `a773a70ec` (see `fixtures/canary-pwr/README.md`).

## What exists and what this adds

| Exists | State at cda540f |
|---|---|
| Java `AutorouteBatchLoop.run` (`autoroute/pipeline/AutorouteBatchLoop.java`): restore check when `bh.size() >= STOP_AT_PASS_MINIMUM` (8) and `currentPass % STOP_AT_PASS_MODULO == 0` (4), restore if `bh.getMaxScore() > boardScoreAfter` (strict); `bh.restoreBoard(MAXIMUM_TRIES_ON_THE_SAME_BOARD = 3)`, stop when it returns null | the restore cadence |
| Java pass-local and global stagnation: `STAGNATION_PASS_LIMIT = 10` passes without a gain above `STAGNATION_SCORE_THRESHOLD = 0.5F` | the stop that ends `canary-pwr` runs around pass 18 |
| Java `BoardHistory` (cap `MAX_HISTORY_SIZE`), final `restoreBestBoard` | the best board is the one saved (D8) |
| A10 `PassController` trait, `FreeroutingPassController` (A10, port of the above) | the default implementation |
| KRT `GridSearch::extend_budget` (#529): a tranche of work must improve the best heuristic by `max(2 cells, 2 % of best_h)` | the quantum idea, at search level |
| KRT `improvement_gate.py` (#600): revert the run if it ends worse on nets lost against gained **or** on disconnected pads, "neither axis may outvote the other" | the output guard |

**Measured in Java (the Java session, 2026-10-15, `a773a70ec`, deterministic,
`canary-pwr` "pours1"):**
- **Pass 1 is the best pass at every ripup cost.** With `start_ripup_costs`
  100, 500 and 1500, pass 1 leaves 120, 109 and 100 connections unrouted.
  Every later pass is worse (161–185), and the loop's periodic restore puts the
  pass-1 board back.
- **The later passes are wasted time.** `-mp 1` gives the same SES as
  `-mp 10`, so passes 2–10 are about 60 % of the routing time for nothing.
- **Cause:** Java's ripup is greedy. Ripped blockers often can't be rerouted,
  and nothing remembers congestion between passes.

This is the strongest case for Q04 on Dan's board. A stop rule that ends the
run after the first restore that can't beat pass 1 would have saved about
60 % of the routing time with a byte-identical result.

## Decisions made here

- **A wrapper, not a rewrite.** `QuantumStop` wraps `FreeroutingPassController`
  and forwards every call. It adds one stop decision in `after_pass`; every
  other decision (restore, recovery, last-mile, final best restore) stays
  Java's. Id `quantum-stop`, selected by `router.autorouter.pass_controller`.
- **The rule:** at each of Java's restore checks, the gain is the best score
  in history minus the best score at the previous check (at the first check:
  minus the score after pass 1). A gain below `max(q_abs, q_pct · |best|)`
  (defaults 1.0 and 1 %) counts as none. After G grace checks (default 0:
  Java's 8 passes before the first check already give grace), stop after N
  consecutive checks without a gain (default 2). Defaults are preregistered
  and changed only through amendments.
- **State is counters and scores only**: pass number, check count, best score
  at the last check (`f32`, as Java's score). No board references, so
  `reset()` after a restore is not needed for this state (A10's contract).
- **The output guard is a separate wrapper on `finish`.** It compares the
  board about to be saved against the input board on two axes: connections
  (completed connections lost against gained) and unconnected pins. If either
  axis is worse, it saves the best board in history that is worse on neither,
  or the input board if none is. Id `output-guard`, combinable with
  `quantum-stop`. On a board with no input wiring it can never fire.

## File ownership

| Item | New files | May edit |
|---|---|---|
| Q04.1 | `crates/freerouting/src/quality/stop_rule.rs` | `quality/mod.rs`, `quality/register.rs` (append) |
| Q04.2 | `crates/freerouting/tests/quality_stop_rule.rs` | — |
| Q04.3 | `crates/freerouting/src/quality/output_guard.rs`, `fixtures/quality/q04-prerouted.dsn` | `quality/mod.rs`, `quality/register.rs` (append) |
| Q04.4 | the Q04 preregistration and baseline files | `docs/DIVERGENCES.md` (append, only if kept) |

## Checklist

- [x] **Q04.1 — `QuantumStop` pass controller.**
  Java source for the wrapped behaviour: `AutorouteBatchLoop.run` lines
  around 331–613 at cda540f (restore check, stagnation, final restore).
  Rust design: `quality::stop_rule::QuantumStop { inner:
  FreeroutingPassController, grace: u32, n: u32, q_abs: f32, q_pct: f32, ... }`.
  `after_pass` calls `inner.after_pass` first; if the inner decision is
  `Continue` or `Restore` and the rule fires, it returns `Stop(Reason::Quantum)`
  instead. A `Stop` from the inner controller always wins.
  - **Orders that decide results:** the rule is evaluated only on passes where
    Java runs its restore check (`pass % 4 == 0`, history ≥ 8), after Java's
    decision; float comparisons use `f32` exactly as Java's score.
  - **Accept:** table-driven unit tests over synthetic score sequences: a
    flat score stops at the second check (pass 12 with the defaults, where
    the Java controller alone stops at 18); a score that rises by more than
    the quantum at every check never stops early; a rise just below the
    quantum counts as no gain; an inner `Stop` passes through unchanged.

  *Done 2026-10-18:* `crates/freerouting/src/quality/stop_rule.rs`: `QuantumStop { inner:
  FreeroutingPassController, rule: StopRule { grace, checks, quantum_abs, quantum_pct }, ... }`,
  id `quantum-stop` (registered in `quality/register.rs`, listed by the stage catalog's pass
  controller slot), new `StopReason::Quantum` (requests a router stop like Java's stagnation
  stops; the loop logs "... Stopping the auto-router (quantum-stop)."). Settings: new section
  `quantum_stop.{grace, checks, quantum_abs, quantum_pct}` (all `opt`, unset = 0, 2, 1.0, 0.01,
  in no profile; `tests/settings_defaults.rs` lists them), checked before routing
  (`stop_rule::check_settings`: grace < 0, checks < 1, a negative or non-finite quantum fail the
  job naming the key). The rule runs only where Java's periodic restore check runs (history >= 8,
  pass >= 8, pass % 4 == 0), after the inner decision, in `f32`: best = max(history best, this
  pass's score), gain against the best at the previous check (the first: the score after pass
  1), a miss when gain < max(q_abs, q_pct * |best|). One refinement of the design: besides
  `Continue` and `Restore`, a `FanoutRecovery` or `LastMileRipup` decided at the firing pass is
  replaced by the stop too (they only prepare the next pass; ripping first would leave the final
  restore without this pass's board, which is not in the history yet). Tests
  `crates/freerouting/tests/quality_stop_rule.rs` (`cargo test -p freerouting --test
  quality_stop_rule`: 9 passed, Q04.1 and Q04.2 parts): `the_rule_over_synthetic_score_sequences`
  (9 rows: flat stops at pass 12; rising by 10 per pass and +11 per check equal Java decision for
  decision; +9 per check, just below 1 % of ~1030, stops at 16; quantum_abs 9 with gain 9 counts,
  9.001 does not; grace 1 stops at 16; checks 1 at 8; nothing left to route at pass 12 gives
  `Stop(Completed)`, the inner stop winning; every decision before the stop is Java's),
  `a_flat_score_stops_at_the_second_check_not_at_pass_18` (Java alone: `Stop(Stagnation)` at 18;
  Java's last-mile ripup at 12 becomes the stop), `a_restore_at_the_firing_check_becomes_the_stop`,
  `the_inner_controller_sees_every_pass`, `the_rule_reads_its_settings_and_refuses_bad_values`,
  `quantum_stop_is_registered_and_selected_by_the_pass_controller_key`. DIVERGENCES line under
  "freerouting (board model)" with the other settings fields.

- [~] **Q04.2 — Byte-identical where it only saves time.**
  For each board in `benchmark` and `owner`, route with the default controller
  and with `quantum-stop`, and record the pass of the last score improvement
  in the default run.
  - **Accept:**
    - on every board where `quantum-stop` fires after that pass, `ses-diff`
      shows identical sessions (the final best-board restore makes them so);
    - the done note lists, per board, passes and wall time for both arms,
      and the boards where it fired before the last improvement (those go to
      Q04.4);
    - `tools/determinism-check.sh` passes on `canary-pwr`.

  *Partial 2026-10-18:* heavy run pending (another agent held the heavy-job slot; only runs
  under 2 minutes were allowed). Done: the test file `crates/freerouting/tests/quality_stop_rule.rs`
  (Q04.2 part, the batch loop with a scripted pass runner): `a_stop_after_the_last_improvement_saves_the_same_session`
  (pass 1 best, later passes worse: Java restores and stops at pass 18, quantum-stop at 12 with
  `StopReason::Quantum`, both end with the final best-board restore and the SES is equal),
  `a_stop_before_the_last_improvement_changes_the_result` (improvement at pass 14: the sessions
  differ, the Q04.4 case), `the_default_pass_controller_is_java_s`. Short release check (110 s
  limit per run, CLI of this tree, optimizer off; wall times under load, about 3.4, so not
  timings), default / quantum-stop passes and seconds, default last improvement, SES:
  `Issue433-my-board` 3/3, 0.04/0.05 s, 2, identical; `ecc83` 2/2, 1, identical; `bm02` 2/2, 1,
  identical; **`Issue690-sonde_xilinx` 18/12, 0.37/0.26 s, last improvement 1, quantum-stop fired
  at 12, identical**; `bm07` 3/3, identical; `bm01` 11/11, 35.4/35.3 s, identical; `bm10` 5/5,
  identical; `Issue093-interf_u` 18/18, 1.55/1.61 s, last improvement 1, identical, the rule never
  checked (its 18 passes leave one and the same board, so the history never holds 8 boards and
  Java runs no restore check); `CNH_Functional_Tester_1` 18/18, last improvement 3, identical (4
  distinct boards, no check). `Issue269-z10_module` and `kit-dev-coldfire-xilinx_5213` did not
  finish the default arm in 110 s; the larger boards and canary-pwr were not run. No board so far
  stopped before its last improvement. Remainder, with the release CLI of the tree that lands Q04:
  `/home/thebo/code/freerouting-rs/target/orchestrator/q04/q04-heavy.sh target/release/freerouting <out> 7200 tools/bench/sets/benchmark.txt tools/bench/sets/owner.txt`
  (`/home/thebo/code/freerouting-rs/target/orchestrator/q04/q04-heavy.sh` (outside the repository; it routes each board of the sets with the default controller, with `--router.autorouter.pass_controller=quantum-stop` and, with `GUARD=1`, with `--router.autorouter.output_guard=true`, optimizer off, passes unlimited, one run at a time, and prints passes, wall seconds, the default run's last score improvement, where quantum-stop stopped, and `cmp` of the SES files)), then
  `tools/determinism-check.sh --out <out>/det -- target/release/freerouting -de fixtures/canary-pwr/canary-pwr-pours1.dsn -do {OUT} --router.optimizer.enabled=false --router.autorouter.pass_controller=quantum-stop`;
  the boards where quantum-stop stops before the last improvement go to Q04.4 (an amendment of
  `docs/roadmap/prereg/Q04.json` narrows its rows), and the plugin's stop option (K09.7) waits for
  canary-pwr's row.

- [~] **Q04.3 — Two-axis output guard.**
  Source: KRT `improvement_gate.py` (#600; on spartan6_4layer a run lost 36
  nets and gained 43 while disconnected pads went from 83 to 154).
  Rust design: `quality::output_guard::OutputGuard` wraps any
  `PassController`; in `finish` it computes, from `drc::incompletes`,
  completed connections and unconnected pins for the input board (kept from
  `start`) and for each candidate board, in history order best-first.
  - **Accept:**
    - `q04-prerouted.dsn` (pre-routed wiring that the batch run rips and
      fails to restore, built so the routed board loses 2 connections and
      gains 3): the guard writes a board that is not worse on either axis and
      logs both axes;
    - on every `benchmark` board (no input wiring) the guard never fires
      and the SES is identical.

  *Partial 2026-10-18:* the guard is in and the fixture clause holds; the run over every
  `benchmark` board is pending (heavy-job slot; N05 decides `enhanced`). Code:
  `crates/freerouting/src/quality/output_guard.rs` (`OutputGuard`, `PinGroups`, `Axes`,
  `GuardOutcome`), on with the new settings field `autorouter.output_guard` (`opt` bool, unset is
  off, in no profile). It is not a `PassController` wrapper: A10's controller sees no board, so the
  batch loop keeps the input board (a B01 snapshot before the fanout) when the setting is on and
  calls the guard in its finish, after the controller's best-board restore and before the final
  tail removal; the history is read best first without changing it
  (`BoardHistory::best_first`); combinable with `quantum-stop` (a setting next to the controller
  id). The axes come from `BasicBoard::connected_set` per pin (the guard needs which pins are
  joined; `drc::incompletes` gives only counts). The fixture is
  `crates/freerouting/tests/data/quality/q04-prerouted.dsn` (the repository's convention since
  Q06, not `fixtures/quality/`): one layer, nets B and C routed as walls, net A with A1-A2 routed
  and A3-A5 beyond them. Tests `crates/freerouting/tests/quality_output_guard.rs` (`cargo test -p
  freerouting --test quality_output_guard`: 9 passed):
  `the_batch_run_rips_the_walls_and_the_guard_saves_the_input_wiring` (the real router with `-mp
  1` rips both walls and never restores them: lost 2, gained 3, unconnected pins 3 -> 4, the
  routed board scores better so Java keeps it; with the guard the log has "Output guard: the routed
  board against the input: completed connections lost 2, gained 3; unconnected pins 3 -> 4." and
  the board before pass 1 is saved, worse on neither axis),
  `with_more_passes_the_guarded_run_is_never_worse_than_the_input` (`-mp` 2, 3, 5),
  `in_the_loop_the_guard_saves_a_board_not_worse_than_the_input` (scripted pass),
  `the_guard_takes_the_best_history_board_worse_on_neither_axis`,
  `with_no_history_board_the_guard_saves_the_input`, `a_board_worse_on_neither_axis_is_kept`,
  `the_axes_count_lost_and_gained_connections_and_unconnected_pins`,
  `the_input_has_one_connection_of_a_and_the_two_walls`,
  `without_input_wiring_the_guard_never_fires` (two-pin board and `Issue433-my-board`, SES equal).
  Release CLI, nine `benchmark` boards (433, ecc83, bm02, sonde, bm07, bm01, bm10, interf_u, CNH):
  the guard kept the routed board on all nine and every SES equals the unguarded one. Remainder:
  `GUARD=1 /home/thebo/code/freerouting-rs/target/orchestrator/q04/q04-heavy.sh target/release/freerouting <out> 7200 tools/bench/sets/benchmark.txt`
  (columns `guard_identical` and `guard_kept` must be `yes` on all 16 rows).

- [~] **Q04.4 — Measurement where the stop changes results.**
  Only boards where Q04.2 found an earlier stop. Preregister (F03.4): arms
  default and `quantum-stop`; signal: wall time; guards: unrouted, vias,
  `drc_real`.
  - **Accept:** measured: no regression in unrouted on any board, and time
    saved reported per board. If unrouted rises anywhere, tune only through a
    preregistration amendment, or reject and keep the numbers here. If kept,
    a DIVERGENCES line with the time saved on `canary-pwr`.

  *Partial 2026-10-18:* preregistered, not measured. `docs/roadmap/prereg/Q04.json` (sha256
  8b587d03a724cc9f6e03c7f32beb3e28e9134982196cc39256d7fc49807e3679 at registration (amendment 1 of Q04.5 changed it, see there), pinned by
  `crates/freerouting-dev/tests/prereg_q04.rs`, 3 passed): arms `default` and `quantum-stop`
  (CLI, optimizer off, passes unlimited), N02's totals rule with the speed clause (signal
  `unrouted` within noise, `wall_seconds` down >= 20 %), guards `unrouted`, `vias` and
  `clearance_violations` (the router's DRC stands for `drc_real`: the boards are DSN files) not
  rising on any board, `via_in_pad` hard, the defaults (0, 2, 1.0, 0.01) stated. Rows: the pool is
  the 16 `benchmark` boards and the five in-repository canary-pwr variants with their hashes; an
  amendment narrows it to the boards Q04.2 finds stopping before their last improvement, before
  any Q04.4 number. The registration was written after the sonde check and while the short Q04.2
  check ran, before any of its rows was read; no Q04.4 number exists. Remainder: Q04.2's heavy run,
  the amendment, then per arm and board the registered commands on a quiet machine, `freerouting-dev
  gate collect` and `freerouting-dev gate --prereg docs/roadmap/prereg/Q04.json`; if kept, the
  DIVERGENCES line with canary-pwr's time saved, and N05 decides `enhanced`.
- [x] **Q04.5 — Repeat stop.** Added 2026-10-18 from Q04.2's short check: on boards whose
  passes leave the same board (`Issue093-interf_u`: 18 passes, one board; CNH: 4 distinct
  boards), the history never holds 8 boards, so no restore check runs and `quantum-stop` saves
  nothing. An opt-in `quantum_stop` option stops when a pass leaves the board with the state
  hash it started with; the session is byte-identical where the later passes would also have left the board alone (not by construction: see the done note).
  - **Accept:**
    - a fixture whose passes repeat stops at the first repeat with an identical session;
    - unset, routing is unchanged;
    - registered as a Q04.json amendment before it is measured in Q04.4.

  *Done 2026-10-18:* setting `quantum_stop.repeat` (`bool`, `opt`, unset = off, in no profile;
  CLI `--router.quantum_stop.repeat=true`; listed in the catalog's pass controller slot and in
  `tests/settings_defaults.rs`). It lives in the batch loop (`autoroute/batch/batch_loop.rs`,
  `Run::passes`), not in `QuantumStop`, so it works with any pass controller: after
  `after_pass`, if the board's `state_hash()` (the one the history and the pass log line use, no
  second hash) equals the hash taken before the pass and the controller did not already decide
  `Stop`, the decision becomes the new `StopReason::Repeat` (requests a router stop; logged
  "The auto-router pass left the board in the state it started with (score ..., N items still
  unconnected). Stopping the auto-router (quantum-stop repeat)."). The final best-board restore is
  unchanged. Unset it costs nothing (no hash after the pass). Tests (`cargo test -p freerouting
  --test quality_stop_rule`: 14 passed, 5 new): `the_first_repeated_board_state_stops_the_loop`
  (scripted runner, board changes to pass 3: stops at pass 4 with `Repeat`, the log line, SES
  equal to the run without it, which goes on to Java's stagnation stop past pass 10),
  `a_board_that_changes_every_pass_never_repeats` (same stop, passes and SES; `Some(false)` is
  off), `a_repeat_on_the_second_pass_stops_there` and `the_controller_s_stop_wins_over_the_repeat`
  (a pass that repeats and finds nothing left to route ends as `Completed`), and **`issue093_interf_u_stops_at_the_first_repeat_with_an_identical_session`**
  (real router, `fixtures/Issue093-interf_u.dsn`, fanout and optimizer off, passes unlimited:
  18 passes with one board hash in the full run, with `repeat` 2 passes, `StopReason::Repeat`,
  SES byte-identical). Release CLI spot check (`freerouting -de <dsn> -do <ses>
  --router.optimizer.enabled=false [--router.quantum_stop.repeat=true]`, machine under load, so
  walls are not timings), passes and wall, off / on: `Issue093-interf_u` 18 / 2, 1.28 s / 0.22 s,
  **SES identical**; `Issue433-my-board` 3 / 3, `Issue649-kicad_ecc83` 2 / 2, `Issue690-sonde_xilinx`
  18 / 18 (never repeats): identical SES, equal passes, the option changes nothing; and
  **`Issue555-CNH_Functional_Tester_1` 18 / 4, 6.1 s / 5.2 s, SES differs**: pass 4 repeats pass
  3's board (score 993.10, 1 unrouted), but the full run changes the board at pass 12 (same
  score, other hash), so the repeat is not a fixed point there; that board is a Q04.4 row. So the
  option is "byte-identical by construction" only where the passes stay put, which the
  checklist's wording overstated; the DIVERGENCES line says so. Registered as amendment 1 of
  `docs/roadmap/prereg/Q04.json` before any Q04.4 number (the `quantum-stop` arm's command gains
  `--router.quantum_stop.repeat=true`; sha256 now
  291de123ff9857d10a64ddfdf3bc09abe37130c9510d94949c56eb74400ba0e7, pinned by
  `crates/freerouting-dev/tests/prereg_q04.rs`, amendments == 1). DIVERGENCES line next to the
  quantum-stop one. Remainder for Q04.2: `q04-heavy.sh` (outside the repository) must add the
  flag to its `qs` arm before the heavy run reads the amended candidate.
