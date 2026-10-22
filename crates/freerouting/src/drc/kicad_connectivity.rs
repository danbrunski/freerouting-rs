//! K15.16: the split nets and isolated islands of a KiCad-counted report.
//!
//! Not Java. Under [`ReportRules::Kicad`](super::ReportRules::Kicad) the
//! report's `unconnectedItems` and its `isolated_island_dead_copper` findings
//! come from KiCad 10's connectivity, not from Java's
//! (`DesignRulesChecker.getAllUnconnectedItems`, which joins every item a
//! conduction area's outline touches, and `getZoneIslandViolations`):
//!
//! - **Zone copper is the fill, one node per island.** A `.kicad_pcb` read
//!   with its fills current ([`KicadReportRules::fills_current`]: every zone
//!   filled and the board unchanged since the read; `-drc` on a `.kicad_pcb`)
//!   gives each `filled_polygon` of each zone as an island, the pours the DSN
//!   reader dropped (`BasicBoard::dropped_pours`) included. Otherwise (a DSN,
//!   a routed or edited board) the router's fill model gives them: the
//!   islands of each conduction area's detailed fill (outline minus the
//!   obstacles of other nets, [`fill_islands`]), and a dropped pour's whole
//!   outline (it is no board item, so it has no fill model). A DSN carries no
//!   fill, so the fill model is all a DSN input can count with.
//! - **Items join by copper** (`CN_VISITOR`): two pins, traces or vias of a
//!   net on a common layer whose own copper ([`Copper`]) touches; a pad or via
//!   and an island whose shapes overlap (`checkZoneItemConnection`); a trace
//!   and an island that contains an end of one of its segments; two islands
//!   of a net on a layer when a corner of one lies in the other
//!   (`checkZoneZoneConnection`).
//! - **Isolated islands** are the islands whose group has no pin
//!   (`CN_CLUSTER::IsOrphaned`). KiCad's filler removes them as the zone's
//!   `island_removal_mode` says (0 always, 1 never, 2 below
//!   `island_area_min`; a DSN zone, which carries none, takes KiCad's default
//!   0), except where every island of the zone on that layer is isolated:
//!   KiCad 10.0.7 then keeps them all (measured on hand-made boards, K15.16).
//!   A kept one is an `isolated_island_dead_copper` finding (KiCad's
//!   `isolated_copper`); a removed one is no copper. The groups are then
//!   counted again without the removed islands.
//! - **One finding per missing connection**: a net whose items form `n`
//!   groups has `n - 1` `unconnectedItems` (KiCad's ratsnest edges; Java has
//!   one per split net), each naming the first group and one other. A group
//!   that is one kept isolated island alone is no group (KiCad leaves
//!   "intentionally-kept zone islands" out of the ratsnest).
//!
//! Not modelled: KiCad's thermal relief and pad zone-connection settings, the
//! zones footprints own, via flashing (a via's copper on every layer of its
//! padstack), and the minimum-thickness opening of the fill model.

use std::collections::{BTreeMap, BTreeSet};

use freerouting_geometry::{Area, FloatPoint, PolylineShapeOps, Shape};

use super::kicad_copper::Copper;
use super::kicad_report::segment_ends;
use super::kicad_rules::IslandRemoval;
use super::unconnected::{UnconnectedItems, UnconnectedKind, representative_item};
use super::zone_islands::{Island, fill_islands};
use crate::board::BasicBoard;
use crate::board::items::{Item, ItemId};

/// An isolated island KiCad keeps: an `isolated_island_dead_copper` finding.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct KicadIsland {
    pub net_number: i32,
    pub layer: i32,
    /// The conduction area the island belongs to, if the board has one.
    pub area: Option<ItemId>,
    /// The island's first corner (KiCad reports `Outline(idx).CPoint(0)`).
    pub location: FloatPoint,
}

/// The KiCad-counted split nets and kept isolated islands of a board.
pub(crate) struct KicadConnectivity {
    pub split_nets: Vec<UnconnectedItems>,
    pub islands: Vec<KicadIsland>,
}

/// The copper of one island.
enum ZoneShape {
    /// A `filled_polygon` of the file.
    Polygon(Vec<FloatPoint>),
    /// An island of the router's fill model.
    Fill(Island),
    /// A whole outline (a dropped pour, or a conduction area when the report
    /// computes no fills).
    Outline(Area),
}

