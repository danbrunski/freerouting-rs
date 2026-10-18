//! The fanout's ordering and stop rules (A10.6), pure helpers for A12's
//! fanout stage, at `a773a70ec`.
//!
//! - [`fanout_components`]: `BatchFanout`'s constructor and its private
//!   `Component` (`BatchFanout.java:43-90`, `:698-847`): the SMD pins with
//!   a net and no ignored net, grouped by component (component ids `1..=n`),
//!   each component's centre of gravity summed in pin-list order, the
//!   components ordered by SMD pin count descending then component id, the
//!   pins by [`PinSortingOrder`] with the pin index as the tie-break.
//! - [`retry_fanout`]: `BatchFanout.retryFanout` (`:573-575`).
//! - [`FanoutPassTracker`]: the stop rules of the fanout pass loop
//!   (`fanoutBoard`, `:111-165`).
//!
//! The pin list is Java's `getSmdPins()`, in item-list order (highest id
//! first); `distanceToClosestOnNet` scans `getPins()` in the same order.

use std::cmp::Ordering;

use freerouting_geometry::FloatPoint;

use crate::board::basic_board::BasicBoard;
use crate::board::items::{Item, ItemId};
use crate::board::structure::Unit;

/// The radius of `surroundingsDensity` in micrometers (20 mm,
/// `BatchFanout.java:797`).
pub const SURROUNDINGS_RADIUS_UM: f64 = 20000.0;

/// Consecutive identical pass outcomes that stop the fanout
/// (`stagnationPassLimit`, `:113`).
pub const FANOUT_STAGNATION_PASS_LIMIT: u32 = 3;

/// The pin order inside a component (`router.fanout.pin_sorting_order`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinSortingOrder {
    /// `outer_first` (the default): farthest from the centre first.
    OuterFirst,
    /// `inner_first`: nearest to the centre first.
    InnerFirst,
    /// `distanceToClosestOnNet`: nearest other pin of the net first.
    DistanceToClosestOnNet,
    /// `surroundingsDensity`: most SMD pins within 20 mm first.
    SurroundingsDensity,
    /// Any other name: pin index only. PARITY: `BatchFanout.java:811-846`
    /// compares the exact strings, so the documented spellings
    /// `distance_to_closest_on_net` and `surroundings_density` land here
    /// (A00 open question 7: kept through rung 3);
    /// `router.java_quirks.fanout_pin_order_names = false` accepts them (N06.6).
    PinIndex,
}

impl PinSortingOrder {
    /// The order Java's comparator picks for `name`.
    pub fn from_name(name: &str) -> PinSortingOrder {
        Self::from_name_with(name, true)
    }

    /// [`Self::from_name`] behind `router.java_quirks.fanout_pin_order_names`
    /// (N06): `true` is Java's exact strings, `false` also accepts the
    /// documented spellings `distance_to_closest_on_net` and
    /// `surroundings_density`.
    pub fn from_name_with(name: &str, java_quirk: bool) -> PinSortingOrder {
        match name {
            "outer_first" => PinSortingOrder::OuterFirst,
            "inner_first" => PinSortingOrder::InnerFirst,
            "distanceToClosestOnNet" => PinSortingOrder::DistanceToClosestOnNet,
            "surroundingsDensity" => PinSortingOrder::SurroundingsDensity,
            // PARITY(quirk, java_quirks.fanout_pin_order_names): BatchFanout.java:811-846.
            "distance_to_closest_on_net" if !java_quirk => PinSortingOrder::DistanceToClosestOnNet,
            "surroundings_density" if !java_quirk => PinSortingOrder::SurroundingsDensity,
            _ => PinSortingOrder::PinIndex,
        }
    }
}

/// One SMD pin of a [`FanoutComponent`] with what its comparator reads.
#[derive(Clone, Debug, PartialEq)]
pub struct FanoutPin {
    /// The pin.
    pub pin: ItemId,
    /// Its index in the package.
    pub pin_index: usize,
    /// Distance from the component's centre of gravity.
    pub distance_to_center: f64,
    /// Distance to the nearest other pin of its first net
    /// (`Double.MAX_VALUE` for none).
    pub distance_to_closest_on_net: f64,
    /// Other SMD pins with a net within 20 mm.
    pub surroundings_density: i32,
}

