//! The trace tightener: one struct for Java's three `TraceTightener`
//! classes, with the angle mode selecting the variant's steps.
//!
//! Port of `board/optimize/TraceTightener.java` (constructor,
//! `getInstance`, `pullTight(Polyline, …)`, `isStopRequested`,
//! `repositionLines`, `repositionLine`, `skipSegmentsOfLength0` and the dead
//! `avoidAcidTraps`) at `a773a70ec`; the 45° steps are in [`diag45`], the 90°
//! steps in [`ortho`], the any-angle steps (`TraceTightenerAnyAngle`, O02) in
//! [`any_angle`].
//!
//! Every step returns an explicit [`Outcome`]: Java signals "nothing
//! changed" by returning the same `Polyline` object, and its fixpoint loops
//! stop on that reference equality, so `Changed` is reported exactly where
//! Java built a new object, even when the new value equals the old one.
//! For the same reason each line of the working polyline carries a
//! [`LineOrigin`]: `PolylineTrace.change` keeps the tree leaves of the
//! leading and trailing lines that are the trace's own `Line` objects.
//!
//! Java's `new Polyline(Line[])` reverses lines *in the caller's array* when
//! it drops none, which happens to the three-line check polylines of
//! `repositionLine` and `smoothenCorner`; their accepted line is that
//! reversed line. The port replays this where the array is read again.
//! N06.12 left it as Java in both profiles (later reads depend on it, and no
//! fix is clearly right); N07.4 removed its inert switch, and the replays carry
//! a comment.

pub mod any_angle;
pub mod diag45;
pub mod ortho;

use std::collections::BTreeSet;
use std::sync::Arc;

use freerouting_geometry::{
    FloatPoint, IntOctagon, Line, Point, Polyline, Side, Signum, Stoppable, TileShapeOps,
};

use crate::board::BasicBoard;
use crate::board::items::ItemId;
use crate::board::search::WorkLimit;
use crate::board::structure::AngleRestriction;
use crate::optimize::params::PullTightParams;

/// Corners closer than this (squared) are the same corner (Java
/// `c_min_corner_dist_square`).
pub(crate) const MIN_CORNER_DIST_SQUARE: f64 = 0.9;

/// The tightener variant (Java's three `TraceTightener` subclasses).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AngleMode {
    /// `TraceTightener45`.
    Diag45,
    /// `TraceTightener90`.
    Ortho,
    /// `TraceTightenerAnyAngle` (O02).
    Any,
}

impl AngleMode {
    /// Java `getInstance`'s choice from the board's trace angle restriction.
    pub fn of(restriction: AngleRestriction) -> AngleMode {
        match restriction {
            AngleRestriction::NinetyDegree => AngleMode::Ortho,
            AngleRestriction::FortyfiveDegree => AngleMode::Diag45,
            AngleRestriction::None => AngleMode::Any,
        }
    }
}

/// Where a line of the working polyline comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineOrigin {
    /// The trace's own line at this index (the same Java object).
    Kept(usize),
    /// A line Java created (`new Line`, `Line.getInstance`, `translate`,
    /// `opposite`), or one from another trace.
    New,
}

/// A polyline with the origin of each of its lines.
#[derive(Clone, Debug)]
pub struct TrackedPolyline {
    pub polyline: Polyline,
    pub origins: Vec<LineOrigin>,
}

impl TrackedPolyline {
    /// A trace's own polyline: every line is kept.
    pub fn of_trace(polyline: Polyline) -> Self {
        let origins = (0..polyline.lines().len()).map(LineOrigin::Kept).collect();
        TrackedPolyline { polyline, origins }
    }

    /// A polyline whose lines are all new (Java `new Polyline(Point[])`, or
    /// one built from another trace's lines).
    pub fn all_new(polyline: Polyline) -> Self {
        let origins = vec![LineOrigin::New; polyline.lines().len()];
        TrackedPolyline { polyline, origins }
    }

