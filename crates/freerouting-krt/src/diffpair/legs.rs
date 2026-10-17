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
//! **Polarity** (X09.2). P takes the side of the centreline its start pin
//! is on (KRT's cross-product rule). A pair whose P pin is on the other
//! side at the far end ([`crossing`]) is reported and not tried: its tracks
//! would cross, and KRT's fix, a pad swap in the schematic, is not one
//! freerouting-rs makes.
//!
//! **Layer changes** (X09.1). When the ends sit on different layers the
//! centreline is searched with a compound via ([`super::vias`]); each
//! section is offset on its own layer, the via pair goes in at each layer
//! change, and each section's tracks run straight from their offset ends
//! (moved back from the change by half the via spacing) to the vias.
//!
//! **Skew** (X09.4). With [`PairOptions::skew_um`], the connector joins'
//! tracks get [`super::skew`]'s bumps before the transaction.
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
    CentrelineParams, PairEnds, net_clearance_class, pad_reach, pair_ends, route_sections, simplify,
};
use super::model::Pair;
use super::offset::offset_pair;
use super::skew::{SkewOutcome, compensate_skew};
use super::vias::{ViaSpec, via_in_pad, via_pair, via_spec};
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
    /// The intra-pair skew tolerance in µm (X09.4,
    /// `router.diff_pairs.skew_um`); `None` leaves the skew alone.
    pub skew_um: Option<f64>,
}

impl Default for PairOptions {
    fn default() -> Self {
        PairOptions {
            centreline_budget_ms: maze_budget_ms(1),
            forced_leg_failures: 0,
            connectors: true,
            skew_um: None,
        }
    }
}

