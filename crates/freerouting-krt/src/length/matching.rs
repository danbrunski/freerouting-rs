//! The matching loop (X07.4): bumps into a net's straight runs until it is
//! in its target window, each bump checked by the exact DRC.
//!
//! **Order** (D2): groups by name, then the nets with a class window that
//! no group took, by net number; inside a group its members by net number.
//! A net's runs are the integer-cornered, 45°-legal segments of its
//! unfixed traces, longest first (ties by trace id, then segment index);
//! bumps go along a run from its start. After a run is used up the runs are
//! found again on the changed board, without the runs already used and the
//! segments the stage made, so a meander spills over to the next run.
//!
//! **One bump** replaces the trace by the same trace with the bump
//! ([`super::meander`]) in one B09 transaction
//! ([`RoutingBoard::try_change`], new items checked): the old trace is
//! removed and the new one inserted without normalization, so its ends,
//! nets, layer, width, clearance class and fixed state are the old ones.
//! The amplitude is planned so the run's bumps share the remaining length
//! equally, at most [`MAX_AMPLITUDE_UM`] and at least the arm spacing
//! `2·hw + clearance`; it is rounded down to whole steps, so a net never
//! ends past its aim. A bump the DRC rejects is retried on the right side,
//! then at half the amplitude (left, then right), down to the minimum; a
//! position where every try fails is skipped by one pitch. A net stops when
//! it is in its window, when no run has room (the shortfall is reported),
//! or when the cancel token asks.
//!
//! Written from the idea of KiCadRoutingTools' `apply_meanders_to_route`
//! (`py_router/length_matching.py` at `0486e799`: runs longest first,
//! spilling across runs); KRT checks clearance with its own index, here the
//! board's exact DRC decides.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use freerouting::autoroute::events::CancelToken;
use freerouting::board::RoutingBoard;
use freerouting::board::items::{Item, ItemId};
use freerouting::board::structure::FixedState;
use freerouting::board::transaction::{CheckPolicy, TransactionResult};
use freerouting::geometry::{IntPoint, Line, Point, Polyline};

use super::meander::{AngleStyle, Run, Side, added_length, amplitude_for, bump_points};
use super::measure::{LengthModel, Units, measure_net};
use super::targets::{Group, Target, Tolerance, class_target, class_window, combine};
use super::timing::Timing;

/// The largest bump amplitude, KRT's `--meander-amplitude` default (1 mm).
pub const MAX_AMPLITUDE_UM: f64 = 1000.0;

/// What a net's value is measured in.
pub enum Metric<'a> {
    /// Length in board units ([`super::measure`]).
    Length(&'a LengthModel),
    /// Delay in picoseconds ([`super::timing`]).
    Time(&'a Timing),
}

impl Metric<'_> {
    /// The value of `net`.
    pub fn value(&self, board: &RoutingBoard, net: i32) -> f64 {
        match self {
            Metric::Length(model) => measure_net(board, net, model).total(),
            Metric::Time(timing) => timing.net_delay_ps(board, net),
        }
    }

    /// The value one board unit of trace on `layer` adds.
    pub fn per_unit(&self, layer: i32) -> f64 {
        match self {
            Metric::Length(_) => 1.0,
            Metric::Time(timing) => timing.ps_per_unit(layer),
        }
    }

    /// The unit name for reports.
    pub fn unit(&self) -> &'static str {
        match self {
            Metric::Length(_) => "mm",
            Metric::Time(_) => "ps",
        }
    }
}

/// Why a net was left where it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Status {
    /// In its window (it may have been already).
    Met,
    /// Below its window; no run had room. The shortfall to `lo`, in the
    /// metric's units.
    Shortfall(f64),
    /// The group's target and the class window don't overlap.
    Conflict,
    /// The cancel token stopped the stage.
    Stopped,
}

