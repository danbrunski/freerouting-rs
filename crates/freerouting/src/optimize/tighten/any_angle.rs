//! The any-angle steps of the tightener.
//!
//! Port of `board/optimize/TraceTightenerAnyAngle.java` at `a773a70ec`:
//! the fixpoint `skipSegmentsOfLength0 → reduceLines → skipLines →
//! reduceCorners → repositionLines → smoothenCorners` (`pullTight`), the
//! three line reductions, the corner smoothing (`smoothenCorner`), the
//! `repositionLines` / `repositionLine` overrides and the start and end
//! smoothing against contact traces. Clip tests use `contains` of the float
//! corner, except the end smoothings, which use `isOutside` of the end corner,
//! as Java does.
//!
//! Float corners, cosine thresholds and rounding decide the results, so every
//! expression keeps Java's operation order and types (D6): no reassociation,
//! no fused multiply-add.
//!
//! Java's `new Polyline(Line[])` turns lines around *in the caller's array*
//! when it drops none, and `smoothenCorner`, `repositionLine` and
//! `reduceLines` read that array again (the next bisection step, the changed
//! area, the accepted line). [`normalized`] replays it.
//!
//! Termination: Java's comment at `pullTight` says `reduceCorners` may loop
//! with `smoothenCorners` when consecutive corners are closer than a grid
//! point. The port has the same stop checks as Java and no other, so such a
//! loop ends at the same work-limit check as in Java
//! (`tests/pull_tight_any_angle_budget.rs`). A hang outside the tightener's
//! checks hangs as in Java: on `Issue756-minimal-hang` the `split_traces`
//! after an end smoothing doesn't return (DIVERGENCES "Pull-tight").
//!
//! One deliberate difference: the end-corner smoothing with a bend, a short
//! last segment that is skipped, and three corners calls Java's
//! `repositionLine` with start index `-1`, which throws an
//! `ArrayIndexOutOfBoundsException` nobody catches; the port returns no
//! smoothing there (DIVERGENCES, "Pull-tight").

use freerouting_geometry::{
    FloatPoint, IntOctagon, Line, Point, Polyline, SQRT2, Side, Signum, TileShapeOps,
};

use super::{LineOrigin, Outcome, Tightener, TrackedPolyline};
use crate::board::BasicBoard;
use crate::board::items::ItemId;

/// Java `TraceTightenerAnyAngle.SKIP_LENGTH`: an end segment of a non-integer
/// end corner shorter than this (squared) is skipped by the end smoothing.
pub(crate) const SKIP_LENGTH: f64 = 10.0;

/// Whether the end smoothing skips the short end segment from `end_corner` to
/// `prev_end_corner` (`!(endCorner instanceof IntPoint) && distance < SKIP_LENGTH`).
/// PARITY(quirk, java_quirks.any_angle_skip_length_squared):
/// TraceTightenerAnyAngle.java:774, 896 compare [`SKIP_LENGTH`] itself with a
/// squared distance (effectively 3.2 units); `java = false` compares its square.
/// `run_stats` counts the segments the switch's other value would decide
/// differently, in either arm (N06.12).
fn skips_short_end_segment(java: bool, end_corner: &Point, prev_end_corner: &Point) -> bool {
    if matches!(end_corner, Point::Int(_)) {
        return false;
    }
    let distance_square = end_corner
        .to_float()
        .distance_square(prev_end_corner.to_float());
    let java_skip = distance_square < SKIP_LENGTH;
    let fixed_skip = distance_square < SKIP_LENGTH * SKIP_LENGTH;
    if java_skip != fixed_skip {
        crate::run_stats::record_n06(crate::run_stats::Counter::AnyAngleSkipLengthFlips);
    }
    if java { java_skip } else { fixed_skip }
}

/// Java `TraceTightener.c_max_cos_angle`: two lines whose angle has a bigger
/// cosine are nearly parallel and their corner isn't cut again.
pub(crate) const MAX_COS_ANGLE: f64 = 0.999;

/// Java `new Polyline(lines)` with the in-place line reversal replayed:
/// when the constructor dropped no line, it turned lines around in `lines`
/// itself, so `lines` becomes the result's lines.
fn normalized(lines: &mut [Line]) -> Polyline {
    let result = Polyline::new(lines);
    if result.lines().len() == lines.len() {
        lines.copy_from_slice(result.lines());
    }
    result
}

/// What the end smoothing found at a trace end (Java's `otherTrace…`
/// locals of the last contact found).
struct AnyEndContact {
    corner_approx: FloatPoint,
    line: Line,
    prev_line: Line,
    prev_corner_side: Side,
}

/// The data of a contact trace at `end_corner`: the corner after the shared
/// one, the line through both (pointing away from the shared end) and the
/// line after it.
fn contact_lines(other: &Polyline, end_corner: &Point) -> (FloatPoint, Line, Line) {
    if other.first_corner() == *end_corner {
        (other.corner_approx(1), other.lines()[1], other.lines()[2])
    } else {
        let no = other.corner_count() - 2;
        (
            other.corner_approx(no),
            other.lines()[no + 1].opposite(),
            other.lines()[no],
        )
    }
}

impl Tightener {
    /// Java `currentClipShape == null || currentClipShape.contains(point)`.
    fn any_clip_contains(&self, point: FloatPoint) -> bool {
        self.clip
            .as_ref()
            .is_none_or(|c: &IntOctagon| c.contains_float(point))
    }

