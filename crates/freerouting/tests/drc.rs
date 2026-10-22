//! Ports of the Java tests of the design rules check
//! (`src/test/java/app/freerouting/drc`, the clearance and connection
//! parts of `core/scoring` and the reading part of
//! `io/specctra/SesRoundTripTest`), and a test of the drawable DRC data a
//! viewer uses.
//!
//! `ClearanceViolationTest`'s mocks are real items of a small board.
//!
//! Not ported: the SES writer tests (no SES writer
//! yet), `NetRoutingLedgerTest`'s incremental ledger (only its full-scan
//! counts, which the ledger must match, are checked), the KiCad JSON
//! constructor of `BoardStatisticsTest` and the GUI tests.

use std::path::{Path, PathBuf};

use freerouting::board::RoutingBoard;
use freerouting::board::items::{ItemId, ItemKind};
use freerouting::board::structure::{FixedState, Layer, LayerStructure};
use freerouting::drc::{
    ClearanceViolation, ClearanceViolationCategory, DesignRulesChecker, ReportRules,
    UnconnectedKind, ZoneIslandKind, aggregate_sorted_by_severity, load_design,
};
use freerouting::geometry::{
    Area, Circle, ConvexShape, IntBox, IntPoint, Point, Polyline, PolylineShape, PolylineShapeOps,
    Shape, TileShape,
};
use freerouting::io::{BoardReadResult, read_board, read_session};
use freerouting::rules::{BoardRules, ClearanceMatrix};
use freerouting::scoring::{BoardStatistics, OptimizerScoreSettings, RouterScoreSettings};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

/// `DsnReader.readBoard` without post-processing.
fn read_fixture(name: &str) -> RoutingBoard {
    let bytes = std::fs::read(fixture(name)).unwrap();
    match read_board(&bytes) {
        BoardReadResult::Success { board, .. } => *board,
        BoardReadResult::OutlineMissing {
            board: Some(board), ..
        } => *board,
        _ => panic!("{name} must load"),
    }
}

/// `BoardLoader.loadBoardIfNeeded`: the board as the DRC CLI loads it.
fn load_fixture(name: &str) -> RoutingBoard {
    load_design(&std::fs::read(fixture(name)).unwrap()).unwrap()
}

const INCOMPLETE_FIXTURE: &str = "Issue575-drc_dev-board_4_hole_clearance_violations.dsn";
const VIOLATION_FIXTURE: &str = "Issue575-drc_BBD_Mars-64_6_track_1_hole_clearance_violations.dsn";
const UNCONNECTED_FIXTURE: &str = "Issue575-drc_Natural_Tone_Preamp_7_unconnected_items.dsn";

// ---------------------------------------------------------------------------
// Drawable data for viewers (no Java counterpart).

#[test]
fn drc_findings_are_drawable() {
    let board = load_fixture(VIOLATION_FIXTURE);
    let findings = board.design_rules_check();
    let layer_count = board.layer_count() as i32;

    // Clearance violations: a shape on a layer between two items, with
    // the required and the actual clearance.
    assert_eq!(findings.clearance_violations.len(), 67);
    for v in &findings.clearance_violations {
        assert!((0..layer_count).contains(&v.layer));
        assert!(v.expected_clearance > v.actual_clearance);
        assert!(v.actual_clearance >= 0.0);
        assert!(!v.shape.is_empty());
        let centre = v.location();
        for id in [v.first_item, v.second_item] {
            let item = board.item(id).expect("violation items exist");
            assert!(item.first_layer() <= v.layer && v.layer <= item.last_layer());
            // The violation lies within the clearance of both items.
            let b = item.bounding_box();
            let margin = v.expected_clearance;
            assert!(
                centre.x >= f64::from(b.ll.x) - margin
                    && centre.x <= f64::from(b.ur.x) + margin
                    && centre.y >= f64::from(b.ll.y) - margin
                    && centre.y <= f64::from(b.ur.y) + margin,
                "violation at {centre:?} is not near item {id:?}"
            );
        }
    }

    // Airlines: two endpoints on two unconnected items of the net.
    let airlines: Vec<_> = findings.airlines().collect();
    assert!(!airlines.is_empty());
    for airline in &airlines {
        for (id, corner, layers) in [
            (airline.from_item, airline.from_corner, airline.from_layers),
            (airline.to_item, airline.to_corner, airline.to_layers),
        ] {
            let item = board.item(id).expect("airline items exist");
            assert!(item.contains_net(airline.net_number));
            assert_eq!(layers, [item.first_layer(), item.last_layer()]);
            let b = item.bounding_box();
            assert!(
                corner.x >= f64::from(b.ll.x)
                    && corner.x <= f64::from(b.ur.x)
                    && corner.y >= f64::from(b.ll.y)
                    && corner.y <= f64::from(b.ur.y),
                "airline end {corner:?} is not on item {id:?}"
            );
        }
        assert_ne!(airline.from_item, airline.to_item);
    }
    let per_net: usize = findings.incompletes.iter().map(|n| n.airlines.len()).sum();
    assert_eq!(per_net, airlines.len());
    assert_eq!(board.incompletes(), findings.incompletes);

    // The counts agree with the board statistics.
    let stats = BoardStatistics::new(&board);
    assert_eq!(stats.connections.incomplete_count, airlines.len());
    assert_eq!(
        stats.clearance_violations.total_count,
        findings.clearance_violations.len()
    );
}

