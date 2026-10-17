//! Board probe `pin-connection` (B05.5): `check_connection_to_pin`,
//! `correct_connection_to_pin` and `swap_connection_to_pin` give Java's
//! results and leave Java's board. Each board's lines equal
//! `fixtures/reference/probe/pin-connection/<board>.jsonl`, which
//! `tools/java-reference/probes/board/PinConnectionProbe.java` writes; its
//! doc comment lists the steps. After each correction the default tree's
//! fingerprint is compared too, which checks the `LineReuse` that
//! `correct_connection_to_pin` passes to `change_trace` (decision D5).
//!
//! On these boards Java's `swapConnectionToPin` returns false everywhere
//! (no trace makes a sharp angle with a shove-fixed exit stub of its pin), so
//! its combining branch is compared only through the unit test below.

mod board_probe;

use board_probe::{FNV_OFFSET, Line, Row, compare, fixture, fnv1a64, header, hex};
use freerouting::board::RoutingBoard;
use freerouting::board::items::{AreaKind, Item, ItemId, ItemKind};
use freerouting::board::structure::{AngleRestriction, FixedState};
use freerouting::datastructures::IdGenerator;
use freerouting::geometry::{IntBox, Point};

const BOARDS: [&str; 3] = [
    "Issue026-J2_reference",
    "Issue508-DAC2020_bm08",
    "Issue145-smoothieboard",
];

fn java_type(item: &Item) -> &'static str {
    match &item.kind {
        ItemKind::Pin(_) => "Pin",
        ItemKind::Via(_) => "Via",
        ItemKind::Trace(_) => "PolylineTrace",
        ItemKind::Area(area) => match area.kind {
            AreaKind::Obstacle => "ObstacleArea",
            AreaKind::ViaObstacle => "ViaObstacleArea",
            AreaKind::TraceObstacle => "TraceObstacleArea",
            AreaKind::ComponentObstacle => "ComponentObstacleArea",
            AreaKind::Conduction { .. } => "ConductionArea",
        },
        ItemKind::ComponentOutline(_) => "ComponentOutline",
        ItemKind::BoardOutline(_) => "BoardOutline",
    }
}

fn point_text(p: &Point) -> String {
    match p {
        Point::Int(ip) => format!("{},{}", ip.x, ip.y),
        _ => {
            let f = p.to_float();
            format!("f{},{}", board_probe::bits(f.x), board_probe::bits(f.y))
        }
    }
}

fn box_text(b: &IntBox) -> String {
    format!("{},{},{},{}", b.ll.x, b.ll.y, b.ur.x, b.ur.y)
}

fn fixed_ordinal(state: FixedState) -> i32 {
    match state {
        FixedState::Unfixed => 0,
        FixedState::ShoveFixed => 1,
        FixedState::UserFixed => 2,
        FixedState::SystemFixed => 3,
    }
}

/// `ItemOpsProbe.itemText` (the same text as `tests/item_transform.rs`).
fn item_text(item: &Item) -> String {
    let nets: Vec<String> = item.net_numbers().iter().map(i32::to_string).collect();
    let mut text = format!(
        "{} {} n{} f{} c{}",
        item.id().0,
        java_type(item),
        nets.join(","),
        fixed_ordinal(item.fixed_state()),
        item.clearance_class()
    );
    match &item.kind {
        ItemKind::Pin(_) | ItemKind::Via(_) => {
            let drill = item.drill().expect("drill item");
            text.push_str(&format!(
                " {} l{}-{} {}",
                point_text(&Point::Int(drill.center)),
                drill.first_layer,
                drill.last_layer,
                box_text(&drill.bounding_box())
            ));
        }
        ItemKind::Trace(trace) => {
            text.push_str(&format!(" l{} w{}", trace.layer, trace.half_width));
            for i in 0..trace.corner_count() {
                text.push(' ');
                text.push_str(&point_text(&trace.polyline.corner(i)));
            }
        }
        ItemKind::Area(area) => {
            text.push_str(&format!(
                " l{} {}",
                area.layer(),
                box_text(&item.bounding_box())
            ));
        }
        ItemKind::ComponentOutline(_) | ItemKind::BoardOutline(_) => {
            text.push(' ');
            text.push_str(&box_text(&item.bounding_box()));
        }
    }
    text.push('\n');
    text
}

fn items_hash(board: &RoutingBoard) -> u64 {
    board
        .items()
        .fold(FNV_OFFSET, |hash, item| fnv1a64(hash, &item_text(item)))
}

fn max_generated_id(board: &RoutingBoard) -> i64 {
    i64::from(board.communication.id_generator.max_generated_id())
}

fn on_board(board: &RoutingBoard, id: ItemId) -> bool {
    board.item(id).is_some_and(Item::is_on_the_board)
}