    /// One round of Java `TraceTightenerAnyAngle.pullTight`'s loop:
    /// `skipSegmentsOfLength0 → reduceLines → skipLines → reduceCorners →
    /// repositionLines → smoothenCorners`, `Changed` if any step changed.
    pub(super) fn any_angle_round(
        &mut self,
        board: &mut BasicBoard,
        polyline: &TrackedPolyline,
    ) -> Outcome {
        let mut current = polyline.clone();
        let mut changed = false;
        for step in 0..6 {
            let result = match step {
                0 => self.skip_segments_of_length_0(board, &current),
                1 => self.reduce_lines(board, &current),
                2 => self.skip_lines(board, &current),
                3 => self.reduce_corners_any(board, &current),
                4 => self.reposition_lines_any(board, &current),
                _ => self.smoothen_corners_any(board, &current),
            };
            if let Outcome::Changed(p) = result {
                current = p;
                changed = true;
            }
        }
        if changed {
            Outcome::Changed(current)
        } else {
            Outcome::Unchanged
        }
    }

    /// Java `TraceTightenerAnyAngle.reduceCorners`: replaces two
    /// consecutive lines by one line through the rounded corners before and
    /// after them, where that is shorter and fits.
    pub fn reduce_corners_any(
        &mut self,
        board: &mut BasicBoard,
        polyline: &TrackedPolyline,
    ) -> Outcome {
        let lines = polyline.lines();
        let len = lines.len();
        if len < 4 {
            return Outcome::Unchanged;
        }
        let last_index = len - 4;
        let first_corner = polyline.polyline.first_corner();
        let last_corner = polyline.polyline.last_corner();
        let first_is_int = matches!(first_corner, Point::Int(_));
        let last_is_int = matches!(last_corner, Point::Int(_));
        let mut new_lines: Vec<Line> = vec![lines[0]; len];
        let mut new_origins: Vec<LineOrigin> = vec![LineOrigin::New; len];
        new_lines[1] = lines[1];
        new_origins[0] = polyline.origins[0];
        new_origins[1] = polyline.origins[1];
        let mut nli = 1usize;
        let mut changed = false;
        // Java's `currentLines`, kept across iterations
        let mut current = [lines[0]; 3];
        let mut current_origins = [LineOrigin::New; 3];
        for i in 0..=last_index {
            let mut skip_line = false;
            let new_a = new_lines[nli - 1].intersection_approx(&new_lines[nli]);
            let new_b = polyline.polyline.corner_approx(i + 2);
            let in_clip = self.clip.is_none()
                || self.any_clip_contains(new_a)
                    && self.any_clip_contains(new_b)
                    && self.any_clip_contains(polyline.polyline.corner_approx(nli));
            if in_clip {
                current[1] = Line::new(new_a.round(), new_b.round());
                current_origins[1] = LineOrigin::New;
                let mut ok = true;
                if nli == 1 {
                    if let Point::Int(p) = &first_corner {
                        let dir = current[1].direction().turn_45_degree(2);
                        current[0] = Line::get_instance(*p, dir.expect_int());
                        current_origins[0] = LineOrigin::New;
                    } else {
                        // the first corner must not change
                        ok = false;
                    }
                } else {
                    current[0] = new_lines[nli - 1];
                    current_origins[0] = new_origins[nli - 1];
                }
                if i == last_index {
                    if let Point::Int(p) = &last_corner {
                        let dir = current[1].direction().turn_45_degree(2);
                        current[2] = Line::get_instance(*p, dir.expect_int());
                        current_origins[2] = LineOrigin::New;
                    } else {
                        // the last corner must not change
                        ok = false;
                    }
                } else {
                    current[2] = lines[i + 3];
                    current_origins[2] = polyline.origins[i + 3];
                }
                // the new corners must be near the old ones: numerical
                // stability with nearly parallel lines (Java's checkDist)
                const CHECK_DIST: f64 = 100.0;
                if ok {
                    let check_is = current[0].intersection_approx(&current[1]);
                    if check_is.distance_square(new_a) > CHECK_DIST {
                        ok = false;
                    }
                }
                if ok {
                    let check_is = current[1].intersection_approx(&current[2]);
                    if check_is.distance_square(new_b) > CHECK_DIST {
                        ok = false;
                    }
                }
                if ok && i == 1 && !first_is_int {
                    // a connection to a trace: the second corner must stay on
                    // the same side of the trace as the third
                    let new_corner = current[0].intersection(&current[1]);
                    ok = new_corner.side_of_line(&new_lines[0])
                        == polyline.polyline.corner(1).side_of_line(&new_lines[0]);
                }
                if ok && i + 1 == last_index && !last_is_int {
                    // the same at the end (Java compares with the first line)
                    let new_corner = current[1].intersection(&current[2]);
                    let corner = polyline
                        .polyline
                        .corner(polyline.polyline.corner_count() - 2);
                    ok = new_corner.side_of_line(&new_lines[0])
                        == corner.side_of_line(&new_lines[0]);
                }
                let mut check = None;
                if ok {
                    let skip_corner = new_lines[nli].intersection_approx(&lines[i + 2]);
                    let tmp = normalized(&mut current);
                    if tmp.lines().len() == 3 {
                        let length_before =
                            skip_corner.distance(new_a) + skip_corner.distance(new_b);
                        // 1.5 for the inaccuracy of rounding twice (Java's comment)
                        let length_after = tmp.length_approx() + 1.5;
                        if length_after >= length_before {
                            // may happen from rounding to integer; prevents a loop
                            ok = false;
                        }
                    } else {
                        ok = false;
                    }
                    check = Some(tmp);
                }
                if let (true, Some(tmp)) = (ok, &check) {
                    ok = self.check_segment(board, tmp, 0);
                }
                skip_line = ok;
            }
            if skip_line {
                changed = true;
                new_lines[nli] = current[1];
                new_origins[nli] = current_origins[1];
                if nli == 1 {
                    // the first line perpendicular to the new line
                    new_lines[0] = current[0];
                    new_origins[0] = current_origins[0];
                }
                if i == last_index {
                    // the last line perpendicular to the new line
                    nli += 1;
                    new_lines[nli] = current[2];
                    new_origins[nli] = current_origins[2];
                }
                let layer = self.layer;
                board.join_changed_area(new_a, layer);
                board.join_changed_area(new_b, layer);
            } else {
                nli += 1;
                new_lines[nli] = lines[i + 2];
                new_origins[nli] = polyline.origins[i + 2];
                if i == last_index {
                    nli += 1;
                    new_lines[nli] = lines[i + 3];
                    new_origins[nli] = polyline.origins[i + 3];
                }
            }
            if new_lines[nli].is_parallel(&new_lines[nli - 1]) {
                // skip a line parallel to the previous one
                nli -= 1;
            }
        }
        if !changed {
            return Outcome::Unchanged;
        }
        Outcome::Changed(TrackedPolyline::from_lines(
            &new_lines[..=nli],
            &new_origins[..=nli],
        ))
    }

