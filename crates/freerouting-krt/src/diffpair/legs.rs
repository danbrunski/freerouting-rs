//! Terminal legs and the insertion (X08.4).
//!
//! **One attempt** routes the centreline between the two terminals
//! ([`super::centreline`]), offsets it into the P and N tracks
//! ([`super::offset`]) and inserts both in one B09 transaction
//! ([`RoutingBoard::try_change`], new items checked) together with the
//! four legs: at each end, a pin whose centre is not a track end (a
//! Freerouting contact is at the pin centre) is joined to its track by
//! Freerouting's connection router (single-ended, the pair's track width,
//! the pair's layer, no vias, no ripup, only that track as destination).
//! The leg's insertion may split the track where it lands; the dead ends
//! this leaves of P and N are removed (`remove_trace_tails`). The
//! transaction's new traces of P and N are then user fixed, so no later
//! pass, ripup or pull-tight moves one track of the pair (X09 makes the
//! optimizer pair-aware); the SES marks them `protect`. The transaction is
//! kept only if every leg routed and the exact DRC finds no clearance
//! violation among the new items; otherwise the board is as before.
//!
//! **Polarity.** P takes the side of the centreline its start pin is on
//! (KRT's cross-product rule); a pair whose P pin is on the other side at
//! the far end is reported as swapping, and its legs must cross the
//! partner, which the DRC then refuses.
//!
//! **Attempts** (KRT's hybrid fallback): first the direct attempt, the
//! terminals at the midpoints between the P and N pins; then the coupled
//! middle with the terminals moved out by 1, 2 and 4 mm
//! ([`SETBACKS_UM`]) along each end's escape direction, with longer
//! single-ended joins. Each attempt is one log line. The first attempt
//! that commits ends the pair.
//!
//! **Heading runs.** The centreline leaves each terminal straight along the
//! end's escape direction, past the pins' pads by the compound half width
//! and for at least one P–N centre distance, before the maze search takes
//! over (KRT's pose search starts with the stub's heading;
//! this is the gridless stand-in). Without it a search that turns at once
//! skews the two offset ends along the track, and the partner's join then
//! passes under the other track's start.
//!
//! **Joins.** Each attempt first extends the tracks straight to the pin
//! centres (KRT's connectors). If the DRC refuses that, the same tracks
//! are joined by routed legs instead (KRT's hybrid legs).

use std::collections::BTreeSet;

use freerouting::autoroute::AttemptState;
use freerouting::autoroute::batch::connection::maze_budget_ms;
use freerouting::autoroute::connection::{Ripped, RouteLimits, route_connection};
use freerouting::autoroute::control::AutorouteControl;
use freerouting::autoroute::session::search::SearchSettings;
use freerouting::autoroute::session::sink::NoopSink;
use freerouting::autoroute::stages::StageCtx;
use freerouting::board::RoutingBoard;
use freerouting::board::items::{ItemId, StopConnectionOption};
use freerouting::board::search::WorkLimit;
use freerouting::board::structure::FixedState;
use freerouting::board::transaction::{CheckPolicy, TransactionResult};
use freerouting::geometry::{IntPoint, Line, Polyline};

use super::centreline::{
    CentrelineParams, PairEnds, net_clearance_class, pad_reach, pair_ends, route_centreline,
    simplify,
};
use super::model::Pair;
use super::offset::{int_corners, offset_pair};
use crate::length::Units;

/// The setbacks of the fallback attempts, in µm (KRT's hybrid: 1, 2, 4 mm).
pub const SETBACKS_UM: [f64; 3] = [1000.0, 2000.0, 4000.0];

/// The site name of a leg search's work limit.
const LEG_SITE: &str = "krt_diffpair_leg";

/// The tuning of the pair router.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PairOptions {
    /// The centreline search budget, in nominal milliseconds of work
    /// (B02.7: 64,000 units each).
    pub centreline_budget_ms: i64,
    /// Test hook: the number of attempts, from the first, whose joins are
    /// failed on purpose (0 in a job).
    pub forced_leg_failures: usize,
    /// Try straight connectors before routed legs (true in a job; false
    /// tests the legs alone).
    pub connectors: bool,
}

impl Default for PairOptions {
    fn default() -> Self {
        PairOptions {
            centreline_budget_ms: maze_budget_ms(1),
            forced_leg_failures: 0,
            connectors: true,
        }
    }
}

