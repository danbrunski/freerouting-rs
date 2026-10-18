# Q16 — No dangling vias or trace stubs in the result

Opened 2026-10-18, from canary-7a's grade of the owner's `enhanced` plugin run on `canary-pwr`
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

- [x] **Q16.1 — Where the stubs come from.** On `canary-pwr` (the plugin45 handoff DSN and
  the default plugin flags) and two corpus boards, attribute each dangling via and track that
  KiCad reports to the stage that inserted it (last pass, sweep, optimizer) using the
  routing ledger or the verdicts.
  - **Accept:** a table in this file, with a count per stage.

  *Done 2026-10-18:* the table is at the end of this note; the runs followed the steps below. The
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

  Runs (origin/main `17431c4`, release CLI sha256 `589b0fd3…`, WSL KiCad 10.0.7; enhanced is
  `power_layer_traces = none` there, neck-down is not on main). KiCad's `via_dangling` /
  `track_dangling` per arm, then each finding of the last arm attributed to the first arm with a
  finding of the same type at the same position (`dangling.py`):

  | board | pass 1 only | + sweep | + optimizer (plugin45) | last pass | sweep | optimizer |
  |---|---|---|---|---|---|---|
  | canary-pwr, plugin45 handoff DSN | 22 / 0 | 16 / 0 | 16 / 0 | 16 vias | 0 | 0 |
  | interf_u (plugin path) | 9 / 0 | 9 / 0 | 9 / 0 | 9 vias | 0 | 0 |
  | pic_programmer (plugin path) | 0 / 0 | 0 / 0 | 0 / 0 | 0 | 0 | 0 |
  | CM5_MINIMA_3 (plugin path, extra) | 4 / 1 | 0 / 1 | 0 / 1 | 1 track | 0 | 0 |

  Count per stage over the four boards: **last pass 26 vias and 1 track, sweep 0, optimizer
  0**. Every stub left at the end is copper the first (and, with `--max-passes 1`, last) pass
  inserted; the sweep removes some of them (canary 22 → 16, CM5 4 → 0 vias) and adds none. The
  optimizer never ran: it skips a board with unrouted connections (canary 95, interf_u 4, CM5 8),
  so on canary the `sweep` and `plugin45` sessions are byte-identical (`04de8e96…`), as are all
  three interf_u sessions (`f5b27354…`) and pic_programmer's (`3dcecedb…`). KiCad unconnected
  on the handoff DSN: 48 (pass 1), 37, 37. The `plugin45` session differs from the owner's
  (`5ad40948…`, the Windows build, N05.3's amendment) at byte 9964. The canary stubs are on
  supervisor and holdup signals (`/Holdup/LTC_CAPRTN` 2, `LTC_PFO` 2, `MCU_SCL` 2,
  `/Supervisor/CAN_FAULT_N` 2, and one each on `/Holdup/VCAP`, `/Holdup/LTC_CFP`,
  `LTC_SMBALERT`, `BUCK_PG`, `+3V3_AON`, `/Pi_Interface/PI_NRST`, `/Supervisor/LED_BANK`,
  `/Pi_Interface/I2C_READY`). CM5's pass-1 run is `8fa5b64a…`; its sweep run is the plugin45
  run (`384f05df…`, the `q163-base` session of Q16.3). Runs:
  `target/orchestrator/q16/scratch/runs/{pass1,sweep,plugin45,q161-*,q163-base}`.
- [x] **Q16.2 — The cleanup stage.** `router.cleanup.remove_stubs = true | false`, unset by
  default and set in no profile until Q16.3 measures it.
  - **Accept:**
    - fixtures: a via pair on a failed connection is removed; a via onto its own plane is
      kept; a fixed via is kept; a three-stub chain goes in one call;
    - with the setting unset, A14 is unchanged.

  *Done 2026-10-18:* the stage is in and the full A14 with the setting unset is unchanged.
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
  - Unset, full A14 (release, on origin/main `17431c4` plus this change's docs, which touch no
    engine code): `CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 PARITY_SUBDIRS=1
    PARITY_OUT=target/orchestrator/q16/a14/parity-full cargo test --release -p freerouting --test
    autoroute_board_parity -- --ignored autoroute_board_parity --test-threads=1 --nocapture`:
    `test result: ok. 1 passed`, EXIT 0, 5108 s. `summary.md`: exact 122 boards, 120 pass, 2
    expected (Issue684, Issue721, as at `d7d7a6d`), 0 fail, 119 SES byte-identical and 120
    equivalent (Issue110-RelayModule differs only in the non-ASCII component name, as in N06's
    A14); cutoff 15/15 pass, all byte-identical (class sums 1316 unrouted, 1743 violations, 4670
    vias, Java = Rust); java-failure 3/3; 0 D8 breaches added.
- [~] **Q16.3 — Measure under N02 (no-regress).** Register first: `canary-pwr` via the plugin
  path plus the K09 boards.
  - **Accept:**
    - KiCad's `via_dangling` and `track_dangling` are 0 on every board;
    - unconnected does not rise on any board;
    - if accepted, N05 adds the setting to `enhanced`.

  *Partial 2026-10-18:* measured as registered; the gate **fails** on two hard guards, so the
  setting stays unset in both profiles (the registration's keep rule). Registered before any number: `docs/roadmap/prereg/Q16.json` (sha256
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

  - Amendment, before any Q16.3 number: `enhanced` at the measured tree is
    `power_layer_traces = none` (N05, `f010b1e`, after the registration); both arms run under it
    unchanged; neck-down is not on origin/main (`17431c4`) and is in neither arm. New sha256
    `d3d20814b910f432e799b2262218f8dbe4b700e408a6b2323787b7bdd0a24493`, pinned by
    `crates/freerouting-dev/tests/prereg_q16.rs` (1 amendment).
  - Runs as registered (one release build, CLI `589b0fd3…`, WSL KiCad 10.0.7, the commands above);
    records `target/orchestrator/q16/scratch/records-q163.json` (`analyze163.py`; drc_real per the
    registration against K09's `_input` grades; each row's `connections` is its unrouted input's
    KiCad unconnected count, for the registered default band). `freerouting-dev gate --records
    records-q163.json --baseline baseline-q163.json --prereg docs/roadmap/prereg/Q16.json`
    (baseline written from the same records): 0 refusals, **gate: FAIL** (exit 1), totals 121 →
    122 against set noise 18.08, rejected on the hard guards
    `canary-pwr.kicad_pcb.kicad_unconnected 37 -> 38` and `CM5_MINIMA_3.track_dangling = 1`.

    | row | KiCad unconnected | via_dangling | track_dangling | drc_real | starved | wall s |
    |---|---|---|---|---|---|---|
    | canary-pwr (plugin path) | 37 → **38** | 15 → 0 | 0 → 0 | 1 → 0 | 5 → 5 | 164.2 → 162.5 |
    | canary-pwr (handoff DSN) | 37 → 37 | 16 → 0 | 0 → 0 | 1 → 0 | 5 → 4 | 107.2 → 106.3 |
    | interf_u | 18 → 18 | 9 → 0 | 0 → 0 | 0 → 0 | 9 → 9 | 6.4 → 6.3 |
    | CM5_MINIMA_3 | 27 → 27 | 0 → 0 | 1 → **1** | 49 → 49 | 0 → 0 | 29.4 → 29.4 |
    | pic_programmer | 2 → 2 | 0 → 0 | 0 → 0 | 2 → 2 | 0 → 0 | 0.3 → 0.3 |

    via_in_pad and below_minimum are 0 in every run. Determinism: the candidate's two runs are
    byte-identical on every board (`cmp`: canary-pwr `be9ce10b…`, handoff `f417d59f…`, interf_u,
    CM5 `384f05df…` (= the baseline: 0 removed), pic_programmer). Engine log of the candidate:
    canary-pwr 16 vias and 11 traces removed in 3 rounds, handoff 17 and 14, interf_u 9 and 6,
    CM5 and pic_programmer 0.
  - Cause 1, the unconnected rise on canary-pwr: the cleanup removed the `+12V_PROT` via at
    (136.355, 68.960) mm and its B.Cu trace to D5 pad 2. On F.Cu the via sits inside the
    `+12V_PROT` pour, which the plugin exports as fixed wires (`fixed_pour_classes` Power_High:
    `(wire (path F.Cu 250 136325 -67127 136325 -71964) ... (type fix))`), crossing that strip
    mid-segment. The stage counts contacts as Java's normal contacts (`BasicBoard::normal_contacts`,
    `contact_at_point`), where a via touches a trace only when an end corner of the trace is the
    via's center, so the via had one contact (the B.Cu trace) and went; KiCad counts
    any overlap, so the via joined D5 pad 2 to the pour (it was not in KiCad's `via_dangling`) and
    its removal adds the `+12V_PROT` item D5 pad 2 ↔ pour (1 → 2 on that net, the only net that
    changed). The same via goes on the handoff row (`+12V_PROT` 1 → 2 there too), where a GND
    thermal that its removal frees (GND 8 → 7, starved 5 → 4) hides the rise. This via is also the
    one clearance error of both baselines (0.245 mm from an `LTC_INTVCC` track, Q14).
  - Cause 2, CM5's kept `track_dangling`: a `+3V3_PI` B.Cu trace of 0.555 mm from R102 pad 1
    (123.93, 62.98) to (124.485, 62.98), inserted by the first pass (Q16.1). Its open end lies
    inside the DSN's `+3V3_PI` B.Cu plane polygon (the zone outline, `(polygon B.Cu 0 127150
    -63200 126560 -62600 124410 -62600 124410 -63140 ...)`), so the router and the cleanup see a
    plane contact and keep it (neither input copper nor a fixed item nor the contact guard: the
    plane contact itself); KiCad's refilled zone does not reach the point (`ZONE.Outline().
    Contains` true, `HitTestFilledArea` false at the end), so KiCad reports the end as open. The
    stage cannot see this from the DSN: the plane is the zone's outline, not its fill.
  - Remains: Q16.4. The operator decided on 2026-10-18: (1) the stub test counts contacts as KiCad
    does (any same-net copper overlapping on a shared layer, by the board's exact shapes);
    (2) cause 2 is out of the stage's scope and a known limit (the DSN's plane is the zone outline,
    not KiCad's fill), and the hard clause becomes "no dangling via or track visible from the DSN";
    (3) a new registration on boards Q16.3 did not use. Sessions with and without the setting:
    `target/canary-handoff/2026-10-18-q16/`.
- [~] **Q16.4 — Contacts counted as KiCad counts them, measured again.** Q16.3 failed on two
  stubs the stage judged by Java's normal contacts and by the DSN's plane outlines; the operator's
  decisions are in Q16.3's "Remains".
  - **Accept:**
    - the stub test counts any same-net copper overlapping the item on a shared layer as a contact
      (a fixture: a via inside a fixed pour strip, crossing it mid-segment, is kept);
    - the CM5 case (a trace ending inside a plane's outline outside KiCad's fill) is a known limit,
      recorded here and in DIVERGENCES;
    - a new registration on boards Q16.3 did not use, then: KiCad's `via_dangling` and
      `track_dangling` visible from the DSN are 0 on every board, unconnected does not rise on any
      board, no new DRC error, deterministic; if accepted, N05 adds the setting to `enhanced`.

  *Partial 2026-10-18:* the code, the fixtures, the known limit and the registration are in; the
  registered measurement is pending (heavy run pending: the heavy slot went to a full A14).
  - Code: `quality::stub_contacts` (new): an item's copper per layer (`pieces`: a trace's segment
    shapes at its half width, a via's or pin's pad shapes, a conduction area's tile shapes),
    `touches` (a common net and two pieces on one layer that intersect, `Shape::intersects`,
    borders included, no tolerance), `overlap_contacts`, `end_has_contact` (N04's end cap: the disc
    of half the width around the end, `Shape::intersects_circle`) and `via_contact_layers`.
    `quality::stubs::is_removable_stub` now calls a trace a stub when an end has no contact and a
    via when its pad meets copper of its net on at most one layer, and its guard asks that the
    overlap contacts all touch one another; `remove_stubs` follows the removed item's overlap
    contacts into the next round. N04's helpers in `board/contact_rule.rs` are gated on
    `pad_contacts = inside_pad`, so the stage reuses its end-cap test (`Shape::intersects_circle`
    on an integer centre, the point alone on a rational one) rather than the functions. Independent
    of `router.pad_contacts`; nothing runs with the setting unset, so `legacy` and A14 do not move.
    Module docs (both modules) and the DIVERGENCES line (freerouting, `cleanup.remove_stubs`) name
    the known limit.
  - Unset, nothing moves: A14 subset (dev profile) `PARITY_MAX_JAVA_S=20 cargo test -p freerouting
    --test autoroute_board_parity -- --ignored autoroute_board_parity --test-threads=1`: exact 57
    boards, 57 pass, 57 SES byte-identical, java-failure 3/3, test ok in 48.5 s;
    `PARITY_SET=tools/bench/sets/connection-parity.txt`: exact 7, all byte-identical.
  - Fixtures, `tests/stub_contacts.rs` on `tests/data/quality/q16-overlap-stubs.dsn` (7 passed):
    `java_sees_stubs_where_kicad_sees_joints` (each of the four joints is a Java tail and no stub
    here), `a_via_crossing_a_fixed_strip_mid_segment_is_kept` (Q16.3's canary via: STRIP 2 vias and
    3 traces before and after), `a_trace_ending_on_a_trace_body_is_kept`,
    `a_via_off_a_pad_centre_is_kept`, `copper_that_touches_nothing_still_goes` (OPEN 0 and 0),
    `the_border_counts_and_one_unit_off_does_not` (a pad meeting the strip's copper exactly at its
    border is kept, one board unit (0.1 um) further it and its chain go),
    `the_cleanup_completes_nothing_less_and_is_deterministic` (4 vias and 4 traces removed,
    incomplete count equal, same ids twice). Q16.2's `tests/stub_cleanup.rs` passes unchanged (12).
  - Known limit, measured on Q16.3's runs by `tools/kicad-oracle/dangling_visible.py` (new; KiCad
    python on the saved board, its DRC report and the run's DSN): CM5_MINIMA_3's candidate
    `track_dangling` 1 → `track_dangling_dsn` 0 (the `+3V3_PI` end at (124.485, 62.98) mm on B.Cu
    is inside the DSN plane, outside the fill); canary-pwr base 15 → 15 and interf_u 9 → 9 (all
    visible from the DSN).
    Unit tests of its DSN plane reader: `tools/kicad-oracle/tests/test_dangling_visible.py` (3;
    `python3 -m unittest discover -s tools/kicad-oracle/tests -p 'test_*.py'`: 12 OK).
  - Registration before any number: `docs/roadmap/prereg/Q16-4.json` (sha256
    `45a2679297b0ce5cc0684f98d9d90c14e965b2fdf8955d3dc04679964cebbf2d`, pinned by
    `crates/freerouting-dev/tests/prereg_q16_4.rs`): N02 totals, `accept: no-regress`, arms
    `plugin45` and `plugin45+cleanup` under the current `enhanced` (`power_layer_traces = none`,
    `neck_width_um = 300`, origin/main `27d7409`), boards canary-pwr (plugin path) and the stripped
    KiCad 10.0.7 demos kit-dev-coldfire-xilinx_5213, StickHub, RoyalBlue54L-Feather,
    multichannel_mixer (from `multichannel_mixer-unrouted.kicad_pcb`), stripped into
    `target/orchestrator/q16/scratch/boards/`; hard guards `via_dangling_dsn` and
    `track_dangling_dsn` 0, unconnected and drc_real not above the baseline on any board,
    via_in_pad and below_minimum 0, determinism.
  - Heavy run pending, one at a time (`S=/home/thebo/code/freerouting-rs/target/orchestrator/q16/scratch`,
    `O=/home/thebo/code/freerouting-rs/target/canary-handoff/2026-10-17-plugin45/route-options.json`):
    1. `cd /home/thebo/code/freerouting-rs/target/orchestrator/q16/wt && CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0
       CARGO_TARGET_DIR=/home/thebo/code/freerouting-rs/target/orchestrator/q16/target cargo build --release -p freerouting-cli -p freerouting-dev`;
    2. the inputs: `for b in kit-dev-coldfire-xilinx_5213 StickHub RoyalBlue54L-Feather
       multichannel_mixer; do sha256sum $S/boards/$b/$b.kicad_pcb; mkdir -p $S/runs/_input164/$b;
       cp -r $S/boards/$b/. $S/runs/_input164/$b/; kicad-cli pcb drc --refill-zones --severity-all
       --format json -o $S/runs/_input164/$b/drc.json $S/runs/_input164/$b/$b.kicad_pcb; done`
       (the hashes must equal the registration's; canary-pwr's input grade is K09's
       `runs/_input/canary-pwr/drc.json`);
    3. `for b in canary-pwr kit-dev-coldfire-xilinx_5213 StickHub RoyalBlue54L-Feather
       multichannel_mixer; do timeout -k 10 3600 $S/q164-arm.sh q164-base $b $O; for a in
       q164-cleanup q164-cleanup-rerun; do timeout -k 10 3600 $S/q164-arm.sh $a $b $O --
       --router.cleanup.remove_stubs=true; done; cmp $S/runs/q164-cleanup/$b/work/$b.ses
       $S/runs/q164-cleanup-rerun/$b/work/$b.ses && echo "CMP $b identical"; done` (`q164-arm.sh`,
       sha256 `822508fe…`, is Q16.3's `arm.sh` with the demos from `$S/boards` and
       `dangling_visible.py` after the DRC, its counts in `summary.json`);
    4. `python3 -I $S/analyze164.py docs/roadmap/prereg/Q16-4.json $S/records-q164.json`, then
       `../target/release/freerouting-dev gate --records $S/records-q164.json --baseline
       $S/baseline-q164.json --prereg docs/roadmap/prereg/Q16-4.json --write-baseline` and the same
       without `--write-baseline` for the verdict.

  *Partial 2026-10-18 (measured):* the registered run is done and the gate **fails** on four hard
  guards, so the setting stays unset in both profiles (the keep rule); no promotion.
  - Amendment 1 before any route (`Q16-4.json` sha256
    `c0674fb312912cbd6d006404c2c0c0fa247281392533b66d8a908e261adcec9e`, re-pinned in
    `prereg_q16_4.rs`, `cargo test --release -p freerouting-dev --test prereg_q16_4`: 2 passed):
    engine main b93bcd0, whose `enhanced` adds `via_clearance = net_class`; the arms keep plugin45's
    route options as registered (they already name `sweep = free`, `sweep_passes = 2`, so K09.9's
    plugin default changes nothing in them); adds `seen_before_registration`. Board hashes checked:
    the four stripped demos equal the registration's, canary-pwr's input `15aa06a3…`;
    `q164-arm.sh` `822508fe…`. Unrouted inputs (KiCad unconnected): kit-dev 478, StickHub 128,
    RoyalBlue54L-Feather 146, multichannel_mixer 176; canary-pwr K09's grade.
  - Per board, baseline → candidate (KiCad 10.0.7, `records-q164.json` and `gate-q164.txt` in
    `target/orchestrator/q16/scratch/`):

    | board | unconnected | via_dangling_dsn | track_dangling_dsn | drc_real | below_minimum | determinism |
    |---|---|---|---|---|---|---|
    | canary-pwr (plugin path) | 41 → 41 | 17 → 0 | 0 → 0 | 0 → 0 | 0 → 0 | identical |
    | kit-dev-coldfire-xilinx_5213 | 33 → 33 | 11 → **1** | 0 → 0 | 200 → 200 | **199 → 199** | identical |
    | StickHub | 31 → 30 | 7 → 0 | 0 → 0 | 13 → **14** | 0 → 0 | identical |
    | RoyalBlue54L-Feather | 96 → 95 | 2 → 0 | 0 → 0 | 26 → 26 | 0 → 0 | identical |
    | multichannel_mixer | 6 → 6 | 0 → 0 | 0 → 0 | 75 → 75 | **58 → 58** | identical |

    KiCad's raw `via_dangling` 17 → 0, 13 → 3, 8 → 1, 4 → 2, 0 → 0 and `track_dangling` 0, 0, 0,
    56 → 56, 0 (the rest are the plane-fill limit); via_in_pad 0 everywhere. Totals 207 → 205
    against a set noise of 29.50. `freerouting-dev gate`: **FAIL** (totals verdict reject on the
    hard guards). Determinism: the candidate rerun's SES is byte-identical to the registered one on
    all five boards (`cmp`). The stage removed 17 vias and 12 traces on canary-pwr, 10 vias and 2
    traces on kit-dev.
  - Cause 1, `below_minimum` 199 and 58 in **both** arms: plugin45's `min_trace_width_um = 150` (with
    `enhanced`'s neck-down) puts 0.15 mm traces on demos whose Board Setup minimum is 0.1981 mm
    (kit-dev) and 0.2 mm (multichannel_mixer). The registration's options, not the stage: the
    baseline breaks the same hard limit by the same count.
  - Cause 2, kit-dev's one visible dangling via: `Net-(D101-K)` at (162.2687, 108.747) mm, a
    through via the router left on Top_layer with three same-net contacts there: two traces that end
    at its centre (to (162.0497, 108.966) and to (163.8948, 107.1209)) and the trace from U101 pad 4
    ending at (162.0497, 108.966), 0.31 mm off centre, inside its 0.6 mm pad. The stage's guard asks
    that every two contacts touch; the last two do not (they meet only through the first), so the
    via stays, though its removal would split nothing. A guard on the contacts' touch graph being
    connected, rather than complete, would remove it; that is an engine change after the
    registration, so it is not in this run (stop rule).
  - Cause 3, StickHub `drc_real` 13 → 14: one more `solder_mask_bridge`, which is KiCad's grading
    noise: the determinism rerun's SES is byte-identical and grades 13. KiCad reports one item pair
    per bridged mask aperture, and the pair it picks differs between gradings of the same session.
  - canary-pwr alone passes every guard: 41 → 41 unconnected, 17 → 0 dangling vias, no new error.
    Sessions for canary's official grade: `target/canary-handoff/2026-10-18-q164/`.
  - Remains (operator decision): a new registration or an amendment that (a) runs the demos with
    options that respect their Board Setup minimum (no plugin45 `min_trace_width_um`), (b) takes
    the connected-graph guard if it is adopted, and (c) judges `solder_mask_bridge` without KiCad's
    pair-picking noise (for example by aperture, or by grading twice). Q16 stays open.
