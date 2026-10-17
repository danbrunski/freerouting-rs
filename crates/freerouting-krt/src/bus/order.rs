// SPDX-License-Identifier: GPL-3.0-only AND MIT
// SPDX-FileCopyrightText: 2026 drandyhaas (KiCadRoutingTools)
// Portions ported from KiCadRoutingTools py_router/bus_detection.py
// (get_bus_routing_order) at 0486e799.

//! The `krt-bus` net ordering (X11.2): bus members middle out.
//!
//! KRT's `get_bus_routing_order` routes a bus from its middle member and
//! alternates outward, left side first: physical order `A B C D E` routes
//! `C, B, D, A, E` (the middle is index `n / 2`, so 8 members `0..8` route
//! `4, 3, 5, 2, 6, 1, 7, 0`). Each member then has a routed neighbour on
//! its inner side for [`super::attraction`] to follow.
//!
//! [`KrtBusOrdering`] is Freerouting's ordering ([`FreeroutingOrdering`],
//! A10.4) with the pass-1 list regrouped:
//! - plane-net tasks stay first, in Freerouting's order;
//! - then each bus group ([`super::detect`]) as one block, the groups by
//!   their smallest net number, the members middle out, a member's tasks
//!   (one per pin to connect) in Freerouting's order;
//! - then every other task in Freerouting's order.
//!
//! `select` regroups on every call, but the pass loop applies
//! [`NetOrdering::order`] to every pass after the first, and Freerouting's
//! reorder there sorts the signal tasks by `(net, item id)` (or shuffles a
//! sorted list), which discards the regrouping: the bus blocks shape pass 1
//! only. The anti-oscillation state is Freerouting's, cleared by `reset`.
//! Bus detection reads pins only, so it is the same on every call and after
//! every restore.

use freerouting::autoroute::batch::ordering::{FreeroutingOrdering, select_items};
use freerouting::autoroute::stages::{NetOrdering, PassReport, RouteTask};
use freerouting::board::RoutingBoard;

use super::detect::{BusGroup, BusParams, detect};

/// The ordering id (`router.autorouter.ordering`).
pub const BUS_ORDERING_ID: &str = "krt-bus";

/// The physical indices of `n` members in routing order: the middle
/// (`n / 2`) first, then alternating outward, the lower side first (KRT
/// `get_bus_routing_order`).
pub fn middle_out(n: usize) -> Vec<usize> {
    if n == 0 {
        return Vec::new();
    }
    let middle = n / 2;
    let mut order = vec![middle];
    for i in 1..n {
        if let Some(left) = middle.checked_sub(i) {
            order.push(left);
        }
        if middle + i < n {
            order.push(middle + i);
        }
    }
    order
}

/// The nets of `group` in routing order.
pub fn routing_order(group: &BusGroup) -> Vec<i32> {
    middle_out(group.members.len())
        .into_iter()
        .map(|i| group.members[i].net)
        .collect()
}

/// `tasks` (Freerouting's pass-1 list) regrouped by `groups` (module docs).
pub fn regroup(tasks: Vec<RouteTask>, groups: &[BusGroup]) -> Vec<RouteTask> {
    let mut by_first: Vec<&BusGroup> = groups.iter().collect();
    by_first.sort_by_key(|g| g.first_net());
    let order: Vec<i32> = by_first.iter().flat_map(|g| routing_order(g)).collect();
    let rank = |net: i32| order.iter().position(|n| *n == net);
    let (plane, signal): (Vec<RouteTask>, Vec<RouteTask>) =
        tasks.into_iter().partition(|t| t.is_plane);
    let (bus, other): (Vec<RouteTask>, Vec<RouteTask>) =
        signal.into_iter().partition(|t| rank(t.net).is_some());
    let mut bus: Vec<(usize, usize, RouteTask)> = bus
        .into_iter()
        .enumerate()
        .map(|(i, t)| (rank(t.net).unwrap_or(usize::MAX), i, t))
        .collect();
    bus.sort_by_key(|(rank, index, _)| (*rank, *index));
    let mut out = plane;
    out.extend(bus.into_iter().map(|(_, _, t)| t));
    out.extend(other);
    out
}

/// The `krt-bus` ordering (module docs).
#[derive(Clone, Debug)]
pub struct KrtBusOrdering {
    inner: FreeroutingOrdering,
    /// The detection parameters; `None` for [`BusParams::for_board`].
    pub params: Option<BusParams>,
}

impl Default for KrtBusOrdering {
    fn default() -> Self {
        KrtBusOrdering {
            inner: FreeroutingOrdering::new(),
            params: None,
        }
    }
}

impl KrtBusOrdering {
    /// The ordering with the X11 detection defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// The bus groups of `board` with this ordering's parameters.
    pub fn groups(&self, board: &RoutingBoard) -> Vec<BusGroup> {
        let params = self.params.unwrap_or_else(|| BusParams::for_board(board));
        detect(board, &params)
    }
}

impl NetOrdering for KrtBusOrdering {
    fn id(&self) -> &'static str {
        BUS_ORDERING_ID
    }

    fn select(&mut self, board: &RoutingBoard) -> Vec<RouteTask> {
        regroup(select_items(board), &self.groups(board))
    }

    fn order(&mut self, tasks: Vec<RouteTask>, board: &RoutingBoard, pass: u32) -> Vec<RouteTask> {
        self.inner.order(tasks, board, pass)
    }

    fn on_pass_end(&mut self, report: &PassReport) {
        self.inner.on_pass_end(report);
    }

    fn reset(&mut self) {
        self.inner.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use freerouting::board::items::ItemId;

    #[test]
    fn middle_out_matches_krts_examples() {
        assert_eq!(middle_out(5), [2, 1, 3, 0, 4]);
        assert_eq!(middle_out(8), [4, 3, 5, 2, 6, 1, 7, 0]);
        assert_eq!(middle_out(1), [0]);
        assert!(middle_out(0).is_empty());
    }

    #[test]
    fn regroup_keeps_planes_first_and_others_in_order() {
        use super::super::detect::Connection;
        use freerouting::geometry::IntPoint;
        let task = |item, net, is_plane| RouteTask {
            item: ItemId(item),
            net,
            is_plane,
        };
        let member = |net| Connection {
            net,
            start_pin: ItemId(0),
            start: IntPoint::new(0, 0),
            end_pin: ItemId(0),
            end: IntPoint::new(0, 0),
        };
        let group = BusGroup {
            members: vec![member(5), member(3), member(7)],
        };
        let tasks = vec![
            task(1, 9, true),
            task(2, 7, false),
            task(3, 1, false),
            task(4, 3, false),
            task(5, 5, false),
            task(6, 2, false),
            task(7, 3, false),
        ];
        let out: Vec<i32> = regroup(tasks, &[group]).iter().map(|t| t.item.0).collect();
        // Plane 1; the bus middle out: net 3 (items 4, 7), 5 (5), 7 (2); others 3, 6.
        assert_eq!(out, [1, 4, 7, 5, 2, 3, 6]);
    }
}