/// How a pair ended.
#[derive(Clone, Debug)]
pub enum PairStatus {
    /// Inserted at `setback_um` (0 for the direct attempt).
    Routed {
        setback_um: f64,
        /// The coupled section as offset: the P and N tracks.
        p_coupled: Polyline,
        n_coupled: Polyline,
        layer: i32,
        /// The centreline's length in board units.
        centreline_length: f64,
    },
    /// Both nets were connected already.
    AlreadyRouted,
    /// The pair can't be routed by X08 (pins, layers).
    Skipped(String),
    /// Every attempt failed; the last reason.
    Failed(String),
    /// The cancel token stopped the stage.
    Stopped,
}

/// What happened to one pair: its status and one line per attempt.
#[derive(Clone, Debug)]
pub struct PairOutcome {
    pub pair: Pair,
    pub status: PairStatus,
    pub attempts: Vec<String>,
}

fn connected(board: &RoutingBoard, net: i32) -> bool {
    board.connected_sets(net).len() <= 1
}

/// Routes `pair` on `board` ([module docs](self)).
pub fn route_pair(
    board: &mut RoutingBoard,
    ctx: &StageCtx,
    pair: &Pair,
    options: &PairOptions,
) -> PairOutcome {
    let mut outcome = PairOutcome {
        pair: pair.clone(),
        status: PairStatus::Failed(String::new()),
        attempts: Vec::new(),
    };
    if connected(board, pair.p) && connected(board, pair.n) {
        outcome.status = PairStatus::AlreadyRouted;
        return outcome;
    }
    let ends = match pair_ends(board, pair) {
        Ok(e) => e,
        Err(e) => {
            outcome.status = PairStatus::Skipped(e);
            return outcome;
        }
    };
    let units = Units::of(board);
    let setbacks = std::iter::once(0.0).chain(SETBACKS_UM);
    for (attempt, setback_um) in setbacks.enumerate() {
        if ctx.cancel.is_stop_requested() {
            outcome.status = PairStatus::Stopped;
            return outcome;
        }
        let name = if attempt == 0 {
            "direct".to_string()
        } else {
            format!("setback {} mm", setback_um / 1000.0)
        };
        let forced = attempt < options.forced_leg_failures;
        let setback = units.from_um(setback_um);
        match try_attempt(board, ctx, pair, &ends, setback, forced, options) {
            Ok((p_coupled, n_coupled, length)) => {
                outcome.attempts.push(format!(
                    "{name}: inserted, centreline {:.4} mm",
                    units.to_um(length) / 1000.0
                ));
                outcome.status = PairStatus::Routed {
                    setback_um,
                    p_coupled,
                    n_coupled,
                    layer: ends.layer,
                    centreline_length: length,
                };
                return outcome;
            }
            Err(e) => {
                outcome.attempts.push(format!("{name}: {e}"));
                outcome.status = PairStatus::Failed(e);
            }
        }
    }
    outcome
}

/// The side of `p` relative to the direction `a → b`: true for the left.
fn left_of(a: IntPoint, b: IntPoint, p: IntPoint) -> bool {
    let cross = (i64::from(b.x) - i64::from(a.x)) * (i64::from(p.y) - i64::from(a.y))
        - (i64::from(b.y) - i64::from(a.y)) * (i64::from(p.x) - i64::from(a.x));
    cross >= 0
}

/// The planned coupled tracks of one attempt.
struct Plan {
    p_poly: Polyline,
    n_poly: Polyline,
    length: f64,
    swap: &'static str,
}

/// How the pins are joined to the coupled tracks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Join {
    /// KRT's connectors: each track extended straight to its pin centres.
    Connectors,
    /// KRT's hybrid legs: routed by Freerouting's connection router.
    Legs,
}

/// One attempt at `setback` board units: straight connectors, then routed
/// legs; the coupled tracks as offset and the centreline length.
fn try_attempt(
    board: &mut RoutingBoard,
    ctx: &StageCtx,
    pair: &Pair,
    ends: &PairEnds,
    setback: f64,
    forced: bool,
    options: &PairOptions,
) -> Result<(Polyline, Polyline, f64), String> {
    let plan = plan(board, ctx, pair, ends, setback, options)?;
    if forced {
        return Err(format!("leg failed (forced){}", plan.swap));
    }
    let connectors = if options.connectors {
        match insert_plan(board, ctx, pair, ends, &plan, Join::Connectors) {
            Ok(()) => return Ok((plan.p_poly, plan.n_poly, plan.length)),
            Err(e) => e,
        }
    } else {
        "not tried".to_string()
    };
    match insert_plan(board, ctx, pair, ends, &plan, Join::Legs) {
        Ok(()) => Ok((plan.p_poly, plan.n_poly, plan.length)),
        Err(legs) => Err(format!(
            "connectors: {connectors}; legs: {legs}{}",
            plan.swap
        )),
    }
}

