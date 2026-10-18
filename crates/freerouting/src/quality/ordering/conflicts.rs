// SPDX-License-Identifier: GPL-3.0-only AND MIT
// SPDX-FileCopyrightText: 2026 drandyhaas (KiCadRoutingTools)
// Portions ported from KiCadRoutingTools py_router/net_queries.py
// (compute_mps_net_ordering) and py_router/connectivity.py
// (get_net_mst_segments, compute_mst_edges, segments_intersect) at 0486e799.

//! The crossing-conflict graph of Q07.1: per net, the minimum spanning tree
//! of its pin centres; two nets conflict when a tree segment of one properly
//! crosses a tree segment of the other and their pins share a layer.
//!
//! KRT builds the same graph from stub free ends or pad positions, in
//! floats. Here it is integer geometry:
//! - **MST** ([`mst`]): Prim's algorithm over the pin centres of the net,
//!   started at the pin with the lowest item id. Edge weights are squared
//!   distances in `i64` (exact: board coordinates are within
//!   `CRIT_INT = 2^25`, so a squared distance is below `2^53`; beyond that
//!   it saturates). Among equal weights the edge with the smaller
//!   `(tree pin id, new pin id)` pair wins. The MST length is the sum of the
//!   edges' integer square roots (floor), in board units.
//! - **Crossing** ([`properly_cross`]): exact `i128` orientation tests. Two
//!   segments cross only when each one's end points lie strictly on
//!   opposite sides of the other's line. So segments that share an end
//!   point, or where one end point lies on the other segment, do not
//!   cross, and collinear segments never cross, even when they overlap
//!   (KRT's float test treats shared end points the same way and makes
//!   collinear overlap depend on rounding; here it is never a crossing).
//! - **Layers**: the set of layers the net's pins are on (a through-hole pin
//!   is on all its drill's layers). KRT reads stub layers first and assumes
//!   "any layer" without them; there are no stubs in pass 1's input here.
//!
//! Nets are the nodes, in ascending net number; the adjacency of each is a
//! sorted `Vec<u32>` of node indices.

use std::collections::BTreeSet;

use freerouting_geometry::IntPoint;

use super::{PinPoint, pins_by_net, signal_nets};
use crate::autoroute::stages::RouteTask;
use crate::board::basic_board::BasicBoard;

/// One net of the graph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetGeometry {
    /// The net number.
    pub net: i32,
    /// The MST segments over the net's pin centres.
    pub segments: Vec<(IntPoint, IntPoint)>,
    /// The layers the net's pins are on (one bit per layer).
    pub layers: u128,
    /// The MST length in board units (sum of the floor square roots).
    pub mst_length: i64,
}

impl NetGeometry {
    /// The geometry of a net with pins `pins` (in ascending item id).
    pub fn of_pins(net: i32, pins: &[PinPoint]) -> NetGeometry {
        let centres: Vec<IntPoint> = pins.iter().map(|p| p.centre).collect();
        let edges = mst(&centres);
        let mst_length = edges
            .iter()
            .map(|&(a, b)| isqrt(squared_distance(centres[a], centres[b])))
            .fold(0i64, i64::saturating_add);
        NetGeometry {
            net,
            segments: edges
                .iter()
                .map(|&(a, b)| (centres[a], centres[b]))
                .collect(),
            layers: pins.iter().fold(0, |acc, p| acc | p.layers),
            mst_length,
        }
    }
}

/// The crossing-conflict graph of a set of nets.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConflictGraph {
    nets: Vec<NetGeometry>,
    adjacency: Vec<Vec<u32>>,
}

impl ConflictGraph {
    /// The graph of the nets of the signal tasks in `tasks` (plane-net tasks
    /// are not reordered, so their nets are left out), from every pin of
    /// those nets on `board`.
    pub fn build(board: &BasicBoard, tasks: &[RouteTask]) -> ConflictGraph {
        let nets = signal_nets(tasks);
        let pins = pins_by_net(board, Some(&nets));
        let geometry = nets
            .iter()
            .map(|&net| NetGeometry::of_pins(net, pins.get(&net).map_or(&[], Vec::as_slice)))
            .collect();
        ConflictGraph::from_nets(geometry)
    }

