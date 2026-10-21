//! K15.1: an item whose box is not bounded is left out of the board's
//! bounding box, with a warning that names it.
//!
//! The boards are KiCad 10.0.7's own DSN export of bug 1's repro
//! (`tests/data/k15/README.md`). C65's F.Fab circle places to a simplex with
//! three parallel lines, whose box reaches `i32::MAX`. Java at the pin
//! (`BasicBoard.expandBoundingBoxToIncludeAllItems`, `IntBox.offset`) takes
//! the union and wraps, so the board box is empty; it then aborts in
//! `DrillPageArray` with `NegativeArraySizeException`. The port leaves the
//! item out (DIVERGENCES, "freerouting (board model)").

use freerouting::board::RoutingBoard;
use freerouting::geometry::IntBox;
use freerouting::io::specctra::{BoardReadResult, read_board_from_str};

fn board(name: &str) -> RoutingBoard {
    let path = format!("{}/tests/data/k15/{name}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).expect("fixture");
    match read_board_from_str(&text) {
        BoardReadResult::Success { board, .. } => *board,
        _ => panic!("{name} does not read"),
    }
}

fn is_empty(b: &IntBox) -> bool {
    b.ll.x > b.ur.x || b.ll.y > b.ur.y
}

#[test]
fn the_unbounded_outline_is_left_out_and_named() {
    let mut board = board("c65-nm-position.kicad10.dsn");
    let unbounded: Vec<_> = board
        .items()
        .filter(|i| i.bounding_box().has_infinite_coordinate())
        .map(|i| (i.id(), i.as_component_outline().is_some()))
        .collect();
    assert_eq!(unbounded.len(), 1, "one unbounded item: {unbounded:?}");
    assert!(unbounded[0].1, "it is a component outline");
    let warnings = board.expand_bounding_box_to_include_all_items();
    let bounds = board.bounding_box();
    assert!(!is_empty(&bounds), "{bounds:?}");
    assert!(!bounds.has_infinite_coordinate(), "{bounds:?}");
    // every other item, the pads among them, is inside the box
    for item in board.items() {
        let b = item.bounding_box();
        if item.id() == unbounded[0].0 || is_empty(&b) {
            continue;
        }
        assert!(
            bounds.ll.x <= b.ll.x
                && bounds.ll.y <= b.ll.y
                && b.ur.x <= bounds.ur.x
                && b.ur.y <= bounds.ur.y,
            "{:?} {b:?} outside {bounds:?}",
            item.id()
        );
    }
    assert_eq!(
        warnings,
        vec![
            "The fabrication outline of component C65 at (57251.006, -105242.039) um has an \
             unbounded shape after placement; it is left out of the board's bounding box."
                .to_string()
        ]
    );
}

#[test]
fn the_snapped_board_is_unchanged() {
    let mut board = board("c65-snapped.kicad10.dsn");
    assert!(
        board
            .items()
            .all(|i| !i.bounding_box().has_infinite_coordinate())
    );
    let before = board.bounding_box();
    let warnings = board.expand_bounding_box_to_include_all_items();
    assert!(warnings.is_empty(), "{warnings:?}");
    // the box of the DSN boundary already holds every item: Java's result
    assert_eq!(board.bounding_box(), before);
    assert!(!is_empty(&before));
}

#[test]
fn the_far_board_box_holds_the_far_nets() {
    let mut board = board("far-nm.kicad10.dsn");
    let warnings = board.expand_bounding_box_to_include_all_items();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("component C65 "), "{warnings:?}");
    let bounds = board.bounding_box();
    assert!(!is_empty(&bounds) && !bounds.has_infinite_coordinate());
}
