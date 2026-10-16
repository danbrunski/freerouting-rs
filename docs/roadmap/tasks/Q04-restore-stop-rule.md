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

- [ ] **Q04.1 — `QuantumStop` pass controller.**
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

- [ ] **Q04.2 — Byte-identical where it only saves time.**
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

- [ ] **Q04.3 — Two-axis output guard.**
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

- [ ] **Q04.4 — Measurement where the stop changes results.**
  Only boards where Q04.2 found an earlier stop. Preregister (F03.4): arms
  default and `quantum-stop`; signal: wall time; guards: unrouted, vias,
  `drc_real`.
  - **Accept:** measured: no regression in unrouted on any board, and time
    saved reported per board. If unrouted rises anywhere, tune only through a
    preregistration amendment, or reject and keep the numbers here. If kept,
    a DIVERGENCES line with the time saved on `canary-pwr`.