// ---------------------------------------------------------------------------
// drc.RatsnestClearanceHeadlessTest

#[test]
fn incompletes_are_computable_via_design_rules_checker() {
    let board = read_fixture(INCOMPLETE_FIXTURE);
    let mut drc = DesignRulesChecker::new(&board);
    drc.calculate_all_incompletes();
    assert_eq!(drc.incomplete_count(), 9);
    assert_eq!(drc.all_airlines().len(), 9);
    let sum: usize = (1..=board.rules.nets.max_net_number())
        .map(|net| drc.incomplete_count_of(net))
        .sum();
    assert_eq!(sum, 9);
}

#[test]
fn clearance_violations_are_computable_via_drc() {
    let board = read_fixture(VIOLATION_FIXTURE);
    let drc = DesignRulesChecker::new(&board);
    assert_eq!(drc.all_clearance_violations().len(), 67);
}

#[test]
fn clearance_violation_aggregation_is_severity_sorted() {
    let board = read_fixture(VIOLATION_FIXTURE);
    let ids: Vec<ItemId> = board.items().map(|i| i.id()).collect();
    let (aggregated, smallest) = aggregate_sorted_by_severity(&board, &ids);
    assert!(aggregated.len() >= 67);
    for pair in aggregated.windows(2) {
        assert!(pair[0].shortfall() >= pair[1].shortfall());
    }
    assert!((0.0..f64::MAX).contains(&smallest));
}

#[test]
fn empty_board_has_no_incompletes_and_no_violations() {
    let board = read_fixture("empty_board.dsn");
    let mut drc = DesignRulesChecker::new(&board);
    assert_eq!(drc.incomplete_count(), 0);
    assert!(drc.all_clearance_violations().is_empty());
    let ids: Vec<ItemId> = board.items().map(|i| i.id()).collect();
    assert!(aggregate_sorted_by_severity(&board, &ids).0.is_empty());
}

// ---------------------------------------------------------------------------
// drc.ClearanceViolationTest, on real items instead of mocks.

#[test]
fn clearance_violation_categories() {
    let board = read_fixture(VIOLATION_FIXTURE);
    let violations = board.all_clearance_violations();
    for v in &violations {
        let first = board.item(v.first_item).unwrap();
        let second = board.item(v.second_item).unwrap();
        let unfixable = !first.is_routable() && !second.is_routable();
        assert_eq!(v.is_unfixable(&board), unfixable);
        let category = v.category(&board);
        if !unfixable {
            assert_eq!(category, ClearanceViolationCategory::PotentiallyFixable);
        } else if first.is_pin() && second.is_pin() {
            assert_eq!(category, ClearanceViolationCategory::PinToPin);
        }
    }
    assert!(!violations.is_empty());
}

// ---------------------------------------------------------------------------
// drc.UnconnectedItemsReproductionTest

