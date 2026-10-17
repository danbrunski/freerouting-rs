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

- [ ] **Q01.1 — Settings key and exemption table.**
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

- [ ] **Q01.2 — The obstacle policy and "off means identical".**
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

- [ ] **Q01.3 — Routing fixtures.**
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

- [ ] **Q01.4 — Pour-cut report (report only).**
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

- [ ] **Q01.5 — Measurement.**
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

- [ ] **Q01.6 — Narrowness cost (optional experiment, after Q05.2).**
  Source: KRT `plane_fragility.py` (a cost ramp, steep at pour boundaries and
  necks, zero deeper than 2 mm, leaving out the routed net's own pours).
  Rust design: a `CostHook` implementation (A04's hook) that reads a Q05
  lattice stamped once per job from each listed pour's islands: cost falls
  linearly from the boundary to 0 at depth W (default 2 mm).
  - **Accept:** measured, with Q01.4's losses falling on at least N−1 boards
    and unrouted rising on none. Otherwise removed, with the numbers here.

- [ ] **Q01.7 — No foreign-net traces on power-type layers (D8).**
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
