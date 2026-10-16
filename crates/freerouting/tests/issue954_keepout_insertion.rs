//! Port of Java's `Issue954CircularKeepoutInsertionTest`
//! (`src/test/java/app/freerouting/fixtures/`, at `a773a70ec`), O04.6.
//!
//! All 10 methods (75 cases) on the board built from
//! `fixtures/Issue954-circle.dsn` as the Java test builds it: the circle
//! diameter replaced, the circular keepouts optionally removed, any-angle
//! traces, the vias removed. The insertion is
//! [`RoutingBoard::insert_forced_trace_polyline`] with Java's arguments
//! (depths 10, 10, 20, tidy width 0, accuracy 100, with check); the cleanup
//! normalizes net 1 and pulls every trace tight on its own net with accuracy
//! 100.
//!
//! The cases whose cleanup pull-tightens in angle mode `NONE` (the 20 `NONE`
//! cases of `insertsDetour` and all of `obliqueCircularDetourSurvivesCleanup`
//! (3), `asymmetricPolygonDetourSurvivesCleanup` (3),
//! `partitionedRectangleSupportsDetour`,
//! `successiveCircularObstaclesRemainClearAfterCleanup` and
//! `circularCopperAreaRespectsNetAndObstacleFlag` (3), because Java's
//! `loadBoard` sets `NONE` for them) use the any-angle tightener (O02). All
//! 75 cases run.

use freerouting::board::RoutingBoard;
use freerouting::board::items::{Item, ItemKind};
use freerouting::board::structure::{AngleRestriction, FixedState};
use freerouting::drc::load_design;
use freerouting::geometry::jmath::round_to_i32;
use freerouting::geometry::shapes::{Area, PolylineArea, PolylineShape, Shape};
use freerouting::geometry::{
    Circle, FloatPoint, IntBox, IntPoint, Point, PolygonShape, Polyline, TileShape,
};
use freerouting::shove::{ForcedInsert, ForcedTrace, ForcedTraceParams, SpringOver, TraceShover};

const PROFILES: [(i32, i32); 5] = [(750, 0), (250, 0), (1250, 0), (750, -1), (750, 1)];

fn fixture_text() -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/Issue954-circle.dsn"
    );
    std::fs::read_to_string(path).expect("fixture")
}

/// Java `loadBoard(diameter, keepCircularObstacles)`.
fn load_board(diameter: i32, keep_circular_obstacles: bool) -> RoutingBoard {
    let mut dsn = fixture_text().replace("5000)", &format!("{diameter})"));
    if !keep_circular_obstacles {
        dsn = dsn
            .replace(&format!("(keepout \"\" (circle F.Cu {diameter}))"), "")
            .replace(&format!("(keepout \"\" (circle B.Cu {diameter}))"), "");
    }
    let mut board = load_design(dsn.as_bytes()).expect("board");
    board
        .rules
        .set_trace_angle_restriction(AngleRestriction::None);
    if !keep_circular_obstacles {
        assert_eq!(area_count(&board), 0);
    }
    for via in board.vias() {
        board.set_fixed_state(via, FixedState::Unfixed);
        board.remove_item(via);
    }
    assert!(board.vias().is_empty());
    assert!(board.traces().is_empty());
    assert!(board.all_clearance_violations().is_empty());
    board
}

/// Java's count of `ObstacleArea` instances (conduction areas included).
fn area_count(board: &RoutingBoard) -> usize {
    board
        .items()
        .filter(|i| matches!(i.kind, ItemKind::Area(_)))
        .count()
}

fn keepout_on_layer_0(board: &RoutingBoard) -> Item {
    board
        .items()
        .find(|i| matches!(i.kind, ItemKind::Area(_)) && i.is_on_layer(0))
        .expect("keepout on layer 0")
        .clone()
}

fn circle_of(item: &Item) -> Circle {
    let ItemKind::Area(area) = &item.kind else {
        panic!("not an area");
    };
    match area.area() {
        Area::Shape(Shape::Circle(circle)) => *circle,
        _ => panic!("the keepout is not a circle"),
    }
}