/// Java's test asserts lower bounds taken from a KiCad report (9 groups,
/// 24 dangling tracks, 4 dangling vias). The values here are what Java's
/// DRC finds on this board at `cda540f`: the DRC command line
/// (`tests/data/java_drc_digest.tsv`, `load` mode) and an in-process probe of
/// `UnconnectedItemsReproductionTest`'s steps (`getAllUnconnectedItems` and
/// `generateReport` on `BoardLoader.loadBoardIfNeeded`) both give 44 groups,
/// 110 dangling tracks and 4 dangling vias (2484, 2493, 2494, 2518), with
/// trace 2402 and 2411 tails of net 1 with one contact each and via 2522 on
/// layers 0 to 1, not a tail, with two contacts.
#[test]
fn issue575_unconnected_items() {
    let board = load_fixture(UNCONNECTED_FIXTURE);
    let trace1 = board.get_item(2402).expect("item 2402");
    let trace2 = board.get_item(2411).expect("item 2411");
    assert!(trace1.is_trace() && trace2.is_trace());
    assert_eq!(trace1.net_number(0), 1);
    assert_eq!(trace2.net_number(0), 1);
    for trace in [trace1, trace2] {
        assert!(board.is_tail(trace.id()));
        assert_eq!(board.normal_contacts(trace.id()).len(), 1);
    }
    let pin = board.get_item(321).expect("item 321");
    assert!(pin.is_pin());
    assert!(!board.normal_contacts(trace1.id()).contains(&pin.id()));
    let via = board.get_item(2522).expect("item 2522");
    assert!(via.is_via());
    assert_eq!((via.first_layer(), via.last_layer()), (0, 1));
    assert!(!board.is_tail(via.id()));
    assert_eq!(board.normal_contacts(via.id()).len(), 2);

    let mut drc = DesignRulesChecker::new(&board);
    let all = drc.all_unconnected_items();
    let ids = |kind| -> Vec<i32> {
        let mut ids: Vec<i32> = all
            .iter()
            .filter(|u| u.kind == kind)
            .map(|u| u.first_item.0)
            .collect();
        ids.sort_unstable();
        ids
    };
    let groups = all
        .iter()
        .filter(|u| u.kind == UnconnectedKind::UnconnectedItems)
        .count();
    assert_eq!(groups, 44);
    assert_eq!(ids(UnconnectedKind::ViaDangling), [2484, 2493, 2494, 2518]);
    // Java reports 110 dangling tracks. Here trace 1919 represents its
    // group (the first trace of a group without a pin, see
    // docs/DIVERGENCES.md) and is not reported, so 109. Java's
    // representative of that group is another item, so 1919 is dangling
    // there (in both Java runs). Java's choice in the group of trace 1706
    // varies from run to run with the identity hash codes.
    let dangling = ids(UnconnectedKind::TrackDangling);
    assert_eq!(dangling.len(), 109);
    assert!(!dangling.contains(&1919));
    assert!(board.is_tail(board.get_item(1919).unwrap().id()));
    assert!(
        all.iter()
            .any(|u| u.kind == UnconnectedKind::UnconnectedItems && u.first_item.0 == 1919)
    );
    for id in [2340, 1869, 2372, 1802, 2402, 2411] {
        let item = board.get_item(id).expect("dangling track exists");
        assert!(item.is_trace());
        assert!(board.is_tail(item.id()));
        assert!(dangling.contains(&id), "track {id} must be dangling");
    }

    // Java's counting (K15.5: the report counts as KiCad does by default).
    drc.set_report_rules(ReportRules::Java);
    let report = drc.generate_report(UNCONNECTED_FIXTURE, "mm");
    let count = |report: &freerouting::drc::DrcReport, t: &str| {
        report
            .violations
            .iter()
            .filter(|v| v.violation_type == t)
            .count()
    };
    // Java: 110, 4 and 44 (the 1919 difference above).
    assert_eq!(count(&report, "track_dangling"), 109);
    assert_eq!(count(&report, "via_dangling"), 4);
    assert_eq!(report.unconnected_items.len(), 44);
    // KiCad's counting (K15.5, K15.11): the 109 tracks end on copper of their
    // net (most inside a pad off its centre), but 5 have a short segment whose
    // both ends lie on one other track, which meets only the nearer end
    // (KiCad's `TestTrackEndpointDangling`): KiCad 9.0.1's report of the board
    // (`Issue575-drc_Natural_Tone_Preamp_7_unconnected_items-kicad_drc.json` of
    // Java's fixtures at the pin) has the same 5. The 4 vias stay dangling,
    // and the split nets are Java's.
    let kicad = DesignRulesChecker::new(&board).generate_report(UNCONNECTED_FIXTURE, "mm");
    let mut kicad_dangling: Vec<i32> = kicad
        .violations
        .iter()
        .filter(|v| v.violation_type == "track_dangling")
        .map(|v| v.items[0].uuid.parse().unwrap())
        .collect();
    kicad_dangling.sort_unstable();
    assert_eq!(kicad_dangling, [1697, 1858, 1860, 2265, 2372]);
    assert_eq!(count(&kicad, "via_dangling"), 4);
    assert_eq!(kicad.unconnected_items.len(), 44);
}

// ---------------------------------------------------------------------------
// drc.DesignRulesCheckerTest, drc.DrcCoordinateTest,
// drc.DrcSummaryResponseTest

