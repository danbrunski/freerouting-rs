//! Islands, anchors and main components of plane nets (X02.1).
//!
//! [`PlaneComponents::build`] reads a board once:
//! - for each filled plane (a conduction area of a net that belongs to no
//!   component), in ascending [`ItemId`], its islands in the order of
//!   Java's detailed fill ([`conduction_fill`], `ConductionArea.
//!   getDetailedFillArea` as the DRC computes it; the only fill source, with
//!   no fallback that depends on the environment);
//! - each island's anchors: the pins and vias of the plane's net whose drill
//!   centre lies on the island, ascending `ItemId` (the fill model's anchors
//!   without the traces);
//! - the main island of each plane and the main component of each plane net
//!   on each layer: the island with the most anchors, ties by the lowest
//!   anchor `ItemId` inside it; an island without anchors is never main;
//! - the electrical groups: Freerouting's connectivity treats a plane as one
//!   conductor, so every item touching it looks connected. Here a plane is
//!   replaced by its islands. The net's items fall into what connects
//!   without planes ([`net_components`] with `stop_at_plane`), and each
//!   island joins the components of its anchors (a union-find whose root is
//!   always the lowest node, so the groups do not depend on union order).
//!   Two islands are joined when they are in one group: through a via to
//!   another layer's island, a trace, or a strap.
//!
//! The idea of a main plane component and of fill-aware connectivity is
//! KiCadRoutingTools' `plane_component_oracle.py` (at `0486e799`); no code is
//! ported.
//!
//! Orders that decide results (D2): planes ascending by id, islands in fill
//! order, anchors ascending by id, nets ascending; no hash order is read.
//! "Ascending by id" is the id number's order (`ItemId`'s own `Ord` is
//! Java's `TreeSet<Item>` order, highest first).

use std::collections::{BTreeMap, BTreeSet};

use freerouting::board::BasicBoard;
use freerouting::board::items::{Item, ItemId};
use freerouting::drc::{Island, conduction_fill, net_components};
use freerouting::geometry::IntPoint;

/// One island of a plane's fill.
#[derive(Clone, Debug, PartialEq)]
pub struct PlaneIsland {
    /// The plane (conduction area) the island belongs to.
    pub area: ItemId,
    /// The island's index in the plane's fill order.
    pub index_in_area: usize,
    /// The plane's net.
    pub net: i32,
    /// The plane's layer.
    pub layer: i32,
    /// The pins and vias of the net whose drill centre lies on the island,
    /// ascending.
    pub anchors: Vec<ItemId>,
    /// The island as the DRC computes it.
    pub island: Island,
}

/// One filled plane and its islands.
#[derive(Clone, Debug, PartialEq)]
pub struct PlaneArea {
    /// The conduction area.
    pub area: ItemId,
    /// Its net.
    pub net: i32,
    /// Its layer.
    pub layer: i32,
    /// Its islands: indices into [`PlaneComponents::islands`], in fill order.
    pub islands: std::ops::Range<usize>,
    /// The island with the most anchors (ties by the lowest anchor id);
    /// `None` when no island has an anchor.
    pub main: Option<usize>,
}

/// The islands of one plane net on one layer that are joined: one strap
/// target for region repair.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Region {
    /// The islands, ascending index.
    pub islands: Vec<usize>,
    /// Their anchors, ascending id.
    pub anchors: Vec<ItemId>,
}

/// The islands, anchors, main components and electrical groups of every
/// plane net of a board (module docs).
#[derive(Clone, Debug)]
pub struct PlaneComponents {
    islands: Vec<PlaneIsland>,
    areas: Vec<PlaneArea>,
    /// The component (of [`net_components`] with `stop_at_plane`) of each
    /// connectable item of a plane net.
    component_of: BTreeMap<ItemId, usize>,
    /// The members of each component, ascending.
    members: Vec<Vec<ItemId>>,
    component_count: usize,
    groups: UnionFind,
    main: BTreeMap<(i32, i32), usize>,
    centres: BTreeMap<ItemId, IntPoint>,
}