    /// Java `new Polyline(lines)`, with the origins of the lines that
    /// survive the constructor; a reversed line is new.
    pub fn from_lines(lines: &[Line], origins: &[LineOrigin]) -> Self {
        debug_assert_eq!(lines.len(), origins.len());
        let (polyline, map) = Polyline::new_mapped(lines);
        let origins = map
            .iter()
            .map(|m| m.map_or(LineOrigin::New, |i| origins[i]))
            .collect();
        TrackedPolyline { polyline, origins }
    }

    pub fn lines(&self) -> &[Line] {
        self.polyline.lines()
    }

    /// The leading and trailing lines that are the old trace's own lines
    /// at the same index from the start (and from the end), as
    /// `PolylineTrace.change` counts them by reference
    /// (`PolylineTrace.java:956-982`).
    pub fn line_reuse(&self, old_line_count: usize) -> crate::board::LineReuse {
        let new_len = self.origins.len();
        let last_index = new_len.min(old_line_count);
        let prefix = (0..last_index)
            .take_while(|&i| self.origins[i] == LineOrigin::Kept(i))
            .count();
        let suffix = (1..=last_index)
            .take_while(|&i| self.origins[new_len - i] == LineOrigin::Kept(old_line_count - i))
            .count();
        crate::board::LineReuse { prefix, suffix }
    }
}

/// The result of a step: Java's "same object" or a new polyline.
#[derive(Clone, Debug)]
pub enum Outcome {
    Unchanged,
    Changed(TrackedPolyline),
}

impl Outcome {
    pub fn is_changed(&self) -> bool {
        matches!(self, Outcome::Changed(_))
    }

    /// The new polyline, or `input` when unchanged.
    pub fn or(self, input: TrackedPolyline) -> TrackedPolyline {
        match self {
            Outcome::Unchanged => input,
            Outcome::Changed(p) => p,
        }
    }
}

/// The pull-tight algorithm with its per-trace scratch (Java
/// `TraceTightener` and its subclasses).
///
/// Built once per run ([`Tightener::new`]) and reused for every trace of
/// it; [`set_trace`](Self::set_trace) loads the trace data Java's
/// `pullTight(polyline, layer, …)` stores in its fields.
pub struct Tightener {
    pub(crate) mode: AngleMode,
    pub(crate) only_nets: Vec<i32>,
    pub(crate) clip: Option<IntOctagon>,
    pub(crate) min_translate_dist: i32,
    pub(crate) keep_point: Option<(Point, i32)>,
    stop: Option<Arc<dyn Stoppable + Send + Sync>>,
    limit: Option<WorkLimit>,
    /// The calls of `is_stop_requested` so far.
    stop_checks: u64,
    /// The limit's units done at the last stop check.
    units_at_last_check: Option<u64>,
    /// Java `currentLayer`.
    pub(crate) layer: i32,
    /// Java `currentHalfWidth`: compensated in `pullTight`, plain in the
    /// end-corner smoothing.
    pub(crate) half_width: i32,
    /// Java `currentNetNumbers`.
    pub(crate) nets: Vec<i32>,
    /// Java `currentClearanceClassIndex`.
    pub(crate) clearance_class: i32,
    /// Java `contactPins`; `None` is Java's `null` (no pin is an obstacle
    /// by itself).
    pub(crate) contact_pins: Option<BTreeSet<ItemId>>,
}

impl Tightener {
    /// Java `TraceTightener.getInstance`: the variant from the board's
    /// trace angle restriction, `min_translate_dist = max(accuracy, 100)`,
    /// and the work limit started now on the board's meter.
    pub fn new(board: &BasicBoard, params: &PullTightParams) -> Self {
        let limit = params
            .limit
            .filter(|ms| *ms > 0)
            .map(|ms| WorkLimit::with_site(board.search_trees.work_meter(), ms, "pull_tight"));
        Tightener {
            mode: AngleMode::of(board.rules.trace_angle_restriction()),
            only_nets: params.only_nets.clone(),
            clip: params.clip.octagon(),
            min_translate_dist: params.accuracy.max(100),
            keep_point: params.keep_point.clone(),
            stop: params.stop.clone(),
            limit,
            stop_checks: 0,
            units_at_last_check: None,
            layer: 0,
            half_width: 0,
            nets: Vec::new(),
            clearance_class: 0,
            contact_pins: None,
        }
    }

    pub fn mode(&self) -> AngleMode {
        self.mode
    }

