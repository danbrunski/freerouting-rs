//! Region classes of a board's connections (X06.2): `pin-field` where an
//! end pad is fine-pitch, `open` everywhere else. X06.3 split its
//! measurement of the (since removed) `krt-astar` bundle by these classes,
//! to test the preregistered expectation that the grid router loses in pin
//! fields.
//!
//! Written from the X06 checklist's rule, which takes KiCadRoutingTools'
//! fine-pitch test (`pad_is_fine_pitch` in `py_router/plane_pad_tap.py`,
//! thresholds in `routing_defaults.py`, at `0486e799`) as its idea; no KRT
//! code is ported.
//!
//! **The rule**, in integer board units ([`PitchRule::for_board`]): a pin
//! is `pin-field` when
//! - another pin of its component lies within 0.65 mm centre to centre (and
//!   not at the same centre), or
//! - the smaller side of its pad's bounding box (on the pin's first layer)
//!   is under 0.35 mm, or
//! - its component has at least 32 pins and a pitch of at most 0.8 mm,
//!   where the pitch is the smallest centre-to-centre distance between two
//!   of its pins at distinct centres.
//!
//! Both distance limits carry KRT's 1 µm tolerance (`_DIST_TOL`, "so an
//! exact 0.65 mm pitch qualifies"), so a pad lattice that rotation rounded
//! by a board unit still counts. Distances are compared squared in `i128`,
//! exact for any `i32` coordinates (a squared `i32` difference alone can
//! pass `i64::MAX`).
//!
//! **Connections** are the input board's incomplete connections (the
//! ratsnest's airlines, [`DesignRulesChecker::all_airlines`]), one per
//! airline, keyed by `(net, lower end item, higher end item)`. The pads of
//! an end are the end item itself when it is a pin, the pins of its
//! connected set (not passing planes) when it is a trace or a via (a
//! pre-routed stub), and none when it is a plane or another area. A
//! connection is `pin-field` when a pad of either end is.
//!
//! **Completion** ([`Connection::is_complete`]): on a routed board, a
//! connection is complete when an anchor of one end shares a connected set
//! with an anchor of the other. The anchors of an end are its pads plus the
//! end item itself if it is not a trace or via (traces and vias may be
//! ripped and replaced); an end with no anchor left on the board makes the
//! completion unknown (`None`). Summed over a net, the incomplete
//! connections can exceed the net's final airline count (two input
//! connections may share one remaining gap), so a report gives both.
//!
//! Determinism (D2): every collection is ordered; nothing depends on hash
//! order or a clock.

use std::collections::{BTreeMap, BTreeSet};

use freerouting::board::BasicBoard;
use freerouting::board::items::{Item, ItemId};
use freerouting::drc::DesignRulesChecker;
use freerouting::geometry::IntPoint;
use freerouting::job::overrides::um_to_board_units;

/// KRT `FINE_PITCH_NEIGHBOR_DIST`: a pin of the same component this close
/// (centre to centre, µm) makes a pin fine-pitch.
pub const NEIGHBOUR_UM: f64 = 650.0;
/// KRT `FINE_PITCH_MIN_PAD_DIM`: a pad whose smaller side is under this
/// (µm) is fine-pitch.
pub const MIN_PAD_DIM_UM: f64 = 350.0;
/// A component with at least this many pins...
pub const LARGE_COMPONENT_PINS: usize = 32;
/// ...and a pitch of at most this (µm) is a pin field.
pub const LARGE_COMPONENT_PITCH_UM: f64 = 800.0;
/// KRT `_DIST_TOL` (1e-3 mm), added to both distance limits.
pub const DISTANCE_TOLERANCE_UM: f64 = 1.0;

/// The region class of a connection or a pin.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Region {
    /// An end pad is fine-pitch.
    PinField,
    /// Everything else.
    Open,
}

impl Region {
    /// Both classes, in report order.
    pub const ALL: [Region; 2] = [Region::PinField, Region::Open];

    /// The class's name in reports and the preregistration.
    pub fn name(self) -> &'static str {
        match self {
            Region::PinField => "pin-field",
            Region::Open => "open",
        }
    }
}

/// A connection of the input board: one ratsnest airline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConnectionKey {
    /// The net.
    pub net: i32,
    /// The lower id of the two end items.
    pub from: ItemId,
    /// The higher id of the two end items.
    pub to: ItemId,
}

impl ConnectionKey {
    /// The key of an airline of `net` between `a` and `b`.
    pub fn new(net: i32, a: ItemId, b: ItemId) -> Self {
        ConnectionKey {
            net,
            from: a.min(b),
            to: a.max(b),
        }
    }
}

/// The limits of the rule in board units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PitchRule {
    /// The neighbour distance with the tolerance.
    pub neighbour: i64,
    /// The smallest pad side that is not fine-pitch.
    pub min_pad_dim: i64,
    /// The large-component pitch with the tolerance.
    pub large_pitch: i64,
}