/// The corners of `shape` (a circle's centre).
fn shape_corners(shape: &Shape) -> Vec<FloatPoint> {
    match shape {
        Shape::Polyline(p) => p.corner_approx_arr(),
        Shape::Circle(c) => vec![c.center.to_float()],
    }
}

fn polygon_area(corners: &[FloatPoint]) -> f64 {
    let n = corners.len();
    let twice: f64 = (0..n)
        .map(|i| {
            let (p, q) = (corners[i], corners[(i + 1) % n]);
            p.x * q.y - q.x * p.y
        })
        .sum();
    (twice / 2.0).abs()
}

fn bounds_of(points: &[FloatPoint]) -> [f64; 4] {
    points.iter().fold(
        [f64::MAX, f64::MAX, f64::MIN, f64::MIN],
        |[x0, y0, x1, y1], p| [x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y)],
    )
}

fn boxes_meet(a: &[f64; 4], b: &[f64; 4]) -> bool {
    a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3]
}

impl ZoneShape {
    fn bounds(&self) -> [f64; 4] {
        match self {
            ZoneShape::Polygon(corners) => bounds_of(corners),
            ZoneShape::Fill(island) => island.bounds,
            ZoneShape::Outline(area) => {
                let b = area.bounding_box();
                [
                    f64::from(b.ll.x),
                    f64::from(b.ll.y),
                    f64::from(b.ur.x),
                    f64::from(b.ur.y),
                ]
            }
        }
    }

    fn contains(&self, p: FloatPoint) -> bool {
        match self {
            ZoneShape::Polygon(corners) => Copper::Polygon(corners.clone()).contains(p),
            ZoneShape::Fill(island) => island.contains(p),
            ZoneShape::Outline(area) => area.contains_float(p),
        }
    }

    /// True if copper `copper` (bounds `bounds`, centre `centre`) overlaps
    /// the island, or comes within `reach` of it.
    fn overlaps(&self, copper: &Copper, bounds: &[f64; 4], centre: FloatPoint, reach: f64) -> bool {
        let grown = [
            bounds[0] - reach,
            bounds[1] - reach,
            bounds[2] + reach,
            bounds[3] + reach,
        ];
        let bounds = &grown;
        if !boxes_meet(&self.bounds(), bounds) {
            return false;
        }
        match self {
            ZoneShape::Polygon(corners) => Copper::Polygon(corners.clone()).gap(copper) <= 0.0,
            ZoneShape::Fill(island) => {
                island.contains(centre)
                    || island.trapezoids.iter().any(|t| {
                        let corners = t.corners();
                        boxes_meet(&bounds_of(&corners), bounds)
                            && Copper::Polygon(corners.to_vec()).gap(copper) <= reach
                    })
            }
            ZoneShape::Outline(area) => {
                area.contains_float(centre)
                    || Copper::Polygon(shape_corners(&area.get_border())).gap(copper) <= 0.0
            }
        }
    }

    fn corners(&self) -> Vec<FloatPoint> {
        match self {
            ZoneShape::Polygon(corners) => corners.clone(),
            ZoneShape::Fill(island) => island.trapezoids.iter().flat_map(|t| t.corners()).collect(),
            ZoneShape::Outline(area) => shape_corners(&area.get_border()),
        }
    }

    fn area(&self) -> f64 {
        match self {
            ZoneShape::Polygon(corners) => polygon_area(corners),
            ZoneShape::Fill(island) => island
                .trapezoids
                .iter()
                .map(|t| {
                    (t.x_right - t.x_left) * ((t.left[1] - t.left[0]) + (t.right[1] - t.right[0]))
                        / 2.0
                })
                .sum(),
            ZoneShape::Outline(area) => polygon_area(&shape_corners(&area.get_border())),
        }
    }

    fn first_corner(&self) -> FloatPoint {
        match self {
            ZoneShape::Fill(island) => FloatPoint::new(island.bounds[0], island.bounds[1]),
            other => other
                .corners()
                .first()
                .copied()
                .unwrap_or(FloatPoint::new(0.0, 0.0)),
        }
    }
}

/// Which zone (and layer) an island belongs to: islands of one zone on one
/// layer are removed together or kept together.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum ZoneKey {
    /// A zone of the file ([`KicadReportRules::zones`] index).
    File(usize),
    /// A conduction area of the board.
    Area(ItemId),
    /// A dropped pour (index).
    Pour(usize),
}