    /// The graph of `nets` (sorted here by net number).
    pub fn from_nets(mut nets: Vec<NetGeometry>) -> ConflictGraph {
        nets.sort_by_key(|n| n.net);
        let adjacency = conflicts(&nets);
        ConflictGraph { nets, adjacency }
    }

    /// The number of nets.
    pub fn len(&self) -> usize {
        self.nets.len()
    }

    /// True if the graph has no nets.
    pub fn is_empty(&self) -> bool {
        self.nets.is_empty()
    }

    /// The nets, in ascending net number.
    pub fn nets(&self) -> &[NetGeometry] {
        &self.nets
    }

    /// The node indices `node` conflicts with, ascending.
    pub fn neighbours(&self, node: u32) -> &[u32] {
        &self.adjacency[node as usize]
    }

    /// The node of net `net`.
    pub fn node_of(&self, net: i32) -> Option<u32> {
        self.nets
            .binary_search_by_key(&net, |n| n.net)
            .ok()
            .map(|i| i as u32)
    }

    /// True if nets `a` and `b` conflict.
    pub fn conflict(&self, a: i32, b: i32) -> bool {
        match (self.node_of(a), self.node_of(b)) {
            (Some(x), Some(y)) => self.neighbours(x).binary_search(&y).is_ok(),
            _ => false,
        }
    }

    /// The number of conflicting pairs.
    pub fn edge_count(&self) -> usize {
        self.adjacency.iter().map(Vec::len).sum::<usize>() / 2
    }
}

/// One MST segment with its net node and bounding box, for the sweep.
struct SweepSegment {
    node: u32,
    a: IntPoint,
    b: IntPoint,
    min_x: i32,
    max_x: i32,
    min_y: i32,
    max_y: i32,
}

/// The adjacency lists of `nets`: a sweep over the segments in ascending
/// `min_x` compares every pair whose x ranges overlap.
fn conflicts(nets: &[NetGeometry]) -> Vec<Vec<u32>> {
    let mut segments: Vec<SweepSegment> = nets
        .iter()
        .enumerate()
        .flat_map(|(node, n)| {
            n.segments.iter().map(move |&(a, b)| SweepSegment {
                node: node as u32,
                a,
                b,
                min_x: a.x.min(b.x),
                max_x: a.x.max(b.x),
                min_y: a.y.min(b.y),
                max_y: a.y.max(b.y),
            })
        })
        .collect();
    segments.sort_by_key(|s| (s.min_x, s.node, s.a.x, s.a.y, s.b.x, s.b.y));
    let mut pairs: BTreeSet<(u32, u32)> = BTreeSet::new();
    for (i, s) in segments.iter().enumerate() {
        for t in &segments[i + 1..] {
            if t.min_x > s.max_x {
                break;
            }
            if s.node == t.node
                || t.min_y > s.max_y
                || s.min_y > t.max_y
                || nets[s.node as usize].layers & nets[t.node as usize].layers == 0
            {
                continue;
            }
            let pair = (s.node.min(t.node), s.node.max(t.node));
            if !pairs.contains(&pair) && properly_cross(s.a, s.b, t.a, t.b) {
                pairs.insert(pair);
            }
        }
    }
    let mut adjacency = vec![Vec::new(); nets.len()];
    for (a, b) in pairs {
        adjacency[a as usize].push(b);
        adjacency[b as usize].push(a);
    }
    for list in &mut adjacency {
        list.sort_unstable();
    }
    adjacency
}

/// The squared distance of `a` and `b`, saturating beyond `i64`.
pub fn squared_distance(a: IntPoint, b: IntPoint) -> i64 {
    let dx = i64::from(a.x) - i64::from(b.x);
    let dy = i64::from(a.y) - i64::from(b.y);
    dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy))
}

/// The floor square root of a non-negative `v`.
fn isqrt(v: i64) -> i64 {
    v.max(0).unsigned_abs().isqrt() as i64
}