    /// Java `TraceTightenerAnyAngle.smoothenCorners`: cuts off corners by a
    /// new line where that fits; the new line goes after the corner's first
    /// line, and the next try starts after it.
    pub fn smoothen_corners_any(
        &mut self,
        board: &mut BasicBoard,
        polyline: &TrackedPolyline,
    ) -> Outcome {
        if polyline.lines().len() < 4 {
            return Outcome::Unchanged;
        }
        let mut changed = false;
        let mut lines = polyline.lines().to_vec();
        let mut origins = polyline.origins.clone();
        let mut i = 0;
        while i + 3 < lines.len() {
            if let Some(new_line) = self.smoothen_corner_any(board, &lines, i) {
                changed = true;
                lines.insert(i + 2, new_line);
                origins.insert(i + 2, LineOrigin::New);
                i += 1;
            }
            i += 1;
        }
        if !changed {
            return Outcome::Unchanged;
        }
        Outcome::Changed(TrackedPolyline::from_lines(&lines, &origins))
    }

    /// Java `TraceTightenerAnyAngle.smoothenCorner`: a line cutting off the
    /// corner between lines `start_no + 1` and `start_no + 2`, a translate of
    /// the line through the rounded corner in the middle direction, moved by
    /// bisection as far towards the nearer neighbouring corner as the
    /// clearance allows. `None` if there is none.
    fn smoothen_corner_any(
        &mut self,
        board: &mut BasicBoard,
        lines: &[Line],
        start_no: usize,
    ) -> Option<Line> {
        if lines.len() < start_no + 4 {
            return None;
        }
        let s = start_no;
        let current_corner = lines[s + 1].intersection_approx(&lines[s + 2]);
        if self.clip.is_some() && !self.any_clip_contains(current_corner) {
            return None;
        }
        let cos_angle = lines[s + 1].cos_angle(&lines[s + 2]);
        if cos_angle > MAX_COS_ANGLE {
            // nearly parallel: don't divide the angle further (numerical stability)
            return None;
        }
        let prev_corner = lines[s].intersection_approx(&lines[s + 1]);
        let next_corner = lines[s + 2].intersection_approx(&lines[s + 3]);
        // a line about through the corner, in about the middle direction
        let prev_dir = lines[s + 1].direction();
        let next_dir = lines[s + 2].direction();
        let middle_dir = prev_dir.middle_approx(&next_dir);
        let translate_line = Line::get_instance(current_corner.round(), middle_dir.expect_int());
        let prev_dist = translate_line.signed_distance(prev_corner);
        let next_dist = translate_line.signed_distance(next_corner);
        let (nearest_point, mut max_translate_dist) = if prev_dist.abs() < next_dist.abs() {
            (prev_corner, prev_dist)
        } else {
            (next_corner, next_dist)
        };
        if max_translate_dist.abs() < 1.0 {
            return None;
        }
        let mut current_lines = Vec::with_capacity(lines.len() + 1);
        current_lines.extend_from_slice(&lines[..s + 2]);
        current_lines.push(lines[s + 2]);
        current_lines.extend_from_slice(&lines[s + 2..]);
        let mut translate_dist = max_translate_dist;
        let mut delta_dist = max_translate_dist;
        let side_of_nearest_point = translate_line.side_of_float(nearest_point);
        let sign = f64::from(Signum::as_int(max_translate_dist));
        let min_translate_dist = f64::from(self.min_translate_dist);
        let mut result = None;
        while delta_dist.abs() > min_translate_dist {
            let new_line = translate_line.translate(-translate_dist);
            let new_side = new_line.side_of_float(nearest_point);
            if new_side == side_of_nearest_point || new_side == Side::Collinear {
                current_lines[s + 2] = new_line;
                let tmp = normalized(&mut current_lines);
                let check_ok = tmp.lines().len() == current_lines.len()
                    && self.check_segment(board, &tmp, s + 1);
                delta_dist /= 2.0;
                if check_ok {
                    result = Some(current_lines[s + 2]);
                    if translate_dist == max_translate_dist {
                        // biggest possible change
                        break;
                    }
                    translate_dist += delta_dist;
                } else {
                    translate_dist -= delta_dist;
                }
            } else {
                // moved a little too far the first time (numerical inaccuracy)
                let shorten_value = sign * 0.5;
                max_translate_dist -= shorten_value;
                translate_dist -= shorten_value;
                delta_dist -= shorten_value;
            }
        }
        let result = result?;
        let layer = self.layer;
        let new_prev_corner = current_lines[s].intersection_approx(&current_lines[s + 1]);
        let new_next_corner = current_lines[s + 3].intersection_approx(&current_lines[s + 4]);
        board.join_changed_area(new_prev_corner, layer);
        board.join_changed_area(new_next_corner, layer);
        Some(result)
    }

