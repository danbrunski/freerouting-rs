//! Q09: trace-only keepouts.
//!
//! - Q09.2: `AreaKind::TraceObstacle` keeps the traces of every net out and
//!   lets every via through, in the predicates the search and the DRC use;
//!   a plain keepout still blocks both.
//! - Q09.3: `router.wire_keepout`: a Specctra `wire_keepout` is a keepout
//!   (unset, Java's reading) or a trace-only keepout (`traces_only`), and the
//!   maze search then puts vias through it.
//! - Q09.4: `router.plane_obstacle_block = "traces"`: a pour of a listed
//!   class bars foreign traces and lets foreign vias through.
//!
//! The fixtures are synthetic three-layer boards in `tests/data/quality/`
//! (not `fixtures/`, whose boards all need Java references):
//! - `q09-wire-keepout.dsn`: a `wire_keepout` over all of In1.Cu, net A from
//!   an F.Cu pad to a B.Cu pad (so A needs a via through In1.Cu), and a plain
//!   `keepout` on B.Cu away from the route;
//! - `q09-plane-island.dsn`: a plane of net P12 (class `Pwr`) over all of
//!   In1.Cu; GND from an F.Cu pad to a B.Cu pad needs a via through it, and
//!   S, whose two F.Cu pads are either side of a keepout wall on F.Cu and
//!   B.Cu, can only connect with a trace on In1.Cu.

mod autoroute_common;

use std::path::Path;
use std::sync::Arc;

