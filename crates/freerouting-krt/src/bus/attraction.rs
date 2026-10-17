//! Bus attraction as a search cost (X11.3), and the spacing measure that
//! checks it.
//!
//! Idea from KiCadRoutingTools (`rust_router/src/router.rs`
//! `get_path_attraction_bonus`, `--bus-attraction-bonus 5000`, at
//! `0486e799`): a bus member is drawn towards the path of its routed
//! neighbour. KRT subtracts a bonus from grid moves near that path; the
//! gridless maze search here only adds costs, so the X11 decision turns
//! the bonus into a penalty. Expanding from a door section entered at `f`
//! to one entered at `e` costs, on top of Freerouting's trace cost,
//!
//! `weight · |e − f| · (x(f) + x(e)) / 2 / pitch`
//!
//! with the excess `x(p) = min(cap, |d(p) − pitch|)`, `d(p)` the distance
//! from `p` to the nearest segment of a routed neighbour's trace (any
//! layer: a member may follow on the other one) and `pitch = hw + hw_n + c`
//! of that segment (`hw` the member's trace half width on the segment's
//! layer, `hw_n` the segment's half width, `c` the clearance between the
//! two classes there; `2·hw + c` for one class); the `/ pitch` takes the
//! smallest pitch of the connection. So a stretch run `k` pitches off its
//! place costs `1 + weight·k` times its length at cost factor 1, up to the
//! cap. The cost is never negative, so the search's distance estimate stays
//! a lower bound. It is two-sided: closer than one pitch is only possible
//! by ripping the neighbour or by running over it on another layer, which
//! a one-sided cost makes free (on `x11-bus8.dsn`, pass 1 only, one-sided
//! weights 1, 4 and 16 gave a spacing of 1459, 2277 and 1671 µm, the
//! second with 2 connections unrouted, against 689 µm two-sided at
//! weight 1).
//!
//! **Which neighbour.** The members next to the routed member in the bus's
//! physical order ([`super::detect`]) whose traces are on the board when
//! the connection starts; with both routed, the nearer one counts at each
//! point. A member with no routed neighbour (the middle member in pass 1,
//! under [`super::order`]) routes without the cost. KRT prefers the left
//! neighbour and samples its path; here every trace segment of both
//! neighbours counts.
//!
//! **Where it applies.** [`BusAttraction`] is a [`ConnectionCost`] of
//! Freerouting's connection router
//! (`freerouting::autoroute::batch::connection_cost`): in pass 1 only (as
//! the bus order; [`AttractionParams::last_pass`]), the first attempt of
//! each bus member's connection and its plane fallback search with it; the
//! neck-down retry and every later pass search without it, so ripup repair
//! is Freerouting's (with the cost in every pass, two-sided weights 1, 4
//! and 16 left 1, 0 and 1 connections unrouted at 1069, 1130 and 861 µm).
//! The
//! groups are detected once per router (pins do not move during a job). It
//! reads board items only, never the search trees, so it spends none of
//! the connection's budget.
//!
//! Defaults ([`AttractionParams::default`]): weight 1, cap 5 mm (KRT's
//! `--bus-attraction-radius`), any layer, two-sided, pass 1. They are the
//! setting of a sweep on `x11-bus8.dsn` (weights 1 to 64, caps 1 and 5 mm,
//! same layer or any, one- or two-sided, pass 1 or all; the `bus` test
//! `attraction_trial`) that left nothing unrouted with the smallest
//! spacing: 689 µm against 1834 µm for Freerouting and 2054 µm for the bus
//! order alone (which left one connection unrouted), one pitch being
//! 500 µm. Larger weights scatter the bus: the cost makes ripping cheaper
//! than a path, and the later passes and the optimizer rebuild what pass 1
//! laid down.
//!
//! [`spacing`] measures what X11.3 accepts on: for each pair of physical
//! neighbours, the mean distance between their trace centrelines on the
//! same layer over the length where they run side by side.

use std::collections::BTreeMap;

use freerouting::autoroute::batch::connection_cost::ConnectionCost;
use freerouting::autoroute::control::AutorouteControl;
use freerouting::autoroute::stages::{ConnectionParams, RouteTask};
use freerouting::board::{BasicBoard, RoutingBoard};
use freerouting::geometry::{FloatPoint, TileShape};

use super::detect::{BusGroup, BusParams, detect};

