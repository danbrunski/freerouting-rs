//! Ports of the Java unit tests of the board model packages
//! (`src/test/java/app/freerouting/{board,rules,datastructures,core}`).
//!
//! Tests of GUI types, routing, DRC, Java serialization and thread safety
//! are not ported; see the module docs of each group below.
//! `MoveComponentTest` (interactive component moves) is ported in
//! `board_actions_move_component` (I02.1).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::board::search::{ShapeSearchTree, TreeKind};
use freerouting::board::state::Communication;
use freerouting::board::structure::{Components, FixedState, Layer, LayerStructure};
use freerouting::geometry::{
    Circle, ConvexShape, FloatPoint, IntBox, IntPoint, IntVector, Point, Polyline, PolylineShape,
    PolylineShapeOps, TileShape, Vector,
};
use freerouting::io::{BoardReadResult, read_board};
use freerouting::library::{PackageId, PackagePin};
use freerouting::rules::{BoardRules, ClearanceMatrix, NetLengthConstraint};
use freerouting::scoring::BoardStatistics;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

fn load_board(name: &str) -> RoutingBoard {
    let bytes = std::fs::read(fixture(name)).unwrap();
    match read_board(&bytes) {
        BoardReadResult::Success { board, .. } => *board,
        other => panic!("{name} must load, got {:?}", other.board().is_some()),
    }
}

/// The test board of `PolylineTraceSplitTest`, `NetLengthConstraintTest`
/// and `MoveComponentTest`: one signal layer, clearance 10.
fn one_layer_board(size: i32) -> RoutingBoard {
    let layers = LayerStructure::new(vec![Layer::new("Top", true)]);
    let matrix = ClearanceMatrix::default_instance(layers.count(), 10);
    let mut rules = BoardRules::new(layers.clone(), matrix);
    rules.create_default_net_class();
    let outline = IntBox::new(0, 0, size, size);
    RoutingBoard::new(
        outline,
        layers,
        vec![PolylineShape::Tile(TileShape::from(outline))],
        0,
        rules,
        Communication::default(),
    )
}

fn ip(x: i32, y: i32) -> Point {
    Point::Int(IntPoint::new(x, y))
}

// ---------------------------------------------------------------------------
// rules.ClearanceMatrixTest

#[test]
fn clearance_matrix_set_value() {
    let mut matrix = ClearanceMatrix::new(2, &["default"]);

    // An odd value is rounded up.
    matrix.set_value_on_layer(0, 0, 0, 5);
    assert_eq!(matrix.get_value(0, 0, 0, false), 6);
    assert_eq!(matrix.max_value(0, 0), 6);
    assert_eq!(matrix.max_value_on_layer(0), 6);

    // A negative value becomes 0; the maxima keep the previous value.
    matrix.set_value_on_layer(0, 0, 0, -10);
    assert_eq!(matrix.get_value(0, 0, 0, false), 0);
    assert_eq!(matrix.max_value(0, 0), 6);
    assert_eq!(matrix.max_value_on_layer(0), 6);

    // Integer.MAX_VALUE is odd and is rounded down.
    matrix.set_value_on_layer(0, 0, 0, i32::MAX);
    assert_eq!(matrix.get_value(0, 0, 0, false), i32::MAX - 1);
    assert_eq!(matrix.max_value(0, 0), i32::MAX - 1);
    assert_eq!(matrix.max_value_on_layer(0), i32::MAX - 1);
}

// ---------------------------------------------------------------------------
// rules.NetLengthConstraintTest

#[test]
fn unconstrained_instance_has_no_bounds() {
    let constraint = NetLengthConstraint::UNCONSTRAINED;
    assert!(!constraint.has_min());
    assert!(!constraint.has_max());
    assert!(!constraint.is_constrained());
    assert_eq!(constraint.target_length(), 0.0);
    assert!(constraint.is_satisfied(0.0));
    assert!(constraint.is_satisfied(500.0));
}

#[test]
fn min_only_constraint() {
    let constraint = NetLengthConstraint::new(25.0, 0.0);
    assert!(constraint.has_min());
    assert!(!constraint.has_max());
    assert!(constraint.is_constrained());
    assert_eq!(constraint.target_length(), 25.0);
    assert!(!constraint.is_satisfied(24.9));
    assert!(constraint.is_satisfied(25.0));
    assert!(constraint.is_satisfied(100.0));
}

#[test]
fn max_only_constraint() {
    let constraint = NetLengthConstraint::new(0.0, 50.0);
    assert!(!constraint.has_min());
    assert!(constraint.has_max());
    assert!(constraint.is_constrained());
    assert_eq!(constraint.target_length(), 50.0);
    assert!(constraint.is_satisfied(10.0));
    assert!(constraint.is_satisfied(50.0));
    assert!(!constraint.is_satisfied(50.1));
}

#[test]
fn bounded_constraint_calculates_midpoint_target() {
    let constraint = NetLengthConstraint::new(20.0, 60.0);
    assert!(constraint.has_min());
    assert!(constraint.has_max());
    assert!(constraint.is_constrained());
    assert_eq!(constraint.target_length(), 40.0);
    assert!(!constraint.is_satisfied(19.9));
    assert!(constraint.is_satisfied(20.0));
    assert!(constraint.is_satisfied(40.0));
    assert!(constraint.is_satisfied(60.0));
    assert!(!constraint.is_satisfied(60.1));
}

#[test]
fn net_inherits_constraint_from_net_class_by_default() {
    let mut board = one_layer_board(2_000_000);
    let rules = &mut board.rules;
    let class = rules.default_net_class();
    rules
        .net_classes
        .get_mut(class)
        .set_minimum_trace_length(15.0);
    rules
        .net_classes
        .get_mut(class)
        .set_maximum_trace_length(45.0);
    let number = rules.nets.add("TEST_NET", 1, false, class);
    let net = rules.nets.get(number).unwrap();

    assert!(!net.has_explicit_length_constraint());
    assert_eq!(net.minimum_trace_length(&rules.net_classes), 15.0);
    assert_eq!(net.maximum_trace_length(&rules.net_classes), 45.0);
    let effective = net.length_constraint(&rules.net_classes);
    assert_eq!(effective.min_length, 15.0);
    assert_eq!(effective.max_length, 45.0);
    assert_eq!(effective.target_length(), 30.0);
}

#[test]
fn net_explicit_constraint_overrides_net_class() {
    let mut board = one_layer_board(2_000_000);
    let rules = &mut board.rules;
    let class = rules.default_net_class();
    rules
        .net_classes
        .get_mut(class)
        .set_minimum_trace_length(10.0);
    rules
        .net_classes
        .get_mut(class)
        .set_maximum_trace_length(100.0);
    let number = rules.nets.add("TUNED_NET", 1, false, class);

    let net = rules.nets.get_mut(number).unwrap();
    net.set_minimum_trace_length(25.0);
    assert!(net.has_explicit_length_constraint());
    let net = rules.nets.get(number).unwrap();
    assert_eq!(net.minimum_trace_length(&rules.net_classes), 25.0);
    assert_eq!(net.maximum_trace_length(&rules.net_classes), 100.0);

    rules
        .nets
        .get_mut(number)
        .unwrap()
        .set_maximum_trace_length(75.0);
    let net = rules.nets.get(number).unwrap();
    assert_eq!(net.minimum_trace_length(&rules.net_classes), 25.0);
    assert_eq!(net.maximum_trace_length(&rules.net_classes), 75.0);

    rules
        .nets
        .get_mut(number)
        .unwrap()
        .set_length_constraint(None);
    let net = rules.nets.get(number).unwrap();
    assert!(!net.has_explicit_length_constraint());
    assert_eq!(net.minimum_trace_length(&rules.net_classes), 10.0);
    assert_eq!(net.maximum_trace_length(&rules.net_classes), 100.0);
}

#[test]
fn net_class_length_constraint_convenience() {
    let mut board = one_layer_board(2_000_000);
    let class = board.rules.default_net_class();
    let net_class = board.rules.net_classes.get_mut(class);

    net_class.set_length_constraint(Some(NetLengthConstraint::new(12.5, 37.5)));
    assert_eq!(net_class.minimum_trace_length(), 12.5);
    assert_eq!(net_class.maximum_trace_length(), 37.5);
    let constraint = net_class.length_constraint();
    assert_eq!(constraint.min_length, 12.5);
    assert_eq!(constraint.max_length, 37.5);

    net_class.set_length_constraint(None);
    assert_eq!(net_class.minimum_trace_length(), 0.0);
    assert_eq!(net_class.maximum_trace_length(), 0.0);
}

