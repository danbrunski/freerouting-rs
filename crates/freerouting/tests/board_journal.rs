//! The board change journal (B01.7).

mod board_probe;

use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::board::structure::FixedState;
use freerouting::geometry::{IntBox, IntPoint, Point, Polyline};

fn two_pin_board() -> RoutingBoard {
    board_probe::load(&board_probe::fixture("scoring-perfect-two-pin"))
}

/// The centers and the net of the two pins.
fn two_pins(board: &RoutingBoard) -> (IntPoint, IntPoint, i32) {
    let pins: Vec<_> = board.items().filter(|i| i.is_pin()).collect();
    let center = |i: usize| pins[i].drill().expect("drill").center;
    (center(0), center(1), pins[0].net_numbers()[0])
}

fn insert_trace(board: &mut RoutingBoard, points: &[IntPoint], net: i32) -> ItemId {
    let points: Vec<Point> = points.iter().map(|p| Point::Int(*p)).collect();
    board
        .insert_trace_without_cleaning(
            Polyline::from_points(&points),
            0,
            100,
            &[net],
            0,
            FixedState::Unfixed,
        )
        .expect("trace inserted")
}

fn contains(outer: &IntBox, inner: &IntBox) -> bool {
    outer.ll.x <= inner.ll.x
        && outer.ll.y <= inner.ll.y
        && outer.ur.x >= inner.ur.x
        && outer.ur.y >= inner.ur.y
}

fn item_box(board: &RoutingBoard, id: ItemId) -> IntBox {
    board.item(id).expect("item").bounding_box()
}

#[test]
fn journal_records_insert() {
    let mut board = two_pin_board();
    let (a, b, net) = two_pins(&board);
    board.set_journaling(true);
    let id = insert_trace(&mut board, &[a, b], net);
    let changes = board.take_changes();
    assert_eq!(changes.inserted, vec![id]);
    assert!(changes.removed.is_empty() && changes.changed.is_empty());
    assert_eq!(
        changes.changed_nets.into_iter().collect::<Vec<_>>(),
        vec![net]
    );
    assert!(contains(
        &changes.dirty.expect("dirty"),
        &item_box(&board, id)
    ));
    assert!(!changes.overflow);
    assert!(board.take_changes().is_empty(), "taking empties the log");
}

#[test]
fn journal_records_remove() {
    let mut board = two_pin_board();
    let (a, b, net) = two_pins(&board);
    let id = insert_trace(&mut board, &[a, b], net);
    let bounds = item_box(&board, id);
    board.set_journaling(true);
    assert!(board.remove_item(id));
    let changes = board.take_changes();
    assert_eq!(changes.removed, vec![id]);
    assert!(changes.inserted.is_empty() && changes.changed.is_empty());
    assert!(changes.changed_nets.contains(&net));
    assert!(contains(&changes.dirty.expect("dirty"), &bounds));
}

#[test]
fn journal_records_change() {
    let mut board = two_pin_board();
    let (a, b, net) = two_pins(&board);
    let id = insert_trace(&mut board, &[a, b], net);
    let before = item_box(&board, id);
    board.set_journaling(true);
    let bend = IntPoint::new(a.x + (b.x - a.x) / 2 + 7000, a.y + (b.y - a.y) / 2 + 7000);
    board.change_trace(
        id,
        Polyline::from_points(&[Point::Int(a), Point::Int(bend), Point::Int(b)]),
        freerouting::board::LineReuse::NONE,
    );
    let changes = board.take_changes();
    assert!(changes.changed.contains(&id), "{changes:?}");
    assert!(changes.inserted.is_empty() && changes.removed.is_empty());
    let dirty = changes.dirty.expect("dirty");
    assert!(contains(&dirty, &before), "covers the trace before");
    assert!(contains(&dirty, &item_box(&board, id)), "covers it after");
}

