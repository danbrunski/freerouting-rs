# Q16 — No dangling vias or trace stubs in the result

Opened 2026-10-18, from canary-7a's grade of Dan's `enhanced` plugin run on `canary-pwr`
(`target/canary-handoff/2026-10-17-plugin45/`, 45 unconnected). KiCad's DRC counted **32
dangling vias**, mostly pairs on supervisor signals (CAN_*, LED_*, I2C_READY, MCU_SCL). They
are stubs of connections the router started and never finished. KiCad only warns about them,
but they are copper a designer has to delete by hand, and they block later manual routing.

What exists:
- the autorouter's rip-up already removes unconnected vias
  (`AutorouteControl::remove_unconnected_vias`, Java's `removeUnconnectedVias`, on by
  default) when it rips a connection;
- the DRC reports dangling stubs (`drc/checker.rs`, `UnconnectedKind::ViaDangling` and
  `TrackDangling`) and advises an optimizer pass.

Nothing removes stubs that are still there at the end of the run: a connection that failed in
the last pass, the sweep's partial inserts (Q10), or the optimizer's leftovers.

Prerequisites: Q10 (the sweep), N02 (the totals rule), F02.6 (the KiCad oracle).

## Decisions made here

- **A final cleanup stage under `enhanced`, off in `legacy`:** after the last routing or
  optimizer step, remove every routed via and trace that is a stub:
  - a stub connects fewer than two of the net's items (pins, planes, other copper), counted
    the way KiCad counts them. That includes contact with a plane of the net, so a via that
    stitches a trace to its plane is kept;
  - fixed or locked copper (`type fix`, the user's tracks) is never removed;
  - the removal repeats until nothing changes, so a chain of stubs goes as a whole.
- **It removes only copper that connects nothing.** It cannot change which connections are
  complete, so the unconnected count must not rise. That is a hard guard of the measurement.

## Checklist

- [~] **Q16.1 — Where the stubs come from.** On `canary-pwr` (the plugin45 handoff DSN and
  the default plugin flags) and two corpus boards, attribute each dangling via and track that
  KiCad reports to the stage that inserted it (last pass, sweep, optimizer) using the
  routing ledger or the verdicts.
  - **Accept:** a table in this file, with a count per stage.

  *Partial 2026-10-18:* heavy run pending (the heavy-job slot was held by N05; nothing was run). The
  scripts are in `/home/thebo/code/freerouting-rs/target/orchestrator/q16/scratch/` (outside the repository): `route-grade.sh NAME ARGS...` routes
  the plugin45 handoff DSN (`target/canary-handoff/2026-10-17-plugin45/canary-pwr.dsn`) with Q16's
  release CLI and grades the SES with K09's `grade_ses.sh` (KiCad 10.0.7 DRC, `--schematic-parity
  --refill-zones --severity-all`); `arm.sh` is K09's plugin-path harness pointed at Q16's
  worktree and CLI; `dangling.py` counts KiCad's `via_dangling` and `track_dangling` per arm and
  attributes each finding of the last arm to the first arm with a finding of the same type at the
  same position. The plugin45 flags (`route-options.json` through `RouteOptions.to_args`) are
  `--max-passes 1 --router.min_trace_width_um=150 --router.scoring.start_ripup_costs=1500
  --router.autorouter.sweep=free --router.autorouter.sweep_passes=2` (profile `enhanced`, the
  default). Commands, one at a time:
  1. build: `cd /home/thebo/code/freerouting-rs/target/orchestrator/q16/wt && CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=/home/thebo/code/freerouting-rs/target/orchestrator/q16/target cargo build --release -p freerouting-cli`;
  2. `S=/home/thebo/code/freerouting-rs/target/orchestrator/q16/scratch; B="--max-passes 1 --router.min_trace_width_um=150 --router.scoring.start_ripup_costs=1500"`;
  3. `$S/route-grade.sh pass1 $B --router.optimizer.enabled=false` (the last pass only);
  4. `$S/route-grade.sh sweep $B --router.autorouter.sweep=free --router.autorouter.sweep_passes=2 --router.optimizer.enabled=false`;
  5. `$S/route-grade.sh plugin45 $B --router.autorouter.sweep=free --router.autorouter.sweep_passes=2`
     (the plugin45 run; `cmp $S/runs/plugin45/route.ses target/canary-handoff/2026-10-17-plugin45/canary-pwr.ses`
     says whether the engine still routes it as on 2026-10-17);
  6. `python3 -I $S/dangling.py $S/runs pass1 sweep plugin45`: the table (a finding first seen in
     `pass1` is the last pass's, in `sweep` the sweep's, in `plugin45` only the optimizer's);
  7. two corpus boards through the plugin path with the same options:
     `for b in interf_u pic_programmer; do for a in q161-pass1 q161-sweep q161-plugin45; do ...; done; done`
     with `$S/arm.sh <arm> $b /home/thebo/code/freerouting-rs/target/canary-handoff/2026-10-17-plugin45/route-options.json -- <the
     arm's extra flags of steps 3-5 beyond the options>` (`--router.autorouter.sweep=off
     --router.optimizer.enabled=false` for pass1, `--router.optimizer.enabled=false` for sweep,
     none for plugin45); each run's `summary.json` holds the DRC counts, `drc.json` the positions.
- [~] **Q16.2 — The cleanup stage.** `router.cleanup.remove_stubs = true | false`, unset by
  default and set in no profile until Q16.3 measures it.
  - **Accept:**
    - fixtures: a via pair on a failed connection is removed; a via onto its own plane is
      kept; a fixed via is kept; a three-stub chain goes in one call;
    - with the setting unset, A14 is unchanged.

  *Partial 2026-10-18:* the stage is in; the full A14 run is pending.
  - Code: settings field `cleanup.remove_stubs` (new section `cleanup`, `opt` bool, unset, in no
    profile), `quality::stubs` (`remove_stubs`, `is_removable_stub`, `input_copper`), called by
    `run_pipeline` last (after the optimizer and the `AfterOptimizer` board stages, also after a
    stop) with the ids of the traces and vias of the board as loaded, which it never removes
    (coordinator's decision 2026-10-18: only copper the router inserted in this run may go, so a
    designer's unfinished hand-laid track stays, locked or not), listed
    in the stage catalog under the optimizer slot (so `--list-stages`, the GUI's Algorithms
    section and the plugin's Advanced group show it); DIVERGENCES line under freerouting.
  - Fixtures, `tests/stub_cleanup.rs` on `tests/data/quality/q16-stubs.dsn` (12 passed; the
    rule tests pass an empty keep set, so the fixture's wiring stands for router copper):
    `a_via_pair_on_a_failed_connection_is_removed`, `a_via_onto_its_own_plane_is_kept`,
    `a_fixed_via_is_kept`, `a_three_stub_chain_goes_in_one_call`,
    `a_completed_connection_is_untouched`, plus a via used on one layer
    (`a_via_used_on_one_layer_goes_and_its_traces_stay_joined`), the totals and determinism
    (5 vias and 6 traces removed, incomplete 3 → 3), a keep set (FAILED's 2 vias and 3 traces
    removed outside the keep set: `only_copper_outside_the_keep_set_goes`), the pipeline with
    the setting on keeping all input copper (`an_unfixed_input_via_pair_that_dangles_is_kept`:
    FAILED's unfixed dangling via pair stays, 0 removed, its log line) and unset or `false`
    (`outcome.stubs == None`, the same item ids),
    and the catalog listing (unset in `legacy` and `enhanced`); `settings_defaults.rs` lists
    `cleanup.remove_stubs = None`.
  - Unset, A14 subset (dev profile, this worktree): `PARITY_MAX_JAVA_S=20 cargo test -p
    freerouting --test autoroute_board_parity -- --ignored autoroute_board_parity
    --test-threads=1`: 60 boards, all 57 `exact` boards `bytes true` (the SES byte-identical to
    Java's), test ok in 39.6 s; `PARITY_SET=tools/bench/sets/connection-parity.txt`: 7 exact,
    all `bytes true`.
  - Remains: heavy run pending, the full A14 with the setting unset:
    `cd /home/thebo/code/freerouting-rs/target/orchestrator/q16/wt && CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=/home/thebo/code/freerouting-rs/target/orchestrator/q16/target PARITY_SUBDIRS=1 PARITY_OUT=/home/thebo/code/freerouting-rs/target/orchestrator/q16/scratch/parity-full cargo test --release
    -p freerouting --test autoroute_board_parity -- --ignored autoroute_board_parity
    --test-threads=1 --nocapture > /home/thebo/code/freerouting-rs/target/orchestrator/q16/scratch/a14.log 2>&1; echo "EXIT $?" >> /home/thebo/code/freerouting-rs/target/orchestrator/q16/scratch/a14.log`
    (about 1.5 h; accept: EXIT 0, i.e. every exact board equal to Java's or in
    `EXPECTED_MISMATCHES` as at `d7d7a6d`).
- [~] **Q16.3 — Measure under N02 (no-regress).** Register first: `canary-pwr` via the plugin
  path plus the K09 boards.
  - **Accept:**
    - KiCad's `via_dangling` and `track_dangling` are 0 on every board;
    - unconnected does not rise on any board;
    - if accepted, N05 adds the setting to `enhanced`.

  *Partial 2026-10-18:* heavy run pending; nothing was run (the heavy-job slot was held by N05).
  Registered before any number: `docs/roadmap/prereg/Q16.json` (sha256
  83d71ce12b22a3aba7f3fd6bc33339d86e0911afbbfd5b200c6c173181438c82, pinned by
  `crates/freerouting-dev/tests/prereg_q16.rs`): N02's totals rule with `accept: no-regress`,
  arms `plugin45` (the plugin45 options) and `plugin45+cleanup` (plus
  `--router.cleanup.remove_stubs=true`), boards canary-pwr (plugin path, and the handoff DSN as a
  variant), `interf_u`, `CM5_MINIMA_3`, `pic_programmer` with K09's hashes and bands; hard
  guards: `kicad_unconnected` and `drc_real` not above the baseline on any board, via_in_pad,
  below_minimum, `via_dangling` and `track_dangling` 0 in the candidate, and determinism (the
  candidate's CLI run twice per board, SES byte-identical). Then, one at a time, with the
  release CLI of step 1 of
  Q16.1 (`cd /home/thebo/code/freerouting-rs/target/orchestrator/q16/wt && CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=/home/thebo/code/freerouting-rs/target/orchestrator/q16/target cargo build --release -p freerouting-cli`):
  `S=/home/thebo/code/freerouting-rs/target/orchestrator/q16/scratch; O=/home/thebo/code/freerouting-rs/target/canary-handoff/2026-10-17-plugin45/route-options.json;
  for b in canary-pwr interf_u CM5_MINIMA_3 pic_programmer; do $S/arm.sh q163-base $b $O;
  for a in q163-cleanup q163-cleanup-rerun; do $S/arm.sh $a $b $O --
  --router.cleanup.remove_stubs=true; done; cmp $S/runs/q163-cleanup/$b/work/$b.ses
  $S/runs/q163-cleanup-rerun/$b/work/$b.ses; done` (the `cmp` is the determinism guard); the numbers are in
  `$S/runs/<arm>/<board>/summary.json` (`drc` holds `warning:via_dangling` and
  `warning:track_dangling`, `unconnected` the KiCad count). Also the handoff DSN itself:
  `$S/route-grade.sh plugin45-cleanup $B --router.autorouter.sweep=free
  --router.autorouter.sweep_passes=2 --router.cleanup.remove_stubs=true`, run twice (second name
  `plugin45-cleanup-rerun`, then `cmp` the two `route.ses`), against Q16.1's `plugin45` (with
  `B` as in Q16.1 step 2). If accepted, N05 adds `cleanup.remove_stubs = true` to `ENHANCED`.