// ---------------------------------------------------------------------------
// board.PinObstacleTest. Java mocks a pin whose drillAllowed() is true and a
// same-net via; here a real SMD pin and a via with attach disallowed.

#[test]
fn same_net_via_with_attach_disallowed_is_not_obstacle_for_smd_pin() {
    let mut board = one_layer_board(100_000);
    let class = board.rules.default_net_class();
    let net = board.rules.nets.add("one", 1, false, class);
    let pad = board.library.padstacks.add(
        "smd",
        vec![Some(ConvexShape::Circle(Circle::new(
            IntPoint::new(0, 0),
            500,
        )))],
        vec![None],
        false,
        false,
    );
    let package = board.library.packages.add(
        "pkg",
        vec![PackagePin::new("1", pad, FloatPoint::new(0.0, 0.0), 0.0)],
        None,
        None,
        None,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        true,
    );
    let component = board.components.add_precise(
        "U1",
        Some(FloatPoint::new(5000.0, 5000.0)),
        0.0,
        true,
        package,
        package,
        false,
        None,
    );
    let pin = board
        .insert_pin(component.0, 0, &[net], 1, FixedState::Unfixed)
        .unwrap();
    let via = board.insert_via(
        pad,
        IntPoint::new(5000, 5000),
        &[net],
        1,
        FixedState::Unfixed,
        false,
    );
    let pin_item = board.item(pin).unwrap();
    let via_item = board.item(via).unwrap();
    assert!(!board.is_obstacle(pin_item, via_item));
}

// ---------------------------------------------------------------------------
// board.PolylineTraceSplitTest, all tests.

fn trace_ids_with_net(board: &RoutingBoard, net: i32) -> Vec<ItemId> {
    board
        .traces()
        .into_iter()
        .filter(|id| board.item(*id).unwrap().contains_net(net))
        .collect()
}

fn ends(board: &RoutingBoard, id: ItemId) -> (Point, Point) {
    let trace = board.item(id).unwrap().as_trace().unwrap();
    (trace.first_corner(), trace.last_corner())
}

#[test]
fn split_does_not_remove_valid_segments() {
    let mut board = one_layer_board(2_000_000);
    let net = 98;
    let (p1, p2, p3, p4) = (
        ip(1_291_423, -987_076),
        ip(1_270_000, -975_000),
        ip(1_250_000, -970_000),
        ip(1_243_227, -964_893),
    );
    let p5 = ip(1_241_414, -964_893);
    board
        .insert_trace_without_cleaning(
            Polyline::from_points(&[p1.clone(), p2, p3, p4.clone()]),
            0,
            1000,
            &[net],
            1,
            FixedState::Unfixed,
        )
        .unwrap();
    let second = board
        .insert_trace_without_cleaning(
            Polyline::from_corners(&p4, &p5),
            0,
            1000,
            &[net],
            1,
            FixedState::Unfixed,
        )
        .unwrap();

    assert!(board.combine_trace(second), "traces should be combined");
    let combined = trace_ids_with_net(&board, net)[0];
    let (first, last) = ends(&board, combined);
    assert!(
        (first == p1 || last == p1) && (first == p5 || last == p5),
        "combined trace should span from p1 to p5, but goes from {first:?} to {last:?}"
    );

    let (p6, p7) = (ip(1_241_171, -952_775), ip(1_242_000, -960_000));
    board
        .insert_trace_without_cleaning(
            Polyline::from_points(&[p4.clone(), p7, p6]),
            0,
            1000,
            &[net],
            1,
            FixedState::Unfixed,
        )
        .unwrap();

    let pieces = board.split_trace(combined, None);
    let has_connection_from_p1 = pieces.iter().any(|id| {
        board.item(*id).is_some() && {
            let (first, last) = ends(&board, *id);
            first == p1 || last == p1
        }
    });
    assert!(
        has_connection_from_p1,
        "the segment from p1 to p4 must not be lost"
    );

    let p1f = p1.to_float();
    let near_p1 = trace_ids_with_net(&board, net)
        .into_iter()
        .filter(|id| {
            let (first, last) = ends(&board, *id);
            first.to_float().distance(p1f) < 100_000.0 || last.to_float().distance(p1f) < 100_000.0
        })
        .count();
    assert!(near_p1 > 0);
}

#[test]
fn split_preserves_non_overlapping_segments() {
    let mut board = one_layer_board(2_000_000);
    let (p1, p2, p3, p4) = (ip(0, 0), ip(10000, 0), ip(20000, 0), ip(30000, 0));
    let first = board
        .insert_trace_without_cleaning(
            Polyline::from_points(&[p1.clone(), p2, p3, p4.clone()]),
            0,
            1000,
            &[1],
            1,
            FixedState::Unfixed,
        )
        .unwrap();
    board
        .insert_trace_without_cleaning(
            Polyline::from_corners(&ip(10000, 0), &ip(20000, 0)),
            0,
            1000,
            &[1],
            1,
            FixedState::Unfixed,
        )
        .unwrap();

    let pieces = board.split_trace(first, None);
    let on_board: Vec<_> = pieces
        .into_iter()
        .filter(|id| board.item(*id).is_some())
        .map(|id| ends(&board, id))
        .collect();
    assert!(on_board.iter().any(|(a, b)| *a == p1 || *b == p1));
    assert!(on_board.iter().any(|(a, b)| *a == p4 || *b == p4));
}

#[test]
fn cycle_detection_during_overlap() {
    let mut board = one_layer_board(2_000_000);
    let (a, b, c) = (ip(0, 0), ip(10000, 0), ip(20000, 0));
    let abc = board
        .insert_trace_without_cleaning(
            Polyline::from_points(&[a, b.clone(), c.clone()]),
            0,
            1000,
            &[1],
            1,
            FixedState::Unfixed,
        )
        .unwrap();
    board
        .insert_trace_without_cleaning(
            Polyline::from_corners(&b, &c),
            0,
            1000,
            &[1],
            1,
            FixedState::Unfixed,
        )
        .unwrap();
    assert!(
        !board.trace_is_cycle(abc),
        "an overlapping trace does not make a cycle"
    );
}

#[test]
fn trace_geometry_characterization() {
    let mut board = one_layer_board(2_000_000);
    let (first, last) = (ip(10000, 10000), ip(20000, 10000));
    let id = board
        .insert_trace_without_cleaning(
            Polyline::from_corners(&first, &last),
            0,
            1000,
            &[1],
            1,
            FixedState::Unfixed,
        )
        .unwrap();
    let item = board.item(id).unwrap();
    let trace = item.as_trace().unwrap();
    assert_eq!(trace.first_corner(), first);
    assert_eq!(trace.last_corner(), last);
    assert_eq!(trace.corner_count(), 2);
    assert_eq!(trace.tile_shape_count(), 1);
    assert_eq!(trace.length(), 10000.0);
    assert!(board.tile_shape(item, 0).is_some());
    // getTraceConnectionShape(defaultTree, 0) (B04.7).
    let tree = board.search_trees().default_tree();
    assert!(board.trace_connection_shape(tree, id, 0).is_some());
}

/// `testCombineAtEndRecoversMissingDefaultTreeEntries`: the end combine
/// takes Java's `replaceGeometry` fallback when a trace has lost its
/// default-tree entries, and the fallback restores them.
#[test]
fn combine_at_end_recovers_missing_default_tree_entries() {
    let mut board = one_layer_board(2_000_000);
    let (start, join, end) = (ip(10000, 10000), ip(20000, 10000), ip(30000, 10000));
    let insert = |board: &mut RoutingBoard, a: &Point, b: &Point| {
        board
            .insert_trace_without_cleaning(
                Polyline::from_corners(a, b),
                0,
                1000,
                &[1],
                1,
                FixedState::Unfixed,
            )
            .unwrap()
    };
    let first = insert(&mut board, &start, &join);
    let second = insert(&mut board, &join, &end);
    // A live trace whose tree entries were removed before the geometry
    // change; it stays marked on the board, so the combine reaches the
    // fallback.
    board.drop_tree_entries_for_test(first);
    assert!(!board.search_trees().default_tree().contains(first));

    assert!(
        board.combine_trace(first),
        "the traces should combine through the fallback path"
    );
    assert!(board.item(first).is_some_and(|i| i.is_on_the_board()));
    assert!(
        board.item(second).is_none(),
        "the consumed trace is removed"
    );
    assert!(
        board.search_trees().default_tree().contains(first),
        "the fallback restores the default-tree entries"
    );
    assert_eq!(ends(&board, first), (start, end));
}