struct ZoneNode {
    net: i32,
    layer: i32,
    key: ZoneKey,
    removal: IslandRemoval,
    area: Option<ItemId>,
    shape: ZoneShape,
}

/// Union-find with path halving.
struct Groups(Vec<usize>);

impl Groups {
    fn new(n: usize) -> Self {
        Groups((0..n).collect())
    }

    fn find(&mut self, mut i: usize) -> usize {
        while self.0[i] != i {
            self.0[i] = self.0[self.0[i]];
            i = self.0[i];
        }
        i
    }

    fn unite(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            // The lower index is the root: the result does not depend on the
            // order of the edges.
            let (low, high) = (ra.min(rb), ra.max(rb));
            self.0[high] = low;
        }
    }
}

fn first_net(item: &Item) -> i32 {
    if item.net_count() > 0 {
        item.net_number(0)
    } else {
        0
    }
}

/// The zone of the file that conduction area `ca` is on: same net and layer,
/// the first whose outline contains the area's first corner, else the first.
pub(crate) fn file_zone_of(board: &BasicBoard, ca: &Item, net: i32, layer: i32) -> Option<usize> {
    let zones = &board.kicad_report_rules().zones;
    let candidates: Vec<usize> = (0..zones.len())
        .filter(|&i| zones[i].net_number == net && zones[i].layer == layer)
        .collect();
    let corner = ca
        .as_conduction_area()
        .and_then(|a| shape_corners(&a.area().get_border()).first().copied());
    candidates
        .iter()
        .copied()
        .find(|&i| corner.is_some_and(|c| Copper::Polygon(zones[i].outline.clone()).contains(c)))
        .or(candidates.first().copied())
}

/// The islands of the board's zone copper (module docs).
fn zone_nodes(board: &BasicBoard, compute_fills: bool) -> Vec<ZoneNode> {
    let rules = board.kicad_report_rules();
    let areas: Vec<&Item> = board
        .conduction_areas()
        .into_iter()
        .filter_map(|id| board.item(id))
        .filter(|ca| first_net(ca) > 0)
        .collect();
    let mut nodes = Vec::new();
    if rules.fills_current(board) {
        for (index, zone) in rules.zones.iter().enumerate() {
            if zone.net_number <= 0 || zone.layer < 0 {
                continue;
            }
            for island in zone.islands.iter().filter(|i| i.len() >= 3) {
                let first = island[0];
                let area = areas
                    .iter()
                    .find(|ca| {
                        first_net(ca) == zone.net_number && ca.first_layer() == zone.layer && {
                            let b = ca.bounding_box();
                            (f64::from(b.ll.x)..=f64::from(b.ur.x)).contains(&first.x)
                                && (f64::from(b.ll.y)..=f64::from(b.ur.y)).contains(&first.y)
                        }
                    })
                    .map(|ca| ca.id());
                nodes.push(ZoneNode {
                    net: zone.net_number,
                    layer: zone.layer,
                    key: ZoneKey::File(index),
                    removal: zone.removal,
                    area,
                    shape: ZoneShape::Polygon(island.clone()),
                });
            }
        }
        return nodes;
    }
    let removal_of =
        |zone: Option<usize>| zone.map_or(IslandRemoval::Always, |i| rules.zones[i].removal);
    let fills = if compute_fills {
        super::clearance::parallel_map(&areas, |ca| fill_islands(board, ca))
    } else {
        vec![Vec::new(); areas.len()]
    };
    for (ca, islands) in areas.iter().zip(fills) {
        let (net, layer) = (first_net(ca), ca.first_layer());
        let removal = removal_of(file_zone_of(board, ca, net, layer));
        let node = |shape| ZoneNode {
            net,
            layer,
            key: ZoneKey::Area(ca.id()),
            removal,
            area: Some(ca.id()),
            shape,
        };
        if compute_fills {
            nodes.extend(islands.into_iter().map(|i| node(ZoneShape::Fill(i))));
        } else if let Some(a) = ca.as_conduction_area() {
            nodes.push(node(ZoneShape::Outline(a.area().clone())));
        }
    }
    for (index, pour) in board.dropped_pours().iter().enumerate() {
        let zone = (0..rules.zones.len()).find(|&i| {
            let z = &rules.zones[i];
            z.net_number == pour.net_number
                && z.layer == pour.layer
                && shape_corners(&pour.area.get_border())
                    .first()
                    .is_some_and(|c| Copper::Polygon(z.outline.clone()).contains(*c))
        });
        nodes.push(ZoneNode {
            net: pour.net_number,
            layer: pour.layer,
            key: ZoneKey::Pour(index),
            removal: removal_of(zone),
            area: None,
            shape: ZoneShape::Outline(pour.area.clone()),
        });
    }
    nodes
}