fn tree_shape_count(board: &RoutingBoard, id: freerouting::board::items::ItemId) -> usize {
    let item = board.item(id).expect("item");
    board
        .search_trees()
        .default_tree()
        .tree_shape_count(item, &board.shape_context())
}

/// Java `insertAndClean(board, path, halfWidth, checkGeometry)`.
fn insert_and_clean(
    board: &mut RoutingBoard,
    path: &Polyline,
    half_width: i32,
    check: &dyn Fn(&RoutingBoard),
) {
    let outcome = board.insert_forced_trace_polyline(
        path,
        ForcedTrace {
            half_width,
            layer: 0,
            net_numbers: &[1],
            clearance_class: 1,
        },
        ForcedTraceParams {
            max_recursion_depth: 10,
            max_via_recursion_depth: 10,
            max_spring_over_recursion_depth: 20,
            tidy_width: 0,
            pull_tight_accuracy: 100,
            with_check: true,
        },
        None,
    );
    let (first, last) = (path.first_corner(), path.last_corner());
    assert_eq!(
        outcome.point(&first, &last),
        Some(last.clone()),
        "insertion must reach the requested destination ({outcome:?})"
    );
    assert_connected_and_clear(board, path);
    check(board);
    board.normalize_traces(1);
    for id in board.traces() {
        board.pull_tight_own_net(id, 100);
    }
    assert_connected_and_clear(board, path);
    check(board);
}

fn trace_at(board: &RoutingBoard, corner: &Point) -> freerouting::board::items::ItemId {
    board
        .traces()
        .into_iter()
        .find(|&id| {
            let t = board.item(id).and_then(Item::as_trace).expect("trace");
            t.polyline.first_corner() == *corner || t.polyline.last_corner() == *corner
        })
        .expect("a trace at the path end")
}

/// Java `assertConnectedAndClear(board, path)`.
fn assert_connected_and_clear(board: &RoutingBoard, path: &Polyline) {
    assert!(!board.traces().is_empty());
    assert_eq!(board.all_clearance_violations().len(), 0);
    let start = trace_at(board, &path.first_corner());
    let end = trace_at(board, &path.last_corner());
    assert!(
        board.connected_set(start, 1, false).contains(&end),
        "insertion and optimization must preserve end-to-end connectivity"
    );
}

/// Java `segmentDistance(p, a, b)`.
fn segment_distance(p: FloatPoint, a: FloatPoint, b: FloatPoint) -> f64 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let length_square = dx * dx + dy * dy;
    let t = if length_square == 0.0 {
        0.0
    } else {
        (((p.x - a.x) * dx + (p.y - a.y) * dy) / length_square).clamp(0.0, 1.0)
    };
    (p.x - a.x - t * dx).hypot(p.y - a.y - t * dy)
}

/// Java `assertSafeTraces(board, circle, clearanceClass, obstacleClass)`.
fn assert_safe_traces(board: &RoutingBoard, circle: &Circle, class: i32, obstacle_class: i32) {
    assert_eq!(board.all_clearance_violations().len(), 0);
    let clearance = f64::from(board.rules.clearance_matrix.get_value(
        class,
        obstacle_class,
        0,
        false,
    ));
    let restriction = board.rules.trace_angle_restriction();
    for id in board.traces() {
        let item = board.item(id).expect("trace");
        let trace = item.as_trace().expect("trace");
        let routed = &trace.polyline;
        for i in 0..routed.corner_count().saturating_sub(1) {
            let line = routed.lines()[i + 1];
            match restriction {
                AngleRestriction::NinetyDegree => assert!(line.is_orthogonal()),
                AngleRestriction::FortyfiveDegree => assert!(line.is_multiple_of_45_degree()),
                AngleRestriction::None => {}
            }
            let distance = segment_distance(
                circle.center.to_float(),
                routed.corner_approx(i),
                routed.corner_approx(i + 1),
            ) - f64::from(circle.radius)
                - f64::from(trace.half_width);
            assert!(
                distance >= clearance - 0.01,
                "independently measured physical clearance must satisfy the rule"
            );
        }
    }
}

