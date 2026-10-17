//! The fill model of conduction areas and own-net connectivity (B09.3).
//!
//! For plane-tap verification, the pour-cut audit (Q01.2) and region repair
//! (X02): which copper islands a pour's fill has, which items of its net
//! anchor each island, and how the net's items connect. KiCadRoutingTools
//! keeps a raster fill for this (`plane_fill_model.py`) and a union-find
//! over pins, traces, vias and areas (`plane_component_oracle.py`); both
//! ideas are credited here, no code is ported.
//!
//! There is one fill computation: Java's detailed fill as the DRC computes it
//! ([`fill_islands`], `ConductionArea.getDetailedFillArea`). This module adds
//! no fallback that depends on the environment (KRT's fill source depends on
//! whether KiCad is installed, its #675).
//!
//! - [`conduction_fill`]: the islands of one area, in [`fill_islands`]'s
//!   order (lowest y, then lowest x), each with its boundary polygons and
//!   its anchors: the net's items whose centre (first corner for traces)
//!   lies on the island, highest id first, exactly as
//!   [`ZoneIslandViolation::items_in_island`](super::ZoneIslandViolation)
//!   computes them;
//! - [`island_outline`]: an island's trapezoids merged into boundary
//!   polygons. Each trapezoid side is a chain element; sides on one vertical
//!   or horizontal line cancel by exact interval arithmetic, other sides
//!   cancel against their exact reverse, and what is left is traced into
//!   rings. The arithmetic is exact on the trapezoids' own coordinates, so
//!   the rings always close; outer rings run counterclockwise, holes
//!   clockwise;
//! - [`net_components`]: the connected components of a net's pins, traces,
//!   vias and conduction areas, built on `connected_set`.
//!
//! KiCad's minimum-thickness opening of the fill (erode, then dilate by half
//! the thickness) is not modelled; Q01.4 measures whether it is needed.

use std::collections::{BTreeMap, BTreeSet};

use crate::util::HashMap;

use freerouting_geometry::FloatPoint;

use super::zone_islands::{Island, fill_islands};
use crate::board::BasicBoard;
use crate::board::items::{Item, ItemId, ItemKind};

/// A closed boundary ring of an island, without the repeated first point.
#[derive(Clone, Debug, PartialEq)]
pub struct FillPolygon {
    pub points: Vec<FloatPoint>,
}

impl FillPolygon {
    /// The shoelace area: positive for a counterclockwise (outer) ring,
    /// negative for a clockwise one (a hole).
    pub fn signed_area(&self) -> f64 {
        let n = self.points.len();
        let mut twice = 0.0;
        for i in 0..n {
            let (p, q) = (self.points[i], self.points[(i + 1) % n]);
            twice += p.x * q.y - q.x * p.y;
        }
        twice / 2.0
    }

    /// True for a clockwise ring.
    pub fn is_hole(&self) -> bool {
        self.signed_area() < 0.0
    }
}

/// One island of a conduction area's fill.
#[derive(Clone, Debug, PartialEq)]
pub struct FillIsland {
    /// The island as the DRC computes it (trapezoids and bounds).
    pub island: Island,
    /// The boundary rings: outer rings first, then holes, each group by its
    /// lowest vertex (y, then x).
    pub outline: Vec<FillPolygon>,
    /// The items of the area's net on the area's layer whose centre (first
    /// corner for traces) lies on the island, highest id first.
    pub anchors: Vec<ItemId>,
}

/// The islands of the detailed fill of conduction area `area`, with their
/// outlines and anchors; empty for a missing or unfilled area.
pub fn conduction_fill(board: &BasicBoard, area: ItemId) -> Vec<FillIsland> {
    let Some(ca) = board.item(area) else {
        return Vec::new();
    };
    let islands = fill_islands(board, ca);
    if islands.is_empty() {
        return Vec::new();
    }
    let candidates = anchor_candidates(board, ca);
    islands
        .into_iter()
        .map(|island| {
            let anchors = candidates
                .iter()
                .filter(|(_, c)| island.contains(*c))
                .map(|(id, _)| *id)
                .collect();
            FillIsland {
                outline: island_outline(&island),
                island,
                anchors,
            }
        })
        .collect()
}

/// The items that can anchor an island of `ca`, with their centres, in
/// item order: the filter and centres of `zone_island_violations`.
fn anchor_candidates(board: &BasicBoard, ca: &Item) -> Vec<(ItemId, FloatPoint)> {
    board
        .items()
        .filter(|i| {
            i.id() != ca.id() && i.is_connectable_kind() && i.shares_net(ca) && i.shares_layer(ca)
        })
        .filter_map(|i| {
            let center = match &i.kind {
                ItemKind::Pin(_) | ItemKind::Via(_) => i.drill().map(|d| d.center.to_float()),
                ItemKind::Trace(t) => Some(t.first_corner().to_float()),
                _ => None,
            };
            center.map(|c| (i.id(), c))
        })
        .collect()
}

