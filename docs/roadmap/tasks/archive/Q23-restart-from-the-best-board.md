# Q23 — Restart from the best board

Opened 2026-10-21 from the PCU rev 2.0 board report
([pcu-rev2-board-report.md](../../../development/pcu-rev2-board-report.md), "Runs"). Routing a run's
own output again was the report's most effective step: v7's 186 unrouted became 82 in v7c, then
79 in v7c2. The second run did what the first run's last 45 minutes didn't. This checklist finds
out which step gains, and builds that step into the engine only if a measurement keeps it.
The report's bugs are [K15](K15-pcu-board-findings.md)'s.

Prerequisites: Q10 (the add-only sweep, opt-in), A13 (best-board restore), N02 (the totals rule),
N05 (promotion). Q23.1 and Q23.4 are heavy runs, queued through the orchestrator.

## What the work starts from

From the report's logs, gzipped in `target/orchestrator/pcu-rev2/logs/` (all `8c952e7`). The
boards are copied to `target/orchestrator/pcu-rev2/boards/` (2026-10-21):
- `placed_v7/`: v7's input, 0 tracks; `.kicad_pcb` sha256 `948a4804…`, `.kicad_pro` `009b7afc…`;
- `final_v7/`: v7's output, v7c's input; `.kicad_pcb` `1e16823a…`;
- `committed-1b161c7/`: the committed board, `af63f202…`.

The committed `.kicad_dru` (`c2e552f2…`) sits beside each, named for its board. The report's
runs had a `.kicad_dru` next to the board; whether it was this file is not recorded.
Unrouted counts are the router's.

| Run | Input | After fanout | Pass 1 | Passes 2.. | s / pass | Best | Stop |
|---|---|---|---|---|---|---|---|
| v7 | `placed_v7` (1208) | 519 | 235 | 337, 292, 289, 243, ..., 186 at pass 14 | 119–275 | 186 | timeout 3600 s, pass 15 |
| v7c | v7's output (186) | **105** | **82** | 181, 145, 177, 147, 175, 146, 82, 181, ... | 64–89 | 82 (pass 1) | stagnation, pass 18, 1353 s |
| v7c2 | v7c's output (84) | 82 | 80 | 79–83 | 59–76 | 79 | stagnation, pass 19, 1269 s |
| v9n | v9n's input, not kept (1208) | 516 | 235 | 346, 253, ..., 188 at pass 12 | 183–338 | 188 | timeout 3600 s, pass 12 |
| v9nc | v9n's output (188) | **121** | **98** | 174, 170, 176, ..., 98 every 4th pass | 86–101 | 98 (pass 1) | stagnation, pass 18, 1783 s |

- **Where the gain is.**
  - The fanout stage, run again on a routed board, completes 81 (v7c) and 67 (v9nc)
    connections before any pass.
  - Pass 1 then completes 23 more on each.
  - Every later pass rips up 60–100 connections and loses them. The best board comes back every
    fourth pass, which is Java's periodic restore. So the 17 passes after the best (about 20
    minutes on v7c) gain nothing.
- **The settings differ between runs.** v7c also set `min_trace_width_um = 130` and a 45-minute
  timeout; v7 had neither. Q23.1 separates that.
- **The report's table says v9nc ended at 168.** That is its last pass. The run restored its best
  board, 98.
- **What exists.**
  - Q10's add-only sweep: `router.autorouter.sweep = fixed | free`, `sweep_passes`,
    `sweep_after_optimizer`. Opt-in; its measurement failed every registered pair on
    2026-10-18.
  - The batch loop's best-board restore (A13).
  - The fanout stage (A12) runs once, before pass 1.
  - Java's stop rule counts passes, not work: 10 passes without a 0.5 gain, counted from pass 8.
    On this board a pass takes 1–5 minutes.
  - Budgets count work, never time (D2).

## Decisions made here

