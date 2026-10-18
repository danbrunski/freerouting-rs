//! The routed connections of a board, from its copper (Q15.1).
//!
//! A *connection* is a pair of pins of one net joined by routed copper.
//! The copper of a net is a graph: a trace has two end nodes joined by an edge
//! as long as the trace; a via or pin is one node; two nodes in normal contact
//! (a trace end on a pin or via centre, two trace ends at one corner, a via on
//! a pin) are joined by an edge of length 0. Conduction areas (planes, pours)
//! carry no path, so a pin connected only through a plane has no connection
//! here. Copper that leads to no second pin (stubs) is not on any connection.
//!
//! **Which pairs.** For the pins of one connected piece of copper the
//! connections are the edges of a minimum spanning tree of the pins, where the
//! distance between two pins is the length of the shortest copper path
//! between them. Two pins give one connection; three pins on a Y give the two
//! shorter legs, not the long one through both. The tree is built the way
//! Mehlhorn does (one multi-source search, then Kruskal over the edges that
//! join two pins' territories), and ties break by node and edge order, so the
//! result does not depend on hash order. A pair whose copper is shorter than
//! 1 board unit, or whose pins are at one point (a via in a pad joining two
//! stacked pads), is not a connection and is counted in `skipped`.
//!
//! **What is measured** is the copper path of the pair: the traces in order,
//! split into runs at every via and pin (a layer change, or a pad passed
//! through), measured by [`measure_run`]. `vias` counts the vias on the path,
//! plus the through-hole pins the path passes through where it changes layer.
//! `detour` is the path length over the distance between the two pin centres.

use std::cmp::{Ordering, Reverse};
use std::collections::{BTreeMap, BinaryHeap};

use freerouting::board::BasicBoard;
use freerouting::board::items::{Item, ItemId, ItemKind};
use freerouting::board::structure::Unit;

use crate::tidy_geometry::{RunStats, Seg, measure_run};

/// One connection and its five measures.
#[derive(Clone, Debug, PartialEq)]
pub struct Connection {
    pub net: String,
    /// The pins as `component-pin`.
    pub from: String,
    pub to: String,
    pub length_mm: f64,
    pub straight_mm: f64,
    pub detour: f64,
    pub bends: usize,
    pub bends_per_10mm: f64,
    pub acute: usize,
    pub jogs: usize,
    pub vias: usize,
}

/// All connections of a board.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Measured {
    pub connections: Vec<Connection>,
    /// Pin pairs on routed copper that are not connections (zero length).
    pub skipped: usize,
    /// Nets with at least one connection.
    pub nets: usize,
}

/// A float with a total order, for the search queue.
#[derive(Clone, Copy, PartialEq)]
struct Dist(f64);
impl Eq for Dist {}
impl PartialOrd for Dist {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Dist {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.total_cmp(&other.0)
    }
}

/// The copper graph of one net.
struct Graph {
    /// The items of the net, by id.
    items: Vec<ItemId>,
    /// The first node of each item: one node for a pin or via, two for a trace.
    first_node: Vec<usize>,
    /// Whether each item is a trace.
    is_trace: Vec<bool>,
    /// Item index of each node and, for a trace, which end it is.
    node_item: Vec<(usize, u8)>,
    edges: Vec<(usize, usize, f64)>,
    adjacent: Vec<Vec<usize>>,
}

impl Graph {
    fn build(board: &BasicBoard, items: Vec<ItemId>) -> Graph {
        let mut first_node = Vec::with_capacity(items.len());
        let mut is_trace = Vec::with_capacity(items.len());
        let mut node_item = Vec::new();
        for (index, id) in items.iter().enumerate() {
            first_node.push(node_item.len());
            node_item.push((index, 0));
            let trace = board.item(*id).is_some_and(Item::is_trace);
            is_trace.push(trace);
            if trace {
                node_item.push((index, 1));
            }
        }
        let mut graph = Graph {
            items,
            first_node,
            is_trace,
            node_item,
            edges: Vec::new(),
            adjacent: Vec::new(),
        };
        graph.add_edges(board);
        let mut adjacent = vec![Vec::new(); graph.node_item.len()];
        for (index, (a, b, _)) in graph.edges.iter().enumerate() {
            adjacent[*a].push(index);
            adjacent[*b].push(index);
        }
        graph.adjacent = adjacent;
        graph
    }

