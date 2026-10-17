//! The drawable geometry of board items (B08.1).
//!
//! `every_item_of_every_probe_board_has_geometry` checks all rows of
//! `tools/java-reference/probes/board/fixtures.tsv`: the union of the bounding boxes of an
//! item's shapes equals the item's `bounding_box()`, and the kind, the layer range, the nets
//! and the component equal the item's. The unit tests build one item of each kind and compare
//! its geometry with literals.

mod board_probe;

use freerouting::board::items::{AreaKind, BoardItemType, ItemId, ItemKind};
use freerouting::board::state::Communication;
use freerouting::board::structure::{FixedState, Layer, LayerStructure};
use freerouting::board::{GeometryShape, ItemGeometry, LayerShape, RoutingBoard};
use freerouting::geometry::CRIT_INT;
use freerouting::geometry::{
    Area, Circle, ConvexShape, FloatPoint, IntBox, IntOctagon, IntPoint, Point, Polyline,
    PolylineArea, PolylineShape, PolylineShapeOps, TileShape, Vector,
};
use freerouting::library::PackagePin;
use freerouting::rules::{BoardRules, ClearanceMatrix};

fn two_layer_board() -> RoutingBoard {
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

fn shape_of(geometry: &ItemGeometry, index: usize) -> &GeometryShape {
    &geometry.shapes[index].shape
}

fn geometry(board: &RoutingBoard, id: ItemId) -> ItemGeometry {
    board.item_geometry(id).expect("item on the board")
}

fn float_ring(points: &[(f64, f64)]) -> Vec<FloatPoint> {
    points.iter().map(|&(x, y)| FloatPoint::new(x, y)).collect()
}

#[test]
fn trace_is_one_polyline_with_its_half_width() {
    let mut board = two_layer_board();
    let points = [
        Point::Int(IntPoint::new(10_000, 10_000)),
        Point::Int(IntPoint::new(30_000, 10_000)),
        Point::Int(IntPoint::new(30_000, 40_000)),
    ];
    let id = board
        .insert_trace_without_cleaning(
            Polyline::from_points(&points),
            1,
            750,
            &[1],
            0,
            FixedState::UserFixed,
        )
        .unwrap();
    let g = geometry(&board, id);
    assert_eq!(g.id, id);
    assert_eq!(g.kind, BoardItemType::Trace);
    assert_eq!((g.first_layer, g.last_layer), (1, 1));
    assert_eq!(g.nets, vec![1]);
    assert_eq!(g.fixed_state, FixedState::UserFixed);
    assert_eq!(g.component, 0);
    assert_eq!(
        g.shapes,
        vec![LayerShape {
            layer: 1,
            shape: GeometryShape::Polyline {
                corners: float_ring(&[
                    (10_000.0, 10_000.0),
                    (30_000.0, 10_000.0),
                    (30_000.0, 40_000.0)
                ]),
                half_width: 750,
            },
        }]
    );
    assert_eq!(g.bounding_box(), IntBox::new(9_250, 9_250, 30_750, 40_750));
    assert_eq!(g.bounding_box(), board.item(id).unwrap().bounding_box());
}

#[test]
fn via_has_one_shape_per_padstack_layer() {
    let mut board = two_layer_board();
    let pad = board.library.padstacks.add(
        "via",
        vec![
            Some(ConvexShape::Circle(Circle::new(IntPoint::new(0, 0), 300))),
            Some(ConvexShape::Tile(TileShape::IntBox(IntBox::new(
                -200, -200, 200, 200,
            )))),
        ],
        vec![None, None],
        true,
        false,
    );
    let id = board.insert_via(
        pad,
        IntPoint::new(5_000, 6_000),
        &[1],
        0,
        FixedState::Unfixed,
        true,
    );
    let g = geometry(&board, id);
    assert_eq!(g.kind, BoardItemType::Via);
    assert_eq!((g.first_layer, g.last_layer), (0, 1));
    assert_eq!(
        g.shapes,
        vec![
            LayerShape {
                layer: 0,
                shape: GeometryShape::Circle {
                    center: IntPoint::new(5_000, 6_000),
                    radius: 300,
                },
            },
            LayerShape {
                layer: 1,
                shape: GeometryShape::Box(IntBox::new(4_800, 5_800, 5_200, 6_200)),
            },
        ]
    );
    assert_eq!(g.bounding_box(), IntBox::new(4_700, 5_700, 5_300, 6_300));
    assert_eq!(g.bounding_box(), board.item(id).unwrap().bounding_box());
}

#[test]
fn pin_geometry_follows_the_placed_component() {
    let mut board = two_layer_board();
    let pad = board.library.padstacks.add(
        "smd",
        vec![
            Some(ConvexShape::Tile(TileShape::IntOctagon(IntOctagon::new(
                -400, -300, 400, 300, -500, 500, -500, 500,
            )))),
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
        Some(FloatPoint::new(7_000.0, 8_000.0)),
        0.0,
        true,
        package,
        package,
        false,
        None,
    );
    let id = board
        .insert_pin(component.0, 0, &[1], 0, FixedState::SystemFixed)
        .unwrap();
    let g = geometry(&board, id);
    assert_eq!(g.kind, BoardItemType::Pin);
    assert_eq!((g.first_layer, g.last_layer), (0, 0));
    assert_eq!(g.component, component.0);
    assert_eq!(g.shapes.len(), 1);
    assert_eq!(g.shapes[0].layer, 0);
    let GeometryShape::Octagon(octagon) = shape_of(&g, 0) else {
        panic!("an octagon pad stays an octagon: {:?}", g.shapes);
    };
    assert_eq!(
        (
            octagon.left_x,
            octagon.bottom_y,
            octagon.right_x,
            octagon.top_y
        ),
        (6_600, 7_700, 7_400, 8_300)
    );
    assert_eq!(g.bounding_box(), board.item(id).unwrap().bounding_box());
}

#[test]
fn keepouts_and_conduction_area_keep_their_shapes() {
    let mut board = two_layer_board();
    let square = IntBox::new(1_000, 1_000, 5_000, 4_000);
    let keepout = board.insert_obstacle(
        Area::from(freerouting::geometry::Shape::from(TileShape::from(square))),
        0,
        0,
        FixedState::SystemFixed,
    );
    let g = geometry(&board, keepout);
    assert_eq!(g.kind, BoardItemType::ObstacleArea);
    assert_eq!(
        g.shapes,
        vec![LayerShape {
            layer: 0,
            shape: GeometryShape::Box(square),
        }]
    );

    let circle = Area::from(freerouting::geometry::Shape::Circle(Circle::new(
        IntPoint::new(20_000, 20_000),
        900,
    )));
    let via_keepout = board.insert_via_obstacle(circle, 1, 0, FixedState::SystemFixed);
    let g = geometry(&board, via_keepout);
    assert_eq!(g.kind, BoardItemType::ViaObstacleArea);
    assert_eq!(
        g.shapes,
        vec![LayerShape {
            layer: 1,
            shape: GeometryShape::Circle {
                center: IntPoint::new(20_000, 20_000),
                radius: 900,
            },
        }]
    );

    let triangle = TileShape::from(IntBox::new(40_000, 40_000, 44_000, 44_000));
    let place_keepout = board.insert_component_obstacle(
        Area::from(freerouting::geometry::Shape::from(triangle)),
        0,
        0,
        FixedState::SystemFixed,
    );
    assert_eq!(
        geometry(&board, place_keepout).kind,
        BoardItemType::ComponentObstacleArea
    );

    // A conduction area with a square hole is a polygon with one ring.
    let border = PolylineShape::Tile(TileShape::from(IntBox::new(0, 50_000, 30_000, 70_000)));
    let hole = PolylineShape::Tile(TileShape::from(IntBox::new(10_000, 55_000, 20_000, 65_000)));
    let zone = board.insert_conduction_area(
        Area::from(PolylineArea::new(border, vec![hole])),
        0,
        &[1],
        0,
        true,
        FixedState::Unfixed,
    );
    let g = geometry(&board, zone);
    assert_eq!(g.kind, BoardItemType::ConductionArea);
    assert_eq!(g.nets, vec![1]);
    let GeometryShape::Polygon { outer, holes } = shape_of(&g, 0) else {
        panic!("a polygon: {:?}", g.shapes);
    };
    assert_eq!(outer.len(), 4);
    assert!(outer.contains(&FloatPoint::new(0.0, 50_000.0)));
    assert!(outer.contains(&FloatPoint::new(30_000.0, 70_000.0)));
    assert_eq!(holes.len(), 1);
    assert_eq!(holes[0].len(), 4);
    assert!(holes[0].contains(&FloatPoint::new(10_000.0, 55_000.0)));
    assert_eq!(g.bounding_box(), board.item(zone).unwrap().bounding_box());
    assert!(matches!(
        board.item(zone).unwrap().kind,
        ItemKind::Area(ref a) if matches!(a.kind, AreaKind::Conduction { .. })
    ));
}

#[test]
fn component_outline_is_on_the_layer_of_its_side() {
    let mut board = two_layer_board();
    let box_area = Area::from(freerouting::geometry::Shape::from(TileShape::from(
        IntBox::new(-500, -400, 500, 400),
    )));
    let back = board
        .insert_component_outline(
            box_area,
            false,
            Vector::ZERO,
            0.0,
            0,
            false,
            false,
            true,
            FixedState::SystemFixed,
        )
        .unwrap();
    let g = geometry(&board, back);
    assert_eq!(g.kind, BoardItemType::ComponentOutline);
    assert_eq!(g.shapes.len(), 1);
    assert_eq!(g.shapes[0].layer, 1);
    assert_eq!(g.bounding_box(), board.item(back).unwrap().bounding_box());
}

#[test]
fn board_outline_has_one_polygon_per_outline_shape() {
    let board = two_layer_board();
    let outline = board.outline().expect("outline");
    let g = geometry(&board, outline.id());
    assert_eq!(g.kind, BoardItemType::BoardOutline);
    assert_eq!((g.first_layer, g.last_layer), (0, 1));
    assert_eq!(g.shapes.len(), 1);
    assert_eq!(
        g.shapes[0].shape,
        GeometryShape::Box(IntBox::new(0, 0, 100_000, 80_000))
    );
    assert_eq!(g.bounding_box(), IntBox::new(0, 0, 100_000, 80_000));
}

#[test]
fn an_item_that_is_not_on_the_board_has_no_geometry() {
    let board = two_layer_board();
    assert!(board.item_geometry(ItemId(999_999)).is_none());
}

/// Every item of every probe board (as loaded, with the session imported where the row has
/// one) has a geometry whose boxes union to the item's bounding box.
#[test]
fn every_item_of_every_probe_board_has_geometry() {
    let mut items = 0usize;
    let mut shapes = 0usize;
    for row in board_probe::fixtures() {
        let mut board = board_probe::load(&row);
        board_probe::import_session(&mut board, &row);
        for item in board.items() {
            let g = board
                .item_geometry(item.id())
                .unwrap_or_else(|| panic!("{}: item {} has no geometry", row.board, item.id()));
            let expected_kind = match &item.kind {
                ItemKind::Pin(_) => BoardItemType::Pin,
                ItemKind::Via(_) => BoardItemType::Via,
                ItemKind::Trace(_) => BoardItemType::Trace,
                ItemKind::Area(a) => match a.kind {
                    AreaKind::Obstacle | AreaKind::TraceObstacle => BoardItemType::ObstacleArea,
                    AreaKind::ViaObstacle => BoardItemType::ViaObstacleArea,
                    AreaKind::ComponentObstacle => BoardItemType::ComponentObstacleArea,
                    AreaKind::Conduction { .. } => BoardItemType::ConductionArea,
                },
                ItemKind::ComponentOutline(_) => BoardItemType::ComponentOutline,
                ItemKind::BoardOutline(_) => BoardItemType::BoardOutline,
            };
            let at = format!("{} item {} ({:?})", row.board, item.id(), expected_kind);
            assert_eq!(g.id, item.id(), "{at}");
            assert_eq!(g.kind, expected_kind, "{at}");
            assert_eq!(g.first_layer, item.first_layer(), "{at}");
            assert_eq!(g.last_layer, item.last_layer(), "{at}");
            assert_eq!(g.nets, item.net_numbers(), "{at}");
            assert_eq!(g.component, item.component_id(), "{at}");
            assert_eq!(g.fixed_state, item.fixed_state(), "{at}");
            assert!(
                !g.shapes.is_empty() || item.bounding_box().is_empty(),
                "{at}: no shapes"
            );
            for s in &g.shapes {
                assert!(
                    s.layer >= g.first_layer && s.layer <= g.last_layer,
                    "{at}: shape on layer {}",
                    s.layer
                );
            }
            // Drill items and the outline start their box union from `IntBox::EMPTY`, which
            // clamps boxes beyond CRIT_INT (see `ItemGeometry::bounding_box`).
            let folded = match item.kind {
                ItemKind::Pin(_) | ItemKind::Via(_) | ItemKind::BoardOutline(_) => {
                    g.shapes.iter().fold(IntBox::EMPTY, |acc, s| {
                        acc.union_box(&s.shape.bounding_box())
                    })
                }
                _ => g.bounding_box(),
            };
            assert_eq!(folded, item.bounding_box(), "{at}");
            let b = g.bounding_box();
            if b.ll.x.min(b.ll.y) >= -CRIT_INT && b.ur.x.max(b.ur.y) <= CRIT_INT {
                assert_eq!(b, item.bounding_box(), "{at}");
            }
            items += 1;
            shapes += g.shapes.len();
        }
    }
    eprintln!("{items} items, {shapes} shapes");
    assert!(items > 100_000, "the probe set has many items: {items}");
}
