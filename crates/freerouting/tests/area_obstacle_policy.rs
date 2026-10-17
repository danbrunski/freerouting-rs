//! The per-net obstacle hook for conduction areas (B09.4).
//!
//! With no policy the board keeps Java's per-area rule; the parity suites
//! (`reference_load`, `reference_drc`, `board_probe_tree`) cover that path.
//! These tests install a policy that blocks one net and check that the
//! read-only checks (B04.3) and the clearance check see the area as an
//! obstacle for that net only.

mod board_probe;

use std::collections::BTreeSet;
use std::sync::Arc;

use freerouting::board::RoutingBoard;
use freerouting::board::items::{AreaKind, ItemId};
use freerouting::board::structure::FixedState;
use freerouting::geometry::{IntPoint, Point, Polyline, TileShape};
use freerouting::rules::AreaObstaclePolicy;

/// Blocks net `0` of the tuple for every area, or only for `area` if set.
#[derive(Debug)]
struct BlockNet {
    net: i32,
    area: Option<ItemId>,
}

impl AreaObstaclePolicy for BlockNet {
    fn blocks(&self, area: ItemId, net: i32) -> bool {
        net == self.net && self.area.is_none_or(|a| a == area)
    }
}

/// A probe inside a non-obstacle conduction area: the area, its layer and
/// net, the segment's end points and half width, and two foreign nets for
/// which the segment fits without a policy.
struct Probe {
    area: ItemId,
    layer: i32,
    area_net: i32,
    from: IntPoint,
    to: IntPoint,
    half_width: i32,
    blocked_net: i32,
    other_net: i32,
}

fn trace_clearance_class(board: &RoutingBoard, net: i32) -> i32 {
    let net = board.rules.nets.get(net).expect("net");
    board
        .rules
        .net_classes
        .get(net.net_class())
        .trace_clearance_class()
}

fn segment_shape(from: IntPoint, to: IntPoint, half_width: i32) -> TileShape {
    Polyline::from_corners(&Point::Int(from), &Point::Int(to))
        .offset_shape(half_width, 0)
        .expect("a segment shape")
}

fn fits(board: &RoutingBoard, probe: &Probe, net: i32) -> bool {
    let shape = segment_shape(probe.from, probe.to, probe.half_width);
    let class = trace_clearance_class(board, net);
    board.check_trace_shape(&shape, probe.layer, &[net], class, None)
}

/// The pin nets of the board other than `area_net`, ascending.
fn foreign_nets(board: &RoutingBoard, area_net: i32) -> Vec<i32> {
    let nets: BTreeSet<i32> = board
        .items()
        .filter(|i| i.is_pin())
        .flat_map(|i| i.net_numbers().to_vec())
        .filter(|n| *n > 0 && *n != area_net)
        .collect();
    nets.into_iter().collect()
}

/// The first probe, scanning the non-obstacle conduction areas in item
/// order and points on a 16 x 16 grid over each area's box.
fn find_probe(board: &RoutingBoard) -> Probe {
    for area_id in board.conduction_areas() {
        let item = board.item(area_id).expect("area");
        let area = item.as_conduction_area().expect("conduction area");
        if !matches!(
            area.kind,
            AreaKind::Conduction {
                is_obstacle: false,
                ..
            }
        ) || item.net_count() == 0
        {
            continue;
        }
        let area_net = item.net_number(0);
        let nets = foreign_nets(board, area_net);
        let bounds = area.area().bounding_box();
        let half_width = 100;
        let length = (bounds.ur.x - bounds.ll.x) / 64;
        for i in 1..16 {
            for j in 1..16 {
                let from = IntPoint::new(
                    bounds.ll.x + (bounds.ur.x - bounds.ll.x) / 16 * i,
                    bounds.ll.y + (bounds.ur.y - bounds.ll.y) / 16 * j,
                );
                let to = IntPoint::new(from.x + length, from.y);
                if !area.area().contains(&Point::Int(from))
                    || !area.area().contains(&Point::Int(to))
                {
                    continue;
                }
                let mut probe = Probe {
                    area: area_id,
                    layer: area.layer(),
                    area_net,
                    from,
                    to,
                    half_width,
                    blocked_net: 0,
                    other_net: 0,
                };
                let fitting: Vec<i32> = nets
                    .iter()
                    .copied()
                    .filter(|n| fits(board, &probe, *n))
                    .take(2)
                    .collect();
                if fitting.len() == 2 {
                    probe.blocked_net = fitting[0];
                    probe.other_net = fitting[1];
                    return probe;
                }
            }
        }
    }
    panic!("no free spot inside a non-obstacle conduction area");
}