fn item_bounds(item: &Item) -> [f64; 4] {
    let b = item.bounding_box();
    [
        f64::from(b.ll.x),
        f64::from(b.ll.y),
        f64::from(b.ur.x),
        f64::from(b.ur.y),
    ]
}

/// True if item `item` on `layer` joins island `zone`.
///
/// The fill model keeps a pad or via of the area's net at the clearance from
/// the fill and joins it by spokes that may not reach the fill (Java's
/// `ConductionArea.getDetailedFillArea`); KiCad's fill reaches it (solid or by
/// thermal relief), so a pad or via within that clearance of an island of the
/// fill model joins it.
fn item_meets_zone(board: &BasicBoard, item: &Item, layer: i32, zone: &ZoneNode) -> bool {
    if item.is_trace() {
        return segment_ends(board, item)
            .into_iter()
            .any(|p| zone.shape.contains(p));
    }
    let (Some(copper), Some(drill)) = (Copper::of(item, layer), item.drill()) else {
        return false;
    };
    let reach = match (&zone.shape, zone.area.and_then(|id| board.item(id))) {
        (ZoneShape::Fill(_), Some(ca)) => {
            f64::from(board.clearance_value(ca.clearance_class(), item.clearance_class(), layer))
                + 1.0
        }
        _ => 0.0,
    };
    zone.shape
        .overlaps(&copper, &item_bounds(item), drill.center.to_float(), reach)
}

/// The edges between the board's connectable items (indexes into `items`)
/// whose copper touches on a common layer.
fn item_edges(
    board: &BasicBoard,
    items: &[&Item],
    index: &BTreeMap<ItemId, usize>,
) -> Vec<(usize, usize)> {
    let mut edges = Vec::new();
    for (i, item) in items.iter().enumerate() {
        for layer in item.first_layer()..=item.last_layer() {
            let Some(copper) = Copper::of(item, layer) else {
                continue;
            };
            let region = freerouting_geometry::TileShape::IntBox(item.bounding_box());
            for id in board.overlapping_objects(&region, layer) {
                let Some(&j) = index.get(&id) else {
                    continue;
                };
                if j <= i || !items[j].shares_net(item) {
                    continue;
                }
                if Copper::of(items[j], layer).is_some_and(|other| copper.gap(&other) <= 0.0) {
                    edges.push((i, j));
                }
            }
        }
    }
    edges
}

