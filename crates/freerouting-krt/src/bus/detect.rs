// SPDX-License-Identifier: GPL-3.0-only AND MIT
// SPDX-FileCopyrightText: 2026 drandyhaas (KiCadRoutingTools)
// Portions ported from KiCadRoutingTools py_router/bus_detection.py
// (detect_bus_groups, _find_largest_clique, _order_nets_by_position) at 0486e799.

//! Bus detection (X11.1): groups of two-pin nets that run side by side
//! between the same two places.
//!
//! KRT's `detect_bus_groups` finds cliques of nets whose source (or target)
//! endpoints lie pairwise within `--bus-detection-radius`, then sub-clusters
//! each clique by its other endpoints so a group hangs together at both
//! ends. The X11 decision makes both ends part of the clique test and does
//! it in integers:
//! - **Connections.** A net qualifies when it has exactly two pins, each on
//!   that net only, no plane, and is not ignored by the autorouter. Its
//!   ends are the two pin centres, the lower item id first. Multi-pin nets
//!   are not bus members (KRT routes their main edge as a member; not
//!   ported).
//! - **Neighbours.** Two connections are bus neighbours when their start
//!   pins are within the radius (5 mm, squared distance in `i128`) and so
//!   are their end pins, after orienting the second to the first: directly
//!   (start to start) when that holds, else crossed (start to end).
//! - **Groups** are maximal cliques found greedily in net-number order:
//!   each remaining net in ascending order seeds a group, every other
//!   remaining net in ascending order joins when, oriented to the seed, it
//!   is a neighbour of every member. A group of at least
//!   [`BusParams::min_members`] (3) nets is kept and its nets leave the
//!   pool; a smaller one is dropped and only its seed stops seeding.
//!   KRT's `remaining = set(...)` iteration and its closest-first sort with
//!   distance ties are not copied (X00 rule 6): `BTreeSet`s and net numbers
//!   decide everything.
//! - **Physical order** (KRT `_order_nets_by_position`, along the dominant
//!   axis): members sorted by their start pin's position along the line
//!   through the two start pins farthest apart (first pair in member order
//!   on a tie), oriented to point right (or up when vertical), then by net
//!   number. [`super::order`] routes the middle member first from it.

use std::collections::BTreeSet;

use freerouting::board::BasicBoard;
use freerouting::board::items::{Item, ItemId};
use freerouting::geometry::IntPoint;

/// The detection radius in µm (KRT `--bus-detection-radius 5.0`).
pub const DETECTION_RADIUS_UM: f64 = 5000.0;

/// The smallest group kept (the X11 decision; KRT's `--bus-min-nets` is 2,
/// its strict geometric filter 3).
pub const MIN_MEMBERS: usize = 3;

/// The detection parameters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BusParams {
    /// The largest distance between two members' start (or end) pins, in
    /// board units.
    pub radius: i64,
    /// The smallest group kept.
    pub min_members: usize,
}

impl BusParams {
    /// The X11 defaults on `board`: 5 mm, 3 members.
    pub fn for_board(board: &BasicBoard) -> BusParams {
        BusParams {
            radius: i64::from(freerouting::job::overrides::um_to_board_units(
                board,
                DETECTION_RADIUS_UM,
            )),
            min_members: MIN_MEMBERS,
        }
    }
}

/// A two-pin connection: its net and its two pins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Connection {
    /// The net number.
    pub net: i32,
    /// The start pin (as oriented in its group; the lower id otherwise).
    pub start_pin: ItemId,
    /// The start pin's centre.
    pub start: IntPoint,
    /// The end pin.
    pub end_pin: ItemId,
    /// The end pin's centre.
    pub end: IntPoint,
}

impl Connection {
    /// The connection with its ends swapped.
    pub fn reversed(&self) -> Connection {
        Connection {
            net: self.net,
            start_pin: self.end_pin,
            start: self.end,
            end_pin: self.start_pin,
            end: self.start,
        }
    }
}

/// A bus: its members in physical order (module docs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BusGroup {
    /// The members, oriented to the seed, in physical order.
    pub members: Vec<Connection>,
}

impl BusGroup {
    /// The smallest net number of the group (the order of groups).
    pub fn first_net(&self) -> i32 {
        self.members.iter().map(|m| m.net).min().unwrap_or(0)
    }

