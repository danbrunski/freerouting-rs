# Q01 — Planes block only pass-through nets

Opened 2026-10-15. On `canary-pwr`, signal traces crossed the power pours on
F.Cu and B.Cu, and KiCad's refill then cut the pours apart. Java can only make
every plane an obstacle at once (`router.plane_as_obstacle`), which also blocks
nets whose own pads sit inside a pour: the ordinary GND-pour case on a
two-layer board. This checklist adds a per-net-class rule taken from KRT's
pour guard (`py_router/pour_guard.py`, `compute_pour_barriers`, at `0486e799`):
**a pour of a listed class blocks a net only if that net has no pin or via
inside it.** It also adds a report of which pours routing cut.

Prerequisites: board parity (rung 3); the obstacle predicate hook (B09,
provisional id; see "Decisions"); C02 (settings model) and C03 (settings
applied to the board); F03.1 and F03.4 for the measurement; F02.6 for KiCad
grading.

**Fixture:** `fixtures/canary-pwr/canary-pwr-pours-as-planes.dsn` is Dan's board with the pours left as KiCad planes: the case this checklist exists for. Its siblings `canary-pwr-pours1..3.dsn` are the same board with the pours exported as fixed wires; that is the workaround to compare against (see `fixtures/canary-pwr/README.md`).

## What exists and what this adds

| Exists | State |
|---|---|
| Rust `board::items::AreaKind::Conduction { is_obstacle, is_filled }`; `Item::conduction_is_obstacle` (`board/items/mod.rs`); `BasicBoard::is_trace_obstacle` (`board/basic_board.rs`, Java `isTraceObstacle`): a conduction area is an obstacle for net M iff `is_obstacle && !contains_net(M)`. (`Item::is_obstacle_for_net` is only the same-net test the tree queries use.) | the Java rule, per area, not per net |
| Rust `RoutingBoard::change_plane_as_obstacle` (`board/routing_board.rs`) | port of Java `RoutingBoard.changePlaneAsObstacle`: flips every signal-layer area and reinserts the tree items |
| Rust `drc::zone_islands::fill_islands(board, ca)` | port of Java `ConductionArea.getDetailedFillArea`: the area minus foreign items at clearance, split into islands |
| Java `HeadlessBoardManager.applyPlaneAsObstacleOverride` → `RoutingBoard.changePlaneAsObstacle` (cda540f) | the all-or-nothing switch |
| Java D8 rule: signal-layer pours are ignored for nets that have a power-layer plane | must stay true |
| KRT `pour_guard.compute_pour_barriers`, `obstacle_map.add_pour_barrier_obstacles`, `pour_guard.audit_pour_cuts` | the rule and the audit; its test is "any net in the run has a terminal inside", which depends on the run's scope |

What this adds: a settings key, a per-(area, net) exemption table built once
per job, the predicate that uses it, a pour-cut report, and the measurement.

## Decisions made here

- **The key** is `router.plane_obstacle_net_classes`: a list of net class
  names, default empty (off). It sits beside Java's `plane_as_obstacle` in the
  C02 settings struct. When `plane_as_obstacle` is true, Java's rule wins and
  this key changes nothing.
- **The exemption table is computed once, from the input board.** For each
  conduction area A whose net's class is listed, on A's layer: the set of nets
  M ≠ A's net that have a pin, or a via present in the input, whose shape
  centre lies inside A's outer boundary (holes count as inside). Vias the
  router adds later don't change the table, so routing order can't change it
  (D2). Stored as `BTreeMap<ItemId, Vec<i32>>` with each `Vec` sorted.
- **The predicate:** A is an obstacle for M iff Java's rule says so, or (A is
  listed, M is not A's net, and M is not exempt for A). Same-net items connect
  as today, including at the edge.
- **Where the predicate lives.** The search consults conduction areas through
  `BasicBoard::is_trace_obstacle` (its conduction branch). Q01 needs a hook there that is `None` in the
  parity path: an `Option<Arc<dyn AreaObstaclePolicy>>` on the board rules,
  consulted only in the conduction-area branch. That hook is a B-series
  board service (provisional B09). Q01 implements the policy; it doesn't edit
  `board/items/mod.rs`.