    /// Java `TraceTightenerAnyAngle.repositionLines`: tries
    /// [`reposition_line_any`](Self::reposition_line_any) at every start
    /// index and keeps each success; stops after a success that made the
    /// moved line parallel to a neighbour.
    pub fn reposition_lines_any(
        &mut self,
        board: &mut BasicBoard,
        polyline: &TrackedPolyline,
    ) -> Outcome {
        let len = polyline.lines().len();
        if len < 5 {
            return Outcome::Unchanged;
        }
        let mut changed = false;
        let mut lines = polyline.lines().to_vec();
        let mut origins = polyline.origins.clone();
        for i in 0..len - 4 {
            if let Some(new_line) = self.reposition_line_any(board, &lines, i) {
                changed = true;
                lines[i + 2] = new_line;
                origins[i + 2] = LineOrigin::New;
                if lines[i + 2].is_parallel(&lines[i + 1])
                    || lines[i + 2].is_parallel(&lines[i + 3])
                {
                    // the corners can't be computed before the parallel lines go
                    break;
                }
            }
        }
        if !changed {
            return Outcome::Unchanged;
        }
        Outcome::Changed(TrackedPolyline::from_lines(&lines, &origins))
    }

    /// Java `TraceTightenerAnyAngle.repositionLine`: moves line
    /// `start_no + 2` towards the nearer of the corners before and after its
    /// neighbours, by bisection, as far as the clearance allows; lines through
    /// the same corner as the moved line's end move with it. `None` if it
    /// can't move.
    pub fn reposition_line_any(
        &mut self,
        board: &mut BasicBoard,
        lines: &[Line],
        start_no: usize,
    ) -> Option<Line> {
        if lines.len() < start_no + 5 {
            return None;
        }
        let s = start_no;
        if self.clip.is_some() {
            // the corners of the line to translate must be inside the clip
            for i in 1..3 {
                let corner = lines[s + i].intersection_approx(&lines[s + i + 1]);
                if !self.any_clip_contains(corner) {
                    return None;
                }
            }
        }
        let translate_line = lines[s + 2];
        let mut prev_corner = lines[s].intersection_approx(&lines[s + 1]);
        let mut next_corner = lines[s + 3].intersection_approx(&lines[s + 4]);
        let mut prev_dist = translate_line.signed_distance(prev_corner);
        let mut skipped_before = 0usize;
        let mut skipped_after = 0usize;
        const EPSILON: f64 = 0.001;
        while prev_dist.abs() < EPSILON {
            // move also the lines through the start corner of the line
            skipped_before += 1;
            if skipped_before > s {
                // the first corner is on the line to translate
                return None;
            }
            let current_no = s - skipped_before;
            prev_corner = lines[current_no].intersection_approx(&lines[current_no + 1]);
            prev_dist = translate_line.signed_distance(prev_corner);
        }
        let mut next_dist = translate_line.signed_distance(next_corner);
        while next_dist.abs() < EPSILON {
            // move also the lines through the end corner of the line
            skipped_after += 1;
            let current_no = s + 3 + skipped_after;
            if current_no + 2 >= lines.len() {
                // the last corner is on the line to translate
                return None;
            }
            next_corner = lines[current_no].intersection_approx(&lines[current_no + 1]);
            next_dist = translate_line.signed_distance(next_corner);
        }
        if Signum::of(prev_dist) != Signum::of(next_dist) {
            // the two corners are on different sides of the line
            return None;
        }
        let (nearest_point, mut max_translate_dist) = if prev_dist.abs() < next_dist.abs() {
            (prev_corner, prev_dist)
        } else {
            (next_corner, next_dist)
        };
        let mut current_lines = lines.to_vec();
        let mut translate_dist = max_translate_dist;
        let mut delta_dist = max_translate_dist;
        let side_of_nearest_point = translate_line.side_of_float(nearest_point);
        let sign = f64::from(Signum::as_int(max_translate_dist));
        let min_translate_dist = f64::from(self.min_translate_dist);
        let mut result = None;
        let mut first_time = true;
        while first_time || delta_dist.abs() > min_translate_dist {
            let mut new_line = translate_line.translate(-translate_dist);
            if first_time && translate_dist.abs() < 1.0 {
                if new_line == translate_line {
                    // try the parallel line through the nearest point
                    let rounded = nearest_point.round();
                    if nearest_point.distance(rounded.to_float()) < translate_dist.abs() {
                        new_line = Line::get_instance(rounded, translate_line.int_direction());
                    }
                    first_time = false;
                }
                if new_line == translate_line {
                    return None;
                }
            }
            let new_side = new_line.side_of_float(nearest_point);
            if new_side == side_of_nearest_point || new_side == Side::Collinear {
                first_time = false;
                current_lines[s + 2] = new_line;
                // Java: rare, but needed when 3 or more consecutive corners
                // are equal
                let mut prev_translated = new_line;
                for i in 0..skipped_before {
                    // the previous lines onto or past the intersection of the
                    // new line with the first unmoved line
                    let prev_line_no = s + 1 - skipped_before;
                    let corner = prev_translated.intersection_approx(&current_lines[prev_line_no]);
                    let line = lines[s + 1 - i];
                    let dist = line.signed_distance(corner);
                    prev_translated = line.translate(-dist);
                    current_lines[s + 1 - i] = prev_translated;
                }
                prev_translated = new_line;
                for i in 0..skipped_after {
                    let next_line_no = s + 3 + skipped_after;
                    let corner = prev_translated.intersection_approx(&current_lines[next_line_no]);
                    let line = lines[s + 3 + i];
                    let dist = line.signed_distance(corner);
                    prev_translated = line.translate(-dist);
                    current_lines[s + 3 + i] = prev_translated;
                }
                let tmp = normalized(&mut current_lines);
                let check_ok = tmp.lines().len() == current_lines.len()
                    && self.check_segment(board, &tmp, s + 1);
                delta_dist /= 2.0;
                if check_ok {
                    result = Some(current_lines[s + 2]);
                    if translate_dist == max_translate_dist {
                        // biggest possible change
                        break;
                    }
                    translate_dist += delta_dist;
                } else {
                    translate_dist -= delta_dist;
                }
            } else {
                // moved a little too far the first time (numerical inaccuracy)
                let shorten_value = sign * 0.5;
                max_translate_dist -= shorten_value;
                translate_dist -= shorten_value;
                delta_dist -= shorten_value;
            }
        }
        let result = result?;
        let layer = self.layer;
        let new_prev_corner = current_lines[s].intersection_approx(&current_lines[s + 1]);
        let new_next_corner = current_lines[s + 3].intersection_approx(&current_lines[s + 4]);
        board.join_changed_area(new_prev_corner, layer);
        board.join_changed_area(new_next_corner, layer);
        Some(result)
    }

