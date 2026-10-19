//! The 45° steps of the tightener.
//!
//! Port of `board/optimize/TraceTightener45.java` at `a773a70ec`:
//! `reduceCorners`, `smoothenCorners`, `smoothenSharpCorner`,
//! `smoothenNonIntegerCorner`, `smoothenCorner` and
//! `smoothenStart/EndCornerAtTrace`. Clip tests use `isOutside`.

use freerouting_geometry::jmath::d2i;
use freerouting_geometry::{
    Direction, FloatPoint, IntDirection, IntPoint, Line, Point, Polyline, SQRT2, Side, Signum,
};

use super::{LineOrigin, Outcome, Tightener, TrackedPolyline};
use crate::board::BasicBoard;
use crate::board::items::ItemId;

/// What `smoothenStart/EndCornerAtTrace` found at a trace end.
struct EndContact {
    corner_approx: FloatPoint,
    line: Line,
    prev_line: Line,
    prev_corner_side: Side,
}

impl Tightener {
    /// True if the clip contains `point` (Java `currentClipShape == null ||
    /// !currentClipShape.isOutside(point)`).
    fn in_clip(&self, point: &Point) -> bool {
        !self.clip_excludes(point)
    }

    /// The check of a corner move in `reduceCorners`: the segment from
    /// `from` to `to`, true if it is degenerate (Java's `lines.length != 3`
    /// branch) or fits.
    fn check_move(&self, board: &BasicBoard, from: &Point, to: &Point) -> Option<bool> {
        let check = Polyline::from_points(&[from.clone(), to.clone()]);
        if check.lines().len() == 3 {
            Some(self.check_segment(board, &check, 0))
        } else {
            None
        }
    }

    /// Java `reduceCorners`: removes corners by moving a segment to the
    /// next or the previous corner where the clearance allows it.
    pub fn reduce_corners(
        &mut self,
        board: &mut BasicBoard,
        polyline: &TrackedPolyline,
    ) -> Outcome {
        let p = &polyline.polyline;
        let len = p.lines().len();
        if len <= 4 {
            return Outcome::Unchanged;
        }
        let mut corner: [Point; 4] = std::array::from_fn(|i| p.corner(i));
        if corner.iter().any(|c| !matches!(c, Point::Int(_))) {
            return Outcome::Unchanged;
        }
        let mut in_clip: [bool; 4] = std::array::from_fn(|i| self.in_clip(&corner[i]));
        let mut changed = false;
        let mut new_corners: Vec<Point> = Vec::with_capacity(len - 3);
        new_corners.push(corner[0].clone());
        let mut new_corner: Option<Point> = None;
        let mut corner_index = 3;
        while corner_index < len - 1 {
            corner[3] = p.corner(corner_index);
            if !matches!(corner[3], Point::Int(_)) {
                return Outcome::Unchanged;
            }
            if corner[1] == corner[2]
                || corner_index < len - 2
                    && corner[3].side_of(&corner[1], &corner[2]) == Side::Collinear
            {
                // corners in the middle of a line can be skipped
                corner_index += 1;
                corner[2] = corner[3].clone();
                in_clip[2] = in_clip[3];
                if corner_index < len - 1 {
                    corner[3] = p.corner(corner_index);
                    if !matches!(corner[3], Point::Int(_)) {
                        return Outcome::Unchanged;
                    }
                }
                changed = true;
            }
            in_clip[3] = self.in_clip(&corner[3]);
            let mut corner_removed = false;
            if in_clip[1] && in_clip[2] && in_clip[3] {
                // move the segment from corner 2 to corner 1 to corner 3
                let delta = corner[3].difference_by(&corner[2]);
                let candidate = corner[1].translate_by(&delta);
                if corner[3] == corner[2] {
                    // just remove a multiple corner
                    corner_removed = true;
                } else if candidate.side_of(&corner[0], &corner[1]) == Side::Collinear {
                    corner_removed = match self.check_move(board, &candidate, &corner[1]) {
                        None => true,
                        Some(false) => false,
                        Some(true) => {
                            candidate == corner[3]
                                || self
                                    .check_move(board, &candidate, &corner[3])
                                    .unwrap_or(true)
                        }
                    };
                }
                new_corner = Some(candidate);
            }
            if !corner_removed && in_clip[0] && in_clip[1] && in_clip[2] {
                // move the segment from corner 2 to corner 1 to corner 0
                let delta = corner[0].difference_by(&corner[1]);
                let candidate = corner[2].translate_by(&delta);
                if corner[0] == corner[1] {
                    corner_removed = true;
                } else if candidate.side_of(&corner[2], &corner[3]) == Side::Collinear {
                    corner_removed = match self.check_move(board, &candidate, &corner[0]) {
                        None => true,
                        Some(false) => false,
                        Some(true) => self
                            .check_move(board, &candidate, &corner[2])
                            .unwrap_or(true),
                    };
                }
                new_corner = Some(candidate);
            }
            if corner_removed {
                changed = true;
                let moved = new_corner.clone().expect("set where a corner is removed");
                corner[1] = moved.clone();
                in_clip[1] = self.in_clip(&corner[1]);
                let layer = self.layer;
                board.join_changed_area(moved.to_float(), layer);
                board.join_changed_area(corner[1].to_float(), layer);
                board.join_changed_area(corner[2].to_float(), layer);
            } else {
                new_corners.push(corner[1].clone());
                corner[0] = corner[1].clone();
                corner[1] = corner[2].clone();
                in_clip[0] = in_clip[1];
                in_clip[1] = in_clip[2];
            }
            corner[2] = corner[3].clone();
            in_clip[2] = in_clip[3];
            corner_index += 1;
        }
        if !changed {
            return Outcome::Unchanged;
        }
        new_corners.push(corner[1].clone());
        new_corners.push(corner[2].clone());
        Outcome::Changed(TrackedPolyline::all_new(Polyline::from_points(
            &new_corners,
        )))
    }