/// One case of Java `insertsDetour`.
fn inserts_detour(
    restriction: AngleRestriction,
    diameter: i32,
    reverse: bool,
    half_width: i32,
    vertical_offset: i32,
) {
    let mut board = load_board(diameter, true);
    board.rules.set_trace_angle_restriction(restriction);
    let obstacle = keepout_on_layer_0(&board);
    let circle = circle_of(&obstacle);
    let r = circle.radius;
    let center = circle.center;
    let sign = if reverse { 2 } else { -2 };
    let start = IntPoint {
        x: center.x + sign * r,
        y: center.y + vertical_offset * r / 2,
    };
    let end = IntPoint {
        x: center.x - sign * r,
        y: center.y + vertical_offset * r / 2,
    };
    let class = 1;
    let path = Polyline::from_points(&[Point::Int(start), Point::Int(end)]);
    if diameter > 5000 {
        assert!(
            tree_shape_count(&board, obstacle.id()) > 1,
            "exercise a partitioned convex obstacle"
        );
    }
    let detour =
        TraceShover::new(&mut board).spring_over_obstacles(&path, half_width, 0, &[1], class, None);
    let detour = match detour {
        SpringOver::Wrapped(p) => p,
        SpringOver::NoObstacle => path.clone(),
        SpringOver::Blocked => panic!("a convex keepout must support spring-over"),
    };
    assert!(detour.corner_count() > 2, "the route must actually detour");
    for shape in detour.offset_shapes(half_width) {
        assert!(
            board
                .overlapping_items_with_clearance(&shape, 0, &[1], class)
                .is_empty(),
            "the detour must satisfy the clearance query used during insertion"
        );
    }
    let obstacle_class = obstacle.clearance_class();
    insert_and_clean(&mut board, &path, half_width, &|b| {
        assert_safe_traces(b, &circle, class, obstacle_class)
    });
}

fn inserts_detour_cases(restriction: AngleRestriction) -> usize {
    let mut cases = 0;
    for diameter in [5000, 6000] {
        for reverse in [false, true] {
            for (half_width, vertical_offset) in PROFILES {
                let case = format!(
                    "{restriction:?} diameter {diameter} reverse {reverse} \
                     half width {half_width} offset {vertical_offset}"
                );
                let result = std::panic::catch_unwind(|| {
                    inserts_detour(restriction, diameter, reverse, half_width, vertical_offset)
                });
                if let Err(e) = result {
                    std::panic::resume_unwind(Box::new(format!("{case}: {e:?}")));
                }
                cases += 1;
            }
        }
    }
    cases
}

#[test]
fn inserts_detour_45_degree() {
    assert_eq!(inserts_detour_cases(AngleRestriction::FortyfiveDegree), 20);
}

#[test]
fn inserts_detour_90_degree() {
    assert_eq!(inserts_detour_cases(AngleRestriction::NinetyDegree), 20);
}

#[test]
fn inserts_detour_any_angle() {
    assert_eq!(inserts_detour_cases(AngleRestriction::None), 20);
}

#[test]
fn invalid_circular_keepout_placement_still_fails_drc() {
    let dsn = fixture_text().replace("11176.751555 -12840.929562", "11119.349040 -12702.347633");
    let board = load_design(dsn.as_bytes()).expect("board");
    assert_eq!(
        board.all_clearance_violations().len(),
        2,
        "true 0.145 mm circle clearance must fail the 0.200 mm rule on both copper layers"
    );
}

#[test]
fn endpoint_inside_keepout_does_not_insert_copper() {
    let mut board = load_board(5000, true);
    board
        .rules
        .set_trace_angle_restriction(AngleRestriction::None);
    let start = Point::Int(IntPoint {
        x: 50000,
        y: -100000,
    });
    let end = Point::Int(IntPoint {
        x: 100000,
        y: -100000,
    });
    let path = Polyline::from_points(&[start.clone(), end.clone()]);
    let outcome = board.insert_forced_trace_polyline(
        &path,
        ForcedTrace {
            half_width: 750,
            layer: 0,
            net_numbers: &[1],
            clearance_class: 1,
        },
        ForcedTraceParams {
            max_recursion_depth: 10,
            max_via_recursion_depth: 10,
            max_spring_over_recursion_depth: 20,
            tidy_width: 0,
            pull_tight_accuracy: 100,
            with_check: true,
        },
        None,
    );
    assert_eq!(outcome, ForcedInsert::Unchanged);
    assert_eq!(outcome.point(&start, &end), Some(start));
    assert!(board.traces().is_empty());
    assert!(board.all_clearance_violations().is_empty());
}

