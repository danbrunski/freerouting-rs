//! The centreline search (X08.2): the pair's middle line, routed by
//! Freerouting's room/door maze search at the compound half width.
//!
//! **Ends.** A pair connects two P pins and two N pins (two-pin nets only
//! in X08). Each P pin is matched with an N pin as one end: of the two
//! assignments the one with the smaller summed pin distance wins, the
//! identity on a tie (lowest index, where KRT's
//! `diff_pair_multipoint.py` leaves ties to scipy's
//! `linear_sum_assignment`). An end's terminal is the midpoint of its P
//! and N pin centres, moved out by the setback (0 for the direct attempt)
//! along the escape direction: perpendicular to the P–N axis, towards the
//! other end (the left normal of P→N on a tie).
//!
//! **Search.** On a scratch copy of the board, every item of N is
//! reassigned to P, so the pair's own copper is no obstacle and is no
//! target either (only the far terminal ends the search,
//! [`AutorouteControl::destinations_only`]). Each terminal is a user-fixed
//! two-unit stub of P at the compound half width `hw + D/2`, `D = 2·hw +
//! g` the P–N centre distance, so the outer track edge is the stub's edge
//! (plus one unit for the offset's rounding, [`Pair::search_half_width`]).
//! The control is P's (its clearance class, so the search keeps the pair
//! class's clearance to every foreign item), with the compound half width
//! on every layer, one active layer (X08 is single layer; vias are X09), no
//! vias, no ripup, no neck-down, and a work budget. The routed traces are
//! chained from the start terminal to the end terminal; the first and last
//! corner snap onto the terminals. A search that succeeds without a new
//! trace (the stubs touch) gives the straight line between the terminals. The scratch board is dropped: nothing
//! here changes the board.
//!
//! The idea of a centreline at the pair's outer width comes from
//! KiCadRoutingTools (`docs/differential-pairs.md`, `route_diff.py` at
//! `0486e799`); KRT's grid pose search and Dubins heuristic are not used.

use std::collections::BTreeSet;

use freerouting::autoroute::AttemptState;
use freerouting::autoroute::connection::{Ripped, RouteLimits, route_connection};
use freerouting::autoroute::control::AutorouteControl;
use freerouting::autoroute::session::search::SearchSettings;
use freerouting::autoroute::session::sink::NoopSink;
use freerouting::autoroute::stages::StageCtx;
use freerouting::board::items::ItemId;
use freerouting::board::search::WorkLimit;
use freerouting::board::structure::FixedState;
use freerouting::board::{BasicBoard, RoutingBoard};
use freerouting::geometry::{FloatPoint, IntPoint, Point, Polyline, PolylineShapeOps};

use super::model::Pair;

/// The site name of the centreline search's work limit.
const CENTRELINE_SITE: &str = "krt_diffpair_centreline";

/// One end of a pair: its P and N pins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct End {
    pub p_pin: ItemId,
    pub n_pin: ItemId,
    pub p_centre: IntPoint,
    pub n_centre: IntPoint,
}

impl End {
    /// The midpoint of the pin centres (rounded towards negative
    /// infinity).
    pub fn midpoint(&self) -> IntPoint {
        IntPoint::new(
            ((i64::from(self.p_centre.x) + i64::from(self.n_centre.x)).div_euclid(2)) as i32,
            ((i64::from(self.p_centre.y) + i64::from(self.n_centre.y)).div_euclid(2)) as i32,
        )
    }

    /// The unit escape direction towards `other` ([module docs](self)).
    pub fn escape(&self, other: &End) -> (f64, f64) {
        let ax = f64::from(self.n_centre.x) - f64::from(self.p_centre.x);
        let ay = f64::from(self.n_centre.y) - f64::from(self.p_centre.y);
        let len = ax.hypot(ay);
        // The left normal of P→N.
        let (mut ux, mut uy) = if len > 0.0 {
            (-ay / len, ax / len)
        } else {
            (1.0, 0.0)
        };
        let m = self.midpoint();
        let o = other.midpoint();
        let dot = ux * (f64::from(o.x) - f64::from(m.x)) + uy * (f64::from(o.y) - f64::from(m.y));
        if dot < 0.0 {
            ux = -ux;
            uy = -uy;
        }
        (ux, uy)
    }

    /// The terminal at `setback` board units ([module docs](self)).
    pub fn terminal(&self, other: &End, setback: f64) -> IntPoint {
        let m = self.midpoint();
        let (ux, uy) = self.escape(other);
        IntPoint::new(
            (f64::from(m.x) + ux * setback).round() as i32,
            (f64::from(m.y) + uy * setback).round() as i32,
        )
    }
}

