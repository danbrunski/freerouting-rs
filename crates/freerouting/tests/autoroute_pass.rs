//! The batch passes' connection routing (A11): the Freerouting connection
//! router and its policies, the strict DRC, the pass runner and the
//! optimizer's entry point.
//!
//! `pass_matches_java` (A11.7) is rung 2 of the parity ladder for whole
//! passes: passes 1 and 2 on every board of the `pass` row of
//! `tools/java-reference/probes/autoroute/fixtures.tsv`
//! (`tools/bench/sets/connection-parity.txt` and `Issue026-J2_reference`)
//! give the same lines as `AutorouteProbe.java` kind `pass`, connection by
//! connection, work units included (see `tests/support/pass_lines.rs`).
//!
//! Java tests ported here (they belong in `tests/java_ports.rs`, which
//! carries another session's uncommitted hunks): `autoroute.StrictDrcEnforcementTest`
//! (both tests) and the routing half of `fixtures.PlaneFanoutViaTest` (pass 1
//! without the fanout, which A12 adds). Not ported: `BatchAutorouterDebugTest`
//! (the GUI debugger's `DebugControl`, which the port does not have),
//! `RoutableLayersSafetyCheckTest` (A10.7 ported it),
//! `BatchOptimizerTraceMetricTest`, `BatchOptimizerDeterminismTest` (O06),
//! `NamedAlgorithmSealingTest` (Java class sealing) and
//! `Issue684MemoryLeakRoutingTest` (a heap measurement).

mod autoroute_common;
#[path = "support/insert_lines.rs"]
mod insert_lines;
#[path = "support/pass_lines.rs"]
mod pass_lines;

use std::cell::Cell;
use std::collections::BTreeSet;
use std::rc::Rc;

use autoroute_common::{Loaded, load};
use freerouting::autoroute::AttemptState;
use freerouting::autoroute::batch::connection::{
    FreeroutingConnectionRouter, FreeroutingNeckDown, FreeroutingPlaneTaps, clamp_half_widths,
    connection_params, narrower_somewhere, neck_half_width,
};
use freerouting::autoroute::batch::ordering::FreeroutingOrdering;
use freerouting::autoroute::batch::pass::{PassRunner, PassStages, route_passes_for_optimizer};
use freerouting::autoroute::batch::ripup_policy::FreeroutingRipupPolicy;
use freerouting::autoroute::batch::strict_drc::{apply_strict_drc_after_route, enforce_strict_drc};
use freerouting::autoroute::control::AutorouteControl;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::stages::{
    ConnectionOutcome, ConnectionParams, ConnectionRouter, Destinations, NeckDownAttempt,
    NeckDownPolicy, PlaneTapPolicy, RouteTask, StageCtx,
};
use freerouting::board::RoutingBoard;
use freerouting::board::items::{ItemId, StopConnectionOption};
use freerouting::board::structure::FixedState;
use freerouting::datastructures::IdGenerator;
use freerouting::scoring::BoardStatistics;
use freerouting::settings::RouterSettings;
use freerouting::settings::board::BoardRouterSettings;

const MIN_FR_TEST: &str = "fixtures/Issue269-min_fr_test/min_fr_test.dsn";
const STRICT_FIXTURE: &str =
    "fixtures/Issue575-drc_BBD_Mars-64_6_track_1_hole_clearance_violations.dsn";

/// A loaded board with its settings changed by `edit` (the board settings
/// rebuilt from them).
fn load_with(dsn: &str, edit: impl FnOnce(&mut RouterSettings)) -> Loaded {
    let mut loaded = load(dsn);
    edit(&mut loaded.settings);
    loaded.router = BoardRouterSettings::from_settings(&loaded.settings);
    loaded
}

/// Runs `f` with a stage context over `loaded`'s settings.
fn with_ctx<T>(
    loaded: &mut Loaded,
    cancel: &CancelToken,
    f: impl FnOnce(&mut RoutingBoard, &mut StageCtx) -> T,
) -> T {
    let events = NullEvents;
    let mut ctx = StageCtx {
        settings: &loaded.settings,
        board_settings: &loaded.router,
        cancel,
        events: &events,
    };
    f(&mut loaded.board, &mut ctx)
}