impl PlaneComponents {
    /// Reads `board` (module docs).
    pub fn build(board: &BasicBoard) -> PlaneComponents {
        let mut area_ids = board.conduction_areas();
        area_ids.sort_by_key(|i| i.0);
        let mut islands = Vec::new();
        let mut areas = Vec::new();
        let mut centres = BTreeMap::new();
        for id in area_ids {
            let Some((net, layer)) = board.item(id).and_then(plane_net_and_layer) else {
                continue;
            };
            let fill = conduction_fill(board, id);
            if fill.is_empty() {
                continue;
            }
            let first = islands.len();
            for (index_in_area, f) in fill.into_iter().enumerate() {
                let mut anchors: Vec<ItemId> = f
                    .anchors
                    .iter()
                    .copied()
                    .filter(|a| board.item(*a).is_some_and(|i| i.is_pin() || i.is_via()))
                    .collect();
                anchors.sort_by_key(|i| i.0);
                for a in &anchors {
                    if let Some(drill) = board.item(*a).and_then(Item::drill) {
                        centres.insert(*a, drill.center);
                    }
                }
                islands.push(PlaneIsland {
                    area: id,
                    index_in_area,
                    net,
                    layer,
                    anchors,
                    island: f.island,
                });
            }
            let range = first..islands.len();
            let main = main_of(&islands, range.clone());
            areas.push(PlaneArea {
                area: id,
                net,
                layer,
                islands: range,
                main,
            });
        }

        let nets: BTreeSet<i32> = areas.iter().map(|a| a.net).collect();
        let mut component_of = BTreeMap::new();
        let mut members = Vec::new();
        for net in nets {
            for mut component in net_components(board, net, true) {
                component.sort_by_key(|i| i.0);
                for item in &component {
                    component_of.insert(*item, members.len());
                }
                members.push(component);
            }
        }
        let component_count = members.len();
        let mut groups = UnionFind::new(component_count + islands.len());
        for (k, island) in islands.iter().enumerate() {
            for a in &island.anchors {
                if let Some(&c) = component_of.get(a) {
                    groups.union(component_count + k, c);
                }
            }
        }

        let mut by_net_layer: BTreeMap<(i32, i32), Vec<usize>> = BTreeMap::new();
        for (k, island) in islands.iter().enumerate() {
            by_net_layer
                .entry((island.net, island.layer))
                .or_default()
                .push(k);
        }
        let main = by_net_layer
            .into_iter()
            .filter_map(|(key, ks)| best_island(&islands, ks).map(|m| (key, m)))
            .collect();

        PlaneComponents {
            islands,
            areas,
            component_of,
            members,
            component_count,
            groups,
            main,
            centres,
        }
    }

    /// Every island, planes ascending by id, each plane's islands in fill
    /// order.
    pub fn islands(&self) -> &[PlaneIsland] {
        &self.islands
    }

    /// Every filled plane, ascending id.
    pub fn areas(&self) -> &[PlaneArea] {
        &self.areas
    }

    /// The main island of plane `area`.
    pub fn main_island(&self, area: ItemId) -> Option<usize> {
        self.areas.iter().find(|a| a.area == area)?.main
    }

    /// The main component of net `net` on layer `layer`: of all the net's
    /// islands on the layer, the one with the most anchors, ties by the
    /// lowest anchor id.
    pub fn main_of(&self, net: i32, layer: i32) -> Option<usize> {
        self.main.get(&(net, layer)).copied()
    }