/// How far the pins' pads on `layer` reach from the midpoint of `end`
/// along its escape direction towards `other`, in board units (0 if they
/// reach no farther than the midpoint).
pub fn pad_reach(board: &BasicBoard, end: &End, other: &End, layer: i32) -> f64 {
    let m = end.midpoint().to_float();
    let (ux, uy) = end.escape(other);
    let mut reach: f64 = 0.0;
    for pin in [end.p_pin, end.n_pin] {
        let Some(item) = board.item(pin) else {
            continue;
        };
        for index in 0..board.tile_shape_count(item) {
            if item.shape_layer(index) != layer {
                continue;
            }
            if let Some(shape) = board.tile_shape(item, index) {
                for c in shape.corner_approx_arr() {
                    reach = reach.max((c.x - m.x) * ux + (c.y - m.y) * uy);
                }
            }
        }
    }
    reach
}

/// The pins and layer of a pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PairEnds {
    pub start: End,
    pub end: End,
    /// The routing layer: the lowest signal layer all four pins reach.
    pub layer: i32,
}

fn pins_of(board: &BasicBoard, net: i32) -> Vec<(ItemId, IntPoint, i32, i32)> {
    board
        .items()
        .filter(|i| i.is_pin() && i.contains_net(net))
        .filter_map(|i| {
            let d = i.drill()?;
            Some((i.id(), d.center, d.first_layer, d.last_layer))
        })
        .collect()
}

/// The ends of `pair` ([module docs](self)); `Err` names why the pair
/// can't be routed by X08.
pub fn pair_ends(board: &BasicBoard, pair: &Pair) -> Result<PairEnds, String> {
    let p = pins_of(board, pair.p);
    let n = pins_of(board, pair.n);
    if p.len() != 2 || n.len() != 2 {
        return Err(format!(
            "{} P pins and {} N pins; X08 routes pairs of two-pin nets",
            p.len(),
            n.len()
        ));
    }
    let d = |a: IntPoint, b: IntPoint| a.distance(b);
    let identity = d(p[0].1, n[0].1) + d(p[1].1, n[1].1);
    let swapped = d(p[0].1, n[1].1) + d(p[1].1, n[0].1);
    let (n0, n1) = if swapped < identity {
        (n[1], n[0])
    } else {
        (n[0], n[1])
    };
    let first = [p[0], p[1], n0, n1].iter().map(|x| x.2).max().unwrap_or(0);
    let last = [p[0], p[1], n0, n1].iter().map(|x| x.3).min().unwrap_or(-1);
    let layer = (first..=last)
        .find(|l| board.layer_structure.layers[*l as usize].is_signal)
        .ok_or_else(|| "the four pins share no signal layer".to_string())?;
    let end = |pp: (ItemId, IntPoint, i32, i32), nn: (ItemId, IntPoint, i32, i32)| End {
        p_pin: pp.0,
        n_pin: nn.0,
        p_centre: pp.1,
        n_centre: nn.1,
    };
    Ok(PairEnds {
        start: end(p[0], n0),
        end: end(p[1], n1),
        layer,
    })
}

/// The search parameters of one centreline.
#[derive(Clone, Copy, Debug)]
pub struct CentrelineParams {
    pub layer: i32,
    pub start: IntPoint,
    pub end: IntPoint,
    /// The search budget in work units (B02.7).
    pub work_budget: i64,
}

/// A stub of net `net` at `at`, two units long towards `-dir`, user fixed.
fn insert_stub(
    board: &mut RoutingBoard,
    at: IntPoint,
    dir: (f64, f64),
    layer: i32,
    hw: i32,
    net: i32,
    clearance_class: i32,
) -> Option<ItemId> {
    let step = |v: f64| {
        if v > 0.3 {
            -2
        } else if v < -0.3 {
            2
        } else {
            0
        }
    };
    let (sx, sy) = (step(dir.0), step(dir.1));
    let (sx, sy) = if sx == 0 && sy == 0 { (2, 0) } else { (sx, sy) };
    let other = IntPoint::new(at.x.saturating_add(sx), at.y.saturating_add(sy));
    board.insert_trace_without_cleaning(
        Polyline::from_corners(&Point::Int(at), &Point::Int(other)),
        layer,
        hw,
        &[net],
        clearance_class,
        FixedState::UserFixed,
    )
}

/// The clearance class of net `net`'s traces.
pub fn net_clearance_class(board: &BasicBoard, net: i32) -> i32 {
    board
        .rules
        .nets
        .get(net)
        .map(|n| {
            board
                .rules
                .net_classes
                .get(n.net_class())
                .trace_clearance_class()
        })
        .unwrap_or(1)
}