/// The default weight: the extra cost of a unit of path length one pitch
/// off its place, in units of trace length at cost factor 1.
pub const DEFAULT_WEIGHT: f64 = 1.0;

/// The default cap of the excess distance, in µm (KRT's
/// `--bus-attraction-radius 5.0`).
pub const DEFAULT_CAP_UM: f64 = 5000.0;

/// The default last pass with the cost.
pub const DEFAULT_LAST_PASS: u32 = 1;

/// The attraction parameters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AttractionParams {
    /// The extra cost per unit of path length per pitch beyond one pitch.
    pub weight: f64,
    /// The excess distance at which the cost stops rising, in µm.
    pub cap_um: f64,
    /// Only the neighbour's segments on the door's layer count; a layer
    /// without them is at the cap.
    pub same_layer: bool,
    /// The last pass that routes with the cost (1: pass 1 only, as the
    /// bus order).
    pub last_pass: u32,
    /// Closer than one pitch costs as much as farther (only possible on
    /// another layer or by ripping the neighbour).
    pub two_sided: bool,
}

impl Default for AttractionParams {
    fn default() -> Self {
        AttractionParams {
            weight: DEFAULT_WEIGHT,
            cap_um: DEFAULT_CAP_UM,
            same_layer: false,
            last_pass: DEFAULT_LAST_PASS,
            two_sided: true,
        }
    }
}

/// One neighbour trace segment and the pitch to it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Segment {
    a: FloatPoint,
    b: FloatPoint,
    layer: i32,
    pitch: f64,
}

/// The distance from `p` to the segment `a`–`b`.
pub fn point_segment_distance(p: FloatPoint, a: FloatPoint, b: FloatPoint) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 {
        (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (qx, qy) = (a.x + t * dx, a.y + t * dy);
    ((p.x - qx) * (p.x - qx) + (p.y - qy) * (p.y - qy)).sqrt()
}

/// Where a member's bus is: its group and its physical index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Place {
    group: usize,
    index: usize,
}

/// The bus attraction cost (module docs).
#[derive(Clone, Debug, Default)]
pub struct BusAttraction {
    /// The cost parameters.
    pub params: AttractionParams,
    /// The detection parameters; `None` for [`BusParams::for_board`].
    pub detection: Option<BusParams>,
    groups: Option<Vec<BusGroup>>,
    places: BTreeMap<i32, Place>,
    segments: Vec<Segment>,
    cap: f64,
    pitch: f64,
    /// Connections that searched with the cost (for reports).
    pub applied: usize,
}

impl BusAttraction {
    /// The cost with `params` and the default detection.
    pub fn new(params: AttractionParams) -> Self {
        BusAttraction {
            params,
            ..Default::default()
        }
    }

    fn ensure_groups(&mut self, board: &BasicBoard) {
        if self.groups.is_some() {
            return;
        }
        let params = self
            .detection
            .unwrap_or_else(|| BusParams::for_board(board));
        let groups = detect(board, &params);
        for (g, group) in groups.iter().enumerate() {
            for (index, m) in group.members.iter().enumerate() {
                self.places.insert(m.net, Place { group: g, index });
            }
        }
        self.groups = Some(groups);
    }

    /// The nets next to `net` in its bus, if it is a member.
    fn neighbours(&self, net: i32) -> Vec<i32> {
        let (Some(groups), Some(place)) = (&self.groups, self.places.get(&net)) else {
            return Vec::new();
        };
        let members = &groups[place.group].members;
        let mut out = Vec::new();
        if place.index > 0 {
            out.push(members[place.index - 1].net);
        }
        if place.index + 1 < members.len() {
            out.push(members[place.index + 1].net);
        }
        out
    }
}

/// The trace segments of `nets` on `board` with the pitch to a trace of
/// `control`, in item order.
fn neighbour_segments(
    board: &BasicBoard,
    nets: &[i32],
    control: &AutorouteControl,
) -> Vec<Segment> {
    let mut out = Vec::new();
    for item in board.items() {
        let Some(trace) = item.as_trace() else {
            continue;
        };
        if !matches!(item.net_numbers(), [n] if nets.contains(n)) {
            continue;
        }
        let layer = trace.layer;
        let own = control
            .trace_half_width
            .get(layer as usize)
            .copied()
            .unwrap_or(0);
        let clearance = board.rules.clearance_matrix.get_value(
            control.trace_clearance_class_index,
            item.clearance_class(),
            layer,
            false,
        );
        let pitch = f64::from(own) + f64::from(trace.half_width) + f64::from(clearance);
        let corners = trace.polyline.corner_approx_arr();
        for w in corners.windows(2) {
            out.push(Segment {
                a: w[0],
                b: w[1],
                layer,
                pitch,
            });
        }
    }
    out
}