    /// The plane nets, ascending.
    pub fn nets(&self) -> Vec<i32> {
        self.areas
            .iter()
            .map(|a| a.net)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// The layers with a filled plane of net `net`, ascending.
    pub fn layers_of(&self, net: i32) -> Vec<i32> {
        self.areas
            .iter()
            .filter(|a| a.net == net)
            .map(|a| a.layer)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// The islands of net `net` on layer `layer`, ascending index.
    pub fn islands_on(&self, net: i32, layer: i32) -> Vec<usize> {
        (0..self.islands.len())
            .filter(|k| self.islands[*k].net == net && self.islands[*k].layer == layer)
            .collect()
    }

    /// The islands `item` anchors, ascending index.
    pub fn islands_of_anchor(&self, item: ItemId) -> Vec<usize> {
        (0..self.islands.len())
            .filter(|k| {
                self.islands[*k]
                    .anchors
                    .binary_search_by_key(&item.0, |a| a.0)
                    .is_ok()
            })
            .collect()
    }

    /// The items connected to `item` without passing through a plane
    /// (its [`net_components`] component), or `None` for an item of no
    /// plane net.
    pub fn component_items(&self, item: ItemId) -> Option<&[ItemId]> {
        let c = *self.component_of.get(&item)?;
        Some(&self.members[c])
    }

    /// The electrical group of `item`, or `None` for an item of no plane
    /// net. Groups are opaque numbers: equal means connected.
    pub fn group_of_item(&self, item: ItemId) -> Option<usize> {
        let c = *self.component_of.get(&item)?;
        Some(self.groups.find(c))
    }

    /// The electrical group of island `k`.
    pub fn group_of_island(&self, k: usize) -> usize {
        self.groups.find(self.component_count + k)
    }

    /// True if islands `a` and `b` are connected (one group).
    pub fn joined(&self, a: usize, b: usize) -> bool {
        self.group_of_island(a) == self.group_of_island(b)
    }

    /// The drill centre of anchor `item`.
    pub fn anchor_centre(&self, item: ItemId) -> Option<IntPoint> {
        self.centres.get(&item).copied()
    }

    /// The regions of net `net` on layer `layer`: its islands with anchors,
    /// grouped by electrical group, each region's islands and anchors
    /// ascending, the regions ordered by their lowest anchor id. Islands
    /// without anchors (dead copper) have nothing to join and are left out.
    pub fn regions(&self, net: i32, layer: i32) -> Vec<Region> {
        let mut by_group: BTreeMap<usize, Region> = BTreeMap::new();
        for k in self.islands_on(net, layer) {
            if self.islands[k].anchors.is_empty() {
                continue;
            }
            let region = by_group
                .entry(self.group_of_island(k))
                .or_insert_with(|| Region {
                    islands: Vec::new(),
                    anchors: Vec::new(),
                });
            region.islands.push(k);
            region.anchors.extend(&self.islands[k].anchors);
        }
        let mut regions: Vec<Region> = by_group
            .into_values()
            .map(|mut r| {
                r.anchors.sort_by_key(|i| i.0);
                r.anchors.dedup();
                r
            })
            .collect();
        regions.sort_by_key(|r| r.anchors[0].0);
        regions
    }
}

/// The net and layer of `item` if it is a conduction area of a net that
/// belongs to no component (a plane).
fn plane_net_and_layer(item: &Item) -> Option<(i32, i32)> {
    if !item.is_conduction_area() || item.component_id() > 0 || item.net_count() == 0 {
        return None;
    }
    let net = item.net_number(0);
    (net > 0).then(|| (net, item.first_layer()))
}

/// The main island among `range` (module docs).
fn main_of(islands: &[PlaneIsland], range: std::ops::Range<usize>) -> Option<usize> {
    best_island(islands, range.collect())
}

/// The island of `ks` with the most anchors, ties by the lowest anchor id;
/// `None` when none has an anchor.
fn best_island(islands: &[PlaneIsland], ks: Vec<usize>) -> Option<usize> {
    ks.into_iter()
        .filter(|k| !islands[*k].anchors.is_empty())
        .min_by(|a, b| {
            let (a, b) = (&islands[*a], &islands[*b]);
            b.anchors
                .len()
                .cmp(&a.anchors.len())
                .then(a.anchors[0].0.cmp(&b.anchors[0].0))
        })
}

/// A union-find whose root is always the lowest node of its set.
#[derive(Clone, Debug)]
struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> UnionFind {
        UnionFind {
            parent: (0..n).collect(),
        }
    }

    fn find(&self, mut x: usize) -> usize {
        while self.parent[x] != x {
            x = self.parent[x];
        }
        x
    }

    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            let (low, high) = (ra.min(rb), ra.max(rb));
            self.parent[high] = low;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planes::test_boards::{Pad, board};

    const CUT: Option<(i32, i32, i32, i32)> = Some((20000, 2000, 20000, 28000));

    fn pin_id(b: &BasicBoard, component: &str) -> ItemId {
        b.items()
            .filter(|i| i.is_pin())
            .find(|i| {
                b.components
                    .get(freerouting::board::structure::ComponentId(i.component_id()))
                    .is_some_and(|c| c.name == component)
            })
            .map(Item::id)
            .unwrap_or_else(|| panic!("pin {component}"))
    }

    #[test]
    fn a_pour_cut_by_a_foreign_trace_has_two_islands_with_their_anchors() {
        let pads = [
            Pad::gnd("G1", 13000, 10000),
            Pad::gnd("G2", 13000, 20000),
            Pad::gnd("G3", 27000, 15000),
        ];
        let b = board(&pads, CUT);
        let pc = PlaneComponents::build(&b);
        assert_eq!(pc.areas().len(), 1);
        assert_eq!(pc.islands().len(), 2);
        let (g1, g2, g3) = (pin_id(&b, "G1"), pin_id(&b, "G2"), pin_id(&b, "G3"));
        let mut left = vec![g1, g2];
        left.sort_by_key(|i| i.0);
        // fill order: lowest y then lowest x of the island; both start at
        // the pour's bottom edge, so the left one comes first.
        assert_eq!(pc.islands()[0].anchors, left);
        assert_eq!(pc.islands()[1].anchors, vec![g3]);
        let area = pc.areas()[0].area;
        assert_eq!(pc.main_island(area), Some(0));
        let net = pc.areas()[0].net;
        assert_eq!(pc.main_of(net, pc.areas()[0].layer), Some(0));
        assert!(!pc.joined(0, 1));
        assert_eq!(pc.group_of_item(g1), pc.group_of_item(g2));
        assert_ne!(pc.group_of_item(g1), pc.group_of_item(g3));
        let regions = pc.regions(net, pc.areas()[0].layer);
        assert_eq!(regions.len(), 2);
        assert_eq!(regions[0].anchors[0].0, left[0].0.min(g3.0));
    }

    #[test]
    fn the_main_island_is_the_one_with_more_anchors() {
        // Two anchors on the right, one on the left.
        let pads = [
            Pad::gnd("G1", 13000, 15000),
            Pad::gnd("G2", 27000, 10000),
            Pad::gnd("G3", 27000, 20000),
        ];
        let b = board(&pads, CUT);
        let pc = PlaneComponents::build(&b);
        assert_eq!(pc.islands().len(), 2);
        assert_eq!(pc.islands()[1].anchors.len(), 2);
        assert_eq!(pc.main_island(pc.areas()[0].area), Some(1));
    }

    #[test]
    fn a_tie_picks_the_island_with_the_lower_anchor_id() {
        // One anchor each; placed in both orders, the main island is always
        // the one holding the lower pin id, wherever it lies.
        for pads in [
            [Pad::gnd("G1", 13000, 15000), Pad::gnd("G2", 27000, 15000)],
            [Pad::gnd("G1", 27000, 15000), Pad::gnd("G2", 13000, 15000)],
        ] {
            let b = board(&pads, CUT);
            let pc = PlaneComponents::build(&b);
            assert_eq!(pc.islands().len(), 2);
            let (g1, g2) = (pin_id(&b, "G1"), pin_id(&b, "G2"));
            let lowest = if g1.0 < g2.0 { g1 } else { g2 };
            let main = pc.main_island(pc.areas()[0].area).expect("a main island");
            assert_eq!(pc.islands()[main].anchors, vec![lowest]);
        }
    }

    #[test]
    fn an_uncut_pour_is_one_island_holding_every_anchor() {
        let pads = [Pad::gnd("G1", 13000, 15000), Pad::gnd("G2", 27000, 15000)];
        let b = board(&pads, None);
        let pc = PlaneComponents::build(&b);
        assert_eq!(pc.islands().len(), 1);
        assert_eq!(pc.islands()[0].anchors.len(), 2);
        assert_eq!(pc.regions(pc.areas()[0].net, pc.areas()[0].layer).len(), 1);
    }

    #[test]
    fn a_trace_between_the_halves_joins_their_islands() {
        // The two halves stay two fill islands (Java's fill ignores
        // same-net traces) but are one group once a GND trace on the other
        // layer connects their pins.
        let pads = [Pad::gnd("G1", 13000, 15000), Pad::gnd("G2", 27000, 15000)];
        let b = crate::planes::test_boards::board_with_wires(
            &pads,
            CUT,
            &[("GND", "F.Cu", (13000, 15000, 27000, 15000))],
        );
        let pc = PlaneComponents::build(&b);
        assert_eq!(pc.islands().len(), 2);
        assert!(pc.joined(0, 1));
        assert_eq!(pc.regions(pc.areas()[0].net, pc.areas()[0].layer).len(), 1);
    }
}