    /// The members' nets in physical order.
    pub fn nets(&self) -> Vec<i32> {
        self.members.iter().map(|m| m.net).collect()
    }

    /// The physical index of `net`, if it is a member.
    pub fn index_of(&self, net: i32) -> Option<usize> {
        self.members.iter().position(|m| m.net == net)
    }
}

/// The squared distance of two points, exact for any `i32` coordinates.
pub fn distance_square(a: IntPoint, b: IntPoint) -> i128 {
    let dx = i128::from(a.x) - i128::from(b.x);
    let dy = i128::from(a.y) - i128::from(b.y);
    dx * dx + dy * dy
}

fn within(a: IntPoint, b: IntPoint, radius: i64) -> bool {
    let r = i128::from(radius);
    distance_square(a, b) <= r * r
}

/// `c` oriented to `seed`: directly when both ends are within `radius`,
/// else crossed when that holds, else `None` (not neighbours).
pub fn orient_to(seed: &Connection, c: &Connection, radius: i64) -> Option<Connection> {
    if within(seed.start, c.start, radius) && within(seed.end, c.end, radius) {
        return Some(*c);
    }
    let r = c.reversed();
    (within(seed.start, r.start, radius) && within(seed.end, r.end, radius)).then_some(r)
}

/// True if oriented `a` and `b` are neighbours as they are.
fn oriented_neighbours(a: &Connection, b: &Connection, radius: i64) -> bool {
    within(a.start, b.start, radius) && within(a.end, b.end, radius)
}

fn is_single_net_pin(item: &Item, net: i32) -> bool {
    item.is_pin() && item.net_numbers() == [net]
}

/// The two-pin connections of `board`, by net number (module docs).
pub fn connections(board: &BasicBoard) -> Vec<Connection> {
    let mut pins: std::collections::BTreeMap<i32, Vec<&Item>> = Default::default();
    let mut multi: BTreeSet<i32> = BTreeSet::new();
    for item in board.items() {
        if !item.is_pin() {
            continue;
        }
        match item.net_numbers() {
            [net] => pins.entry(*net).or_default().push(item),
            nets => multi.extend(nets.iter().copied()),
        }
    }
    let mut out = Vec::new();
    for (net, items) in pins {
        if net <= 0 || multi.contains(&net) || items.len() != 2 {
            continue;
        }
        let Some(rule_net) = board.rules.nets.get(net) else {
            continue;
        };
        if rule_net.contains_plane() || items.iter().any(|i| board.has_ignored_nets(i)) {
            continue;
        }
        let mut pair: Vec<&Item> = items
            .into_iter()
            .filter(|i| is_single_net_pin(i, net))
            .collect();
        pair.sort_by_key(|i| i.id());
        let centre = |i: &Item| i.drill().map(|d| d.center);
        let (Some(a), Some(b)) = (centre(pair[0]), centre(pair[1])) else {
            continue;
        };
        out.push(Connection {
            net,
            start_pin: pair[0].id(),
            start: a,
            end_pin: pair[1].id(),
            end: b,
        });
    }
    out
}

/// The bus groups of `board`, by their seed's net number.
pub fn detect(board: &BasicBoard, params: &BusParams) -> Vec<BusGroup> {
    group_connections(&connections(board), params)
}

/// The bus groups of `connections` (sorted by net number), by their seed's
/// net number (module docs).
pub fn group_connections(connections: &[Connection], params: &BusParams) -> Vec<BusGroup> {
    let mut remaining: BTreeSet<usize> = (0..connections.len()).collect();
    let mut groups = Vec::new();
    for seed_index in 0..connections.len() {
        if !remaining.contains(&seed_index) {
            continue;
        }
        let seed = connections[seed_index];
        let mut members = vec![(seed_index, seed)];
        for &candidate in &remaining {
            if candidate == seed_index {
                continue;
            }
            let Some(oriented) = orient_to(&seed, &connections[candidate], params.radius) else {
                continue;
            };
            if members
                .iter()
                .all(|(_, m)| oriented_neighbours(m, &oriented, params.radius))
            {
                members.push((candidate, oriented));
            }
        }
        if members.len() < params.min_members.max(1) {
            continue;
        }
        for (index, _) in &members {
            remaining.remove(index);
        }
        let members: Vec<Connection> = members.into_iter().map(|(_, m)| m).collect();
        groups.push(BusGroup {
            members: physical_order(members),
        });
    }
    groups
}

