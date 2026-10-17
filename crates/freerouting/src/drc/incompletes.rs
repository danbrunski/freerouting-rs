//! Incomplete connections (the ratsnest).
//!
//! Port of `drc.NetIncompletes` and `drc.AirLine`.

use std::cmp::Ordering;
use std::collections::BTreeSet;

use crate::util::HashMap;

use freerouting_geometry::{FloatPoint, Point};

use crate::board::BasicBoard;
use crate::board::items::{Item, ItemId, ItemKind};
use crate::datastructures::PlanarDelaunayTriangulation;

/// An incomplete connection between two items of a net.
///
/// Port of `drc.AirLine`. A viewer draws a straight line from
/// [`from_corner`](Self::from_corner) to [`to_corner`](Self::to_corner)
/// (board coordinates). The layer ranges tell which layers the two
/// connected items are on; an airline itself has no layer.
#[derive(Clone, Debug, PartialEq)]
pub struct AirLine {
    /// The net of the airline.
    pub net_number: i32,
    /// The item where the airline starts.
    pub from_item: ItemId,
    /// The start of the airline.
    pub from_corner: FloatPoint,
    /// The first and last layer of `from_item`.
    pub from_layers: [i32; 2],
    /// The item where the airline ends.
    pub to_item: ItemId,
    /// The end of the airline.
    pub to_corner: FloatPoint,
    /// The first and last layer of `to_item`.
    pub to_layers: [i32; 2],
}

/// The incomplete connections and the length violation of one net.
///
/// Port of `drc.NetIncompletes`.
#[derive(Clone, Debug, PartialEq)]
pub struct NetIncompletes {
    /// The net number.
    pub net_number: i32,
    /// The airlines, shortest first.
    pub airlines: Vec<AirLine>,
    /// The number of connected groups of the net items at calculation time.
    pub connected_group_count: usize,
    /// The radius of the markers drawn at airline ends (Java
    /// `getMarkerRadius`): twice the minimum trace half width.
    pub marker_radius: f64,
    /// Positive: the trace length exceeds the maximum by this amount;
    /// negative: it is short of the minimum by this amount; 0: no violation.
    length_violation: f64,
}

/// A net item with its group (Java `NetItem` with its connected set).
#[derive(Debug)]
struct NetItem {
    id: ItemId,
    group: usize,
}

/// An edge of the triangulation (Java `NetIncompletes.Edge`).
struct Edge {
    from: usize,
    from_corner: FloatPoint,
    to: usize,
    to_corner: FloatPoint,
    length_square: f64,
}

impl Edge {
    /// Java `Edge.compareTo`: by length, then by the corner coordinates.
    fn compare(&self, other: &Edge) -> Ordering {
        let keys = [
            self.length_square - other.length_square,
            self.from_corner.x - other.from_corner.x,
            self.from_corner.y - other.from_corner.y,
            self.to_corner.x - other.to_corner.x,
            self.to_corner.y - other.to_corner.y,
        ];
        for key in keys {
            if key > 0.0 {
                return Ordering::Greater;
            }
            if key < 0.0 {
                return Ordering::Less;
            }
        }
        Ordering::Equal
    }
}

impl NetIncompletes {
    /// Calculates the incomplete connections of net `net_number` from its
    /// connectable items `net_items`.
    pub fn new(board: &BasicBoard, net_number: i32, net_items: &[ItemId]) -> Self {
        Self::new_cached(board, net_number, net_items, &mut HashMap::default())
    }

