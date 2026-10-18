// SPDX-License-Identifier: GPL-3.0-only AND MIT
// SPDX-FileCopyrightText: 2026 drandyhaas (KiCadRoutingTools)
// Portions ported from KiCadRoutingTools py_router/net_ordering.py
// (order_nets_inside_out) at 0486e799.

//! The `inside-out` pass-1 order (Q07.3), KRT's BGA inside-out order.
//!
//! A **dense part** is a component with at least [`MIN_PINS`] pins whose
//! smallest pin pitch (the shortest non-zero distance between two of its pin
//! centres) is at most 0.65 mm, KRT's fine-pitch threshold, plus KRT's 1 µm
//! distance tolerance (`_DIST_TOL`, as X06 and X10 use it). Its **pin field**
//! is the bounding box of its pin centres; KRT uses its BGA exclusion zones.
//!
//! A net with a pin whose centre is inside (or on the edge of) a dense
//! part's pin field is a dense net. Its distance is the smallest squared
//! distance from such a pin to that field's centre, over all dense parts
//! whose field holds the pin, in exact `i64` (doubled coordinates, so the
//! centre of an odd-sized field needs no rounding). Dense nets come first,
//! nearest first, ties by net number; the other nets keep Java's order. As
//! in [`super::conflict_rounds`], plane-net tasks stay first and nets move
//! as whole nets.

use std::collections::{BTreeMap, BTreeSet};

use freerouting_geometry::IntPoint;

use super::conflicts::squared_distance;
use super::{PassOneOrdering, arrange_by_nets, pins_by_net, signal_nets};
use crate::autoroute::stages::RouteTask;
use crate::board::basic_board::BasicBoard;
use crate::job::overrides::um_to_board_units;

/// The registry id (`router.autorouter.ordering=inside-out`).
pub const ORDERING_ID: &str = "inside-out";

/// The fewest pins of a dense part.
pub const MIN_PINS: usize = 16;

/// The largest pin pitch of a dense part, in µm (KRT's fine pitch).
pub const MAX_PITCH_UM: f64 = 650.0;

/// KRT's distance tolerance, in µm.
pub const PITCH_TOLERANCE_UM: f64 = 1.0;

/// A dense part.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DensePart {
    /// The component number.
    pub component: i32,
    /// The number of its pins.
    pub pins: usize,
    /// Its smallest non-zero squared pin pitch, in board units.
    pub pitch_squared: i64,
    /// The lower-left corner of its pin field.
    pub min: IntPoint,
    /// The upper-right corner of its pin field.
    pub max: IntPoint,
}

impl DensePart {
    /// True if `p` is inside the pin field or on its edge.
    pub fn holds(&self, p: IntPoint) -> bool {
        (self.min.x..=self.max.x).contains(&p.x) && (self.min.y..=self.max.y).contains(&p.y)
    }

    /// Four times the squared distance of `p` from the field's centre.
    pub fn distance_to_centre_x4(&self, p: IntPoint) -> i64 {
        let dx = 2 * i64::from(p.x) - i64::from(self.min.x) - i64::from(self.max.x);
        let dy = 2 * i64::from(p.y) - i64::from(self.min.y) - i64::from(self.max.y);
        dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy))
    }
}

/// The smallest non-zero squared distance between two of `points`.
fn min_pitch_squared(points: &[IntPoint]) -> Option<i64> {
    let mut best: Option<i64> = None;
    for (i, a) in points.iter().enumerate() {
        for b in &points[i + 1..] {
            let d = squared_distance(*a, *b);
            if d > 0 && best.is_none_or(|m| d < m) {
                best = Some(d);
            }
        }
    }
    best
}

/// The dense parts of `board`, by component number.
pub fn dense_parts(board: &BasicBoard) -> Vec<DensePart> {
    let max_pitch = i64::from(um_to_board_units(board, MAX_PITCH_UM + PITCH_TOLERANCE_UM));
    let mut by_component: BTreeMap<i32, Vec<IntPoint>> = BTreeMap::new();
    for item in board.items() {
        if let Some(pin) = item.as_pin()
            && item.component_id() > 0
        {
            by_component
                .entry(item.component_id())
                .or_default()
                .push(pin.drill.center);
        }
    }
    by_component
        .into_iter()
        .filter(|(_, points)| points.len() >= MIN_PINS)
        .filter_map(|(component, points)| {
            let pitch_squared = min_pitch_squared(&points)?;
            if pitch_squared > max_pitch.saturating_mul(max_pitch) {
                return None;
            }
            let min = IntPoint {
                x: points.iter().map(|p| p.x).min()?,
                y: points.iter().map(|p| p.y).min()?,
            };
            let max = IntPoint {
                x: points.iter().map(|p| p.x).max()?,
                y: points.iter().map(|p| p.y).max()?,
            };
            Some(DensePart {
                component,
                pins: points.len(),
                pitch_squared,
                min,
                max,
            })
        })
        .collect()
}

/// The dense nets among `nets`, nearest first, ties by net number, with
/// their distances (four times the squared distance, module docs).
pub fn dense_net_order(
    board: &BasicBoard,
    parts: &[DensePart],
    nets: &BTreeSet<i32>,
) -> Vec<(i32, i64)> {
    if parts.is_empty() {
        return Vec::new();
    }
    let pins = pins_by_net(board, Some(nets));
    let mut dense: Vec<(i32, i64)> = pins
        .iter()
        .filter_map(|(&net, pins)| {
            pins.iter()
                .flat_map(|pin| {
                    parts
                        .iter()
                        .filter(|part| part.holds(pin.centre))
                        .map(|part| part.distance_to_centre_x4(pin.centre))
                })
                .min()
                .map(|d| (net, d))
        })
        .collect();
    dense.sort_by_key(|&(net, d)| (d, net));
    dense
}

/// Java's pass-1 list `tasks` in inside-out order (module docs).
pub fn inside_out_order(board: &BasicBoard, tasks: Vec<RouteTask>) -> Vec<RouteTask> {
    let parts = dense_parts(board);
    let order: Vec<i32> = dense_net_order(board, &parts, &signal_nets(&tasks))
        .into_iter()
        .map(|(net, _)| net)
        .collect();
    arrange_by_nets(tasks, &order)
}

/// The `inside-out` ordering: [`inside_out_order`] in pass 1, Freerouting's
/// ordering afterwards.
pub fn ordering() -> PassOneOrdering {
    PassOneOrdering::new(ORDERING_ID, inside_out_order)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn min_pitch_ignores_coincident_pins() {
        let p = |x, y| IntPoint { x, y };
        assert_eq!(min_pitch_squared(&[p(0, 0), p(0, 0), p(3, 4)]), Some(25));
        assert_eq!(min_pitch_squared(&[p(0, 0)]), None);
    }

    #[test]
    fn distance_to_an_odd_field_centre_is_exact() {
        let part = DensePart {
            component: 1,
            pins: 16,
            pitch_squared: 1,
            min: IntPoint { x: 0, y: 0 },
            max: IntPoint { x: 3, y: 1 },
        };
        // Centre (1.5, 0.5): (1, 0) is at squared distance 0.5, x4 = 2.
        assert_eq!(part.distance_to_centre_x4(IntPoint { x: 1, y: 0 }), 2);
        assert_eq!(part.distance_to_centre_x4(IntPoint { x: 0, y: 0 }), 10);
        assert!(part.holds(IntPoint { x: 3, y: 1 }));
        assert!(!part.holds(IntPoint { x: 4, y: 1 }));
    }
}
