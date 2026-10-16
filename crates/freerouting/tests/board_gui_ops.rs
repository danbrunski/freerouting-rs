//! The interactive leftovers on the board (B08.5) and the queries for the rule dialogs
//! (B08.6): `detach_item_for_move`, `unfill_conduction_areas`, `non_45_degree_trace_count`.

mod board_probe;

use freerouting::board::items::{AreaKind, ItemId, ItemKind};
use freerouting::board::state::Communication;
use freerouting::board::structure::{FixedState, Layer, LayerStructure};
use freerouting::board::{DetachError, RoutingBoard};
use freerouting::geometry::{
    Area, Circle, ConvexShape, FloatPoint, IntBox, IntPoint, Point, Polyline, PolylineShape, Shape,
    TileShape,
};
use freerouting::library::PackagePin;
use freerouting::rules::{BoardRules, ClearanceMatrix};

fn board() -> RoutingBoard {
    let layers = LayerStructure::new(vec![Layer::new("Top", true), Layer::new("Bottom", true)]);
    let matrix = ClearanceMatrix::default_instance(layers.count(), 10);
    let mut rules = BoardRules::new(layers.clone(), matrix);
    rules.create_default_net_class();
    let outline = IntBox::new(0, 0, 100_000, 80_000);
    let mut board = RoutingBoard::new(
        outline,
        layers,
        vec![PolylineShape::Tile(TileShape::from(outline))],
        0,
        rules,
        Communication::default(),
    );
    let class = board.rules.default_net_class();
    board.rules.nets.add("N", 1, false, class);
    board
}

/// A board with one pin of component "U1" (`position_fixed` as given).
fn board_with_pin(position_fixed: bool, pin_fixed: FixedState) -> (RoutingBoard, ItemId) {
    let mut board = board();
    let pad = board.library.padstacks.add(
        "smd",
        vec![
            Some(ConvexShape::Circle(Circle::new(IntPoint::new(0, 0), 500))),
            None,
        ],
        vec![None, None],
        false,
        false,
    );
    let package = board.library.packages.add(
        "pkg",
        vec![PackagePin::new("1", pad, FloatPoint::new(0.0, 0.0), 0.0)],
        None,
        None,
        None,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        true,
    );
    let component = board.components.add_precise(
        "U1",
        Some(FloatPoint::new(5000.0, 5000.0)),
        0.0,
        true,
        package,
        package,
        position_fixed,
        None,
    );
    let pin = board
        .insert_pin(component.0, 0, &[1], 0, pin_fixed)
        .expect("pin");
    (board, pin)
}

fn trace(board: &mut RoutingBoard, from: (i32, i32), to: (i32, i32)) -> ItemId {
    let points = [
        Point::Int(IntPoint::new(from.0, from.1)),
        Point::Int(IntPoint::new(to.0, to.1)),
    ];
    board
        .insert_trace_without_cleaning(
            Polyline::from_points(&points),
            0,
            500,
            &[1],
            0,
            FixedState::Unfixed,
        )
        .expect("trace")
}

#[test]
fn detach_refuses_a_user_fixed_item() {
    let mut board = board();
    let id = trace(&mut board, (10_000, 10_000), (20_000, 10_000));
    board.set_fixed_state(id, FixedState::UserFixed);
    assert_eq!(board.detach_item_for_move(id), Err(DetachError::Fixed(id)));
    assert!(board.item(id).is_some());
    board.set_fixed_state(id, FixedState::Unfixed);
    assert_eq!(board.detach_item_for_move(id), Ok(()));
    assert!(board.item(id).is_none());
}

#[test]
fn detach_refuses_an_item_of_a_position_fixed_component() {
    let (mut board, pin) = board_with_pin(true, FixedState::Unfixed);
    assert_eq!(
        board.detach_item_for_move(pin),
        Err(DetachError::Fixed(pin))
    );
    assert!(board.item(pin).is_some());
}

#[test]
fn detach_removes_a_pin_of_a_free_component_though_deletion_is_forbidden() {
    let (mut board, pin) = board_with_pin(false, FixedState::Unfixed);
    // Ordinary removal refuses a component item.
    assert!(board.is_deletion_forbidden(board.item(pin).unwrap()));
    assert!(!board.remove_item(pin));
    assert!(board.item(pin).is_some());
    assert_eq!(board.detach_item_for_move(pin), Ok(()));
    assert!(board.item(pin).is_none());
    // The pin left the search trees too.
    let shape = TileShape::from(IntBox::new(4_000, 4_000, 6_000, 6_000));
    assert!(board.overlapping_objects(&shape, 0).is_empty());
}

#[test]
fn detach_of_an_unknown_item_is_an_error() {
    let mut board = board();
    assert_eq!(
        board.detach_item_for_move(ItemId(999_999)),
        Err(DetachError::NotOnBoard(ItemId(999_999)))
    );
}

