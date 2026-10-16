//! Item moves, transforms and copies on the board (B05.1, B05.2, B05.6) and
//! the placement tests of B05.8.
//!
//! The probe tests compare with `fixtures/reference/probe/item-ops/<board>.jsonl`,
//! which `tools/java-reference/probes/board/ItemOpsProbe.java` writes; its doc
//! comment describes the stages. The unit tests' expected values for
//! `rotate_approx` come from that probe's Java at `a773a70ec` (see each
//! test).
//!
//! B05.8 ports `fixtures/Issue955PlacementLifecycleTest` and
//! `fixtures/Issue955RotatedPadContactTest` here (the B05.8 section lists
//! the ported ones). Skipped Java tests and their owners:
//! - `Issue955PlacementLifecycleTest.dsnRoundTripPreservesBothContacts`:
//!   already ported by C06, `tests/dsn_round_trip.rs`.
//! - `quarterTurnPreservesContact`, `arbitraryTurnPreservesContact`,
//!   `mirrorPreservesContact`: need `board.actions.ComponentItemTransform`
//!   and `detachItemForMove`; owner I04.
//! - `packageIdentityIncludesFractionalOffsets`: needs
//!   `io.kicad.KiCadJsonReader`, which no checklist ports (K05 reads
//!   `.kicad_pcb`); owner: none.
//! - `board.actions.ComponentItemTransformTest`: all but
//!   `componentRotationApiUsesExactQuarterTurns` (in `tests/java_ports.rs`)
//!   need `ComponentItemTransform`; owner I04.
//!
//! `board.BoardItemRenderingMetadataTest` is in `tests/java_ports.rs`.

mod board_probe;

use std::collections::BTreeSet;

use board_probe::{FNV_OFFSET, Line, Row, compare, fixture, fnv1a64, header, hex};
use freerouting::board::items::{AreaKind, Item, ItemId, ItemKind};
use freerouting::board::structure::{ComponentId, FixedState};
use freerouting::board::{ItemTransform, RoutingBoard};
use freerouting::datastructures::IdGenerator;
use freerouting::geometry::jmath::JavaRandom;
use freerouting::geometry::{
    Circle, ConvexShape, FloatPoint, IntBox, IntPoint, IntVector, Point, Vector,
};
use freerouting::library::PackagePin;

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

/// The probe's `itemText`.
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

fn default_tree_line(stage: &str, board: &RoutingBoard) -> String {
    let tree = board.search_trees().default_tree();
    let fingerprint = tree.fingerprint();
    Line::new()
        .str("stage", stage)
        .str("tree", &tree.key())
        .num("nodes", fingerprint.nodes as i64)
        .num("leaves", fingerprint.leaves as i64)
        .num("maxDepth", fingerprint.max_depth as i64)
        .str("shapeHash", &hex(fingerprint.shape_hash))
        .str("contentDigest", &hex(tree.content_digest()))
        .end()
}

fn loaded(row: &Row) -> RoutingBoard {
    let mut board = board_probe::load(row);
    board_probe::import_session(&mut board, row);
    board
}

/// The probe's `move` stage.
fn move_lines(row: &Row) -> Vec<String> {
    let mut board = loaded(row);
    let mut lines = vec![header("item-ops", &row.board)];
    let before = board.state_hash();
    let items_before = items_hash(&board);
    board.generate_snapshot();
    let snapshot = board.state_hash();
    let vias: Vec<ItemId> = board
        .items()
        .filter(|i| i.is_via())
        .map(Item::id)
        .take(20)
        .collect();
    for (k, via) in vias.into_iter().enumerate() {
        assert!(board.move_item(via, &Vector::Int(IntVector::new(500, 250))));
        let center = board.item(via).and_then(Item::drill).expect("via").center;
        lines.push(
            Line::new()
                .str("stage", "move")
                .num("k", k as i64)
                .num("via", i64::from(via.0))
                .str("center", &point_text(&Point::Int(center)))
                .num("maxGeneratedId", max_generated_id(&board))
                .num("items", board.item_count() as i64)
                .str("itemsHash", &hex(items_hash(&board)))
                .end(),
        );
        lines.push(default_tree_line("move", &board));
    }
    board.undo(None);
    lines.push(
        Line::new()
            .str("stage", "move")
            .str("step", "undo")
            .num("maxGeneratedId", max_generated_id(&board))
            .num("items", board.item_count() as i64)
            .bool("hashEqualsBefore", board.state_hash() == before)
            .bool("hashEqualsSnapshot", board.state_hash() == snapshot)
            .bool("itemsEqualBefore", items_hash(&board) == items_before)
            .end(),
    );
    lines
}