#[test]
fn drc_report_structure_and_json_format() {
    let board = load_fixture("Issue555-BBD_Mars-64.dsn");
    let report = DesignRulesChecker::new(&board).generate_report("test.dsn", "mm");
    assert_eq!(report.coordinate_units, "mm");
    assert_eq!(report.source, "test.dsn");
    assert!(report.freerouting_version.contains("Freerouting"));
    let json = report.to_json();
    for key in [
        "\"$schema\": \"https://schemas.kicad.org/drc.v1.json\"",
        "\"coordinateUnits\"",
        "\"date\"",
        "\"kicadVersion\"",
        "\"source\"",
        "\"violations\"",
        "\"unconnectedItems\"",
        "\"schematicParity\"",
    ] {
        assert!(json.contains(key), "missing {key}");
    }
}

#[test]
fn drc_coordinates_are_in_millimeters() {
    let board = load_fixture(UNCONNECTED_FIXTURE);
    let report = DesignRulesChecker::new(&board).generate_report("test.dsn", "mm");
    let item = &report.unconnected_items[0].items[0];
    assert!(item.pos.x.abs() < 500.0 && item.pos.x.abs() > 10.0);
    assert!(item.pos.y.abs() < 500.0 && item.pos.y.abs() > 10.0);
}

#[test]
fn drc_summary() {
    let board = load_fixture("Issue508-DAC2020_bm01.dsn");
    let summary = DesignRulesChecker::new(&board).generate_summary();
    assert_eq!(
        summary.clearance_violations_count,
        board.all_clearance_violations().len()
    );
    assert!(summary.violations.len() >= summary.clearance_violations_count);
}

// ---------------------------------------------------------------------------
// drc.ZoneIslandConnectivityTest

struct ZoneBoard {
    board: RoutingBoard,
    gnd: i32,
    sig: i32,
    padstack: freerouting::library::PadstackId,
}

fn zone_board() -> ZoneBoard {
    let layers = LayerStructure::new(vec![Layer::new("Top", false), Layer::new("GND", false)]);
    let matrix = ClearanceMatrix::default_instance(layers.count(), 10);
    let mut rules = BoardRules::new(layers.clone(), matrix);
    rules.create_default_net_class();
    let outline = IntBox::new(0, 0, 10_000_000, 10_000_000);
    let mut board = RoutingBoard::new(
        outline,
        layers,
        vec![PolylineShape::Tile(TileShape::from(outline))],
        0,
        rules,
        Default::default(),
    );
    let class = board.rules.default_net_class();
    let gnd = board.rules.nets.add("GND", 1, true, class);
    let sig = board.rules.nets.add("SIG", 1, false, class);
    // Java uses a square pad. Its thermal spokes, rotated by 45 degrees
    // and cut to the pad clearance, end inside the clearance square and do
    // not reach the fill, so the via is an island of its own (Java merges
    // it into the pour by mistake, see DIVERGENCES.md). The spokes of a
    // round pad reach the fill.
    let round = ConvexShape::Circle(Circle::new(IntPoint::new(0, 0), 100_000));
    let padstack = board.library.padstacks.add(
        "TestViaPadstack",
        vec![Some(round.clone()), Some(round)],
        vec![None, None],
        true,
        false,
    );
    ZoneBoard {
        board,
        gnd,
        sig,
        padstack,
    }
}

fn pour(z: &mut ZoneBoard, layer: i32, b: IntBox) {
    let gnd = z.gnd;
    z.board.insert_conduction_area(
        Area::from(Shape::Polyline(PolylineShape::Tile(TileShape::from(b)))),
        layer,
        &[gnd],
        0,
        false,
        FixedState::Unfixed,
    );
}

fn gnd_via(z: &mut ZoneBoard, x: i32, y: i32) {
    let (padstack, gnd) = (z.padstack, z.gnd);
    z.board.insert_via(
        padstack,
        IntPoint::new(x, y),
        &[gnd],
        0,
        FixedState::Unfixed,
        true,
    );
}

fn sig_trace(z: &mut ZoneBoard, corners: &[(i32, i32)]) {
    let points: Vec<Point> = corners
        .iter()
        .map(|&(x, y)| Point::Int(IntPoint::new(x, y)))
        .collect();
    let sig = z.sig;
    z.board.insert_trace(
        Polyline::from_points(&points),
        0,
        100_000,
        &[sig],
        0,
        FixedState::Unfixed,
    );
}

#[test]
fn continuous_pour_has_no_island_violations() {
    let mut z = zone_board();
    pour(
        &mut z,
        1,
        IntBox::new(100_000, 100_000, 9_000_000, 9_000_000),
    );
    assert!(
        DesignRulesChecker::new(&z.board)
            .zone_island_violations()
            .is_empty()
    );
}

