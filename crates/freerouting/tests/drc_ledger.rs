//! The routing ledger (B01.6): incremental incomplete-connection counts that
//! always equal the full count of the `DesignRulesChecker`.
//!
//! Java tests ported (`app.freerouting.drc`, at `cda540f`):
//! `NetRoutingLedgerTest.incompleteCountMatchesFullScanAcrossInsertAndRemove`.
//! It lives here rather than in `java_ports.rs`, which another session was
//! editing when B01.6 landed.

mod board_probe;

use freerouting::board::RoutingBoard;
use freerouting::board::structure::FixedState;
use freerouting::drc::DesignRulesChecker;
use freerouting::geometry::{Point, Polyline};

fn assert_matches_full_scan(board: &RoutingBoard, context: &str) {
    let mut checker = DesignRulesChecker::new(board);
    assert_eq!(
        board.ledger_incomplete_count(),
        checker.incomplete_count(),
        "{context}: incomplete count"
    );
    assert_eq!(
        board.ledger_maximum_connections(),
        checker.max_connections(),
        "{context}: maximum connections"
    );
    assert_eq!(
        board.ledger_incomplete_net_numbers(),
        checker.incomplete_net_numbers(),
        "{context}: incomplete nets"
    );
}

/// Port of `NetRoutingLedgerTest.incompleteCountMatchesFullScanAcrossInsertAndRemove`.
#[test]
fn incomplete_count_matches_full_scan_across_insert_and_remove() {
    let mut board = board_probe::load(&board_probe::fixture("scoring-perfect-two-pin"));
    assert_matches_full_scan(&board, "loaded");

    let pins: Vec<_> = board.items().filter(|i| i.is_pin()).collect();
    let first = pins[0].drill().expect("drill").center;
    let second = pins[1].drill().expect("drill").center;
    let net = pins[0].net_numbers()[0];
    let class = board.rules.net_class_of(net).expect("net class");
    let half_width = class.trace_half_width(0);
    let clearance_class = class.trace_clearance_class();
    board.insert_trace_without_cleaning(
        Polyline::from_points(&[Point::Int(first), Point::Int(second)]),
        0,
        half_width,
        &[net],
        clearance_class,
        FixedState::Unfixed,
    );
    assert_matches_full_scan(&board, "after insert");
    assert_eq!(board.ledger_incomplete_count(), 0);

    let trace = board.traces()[0];
    board.remove_item(trace);
    assert_matches_full_scan(&board, "after remove");
    assert_eq!(board.ledger_incomplete_count(), 1);
}

#[test]
fn ledger_matches_full_count() {
    let rows = board_probe::fixtures();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let chunk = rows.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        for rows in rows.chunks(chunk) {
            scope.spawn(move || {
                for row in rows {
                    let mut board = board_probe::load(row);
                    assert_matches_full_scan(&board, &format!("{} loaded", row.board));
                    if board_probe::import_session(&mut board, row).is_some() {
                        assert_matches_full_scan(&board, &format!("{} with session", row.board));
                    }
                }
            });
        }
    });
}

#[test]
fn ledger_matches_full_count_after_undo_redo_and_net_change() {
    for name in [
        "Issue026-J2_reference",
        "Issue508-DAC2020_bm08",
        "Issue413-test",
    ] {
        let row = board_probe::fixture(name);
        let mut board = board_probe::load(&row);
        board_probe::import_session(&mut board, &row).expect("a session");
        // Build the ledger, then change the board under it.
        assert_matches_full_scan(&board, &format!("{name} with session"));

        board.generate_snapshot();
        for id in board.traces().into_iter().take(10) {
            // The session's wires are user fixed, which remove_item refuses.
            board.unfix(id);
            assert!(board.remove_item(id));
        }
        assert_matches_full_scan(&board, &format!("{name} removed 10 traces"));
        assert!(board.undo(None));
        assert_matches_full_scan(&board, &format!("{name} undo"));
        assert!(board.redo(None));
        assert_matches_full_scan(&board, &format!("{name} redo"));
        assert!(board.undo(None));
        assert_matches_full_scan(&board, &format!("{name} undo again"));

        let max_net = board.rules.nets.max_net_number();
        let vias = board.vias();
        for (i, id) in vias.iter().take(5).enumerate() {
            let net = (i as i32 % max_net.max(1)) + 1;
            board.assign_net_no(*id, net);
        }
        assert_matches_full_scan(&board, &format!("{name} net change"));
        let traces = board.traces();
        if let Some(id) = traces.first() {
            let net = board.item(*id).expect("trace").net_numbers()[0];
            board.remove_from_net(*id, net);
        }
        assert_matches_full_scan(&board, &format!("{name} remove from net"));
    }
}

#[test]
fn ledger_restarts_after_delete_all_and_restore() {
    let row = board_probe::fixture("Issue026-J2_reference");
    let mut board = board_probe::load(&row);
    board_probe::import_session(&mut board, &row).expect("a session");
    assert_matches_full_scan(&board, "with session");
    assert!(board.routing_ledger().is_built());
    let copy = board.deep_copy();
    assert!(
        !copy.routing_ledger().is_built(),
        "a restore starts unbuilt"
    );
    board.delete_all_traces_and_vias();
    assert!(!board.routing_ledger().is_built(), "delete all invalidates");
    assert_matches_full_scan(&board, "after delete all");
    assert_matches_full_scan(&copy, "copy");
}

#[test]
fn ledger_insert_and_read_is_fast() {
    // B01.6 asks for under 1 ms per insert plus read on Issue190 in release;
    // this reports the time (debug builds are slower).
    let row = board_probe::fixture("Issue190-processor.Z80");
    let mut board = board_probe::load(&row);
    let _ = board.ledger_incomplete_count();
    let pins: Vec<_> = board.items().filter(|i| i.is_pin()).collect();
    let (a, net) = (
        pins[0].drill().expect("drill").center,
        pins[0].net_numbers().first().copied().unwrap_or(1),
    );
    let b = freerouting::geometry::IntPoint::new(a.x + 100, a.y);
    let start = std::time::Instant::now();
    board.insert_trace_without_cleaning(
        Polyline::from_points(&[Point::Int(a), Point::Int(b)]),
        0,
        100,
        &[net],
        0,
        FixedState::Unfixed,
    );
    let count = board.ledger_incomplete_count();
    eprintln!("insert + ledger read: {:?} ({count})", start.elapsed());
    assert_matches_full_scan(&board, "Issue190 after insert");
}