    /// Java `TraceTightenerAnyAngle.reduceLines`: moves a line parallel
    /// past the corner of its previous or next line, which drops that line,
    /// where the clearance allows it.
    pub fn reduce_lines(&mut self, board: &mut BasicBoard, polyline: &TrackedPolyline) -> Outcome {
        if polyline.lines().len() < 6 {
            return Outcome::Unchanged;
        }
        let mut changed = false;
        let mut lines = polyline.lines().to_vec();
        let mut origins = polyline.origins.clone();
        let mut i = 2;
        while i + 2 < lines.len() {
            let Some((new_lines, new_origins, prev_corner, next_corner)) =
                self.reduce_line(board, &lines, &origins, i)
            else {
                i += 1;
                continue;
            };
            let layer = self.layer;
            board.join_changed_area(prev_corner, layer);
            board.join_changed_area(next_corner, layer);
            changed = true;
            lines = new_lines;
            origins = new_origins;
            // Java's `--i` and the loop's `i++`: try the same index again
        }
        if !changed {
            return Outcome::Unchanged;
        }
        Outcome::Changed(TrackedPolyline::from_lines(&lines, &origins))
    }

    /// One try of `reduceLines` at line `i`: the new lines and origins and
    /// the two corners for the changed area, or `None` (Java's `continue`).
    fn reduce_line(
        &mut self,
        board: &mut BasicBoard,
        lines: &[Line],
        origins: &[LineOrigin],
        i: usize,
    ) -> Option<(Vec<Line>, Vec<LineOrigin>, FloatPoint, FloatPoint)> {
        let prev_corner = lines[i - 2].intersection_approx(&lines[i - 1]);
        let next_corner = lines[i + 1].intersection_approx(&lines[i + 2]);
        let in_clip = self.clip.is_none()
            || self.any_clip_contains(prev_corner) && self.any_clip_contains(next_corner);
        if !in_clip {
            return None;
        }
        let translate_line = lines[i];
        let prev_dist = translate_line.signed_distance(prev_corner);
        let next_dist = translate_line.signed_distance(next_corner);
        if Signum::of(prev_dist) != Signum::of(next_dist) {
            // the two corners are on different sides of the line
            return None;
        }
        let mut translate_dist = if prev_dist.abs() < next_dist.abs() {
            prev_dist
        } else {
            next_dist
        };
        if translate_dist == 0.0 {
            // the segment may have length 0
            return None;
        }
        let line_side = translate_line.side_of_float(prev_corner);
        let mut new_line = translate_line.translate(-translate_dist);
        // make sure the nearest corner is crossed
        let sign = f64::from(Signum::as_int(translate_dist));
        let mut side_of_prev = new_line.side_of_float(prev_corner);
        let mut side_of_next = new_line.side_of_float(next_corner);
        while side_of_prev == line_side && side_of_next == line_side {
            translate_dist += sign * 0.5;
            new_line = translate_line.translate(-translate_dist);
            side_of_prev = new_line.side_of_float(prev_corner);
            side_of_next = new_line.side_of_float(next_corner);
        }
        let crossed_before = usize::from(side_of_prev != line_side);
        let crossed_after = usize::from(side_of_next != line_side);
        // the corners before and after the crossed one must stay on the old side
        if crossed_before > 0 {
            if i < 3 {
                return None;
            }
            let prev_prev_corner = lines[i - 3].intersection_approx(&lines[i - 2]);
            if new_line.side_of_float(prev_prev_corner) != line_side {
                return None;
            }
        }
        if crossed_after > 0 {
            if i + 3 >= lines.len() {
                return None;
            }
            let next_next_corner = lines[i + 2].intersection_approx(&lines[i + 3]);
            if new_line.side_of_float(next_next_corner) != line_side {
                return None;
            }
        }
        let keep_before = i - crossed_before;
        let mut current_lines = Vec::with_capacity(lines.len() - crossed_before - crossed_after);
        let mut current_origins = Vec::with_capacity(current_lines.capacity());
        current_lines.extend_from_slice(&lines[..keep_before]);
        current_origins.extend_from_slice(&origins[..keep_before]);
        current_lines.push(new_line);
        current_origins.push(LineOrigin::New);
        current_lines.extend_from_slice(&lines[i + 1 + crossed_after..]);
        current_origins.extend_from_slice(&origins[i + 1 + crossed_after..]);
        let tmp = TrackedPolyline::from_lines(&current_lines, &current_origins);
        if tmp.lines().len() != current_lines.len()
            || !self.check_segment(board, &tmp.polyline, keep_before - 1)
        {
            return None;
        }
        // the constructor turned lines around in Java's array: it is now
        // the new polyline's
        Some((
            tmp.polyline.lines().to_vec(),
            tmp.origins,
            prev_corner,
            next_corner,
        ))
    }