    /// Java `smoothenCorners`: cuts every corner of 90° or sharper between
    /// 45°-multiple lines with a new diagonal line, repeated until nothing
    /// changes.
    pub fn smoothen_corners(
        &mut self,
        board: &mut BasicBoard,
        polyline: &TrackedPolyline,
    ) -> Outcome {
        let mut result = polyline.clone();
        let mut changed_any = false;
        loop {
            if result.lines().len() < 4 {
                break;
            }
            let mut changed = false;
            let mut lines = result.lines().to_vec();
            let mut origins = result.origins.clone();
            let mut i = 1;
            while i + 2 < lines.len() {
                let d1 = lines[i].direction();
                let d2 = lines[i + 1].direction();
                if d1.is_multiple_of_45_degree()
                    && d2.is_multiple_of_45_degree()
                    && d1.projection(&d2) != Signum::Positive
                {
                    // a 90 degree or sharper angle
                    let new_line = self
                        .smoothen_corner(board, &lines, i)
                        .or_else(|| self.smoothen_sharp_corner(board, &lines, i));
                    if let Some(line) = new_line {
                        changed = true;
                        lines.insert(i + 1, line);
                        origins.insert(i + 1, LineOrigin::New);
                        i += 1;
                    }
                }
                i += 1;
            }
            if !changed {
                break;
            }
            result = TrackedPolyline::from_lines(&lines, &origins);
            changed_any = true;
        }
        if changed_any {
            Outcome::Changed(result)
        } else {
            Outcome::Unchanged
        }
    }

    /// The diagonal through the rounded corner between `lines[no]` and
    /// `lines[no + 1]` (both 45° multiples).
    fn corner_cut_line(lines: &[Line], no: usize, corner: FloatPoint) -> Line {
        let prev_dir = lines[no].direction();
        let next_dir = lines[no + 1].direction();
        let new_dir = Direction::get_instance(&prev_dir.get_vector().add(&next_dir.get_vector()));
        Line::get_instance(corner.round(), new_dir.expect_int())
    }

    /// Java `smoothenSharpCorner`: a cut so close to the corner that no
    /// check is needed.
    fn smoothen_sharp_corner(
        &mut self,
        board: &mut BasicBoard,
        lines: &[Line],
        no: usize,
    ) -> Option<Line> {
        let current_corner = lines[no].intersection_approx(&lines[no + 1]);
        if current_corner.x != f64::from(d2i(current_corner.x)) {
            // the intersection of two diagonal lines is not integer
            if let Some(result) = Self::smoothen_non_integer_corner(lines, no) {
                return Some(result);
            }
        }
        let prev_corner = lines[no].intersection_approx(&lines[no - 1]);
        let next_corner = lines[no + 1].intersection_approx(&lines[no + 2]);
        let translate_line = Self::corner_cut_line(lines, no, current_corner);
        let mut translate_dist = (SQRT2 - 1.0) * f64::from(self.half_width);
        let prev_dist = translate_line.signed_distance(prev_corner).abs();
        let next_dist = translate_line.signed_distance(next_corner).abs();
        translate_dist = translate_dist.min(prev_dist);
        translate_dist = translate_dist.min(next_dist);
        if translate_dist < 0.99 {
            return None;
        }
        translate_dist = (translate_dist - 1.0).max(1.0);
        if translate_line.side_of_float(next_corner) == Side::OnTheLeft {
            translate_dist = -translate_dist;
        }
        let result = translate_line.translate(translate_dist);
        board.join_changed_area(current_corner, self.layer);
        Some(result)
    }