#[test]
fn severed_pour_has_an_isolated_unconnected_island() {
    let mut z = zone_board();
    pour(&mut z, 0, IntBox::new(0, 0, 4_000_000, 4_000_000));
    gnd_via(&mut z, 500_000, 2_000_000);
    gnd_via(&mut z, 3_500_000, 2_000_000);
    sig_trace(&mut z, &[(2_000_000, 0), (2_000_000, 4_000_000)]);
    let violations = DesignRulesChecker::new(&z.board).zone_island_violations();
    let unconnected: Vec<_> = violations
        .iter()
        .filter(|v| v.kind == ZoneIslandKind::IsolatedIslandUnconnected)
        .collect();
    assert!(!unconnected.is_empty());
    for v in unconnected {
        assert_eq!(v.layer, 0);
        assert_eq!(v.net_number, z.gnd);
        assert!(!v.items_in_island.is_empty());
    }
}

#[test]
fn cut_off_pour_has_a_dead_copper_island() {
    let mut z = zone_board();
    pour(&mut z, 0, IntBox::new(0, 0, 4_000_000, 4_000_000));
    gnd_via(&mut z, 500_000, 2_000_000);
    sig_trace(&mut z, &[(2_500_000, 0), (2_500_000, 4_000_000)]);
    let violations = DesignRulesChecker::new(&z.board).zone_island_violations();
    let dead: Vec<_> = violations
        .iter()
        .filter(|v| v.kind == ZoneIslandKind::IsolatedIslandDeadCopper)
        .collect();
    assert!(!dead.is_empty());
    for v in dead {
        assert_eq!(v.layer, 0);
        assert_eq!(v.net_number, z.gnd);
        assert!(v.items_in_island.is_empty());
        assert!(!v.island.trapezoids.is_empty());
    }
}

/// Java inserts a closed square trace, which the trace normalization
/// removes as a cycle (in both ports), so the Java test checks a pour
/// without a hole. Here an open trace makes a hole.
#[test]
fn hole_inside_pour_is_not_an_island() {
    let mut z = zone_board();
    pour(&mut z, 0, IntBox::new(0, 0, 4_000_000, 4_000_000));
    gnd_via(&mut z, 500_000, 2_000_000);
    sig_trace(
        &mut z,
        &[
            (2_000_000, 2_000_000),
            (2_500_000, 2_000_000),
            (2_500_000, 2_500_000),
        ],
    );
    assert_eq!(z.board.traces().len(), 1);
    assert!(
        DesignRulesChecker::new(&z.board)
            .zone_island_violations()
            .is_empty()
    );
}

// ---------------------------------------------------------------------------
// core.scoring.BoardStatisticsTest (V2 scores), BoardStatisticsClearancePathTest,
// SyntheticPerfectTwoPinTest; drc.NetRoutingLedgerTest (full scan only)

#[test]
fn v2_router_score_uses_two_half_unrouted_weights() {
    let mut stats = BoardStatistics::default();
    stats.connections.maximum_count = 10;
    stats.connections.incomplete_count = 2;
    stats.clearance_violations.total_count = 2;
    stats.clearance_violations.total_violation_um = 3000.0;
    stats.difficulty.difficulty_d = 10.0;
    let settings = RouterScoreSettings::default();
    assert!((stats.router_score_v2(&settings) - 638.333).abs() < 0.01);
    stats.clearance_violations.total_count = 0;
    stats.clearance_violations.total_violation_um = 0.0;
    stats.connections.incomplete_count = 5;
    assert!((stats.router_score_v2(&settings) - 333.333).abs() < 0.01);
    stats.connections.incomplete_count = 8;
    assert!((stats.router_score_v2(&settings) - 133.333).abs() < 0.01);
    stats.connections.incomplete_count = 10;
    assert!(stats.router_score_v2(&settings).abs() < 0.001);
    stats.connections.incomplete_count = 0;
    assert!((stats.router_score_v2(&settings) - 1000.0).abs() < 0.001);
}

#[test]
fn v2_optimizer_score_uses_lower_bounds_and_difficulty() {
    let mut stats = BoardStatistics::default();
    stats.bounds.min_trace_length_mm = 10.0;
    stats.bounds.min_via_count = 1;
    stats.bounds.min_bend_count = 1;
    stats.traces.total_length_mm = 15.0;
    stats.vias.total_count = 2;
    stats.bends.total_count = 3;
    stats.difficulty.difficulty_d = 10.0;
    let score = stats.optimizer_score_v2(&OptimizerScoreSettings::default());
    assert!((score - 200.0).abs() < 0.001);
}

