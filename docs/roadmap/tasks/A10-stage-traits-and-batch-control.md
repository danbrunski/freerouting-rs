# A10 — Stage traits, registry and pure batch control

Opened 2026-10-15. Decision D9 makes the routing job a pipeline of interchangeable
stages. The A series defines the traits when it ports the batch pipeline, so Java
Freerouting's stages are their first implementations and the parity baseline, and the
KiCadRoutingTools ports (X series) plug in behind the same traits later. This checklist
defines the traits and the registry, and ports the decision logic of the pipeline that
needs no router: the pass controller's stop and restore rules, item selection and
ordering with the seeded shuffle, the ripup cost schedule, the fanout ordering and stop
rules, and the unrouted report. All of it is testable with synthetic inputs
([A00](A00-autorouter-README.md); pipeline report P7).

**Prerequisites:**
- C02 (the settings model; this checklist appends the stage keys to it) and C03.7
  (`BoardRouterSettings`).
- A08 (events, cancel token, `BoardHistory`).
- Decisions D2, D7, D9.

## What exists and what this adds

| Exists | State |
|---|---|
| `JavaRandom::shuffle` (Java `Collections.shuffle`) | Present, bit-exact |
| `BasicBoard::{connected_set, connectable_item_count, connectable_items}`, item kinds, `Net::contains_plane` | Present |
| `pipeline/AutorouteBatchLoop.java` (713 lines): pass loop, restore, stagnation, fanout recovery, last-mile, final best restore | Missing; the decisions are ported here, the driver in A13 |
| `pipeline/AutoroutePassRunner.java`: `reorderSingleThreadItems` (`:257-317`), anti-oscillation state (`:233-239`, `:319-323`) | Missing |
| `pipeline/BatchAutorouter.java`: constants (`:39-64`), `getAutorouteItems` (`:353-440`), `isPlaneItem` (`:442-453`) | Missing |
| `pipeline/BatchFanout.java`: component and pin order (`:693-833`), `retryFanout` (`:559-561`), pass-loop stop rules (`:109-160`) | Missing |
| `pipeline/RoutingPipeline.java`: algorithm normalisation (`:138-150`) | Missing |
| `pipeline/AutorouteUnroutedReport.java` (80), `LastMileBlockerRipup.corridor` and `isRemovableBlocker` (`:80-93`, `:65-78`) | Missing |

## Decisions made here

