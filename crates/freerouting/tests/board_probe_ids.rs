//! Board probe `ids` (B01.1): the Java and Rust loaders give every item the
//! same id. Per fixture and stage (`load`, then `ses`), `maxGeneratedId`, the
//! item count and FNV-1a 64 over `"<id> <type>"` in item-list order equal
//! `fixtures/reference/probe/ids/<board>.jsonl`, which
//! `tools/java-reference/probes/board/IdsProbe.java` writes.

mod board_probe;

use board_probe::{FNV_OFFSET, Line, Row, compare, fixtures, fnv1a64, header, hex};
use freerouting::board::RoutingBoard;
use freerouting::board::items::{AreaKind, Item, ItemKind};
use freerouting::datastructures::IdGenerator;

/// Fixtures and stages where Rust's ids differ from Java's on purpose.
const EXPECTED_MISMATCHES: &[(&str, &str, &str)] = &[(
    "Issue676-ch32v-tx118s",
    "ses",
    "Java's SesReader fails on all 48 wires with a ClassCastException (RationalVector to \
     IntVector) and imports none; the port imports them (docs/DIVERGENCES.md)",
)];

/// The Java simple class name of `item`.
fn java_type(item: &Item) -> &'static str {
    match &item.kind {
        ItemKind::Pin(_) => "Pin",
        ItemKind::Via(_) => "Via",
        ItemKind::Trace(_) => "PolylineTrace",
        ItemKind::Area(area) => match area.kind {
            AreaKind::Obstacle => "ObstacleArea",
            AreaKind::ViaObstacle => "ViaObstacleArea",
            AreaKind::ComponentObstacle => "ComponentObstacleArea",
            AreaKind::Conduction { .. } => "ConductionArea",
        },
        ItemKind::ComponentOutline(_) => "ComponentOutline",
        ItemKind::BoardOutline(_) => "BoardOutline",
    }
}

fn stage(name: &str, board: &RoutingBoard) -> String {
    let mut hash = FNV_OFFSET;
    let mut count = 0;
    for item in board.items() {
        hash = fnv1a64(hash, &format!("{} {}\n", item.id().0, java_type(item)));
        count += 1;
    }
    Line::new()
        .str("stage", name)
        .num(
            "maxGeneratedId",
            i64::from(board.communication.id_generator.max_generated_id()),
        )
        .num("items", count)
        .str("fnv", &hex(hash))
        .end()
}

fn rust_lines(row: &Row) -> Vec<String> {
    let mut board = board_probe::load(row);
    let mut lines = vec![header("ids", &row.board), stage("load", &board)];
    if board_probe::import_session(&mut board, row).is_some() {
        lines.push(stage("ses", &board));
    }
    lines
}

/// The stage of the first line that differs, from `compare`'s message.
fn differing_stage(message: &str) -> Option<String> {
    let java_line = message
        .lines()
        .find_map(|l| l.trim().strip_prefix("Java: {"))?;
    let rest = java_line.strip_prefix("\"stage\":\"")?;
    Some(rest.split('"').next()?.to_string())
}

#[test]
fn ids_match_java() {
    let mut failures = Vec::new();
    let mut seen = Vec::new();
    for row in fixtures() {
        let Err(message) = compare("ids", &row, &rust_lines(&row)) else {
            continue;
        };
        let stage = differing_stage(&message).unwrap_or_default();
        if let Some(entry) = EXPECTED_MISMATCHES
            .iter()
            .find(|(board, s, _)| *board == row.board && *s == stage)
        {
            seen.push(*entry);
        } else {
            failures.push(message);
        }
    }
    for entry in EXPECTED_MISMATCHES {
        if !seen.contains(entry) {
            failures.push(format!(
                "listed mismatch {} {} no longer occurs; remove it from EXPECTED_MISMATCHES",
                entry.0, entry.1
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

/// The `// BEGIN Boards` .. `// END Boards` block of a probe source.
fn boards_block(source: &str) -> Option<&str> {
    let start = source.find("// BEGIN Boards")?;
    let end = source[start..].find("// END Boards")? + start;
    Some(&source[start..end])
}

#[test]
fn probe_loader_copies_identical() {
    let dir = board_probe::repo_root().join("tools/java-reference/probes/board");
    let reference = std::fs::read_to_string(dir.join("IdsProbe.java")).expect("IdsProbe.java");
    let reference = boards_block(&reference).expect("IdsProbe.java has a Boards block");
    let mut probes: Vec<_> = std::fs::read_dir(&dir)
        .expect("probe directory")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.to_string_lossy().ends_with("Probe.java"))
        .collect();
    probes.sort();
    for path in probes {
        let source = std::fs::read_to_string(&path).expect("probe source");
        assert_eq!(
            boards_block(&source),
            Some(reference),
            "{}: its Boards block differs from IdsProbe.java's",
            path.display()
        );
    }
}

// B01.2: ids are taken where Java takes them.

fn two_pin_board() -> RoutingBoard {
    board_probe::load(&board_probe::fixture("scoring-perfect-two-pin"))
}

fn zero_length_polyline(board: &RoutingBoard) -> freerouting::geometry::Polyline {
    let pin = board.items().find(|i| i.is_pin()).expect("a pin");
    let center = freerouting::geometry::Point::Int(pin.drill().expect("drill").center);
    // A closed triangle: at least two corners, first corner == last corner
    // (two equal points alone make no polyline, in Java as here).
    let c = center.expect_int();
    let p = |dx: i32, dy: i32| {
        freerouting::geometry::Point::Int(freerouting::geometry::IntPoint::new(c.x + dx, c.y + dy))
    };
    freerouting::geometry::Polyline::from_points(&[center.clone(), p(1000, 0), p(0, 1000), center])
}

#[test]
fn zero_length_unfixed_trace_consumes_an_id() {
    use freerouting::board::structure::FixedState;
    let mut board = two_pin_board();
    let polyline = zero_length_polyline(&board);
    let before_id = board.communication.id_generator.max_generated_id();
    let before_count = board.item_count();
    let result =
        board.insert_trace_without_cleaning(polyline, 0, 100, &[1], 0, FixedState::Unfixed);
    assert_eq!(result, None);
    assert_eq!(
        board.communication.id_generator.max_generated_id(),
        before_id + 1
    );
    assert_eq!(board.item_count(), before_count);
}

#[test]
fn zero_length_user_fixed_trace_is_inserted() {
    use freerouting::board::structure::FixedState;
    let mut board = two_pin_board();
    let polyline = zero_length_polyline(&board);
    let before_id = board.communication.id_generator.max_generated_id();
    let id = board
        .insert_trace_without_cleaning(polyline, 0, 100, &[1], 0, FixedState::UserFixed)
        .expect("a user fixed zero length trace is inserted");
    assert_eq!(id.0, before_id + 1);
    assert!(board.item(id).is_some_and(Item::is_trace));
}