/// `members` sorted along the line through their two start pins farthest
/// apart (module docs).
pub fn physical_order(mut members: Vec<Connection>) -> Vec<Connection> {
    if members.len() < 2 {
        return members;
    }
    let mut best = (0usize, 0usize, -1i128);
    for i in 0..members.len() {
        for j in i + 1..members.len() {
            let d = distance_square(members[i].start, members[j].start);
            if d > best.2 {
                best = (i, j, d);
            }
        }
    }
    let (a, b) = (members[best.0].start, members[best.1].start);
    let (mut dx, mut dy) = (
        i128::from(b.x) - i128::from(a.x),
        i128::from(b.y) - i128::from(a.y),
    );
    if dx < 0 || (dx == 0 && dy < 0) {
        dx = -dx;
        dy = -dy;
    }
    let position = |p: IntPoint| {
        (i128::from(p.x) - i128::from(a.x)) * dx + (i128::from(p.y) - i128::from(a.y)) * dy
    };
    members.sort_by_key(|m| (position(m.start), m.net));
    members
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(net: i32, start: (i32, i32), end: (i32, i32)) -> Connection {
        Connection {
            net,
            start_pin: ItemId(2 * net),
            start: IntPoint::new(start.0, start.1),
            end_pin: ItemId(2 * net + 1),
            end: IntPoint::new(end.0, end.1),
        }
    }

    const P: BusParams = BusParams {
        radius: 50_000,
        min_members: 3,
    };

    #[test]
    fn distances_are_exact_at_the_coordinate_limits() {
        let a = IntPoint::new(i32::MIN, i32::MIN);
        let b = IntPoint::new(i32::MAX, i32::MAX);
        let d = (1i128 << 32) - 1;
        assert_eq!(distance_square(a, b), 2 * d * d);
        assert!(!within(a, b, i64::from(i32::MAX)));
        assert!(within(a, a, 0));
        let limit = i64::from(freerouting::geometry::CRIT_INT);
        let edge = IntPoint::new(-(1 << 25), 0);
        let other = IntPoint::new(1 << 25, 0);
        assert!(within(edge, other, 2 * limit));
        assert!(!within(edge, other, 2 * limit - 1));
    }

    #[test]
    fn a_crossed_member_is_oriented_to_the_seed() {
        let seed = c(1, (0, 0), (100_000, 0));
        let crossed = c(2, (100_000, 5_000), (0, 5_000));
        let oriented = orient_to(&seed, &crossed, P.radius).expect("neighbours");
        assert_eq!(oriented.start, IntPoint::new(0, 5_000));
        assert_eq!(oriented.start_pin, ItemId(5));
        let far = c(3, (0, 10_000), (300_000, 0));
        assert_eq!(orient_to(&seed, &far, P.radius), None);
    }

    #[test]
    fn groups_need_both_ends_and_three_members() {
        let mut cs: Vec<Connection> = (1..=4)
            .map(|n| c(n, (0, n * 5_000), (200_000, n * 5_000)))
            .collect();
        // Close at the start, far at the end.
        cs.push(c(5, (0, 25_000), (0, 400_000)));
        // A pair: kept out (fewer than 3).
        cs.push(c(6, (900_000, 0), (900_000, 400_000)));
        cs.push(c(7, (905_000, 0), (905_000, 400_000)));
        let groups = group_connections(&cs, &P);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].nets(), [1, 2, 3, 4]);
    }

    #[test]
    fn the_physical_order_follows_the_start_line_whatever_the_net_order() {
        let cs = vec![
            c(1, (30_000, 0), (30_000, 100_000)),
            c(2, (10_000, 0), (10_000, 100_000)),
            c(3, (20_000, 0), (20_000, 100_000)),
            c(4, (0, 0), (0, 100_000)),
        ];
        let groups = group_connections(&cs, &P);
        assert_eq!(groups[0].nets(), [4, 2, 3, 1]);
        let mut reversed = cs.clone();
        reversed.reverse();
        assert_eq!(physical_order(reversed), groups[0].members);
    }
}