/// Runs passes `1..=passes` of a job's pass runner with the Freerouting
/// stages; returns the outcomes.
fn run_passes(
    loaded: &mut Loaded,
    passes: u32,
) -> Vec<freerouting::autoroute::batch::pass::PassOutcome> {
    let mut runner = PassRunner::for_job(&loaded.settings, &loaded.router);
    let mut ordering = FreeroutingOrdering::new();
    let mut router = FreeroutingConnectionRouter::freerouting();
    let mut ripup = FreeroutingRipupPolicy::from_settings(&loaded.settings);
    let cancel = CancelToken::new();
    with_ctx(loaded, &cancel, |board, ctx| {
        let mut stages = PassStages {
            ordering: &mut ordering,
            router: &mut router,
            ripup: &mut ripup,
        };
        (1..=passes)
            .map(|pass| runner.run_pass(board, pass, &mut stages, ctx))
            .collect()
    })
}

fn net_number(board: &RoutingBoard, name: &str) -> i32 {
    board
        .rules
        .nets
        .get_by_name(name, 1)
        .expect("the net")
        .net_number
}

fn smd_pins_of(board: &RoutingBoard, net: i32) -> Vec<ItemId> {
    board
        .items()
        .filter(|i| i.is_pin() && i.contains_net(net) && i.first_layer() == i.last_layer())
        .map(|i| i.id())
        .collect()
}

/// Items reached from `start` through normal contacts, not through a
/// conduction area (Java `copperReachableWithoutPlane`).
fn reachable_without_plane(board: &RoutingBoard, start: ItemId) -> BTreeSet<ItemId> {
    let mut reached = BTreeSet::new();
    let mut queue = vec![start];
    while let Some(id) = queue.pop() {
        let Some(item) = board.item(id) else {
            continue;
        };
        if item.is_conduction_area() || !reached.insert(id) {
            continue;
        }
        queue.extend(board.normal_contacts(id));
    }
    reached
}

// ---------------------------------------------------------------------------
// A11.2: plane taps (the routing half of PlaneFanoutViaTest)
// ---------------------------------------------------------------------------

#[test]
fn pass_one_gives_every_gnd_pad_its_own_via_to_the_plane() {
    let mut loaded = load(MIN_FR_TEST);
    let outcomes = run_passes(&mut loaded, 1);
    assert!(outcomes[0].routed > 0, "{:?}", outcomes[0]);
    let board = &loaded.board;
    let gnd = net_number(board, "GND");
    let pads = smd_pins_of(board, gnd);
    assert_eq!(pads.len(), 2, "the fixture has two GND SMD pads");
    for &pad in &pads {
        let connected = board.connected_set(pad, gnd, false);
        assert!(
            connected
                .iter()
                .any(|id| board.item(*id).is_some_and(|i| i.is_conduction_area())),
            "{pad:?} must be connected to the GND plane"
        );
        let reached = reachable_without_plane(board, pad);
        for &other in &pads {
            assert!(
                other == pad || !reached.contains(&other),
                "{pad:?} must not be chained to {other:?} by traces"
            );
        }
    }
    let gnd_vias = board
        .items()
        .filter(|i| i.is_via() && i.contains_net(gnd))
        .count();
    assert_eq!(gnd_vias, pads.len(), "every GND pad needs its own via");
    for via in board.items().filter(|i| i.is_via()) {
        for pin in board
            .items()
            .filter(|i| i.is_pin() && i.first_layer() == i.last_layer())
        {
            let pad = board.tile_shape(pin, 0).expect("pad shape");
            for k in 0..board.tile_shape_count(via) {
                let layer = via.drill().expect("drill").shape_layer(k);
                if layer != pin.first_layer() {
                    continue;
                }
                let shape = board.tile_shape(via, k).expect("via shape");
                assert!(
                    !shape.intersects_tile(&pad),
                    "via {:?} sits on pad {:?}",
                    via.id(),
                    pin.id()
                );
            }
        }
    }
    let plane_layer_traces = board
        .items()
        .filter_map(|i| i.as_trace())
        .filter(|t| !board.layer_structure.layers[t.layer as usize].is_signal)
        .count();
    assert_eq!(plane_layer_traces, 0, "no trace on the plane layer");
}

// ---------------------------------------------------------------------------
// A11.2: the neck-down width
// ---------------------------------------------------------------------------