    /// Java `TraceTightenerAnyAngle.skipLines`: drops one or two lines
    /// where the corners around them lie on different sides of a
    /// neighbouring line and the shortcut fits; returns at the first
    /// success.
    pub fn skip_lines(&mut self, board: &mut BasicBoard, polyline: &TrackedPolyline) -> Outcome {
        let p = &polyline.polyline;
        let lines = polyline.lines();
        let len = lines.len();
        for i in 1..len.saturating_sub(3) {
            for j in 0..=1 {
                let (mut current_line, corner1, corner2) = if j == 0 {
                    // the line before line i + 2
                    (lines[i + 2], p.corner_approx(i), p.corner_approx(i - 1))
                } else {
                    // the line after line i
                    (lines[i], p.corner_approx(i + 1), p.corner_approx(i + 2))
                };
                let in_clip = self.clip.is_none()
                    || self.any_clip_contains(corner1) && self.any_clip_contains(corner2);
                if !in_clip {
                    continue;
                }
                let mut side1 = current_line.side_of_float(corner1);
                let mut side2 = current_line.side_of_float(corner2);
                let shape_index = if j == 0 { i } else { i - 1 };
                if side1 != side2 {
                    // the two corners are on different sides of the line
                    let reduced = Self::without_lines(polyline, i + 1, i + 1);
                    if reduced.lines().len() == len - 1
                        && self.check_segment(board, &reduced.polyline, shape_index)
                    {
                        let layer = self.layer;
                        board.join_changed_area(corner1, layer);
                        board.join_changed_area(corner2, layer);
                        return Outcome::Changed(reduced);
                    }
                }
                // now try skipping two lines
                if i + 4 >= len {
                    break;
                }
                let corner3 = if j == 1 {
                    p.corner_approx(i + 3)
                } else {
                    p.corner_approx(i + 1)
                };
                if self.clip.is_some() && !self.any_clip_contains(corner3) {
                    continue;
                }
                if j == 0 {
                    // one line later than when skipping one line from behind
                    current_line = lines[i + 3];
                    side1 = current_line.side_of_float(corner1);
                    side2 = current_line.side_of_float(corner2);
                } else {
                    side1 = current_line.side_of_float(corner3);
                }
                if side1 != side2 {
                    let reduced = Self::without_lines(polyline, i + 1, i + 2);
                    if reduced.lines().len() == len - 2
                        && self.check_segment(board, &reduced.polyline, shape_index)
                    {
                        let layer = self.layer;
                        board.join_changed_area(corner1, layer);
                        board.join_changed_area(corner2, layer);
                        board.join_changed_area(corner3, layer);
                        return Outcome::Changed(reduced);
                    }
                }
            }
        }
        Outcome::Unchanged
    }

    /// Java `Polyline.skipLines(from, to)` with the line origins (the range
    /// is always valid here).
    fn without_lines(polyline: &TrackedPolyline, from: usize, to: usize) -> TrackedPolyline {
        let mut lines = polyline.lines()[..from].to_vec();
        lines.extend_from_slice(&polyline.lines()[to + 1..]);
        let mut origins = polyline.origins[..from].to_vec();
        origins.extend_from_slice(&polyline.origins[to + 1..]);
        TrackedPolyline::from_lines(&lines, &origins)
    }

