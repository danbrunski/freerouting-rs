// SPDX-License-Identifier: GPL-3.0-only AND MIT
// SPDX-FileCopyrightText: 2026 drandyhaas (KiCadRoutingTools)
// Portions ported from KiCadRoutingTools py_router/net_queries.py
// (_greedy_order_mps_units) at 0486e799.

//! The `conflict-rounds` pass-1 order (Q07.2): greedy rounds over the
//! crossing-conflict graph ([`super::conflicts`]), KRT's "MPS" order.
//!
//! Until no net remains, a round starts with every remaining net. Within
//! the round, the net with the fewest conflicts among the nets still in the
//! round is picked (ties: the shorter MST, then the lower net number); it is
//! emitted, and its conflicting neighbours still in the round leave it for
//! the next round. Each round's nets are then sorted by `(MST length, net
//! number)` and the rounds are concatenated.
//!
//! KRT's distance tie-break ("routing-aware" around BGA corners) is
//! grid-specific and dropped; its reverse-rounds option, its diff-pair units
//! and its layer swaps are not ported. KRT's float distances and Python sets
//! become integer MST lengths and ordered collections (X00 rule 6).

use std::collections::BTreeSet;

use super::conflicts::ConflictGraph;
use super::{PassOneOrdering, arrange_by_nets};
use crate::autoroute::stages::RouteTask;
use crate::board::basic_board::BasicBoard;

/// The ordering's id (its [`NetOrdering::id`](crate::autoroute::stages::NetOrdering::id)
/// and the portfolio arm's label; not a registered ordering, Q12).
pub const ORDERING_ID: &str = "conflict-rounds";

/// The greedy rounds of `graph`, as node lists, each sorted by
/// `(MST length, net number)` (module docs).
pub fn rounds(graph: &ConflictGraph) -> Vec<Vec<u32>> {
    let key = |node: u32| {
        let n = &graph.nets()[node as usize];
        (n.mst_length, n.net)
    };
    let mut remaining: BTreeSet<u32> = (0..graph.len() as u32).collect();
    let mut out = Vec::new();
    while !remaining.is_empty() {
        let mut in_round = vec![false; graph.len()];
        for &node in &remaining {
            in_round[node as usize] = true;
        }
        // conflicts[v]: v's neighbours still in the round.
        let mut conflicts: Vec<usize> = (0..graph.len() as u32)
            .map(|v| {
                graph
                    .neighbours(v)
                    .iter()
                    .filter(|&&u| in_round[u as usize])
                    .count()
            })
            .collect();
        let mut round_left = remaining.clone();
        let mut losers = BTreeSet::new();
        let mut winners = Vec::new();
        let leave = |node: u32, in_round: &mut [bool], conflicts: &mut [usize]| {
            in_round[node as usize] = false;
            for &u in graph.neighbours(node) {
                conflicts[u as usize] -= 1;
            }
        };
        while let Some(best) = round_left
            .iter()
            .copied()
            .min_by_key(|&v| (conflicts[v as usize], key(v)))
        {
            winners.push(best);
            round_left.remove(&best);
            leave(best, &mut in_round, &mut conflicts);
            for &loser in graph.neighbours(best) {
                if round_left.remove(&loser) {
                    losers.insert(loser);
                    leave(loser, &mut in_round, &mut conflicts);
                }
            }
        }
        winners.sort_by_key(|&v| key(v));
        out.push(winners);
        remaining = losers;
    }
    out
}

/// The nets of `graph` in conflict-round order.
pub fn net_order(graph: &ConflictGraph) -> Vec<i32> {
    rounds(graph)
        .into_iter()
        .flatten()
        .map(|node| graph.nets()[node as usize].net)
        .collect()
}

/// Java's pass-1 list `tasks` in conflict-round order: plane-net tasks
/// first in their order, then the signal tasks net by net in the rounds'
/// order, each net's tasks in their order.
pub fn conflict_rounds_order(board: &BasicBoard, tasks: Vec<RouteTask>) -> Vec<RouteTask> {
    let graph = ConflictGraph::build(board, &tasks);
    arrange_by_nets(tasks, &net_order(&graph))
}

