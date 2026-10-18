# Q10 — Add-only completion sweep

Opened 2026-10-17 (research by freerouting-rs-f1). Java's best canary-pwr result (40)
comes from a second run with pass-1 wiring frozen (`dsn_export --fix-existing`,
`-mp 2`). Q06.5 shows 83 of 109 incomplete connections are routable on the final
board with rip-up off. That is the measured case for adding without ripping. Altium
Situs has the same idea as its "Completion" pass, which re-runs the main pass with
different costs such as cheaper vias [V, Altium docs].

Prerequisites: board parity (rung 3); A10 (`PassController`, `RipupPolicy`), A13 (best-board restore), B01 (snapshots); Q06 (the `found` verdicts that motivate it); F03.1, F03.4; measured after P04's default-tree decision. Code may start before P04.

Source: research by freerouting-rs-f1 at Dan's request (2026-10-17), `docs/roadmap/research/2026-10-17-routing-algorithms.md` (candidate N1); the preregistration draft is `docs/roadmap/prereg/Q10.json`, marked `DRAFT` until the implementer registers it (sha256s pinned) before the first number.

## What exists

- `ConnectionParams::ripup_allowed` exists: fanout uses it, and Q06's analysis search
  runs with rip-up off.
- `PassController` and `RipupPolicy` are A10 traits. The batch loop restores the best
  board (A13), and B01 snapshots exist.
- Java has no in-engine sweep: the two-run flow goes through KiCad's exporter.

## Decisions made here

- A sweep pass is Java's pass with `ripup_allowed = false` for the autoroute phase.
  Same ordering stage, same budgets.
- It starts from the board the loop would output: the best board after restores, not
  the last pass.
- Sweeps repeat until one routes nothing, capped at `sweep_passes` (default 2).
  Q04's quantum rule can replace the cap later.
- The `fixed` arm marks pre-sweep wiring as user-fixed for the sweep and unfixes it
  afterwards. That reproduces Java's two-run flow. The `free` arm leaves it unfixed, so
  pull-tight inside the sweep may move it.

## File ownership

- Creates `quality/sweep.rs`.
- Appends to `quality/mod.rs`, `quality/register.rs`, and the C02 settings struct
  (`router.autorouter.sweep = off|fixed|free`, `sweep_passes`, `sweep_after_optimizer`).

## Checklist