/// Java `assertRejectedArea(board, area)`.
fn assert_rejected_area(board: &mut RoutingBoard, area: Area) {
    assert!(area.split_to_convex().expect("convex pieces").len() > 1);
    let obstacle = board.insert_obstacle(area, 0, 1, FixedState::UserFixed);
    assert_eq!(area_count(board), 1);
    assert!(tree_shape_count(board, obstacle) > 1);
    let path = Polyline::from_points(&[
        Point::Int(IntPoint {
            x: 50000,
            y: -100000,
        }),
        Point::Int(IntPoint {
            x: 150000,
            y: -100000,
        }),
    ]);
    let result = TraceShover::new(board).spring_over_obstacles(&path, 750, 0, &[1], 1, None);
    assert!(matches!(result, SpringOver::Blocked));
    assert_eq!(board.shove_failing_obstacle(), Some(obstacle));
    assert!(board.traces().is_empty());
}

fn int_point(x: i32, y: i32) -> Point {
    Point::Int(IntPoint { x, y })
}

#[test]
fn non_convex_keepout_is_not_replaced_with_one_of_its_sections() {
    let mut board = load_board(5000, false);
    let concave = PolygonShape::new(&[
        int_point(80000, -120000),
        int_point(120000, -120000),
        int_point(120000, -110000),
        int_point(100000, -110000),
        int_point(100000, -80000),
        int_point(80000, -80000),
    ]);
    assert_rejected_area(&mut board, Area::Shape(Shape::from(concave)));
}

#[test]
fn holed_keepout_is_not_replaced_with_its_outer_boundary() {
    let border = PolylineShape::Tile(TileShape::IntBox(IntBox::new(
        70000, -130000, 130000, -70000,
    )));
    let hole = PolylineShape::Tile(TileShape::IntBox(IntBox::new(
        90000, -110000, 110000, -90000,
    )));
    let area = Area::PolylineArea(PolylineArea::new(border, vec![hole]));
    assert_rejected_area(&mut load_board(5000, false), area);
}

/// Java `obliquePath(radius, angle)`.
fn oblique_path(radius: i32, angle: i32) -> Polyline {
    let direction = f64::from(angle).to_radians();
    let dx = round_to_i32(2.1 * f64::from(radius) * direction.cos());
    let dy = round_to_i32(2.1 * f64::from(radius) * direction.sin());
    Polyline::from_points(&[
        int_point(100000 - dx, -100000 - dy),
        int_point(100000 + dx, -100000 + dy),
    ])
}

/// Java `setClearance(board, clearance)`.
fn set_clearance(board: &mut RoutingBoard, clearance: i32) {
    board.rules.clearance_matrix.set_value(1, 1, clearance);
    board.clearance_value_changed();
}

fn oblique_circular_detour(diameter: i32, clearance: i32, width: i32, angle: i32) {
    let mut board = load_board(diameter, true);
    set_clearance(&mut board, clearance);
    let obstacle = keepout_on_layer_0(&board);
    let circle = circle_of(&obstacle);
    let obstacle_class = obstacle.clearance_class();
    insert_and_clean(
        &mut board,
        &oblique_path(circle.radius, angle),
        width,
        &|b| assert_safe_traces(b, &circle, 1, obstacle_class),
    );
}

#[test]
fn oblique_circular_detour_survives_cleanup() {
    for (diameter, clearance, width, angle) in [
        (5000, 17, 1, 7),
        (5000, 2001, 751, 22),
        (6000, 3017, 250, 73),
    ] {
        oblique_circular_detour(diameter, clearance, width, angle);
    }
}