#[test]
fn v2_router_score_is_defined_for_zero_connection_boards() {
    let mut stats = BoardStatistics::default();
    stats.difficulty.difficulty_d = 1.0;
    assert!((stats.router_score_v2_default() - 1000.0).abs() < 0.001);
    stats.clearance_violations.total_count = 2;
    assert!((stats.router_score_v2_default() - 950.0).abs() < 0.001);
}

#[test]
fn board_statistics_use_the_full_drc_count() {
    let board = read_fixture(VIOLATION_FIXTURE);
    let full = DesignRulesChecker::new(&board)
        .all_clearance_violations()
        .len();
    let stats = BoardStatistics::new(&board);
    assert_eq!(stats.clearance_violations.total_count, full);
    assert_eq!(full, 67);
}

/// The pins of `board` with their centres and nets.
fn pins(board: &RoutingBoard) -> Vec<(IntPoint, i32)> {
    board
        .pins()
        .into_iter()
        .map(|id| {
            let pin = board.item(id).unwrap();
            (pin.drill().unwrap().center, pin.net_number(0))
        })
        .collect()
}

/// Inserts the straight trace between the two pins of the perfect two-pin
/// fixture.
fn insert_centerline(board: &mut RoutingBoard) -> ItemId {
    let pins = pins(board);
    assert_eq!(pins.len(), 2);
    let net = pins[0].1;
    let class = board.rules.net_class_of(net).unwrap();
    let half_width = class.trace_half_width(0);
    let clearance_class = class.trace_clearance_class();
    board
        .insert_trace_without_cleaning(
            Polyline::from_corners(&Point::Int(pins[0].0), &Point::Int(pins[1].0)),
            0,
            half_width,
            &[net],
            clearance_class,
            FixedState::Unfixed,
        )
        .unwrap()
}

#[test]
fn synthetic_perfect_two_pin_scores_are_one_thousand() {
    let mut board = read_fixture("scoring-perfect-two-pin.dsn");
    insert_centerline(&mut board);
    let stats = BoardStatistics::new(&board);
    assert_eq!(stats.layers.signal_count, 1);
    assert_eq!(stats.connections.incomplete_count, 0);
    assert_eq!(stats.vias.total_count, 0);
    assert_eq!(stats.bends.total_count, 0);
    assert_eq!(stats.bounds.min_via_count, 0);
    assert_eq!(stats.bounds.min_bend_count, 0);
    assert!((stats.bounds.min_trace_length_mm - stats.traces.total_length_mm).abs() < 0.01);
    assert!(board.all_clearance_violations().is_empty());
    assert_eq!(stats.clearance_violations.total_count, 0);
    assert!((stats.router_score_v2_default() - 1000.0).abs() < 0.01);
    assert!((stats.optimizer_score_v2_default() - 1000.0).abs() < 0.01);
}

#[test]
fn incomplete_count_follows_insert_and_remove() {
    let mut board = read_fixture("scoring-perfect-two-pin.dsn");
    let count = |board: &RoutingBoard| {
        let mut drc = DesignRulesChecker::new(board);
        (
            drc.incomplete_count(),
            drc.max_connections(),
            drc.incomplete_net_numbers().len(),
        )
    };
    assert_eq!(count(&board), (1, 1, 1));
    let trace = insert_centerline(&mut board);
    assert_eq!(count(&board), (0, 1, 0));
    board.remove_item(trace);
    assert_eq!(count(&board), (1, 1, 1));
}

// ---------------------------------------------------------------------------
// io.specctra.SesRoundTripTest (reading only)

#[test]
fn ses_import_reads_wires_without_errors() {
    let mut board = load_fixture("Issue593-BBD_Mars-64.dsn");
    let text = std::fs::read_to_string(fixture("Issue593-BBD_Mars-64.ses")).unwrap();
    let summary = read_session(&mut board, &text).unwrap();
    assert!(summary.wires_imported > 0);
    assert_eq!(summary.errors_encountered, 0);
}

#[test]
fn invalid_ses_is_an_error() {
    let mut board = load_fixture("Issue143-rpi_splitter.dsn");
    assert!(read_session(&mut board, "garbage").is_err());
}

// ---------------------------------------------------------------------------
// drc.ClearanceViolationTest, one test per Java test, on real items.
//
// Java builds each violation from two mocked items. Here the items are
// real items of a small board, and the violation is the one the clearance
// check finds between them, except where the check never reports such a
// pair (a component outline has no search-tree shapes): then the violation
// is built from the two real items as Java builds it.