- **Traits** (`autoroute/stages.rs`). Every stage mutates the one `RoutingBoard` through
  its API only, gets its settings, work budget, cancel token and event sink through a
  `StageCtx`, and keeps no board reference across calls:

  ```rust
  pub struct StageCtx<'a> { pub settings: &'a RouterSettings,
      pub board_settings: &'a BoardRouterSettings, pub cancel: &'a CancelToken,
      pub events: &'a EventSink }
  pub trait FanoutStage { fn id(&self) -> &'static str;
      fn run(&mut self, board: &mut RoutingBoard, ctx: &mut StageCtx) -> FanoutSummary; }
  pub trait NetOrdering { fn id(&self) -> &'static str;
      fn select(&mut self, board: &RoutingBoard) -> Vec<RouteTask>;
      fn order(&mut self, tasks: Vec<RouteTask>, board: &RoutingBoard, pass: u32) -> Vec<RouteTask>;
      fn on_pass_end(&mut self, report: &PassReport); fn reset(&mut self); }
  pub trait ConnectionRouter { fn id(&self) -> &'static str;
      fn route(&mut self, board: &mut RoutingBoard, task: &RouteTask, p: &ConnectionParams,
               ctx: &mut StageCtx) -> ConnectionOutcome; }
  pub trait RipupPolicy { fn id(&self) -> &'static str;
      fn ripup_cost(&self, phase: Phase, pass: u32, start_ripup_costs: i32) -> i32;
      fn strict_check(&self, pass: u32, strict_drc: bool) -> bool;
      fn after_failure(&mut self, board: &mut RoutingBoard, task: &RouteTask, failures: u32); }
  pub trait PassController { fn id(&self) -> &'static str;
      fn before_pass(&mut self, pass: u32) -> PassStart;      // Run | Stop(StopReason)
      fn after_pass(&mut self, r: &PassReport) -> PassDecision; // see A10.3
      fn finish(&mut self, final_score: f32, router_stopped: bool) -> FinishDecision; }
  pub trait PlaneTapPolicy { fn id(&self) -> &'static str;
      fn destinations(&self, board: &RoutingBoard, task: &RouteTask, unconnected: &BTreeSet<ItemId>)
          -> Destinations; }                               // { primary, fallback: Option<_> }
  pub trait NeckDownPolicy { fn id(&self) -> &'static str;
      fn retry(&mut self, board: &mut RoutingBoard, task: &RouteTask, failed: &ConnectionOutcome,
               p: &ConnectionParams, ctx: &mut StageCtx) -> Option<ConnectionOutcome>; }
  pub trait BoardStage { fn id(&self) -> &'static str; fn phase(&self) -> StagePhase;
      fn run(&mut self, board: &mut RoutingBoard, ctx: &mut StageCtx) -> StageReport; }
  pub trait Optimizer { fn id(&self) -> &'static str;
      fn run(&mut self, board: &mut RoutingBoard, ctx: &mut StageCtx) -> OptimizerSummary; }
  ```

  - `RouteTask { item: ItemId, net: i32, is_plane: bool }`.
  - `ConnectionParams { pass, ripup_allowed, ripup_cost, via_cost, trace_costs,
    remove_unconnected_vias, work_budget: u64 }`. The stage starts a B02.7 `WorkLimit`
    on the board's own meter (the meter lives in the board's `SearchTreeManager`, so a
    stage never holds one).
  - `ConnectionOutcome { state, details, ripped: BTreeSet<ItemId>, ripped_costs:
    Vec<(ItemId, i32)>, first_new_id: i32, failure: Option<FailureRecord>,
    work_limit_hit: bool, work: WorkStats }`. `failure` is A04's failure record;
    `work_limit_hit` is what Q06 reads.
  - `StagePhase { BeforeFanout, AfterFanout, AfterPass, AfterRouting, AfterOptimizer }`
    hosts the special stages: diagnostics, plane repair, diff pairs, length matching.
  - Plane taps and neck-down are policies inside connection routing, because that is where
    Java applies them (`AutorouteConnectionRouter.java:59-77, 136-162, 193-273`).
- **Dispatch is dynamic between stages, static inside them.** One `Box<dyn …>` call per
  connection costs nothing measurable. The maze search inside the Freerouting connection
  router stays monomorphized over A04's sinks.
- **Registry and selection.** `StageRegistry` holds one `BTreeMap<&'static str, fn() ->
  Box<dyn Trait>>` per trait (sorted iteration, never hash order). `StageSelection` comes
  from settings:
  - `router.autorouter.algorithm` (Java key, default `freerouting-router`) selects a
    bundle: ordering, connection router, ripup policy, pass controller, plane taps and
    neck-down. As in Java (`RoutingPipeline.java:138-150`), any other value, including the
    legacy `freerouting-router-v19`, logs Java's warning and becomes the default.
  - `router.optimizer.algorithm` (default `freerouting-optimizer`) selects the optimizer.
    O06 registers it.
  - New per-stage keys override one stage: `router.fanout.algorithm`,
    `router.autorouter.ordering`, `router.autorouter.connection_router`,
    `router.autorouter.ripup_policy`, `router.autorouter.pass_controller`,
    `router.special.plane_taps`, `router.special.neck_down`, and the list
    `router.special.stages`. An unknown id in these keys is a settings error (A00 open
    question 1).
  - The resolved selection is logged and included in A08's `RunSummary`. It is not
    written into the SES, which C05 keeps byte-identical to Java's.
- **The Freerouting implementations** are registered under these ids:
  `freerouting-ordering` (A10.4), `freerouting-ripup` (A10.5), `freerouting-passes`
  (A10.3), `freerouting-connection` (A11.1), `freerouting-plane-taps` and
  `freerouting-neck-down` (A11.2), `freerouting-fanout` (A12.3),
  `freerouting-unrouted-report` (A10.7, a `BoardStage` at `AfterRouting`).
- **f32 scores.** Every score comparison uses `f32` as Java does, with the
  `STAGNATION_SCORE_THRESHOLD = 0.5f` added in `f32` (`BatchAutorouter.java:64`).

## File ownership