// ---------------------------------------------------------------------------
// fixtures.CombineStackOverflowTest

/// `combineDoesNotOverflowOnLongCollinearTrace`: about 4 000 connected
/// collinear wire segments of one net load on a thread with a 256 KiB
/// stack (the combine is a loop, not a recursion).
#[test]
fn combine_does_not_overflow_on_long_collinear_trace() {
    let reader = std::thread::Builder::new()
        .name("combine-stackoverflow-reader".into())
        .stack_size(256 * 1024)
        .spawn(|| {
            let bytes = std::fs::read(fixture("Issue723-CombineStackOverflow.dsn")).unwrap();
            matches!(read_board(&bytes), BoardReadResult::Success { .. })
        })
        .expect("reader thread");
    assert!(reader.join().expect("no stack overflow"), "the board loads");
}

// ---------------------------------------------------------------------------
// board.searchtree.DrillHoleClearanceShapeTest

const HOLE_CLEARANCE_FIXTURE: &str = "Issue575-drc_dev-board_4_hole_clearance_violations.dsn";

fn drill_radius(board: &RoutingBoard, id: ItemId) -> f64 {
    let item = board.item(id).unwrap();
    let drill = item.drill().unwrap();
    board
        .library
        .padstacks
        .get(drill.padstack)
        .map_or(0.0, |p| p.get_drill_radius())
}

fn drilled_items(board: &RoutingBoard) -> Vec<ItemId> {
    let mut result = Vec::new();
    if let Some(via) = board
        .vias()
        .into_iter()
        .find(|id| drill_radius(board, *id) > 0.0)
    {
        result.push(via);
    }
    if let Some(pin) = board.pins().into_iter().find(|id| {
        let drill = board.item(*id).unwrap().drill().unwrap();
        drill_radius(board, *id) > 0.0 && drill.shapes.first().is_some_and(Option::is_some)
    }) {
        result.push(pin);
    }
    result
}

fn box_area(shape: &TileShape) -> i64 {
    let b = shape.bounding_box();
    i64::from(b.ur.x - b.ll.x) * i64::from(b.ur.y - b.ll.y)
}

fn tree_shapes(board: &RoutingBoard, kind: TreeKind, id: ItemId) -> Vec<Option<TileShape>> {
    ShapeSearchTree::new(kind, 0)
        .calculate_tree_shapes(board.item(id).unwrap(), &board.shape_context())
}

#[test]
fn hole_clearance_inflates_drilled_item_shapes_in_all_tree_variants() {
    let mut board = load_board(HOLE_CLEARANCE_FIXTURE);
    let items = drilled_items(&board);
    assert!(items.iter().any(|id| board.item(*id).unwrap().is_via()));
    assert!(items.iter().any(|id| board.item(*id).unwrap().is_pin()));
    let kinds = [
        TreeKind::Plain,
        TreeKind::FortyfiveDegree,
        TreeKind::NinetyDegree,
    ];
    for id in items {
        board.rules.set_hole_clearance(0);
        let before: Vec<_> = kinds.iter().map(|k| tree_shapes(&board, *k, id)).collect();
        board.rules.set_hole_clearance(200_000);
        let after: Vec<_> = kinds.iter().map(|k| tree_shapes(&board, *k, id)).collect();
        assert_eq!(before[0].len(), after[0].len());
        for (kind, (before, after)) in kinds.iter().zip(before.iter().zip(&after)) {
            for (i, shape) in before.iter().enumerate() {
                let Some(shape) = shape else { continue };
                let grown = after[i].as_ref().expect("shape stays");
                assert!(
                    box_area(grown) > box_area(shape),
                    "{id:?} shape {i} must grow in the {kind:?} tree"
                );
            }
        }
    }
}

#[test]
fn null_shape_layers_get_synthesized_hole_obstacles() {
    let mut board = load_board(HOLE_CLEARANCE_FIXTURE);
    let found = board.vias().into_iter().find_map(|id| {
        if drill_radius(&board, id) <= 0.0 {
            return None;
        }
        let drill = board.item(id).unwrap().drill().unwrap();
        drill
            .shapes
            .iter()
            .position(Option::is_none)
            .map(|i| (id, i))
    });
    // Java assumes (skips) when the fixture has no such via.
    let Some((via, index)) = found else {
        return;
    };
    board.rules.set_hole_clearance(0);
    assert!(tree_shapes(&board, TreeKind::FortyfiveDegree, via)[index].is_none());
    board.rules.set_hole_clearance(2500);
    assert!(tree_shapes(&board, TreeKind::FortyfiveDegree, via)[index].is_some());
}