/// A component with SMD pins, as the fanout visits it.
#[derive(Clone, Debug, PartialEq)]
pub struct FanoutComponent {
    /// The component id.
    pub component: i32,
    /// The centre of gravity of its SMD pins.
    pub gravity_center: FloatPoint,
    /// Its SMD pins in fanout order.
    pub pins: Vec<FanoutPin>,
}

impl FanoutComponent {
    /// The number of SMD pins.
    pub fn smd_pin_count(&self) -> usize {
        self.pins.len()
    }
}

fn center(item: &Item) -> FloatPoint {
    item.drill()
        .map(|drill| drill.center.to_float())
        .expect("a pin has drill data")
}

/// Java's `double` comparator pattern `delta > 0 ? 1 : delta < 0 ? -1 : 0`
/// on `a - b`.
fn delta_order(delta: f64) -> Ordering {
    if delta > 0.0 {
        Ordering::Greater
    } else if delta < 0.0 {
        Ordering::Less
    } else {
        Ordering::Equal
    }
}

/// `Component.Pin.compareTo` (`:811-846`).
fn compare_pins(order: PinSortingOrder, a: &FanoutPin, b: &FanoutPin) -> Ordering {
    let result = match order {
        PinSortingOrder::InnerFirst => delta_order(a.distance_to_center - b.distance_to_center),
        PinSortingOrder::OuterFirst => {
            delta_order(a.distance_to_center - b.distance_to_center).reverse()
        }
        PinSortingOrder::DistanceToClosestOnNet => {
            delta_order(a.distance_to_closest_on_net - b.distance_to_closest_on_net)
        }
        // PARITY: :835, `other - this`, densest first.
        PinSortingOrder::SurroundingsDensity => {
            (b.surroundings_density - a.surroundings_density).cmp(&0)
        }
        PinSortingOrder::PinIndex => Ordering::Equal,
    };
    result.then(a.pin_index.cmp(&b.pin_index))
}

/// The components the fanout visits, in its order, with their pins in
/// `order` (Java `BatchFanout`'s `sortedComponents`).
pub fn fanout_components(board: &BasicBoard, order: PinSortingOrder) -> Vec<FanoutComponent> {
    // PARITY: BatchFanout.java:50-58, getSmdPins in item-list order.
    let smd: Vec<&Item> = board
        .smd_pins()
        .into_iter()
        .filter_map(|id| board.item(id))
        .filter(|pin| pin.net_count() > 0 && !board.has_ignored_nets(pin))
        .collect();
    let all_pins: Vec<&Item> = board
        .pins()
        .into_iter()
        .filter_map(|id| board.item(id))
        .collect();
    let max_dist = SURROUNDINGS_RADIUS_UM * board.communication.get_resolution(Unit::Um);
    let mut components = Vec::new();
    for component_id in 1..=board.components.count() as i32 {
        let pins: Vec<&Item> = smd
            .iter()
            .copied()
            .filter(|pin| pin.component_id() == component_id)
            .collect();
        if pins.is_empty() {
            continue;
        }
        // PARITY: :727-739, summed in pin-list order, then divided.
        let (mut x, mut y) = (0.0, 0.0);
        for pin in &pins {
            let c = center(pin);
            x += c.x;
            y += c.y;
        }
        let n = pins.len() as f64;
        let gravity_center = FloatPoint::new(x / n, y / n);
        let mut fanout_pins: Vec<FanoutPin> = pins
            .iter()
            .map(|pin| fanout_pin(pin, gravity_center, &all_pins, &smd, max_dist))
            .collect();
        fanout_pins.sort_by(|a, b| compare_pins(order, a, b));
        components.push(FanoutComponent {
            component: component_id,
            gravity_center,
            pins: fanout_pins,
        });
    }
    // PARITY: :751-765, more SMD pins first, then component id.
    components.sort_by(|a, b| {
        b.smd_pin_count()
            .cmp(&a.smd_pin_count())
            .then(a.component.cmp(&b.component))
    });
    components
}