#[test]
fn journal_records_undo() {
    let mut board = two_pin_board();
    let (a, b, net) = two_pins(&board);
    board.generate_snapshot();
    let id = insert_trace(&mut board, &[a, b], net);
    let bounds = item_box(&board, id);
    board.set_journaling(true);
    assert!(board.undo(None));
    let changes = board.take_changes();
    assert_eq!(changes.removed, vec![id], "the cancelled insert is removed");
    assert!(changes.inserted.is_empty() && changes.changed.is_empty());
    assert!(changes.changed_nets.contains(&net));
    assert!(contains(&changes.dirty.expect("dirty"), &bounds));
}

#[test]
fn journal_records_redo() {
    let mut board = two_pin_board();
    let (a, b, net) = two_pins(&board);
    board.generate_snapshot();
    let id = insert_trace(&mut board, &[a, b], net);
    assert!(board.undo(None));
    board.set_journaling(true);
    assert!(board.redo(None));
    let changes = board.take_changes();
    assert_eq!(
        changes.inserted,
        vec![id],
        "the restored insert is inserted"
    );
    assert!(changes.removed.is_empty() && changes.changed.is_empty());
    assert!(changes.changed_nets.contains(&net));
    assert!(contains(
        &changes.dirty.expect("dirty"),
        &item_box(&board, id)
    ));
}

#[test]
fn journal_records_net_change() {
    let mut board = board_probe::load(&board_probe::fixture("Issue026-J2_reference"));
    let pin = board
        .items()
        .find(|i| i.is_pin() && i.net_numbers().len() == 1)
        .expect("a pin with a net");
    let (id, net) = (pin.id(), pin.net_numbers()[0]);
    let other = if net == 1 { 2 } else { 1 };
    board.set_journaling(true);
    board.assign_net_no(id, other);
    let changes = board.take_changes();
    assert_eq!(changes.changed, vec![id]);
    assert!(changes.inserted.is_empty() && changes.removed.is_empty());
    assert!(changes.changed_nets.contains(&net), "the old net");
    assert!(changes.changed_nets.contains(&other), "the new net");
    assert!(contains(
        &changes.dirty.expect("dirty"),
        &item_box(&board, id)
    ));
}

#[test]
fn journal_reduces_insert_then_remove_to_nothing() {
    let mut board = two_pin_board();
    let (a, b, net) = two_pins(&board);
    board.set_journaling(true);
    let id = insert_trace(&mut board, &[a, b], net);
    board.set_fixed_state(id, FixedState::ShoveFixed);
    assert!(board.remove_item(id));
    let changes = board.take_changes();
    assert!(changes.inserted.is_empty() && changes.removed.is_empty());
    assert!(changes.changed.is_empty());
}

#[test]
fn journal_overflow_signals_rebuild() {
    let mut board = two_pin_board();
    let (a, b, net) = two_pins(&board);
    board.set_journaling_with_cap(true, 3);
    for _ in 0..5 {
        insert_trace(&mut board, &[a, b], net);
    }
    let changes = board.take_changes();
    assert!(changes.overflow);
    assert!(changes.inserted.is_empty());
    let id = insert_trace(&mut board, &[a, b], net);
    let changes = board.take_changes();
    assert!(!changes.overflow, "a new log starts after the take");
    assert_eq!(changes.inserted, vec![id]);
}

/// A sequence touching every journal choke point; returns the state hash.
fn scripted(journaling: bool) -> u128 {
    let row = board_probe::fixture("Issue026-J2_reference");
    let mut board = board_probe::load(&row);
    board.set_journaling(journaling);
    board_probe::import_session(&mut board, &row).expect("a session");
    board.generate_snapshot();
    let traces = board.traces();
    for id in traces.iter().take(5) {
        board.unfix(*id);
        board.remove_item(*id);
    }
    for id in board.vias().into_iter().take(3) {
        board.assign_net_no(id, 1);
        board.set_fixed_state(id, FixedState::ShoveFixed);
    }
    assert!(board.undo(None));
    assert!(board.redo(None));
    board.normalize_all_traces();
    let _ = board.take_changes();
    board.state_hash()
}

#[test]
fn journal_has_no_effect() {
    assert_eq!(scripted(true), scripted(false));
}