/// Routes the centreline of `pair` from `params.start` to `params.end`
/// ([module docs](self)). The board is not changed.
pub fn route_centreline(
    board: &RoutingBoard,
    ctx: &StageCtx,
    pair: &Pair,
    params: &CentrelineParams,
) -> Result<Polyline, String> {
    let mut scratch = board.clone();
    let layer = params.layer;
    let hw = pair.search_half_width(layer);
    let n_items: Vec<ItemId> = scratch
        .items()
        .filter(|i| i.contains_net(pair.n))
        .map(|i| i.id())
        .collect();
    for id in n_items {
        scratch.assign_net_no(id, pair.p);
    }
    let cl = net_clearance_class(&scratch, pair.p);
    let dir = {
        let dx = f64::from(params.end.x) - f64::from(params.start.x);
        let dy = f64::from(params.end.y) - f64::from(params.start.y);
        let len = dx.hypot(dy).max(1.0);
        (dx / len, dy / len)
    };
    let last_id = scratch.items().map(|i| i.id().0).max().unwrap_or(0);
    let start = insert_stub(&mut scratch, params.start, dir, layer, hw, pair.p, cl)
        .ok_or("the start terminal stub was refused")?;
    let end = insert_stub(
        &mut scratch,
        params.end,
        (-dir.0, -dir.1),
        layer,
        hw,
        pair.p,
        cl,
    )
    .ok_or("the end terminal stub was refused")?;
    let dest: BTreeSet<ItemId> = [end].into();
    let control = centreline_control(&scratch, ctx, pair.p, layer, hw, &dest)?;
    let limits = RouteLimits {
        settings: SearchSettings::from_router(ctx.board_settings),
        stop: Some(ctx.cancel),
        work_limit: Some(WorkLimit::with_site(
            scratch.search_trees().work_meter(),
            params.work_budget,
            CENTRELINE_SITE,
        )),
    };
    let mut ripped = BTreeSet::new();
    let ripped = Ripped {
        items: &mut ripped,
        costs: None,
    };
    let starts: BTreeSet<ItemId> = [start].into();
    let (result, _) = route_connection(
        &mut scratch,
        &starts,
        &dest,
        &control,
        limits,
        ripped,
        NoopSink,
    );
    if result.state != AttemptState::Routed {
        return Err(
            format!("centreline search: {:?} {}", result.state, result.details)
                .trim()
                .to_string(),
        );
    }
    let traces: Vec<Vec<IntPoint>> = scratch
        .items()
        .filter(|i| i.id().0 > last_id && i.id() != start && i.id() != end)
        .filter_map(|i| i.as_trace())
        .filter(|t| t.layer == layer && t.half_width == hw)
        .map(|t| t.polyline.corners().iter().map(round_point).collect())
        .collect();
    if traces.is_empty() {
        // The terminals' stubs met without a new trace (ends a few
        // compound widths apart): the straight line between them, which
        // the stage's DRC gate then judges.
        return chain(&[vec![params.start, params.end]], params.start, params.end);
    }
    chain(&traces, params.start, params.end)
}

/// The control of the centreline search ([module docs](self)).
fn centreline_control(
    board: &RoutingBoard,
    ctx: &StageCtx,
    net: i32,
    layer: i32,
    hw: i32,
    dest: &BTreeSet<ItemId>,
) -> Result<AutorouteControl, String> {
    let mut control = AutorouteControl::for_net_default(board, net, ctx.board_settings)
        .map_err(|e| format!("centreline control: {e}"))?;
    if !control
        .layer_active
        .get(layer as usize)
        .copied()
        .unwrap_or(false)
    {
        return Err(format!("layer {layer} is not active for routing"));
    }
    let class = control.trace_clearance_class_index;
    for l in 0..control.layer_count {
        control.layer_active[l] = l == layer as usize;
        control.trace_half_width[l] = hw;
        control.compensated_trace_half_width[l] = hw
            + board
                .rules
                .clearance_matrix
                .clearance_compensation_value(class, l as i32);
    }
    control.vias_allowed = false;
    control.ripup_allowed = false;
    control.with_neckdown = false;
    control.destinations_only = Some(dest.clone());
    Ok(control)
}

fn round_point(p: &Point) -> IntPoint {
    match p.as_int() {
        Some(i) => i,
        None => {
            let f = p.to_float();
            IntPoint::new(f.x.round() as i32, f.y.round() as i32)
        }
    }
}