/// `Component.Pin`'s constructor (`:771-809`).
fn fanout_pin(
    pin: &Item,
    gravity_center: FloatPoint,
    all_pins: &[&Item],
    smd: &[&Item],
    max_dist: f64,
) -> FanoutPin {
    let location = center(pin);
    let net = pin.net_numbers().first().copied().unwrap_or(0);
    let mut closest = f64::MAX;
    if net > 0 {
        for other in all_pins {
            if other.id() != pin.id() && other.contains_net(net) {
                let dist = location.distance(center(other));
                if dist < closest {
                    closest = dist;
                }
            }
        }
    }
    let density = smd
        .iter()
        .filter(|other| other.id() != pin.id() && location.distance(center(other)) <= max_dist)
        .count() as i32;
    FanoutPin {
        pin: pin.id(),
        pin_index: pin.as_pin().map_or(0, |p| p.pin_index),
        distance_to_center: location.distance(gravity_center),
        distance_to_closest_on_net: closest,
        surroundings_density: density,
    }
}

/// Java `BatchFanout.retryFanout`: a pin that failed is retried once its
/// component escaped something else since (its generation changed).
pub fn retry_fanout(failed_generation: Option<i32>, current_generation: i32) -> bool {
    failed_generation != Some(current_generation)
}

/// Why the fanout pass loop stops.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FanoutStop {
    /// The fanout deadline passed (`fanout.timeout`, a stop only).
    Deadline,
    /// `fanout.max_items` pins were fanned out.
    MaxItems,
    /// A pass routed no pin.
    NoPinRouted,
    /// The pass outcome `(routed << 32) ^ via_count` repeated
    /// [`FANOUT_STAGNATION_PASS_LIMIT`] times in a row.
    NoProgress,
    /// The board hash did not change over a pass.
    BoardUnchanged,
}

/// The stop rules of `BatchFanout.fanoutBoard`'s pass loop. The driver
/// runs at most `fanout.max_passes` passes and asks the tracker before and
/// after each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FanoutPassTracker {
    /// `previousBoardState`, `Long.MIN_VALUE` at the start.
    previous_state: i64,
    /// `identicalPasses`.
    identical_passes: u32,
    /// `lastBoardHash`.
    last_hash: u128,
}

impl FanoutPassTracker {
    /// A tracker for a board whose hash is `board_hash` before the first
    /// pass.
    pub fn new(board_hash: u128) -> Self {
        FanoutPassTracker {
            previous_state: i64::MIN,
            identical_passes: 0,
            last_hash: board_hash,
        }
    }

    /// Before a pass (`:118-131`): `deadline_passed` is the caller's clock
    /// check, `fanned_out` the pins fanned out so far and `max_items`
    /// `fanout.max_items`.
    pub fn before_pass(
        &self,
        deadline_passed: bool,
        fanned_out: i32,
        max_items: i32,
    ) -> Option<FanoutStop> {
        if deadline_passed {
            return Some(FanoutStop::Deadline);
        }
        if max_items > 0 && fanned_out >= max_items {
            return Some(FanoutStop::MaxItems);
        }
        None
    }

    /// After a pass (`:132-165`) that routed `routed` pins and left
    /// `via_count` vias; `timed_out` is the fanout's timed-out flag after
    /// the pass and `board_hash` the board's hash.
    pub fn after_pass(
        &mut self,
        routed: i32,
        via_count: usize,
        timed_out: bool,
        board_hash: u128,
    ) -> Option<FanoutStop> {
        if routed == 0 {
            return Some(FanoutStop::NoPinRouted);
        }
        // PARITY: :141, `((long) routedCount << 32) ^ board.getVias().size()`.
        let state = (i64::from(routed) << 32) ^ i64::from(via_count as i32);
        if state == self.previous_state {
            self.identical_passes += 1;
            if self.identical_passes >= FANOUT_STAGNATION_PASS_LIMIT {
                return Some(FanoutStop::NoProgress);
            }
        } else {
            self.identical_passes = 0;
            self.previous_state = state;
        }
        if timed_out {
            return Some(FanoutStop::Deadline);
        }
        if board_hash == self.last_hash {
            return Some(FanoutStop::BoardUnchanged);
        }
        self.last_hash = board_hash;
        None
    }
}
