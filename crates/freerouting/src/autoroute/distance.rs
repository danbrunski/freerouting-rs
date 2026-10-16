//! `DestinationDistance`: a lower bound of the cost from a point or box to
//! the destination of a maze search (A01.4).
//!
//! Port of `autoroute/maze/DestinationDistance.java` (at `a773a70ec`). The
//! maze search adds this bound to the expansion value of every queue
//! element, so the queue order, and with it the route, depends on its bits.
//! The port keeps Java's arithmetic exactly (D6):
//!
//! - the destination boxes are three: the component side (layer 0), the
//!   solder side (the last layer) and all inner layers joined;
//! - the deltas are Java `int` differences (wrapping) widened to `double`;
//! - every `Math.min` chain and every sum is evaluated in Java's order,
//!   left to right, with no `mul_add` and no reassociation; `Math.min` is
//!   [`jmath::min_f64`](crate::geometry::jmath::min_f64);
//! - an empty destination gives `Integer.MAX_VALUE`.
//!
//! The cost minima are computed once in the constructor
//! (`DestinationDistance.java:45-100`); a `calculate` call allocates
//! nothing.

use crate::geometry::jmath::{max_f64, min_f64};
use crate::geometry::{FloatPoint, IntBox};

use super::control::ExpansionCostFactor;

/// The lower bound of the cost to the destination set of a search.
#[derive(Clone, Debug, PartialEq)]
pub struct DestinationDistance {
    trace_costs: Vec<ExpansionCostFactor>,
    layer_active: Vec<bool>,
    layer_count: usize,
    active_layer_count: usize,
    min_normal_via_cost: f64,
    min_cheap_via_cost: f64,
    min_component_side_trace_cost: f64,
    max_component_side_trace_cost: f64,
    min_solder_side_trace_cost: f64,
    max_solder_side_trace_cost: f64,
    /// The minimum of the maximal trace costs on each inner layer.
    max_inner_side_trace_cost: f64,
    min_component_inner_trace_cost: f64,
    min_solder_inner_trace_cost: f64,
    min_component_solder_inner_trace_cost: f64,
    component_side_box: IntBox,
    solder_side_box: IntBox,
    inner_side_box: IntBox,
    box_is_empty: bool,
    component_side_box_is_empty: bool,
    solder_side_box_is_empty: bool,
    inner_side_box_is_empty: bool,
}

/// The cost minimum and maximum of one layer: `(min, max)` of its
/// horizontal and vertical costs, the vertical cost first on a tie.
fn min_max(cost: ExpansionCostFactor) -> (f64, f64) {
    if cost.horizontal < cost.vertical {
        (cost.horizontal, cost.vertical)
    } else {
        (cost.vertical, cost.horizontal)
    }
}

/// The distance of `[low, high]` from `[other_low, other_high]` along one
/// axis, as Java computes it: an `int` difference widened to `double`.
fn axis_delta(low: i32, high: i32, other_low: i32, other_high: i32) -> f64 {
    if low > other_high {
        f64::from(low.wrapping_sub(other_high))
    } else if high < other_low {
        f64::from(other_low.wrapping_sub(high))
    } else {
        0.0
    }
}

/// `(max, min)` of two deltas, the second first on a tie.
fn ordered(delta_x: f64, delta_y: f64) -> (f64, f64) {
    if delta_x > delta_y {
        (delta_x, delta_y)
    } else {
        (delta_y, delta_x)
    }
}

impl DestinationDistance {
    /// `new DestinationDistance(traceCosts, layerActive, minNormalViaCost,
    /// minCheapViaCost)`; `layer_active` has one entry per board layer.
    pub fn new(
        trace_costs: &[ExpansionCostFactor],
        layer_active: &[bool],
        min_normal_via_cost: f64,
        min_cheap_via_cost: f64,
    ) -> DestinationDistance {
        let layer_count = layer_active.len();
        let active_layer_count = layer_active.iter().filter(|a| **a).count();
        let (mut min_component, mut max_component) = (0.0, 0.0);
        if layer_active[0] {
            (min_component, max_component) = min_max(trace_costs[0]);
        }
        let (mut min_solder, mut max_solder) = (0.0, 0.0);
        if layer_active[layer_count - 1] {
            (min_solder, max_solder) = min_max(trace_costs[layer_count - 1]);
        }
        // PARITY: DestinationDistance.java:87-99, the minima in this order.
        let mut max_inner = min_f64(max_component, max_solder);
        for layer in 1..layer_count.saturating_sub(1) {
            if !layer_active[layer] {
                continue;
            }
            let cost = trace_costs[layer];
            max_inner = min_f64(max_inner, max_f64(cost.horizontal, cost.vertical));
        }
        let min_component_inner = min_f64(min_component, max_inner);
        let min_solder_inner = min_f64(min_solder, max_inner);
        DestinationDistance {
            trace_costs: trace_costs.to_vec(),
            layer_active: layer_active.to_vec(),
            layer_count,
            active_layer_count,
            min_normal_via_cost,
            min_cheap_via_cost,
            min_component_side_trace_cost: min_component,
            max_component_side_trace_cost: max_component,
            min_solder_side_trace_cost: min_solder,
            max_solder_side_trace_cost: max_solder,
            max_inner_side_trace_cost: max_inner,
            min_component_inner_trace_cost: min_component_inner,
            min_solder_inner_trace_cost: min_solder_inner,
            min_component_solder_inner_trace_cost: min_f64(min_component_inner, min_solder_inner),
            component_side_box: IntBox::EMPTY,
            solder_side_box: IntBox::EMPTY,
            inner_side_box: IntBox::EMPTY,
            box_is_empty: true,
            component_side_box_is_empty: true,
            solder_side_box_is_empty: true,
            inner_side_box_is_empty: true,
        }
    }

