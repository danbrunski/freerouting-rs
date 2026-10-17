//! A trace normalization that removes the trace it was given and reports no
//! change. `PolylineTrace.split` cuts the trace at a pin on its last tree
//! entry (the drill case keeps no pieces) and returns the removed trace as its
//! only piece; `normalize` then says nothing changed. Java's
//! `BasicBoard.normalizeTraces` and `normalizeAllTraces` go on with the stale
//! object (`isUserFixed` reads a field, `removeIfCycle` returns false for a
//! trace off the board). The port panicked there reading the removed item
//! (`item_ref`, "item on the board"), which stopped `freerouting -de` on a
//! KiCad 10.0.7 export of canary-pwr in the fanout stage
//! (`insert_segments` -> `normalize_traces`).
//!
//! `tests/data/normalize-stale-trace.dsn` is that board cut down to the one
//! wire and pin that trigger it: a GND wire on F.Cu that doubles back over
//! itself and runs across pad R10-2. The expected boards are what
//! `tools/java-reference/probes/NormalizeStaleTraceProbe.java` printed at the
//! pin `a773a70ec`.

use freerouting::board::RoutingBoard;
use freerouting::board::items::{AreaKind, Item, ItemKind};
use freerouting::board::structure::FixedState;
use freerouting::datastructures::IdGenerator;
use freerouting::geometry::{IntPoint, Point, Polyline};
use freerouting::io::specctra::{BoardReadResult, read_board_from_str};

const DSN: &str = include_str!("data/normalize-stale-trace.dsn");

/// The wire of the DSN in board units (resolution um 10).
const WIRE: [(i32, i32); 6] = [
    (1246000, -650750),
    (1246000, -650749),
    (1230000, -650750),
    (1293500, -650750),
    (1278000, -650750),
    (1278000, -650749),
];

fn read(text: &str) -> RoutingBoard {
    match read_board_from_str(text) {
        BoardReadResult::Success { board, .. } => *board,
        BoardReadResult::OutlineMissing {
            board: Some(board), ..
        } => *board,
        _ => panic!("normalize-stale-trace.dsn must load"),
    }
}

/// The Java simple class name of `item` (only the kinds this board has).
fn java_type(item: &Item) -> &'static str {
    match &item.kind {
        ItemKind::Pin(_) => "Pin",
        ItemKind::Via(_) => "Via",
        ItemKind::Trace(_) => "PolylineTrace",
        ItemKind::BoardOutline(_) => "BoardOutline",
        ItemKind::ComponentOutline(_) => "ComponentOutline",
        ItemKind::Area(area) => match area.kind {
            AreaKind::Obstacle => "ObstacleArea",
            AreaKind::ViaObstacle => "ViaObstacleArea",
            AreaKind::ComponentObstacle => "ComponentObstacleArea",
            AreaKind::Conduction { .. } => "ConductionArea",
        },
    }
}

/// The probe's lines: `maxGeneratedId`, then every item in item-list order.
fn items(board: &RoutingBoard) -> Vec<String> {
    let mut lines = vec![format!(
        "maxGeneratedId {}",
        board.communication.id_generator.max_generated_id()
    )];
    for item in board.items() {
        let nets: Vec<String> = item.net_numbers().iter().map(i32::to_string).collect();
        let mut line = format!(
            "{} {} nets [{}]",
            item.id().0,
            java_type(item),
            nets.join(", ")
        );
        if let Some(trace) = item.as_trace() {
            line.push_str(&format!(
                " layer {} half_width {} corners",
                trace.layer, trace.half_width
            ));
            for i in 0..trace.polyline.corner_count() {
                match trace.polyline.corner(i) {
                    Point::Int(p) => line.push_str(&format!(" {},{}", p.x, p.y)),
                    other => line.push_str(&format!(" {other:?}")),
                }
            }
        }
        lines.push(line);
    }
    lines
}

/// `normalizeAllTraces` while the DSN loads.
#[test]
fn load_normalization_skips_the_removed_trace_as_java_does() {
    let board = read(DSN);
    assert_eq!(
        items(&board),
        [
            "maxGeneratedId 6",
            "6 PolylineTrace nets [1] layer 0 half_width 2500 corners 1262000,-650750 1278000,-650750",
            "5 PolylineTrace nets [1] layer 0 half_width 2500 corners 1246000,-650750 1262000,-650750",
            "3 Pin nets [1]",
            "2 Pin nets []",
            "1 BoardOutline nets []",
        ]
    );
}

/// `normalizeTraces(net)`, as `insert_segments` calls it after a route, on
/// the same wire inserted into the loaded board. This run splits the wire
/// in other steps and does not take the removed-trace path (it passed before
/// the fix too); it pins the net normalization of this geometry to Java's.
#[test]
fn net_normalization_of_the_wire_matches_java() {
    let wiring = DSN.find("  (wiring").expect("the DSN has a wiring section");
    let mut board = read(&format!("{})\n", &DSN[..wiring]));
    let corners: Vec<Point> = WIRE
        .iter()
        .map(|&(x, y)| Point::Int(IntPoint::new(x, y)))
        .collect();
    board
        .insert_trace_without_cleaning(
            Polyline::from_points(&corners),
            0,
            2500,
            &[1],
            1,
            FixedState::Unfixed,
        )
        .expect("the wire is inserted");
    assert!(board.normalize_traces(1), "Java: normalizeTraces true");
    assert_eq!(
        items(&board),
        [
            "maxGeneratedId 8",
            "8 PolylineTrace nets [1] layer 0 half_width 2500 corners 1262000,-650750 1278000,-650750 1278000,-650749",
            "7 PolylineTrace nets [1] layer 0 half_width 2500 corners 1246000,-650750 1262000,-650750",
            "3 Pin nets [1]",
            "2 Pin nets []",
            "1 BoardOutline nets []",
        ]
    );
}