- **Measure before building.** Q23.1 attributes the gain at main with the steps the CLI already
  has: a second CLI run, with and without fanout, and the Q10 sweep. Q23.3 builds an in-engine
  stage only for a step that gains there.
- **An opt-in stage first.** The restart is a new setting, `router.autorouter.restarts` (unset:
  off, both profiles). It is judged by the paired rule. It joins `ENHANCED` only through N05's
  recipe and the totals rule.
- **Work is recorded, not only time.** The engine has no maze-work budget setting, so the arms
  run to their own stop. Each arm's maze work comes from `--stats json`. The registration bounds
  how much more work `restart` may use than `base`.
- **The restart's steps**, unless Q23.1 says otherwise:
  1. When the loop stops (stagnation, or the pass limit), take the best board.
  2. Run the fanout stage on it.
  3. Run the pass loop again from pass 1, with pass-1 rip-up costs.
  4. Keep the result only if it beats the best. Stop restarting when a restart doesn't.

## File ownership

| Item | New files | May edit |
|---|---|---|
| Q23.1 | `target/orchestrator/pcu-rev2/out/q23/` (not committed) | this file |
| Q23.2 | `docs/roadmap/prereg/Q23.json`, `tools/bench/sets/q23-restart.txt`, `crates/freerouting-dev/tests/prereg_q23.rs` | — |
| Q23.3 | `crates/freerouting/src/quality/restart.rs`, `crates/freerouting/tests/restart.rs` | `quality/mod.rs`, `quality/register.rs`, `settings/fields.rs` (one field), `autoroute/batch/batch_loop.rs` (one hook), `crates/freerouting-cli/src/help.txt`, `docs/DIVERGENCES.md` |
| Q23.4–Q23.5 | `tools/bench/baselines/Q23.json` | `settings/profile.rs` (Q23.5 only), `docs/roadmap/routing-quality.md` |

## Checklist