    /// The contact trace at an end of `trace` that makes an acute angle
    /// (`Some((true, …))`) or a bend (`Some((false, …))`) with it, from the
    /// last contact found; `Err(())` where Java returns `null` because a
    /// contact isn't an unfixed trace. At the end, contact traces with two
    /// corners are passed over (Java checks `cornerCount() > 2` there only).
    fn any_end_contact(
        board: &BasicBoard,
        trace: ItemId,
        at_start: bool,
        end_corner: &Point,
        prev_end_corner: &Point,
        (line_direction, prev_line_direction): (
            &freerouting_geometry::Direction,
            &freerouting_geometry::Direction,
        ),
    ) -> Result<Option<(bool, AnyEndContact)>, ()> {
        let corner_count = board
            .item_ref(trace)
            .as_trace()
            .expect("a trace")
            .polyline
            .corner_count();
        let contacts = if at_start {
            board.trace_start_contacts(trace)
        } else {
            board.trace_end_contacts(trace)
        };
        let mut found: Option<AnyEndContact> = None;
        let mut acute = false;
        let mut bend = false;
        for contact in contacts {
            let item = board.item_ref(contact);
            let Some(contact_trace) = item.as_trace() else {
                return Err(());
            };
            if board.is_shove_fixed(item) {
                return Err(());
            }
            let other = &contact_trace.polyline;
            // N04.4: under `inside_pad` a trace this end meets in its middle
            // (a T junction) has no end here to smoothen with.
            if other.first_corner() != *end_corner && other.last_corner() != *end_corner {
                return Err(());
            }
            if !at_start && other.corner_count() <= 2 {
                continue;
            }
            let (corner_approx, line, prev_line) = contact_lines(other, end_corner);
            let prev_corner_side = prev_end_corner.side_of_line(&line);
            let projection = line_direction.projection(&line.direction());
            let mut other_found = false;
            if projection == Signum::Positive && prev_corner_side != Side::Collinear {
                acute = true;
                other_found = true;
            } else if projection == Signum::Zero
                && corner_count > 2
                && prev_line_direction.projection(&line.direction()) == Signum::Positive
            {
                bend = true;
                other_found = true;
            }
            if other_found {
                found = Some(AnyEndContact {
                    corner_approx,
                    line,
                    prev_line,
                    prev_corner_side,
                });
            }
        }
        // Java decides on the flags, which an earlier contact may have set,
        // and uses the data of the last contact found.
        Ok(found.map(|c| (acute, c)).filter(|_| acute || bend))
    }

    /// The line cutting off an acute angle at `end_corner` (both end
    /// smoothings): `None` where the room is below 0.99.
    fn acute_cut_line(
        &self,
        end_corner: &Point,
        prev_end_corner: &Point,
        contact: &AnyEndContact,
        turn: i32,
    ) -> Option<Line> {
        let new_dir = contact.line.direction().turn_45_degree(turn);
        let translate_line =
            Line::get_instance(end_corner.to_float().round(), new_dir.expect_int());
        let mut translate_dist = (SQRT2 - 1.0) * f64::from(self.half_width);
        let prev_corner_dist = translate_line
            .signed_distance(prev_end_corner.to_float())
            .abs();
        let other_dist = translate_line.signed_distance(contact.corner_approx).abs();
        translate_dist = translate_dist.min(prev_corner_dist);
        translate_dist = translate_dist.min(other_dist);
        if translate_dist < 0.99 {
            return None;
        }
        translate_dist = (translate_dist - 1.0).max(1.0);
        if translate_line.side_of(prev_end_corner) == Side::OnTheLeft {
            translate_dist = -translate_dist;
        }
        Some(translate_line.translate(translate_dist))
    }

    /// Java `TraceTightenerAnyAngle.smoothenStartCornerAtTrace`.
    pub(super) fn smoothen_start_corner_any(
        &mut self,
        board: &mut BasicBoard,
        trace: ItemId,
    ) -> Option<Polyline> {
        let polyline = board.item_ref(trace).as_trace()?.polyline.clone();
        let lines = polyline.lines();
        let end_corner = polyline.corner(0);
        if self.clip_excludes(&end_corner) {
            return None;
        }
        let mut prev_end_corner = polyline.corner(1);
        let skip_short_segment = skips_short_end_segment(
            board.java_quirks().any_angle_skip_length_squared,
            &end_corner,
            &prev_end_corner,
        );
        let mut start_line_no = 1;
        if skip_short_segment {
            if polyline.corner_count() < 3 {
                return None;
            }
            prev_end_corner = polyline.corner(2);
            start_line_no += 1;
        }
        let line_direction = lines[start_line_no].direction();
        let prev_line_direction = lines[start_line_no + 1].direction();
        let (acute, contact) = Self::any_end_contact(
            board,
            trace,
            true,
            &end_corner,
            &prev_end_corner,
            (&line_direction, &prev_line_direction),
        )
        .ok()??;
        // the lines of the trace after its first one (Java's `2 - diff`)
        let rest = if skip_short_segment {
            &lines[2..]
        } else {
            &lines[1..]
        };
        if acute {
            let turn = if contact.prev_corner_side == Side::OnTheLeft {
                2
            } else {
                6
            };
            let add_line = self.acute_cut_line(&end_corner, &prev_end_corner, &contact, turn)?;
            let mut new_lines = vec![contact.line, add_line];
            new_lines.extend_from_slice(rest);
            return Some(Polyline::new(&new_lines));
        }
        let mut check_lines = vec![contact.prev_line, contact.line];
        check_lines.extend_from_slice(rest);
        let new_line = self.reposition_line_any(board, &check_lines, 0)?;
        let mut new_lines = vec![contact.line, new_line];
        new_lines.extend_from_slice(&lines[2..]);
        Some(Polyline::new(&new_lines))
    }