- [x] **Q10.1: the sweep pass controller.** `SweepController` wraps the configured
  pass controller. When it finishes, it restores the best board and runs sweep passes
  with rip-up off.
  - **Accept:**
    - unit test: a fixture where pass 1 rips a connection that is routable afterwards
      (q06-sole-wall style) ends complete with the sweep and incomplete without;
    - off is byte-identical on `connection-parity` and `benchmark`;
    - determinism check on bm01.
  *Done 2026-10-17:* `quality/sweep.rs`: `SweepMode` (`off|fixed|free`), `SweepConfig`,
  `SweepController` (the stop rule: a pass that routes nothing, a complete board, or
  `sweep_passes`, default 2; a pass is kept only if neither the incomplete count nor the clearance
  violations rise, else the pre-pass board `Clone` comes back), `run_sweep` (Java's pass through the
  job's own `PassExecutor` with the same ordering stage, reset first, and the same connection
  router and ripup policy, wrapped so `ripup_allowed` is false and `after_failure` rips nothing;
  sweep passes numbered from 1 as in Java's second run). It starts from the board the loop
  outputs: `after_loop` is called by `run_batch_loop` after `finish` (best-board restore and
  final tails), one additive hook (`batch_loop.rs` +16 lines, `LoopOutcome::{final_tails, sweep}`).
  The PassController trait has no way to change `ripup_allowed` or run passes, so the controller
  drives the sweep after the configured controller's `finish` instead of wrapping its trait object
  (D1). Settings `router.autorouter.{sweep, sweep_passes, sweep_after_optimizer}` (C02 table, `opt`,
  unset = off/2/false); an unknown mode or negative pass cap fails in `run_pipeline` before routing.
  - Unit: `tests/quality_sweep.rs::sole_wall_ends_complete_with_the_sweep_and_incomplete_without`
    (Q06's `q06-sole-wall.dsn`, `-mp 1`: off leaves 1 incomplete, `free` and `fixed` end at 0, every
    pass-1 trace and via is still on the board), plus `off_is_the_unchanged_engine`,
    `unknown_sweep_settings_are_refused_before_routing`, `the_sweep_is_deterministic` and three unit
    tests in `quality::sweep::tests`.
  - Off byte-identical: the A14 gate in release (`PARITY_SET` = `connection-parity` + `benchmark`,
    and `PARITY_SUBDIRS=1` for the three subdirectory boards min_fr_test, NoViasOnPowerPlanes,
    TestSensel): 25 boards (19 exact, 6 cutoff), all pass, **25 of 25 SES byte-identical** to Java's
    references, 2525.7 s + 0.4 s. Release CLI at the base `97d10d6` against this tree, default
    settings (optimizer on), `connection-parity` + `benchmark`'s first five: 14 of 14 `cmp`
    identical, also with `--router.autorouter.sweep=off --router.autorouter.sweep_after_optimizer=true`.
  - D2: `tools/determinism-check.sh` on bm01 (`-mp 1 --router.autorouter.sweep=fixed`, release CLI):
    SES byte-identical and the log identical apart from the output path (idle load 2.86, 8 burners
    7.67); the sweep takes bm01 from 49 to 24 to 18 incomplete.
- [x] **Q10.2: the `fixed` arm.** Freeze wiring during the sweep, then unfreeze it. Compare
  with Java `7f56b062d`'s two-run flow on canary-pwr-9511d9a, item by item where
  possible.
  - **Accept:** the done note lists the differences and their causes. Byte equality
    is not required, because the Java flow goes through the KiCad exporter.
  *Done 2026-10-17:* `Frozen::freeze` marks every trace and via below user-fixed as user-fixed (id
  order) and `Frozen::thaw` gives each its state back afterwards; a piece a sweep connection split
  off frozen wiring takes its vanished parent's state when unambiguous, else is unfixed and counted
  (`the_fixed_arm_gives_the_wiring_its_state_back`, `the_sweep_keeps_fixed_states_of_the_input`).
  On canary-pwr-9511d9a (`-mp 1 --router.scoring.start_ripup_costs=1500
  --router.min_trace_width_um=150`): 556 items frozen, 0 split pieces unfixed, the output SES has no
  `protect` wire (pass 1 had none). Comparison (all with those flags, router counts):
  - Java `7f56b062d`'s two-run flow without KiCad (`-de 9511d9a.dsn <pass-1 SES> -mp 2`, the session's
    wires imported user-fixed): pass 1 101, pass 2 100 unrouted, 40 violations (all input), 70.5 s.
    The same flow with the Rust engine writes a **byte-identical SES** (`cmp`). With KiCad's exporter:
    on the committed `canary-pwr-1a7df77-addonly.dsn` (`782815f`) Rust `-mp 2` writes Java's
    `canary-pwr-1a7df77-addonly.7f56b062d.ses` (`d0b38e67`, the **37** reference) byte for byte apart
    from the session name, which comes from the input file name; `ses-diff` equivalent (123 router
    unrouted, 40 violations, 172 vias).
  - `sweep=fixed`: pass 1 109 → 100, pass 2 → 96 unrouted, 40 violations, 74.6 s.
  - `ses-diff` Java two-run vs `fixed`: 16 nets differ; the sweep is better on 6 nets (+5V_PI,
    /Holdup/VCAP, /Supervisor/LED_STATUS, GND, MCU_NRST, MCU_SCL) and worse on 2 (/Pi_Supply/LM_SW,
    ~{PWR_FAIL}); 64 wires and vias only in Java's, 133 only in the sweep's; incomplete 100 → 96,
    vias 170 → 180, length 2297.87 → 2447.92 mm, bends 597 → 617.
  - Causes: (1) Java's second run runs the fanout stage again (20 passes, 265 → 292 of 367 SMD pins
    escaped, about 22 fanout vias) before routing; the sweep runs no fanout. (2) Java's passes rip
    up (the new run's own unfixed wiring, and pass 2's repeated-failure ripup); the sweep never rips.
    (3) Java's board is rebuilt from DSN + SES (a fresh search tree, D5, fresh item ids and failure
    log), so every work count and tie after it differs; the sweep continues on the loop's board.
    (4) Java's output keeps the imported wiring as `protect` (555 wires and vias); the `fixed` arm
    unfreezes it. (5) Through KiCad (the 37 flow) the frozen copper is KiCad's export of the
    imported tracks (`type fix`, re-segmented), which neither in-engine flow reproduces; the
    flows' router counts are not comparable (123 vs 100), only KiCad's grades are.