    pub fn min_translate_dist(&self) -> i32 {
        self.min_translate_dist
    }

    pub fn only_nets(&self) -> &[i32] {
        &self.only_nets
    }

    /// The clip octagon; `None` for no clip.
    pub fn clip(&self) -> Option<IntOctagon> {
        self.clip
    }

    /// Loads the trace data of Java's `pullTight(polyline, layer, halfWidth,
    /// netNumbers, clearanceClassIndex, contactPins)`: the half width gets
    /// the default tree's clearance compensation.
    pub fn set_trace(
        &mut self,
        board: &BasicBoard,
        layer: i32,
        half_width: i32,
        nets: &[i32],
        clearance_class: i32,
        contact_pins: Option<BTreeSet<ItemId>>,
    ) {
        self.layer = layer;
        let compensation = board
            .search_trees
            .default_tree()
            .clearance_compensation_value(&board.rules.clearance_matrix, clearance_class, layer);
        self.half_width = half_width + compensation;
        self.nets.clear();
        self.nets.extend_from_slice(nets);
        self.clearance_class = clearance_class;
        self.contact_pins = contact_pins;
    }

    /// Loads the trace data the way Java's `smoothenEndCornersAtTrace`
    /// does: the plain half width, without the clearance compensation; the
    /// contact pins stay as they are.
    pub fn set_trace_plain(
        &mut self,
        layer: i32,
        half_width: i32,
        nets: &[i32],
        clearance_class: i32,
    ) {
        self.layer = layer;
        self.half_width = half_width;
        self.nets.clear();
        self.nets.extend_from_slice(nets);
        self.clearance_class = clearance_class;
    }

    /// Sets the contact pins (Java `contactPins`; `None` for `null`).
    pub fn set_contact_pins(&mut self, pins: Option<BTreeSet<ItemId>>) {
        self.contact_pins = pins;
    }

    /// Java `isStopRequested`: the cancel token first, then the work limit
    /// (which logs its cut-off line once).
    pub fn is_stop_requested(&mut self, board: &mut BasicBoard) -> bool {
        self.stop_checks += 1;
        self.units_at_last_check = self.units_done(board);
        if self.stop.as_ref().is_some_and(|s| s.is_stop_requested()) {
            return true;
        }
        let Some(limit) = self.limit.as_mut() else {
            return false;
        };
        let nets = &self.only_nets;
        let context = || {
            let list: Vec<String> = nets.iter().map(i32::to_string).collect();
            format!("nets=[{}]", list.join(", "))
        };
        limit.exceeded_logging(board.search_trees.work_meter(), context, &mut board.log)
    }

    /// The stop request alone, without the work limit: what may end a
    /// normalization early
    /// ([`BasicBoard::normalize_traces_until`](crate::board::BasicBoard::normalize_traces_until)).
    pub(crate) fn stop_request(&self) -> Option<&dyn Stoppable> {
        self.stop.as_deref().map(|s| s as &dyn Stoppable)
    }

    /// The calls of [`is_stop_requested`](Self::is_stop_requested) so far.
    pub fn stop_checks(&self) -> u64 {
        self.stop_checks
    }

    /// The work units done since the limit started, at the last stop check;
    /// `None` without a limit or before the first check.
    pub fn units_at_last_check(&self) -> Option<u64> {
        self.units_at_last_check
    }

    /// Java `currentLayer`.
    pub fn layer(&self) -> i32 {
        self.layer
    }

    /// Java `currentHalfWidth`.
    pub fn half_width(&self) -> i32 {
        self.half_width
    }

    /// The point a trace through it must keep, and its layer.
    pub fn keep_point(&self) -> Option<&(Point, i32)> {
        self.keep_point.as_ref()
    }

    /// Takes the contact pins, leaving Java's `null`.
    pub fn take_contact_pins(&mut self) -> Option<BTreeSet<ItemId>> {
        self.contact_pins.take()
    }

    /// The work units done since the limit started; `None` without a limit.
    pub fn units_done(&self, board: &BasicBoard) -> Option<u64> {
        self.limit
            .as_ref()
            .map(|l| l.units_done(board.search_trees.work_meter()))
    }

