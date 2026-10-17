//! Per-net clearance-inflated obstacles in a window (B09.2).
//!
//! On Issue026-J2_reference and Issue145-smoothieboard, for a few nets and
//! a window around one of their pins:
//! - every item that `check_trace_shape` (B04.3) finds in the way of a probe
//!   segment inside the window is in the view, one of its view shapes
//!   reaches the probe's centreline, and no item of the net is in the view;
//! - the view is the same on a second call and equals the union of the
//!   views of the window's four quadrants, deduplicated by (id, shape index).

mod board_probe;

use std::collections::BTreeMap;

use freerouting::board::items::ItemId;
use freerouting::board::{ObstacleShape, RoutingBoard};
use freerouting::geometry::{IntBox, IntPoint, Point, Polyline, TileShape};

fn trace_clearance_class(board: &RoutingBoard, net: i32) -> i32 {
    let net = board.rules.nets.get(net).expect("net");
    board
        .rules
        .net_classes
        .get(net.net_class())
        .trace_clearance_class()
}

fn segment(from: IntPoint, to: IntPoint, half_width: i32) -> TileShape {
    Polyline::from_corners(&Point::Int(from), &Point::Int(to))
        .offset_shape(half_width, 0)
        .expect("a segment shape")
}

/// A net to probe: one of its pins' centre and layer.
struct Case {
    net: i32,
    center: IntPoint,
    layer: i32,
}

/// Up to `count` nets, from pins spread over the item list.
fn cases(board: &RoutingBoard, count: usize) -> Vec<Case> {
    let mut by_net: BTreeMap<i32, Case> = BTreeMap::new();
    for item in board.items().filter(|i| i.is_pin() && i.net_count() == 1) {
        let net = item.net_number(0);
        by_net.entry(net).or_insert_with(|| Case {
            net,
            center: item.drill().expect("drill").center,
            layer: item.first_layer(),
        });
    }
    let all: Vec<Case> = by_net.into_values().collect();
    let step = (all.len() / count).max(1);
    all.into_iter().step_by(step).take(count).collect()
}

fn window_of(board: &RoutingBoard, case: &Case) -> IntBox {
    let b = board.bounding_box();
    let half = ((b.ur.x - b.ll.x).min(b.ur.y - b.ll.y) / 16).max(1000);
    IntBox::new(
        case.center.x - half,
        case.center.y - half,
        case.center.x + half,
        case.center.y + half,
    )
}

fn key(o: &ObstacleShape) -> String {
    format!(
        "{:?} {} {} {:?}",
        o.item, o.shape_index, o.is_via_only, o.shape
    )
}

fn view_half_width(board: &RoutingBoard, net: i32, layer: i32) -> (i32, i32) {
    let class = trace_clearance_class(board, net);
    let half_width = board.rules.trace_half_width(net, layer);
    let compensation = board
        .rules
        .clearance_matrix
        .clearance_compensation_value(class, layer);
    (half_width, half_width + compensation)
}