/// One net's result.
#[derive(Clone, Debug, PartialEq)]
pub struct NetOutcome {
    /// The net.
    pub net: i32,
    /// The group, `None` for a class window.
    pub group: Option<String>,
    /// The value before the stage.
    pub before: f64,
    /// The value after.
    pub after: f64,
    /// The target, `None` for a conflict.
    pub target: Option<Target>,
    /// Bumps inserted.
    pub bumps: usize,
    /// Bumps the DRC rejected.
    pub rejected: usize,
    /// How it ended.
    pub status: Status,
    /// True if the values are delays in ps, false for lengths in board
    /// units.
    pub time: bool,
}

/// What the matching loop did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MatchReport {
    /// One entry per net with a target, in processing order.
    pub nets: Vec<NetOutcome>,
    /// Settings errors and refusals, one line each.
    pub errors: Vec<String>,
    /// True if no stackup fitted the board.
    pub stackup_unknown: bool,
}

/// One meander run being worked on.
struct RunState {
    trace: ItemId,
    /// The trace's lines when the run was started.
    lines: Vec<Line>,
    /// The run's segment index in `lines` terms: the run is on
    /// `lines[segment + 1]`.
    segment: usize,
    run: Run,
    layer: i32,
    half_width: i32,
    nets: Vec<i32>,
    clearance_class: i32,
    fixed: FixedState,
    /// The bumps placed: `(at, amplitude, side)`, ascending `at`.
    bumps: Vec<(i64, i64, Side)>,
    /// The bump pitch in steps (set when the run is filled).
    pitch: i64,
}

/// A run's identity across board changes: layer and end points.
type RunKey = (i32, (i32, i32), (i32, i32));

fn key(layer: i32, a: IntPoint, b: IntPoint) -> RunKey {
    let (a, b) = ((a.x, a.y), (b.x, b.y));
    (layer, a.min(b), a.max(b))
}

/// The matching state of one stage run.
pub struct Matcher<'a> {
    style: AngleStyle,
    units: Units,
    cancel: &'a CancelToken,
    /// Runs already used.
    used: Vec<RunKey>,
    /// Segments the stage made.
    made: BTreeSet<RunKey>,
}

