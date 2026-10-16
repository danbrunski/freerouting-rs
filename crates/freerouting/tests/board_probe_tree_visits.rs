//! Board probe `tree-visits` (B02.7): the units of the board's work meter
//! (search-tree nodes visited) per overlap query equal Java's `WorkMeter`
//! count, in `fixtures/reference/probe/tree-visits/<board>.jsonl`, which
//! `tools/java-reference/probes/board/TreeVisitsProbe.java` writes at a pin
//! with `WorkMeter` (`a773a70ec`). 1 000 seeded queries per stage (`load`,
//! then `ses`) on the default tree; see the probe's doc comment.

mod board_probe;

use board_probe::{Line, Row, compare, fixture, header};
use freerouting::board::RoutingBoard;
use freerouting::board::search::TreeEntry;
use freerouting::geometry::jmath::JavaRandom;
use freerouting::geometry::{IntBox, TileShape};

const BOARDS: &[&str] = &[
    "Issue026-J2_reference",
    "Issue508-DAC2020_bm08",
    "Issue145-smoothieboard",
    "Issue190-processor.Z80",
];

fn queries(lines: &mut Vec<String>, stage: &str, board: &RoutingBoard) {
    let tree = board.search_trees().default_tree();
    let meter = board.search_trees().work_meter();
    let bounds = board.bounding_box();
    let width = (bounds.ur.x - bounds.ll.x).max(1);
    let height = (bounds.ur.y - bounds.ll.y).max(1);
    let max_half = (width.min(height) / 20).max(1);
    let mut random = JavaRandom::new(7);
    let mut sum = 0;
    let mut query_lines = Vec::new();
    for q in 0..1000 {
        let x = bounds.ll.x + random.next_int(width);
        let y = bounds.ll.y + random.next_int(height);
        let half = 1 + random.next_int(max_half);
        let shape = TileShape::IntOctagon(
            IntBox::new(x - half, y - half, x + half, y + half).to_int_octagon(),
        );
        let mut found: Vec<TreeEntry> = Vec::new();
        let before = meter.units();
        tree.overlapping_tree_entries(&shape, -1, &[], board.item_list(), &mut found);
        let units = meter.units() - before;
        sum += units;
        query_lines.push(
            Line::new()
                .str("stage", stage)
                .num("q", q)
                .num("units", units as i64)
                .num("found", found.len() as i64)
                .end(),
        );
    }
    lines.push(
        Line::new()
            .str("stage", stage)
            .num("queries", 1000)
            .num("sum", sum as i64)
            .end(),
    );
    lines.extend(query_lines);
}

fn rust_lines(row: &Row) -> Vec<String> {
    let mut lines = vec![header("tree-visits", &row.board)];
    let mut board = board_probe::load(row);
    queries(&mut lines, "load", &board);
    board_probe::import_session(&mut board, row);
    queries(&mut lines, "ses", &board);
    lines
}

#[test]
fn tree_visits_match_java() {
    let mut failures = Vec::new();
    for board in BOARDS {
        let row = fixture(board);
        if let Err(message) = compare("tree-visits", &row, &rust_lines(&row)) {
            failures.push(message);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