/// The centreline, its offset and the polarity of one attempt.
fn plan(
    board: &RoutingBoard,
    ctx: &StageCtx,
    pair: &Pair,
    ends: &PairEnds,
    setback: f64,
    options: &PairOptions,
) -> Result<Plan, String> {
    let layer = ends.layer;
    let (t_start, t_end) = (
        ends.start.terminal(&ends.end, setback),
        ends.end.terminal(&ends.start, setback),
    );
    // The heading runs ([module docs](self)).
    let run = |end: &super::centreline::End, other: &super::centreline::End| {
        let past_pads = pad_reach(board, end, other, layer) - setback
            + f64::from(pair.compound_half_width(layer));
        setback + past_pads.max(f64::from(pair.centre_distance(layer)))
    };
    let (h_start, h_end) = (
        ends.start.terminal(&ends.end, run(&ends.start, &ends.end)),
        ends.end.terminal(&ends.start, run(&ends.end, &ends.start)),
    );
    let params = CentrelineParams {
        layer,
        start: h_start,
        end: h_end,
        work_budget: options.centreline_budget_ms,
    };
    let routed = route_centreline(board, ctx, pair, &params)?;
    let routed = int_corners(&routed).ok_or("the centreline has a rational corner")?;
    let mut points = vec![t_start];
    points.extend(routed);
    points.push(t_end);
    let corners = simplify(&points);
    if corners.len() < 2 {
        return Err("the centreline is a point".to_string());
    }
    let centre = Polyline::from_points(
        &corners
            .iter()
            .map(|p| freerouting::geometry::Point::Int(*p))
            .collect::<Vec<_>>(),
    );
    let tracks = offset_pair(&corners, pair.centre_distance(layer))?;
    let n = corners.len();
    let p_left = left_of(corners[0], corners[1], ends.start.p_centre);
    let far_left = left_of(corners[n - 2], corners[n - 1], ends.end.p_centre);
    let swap = if p_left == far_left {
        ""
    } else {
        " (polarity swaps between the ends)"
    };
    let (p_poly, n_poly) = if p_left {
        (tracks.left, tracks.right)
    } else {
        (tracks.right, tracks.left)
    };
    Ok(Plan {
        p_poly,
        n_poly,
        length: centre.length_approx(),
        swap,
    })
}

/// `track` extended by straight connectors from `from` to its first
/// corner and from its last corner to `to` (skipped where they coincide).
fn with_connectors(track: &Polyline, from: IntPoint, to: IntPoint) -> Result<Polyline, String> {
    let lines = track.lines();
    let first = track.first_corner().as_int().ok_or("rational track end")?;
    let last = track.last_corner().as_int().ok_or("rational track end")?;
    let cap = |at: IntPoint, other: IntPoint| {
        let (dx, dy) = (other.x - at.x, other.y - at.y);
        Line::new(at, IntPoint::new(at.x - dy, at.y + dx))
    };
    let mut out: Vec<Line> = Vec::with_capacity(lines.len() + 2);
    if from == first {
        out.push(lines[0]);
    } else {
        out.push(cap(from, first));
        out.push(Line::new(from, first));
    }
    out.extend_from_slice(&lines[1..lines.len() - 1]);
    if to == last {
        out.push(lines[lines.len() - 1]);
    } else {
        out.push(Line::new(last, to));
        out.push(cap(to, last));
    }
    let polyline = Polyline::new(&out);
    if polyline.is_empty() {
        return Err("the connectors made an empty track".to_string());
    }
    Ok(polyline)
}

