//! `FreeroutingOrdering` (A10.4): which items a routing pass routes and in
//! what order, at `a773a70ec`.
//!
//! - [`select_items`] is `BatchAutorouter.getAutorouteItems`
//!   (`BatchAutorouter.java:353-439`): the item list is walked highest id
//!   first; every connectable item that is not routable (pins, conduction
//!   areas, fixed traces and vias) and not yet handled is checked net by
//!   net. Members of its connected set with at most one net become handled.
//!   The item needs routing on a net when its connected set is smaller than
//!   the net's connectable item count and it has no ignored net; on a plane
//!   net a connected set that holds a conduction area does not count. Items
//!   with a plane net that needs routing go to the plane list, the others
//!   to the signal list, and the plane list comes first.
//! - [`FreeroutingOrdering::order`] for passes after the first is
//!   `AutoroutePassRunner.reorderSingleThreadItems` (`:257-317`). It splits
//!   by [`is_plane_item`] (any net with a plane, `BatchAutorouter.java:442-453`),
//!   which is not `select`'s test. Plane items keep their order. Signal
//!   items are sorted by `(first net or 0, id ascending)`; after two
//!   stagnating passes they are then shuffled with
//!   `new Random(pass * 1000003 + maxNetNumber)`, otherwise items on the
//!   nets the last pass left incomplete come first, each group sorted.
//! - The anti-oscillation state (`:33-35`, `:233-239`, `:319-323`) is
//!   updated by [`NetOrdering::on_pass_end`] and cleared by
//!   [`NetOrdering::reset`] after a restore.

use std::collections::BTreeSet;

use freerouting_geometry::jmath::JavaRandom;

use crate::autoroute::stages::{NetOrdering, PassReport, RouteTask};
use crate::board::basic_board::BasicBoard;
use crate::board::items::{Item, ItemId};
use crate::board::routing_board::RoutingBoard;

/// The multiplier of the pass number in the shuffle seed
/// (`AutoroutePassRunner.java:278`).
pub const SHUFFLE_PASS_FACTOR: i64 = 1_000_003;

/// The stagnation count from which the signal items are shuffled.
pub const SHUFFLE_STAGNATION: u32 = 2;

/// True if any net of `item` contains a plane (Java
/// `BatchAutorouter.isPlaneItem`).
pub fn is_plane_item(board: &BasicBoard, item: &Item) -> bool {
    item.net_numbers().iter().any(|net| {
        board
            .rules
            .nets
            .get(*net)
            .is_some_and(|n| n.contains_plane())
    })
}

fn first_net(item: &Item) -> i32 {
    item.net_numbers().first().copied().unwrap_or(0)
}

/// Java `getAutorouteItems`: the items to route, plane items first.
pub fn select_items(board: &BasicBoard) -> Vec<RouteTask> {
    let mut handled: BTreeSet<ItemId> = BTreeSet::new();
    let mut plane = Vec::new();
    let mut signal = Vec::new();
    // PARITY: BatchAutorouter.java:361-368, the item list, highest id first.
    for item in board.items() {
        if !item.is_connectable_kind() || item.is_routable() || handled.contains(&item.id()) {
            continue;
        }
        let mut needs_routing = false;
        let mut has_plane_net = false;
        for &net in item.net_numbers() {
            let connected = board.connected_set(item.id(), net, false);
            // PARITY: :381-386, the handled set grows before the test.
            for id in &connected {
                if board.item(*id).is_some_and(|c| c.net_count() <= 1) {
                    handled.insert(*id);
                }
            }
            let net_item_count = board.connectable_item_count(net);
            if connected.len() < net_item_count && !board.has_ignored_nets(item) {
                let is_plane = board
                    .rules
                    .nets
                    .get(net)
                    .is_some_and(|n| n.contains_plane());
                if is_plane {
                    // PARITY: :393-400, a plane net already on its plane.
                    let on_plane = connected
                        .iter()
                        .any(|id| board.item(*id).is_some_and(Item::is_conduction_area));
                    if on_plane {
                        continue;
                    }
                    has_plane_net = true;
                }
                needs_routing = true;
            }
        }
        if needs_routing {
            let task = RouteTask {
                item: item.id(),
                net: first_net(item),
                is_plane: has_plane_net,
            };
            if has_plane_net {
                plane.push(task);
            } else {
                signal.push(task);
            }
        }
    }
    // PARITY: :436-437, plane items first.
    plane.extend(signal);
    plane
}