    /// Java `smoothenNonIntegerCorner`: a short axis-parallel line through
    /// the rounded rational corner of two diagonal lines.
    fn smoothen_non_integer_corner(lines: &[Line], no: usize) -> Option<Line> {
        let prev_line = &lines[no];
        let next_line = &lines[no + 1];
        if prev_line.is_equal_or_opposite(next_line) {
            return None;
        }
        if !(prev_line.is_diagonal() && next_line.is_diagonal()) {
            return None;
        }
        let current = prev_line.intersection_approx(next_line);
        let prev = prev_line.intersection_approx(&lines[no - 1]);
        let next = next_line.intersection_approx(&lines[no + 2]);
        let ceil = || IntPoint::new(d2i(current.x.ceil()), d2i(current.y.ceil()));
        let floor = || IntPoint::new(d2i(current.x.floor()), d2i(current.y.floor()));
        let (point, vertical) = if prev.x > current.x && next.x > current.x {
            (ceil(), true)
        } else if prev.x < current.x && next.x < current.x {
            (floor(), true)
        } else if prev.y > current.y && next.y > current.y {
            (ceil(), false)
        } else if prev.y < current.y && next.y < current.y {
            (floor(), false)
        } else {
            return None;
        };
        let dir = match (vertical, prev.y < next.y, prev.x < next.x) {
            (true, true, _) => IntDirection::UP,
            (true, false, _) => IntDirection::DOWN,
            (false, _, true) => IntDirection::RIGHT,
            (false, _, false) => IntDirection::LEFT,
        };
        Some(Line::from_point_and_direction(point, dir))
    }

    /// Java `smoothenCorner`: the biggest cut of the corner that keeps the
    /// clearance, by bisection.
    fn smoothen_corner(
        &mut self,
        board: &mut BasicBoard,
        lines: &[Line],
        no: usize,
    ) -> Option<Line> {
        let prev_corner = lines[no].intersection_approx(&lines[no - 1]);
        let current_corner = lines[no].intersection_approx(&lines[no + 1]);
        let next_corner = lines[no + 1].intersection_approx(&lines[no + 2]);
        let translate_line = Self::corner_cut_line(lines, no, current_corner);
        let prev_dist = translate_line.signed_distance(prev_corner).abs();
        let next_dist = translate_line.signed_distance(next_corner).abs();
        if prev_dist == 0.0 || next_dist == 0.0 {
            return None;
        }
        let (mut max_translate_dist, nearest_corner) = if prev_dist <= next_dist {
            (prev_dist, prev_corner)
        } else {
            (next_dist, next_corner)
        };
        if max_translate_dist < 1.0 {
            return None;
        }
        max_translate_dist = (max_translate_dist - 1.0).max(1.0);
        if translate_line.side_of_float(next_corner) == Side::OnTheLeft {
            max_translate_dist = -max_translate_dist;
        }
        let mut check_lines = [lines[no], lines[no], lines[no + 1]];
        let mut translate_dist = max_translate_dist;
        let mut delta_dist = max_translate_dist;
        let side_of_nearest = translate_line.side_of_float(nearest_corner);
        let sign = Signum::as_int(max_translate_dist);
        let min_translate_dist = f64::from(self.min_translate_dist);
        let mut result = None;
        while delta_dist.abs() > min_translate_dist {
            let new_line = translate_line.translate(translate_dist);
            let new_side = new_line.side_of_float(nearest_corner);
            if new_side == side_of_nearest || new_side == Side::Collinear {
                check_lines[1] = new_line;
                let tmp = Polyline::new(&check_lines);
                let mut check_ok = false;
                if tmp.lines().len() == 3 {
                    // Java's constructor reversed the middle line in check_lines.
                    // PARITY(quirk, java_quirks.polyline_array_reversal): inert, left as
                    // Java by N06.12 (see `tighten`'s module docs).
                    check_lines[1] = tmp.lines()[1];
                    check_ok = self.check_segment(board, &tmp, 0);
                }
                delta_dist /= 2.0;
                if check_ok {
                    result = Some(check_lines[1]);
                    if translate_dist == max_translate_dist {
                        // biggest possible change
                        break;
                    }
                    translate_dist += delta_dist;
                } else {
                    translate_dist -= delta_dist;
                }
            } else {
                // moved a little too far because of numerical inaccuracy
                let shorten_value = f64::from(sign) * 0.5;
                max_translate_dist -= shorten_value;
                translate_dist -= shorten_value;
                delta_dist -= shorten_value;
            }
        }
        if let Some(line) = &result {
            let layer = self.layer;
            board.join_changed_area(check_lines[0].intersection_approx(line), layer);
            board.join_changed_area(check_lines[2].intersection_approx(line), layer);
            board.join_changed_area(current_corner, layer);
        }
        result
    }