    /// Java `pullTight(Polyline)` of the variant: the fixpoint of its three
    /// steps. The stop check runs before the first round and after every
    /// round that changed something (Java's `newResult != prevResult &&
    /// !isStopRequested()`).
    pub fn pull_tight(&mut self, board: &mut BasicBoard, polyline: TrackedPolyline) -> Outcome {
        let mut current = self.avoid_acid_traps(polyline);
        let mut changed_any = false;
        let mut changed_last = true;
        while changed_last && !self.is_stop_requested(board) {
            let mut changed = false;
            for step in 0..3 {
                let result = self.round_step(board, step, &current);
                if let Outcome::Changed(p) = result {
                    current = p;
                    changed = true;
                }
            }
            changed_last = changed;
            changed_any |= changed;
        }
        if changed_any {
            Outcome::Changed(current)
        } else {
            Outcome::Unchanged
        }
    }

    /// Step `step` (0, 1, 2) of one round of the variant's fixpoint.
    fn round_step(
        &mut self,
        board: &mut BasicBoard,
        step: usize,
        polyline: &TrackedPolyline,
    ) -> Outcome {
        match (self.mode, step) {
            (AngleMode::Diag45, 0) => self.reduce_corners(board, polyline),
            (AngleMode::Diag45, 1) => self.smoothen_corners(board, polyline),
            (AngleMode::Ortho, 0) => self.try_skip_second_corner(board, polyline),
            (AngleMode::Ortho, 1) => self.try_skip_corners(board, polyline),
            // the six any-angle steps are one round (any_angle.rs)
            (AngleMode::Any, 0) => self.any_angle_round(board, polyline),
            (AngleMode::Any, _) => Outcome::Unchanged,
            _ => self.reposition_lines(board, polyline),
        }
    }

    /// Java `avoidAcidTraps`: dead code at the pin (`if (true) return
    /// polyline;`, `TraceTightener.java:517-520`), so the identity.
    fn avoid_acid_traps(&self, polyline: TrackedPolyline) -> TrackedPolyline {
        // Java, kept in both profiles: Java turned the step off. N06.12 left it
        // (turning it on is a feature, a Q candidate).
        polyline
    }

    /// `board.checkTraceShape` of the trace shape of segment `no + 1` of
    /// `polyline` with the current trace data.
    pub(crate) fn check_segment(&self, board: &BasicBoard, polyline: &Polyline, no: usize) -> bool {
        let Some(shape) = polyline.offset_shape(self.half_width, no) else {
            return false;
        };
        board.check_trace_shape(
            &shape,
            self.layer,
            &self.nets,
            self.clearance_class,
            self.contact_pins.as_ref(),
        )
    }

    /// True if the clip excludes `point` (Java `currentClipShape != null &&
    /// currentClipShape.isOutside(point)`).
    pub(crate) fn clip_excludes(&self, point: &Point) -> bool {
        self.clip.as_ref().is_some_and(|c| c.is_outside(point))
    }

    /// Java `repositionLines`: tries `repositionLine` at `i = 2 .. len-2`
    /// and takes the first success, then skips the segments of length 0.
    pub fn reposition_lines(
        &mut self,
        board: &mut BasicBoard,
        polyline: &TrackedPolyline,
    ) -> Outcome {
        let len = polyline.lines().len();
        if len < 5 {
            return Outcome::Unchanged;
        }
        for i in 2..len - 2 {
            if let Some(new_line) = self.reposition_line(board, polyline.lines(), i) {
                let mut lines = polyline.lines().to_vec();
                let mut origins = polyline.origins.clone();
                lines[i] = new_line;
                origins[i] = LineOrigin::New;
                let result = TrackedPolyline::from_lines(&lines, &origins);
                let skipped = self.skip_segments_of_length_0(board, &result);
                return Outcome::Changed(skipped.or(result));
            }
        }
        Outcome::Unchanged
    }