/// The connected components of net `net`'s connectable items (pins,
/// traces, vias, conduction areas), each highest id first, the components
/// ordered by their highest member (highest first).
///
/// Without `stop_at_plane` this is [`BasicBoard::connected_sets`]. With it,
/// connections through conduction areas that belong to no component (the
/// planes) are not followed: each plane is a component of its own and the
/// rest falls apart into what connects without planes.
pub fn net_components(board: &BasicBoard, net: i32, stop_at_plane: bool) -> Vec<Vec<ItemId>> {
    let mut result = Vec::new();
    if net <= 0 {
        return result;
    }
    let mut to_handle: BTreeSet<ItemId> = board.connectable_items(net).into_iter().collect();
    while let Some(first) = to_handle.pop_first() {
        let is_plane = board
            .item(first)
            .is_some_and(|i| i.is_conduction_area() && i.component_id() <= 0);
        let set: BTreeSet<ItemId> = if stop_at_plane && is_plane {
            BTreeSet::from([first])
        } else {
            board.connected_set(first, net, stop_at_plane)
        };
        for id in &set {
            to_handle.remove(id);
        }
        result.push(set.into_iter().collect());
    }
    result
}

// ---------------------------------------------------------------------------
// Island outlines.

/// A float as an exact map key; `-0.0` and `0.0` are one key.
fn bits(v: f64) -> u64 {
    (v + 0.0).to_bits()
}

type Key = (u64, u64);

fn key(p: FloatPoint) -> Key {
    (bits(p.x), bits(p.y))
}

/// A directed boundary edge.
#[derive(Clone, Copy, Debug)]
struct Edge {
    from: FloatPoint,
    to: FloatPoint,
}

/// Signed intervals on one vertical (or horizontal) line: `(low, high, +1
/// or -1)`; the sign is +1 when the side runs towards higher coordinates.
#[derive(Default)]
struct LineChain {
    /// The line's fixed coordinate.
    at: f64,
    pieces: Vec<(f64, f64, i32)>,
}

impl LineChain {
    /// The edges left after cancellation, one per elementary interval.
    fn edges(&self, vertical: bool, out: &mut Vec<Edge>) {
        let mut cuts: Vec<f64> = self.pieces.iter().flat_map(|p| [p.0, p.1]).collect();
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|a, b| bits(*a) == bits(*b));
        let index: HashMap<u64, usize> = cuts
            .iter()
            .enumerate()
            .map(|(i, c)| (bits(*c), i))
            .collect();
        let mut delta = vec![0i32; cuts.len()];
        for &(low, high, sign) in &self.pieces {
            delta[index[&bits(low)]] += sign;
            delta[index[&bits(high)]] -= sign;
        }
        let mut sum = 0;
        for i in 0..cuts.len().saturating_sub(1) {
            sum += delta[i];
            if sum == 0 {
                continue;
            }
            let (a, b) = if sum > 0 {
                (cuts[i], cuts[i + 1])
            } else {
                (cuts[i + 1], cuts[i])
            };
            let point = |c: f64| {
                if vertical {
                    FloatPoint::new(self.at, c)
                } else {
                    FloatPoint::new(c, self.at)
                }
            };
            // A sum of more than one would mean overlapping trapezoids.
            for _ in 0..sum.unsigned_abs() {
                out.push(Edge {
                    from: point(a),
                    to: point(b),
                });
            }
        }
    }
}

/// The boundary of `island` as closed rings (module docs).
pub fn island_outline(island: &Island) -> Vec<FillPolygon> {
    let edges = boundary_edges(island);
    let mut rings: Vec<FillPolygon> = trace_rings(&edges)
        .into_iter()
        .map(|points| FillPolygon {
            points: drop_collinear(points),
        })
        .filter(|ring| ring.points.len() >= 3)
        .collect();
    rings.sort_by(|a, b| {
        let lowest = |r: &FillPolygon| {
            r.points
                .iter()
                .map(|p| (p.y, p.x))
                .min_by(|p, q| p.0.total_cmp(&q.0).then(p.1.total_cmp(&q.1)))
                .unwrap_or((0.0, 0.0))
        };
        let (la, lb) = (lowest(a), lowest(b));
        a.is_hole()
            .cmp(&b.is_hole())
            .then(la.0.total_cmp(&lb.0))
            .then(la.1.total_cmp(&lb.1))
    });
    rings
}