impl<'a> Matcher<'a> {
    /// A matcher for `board`.
    pub fn new(board: &RoutingBoard, cancel: &'a CancelToken) -> Matcher<'a> {
        Matcher {
            style: AngleStyle::for_restriction(board.rules.trace_angle_restriction()),
            units: Units::of(board),
            cancel,
            used: Vec::new(),
            made: BTreeSet::new(),
        }
    }

    /// Lengthens `net` toward `target` (module docs).
    pub fn lengthen(
        &mut self,
        board: &mut RoutingBoard,
        target: &Target,
        metric: &Metric,
        group: Option<&str>,
    ) -> NetOutcome {
        let net = target.net;
        let before = metric.value(board, net);
        let mut outcome = NetOutcome {
            net,
            group: group.map(str::to_string),
            before,
            after: before,
            target: Some(*target),
            bumps: 0,
            rejected: 0,
            status: Status::Met,
            time: matches!(metric, Metric::Time(_)),
        };
        loop {
            let value = metric.value(board, net);
            outcome.after = value;
            if value >= target.lo {
                outcome.status = Status::Met;
                break;
            }
            if self.cancel.is_stop_requested() {
                outcome.status = Status::Stopped;
                break;
            }
            let Some(mut state) = self.next_run(board, net) else {
                outcome.status = Status::Shortfall(target.lo - value);
                break;
            };
            self.used
                .push(key(state.layer, state.run.start, state.run.end));
            self.fill_run(board, &mut state, target, metric, &mut outcome);
        }
        outcome
    }

    /// The longest unused run of `net` (module docs).
    fn next_run(&self, board: &RoutingBoard, net: i32) -> Option<RunState> {
        let restriction = board.rules.trace_angle_restriction();
        let mut best: Option<(f64, ItemId, usize, RunState)> = None;
        for item in board.items().filter(|i| i.contains_net(net)) {
            let Some(trace) = item.as_trace() else {
                continue;
            };
            if item.fixed_state() != FixedState::Unfixed || item.net_count() != 1 {
                continue;
            }
            let corners = trace.polyline.corners();
            for (i, w) in corners.windows(2).enumerate() {
                let (Some(a), Some(b)) = (w[0].as_int(), w[1].as_int()) else {
                    continue;
                };
                let k = key(trace.layer, a, b);
                if self.made.contains(&k) || self.is_used(trace.layer, a, b) {
                    continue;
                }
                let Some(run) = Run::new(a, b, restriction) else {
                    continue;
                };
                let length = run.length();
                let better = match &best {
                    None => true,
                    Some((l, id, s, _)) => {
                        length > *l || length == *l && (item.id(), i) < (*id, *s)
                    }
                };
                if better {
                    best = Some((length, item.id(), i, run_state(item, trace, i, run)));
                }
            }
        }
        best.map(|(_, _, _, state)| state)
    }

    /// True if segment `a`–`b` on `layer` lies on a used run.
    fn is_used(&self, layer: i32, a: IntPoint, b: IntPoint) -> bool {
        self.used.iter().any(|&(l, p, q)| {
            l == layer && on_segment(p, q, (a.x, a.y)) && on_segment(p, q, (b.x, b.y))
        })
    }

    /// Bumps along one run (module docs).
    fn fill_run(
        &mut self,
        board: &mut RoutingBoard,
        state: &mut RunState,
        target: &Target,
        metric: &Metric,
        outcome: &mut NetOutcome,
    ) {
        let run = state.run;
        let step = run.step_length();
        let clearance =
            board.clearance_value(state.clearance_class, state.clearance_class, state.layer);
        let spacing = 2.0 * f64::from(state.half_width) + f64::from(clearance.max(0));
        let s = run.steps_for(spacing);
        let pitch = match self.style {
            AngleStyle::Ninety => s,
            AngleStyle::FortyFive => (3 * s + 1) / 2,
        };
        state.pitch = pitch;
        let min_amp = s.max(2 * super::meander::chamfer(pitch, self.style));
        let max_amp = run
            .steps_for(self.units.from_um(MAX_AMPLITUDE_UM))
            .max(min_amp);
        let per_unit = metric.per_unit(state.layer);
        let mut at = pitch;
        while at + 2 * pitch <= run.steps {
            if self.cancel.is_stop_requested() {
                return;
            }
            let value = metric.value(board, target.net);
            if value >= target.lo {
                return;
            }
            let needed = (target.aim - value) / per_unit;
            let full = added_length(max_amp, pitch, self.style, step);
            let count = (needed / full).ceil().max(1.0);
            let plan = amplitude_for(needed / count, pitch, self.style, step).min(max_amp);
            if plan < min_amp {
                return;
            }
            let mut placed = false;
            let mut amp = plan;
            'amps: loop {
                for side in Side::ORDER {
                    if self.try_bump(board, state, (at, amp, side)) {
                        placed = true;
                        break 'amps;
                    }
                    outcome.rejected += 1;
                }
                if amp == min_amp {
                    break;
                }
                amp = (amp / 2).max(min_amp);
            }
            if placed {
                outcome.bumps += 1;
                at += 2 * pitch;
            } else {
                at += pitch;
            }
        }
    }

    /// Inserts the run's trace with its bumps and `candidate` in one
    /// transaction; true if it was committed.
    fn try_bump(
        &mut self,
        board: &mut RoutingBoard,
        state: &mut RunState,
        candidate: (i64, i64, Side),
    ) -> bool {
        let mut bumps = state.bumps.clone();
        bumps.push(candidate);
        let Some((polyline, made)) = bumped_polyline(state, &bumps, self.style) else {
            return false;
        };
        let old = state.trace;
        let (layer, hw, cl, fixed) = (
            state.layer,
            state.half_width,
            state.clearance_class,
            state.fixed,
        );
        let nets = state.nets.clone();
        let result = board.try_change(
            |b| {
                if !b.remove_item(old) {
                    return Err(());
                }
                b.insert_trace_without_cleaning(polyline, layer, hw, &nets, cl, fixed)
                    .ok_or(())
            },
            CheckPolicy::NewItemsOnly,
        );
        match result {
            TransactionResult::Committed(id) => {
                state.trace = id;
                state.bumps = bumps;
                self.made
                    .extend(made.into_iter().map(|(a, b)| key(layer, a, b)));
                true
            }
            TransactionResult::Reverted { .. } | TransactionResult::Aborted(()) => {
                drop_redo(board);
                false
            }
        }
    }
}

