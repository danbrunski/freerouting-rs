# Q23 — Restart from the best board

Opened 2026-10-21 from the PCU rev 2.0 board report
([pcu-rev2-board-report.md](../../development/pcu-rev2-board-report.md), "Runs"). Routing a run's
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

- [ ] **Q23.1 — Which step gains, at main.**
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

- [ ] **Q23.2 — Preregister the measurement.**
  - **Change:** register before any number of Q23.4 exists:
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

- [ ] **Q23.3 — The restart stage.**
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

- [ ] **Q23.4 — Measure and decide.**
  - **Change:** run Q23.2's registration. Apply its decision table: keep the stage opt-in, or
    remove it and its setting (Q rule 5).
  - **Accept:**
    - `freerouting-dev gate` output and the per-board table in the note;
    - the 1-in-2^N luck rate printed;
    - heavy run pending until the orchestrator runs it.

- [ ] **Q23.5 — Promote, if kept.**
  - **Change:** only if Q23.4 keeps the stage: N05's recipe, judged by the totals rule, decides
    whether `router.autorouter.restarts` joins `ENHANCED`.
  - **Accept:**
    - N05's recipe followed, with its registration and gate output;
    - or a note that Q23.4 removed the stage, which closes this item.