/// Java Freerouting's net ordering (`freerouting-ordering`) with its
/// anti-oscillation state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FreeroutingOrdering {
    /// `previousIncompleteNets`.
    previous_incomplete_nets: BTreeSet<i32>,
    /// `previousIncompleteCount`, `None` for Java's -1.
    previous_incomplete: Option<usize>,
    /// `stagnationCount`.
    stagnation_count: u32,
}

impl FreeroutingOrdering {
    /// An ordering with no history.
    pub fn new() -> Self {
        FreeroutingOrdering::default()
    }

    /// The number of consecutive passes that did not lower the incomplete
    /// count.
    pub fn stagnation_count(&self) -> u32 {
        self.stagnation_count
    }

    /// Sets the stagnation count (the `items` probe forces 2, as a test
    /// would through reflection).
    pub fn set_stagnation_count(&mut self, count: u32) {
        self.stagnation_count = count;
    }

    /// The nets the last pass left incomplete.
    pub fn previous_incomplete_nets(&self) -> &BTreeSet<i32> {
        &self.previous_incomplete_nets
    }

    /// The order of `tasks` for pass `pass` (Java
    /// `reorderSingleThreadItems`); pass 1 keeps the selection order.
    pub fn reorder(&self, tasks: Vec<RouteTask>, board: &BasicBoard, pass: u32) -> Vec<RouteTask> {
        if pass <= 1 {
            return tasks;
        }
        let item_of = |task: &RouteTask| board.item(task.item);
        let (plane, mut signal): (Vec<RouteTask>, Vec<RouteTask>) = tasks
            .iter()
            .partition(|task| item_of(task).is_some_and(|item| is_plane_item(board, item)));
        if signal.is_empty() {
            return tasks;
        }
        // PARITY: AutoroutePassRunner.java:272-274, (first net or 0, id).
        let key = |task: &RouteTask| (item_of(task).map_or(0, first_net), task.item.0);
        if self.stagnation_count >= SHUFFLE_STAGNATION {
            signal.sort_by_key(key);
            // PARITY: :277-278, the seed in long arithmetic.
            let seed = i64::from(pass) * SHUFFLE_PASS_FACTOR
                + i64::from(board.rules.nets.max_net_number());
            JavaRandom::new(seed).shuffle(&mut signal);
        } else if !self.previous_incomplete_nets.is_empty() {
            let persistent = |task: &RouteTask| {
                item_of(task).is_some_and(|item| {
                    item.net_numbers()
                        .iter()
                        .any(|net| self.previous_incomplete_nets.contains(net))
                })
            };
            let (mut first, mut rest): (Vec<RouteTask>, Vec<RouteTask>) =
                signal.into_iter().partition(persistent);
            first.sort_by_key(key);
            rest.sort_by_key(key);
            first.extend(rest);
            signal = first;
        } else {
            signal.sort_by_key(key);
        }
        let mut out = plane;
        out.extend(signal);
        out
    }
}

impl NetOrdering for FreeroutingOrdering {
    fn id(&self) -> &'static str {
        "freerouting-ordering"
    }

    fn select(&mut self, board: &RoutingBoard) -> Vec<RouteTask> {
        select_items(board)
    }

    fn order(&mut self, tasks: Vec<RouteTask>, board: &RoutingBoard, pass: u32) -> Vec<RouteTask> {
        self.reorder(tasks, board, pass)
    }

    /// Java's update at the end of `runSingleThread` (`:233-239`): the driver
    /// calls it after every pass that had items, with the incomplete count
    /// and nets after the tail removal and before the restore check.
    fn on_pass_end(&mut self, report: &PassReport) {
        self.stagnation_count = match self.previous_incomplete {
            Some(previous) if report.incomplete >= previous => self.stagnation_count + 1,
            _ => 0,
        };
        self.previous_incomplete = Some(report.incomplete);
        self.previous_incomplete_nets = report.incomplete_nets.clone();
    }

    /// Java `resetAntiOscillationState` (`:319-323`).
    fn reset(&mut self) {
        *self = FreeroutingOrdering::new();
    }
}
