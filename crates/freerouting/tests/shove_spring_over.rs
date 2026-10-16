//! Spring-over (O03.5): the `springOverObstacles` assertions of Java's
//! `Issue954CircularKeepoutInsertionTest.insertsDetour`
//! (`src/test/java/app/freerouting/fixtures/`, at `a773a70ec`).
//!
//! Ported: the 60 cases (3 angle modes x 2 circle diameters x 2 directions x
//! 5 width/offset profiles) on the board built from
//! `fixtures/Issue954-circle.dsn` as the Java test builds it; the detour
//! exists, has more than 2 corners, and each of its offset shapes is clear
//! by the clearance query the insertion uses; with diameter 6000 the
//! keepout is partitioned in the tree.
//!
//! Not ported here: the insertion and cleanup half of `insertsDetour`
//! (`insertAndClean`: `insertForcedTracePolyline`, pull-tight), which O04
//! ports in `tests/issue954_keepout_insertion.rs`; the spring-over halves of
//! `assertRejectedArea` and `circularCopperAreaRespectsNetAndObstacleFlag`,
//! which insert a polygon keepout or a circular copper area first (O04, with
//! the rest of those tests); `invalidCircularKeepoutPlacementStillFailsDrc`
//! (DRC, not shove); the other tests of the class insert traces (O04).

use freerouting::board::RoutingBoard;
use freerouting::board::items::{Item, ItemKind};
use freerouting::board::structure::{AngleRestriction, FixedState};
use freerouting::drc::load_design;
use freerouting::geometry::shapes::{Area, Shape};
use freerouting::geometry::{IntPoint, Point, Polyline};
use freerouting::shove::{SpringOver, TraceShover};

const PROFILES: [(i32, i32); 5] = [(750, 0), (250, 0), (1250, 0), (750, -1), (750, 1)];

/// Java `loadBoard(diameter)`: the fixture with the circle diameter
/// replaced, any-angle traces, and the vias removed.
fn load_board(diameter: i32) -> RoutingBoard {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/Issue954-circle.dsn"
    );
    let dsn = std::fs::read_to_string(path)
        .expect("fixture")
        .replace("5000)", &format!("{diameter})"));
    let mut board = load_design(dsn.as_bytes()).expect("board");
    board
        .rules
        .set_trace_angle_restriction(AngleRestriction::None);
    for via in board.vias() {
        board.set_fixed_state(via, FixedState::Unfixed);
        board.remove_item(via);
    }
    assert!(board.vias().is_empty());
    assert!(board.traces().is_empty());
    board
}

fn keepout_on_layer_0(board: &RoutingBoard) -> Item {
    board
        .items()
        .find(|i| {
            matches!(i.kind, ItemKind::Area(_)) && !i.is_conduction_area() && i.is_on_layer(0)
        })
        .expect("keepout on layer 0")
        .clone()
}

#[test]
fn spring_over_obstacles_wraps_circular_keepout() {
    let mut cases = 0;
    for restriction in [
        AngleRestriction::None,
        AngleRestriction::FortyfiveDegree,
        AngleRestriction::NinetyDegree,
    ] {
        for diameter in [5000, 6000] {
            for reverse in [false, true] {
                for (half_width, vertical_offset) in PROFILES {
                    let context = format!(
                        "{restriction:?} diameter {diameter} reverse {reverse} \
                         half width {half_width} offset {vertical_offset}"
                    );
                    insert_detour_case(
                        restriction,
                        diameter,
                        reverse,
                        half_width,
                        vertical_offset,
                        &context,
                    );
                    cases += 1;
                }
            }
        }
    }
    assert_eq!(cases, 60);
}

fn insert_detour_case(
    restriction: AngleRestriction,
    diameter: i32,
    reverse: bool,
    half_width: i32,
    vertical_offset: i32,
    context: &str,
) {
    let mut board = load_board(diameter);
    board.rules.set_trace_angle_restriction(restriction);
    let obstacle = keepout_on_layer_0(&board);
    let ItemKind::Area(area) = &obstacle.kind else {
        unreachable!()
    };
    let Area::Shape(Shape::Circle(circle)) = area.area() else {
        panic!("{context}: the keepout is not a circle");
    };
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
    let clearance_class = 1;
    let path = Polyline::from_points(&[Point::Int(start), Point::Int(end)]);
    if diameter > 5000 {
        let tree = board.search_trees().default_tree();
        assert!(
            tree.tree_shape_count(&obstacle, &board.shape_context()) > 1,
            "{context}: exercise a partitioned convex obstacle"
        );
    }
    let detour = TraceShover::new(&mut board).spring_over_obstacles(
        &path,
        half_width,
        0,
        &[1],
        clearance_class,
        None,
    );
    let detour = match detour {
        SpringOver::Wrapped(p) => p,
        SpringOver::NoObstacle => path.clone(),
        SpringOver::Blocked => {
            panic!("{context}: a convex keepout must support spring-over even when partitioned")
        }
    };
    assert!(
        detour.corner_count() > 2,
        "{context}: the route must actually detour around the keepout"
    );
    for shape in detour.offset_shapes(half_width) {
        assert!(
            board
                .overlapping_items_with_clearance(&shape, 0, &[1], clearance_class)
                .is_empty(),
            "{context}: the detour must satisfy the clearance query used during insertion"
        );
    }
}