#[test]
fn neck_down_clamps_every_half_width_and_keeps_the_compensation() {
    let loaded = load(MIN_FR_TEST);
    let board = &loaded.board;
    let net = net_number(board, "GND");
    let mut control = AutorouteControl::for_net_default(board, net, &loaded.router).unwrap();
    // The fixture's 200 um traces at resolution 10: half width 1000.
    assert!(control.trace_half_width.iter().all(|w| *w == 1000));
    control.compensated_trace_half_width = control.trace_half_width.iter().map(|w| w + 7).collect();
    control.trace_half_width[1] = 300;
    control.compensated_trace_half_width[1] = 310;
    clamp_half_widths(&mut control, 500);
    assert_eq!(control.trace_half_width, vec![500, 300, 500, 500]);
    assert_eq!(
        control.compensated_trace_half_width,
        vec![507, 310, 507, 507]
    );

    // 100 um at resolution 10 is 1000 units, half 500; at least 1, and the
    // `router.min_trace_width_um` floor (46bf9e2ef) raises it.
    assert_eq!(neck_half_width(board, 100.0, 0), 500);
    assert_eq!(neck_half_width(board, 0.01, 0), 1);
    assert_eq!(neck_half_width(board, 100.0, 750), 750);
    // Java rounds before halving: 0.15 um is 1.5 units, rounded to 2, half 1.
    assert_eq!(neck_half_width(board, 0.15, 0), 1);
    assert_eq!(neck_half_width(board, 0.35, 0), 2);

    let wide = AutorouteControl::for_net_default(board, net, &loaded.router).unwrap();
    assert!(narrower_somewhere(&wide, 999));
    assert!(!narrower_somewhere(&wide, 1000));
    let mut inactive = wide.clone();
    inactive.layer_active.iter_mut().for_each(|a| *a = false);
    assert!(!narrower_somewhere(&inactive, 1));
}

// ---------------------------------------------------------------------------
// A11.1: the fallback continues the connection's work limit
// ---------------------------------------------------------------------------

/// Plane taps that aim at the planes and fall back to them again,
/// recording the meter when asked.
struct PlaneTwice {
    fallback: bool,
    meter_at_call: Rc<Cell<u64>>,
}

impl PlaneTapPolicy for PlaneTwice {
    fn id(&self) -> &'static str {
        "test-plane-twice"
    }
    fn destinations(
        &self,
        board: &RoutingBoard,
        task: &RouteTask,
        unconnected: &BTreeSet<ItemId>,
    ) -> Destinations {
        self.meter_at_call
            .set(board.search_trees().work_meter().units());
        let primary = FreeroutingPlaneTaps
            .destinations(board, task, unconnected)
            .primary;
        Destinations {
            fallback: self.fallback.then(|| primary.clone()),
            primary,
        }
    }
}

/// A neck-down that records what the failed attempt handed it.
struct Recorder {
    seen: Rc<Cell<Option<(u64, u64, u64)>>>,
}

impl NeckDownPolicy for Recorder {
    fn id(&self) -> &'static str {
        "test-recorder"
    }
    fn retry(
        &mut self,
        board: &mut RoutingBoard,
        _task: &RouteTask,
        attempt: NeckDownAttempt<'_>,
        _failed: &ConnectionOutcome,
        _p: &ConnectionParams,
        _ctx: &mut StageCtx,
    ) -> Option<ConnectionOutcome> {
        let meter = board.search_trees().work_meter();
        self.seen.set(Some((
            attempt.work_limit.units_done(meter),
            attempt.work_limit.max_units(),
            meter.units(),
        )));
        None
    }
}

/// Routes the first GND pad of min_fr_test with no budget; returns the
/// limit's units, its budget, the meter units since the plane-tap call and
/// the outcome.
fn route_gnd_without_vias(fallback: bool) -> (u64, u64, u64, ConnectionOutcome, u64) {
    let mut loaded = load(MIN_FR_TEST);
    let gnd = net_number(&loaded.board, "GND");
    let pad = smd_pins_of(&loaded.board, gnd)[0];
    let meter_at_call = Rc::new(Cell::new(0));
    let seen = Rc::new(Cell::new(None));
    let mut router = FreeroutingConnectionRouter::with_policies(
        Box::new(PlaneTwice {
            fallback,
            meter_at_call: meter_at_call.clone(),
        }),
        Box::new(Recorder { seen: seen.clone() }),
    );
    let task = RouteTask {
        item: pad,
        net: gnd,
        is_plane: true,
    };
    let cancel = CancelToken::new();
    let trace_costs = loaded.router.trace_costs().to_vec();
    let (outcome, budget) = with_ctx(&mut loaded, &cancel, |board, ctx| {
        let mut p = connection_params(board, &task, 1, ctx, 100, &trace_costs, false, 500, false);
        assert_eq!(p.work_budget, 100_000 * 64_000, "pass 1's budget");
        // A budget of 0 makes both attempts fail at their first limit check.
        p.work_budget = 0;
        let budget = p.work_budget;
        (router.route(board, &task, &p, ctx), budget)
    });
    let (units_done, max_units, meter_now) = seen
        .get()
        .unwrap_or_else(|| panic!("the neck-down was not asked: {outcome:?}"));
    (
        units_done,
        max_units,
        meter_now - meter_at_call.get(),
        outcome,
        budget,
    )
}