| Item | New files | May edit |
|---|---|---|
| A10.1 | `autoroute/stages.rs` | `autoroute/mod.rs` (append) |
| A10.2 | `crates/freerouting/tests/autoroute_batch_control.rs` | `autoroute/stages.rs`; `settings/fields.rs` (C02.1's field table: append the stage keys only, as C02 expects) |
| A10.3 | `autoroute/batch/mod.rs`, `autoroute/batch/control.rs` | — |
| A10.4 | `autoroute/batch/ordering.rs` | `AutorouteProbe.java`, `fixtures.tsv` (kind `items`) |
| A10.5 | `autoroute/batch/ripup_policy.rs` | — |
| A10.6 | `autoroute/batch/fanout_rules.rs` | — |
| A10.7 | `autoroute/batch/report.rs` | — |

## Checklist

- [ ] **A10.1 — Traits and types.** `stages.rs` as in the decisions, with doc comments that
  state each trait's contract: what it may change on the board, what it must reset on
  `reset()` (state keyed by `ItemId` or net, because a restore brings back old ids), and
  that budgets are work, never time.
  - **Accept:** the crate builds; a doc test shows a minimal custom `NetOrdering`.

- [ ] **A10.2 — Registry and selection.** `StageRegistry::builtin()`,
  `StageSelection::from_settings`, `StageRegistry::build(&selection) -> Result<Pipeline,
  UnknownStage>`. Append the per-stage keys to C02's settings as `Option<String>` (and a
  list for `router.special.stages`) with no default other than "use the bundle".
  - **Accept:**
    - port `autoroute/pipeline/RoutingPipelineAlgorithmNormalizationTest`
      (`custom-unsupported-router` and `freerouting-router-v19` become
      `freerouting-router`; the canonical value is unchanged);
    - an unknown per-stage id is an error naming the key and the known ids;
    - a test-only `reverse-ordering` registered in the test changes the task order and is
      named in the resolved selection.