- **The pour-cut report uses Java's own fill** (`fill_islands`). Whether it
  needs KiCad's minimum-thickness opening is measured (Q01.4), not assumed.
- **KRT's fragility cost** (a soft cost near pour edges, measured on one board)
  is an optional experiment (Q01.6), after Q05's lattice and A04's cost hook.

## File ownership

| Item | New files | May edit |
|---|---|---|
| Q01.1 | `crates/freerouting/src/quality/pour_guard.rs` | `quality/mod.rs` (append), the C02 settings struct (one field) |
| Q01.2 | — | `quality/pour_guard.rs`; the C03 apply step (one call to install the policy) |
| Q01.3 | `fixtures/quality/q01-pour-between-pads.dsn`, `fixtures/quality/q01-pad-inside-pour.dsn`, `crates/freerouting/tests/quality_pour_guard.rs` | — |
| Q01.4 | `crates/freerouting/src/quality/pour_audit.rs` | `quality/mod.rs` (append) |
| Q01.5 | `tools/bench/sets/q01-pours.txt`, the Q01 preregistration and baseline files | `docs/DIVERGENCES.md` (append, only if kept) |
| Q01.6 | `crates/freerouting/src/quality/pour_fragility.rs` (only if kept) | `quality/register.rs` (append) |

## Checklist

- [x] **Q01.1 — Settings key and exemption table.**
  Source: KRT `pour_guard.compute_pour_barriers` (an island is a barrier when
  no routed net has a terminal inside it).
  Rust design: `quality::pour_guard::ExemptionTable::build(board: &BasicBoard,
  classes: &[String]) -> ExemptionTable`. Iterate `board.conduction_areas()` in
  ascending `ItemId`; skip areas whose net's class isn't listed; for each pin
  and via in ascending `ItemId` on the area's layer, test its shape centre
  against the area's outer boundary polygon with the existing point-in-shape
  test. Unknown class names are a settings error naming the class.
  - **Orders that decide results:** none reach routes; the table is sorted.
  - **Accept:** unit tests on a hand-built board: a pad inside a hole of the
    pour is exempt; a pad outside is not; the area's own net is never listed;
    an unknown class name fails with its name in the message.
  *Done 2026-10-16:* `quality::pour_guard::ExemptionTable::build` and the settings field
  `plane_obstacle_net_classes` (`settings/fields.rs`, `opt` `Vec<String>`, unset by default;
  `settings_defaults` updated). Only areas on signal layers are listed (a power-layer plane is
  crossed by every net's vias, and Q01.7 keeps foreign nets off power layers); pin and via
  centres are their pad shape's centre of gravity on the area's layer. Tests in
  `tests/quality_pour_guard.rs` on the synthetic boards of Q01.3:
  `a_pad_inside_a_hole_of_the_pour_is_exempt` (T inside the hole is the one exemption),
  `a_pad_outside_the_pour_is_not_exempt` (S), `the_area_own_net_is_never_listed` (Z's two pads
  lie inside its pour; 0 exemptions), `an_unlisted_class_lists_no_pour`,
  `an_unknown_class_name_fails_with_its_name` (`router.plane_obstacle_net_classes: the board has
  no net class 'Nope' (its classes: ...)`, from `build` and from `prepare_job` as
  `JobError::Settings`). `cargo test -p freerouting --test quality_pour_guard`: 13 passed.

- [x] **Q01.2 — The obstacle policy and "off means identical".**
  Implement `AreaObstaclePolicy` for `ExemptionTable` and install it in the
  C03 apply step when the key is non-empty and `plane_as_obstacle` is false.
  Keep D8's rule: an area that the Java code ignores for a net with a
  power-layer plane stays ignored for it.
  - **Accept:**
    - with the key empty, `ses-diff` shows identical sessions on
      `connection-parity` and `benchmark` against the pre-change engine;
    - a unit test: with `plane_as_obstacle = true` the policy isn't
      installed;
    - `tools/determinism-check.sh` passes on `q01-pour-between-pads.dsn` with
      the key set.
  *Done 2026-10-16:* `impl AreaObstaclePolicy for ExemptionTable` (a listed area blocks every
  foreign net that is not exempt); `quality::pour_guard::install` runs as step 8 of
  `job::prepare::prepare_job` (and in `prepare_board_for_routing`, where an unknown class is
  logged as an error), on the board as loaded with the final settings; it installs nothing when
  the key is unset or empty or `plane_as_obstacle` is true. B09's bypasses now go through the
  policy: the drill page's `is_drillable` (`BasicBoard::is_drillable_with_policy`,
  `autoroute/session/drills.rs`) and the shover's two reads (`area_policy_blocks_any_net`,
  `shove/spring.rs`, `shove/trace_entries.rs`); each is one `Option` test without a policy.
  D8 holds by construction: the DSN reader drops a signal-layer pour of a net with a power-layer
  plane, so it is never listed (`pours_of_nets_with_a_power_plane_stay_ignored`, canary-pwr
  pours-as-planes with `Power_High,GND`).
  - Off means identical: the A14 gate (worktree of `0ab934a` plus this diff, key unset,
    `PARITY_SUBDIRS=1`, 140 boards, 6515 s) is green, and every board of `connection-parity` and
    `benchmark` (25 rows) gives a session byte-identical to Java's reference. Against the
    pre-change engine (the A14 run at `b5ec794`, `target/parity/*.ses`): 114 of the 116 sessions
    are byte-identical; the two that differ, `Issue145-smoothieboard` and `Issue753-CPU-85_r104`,
    are Q01.7's change and equal Java `7f56b062d`'s. Compared byte for byte (`cmp`), which is
    stronger than `ses-diff`.
  - `plane_as_obstacle_wins_and_the_policy_is_not_installed` and
    `the_policy_is_installed_only_when_the_key_lists_a_class` (`tests/quality_pour_guard.rs`).
  - `tools/determinism-check.sh --out <dir> -- bash -c '... freerouting -de
    q01-pour-between-pads.dsn -do "$1/out.ses" --router.plane_obstacle_net_classes=Pwr
    --router.autorouter.max_passes=5' _ {OUT} <cli>`: `identical` (idle and beside 8 burners).