#[test]
fn the_plane_fallback_and_the_neck_down_continue_the_connections_work_limit() {
    let (primary_only, max_units, since_call, outcome, budget) = route_gnd_without_vias(false);
    assert_eq!(outcome.state, AttemptState::Failed, "{}", outcome.details);
    assert_eq!(max_units, budget, "the limit is the connection's budget");
    assert!(outcome.work_limit_hit);
    // The limit starts right after the plane-tap call (the airline does no
    // tree work).
    assert_eq!(primary_only, since_call);
    assert!(primary_only > 0, "the primary attempt did work");

    let (both, max_units, since_call, outcome, _) = route_gnd_without_vias(true);
    assert_eq!(outcome.state, AttemptState::Failed);
    assert_eq!(max_units, budget);
    // One limit spans both attempts, counted from before the primary one.
    // It fired in the primary attempt, so the fallback stops at its first
    // check without any tree work; a fresh limit would have let it redo the
    // start-room work the primary attempt did before its first check.
    assert_eq!(both, since_call);
    assert_eq!(
        both, primary_only,
        "the fallback inherited the exceeded limit ({both} vs {primary_only})"
    );
    assert_eq!(outcome.work.units, since_call);
    // A plane is no pin or via: Java's airline is `FloatLine(null, null)`.
    assert!(outcome.airline.is_none());
}

// ---------------------------------------------------------------------------
// A11.3: StrictDrcEnforcementTest
// ---------------------------------------------------------------------------

/// A net whose traces or vias include a clearance violation (Java
/// `violatingNet`), in item-list order.
fn violating_net(board: &RoutingBoard) -> i32 {
    board
        .items()
        .find(|i| {
            (i.is_trace() || i.is_via())
                && i.net_count() > 0
                && !board.clearance_violations(i.id()).is_empty()
        })
        .map_or(-1, |i| i.net_number(0))
}

fn trace_count(board: &RoutingBoard, net: i32) -> usize {
    board
        .items()
        .filter(|i| i.is_trace() && i.contains_net(net))
        .count()
}

/// `ripsNewItemsWhenTheyCarryViolations`.
#[test]
fn strict_drc_rips_new_items_when_they_carry_violations() {
    let mut board = load(STRICT_FIXTURE).board;
    let net = violating_net(&board);
    assert!(net > 0, "the fixture has a violating routed net");
    let wiring: Vec<ItemId> = board
        .items()
        .filter(|i| (i.is_trace() || i.is_via()) && i.contains_net(net))
        .map(|i| i.id())
        .collect();
    for id in wiring {
        board.set_fixed_state(id, FixedState::Unfixed);
    }
    let before = trace_count(&board, net);
    assert!(before > 0);
    let result = enforce_strict_drc(&mut board, net, 0).expect("the connection is rejected");
    assert_eq!(result.state, AttemptState::Failed);
    assert!(
        result
            .details
            .starts_with("strict_drc: connection ripped because "),
        "{}",
        result.details
    );
    assert!(trace_count(&board, net) < before);
}

/// `keepsConnectionsWhoseNewItemsAreClean`.
#[test]
fn strict_drc_keeps_connections_whose_new_items_are_clean() {
    let mut board = load(STRICT_FIXTURE).board;
    let net = violating_net(&board);
    assert!(net > 0);
    let max_id = board.communication.id_generator.max_generated_id();
    let before = BoardStatistics::new(&board)
        .clearance_violations
        .total_count;
    assert!(enforce_strict_drc(&mut board, net, max_id).is_none());
    assert_eq!(
        BoardStatistics::new(&board)
            .clearance_violations
            .total_count,
        before
    );
}