    /// Java `repositionLine`: moves line `no` towards the nearer of the
    /// corners before and after its neighbours, by bisection, as far as the
    /// clearance allows. `None` if it can't move.
    pub fn reposition_line(
        &mut self,
        board: &mut BasicBoard,
        lines: &[Line],
        no: usize,
    ) -> Option<Line> {
        if lines.len() - no < 3 {
            return None;
        }
        if self.clip.is_some() {
            // the corners of the line to translate must be inside the clip
            for i in [no - 1, no] {
                if self.clip_excludes(&lines[i].intersection(&lines[i + 1])) {
                    return None;
                }
            }
        }
        let translate_line = lines[no];
        let prev_corner = lines[no - 2].intersection(&lines[no - 1]);
        let next_corner = lines[no + 1].intersection(&lines[no + 2]);
        let prev_dist = translate_line.signed_distance(prev_corner.to_float());
        let next_dist = translate_line.signed_distance(next_corner.to_float());
        if Signum::of(prev_dist) != Signum::of(next_dist) {
            // the two corners are on different sides of the line
            return None;
        }
        let (nearest_point, max_translate_dist) = if prev_dist.abs() < next_dist.abs() {
            (prev_corner, prev_dist)
        } else {
            (next_corner, next_dist)
        };
        let mut translate_dist = max_translate_dist;
        let mut delta_dist = max_translate_dist;
        let side_of_nearest_point = translate_line.side_of(&nearest_point);
        let sign = Signum::as_int(max_translate_dist);
        let mut new_line = None;
        let mut check_lines = [lines[no - 1], translate_line, lines[no + 1]];
        let min_translate_dist = f64::from(self.min_translate_dist);
        let mut first_time = true;
        while first_time || delta_dist.abs() > min_translate_dist {
            check_lines[1] = match (&nearest_point, first_time) {
                (Point::Int(p), true) => Line::get_instance(*p, translate_line.int_direction()),
                _ => translate_line.translate(-translate_dist),
            };
            if check_lines[1] == translate_line {
                // may happen the first time if the nearest point is rational
                return None;
            }
            let new_side = check_lines[1].side_of(&nearest_point);
            if new_side != side_of_nearest_point && new_side != Side::Collinear {
                // moved a little too far because of numerical inaccuracy
                let shorten_value = f64::from(sign) * 0.5;
                translate_dist -= shorten_value;
                delta_dist -= shorten_value;
                continue;
            }
            let tmp = Polyline::new(&check_lines);
            let mut check_ok = false;
            if tmp.lines().len() == 3 {
                // Java's constructor reversed the middle line in check_lines.
                // Java, kept in both profiles (N06.12): later reads depend on it and
                // no fix is clearly right.
                check_lines[1] = tmp.lines()[1];
                check_ok = self.check_segment(board, &tmp, 0);
            }
            delta_dist /= 2.0;
            if check_ok {
                new_line = Some(check_lines[1]);
                if first_time {
                    // biggest possible change
                    break;
                }
                translate_dist += delta_dist;
            } else {
                translate_dist -= delta_dist;
            }
            first_time = false;
        }
        if let Some(line) = &new_line {
            let layer = self.layer;
            board.join_changed_area(check_lines[0].intersection_approx(line), layer);
            board.join_changed_area(check_lines[2].intersection_approx(line), layer);
            board.join_changed_area(lines[no - 1].intersection_approx(&lines[no]), layer);
            board.join_changed_area(lines[no].intersection_approx(&lines[no + 1]), layer);
        }
        new_line
    }

    /// Java `skipSegmentsOfLength0`: drops lines whose segment has length
    /// 0, the first and last exactly, the others below 0.9 squared, where
    /// the clearance check allows it (no check for 45°-multiple lines).
    pub fn skip_segments_of_length_0(
        &mut self,
        board: &mut BasicBoard,
        polyline: &TrackedPolyline,
    ) -> Outcome {
        let mut changed = false;
        let mut current = polyline.clone();
        let mut i = 1;
        while i + 1 < current.lines().len() {
            let len = current.lines().len();
            let try_skip = if i == 1 || i == len - 2 {
                // the first and the last corner must stay exactly where they are
                current.polyline.corner(i) == current.polyline.corner(i - 1)
            } else {
                let prev = current.polyline.corner_approx(i - 1);
                let here: FloatPoint = current.polyline.corner_approx(i);
                here.distance_square(prev) < MIN_CORNER_DIST_SQUARE
            };
            if try_skip {
                let mut lines = current.lines().to_vec();
                let mut origins = current.origins.clone();
                lines.remove(i);
                origins.remove(i);
                let tmp = TrackedPolyline::from_lines(&lines, &origins);
                let mut check_ok = tmp.lines().len() == lines.len();
                if check_ok && !current.lines()[i].is_multiple_of_45_degree() {
                    if i > 1 {
                        check_ok = self.check_segment(board, &tmp.polyline, i - 2);
                    }
                    if check_ok && i < len - 2 {
                        check_ok = self.check_segment(board, &tmp.polyline, i - 1);
                    }
                }
                if check_ok {
                    changed = true;
                    current = tmp;
                    i -= 1;
                }
            }
            i += 1;
        }
        if changed {
            Outcome::Changed(current)
        } else {
            Outcome::Unchanged
        }
    }