    /// Joins `bounds` to the destination box of `layer`.
    pub fn join(&mut self, bounds: &IntBox, layer: i32) {
        if layer == 0 {
            self.component_side_box = self.component_side_box.union_box(bounds);
            self.component_side_box_is_empty = false;
        } else if layer == self.layer_count as i32 - 1 {
            self.solder_side_box = self.solder_side_box.union_box(bounds);
            self.solder_side_box_is_empty = false;
        } else {
            self.inner_side_box = self.inner_side_box.union_box(bounds);
            self.inner_side_box_is_empty = false;
        }
        self.box_is_empty = false;
    }

    /// The lower bound from `point` on `layer`: the bound of its bounding
    /// box.
    pub fn calculate_point(&self, point: &FloatPoint, layer: i32) -> f64 {
        self.calculate_box(&point.bounding_box(), layer)
    }

    /// The lower bound from `bounds` on `layer`.
    pub fn calculate_box(&self, bounds: &IntBox, layer: i32) -> f64 {
        self.calculate(bounds, layer, self.min_normal_via_cost)
    }

    /// The lower bound from `bounds` on `layer` with the cheap via cost in
    /// place of the normal one (`calculateCheapDistance`).
    pub fn calculate_cheap_distance(&self, bounds: &IntBox, layer: i32) -> f64 {
        self.calculate(bounds, layer, self.min_cheap_via_cost)
    }

    /// The trace costs the bound was built with.
    pub fn trace_costs(&self) -> &[ExpansionCostFactor] {
        &self.trace_costs
    }

    /// The active layers the bound was built with.
    pub fn layer_active(&self) -> &[bool] {
        &self.layer_active
    }

    /// `DestinationDistance.calculate(IntBox, int)` with `via` as
    /// `minNormalViaCost` (`DestinationDistance.java:117-380`).
    fn calculate(&self, b: &IntBox, layer: i32, via: f64) -> f64 {
        if self.box_is_empty {
            return f64::from(i32::MAX);
        }
        let (c, s, n) = (
            &self.component_side_box,
            &self.solder_side_box,
            &self.inner_side_box,
        );
        let (c_max, c_min) = ordered(
            axis_delta(b.ll.x, b.ur.x, c.ll.x, c.ur.x),
            axis_delta(b.ll.y, b.ur.y, c.ll.y, c.ur.y),
        );
        let (s_max, s_min) = ordered(
            axis_delta(b.ll.x, b.ur.x, s.ll.x, s.ur.x),
            axis_delta(b.ll.y, b.ur.y, s.ll.y, s.ur.y),
        );
        let (n_max, n_min) = ordered(
            axis_delta(b.ll.x, b.ur.x, n.ll.x, n.ur.x),
            axis_delta(b.ll.y, b.ur.y, n.ll.y, n.ur.y),
        );
        let deltas = Deltas {
            c_max,
            c_min,
            s_max,
            s_min,
            n_max,
            n_min,
        };
        if layer == 0 {
            self.component_side(b, &deltas, via)
        } else if layer == self.layer_count as i32 - 1 {
            self.solder_side(b, layer, &deltas, via)
        } else {
            self.inner_side(b, layer, &deltas, via)
        }
    }

    /// The cost on one layer to the box `target`, or `Integer.MAX_VALUE`
    /// when that box is empty.
    fn one_layer(&self, b: &IntBox, target: &IntBox, empty: bool, layer: usize) -> f64 {
        if empty {
            f64::from(i32::MAX)
        } else {
            let cost = self.trace_costs[layer];
            b.weighted_distance(target, cost.horizontal, cost.vertical)
        }
    }

    // PARITY: DestinationDistance.java:240-300, every sum left to right and
    // the minima in this order.
    fn component_side(&self, b: &IntBox, d: &Deltas, via: f64) -> f64 {
        let mut result = self.one_layer(
            b,
            &self.component_side_box,
            self.component_side_box_is_empty,
            0,
        );
        if self.active_layer_count <= 1 {
            return result;
        }
        let (min_s, min_c) = (
            self.min_solder_side_trace_cost,
            self.min_component_side_trace_cost,
        );
        let mut tmp = if min_s < min_c {
            min_s * d.s_max + min_c * d.s_min + via
        } else {
            min_c * d.s_max + min_s * d.s_min + via
        };
        result = min_f64(result, tmp);
        tmp = d.c_max + d.c_min * self.min_component_inner_trace_cost + 2.0 * via;
        result = min_f64(result, tmp);
        if self.active_layer_count == 2 {
            return result;
        }
        tmp = d.n_max + d.n_min * self.min_component_inner_trace_cost + via;
        result = min_f64(result, tmp);
        tmp = d.s_max + self.min_component_solder_inner_trace_cost * d.s_min + 2.0 * via;
        result = min_f64(result, tmp);
        tmp = d.c_max + d.c_min + 2.0 * via;
        result = min_f64(result, tmp);
        if self.active_layer_count == 3 {
            return result;
        }
        tmp = d.n_max + d.n_min + 2.0 * via;
        result = min_f64(result, tmp);
        tmp = d.s_max + d.s_min + 3.0 * via;
        min_f64(result, tmp)
    }