impl PitchRule {
    /// The rule's limits in `board`'s units.
    pub fn for_board(board: &BasicBoard) -> Self {
        let units = |um: f64| i64::from(um_to_board_units(board, um));
        PitchRule {
            neighbour: units(NEIGHBOUR_UM + DISTANCE_TOLERANCE_UM),
            min_pad_dim: units(MIN_PAD_DIM_UM),
            large_pitch: units(LARGE_COMPONENT_PITCH_UM + DISTANCE_TOLERANCE_UM),
        }
    }
}

/// One connection of the input board with its class and anchors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Connection {
    /// The key.
    pub key: ConnectionKey,
    /// The class.
    pub region: Region,
    /// The anchors of the `from` end (module docs).
    pub from_anchors: BTreeSet<ItemId>,
    /// The anchors of the `to` end.
    pub to_anchors: BTreeSet<ItemId>,
}

impl Connection {
    /// True if the two ends share a connected set on `board`; `None` if an
    /// end has no anchor left on it.
    pub fn is_complete(&self, board: &BasicBoard) -> Option<bool> {
        let present = |set: &BTreeSet<ItemId>| -> Vec<ItemId> {
            set.iter()
                .copied()
                .filter(|id| board.item(*id).is_some())
                .collect()
        };
        let (from, to) = (present(&self.from_anchors), present(&self.to_anchors));
        if from.is_empty() || to.is_empty() {
            return None;
        }
        let reached = board.connected_set(from[0], self.key.net, false);
        let mut joined: BTreeSet<ItemId> = reached;
        for id in &from[1..] {
            if !joined.contains(id) {
                joined.extend(board.connected_set(*id, self.key.net, false));
            }
        }
        Some(to.iter().any(|id| joined.contains(id)))
    }
}

/// The squared distance of two points, exact for any `i32` coordinates.
fn distance_square(a: IntPoint, b: IntPoint) -> i128 {
    let dx = i128::from(a.x) - i128::from(b.x);
    let dy = i128::from(a.y) - i128::from(b.y);
    dx * dx + dy * dy
}

/// A pin's centre and the smaller side of its first pad shape's bounding
/// box.
fn pin_geometry(item: &Item) -> Option<(IntPoint, i64)> {
    let drill = item.drill()?;
    let side = drill
        .shapes
        .iter()
        .flatten()
        .next()
        .map(|s| {
            let b = s.bounding_box();
            i64::from(b.width().min(b.height()))
        })
        .unwrap_or(i64::MAX);
    Some((drill.center, side))
}

/// The class of every pin of `board`, by item id.
pub fn pin_regions(board: &BasicBoard) -> BTreeMap<ItemId, Region> {
    let rule = PitchRule::for_board(board);
    let mut components: BTreeMap<i32, Vec<(ItemId, IntPoint, i64)>> = BTreeMap::new();
    for item in board.items().filter(|i| i.is_pin()) {
        if let Some((centre, side)) = pin_geometry(item) {
            components
                .entry(item.component_id())
                .or_default()
                .push((item.id(), centre, side));
        }
    }
    let mut out = BTreeMap::new();
    for pins in components.values() {
        out.extend(component_regions(pins, &rule));
    }
    out
}

/// The classes of one component's pins `(id, centre, smaller pad side)`.
pub fn component_regions(
    pins: &[(ItemId, IntPoint, i64)],
    rule: &PitchRule,
) -> Vec<(ItemId, Region)> {
    let neighbour_sq = i128::from(rule.neighbour).pow(2);
    // The nearest distinct-centre neighbour of each pin, squared.
    let nearest: Vec<i128> = pins
        .iter()
        .map(|(_, a, _)| {
            pins.iter()
                .map(|(_, b, _)| distance_square(*a, *b))
                .filter(|d| *d > 0)
                .min()
                .unwrap_or(i128::MAX)
        })
        .collect();
    let pitch_sq = nearest.iter().copied().min().unwrap_or(i128::MAX);
    let large_field =
        pins.len() >= LARGE_COMPONENT_PINS && pitch_sq <= i128::from(rule.large_pitch).pow(2);
    pins.iter()
        .zip(&nearest)
        .map(|(&(id, _, side), &near)| {
            let fine = large_field || near <= neighbour_sq || side < rule.min_pad_dim;
            (id, if fine { Region::PinField } else { Region::Open })
        })
        .collect()
}

/// The pads and anchors of the airline end `id` of `net` (module docs).
fn end_pads(board: &BasicBoard, id: ItemId, net: i32) -> (BTreeSet<ItemId>, BTreeSet<ItemId>) {
    let Some(item) = board.item(id) else {
        return (BTreeSet::new(), BTreeSet::new());
    };
    if item.is_pin() {
        let set = BTreeSet::from([id]);
        return (set.clone(), set);
    }
    if item.is_trace() || item.is_via() {
        let pins: BTreeSet<ItemId> = board
            .connected_set(id, net, true)
            .into_iter()
            .filter(|i| board.item(*i).is_some_and(Item::is_pin))
            .collect();
        let anchors = if pins.is_empty() {
            BTreeSet::from([id])
        } else {
            pins.clone()
        };
        return (pins, anchors);
    }
    (BTreeSet::new(), BTreeSet::from([id]))
}