impl ConnectionCost for BusAttraction {
    fn id(&self) -> &'static str {
        "krt-bus-attraction"
    }

    fn begin(
        &mut self,
        board: &RoutingBoard,
        task: &RouteTask,
        p: &ConnectionParams,
        control: &AutorouteControl,
    ) -> bool {
        if p.pass > self.params.last_pass {
            self.segments.clear();
            return false;
        }
        self.ensure_groups(board);
        let nets = self.neighbours(task.net);
        self.segments = if nets.is_empty() {
            Vec::new()
        } else {
            neighbour_segments(board, &nets, control)
        };
        self.cap = f64::from(freerouting::job::overrides::um_to_board_units(
            board,
            self.params.cap_um,
        ));
        self.pitch = self
            .segments
            .iter()
            .map(|s| s.pitch)
            .fold(f64::INFINITY, f64::min)
            .max(1.0);
        let applies = !self.segments.is_empty() && self.params.weight > 0.0;
        if applies {
            self.applied += 1;
        }
        applies
    }

    fn section_cost(
        &mut self,
        layer: i32,
        _door_shape: &TileShape,
        from: FloatPoint,
        entry: FloatPoint,
    ) -> f64 {
        let excess = (self.excess(layer, from) + self.excess(layer, entry)) / 2.0;
        self.params.weight * from.distance(entry) * excess / self.pitch
    }
}

impl BusAttraction {
    /// The distance of `p` beyond one pitch from the nearest neighbour
    /// segment (on `layer` with [`AttractionParams::same_layer`]), in
    /// `[0, cap]`; 0 without segments.
    fn excess(&self, layer: i32, p: FloatPoint) -> f64 {
        let mut excess = f64::INFINITY;
        for s in &self.segments {
            if self.params.same_layer && s.layer != layer {
                continue;
            }
            let d = point_segment_distance(p, s.a, s.b) - s.pitch;
            let e = if self.params.two_sided { d.abs() } else { d };
            if e < excess {
                excess = e;
            }
        }
        if self.segments.is_empty() {
            return 0.0;
        }
        if excess.is_finite() {
            excess.clamp(0.0, self.cap)
        } else {
            self.cap
        }
    }
}

// ---------------------------------------------------------------------
// The spacing measure
// ---------------------------------------------------------------------

/// The spacing of one pair of physical neighbours.
#[derive(Clone, Debug, PartialEq)]
pub struct PairSpacing {
    /// The first net (lower physical index).
    pub net: i32,
    /// Its neighbour.
    pub neighbour: i32,
    /// The length of `net`'s trace that runs beside the neighbour, in
    /// board units.
    pub common: f64,
    /// The mean centreline distance over that length, in board units.
    pub mean: f64,
}

/// The spacing of a bus's routed members.
#[derive(Clone, Debug, PartialEq)]
pub struct Spacing {
    /// The pairs with a common length, in physical order.
    pub pairs: Vec<PairSpacing>,
    /// The mean over all pairs, weighted by their common length.
    pub mean: f64,
}

/// The segments of `net`'s traces on `layer`.
fn net_segments(board: &BasicBoard, net: i32, layer: i32) -> Vec<(FloatPoint, FloatPoint)> {
    let mut out = Vec::new();
    for item in board.items() {
        if let Some(trace) = item.as_trace()
            && trace.layer == layer
            && item.net_numbers() == [net]
        {
            let corners = trace.polyline.corner_approx_arr();
            out.extend(corners.windows(2).map(|w| (w[0], w[1])));
        }
    }
    out
}