    // PARITY: DestinationDistance.java:301-345.
    fn solder_side(&self, b: &IntBox, layer: i32, d: &Deltas, via: f64) -> f64 {
        let mut result = self.one_layer(
            b,
            &self.solder_side_box,
            self.solder_side_box_is_empty,
            layer as usize,
        );
        let (min_s, min_c) = (
            self.min_solder_side_trace_cost,
            self.min_component_side_trace_cost,
        );
        let mut tmp = if min_c < min_s {
            min_c * d.c_max + min_s * d.c_min + via
        } else {
            min_s * d.c_max + min_c * d.c_min + via
        };
        result = min_f64(result, tmp);
        tmp = d.s_max + d.s_min * self.min_solder_inner_trace_cost + 2.0 * via;
        result = min_f64(result, tmp);
        if self.active_layer_count <= 2 {
            return result;
        }
        tmp = d.n_min * self.min_solder_inner_trace_cost + d.n_max + via;
        result = min_f64(result, tmp);
        tmp = d.c_max + self.min_component_solder_inner_trace_cost * d.c_min + 2.0 * via;
        result = min_f64(result, tmp);
        tmp = d.s_max + d.s_min + 2.0 * via;
        result = min_f64(result, tmp);
        if self.active_layer_count == 3 {
            return result;
        }
        tmp = d.n_max + d.n_min + 2.0 * via;
        result = min_f64(result, tmp);
        tmp = d.c_max + d.c_min + 3.0 * via;
        min_f64(result, tmp)
    }

    // PARITY: DestinationDistance.java:347-380.
    fn inner_side(&self, b: &IntBox, layer: i32, d: &Deltas, via: f64) -> f64 {
        let mut result = self.one_layer(
            b,
            &self.inner_side_box,
            self.inner_side_box_is_empty,
            layer as usize,
        );
        let mut tmp = d.n_max + d.n_min + via;
        result = min_f64(result, tmp);
        tmp = d.c_max + d.c_min * self.min_component_inner_trace_cost + via;
        result = min_f64(result, tmp);
        tmp = d.s_max + d.s_min * self.min_solder_inner_trace_cost + via;
        result = min_f64(result, tmp);
        tmp = d.c_max + d.c_min + 2.0 * via;
        result = min_f64(result, tmp);
        tmp = d.s_max + d.s_min + 2.0 * via;
        min_f64(result, tmp)
    }
}

/// The larger and smaller axis delta to each of the three boxes.
struct Deltas {
    c_max: f64,
    c_min: f64,
    s_max: f64,
    s_min: f64,
    n_max: f64,
    n_min: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cost(h: f64, v: f64) -> ExpansionCostFactor {
        ExpansionCostFactor {
            horizontal: h,
            vertical: v,
        }
    }

    #[test]
    fn empty_destination_is_integer_max() {
        let d = DestinationDistance::new(&[cost(1.0, 2.0); 2], &[true, true], 50.0, 40.0);
        assert_eq!(
            d.calculate_box(&IntBox::new(0, 0, 1, 1), 0),
            2_147_483_647.0
        );
    }

    #[test]
    fn one_layer_distance_is_weighted() {
        let mut d = DestinationDistance::new(&[cost(1.0, 2.0)], &[true], 50.0, 40.0);
        d.join(&IntBox::new(100, 0, 110, 10), 0);
        // Same y range: horizontal distance 90 at weight 1.
        assert_eq!(d.calculate_box(&IntBox::new(0, 0, 10, 10), 0), 90.0);
        assert_eq!(d.calculate_point(&FloatPoint::new(10.0, 5.0), 0), 90.0);
    }

    #[test]
    fn a_via_bounds_the_distance_on_the_other_side() {
        let mut d = DestinationDistance::new(&[cost(1.0, 2.0); 2], &[true, true], 5.0, 4.0);
        d.join(&IntBox::new(100, 0, 110, 10), 1);
        // On layer 0 the component box is empty: min over the via paths.
        // min_c = min_s = 1: 1 * 90 + 1 * 0 + 5 = 95; 2 vias: dx to the empty
        // component box is huge.
        assert_eq!(d.calculate_box(&IntBox::new(0, 0, 10, 10), 0), 95.0);
        assert_eq!(
            d.calculate_cheap_distance(&IntBox::new(0, 0, 10, 10), 0),
            94.0
        );
    }
}