/// The `conflict-rounds` ordering: [`conflict_rounds_order`] in pass 1,
/// Freerouting's ordering afterwards.
pub fn ordering() -> PassOneOrdering {
    PassOneOrdering::new(ORDERING_ID, conflict_rounds_order)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::items::ItemId;
    use crate::quality::ordering::PinPoint;
    use crate::quality::ordering::conflicts::NetGeometry;
    use freerouting_geometry::IntPoint;

    fn net(net: i32, a: (i32, i32), b: (i32, i32)) -> NetGeometry {
        let pin = |id: i32, (x, y): (i32, i32)| PinPoint {
            id: ItemId(id),
            centre: IntPoint { x, y },
            layers: 1,
            component: 0,
        };
        NetGeometry::of_pins(net, &[pin(2 * net, a), pin(2 * net + 1, b)])
    }

    /// Four two-pin nets on one layer:
    /// - net 1, (0,0)-(50,0)-ish long horizontal: (0, 50) to (100, 50), 100 long;
    /// - net 2, vertical (20, 0) to (20, 80), 80 long, crosses net 1;
    /// - net 3, vertical (60, 30) to (60, 70), 40 long, crosses net 1;
    /// - net 4, (70, 0) to (130, 0), 60 long, crosses nothing.
    ///
    /// Round 1: conflict counts 1→2, 2→1, 3→1, 4→0. Net 4 (0 conflicts)
    /// first; then 2 and 3 tie at one conflict and 3 is shorter (40 < 80):
    /// 3 is picked and its neighbour 1 leaves for round 2; then 2 (now 0
    /// conflicts). Round 1 = {4, 3, 2}, sorted by length: 3 (40), 4 (60),
    /// 2 (80). Round 2 = {1}. Order: 3, 4, 2, 1.
    #[test]
    fn four_nets_give_the_hand_derived_order() {
        let g = ConflictGraph::from_nets(vec![
            net(1, (0, 50), (100, 50)),
            net(2, (20, 0), (20, 80)),
            net(3, (60, 30), (60, 70)),
            net(4, (70, 0), (130, 0)),
        ]);
        assert!(g.conflict(1, 2) && g.conflict(1, 3));
        assert!(!g.conflict(2, 3) && !g.conflict(1, 4) && !g.conflict(3, 4));
        assert_eq!(rounds(&g), [vec![2, 3, 1], vec![0]]);
        assert_eq!(net_order(&g), [3, 4, 2, 1]);
    }

    #[test]
    fn ties_fall_to_the_shorter_mst_then_the_lower_net() {
        // Two crossing nets of equal length: the lower net number wins
        // round 1.
        let g =
            ConflictGraph::from_nets(vec![net(8, (0, 0), (100, 100)), net(5, (0, 100), (100, 0))]);
        assert_eq!(net_order(&g), [5, 8]);
        // No conflicts: one round sorted by (length, net).
        let g = ConflictGraph::from_nets(vec![
            net(1, (0, 0), (0, 30)),
            net(2, (10, 0), (10, 10)),
            net(3, (20, 0), (20, 10)),
        ]);
        assert_eq!(rounds(&g).len(), 1);
        assert_eq!(net_order(&g), [2, 3, 1]);
        assert!(net_order(&ConflictGraph::default()).is_empty());
    }

    #[test]
    fn a_net_crossing_all_others_goes_last_and_rounds_cover_every_net() {
        // A star: net 1 crosses nets 2..=6, which are parallel.
        let mut nets = vec![net(1, (0, 50), (1000, 50))];
        for k in 2..=6 {
            nets.push(net(k, (k * 100, 0), (k * 100, 100)));
        }
        let g = ConflictGraph::from_nets(nets);
        assert_eq!(g.edge_count(), 5);
        assert_eq!(net_order(&g), [2, 3, 4, 5, 6, 1]);
        let r = rounds(&g);
        let mut all: Vec<u32> = r.iter().flatten().copied().collect();
        all.sort_unstable();
        assert_eq!(all, (0..6).collect::<Vec<u32>>());
    }
}