    fn index_of(&self, id: ItemId) -> Option<usize> {
        self.items.binary_search(&id).ok()
    }

    fn add_edges(&mut self, board: &BasicBoard) {
        for index in 0..self.items.len() {
            let id = self.items[index];
            let Some(item) = board.item(id) else { continue };
            match &item.kind {
                ItemKind::Trace(trace) => {
                    let first = self.first_node[index];
                    self.edges.push((first, first + 1, trace.length()));
                    for (end, corner) in [(0, trace.first_corner()), (1, trace.last_corner())] {
                        for other in board.trace_normal_contacts(id, &corner, false) {
                            self.contact(board, first + end, &corner, other);
                        }
                    }
                }
                ItemKind::Pin(_) | ItemKind::Via(_) => {
                    for other in board.normal_contacts(id) {
                        let Some(j) = self.index_of(other) else {
                            continue;
                        };
                        if board.item(other).is_some_and(|o| o.drill().is_some()) && j > index {
                            self.edges
                                .push((self.first_node[index], self.first_node[j], 0.0));
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Joins trace end `node` (at `corner`) to the node of `other` there.
    fn contact(
        &mut self,
        board: &BasicBoard,
        node: usize,
        corner: &freerouting::geometry::Point,
        other: ItemId,
    ) {
        let Some(j) = self.index_of(other) else {
            return;
        };
        let Some(item) = board.item(other) else {
            return;
        };
        match &item.kind {
            ItemKind::Trace(t) => {
                for (end, c) in [(0, t.first_corner()), (1, t.last_corner())] {
                    if c == *corner && self.first_node[j] + end != node {
                        let other_node = self.first_node[j] + end;
                        // Both traces report the contact: keep one edge.
                        if node < other_node {
                            self.edges.push((node, other_node, 0.0));
                        }
                    }
                }
            }
            ItemKind::Pin(_) | ItemKind::Via(_) => {
                self.edges.push((node, self.first_node[j], 0.0));
            }
            _ => {}
        }
    }
}

/// One step of a path: a trace and the direction it is walked, or a pin or
/// via.
enum Step {
    Trace { item: ItemId, reversed: bool },
    Drill { item: ItemId },
}

/// The pins of a net and their territories after one multi-source search.
struct Territories {
    dist: Vec<f64>,
    source: Vec<Option<usize>>,
    pred: Vec<Option<usize>>,
}

fn search(graph: &Graph, pins: &[usize]) -> Territories {
    let n = graph.node_item.len();
    let mut t = Territories {
        dist: vec![f64::INFINITY; n],
        source: vec![None; n],
        pred: vec![None; n],
    };
    let mut queue = BinaryHeap::new();
    for (k, &node) in pins.iter().enumerate() {
        t.dist[node] = 0.0;
        t.source[node] = Some(k);
        queue.push(Reverse((Dist(0.0), node)));
    }
    while let Some(Reverse((Dist(d), node))) = queue.pop() {
        if d > t.dist[node] {
            continue;
        }
        for &e in &graph.adjacent[node] {
            let (a, b, w) = graph.edges[e];
            let next = if a == node { b } else { a };
            let nd = d + w;
            if nd < t.dist[next] {
                t.dist[next] = nd;
                t.source[next] = t.source[node];
                t.pred[next] = Some(node);
                queue.push(Reverse((Dist(nd), next)));
            }
        }
    }
    t
}

fn find(parent: &mut [usize], x: usize) -> usize {
    let mut root = x;
    while parent[root] != root {
        root = parent[root];
    }
    let mut x = x;
    while parent[x] != root {
        let next = parent[x];
        parent[x] = root;
        x = next;
    }
    root
}

/// The node path from the source of `node` to `node`.
fn walk_back(t: &Territories, node: usize) -> Vec<usize> {
    let mut path = vec![node];
    let mut at = node;
    while let Some(p) = t.pred[at] {
        path.push(p);
        at = p;
    }
    path.reverse();
    path
}

/// The pairs of pin indexes (into `pins`) of the spanning tree and the node
/// path of each, from the first pin to the second.
fn spanning_pairs(graph: &Graph, pins: &[usize]) -> Vec<(usize, usize, Vec<usize>)> {
    let t = search(graph, pins);
    let mut candidates = Vec::new();
    for (e, &(a, b, w)) in graph.edges.iter().enumerate() {
        let (Some(sa), Some(sb)) = (t.source[a], t.source[b]) else {
            continue;
        };
        if sa != sb {
            candidates.push((Dist(t.dist[a] + w + t.dist[b]), sa.min(sb), sa.max(sb), e));
        }
    }
    candidates.sort();
    let mut parent: Vec<usize> = (0..pins.len()).collect();
    let mut pairs = Vec::new();
    for (_, _, _, e) in candidates {
        let (a, b, _) = graph.edges[e];
        let (sa, sb) = (t.source[a].unwrap_or(0), t.source[b].unwrap_or(0));
        let (ra, rb) = (find(&mut parent, sa), find(&mut parent, sb));
        if ra == rb {
            continue;
        }
        parent[ra] = rb;
        let (first, second) = if sa < sb { (a, b) } else { (b, a) };
        let mut path = walk_back(&t, first);
        let mut tail = walk_back(&t, second);
        tail.reverse();
        path.extend(tail);
        pairs.push((sa.min(sb), sa.max(sb), path));
    }
    pairs
}

fn steps_of(graph: &Graph, path: &[usize]) -> Vec<Step> {
    let mut steps = Vec::new();
    let mut i = 0;
    while i < path.len() {
        let (item, end) = graph.node_item[path[i]];
        let id = graph.items[item];
        match path.get(i + 1).map(|&n| graph.node_item[n]) {
            Some((next_item, next_end)) if next_item == item && next_end != end => {
                steps.push(Step::Trace {
                    item: id,
                    reversed: end == 1,
                });
                i += 2;
            }
            _ => {
                if !graph.is_trace[item] {
                    steps.push(Step::Drill { item: id });
                }
                i += 1;
            }
        }
    }
    steps
}

/// The distance between two board points.
fn between(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

/// Measures one path: the runs of copper between pins and vias.
fn measure_steps(board: &BasicBoard, steps: &[Step]) -> (RunStats, usize) {
    let mut total = RunStats::default();
    let mut vias = 0;
    let mut run: Vec<Seg> = Vec::new();
    let flush = |run: &mut Vec<Seg>, total: &mut RunStats| {
        total.add(&measure_run(run));
        run.clear();
    };
    let mut last_layer: Option<i32> = None;
    let mut passed_pin = false;
    for step in steps {
        match step {
            Step::Drill { item } => {
                flush(&mut run, &mut total);
                match board.item(*item).map(|i| &i.kind) {
                    Some(ItemKind::Via(_)) => {
                        vias += 1;
                        passed_pin = false;
                    }
                    Some(ItemKind::Pin(_)) => passed_pin = true,
                    _ => {}
                }
            }
            Step::Trace { item, reversed } => {
                let Some(trace) = board.item(*item).and_then(Item::as_trace) else {
                    continue;
                };
                if passed_pin && last_layer.is_some_and(|l| l != trace.layer) {
                    vias += 1;
                }
                passed_pin = false;
                last_layer = Some(trace.layer);
                let mut corners: Vec<(f64, f64)> = trace
                    .polyline
                    .corner_approx_arr()
                    .iter()
                    .map(|c| (c.x, c.y))
                    .collect();
                if *reversed {
                    corners.reverse();
                }
                let width = 2.0 * f64::from(trace.half_width);
                for pair in corners.windows(2) {
                    run.push(Seg {
                        a: pair[0],
                        b: pair[1],
                        width,
                    });
                }
            }
        }
    }
    flush(&mut run, &mut total);
    (total, vias)
}

fn pin_label(board: &BasicBoard, id: ItemId) -> String {
    let item = board.item(id);
    let component = item
        .and_then(|i| {
            board
                .components
                .get(freerouting::board::structure::ComponentId(i.component_id()))
        })
        .map_or_else(|| "?".to_string(), |c| c.name.clone());
    let pin = item
        .and_then(|i| board.pin_name(i))
        .map_or_else(|| "?".to_string(), str::to_string);
    format!("{component}-{pin}")
}

fn pin_centre(board: &BasicBoard, id: ItemId) -> (f64, f64) {
    board
        .item(id)
        .and_then(Item::drill)
        .map_or((0.0, 0.0), |d| {
            (f64::from(d.center.x), f64::from(d.center.y))
        })
}

/// Millimetres per board unit.
pub fn mm_per_unit(board: &BasicBoard) -> f64 {
    let resolution = board.communication.resolution;
    Unit::scale(1.0, board.communication.unit, Unit::Mm)
        / f64::from(if resolution > 0 { resolution } else { 1 })
}

/// The copper items (pins, vias, traces) of every net, by net number.
fn items_by_net(board: &BasicBoard) -> BTreeMap<i32, Vec<ItemId>> {
    let mut by_net: BTreeMap<i32, Vec<ItemId>> = BTreeMap::new();
    for item in board.items() {
        if !matches!(
            item.kind,
            ItemKind::Pin(_) | ItemKind::Via(_) | ItemKind::Trace(_)
        ) {
            continue;
        }
        for &net in item.net_numbers() {
            if net > 0 {
                by_net.entry(net).or_default().push(item.id());
            }
        }
    }
    for items in by_net.values_mut() {
        items.sort();
        items.dedup();
    }
    by_net
}

/// Measures every routed connection of `board` (its pins joined by traces and
/// vias, whatever put them there).
pub fn measure_board(board: &BasicBoard) -> Measured {
    let to_mm = mm_per_unit(board);
    let mut result = Measured::default();
    for (net, items) in items_by_net(board) {
        let graph = Graph::build(board, items);
        let pins: Vec<usize> = (0..graph.items.len())
            .filter(|&i| board.item(graph.items[i]).is_some_and(Item::is_pin))
            .map(|i| graph.first_node[i])
            .collect();
        if pins.len() < 2 {
            continue;
        }
        let name = board
            .rules
            .nets
            .get(net)
            .map_or_else(|| net.to_string(), |n| n.name.clone());
        let before = result.connections.len();
        for (a, b, path) in spanning_pairs(&graph, &pins) {
            let (from, to) = (
                graph.items[graph.node_item[pins[a]].0],
                graph.items[graph.node_item[pins[b]].0],
            );
            let (stats, vias) = measure_steps(board, &steps_of(&graph, &path));
            let straight = between(pin_centre(board, from), pin_centre(board, to));
            if stats.length < 1.0 || straight < 1.0 {
                result.skipped += 1;
                continue;
            }
            let length_mm = stats.length * to_mm;
            result.connections.push(Connection {
                net: name.clone(),
                from: pin_label(board, from),
                to: pin_label(board, to),
                length_mm,
                straight_mm: straight * to_mm,
                detour: stats.length / straight,
                bends: stats.bends,
                bends_per_10mm: stats.bends as f64 * 10.0 / length_mm,
                acute: stats.acute,
                jogs: stats.jogs,
                vias,
            });
        }
        if result.connections.len() > before {
            result.nets += 1;
        }
    }
    result
}
