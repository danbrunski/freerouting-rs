//! First-pass net orderings (Q07,
//! `docs/roadmap/tasks/Q07-first-pass-ordering.md`).
//!
//! Java's pass 1 routes in item order (`BatchAutorouter.getAutorouteItems`,
//! ported as [`select_items`]); later passes sort and shuffle with the D2
//! seed ([`FreeroutingOrdering`]). The orderings here replace **pass 1's
//! order only**, as experiments against Java's:
//! - `conflict-rounds` ([`conflict_rounds`]): greedy rounds over a
//!   crossing-conflict graph of the nets' pin MSTs ([`conflicts`]), after
//!   KiCadRoutingTools' "MPS" order;
//! - `inside-out` ([`inside_out`]): nets with a pin inside a dense part's pin
//!   field first, nearest the part's centre first, after KiCadRoutingTools'
//!   BGA inside-out order.
//!
//! Both are selected by `router.autorouter.ordering`; the default stays
//! `freerouting-ordering`, so a job that does not name them routes exactly as
//! before.
//!
//! Each order is a plain function from a board and Java's pass-1 task list to
//! a permutation of that list ([`PassOneOrder`]), so other stages (Q12's
//! pass-1 portfolio) can call [`conflict_rounds::conflict_rounds_order`] and
//! [`inside_out::inside_out_order`] directly. [`PassOneOrdering`] wraps such
//! a function as a [`NetOrdering`]:
//! - `select` returns Java's list in pass 1 reordered, and Java's list
//!   unchanged afterwards;
//! - `order` (passes after the first), `on_pass_end` and `reset` delegate to
//!   [`FreeroutingOrdering`]. Its pass-2+ order sorts the signal items by
//!   `(net, id)` whatever order `select` gave, and keeps the plane items in
//!   `select`'s order, which pass 1's reordering never touches.
//! - Plane-net tasks stay first, in Java's order (A10's invariant). Signal
//!   tasks are moved as whole nets: the tasks of one net keep Java's
//!   relative order.
//!
//! A pass that runs through Java's `autoroutePassesForOptimizingItem` (the
//! optimizer's re-route, [`route_passes_for_optimizer`]) builds a fresh
//! ordering, so its first pass is reordered too.
//!
//! [`route_passes_for_optimizer`]: crate::autoroute::batch::pass::route_passes_for_optimizer

pub mod conflict_rounds;
pub mod conflicts;
pub mod inside_out;

use std::collections::{BTreeMap, BTreeSet};

use freerouting_geometry::IntPoint;

use crate::autoroute::batch::ordering::{FreeroutingOrdering, select_items};
use crate::autoroute::stages::{NetOrdering, PassReport, RouteTask};
use crate::board::basic_board::BasicBoard;
use crate::board::items::ItemId;
use crate::board::routing_board::RoutingBoard;

/// A pass-1 order: a permutation of Java's pass-1 task list.
pub type PassOneOrder = fn(&BasicBoard, Vec<RouteTask>) -> Vec<RouteTask>;

/// A [`NetOrdering`] that reorders pass 1 with `order` and is Freerouting's
/// ordering otherwise (module docs).
#[derive(Clone, Debug)]
pub struct PassOneOrdering {
    id: &'static str,
    order: PassOneOrder,
    inner: FreeroutingOrdering,
    /// True once a pass with items has ended. A restore does not clear it:
    /// it brings back a board, not pass 1.
    first_pass_done: bool,
}

impl PassOneOrdering {
    /// The ordering `id` that applies `order` to pass 1.
    pub fn new(id: &'static str, order: PassOneOrder) -> Self {
        PassOneOrdering {
            id,
            order,
            inner: FreeroutingOrdering::new(),
            first_pass_done: false,
        }
    }
}

impl NetOrdering for PassOneOrdering {
    fn id(&self) -> &'static str {
        self.id
    }

    fn select(&mut self, board: &RoutingBoard) -> Vec<RouteTask> {
        let tasks = select_items(board);
        if self.first_pass_done {
            tasks
        } else {
            (self.order)(board, tasks)
        }
    }

    fn order(&mut self, tasks: Vec<RouteTask>, board: &RoutingBoard, pass: u32) -> Vec<RouteTask> {
        self.inner.reorder(tasks, board, pass)
    }

    fn on_pass_end(&mut self, report: &PassReport) {
        self.first_pass_done = true;
        self.inner.on_pass_end(report);
    }

    fn reset(&mut self) {
        self.inner.reset();
    }
}

