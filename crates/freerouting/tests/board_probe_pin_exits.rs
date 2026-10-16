//! Board probe `pin-exits` (B05.3, B05.4): pin exit restrictions, pin
//! widths, nearest exit corners and directions equal Java's. Each board's
//! lines equal `fixtures/reference/probe/pin-exits/<board>.jsonl`, which
//! `tools/java-reference/probes/board/PinExitsProbe.java` writes; its doc
//! comment lists what a line holds.
//!
//! [`ATAN2_BOUNDARY`] lists the lines that differ only because `atan2` landed
//! on a direction boundary (Java's fdlibm against the platform's), with the
//! raw values. It is empty: no line differs.

mod board_probe;

use board_probe::{Line, bits, compare, fixture, header};
use freerouting::board::RoutingBoard;
use freerouting::board::items::{Item, ItemId};
use freerouting::geometry::jmath::JavaRandom;
use freerouting::geometry::{Direction, FloatPoint, IntPoint, Point, Polyline, Vector};

/// The boards: the three the checklist names and the first `fixtures.tsv`
/// row with a placed back-side component with pins (found by the probe's
/// `--find-back`).
const BOARDS: [&str; 4] = [
    "Issue955-rotated-pad-contacts",
    "Issue145-smoothieboard",
    "Issue026-J2_reference",
    "Issue022-AutoRouter_interrupted",
];

/// (board, pin, layer, Java's line, Rust's line): lines that differ only
/// because `atan2` landed on a direction boundary.
const ATAN2_BOUNDARY: &[(&str, i32, i32, &str, &str)] = &[];

const HALF_WIDTH: i32 = 125;

fn dir(d: Option<&Direction>) -> String {
    match d {
        None => "null".into(),
        Some(d) => match d.get_vector() {
            Vector::Int(v) => format!("{},{}", v.x, v.y),
            other => format!("?{other:?}"),
        },
    }
}

fn fp(p: Option<FloatPoint>) -> String {
    p.map_or_else(|| "null".into(), |p| format!("{},{}", bits(p.x), bits(p.y)))
}

fn pin_lines(board: &RoutingBoard, id: ItemId, random: &mut JavaRandom, lines: &mut Vec<String>) {
    let item = board.item(id).expect("pin");
    let center = item.drill().expect("pin").center;
    let points: Vec<IntPoint> = (0..5)
        .map(|_| {
            let x = center.x + random.next_int(10001) - 5000;
            let y = center.y + random.next_int(10001) - 5000;
            IntPoint::new(x, y)
        })
        .collect();
    lines.push(
        Line::new()
            .str("pin", &id.0.to_string())
            .bool("front", board.pin_is_placed_on_front(id))
            .bool("restricted", board.has_trace_exit_restrictions(id))
            .num("first", i64::from(item.first_layer()))
            .num("last", i64::from(item.last_layer()))
            .end(),
    );
    for layer in item.first_layer()..=item.last_layer() {
        let exits: Vec<String> = board
            .trace_exit_restrictions(id, layer)
            .iter()
            .map(|e| format!("{},{}", dir(Some(&e.direction)), bits(e.min_length)))
            .collect();
        let corners: Vec<String> = points
            .iter()
            .map(|p| fp(board.nearest_trace_exit_corner(id, p.to_float(), HALF_WIDTH, layer)))
            .collect();
        let directions: Vec<String> = points
            .iter()
            .map(|p| {
                if *p == center {
                    return "same".into();
                }
                let polyline = Polyline::from_points(&[Point::Int(center), Point::Int(*p)]);
                dir(board
                    .calc_nearest_exit_restriction_direction(id, &polyline, HALF_WIDTH, layer)
                    .as_ref())
            })
            .collect();
        lines.push(
            Line::new()
                .str("pin", &id.0.to_string())
                .num("layer", i64::from(layer))
                .str("exits", &exits.join(";"))
                .dbl("minWidth", board.pin_min_width(id, layer))
                .dbl("maxWidth", board.pin_max_width(id, layer))
                .num(
                    "neckdown",
                    i64::from(board.trace_neckdown_half_width(id, layer)),
                )
                .str("corners", &corners.join(";"))
                .str("directions", &directions.join(";"))
                .end(),
        );
    }
}

fn rust_lines(board_name: &str) -> Vec<String> {
    let row = fixture(board_name);
    let board = board_probe::load(&row);
    let pins: Vec<ItemId> = board.items().filter(|i| i.is_pin()).map(Item::id).collect();
    let role = if board_name == BOARDS[3] {
        "first-back-side"
    } else {
        "listed"
    };
    let mut lines = vec![
        header("pin-exits", &row.board),
        Line::new()
            .str("stage", "pins")
            .str("role", role)
            .num("pinCount", pins.len() as i64)
            .end(),
    ];
    let mut random = JavaRandom::new(3);
    for id in pins {
        pin_lines(&board, id, &mut random, &mut lines);
    }
    lines
}

#[test]
fn board_probe_pin_exits() {
    assert!(ATAN2_BOUNDARY.is_empty());
    let mut failures = Vec::new();
    for board in BOARDS {
        let row = fixture(board);
        if let Err(e) = compare("pin-exits", &row, &rust_lines(board)) {
            failures.push(e);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

/// The back-side board really has back-side pins with restrictions, so the
/// mirrored path is covered.
#[test]
fn back_side_board_has_restricted_back_pins() {
    let board = board_probe::load(&fixture(BOARDS[3]));
    let back: Vec<ItemId> = board
        .pins()
        .into_iter()
        .filter(|&id| !board.pin_is_placed_on_front(id))
        .collect();
    assert!(!back.is_empty());
    assert!(back.iter().any(|&id| board.has_trace_exit_restrictions(id)));
}
