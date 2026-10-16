//! The route stubs list against Java (B08.3).
//!
//! The probe `route-stubs` (`tools/java-reference/probes/board/RouteStubsProbe.java`) holds a
//! copy of `WindowRouteStubs.fillList` and `RouteStubInfo.compareTo` as a stand-in for the
//! window, which needs a GUI. Per board it lists the stubs after the Java-routed session is
//! imported, and again after every 7th trace (item-list order) was removed, which leaves stubs
//! on boards the router completed. The Rust list must be equal line for line, including the
//! user coordinates as raw `double` bits.

mod board_probe;

use board_probe::{Line, Row, compare, fixture, header};
use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::drc::{RouteStub, RouteStubKind, route_stubs};

const KIND: &str = "route-stubs";
const BOARDS: [&str; 3] = [
    "Issue026-J2_reference",
    "Issue508-DAC2020_bm05",
    "Issue690-ecc83",
];

fn dump(lines: &mut Vec<String>, suffix: &str, board: &RoutingBoard) -> usize {
    let stubs = route_stubs(board);
    lines.push(
        Line::new()
            .str("stage", &format!("stubs{suffix}"))
            .num("count", stubs.len() as i64)
            .end(),
    );
    for stub in &stubs {
        lines.push(stub_line(suffix, stub));
    }
    stubs.len()
}

fn stub_line(suffix: &str, stub: &RouteStub) -> String {
    Line::new()
        .str("stage", &format!("stub{suffix}"))
        .num("item", i64::from(stub.item.0))
        .str(
            "type",
            match stub.kind {
                RouteStubKind::Trace => "trace",
                RouteStubKind::Via => "via",
            },
        )
        .str("net", &stub.net_name)
        .dbl("x", stub.user_location.x)
        .dbl("y", stub.user_location.y)
        .dbl("boardX", stub.location.x)
        .dbl("boardY", stub.location.y)
        .num("layer", i64::from(stub.layer))
        .end()
}

fn rust_lines(row: &Row) -> (Vec<String>, usize, usize) {
    let mut board = board_probe::load(row);
    board_probe::import_session(&mut board, row);
    let mut lines = vec![header(KIND, &row.board)];
    let before = dump(&mut lines, "", &board);
    let traces: Vec<ItemId> = board.traces();
    for id in traces.into_iter().step_by(7) {
        board.unfix(id);
        board.remove_item(id);
    }
    let after = dump(&mut lines, "-after-removal", &board);
    (lines, before, after)
}

/// The stub counts Java's output records, before and after the removal.
fn java_counts(board: &str) -> (usize, usize) {
    let path = board_probe::repo_root()
        .join("fixtures/reference/probe")
        .join(KIND)
        .join(format!("{board}.jsonl"));
    let text = std::fs::read_to_string(path).expect("Java output");
    let count = |stage: &str| -> usize {
        let key = format!("{{\"stage\":\"{stage}\",\"count\":");
        let line = text
            .lines()
            .find(|l| l.starts_with(&key))
            .expect("count line");
        line[key.len()..]
            .trim_end_matches('}')
            .parse()
            .expect("count")
    };
    (count("stubs"), count("stubs-after-removal"))
}

fn check(board: &str) {
    let row = fixture(board);
    let (lines, before, after) = rust_lines(&row);
    if let Err(message) = compare(KIND, &row, &lines) {
        panic!("{message}");
    }
    assert_eq!((before, after), java_counts(board));
}

#[test]
fn route_stubs_match_java_on_j2() {
    check(BOARDS[0]);
}

/// The one board whose routed session has stubs of its own.
#[test]
fn route_stubs_match_java_on_dac2020_bm05() {
    let (before, after) = check_counts(BOARDS[1]);
    assert!(before > 0 && after > before);
}

#[test]
fn route_stubs_match_java_on_ecc83() {
    check(BOARDS[2]);
}

fn check_counts(board: &str) -> (usize, usize) {
    check(board);
    java_counts(board)
}