/// One pin of a net as the orderings see it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PinPoint {
    /// The pin item.
    pub id: ItemId,
    /// The pin's centre (its drill centre).
    pub centre: IntPoint,
    /// The layers the pin is on, one bit per layer (layers from 128 on
    /// share bit 127).
    pub layers: u128,
    /// The pin's component (0 for none).
    pub component: i32,
}

/// The layer mask of layers `first..=last`.
pub fn layer_mask(first: i32, last: i32) -> u128 {
    let lo = first.clamp(0, 127) as u32;
    let hi = last.clamp(0, 127) as u32;
    if hi < lo {
        return 0;
    }
    let width = hi - lo + 1;
    let ones = if width >= 128 {
        u128::MAX
    } else {
        (1u128 << width) - 1
    };
    ones << lo
}

/// Every pin of the board, by net, each list in ascending item id. A pin
/// with several nets is in each net's list. Only the nets in `nets` are
/// collected when it is `Some`.
pub fn pins_by_net(
    board: &BasicBoard,
    nets: Option<&BTreeSet<i32>>,
) -> BTreeMap<i32, Vec<PinPoint>> {
    let mut by_net: BTreeMap<i32, Vec<PinPoint>> = BTreeMap::new();
    for item in board.items() {
        let Some(drill) = item.as_pin().map(|pin| &pin.drill) else {
            continue;
        };
        let point = PinPoint {
            id: item.id(),
            centre: drill.center,
            layers: layer_mask(drill.first_layer, drill.last_layer),
            component: item.component_id(),
        };
        for &net in item.net_numbers() {
            if nets.is_none_or(|wanted| wanted.contains(&net)) {
                by_net.entry(net).or_default().push(point);
            }
        }
    }
    for pins in by_net.values_mut() {
        pins.sort_by_key(|p| p.id);
    }
    by_net
}

/// The nets of the signal (non-plane) tasks.
pub fn signal_nets(tasks: &[RouteTask]) -> BTreeSet<i32> {
    tasks
        .iter()
        .filter(|t| !t.is_plane)
        .map(|t| t.net)
        .collect()
}

/// Java's list with the signal tasks moved net by net into `net_order`:
/// plane tasks first in their order, then the signal tasks of each net of
/// `net_order` in turn (each net's tasks in their order), then the signal
/// tasks of the nets not in `net_order`, in their order.
pub fn arrange_by_nets(tasks: Vec<RouteTask>, net_order: &[i32]) -> Vec<RouteTask> {
    let rank: BTreeMap<i32, usize> = net_order
        .iter()
        .enumerate()
        .map(|(i, net)| (*net, i))
        .collect();
    let (plane, signal): (Vec<RouteTask>, Vec<RouteTask>) =
        tasks.into_iter().partition(|t| t.is_plane);
    let (mut ranked, rest): (Vec<RouteTask>, Vec<RouteTask>) =
        signal.into_iter().partition(|t| rank.contains_key(&t.net));
    // A stable sort: the tasks of one net keep their order.
    ranked.sort_by_key(|t| rank[&t.net]);
    let mut out = plane;
    out.extend(ranked);
    out.extend(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(item: i32, net: i32, is_plane: bool) -> RouteTask {
        RouteTask {
            item: ItemId(item),
            net,
            is_plane,
        }
    }

    #[test]
    fn layer_masks_cover_the_span() {
        assert_eq!(layer_mask(0, 0), 1);
        assert_eq!(layer_mask(0, 3), 0b1111);
        assert_eq!(layer_mask(2, 3), 0b1100);
        assert_eq!(layer_mask(0, 200), u128::MAX);
        assert_eq!(layer_mask(3, 1), 0);
    }

    #[test]
    fn arranging_keeps_plane_tasks_first_and_nets_whole() {
        let tasks = vec![
            task(9, 5, true),
            task(8, 1, false),
            task(7, 2, false),
            task(6, 1, false),
            task(5, 3, false),
            task(4, 6, true),
            task(3, 2, false),
        ];
        let out = arrange_by_nets(tasks, &[2, 1]);
        let items: Vec<i32> = out.iter().map(|t| t.item.0).collect();
        assert_eq!(items, [9, 4, 7, 3, 8, 6, 5]);
    }
}