/// The connections of `board` (its airlines), ascending by key, with
/// their classes and anchors.
pub fn connections(board: &BasicBoard) -> Vec<Connection> {
    let pins = pin_regions(board);
    let mut checker = DesignRulesChecker::new(board);
    let mut out: BTreeMap<ConnectionKey, Connection> = BTreeMap::new();
    for airline in checker.all_airlines() {
        let net = airline.net_number;
        let key = ConnectionKey::new(net, airline.from_item, airline.to_item);
        let (from_pads, from_anchors) = end_pads(board, key.from, net);
        let (to_pads, to_anchors) = end_pads(board, key.to, net);
        let fine = from_pads
            .iter()
            .chain(&to_pads)
            .any(|p| pins.get(p) == Some(&Region::PinField));
        let region = if fine { Region::PinField } else { Region::Open };
        out.entry(key).or_insert(Connection {
            key,
            region,
            from_anchors,
            to_anchors,
        });
    }
    out.into_values().collect()
}

/// The class of every connection of `board` (X06.2's interface).
pub fn classify(board: &BasicBoard) -> BTreeMap<ConnectionKey, Region> {
    connections(board)
        .into_iter()
        .map(|c| (c.key, c.region))
        .collect()
}

/// Connections per class on a routed board: how many there were and how
/// many are still incomplete or unknown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RegionTally {
    /// Connections of the class on the input board.
    pub total: usize,
    /// Of them, still incomplete on the routed board.
    pub incomplete: usize,
    /// Of them, with an end that has no anchor left (not counted above).
    pub unknown: usize,
}

/// The tally of `connections` (from the input board) on `routed`, per
/// class in [`Region::ALL`] order.
pub fn tally(connections: &[Connection], routed: &BasicBoard) -> BTreeMap<Region, RegionTally> {
    let mut out: BTreeMap<Region, RegionTally> = Region::ALL
        .iter()
        .map(|r| (*r, RegionTally::default()))
        .collect();
    for c in connections {
        let t = out.entry(c.region).or_default();
        t.total += 1;
        match c.is_complete(routed) {
            Some(true) => {}
            Some(false) => t.incomplete += 1,
            None => t.unknown += 1,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule() -> PitchRule {
        // 10 board units per µm.
        PitchRule {
            neighbour: 6510,
            min_pad_dim: 3500,
            large_pitch: 8010,
        }
    }

    fn row(count: i32, pitch: i32, side: i64) -> Vec<(ItemId, IntPoint, i64)> {
        (0..count)
            .map(|i| (ItemId(i + 1), IntPoint::new(i * pitch, 0), side))
            .collect()
    }

    #[test]
    fn an_exact_0_65_mm_pitch_is_a_pin_field_and_0_66_is_not() {
        let at = |pitch| component_regions(&row(4, pitch, 6000), &rule());
        assert!(at(6500).iter().all(|(_, r)| *r == Region::PinField));
        // The 1 µm tolerance.
        assert!(at(6510).iter().all(|(_, r)| *r == Region::PinField));
        assert!(at(6600).iter().all(|(_, r)| *r == Region::Open));
    }

    #[test]
    fn a_narrow_pad_alone_is_fine_pitch() {
        let mut pins = row(2, 50_000, 6000);
        pins[1].2 = 3499;
        let regions = component_regions(&pins, &rule());
        assert_eq!(regions[0].1, Region::Open);
        assert_eq!(regions[1].1, Region::PinField);
        pins[1].2 = 3500;
        assert_eq!(component_regions(&pins, &rule())[1].1, Region::Open);
    }

    #[test]
    fn a_large_component_at_0_8_mm_is_a_pin_field() {
        let at = |count, pitch| component_regions(&row(count, pitch, 6000), &rule());
        assert!(at(32, 8000).iter().all(|(_, r)| *r == Region::PinField));
        assert!(at(31, 8000).iter().all(|(_, r)| *r == Region::Open));
        assert!(at(32, 8100).iter().all(|(_, r)| *r == Region::Open));
    }

    #[test]
    fn pins_at_one_centre_are_not_neighbours() {
        let pins = vec![
            (ItemId(1), IntPoint::new(0, 0), 6000),
            (ItemId(2), IntPoint::new(0, 0), 6000),
        ];
        assert!(
            component_regions(&pins, &rule())
                .iter()
                .all(|(_, r)| *r == Region::Open)
        );
    }

    #[test]
    fn distances_are_exact_at_the_coordinate_limits() {
        let span = i128::from(i32::MAX) - i128::from(i32::MIN);
        let a = IntPoint::new(i32::MIN, i32::MIN);
        let b = IntPoint::new(i32::MAX, i32::MAX);
        assert_eq!(distance_square(a, b), 2 * span * span);
        // Two pins at the far corners are not neighbours; no overflow.
        let pins = vec![(ItemId(1), a, 6000), (ItemId(2), b, 6000)];
        assert!(
            component_regions(&pins, &rule())
                .iter()
                .all(|(_, r)| *r == Region::Open)
        );
    }
}