    /// [`new`](Self::new) with the normal contacts memoized in `contacts`
    /// (the board must not change while the cache is used).
    pub(crate) fn new_cached(
        board: &BasicBoard,
        net_number: i32,
        net_items: &[ItemId],
        contacts: &mut HashMap<ItemId, Vec<ItemId>>,
    ) -> Self {
        let mut result = NetIncompletes {
            net_number,
            airlines: Vec::new(),
            connected_group_count: 0,
            marker_radius: f64::from(board.rules.min_trace_half_width()) * 2.0,
            length_violation: 0.0,
        };
        // Dangling traces and vias, and unconnected items other than drill
        // items and conduction areas, are violations, not incompletes.
        let filtered: Vec<ItemId> = net_items
            .iter()
            .copied()
            .filter(|id| {
                let Some(item) = board.item(*id) else {
                    return false;
                };
                if board.is_tail(*id) {
                    return false;
                }
                item.is_conduction_area()
                    || item.is_drill_item()
                    || !board.normal_contacts(*id).is_empty()
            })
            .collect();

        let mut net_item_list = calculate_net_items(board, net_number, &filtered, contacts);
        let group_count = net_item_list
            .iter()
            .map(|n| n.group)
            .collect::<BTreeSet<_>>()
            .len();
        result.connected_group_count = group_count;
        if group_count <= 1 {
            result.calc_length_violation(board);
            return result;
        }

        let objects: Vec<Vec<Point>> = net_item_list
            .iter()
            .map(|n| ratsnest_corners(board, n.id))
            .collect();
        let triangulation = PlanarDelaunayTriangulation::new(&objects);
        let mut edges: Vec<Edge> = triangulation
            .edge_lines()
            .into_iter()
            .map(|line| {
                let from_corner = line.start_point.to_float();
                let to_corner = line.end_point.to_float();
                Edge {
                    from: line.start_object,
                    from_corner,
                    to: line.end_object,
                    to_corner,
                    length_square: to_corner.distance_square(from_corner),
                }
            })
            .collect();
        // Java collects the edges in a TreeSet: sorted, and of equal edges
        // only the first one is kept.
        edges.sort_by(Edge::compare);
        edges.dedup_by(|b, a| a.compare(b) == Ordering::Equal);

        for edge in edges {
            let from_group = net_item_list[edge.from].group;
            let to_group = net_item_list[edge.to].group;
            if from_group == to_group {
                continue;
            }
            let from_item = net_item_list[edge.from].id;
            let to_item = net_item_list[edge.to].id;
            result.airlines.push(AirLine {
                net_number,
                from_item,
                from_corner: edge.from_corner,
                from_layers: layer_range(board, from_item),
                to_item,
                to_corner: edge.to_corner,
                to_layers: layer_range(board, to_item),
            });
            for n in &mut net_item_list {
                if n.group == from_group {
                    n.group = to_group;
                }
            }
        }
        result.calc_length_violation(board);
        result
    }

    /// The number of airlines.
    pub fn count(&self) -> usize {
        self.airlines.len()
    }

    /// The length violation of the net: positive if the traces are too
    /// long, negative if too short, 0 if valid.
    pub fn length_violation(&self) -> f64 {
        self.length_violation
    }

    /// Java `calcLengthViolation`: recalculates the length violation and
    /// returns true if it changed by more than 0.1.
    pub fn calc_length_violation(&mut self, board: &BasicBoard) -> bool {
        let Some(net) = board.rules.nets.get(self.net_number) else {
            return false;
        };
        let classes = &board.rules.net_classes;
        let max_length = net.maximum_trace_length(classes);
        let min_length = net.minimum_trace_length(classes);
        if max_length <= 0.0 && min_length <= 0.0 {
            self.length_violation = 0.0;
            return false;
        }
        let mut new_violation = 0.0;
        let trace_length = board.net_trace_length(self.net_number);
        if max_length > 0.0 && trace_length > max_length {
            new_violation = trace_length - max_length;
        }
        if min_length > 0.0
            && trace_length < min_length
            && self.airlines.is_empty()
            && !(trace_length <= 0.0
                && self.connected_group_count <= 1
                && board.net_terminal_items(self.net_number).len() < 2)
        {
            new_violation = trace_length - min_length;
        }
        let old_violation = self.length_violation;
        self.length_violation = new_violation;
        (new_violation - old_violation).abs() > 0.1
    }
}

/// The first and last layer of item `id`.
fn layer_range(board: &BasicBoard, id: ItemId) -> [i32; 2] {
    board
        .item(id)
        .map_or([0, 0], |i| [i.first_layer(), i.last_layer()])
}

/// Java `NetIncompletes.calculateNetItems`: groups the items by connected
/// set.
///
/// The first remaining item in `items` order starts the next group, as in
/// Java's `LinkedHashSet` since `30baad7e2` (at `cda540f` Java took it from
/// a `HashSet` in identity-hash order). Within a group the items follow the
/// connected set, a `TreeSet` in Java and a `BTreeSet` here, both ordered
/// by descending id. The order matters: it is the corner order of the
/// Delaunay triangulation, which decides ties between airlines.
fn calculate_net_items(
    board: &BasicBoard,
    net_number: i32,
    items: &[ItemId],
    contacts: &mut HashMap<ItemId, Vec<ItemId>>,
) -> Vec<NetItem> {
    let mut result = Vec::new();
    let mut seen = crate::util::HashSet::default();
    let mut remaining: Vec<ItemId> = items
        .iter()
        .copied()
        .filter(|id| seen.insert(*id))
        .collect();
    let mut group = 0;
    while let Some(&start) = remaining.first() {
        let connected = board.connected_set_cached(start, net_number, false, contacts);
        let still_remaining: crate::util::HashSet<ItemId> = remaining.iter().copied().collect();
        let mut in_component: Vec<ItemId> = connected
            .iter()
            .copied()
            .filter(|id| still_remaining.contains(id))
            .collect();
        remaining.retain(|id| !connected.contains(id));
        if in_component.is_empty() {
            // The start item is not in its own connected set (wrong net);
            // Java would loop forever, here it forms its own group.
            remaining.retain(|id| *id != start);
            in_component.push(start);
        }
        for id in in_component {
            result.push(NetItem { id, group });
        }
        group += 1;
    }
    result
}