fn check_probes(board: &mut RoutingBoard, case: &Case) -> usize {
    let window = window_of(board, case);
    let (half_width, view_width) = view_half_width(board, case.net, case.layer);
    let class = trace_clearance_class(board, case.net);
    let view = board.net_obstacles(case.net, case.layer, window, view_width);
    let mut by_item: BTreeMap<ItemId, Vec<&ObstacleShape>> = BTreeMap::new();
    for o in &view {
        let item = board.item(o.item).expect("view item on the board");
        assert!(!item.contains_net(case.net), "own-net item {:?}", o.item);
        by_item.entry(o.item).or_default().push(o);
    }
    let (w, h) = (window.ur.x - window.ll.x, window.ur.y - window.ll.y);
    let length = w / 16;
    let mut blocked = 0;
    for i in 0..12 {
        for j in 0..12 {
            let from = IntPoint::new(
                window.ll.x + w / 16 * (i + 2),
                window.ll.y + h / 16 * (j + 2),
            );
            let to = if (i + j) % 2 == 0 {
                IntPoint::new(from.x + length, from.y)
            } else {
                IntPoint::new(from.x + length, from.y + length)
            };
            let probe = segment(from, to, half_width);
            let fits = board.check_trace_shape(&probe, case.layer, &[case.net], class, None);
            let blockers: Vec<ItemId> = board
                .overlapping_items_with_clearance_java(&probe, case.layer, &[], class)
                .into_iter()
                .filter(|id| {
                    board
                        .item(*id)
                        .is_some_and(|item| board.is_trace_obstacle(item, case.net))
                })
                .collect();
            assert_eq!(fits, blockers.is_empty(), "{from:?}-{to:?}: {blockers:?}");
            if fits {
                continue;
            }
            blocked += 1;
            // The centreline, widened by 2 units for the rounding of the
            // enlargements.
            let centreline = segment(from, to, 2);
            for id in blockers {
                let shapes = by_item
                    .get(&id)
                    .unwrap_or_else(|| panic!("blocker {id:?} of {from:?}-{to:?} not in the view"));
                assert!(
                    shapes.iter().any(|o| o.shape.intersects_tile(&centreline)),
                    "no view shape of {id:?} reaches {from:?}-{to:?}"
                );
            }
        }
    }
    blocked
}

fn check_order_and_quadrants(board: &mut RoutingBoard, case: &Case) -> usize {
    let window = window_of(board, case);
    let (_, view_width) = view_half_width(board, case.net, case.layer);
    let whole: Vec<String> = board
        .net_obstacles(case.net, case.layer, window, view_width)
        .iter()
        .map(key)
        .collect();
    let again: Vec<String> = board
        .net_obstacles(case.net, case.layer, window, view_width)
        .iter()
        .map(key)
        .collect();
    assert_eq!(whole, again);

    let (mx, my) = (
        window.ll.x + (window.ur.x - window.ll.x) / 2,
        window.ll.y + (window.ur.y - window.ll.y) / 2,
    );
    let quadrants = [
        IntBox::new(window.ll.x, window.ll.y, mx, my),
        IntBox::new(mx, window.ll.y, window.ur.x, my),
        IntBox::new(window.ll.x, my, mx, window.ur.y),
        IntBox::new(mx, my, window.ur.x, window.ur.y),
    ];
    // Keyed by (id, shape index) in the view's order: highest id first.
    let mut merged: BTreeMap<(ItemId, usize), String> = BTreeMap::new();
    for quadrant in quadrants {
        for o in board.net_obstacles(case.net, case.layer, quadrant, view_width) {
            merged.insert((o.item, o.shape_index), key(&o));
        }
    }
    let merged: Vec<String> = merged.into_values().collect();
    assert_eq!(whole, merged);
    whole.len()
}

fn check_board(name: &str) {
    let mut board = board_probe::load(&board_probe::fixture(name));
    let cases = cases(&board, 6);
    assert!(!cases.is_empty());
    // The view creates the compensated trees it needs and touches no tree
    // that exists already.
    let trees_before: Vec<Vec<String>> = board
        .search_trees()
        .trees()
        .iter()
        .map(|t| t.walk_lines())
        .collect();
    let (mut blocked, mut shapes) = (0, 0);
    for case in &cases {
        blocked += check_probes(&mut board, case);
        shapes += check_order_and_quadrants(&mut board, case);
    }
    let trees_after = board.search_trees().trees();
    assert!(trees_after.len() >= trees_before.len());
    for (before, after) in trees_before.iter().zip(trees_after) {
        assert_eq!(before, &after.walk_lines());
    }
    eprintln!(
        "{name}: {} nets, {blocked} blocked probes, {shapes} view shapes",
        cases.len()
    );
    assert!(blocked > 0, "no probe was blocked");
}

#[test]
fn j2_view_holds_every_blocker() {
    check_board("Issue026-J2_reference");
}

#[test]
fn smoothieboard_view_holds_every_blocker() {
    check_board("Issue145-smoothieboard");
}