- [x] **Q23.1 — Which step gains, at main.**
  - **Change:** none. These are runs at main with `--stats json`, `--result-json` and
    `--router.special.diagnostics=verdicts`, on `placed_v7` and on `canary-pwr-pours1`:
    1. `first`: one run with `--router.plane_obstacle_net_classes=Pour1A,Pour3A
       --router.cleanup.remove_stubs=true --timeout 60m` (main takes edge clearance, hole
       clearance and the track minimum from the `.kicad_pro`);
    2. `cont`: a second run on `first`'s output, same flags;
    3. `cont-nofanout`: as `cont` with `--router.fanout.enabled=false`;
    4. `sweep`: `first` with `--router.autorouter.sweep=free`;
    5. `long`: one run with `--timeout` set to `first`'s and `cont`'s wall time together.
  - **Accept:**
    - a table per board: unrouted after fanout, after pass 1, best, pass of the best, maze work
      and wall time, per arm;
    - which verdicts the second fanout turns from `found` or `blocked_by` into routed;
    - a sentence saying which step gains, or that none does at equal work;
    - heavy run pending until the orchestrator runs the five arms. Each command goes in the
      note with its output path under `target/orchestrator/pcu-rev2/out/q23/`. `first` can
      reuse K15.4's run if the flags match.

  *Done 2026-10-22:* the gain is a second pass 1 on the routed board; the fanout adds a little.
  - **Short form** (decided 2026-10-22: no hours of board runs). The arms start from a routed
    board instead of a new 60-minute first run, and stop after one pass:
    - PCU: v7's 60-minute output `final_v7` (8c952e7, 186 unrouted then), with the committed
      `.kicad_pro`/`.kicad_dru` beside it (`final_v7`'s own project lacks the Pour classes);
    - canary-pwr: `canary-pwr-pours1.dsn` with a `--timeout 5m` first run, then its SES.
    - Flags: `--router.cleanup.remove_stubs=true --router.special.diagnostics=verdicts
      --progress json --stats json`, plus `--router.plane_obstacle_net_classes=Pour1A,Pour3A` on
      the PCU board; `--max-passes 1` for the continuations.
    - Binary: main `fe1c2e0`, release, at `target/orchestrator/pcu-rev2/out/q23/freerouting-rs`.
    - Script and records: `target/orchestrator/pcu-rev2/q23-run.sh`, `out/q23/` (`q23.log`, each
      arm's `progress.jsonl`, `result.json`, `*.stats.json`).
  - **Results** (the router's unrouted count):

    | Board | Arm | Input | After fanout | After pass 1 | Wall |
    |---|---|---|---|---|---|
    | PCU | `cont` | 206 (main reads `final_v7` as 206) | 123 | **103** | 204 s |
    | PCU | `cont-nofanout` | 206 | - | **113** | 130 s |
    | canary-pwr | `first` | 297 | 147 | 119 (best of 14 passes) | 821 s |
    | canary-pwr | `cont` | 119 | 111 | **105** | 37 s |
    | canary-pwr | `cont-nofanout` | 119 | - | **107** | 29 s |

  - **Reading.**
    - On both boards the first run's later passes gain little or nothing: canary-pwr's best is
      pass 1 of 14. The report's logs show the same: PCU v7 was 235 after pass 1 and 186 after
      14 passes and 51 minutes.
    - A second pass 1 on the routed board gains most of the continuation: 206 -> 113 on the PCU
      board and 119 -> 107 on canary-pwr without the fanout.
    - Running the fanout again first adds 10 and 2 more (103 and 105).
    - So the restart step Q23.3 builds is the decision's "fanout, then the pass loop from pass 1
      on the best board", and pass 1 is the part that matters.
  - **Not measured:** `sweep` and `long`. These were dropped with the hours-long arms. The
    first-run passes after pass 1 are the `long` arm's extra time, and they gained 0 on
    canary-pwr and 49 in 46 minutes on the PCU board (v7).
  - **Seen on the way:** canary-pwr's `first` ran 821 s against `--timeout 5m`. Its 14 passes
    took 265 s, and the remaining time went to the fanout and the stages after the passes. A
    timeout should stop those too; not followed up here.

- [x] **Q23.2 — Preregister the measurement.**
  - **Change:** register before any number of Q23.4 exists:
    - short runs only (decided 2026-10-22): each arm a few minutes per board (`--max-passes`
      and `--timeout` set so), no hour-long arms;
    - `docs/roadmap/prereg/Q23.json` and `tools/bench/sets/q23-restart.txt`;
    - boards: `placed_v7` (by absolute path, skipped when missing), the five `canary-pwr` DSNs
      of `owner.txt`, and at least two corpus boards with 300 or more connections;
    - arms: `base` (`enhanced`) against `restart` (`enhanced` plus
      `router.autorouter.restarts=1`, or the step Q23.1 names);
    - the work guard: `restart`'s maze work at most a registered multiple of `base`'s;
    - signal: unrouted;
    - hard guards: `via_in_pad`, `below_minimum`, real DRC;
    - keep rule: the paired rule (at least N−1 boards improve, none regress);
    - the decision table.
  - **Accept:** the prereg pin test `prereg_q23.rs` passes with the hashes.

  *Done 2026-10-22:* registered before any Q23.4 number.
  - Files: `docs/roadmap/prereg/Q23.json` (sha256 `64ca9b3f17860bdf0e39b3d1dfc7291f275a2e04526108b9cbbdf9bfa1524e30`,
    pinned by `the_registration_is_pinned_by_its_hash`) and `tools/bench/sets/q23-restart.txt`.
  - Arms: `base` (`enhanced`, `--max-passes 1`, a safety `--timeout` per row) against `restart`
    (the same plus `--router.autorouter.restarts_from_best=1`; Q23.1's step: the fanout, then
    pass 1 again on the best board). The key is not `restarts`, which is Q12's removed setting
    (see Q23.3); the registration's `setting_name` says so.
  - Rows (7, 5 distinct boards; decided 2026-10-22 by the dispatch, in place of `placed_v7` and the
    five owner DSNs): the PCU board as Q23.1 continued it (`final_v7.kicad_pcb` by absolute path
    with the committed `.kicad_pro`/`.kicad_dru`, `1e16823a...`, skipped when missing, Q23.1's
    flags, 900 s); `canary-pwr-pours1..3` (one board, canary's flags, 300 s); three benchmark
    boards with 500+ connections: `Issue690-kit-dev-coldfire-xilinx_5213` (541),
    `Issue753-CPU-85_r104` (569), `Issue066-Project_GP8B` (759), 300 s each.
  - Signal `unrouted` (score); hard `via_in_pad`, `below_minimum`; guard `drc_real` (KiCad's
    on the PCU and canary rows, the router's `-drc` routing-caused clearance violations on the
    benchmark rows); work guard `maze_work_units` (`--stats json`) of `restart` at most 2.0x
    `base`'s on every row; paired rule `{min_boards 3, improve_all_but 1, max_regress 0}`;
    validity rule (a row whose arm hit its timeout is left out); decision table pass / fail /
    not measured.
  - `cargo test -p freerouting-dev --test prereg_q23`: 5 passed (the arms differ by the one
    setting, the rows match the set with their flags and hashes, the PCU rules files hash as
    registered, the work guard and decision table, the pin).

- [x] **Q23.3 — The restart stage.**
  - **Change:**
    - `quality/restart.rs`, registered as a stage, runs the steps in "Decisions made here"
      (or Q23.1's step) up to `router.autorouter.restarts` times.
    - It is deterministic: no time reaches a decision, and its state lives in ordered
      collections.
    - A `--timeout` still stops it and keeps the best board.
  - **Accept:**
    - tests in `tests/restart.rs`:
      - a board where a restart completes a connection the first loop left open;
      - a board where a restart doesn't gain, and the stage stops after one;
    - unset gives an SES byte-identical to main's on the quick benchmark set (`ses-diff`), and
      `legacy` is untouched;
    - `tools/determinism-check.sh` on two boards with `restarts=1`;
    - help text and a DIVERGENCES line.

  *Done 2026-10-22:*
  - **Setting name.** `router.autorouter.restarts` is Q12's removed portfolio key
    (`settings/removed.rs` refuses it: "router.autorouter.restarts was removed on 2026-10-20"),
    so the stage takes `router.autorouter.restarts_from_best` (i32; unset, 0 or negative: off;
    unset in both profiles, not in `ENHANCED`). Reusing `restarts` needs its tombstone removed:
    an operator decision, raised in the report.
  - **Stage.** `crates/freerouting/src/quality/restart.rs`, called from one hook in
    `run_batch_loop` after the finish and before Q10's sweep (`LoopOutcome::restart` holds its
    outcome). It has no registry slot (none fits a loop-level stage, as for Q10's sweep);
    `quality/register.rs` notes that, and the restart builds a new pass controller of the job's
    kind from `StageRegistry::builtin()` by the controller's id. Each restart: the best board kept
    as a `Clone`, `run_batch_loop` again on it (the fanout, the passes from pass 1, the loop's
    finish) with the job's stages, no board stages, its own `CancelToken` with the job's full
    stops relayed, and `restarts_from_best`, `sweep` and `output_guard` unset; kept only if its
    router score is strictly higher (`beats`), else the best board comes back and the restarts
    stop. No time reaches a decision; a timeout or cancel stops the restart's loop and the better
    board is kept.
  - **Tests** (`cargo test -p freerouting --test restart`, 5 passed):
    `a_restart_completes_the_connection_the_first_loop_left_open` (`q06-sole-wall.dsn`, `-mp 1`:
    1 unrouted without, 0 with one restart);
    `a_restart_that_does_not_gain_stops_after_one_and_keeps_the_best_board`
    (`q06-fixed-cage.dsn`, `restarts_from_best=3`: one restart, dropped, SES equal to the run
    without); `unset_zero_and_negative_change_nothing` (SES and log equal);
    `a_restart_is_deterministic`; `a_stop_during_a_restart_keeps_the_best_board_and_reaches_the_job`
    (a full stop at the restart's first connection: the best board, "stopped on request", the job's
    token stopped). Unit tests `quality::restart::tests` (3).
  - **Unset is main's engine:** the quick benchmark set (9 boards, profile `enhanced`, default
    passes) routed by main `dd9dbf6`'s release CLI and this branch's: 9 of 9 SES byte-identical
    (`cmp`, stronger than `ses-diff`). `legacy` untouched:
    `PARITY_SET=tools/bench/sets/connection-parity.txt tools/parity-check.sh --no-probes`:
    7 of 7 exact, `PARITY OK`.
  - **Determinism:** `tools/determinism-check.sh` (idle against 24 burners), release CLI with
    `--router.autorouter.restarts_from_best=1`: `Issue508-DAC2020_bm07` default passes (the restart
    is kept, unrouted 6 -> 0) "identical"; `Issue508-DAC2020_bm01` `-mp 1` (kept, 44 -> 40)
    "identical".
  - **Help and DIVERGENCES:** `--router.autorouter.restarts_from_best=<number>` in
    `crates/freerouting-cli/src/help.txt`; "Restarts from the best board" in `docs/DIVERGENCES.md`
    (freerouting section, after the add-only sweep).
  - **Ripple of the new field** (outside the ownership table, one line each): the field's unset
    default in `crates/freerouting/tests/settings_defaults.rs` and a `CLI_ONLY` entry in
    `crates/freerouting-gui/src/dialogs/autoroute_settings.rs`
    (`every_router_setting_has_a_control`; `cargo test -p freerouting-gui`: 227 passed).
  - **Gate:** `cargo fmt --all --check` clean; `cargo clippy --workspace --all-targets -- -D
    warnings` clean; `cargo test --no-fail-fast -p freerouting -p freerouting-cli -p
    freerouting-dev`: 2151 passed, 0 failed, 33 ignored (262 test binaries).

- [x] **Q23.4 — Measure and decide.**
  - **Change:** run Q23.2's registration. Apply its decision table: keep the stage opt-in, or
    remove it and its setting (Q rule 5).
  - **Accept:**
    - `freerouting-dev gate` output and the per-board table in the note;
    - the 1-in-2^N luck rate printed;
    - heavy run pending until the orchestrator runs it.

  *Done 2026-10-22:* measured as registered (run approved 2026-10-22). The gate **fails**, so the
  decision table's fail branch removed the stage.
  - **Engine.** main `8a5e365`, one Linux release build in
    `target/orchestrator/q23-measure-target/release/freerouting-rs` (sha256 `dd3b18da…`,
    `freerouting-rs 0.1.0 (git 8a5e365a8cbb)`), used for both arms, with the registered commands.
    The binary path is not `target/release/`, but it is the same engine, and the registration
    names only "one Linux build for both arms", so this needs no amendment. KiCad: `/usr/bin/kicad-cli` and
    pcbnew **10.0.6** (installed package `kicad 10.0.6`; the registration names no version).
  - **Amendment 1** (`docs/roadmap/prereg/Q23.json`, sha256 `576380f4…`, re-pinned in
    `prereg_q23.rs`, written before any route). The canary-pwr pours DSNs have no `.kicad_pcb`,
    so these rows are graded as the router rows: `drc_real` and `below_minimum` come from the
    router's checks, `via_in_pad` is gate collect's `session_via_in_pad`, and `kicad_unconnected`
    is null. The router's clearance violations are the `-drc` types `clearance` and
    `hole_clearance`. The canary group is also judged by hand on summed unrouted.
  - **Run.** `target/orchestrator/pcu-rev2/out/q23-measure/run.sh`, 14 routes, one at a time,
    each under `ulimit -v 8000000`, 13:49–14:17 (`run.log`). Grading:
    - PCU: `pcu_grade.sh`, which runs grade.sh's steps (`force-errors`, `kicad-cli pcb drc
      --severity-all --refill-zones --format json`, `kicad_steps.py stats`, `oracle_lib.py
      summarize` on the copper basis), `via_in_pad.py`, and `via_in_pad_added.py` (via_in_pad.py's
      test on the vias the input lacks);
    - DSN rows: `dsn_grade.sh` (`-drc`), then `build_records.py` and `freerouting-dev gate
      collect --group fixtures/canary-pwr/=canary-pwr`.

    Records are in `records.json` (sha256 `a0bd0efe…`), the baseline in `tools/bench/baselines/Q23.json`,
    and the gate output in `gate.txt`.
  - **Per row** (router unrouted / KiCad unconnected where graded; maze work units; wall s).
    `via_in_pad`, `below_minimum` and `drc_real` are 0 / 0 / 0 in both arms on every row except
    coldfire-5213, where `drc_real` is 4 / 4. No arm hit its timeout; every `final_state` is
    `COMPLETED`, so no row is left out.

    | row | base | restart | work ratio | restart kept |
    |---|---|---|---|---|
    | pcu (`final_v7`, input 206) | 95 / 79; 3,536,888,682; 154.1 | 95 / 79; 6,370,747,981; 250.5 | 1.80 | no (95 -> 95), SES byte-identical |
    | canary-pwr-pours1 | 111; 1,537,706,638; 59.6 | 111; 2,683,513,410; 123.1 | 1.75 | no, byte-identical |
    | canary-pwr-pours2 | 112; 1,666,067,583; 61.4 | 112; 2,734,835,937; 110.5 | 1.64 | no, byte-identical |
    | canary-pwr-pours3 | 108; 1,364,400,741; 55.7 | 108; 2,346,542,486; 105.8 | 1.72 | no, byte-identical |
    | coldfire-5213 | 181; 30,572,692; 242.9 | 180; 46,739,898; 267.9 | 1.53 | yes (181 -> 180) |
    | cpu-85 | 146; 805,100,591; 12.2 | 142; 1,740,279,363; 24.8 | **2.16** | yes (146 -> 142) |
    | gp8b | 222; 2,276,999,413; 58.8 | 159; 4,529,681,025; 117.7 | 1.99 | yes (222 -> 159) |

    - The PCU row's KiCad grade: 0 routing-caused items (512 pre-existing), 79 unconnected against
      the input's 154, 0 vias in pads that the input lacks (117 in both the input and the output).
    - The grade counts 24 `track_width` items below the `.kicad_dru` minimum of 0.127 mm. All 24
      are in the input; `below_min_added.py` finds 0 that the input lacks. So `below_minimum`
      (routed copper) is 0.
    - Canary-pwr summed: 331 -> 331.
  - **Gate.** `freerouting-dev gate --records records.json --prereg docs/roadmap/prereg/Q23.json
    --baseline baseline.json` gives:
    - 0 refusals, 7 of 7 rows judged;
    - "on trial: improved 3/5 board(s), regressed 0, neutral 2 (rule: improve >= N-1, regress <=
      0)": improved coldfire-5213, cpu-85, gp8b; neutral canary-pwr, pcu;
    - "N=5: a change whose per-board direction is a coin flip passes this rule 1 run in 32";
    - **gate: FAIL** (exit 1).

    The registered rule by hand agrees, with the canary rows summed (331 -> 331, neutral):
    - paired rule: 3 of 5 improved, where 4 are needed: fail;
    - hard metrics: 0 everywhere;
    - `drc_real`: never higher than base;
    - work guard: fails on cpu-85 (2.16x > 2.0).

    Luck rate: 1 in 2^5 = 32.
  - **Decision: fail.** Q rule 5 removed the stage:
    - removed: `crates/freerouting/src/quality/restart.rs`, `crates/freerouting/tests/restart.rs`,
      the `run_batch_loop` hook and `LoopOutcome::restart`, the `quality/mod.rs` and
      `register.rs` lines, the `restarts_from_best` field, its help text, its `settings_defaults`
      entry, the GUI `CLI_ONLY` entry and the DIVERGENCES section;
    - the key `router.autorouter.restarts_from_best` (and `restartsFromBest`) is refused by
      `settings/removed.rs` ("removed on 2026-10-22: the restarts from the best board failed their
      measurement (Q23.4)"). The tests are `tests/settings_removed.rs` and
      `removed_portfolio_settings_are_usage_errors`. The DIVERGENCES removed-settings line names
      it;
    - `routing-quality.md` §10 records the result.
  - **What the numbers say.**
    - The restart gained on the three benchmark boards, most on GP8B (−63).
    - On the PCU board and the three canary-pwr variants, its fanout and pass 1 ended at exactly
      the best board's score (the log says "unrouted 95 -> 95, violations 220 -> 220" on the PCU
      board), so it was dropped and the output is byte-identical to `base`.
    - Q23.1's `cont` arm started from `final_v7` (206) and reached 103. Here `base` itself
      reaches 95 from the same input, and a restart on that result adds nothing.
  - **Gate after the removal:**
    - `cargo fmt --all --check` is clean, and so is `cargo clippy --workspace --all-targets -- -D
      warnings`;
    - `cargo test --no-fail-fast -p freerouting -p freerouting-cli -p freerouting-dev -p
      freerouting-gui`: 2456 passed, 0 failed, 36 ignored. One binary, `freerouting-gui --test
      ui_render`, died with SIGSEGV in the parallel run (software GPU) and passes alone with
      `--test-threads=1` (7 passed, 1 ignored). The removal touches only a `CLI_ONLY` entry in that
      crate;
    - `prereg_q23`: 5 passed;
    - `PARITY_SET=tools/bench/sets/connection-parity.txt tools/parity-check.sh --no-probes`: 7 of 7
      `[exact]` with equal bytes, `PARITY OK`.

  - **Review notes (orchestrator, 2026-10-22).** Neither changes the verdict.
    - The PCU row's input was already a routed board (`final_v7`), so its `base` arm was itself
      Q23.1's continuation: 206 -> 95 in one pass. A restart on top of that could add nothing.
      With an unrouted PCU input the row could have shown Q23.1's gain, but the rule would still
      fail on the remaining boards and on the work guard (cpu-85 2.16x).
    - On canary-pwr, one pass (`base`, 111 unrouted on pours1) beat Q23.1's 14-pass first run
      (119). So the continuation gain Q23.1 saw there undoes damage the later passes did; it is
      not a step a restart adds. Where later passes make a board worse is a question for the
      pass loop and the best-board restore, not a reason to restart.

- [x] **Q23.5 — Promote, if kept.**
  - **Change:** only if Q23.4 keeps the stage: N05's recipe, judged by the totals rule, decides
    whether `router.autorouter.restarts` joins `ENHANCED`.
  - **Accept:**
    - N05's recipe followed, with its registration and gate output;
    - or a note that Q23.4 removed the stage, which closes this item.

  *Done 2026-10-22:* closed without a promotion. Q23.4's gate failed (3 of 5 boards improved,
  where 4 are needed, and the work guard failed on CPU-85 at 2.16x). The decision table's fail
  branch removed `quality/restart.rs` and its setting: `router.autorouter.restarts_from_best` is
  refused by `settings/removed.rs`. Nothing remains to promote, and `settings/profile.rs` is
  unchanged.