    /// Java `TraceTightenerAnyAngle.smoothenEndCornerAtTrace`. Java takes
    /// the direction of the second last line for both the line and the
    /// previous line here (kept).
    pub(super) fn smoothen_end_corner_any(
        &mut self,
        board: &mut BasicBoard,
        trace: ItemId,
    ) -> Option<Polyline> {
        let polyline = board.item_ref(trace).as_trace()?.polyline.clone();
        let lines = polyline.lines();
        let n = lines.len();
        let corner_count = polyline.corner_count();
        let end_corner = polyline.last_corner();
        if self.clip_excludes(&end_corner) {
            return None;
        }
        let mut prev_end_corner = polyline.corner(corner_count - 2);
        let skip_short_segment = skips_short_end_segment(
            board.java_quirks().any_angle_skip_length_squared,
            &end_corner,
            &prev_end_corner,
        );
        let mut end_line_no = n - 2;
        if skip_short_segment {
            if corner_count < 3 {
                return None;
            }
            prev_end_corner = polyline.corner(corner_count - 3);
            end_line_no -= 1;
        }
        let line_direction = lines[end_line_no].direction().opposite();
        let prev_line_direction = lines[end_line_no].direction().opposite();
        let (acute, contact) = Self::any_end_contact(
            board,
            trace,
            false,
            &end_corner,
            &prev_end_corner,
            (&line_direction, &prev_line_direction),
        )
        .ok()??;
        // Java's newLineCount
        let new_line_count = if skip_short_segment { n } else { n + 1 };
        if acute {
            let turn = if contact.prev_corner_side == Side::OnTheLeft {
                6
            } else {
                2
            };
            let add_line = self.acute_cut_line(&end_corner, &prev_end_corner, &contact, turn)?;
            let mut new_lines = lines[..new_line_count - 2].to_vec();
            new_lines.push(add_line);
            new_lines.push(contact.line);
            return Some(Polyline::new(&new_lines));
        }
        let diff = usize::from(skip_short_segment);
        let mut check_lines = lines[diff..diff + new_line_count - 2].to_vec();
        check_lines.push(contact.line);
        check_lines.push(contact.prev_line);
        if check_lines.len() < 5 {
            // Java calls repositionLine with a negative start index and
            // throws ArrayIndexOutOfBoundsException (see the module docs)
            return None;
        }
        let start_no = check_lines.len() - 5;
        let new_line = self.reposition_line_any(board, &check_lines, start_no)?;
        let mut new_lines = lines[..n - 2].to_vec();
        new_lines.push(new_line);
        new_lines.push(contact.line);
        Some(Polyline::new(&new_lines))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run_stats::{self, Counter};
    use freerouting_geometry::IntPoint;

    fn int(x: i32, y: i32) -> Point {
        Point::Int(IntPoint { x, y })
    }

    /// N06.12 `any_angle_skip_length_squared`: the rational end corner (1, 0.5)
    /// (the line (0, 0)-(2, 1) meets x = 1). Java skips an end segment only below a
    /// squared length of 10 (3.2 units); `false` below 100 (10 units). An integer end
    /// corner never skips. `run_stats` counts, in either arm, the segments whose
    /// decision the switch's other value flips.
    #[test]
    fn skip_length_switch_both_behaviours_and_counter() {
        let _guard = run_stats::TEST_SERIAL
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        run_stats::enable();
        let end = Line::new(IntPoint { x: 0, y: 0 }, IntPoint { x: 2, y: 1 })
            .intersection(&Line::new(IntPoint { x: 1, y: 0 }, IntPoint { x: 1, y: 1 }));
        assert!(matches!(end, Point::Rational(_)));
        assert_eq!(end.to_float(), FloatPoint::new(1.0, 0.5));
        // (previous corner, Java's skip, the fixed skip): squared lengths 2.25,
        // 56.25 and 380.25.
        let cases = [
            (int(1, 2), true, true),
            (int(1, 8), false, true),
            (int(1, 20), false, false),
        ];
        for java in [true, false] {
            for (prev, java_skip, fixed_skip) in &cases {
                let (skip, counts) =
                    run_stats::capture(|| skips_short_end_segment(java, &end, prev));
                assert_eq!(
                    skip,
                    if java { *java_skip } else { *fixed_skip },
                    "{prev:?}"
                );
                let flips = u64::from(java_skip != fixed_skip);
                assert_eq!(
                    counts.get(Counter::AnyAngleSkipLengthFlips),
                    flips,
                    "{prev:?}"
                );
            }
            let (skip, counts) =
                run_stats::capture(|| skips_short_end_segment(java, &int(1, 1), &int(1, 3)));
            assert!(!skip, "an integer end corner is never skipped");
            assert_eq!(counts.get(Counter::AnyAngleSkipLengthFlips), 0);
        }
        run_stats::disable();
        run_stats::reset();
    }
}