/// The KiCad-counted split nets and kept isolated islands of `board` (module
/// docs); `compute_fills` false counts a conduction area as its outline and
/// finds no islands.
pub(crate) fn kicad_connectivity(board: &BasicBoard, compute_fills: bool) -> KicadConnectivity {
    let items: Vec<&Item> = board
        .items()
        .filter(|i| i.is_connectable_kind() && !i.is_conduction_area() && first_net(i) > 0)
        .collect();
    let index: BTreeMap<ItemId, usize> = items
        .iter()
        .enumerate()
        .map(|(i, item)| (item.id(), i))
        .collect();
    let zones = zone_nodes(board, compute_fills);
    let n = items.len();
    let mut edges = item_edges(board, &items, &index);
    // items of each net and layer, for the zones
    let mut by_net_layer: BTreeMap<(i32, i32), Vec<usize>> = BTreeMap::new();
    for (i, item) in items.iter().enumerate() {
        for layer in item.first_layer()..=item.last_layer() {
            by_net_layer
                .entry((first_net(item), layer))
                .or_default()
                .push(i);
        }
    }
    for (z, zone) in zones.iter().enumerate() {
        let bounds = zone.shape.bounds();
        for &i in by_net_layer
            .get(&(zone.net, zone.layer))
            .into_iter()
            .flatten()
        {
            let item = items[i];
            // a pad's or via's test grows its own bounds (`item_meets_zone`)
            if (!item.is_trace() || boxes_meet(&bounds, &item_bounds(item)))
                && item_meets_zone(board, item, zone.layer, zone)
            {
                edges.push((i, n + z));
            }
        }
    }
    for a in 0..zones.len() {
        let bounds_a = zones[a].shape.bounds();
        for b in a + 1..zones.len() {
            let (za, zb) = (&zones[a], &zones[b]);
            if za.net != zb.net
                || za.layer != zb.layer
                || !boxes_meet(&bounds_a, &zb.shape.bounds())
            {
                continue;
            }
            let meets = za.shape.corners().into_iter().any(|p| zb.shape.contains(p))
                || zb.shape.corners().into_iter().any(|p| za.shape.contains(p));
            if meets {
                edges.push((n + a, n + b));
            }
        }
    }
    let total = n + zones.len();
    let mut groups = Groups::new(total);
    for &(a, b) in &edges {
        groups.unite(a, b);
    }
    // the groups with a pin
    let mut with_pin = BTreeSet::new();
    for (i, item) in items.iter().enumerate() {
        if item.is_pin() {
            with_pin.insert(groups.find(i));
        }
    }
    let isolated: Vec<bool> = (0..zones.len())
        .map(|z| !with_pin.contains(&groups.find(n + z)))
        .collect();
    // islands per zone and layer: all isolated keeps them all
    let mut all_isolated: BTreeMap<(ZoneKey, i32), bool> = BTreeMap::new();
    for (z, zone) in zones.iter().enumerate() {
        *all_isolated.entry((zone.key, zone.layer)).or_insert(true) &= isolated[z];
    }
    let mut removed = vec![false; zones.len()];
    let mut kept_islands = Vec::new();
    for (z, zone) in zones.iter().enumerate() {
        if !isolated[z] {
            continue;
        }
        let remove = !all_isolated[&(zone.key, zone.layer)]
            && match zone.removal {
                IslandRemoval::Always => true,
                IslandRemoval::Never => false,
                IslandRemoval::BelowArea(min) => zone.shape.area() < min,
            };
        if remove {
            removed[z] = true;
        } else {
            kept_islands.push(z);
        }
    }
    // the groups without the removed islands
    let mut groups = Groups::new(total);
    for &(a, b) in &edges {
        let gone = |k: usize| k >= n && removed[k - n];
        if !gone(a) && !gone(b) {
            groups.unite(a, b);
        }
    }
    let mut members: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for k in 0..total {
        if k >= n && removed[k - n] {
            continue;
        }
        members.entry(groups.find(k)).or_default().push(k);
    }
    let mut by_net: BTreeMap<i32, Vec<BTreeSet<ItemId>>> = BTreeMap::new();
    for group in members.values() {
        if let [k] = group.as_slice()
            && *k >= n
        {
            // a kept isolated island alone is not in KiCad's ratsnest
            continue;
        }
        let net = if group[0] < n {
            first_net(items[group[0]])
        } else {
            zones[group[0] - n].net
        };
        let ids: BTreeSet<ItemId> = group
            .iter()
            .filter_map(|&k| {
                if k < n {
                    Some(items[k].id())
                } else {
                    zones[k - n].area
                }
            })
            .collect();
        if !ids.is_empty() {
            by_net.entry(net).or_default().push(ids);
        }
    }
    let mut split_nets = Vec::new();
    for sets in by_net.values() {
        let Some(first) = sets.first() else {
            continue;
        };
        let Some(first_item) = representative_item(board, first) else {
            continue;
        };
        for other in &sets[1..] {
            let Some(second) = representative_item(board, other) else {
                continue;
            };
            let mut all_items: Vec<ItemId> = first.iter().copied().collect();
            all_items.extend(other.iter().copied());
            split_nets.push(UnconnectedItems {
                first_item,
                second_item: Some(second),
                all_items,
                kind: UnconnectedKind::UnconnectedItems,
            });
        }
    }
    let islands = kept_islands
        .into_iter()
        .map(|z| {
            let zone = &zones[z];
            KicadIsland {
                net_number: zone.net,
                layer: zone.layer,
                area: zone.area,
                location: zone.shape.first_corner(),
            }
        })
        .collect();
    KicadConnectivity {
        split_nets,
        islands,
    }
}