    /// Java `smoothenStartCornerAtTrace` of the variant (`None` for 90°).
    pub fn smoothen_start_corner_at_trace(
        &mut self,
        board: &mut BasicBoard,
        trace: ItemId,
    ) -> Option<Polyline> {
        match self.mode {
            AngleMode::Diag45 => self.smoothen_end_corner_45(board, trace, true),
            AngleMode::Any => self.smoothen_start_corner_any(board, trace),
            AngleMode::Ortho => None,
        }
    }

    /// Java `smoothenEndCornerAtTrace` of the variant.
    pub fn smoothen_end_corner_at_trace(
        &mut self,
        board: &mut BasicBoard,
        trace: ItemId,
    ) -> Option<Polyline> {
        match self.mode {
            AngleMode::Diag45 => self.smoothen_end_corner_45(board, trace, false),
            AngleMode::Any => self.smoothen_end_corner_any(board, trace),
            AngleMode::Ortho => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::optimize::params::Clip;

    #[test]
    fn angle_dispatch() {
        assert_eq!(
            AngleMode::of(AngleRestriction::FortyfiveDegree),
            AngleMode::Diag45
        );
        assert_eq!(
            AngleMode::of(AngleRestriction::NinetyDegree),
            AngleMode::Ortho
        );
        assert_eq!(AngleMode::of(AngleRestriction::None), AngleMode::Any);
    }

    #[test]
    fn clip_from_java_cases() {
        // null
        assert_eq!(Clip::Unbounded.octagon(), None);
        // IntOctagon.EMPTY
        assert_eq!(Clip::from_tidy_region(IntOctagon::EMPTY), Clip::Skip);
        // EMPTY.enlarge(250) is the EMPTY instance again in Java
        assert_eq!(
            Clip::from_tidy_region(IntOctagon::EMPTY.enlarge(250.0)),
            Clip::Skip
        );
        assert_eq!(Clip::Skip.octagon(), Some(IntOctagon::EMPTY));
        let o = IntOctagon::new(0, 0, 10, 10, -10, 10, 0, 20);
        assert_eq!(Clip::from_tidy_region(o), Clip::Octagon(o));
    }

    #[test]
    fn line_reuse_counts_same_index_origins() {
        let p = Polyline::from_points(&[
            Point::Int(freerouting_geometry::IntPoint::new(0, 0)),
            Point::Int(freerouting_geometry::IntPoint::new(100, 0)),
            Point::Int(freerouting_geometry::IntPoint::new(100, 100)),
        ]);
        let mut t = TrackedPolyline::of_trace(p);
        assert_eq!(
            t.line_reuse(4),
            crate::board::LineReuse {
                prefix: 4,
                suffix: 4
            }
        );
        t.origins[1] = LineOrigin::New;
        assert_eq!(
            t.line_reuse(4),
            crate::board::LineReuse {
                prefix: 1,
                suffix: 2
            }
        );
        // a line kept at another index is not the same object at this one
        t.origins = vec![
            LineOrigin::Kept(1),
            LineOrigin::Kept(0),
            LineOrigin::Kept(2),
            LineOrigin::Kept(3),
        ];
        assert_eq!(
            t.line_reuse(4),
            crate::board::LineReuse {
                prefix: 0,
                suffix: 2
            }
        );
    }
}