/// Chains the corner lists `traces` into one point list from `start` to
/// `end`: the trace nearest `start` first, then each trace that begins
/// (or, reversed, ends) where the chain ends. The first and last points
/// snap onto `start` and `end`.
fn chain(traces: &[Vec<IntPoint>], start: IntPoint, end: IntPoint) -> Result<Polyline, String> {
    if traces.is_empty() {
        return Err("the centreline search inserted no trace".to_string());
    }
    let mut left: Vec<Vec<IntPoint>> = traces.to_vec();
    let near = |p: IntPoint, q: IntPoint| p.distance(q);
    let first = (0..left.len())
        .min_by(|a, b| {
            let da = near(left[*a][0], start).min(near(*left[*a].last().unwrap(), start));
            let db = near(left[*b][0], start).min(near(*left[*b].last().unwrap(), start));
            da.total_cmp(&db)
        })
        .unwrap_or(0);
    let mut points = left.remove(first);
    if near(*points.last().unwrap(), start) < near(points[0], start) {
        points.reverse();
    }
    while !left.is_empty() {
        let tail = *points.last().unwrap();
        let Some(i) = left
            .iter()
            .position(|t| t[0] == tail || *t.last().unwrap() == tail)
        else {
            return Err(format!(
                "the centreline's traces don't chain ({} left at {tail:?})",
                left.len()
            ));
        };
        let mut next = left.remove(i);
        if next[0] != tail {
            next.reverse();
        }
        points.extend(next.into_iter().skip(1));
    }
    snap(&mut points, start, true);
    snap(&mut points, end, false);
    let points = simplify(&points);
    if points.len() < 2 {
        return Err("the centreline is a point".to_string());
    }
    let corners: Vec<Point> = points.iter().map(|p| Point::Int(*p)).collect();
    Ok(Polyline::from_points(&corners))
}

/// Replaces the first (or last) point by `at` if it is at most 3 units
/// away, else adds `at` there.
fn snap(points: &mut Vec<IntPoint>, at: IntPoint, front: bool) {
    let i = if front { 0 } else { points.len() - 1 };
    if points[i].distance(at) <= 3.0 {
        points[i] = at;
    } else if front {
        points.insert(0, at);
    } else {
        points.push(at);
    }
}

/// Drops repeated points and the middle of three collinear points.
pub fn simplify(points: &[IntPoint]) -> Vec<IntPoint> {
    let mut out: Vec<IntPoint> = Vec::with_capacity(points.len());
    for p in points {
        if out.last() == Some(p) {
            continue;
        }
        if out.len() >= 2 {
            let a = out[out.len() - 2];
            let b = out[out.len() - 1];
            let cross = (i64::from(b.x) - i64::from(a.x)) * (i64::from(p.y) - i64::from(a.y))
                - (i64::from(b.y) - i64::from(a.y)) * (i64::from(p.x) - i64::from(a.x));
            let dot = (i64::from(b.x) - i64::from(a.x)) * (i64::from(p.x) - i64::from(b.x))
                + (i64::from(b.y) - i64::from(a.y)) * (i64::from(p.y) - i64::from(b.y));
            if cross == 0 && dot >= 0 {
                out.pop();
            }
        }
        out.push(*p);
    }
    out
}

/// The smallest slack of `polyline` at half width `hw` on `layer` to the
/// items of other nets than `nets`: the exact distance from the
/// polyline's segments to the item's shape there, minus `hw` and the
/// clearance between `clearance_class` and the item's class. Returns the
/// slack and the item, `None` without foreign items on the layer.
pub fn min_foreign_slack(
    board: &BasicBoard,
    polyline: &Polyline,
    layer: i32,
    hw: i32,
    clearance_class: i32,
    nets: &[i32],
) -> Option<(f64, ItemId)> {
    let corners: Vec<FloatPoint> = polyline.corners().iter().map(Point::to_float).collect();
    let mut best: Option<(f64, ItemId)> = None;
    for item in board.items() {
        if nets.iter().any(|n| item.contains_net(*n)) || item.as_board_outline().is_some() {
            continue;
        }
        let clearance = board.rules.clearance_matrix.get_value(
            clearance_class,
            item.clearance_class(),
            layer,
            false,
        );
        for index in 0..board.tile_shape_count(item) {
            if item.shape_layer(index) != layer {
                continue;
            }
            let Some(shape) = board.tile_shape(item, index) else {
                continue;
            };
            let polygon = shape.corner_approx_arr();
            let d = corners
                .windows(2)
                .map(|s| super::measure::segment_polygon_distance(s[0], s[1], &polygon))
                .fold(f64::INFINITY, f64::min);
            let slack = d - f64::from(hw) - f64::from(clearance);
            if best.is_none_or(|(b, _)| slack < b) {
                best = Some((slack, item.id()));
            }
        }
    }
    best
}