fn run_state(
    item: &Item,
    trace: &freerouting::board::items::PolylineTrace,
    segment: usize,
    run: Run,
) -> RunState {
    RunState {
        trace: item.id(),
        lines: trace.polyline.lines().to_vec(),
        segment,
        run,
        layer: trace.layer,
        half_width: trace.half_width,
        nets: item.net_numbers().to_vec(),
        clearance_class: item.clearance_class(),
        fixed: item.fixed_state(),
        bumps: Vec::new(),
        pitch: 0,
    }
}

/// True if `x` lies on the segment `p`–`q` (exact).
fn on_segment(p: (i32, i32), q: (i32, i32), x: (i32, i32)) -> bool {
    let (px, py, qx, qy, xx, xy) = (
        i64::from(p.0),
        i64::from(p.1),
        i64::from(q.0),
        i64::from(q.1),
        i64::from(x.0),
        i64::from(x.1),
    );
    let cross = (qx - px) * (xy - py) - (qy - py) * (xx - px);
    cross == 0 && xx >= px.min(qx) && xx <= px.max(qx) && xy >= py.min(qy) && xy <= py.max(qy)
}

/// The run's trace with `bumps` inserted into its segment, and the bump
/// segments it adds (for [`Matcher::made`]).
fn bumped_polyline(
    state: &RunState,
    bumps: &[(i64, i64, Side)],
    style: AngleStyle,
) -> Option<(Polyline, Vec<(IntPoint, IntPoint)>)> {
    let base = *state.lines.get(state.segment + 1)?;
    let mut lines: Vec<Line> = state.lines[..=state.segment + 1].to_vec();
    let mut made = Vec::new();
    let mut sorted = bumps.to_vec();
    sorted.sort_by_key(|b| b.0);
    let mut last_end = -1;
    for &(at, amp, side) in &sorted {
        if at <= last_end {
            return None;
        }
        let pts = bump_points(&state.run, at, amp, state.pitch, side, style)?;
        for w in pts.windows(2) {
            if w[0] != w[1] {
                lines.push(Line::new(w[0], w[1]));
                made.push((w[0], w[1]));
            }
        }
        lines.push(base);
        last_end = at + state.pitch;
    }
    lines.extend_from_slice(&state.lines[state.segment + 2..]);
    let polyline = Polyline::new(&lines);
    let old = Polyline::new(&state.lines);
    if polyline.lines().len() < 3
        || polyline.first_corner() != old.first_corner()
        || polyline.last_corner() != old.last_corner()
    {
        return None;
    }
    // Every bump corner must be a corner of the new polyline.
    let corners: BTreeSet<(i32, i32)> = polyline
        .corners()
        .iter()
        .filter_map(Point::as_int)
        .map(|p| (p.x, p.y))
        .collect();
    if made
        .iter()
        .any(|(a, b)| !corners.contains(&(a.x, a.y)) || !corners.contains(&(b.x, b.y)))
    {
        return None;
    }
    Some((polyline, made))
}

/// Drops the redo data a reverted transaction leaves (as X05's inserter).
fn drop_redo(board: &mut RoutingBoard) {
    board.generate_snapshot();
    board.pop_snapshot();
}