    /// The contact trace at the start (or end) of `trace` that makes an
    /// acute angle (`Some((true, …))`) or a bend (`Some((false, …))`) with
    /// it; `Err(())` where Java returns `null` because a contact isn't an
    /// unfixed trace.
    fn end_contact(
        &self,
        board: &BasicBoard,
        trace: ItemId,
        at_start: bool,
    ) -> Result<Option<(bool, EndContact)>, ()> {
        let polyline = &board.item_ref(trace).as_trace().expect("a trace").polyline;
        let lines = polyline.lines();
        let n = lines.len();
        let end_corner = if at_start {
            polyline.corner(0)
        } else {
            polyline.last_corner()
        };
        let prev_end_corner = if at_start {
            polyline.corner(1)
        } else {
            polyline.corner(polyline.corner_count() - 2)
        };
        let (line_direction, prev_line_direction) = if at_start {
            (lines[1].direction(), lines[2].direction())
        } else {
            (
                lines[n - 2].direction().opposite(),
                lines[n - 3].direction().opposite(),
            )
        };
        let contacts = if at_start {
            board.trace_start_contacts(trace)
        } else {
            board.trace_end_contacts(trace)
        };
        let mut found: Option<EndContact> = None;
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
            if other.first_corner() != end_corner && other.last_corner() != end_corner {
                return Err(());
            }
            let (corner_approx, line, prev_line) = if other.first_corner() == end_corner {
                (other.corner_approx(1), other.lines()[1], other.lines()[2])
            } else {
                let no = other.corner_count() - 2;
                (
                    other.corner_approx(no),
                    other.lines()[no + 1].opposite(),
                    other.lines()[no],
                )
            };
            let prev_corner_side = prev_end_corner.side_of_line(&line);
            let projection = line_direction.projection(&line.direction());
            let mut other_found = false;
            if projection == Signum::Positive && prev_corner_side != Side::Collinear {
                if line.direction().is_orthogonal() {
                    acute = true;
                    other_found = true;
                }
            } else if projection == Signum::Zero
                && polyline.corner_count() > 2
                && prev_line_direction.projection(&line.direction()) == Signum::Positive
            {
                bend = true;
                other_found = true;
            }
            if other_found {
                found = Some(EndContact {
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

    /// Java `TraceTightener45.smoothenStartCornerAtTrace` (`at_start`) and
    /// `smoothenEndCornerAtTrace`.
    pub(super) fn smoothen_end_corner_45(
        &mut self,
        board: &mut BasicBoard,
        trace: ItemId,
        at_start: bool,
    ) -> Option<Polyline> {
        let polyline = board.item_ref(trace).as_trace()?.polyline.clone();
        let end_corner = if at_start {
            polyline.corner(0)
        } else {
            polyline.last_corner()
        };
        if self.clip_excludes(&end_corner) {
            return None;
        }
        let prev_end_corner = if at_start {
            polyline.corner(1)
        } else {
            polyline.corner(polyline.corner_count() - 2)
        };
        let (acute, contact) = self.end_contact(board, trace, at_start).ok()??;
        let lines = polyline.lines();
        let n = lines.len();
        if acute {
            let turn = match (contact.prev_corner_side == Side::OnTheLeft, at_start) {
                (true, true) | (false, false) => 2,
                _ => 6,
            };
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
            if translate_dist >= 0.99 {
                translate_dist = (translate_dist - 1.0).max(1.0);
                if translate_line.side_of(&prev_end_corner) == Side::OnTheLeft {
                    translate_dist = -translate_dist;
                }
                let add_line = translate_line.translate(translate_dist);
                let mut new_lines = Vec::with_capacity(n + 1);
                if at_start {
                    new_lines.push(contact.line);
                    new_lines.push(add_line);
                    new_lines.extend_from_slice(&lines[1..]);
                } else {
                    new_lines.extend_from_slice(&lines[..n - 1]);
                    new_lines.push(add_line);
                    new_lines.push(contact.line);
                }
                return Some(Polyline::new(&new_lines));
            }
        } else {
            let (check_lines, no) = if at_start {
                let mut v = vec![contact.prev_line, contact.line];
                v.extend_from_slice(&lines[1..]);
                (v, 2)
            } else {
                let mut v = lines[..n - 1].to_vec();
                v.push(contact.line);
                v.push(contact.prev_line);
                (v, n - 2)
            };
            if let Some(new_line) = self.reposition_line(board, &check_lines, no) {
                let mut new_lines = Vec::with_capacity(n);
                if at_start {
                    new_lines.push(contact.line);
                    new_lines.push(new_line);
                    new_lines.extend_from_slice(&lines[2..]);
                } else {
                    new_lines.extend_from_slice(&lines[..n - 2]);
                    new_lines.push(new_line);
                    new_lines.push(contact.line);
                }
                return Some(Polyline::new(&new_lines));
            }
        }
        None
    }
}