/// Java `Item.getRatsnestCorners`.
pub(crate) fn ratsnest_corners(board: &BasicBoard, id: ItemId) -> Vec<Point> {
    let Some(item) = board.item(id) else {
        return Vec::new();
    };
    match &item.kind {
        ItemKind::Pin(_) | ItemKind::Via(_) => {
            vec![Point::Int(item.drill().expect("drill item").center)]
        }
        ItemKind::Trace(trace) => {
            let mut result = Vec::new();
            if board.trace_start_contacts(id).is_empty() {
                result.push(trace.first_corner());
            }
            if board.trace_end_contacts(id).is_empty() {
                result.push(trace.last_corner());
            }
            result
        }
        ItemKind::Area(area) if area.is_conduction_area() => area
            .area()
            .corner_approx_arr()
            .into_iter()
            .map(|c| Point::Int(c.round()))
            .collect(),
        _ => Vec::new(),
    }
}

/// The connectable items of each net, in item order (index 0 is net 1).
pub(crate) fn net_item_lists(board: &BasicBoard) -> Vec<Vec<ItemId>> {
    let max_net = board.rules.nets.max_net_number().max(0) as usize;
    let mut lists = vec![Vec::new(); max_net];
    for item in board.items() {
        if !item.is_connectable_kind() {
            continue;
        }
        for net in item.net_numbers() {
            if *net >= 1 && (*net as usize) <= max_net {
                let list: &mut Vec<ItemId> = &mut lists[*net as usize - 1];
                if !list.contains(&item.id()) {
                    list.push(item.id());
                }
            }
        }
    }
    lists
}

/// Java `calculateAllIncompletes().maxConnections`: per net the number of
/// pins and conduction areas minus one.
pub(crate) fn max_connections(board: &BasicBoard, lists: &[Vec<ItemId>]) -> usize {
    lists
        .iter()
        .map(|list| {
            let endpoints = list
                .iter()
                .filter_map(|id| board.item(*id))
                .filter(|i: &&Item| i.is_pin() || i.is_conduction_area())
                .count();
            endpoints.saturating_sub(1)
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::{BoardReadResult, read_board};

    /// Java's `calculateNetItems` at `a773a70ec`: groups start in input
    /// order (`LinkedHashSet`), and the items of a group follow the
    /// connected set's `TreeSet` order, highest id first.
    #[test]
    fn net_items_group_in_input_order_and_list_by_descending_id() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/Issue575-drc_Natural_Tone_Preamp_7_unconnected_items.dsn"
        );
        let board = match read_board(&std::fs::read(path).unwrap()) {
            BoardReadResult::Success { board, .. } => *board,
            _ => panic!("fixture must load"),
        };
        let mut checked_groups = 0;
        for net_number in 1..=board.rules.nets.max_net_number() {
            // Ascending ids: the reverse of the connected-set order.
            let mut items = board.connectable_items(net_number);
            items.sort_by_key(|id| id.0);
            let mut contacts = HashMap::default();
            let net_items = calculate_net_items(&board, net_number, &items, &mut contacts);
            assert_eq!(net_items.len(), items.len());
            let mut first_of_group: Vec<ItemId> = Vec::new();
            for pair in net_items.windows(2) {
                if pair[0].group == pair[1].group {
                    assert!(pair[0].id.0 > pair[1].id.0, "net {net_number}: {pair:?}");
                    checked_groups += 1;
                }
            }
            for (i, n) in net_items.iter().enumerate() {
                if i == 0 || net_items[i - 1].group != n.group {
                    first_of_group.push(n.id);
                }
            }
            // Each group holds the first item, in input order, not taken by
            // an earlier group.
            let mut taken: crate::util::HashSet<ItemId> = crate::util::HashSet::default();
            let mut group_no = 0;
            for id in &items {
                if taken.contains(id) {
                    continue;
                }
                let group = net_items.iter().find(|n| n.id == *id).unwrap().group;
                assert_eq!(group, group_no, "net {net_number}: group of {id:?}");
                taken.extend(net_items.iter().filter(|n| n.group == group).map(|n| n.id));
                group_no += 1;
            }
            assert_eq!(first_of_group.len(), group_no);
        }
        assert!(
            checked_groups > 0,
            "the fixture must have multi-item groups"
        );
    }
}
