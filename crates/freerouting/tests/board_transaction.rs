//! Insert, check, commit or revert (B09.1).

mod board_probe;

use std::convert::Infallible;

use freerouting::board::items::ItemId;
use freerouting::board::structure::FixedState;
use freerouting::board::{CheckPolicy, RoutingBoard, TransactionResult};
use freerouting::datastructures::IdGenerator;
use freerouting::geometry::{IntPoint, Point, Polyline};

fn j2() -> RoutingBoard {
    board_probe::load(&board_probe::fixture("Issue026-J2_reference"))
}

/// A pin with a net: its id, centre, layer and net.
#[derive(Clone, Copy, Debug)]
struct PinAt {
    id: ItemId,
    center: IntPoint,
    layer: i32,
    net: i32,
}

/// Two pins of different nets on the same layer, the second the nearest to
/// the first.
fn foreign_pin_pair(board: &RoutingBoard) -> (PinAt, PinAt) {
    let pins: Vec<PinAt> = board
        .items()
        .filter(|i| i.is_pin() && i.net_count() == 1 && i.first_layer() == i.last_layer())
        .map(|i| PinAt {
            id: i.id(),
            center: i.drill().expect("drill").center,
            layer: i.first_layer(),
            net: i.net_number(0),
        })
        .collect();
    let first = pins[0];
    let second = pins
        .iter()
        .filter(|p| p.net != first.net && p.layer == first.layer)
        .min_by_key(|p| {
            let (dx, dy) = (
                i64::from(p.center.x - first.center.x),
                i64::from(p.center.y - first.center.y),
            );
            (dx * dx + dy * dy, p.id.0)
        })
        .copied()
        .expect("a pin of another net");
    (first, second)
}

fn trace(board: &mut RoutingBoard, from: IntPoint, to: IntPoint, layer: i32, net: i32) -> ItemId {
    let polyline = Polyline::from_corners(&Point::Int(from), &Point::Int(to));
    board
        .insert_trace_without_cleaning(polyline, layer, 50, &[net], 1, FixedState::Unfixed)
        .expect("trace inserted")
}

/// A short stub inside the pad of `pin`: clean.
fn clean_stub(board: &mut RoutingBoard, pin: PinAt) -> ItemId {
    let to = IntPoint::new(pin.center.x + 10, pin.center.y);
    trace(board, pin.center, to, pin.layer, pin.net)
}

/// A trace of `from`'s net onto the pad of `to`: a clearance violation.
fn violating_trace(board: &mut RoutingBoard, from: PinAt, to: PinAt) -> ItemId {
    trace(board, from.center, to.center, from.layer, from.net)
}

/// Every live item, by its `Debug` text, highest id first.
fn items(board: &RoutingBoard) -> Vec<String> {
    board.items().map(|i| format!("{i:?}")).collect()
}

fn max_id(board: &RoutingBoard) -> i32 {
    board.communication.id_generator.max_generated_id()
}

#[test]
fn a_clean_insert_commits_like_a_snapshot_and_pop() {
    let mut board = j2();
    let mut plain = j2();
    let (pin, _) = foreign_pin_pair(&board);

    let result = board.try_change(
        |b| Ok::<_, Infallible>(clean_stub(b, pin)),
        CheckPolicy::NewItemsOnly,
    );
    let TransactionResult::Committed(id) = result else {
        panic!("a clean stub commits: {result:?}");
    };
    assert!(board.item(id).is_some());

    plain.generate_snapshot();
    clean_stub(&mut plain, pin);
    plain.pop_snapshot();
    assert_eq!(board.state_hash(), plain.state_hash());
    assert_eq!(items(&board), items(&plain));
}

#[test]
fn a_violating_insert_reverts_to_the_before_state() {
    let mut board = j2();
    let (a, b) = foreign_pin_pair(&board);
    let before_items = items(&board);
    let before_hash = board.state_hash();
    let before_id = max_id(&board);

    for check in [CheckPolicy::NewItemsOnly, CheckPolicy::NewAndChangedItems] {
        let result = board.try_change(|bd| Ok::<_, Infallible>(violating_trace(bd, a, b)), check);
        let TransactionResult::Reverted { value, violations } = result else {
            panic!("a trace onto a foreign pad reverts: {result:?}");
        };
        assert!(board.item(value).is_none());
        assert!(
            violations
                .iter()
                .any(|v| v.first_item == value && v.second_item == b.id)
        );
        assert_eq!(items(&board), before_items);
        // Java's undo keeps the id generator, and the reverted change can be
        // redone until the next snapshot drops the redo data. After that the
        // whole state, undo structure included, is the before-state.
        assert!(max_id(&board) > before_id);
        board.generate_snapshot();
        board.pop_snapshot();
        assert_eq!(board.state_hash(), before_hash);
    }
}