/// The MST edges `(tree point, new point)` over `points`, in the order Prim
/// adds them, from point 0 (module docs). `points` must be in ascending pin
/// id, so an index compares as the id does.
pub fn mst(points: &[IntPoint]) -> Vec<(usize, usize)> {
    let n = points.len();
    if n < 2 {
        return Vec::new();
    }
    let mut in_tree = vec![false; n];
    // best[v] = (weight, tree point) of v's lightest edge into the tree.
    let mut best: Vec<(i64, usize)> = (0..n)
        .map(|v| (squared_distance(points[0], points[v]), 0))
        .collect();
    in_tree[0] = true;
    let mut edges = Vec::with_capacity(n - 1);
    for _ in 1..n {
        let next = (0..n)
            .filter(|&v| !in_tree[v])
            .min_by_key(|&v| (best[v].0, best[v].1, v))
            .expect("a point outside the tree");
        in_tree[next] = true;
        edges.push((best[next].1, next));
        for v in 0..n {
            if in_tree[v] {
                continue;
            }
            let w = squared_distance(points[next], points[v]);
            if (w, next) < best[v] {
                best[v] = (w, next);
            }
        }
    }
    edges
}

/// The sign of the cross product `(b - a) × (c - a)`, exact in `i128`.
fn orientation(a: IntPoint, b: IntPoint, c: IntPoint) -> i32 {
    let abx = i128::from(b.x) - i128::from(a.x);
    let aby = i128::from(b.y) - i128::from(a.y);
    let acx = i128::from(c.x) - i128::from(a.x);
    let acy = i128::from(c.y) - i128::from(a.y);
    match (abx * acy - aby * acx).signum() {
        1 => 1,
        -1 => -1,
        _ => 0,
    }
}

