//! Board snapshots and deep copies with Java's restore semantics (B01.3), and
//! the transient routing state (B01.8).

mod board_probe;

use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::board::search::{ShapeSearchTree, TreeKind};
use freerouting::board::structure::FixedState;
use freerouting::datastructures::IdGenerator;
use freerouting::geometry::{FloatPoint, IntPoint, Point, Polyline};

fn j2_with_session() -> RoutingBoard {
    let row = board_probe::fixture("Issue026-J2_reference");
    let mut board = board_probe::load(&row);
    board_probe::import_session(&mut board, &row).expect("J2 has a session");
    board
}

fn two_pin_board() -> RoutingBoard {
    board_probe::load(&board_probe::fixture("scoring-perfect-two-pin"))
}

/// The centers and the net of the two pins of `scoring-perfect-two-pin`.
fn two_pins(board: &RoutingBoard) -> (IntPoint, IntPoint, i32) {
    let pins: Vec<_> = board.items().filter(|i| i.is_pin()).collect();
    let center = |i: usize| pins[i].drill().expect("drill").center;
    (center(0), center(1), pins[0].net_numbers()[0])
}

/// Inserts two unfixed traces from pin to pin that meet halfway, so that
/// normalizing the net combines them.
fn insert_split_connection(board: &mut RoutingBoard) -> i32 {
    let (a, b, net) = two_pins(board);
    let mid = IntPoint::new(a.x + (b.x - a.x) / 2, a.y + (b.y - a.y) / 2);
    for (from, to) in [(a, mid), (mid, b)] {
        let polyline = Polyline::from_points(&[Point::Int(from), Point::Int(to)]);
        board
            .insert_trace_without_cleaning(polyline, 0, 100, &[net], 0, FixedState::Unfixed)
            .expect("trace inserted");
    }
    net
}

/// The shape of a tree: per leaf (left to right) its item, shape index,
/// depth and bounds.
fn tree_shape(tree: &ShapeSearchTree) -> Vec<String> {
    let shape_tree = tree.shape_tree();
    shape_tree
        .to_array()
        .into_iter()
        .map(|leaf| {
            let (item, index) = shape_tree.leaf(leaf);
            format!(
                "{:?} {} {} {:?}",
                item,
                index,
                shape_tree.distance_to_root(leaf),
                shape_tree.leaf_bounds(leaf)
            )
        })
        .collect()
}

#[test]
fn restore_resets_transient_state() {
    let mut board = j2_with_session();
    board.suppress_normalize(1);
    board.start_marking_changed_area();
    board.join_changed_area(FloatPoint::new(0.0, 0.0), 0);
    board.set_shove_failing_obstacle(board.item_ids().first().copied());
    board.set_shove_failing_layer(1);
    let _ = board.edge_pin_nets();
    let _ = board.ledger_incomplete_count();
    board.set_journaling(true);
    assert!(board.revision() > 0);
    assert!(board.edge_pin_nets_cached());
    assert!(board.routing_ledger().is_built());

    let copy = board.snapshot().restore();
    assert_eq!(copy.normalize_suppressed_nets(), Vec::<i32>::new());
    assert_eq!(copy.revision(), 0);
    assert!(copy.changed_area().is_none());
    assert_eq!(copy.shove_failing_obstacle(), None);
    assert_eq!(copy.shove_failing_layer(), -1);
    assert!(!copy.edge_pin_nets_cached());
    assert!(!copy.routing_ledger().is_built());
    assert!(!copy.is_journaling());
    assert!(copy.log_messages().is_empty());
    // B02.7's work meter is reset here too once it exists.
    // The persistent state is kept.
    assert_eq!(copy.item_ids(), board.item_ids());
    assert_eq!(
        copy.communication.id_generator.max_generated_id(),
        board.communication.id_generator.max_generated_id()
    );
}

#[test]
fn deep_copy_resets_normalize_suppression() {
    let mut board = two_pin_board();
    let net = insert_split_connection(&mut board);
    board.suppress_normalize(net);
    let mut copy = board.deep_copy();
    assert!(!board.normalize_traces(net), "suppressed on the original");
    assert!(copy.normalize_traces(net), "normalized again on the copy");
    assert_eq!(copy.traces().len(), 1);
    assert_eq!(board.traces().len(), 2);
}

#[test]
fn deep_copy_keeps_ids_generator_and_undo() {
    let mut board = two_pin_board();
    board.generate_snapshot();
    insert_split_connection(&mut board);
    let mut copy = board.deep_copy();
    assert!(board.undo(None));
    assert!(copy.undo(None));
    assert_eq!(copy.item_ids(), board.item_ids());
    assert!(board.redo(None));
    assert!(copy.redo(None));
    assert_eq!(copy.item_ids(), board.item_ids());
    let next = |b: &mut RoutingBoard| {
        let (a, c, net) = two_pins(b);
        let polyline = Polyline::from_points(&[Point::Int(a), Point::Int(c)]);
        b.insert_trace_without_cleaning(polyline, 0, 100, &[net], 0, FixedState::Unfixed)
    };
    assert_eq!(next(&mut copy), next(&mut board));
}

#[test]
fn deep_copy_drops_compensated_trees() {
    let mut board = j2_with_session();
    let _ = board.autoroute_search_tree(1);
    assert_eq!(board.search_trees().trees().len(), 2);
    let copy = board.deep_copy();
    assert_eq!(copy.search_trees().trees().len(), 1);
    let tree = copy.search_trees().default_tree();
    assert_eq!(tree.kind(), TreeKind::Plain);
    assert_eq!(tree.compensated_clearance_class(), 0);
}