#[test]
fn detach_keeps_a_power_plane_like_java() {
    // The layer is not a signal layer: ordinary deletion is forbidden and a
    // move does not change that (only component items are exempt).
    let layers = LayerStructure::new(vec![Layer::new("Top", true), Layer::new("Plane", false)]);
    let matrix = ClearanceMatrix::default_instance(layers.count(), 10);
    let mut rules = BoardRules::new(layers.clone(), matrix);
    rules.create_default_net_class();
    let outline = IntBox::new(0, 0, 100_000, 80_000);
    let mut board = RoutingBoard::new(
        outline,
        layers,
        vec![PolylineShape::Tile(TileShape::from(outline))],
        0,
        rules,
        Communication::default(),
    );
    let class = board.rules.default_net_class();
    board.rules.nets.add("N", 1, false, class);
    let plane = board.insert_conduction_area(
        Area::from(Shape::from(TileShape::from(outline))),
        1,
        &[1],
        0,
        true,
        FixedState::Unfixed,
    );
    assert_eq!(board.detach_item_for_move(plane), Ok(()));
    assert!(board.item(plane).is_some());
}

fn filled_flags(board: &RoutingBoard) -> Vec<(ItemId, bool, bool)> {
    board
        .items()
        .filter_map(|item| match &item.kind {
            ItemKind::Area(a) => match a.kind {
                AreaKind::Conduction {
                    is_obstacle,
                    is_filled,
                } => Some((item.id(), is_filled, is_obstacle)),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

fn board_with_zones() -> (RoutingBoard, Vec<ItemId>) {
    let mut board = board();
    let mut zones = Vec::new();
    for (i, layer) in [0, 1].into_iter().enumerate() {
        let x = i as i32 * 40_000;
        zones.push(board.insert_conduction_area(
            Area::from(Shape::from(TileShape::from(IntBox::new(
                x,
                0,
                x + 30_000,
                30_000,
            )))),
            layer,
            &[1],
            0,
            true,
            FixedState::Unfixed,
        ));
    }
    (board, zones)
}

#[test]
fn unfill_clears_every_area_and_the_router_ignores_conduction() {
    let (mut board, zones) = board_with_zones();
    board.rules.set_ignore_conduction(false);
    assert_eq!(
        filled_flags(&board),
        vec![(zones[1], true, true), (zones[0], true, true)]
    );
    board.unfill_conduction_areas();
    assert_eq!(
        filled_flags(&board),
        vec![(zones[1], false, false), (zones[0], false, false)]
    );
    assert!(board.rules.ignore_conduction());
}

#[test]
fn unfill_is_undone_by_undo() {
    let (mut board, zones) = board_with_zones();
    board.generate_snapshot();
    board.unfill_conduction_areas();
    assert!(filled_flags(&board).iter().all(|&(_, filled, _)| !filled));
    assert!(board.undo(None));
    assert_eq!(
        filled_flags(&board),
        vec![(zones[1], true, true), (zones[0], true, true)]
    );
    assert!(board.redo(None));
    assert!(filled_flags(&board).iter().all(|&(_, filled, _)| !filled));
}

#[test]
fn set_conduction_is_filled_only_touches_conduction_areas() {
    let (mut board, zones) = board_with_zones();
    let t = trace(&mut board, (10_000, 50_000), (20_000, 50_000));
    assert!(!board.set_conduction_is_filled(t, false));
    assert!(board.set_conduction_is_filled(zones[0], false));
    assert_eq!(
        filled_flags(&board),
        vec![(zones[1], true, true), (zones[0], false, true)]
    );
}

#[test]
fn non_45_degree_count_counts_only_odd_angles() {
    let mut board = board();
    trace(&mut board, (10_000, 10_000), (20_000, 10_000));
    trace(&mut board, (10_000, 20_000), (20_000, 30_000));
    assert_eq!(board.non_45_degree_trace_count(), 0);
    trace(&mut board, (10_000, 40_000), (30_000, 50_000));
    trace(&mut board, (50_000, 40_000), (51_000, 70_000));
    assert_eq!(board.non_45_degree_trace_count(), 2);
}

#[test]
fn non_45_degree_count_matches_a_direct_count_on_issue676() {
    let row = board_probe::fixture("Issue676-ch32v-tx118s");
    let mut board = board_probe::load(&row);
    board_probe::import_session(&mut board, &row);
    let direct = board
        .items()
        .filter_map(|item| item.as_trace())
        .filter(|trace| {
            trace
                .polyline
                .lines()
                .iter()
                .any(|line| !line.is_multiple_of_45_degree())
        })
        .count();
    eprintln!("Issue676: {direct} traces off the 45 degree grid");
    assert_eq!(board.non_45_degree_trace_count(), direct);
}

// ---------------------------------------------------------------------------
// B08.6: the queries of the rule dialogs, on Issue145-smoothieboard.

const SMOOTHIE: &str = "Issue145-smoothieboard";

fn smoothie() -> RoutingBoard {
    let row = board_probe::fixture(SMOOTHIE);
    let mut board = board_probe::load(&row);
    board_probe::import_session(&mut board, &row);
    board
}

#[test]
fn clearance_rows_read_like_java() {
    let board = smoothie();
    let matrix = &board.rules.clearance_matrix;
    let (classes, layers) = (matrix.class_count(), matrix.layer_count());
    assert!(classes >= 3, "smoothieboard has net classes: {classes}");
    assert!(matrix.get_row(-1).is_none());
    assert!(matrix.get_row(classes as i32).is_none());
    assert!(matrix.row(classes, 0).is_none());
    assert!(matrix.row(0, layers).is_none());
    for j in 0..classes {
        let row = matrix.get_row(j as i32).expect("row");
        assert_eq!(row.index(), j);
        assert_eq!(Some(row.name()), matrix.get_name(j as i32));
        for layer in 0..layers {
            let values = matrix.row(j, layer).expect("row values");
            assert_eq!(values.len(), classes);
            for (i, value) in values.iter().enumerate() {
                assert_eq!(
                    *value,
                    matrix.get_value(i as i32, j as i32, layer as i32, false)
                );
                assert_eq!(row.value(i, layer), Some(*value));
            }
            assert_eq!(
                row.max_value(layer),
                matrix.max_value(j as i32, layer as i32)
            );
        }
        assert_eq!(row.value(classes, 0), None);
        for i in 0..classes {
            assert_eq!(row.is_layer_dependent(i), matrix.is_layer_dependent(i, j));
        }
        assert!(!row.is_layer_dependent(classes));
    }
    // The default class has a real clearance to itself.
    assert!(matrix.row(1, 0).unwrap()[1] > 0);
}

#[test]
fn net_items_are_all_items_of_the_net_highest_id_first() {
    let board = smoothie();
    let mut checked = 0;
    for net in board.rules.nets.iter().map(|n| n.net_number) {
        let direct: Vec<ItemId> = board
            .items()
            .filter(|item| item.net_numbers().contains(&net))
            .map(|item| item.id())
            .collect();
        let items = board.net_items(net);
        assert_eq!(items, direct, "net {net}");
        assert!(
            items.windows(2).all(|w| w[0].0 > w[1].0),
            "highest id first"
        );
        let connectable = board.connectable_items(net);
        assert!(connectable.iter().all(|id| items.contains(id)));
        checked += items.len();
    }
    assert!(checked > 1000, "{checked} items in nets");
    assert!(board.net_items(0).is_empty());
    assert!(board.net_items(i32::MAX).is_empty());
}

#[test]
fn padstack_is_used_by_an_item_or_a_package_pin() {
    let mut board = smoothie();
    let basic = board.basic().clone();
    let mut used_by_item = 0;
    let mut used_by_package_only = 0;
    let mut unused = 0;
    for padstack in board.library.padstacks.iter() {
        let by_item = basic
            .items()
            .any(|i| i.drill().is_some_and(|d| d.padstack == padstack.id));
        let by_package = board
            .library
            .packages
            .iter()
            .any(|p| p.pins().iter().any(|pin| pin.padstack_id == padstack.id));
        assert_eq!(
            board.library.is_used(padstack.id, &basic),
            by_item || by_package,
            "{}",
            padstack.name
        );
        match (by_item, by_package) {
            (true, _) => used_by_item += 1,
            (false, true) => used_by_package_only += 1,
            (false, false) => unused += 1,
        }
    }
    eprintln!(
        "{used_by_item} by an item, {used_by_package_only} by a package only, {unused} unused"
    );
    assert!(used_by_item > 0);
    // A new padstack nobody uses.
    let fresh = board.library.padstacks.add_layer_range(
        ConvexShape::Circle(Circle::new(IntPoint::new(0, 0), 100)),
        0,
        0,
    );
    assert!(!board.library.is_used(fresh, &basic));
}

#[test]
fn new_net_is_named_after_the_count_in_the_default_class() {
    let mut board = smoothie();
    let count = board.rules.nets.max_net_number();
    let class = board.rules.default_net_class();
    let number = board.rules.nets.new_net(class);
    assert_eq!(number, count + 1);
    let net = board.rules.nets.get(number).expect("the new net");
    assert_eq!(net.name, format!("net#{}", count + 1));
    assert_eq!(net.subnet_number, 1);
    assert!(!net.contains_plane());
    assert_eq!(net.net_class(), class);
    assert_eq!(board.rules.nets.max_net_number(), count + 1);
}

#[test]
fn padstack_used_only_by_a_package_pin_is_used() {
    let mut board = board();
    let pad = board.library.padstacks.add_layer_range(
        ConvexShape::Circle(Circle::new(IntPoint::new(0, 0), 100)),
        0,
        0,
    );
    assert!(!board.library.is_used(pad, board.basic()));
    board.library.packages.add(
        "pkg",
        vec![PackagePin::new("1", pad, FloatPoint::new(0.0, 0.0), 0.0)],
        None,
        None,
        None,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        true,
    );
    assert!(board.items().all(|i| i.drill().is_none()));
    assert!(board.library.is_used(pad, board.basic()));
}
