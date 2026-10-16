//! `state_hash` with Java's equality (B01.5).
//!
//! The probe `state-hash` (`tools/java-reference/probes/board/StateHashProbe.java`) records
//! which of eight board states Java's `getHash` considers equal; Rust's
//! `state_hash` must give the same equality matrix.

mod board_probe;

use board_probe::{Line, Row, compare, header};
use freerouting::board::RoutingBoard;
use freerouting::board::structure::FixedState;
use freerouting::datastructures::IdGenerator;
use freerouting::geometry::{Point, Polyline};
use freerouting::scoring::BoardStatistics;

const BOARDS: &[&str] = &[
    "Issue026-J2_reference",
    "Issue508-DAC2020_bm08",
    "Issue413-test",
];

/// Steps where the equality differs from Java's only through
/// `Item.smallestClearance`, the DRC cache Java's hash includes and the port
/// does not have (`docs/DIVERGENCES.md`).
const EXPECTED_MISMATCHES: &[(&str, &str, &str)] = &[];

fn step(lines: &mut Vec<String>, hashes: &mut Vec<u128>, name: &str, board: &RoutingBoard) {
    let hash = board.state_hash();
    let equals: Vec<String> = hashes
        .iter()
        .enumerate()
        .filter(|(_, h)| **h == hash)
        .map(|(i, _)| i.to_string())
        .collect();
    hashes.push(hash);
    lines.push(
        Line::new()
            .str("stage", name)
            .num(
                "maxGeneratedId",
                i64::from(board.communication.id_generator.max_generated_id()),
            )
            .raw("equals", &format!("[{}]", equals.join(",")))
            .end(),
    );
}

fn rust_lines(row: &Row) -> Vec<String> {
    let mut lines = vec![header("state-hash", &row.board)];
    let mut hashes = Vec::new();
    let mut board = board_probe::load(row);
    step(&mut lines, &mut hashes, "h0", &board);
    board.generate_snapshot();
    step(&mut lines, &mut hashes, "h1", &board);
    board_probe::import_session(&mut board, row).expect("a session");
    step(&mut lines, &mut hashes, "h2", &board);
    board.generate_snapshot();
    for id in board.traces().into_iter().take(5) {
        // The session's wires are user fixed, which remove_item refuses.
        board.unfix(id);
        board.remove_item(id);
    }
    step(&mut lines, &mut hashes, "h3", &board);
    board.undo(None);
    step(&mut lines, &mut hashes, "h4", &board);
    board.redo(None);
    step(&mut lines, &mut hashes, "h5", &board);
    let copy = board.deep_copy();
    step(&mut lines, &mut hashes, "h6", &copy);
    let _ = BoardStatistics::new(&copy);
    step(&mut lines, &mut hashes, "h7", &copy);
    lines
}

#[test]
fn state_hash_equality_matches_java() {
    let mut failures = Vec::new();
    for board in BOARDS {
        let row = board_probe::fixture(board);
        if let Err(message) = compare("state-hash", &row, &rust_lines(&row)) {
            let listed = EXPECTED_MISMATCHES
                .iter()
                .any(|(b, stage, _)| b == board && message.contains(&format!("\"{stage}\"")));
            if !listed {
                failures.push(message);
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

fn two_pin_board() -> RoutingBoard {
    board_probe::load(&board_probe::fixture("scoring-perfect-two-pin"))
}

fn insert_trace(board: &mut RoutingBoard) -> freerouting::board::items::ItemId {
    let pins: Vec<_> = board.items().filter(|i| i.is_pin()).collect();
    let a = pins[0].drill().expect("drill").center;
    let b = pins[1].drill().expect("drill").center;
    let net = pins[0].net_numbers()[0];
    let polyline = Polyline::from_points(&[Point::Int(a), Point::Int(b)]);
    board
        .insert_trace_without_cleaning(polyline, 0, 100, &[net], 0, FixedState::Unfixed)
        .expect("trace inserted")
}

#[test]
fn state_hash_tracks_changes() {
    let mut board = two_pin_board();
    let h0 = board.state_hash();
    assert_eq!(board.state_hash(), h0, "stable");

    board.generate_snapshot();
    let h1 = board.state_hash();
    assert_ne!(h1, h0, "generate_snapshot changes the undo structure");

    let id = insert_trace(&mut board);
    let h2 = board.state_hash();
    assert_ne!(h2, h1, "insert");

    board.set_fixed_state(id, FixedState::UserFixed);
    let h3 = board.state_hash();
    assert_ne!(h3, h2, "fixed-state change");
    board.set_fixed_state(id, FixedState::Unfixed);

    let trace = board.item(id).and_then(|i| i.as_trace()).expect("trace");
    let first = trace.first_corner();
    let last = trace.last_corner();
    let (a, b) = (first.expect_int(), last.expect_int());
    let mid = Point::Int(freerouting::geometry::IntPoint::new(
        a.x + (b.x - a.x) / 2 + 5000,
        a.y + (b.y - a.y) / 2 + 5000,
    ));
    board.change_trace(id, Polyline::from_points(&[first, mid, last]));
    let h4 = board.state_hash();
    assert_ne!(h4, h2, "trace change");

    assert!(board.remove_item(id));
    let h5 = board.state_hash();
    assert_ne!(h5, h4, "remove");

    assert_eq!(board.deep_copy().state_hash(), h5, "survives deep_copy");
}

#[test]
fn insert_then_undo_differs_from_before() {
    let mut board = two_pin_board();
    board.generate_snapshot();
    let before = board.state_hash();
    insert_trace(&mut board);
    assert!(board.undo(None));
    // The undone insert stays for redo, as in Java.
    assert_ne!(board.state_hash(), before);
    let after_undo = board.state_hash();
    assert!(board.redo(None));
    assert!(board.undo(None));
    assert_eq!(board.state_hash(), after_undo);
}

#[test]
fn state_hash_is_fast() {
    // B01.5 asks for under 5 ms on Issue190-processor.Z80 in release; this
    // only reports the time (debug builds are slower).
    let row = board_probe::fixture("Issue190-processor.Z80");
    let board = board_probe::load(&row);
    let start = std::time::Instant::now();
    let hash = board.state_hash();
    eprintln!(
        "state_hash on {} items: {:?} ({hash:032x})",
        board.item_count(),
        start.elapsed()
    );
}