#[test]
fn a_closure_error_reverts_unchecked() {
    let mut board = j2();
    let (pin, _) = foreign_pin_pair(&board);
    let before_items = items(&board);
    let result = board.try_change(
        |b| {
            clean_stub(b, pin);
            Err::<(), _>("stage gave up")
        },
        CheckPolicy::NewItemsOnly,
    );
    assert!(matches!(
        result,
        TransactionResult::Aborted("stage gave up")
    ));
    assert_eq!(items(&board), before_items);
}

#[test]
fn a_revert_inside_an_outer_snapshot_keeps_the_outer_undo() {
    let mut board = j2();
    let (a, b) = foreign_pin_pair(&board);
    let initial = items(&board);

    board.generate_snapshot();
    let outer = clean_stub(&mut board, a);
    let with_outer = items(&board);
    let result = board.try_change(
        |bd| Ok::<_, Infallible>(violating_trace(bd, a, b)),
        CheckPolicy::NewItemsOnly,
    );
    assert!(matches!(result, TransactionResult::Reverted { .. }));
    assert_eq!(items(&board), with_outer);
    assert!(board.item(outer).is_some());

    assert!(board.undo(None));
    assert!(board.item(outer).is_none());
    assert_eq!(items(&board), initial);
}

#[test]
fn a_commit_inside_an_outer_snapshot_is_undone_with_it() {
    let mut board = j2();
    let (a, _) = foreign_pin_pair(&board);
    let initial = items(&board);
    board.generate_snapshot();
    let result = board.try_change(
        |bd| Ok::<_, Infallible>(clean_stub(bd, a)),
        CheckPolicy::NewItemsOnly,
    );
    assert!(result.is_committed());
    assert!(board.undo(None));
    assert_eq!(items(&board), initial);
}

#[test]
fn new_and_changed_items_checks_an_item_changed_in_place() {
    let mut board = j2();
    let (a, b) = foreign_pin_pair(&board);
    // A clean stub of b's net inside b's pad; moving it to a's net in place
    // (same id) makes it overlap a foreign pad.
    let stub = trace(
        &mut board,
        b.center,
        IntPoint::new(b.center.x + 10, b.center.y),
        b.layer,
        b.net,
    );
    assert!(board.clearance_violations(stub).is_empty());
    board.set_journaling(true);
    let outer_log = board.take_changes();
    assert!(outer_log.is_empty());

    for (check, expect_revert) in [
        (CheckPolicy::NewItemsOnly, false),
        (CheckPolicy::NewAndChangedItems, true),
    ] {
        let result = board.try_change(
            |bd| {
                bd.assign_net_no(stub, a.net);
                Ok::<_, Infallible>(())
            },
            check,
        );
        assert_eq!(
            matches!(result, TransactionResult::Reverted { .. }),
            expect_revert,
            "{check:?}: {result:?}"
        );
        if !expect_revert {
            // Undo the committed change for the next round.
            assert!(result.is_committed());
            board.generate_snapshot();
            board.assign_net_no(stub, b.net);
            board.pop_snapshot();
        }
    }
    // The caller's journal saw the transaction's change and its revert.
    let log = board.take_changes();
    assert!(log.changed.contains(&stub), "{log:?}");
    assert_eq!(board.item(stub).expect("stub").net_numbers(), &[b.net]);
}

#[test]
fn the_violation_order_is_stable() {
    let run = || {
        let mut board = j2();
        let (a, b) = foreign_pin_pair(&board);
        let result = board.try_change(
            |bd| {
                let first = violating_trace(bd, a, b);
                let second = violating_trace(bd, b, a);
                Ok::<_, Infallible>((first, second))
            },
            CheckPolicy::NewItemsOnly,
        );
        let TransactionResult::Reverted { violations, .. } = result else {
            panic!("reverts");
        };
        violations
            .iter()
            .map(|v| {
                format!(
                    "{:?} {:?} {} {:?}",
                    v.first_item, v.second_item, v.layer, v.shape
                )
            })
            .collect::<Vec<_>>()
    };
    let first = run();
    assert!(first.len() >= 2);
    assert_eq!(first, run());
    // For `tools/determinism-check.sh`: the violations of one run.
    if let Ok(path) = std::env::var("BOARD_TRANSACTION_OUT") {
        std::fs::write(path, first.join("\n")).expect("write the violations");
    }
}