use autoroute_common::repo_root;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::board::items::{AreaKind, Item, ItemId};
use freerouting::board::structure::FixedState;
use freerouting::board::{BasicBoard, RoutingBoard};
use freerouting::geometry::{IntPoint, Point, Polyline};
use freerouting::io::specctra::ses_writer::{SesStyle, write_ses};
use freerouting::job::prepare::{JobError, JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::scoring::BoardStatistics;
use freerouting::settings::profiles::TestingProfile;
use freerouting::settings::{ResolveContext, Source};

const WIRE: &str = "crates/freerouting/tests/data/quality/q09-wire-keepout.dsn";
const ISLAND: &str = "crates/freerouting/tests/data/quality/q09-plane-island.dsn";
const ISSUE754: &str = "fixtures/Issue754-avionics_hub.dsn";
const Q01_BETWEEN: &str = "crates/freerouting/tests/data/quality/q01-pour-between-pads.dsn";

const TRACES_ONLY: &[(&str, &str)] = &[("wire_keepout", "traces_only")];

/// The job's load sequence on `dsn` with the test settings and `extra`
/// (`path=value` pairs set on the testing source).
fn prepare(dsn: &str, extra: &[(&str, &str)]) -> Result<PreparedJob, JobError> {
    let path = repo_root().join(dsn);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{dsn}: {e}"));
    let mut profile = TestingProfile::default();
    profile.set_optimizer_enabled(false);
    profile.set_max_passes(10);
    for (key, value) in extra {
        profile
            .patch
            .set_path(key, value)
            .unwrap_or_else(|e| panic!("{key}: {e:?}"));
    }
    let mut input = JobInput::new(bytes, JobInput::design_name_of(dsn));
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    input.extra_sources = vec![(Source::Testing, profile.patch)];
    prepare_job(&input, &ResolveContext { cores: 1 })
}

fn board_of(dsn: &str, extra: &[(&str, &str)]) -> RoutingBoard {
    prepare(dsn, extra)
        .unwrap_or_else(|e| panic!("{dsn}: {e}"))
        .board
}

/// The prepared board before routing and the routed board.
fn route(dsn: &str, extra: &[(&str, &str)]) -> (RoutingBoard, RoutingBoard) {
    let prepared = prepare(dsn, extra).unwrap_or_else(|e| panic!("{dsn}: {e}"));
    let before = prepared.board.deep_copy();
    let job = PipelineJob {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: Arc::new(CancelToken::new()),
        events: &NullEvents,
        deadline: None,
        registry: None,
    };
    let mut outcome =
        run_pipeline(prepared.board, &job).unwrap_or_else(|f| panic!("{dsn}: {}", f.error));
    let after = outcome.board.take().expect("no listener took the board");
    (before, after)
}

fn net_number(board: &BasicBoard, name: &str) -> i32 {
    board
        .rules
        .nets
        .get_by_name(name, 1)
        .unwrap_or_else(|| panic!("net {name}"))
        .net_number
}

fn layer(board: &BasicBoard, name: &str) -> i32 {
    board.layer_structure.get_no(name)
}

/// The keepout area of `kind` on `layer`.
fn area_on(board: &BasicBoard, kind: AreaKind, layer: i32) -> ItemId {
    board
        .items()
        .find(|i| {
            i.as_area()
                .is_some_and(|a| a.kind == kind && a.layer() == layer)
        })
        .unwrap_or_else(|| panic!("a {kind:?} area on layer {layer}"))
        .id()
}

fn centre(board: &BasicBoard, id: ItemId) -> IntPoint {
    let bb = board.item(id).expect("item").bounding_box();
    IntPoint::new((bb.ll.x / 2) + (bb.ur.x / 2), (bb.ll.y / 2) + (bb.ur.y / 2))
}

/// The via padstack of the board's via rule.
fn via_padstack(board: &BasicBoard) -> freerouting::library::PadstackId {
    board
        .rules
        .via_infos
        .iter()
        .next()
        .expect("a via")
        .1
        .padstack
}

/// A trace of `net` on `layer` through `centre`, 1 mm each way.
fn trace_through(board: &mut BasicBoard, net: i32, layer: i32, centre: IntPoint) -> ItemId {
    let points = [
        Point::Int(IntPoint::new(centre.x - 10000, centre.y)),
        Point::Int(IntPoint::new(centre.x + 10000, centre.y)),
    ];
    board
        .insert_trace_without_cleaning(
            Polyline::from_points(&points),
            layer,
            1250,
            &[net],
            1,
            FixedState::Unfixed,
        )
        .expect("the trace is inserted")
}

/// True if a violation of `item` names `other`.
fn violates(board: &BasicBoard, item: ItemId, other: ItemId) -> bool {
    board
        .clearance_violations(item)
        .iter()
        .any(|v| v.second_item == other)
}

/// True if the full DRC reports a violation between `a` and `b`.
fn drc_reports(board: &BasicBoard, a: ItemId, b: ItemId) -> bool {
    let findings = freerouting::drc::DesignRulesChecker::new(board).check();
    findings.clearance_violations.iter().any(|v| {
        (v.first_item == a && v.second_item == b) || (v.first_item == b && v.second_item == a)
    })
}

fn incomplete(board: &BasicBoard, net: i32) -> usize {
    let mut checker = freerouting::drc::DesignRulesChecker::new(board);
    checker.net_incompletes_of(net).map_or(0, |n| n.count())
}

fn violations(board: &BasicBoard) -> usize {
    BoardStatistics::new(board).clearance_violations.total_count
}

fn vias_of(board: &BasicBoard, net: i32) -> Vec<&Item> {
    board
        .items()
        .filter(|i| i.is_via() && i.contains_net(net))
        .collect()
}

fn traces_on(board: &BasicBoard, net: i32, layer: i32) -> usize {
    board
        .items()
        .filter(|i| i.is_trace() && i.contains_net(net) && i.first_layer() == layer)
        .count()
}

// ---------------------------------------------------------------------------
// Q09.2: the trace-only area kind
// ---------------------------------------------------------------------------

#[test]
fn a_trace_through_a_trace_only_area_is_refused_and_reported() {
    let mut board = board_of(WIRE, TRACES_ONLY);
    let in1 = layer(&board, "In1.Cu");
    let area = area_on(&board, AreaKind::TraceObstacle, in1);
    let a = net_number(&board, "A");
    let area_item = board.item(area).unwrap();
    // The search's predicate, for the routed net and for any other net.
    assert!(board.is_trace_obstacle(area_item, a));
    assert!(board.is_trace_obstacle(area_item, a + 1));
    let at = centre(&board, area);
    let trace = trace_through(&mut board, a, in1, at);
    let (trace_item, area_item) = (board.item(trace).unwrap(), board.item(area).unwrap());
    assert!(board.is_obstacle(trace_item, area_item));
    assert!(board.is_obstacle(area_item, trace_item));
    assert!(violates(&board, trace, area));
    assert!(drc_reports(&board, trace, area));
}

#[test]
fn a_via_inside_a_trace_only_area_is_accepted_and_not_reported() {
    let mut board = board_of(WIRE, TRACES_ONLY);
    let in1 = layer(&board, "In1.Cu");
    let area = area_on(&board, AreaKind::TraceObstacle, in1);
    let a = net_number(&board, "A");
    // The drill page does not cut it out, for any net.
    assert!(board.item(area).unwrap().is_drillable(a));
    assert!(board.item(area).unwrap().is_drillable(0));
    let at = centre(&board, area);
    let padstack = via_padstack(&board);
    let via = board.insert_via(padstack, at, &[a], 1, FixedState::Unfixed, false);
    let (via_item, area_item) = (board.item(via).unwrap(), board.item(area).unwrap());
    assert!(!board.is_obstacle(via_item, area_item));
    assert!(!board.is_obstacle(area_item, via_item));
    assert!(!violates(&board, via, area));
    assert!(!violates(&board, area, via));
    assert!(!drc_reports(&board, via, area));
}

#[test]
fn an_obstacle_area_still_blocks_traces_and_vias() {
    // The plain keepout on B.Cu, with the setting on.
    let mut board = board_of(WIRE, TRACES_ONLY);
    let b_cu = layer(&board, "B.Cu");
    let keepout = area_on(&board, AreaKind::Obstacle, b_cu);
    let a = net_number(&board, "A");
    assert!(board.is_trace_obstacle(board.item(keepout).unwrap(), a));
    assert!(!board.item(keepout).unwrap().is_drillable(a));
    let at = centre(&board, keepout);
    let trace = trace_through(&mut board, a, b_cu, at);
    let padstack = via_padstack(&board);
    let via = board.insert_via(padstack, at, &[a], 1, FixedState::Unfixed, false);
    for item in [trace, via] {
        let (i, k) = (board.item(item).unwrap(), board.item(keepout).unwrap());
        assert!(board.is_obstacle(i, k), "{item:?}");
        assert!(board.is_obstacle(k, i), "{item:?}");
        assert!(drc_reports(&board, item, keepout), "{item:?}");
    }
    // The wire_keepout itself, built Java's way (the default).
    let mut board = board_of(WIRE, &[]);
    let in1 = layer(&board, "In1.Cu");
    let area = area_on(&board, AreaKind::Obstacle, in1);
    let at = centre(&board, area);
    let trace = trace_through(&mut board, a, in1, at);
    assert!(drc_reports(&board, trace, area));
    // The via splits the trace; the via is reported too.
    let via = board.insert_via(padstack, at, &[a], 1, FixedState::Unfixed, false);
    assert!(drc_reports(&board, via, area));
    assert!(!board.item(area).unwrap().is_drillable(a));
}

// ---------------------------------------------------------------------------
// Q09.3: the `router.wire_keepout` setting
// ---------------------------------------------------------------------------

#[test]
fn the_default_builds_a_wire_keepout_as_java_does() {
    for extra in [&[][..], &[("wire_keepout", "keepout")][..]] {
        let board = board_of(WIRE, extra);
        let in1 = layer(&board, "In1.Cu");
        let area = area_on(&board, AreaKind::Obstacle, in1);
        assert_eq!(board.wire_keepouts(), [area]);
        assert!(!board.items().any(|i| {
            i.as_area()
                .is_some_and(|a| a.kind == AreaKind::TraceObstacle)
        }));
    }
    let board = board_of(WIRE, TRACES_ONLY);
    let in1 = layer(&board, "In1.Cu");
    assert_eq!(
        board.wire_keepouts(),
        [area_on(&board, AreaKind::TraceObstacle, in1)]
    );
    let error = prepare(WIRE, &[("wire_keepout", "vias_only")]).expect_err("refused");
    assert!(error.to_string().contains("router.wire_keepout"), "{error}");
    assert!(error.to_string().contains("vias_only"), "{error}");
}

#[test]
fn with_traces_only_the_router_puts_a_via_through_the_wire_keepout() {
    // Java's reading: no via can pass In1.Cu, so A stays unrouted.
    let (before, after) = route(WIRE, &[]);
    let a = net_number(&after, "A");
    assert_eq!(incomplete(&after, a), 1);
    assert!(vias_of(&after, a).is_empty());
    assert_eq!(violations(&after), violations(&before));
    // Trace-only: A routes through a via, with no trace on In1.Cu.
    let (before, after) = route(WIRE, TRACES_ONLY);
    let in1 = layer(&after, "In1.Cu");
    assert_eq!(incomplete(&after, a), 0);
    let vias = vias_of(&after, a);
    assert!(!vias.is_empty());
    let area = area_on(&after, AreaKind::TraceObstacle, in1);
    for via in &vias {
        assert!(!violates(&after, via.id(), area));
    }
    assert_eq!(traces_on(&after, a, in1), 0);
    assert_eq!(violations(&after), violations(&before));
}

/// Issue754's `wire_keepout` covers its whole Ground layer, so with Java's
/// reading no via may pass Ground. With `traces_only` the area is a
/// trace-only keepout: the drill page keeps it, and a via inside it is no
/// obstacle and no violation.
#[test]
fn issue754_ground_accepts_vias_with_traces_only() {
    for (extra, accepts) in [(&[][..], false), (TRACES_ONLY, true)] {
        let mut board = board_of(ISSUE754, extra);
        let ground = layer(&board, "Ground");
        let kind = if accepts {
            AreaKind::TraceObstacle
        } else {
            AreaKind::Obstacle
        };
        let area = area_on(&board, kind, ground);
        assert_eq!(board.wire_keepouts(), [area]);
        let net = net_number(&board, "SERVO_PWR");
        assert_eq!(board.item(area).unwrap().is_drillable(net), accepts);
        assert!(board.is_trace_obstacle(board.item(area).unwrap(), net));
        let padstack = board
            .items()
            .find_map(|i| i.as_via().map(|v| v.drill.padstack))
            .expect("the board has vias");
        let at = centre(&board, area);
        let via = board.insert_via(padstack, at, &[net], 1, FixedState::Unfixed, false);
        let (via_item, area_item) = (board.item(via).unwrap(), board.item(area).unwrap());
        assert_eq!(board.is_obstacle(via_item, area_item), !accepts);
        assert_eq!(violates(&board, via, area), !accepts);
    }
}

// ---------------------------------------------------------------------------
// Q09.4: the pour guard's `traces` block mode
// ---------------------------------------------------------------------------

const GUARD_TRACES: &[(&str, &str)] = &[
    ("plane_obstacle_net_classes", "Pwr"),
    ("plane_obstacle_block", "traces"),
];
const GUARD_BOTH: &[(&str, &str)] = &[
    ("plane_obstacle_net_classes", "Pwr"),
    ("plane_obstacle_block", "traces_and_vias"),
];

#[test]
fn traces_mode_lets_a_foreign_gnd_via_through_the_island() {
    let (before, after) = route(ISLAND, GUARD_TRACES);
    let (gnd, s) = (net_number(&after, "GND"), net_number(&after, "S"));
    let in1 = layer(&after, "In1.Cu");
    // GND's via passes the +12 V island, with no clearance violation.
    assert_eq!(incomplete(&after, gnd), 0);
    let vias = vias_of(&after, gnd);
    assert!(!vias.is_empty());
    for via in &vias {
        assert!(after.clearance_violations(via.id()).is_empty(), "{via:?}");
    }
    assert_eq!(traces_on(&after, gnd, in1), 0);
    // A foreign trace does not cross it: S has no other way and stays open.
    assert_eq!(traces_on(&after, s, in1), 0);
    assert_eq!(incomplete(&after, s), 1);
    assert_eq!(violations(&after), violations(&before));
}

#[test]
fn traces_and_vias_mode_keeps_q01_behaviour() {
    // The fixture tests something: with the key empty GND vias through and S
    // crosses on In1.Cu.
    let (_, open) = route(ISLAND, &[]);
    let (gnd, s) = (net_number(&open, "GND"), net_number(&open, "S"));
    let in1 = layer(&open, "In1.Cu");
    assert_eq!(incomplete(&open, gnd), 0);
    assert_eq!(incomplete(&open, s), 0);
    assert!(traces_on(&open, s, in1) > 0);
    // Q01's mode blocks the GND via as well.
    let (_, both) = route(ISLAND, GUARD_BOTH);
    assert_eq!(incomplete(&both, gnd), 1);
    assert!(vias_of(&both, gnd).is_empty());
    assert_eq!(incomplete(&both, s), 1);
    // Unset and explicit `traces_and_vias` route byte-identically, here and
    // on Q01's fixture.
    for dsn in [ISLAND, Q01_BETWEEN] {
        let (_, unset) = route(dsn, &[("plane_obstacle_net_classes", "Pwr")]);
        let (_, explicit) = route(dsn, GUARD_BOTH);
        assert_eq!(
            write_ses(&unset, "b", SesStyle::Java),
            write_ses(&explicit, "b", SesStyle::Java),
            "{dsn}"
        );
    }
    let error = prepare(
        ISLAND,
        &[
            ("plane_obstacle_net_classes", "Pwr"),
            ("plane_obstacle_block", "vias"),
        ],
    )
    .expect_err("refused");
    assert!(
        error.to_string().contains("router.plane_obstacle_block"),
        "{error}"
    );
}