/// A two-layer board with every pair the Java tests name. Units are um,
/// 10 board units per um. Each pin has a net of its own.
const CATEGORY_DSN: &str = r#"
(pcb categories.dsn (parser (string_quote ") (space_in_quoted_tokens on))
  (resolution um 10) (unit um)
  (structure
    (layer F.Cu (type signal) (property (index 0)))
    (layer B.Cu (type signal) (property (index 1)))
    (boundary (path pcb 0  0 0  40000 0  40000 -20000  0 -20000  0 0))
    (plane GND (polygon B.Cu 0  20000 -1000  30000 -1000  30000 -9000  20000 -9000))
    (plane VCC (polygon B.Cu 0  25000 -5000  35000 -5000  35000 -15000  25000 -15000))
    (rule (width 250) (clearance 200)))
  (placement
    (component TP
      (place P1 5000 -5000 front 0)
      (place P2 5800 -5000 front 0)
      (place E1 0 -10000 front 0)
      (place K1 22000 -3000 front 0)
      (place F1 10000 -15000 front 0)
      (place R1 15000 -8000 front 0)))
  (library
    (image TP
      (outline (path signal 100  -1000 -1000  1000 -1000  1000 1000  -1000 1000  -1000 -1000))
      (pin THT 1 0 0))
    (padstack THT (shape (circle F.Cu 1500)) (shape (circle B.Cu 1500)) (attach off))
    (padstack VIA (shape (circle F.Cu 600)) (shape (circle B.Cu 600)) (attach off)))
  (network
    (net A (pins P1-1)) (net B (pins P2-1)) (net C (pins E1-1)) (net D (pins K1-1))
    (net E (pins F1-1)) (net H (pins R1-1))
    (net T (pins)) (net V (pins)) (net X (pins)) (net Y (pins)) (net Z (pins))
    (net GND) (net VCC)))
"#;

fn category_board() -> RoutingBoard {
    match read_board(CATEGORY_DSN.as_bytes()) {
        BoardReadResult::Success { board, .. } => *board,
        _ => panic!("the category board must load"),
    }
}

fn net(board: &RoutingBoard, name: &str) -> i32 {
    board.rules.nets.get_by_name(name, 1).unwrap().net_number
}

/// The pin of net `name`.
fn pin_of(board: &RoutingBoard, name: &str) -> ItemId {
    let net = net(board, name);
    board
        .pins()
        .into_iter()
        .find(|id| board.item(*id).unwrap().net_number(0) == net)
        .unwrap()
}

/// The conduction area of net `name`.
fn plane_of(board: &RoutingBoard, name: &str) -> ItemId {
    let net = net(board, name);
    board
        .conduction_areas()
        .into_iter()
        .find(|id| board.item(*id).unwrap().net_number(0) == net)
        .unwrap()
}

/// Inserts a trace of net `name` on F.Cu between two points in um.
fn trace_of(
    board: &mut RoutingBoard,
    name: &str,
    from: (i32, i32),
    to: (i32, i32),
    fixed_state: FixedState,
) -> ItemId {
    let net = net(board, name);
    let class = board.rules.net_class_of(net).unwrap();
    let (half_width, clearance_class) = (class.trace_half_width(0), class.trace_clearance_class());
    let point = |(x, y): (i32, i32)| Point::Int(IntPoint::new(10 * x, 10 * y));
    board
        .insert_trace_without_cleaning(
            Polyline::from_corners(&point(from), &point(to)),
            0,
            half_width,
            &[net],
            clearance_class,
            fixed_state,
        )
        .unwrap()
}

/// The violation the clearance check of `first` finds with `second`.
fn violation_between(board: &RoutingBoard, first: ItemId, second: ItemId) -> ClearanceViolation {
    board
        .clearance_violations(first)
        .into_iter()
        .find(|v| v.second_item == second)
        .unwrap_or_else(|| panic!("no violation between {first:?} and {second:?}"))
}

fn assert_category(
    board: &RoutingBoard,
    v: &ClearanceViolation,
    unfixable: bool,
    category: ClearanceViolationCategory,
) {
    assert_eq!(v.is_unfixable(board), unfixable);
    assert_eq!(v.category(board), category);
}

#[test]
fn test_pin_to_pin_is_unfixable_and_categorized_correctly() {
    let board = category_board();
    let (pin1, pin2) = (pin_of(&board, "A"), pin_of(&board, "B"));
    let v = violation_between(&board, pin1, pin2);
    assert_category(&board, &v, true, ClearanceViolationCategory::PinToPin);
}

#[test]
fn test_pin_to_outline_is_unfixable_and_categorized_correctly() {
    let board = category_board();
    let pin = pin_of(&board, "C");
    let outline = board.outline().unwrap().id();
    let v = violation_between(&board, pin, outline);
    assert_category(
        &board,
        &v,
        true,
        ClearanceViolationCategory::PinToOutlineOrKeepout,
    );
}

/// A keepout without a net is no obstacle for a pin (in Java too), so the
/// keepout is a plane that is an obstacle (a Java `ConductionArea` is an
/// `ObstacleArea`). The keepout comes first, as in the Java test.
#[test]
fn test_pin_to_keepout_is_unfixable_and_categorized_correctly() {
    let mut board = category_board();
    let pin = pin_of(&board, "D");
    let keepout = plane_of(&board, "GND");
    assert!(board.clearance_violations(keepout).is_empty());
    board.set_conduction_is_obstacle(keepout, true);
    let v = violation_between(&board, keepout, pin);
    assert_category(
        &board,
        &v,
        true,
        ClearanceViolationCategory::PinToOutlineOrKeepout,
    );
}

/// A component outline has no shapes in the search tree, so no clearance
/// check reports it; the violation is built from the pin and its
/// component's outline, as Java builds it from mocks.
#[test]
fn test_pin_to_component_outline_is_unfixable_and_categorized_correctly() {
    let board = category_board();
    let pin = pin_of(&board, "A");
    let component = board.item(pin).unwrap().component_id();
    let outline = board
        .items()
        .find(|i| matches!(i.kind, ItemKind::ComponentOutline(_)) && i.component_id() == component)
        .expect("the pin's component has an outline")
        .id();
    assert!(board.tile_shape_count(board.item(outline).unwrap()) == 0);
    let v = ClearanceViolation {
        first_item: pin,
        second_item: outline,
        shape: TileShape::from(board.item(pin).unwrap().bounding_box()),
        layer: 0,
        expected_clearance: 100.0,
        actual_clearance: 50.0,
    };
    assert_category(
        &board,
        &v,
        true,
        ClearanceViolationCategory::PinToOutlineOrKeepout,
    );
}

#[test]
fn test_fixed_route_is_unfixable_and_categorized_correctly() {
    let mut board = category_board();
    let pin = pin_of(&board, "E");
    let fixed_trace = trace_of(
        &mut board,
        "T",
        (8000, -15000),
        (14000, -15000),
        FixedState::UserFixed,
    );
    let v = violation_between(&board, pin, fixed_trace);
    assert_category(&board, &v, true, ClearanceViolationCategory::FixedRoute);

    let padstack = board.library.padstacks.get_by_name("VIA").unwrap().id;
    let via_net = net(&board, "V");
    let via_class = board.item(fixed_trace).unwrap().clearance_class();
    let fixed_via = board.insert_via(
        padstack,
        IntPoint::new(130_000, -150_000),
        &[via_net],
        via_class,
        FixedState::UserFixed,
        false,
    );
    let v = violation_between(&board, fixed_via, fixed_trace);
    assert_category(&board, &v, true, ClearanceViolationCategory::FixedRoute);
}

#[test]
fn test_routable_trace_is_potentially_fixable() {
    let mut board = category_board();
    let pin = pin_of(&board, "H");
    let trace = trace_of(
        &mut board,
        "X",
        (14000, -8000),
        (16000, -8000),
        FixedState::Unfixed,
    );
    let v = violation_between(&board, pin, trace);
    assert_category(
        &board,
        &v,
        false,
        ClearanceViolationCategory::PotentiallyFixable,
    );
}

#[test]
fn test_two_routable_traces_are_potentially_fixable() {
    let mut board = category_board();
    let trace1 = trace_of(
        &mut board,
        "Y",
        (3000, -15000),
        (7000, -15000),
        FixedState::Unfixed,
    );
    let trace2 = trace_of(
        &mut board,
        "Z",
        (5000, -13000),
        (5000, -17000),
        FixedState::Unfixed,
    );
    let v = violation_between(&board, trace1, trace2);
    assert_category(
        &board,
        &v,
        false,
        ClearanceViolationCategory::PotentiallyFixable,
    );
}

/// Java's two mocks are neither pins, outlines, keepouts, traces nor vias.
/// Every real item that is no pin and no route is an outline or an area,
/// so the pair is two overlapping planes of different nets that are
/// obstacles: no pin, so not pin-to-outline, and no route.
#[test]
fn test_other_unfixable_items_categorized_correctly() {
    let mut board = category_board();
    let (gnd, vcc) = (plane_of(&board, "GND"), plane_of(&board, "VCC"));
    board.set_conduction_is_obstacle(gnd, true);
    board.set_conduction_is_obstacle(vcc, true);
    let v = violation_between(&board, gnd, vcc);
    assert_category(&board, &v, true, ClearanceViolationCategory::OtherUnfixable);
}