fn smoothieboard() -> RoutingBoard {
    board_probe::load(&board_probe::fixture("Issue145-smoothieboard"))
}

#[test]
fn a_policy_blocks_only_its_net_in_check_trace_shape() {
    let mut board = smoothieboard();
    let probe = find_probe(&board);
    assert!(fits(&board, &probe, probe.blocked_net));
    assert!(fits(&board, &probe, probe.other_net));
    assert!(fits(&board, &probe, probe.area_net));

    board.rules.area_obstacle_policy = Some(Arc::new(BlockNet {
        net: probe.blocked_net,
        area: Some(probe.area),
    }));
    assert!(!fits(&board, &probe, probe.blocked_net));
    assert!(fits(&board, &probe, probe.other_net));
    assert!(fits(&board, &probe, probe.area_net));

    // A policy for another area leaves this one alone.
    board.rules.area_obstacle_policy = Some(Arc::new(BlockNet {
        net: probe.blocked_net,
        area: Some(ItemId(i32::MAX)),
    }));
    assert!(fits(&board, &probe, probe.blocked_net));

    board.rules.area_obstacle_policy = None;
    assert!(fits(&board, &probe, probe.blocked_net));
}

#[test]
fn the_policy_is_never_asked_for_the_areas_own_net() {
    let mut board = smoothieboard();
    let probe = find_probe(&board);
    board.rules.area_obstacle_policy = Some(Arc::new(BlockNet {
        net: probe.area_net,
        area: None,
    }));
    assert!(fits(&board, &probe, probe.area_net));
    let area = board.item(probe.area).expect("area");
    assert!(!board.is_trace_obstacle(area, probe.area_net));
}

#[test]
fn a_blocked_trace_inside_the_area_is_a_clearance_violation() {
    let mut board = smoothieboard();
    let probe = find_probe(&board);
    let class = trace_clearance_class(&board, probe.blocked_net);
    let polyline = Polyline::from_corners(&Point::Int(probe.from), &Point::Int(probe.to));
    let trace = board
        .insert_trace_without_cleaning(
            polyline,
            probe.layer,
            probe.half_width,
            &[probe.blocked_net],
            class,
            FixedState::Unfixed,
        )
        .expect("trace inserted");
    let with_area = |board: &RoutingBoard, id: ItemId| {
        board
            .clearance_violations(id)
            .iter()
            .any(|v| v.first_item == probe.area || v.second_item == probe.area)
    };
    assert!(!with_area(&board, trace));
    assert!(!with_area(&board, probe.area));

    board.rules.area_obstacle_policy = Some(Arc::new(BlockNet {
        net: probe.blocked_net,
        area: Some(probe.area),
    }));
    // Both directions of `is_obstacle`: the trace's check and the area's.
    assert!(with_area(&board, trace));
    assert!(with_area(&board, probe.area));
    let (area, item) = (
        board.item(probe.area).expect("area"),
        board.item(trace).expect("trace"),
    );
    assert!(board.is_obstacle(area, item));
    assert!(board.is_obstacle(item, area));

    // A policy for another net does not.
    board.rules.area_obstacle_policy = Some(Arc::new(BlockNet {
        net: probe.other_net,
        area: None,
    }));
    assert!(!with_area(&board, trace));
}

#[test]
fn a_board_with_a_policy_stays_sync_and_clones_share_it() {
    fn assert_sync<T: Sync + Send>() {}
    assert_sync::<freerouting::board::BasicBoard>();
    assert_sync::<RoutingBoard>();

    let mut board = smoothieboard();
    let policy: Arc<dyn AreaObstaclePolicy> = Arc::new(BlockNet { net: 1, area: None });
    board.rules.area_obstacle_policy = Some(policy.clone());
    let copy = board.deep_copy();
    let shared = copy
        .rules
        .area_obstacle_policy
        .as_ref()
        .expect("the copy keeps the policy");
    assert!(Arc::ptr_eq(shared, &policy));
    assert_eq!(
        format!("{:?}", copy.rules.area_obstacle_policy),
        "Some(AreaObstaclePolicy)"
    );
}