- [ ] **A10.3 — `FreeroutingPassController`.** The decision logic of
  `AutorouteBatchLoop.run` (`:313-645`), with no board inside: the driver (A13) feeds it a
  `PassReport { pass, continue_routing, score: f32, incomplete, router_stop_requested,
  timed_out, history_size, history_max_score }`. Rules, all `// PARITY:`:
  - before a pass: a timed-out job asks for a router stop; `max_passes > 0 && pass >
    max_passes` stops (`:314-336`);
  - restore check (`:357-405`): when `(history_size >= 8 || stop_requested)` and
    `((pass % 4 == 0 && pass >= 8) || stop_requested)` and `history_max_score > score`
    (strict): `Restore { max_restore_count: 3 }`; the driver reports back whether a board
    came back, and none means stop; on a restore, zero the pass-local counter and set
    `last_best = restored score`;
  - when `pass >= 8 && continue_routing` (`:482-590`): pass-local counter (`score >
    last_best + 0.5` resets it, else increment); one-time fanout recovery when fanout is on,
    there are incompletes and the counter reached 3; last-mile ripup when `0 < incomplete
    <= 8`, fewer than 2 attempts so far and the counter reached 3; stop when the counter
    reaches 10; the global tracker (first recorded at pass 8: `score > global_best + 0.5`
    records the pass, else stop when `pass - pass_of_best >= 10`);
  - otherwise, `incomplete == 0 && score > 0.5` resets the pass-local counter
    (`:591-599`);
  - finish (`:607-642`): restore the best board when `history_max_score > final_score`;
    run the final `remove_tails(NONE)` only when the router was not stopped.
  - `PassDecision` is `Continue | Restore { max_restore_count } | FanoutRecovery |
    LastMileRipup | Stop(StopReason)`, applied by the driver in Java's order.
  - **Accept:** table-driven tests over synthetic score sequences, each expected decision
    derived from the Java code with a line reference in the test: a flat score stops at
    pass 18 (the global tracker's earliest stop); a score that drops at pass 8 restores;
    equal scores never restore; fanout recovery fires once; last-mile fires at most twice;
    `max_passes = 2` stops before pass 3; an exhausted history stops.

- [ ] **A10.4 — `FreeroutingOrdering`.**
  - `select` = `BatchAutorouter.getAutorouteItems` (`:353-440`): walk the item list
    (descending id); connectable items that are not routable (pins, conduction areas,
    fixed traces and vias); skip items already handled (members of an earlier item's
    connected set with at most one net); per net, the item needs routing when its connected
    set is smaller than `connectable_item_count(net)` and it has no ignored nets; a plane
    net whose connected set holds a conduction area does not count; items with a plane net
    needing routing go to the plane list, the rest to the signal list; plane items first.
  - `order` for pass > 1 = `reorderSingleThreadItems` (`:257-317`): split by
    `is_plane_item` (**any** net contains a plane, `BatchAutorouter.java:442-453`, which is
    not the same test as `select`'s, `// PARITY:`); plane items keep their order. Signal
    items: with `stagnation_count >= 2`, sort by `(first net or 0, id ascending)` and
    shuffle with `JavaRandom::new(pass as i64 * 1000003 + max_net_number as i64)`;
    otherwise, if the last pass left incomplete nets, items on those nets first, each
    group sorted; otherwise sort.
  - `on_pass_end`: `stagnation_count` increments when `incomplete >= previous` (with a
    previous count set), else resets; store the incomplete nets (a `BTreeSet<i32>`);
    `reset` clears all three (`resetAntiOscillationState`, `:319-323`).
  - **Accept:**
    - probe kind `items`: `getAutorouteItems` ids, then the pass-2 and pass-3 orders with
      `stagnationCount = 2` forced, on `Issue026-J2_reference`, `Issue508-DAC2020_bm01`
      and `Issue269-z10_module` (a plane board). Rust equal.
    - a unit test of the shuffle against values Java prints for seed
      `2 * 1000003 + 40`.

- [ ] **A10.5 — `FreeroutingRipupPolicy`.** `ripup_cost`: autoroute `start * pass`
  (`AutorouteConnectionRouter.java:46`); fanout `start * (pass + 1)` with pass counted
  from 0, or -1 when fanout ripup is off (`BatchFanout.fanoutPass`); the optimizer's
  factors belong to O06. `strict_check(pass, strict_drc) = strict_drc || pass >= 3`
  (`AutorouteConnectionRouter.java:276`). `after_failure`: with two or more failures of
  the item, remove every trace and via of that net that is not user-fixed and has one net,
  in `connectable_items` order, with one `remove_items` call
  (`AutoroutePassRunner.java:163-180`).
  - **Accept:** unit tests for each rule; `after_failure` on a fixture removes exactly the
    expected ids.

- [ ] **A10.6 — Fanout ordering and stop rules.** Pure helpers for A12:
  - components (`BatchFanout.Component`, `:693-777`): SMD pins with a net and no ignored
    nets; the centre of gravity of the component's SMD pins summed in pin-list order;
    order by SMD pin count descending, then component id;
  - pins (`:779-831`): `outer_first` (default), `inner_first`, `distanceToClosestOnNet`,
    `surroundingsDensity` (within 20 mm), with pin index as the tie-break; any other name
    sorts by pin index only (A00 open question 7);
  - `retry_fanout(failed_generation: Option<i32>, current: i32)` (`:559-561`);
  - the pass-loop stop rules (`:117-160`): a pass that routed 0 pins; `(routed << 32) ^
    via_count` identical for 3 consecutive passes; an unchanged board hash (B01's
    `state_hash`); `fanout.max_items`; the `fanout.timeout` deadline as a stop only.
  - **Accept:** port `autoroute/pipeline/FanoutRetryTest`; table tests for the stop rules;
    a pin-order probe kind `fanout-order` on `Issue558-dev-board` equal.

- [ ] **A10.7 — Reports and small checks.**
  - `AutorouteUnroutedReport.build` as text, and the diagnostics `BoardStage` that logs it.
  - `LastMileBlockerRipup.corridor` (`max(span / 4, 1)` margin) and `is_removable_blocker`
    (traces and vias only, not deletion-forbidden, not user-fixed, not on the airline's
    net, at least one net).
  - The routable-layer check: no layer that is both signal and active is an error
    (`RoutableLayersSafetyCheckTest`).
  - **Accept:** port `autoroute/pipeline/FanoutSliceAndLastMileTest` (margin 100 for
    `(0,0)–(400,100)`) and `autoroute/RoutableLayersSafetyCheckTest`; a golden test of the
    report on `PCBench-retroreflectors-CONGFLOCK` with its load-time airlines.
