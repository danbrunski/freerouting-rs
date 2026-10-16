//! The 90° steps of the tightener.
//!
//! Port of `board/optimize/TraceTightener90.java` at `a773a70ec`:
//! `trySkipSecondCorner` and `trySkipCorners`. Clip tests use `contains`
//! of the float corner; the end-corner smoothing returns nothing.

use freerouting_geometry::{FloatPoint, Line, Polyline, TileShapeOps};

use super::{LineOrigin, Outcome, Tightener, TrackedPolyline};
use crate::board::BasicBoard;

impl Tightener {
    /// True if the clip contains `point` (Java `currentClipShape == null ||
    /// currentClipShape.contains(point)`).
    fn clip_contains_float(&self, point: FloatPoint) -> bool {
        self.clip.as_ref().is_none_or(|c| c.contains_float(point))
    }

    /// Java `trySkipSecondCorner`: drops the second corner by swapping the
    /// first two lines when both new segments fit.
    pub fn try_skip_second_corner(
        &mut self,
        board: &mut BasicBoard,
        polyline: &TrackedPolyline,
    ) -> Outcome {
        let lines = polyline.lines();
        if lines.len() < 5 {
            return Outcome::Unchanged;
        }
        let check_lines = [lines[1], lines[0], lines[3], lines[4]];
        let check = Polyline::new(&check_lines);
        if check.lines().len() != 4 || !self.clip_contains_float(check.corner_approx(1)) {
            return Outcome::Unchanged;
        }
        for i in 0..2 {
            if !self.check_segment(board, &check, i) {
                return Outcome::Unchanged;
            }
        }
        // now the second corner can be skipped
        let mut new_lines = Vec::with_capacity(lines.len() - 1);
        let mut origins = Vec::with_capacity(lines.len() - 1);
        new_lines.extend_from_slice(&[lines[1], lines[0]]);
        origins.extend_from_slice(&[polyline.origins[1], polyline.origins[0]]);
        new_lines.extend_from_slice(&lines[3..]);
        origins.extend_from_slice(&polyline.origins[3..]);
        Outcome::Changed(TrackedPolyline::from_lines(&new_lines, &origins))
    }

    /// Java `trySkipCorners`: skips pairs of corners where the shortcut
    /// segments fit.
    pub fn try_skip_corners(
        &mut self,
        board: &mut BasicBoard,
        polyline: &TrackedPolyline,
    ) -> Outcome {
        let lines = polyline.lines();
        let origins = &polyline.origins;
        let len = lines.len();
        let mut new_lines: Vec<Line> = Vec::with_capacity(len);
        let mut new_origins: Vec<LineOrigin> = Vec::with_capacity(len);
        new_lines.extend_from_slice(&lines[..2]);
        new_origins.extend_from_slice(&origins[..2]);
        let mut changed = false;
        let mut second_last_corner_skipped = false;
        let mut i = 5;
        while i <= len {
            let mut skip_lines = false;
            let mut check_lines = [lines[0]; 4];
            if self.clip_contains_float(polyline.polyline.corner_approx(i - 3)) {
                let last = new_lines.len() - 1;
                check_lines[0] = new_lines[last - 1];
                check_lines[1] = new_lines[last];
                check_lines[2] = lines[i - 1];
                // the concluding line is the second last line at the end
                check_lines[3] = if i < len { lines[i] } else { lines[i - 2] };
                let check = Polyline::new(&check_lines);
                skip_lines =
                    check.lines().len() == 4 && self.clip_contains_float(check.corner_approx(1));
                if skip_lines {
                    skip_lines = self.check_segment(board, &check, 0);
                }
                if skip_lines {
                    skip_lines = self.check_segment(board, &check, 1);
                }
            }
            if skip_lines {
                if i == len {
                    second_last_corner_skipped = true;
                }
                let layer = self.layer;
                let new_corner = check_lines[1].intersection_approx(&check_lines[2]);
                board.join_changed_area(new_corner, layer);
                let skipped_corner = lines[i - 2].intersection_approx(&lines[i - 3]);
                board.join_changed_area(skipped_corner, layer);
                changed = true;
                i += 1;
            } else {
                new_lines.push(lines[i - 3]);
                new_origins.push(origins[i - 3]);
            }
            i += 1;
        }
        if !changed {
            return Outcome::Unchanged;
        }
        if second_last_corner_skipped {
            // the second last corner of the polyline was skipped
            new_lines.extend_from_slice(&[lines[len - 1], lines[len - 2]]);
            new_origins.extend_from_slice(&[origins[len - 1], origins[len - 2]]);
        } else {
            new_lines.extend_from_slice(&lines[len - 3..]);
            new_origins.extend_from_slice(&origins[len - 3..]);
        }
        Outcome::Changed(TrackedPolyline::from_lines(&new_lines, &new_origins))
    }
}
