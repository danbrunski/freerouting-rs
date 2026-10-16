# A12 — Fanout

Opened 2026-10-15. Fanout is on by default in Java (`DefaultSettings.java:182`), so
every board with SMD pins runs it before the first routing pass: each SMD pin gets a short
escape trace and a via, component by component, for up to 20 passes. It is also where
the owner rule "plane-net pads get their own via to the plane" (D8) acts first. This
checklist ports `RoutingBoard.fanout` (one pin) and `BatchFanout` (the passes) as the
Freerouting fanout stage, and compares the fanout pass pin by pin with Java
([A00](A00-autorouter-README.md); pipeline report P8 item 4).

**Prerequisites:**
- A11 (strict DRC, the connection-router pieces), A10.6 (component and pin order, stop
  rules), A06 (`route_connection`), A01 (control and its fallback via rule).
- O05 (`opt_changed_area` with trace costs after each fanned-out pin).
- B01 (`state_hash` for the unchanged-board stop).
- J01 (the re-pin to `30baad7e2`, where the per-pin limit is a work count) for A12.6.
- Decisions D2, D8, D9.

## What exists and what this adds

| Exists | State |
|---|---|
| `scoring::BoardStatistics` fanout fields (`escaped_count`, `pins_to_escape`, `total_smd_pins`; `is_pin_escaped`) | Present (`scoring/board_statistics.rs`) |
| `BasicBoard::{smd_pins, vias, unconnected_set, connected_set, power_plane_areas}` | Present |
| `board/facade/RoutingBoard.java` `fanout` (`:990-1145`) | Missing |
| `autoroute/pipeline/BatchFanout.java` (835 lines): `fanoutBoard` (`:89-172`), `fanoutPass` (`:174-480`) | Missing |
| A10.6's ordering and stop-rule helpers | Prerequisite |

What this adds: `autoroute/fanout_pin.rs` (one pin), `autoroute/batch/fanout.rs`
(`FreeroutingFanout`, the `FanoutStage` implementation), and the `fanout` probe.

## Decisions made here

- **One session for all attempts of a pin.** Java initialises one engine and calls
  `autorouteConnection` up to three times on it (plane, closest target, all targets),
  sharing one ripped set (`RoutingBoard.java:1074-1126`). Each call clears its rooms and
  resets the room counter, but the ripped set accumulates, so a later attempt rips what an
  earlier failed one collected. Kept, `// PARITY(quirk):`.
- **The via-rule fallback is a merged rule.** With `fanout.fallback_to_board_vias` (on by
  default), the net class's rule is extended with every via of `viaRules.firstElement()`
  that it lacks, in order, and `rebuild_via_info` runs again (`:1040-1060`). A01.3's
  accumulating radii apply.
- **Generations live in `BTreeMap`s.** Java's `failedPinGeneration` and
  `componentGeneration` are `HashMap`s used only for lookups; sorted maps change nothing and
  remove the question.
- **The per-pin budget** is a `WorkLimit` of `max_milliseconds_per_pin * (pass + 1)`
  nominal ms (default 10,000) times 64,000 units, created before `fanout_pin`, as at
  `30baad7e2` (`BatchFanout.fanoutPass`). `fanout.timeout` stays a
  stop-only deadline (D2).

## File ownership

| Item | New files | May edit |
|---|---|---|
| A12.1 | `autoroute/fanout_pin.rs` | `autoroute/mod.rs` (append) |
| A12.2, A12.3 | `autoroute/batch/fanout.rs` | `autoroute/batch/mod.rs`, `autoroute/stages.rs` (registry entry) |
| A12.4, A12.5 | `crates/freerouting/tests/autoroute_fanout.rs` | — |
| A12.6 | `fixtures/reference/probe/fanout/` | `AutorouteProbe.java`, `fixtures.tsv` |

## Checklist

- [ ] **A12.1 — `fanout_pin`.** Port `RoutingBoard.fanout` (`:990-1145`):
  - guards: a pin spanning layers or with more than one net → `AlreadyConnected`; ignored
    nets → `Skipped`; any connected-set item off the pin's layer → `AlreadyConnected`; no
    unconnected items → `NoUnconnectedNets`;
  - targets: the unconnected set copied to a `Vec` (descending id) and **stably** sorted by
    squared distance from the pin centre to each item's box centre, computed as
    `(ll.x + ur.x) / 2.0` (integer sum, then a `f64` divide), compared with
    `f64::total_cmp` (`:1020-1037`);
  - the control: `for_net_default`, `is_fanout = true`, the via-rule fallback, the start
    pin centre and layer, `remove_unconnected_vias = false`, ripup allowed with the given
    cost when it is not negative;
  - attempts: plane targets first (plane via costs, `is_plane_fanout`, then the normal via
    cost again); then, with at most 4 targets, the closest target alone and, if that
    failed and there are more, all targets; with more than 4, all targets at once;
  - `Routed` → `opt_changed_area` (O01/O05) limited to the pin's net, with the pull-tight
    accuracy, the trace costs, and a limit of 1,000 nominal ms.
  - **Accept:** covered by A12.6; a unit test of the stable distance sort with two equal
    distances.