fn asymmetric_polygon_detour(radius: i32, clearance: i32, width: i32, angle: i32) {
    let mut board = load_board(5000, false);
    set_clearance(&mut board, clearance);
    let corners: Vec<Point> = (0..5)
        .map(|k| {
            let direction = 2.0 * std::f64::consts::PI * f64::from(k) / 5.0 + 0.13;
            int_point(
                100000 + round_to_i32(f64::from(radius) * direction.cos()),
                -100000 + round_to_i32(0.6 * f64::from(radius) * direction.sin()),
            )
        })
        .collect();
    let polygon = Shape::from(PolygonShape::new(&corners));
    assert_eq!(polygon.split_to_convex().expect("convex").len(), 1);
    let obstacle = board.insert_obstacle(Area::Shape(polygon), 0, 1, FixedState::UserFixed);
    if radius > 25000 {
        assert!(tree_shape_count(&board, obstacle) > 1);
    }
    insert_and_clean(&mut board, &oblique_path(radius, angle), width, &|_| {});
}

#[test]
fn asymmetric_polygon_detour_survives_cleanup() {
    for (radius, clearance, width, angle) in [
        (24000, 17, 1, 7),
        (36000, 2001, 751, 22),
        (36000, 3017, 250, 73),
    ] {
        asymmetric_polygon_detour(radius, clearance, width, angle);
    }
}

#[test]
fn partitioned_rectangle_supports_detour() {
    let mut board = load_board(5000, false);
    let rectangle = Shape::from(IntBox::new(70000, -115000, 130000, -85000));
    let obstacle = board.insert_obstacle(Area::Shape(rectangle), 0, 1, FixedState::UserFixed);
    assert!(tree_shape_count(&board, obstacle) > 1);
    insert_and_clean(&mut board, &oblique_path(30000, 22), 751, &|_| {});
}

#[test]
fn successive_circular_obstacles_remain_clear_after_cleanup() {
    let mut board = load_board(5000, false);
    let first = Circle::new(
        IntPoint {
            x: 80000,
            y: -100000,
        },
        12000,
    );
    let second = Circle::new(
        IntPoint {
            x: 120000,
            y: -105000,
        },
        12000,
    );
    for circle in [&first, &second] {
        board.insert_obstacle(
            Area::Shape(Shape::Circle(*circle)),
            0,
            1,
            FixedState::UserFixed,
        );
    }
    let path = Polyline::from_points(&[int_point(40000, -100000), int_point(160000, -100000)]);
    insert_and_clean(&mut board, &path, 751, &|b| {
        assert_safe_traces(b, &first, 1, 1);
        assert_safe_traces(b, &second, 1, 1);
    });
}

fn circular_copper_area(same_net: bool, blocks_foreign_nets: bool) {
    let mut board = load_board(5000, false);
    let net = if same_net {
        1
    } else {
        let class = board.rules.default_net_class();
        board.rules.nets.add("FOREIGN", 1, false, class)
    };
    let circle = Circle::new(
        IntPoint {
            x: 100000,
            y: -100000,
        },
        30000,
    );
    let area = board.insert_conduction_area(
        Area::Shape(Shape::Circle(circle)),
        0,
        &[net],
        1,
        blocks_foreign_nets,
        FixedState::UserFixed,
    );
    assert!(tree_shape_count(&board, area) > 1);
    let path = oblique_path(circle.radius, 22);
    let detour = TraceShover::new(&mut board).spring_over_obstacles(&path, 751, 0, &[1], 1, None);
    let needs_detour = !same_net && blocks_foreign_nets;
    match detour {
        SpringOver::Blocked => panic!("the detour must exist"),
        SpringOver::Wrapped(p) => {
            assert!(needs_detour, "traversable copper must not force a detour");
            assert!(p.corner_count() > 2);
        }
        SpringOver::NoObstacle => assert!(!needs_detour, "the copper must force a detour"),
    }
    let area_class = board.item(area).expect("area").clearance_class();
    insert_and_clean(&mut board, &path, 751, &|b| {
        if needs_detour {
            assert_safe_traces(b, &circle, 1, area_class);
        }
    });
}

#[test]
fn circular_copper_area_respects_net_and_obstacle_flag() {
    for (same_net, blocks) in [(true, true), (false, true), (false, false)] {
        circular_copper_area(same_net, blocks);
    }
}
