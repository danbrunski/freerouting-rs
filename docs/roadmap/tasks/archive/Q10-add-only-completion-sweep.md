# Q10 — Add-only completion sweep

Opened 2026-10-17 (research by freerouting-rs-f1). Java's best canary-pwr result (40)
comes from a second run with pass-1 wiring frozen (`dsn_export --fix-existing`,
`-mp 2`). Q06.5 shows 83 of 109 incomplete connections are routable on the final
board with rip-up off. That is the measured case for adding without ripping. Altium
Situs has the same idea as its "Completion" pass, which re-runs the main pass with
different costs such as cheaper vias [V, Altium docs].

Prerequisites: board parity (rung 3); A10 (`PassController`, `RipupPolicy`), A13 (best-board restore), B01 (snapshots); Q06 (the `found` verdicts that motivate it); F03.1, F03.4; measured after P04's default-tree decision. Code may start before P04.

Source: research by freerouting-rs-f1 at the owner's request (2026-10-17), `docs/roadmap/research/2026-10-17-routing-algorithms.md` (candidate N1); the preregistration draft is `docs/roadmap/prereg/Q10.json`, marked `DRAFT` until the implementer registers it (sha256s pinned) before the first number.

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
- [x] **Q10.4: measurement** (prereg draft `docs/roadmap/prereg/Q10.json (draft)`). The arms:
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
  *Done 2026-10-18:* measured; the gate **fails** every registered pair, so nothing is promoted
  and every sweep setting stays unset in both profiles (the plugin's `enhanced` keeps K09.9's
  `sweep = free`, unchanged).
  - Amendments before the affected numbers (`docs/roadmap/prereg/Q10.json`, sha256
    `cefea2fafc7e9e487eadd0f6bf777a318ccf7d02721b2620b7c8ad8b44c1f0e2`, re-pinned in
    `crates/freerouting-dev/tests/prereg_q10.rs`): (1) `mp1+opt+sweep+opt` gets
    `--router.optimizer.enable_preflight_guards=false`; (2) engine main `f6e75ab` (one Linux release
    build, CLI sha256 `4c6c3622…`) under its default `enhanced` (`power_layer_traces = none`,
    `neck_width_um = 300`, `via_clearance = net_class`), the grading made precise (canary rows,
    corpus rows, DSN-only rows: KiCad metrics null, `below_minimum` = traces narrower than both the
    narrowest class width and the 150 um floor), and `seen_before_registration` (the 2026-10-17 dry
    run and its official grade, N05.3's graded runs); (3) `eligible`: the 26 rows where `mp1` left
    an incomplete connection (21 boards with the canary-pwr group; not Issue649, bm02, bm07, mccx11),
    and an 1800 s safety bound for every arm but `mp1` (no run hit it).
  - Run: `target/orchestrator/q10/s104/run.py` (one route or KiCad call at a time; the CLI on the
    registered DSN; corpus rows through `tools/kicad-oracle/grade.sh` on
    `target/corpus/projects/<id>`, whose own DSN export equals the registered DSN after the first line
    on all eight; canary-pwr-9511d9a through canary's `grade.sh` steps on WSL KiCad 10.0.7 against
    canary `9511d9a3`); engine-model metrics (statistics, `via_in_pad_count`, `pour_audit::audit`)
    by a scratch tool on `freerouting-dev`'s functions. Records `records-q104.json`, gate output
    `gate-q104.txt`, per-pair verdicts `judge-q104.txt` in `target/orchestrator/q10/s104/`.
  - **Not run: `mpdef` and `mpdef+sweep`** (amendment 3's "not run when the session ends"): pass 1
    alone took 439 s on kit-dev-coldfire and 570 s on LogicBoard_smt, so 100 default passes on about
    14 rows would hit the 1800 s bound in both arms (about 14 h). The pair counts as failed.
  - Per eligible board, router unrouted (KiCad unconnected where graded) / vias / length mm / wall s:

    | board | mp1 | mp1+sweep-fixed | mp1+sweep-free | mp1+opt+sweep+opt |
    |---|---|---|---|---|
    | Issue433-my-board | 2 / 7 / 120 / 0 | 2 / 7 / 120 / 0 | 2 / 7 / 120 / 0 | 1 / 3 / 140 / 0 |
    | Issue690-sonde_xilinx | 11 / 6 / 1284 / 0 | 11 / 6 / 1284 / 0 | 11 / 6 / 1284 / 0 | 11 / 6 / 1284 / 0 |
    | Issue508-DAC2020_bm01 | 12 / 207 / 4591 / 28 | 10 / 216 / 4825 / 31 | 3 / 239 / 5170 / 37 | 3 / 201 / 5157 / 39 |
    | Issue508-DAC2020_bm10 | 2 / 161 / 3264 / 8 | 1 / 162 / 3309 / 9 | 0 / 164 / 3359 / 11 | 0 / 164 / 3359 / 9 |
    | Issue093-interf_u | 19 / 168 / 10206 / 0 | 19 / 168 / 10206 / 0 | 19 / 168 / 10206 / 0 | 18 / 168 / 10207 / 0 |
    | Issue555-CNH_Functional_Tester_1 | 1 / 32 / 5926 / 5 | 1 / 32 / 5926 / 3 | 1 / 32 / 5926 / 3 | 1 / 32 / 5926 / 3 |
    | Issue269-z10_module | 37 / 223 / 3681 / 91 | 35 / 224 / 3687 / 91 | 35 / 223 / 3680 / 112 | 35 / 223 / 3680 / 210 |
    | Issue690-kit-dev-coldfire-xilinx_5213 | 181 / 506 / 18910 / 439 | 181 / 506 / 18919 / 398 | 181 / 506 / 18919 / 469 | 180 / 506 / 18877 / 406 |
    | Issue753-CPU-85_r104 | 8 / 399 / 18250 / 59 | 7 / 405 / 18338 / 62 | 6 / 411 / 18468 / 65 | 6 / 392 / 18457 / 68 |
    | Issue066-Project_GP8B | 25 / 856 / 18816 / 275 | 23 / 861 / 18887 / 286 | 15 / 866 / 19086 / 372 | 10 / 872 / 19189 / 361 |
    | Issue145-smoothieboard | 15 / 564 / 7413 / 175 | 12 / 566 / 7504 / 141 | 9 / 567 / 7520 / 160 | 9 / 567 / 7520 / 164 |
    | Issue219-LogicBoard_smt | 27 / 1261 / 21451 / 570 | 13 / 1282 / 21969 / 500 | 2 / 1299 / 22641 / 550 | 2 / 1300 / 22613 / 571 |
    | Issue420-contribution-board | 9 / 621 / 9858 / 122 | 9 / 621 / 9858 / 127 | 1 / 622 / 10181 / 152 | 1 / 622 / 10177 / 154 |
    | canary-pwr-9511d9a | 111 (**49**) / 148 / 1818 / 52 | 97 (39) / 169 / 2598 / 64 | 88 (**32**) / 189 / 2684 / 83 | 90 (34) / 178 / 2498 / 197 |
    | canary-pwr-pours1 | 113 / 159 / 1826 / 71 | 94 / 164 / 2107 / 108 | 89 / 179 / 2385 / 131 | 87 / 160 / 2095 / 179 |
    | canary-pwr-pours2 | 114 / 156 / 2054 / 76 | 95 / 171 / 2398 / 105 | 87 / 186 / 2541 / 119 | 88 / 183 / 2604 / 143 |
    | canary-pwr-pours3 | 107 / 160 / 1958 / 68 | 92 / 174 / 2244 / 94 | 83 / 185 / 2723 / 88 | 83 / 186 / 2732 / 137 |
    | canary-pwr-pours-as-planes | 58 / 179 / 1682 / 60 | 44 / 203 / 2113 / 71 | 39 / 215 / 2227 / 84 | 39 / 220 / 2245 / 80 |
    | canary-pwr-2026-10-15-1747-unpoured | 93 / 191 / 3484 / 272 | 82 / 196 / 3575 / 267 | 80 / 201 / 3663 / 288 | 82 / 196 / 3536 / 269 |
    | mydevboard-rp2040dev | 37 (34) / 29 / 570 / 6 | 32 (29) / 33 / 605 / 9 | 27 (25) / 34 / 646 / 12 | 29 (27) / 29 / 582 / 11 |
    | slice-dcmt-bread-slice | 4 (12) / 14 / 1280 / 0 | 2 (11) / 14 / 1304 / 0 | 2 (11) / 14 / 1304 / 0 | 2 (11) / 14 / 1304 / 0 |
    | flight-computer-core-board | 1 (5) / 28 / 591 / 0 | 1 (5) / 28 / 591 / 0 | 1 (5) / 28 / 591 / 0 | 1 (5) / 28 / 591 / 0 |
    | laser-backplane-dvi-laserbackplane-dvi | 45 (70) / 3 / 119 / 0 | 45 (70) / 3 / 119 / 0 | 45 (70) / 3 / 119 / 0 | 45 (70) / 3 / 119 / 0 |
    | vfo-controller-catvfoctrl | 2 (14) / 30 / 718 / 2 | 1 (13) / 32 / 747 / 1 | 1 (13) / 32 / 748 / 1 | 1 (13) / 32 / 748 / 1 |
    | eprom-card-80bus | 18 (18) / 361 / 17245 / 65 | 10 (10) / 390 / 18344 / 67 | 1 (1) / 430 / 19042 / 74 | 0 (0) / 433 / 19374 / 65 |
    | rio-iceshield-rio-iceshield | 21 (32) / 83 / 2505 / 29 | 18 (29) / 87 / 2599 / 25 | 15 (29) / 92 / 2758 / 31 | 15 (29) / 92 / 2759 / 30 |

    Sums over the eligible rows: KiCad unconnected (8 graded rows) 234 / 206 / 186 / 189, router
    unrouted 1073 / 937 / 843 / 839, vias 6552 / 6720 / 6898 / 6810. The four complete rows are
    byte-for-byte unchanged in the sweep arms. Wall times overlapped other sessions' routes and
    builds (14:38–14:48, 15:19–15:20, 16:47–17:35 and three one-minute windows after 17:09), so
    they are reported, not compared.
  - Guards and hard limits: `pour_lost_pins` 0 on every row and arm; canary-pwr-9511d9a clean in all
    four arms (no error but unconnected and starved thermals, 0 via-in-pad, 0 inner-layer tracks;
    starved thermals 3 / 7 / 8 / 8). Broken **in both arms of every pair**: `via_in_pad`
    (engine model) 42 on Issue145-smoothieboard and 6 on kit-dev-coldfire; `below_minimum` on six
    corpus boards (KiCad `track_width` under the Board Setup minimum 0.2 mm: the commands' 150 um
    floor with `enhanced`'s neck-down, Q16.4's cause 1; mydevboard 74 → 89, rio-iceshield
    114 → 128, vfo 67 → 71, slice 16, laser-backplane 10, eprom 0 → 5 in the free arm) and on the
    unpoured canary DSN (3). `drc_real` rises with the sweep on mydevboard, vfo, eprom and
    rio-iceshield (rio 129 → 144: `track_width` items).
  - `freerouting-dev gate --records records-q104.json --baseline baseline-q104.json --prereg
    docs/roadmap/prereg/Q10.json` (primary pair, all 30 rows; with `--prereg` the gate ignores
    `--rows` and `--arms`): 0 refusals, **FAIL**, improved 7 of 25 boards, regressed 9, neutral 9,
    nine HARD lines. The registered rule on the eligible rows for each pair (`judge.py`, the gate's
    row mark): `mp1+sweep-fixed` FAIL (21 boards: improve 6, regress 9, neutral 6);
    `mp1+sweep-free` FAIL (7 / 9 / 5); `mp1+opt+sweep+opt` FAIL (10 / 8 / 3); `mpdef+sweep` FAIL
    (not run). Neutral boards: the sweep finds nothing to add on sonde_xilinx, interf_u, CNH,
    kit-dev, flight-computer and laser-backplane.
  - On canary-pwr the registered hypothesis holds: `mp1+sweep-free` **32** KiCad-unconnected
    (≤ 40, below the previous 33), clean; `mp1` 49 on this engine (45 before N05's
    `via_clearance = net_class`). Determinism: a second route of `mp1+sweep-free` on canary-pwr is
    byte-identical. Sessions for canary-7a's official grade: `target/canary-handoff/2026-10-18-q104/`.
  - The decision table's fail branch ("remove `quality/sweep.rs`") is not carried out here: the
    plugin's `enhanced` runs the sweep (K09.9) and canary-pwr's best result needs it; the failures
    come from hard limits both arms break and from boards the sweep cannot help. That is an operator
    decision.

## Expected

 a sweep only adds, so the router's incomplete count cannot rise. The
real guards are:
- pours cut by added traces, so KiCad's count can rise after refill (watch
  `pour_lost_pins` with Q01's pour guard on);
- vias and length;
- time.