- [ ] **A12.2 — The fanout pass.** Port `fanoutPass` (`:174-480`) per component and pin in
  A10.6's order: stop at `fanout.max_items`; skip pins with ignored nets; skip pins whose net
  class has no vias unless the fallback is allowed and the first board via rule has vias;
  skip pins that already failed in the component's current generation (`retry_fanout`);
  `start_marking_changed_area`; `fanout_pin` with ripup cost `start * (pass + 1)` or -1;
  on `Routed`, `enforce_strict_drc` **always** (A11.3); count routed, failed, insert errors
  and already-connected; a success increments the component's generation; a failure records
  the pin's generation; stop on the deadline or a router stop.
  - **Accept:** covered by A12.6.

- [ ] **A12.3 — `FreeroutingFanout` (the stage).** Port `fanoutBoard` (`:89-172`): up to
  `fanout.max_passes` (default 20) passes with A10.6's stop rules; return a `FanoutSummary
  { passes, escaped, total, timed_out }` from `BoardStatistics`; `Progress` events per pin
  and per pass (A08). Register it as `freerouting-fanout`.
  - **Accept:** covered by A12.4–A12.6.

- [ ] **A12.4 — D8 plane vias.** Port `fixtures/PlaneFanoutViaTest` on
  `Issue269-min_fr_test/min_fr_test.dsn`, with the harness settings of the Java test
  (`RoutingFixtureTest`, `TestingSettings`: copper-to-edge clearance 0, the optimizer off):
  exactly 2 GND SMD pads, each connected to a conduction area; the pads not chained to each
  other; exactly 2 GND vias; no via overlapping an SMD pad on its layer; 0 traces on plane
  layers. Also add a synthetic case from the KRT report: a floating same-net copper island
  beside two plane pins must not count as connecting either pin, and the incomplete count
  equals the DRC's. Compare that case with Java (probe kind `plane-island`); if Java connects
  through the island, record it in the test and hand the rule to Q01.
  - **Accept:** `plane_fanout_via` green; `plane_island` matches Java, with the done note
    saying which way Java went.

- [ ] **A12.5 — Ported fanout tests.** As work-bounded Rust tests with `TestingSettings`
  semantics (edge clearance 0, optimizer off):
  - `fixtures/SmdPinFanoutRoutingTest`: `Issue558-dev-board.dsn` 0 unrouted after 10
    passes; fanout-only mode stops at pass 0 with vias > 0; `Issue508-DAC2020_bm06.dsn`
    at most 8 unrouted; `Issue508-SMD-routing-issue-demo.dsn` at most 2; the bm10 case
    `#[ignore]` (slow in Java too);
  - `fixtures/Issue952FanoutIgnoredNetClassTest` cases 1–4 (case 4 without the optimizer):
    nets `+3V3` and `+5V` of class `Power,Default` get no traces or vias;
  - `fixtures/Dac2020Bm11FanoutTraceTest`: fanout-only on `Issue730-DAC2020_bm11.dsn`,
    `escaped_count >= 154`;
  - footprint frames (KRT report): fan out one part of `Issue558-dev-board` as placed,
    rotated 90° and mirrored to the back. Record Java's and Rust's escape counts for each;
    Rust must equal Java per copy. Whether the counts agree across copies is reported, not
    required; a difference goes to the Q series as a finding.
  - **Accept:** all green except the listed `#[ignore]`; numbers in the done note. These
    tests run after A13 wires the stage into the pipeline; until then they call
    `FreeroutingFanout` directly.

- [ ] **A12.6 — `fanout` probe and the work-limit determinism test.**
  - Java kind `fanout`: load as J01's route mode does and run `BatchFanout.fanoutBoard`
    for one pass (`fanout.max_passes = 1`). The probe replicates `fanoutPass`'s pin loop
    around the real `RoutingBoard.fanout`, as A11.7 does for passes, and checks itself
    against a real one-pass run (`replica_matches_real`). Per pin: component id, pin id,
    skip reason or state, `maxGeneratedId` before and after, the digest of added items,
    whether strict DRC rejected it, and the `WorkMeter` units the pin used.
  - Port `fixtures/WorkLimitDeterminismTest` (`30baad7e2`, 94 lines) as
    `work_limit_determinism`: `Issue508-DAC2020_bm01.dsn` with the optimizer off,
    `max_items = 1`, `fanout.max_passes = 1`, `max_passes = 1`.
    - `cut_off_fanout_is_the_same_when_busy`: with `max_milliseconds_per_pin = 1` (so the
      limit fires), a run on an idle machine and a run beside one busy thread per core give
      the same `state_hash` (B01.5).
    - `tiny_budget_really_cuts_off`: `max_milliseconds_per_pin` 1 and 10,000 give different
      boards.
  - **Accept:**
    - `fanout_matches_java` equal, with `replica_matches_real` true, on
      `Issue508-SMD-routing-issue-demo`, `Issue269-min_fr_test/min_fr_test`,
      `Issue558-dev-board` and `Issue508-DAC2020_bm08`;
    - `work_limit_determinism` (both tests) green; and the board after the 1 ms run
      equals Java's (`fanout` probe kind with that setting on bm01) once B02 has proven the
      tree shape, since a limit fires there (`[~]` until then);
    - `tools/determinism-check.sh` on a fanout-only run of the four boards reports
      `identical`.