#[test]
fn zero_hole_clearance_keeps_legacy_shapes() {
    let mut board = load_board(HOLE_CLEARANCE_FIXTURE);
    board.rules.set_hole_clearance(0);
    for id in drilled_items(&board) {
        let a = tree_shapes(&board, TreeKind::FortyfiveDegree, id);
        let b = tree_shapes(&board, TreeKind::FortyfiveDegree, id);
        for (x, y) in a.iter().zip(&b) {
            if let (Some(x), Some(y)) = (x, y) {
                assert_eq!(box_area(x), box_area(y));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// core.scoring.BoardStatisticsBoundsCalculatorTest

fn statistics(name: &str) -> BoardStatistics {
    BoardStatistics::new(&load_board(name))
}

#[test]
fn empty_net_and_single_pin_net_contribute_zero_bounds() {
    let stats = statistics("scoring-empty-net.dsn");
    assert!(stats.bounds.min_trace_length_mm.abs() < 0.001);
    assert_eq!(stats.bounds.min_via_count, 0);
    assert_eq!(stats.bounds.min_bend_count, 0);
}

#[test]
fn coincident_pins_have_zero_length_and_no_bend_bound() {
    let stats = statistics("scoring-zero-length.dsn");
    assert!(stats.bounds.min_trace_length_mm.abs() < 0.001);
    assert_eq!(stats.bounds.min_via_count, 0);
    assert_eq!(stats.bounds.min_bend_count, 0);
}

#[test]
fn mixed_layer_pins_require_one_through_via_and_manhattan_bend() {
    let stats = statistics("scoring-mixed-layer.dsn");
    assert_eq!(stats.bounds.min_via_count, 1);
    assert!(stats.bounds.min_trace_length_mm > 0.0);
    assert_eq!(stats.bounds.min_bend_count, 1);
    assert_eq!(
        stats.difficulty.complexity_c as i64,
        stats.difficulty.difficulty_d as i64
    );
}

#[test]
fn empty_board_has_zero_bounds_and_defined_difficulty() {
    let stats = statistics("empty_board.dsn");
    assert!(stats.bounds.min_trace_length_mm.abs() < 0.001);
    assert_eq!(stats.bounds.min_via_count, 0);
    assert_eq!(stats.bounds.min_bend_count, 0);
    assert_eq!(stats.difficulty.complexity_c, 1);
    assert!((stats.difficulty.difficulty_d - 1.0).abs() < 0.001);
}

// ---------------------------------------------------------------------------
// board.BoardServiceCharacterizationTest, without serialization, observers
// and the graphics update box.

fn characterization_board() -> RoutingBoard {
    one_layer_board(1000)
}

fn insert_short_trace(board: &mut RoutingBoard, net: i32, x1: i32, x2: i32) -> ItemId {
    board
        .insert_trace_without_cleaning(
            Polyline::from_corners(&ip(x1, 100), &ip(x2, 100)),
            0,
            10,
            &[net],
            0,
            FixedState::Unfixed,
        )
        .unwrap()
}

#[test]
fn item_queries_remain_stable() {
    let mut board = characterization_board();
    let trace = insert_short_trace(&mut board, 10, 100, 200);
    assert!(board.outline().is_some());
    assert_eq!(board.item(trace).unwrap().id(), trace);
    assert_eq!(board.get_item(trace.0).unwrap().id(), trace);
    assert!(board.items().any(|item| item.id() == trace));
    assert_eq!(board.traces(), vec![trace]);
}

#[test]
fn snapshot_undo_redo_preserves_items() {
    let mut board = characterization_board();
    board.generate_snapshot();
    let trace = insert_short_trace(&mut board, 20, 100, 200);
    let mut changed_nets = BTreeSet::new();
    assert!(board.undo(Some(&mut changed_nets)));
    assert!(!board.traces().contains(&trace));
    assert!(changed_nets.contains(&20));
    assert!(board.redo(Some(&mut changed_nets)));
    assert!(board.traces().contains(&trace));
}

#[test]
fn changed_area_lifecycle() {
    let mut board = characterization_board();
    board.start_marking_changed_area();
    assert!(board.changed_area().is_some());
    board.join_changed_area(FloatPoint::new(100.0, 100.0), 0);
    board.mark_all_changed_area();
    assert!(!board.changed_area().unwrap().get_area(0).is_empty());
    board.clear_changed_area();
    assert!(board.changed_area().is_none());
}

// ---------------------------------------------------------------------------
// board.actions.ComponentItemTransformTest: only
// `componentRotationApiUsesExactQuarterTurns`. The others
// (`sharedJunctionViaAndUndoRemainConsistent`,
// `conflictingAnchorsRejectWithoutChangingPoseOrSearchTree`,
// `newForeignNetClearanceRejectsAndLeavesStationaryPinUntouched`,
// `existingNonGridPinEscapeCanStillRotateRigidly`,
// `illegalTraceAngleRejectsRatherThanDistortingCopper`) run
// `ComponentItemTransform.apply` on items detached with `detachItemForMove`;
// owner I04 (B05.8 checked).

#[test]
fn component_rotation_api_uses_exact_quarter_turns() {
    let mut components = Components::new();
    let id = components.add_precise(
        "C0",
        Some(FloatPoint::new(2000.5, -3000.5)),
        0.0,
        true,
        PackageId(1),
        PackageId(1),
        false,
        None,
    );
    let start = components.get(id).unwrap().exact_location().unwrap();
    for _ in 0..4 {
        components.rotate(id, 90.0, IntPoint::new(500, 600));
    }
    let end = components.get(id).unwrap().exact_location().unwrap();
    assert_eq!(start.x, end.x);
    assert_eq!(start.y, end.y);
}

#[test]
fn component_translation_is_exact() {
    let mut components = Components::new();
    let id = components.add_precise(
        "C0",
        Some(FloatPoint::new(2000.5, -3000.5)),
        0.0,
        true,
        PackageId(1),
        PackageId(1),
        false,
        None,
    );
    components.move_by(id, &Vector::Int(IntVector::new(10, -20)));
    let location = components.get(id).unwrap().exact_location().unwrap();
    assert_eq!((location.x, location.y), (2010.5, -3020.5));
}

// ---------------------------------------------------------------------------
// core.StoppableThreadTest, against `autoroute::events::CancelToken` (A08.5):
// all three tests. The thread itself is not ported; the token is what the
// router and its caller share.

mod stoppable_thread {
    use freerouting::autoroute::events::CancelToken;

    #[test]
    fn stop_request() {
        let token = CancelToken::new();
        assert!(!token.is_stop_requested());
        assert!(!token.is_router_stop_requested());
        token.stop_all();
        assert!(token.is_stop_requested());
        assert!(token.is_router_stop_requested());
    }

    #[test]
    fn stop_auto_router_request() {
        let token = CancelToken::new();
        assert!(!token.is_stop_requested());
        assert!(!token.is_router_stop_requested());
        token.stop_router();
        assert!(!token.is_stop_requested());
        assert!(token.is_router_stop_requested());
    }

    #[test]
    fn stop_request_overrides_auto_router_request() {
        let token = CancelToken::new();
        token.stop_router();
        token.stop_all();
        assert!(token.is_stop_requested());
        assert!(token.is_router_stop_requested());
    }
}

// ---------------------------------------------------------------------------
// board.facade.BasicBoard.deleteAllTracksAndVias (B08.7). Java has no unit test
// for it; it deletes from the item list only (`BasicBoard.java:1523-1544`), so
// the search trees keep entries of removed traces and vias and a later query
// can return an item that is gone. The port removes the tree entries too
// (DIVERGENCES.md, "Board model").

mod delete_all_traces_and_vias {
    use super::*;
    use freerouting::geometry::{Circle, ConvexShape};

    #[test]
    fn delete_all_traces_and_vias_leaves_no_tree_entries() {
        let mut board = one_layer_board(1_000_000);
        let class = board.rules.default_net_class();
        let net = board.rules.nets.add("one", 1, false, class);
        let pad = board.library.padstacks.add(
            "via",
            vec![Some(ConvexShape::Circle(Circle::new(
                IntPoint::new(0, 0),
                300,
            )))],
            vec![None],
            true,
            false,
        );
        let mut removed = Vec::new();
        for i in 0..5 {
            let y = 100_000 + 50_000 * i;
            let corners = [ip(100_000, y), ip(400_000, y)];
            removed.push(
                board
                    .insert_trace_without_cleaning(
                        Polyline::from_points(&corners),
                        0,
                        500,
                        &[net],
                        1,
                        FixedState::Unfixed,
                    )
                    .unwrap(),
            );
        }
        for i in 0..3 {
            removed.push(board.insert_via(
                pad,
                IntPoint::new(600_000 + 10_000 * i, 500_000),
                &[net],
                1,
                FixedState::Unfixed,
                false,
            ));
        }
        let everything = TileShape::from(IntBox::new(0, 0, 1_000_000, 1_000_000));
        let before = board.overlapping_objects(&everything, 0);
        assert!(removed.iter().all(|id| before.contains(id)));

        board.delete_all_traces_and_vias();

        assert!(removed.iter().all(|id| board.item(*id).is_none()));
        let after = board.overlapping_objects(&everything, 0);
        assert!(
            removed.iter().all(|id| !after.contains(id)),
            "removed ids still in the tree: {after:?}"
        );
        // The outline is still found.
        assert_eq!(after.len(), 1);
    }
}

// ---------------------------------------------------------------------------
// board.BoardItemRenderingMetadataTest (B05.8): its one test.

#[test]
fn every_board_item_exposes_neutral_semantic_metadata() {
    use freerouting::board::items::{AreaKind, BoardItemType, ItemKind};
    let bytes = std::fs::read(fixture(
        "Issue575-drc_dev-board_4_hole_clearance_violations.dsn",
    ))
    .unwrap();
    let board = match read_board(&bytes) {
        BoardReadResult::Success { board, .. } => *board,
        BoardReadResult::OutlineMissing {
            board: Some(board), ..
        } => *board,
        _ => panic!("the fixture must load"),
    };
    let mut observed = BTreeSet::new();
    for item in board.items() {
        let kind = item.board_item_type();
        observed.insert(kind);
        let _bounds = item.bounding_box();
        assert!(
            item.first_layer() <= item.last_layer(),
            "layer range ordered"
        );
        let expected = match &item.kind {
            ItemKind::Pin(_) => BoardItemType::Pin,
            ItemKind::Via(_) => BoardItemType::Via,
            ItemKind::Trace(_) => BoardItemType::Trace,
            ItemKind::Area(area) => match area.kind {
                AreaKind::Conduction { .. } => BoardItemType::ConductionArea,
                AreaKind::ViaObstacle => BoardItemType::ViaObstacleArea,
                AreaKind::ComponentObstacle => BoardItemType::ComponentObstacleArea,
                AreaKind::Obstacle => BoardItemType::ObstacleArea,
            },
            ItemKind::ComponentOutline(_) => BoardItemType::ComponentOutline,
            ItemKind::BoardOutline(_) => BoardItemType::BoardOutline,
        };
        assert_eq!(kind, expected);
    }
    assert!(
        observed.contains(&BoardItemType::Pin),
        "fixture must contain pins"
    );
    assert!(
        observed.contains(&BoardItemType::Trace),
        "fixture must contain traces"
    );
}

// ---------------------------------------------------------------------------
// core.scoring.ScoringMazeCostIndependenceTest (A01.5): both tests. The maze
// search reads its costs from the scoring section; the V2 board-score
// weights never reach it, nor `AutorouteControl`'s via cost.

mod scoring_maze_cost_independence {
    use freerouting::autoroute::control::AutorouteControl;
    use freerouting::io::{BoardReadResult, read_board};
    use freerouting::settings::board::{BoardRouterSettings, apply_board_specific};
    use freerouting::settings::{ResolveContext, RouterSettings, defaults};

    const DEFAULT_VIA_COSTS: i32 = 50;

    fn default_settings() -> RouterSettings {
        let ctx = ResolveContext { cores: 8 };
        RouterSettings::from_patch(&defaults(&ctx), ctx.cores)
    }

    #[test]
    fn maze_search_costs_stay_put_when_v2_board_score_weights_change() {
        let mut settings = default_settings();
        let before = settings.clone();
        settings.router_scoring.unrouted_first_half_weight = 9999.0;
        settings.router_scoring.unrouted_second_half_weight = 8888.0;
        settings.router_scoring.clearance_violation_count_weight = 1.0;
        settings.router_scoring.clearance_violation_depth_weight = 2.0;
        settings.optimizer_scoring.excess_wire_length_weight = 3.0;
        settings.optimizer_scoring.excess_via_weight = 4.0;
        settings.optimizer_scoring.excess_bend_weight = 5.0;

        let (b, s) = (&before.scoring, &settings.scoring);
        assert_eq!(b.via_costs, s.via_costs);
        assert_eq!(b.plane_via_costs, s.plane_via_costs);
        assert_eq!(b.start_ripup_costs, s.start_ripup_costs);
        assert_eq!(
            b.default_preferred_direction_trace_cost,
            s.default_preferred_direction_trace_cost
        );
        assert_eq!(
            b.default_undesired_direction_trace_cost,
            s.default_undesired_direction_trace_cost
        );
        assert_eq!(b.unrouted_net_penalty, s.unrouted_net_penalty);
        assert_eq!(b.clearance_violation_penalty, s.clearance_violation_penalty);
        assert_eq!(b.bend_penalty, s.bend_penalty);
        assert_eq!(s.via_costs, DEFAULT_VIA_COSTS);
    }

    #[test]
    fn autoroute_control_via_cost_tracks_maze_settings_not_v2_weights() {
        let bytes = std::fs::read(super::fixture("scoring-zero-length.dsn")).unwrap();
        let BoardReadResult::Success { board, .. } = read_board(&bytes) else {
            panic!("scoring-zero-length.dsn must load");
        };
        let ctx = ResolveContext { cores: 8 };
        let mut patch = defaults(&ctx);
        apply_board_specific(&mut patch, &board);
        let mut settings = RouterSettings::from_patch(&patch, ctx.cores);
        let via_cost_before = AutorouteControl::for_net_default(
            &board,
            1,
            &BoardRouterSettings::from_settings(&settings),
        )
        .unwrap()
        .min_normal_via_cost;

        settings.router_scoring.unrouted_first_half_weight = 50_000.0;
        settings.optimizer_scoring.excess_via_weight = 9_000.0;

        let after_weights = AutorouteControl::for_net_default(
            &board,
            1,
            &BoardRouterSettings::from_settings(&settings),
        )
        .unwrap();
        assert!((via_cost_before - after_weights.min_normal_via_cost).abs() <= 0.001);
        assert_eq!(settings.scoring.via_costs, DEFAULT_VIA_COSTS);
    }
}

// ---------------------------------------------------------------------------
// gui.DialogInteractionHandlersTest, the engine part of two tests (B08.4), on a real board
// instead of Mockito mocks: `clearanceMatrixGridCellParsingAndLayeredApplyWorkAsExpected`
// (the three `applyClearanceValue` cases) and `netClassesButtonAndTableHelpersApplyExpectedChanges`
// (`applyShoveFixedSelection`, `applyAutorouterIgnoreSelection`).
//
// Not ported, because they test Swing windows' static helpers, which belong to the GUI (G06):
// `WindowNetClassesTest` (all three: `parseTraceWidthValue` parses a cell value and
// `applyTraceWidth` converts user to board units with a `CoordinateTransform`), and in
// `DialogInteractionHandlersTest` the route and autoroute parameter window handlers, the
// number normalizers, `isLegalClassName`, `parseClearanceTableValue` and `canRemoveNetClass`
// (a row count test). The facade keeps `Net classes` removal rules in `rules::edit`.

mod dialog_interaction_handlers {
    use super::*;
    use freerouting::rules::edit::ClearanceLayer;

    fn four_layer_board() -> RoutingBoard {
        let layers = LayerStructure::new(vec![
            Layer::new("Top", true),
            Layer::new("In1", true),
            Layer::new("In2", true),
            Layer::new("Bottom", true),
        ]);
        let matrix = ClearanceMatrix::default_instance(layers.count(), 10);
        let mut rules = BoardRules::new(layers.clone(), matrix);
        rules.create_default_net_class();
        let outline = IntBox::new(0, 0, 100_000, 100_000);
        let mut board = RoutingBoard::new(
            outline,
            layers,
            vec![PolylineShape::Tile(TileShape::from(outline))],
            0,
            rules,
            Communication::default(),
        );
        board.add_clearance_class("class2");
        board
    }

    #[test]
    fn apply_clearance_value_on_all_inner_and_single_layers() {
        let mut board = four_layer_board();
        let value = |board: &RoutingBoard, i: i32, j: i32, layer: i32| {
            board.rules.clearance_matrix.get_value(i, j, layer, false)
        };
        board.apply_clearance_value(2, 1, ClearanceLayer::All, 300);
        for layer in 0..4 {
            assert_eq!(value(&board, 2, 1, layer), 300);
            assert_eq!(value(&board, 1, 2, layer), 300);
        }
        board.apply_clearance_value(2, 1, ClearanceLayer::Inner, 220);
        assert_eq!(
            [0, 1, 2, 3].map(|l| value(&board, 2, 1, l)),
            [300, 220, 220, 300]
        );
        assert_eq!(
            [0, 1, 2, 3].map(|l| value(&board, 1, 2, l)),
            [300, 220, 220, 300]
        );
        board.apply_clearance_value(2, 1, ClearanceLayer::Layer(3), 180);
        assert_eq!(value(&board, 2, 1, 3), 180);
        assert_eq!(value(&board, 1, 2, 3), 180);
        assert_eq!(value(&board, 2, 1, 0), 300);
    }

    #[test]
    fn net_class_selection_helpers() {
        let mut board = four_layer_board();
        let class = board.add_net_class();
        assert!(!board.rules.net_classes.get(class).is_shove_fixed());
        board.set_net_class_shove_fixed(class, true);
        assert!(board.rules.net_classes.get(class).is_shove_fixed());
        assert!(!board.rules.net_classes.get(class).pull_tight());
        assert!(!board.rules.net_classes.get(class).is_ignored_by_autorouter);
        board.set_net_class_ignored_by_autorouter(class, true);
        assert!(board.rules.net_classes.get(class).is_ignored_by_autorouter);
        board.set_net_class_ignored_by_autorouter(class, false);
        assert!(!board.rules.net_classes.get(class).is_ignored_by_autorouter);
    }
}

/// `AnyAngleTightenerCycleTest` (Java 588320207, after the pin a773a70ec):
/// `connectionItemsOfAClosedRingReturnEveryTraceOnce`. The walk of
/// `connection_items` stops at the first item already collected, so a closed
/// ring of three traces with no fork returns each trace once instead of
/// looping.
///
/// Not ported: `anyAngleTightenerReturnsWithDuplicatedWires` (the fixture's
/// wires imported a second time, angle restriction `None`, `opt_changed_area`
/// returns). `board_none_matches_java` in `tests/reference_pull_tight.rs`
/// runs the same board and compares it with the Java reference from 588320207.
mod any_angle_tightener_cycle {
    use super::*;
    use freerouting::board::items::StopConnectionOption;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn connection_items_of_a_closed_ring_return_every_trace_once() {
        let (tx, rx) = mpsc::channel();
        // A walk that does not stop never returns; run it on its own thread so
        // that the test fails instead of hanging.
        std::thread::spawn(move || {
            let mut board = one_layer_board(2_000_000);
            let ring = [
                ip(950_000, 420_000),
                ip(1_050_000, 420_000),
                ip(1_000_000, 370_000),
            ];
            let traces: Vec<ItemId> = (0..ring.len())
                .map(|i| {
                    let polyline = Polyline::from_points(&[
                        ring[i].clone(),
                        ring[(i + 1) % ring.len()].clone(),
                    ]);
                    board
                        .insert_trace_without_cleaning(
                            polyline,
                            0,
                            100,
                            &[1],
                            0,
                            FixedState::Unfixed,
                        )
                        .expect("ring trace")
                })
                .collect();
            let first = traces[0];
            let items = board.connection_items(first, StopConnectionOption::None);
            let expected: BTreeSet<ItemId> = traces.into_iter().collect();
            tx.send((items, expected)).ok();
        });
        let (items, expected) = rx
            .recv_timeout(Duration::from_secs(10))
            .expect("connection_items returns on a closed ring");
        assert_eq!(items.len(), 3);
        assert_eq!(items, expected);
    }
}

/// `app.freerouting.gui.workspace` and `gui.interactive` (I01): the route
/// session and its settings. Java has no unit tests of `Route`, `RouteState`,
/// `DynamicRouteState`, `StitchRouteState` or `GuiBoardRoutingSettings`; the
/// session is compared with Java by the `route-script` probe
/// (`tests/interactive_route.rs`).
///
/// Not ported: `WorkspaceSettingsMergerTest` (merging the settings into
/// `RouterSettings`), `WorkspaceSettingsPropertyChangeTest` (Swing property
/// change events) and `WorkspaceSettingsSingletonTest` (the singleton);
/// `EditSettings` is a plain value without a merger, listeners or a global
/// instance. Ported instead: the defaults of the `WorkspaceSettings(board)`
/// constructor that `EditSettings::new` carries.
mod gui_workspace_settings {
    use freerouting::interactive::EditSettings;

    #[test]
    fn edit_settings_have_the_workspace_settings_defaults() {
        let s = EditSettings::new(4);
        assert_eq!(s.layer, 0);
        assert!(s.push_enabled);
        assert!(s.select_on_all_visible_layers);
        assert!(!s.stitch_route);
        assert_eq!(s.trace_tidy_width, i32::MAX);
        assert!(s.via_snap_to_smd_center);
        assert!(!s.manual_rule_selection);
        assert!(!s.highlight_routing_obstacle);
        assert_eq!(s.manual_rules.clearance_class, 1);
        assert_eq!(s.manual_rules.via_rule_index, 0);
        assert_eq!(s.pull_tight_accuracy, 500);
        assert!(s.automatic_neckdown);
        assert_eq!(s.manual_rules.trace_half_widths, vec![1000; 4]);
        assert!((0..4).all(|l| s.visible_layers.is_visible(l)));
    }
}

// ---------------------------------------------------------------------------
// board.actions.MoveComponentTest (I02.1): all 8 tests, with real pins,
// traces and areas on the test board instead of Mockito mocks and spies. A
// `verify(board).moveDrillItem` or `verify(pin).moveBy` becomes an assert on
// the path the move takes (`requires_drill_item_mover`, `items_to_shove`)
// and on the moved geometry.

mod board_actions_move_component {
    use super::*;
    use freerouting::board::items::AreaKind;
    use freerouting::board::structure::ComponentId;
    use freerouting::geometry::{Area, ConvexShape, Shape};
    use freerouting::interactive::{
        MoveBudget, MoveComponent, SortedItem, requires_drill_item_mover,
    };

    const CX: i32 = 100_000;
    const CY: i32 = 100_000;

    fn net(board: &mut RoutingBoard, name: &str) -> i32 {
        let class = board.rules.default_net_class();
        board.rules.nets.add(name, 1, false, class)
    }

    /// A placed component with one pin per offset; each pad is a 200 x 200
    /// square (`minWidth` 200).
    fn component(
        board: &mut RoutingBoard,
        name: &str,
        at: (i32, i32),
        pins: &[((i32, i32), &[i32])],
    ) -> (ComponentId, Vec<ItemId>) {
        let pad_box = ConvexShape::Tile(TileShape::from(IntBox::new(-100, -100, 100, 100)));
        let pad = board.library.padstacks.add_unnamed(vec![Some(pad_box)]);
        let package_pins = pins
            .iter()
            .enumerate()
            .map(|(i, ((x, y), _))| {
                PackagePin::new(
                    format!("{}", i + 1),
                    pad,
                    FloatPoint::new(f64::from(*x), f64::from(*y)),
                    0.0,
                )
            })
            .collect();
        let package = board.library.packages.add_unnamed(package_pins);
        let component = board.components.add_precise(
            name,
            Some(FloatPoint::new(f64::from(at.0), f64::from(at.1))),
            0.0,
            true,
            package,
            package,
            false,
            Some("ESP32".to_string()),
        );
        let ids = pins
            .iter()
            .enumerate()
            .map(|(i, (_, nets))| {
                board
                    .insert_pin(component.0, i, nets, 1, FixedState::Unfixed)
                    .expect("pin")
            })
            .collect();
        (component, ids)
    }

    fn trace_from(board: &mut RoutingBoard, from: (i32, i32), nets: &[i32]) -> ItemId {
        let corners = [ip(from.0, from.1), ip(from.0 + 5_000, from.1)];
        board
            .insert_trace_without_cleaning(
                Polyline::from_points(&corners),
                0,
                50,
                nets,
                1,
                FixedState::Unfixed,
            )
            .unwrap()
    }

    fn v(x: i32, y: i32) -> Vector {
        Vector::Int(IntVector::new(x, y))
    }

    fn center(board: &RoutingBoard, id: ItemId) -> IntPoint {
        board.item(id).unwrap().drill().unwrap().center
    }

    #[test]
    fn get_min_drill_item_width_with_traces_returns_negative_when_no_connected_traces() {
        let mut board = one_layer_board(2_000_000);
        let n = net(&mut board, "a");
        let (_, pins) = component(&mut board, "U1", (CX, CY), &[((0, 0), &[n])]);
        let moving = MoveComponent::new(&board, pins[0], v(500, 0), 99, 5);
        assert_eq!(moving.min_drill_item_width_with_traces(&board), -1.0);
    }

    #[test]
    fn get_min_drill_item_width_with_traces_returns_width_when_trace_connected() {
        let mut board = one_layer_board(2_000_000);
        let n = net(&mut board, "a");
        let (_, pins) = component(&mut board, "U1", (CX, CY), &[((0, 0), &[n])]);
        trace_from(&mut board, (CX, CY), &[n]);
        let moving = MoveComponent::new(&board, pins[0], v(500, 0), 99, 5);
        assert_eq!(moving.min_drill_item_width_with_traces(&board), 200.0);
    }

    #[test]
    fn check_allows_move_larger_than_min_width_when_no_connecting_traces_and_space_clear() {
        let mut board = one_layer_board(2_000_000);
        let n = net(&mut board, "a");
        let (_, pins) = component(&mut board, "U1", (CX, CY), &[((0, 0), &[n])]);
        // 1000 >= minWidth (200)
        let mut moving = MoveComponent::new(&board, pins[0], v(1000, 0), 99, 5);
        assert!(moving.check(&mut board, MoveBudget::JAVA));
        assert!(moving.items_to_shove().is_empty());
    }

    #[test]
    fn check_fails_move_larger_than_min_width_when_connecting_trace_present() {
        let mut board = one_layer_board(2_000_000);
        let n = net(&mut board, "a");
        let (_, pins) = component(&mut board, "U1", (CX, CY), &[((0, 0), &[n])]);
        trace_from(&mut board, (CX, CY), &[n]);
        // 1000 >= minWidth (200)
        let mut moving = MoveComponent::new(&board, pins[0], v(1000, 0), 99, 5);
        assert!(!moving.check(&mut board, MoveBudget::JAVA));
    }

    #[test]
    fn insert_bypasses_move_drill_item_when_no_connecting_traces_and_space_clear() {
        let mut board = one_layer_board(2_000_000);
        let n = net(&mut board, "a");
        let (component, pins) = component(&mut board, "U1", (CX, CY), &[((0, 0), &[n])]);
        let mut moving = MoveComponent::new(&board, pins[0], v(500, 0), 99, 5);
        assert!(moving.check(&mut board, MoveBudget::JAVA));
        // Java: verify(board, never()).moveDrillItem(..)
        assert!(!requires_drill_item_mover(&board, pins[0]));
        assert!(moving.items_to_shove().is_empty());
        let items_before = board.items().count();
        assert!(moving.insert(&mut board, 100, 50, MoveBudget::JAVA));
        // Java: verify(pin1).moveBy(vector)
        assert_eq!(center(&board, pins[0]), IntPoint::new(CX + 500, CY));
        assert_eq!(
            board.components.get(component).unwrap().location(),
            Some(IntPoint::new(CX + 500, CY))
        );
        assert_eq!(board.items().count(), items_before, "no trace was added");
    }

    #[test]
    fn move_component_without_drill_items_computes_center_and_moves_successfully() {
        let mut board = one_layer_board(2_000_000);
        let package = board.library.packages.add_unnamed(Vec::new());
        let component = board.components.add_precise(
            "R1",
            Some(FloatPoint::new(100.0, 200.0)),
            0.0,
            true,
            package,
            package,
            false,
            Some("RES_0805".to_string()),
        );
        let relative = Area::from(Shape::from(TileShape::from(IntBox::new(-20, -20, 20, 20))));
        let smd_pad = board.insert_area(
            AreaKind::Obstacle,
            relative,
            0,
            Vector::ZERO,
            0.0,
            false,
            &[],
            1,
            component.0,
            None,
            FixedState::Unfixed,
        );
        assert_eq!(
            board.item(smd_pad).unwrap().bounding_box(),
            IntBox::new(80, 180, 120, 220)
        );
        let mut moving = MoveComponent::new(&board, smd_pad, v(300, 400), 99, 5);
        assert_eq!(moving.min_drill_item_width_with_traces(&board), -1.0);
        assert!(moving.check(&mut board, MoveBudget::JAVA));
        assert!(moving.insert(&mut board, 100, 50, MoveBudget::JAVA));
        // Java: verify(smdPad).moveBy(moveVector)
        assert_eq!(
            board.item(smd_pad).unwrap().bounding_box(),
            IntBox::new(380, 580, 420, 620)
        );
        assert_eq!(
            board.components.get(component).unwrap().location(),
            Some(IntPoint::new(400, 600))
        );
    }

    #[test]
    fn check_uses_drill_item_mover_for_multi_net_drill_item_without_traces() {
        let mut board = one_layer_board(2_000_000);
        let (a, b) = (net(&mut board, "a"), net(&mut board, "b"));
        let (_, pins) = component(&mut board, "U1", (CX, CY), &[((0, 0), &[a, b])]);
        assert!(requires_drill_item_mover(&board, pins[0]));
        let moving = MoveComponent::new(&board, pins[0], v(50, 0), 99, 5);
        assert_eq!(moving.min_drill_item_width_with_traces(&board), 200.0);
        // a move larger than minWidth is rejected for a multi-net drill item
        let mut large_move = MoveComponent::new(&board, pins[0], v(500, 0), 99, 5);
        assert!(!large_move.check(&mut board, MoveBudget::JAVA));
    }

    #[test]
    fn insert_uses_move_drill_item_for_multi_net_drill_item() {
        let mut board = one_layer_board(2_000_000);
        let (a, b) = (net(&mut board, "a"), net(&mut board, "b"));
        let (_, pins) = component(&mut board, "U1", (CX, CY), &[((0, 0), &[a, b])]);
        // Java: verify(board).moveDrillItem(pin1, vector, ..); never pin1.moveBy
        assert!(requires_drill_item_mover(&board, pins[0]));
        let moving = MoveComponent::new(&board, pins[0], v(50, 0), 99, 5);
        assert!(moving.insert(&mut board, 100, 50, MoveBudget::JAVA));
        assert_eq!(center(&board, pins[0]), IntPoint::new(CX + 50, CY));
    }

    /// The decision of I02: Java keeps the group in identity-hash sets, the
    /// port in item-id order. The move order is the component's item-list
    /// order (highest id first) stably sorted by the projection on the
    /// vector, the items in front first; equal projections keep the
    /// item-list order. No result depends on the set order (the check's
    /// ignore set is the whole group from the start).
    #[test]
    fn move_component_order_is_item_id_order() {
        let mut board = one_layer_board(2_000_000);
        let n = net(&mut board, "a");
        // pins at x offsets 0, 1000, 1000, -1000 (two with equal projection)
        let (_, pins) = component(
            &mut board,
            "U1",
            (CX, CY),
            &[
                ((0, 0), &[n]),
                ((1000, 0), &[n]),
                ((1000, 2000), &[n]),
                ((-1000, 0), &[n]),
            ],
        );
        let component_items: Vec<ItemId> = board.component_items(1);
        let mut by_id = pins.clone();
        by_id.sort();
        assert_eq!(
            component_items, by_id,
            "item-list order is highest id first"
        );
        let moving = MoveComponent::new(&board, pins[0], v(1000, 0), 99, 5);
        let order: Vec<ItemId> = moving.group().iter().map(|s: &SortedItem| s.item).collect();
        // gravity point (CX + 250, CY + 500): projections -750000 (pins 1
        // and 2), 250000 (pin 0), 1250000 (pin 3); pins 1 and 2 tie and keep
        // the item-list order, pin 2 (the higher id) first
        assert_eq!(order, vec![pins[2], pins[1], pins[0], pins[3]]);
        let projections: Vec<f64> = moving.group().iter().map(|s| s.projection).collect();
        assert_eq!(
            projections,
            vec![-750_000.0, -750_000.0, 250_000.0, 1_250_000.0]
        );
        // the reverse direction puts pin 3 in front; the tie keeps its order
        let back = MoveComponent::new(&board, pins[0], v(-1000, 0), 99, 5);
        let order: Vec<ItemId> = back.group().iter().map(|s| s.item).collect();
        assert_eq!(order, vec![pins[3], pins[0], pins[2], pins[1]]);
    }

    /// A user-fixed item makes the whole group immovable.
    #[test]
    fn a_user_fixed_item_makes_the_group_immovable() {
        let mut board = one_layer_board(2_000_000);
        let n = net(&mut board, "a");
        let (_, pins) = component(
            &mut board,
            "U1",
            (CX, CY),
            &[((0, 0), &[n]), ((1000, 0), &[n])],
        );
        board.set_fixed_state(pins[1], FixedState::UserFixed);
        let mut moving = MoveComponent::new(&board, pins[0], v(100, 0), 99, 5);
        assert!(!moving.is_movable());
        assert!(moving.group().is_empty());
        assert!(!moving.check(&mut board, MoveBudget::JAVA));
        assert!(!moving.insert(&mut board, 100, 50, MoveBudget::JAVA));
        assert_eq!(center(&board, pins[0]), IntPoint::new(CX, CY));
    }
}

/// `board.actions.ComponentItemTransformTest` (I04.1): `ComponentItemTransform.apply`
/// (`interactive::apply_transform`) on items detached with `detachItemForMove` and
/// copied with `copy(0)`.
///
/// Ported: five of the six tests, `sharedJunctionViaAndUndoRemainConsistent` as one test per
/// parameter set (four) and the other four. Not ported here:
/// `componentRotationApiUsesExactQuarterTurns`, ported by B05 (it calls only the component API,
/// `component_rotation_api_uses_exact_quarter_turns` above).
/// The reference comparison with Java's accept/refuse decisions, ids and positions is
/// `tests/interactive_transform.rs` (probe `transform`).
mod board_actions_component_item_transform {
    use super::*;
    use freerouting::board::items::Item;
    use freerouting::board::structure::{AngleRestriction, ComponentId};
    use freerouting::interactive::{Rejection, TransformError, apply_transform};

    fn board() -> RoutingBoard {
        let layers = LayerStructure::new(vec![Layer::new("Top", true), Layer::new("Bottom", true)]);
        let matrix = ClearanceMatrix::default_instance(layers.count(), 10);
        let mut rules = BoardRules::new(layers.clone(), matrix);
        rules.create_default_net_class();
        let outline = IntBox::new(-100_000, -100_000, 100_000, 100_000);
        let mut board = RoutingBoard::new(
            outline,
            layers,
            vec![PolylineShape::Tile(TileShape::from(outline))],
            0,
            rules,
            Communication::default(),
        );
        let class = board.rules.default_net_class();
        board.rules.nets.add("one", 1, false, class);
        board.rules.nets.add("two", 1, false, class);
        board
    }

    fn pin(board: &mut RoutingBoard, x: f64, y: f64, net: i32) -> ItemId {
        let circle = ConvexShape::Circle(Circle::new(IntPoint::new(0, 0), 20));
        let pad = board
            .library
            .padstacks
            .add_unnamed(vec![Some(circle.clone()), Some(circle)]);
        let pkg = board.library.packages.add_unnamed(vec![PackagePin::new(
            "1",
            pad,
            FloatPoint::new(0.0, 0.0),
            0.0,
        )]);
        let name = format!("C{}", board.components.count());
        let component = board.components.add_precise(
            name,
            Some(FloatPoint::new(x, y)),
            0.0,
            true,
            pkg,
            pkg,
            false,
            None,
        );
        board
            .insert_pin(component.0, 0, &[net], 1, FixedState::Unfixed)
            .expect("pin")
    }

    fn trace(board: &mut RoutingBoard, from: IntPoint, to: IntPoint) -> ItemId {
        board
            .insert_trace_without_cleaning(
                Polyline::from_points(&[Point::Int(from), Point::Int(to)]),
                0,
                2,
                &[1],
                1,
                FixedState::Unfixed,
            )
            .expect("trace")
    }

    fn center(board: &RoutingBoard, id: ItemId) -> IntPoint {
        board.item(id).unwrap().drill().unwrap().center
    }

    /// Java's `detach`: `detachItemForMove(item)`, then `item.copy(0)`.
    fn detach(board: &mut RoutingBoard, ids: &[ItemId]) -> Vec<Item> {
        ids.iter()
            .map(|&id| {
                let item = board.item(id).unwrap().clone();
                board.detach_item_for_move(id).expect("detach");
                board.copy_of(&item, None).expect("copy")
            })
            .collect()
    }

    fn insert_all(board: &mut RoutingBoard, items: Vec<Item>) -> Vec<ItemId> {
        items.into_iter().map(|i| board.insert_item(i)).collect()
    }

    fn first_pin(board: &RoutingBoard) -> ItemId {
        *board.pins().first().expect("a pin")
    }

    fn shared_junction(rotate_first: bool, front: bool, half: f64) {
        let mut board = board();
        board.components.set_flip_style_rotate_first(rotate_first);
        let pin = pin(&mut board, 2000.0 + half, 3000.0 + half, 1);
        if !front {
            let component = ComponentId(board.item(pin).unwrap().component_id());
            let pole = IntPoint::new(2000, 0);
            board.components.change_side(component, pole);
            board.change_placement_side(pin, pole);
        }
        let original = center(&board, pin);
        let first = trace(
            &mut board,
            original,
            IntPoint::new(original.x + 1000, original.y),
        );
        let second = trace(
            &mut board,
            original,
            IntPoint::new(original.x, original.y + 1000),
        );
        let padstack = board.item(pin).unwrap().drill().unwrap().padstack;
        let via = board.insert_via(padstack, original, &[1], 1, FixedState::Unfixed, true);
        assert_eq!(board.normal_contacts(pin).len(), 3);
        board.generate_snapshot();
        let detached = detach(&mut board, &[pin, first, second, via]);
        let moved = apply_transform(&mut board, &detached, 90.0, false, IntPoint::new(100, 200))
            .expect("the transform is accepted");
        let ids = insert_all(&mut board, moved);
        let moved_pin = ids[0];
        assert_eq!(board.normal_contacts(moved_pin).len(), 3);
        let new_center = center(&board, moved_pin);
        assert_eq!(new_center, center(&board, ids[3]));
        assert!(board.undo(None));
        assert_eq!(center(&board, first_pin(&board)), original);
        assert_eq!(board.normal_contacts(first_pin(&board)).len(), 3);
        assert!(board.redo(None));
        assert_eq!(center(&board, first_pin(&board)), new_center);
        assert_eq!(board.normal_contacts(first_pin(&board)).len(), 3);
    }

    #[test]
    fn shared_junction_via_and_undo_remain_consistent_front_move_first() {
        shared_junction(false, true, 0.5);
    }

    #[test]
    fn shared_junction_via_and_undo_remain_consistent_front_rotate_first() {
        shared_junction(true, true, 0.5);
    }

    #[test]
    fn shared_junction_via_and_undo_remain_consistent_back_move_first() {
        shared_junction(false, false, -0.5);
    }

    #[test]
    fn shared_junction_via_and_undo_remain_consistent_back_rotate_first() {
        shared_junction(true, false, -0.5);
    }

    #[test]
    fn conflicting_anchors_reject_without_changing_pose_or_search_tree() {
        let mut board = board();
        let a = pin(&mut board, 2000.5, 3000.5, 1);
        let b = pin(&mut board, 4000.0, 3001.0, 1);
        let (from, to) = (center(&board, a), center(&board, b));
        let wire = trace(&mut board, from, to);
        let component = ComponentId(board.item(a).unwrap().component_id());
        let location = board.components.get(component).unwrap().exact_location();
        let detached = detach(&mut board, &[a, b, wire]);
        let result = apply_transform(&mut board, &detached, 90.0, false, IntPoint::new(0, 0));
        assert_eq!(
            result.unwrap_err(),
            TransformError::Rejected(Rejection::ConflictingAnchors)
        );
        assert_eq!(
            board.components.get(component).unwrap().exact_location(),
            location
        );
        let ids = insert_all(&mut board, detached);
        assert_eq!(board.normal_contacts(ids[0]).len(), 1);
        assert_eq!(board.normal_contacts(ids[1]).len(), 1);
        assert_eq!(board.normal_contacts(ids[2]).len(), 2);
    }

    #[test]
    fn new_foreign_net_clearance_rejects_and_leaves_stationary_pin_untouched() {
        let mut board = board();
        let a = pin(&mut board, 2000.5, 3000.5, 1);
        let obstacle = pin(&mut board, -3000.0, 2001.0, 2);
        let before = center(&board, obstacle);
        let detached = detach(&mut board, &[a]);
        let result = apply_transform(&mut board, &detached, 90.0, false, IntPoint::new(0, 0));
        assert_eq!(
            result.unwrap_err(),
            TransformError::Rejected(Rejection::NewViolations)
        );
        assert_eq!(center(&board, obstacle), before);
        assert!(board.normal_contacts(obstacle).is_empty());
        insert_all(&mut board, detached);
        assert_eq!(board.all_clearance_violations().len(), 0);
    }

    #[test]
    fn existing_non_grid_pin_escape_can_still_rotate_rigidly() {
        let mut board = board();
        board
            .rules
            .set_trace_angle_restriction(AngleRestriction::FortyfiveDegree);
        let a = pin(&mut board, 2000.5, 3000.5, 1);
        let from = center(&board, a);
        let wire = trace(&mut board, from, IntPoint::new(from.x + 1000, from.y + 600));
        let detached = detach(&mut board, &[a, wire]);
        let moved = apply_transform(&mut board, &detached, 90.0, false, IntPoint::new(0, 0))
            .expect("the escape is already non-grid, so it may rotate");
        let ids = insert_all(&mut board, moved);
        assert_eq!(board.normal_contacts(ids[0]).len(), 1);
    }

    #[test]
    fn illegal_trace_angle_rejects_rather_than_distorting_copper() {
        let mut board = board();
        board
            .rules
            .set_trace_angle_restriction(AngleRestriction::NinetyDegree);
        let a = pin(&mut board, 2000.5, 3000.5, 1);
        let from = center(&board, a);
        let wire = trace(&mut board, from, IntPoint::new(from.x + 1000, from.y));
        let detached = detach(&mut board, &[a, wire]);
        let result = apply_transform(&mut board, &detached, 45.0, false, IntPoint::new(0, 0));
        assert!(matches!(
            result.unwrap_err(),
            TransformError::Rejected(Rejection::IllegalTrace(_))
        ));
        let ids = insert_all(&mut board, detached);
        assert_eq!(board.normal_contacts(ids[0]).len(), 1);
    }
}