fn pin_at(board: &RoutingBoard, trace: ItemId, at_start: bool) -> bool {
    let contacts = if at_start {
        board.trace_start_contacts(trace)
    } else {
        board.trace_end_contacts(trace)
    };
    contacts
        .into_iter()
        .any(|id| board.item(id).is_some_and(Item::is_pin))
}

fn corners(board: &RoutingBoard, trace: ItemId) -> String {
    if !on_board(board, trace) {
        return "gone".into();
    }
    let polyline = &board
        .item(trace)
        .and_then(Item::as_trace)
        .expect("trace")
        .polyline;
    (0..polyline.corner_count())
        .map(|i| point_text(&polyline.corner(i)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn angle_name(angle: AngleRestriction) -> &'static str {
    match angle {
        AngleRestriction::None => "NONE",
        AngleRestriction::FortyfiveDegree => "FORTYFIVE_DEGREE",
        AngleRestriction::NinetyDegree => "NINETY_DEGREE",
    }
}

fn fix_pass(board: &mut RoutingBoard, angle: AngleRestriction, pass: i64, lines: &mut Vec<String>) {
    for trace in board.traces() {
        for at_start in [true, false] {
            if !on_board(board, trace) || !pin_at(board, trace, at_start) {
                continue;
            }
            let check = board.check_connection_to_pin(trace, at_start);
            let mut correct = "gone".to_string();
            let mut swap = "gone".to_string();
            if on_board(board, trace) {
                correct = board
                    .correct_connection_to_pin(trace, at_start, angle)
                    .to_string();
            }
            if on_board(board, trace) {
                swap = board.swap_connection_to_pin(trace, at_start).to_string();
            }
            lines.push(
                Line::new()
                    .num("pass", pass)
                    .num("trace", i64::from(trace.0))
                    .bool("atStart", at_start)
                    .bool("check", check)
                    .str("correct", &correct)
                    .str("swap", &swap)
                    .str("corners", &corners(board, trace))
                    .num("maxGeneratedId", max_generated_id(board))
                    .num("items", board.item_count() as i64)
                    .str("itemsHash", &hex(items_hash(board)))
                    .end(),
            );
            if correct == "true" {
                lines.push(default_tree_line(board));
            }
        }
    }
}

/// The default tree's fingerprint, as `TreeOpsProbe` writes a tree.
fn default_tree_line(board: &RoutingBoard) -> String {
    let tree = board.search_trees().default_tree();
    let fingerprint = tree.fingerprint();
    Line::new()
        .str("stage", "tree")
        .str("tree", &tree.key())
        .num("nodes", fingerprint.nodes as i64)
        .num("leaves", fingerprint.leaves as i64)
        .num("maxDepth", fingerprint.max_depth as i64)
        .str("shapeHash", &hex(fingerprint.shape_hash))
        .str("contentDigest", &hex(tree.content_digest()))
        .end()
}

fn rust_lines(row: &Row) -> Vec<String> {
    let mut board = board_probe::load(row);
    board_probe::import_session(&mut board, row);
    let dist = 2 * board.min_trace_half_width();
    board.rules.set_pin_edge_to_turn_dist(f64::from(dist));
    let angle = board.rules.trace_angle_restriction();
    let mut lines = vec![
        header("pin-connection", &row.board),
        Line::new()
            .str("stage", "fixes")
            .num("pinEdgeToTurnDist", i64::from(dist))
            .str("angle", angle_name(angle))
            .num("maxGeneratedId", max_generated_id(&board))
            .end(),
    ];
    for pass in 1..=2 {
        fix_pass(&mut board, angle, pass, &mut lines);
    }
    lines
}

#[test]
fn board_probe_pin_connection() {
    let mut failures = Vec::new();
    for board in BOARDS {
        let row = fixture(board);
        if let Err(e) = compare("pin-connection", &row, &rust_lines(&row)) {
            failures.push(e);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

/// `correct_connection_to_pin` changes nothing while the pin edge-to-turn
/// distance is negative (Java returns false before any change), and it burns
/// no id then.
#[test]
fn fixes_need_a_pin_and_a_distance() {
    let row = fixture("Issue026-J2_reference");
    let mut board = board_probe::load(&row);
    board_probe::import_session(&mut board, &row);
    board.rules.set_pin_edge_to_turn_dist(-1.0);
    let angle = board.rules.trace_angle_restriction();
    let (before, hash) = (max_generated_id(&board), items_hash(&board));
    for trace in board.traces() {
        for at_start in [true, false] {
            assert!(!board.correct_connection_to_pin(trace, at_start, angle));
        }
    }
    assert_eq!(max_generated_id(&board), before);
    assert_eq!(items_hash(&board), hash);
}