/// The trapezoid sides that are left after cancellation.
fn boundary_edges(island: &Island) -> Vec<Edge> {
    let mut vertical: BTreeMap<u64, LineChain> = BTreeMap::new();
    let mut horizontal: BTreeMap<u64, LineChain> = BTreeMap::new();
    // Sloped sides by their exact end points, with their multiplicity.
    let mut sloped: BTreeMap<(Key, Key), (Edge, i32)> = BTreeMap::new();
    for t in &island.trapezoids {
        let [ll, lr, ur, ul] = t.corners();
        for (from, to) in [(ll, lr), (lr, ur), (ur, ul), (ul, ll)] {
            if key(from) == key(to) {
                continue;
            }
            if bits(from.x) == bits(to.x) {
                let chain = vertical.entry(bits(from.x)).or_default();
                chain.at = from.x;
                let sign = if to.y > from.y { 1 } else { -1 };
                chain
                    .pieces
                    .push((from.y.min(to.y), from.y.max(to.y), sign));
            } else if bits(from.y) == bits(to.y) {
                let chain = horizontal.entry(bits(from.y)).or_default();
                chain.at = from.y;
                let sign = if to.x > from.x { 1 } else { -1 };
                chain
                    .pieces
                    .push((from.x.min(to.x), from.x.max(to.x), sign));
            } else {
                let (kf, kt) = (key(from), key(to));
                if let Some(reverse) = sloped.get_mut(&(kt, kf)) {
                    reverse.1 -= 1;
                } else {
                    sloped.entry((kf, kt)).or_insert((Edge { from, to }, 0)).1 += 1;
                }
            }
        }
    }
    let mut edges = Vec::new();
    for chain in vertical.values() {
        chain.edges(true, &mut edges);
    }
    for chain in horizontal.values() {
        chain.edges(false, &mut edges);
    }
    for (edge, count) in sloped.into_values() {
        let reversed = Edge {
            from: edge.to,
            to: edge.from,
        };
        let (edge, n) = if count >= 0 {
            (edge, count)
        } else {
            (reversed, -count)
        };
        edges.extend(std::iter::repeat_n(edge, n as usize));
    }
    edges
}

/// Traces the edges into rings. At a vertex with several unused outgoing
/// edges the ring turns most to the left, which keeps the fill on its left
/// and separates rings that only touch at a corner.
fn trace_rings(edges: &[Edge]) -> Vec<Vec<FloatPoint>> {
    let mut order: Vec<usize> = (0..edges.len()).collect();
    order.sort_by(|&a, &b| {
        let (p, q) = (edges[a].from, edges[b].from);
        p.y.total_cmp(&q.y)
            .then(p.x.total_cmp(&q.x))
            .then(edges[a].to.y.total_cmp(&edges[b].to.y))
            .then(edges[a].to.x.total_cmp(&edges[b].to.x))
            .then(a.cmp(&b))
    });
    let mut outgoing: HashMap<Key, Vec<usize>> = HashMap::default();
    for &e in &order {
        outgoing.entry(key(edges[e].from)).or_default().push(e);
    }
    let mut used = vec![false; edges.len()];
    let mut rings = Vec::new();
    for &start in &order {
        if used[start] {
            continue;
        }
        let first = key(edges[start].from);
        let mut ring = Vec::new();
        let mut current = start;
        loop {
            used[current] = true;
            let edge = edges[current];
            ring.push(edge.from);
            if key(edge.to) == first {
                break;
            }
            let incoming = (edge.to.x - edge.from.x, edge.to.y - edge.from.y);
            let next = outgoing
                .get(&key(edge.to))
                .into_iter()
                .flatten()
                .copied()
                .filter(|&e| !used[e])
                .max_by(|&a, &b| {
                    turn(incoming, edges[a])
                        .total_cmp(&turn(incoming, edges[b]))
                        .then(b.cmp(&a))
                });
            match next {
                Some(next) => current = next,
                // Only reachable with inconsistent input; the open chain
                // is dropped.
                None => {
                    ring.clear();
                    break;
                }
            }
        }
        if !ring.is_empty() {
            rings.push(ring);
        }
    }
    rings
}

/// The signed turn angle from direction `incoming` to `edge`.
fn turn(incoming: (f64, f64), edge: Edge) -> f64 {
    let out = (edge.to.x - edge.from.x, edge.to.y - edge.from.y);
    let cross = incoming.0 * out.1 - incoming.1 * out.0;
    let dot = incoming.0 * out.0 + incoming.1 * out.1;
    cross.atan2(dot)
}

/// Drops the vertices where the ring goes straight on (exactly).
fn drop_collinear(points: Vec<FloatPoint>) -> Vec<FloatPoint> {
    let n = points.len();
    if n < 3 {
        return points;
    }
    let mut kept: Vec<FloatPoint> = Vec::with_capacity(n);
    for i in 0..n {
        let (p, q, r) = (points[(i + n - 1) % n], points[i], points[(i + 1) % n]);
        let (ax, ay) = (q.x - p.x, q.y - p.y);
        let (bx, by) = (r.x - q.x, r.y - q.y);
        let straight = ax * by - ay * bx == 0.0 && ax * bx + ay * by > 0.0;
        if !straight {
            kept.push(q);
        }
    }
    kept
}