/// `item-ops` stage `move` (B05.1): 20 via moves on Issue508-DAC2020_bm05
/// with its session, then an undo.
#[test]
fn item_ops_move_matches_java() {
    let row = fixture("Issue508-DAC2020_bm05");
    if let Err(e) = compare("item-ops", &row, &move_lines(&row)) {
        panic!("{e}");
    }
}

/// The ids in `a` but not `b`, highest first (the probe's `difference`).
fn difference(a: &BTreeSet<ItemId>, b: &BTreeSet<ItemId>) -> String {
    a.iter()
        .filter(|id| !b.contains(id))
        .map(|id| id.0.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

fn id_set(board: &RoutingBoard) -> BTreeSet<ItemId> {
    board.items().map(Item::id).collect()
}

/// The probe's `connect` stage.
fn connect_lines(board: &mut RoutingBoard, lines: &mut Vec<String>) {
    for id in board.traces() {
        board.unfix(id);
    }
    let mut random = JavaRandom::new(5);
    for k in 0..50 {
        let traces = board.traces();
        if traces.is_empty() {
            break;
        }
        let trace = traces[random.next_int(traces.len() as i32) as usize];
        let (corner, half_width, class, c) = {
            let item = board.item(trace).expect("trace");
            let t = item.as_trace().expect("trace");
            let corner = random.next_int(t.corner_count() as i32) as usize;
            (
                corner,
                t.half_width,
                item.clearance_class(),
                t.polyline.corner(corner),
            )
        };
        let range = 10 * half_width;
        let dx = random.next_int(2 * range + 1) - range;
        let dy = random.next_int(2 * range + 1) - range;
        let line = Line::new()
            .str("stage", "connect")
            .num("k", k)
            .num("trace", i64::from(trace.0))
            .num("corner", corner as i64);
        let Point::Int(ic) = c else {
            lines.push(line.str("skip", "rational corner").end());
            continue;
        };
        let from = IntPoint::new(ic.x + dx, ic.y + dy);
        let before = id_set(board);
        let result = board.connect_to_trace(from, trace, half_width, class);
        let after = id_set(board);
        lines.push(
            line.str("from", &point_text(&Point::Int(from)))
                .bool("result", result)
                .str("inserted", &difference(&after, &before))
                .str("removed", &difference(&before, &after))
                .num("maxGeneratedId", max_generated_id(board))
                .num("items", board.item_count() as i64)
                .str("itemsHash", &hex(items_hash(board)))
                .end(),
        );
    }
}

const POLE: IntPoint = IntPoint {
    x: 1200000,
    y: -850000,
};
const OPS: [&str; 4] = ["turn90", "side", "rotate30", "rotate45.5"];

/// The first trace, via and component pin in item-list order.
fn transform_items(board: &RoutingBoard) -> Vec<ItemId> {
    let first = |f: &dyn Fn(&Item) -> bool| board.items().find(|i| f(i)).map(Item::id);
    [
        first(&|i| i.is_trace()),
        first(&|i| i.is_via()),
        first(&|i| i.is_pin() && i.component_id() > 0),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Applies the probe's transform `op` to item `id` (and to a pin's
/// component first).
fn apply_op(board: &mut RoutingBoard, id: ItemId, op: &str) {
    let item = board.item(id).expect("item");
    if item.is_pin() {
        let c = ComponentId(item.component_id());
        match op {
            "turn90" => board.components.turn_90_degree(c, 1, POLE),
            "side" => board.components.change_side(c, POLE),
            "rotate30" => board.components.rotate(c, 30.0, POLE),
            _ => board.components.rotate(c, 45.5, POLE),
        }
    }
    let pole = FloatPoint::from_int_point(POLE);
    assert!(match op {
        "turn90" => board.turn_90_degree(id, 1, POLE),
        "side" => board.change_placement_side(id, POLE),
        "rotate30" => board.rotate_approx(id, 30.0, pole),
        _ => board.rotate_approx(id, 45.5, pole),
    });
}

/// The probe's `transform` stage.
fn transform_lines(board: &mut RoutingBoard, lines: &mut Vec<String>) {
    let items = transform_items(board);
    for op in OPS {
        for &id in &items {
            apply_op(board, id, op);
            let text = item_text(board.item(id).expect("item"));
            lines.push(
                Line::new()
                    .str("stage", "transform")
                    .str("op", op)
                    .str("item", text.trim())
                    .end(),
            );
        }
    }
}

/// `item-ops` stages `connect` (B05.6) and `transform` (B05.1) on
/// Issue026-J2_reference with its session.
#[test]
fn item_ops_connect_and_transform_match_java() {
    let row = fixture("Issue026-J2_reference");
    let mut lines = vec![header("item-ops", &row.board)];
    let mut board = loaded(&row);
    connect_lines(&mut board, &mut lines);
    let mut fresh = loaded(&row);
    transform_lines(&mut fresh, &mut lines);
    if let Err(e) = compare("item-ops", &row, &lines) {
        panic!("{e}");
    }
}

// ---------------------------------------------------------------------------
// B05.1 unit tests. The expected coordinates are the Java probe's
// (`ItemOpsProbe` stage `transform`, `a773a70ec`, in
// `fixtures/reference/probe/item-ops/Issue026-J2_reference.jsonl`); the ops
// run one after the other, so each test replays the ones before it.

fn j2() -> RoutingBoard {
    loaded(&fixture("Issue026-J2_reference"))
}

/// The geometry part of `item_text` (after the clearance class).
fn geometry(board: &RoutingBoard, id: ItemId) -> String {
    let text = item_text(board.item(id).expect("item"));
    let mut parts = text.trim().splitn(6, ' ');
    parts.nth(5).unwrap_or_default().to_string()
}

fn replay(board: &mut RoutingBoard, ops: &[&str]) -> Vec<ItemId> {
    let items = transform_items(board);
    for op in ops {
        for &id in &items {
            apply_op(board, id, op);
        }
    }
    items
}

#[test]
fn turn_90_degree_on_trace_via_and_pin() {
    let mut board = j2();
    let items = replay(&mut board, &["turn90"]);
    assert_eq!(items, vec![ItemId(194), ItemId(188), ItemId(81)]);
    assert_eq!(
        geometry(&board, items[0]),
        "l0 w1250 1181250,-786619 1173250,-786619"
    );
    assert_eq!(
        geometry(&board, items[1]),
        "1199221,-789681 l0-1 1195221,-793681,1203221,-785681"
    );
    assert_eq!(
        geometry(&board, items[2]),
        "1105609,-937880 l0-0 1092209,-955880,1119009,-919880"
    );
    // Exact: three more quarter turns give the trace and via back.
    let trace_before = geometry(&j2(), items[0]);
    let via_before = geometry(&j2(), items[1]);
    for _ in 0..3 {
        board.turn_90_degree(items[0], 1, POLE);
        board.turn_90_degree(items[1], 1, POLE);
    }
    assert_eq!(geometry(&board, items[0]), trace_before);
    assert_eq!(geometry(&board, items[1]), via_before);
}

#[test]
fn change_placement_side_on_trace_via_and_pin() {
    let mut board = j2();
    let items = replay(&mut board, &["turn90", "side"]);
    assert_eq!(
        geometry(&board, items[0]),
        "l1 w1250 1218750,-786619 1226750,-786619"
    );
    assert_eq!(
        geometry(&board, items[1]),
        "1200779,-789681 l0-1 1196779,-793681,1204779,-785681"
    );
    assert_eq!(
        geometry(&board, items[2]),
        "1294391,-937880 l1-1 1280991,-955880,1307791,-919880"
    );
}

#[test]
fn rotate_approx_on_trace_via_and_pin() {
    let mut board = j2();
    let items = replay(&mut board, &["turn90", "side", "rotate30"]);
    assert_eq!(
        geometry(&board, items[0]),
        "l1 w1250 1184547,-785735 1191476,-781735"
    );
    assert_eq!(
        geometry(&board, items[1]),
        "1170515,-797373 l0-1 1166515,-801373,1174515,-793373"
    );
    assert_eq!(
        geometry(&board, items[2]),
        "1325685,-878911 l1-1 1305080,-901199,1346290,-856622"
    );
    for &id in &items {
        apply_op(&mut board, id, "rotate45.5");
    }
    assert_eq!(
        geometry(&board, items[0]),
        "l1 w1250 1143332,-815978 1145335,-808232"
    );
    assert_eq!(
        geometry(&board, items[2]),
        "1308714,-780619 l1-1 1287933,-798099,1329496,-763139"
    );
}

#[test]
fn edge_pin_cache_invalidated_on_pin_move() {
    let mut board = j2();
    let _ = board.edge_pin_nets();
    assert!(board.edge_pin_nets_cached());
    let pin = board.pins()[0];
    assert!(board.move_item(pin, &Vector::Int(IntVector::new(100, 0))));
    assert!(!board.edge_pin_nets_cached(), "a pin move invalidates");
    let via = board.vias()[0];
    let _ = board.edge_pin_nets();
    assert!(board.move_item(via, &Vector::Int(IntVector::new(100, 0))));
    assert!(board.edge_pin_nets_cached(), "a via move does not");
}

#[test]
fn edge_pin_cache_invalidated_on_outline_transform() {
    let mut board = j2();
    let outline = board.outline().map(Item::id).expect("outline");
    let pole = IntPoint::new(0, 0);
    let steps: [&dyn Fn(&mut RoutingBoard) -> bool; 4] = [
        &|b| b.move_item(outline, &Vector::Int(IntVector::new(10, 10))),
        &|b| b.turn_90_degree(outline, 1, pole),
        &|b| b.rotate_approx(outline, 30.0, FloatPoint::ZERO),
        &|b| b.change_placement_side(outline, pole),
    ];
    let shapes_before = board.item(outline).expect("outline").bounding_box();
    for step in steps {
        let _ = board.edge_pin_nets();
        assert!(board.edge_pin_nets_cached());
        assert!(step(&mut board));
        assert!(!board.edge_pin_nets_cached());
    }
    // Java's loops never store the moved outline shapes (kept on purpose).
    assert_eq!(
        board.item(outline).expect("outline").bounding_box(),
        shapes_before
    );
}

// ---------------------------------------------------------------------------
// B05.2

#[test]
fn copy_zero_burns_an_id() {
    let mut board = j2();
    let via = board.vias()[0];
    let before = max_generated_id(&board);
    let copy = board.copy_item(via, None).expect("copy");
    assert_eq!(max_generated_id(&board), before + 1);
    assert_eq!(i64::from(copy.id().0), before + 1);
    assert!(!copy.is_on_the_board());
    assert_eq!(
        copy.drill().map(|d| d.center),
        board.item(via).and_then(|i| i.drill()).map(|d| d.center)
    );
    let pin = board.pins()[0];
    let pin_copy = board.copy_item(pin, None).expect("copy");
    assert_eq!(pin_copy.as_pin().expect("pin").changed_to, pin_copy.id());
    assert_eq!(max_generated_id(&board), before + 2);
}

#[test]
fn copy_with_id_burns_none() {
    let mut board = j2();
    let trace = board.traces()[0];
    let before = max_generated_id(&board);
    let copy = board.copy_item(trace, Some(trace)).expect("copy");
    assert_eq!(max_generated_id(&board), before);
    assert_eq!(copy.id(), trace);
    let original = board.item(trace).expect("trace");
    assert_eq!(
        copy.as_trace().expect("trace").polyline.corner_approx_arr(),
        original
            .as_trace()
            .expect("trace")
            .polyline
            .corner_approx_arr()
    );
    assert_eq!(copy.net_numbers(), original.net_numbers());
    assert_eq!(copy.fixed_state(), original.fixed_state());
}

#[test]
fn restore_pose_round_trip() {
    let mut board = j2();
    let id = ComponentId(1);
    let original = board.components.get(id).expect("component").clone();
    board
        .components
        .move_by(id, &Vector::Int(IntVector::new(1234, -567)));
    board.components.rotate(id, 33.3, IntPoint::new(1000, 2000));
    board.components.change_side(id, IntPoint::new(1000, 2000));
    let moved = board.components.get(id).expect("component").clone();
    assert_ne!(moved.exact_location(), original.exact_location());
    board.components.restore_pose(id, &original);
    let restored = board.components.get(id).expect("component");
    assert_eq!(restored.exact_location(), original.exact_location());
    assert_eq!(
        restored.rotation_in_degree().to_bits(),
        original.rotation_in_degree().to_bits()
    );
    assert_eq!(restored.placed_on_front(), original.placed_on_front());
    // Components::copy places the copy at the translated precise location.
    let package = original.package();
    let copy = board
        .components
        .copy(id, &Vector::Int(IntVector::new(10, -20)), package)
        .expect("copy");
    let copied = board.components.get(copy).expect("copy");
    let (a, b) = (
        copied.exact_location().expect("placed"),
        original.exact_location().expect("placed"),
    );
    assert_eq!((a.x, a.y), (b.x + 10.0, b.y - 20.0));
    assert_eq!(copied.name, format!("Component#{}", copy.0));
}

// ---------------------------------------------------------------------------
// B05.7: pin swap, on `tests/data/pin-swap.dsn` (no fixture has a
// logical part): U1's part QUADPART puts pins 1 and 2 in gate A with gate
// pin swap code 1, pin 3 in gate A with code 2, pin 4 in gate B with code 1.

fn pin_swap_board() -> RoutingBoard {
    let bytes =
        std::fs::read(board_probe::repo_root().join("crates/freerouting/tests/data/pin-swap.dsn"))
            .expect("pin-swap.dsn");
    freerouting::drc::load_design(&bytes).expect("pin-swap.dsn loads")
}

fn u1_pins(board: &RoutingBoard) -> Vec<ItemId> {
    let u1 = board.components.get_by_name("U1").expect("U1").id;
    (0..4)
        .map(|i| board.get_pin(u1.0, i).expect("U1 pin"))
        .collect()
}

fn net_name(board: &RoutingBoard, id: ItemId) -> String {
    let item = board.item(id).expect("pin");
    item.net_numbers()
        .first()
        .and_then(|n| board.rules.nets.get(*n))
        .map_or_else(String::new, |net| net.name.clone())
}

fn changed_to(board: &RoutingBoard, id: ItemId) -> ItemId {
    board
        .item(id)
        .and_then(Item::as_pin)
        .expect("pin")
        .changed_to
}

#[test]
fn swappable_pins_follow_gate_and_swap_code() {
    let board = pin_swap_board();
    let pins = u1_pins(&board);
    assert_eq!(board.swappable_pins(pins[0]), BTreeSet::from([pins[1]]));
    assert_eq!(board.swappable_pins(pins[1]), BTreeSet::from([pins[0]]));
    assert!(board.swappable_pins(pins[2]).is_empty(), "code 2 is alone");
    assert!(board.swappable_pins(pins[3]).is_empty(), "gate B is alone");
    let j1 = board.components.get_by_name("J1").expect("J1").id;
    let j1_pin = board.get_pin(j1.0, 0).expect("J1-1");
    assert!(board.swappable_pins(j1_pin).is_empty(), "no logical part");
}

#[test]
fn swap_pins_exchanges_nets_and_changed_to_until_undo() {
    let mut board = pin_swap_board();
    let pins = u1_pins(&board);
    assert_eq!(net_name(&board, pins[0]), "NA");
    assert_eq!(net_name(&board, pins[1]), "NB");
    assert_eq!(changed_to(&board, pins[0]), pins[0]);
    board.generate_snapshot();
    assert!(board.swap_pins(pins[0], pins[1]));
    assert_eq!(net_name(&board, pins[0]), "NB");
    assert_eq!(net_name(&board, pins[1]), "NA");
    assert_eq!(changed_to(&board, pins[0]), pins[1]);
    assert_eq!(changed_to(&board, pins[1]), pins[0]);
    assert!(board.undo(None));
    assert_eq!(net_name(&board, pins[0]), "NA");
    assert_eq!(net_name(&board, pins[1]), "NB");
    assert_eq!(changed_to(&board, pins[0]), pins[0]);
    assert_eq!(changed_to(&board, pins[1]), pins[1]);
    assert!(board.redo(None));
    assert_eq!(net_name(&board, pins[0]), "NB");
    assert_eq!(changed_to(&board, pins[0]), pins[1]);
}

// ---------------------------------------------------------------------------
// B05.8: `fixtures/Issue955PlacementLifecycleTest` and
// `fixtures/Issue955RotatedPadContactTest` (Java at `a773a70ec`).
//
// Ported: shapeAndPinUseSameAnchor, copyConstructionPreservesLocation,
// copyRebindingInvalidatesCachedPadGeometry,
// copyRebindingInvalidatesCachedKeepoutGeometry,
// mirroredExitDirectionsFollowPadOrientation, sesPreservesFractionalPlacement,
// fractionalRotationSurvivesDsnRoundTrip,
// backSideRotateFirstQuarterTurnUsesWorldDirection,
// backSideMirrorFirstReflectionUsesWorldDirection and
// Issue955RotatedPadContactTest.testRotatedPadContactsOnImport.
//
// Not ported here:
// - dsnRoundTripPreservesBothContacts: ported by C06 as
//   `tests/dsn_round_trip.rs::dsn_round_trip_preserves_both_contacts`.
// - quarterTurnPreservesContact, arbitraryTurnPreservesContact,
//   mirrorPreservesContact: they run `board.actions.ComponentItemTransform.apply`
//   on items detached with `detachItemForMove`; owner I04.
// - packageIdentityIncludesFractionalOffsets: it calls the private
//   `io.kicad.KiCadJsonReader.arePackagePinsIdentical`; no checklist ports
//   the KiCad JSON reader (K05 reads `.kicad_pcb`).

fn issue955() -> RoutingBoard {
    let bytes =
        std::fs::read(board_probe::repo_root().join("fixtures/Issue955-rotated-pad-contacts.dsn"))
            .expect("Issue955 fixture");
    match freerouting::io::read_board(&bytes) {
        freerouting::io::BoardReadResult::Success { board, .. } => *board,
        _ => panic!("Issue955-rotated-pad-contacts.dsn must load"),
    }
}

/// The pin `name` of component S21.
fn s21_pin(board: &RoutingBoard, name: &str) -> ItemId {
    board
        .pins()
        .into_iter()
        .find(|&id| {
            let item = board.item(id).expect("pin");
            board
                .components
                .get(ComponentId(item.component_id()))
                .is_some_and(|c| c.name == "S21")
                && board.pin_name(item) == Some(name)
        })
        .unwrap_or_else(|| panic!("S21 pin {name}"))
}

fn center(board: &RoutingBoard, id: ItemId) -> IntPoint {
    board
        .item(id)
        .and_then(Item::drill)
        .expect("drill item")
        .center
}

/// Java's `synthetic`: a component REVIEW with one pin of a 2-layer circle
/// padstack (radius 100) at `offset`, net 1, clearance class 1.
fn synthetic(
    board: &mut RoutingBoard,
    placement: FloatPoint,
    offset: FloatPoint,
    rotation: f64,
    front: bool,
) -> ItemId {
    let circle = ConvexShape::Circle(Circle::new(IntPoint::new(0, 0), 100));
    let pad = board
        .library
        .padstacks
        .add_unnamed(vec![Some(circle.clone()), Some(circle)]);
    let pkg = board
        .library
        .packages
        .add_unnamed(vec![PackagePin::new("1", pad, offset, 0.0)]);
    let component = board.components.add_precise(
        "REVIEW",
        Some(placement),
        rotation,
        front,
        pkg,
        pkg,
        false,
        None,
    );
    board
        .insert_pin(component.0, 0, &[1], 1, FixedState::Unfixed)
        .expect("synthetic pin")
}

#[test]
fn rotated_pad_contacts_on_import() {
    // Java's contact ids from a probe at a773a70ec (getNormalContacts after
    // BoardLoader.loadBoardIfNeeded, with and without awaitPostLoad).
    let board = board_probe::load(&fixture("Issue955-rotated-pad-contacts"));
    let pin_2at1 = s21_pin(&board, "2@1");
    let pin_1 = s21_pin(&board, "1");
    assert_eq!((pin_2at1, pin_1), (ItemId(1413), ItemId(1410)));
    assert_eq!(center(&board, pin_2at1), IntPoint::new(2088569, -1304305));
    assert_eq!(center(&board, pin_1), IntPoint::new(2165372, -1215279));
    assert_eq!(
        board.normal_contacts(pin_2at1),
        BTreeSet::from([ItemId(1694)])
    );
    assert_eq!(board.normal_contacts(pin_1), BTreeSet::from([ItemId(1655)]));
}

#[test]
fn shape_and_pin_use_same_anchor() {
    let mut board = issue955();
    let pin = synthetic(
        &mut board,
        FloatPoint::new(20000.49, 50000.49),
        FloatPoint::new(0.49, 0.49),
        0.0,
        true,
    );
    let shape = board
        .item(pin)
        .and_then(Item::drill)
        .and_then(|d| d.shape(0))
        .expect("shape");
    assert_eq!(center(&board, pin), shape.centre_of_gravity().round());
}

#[test]
fn copy_construction_preserves_location() {
    let mut board = issue955();
    let original = s21_pin(&board, "2@1");
    let item = board.item(original).expect("pin");
    let (c, pin_index) = (
        ComponentId(item.component_id()),
        item.as_pin().expect("pin").pin_index,
    );
    let package = board.components.get(c).expect("S21").package();
    let copied = board
        .components
        .copy(c, &Vector::Int(IntVector::new(0, 0)), package)
        .expect("copy");
    let p = board
        .insert_pin(copied.0, pin_index, &[1], 1, FixedState::Unfixed)
        .expect("pin");
    assert_eq!(center(&board, original), center(&board, p));
}

#[test]
fn copy_rebinding_invalidates_cached_pad_geometry() {
    let mut board = issue955();
    let original = s21_pin(&board, "2@1");
    let c = ComponentId(board.item(original).expect("pin").component_id());
    let mut copy = board.copy_item(original, None).expect("copy");
    let old_center = copy.drill().expect("pin").center;
    let delta = IntVector::new(10000, -20000);
    let package = board.components.get(c).expect("S21").package();
    let copied = board
        .components
        .copy(c, &Vector::Int(delta), package)
        .expect("copy");
    board.assign_component_id(&mut copy, copied.0);
    let new_center = copy.drill().expect("pin").center;
    assert_eq!(new_center, old_center.translate_by_int(delta));
    let shape = copy.drill().and_then(|d| d.shape(0)).expect("shape");
    assert!(shape.contains(&Point::Int(new_center)));
}

#[test]
fn copy_rebinding_invalidates_cached_keepout_geometry() {
    let mut board = issue955();
    let c = ComponentId(
        board
            .item(s21_pin(&board, "2@1"))
            .expect("pin")
            .component_id(),
    );
    let circle = freerouting::geometry::Shape::from(Circle::new(IntPoint::new(0, 0), 100));
    // Java constructs the area without inserting it; the port builds it on
    // the board and works on a detached copy.
    let area = board.insert_area(
        AreaKind::Obstacle,
        freerouting::geometry::Area::from(circle),
        0,
        Vector::Int(IntVector::new(0, 0)),
        0.0,
        false,
        &[],
        1,
        c.0,
        Some("copy-keepout".into()),
        FixedState::Unfixed,
    );
    let mut copy = board.copy_item(area, None).expect("copy");
    let old_bounds = copy.bounding_box();
    let delta = IntVector::new(10000, -20000);
    board.transform_detached(&mut copy, &ItemTransform::Translate(Vector::Int(delta)));
    let package = board.components.get(c).expect("S21").package();
    let copied = board
        .components
        .copy(c, &Vector::Int(delta), package)
        .expect("copy");
    board.assign_component_id(&mut copy, copied.0);
    let bounds = copy.bounding_box();
    assert_eq!(bounds.ll, old_bounds.ll.translate_by_int(delta));
    assert_eq!(bounds.ur, old_bounds.ur.translate_by_int(delta));
    assert_eq!(copy.clone().bounding_box().ll, bounds.ll);
}

#[test]
fn mirrored_exit_directions_follow_pad_orientation() {
    for rotate_first in [false, true] {
        let mut board = issue955();
        board.components.set_flip_style_rotate_first(rotate_first);
        let pad_box = ConvexShape::Tile(freerouting::geometry::TileShape::from(IntBox::new(
            -400, -50, 400, 50,
        )));
        let pad = board
            .library
            .padstacks
            .add_unnamed(vec![Some(pad_box.clone()), Some(pad_box)]);
        let pkg = board.library.packages.add_unnamed(vec![PackagePin::new(
            "1",
            pad,
            FloatPoint::new(0.0, 0.0),
            45.0,
        )]);
        let component = board.components.add_precise(
            "EXIT",
            Some(FloatPoint::new(20000.0, 50000.0)),
            30.0,
            false,
            pkg,
            pkg,
            false,
            None,
        );
        let pin = board
            .insert_pin(component.0, 0, &[1], 1, FixedState::Unfixed)
            .expect("pin");
        let exits = board.trace_exit_restrictions(pin, 0);
        assert_eq!(exits.len(), 2);
        let angle: f64 = if rotate_first { 105.0 } else { 165.0 };
        let expected = freerouting::geometry::Direction::get_instance_approx(angle.to_radians());
        assert!(
            exits.iter().any(|e| e.direction == expected),
            "rotate_first {rotate_first}"
        );
        assert!(exits.iter().all(|e| e.min_length > 0.0));
    }
}

#[test]
fn ses_preserves_fractional_placement() {
    let board = issue955();
    let text = freerouting::io::specctra::write_ses(&board, "review.dsn", Default::default());
    let placement = text
        .lines()
        .find(|l| l.contains("place S21 "))
        .expect("S21 placement");
    assert!(placement.contains("2097468.06 -1213891.58"), "{placement}");
}

#[test]
fn fractional_rotation_survives_dsn_round_trip() {
    let mut board = issue955();
    synthetic(
        &mut board,
        FloatPoint::new(20000.0, 50000.0),
        FloatPoint::new(1000.0, 0.0),
        12.2501234567,
        true,
    );
    let text = freerouting::io::specctra::write_dsn(&board, "review");
    let reload = match freerouting::io::read_board_from_str(&text) {
        freerouting::io::BoardReadResult::Success { board, .. } => *board,
        _ => panic!("the written DSN must load"),
    };
    let component = reload.components.get_by_name("REVIEW").expect("REVIEW");
    assert_eq!(component.rotation_in_degree(), 12.2501234567);
}

#[test]
fn back_side_rotate_first_quarter_turn_uses_world_direction() {
    let mut board = issue955();
    board.components.set_flip_style_rotate_first(true);
    let pin = synthetic(
        &mut board,
        FloatPoint::new(20000.0, 50000.0),
        FloatPoint::new(1000.0, 0.0),
        30.0,
        false,
    );
    let zero = IntPoint::new(0, 0);
    let expected = center(&board, pin).turn_90_degree(1, zero);
    let c = ComponentId(board.item(pin).expect("pin").component_id());
    board.components.turn_90_degree(c, 1, zero);
    assert!(board.turn_90_degree(pin, 1, zero));
    assert_eq!(center(&board, pin), expected);
}

#[test]
fn back_side_mirror_first_reflection_uses_world_direction() {
    let mut board = issue955();
    let pin = synthetic(
        &mut board,
        FloatPoint::new(20000.0, 50000.0),
        FloatPoint::new(1000.0, 0.0),
        30.0,
        true,
    );
    let zero = IntPoint::new(0, 0);
    let expected = center(&board, pin).mirror_vertical(zero);
    let c = ComponentId(board.item(pin).expect("pin").component_id());
    board.components.change_side(c, zero);
    assert!(board.change_placement_side(pin, zero));
    assert_eq!(center(&board, pin), expected);
}