/// How a pair ended.
#[derive(Clone, Debug)]
pub enum PairStatus {
    /// Inserted at `setback_um` (0 for the direct attempt).
    Routed {
        setback_um: f64,
        /// The coupled tracks as offset on the first layer: P and N.
        p_coupled: Polyline,
        n_coupled: Polyline,
        /// The first layer.
        layer: i32,
        /// Every layer the pair runs on, in order from the start (X09.1).
        layers: Vec<i32>,
        /// The P and N via centres at each layer change (X09.1).
        vias: Vec<(IntPoint, IntPoint)>,
        /// The skew compensation (X09.4), when asked for and needed.
        skew: Option<SkewOutcome>,
        /// The centreline's length in board units.
        centreline_length: f64,
    },
    /// Both nets were connected already.
    AlreadyRouted,
    /// The pair can't be routed by X08 (pins, layers).
    Skipped(String),
    /// P and N would cross (X09.2): reported, nothing routed by the stage.
    Crossing(String),
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
    if let Some(why) = crossing(&ends) {
        outcome.status = PairStatus::Crossing(why);
        return outcome;
    }
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
            Ok(Made { plan, skew }) => {
                outcome.attempts.push(format!(
                    "{name}: inserted, centreline {:.4} mm{}",
                    units.to_um(plan.length) / 1000.0,
                    match plan.vias.len() {
                        0 => String::new(),
                        n => format!(", {n} via pair(s)"),
                    }
                ));
                let layers = plan.sections.iter().map(|s| s.layer).collect();
                let first = plan.sections.into_iter().next().expect("a section");
                outcome.status = PairStatus::Routed {
                    setback_um,
                    p_coupled: first.p,
                    n_coupled: first.n,
                    layer: ends.layer,
                    layers,
                    vias: plan.vias,
                    skew,
                    centreline_length: plan.length,
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

/// The polarity of a pair (X09.2): `None` if P is on the same side of the
/// pair's direction of travel at both ends, else why not. The centreline
/// leaves the start along its escape direction and reaches the end against
/// the end's (the heading runs), so this is the side rule the offset uses.
pub fn crossing(ends: &PairEnds) -> Option<String> {
    let side = |end: &super::centreline::End, travel: (f64, f64)| {
        let m = end.midpoint();
        let (px, py) = (
            f64::from(end.p_centre.x) - f64::from(m.x),
            f64::from(end.p_centre.y) - f64::from(m.y),
        );
        travel.0 * py - travel.1 * px >= 0.0
    };
    let (sx, sy) = ends.start.escape(&ends.end);
    let (ex, ey) = ends.end.escape(&ends.start);
    let start_left = side(&ends.start, (sx, sy));
    let end_left = side(&ends.end, (-ex, -ey));
    if start_left == end_left {
        return None;
    }
    let s = |left: bool| if left { "left" } else { "right" };
    Some(format!(
        "P is on the {} of the pair at the start and on the {} at the end, so P and N \
         would cross",
        s(start_left),
        s(end_left)
    ))
}

/// One layer's planned coupled tracks.
struct TrackPlan {
    layer: i32,
    p: Polyline,
    n: Polyline,
}

/// The planned coupled tracks of one attempt.
struct Plan {
    /// One per centreline section, in order.
    sections: Vec<TrackPlan>,
    /// The P and N via centres between consecutive sections.
    vias: Vec<(IntPoint, IntPoint)>,
    /// The via, when the pair changes layer.
    via: Option<ViaSpec>,
    length: f64,
}

/// How the pins are joined to the coupled tracks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Join {
    /// KRT's connectors: each track extended straight to its pin centres.
    Connectors,
    /// KRT's hybrid legs: routed by Freerouting's connection router.
    Legs,
}

/// What a committed attempt made.
struct Made {
    plan: Plan,
    skew: Option<SkewOutcome>,
}

/// One attempt at `setback` board units: straight connectors, then routed
/// legs.
fn try_attempt(
    board: &mut RoutingBoard,
    ctx: &StageCtx,
    pair: &Pair,
    ends: &PairEnds,
    setback: f64,
    forced: bool,
    options: &PairOptions,
) -> Result<Made, String> {
    let plan = plan(board, ctx, pair, ends, setback, options)?;
    if forced {
        return Err("leg failed (forced)".to_string());
    }
    let connectors = if options.connectors {
        match insert_plan(board, ctx, pair, ends, &plan, Join::Connectors, options) {
            Ok(skew) => return Ok(Made { plan, skew }),
            Err(e) => e,
        }
    } else {
        "not tried".to_string()
    };
    match insert_plan(board, ctx, pair, ends, &plan, Join::Legs, options) {
        Ok(skew) => Ok(Made { plan, skew }),
        Err(legs) => Err(format!("connectors: {connectors}; legs: {legs}")),
    }
}

/// The centreline, its offset and its vias for one attempt.
fn plan(
    board: &RoutingBoard,
    ctx: &StageCtx,
    pair: &Pair,
    ends: &PairEnds,
    setback: f64,
    options: &PairOptions,
) -> Result<Plan, String> {
    let (layer, end_layer) = (ends.layer, ends.end_layer);
    let (t_start, t_end) = (
        ends.start.terminal(&ends.end, setback),
        ends.end.terminal(&ends.start, setback),
    );
    let via = if ends.changes_layer() {
        Some(via_spec(board, pair, layer, end_layer, ctx.board_settings)?)
    } else {
        None
    };
    // The heading runs ([module docs](self)); with a layer change they also
    // clear the pads by a via pair's room, since the search may change
    // layer right at a terminal.
    let via_room = via.map_or(0.0, |v| {
        let clearance = board.rules.clearance_matrix.get_value(
            v.clearance_class,
            v.clearance_class,
            layer,
            false,
        );
        f64::from(v.compound().radius + clearance)
    });
    let run = |end: &super::centreline::End, other: &super::centreline::End, layer: i32| {
        let room = f64::from(pair.compound_half_width(layer)).max(via_room);
        let past_pads = pad_reach(board, end, other, layer) - setback + room;
        setback + past_pads.max(f64::from(pair.centre_distance(layer)))
    };
    let (h_start, h_end) = (
        ends.start
            .terminal(&ends.end, run(&ends.start, &ends.end, layer)),
        ends.end
            .terminal(&ends.start, run(&ends.end, &ends.start, end_layer)),
    );
    let params = CentrelineParams {
        layer,
        start: h_start,
        end: h_end,
        work_budget: options.centreline_budget_ms,
    };
    let compound = via.map(|v| v.compound());
    let mut sections = route_sections(board, ctx, pair, &params, end_layer, compound.as_ref())?;
    sections[0].corners.insert(0, t_start);
    sections.last_mut().expect("a section").corners.push(t_end);
    let mut length = 0.0;
    for section in &mut sections {
        section.corners = simplify(&section.corners);
        if section.corners.len() < 2 {
            return Err("a section of the centreline is a point".to_string());
        }
        length += section
            .corners
            .windows(2)
            .map(|w| w[0].distance(w[1]))
            .sum::<f64>();
    }
    let first = &sections[0].corners;
    let last = &sections[sections.len() - 1].corners;
    let n = last.len();
    let p_left = left_of(first[0], first[1], ends.start.p_centre);
    if p_left != left_of(last[n - 2], last[n - 1], ends.end.p_centre) {
        return Err("the centreline swaps the polarity between the ends".to_string());
    }
    let mut vias = Vec::new();
    for w in sections.windows(2) {
        let spec = via
            .as_ref()
            .ok_or("the centreline changes layer without a via")?;
        let (a, b) = (&w[0].corners, &w[1].corners);
        let at = a[a.len() - 1];
        let (p, n) = via_pair(spec, at, (a[a.len() - 2], at), (at, b[1]), p_left)?;
        for centre in [p, n] {
            if let Some(pin) = via_in_pad(board, centre, spec.from_layer, spec.to_layer) {
                return Err(format!(
                    "a via at {centre:?} would be in the pad of {pin:?}"
                ));
            }
        }
        vias.push((p, n));
    }
    if let Some(spec) = &via {
        trim_at_vias(&mut sections, spec);
    }
    let mut tracks = Vec::with_capacity(sections.len());
    for section in &sections {
        let offset = offset_with_repair(&section.corners, pair.centre_distance(section.layer))?;
        let (p, n) = if p_left {
            (offset.left, offset.right)
        } else {
            (offset.right, offset.left)
        };
        tracks.push(TrackPlan {
            layer: section.layer,
            p,
            n,
        });
    }
    Ok(Plan {
        sections: tracks,
        vias,
        via,
        length,
    })
}

/// [`offset_pair`], and where it refuses (a segment too short for its
/// corners' miters, a miter over KRT's cap), the same with the interior
/// corner next to the shortest segment dropped, until it succeeds or only
/// the ends are left. The search found the original corners; the dropped
/// ones shorten a step of it, and the pair's DRC gate judges the result.
fn offset_with_repair(
    corners: &[IntPoint],
    centre_distance: i32,
) -> Result<super::offset::OffsetPair, String> {
    let mut corners = corners.to_vec();
    let first_error = match offset_pair(&corners, centre_distance) {
        Ok(offset) => return Ok(offset),
        Err(e) => e,
    };
    while corners.len() > 2 {
        let len = |a: IntPoint, b: IntPoint| a.distance(b);
        let drop = (1..corners.len() - 1)
            .min_by(|&a, &b| {
                let la = len(corners[a - 1], corners[a]).min(len(corners[a], corners[a + 1]));
                let lb = len(corners[b - 1], corners[b]).min(len(corners[b], corners[b + 1]));
                la.total_cmp(&lb)
            })
            .expect("an interior corner");
        corners.remove(drop);
        corners = simplify(&corners);
        if let Ok(offset) = offset_pair(&corners, centre_distance) {
            return Ok(offset);
        }
    }
    Err(first_error)
}

/// Moves each section's corner at a layer change back along its segment by
/// half the via spacing, where the segment is long enough, so the tracks
/// run from their offset ends to the vias as KRT's approach and exit
/// tracks instead of doubling back where the centreline turns at the via
/// ([`super::vias`]).
fn trim_at_vias(sections: &mut [super::centreline::Section], spec: &ViaSpec) {
    let back = f64::from(spec.spacing) / 2.0;
    let moved = |at: IntPoint, towards: IntPoint| -> Option<IntPoint> {
        let (dx, dy) = (
            f64::from(towards.x) - f64::from(at.x),
            f64::from(towards.y) - f64::from(at.y),
        );
        let len = dx.hypot(dy);
        (len > back + 2.0).then(|| {
            IntPoint::new(
                (f64::from(at.x) + dx / len * back).round() as i32,
                (f64::from(at.y) + dy / len * back).round() as i32,
            )
        })
    };
    let count = sections.len();
    for (k, section) in sections.iter_mut().enumerate() {
        let corners = &mut section.corners;
        let n = corners.len();
        if k + 1 < count
            && let Some(p) = moved(corners[n - 1], corners[n - 2])
        {
            corners[n - 1] = p;
        }
        if k > 0
            && let Some(p) = moved(corners[0], corners[1])
        {
            corners[0] = p;
        }
    }
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

/// The tracks of `plan` with `join` at the pins and connectors to the
/// vias: per section `(layer, P track, N track)`.
fn joined_tracks(
    ends: &PairEnds,
    plan: &Plan,
    join: Join,
) -> Result<Vec<(i32, Polyline, Polyline)>, String> {
    let last = plan.sections.len() - 1;
    let own_first = |t: &Polyline| t.first_corner().as_int().ok_or("rational track end");
    let own_last = |t: &Polyline| t.last_corner().as_int().ok_or("rational track end");
    let mut out = Vec::with_capacity(plan.sections.len());
    for (k, section) in plan.sections.iter().enumerate() {
        let (p_from, n_from) = match (k, join) {
            (0, Join::Connectors) => (ends.start.p_centre, ends.start.n_centre),
            (0, Join::Legs) => (own_first(&section.p)?, own_first(&section.n)?),
            _ => plan.vias[k - 1],
        };
        let (p_to, n_to) = match (k == last, join) {
            (true, Join::Connectors) => (ends.end.p_centre, ends.end.n_centre),
            (true, Join::Legs) => (own_last(&section.p)?, own_last(&section.n)?),
            _ => plan.vias[k],
        };
        out.push((
            section.layer,
            with_connectors(&section.p, p_from, p_to)?,
            with_connectors(&section.n, n_from, n_to)?,
        ));
    }
    Ok(out)
}

/// Inserts `plan` with `join` in one transaction ([module docs](self));
/// the skew compensation it made, if any.
fn insert_plan(
    board: &mut RoutingBoard,
    ctx: &StageCtx,
    pair: &Pair,
    ends: &PairEnds,
    plan: &Plan,
    join: Join,
    options: &PairOptions,
) -> Result<Option<SkewOutcome>, String> {
    let last_id = board.items().map(|i| i.id().0).max().unwrap_or(0);
    let mut tracks = joined_tracks(ends, plan, join)?;
    let skew = match (join, options.skew_um) {
        (Join::Connectors, Some(tolerance_um)) => {
            compensate_skew(board, pair, ends, &mut tracks, tolerance_um)?
        }
        _ => None,
    };
    let result = board.try_change(
        |b| {
            if let Some(spec) = &plan.via {
                for (p, n) in &plan.vias {
                    insert_via(b, spec, *p, pair.p);
                    insert_via(b, spec, *n, pair.n);
                }
            }
            for (layer, p_poly, n_poly) in tracks {
                let hw = pair.half_width(layer);
                insert_track(b, p_poly, layer, hw, pair.p)?;
                insert_track(b, n_poly, layer, hw, pair.n)?;
            }
            if join == Join::Legs {
                for (end, layer) in [(&ends.start, ends.layer), (&ends.end, ends.end_layer)] {
                    leg(b, ctx, pair, layer, end.p_pin, last_id)?;
                    leg(b, ctx, pair, layer, end.n_pin, last_id)?;
                }
            }
            for net in [pair.p, pair.n] {
                b.remove_trace_tails(net, StopConnectionOption::None);
            }
            let new: Vec<ItemId> = b
                .items()
                .filter(|i| i.id().0 > last_id && (i.is_trace() || i.is_via()))
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
        TransactionResult::Committed(()) => Ok(skew),
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

fn insert_via(board: &mut RoutingBoard, spec: &ViaSpec, centre: IntPoint, net: i32) -> ItemId {
    board.insert_via(
        spec.padstack,
        centre,
        &[net],
        spec.clearance_class,
        FixedState::Unfixed,
        false,
    )
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