/// The nearest point to `p` on `segments` and whether it is an end of the
/// chain (a segment end no other segment shares).
fn nearest(p: FloatPoint, segments: &[(FloatPoint, FloatPoint)]) -> Option<(f64, bool)> {
    let mut best: Option<(f64, FloatPoint)> = None;
    for &(a, b) in segments {
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let len2 = dx * dx + dy * dy;
        let t = if len2 > 0.0 {
            (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let q = FloatPoint::new(a.x + t * dx, a.y + t * dy);
        let d = p.distance(q);
        if best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, q));
        }
    }
    let (d, q) = best?;
    let shared = |pt: FloatPoint| {
        segments
            .iter()
            .filter(|(a, b)| a.distance(pt) < 1.0 || b.distance(pt) < 1.0)
            .count()
    };
    let is_end = segments
        .iter()
        .flat_map(|(a, b)| [*a, *b])
        .any(|e| e.distance(q) < 1.0 && shared(e) < 2);
    Some((d, is_end))
}

/// The spacing of `group`'s routed members on `board`, sampling each
/// member's traces every `step` board units: a sample counts when its
/// nearest point on the neighbour's traces on the same layer is not an end
/// of them (module docs).
pub fn spacing(board: &BasicBoard, group: &BusGroup, step: f64) -> Spacing {
    let mut pairs = Vec::new();
    for w in group.members.windows(2) {
        let (net, neighbour) = (w[0].net, w[1].net);
        let (mut common, mut sum) = (0.0, 0.0);
        for layer in 0..board.layer_count() as i32 {
            let own = net_segments(board, net, layer);
            let other = net_segments(board, neighbour, layer);
            if own.is_empty() || other.is_empty() {
                continue;
            }
            for (a, b) in own {
                let len = a.distance(b);
                let n = (len / step).ceil().max(1.0) as usize;
                let piece = len / n as f64;
                for k in 0..n {
                    let t = (k as f64 + 0.5) / n as f64;
                    let p = FloatPoint::new(a.x + t * (b.x - a.x), a.y + t * (b.y - a.y));
                    if let Some((d, false)) = nearest(p, &other) {
                        common += piece;
                        sum += d * piece;
                    }
                }
            }
        }
        if common > 0.0 {
            pairs.push(PairSpacing {
                net,
                neighbour,
                common,
                mean: sum / common,
            });
        }
    }
    let total: f64 = pairs.iter().map(|p| p.common).sum();
    let mean = if total > 0.0 {
        pairs.iter().map(|p| p.mean * p.common).sum::<f64>() / total
    } else {
        0.0
    };
    Spacing { pairs, mean }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_distance_to_a_segment_clamps_to_its_ends() {
        let a = FloatPoint::new(0.0, 0.0);
        let b = FloatPoint::new(10.0, 0.0);
        assert_eq!(point_segment_distance(FloatPoint::new(5.0, 3.0), a, b), 3.0);
        assert_eq!(
            point_segment_distance(FloatPoint::new(13.0, 4.0), a, b),
            5.0
        );
        assert_eq!(
            point_segment_distance(FloatPoint::new(1.0, 1.0), a, a),
            2f64.sqrt()
        );
    }

    #[test]
    fn the_cost_is_zero_within_a_pitch_and_capped_beyond() {
        let mut cost = BusAttraction::new(AttractionParams {
            weight: 2.0,
            cap_um: 0.0,
            same_layer: false,
            last_pass: 1,
            two_sided: false,
        });
        cost.cap = 100.0;
        cost.pitch = 50.0;
        cost.segments = vec![Segment {
            a: FloatPoint::new(0.0, 0.0),
            b: FloatPoint::new(1000.0, 0.0),
            layer: 0,
            pitch: 50.0,
        }];
        let shape = TileShape::from(freerouting::geometry::IntBox::new(0, 0, 1, 1));
        let at = |cost: &mut BusAttraction, layer: i32, y: f64| {
            cost.section_cost(
                layer,
                &shape,
                FloatPoint::new(400.0, y),
                FloatPoint::new(500.0, y),
            )
        };
        assert_eq!(at(&mut cost, 0, 40.0), 0.0);
        // 100 long, 30 beyond the pitch of 50, weight 2.
        assert_eq!(at(&mut cost, 0, 80.0), 120.0);
        assert_eq!(at(&mut cost, 0, 5000.0), 400.0);
        // Any layer counts unless `same_layer`.
        assert_eq!(at(&mut cost, 1, 80.0), 120.0);
        cost.params.same_layer = true;
        assert_eq!(at(&mut cost, 1, 80.0), 400.0);
        cost.segments.clear();
        assert_eq!(at(&mut cost, 0, 80.0), 0.0);
    }
}