/// The targets and outcomes of every group and class window on `board`.
pub fn run_matching(
    board: &mut RoutingBoard,
    groups: &[Group],
    model: &LengthModel,
    cancel: &CancelToken,
) -> MatchReport {
    let mut report = MatchReport {
        stackup_unknown: model.stackup_for(board).is_none(),
        ..MatchReport::default()
    };
    let timing = Timing::new(board, model);
    let units = Units::of(board);
    let mut matcher = Matcher::new(board, cancel);
    let mut grouped: BTreeSet<i32> = BTreeSet::new();
    for group in groups {
        let (metric, tolerance) = match group.tolerance {
            Tolerance::Um(um) => (Metric::Length(model), units.from_um(um)),
            Tolerance::Ps(ps) => match &timing {
                Some(t) => (Metric::Time(t), ps),
                None => {
                    report.errors.push(format!(
                        "group {}: a time tolerance ({ps} ps) needs the board stackup (the \
                         KiCad reader's, K05.1), and this board has none that fits; group skipped",
                        group.name
                    ));
                    continue;
                }
            },
        };
        let measured: Vec<(i32, f64)> = group
            .nets
            .iter()
            .map(|&n| (n, metric.value(board, n)))
            .collect();
        let targets = super::targets::group_targets(&measured, tolerance);
        for &net in &group.nets {
            if !grouped.insert(net) {
                continue;
            }
            let Some(t) = targets.iter().find(|t| t.net == net) else {
                continue;
            };
            let window = match (&metric, class_window(board, net)) {
                (Metric::Length(_), Some((min, max))) => combine(*t, min, max),
                _ => Some(*t),
            };
            let outcome = match window {
                Some(t) => matcher.lengthen(board, &t, &metric, Some(&group.name)),
                None => NetOutcome {
                    net,
                    group: Some(group.name.clone()),
                    before: metric.value(board, net),
                    after: metric.value(board, net),
                    target: None,
                    bumps: 0,
                    rejected: 0,
                    status: Status::Conflict,
                    time: matches!(metric, Metric::Time(_)),
                },
            };
            report.nets.push(outcome);
        }
    }
    let metric = Metric::Length(model);
    let margin = units.from_um(1.0).max(4.0);
    for net in 1..=board.rules.nets.max_net_number() {
        if grouped.contains(&net) {
            continue;
        }
        let Some((min, max)) = class_window(board, net) else {
            continue;
        };
        let value = metric.value(board, net);
        if let Some(t) = class_target(net, min, max, value, margin) {
            report.nets.push(matcher.lengthen(board, &t, &metric, None));
        }
    }
    report
}

impl MatchReport {
    /// The report as log lines; lengths in mm, times in ps.
    pub fn log(&self, board: &RoutingBoard) -> Vec<String> {
        let units = Units::of(board);
        let mm = |v: f64| units.to_um(v) / 1000.0;
        let mut out = Vec::new();
        if self.stackup_unknown {
            out.push(
                "krt-meander: stackup unknown: via barrels count 0 (the KiCad reader's \
                 stackup, K05.1, is needed for them)"
                    .to_string(),
            );
        }
        for e in &self.errors {
            out.push(format!("krt-meander: {e}"));
        }
        for n in &self.nets {
            let name = board
                .rules
                .nets
                .get(n.net)
                .map_or_else(|| format!("#{}", n.net), |net| net.name.clone());
            let conv = |v: f64| if n.time { v } else { mm(v) };
            let unit = if n.time { "ps" } else { "mm" };
            let mut line = format!(
                "krt-meander: net {name} ({}): {:.4} -> {:.4} {unit}",
                n.group
                    .as_deref()
                    .map_or("class window".to_string(), |g| format!("group {g}")),
                conv(n.before),
                conv(n.after)
            );
            if let Some(t) = n.target {
                let _ = write!(line, ", target [{:.4}, {:.4}]", conv(t.lo), conv(t.hi));
            }
            let _ = write!(line, ", {} bumps, {} rejected", n.bumps, n.rejected);
            match n.status {
                Status::Met => line.push_str(", met"),
                Status::Shortfall(s) => {
                    let _ = write!(line, ", shortfall {:.4} (no run has room)", conv(s));
                }
                Status::Conflict => {
                    line.push_str(", conflict: group target outside the class window")
                }
                Status::Stopped => line.push_str(", stopped"),
            }
            out.push(line);
        }
        out
    }
}