/// Inserts `plan` with `join` in one transaction ([module docs](self)).
fn insert_plan(
    board: &mut RoutingBoard,
    ctx: &StageCtx,
    pair: &Pair,
    ends: &PairEnds,
    plan: &Plan,
    join: Join,
) -> Result<(), String> {
    let layer = ends.layer;
    let hw = pair.half_width(layer);
    let last_id = board.items().map(|i| i.id().0).max().unwrap_or(0);
    let (p_poly, n_poly) = match join {
        Join::Connectors => (
            with_connectors(&plan.p_poly, ends.start.p_centre, ends.end.p_centre)?,
            with_connectors(&plan.n_poly, ends.start.n_centre, ends.end.n_centre)?,
        ),
        Join::Legs => (plan.p_poly.clone(), plan.n_poly.clone()),
    };
    let result = board.try_change(
        |b| {
            insert_track(b, p_poly, layer, hw, pair.p)?;
            insert_track(b, n_poly, layer, hw, pair.n)?;
            if join == Join::Legs {
                for end in [&ends.start, &ends.end] {
                    leg(b, ctx, pair, layer, end.p_pin, last_id)?;
                    leg(b, ctx, pair, layer, end.n_pin, last_id)?;
                }
            }
            for net in [pair.p, pair.n] {
                b.remove_trace_tails(net, StopConnectionOption::None);
            }
            let new: Vec<ItemId> = b
                .items()
                .filter(|i| i.id().0 > last_id && i.is_trace())
                .filter(|i| i.contains_net(pair.p) || i.contains_net(pair.n))
                .map(|i| i.id())
                .collect();
            for id in new {
                b.set_fixed_state(id, FixedState::UserFixed);
            }
            if !(connected_pins(b, pair.p) && connected_pins(b, pair.n)) {
                return Err("the pins are not all connected".to_string());
            }
            Ok(())
        },
        CheckPolicy::NewItemsOnly,
    );
    match result {
        TransactionResult::Committed(()) => Ok(()),
        TransactionResult::Reverted { violations, .. } => {
            drop_redo(board);
            Err(format!(
                "the DRC found {} clearance violation(s) among the new items; reverted",
                violations.len()
            ))
        }
        TransactionResult::Aborted(e) => {
            drop_redo(board);
            Err(format!("{e}; reverted"))
        }
    }
}

/// True if every pin of `net` is in one connected set.
fn connected_pins(board: &RoutingBoard, net: i32) -> bool {
    let sets = board.connected_sets(net);
    let pins: Vec<ItemId> = board
        .items()
        .filter(|i| i.is_pin() && i.contains_net(net))
        .map(|i| i.id())
        .collect();
    sets.iter().any(|s| pins.iter().all(|p| s.contains(p)))
}

fn insert_track(
    board: &mut RoutingBoard,
    polyline: Polyline,
    layer: i32,
    hw: i32,
    net: i32,
) -> Result<ItemId, String> {
    let cl = net_clearance_class(board, net);
    board
        .insert_trace_without_cleaning(polyline, layer, hw, &[net], cl, FixedState::Unfixed)
        .ok_or_else(|| format!("net {net}'s track was refused"))
}

/// Joins `pin` to its net's traces made since `last_id` unless a trace
/// already ends at its centre.
fn leg(
    board: &mut RoutingBoard,
    ctx: &StageCtx,
    pair: &Pair,
    layer: i32,
    pin: ItemId,
    last_id: i32,
) -> Result<(), String> {
    let net = board
        .item(pin)
        .map(|i| i.net_number(0))
        .ok_or("the pin is gone")?;
    let dest: BTreeSet<ItemId> = board
        .items()
        .filter(|i| i.id().0 > last_id && i.is_trace() && i.contains_net(net))
        .map(|i| i.id())
        .collect();
    if board.normal_contacts(pin).iter().any(|c| dest.contains(c)) {
        return Ok(());
    }
    let mut control = AutorouteControl::for_net_default(board, net, ctx.board_settings)
        .map_err(|e| format!("leg control: {e}"))?;
    let class = control.trace_clearance_class_index;
    let hw = pair.half_width(layer);
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
    let limits = RouteLimits {
        settings: SearchSettings::from_router(ctx.board_settings),
        stop: Some(ctx.cancel),
        work_limit: Some(WorkLimit::with_site(
            board.search_trees().work_meter(),
            maze_budget_ms(1),
            LEG_SITE,
        )),
    };
    let mut ripped = BTreeSet::new();
    let ripped = Ripped {
        items: &mut ripped,
        costs: None,
    };
    let start: BTreeSet<ItemId> = [pin].into();
    let (result, _) = route_connection(board, &start, &dest, &control, limits, ripped, NoopSink);
    if result.state != AttemptState::Routed {
        return Err(format!(
            "leg of net {net} failed: {:?} {}",
            result.state, result.details
        )
        .trim()
        .to_string());
    }
    Ok(())
}

/// Drops the redo data a reverted transaction leaves (as X05's inserter).
fn drop_redo(board: &mut RoutingBoard) {
    board.generate_snapshot();
    board.pop_snapshot();
}