/// The decision "no limit spans a board replacement": a strict-DRC
/// rejection with `strict_drc` on restores the board taken before the
/// route, whose meter is fresh, and the next connection's limit starts on
/// the restored board.
#[test]
fn a_strict_drc_restore_gives_the_next_connection_a_limit_on_the_restored_board() {
    let mut loaded = load_with(STRICT_FIXTURE, |s| s.strict_drc = true);
    let net = violating_net(&loaded.board);
    let wiring: Vec<ItemId> = loaded
        .board
        .items()
        .filter(|i| (i.is_trace() || i.is_via()) && i.contains_net(net))
        .map(|i| i.id())
        .collect();
    for id in &wiring {
        loaded.board.set_fixed_state(*id, FixedState::Unfixed);
    }
    let snapshot = loaded.board.snapshot();
    let items_before = loaded.board.items().count();
    assert!(loaded.board.search_trees().work_meter().units() > 0);
    let rejection =
        apply_strict_drc_after_route(&mut loaded.board, net, 0, Some(snapshot), true, &NullEvents)
            .expect("rejected");
    assert_eq!(rejection.state, AttemptState::Failed);
    // The whole board came back, with what the rip removed.
    assert_eq!(loaded.board.items().count(), items_before);
    assert!(trace_count(&loaded.board, net) > 0);
    assert_eq!(loaded.board.search_trees().work_meter().units(), 0);

    // The next connection: its limit starts at zero on the restored board.
    let task = autoroute_common::sample(&loaded.board)
        .into_iter()
        .find(|c| c.state.is_none())
        .map(|c| RouteTask {
            item: c.item,
            net: c.net,
            is_plane: c.plane,
        })
        .expect("a connection to route");
    let mut router = FreeroutingConnectionRouter::freerouting();
    let cancel = CancelToken::new();
    let trace_costs = loaded.router.trace_costs().to_vec();
    let (meter_before, outcome) = with_ctx(&mut loaded, &cancel, |board, ctx| {
        let meter_before = board.search_trees().work_meter().units();
        let p = connection_params(board, &task, 3, ctx, 300, &trace_costs, false, 500, true);
        (meter_before, router.route(board, &task, &p, ctx))
    });
    let meter_now = loaded.board.search_trees().work_meter().units();
    // The limit counted this connection's work on the restored board's
    // meter, from where it started: never the units of the board before
    // the restore.
    assert!(outcome.work.units > 0, "{outcome:?}");
    assert!(outcome.work.units <= meter_now - meter_before);
    assert!(!outcome.work_limit_hit);
}

// ---------------------------------------------------------------------------
// A11.4: the pass runner
// ---------------------------------------------------------------------------

#[test]
fn max_items_requests_a_full_stop() {
    let mut loaded = load_with(MIN_FR_TEST, |s| s.autorouter.max_items = 1);
    let mut runner = PassRunner::for_job(&loaded.settings, &loaded.router);
    let mut ordering = FreeroutingOrdering::new();
    let mut router = FreeroutingConnectionRouter::freerouting();
    let mut ripup = FreeroutingRipupPolicy::from_settings(&loaded.settings);
    let cancel = CancelToken::new();
    let outcome = with_ctx(&mut loaded, &cancel, |board, ctx| {
        let mut stages = PassStages {
            ordering: &mut ordering,
            router: &mut router,
            ripup: &mut ripup,
        };
        runner.run_pass(board, 1, &mut stages, ctx)
    });
    assert!(outcome.items > 1, "{outcome:?}");
    assert!(outcome.max_items_reached);
    assert_eq!(outcome.routed + outcome.failed + outcome.skipped, 1);
    assert_eq!(runner.total_items_routed(), 1);
    assert!(cancel.is_stop_requested(), "a full stop, not a router stop");
    assert_eq!(
        outcome.log.last().map(String::as_str),
        Some("Max items limit reached (1). Stopping auto-router.")
    );
}

#[test]
fn a_pass_with_nothing_to_route_says_so() {
    let mut loaded = load(MIN_FR_TEST);
    let outcomes = run_passes(&mut loaded, 3);
    assert!(outcomes[0].continue_routing);
    assert_eq!(outcomes[0].incomplete, 0, "{:?}", outcomes[0]);
    assert!(outcomes[1].nothing_to_route);
    assert!(!outcomes[1].continue_routing);
}