/// True if segment `a1 a2` properly crosses segment `b1 b2` (module docs:
/// shared end points, touching and collinear overlap are no crossing).
pub fn properly_cross(a1: IntPoint, a2: IntPoint, b1: IntPoint, b2: IntPoint) -> bool {
    let o1 = orientation(a1, a2, b1);
    let o2 = orientation(a1, a2, b2);
    let o3 = orientation(b1, b2, a1);
    let o4 = orientation(b1, b2, a2);
    o1 * o2 < 0 && o3 * o4 < 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::items::ItemId;
    use freerouting_geometry::limits::CRIT_INT;

    fn p(x: i32, y: i32) -> IntPoint {
        IntPoint { x, y }
    }

    fn pin(id: i32, x: i32, y: i32, layers: u128) -> PinPoint {
        PinPoint {
            id: ItemId(id),
            centre: p(x, y),
            layers,
            component: 0,
        }
    }

    fn two_pin(net: i32, a: IntPoint, b: IntPoint, layers: u128) -> NetGeometry {
        NetGeometry::of_pins(
            net,
            &[
                pin(net * 10, a.x, a.y, layers),
                pin(net * 10 + 1, b.x, b.y, layers),
            ],
        )
    }

    #[test]
    fn crossing_two_pin_nets_on_one_layer_conflict() {
        let g = ConflictGraph::from_nets(vec![
            two_pin(1, p(0, 0), p(100, 100), 1),
            two_pin(2, p(0, 100), p(100, 0), 1),
        ]);
        assert!(g.conflict(1, 2));
        assert_eq!(g.neighbours(0), [1]);
        assert_eq!(g.edge_count(), 1);
    }

    #[test]
    fn crossing_nets_on_disjoint_layers_do_not_conflict() {
        let g = ConflictGraph::from_nets(vec![
            two_pin(1, p(0, 0), p(100, 100), 0b01),
            two_pin(2, p(0, 100), p(100, 0), 0b10),
        ]);
        assert!(!g.conflict(1, 2));
        // A through-hole net (both layers) conflicts with either.
        let g = ConflictGraph::from_nets(vec![
            two_pin(1, p(0, 0), p(100, 100), 0b01),
            two_pin(2, p(0, 100), p(100, 0), 0b11),
        ]);
        assert!(g.conflict(1, 2));
    }

    #[test]
    fn segments_sharing_an_end_point_do_not_cross() {
        assert!(!properly_cross(p(0, 0), p(100, 100), p(0, 0), p(100, 0)));
        assert!(!properly_cross(
            p(0, 0),
            p(100, 100),
            p(100, 0),
            p(100, 100)
        ));
        // An end point on the other segment's interior (a T) is no crossing.
        assert!(!properly_cross(p(0, 0), p(100, 0), p(50, 0), p(50, 50)));
        let g = ConflictGraph::from_nets(vec![
            two_pin(1, p(0, 0), p(100, 100), 1),
            two_pin(2, p(100, 100), p(200, 0), 1),
        ]);
        assert!(!g.conflict(1, 2));
    }

    #[test]
    fn collinear_overlap_is_no_crossing() {
        assert!(!properly_cross(p(0, 0), p(100, 0), p(50, 0), p(150, 0)));
        assert!(!properly_cross(p(0, 0), p(100, 100), p(20, 20), p(80, 80)));
        let g = ConflictGraph::from_nets(vec![
            two_pin(1, p(0, 0), p(100, 0), 1),
            two_pin(2, p(50, 0), p(150, 0), 1),
        ]);
        assert!(!g.conflict(1, 2));
    }

    #[test]
    fn crossing_is_exact_at_the_coordinate_limits() {
        let m = CRIT_INT - 1;
        assert!(properly_cross(p(-m, -m), p(m, m), p(-m, m), p(m, -m)));
        // A line one unit off the diagonal, parallel, misses; tilted through
        // the centre it crosses; half a unit at full scale is still seen.
        assert!(!properly_cross(
            p(-m, -m),
            p(m, m),
            p(-m, -m + 1),
            p(m - 1, m)
        ));
        assert!(properly_cross(
            p(-m, -m),
            p(m, m),
            p(-m, -m + 1),
            p(m, m - 1)
        ));
        assert!(properly_cross(p(-m, -m), p(m, m - 1), p(0, 0), p(0, -1)));
        assert!(!properly_cross(p(-m, -m), p(m, m - 1), p(0, 1), p(0, 0)));
        assert_eq!(
            squared_distance(p(-m, -m), p(m, m)),
            2 * (2 * i64::from(m)).pow(2)
        );
        // Beyond CRIT_INT the squared distance saturates instead of wrapping.
        assert_eq!(
            squared_distance(p(i32::MIN, i32::MIN), p(i32::MAX, i32::MAX)),
            i64::MAX
        );
    }

    #[test]
    fn mst_starts_at_the_lowest_id_and_breaks_ties_by_id_pair() {
        // A unit square: four equal edges; Prim from 0 takes 0-1, then the
        // lightest edge to the tree with the smallest (tree, new) pair.
        let square = [p(0, 0), p(10, 0), p(0, 10), p(10, 10)];
        assert_eq!(mst(&square), [(0, 1), (0, 2), (1, 3)]);
        let line = [p(0, 0), p(30, 0), p(10, 0)];
        assert_eq!(mst(&line), [(0, 2), (2, 1)]);
        let net = NetGeometry::of_pins(7, &[pin(1, 0, 0, 1), pin(2, 30, 0, 1), pin(3, 10, 0, 1)]);
        assert_eq!(net.mst_length, 30);
        assert_eq!(net.segments, [(p(0, 0), p(10, 0)), (p(10, 0), p(30, 0))]);
        assert!(mst(&[p(5, 5)]).is_empty());
    }

    #[test]
    fn multi_pin_nets_conflict_through_any_segment() {
        // Net 1 is an L of three pins; net 2 crosses only its second leg.
        let n1 = NetGeometry::of_pins(
            1,
            &[pin(1, 0, 0, 1), pin(2, 100, 0, 1), pin(3, 100, 100, 1)],
        );
        let g = ConflictGraph::from_nets(vec![n1, two_pin(2, p(50, 50), p(150, 50), 1)]);
        assert!(g.conflict(1, 2));
        assert!(g.conflict(2, 1));
        assert!(!g.conflict(1, 3));
    }
}