- [x] **Q01.3 — Routing fixtures.**
  `q01-pour-between-pads.dsn`: a 12 mm wide pour of net Z (class `Pwr`) on
  F.Cu between two F.Cu pads of net S, with room to go around it on F.Cu.
  `q01-pad-inside-pour.dsn`: the same pour with a pad of net T inside a hole in
  it.
  - **Accept:**
    - with `router.plane_obstacle_net_classes = ["Pwr"]`, S never crosses the
      pour (no S trace intersects the pour's outline) and routes around it;
    - with the key empty, S crosses it (proving the fixture tests something);
    - T still routes into the pour's hole in both cases;
    - no new clearance violations and no via in a pad on either fixture.
  *Done 2026-10-16:* the fixtures are in `crates/freerouting/tests/data/quality/` (as Q03's),
  not `fixtures/quality/`: every DSN under `fixtures/` needs a Java load reference
  (`reference_load` walks the tree). One-layer boards (F.Cu only), so "around" means on F.Cu and
  no via is possible. Tests in `tests/quality_pour_guard.rs`: `with_the_key_set_s_routes_around_the_pour`
  (S complete, no S trace overlaps the pour by area, violations equal to the input's, no via),
  `with_the_key_empty_s_crosses_the_pour` (S complete and crossing), `t_routes_into_the_hole_in_both_cases`
  (T complete and crossing into the hole with the key empty and set, S off the pour with the
  key set, no new violations, no via). `cargo test -p freerouting --test quality_pour_guard`:
  13 passed.

- [~] **Q01.4 — Pour-cut report (report only).**
  Source: KRT `pour_guard.audit_pour_cuts` (open pads per zone net before and
  after; it reports and doesn't refuse).
  Rust design: `quality::pour_audit::audit(before: &BasicBoard, after:
  &BasicBoard) -> PourAudit`. For each filled conduction area, in ascending
  `ItemId`: its islands (`fill_islands`), and per island the pins and vias of
  the area's net it touches. Report per pour net: island count before and
  after, and pins attached to no island after that were attached before.
  Emitted in the CLI result JSON (C08) under `pour_audit`, and in
  `freerouting-dev bench` records.
  - **Accept:**
    - on `q01-pour-between-pads.dsn` with the key empty, Z's report shows its
      island count rising and names the cut; with the key set it shows none;
    - the SES is byte-identical with the report on and off;
    - on 3 KiCad demo boards with pours (F02.6), the island counts agree with
      KiCad's refill in at least 95 % of zones; disagreements are listed in the
      done note. Below 95 %, record whether KiCad's minimum-thickness opening
      explains them, and open a follow-up item rather than changing
      `fill_islands` (it is Java parity code, B07).
  *Partial 2026-10-16:* `quality::pour_audit::audit(before, after) -> PourAudit` (per pour net:
  areas, islands before and after from `drc::conduction_fill`, pins attached before and to no
  island after, and per area whose island count rose the foreign nets of the new traces that
  overlap it; `PourAudit::to_json` in a fixed key order).
  - `the_report_names_the_cut_with_the_key_empty_and_none_with_it_set`: with the key empty Z's
    islands rise and the cut names `S`; with `Pwr` listed the report is clean.
  - `the_session_is_the_same_with_the_report_on_and_off`: the SES is byte-identical before and
    after the audit (and on a second run).
  - KiCad agreement (KiCad 10.0.7, zones refilled with `ZONE_FILLER`, the refilled board's
    `ExportSpecctraDSN` loaded by the port, counts per (net, layer); islands compared with the
    port's anchored islands where KiCad removes islands, all islands where it keeps them): 36 of
    73 zones agree (49 %): CM5_MINIMA_3 5/13, complex_hierarchy 31/52 (two of its pours are
    dropped by the reader, D8), ecc83-pp 0/1, interf_u 0/1 (KiCad 23, port 27 anchored of 130),
    kit-dev-coldfire 0/3, pic_programmer 0/1 (3 vs 5), video 0/2. The port always counts more.
    KiCad's minimum-thickness opening does not explain it: no anchored island of the port has a
    bounding box narrower than the zone's minimum thickness (0.2-0.381 mm). The likely causes are
    KiCad joining fill regions through solid pad connections and thermal spokes, and the fill
    model's bounding-box anchor test (`Island::contains`). Helper:
    `quality_pour_guard::dump_island_counts` (ignored). `fill_islands` is unchanged.
  - Remains: (1) the `pour_audit` member of the CLI result JSON (C08) and `freerouting-dev bench`
    records: `crates/freerouting-cli/*` is held by the concurrent K02 agent, and `bench` reads the
    CLI's result; (2) a follow-up item for the KiCad island disagreement (proposed: Q01.4a, an
    island-by-island comparison with KiCad's filled polygons).

- [~] **Q01.5 — Measurement.**
  Preregister (F03.4): arms `off` and `on` with Dan's classes on `canary-pwr`
  and its variants, and with the pour net's class listed on at least three
  boards that have signal-layer pours (chosen from the F02.6 demo set and the
  F03.3 corpus, listed in `tools/bench/sets/q01-pours.txt`). Signal: KiCad
  `unconnected_items` for pour nets after refill, and Q01.4's lost pins.
  Guards: unrouted, `drc_real`, vias in pads.
  - **Accept:** measured. Kept only if the pour losses fall on at least N−1
    boards and unrouted rises on none. If kept, a DIVERGENCES line with the
    numbers. If not, the code from Q01.1–Q01.4 that only serves the rule is
    removed (the report may stay if Dan wants it) and the numbers go in this
    done note.
  *Partial 2026-10-16:* preregistered in `docs/roadmap/prereg/Q01.json` (arms `off` and `on`,
  per-board classes decided from the inputs: interf_u `Power`, pic_programmer `POWER`,
  kit-dev-coldfire `POWER`, complex_hierarchy `power`, canary-pwr `Power_High,Switch_Node`;
  hard via_in_pad and below_minimum, guards drc_real and unrouted, scores pour_unconnected and
  pour_lost_pins), boards in `tools/bench/sets/q01-pours.txt` (sha256 of each input in the
  registration). Not measured: the Q00 README puts the first measurement of a route-changing Q
  item after P04's default-tree decision, P04 is open, and canary-pwr is graded only by
  canary-23. Remains: the run and the decision, after P04.

- [~] **Q01.6 — Narrowness cost (optional experiment, after Q05.2).**
  Source: KRT `plane_fragility.py` (a cost ramp, steep at pour boundaries and
  necks, zero deeper than 2 mm, leaving out the routed net's own pours).
  Rust design: a `CostHook` implementation (A04's hook) that reads a Q05
  lattice stamped once per job from each listed pour's islands: cost falls
  linearly from the boundary to 0 at depth W (default 2 mm).
  - **Accept:** measured, with Q01.4's losses falling on at least N−1 boards
    and unrouted rising on none. Otherwise removed, with the numbers here.
  *Partial 2026-10-16:* not started: it needs Q05.2's lattice and A04's cost hook (Q05 is
  open). No code.

- [~] **Q01.7 — No foreign-net traces on power-type layers (D8).**
  Found by canary-23 grading the A13 canary-pwr SES (2026-10-16): both the Rust
  run (4336133) and Java `46bf9e2ef` route signal nets on `Gnd.Cu`, which the DSN
  declares `(type power)` with a GND plane. Rust: `/Holdup/LTC_PFI` (7.0 mm at
  y=67.33) and `/Supervisor/CAN_WAKE`; Java: the same CAN_WAKE path. KiCad's DRC
  does not flag it, but the track cuts the GND plane.
  **Fixed in Java as `7f56b062d`** ("Route only a power layer's own plane nets on it", after
  the pin; ledger row "Port: yes"; jar `~/code/freerouting/logs/jars/freerouting-7f56b062d.jar`).
  Cause: `expandToRoomDoors` returned early on an inactive layer only for signal layers. On
  9511d9a the search started from one existing CAN_WAKE via's target door on `Gnd.Cu`, found
  the other via in the same free room (the GND plane is no obstacle to foreign nets) and
  joined them there; the foreign-conduction-area drill check never ran because neither end
  was a new drill. Java's fix: `AutorouteControl.ownPlaneLayers[layer]` is true when the net
  has a conduction area on that power layer; otherwise `expandToRoomDoors` returns without
  expanding. Plane nets keep the old behaviour (`PlaneFanoutViaTest` unchanged). Java's
  numbers against `588320207`: foreign-net wires on power layers 1/1/5/2 → 0 on 9511d9a,
  9511d9a-in2-open-wirekeepout, Smoothieboard and CPU-85; unrouted 9511d9a 102 → 109, CPU-85
  58 → 57, 11 other plane boards unchanged. Test: `PowerLayerForeignNetTest` with
  `fixtures/PowerLayer-foreign-via-pair.dsn`.
  Rust design: port `7f56b062d` (the `own_plane_layers` control field and the
  `expand_to_room_doors` check) with its test and fixture, after A14's parity gate at the
  current pin, and regenerate the route references of the plane boards it changes (or
  re-pin past it).
  - **Accept:**
    - A fixture assertion: no trace of a net without a plane on that layer lies
      on a power-type layer, on canary-pwr 9511d9a and on every `fixtures/`
      board with a `(type power)` layer.
    - `PowerLayerForeignNetTest` ported and green; the route references of the boards the
      fix changes are regenerated at a Java commit that has it, and the rest are unchanged.
    - canary-23 grades a canary-pwr SES with no In1/In2 tracks; the
      unconnected count is recorded here (Java 41 and Rust 44 include the In1
      tracks).
  *Partial 2026-10-16:* `7f56b062d` ported as Java did it: `AutorouteControl::own_plane_layers`
  (set in `init_net` after `rebuild_via_info`) and the check in `expand_to_room_doors`
  (`autoroute/session/search.rs`), not through the B09.4 hook.
  - `PowerLayerForeignNetTest` ported as
    `autoroute_fixture_ports::power_layer_carries_no_foreign_net_trace` with Java's fixture,
    kept in `crates/freerouting/tests/data/PowerLayer-foreign-via-pair.dsn` (a DSN under
    `fixtures/` would need a Java load reference and DRC digest rows). Green; without the check
    it fails with `["SIG on Gnd.Cu"]`, as Java's did.
  - References: `run.sh route` at `7f56b062d` (a detached worktree compiled by ECJ through the
    J01 harness, `REFERENCE_WORK_DIR=target/orchestrator/q01/java-ref`, `-Xmx3g`) on the 21
    route references with a `(type power)` layer. 17 came out byte-identical; four changed and
    were replaced: `Issue145-smoothieboard` (5 foreign-net wires on `Route2` in the old session,
    0 now), `Issue753-CPU-85_r104` (2 on `In1.Cu`, 0 now; it now has 2 work-limit cut-offs),
    `canary-pwr-9511d9a-in2-open-wirekeepout` (1 on `Gnd.Cu`), `canary-pwr-pours-as-planes` (1
    on `Gnd.Cu`). Their `route` rows in `java_drc_digest.tsv` (8 rows changed; Java's DRC at
    `7f56b062d`, control `Issue269-z10_module` unchanged) and smoothieboard's `ses` dumps were
    regenerated with them. Recorded in `tools/java-reference/README.md` ("References at
    `7f56b062d`"), `fixtures/canary-pwr/README.md` and `docs/DIVERGENCES.md` ("Maze search").
    The frozen jar `freerouting-7f56b062d.jar` was not used directly: its manifest says
    `Build-Revision: 588320207` (built before the commit, from a tree that has the fix) and its
    version constant is `2.5.1-SNAPSHOT`, so the references were built from the commit itself.
  - A14 gate, release, worktree of `0ab934a` plus this diff, `PARITY_SUBDIRS=1` (new: the
    subdirectory references, so the canary-pwr boards are covered): 140 boards, exact 122 (120
    pass, 2 expected), cutoff 15 pass, java-failure 3 pass, 0 D8 breaches beyond Java's, 6515 s.
    The four regenerated boards are byte-identical to Java `7f56b062d`; the rest are unchanged.
  - Probe references: 76 `probe.sh` runs at `7f56b062d` (autoroute kinds `pass`, `insert`,
    `maze-log`, `insert-routed`, `maze-routed` on all their boards; every board-probe kind with a
    smoothieboard or CPU-85 reference). 59 identical below the header; 17 replaced (`pass`,
    `insert`, `maze-log` of `min_fr_test`: connection 2 now takes 68535 work units, 72428 before; 14 board
    probes of the two new sessions). Then 29 test targets green, 133 passed, 0 failed
    (`autoroute_pass`, all `autoroute_probe_*` and `board_probe_*`, `reference_load`,
    `settings_reference`, `ses_round_trip`, `quality_feasibility_java`, `fill_model`,
    `dsn_writer`, freerouting-dsn `java_style_ses`).
  - Fixture assertion: the gate's D8 check now reports a new trace on a power layer of a net
    without a plane there (`board_parity::d8_breaches`); no record of the 140 boards (every
    `fixtures/` board with a `(type power)` layer and a route reference, canary-pwr 9511d9a
    included) has one. canary-pwr 9511d9a with the A13 flags (`-mp 1 --gui.enabled=false
    --router.scoring.start_ripup_costs=1500 --router.min_trace_width_um=150`, release CLI): 109
    unrouted, 40 violations (all in the input), no track on Gnd.Cu or Power.Cu (the A13 session
    had `/Holdup/LTC_PFI` and `/Supervisor/CAN_WAKE` there).
  - Remains: canary-23's grade of that session,
    `target/orchestrator/q01/canary/canary-pwr-9511d9a.rust-q01.ses` (flags in
    `canary/README.txt`); Java `7f56b062d` pass 1 graded 45.