#[test]
fn remove_tails_takes_the_stop_option() {
    let mut loaded = load(MIN_FR_TEST);
    let runner = PassRunner::for_job(&loaded.settings, &loaded.router);
    assert!(!runner.remove_tails(&mut loaded.board, StopConnectionOption::None));
    assert!(loaded.board.changed_area().is_none());
}

// ---------------------------------------------------------------------------
// A11.6: the optimizer's entry point
// ---------------------------------------------------------------------------

#[test]
fn route_passes_for_optimizer_counts_as_java() {
    // Pass 1 completes the board, pass 2 finds nothing: Java returns 2.
    let mut loaded = load(MIN_FR_TEST);
    let cancel = CancelToken::new();
    let settings = loaded.settings.clone();
    let passes = with_ctx(&mut loaded, &cancel, |board, ctx| {
        route_passes_for_optimizer(board, 5, 100, 500, true, &settings, ctx)
    });
    assert_eq!(passes, 2);
    // Nothing left before pass 1: 1.
    let passes = with_ctx(&mut loaded, &cancel, |board, ctx| {
        route_passes_for_optimizer(board, 5, 100, 500, false, &settings, ctx)
    });
    assert_eq!(passes, 1);

    // No routing layer: every pass fails, Java returns max_passes + 1.
    let mut loaded = load_with(MIN_FR_TEST, |s| {
        for layer in s.layers.iter_mut().flatten() {
            layer.routable = Some(false);
        }
    });
    let settings = loaded.settings.clone();
    let passes = with_ctx(&mut loaded, &cancel, |board, ctx| {
        route_passes_for_optimizer(board, 3, 100, 500, true, &settings, ctx)
    });
    assert_eq!(passes, 4);
}

#[test]
fn the_registry_builds_the_ported_freerouting_stages() {
    let settings = load(MIN_FR_TEST).settings;
    let registry = freerouting::autoroute::stages::StageRegistry::builtin();
    let selection = freerouting::autoroute::stages::StageSelection::from_settings(&settings);
    let mut pipeline = registry.build(&selection, &settings).expect("built");
    assert_eq!(pipeline.connection_router.id(), "freerouting-connection");
    let mut loaded = load(MIN_FR_TEST);
    let task = autoroute_common::sample(&loaded.board)
        .into_iter()
        .find(|c| c.state.is_none() && !c.plane)
        .map(|c| RouteTask {
            item: c.item,
            net: c.net,
            is_plane: false,
        })
        .expect("a signal connection");
    {
        let cancel = CancelToken::new();
        let trace_costs = loaded.router.trace_costs().to_vec();
        let outcome = with_ctx(&mut loaded, &cancel, |board, ctx| {
            let p = connection_params(board, &task, 1, ctx, 100, &trace_costs, false, 500, false);
            pipeline.connection_router.route(board, &task, &p, ctx)
        });
        assert_ne!(
            outcome.details,
            "stage 'freerouting-connection' is not ported yet (A11.1)"
        );
        // Pin to pin: the airline joins the two nearest pin centres.
        assert!(outcome.airline.is_some(), "{outcome:?}");
    }
    let _ = FreeroutingNeckDown;
}

// ---------------------------------------------------------------------------
// A11.7: the pass probe
// ---------------------------------------------------------------------------

/// Boards of the `pass` row whose output differs from Java's, with the
/// reason. Empty: every board is required.
const PASS_EXPECTED_MISMATCHES: &[(&str, &str)] = &[];

#[test]
fn pass_matches_java() {
    let mut failures = Vec::new();
    let mut boards = 0;
    // `PASS_PROBE_BOARD=<board name>` runs one board (for debugging).
    let only = std::env::var("PASS_PROBE_BOARD").ok();
    for dsn in autoroute_common::kind_fixtures("pass") {
        let board = autoroute_common::board_name(&dsn);
        if only.as_ref().is_some_and(|b| *b != board) {
            continue;
        }
        boards += 1;
        let result = autoroute_common::compare_probe("pass", &dsn, &pass_lines::pass_lines(&dsn));
        let listed = PASS_EXPECTED_MISMATCHES.iter().any(|(b, _)| *b == board);
        match result {
            Err(report) if !listed => failures.push(report),
            Ok(()) if listed => failures.push(format!(
                "{board}: listed as an expected mismatch but matches Java; remove it"
            )),
            _ => {}
        }
    }
    assert!(boards > 0 || only.is_some(), "no pass fixture");
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
