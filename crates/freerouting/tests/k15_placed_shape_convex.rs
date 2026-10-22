//! K15.2: `router.java_quirks.placed_shape_convex` on the board.
//!
//! Read with the switch `false`, C65's fabrication outline in bug 1's repro
//! (`tests/data/k15/`, KiCad 10.0.7's DSN export) is the convex hull of its
//! rounded corners, so no item's box is unbounded and K15.1's warning is not
//! given. Unset, the outline is Java's unbounded simplex (K15.1's tests,
//! `tests/unbounded_item_box.rs`). A board switched after reading has the shapes
//! of a board read with the switch (`board/placed_shapes.rs`).

use freerouting::board::RoutingBoard;
use freerouting::io::specctra::{BoardReadResult, read_board_from_str_with};
use freerouting::settings::JavaQuirks;

const ID: &str = "placed_shape_convex";

fn board(name: &str, quirks: JavaQuirks) -> RoutingBoard {
    let path = format!("{}/tests/data/k15/{name}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).expect("fixture");
    match read_board_from_str_with(&text, quirks) {
        BoardReadResult::Success { board, .. } => *board,
        _ => panic!("{name} does not read"),
    }
}

fn fixed() -> JavaQuirks {
    JavaQuirks::JAVA.with(ID, false).expect("a known id")
}

/// Every item's id and box, in item order.
fn boxes(board: &RoutingBoard) -> Vec<(i32, [i32; 4])> {
    board
        .items()
        .map(|i| {
            let b = i.bounding_box();
            (i.id().0, [b.ll.x, b.ll.y, b.ur.x, b.ur.y])
        })
        .collect()
}

#[test]
fn the_c65_outline_is_bounded_with_the_switch_off() {
    for name in [
        "c65-nm-position.kicad10.dsn",
        "far-nm.kicad10.dsn",
        "c65-snapped.kicad10.dsn",
    ] {
        let mut board = board(name, fixed());
        assert!(
            board
                .items()
                .all(|i| !i.bounding_box().has_infinite_coordinate()),
            "{name}"
        );
        let warnings = board.expand_bounding_box_to_include_all_items();
        assert!(warnings.is_empty(), "{name}: {warnings:?}");
    }
    // The outline's box is the box of its rounded corners
    // (`freerouting-geometry/tests/placed_shape_convex.rs`).
    let java = board("c65-nm-position.kicad10.dsn", JavaQuirks::JAVA);
    let fixed = board("c65-nm-position.kicad10.dsn", fixed());
    let changed: Vec<_> = boxes(&java)
        .into_iter()
        .zip(boxes(&fixed))
        .filter(|(a, b)| a != b)
        .collect();
    assert_eq!(changed.len(), 1, "only C65's outline changes: {changed:?}");
    let ((id, java_box), (_, fixed_box)) = changed[0];
    assert_eq!(&java_box[2..], &[i32::MAX, i32::MAX], "item {id}");
    assert_eq!(fixed_box, [522059, -1102908, 623010, -1001932], "item {id}");
}

#[test]
fn a_board_switched_after_reading_has_the_shapes_of_one_read_with_the_switch() {
    let read_fixed = board("far-nm.kicad10.dsn", fixed());
    let mut switched = board("far-nm.kicad10.dsn", JavaQuirks::JAVA);
    let java = boxes(&switched);
    switched.set_java_quirks(fixed());
    assert_eq!(boxes(&switched), boxes(&read_fixed));
    assert!(
        switched
            .items()
            .all(|i| !i.bounding_box().has_infinite_coordinate())
    );
    // The tree finds the outline at its new box.
    let warnings = switched.expand_bounding_box_to_include_all_items();
    assert!(warnings.is_empty(), "{warnings:?}");
    // And back to Java's.
    switched.set_java_quirks(JavaQuirks::JAVA);
    assert_eq!(boxes(&switched), java);
}