- [x] **Q10.3: `sweep_after_optimizer`.** Optimizer, sweep, optimizer again: the
  optimized board has more room (Q06's 83 were found after the optimizer).
  - **Accept:** off is byte-identical; determinism.
  *Done 2026-10-17:* `after_optimizer` (called by `run_pipeline` after the batch loop, one additive
  hook in `pipeline.rs`): with `sweep_after_optimizer` and the optimizer on, a first optimizer run
  (its time limit armed as the pipeline's), then the sweep; the pipeline's own optimizer runs after.
  With the optimizer off the sweep runs after the loop as usual.
  `after_the_optimizer_the_sweep_runs_between_two_optimizer_runs` (optimizer lines before and after
  the sweep line; with the optimizer off the SES equals the after-loop sweep's).
  - Off byte-identical: `off_is_the_unchanged_engine` (optimizer on, `sweep=off` with
    `sweep_after_optimizer=true`), and the 14-board release CLI `cmp` of Q10.1 with
    `--router.autorouter.sweep=off --router.autorouter.sweep_after_optimizer=true`: 14 of 14 identical.
  - D2: `tools/determinism-check.sh` on bm01 (`-mp 1 --router.autorouter.sweep=free
    --router.autorouter.sweep_after_optimizer=true --router.optimizer.enable_preflight_guards=false`):
    SES byte-identical, log identical apart from the output path (idle 7.67/6.34, burners 8.80);
    optimizer, sweep 49 → 4 → 2, optimizer.
  - Note: with the default `optimizer.enable_preflight_guards` the optimizer skips any board with an
    incomplete connection, so on canary-pwr (and on bm01 at `-mp 1`) the first optimizer run is a
    no-op and the arm equals the after-loop sweep (canary-pwr `fixed` + after-optimizer: SES
    byte-identical to `fixed`).
- [~] **Q10.4: measurement** (prereg draft `docs/roadmap/prereg/Q10.json (draft)`). The arms:
  - `mp1`: `-mp 1`, the baseline;
  - `mpdef`: the default passes;
  - `mp1+sweep-fixed`;
  - `mp1+sweep-free`;
  - `mp1+opt+sweep+opt`;
  - `mpdef+sweep`.

  The cheaper-via completion arm (`sweep_via_cost_factor` 0.5) goes in only if the
  prereg lists it before the first number.
  - **Accept:** measured. On canary-pwr the hypothesis is ≤ 40 KiCad-unconnected,
    clean (canary-23 grades).
  *Partial 2026-10-17:* registered before the first number: `docs/roadmap/prereg/Q10.json`
  (2026-10-17, sha256 `c280ca8e32a3d9944f558c1751e7a6d759677aa3512595f6c3fc4324655ca347`, pinned by
  `crates/freerouting-dev/tests/prereg_q10.rs`), universe `tools/bench/sets/q10-sweep.txt` (30 inputs,
  sha256 each; canary-pwr rows one board), primary pair `mp1` / `mp1+sweep-free`, all four pairs
  listed; the cheaper-via arm is not registered. Pre-P04 dry run on canary-pwr-9511d9a only (not the
  registered run): router `mp1` 109 / `sweep-fixed` 96 / `sweep-free` 89 unrouted, 40 violations
  each (all in the input), 48.7 / 74.6 / 72.3 s. Self-graded with canary's `autoroute.sh` steps
  (`ses_import.py`, `kicad-cli pcb drc --schematic-parity --refill-zones --severity-all`,
  `via_in_pad.py`, `inner_tracks.py`) on WSL KiCad 10.0.7, calibrated by `mp1` = **45** unconnected,
  4 starved thermals (canary-23's grade of pass 1): `sweep-fixed` **35** unconnected, 10 starved
  thermals; `sweep-free` **33**, 9 starved thermals; both 0 via-in-pad, 0 inner-layer tracks, no
  clearance, width or annular errors. The parity reference is now **37** (Java `7f56b062d` add-only,
  canary-7a, `782815f`; the preregistered ≤ 40 rule is unchanged). Starved thermals rise 4 → 9-10: the
  pour guard the checklist names. Remains: the registered run of every arm on the eligible boards
  after P04's default-tree decision (P04), and canary-23's grading of the canary-pwr sessions.
  *Operator 2026-10-17:* P04's tree decision is made (Java's tree stays, P04 archived), so the
  registered run can go ahead. Arm `mp1+opt+sweep+opt` is degenerate as registered: with the default
  preflight guards the optimizer skips an incomplete board. It gets an amendment before any of its
  numbers, adding `--router.optimizer.enable_preflight_guards=false` to that arm. Starved thermals
  are reported next to the unconnected count but do not gate the keep rule; Q13 and the pour guard
  carry that question. The grader is now canary-7a (formerly canary-23).
  *Official grade 2026-10-17 (canary-7a, Windows KiCad 10.0.5, canary `hardware/tools/grade.sh`):*
  hashes matched (input `65186b28`; `mp1` `502226b0`, `sweep-fixed` `85e9bf03`, `sweep-free`
  `333c79f7`). `mp1` **45** (906 segments, 116 vias), `sweep-fixed` **35** (1159 / 139),
  `sweep-free` **33** (1289 / 159). All clean: no clearance, width, annular, parity, via-in-pad or
  inner-layer errors. Starved thermals 4 / 10 / 9, all GND through-hole connector pins with one
  spoke of two (still joined through In1). The WSL self-grade matches exactly. 33 is the first
  freerouting-rs result below Java's 37 and is the new number to beat (canary spec §P10 item 15).
  Starved thermals are tracked as a secondary score. Still open: the registered run of every arm
  on the eligible boards.

## Expected

 a sweep only adds, so the router's incomplete count cannot rise. The
real guards are:
- pours cut by added traces, so KiCad's count can rise after refill (watch
  `pour_lost_pins` with Q01's pour guard on);
- vias and length;
- time.