#[test]
fn deep_copy_drops_clearance_compensation() {
    // Java behaviour kept on purpose (B00 open question 3): a restore rebuilds
    // the default tree uncompensated even when compensation was on.
    let mut board = j2_with_session();
    board.set_clearance_compensation_used(true);
    assert!(board.search_trees().is_clearance_compensation_used());
    let copy = board.deep_copy();
    assert!(!copy.search_trees().is_clearance_compensation_used());
    assert_eq!(
        copy.search_trees()
            .default_tree()
            .compensated_clearance_class(),
        0
    );
}

#[test]
fn deep_copy_tree_equals_ordered_rebuild() {
    let board = j2_with_session();
    let copy = board.deep_copy();
    let mut rebuilt = ShapeSearchTree::new(TreeKind::Plain, 0);
    for item in board.items() {
        rebuilt.insert(item, &board.shape_context());
    }
    let copied = tree_shape(copy.search_trees().default_tree());
    assert!(!copied.is_empty());
    assert_eq!(copied, tree_shape(&rebuilt));
}

#[test]
fn deep_copy_items_equal_original() {
    let board = j2_with_session();
    let copy = board.deep_copy();
    let ids: Vec<ItemId> = board.item_ids();
    assert_eq!(copy.item_ids(), ids);
    for id in ids {
        assert_eq!(
            format!("{:?}", copy.item(id)),
            format!("{:?}", board.item(id))
        );
    }
}

// B01.8: the transient routing state, and the fields Java's undo keeps.

#[test]
fn undo_keeps_trace_half_width_bounds() {
    let mut board = two_pin_board();
    let (a, b, net) = two_pins(&board);
    board.generate_snapshot();
    let polyline = Polyline::from_points(&[Point::Int(a), Point::Int(b)]);
    board
        .insert_trace_without_cleaning(polyline, 0, 50_000, &[net], 0, FixedState::Unfixed)
        .expect("trace inserted");
    let polyline = Polyline::from_points(&[Point::Int(b), Point::Int(a)]);
    board
        .insert_trace_without_cleaning(polyline, 0, 3, &[net], 0, FixedState::Unfixed)
        .expect("trace inserted");
    assert_eq!(board.max_trace_half_width(), 50_000);
    assert_eq!(board.min_trace_half_width(), 3);
    assert!(board.undo(None));
    assert!(board.traces().is_empty());
    assert_eq!(board.max_trace_half_width(), 50_000);
    assert_eq!(board.min_trace_half_width(), 3);
}

#[test]
fn undo_keeps_id_generator() {
    let mut board = two_pin_board();
    board.generate_snapshot();
    insert_split_connection(&mut board);
    let after_insert = board.communication.id_generator.max_generated_id();
    assert!(board.undo(None));
    assert_eq!(
        board.communication.id_generator.max_generated_id(),
        after_insert
    );
}

#[test]
fn undo_keeps_normalize_suppression() {
    let mut board = two_pin_board();
    board.generate_snapshot();
    let net = insert_split_connection(&mut board);
    board.suppress_normalize(net);
    assert!(board.undo(None));
    assert_eq!(board.normalize_suppressed_nets(), vec![net]);
}

#[test]
fn restore_resets_shove_failing_state() {
    let mut board = two_pin_board();
    let id = board.item_ids()[0];
    board.set_shove_failing_obstacle(Some(id));
    board.set_shove_failing_layer(0);
    assert_eq!(board.shove_failing_obstacle(), Some(id));
    let copy = board.snapshot().restore();
    assert_eq!(copy.shove_failing_obstacle(), None);
    assert_eq!(copy.shove_failing_layer(), -1);
    board.clear_shove_failing_obstacle();
    assert_eq!(board.shove_failing_obstacle(), None);
    assert_eq!(board.shove_failing_layer(), -1);
}

#[test]
fn clear_transient_autoroute_state_resets_changed_area() {
    let mut board = two_pin_board();
    board.start_marking_changed_area();
    board.join_changed_area(FloatPoint::new(10.0, 10.0), 0);
    board.set_shove_failing_obstacle(board.item_ids().first().copied());
    board.set_shove_failing_layer(0);
    assert!(board.changed_area().is_some());
    board.clear_transient_autoroute_state();
    assert!(board.changed_area().is_none());
    assert_eq!(board.shove_failing_obstacle(), None);
    assert_eq!(board.shove_failing_layer(), -1);
}

#[test]
fn failure_log_rolls_back_with_snapshot() {
    use freerouting::autoroute::AttemptState;
    // Java serializes `RoutingBoard.failureLog`, so a restore brings back the
    // log as it was when the snapshot was taken.
    let mut board = two_pin_board();
    let id = board.item_ids()[0];
    board
        .failure_log_mut()
        .record_failure(id, 1, 1, AttemptState::Skipped, Some("first"));
    let snapshot = board.snapshot();
    board
        .failure_log_mut()
        .record_failure(id, 1, 2, AttemptState::Skipped, None);
    assert_eq!(board.failure_log().failure_count(id), 2);
    let restored = snapshot.restore();
    assert_eq!(restored.failure_log().failure_count(id), 1);
    assert_eq!(board.deep_copy().failure_log(), board.failure_log());
}
