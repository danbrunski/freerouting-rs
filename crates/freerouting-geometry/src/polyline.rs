//! Polylines given by a sequence of lines.

use std::fmt;
use std::sync::{Arc, OnceLock};

use crate::direction::Direction;
use crate::float_point::{FloatPoint, java_max, java_min};
use crate::jmath::d2i;
use crate::limits::{INT_MAX_F64, INT_MIN_F64};
use crate::line::Line;
use crate::line_segment::LineSegment;
use crate::point::{IntPoint, Point};
use crate::polygon::Polygon;
use crate::shapes::{IntBox, IntOctagon, TileShape};
use crate::side::Side;
use crate::vector::Vector;

struct PolylineInner {
    lines: Box<[Line]>,
    corners: Box<[OnceLock<Point>]>,
    float_corners: Box<[OnceLock<FloatPoint>]>,
    bounding_box: OnceLock<IntBox>,
}

/// A polyline, described by a sequence of lines instead of points, so that its corners
/// (the intersections of consecutive lines) may be rational. The first and the last line
/// only close the polyline at its end points. A polyline is empty if it has fewer than
/// three lines.
///
/// Port of `app.freerouting.geometry.planar.Polyline`. Cheap to clone (the lines and the
/// lazily computed corners are shared).
#[derive(Clone)]
pub struct Polyline {
    inner: Arc<PolylineInner>,
}

impl fmt::Debug for Polyline {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Polyline")
            .field("lines", &self.inner.lines)
            .finish()
    }
}

impl Polyline {
    fn from_raw_lines(lines: Vec<Line>) -> Polyline {
        let corner_count = lines.len().saturating_sub(1);
        Polyline {
            inner: Arc::new(PolylineInner {
                lines: lines.into_boxed_slice(),
                corners: (0..corner_count).map(|_| OnceLock::new()).collect(),
                float_corners: (0..corner_count).map(|_| OnceLock::new()).collect(),
                bounding_box: OnceLock::new(),
            }),
        }
    }

    /// Creates a polyline through the corners of `polygon` (Java `new Polyline(Polygon)`).
    /// Empty if the polygon has fewer than two corners.
    ///
    /// # Panics
    /// If a corner is rational (Java only implements lines through `IntPoint`s).
    pub fn from_polygon(polygon: &Polygon) -> Polyline {
        let points = polygon.corners();
        if points.len() < 2 {
            // Java logs "Polyline: must contain at least 2 different points".
            return Polyline::from_raw_lines(Vec::new());
        }
        let n = points.len();
        let mut lines = Vec::with_capacity(n + 1);
        let dir = Direction::get_instance_from_points(&points[0], &points[1])
            .expect("consecutive polygon corners differ");
        lines.push(Line::get_instance(
            points[0].expect_int(),
            dir.turn_45_degree(2).expect_int(),
        ));
        for i in 1..n {
            lines.push(Line::from_points(&points[i - 1], &points[i]));
        }
        let dir = Direction::get_instance_from_points(&points[n - 1], &points[n - 2])
            .expect("consecutive polygon corners differ");
        lines.push(Line::get_instance(
            points[n - 1].expect_int(),
            dir.turn_45_degree(2).expect_int(),
        ));
        Polyline::from_raw_lines(lines)
    }

    /// Creates a polyline through `points`; repeated and collinear points are removed
    /// first (Java `new Polyline(Point[])`).
    pub fn from_points(points: &[Point]) -> Polyline {
        Polyline::from_polygon(&Polygon::new(points))
    }

    /// Creates the polyline consisting of the single segment from `from_corner` to
    /// `to_corner`; empty if the points are equal.
    ///
    /// # Panics
    /// If a point is rational.
    pub fn from_corners(from_corner: &Point, to_corner: &Point) -> Polyline {
        let Some(dir) = Direction::get_instance_from_points(from_corner, to_corner) else {
            return Polyline::from_raw_lines(Vec::new());
        };
        let perpendicular = dir.turn_45_degree(2).expect_int();
        Polyline::from_raw_lines(vec![
            Line::get_instance(from_corner.expect_int(), perpendicular),
            Line::from_points(from_corner, to_corner),
            Line::get_instance(to_corner.expect_int(), perpendicular),
        ])
    }

    /// Creates a polyline from `input_lines`. Consecutive parallel lines and overlaps are
    /// removed, and lines are reversed where needed so that the polyline is consistently
    /// directed. Empty if fewer than three lines remain.
    pub fn new(input_lines: &[Line]) -> Polyline {
        let mut filtered = remove_consecutive_parallel_lines(input_lines.to_vec());
        filtered = remove_overlaps(filtered);
        if filtered.len() < 3 {
            return Polyline::from_raw_lines(Vec::new());
        }
        // Java fills the float corner cache here with the corners computed before a line
        // is possibly reversed.
        let mut float_corners: Vec<Option<FloatPoint>> = vec![None; filtered.len() - 1];
        for i in 1..filtered.len() - 1 {
            let corner = filtered[i].intersection_approx(&filtered[i + 1]);
            float_corners[i] = Some(corner);
            let side_of_line = filtered[i - 1].side_of_float(corner);
            if side_of_line != Side::Collinear {
                let d0 = filtered[i - 1].direction();
                let d1 = filtered[i].direction();
                if d0.side_of(&d1) != side_of_line {
                    filtered[i] = filtered[i].opposite();
                }
            }
        }
        let result = Polyline::from_raw_lines(filtered);
        for (cell, corner) in result.inner.float_corners.iter().zip(float_corners) {
            if let Some(c) = corner {
                let _ = cell.set(c);
            }
        }
        result
    }

    /// The lines of this polyline.
    pub fn lines(&self) -> &[Line] {
        &self.inner.lines
    }

    /// The number of corners (the number of lines minus 1).
    pub fn corner_count(&self) -> usize {
        self.inner.lines.len().saturating_sub(1)
    }

    /// True if the polyline has fewer than three lines.
    pub fn is_empty(&self) -> bool {
        self.inner.lines.len() < 3
    }

    /// True if the polyline is empty or all its corners are equal.
    pub fn is_point(&self) -> bool {
        let lines = &self.inner.lines;
        if lines.len() < 3 {
            return true;
        }
        let first_corner = self.corner(0);
        (1..lines.len() - 1).all(|i| self.corner(i) == first_corner)
    }

    /// True if all lines are horizontal or vertical.
    pub fn is_orthogonal(&self) -> bool {
        self.inner.lines.iter().all(Line::is_orthogonal)
    }

    /// True if all lines are multiples of 45 degree.
    pub fn is_multiple_of_45_degree(&self) -> bool {
        self.inner.lines.iter().all(Line::is_multiple_of_45_degree)
    }

    /// The first corner.
    ///
    /// # Panics
    /// If the polyline has fewer than two lines.
    pub fn first_corner(&self) -> Point {
        self.corner(0)
    }

    /// The last corner.
    ///
    /// # Panics
    /// If the polyline has fewer than two lines.
    pub fn last_corner(&self) -> Point {
        self.corner(self.corner_count().saturating_sub(1))
    }

    /// All corners (exact).
    pub fn corners(&self) -> Vec<Point> {
        (0..self.corner_count()).map(|i| self.corner(i)).collect()
    }

    /// Approximations of all corners.
    pub fn corner_approx_arr(&self) -> Vec<FloatPoint> {
        (0..self.corner_count())
            .map(|i| self.corner_approx(i))
            .collect()
    }

    /// Approximation of the corner `corner_index`; an index past the last corner is
    /// clamped to the last corner (Java logs a warning).
    ///
    /// # Panics
    /// If the polyline has fewer than two lines.
    pub fn corner_approx(&self, corner_index: usize) -> FloatPoint {
        let no = self.clamp_corner_index(corner_index);
        let lines = &self.inner.lines;
        *self.inner.float_corners[no].get_or_init(|| lines[no].intersection_approx(&lines[no + 1]))
    }

    /// The corner `corner_index`, the intersection of the lines `corner_index` and
    /// `corner_index + 1`; an index past the last corner is clamped to the last corner
    /// (Java logs a warning).
    ///
    /// # Panics
    /// If the polyline has fewer than two lines (Java returns `null`).
    pub fn corner(&self, corner_index: usize) -> Point {
        let no = self.clamp_corner_index(corner_index);
        let lines = &self.inner.lines;
        self.inner.corners[no]
            .get_or_init(|| lines[no].intersection(&lines[no + 1]))
            .clone()
    }

    fn clamp_corner_index(&self, corner_index: usize) -> usize {
        let corner_count = self.corner_count();
        assert!(
            corner_count > 0,
            "Polyline.corner: polyline has fewer than 2 lines"
        );
        corner_index.min(corner_count - 1)
    }

    /// The polyline with reversed direction.
    pub fn reverse(&self) -> Polyline {
        let reversed: Vec<Line> = self.inner.lines.iter().rev().map(Line::opposite).collect();
        Polyline::new(&reversed)
    }

    /// Approximation of the length of the part between the two corners.
    pub fn length_approx_range(&self, from_corner: usize, to_corner: usize) -> f64 {
        let mut result = 0.0;
        if self.inner.lines.len() < 2 {
            return result;
        }
        let to_corner = to_corner.min(self.inner.lines.len() - 2);
        for i in from_corner..to_corner {
            result += self.corner_approx(i + 1).distance(self.corner_approx(i));
        }
        result
    }

    /// Approximation of the length of this polyline.
    pub fn length_approx(&self) -> f64 {
        self.length_approx_range(0, self.inner.lines.len().saturating_sub(2))
    }

    /// The shapes of the trace with half width `half_width` along all segments of this
    /// polyline.
    pub fn offset_shapes(&self, half_width: i32) -> Vec<TileShape> {
        self.offset_shapes_range(half_width, 0, self.inner.lines.len().saturating_sub(1))
    }

    /// The shapes of the trace with half width `half_width` around the segments between
    /// the lines `from_no` and `to_no`. Corners where the trace turns sharply ("dog
    /// ears") are cut off by the offset lines of the following and preceding segments.
    pub fn offset_shapes_range(
        &self,
        half_width: i32,
        from_no: usize,
        to_no: usize,
    ) -> Vec<TileShape> {
        let lines = &self.inner.lines;
        let to_no = to_no.min(lines.len().saturating_sub(1));
        let shape_count = to_no.saturating_sub(from_no).saturating_sub(1);
        let mut shapes = Vec::with_capacity(shape_count);
        if shape_count == 0 {
            return shapes;
        }
        let neg_half_width = f64::from(half_width.wrapping_neg());
        let mut prev_dir = lines[from_no].direction().get_vector();
        let mut curr_dir = lines[from_no + 1].direction().get_vector();
        for i in from_no + 1..to_no {
            let next_dir = lines[i + 1].direction().get_vector();
            let mut offset_lines = [lines[i]; 4];
            offset_lines[0] = lines[i].translate(neg_half_width);
            let next_dir_from_curr_dir = next_dir.side_of(&curr_dir);
            offset_lines[1] = if next_dir_from_curr_dir == Side::OnTheLeft {
                lines[i + 1].translate(neg_half_width)
            } else {
                lines[i + 1].opposite().translate(neg_half_width)
            };
            offset_lines[2] = lines[i].opposite().translate(neg_half_width);
            let curr_dir_from_prev_dir = curr_dir.side_of(&prev_dir);
            offset_lines[3] = if curr_dir_from_prev_dir == Side::OnTheLeft {
                lines[i - 1].translate(neg_half_width)
            } else {
                lines[i - 1].opposite().translate(neg_half_width)
            };

            // Cut off dog ears at the end of the segment.
            let mut corner_to_check = FloatPoint::new(0.0, 0.0);
            let mut current_line = offset_lines[1];
            let mut check_line = if next_dir_from_curr_dir == Side::OnTheLeft {
                offset_lines[2]
            } else {
                offset_lines[0]
            };
            let mut check_distance_corner = self.corner_approx(i);
            let check_dist_square = 2.0 * f64::from(half_width) * f64::from(half_width);
            let mut cut_dog_ear_lines: Vec<Line> = Vec::new();
            let mut tmp_curr_dir = next_dir.clone();
            let mut direction_changed = false;
            for j in i + 2..lines.len() - 1 {
                if self
                    .corner_approx(j - 1)
                    .distance_square(check_distance_corner)
                    > check_dist_square
                {
                    break;
                }
                if !direction_changed {
                    corner_to_check = current_line.intersection_approx(&check_line);
                }
                let tmp_next_dir = lines[j].direction().get_vector();
                let tmp_next_dir_from_tmp_curr_dir = tmp_next_dir.side_of(&tmp_curr_dir);
                direction_changed = tmp_next_dir_from_tmp_curr_dir != next_dir_from_curr_dir;
                if !direction_changed {
                    let next_border_line = if tmp_next_dir_from_tmp_curr_dir == Side::OnTheLeft {
                        lines[j].translate(neg_half_width)
                    } else {
                        lines[j].opposite().translate(neg_half_width)
                    };
                    if next_border_line.side_of_float(corner_to_check) == Side::OnTheLeft
                        && next_border_line.side_of(&self.corner(i)) == Side::OnTheRight
                        && next_border_line.side_of(&self.corner(i - 1)) == Side::OnTheRight
                    {
                        cut_dog_ear_lines.push(next_border_line);
                    }
                    tmp_curr_dir = tmp_next_dir;
                    current_line = next_border_line;
                }
            }

            // Cut off dog ears at the start of the segment.
            check_distance_corner = self.corner_approx(i - 1);
            check_line = if curr_dir_from_prev_dir == Side::OnTheLeft {
                offset_lines[2]
            } else {
                offset_lines[0]
            };
            current_line = offset_lines[3];
            tmp_curr_dir = prev_dir.clone();
            direction_changed = false;
            for j in (1..=i.saturating_sub(2)).rev() {
                if self.corner_approx(j).distance_square(check_distance_corner) > check_dist_square
                {
                    break;
                }
                if !direction_changed {
                    corner_to_check = current_line.intersection_approx(&check_line);
                }
                let tmp_prev_dir = lines[j].direction().get_vector();
                let tmp_curr_dir_from_tmp_prev_dir = tmp_curr_dir.side_of(&tmp_prev_dir);
                direction_changed = tmp_curr_dir_from_tmp_prev_dir != curr_dir_from_prev_dir;
                if !direction_changed {
                    let prev_border_line = if tmp_curr_dir_from_tmp_prev_dir == Side::OnTheLeft {
                        lines[j].translate(neg_half_width)
                    } else {
                        lines[j].opposite().translate(neg_half_width)
                    };
                    if prev_border_line.side_of_float(corner_to_check) == Side::OnTheLeft
                        && prev_border_line.side_of(&self.corner(i)) == Side::OnTheRight
                        && prev_border_line.side_of(&self.corner(i - 1)) == Side::OnTheRight
                    {
                        cut_dog_ear_lines.push(prev_border_line);
                    }
                    tmp_curr_dir = tmp_prev_dir;
                    current_line = prev_border_line;
                }
            }

            let mut s1 = TileShape::get_instance_from_lines(&offset_lines);
            if !cut_dog_ear_lines.is_empty() {
                s1 = s1.intersection(&TileShape::get_instance_from_lines(&cut_dog_ear_lines));
            }
            let bounding_shape = TileShape::IntOctagon(
                self.bounding_octagon_range(i - 1, i)
                    .offset(f64::from(half_width)),
            );
            // Java logs a warning if the result is empty.
            shapes.push(bounding_shape.intersection_with_simplify(&s1));
            prev_dir = curr_dir;
            curr_dir = next_dir;
        }
        shapes
    }

    /// The trace shape with half width `half_width` around the segment `no + 1`; `None`
    /// if `no` is out of range.
    pub fn offset_shape(&self, half_width: i32, no: usize) -> Option<TileShape> {
        if no + 3 > self.inner.lines.len() {
            // Java logs "Polyline.offsetShape: no out of range".
            return None;
        }
        self.offset_shapes_range(half_width, no, no + 2)
            .into_iter()
            .next()
    }

    /// The bounding box of the segment `no + 1`, enlarged by `half_width`.
    ///
    /// # Panics
    /// If `no + 1` is not a valid segment number.
    pub fn offset_box(&self, half_width: i32, no: usize) -> IntBox {
        LineSegment::from_polyline(self, no + 1)
            .bounding_box()
            .offset(f64::from(half_width))
    }

    /// The polyline translated by `vector`.
    pub fn translate_by(&self, vector: &Vector) -> Polyline {
        if vector.is_zero() {
            return self.clone();
        }
        let lines: Vec<Line> = self
            .inner
            .lines
            .iter()
            .map(|l| l.translate_by(vector))
            .collect();
        Polyline::new(&lines)
    }

    /// The polyline turned by `factor` times 90 degrees around `pole`.
    pub fn turn_90_degree(&self, factor: i32, pole: IntPoint) -> Polyline {
        let lines: Vec<Line> = self
            .inner
            .lines
            .iter()
            .map(|l| l.turn_90_degree(factor, pole))
            .collect();
        Polyline::new(&lines)
    }

    /// The polyline rotated by `angle` around `pole`; the corners are rounded.
    pub fn rotate_approx(&self, angle: f64, pole: FloatPoint) -> Polyline {
        if angle == 0.0 {
            return self.clone();
        }
        let new_corners: Vec<Point> = (0..self.corner_count())
            .map(|i| Point::Int(self.corner_approx(i).rotate(angle, pole).round()))
            .collect();
        Polyline::from_points(&new_corners)
    }

    /// The polyline mirrored at the vertical line through `pole`.
    pub fn mirror_vertical(&self, pole: IntPoint) -> Polyline {
        let lines: Vec<Line> = self
            .inner
            .lines
            .iter()
            .map(|l| l.mirror_vertical(pole))
            .collect();
        Polyline::new(&lines)
    }

    /// The polyline mirrored at the horizontal line through `pole`.
    pub fn mirror_horizontal(&self, pole: IntPoint) -> Polyline {
        let lines: Vec<Line> = self
            .inner
            .lines
            .iter()
            .map(|l| l.mirror_horizontal(pole))
            .collect();
        Polyline::new(&lines)
    }

    /// The smallest box containing the approximated corners `from_corner_no` to
    /// `to_corner_no`.
    pub fn bounding_box_range(&self, from_corner_no: usize, to_corner_no: usize) -> IntBox {
        let mut llx = INT_MAX_F64;
        let mut lly = llx;
        let mut urx = INT_MIN_F64;
        let mut ury = urx;
        for i in self.corner_range(from_corner_no, to_corner_no) {
            let c = self.corner_approx(i);
            llx = java_min(llx, c.x);
            lly = java_min(lly, c.y);
            urx = java_max(urx, c.x);
            ury = java_max(ury, c.y);
        }
        IntBox::from_points(
            IntPoint::new(d2i(llx.floor()), d2i(lly.floor())),
            IntPoint::new(d2i(urx.ceil()), d2i(ury.ceil())),
        )
    }

    fn corner_range(&self, from_corner_no: usize, to_corner_no: usize) -> std::ops::Range<usize> {
        let lines_len = self.inner.lines.len();
        if lines_len < 2 {
            return 0..0;
        }
        from_corner_no..to_corner_no.min(lines_len - 2).saturating_add(1)
    }

    /// The smallest box containing the approximated corners.
    pub fn bounding_box(&self) -> IntBox {
        *self.inner.bounding_box.get_or_init(|| {
            if self.corner_count() == 0 {
                // Java computes boundingBox(0, -1), an empty range.
                self.bounding_box_range(1, 0)
            } else {
                self.bounding_box_range(0, self.corner_count() - 1)
            }
        })
    }

    /// The smallest (not normalized) octagon containing the approximated corners
    /// `from_corner_no` to `to_corner_no`.
    pub fn bounding_octagon_range(&self, from_corner_no: usize, to_corner_no: usize) -> IntOctagon {
        let mut lx = INT_MAX_F64;
        let mut ly = INT_MAX_F64;
        let mut rx = INT_MIN_F64;
        let mut uy = INT_MIN_F64;
        let mut ulx = INT_MAX_F64;
        let mut lrx = INT_MIN_F64;
        let mut llx = INT_MAX_F64;
        let mut urx = INT_MIN_F64;
        for i in self.corner_range(from_corner_no, to_corner_no) {
            let c = self.corner_approx(i);
            lx = java_min(lx, c.x);
            ly = java_min(ly, c.y);
            rx = java_max(rx, c.x);
            uy = java_max(uy, c.y);
            let tmp = c.x - c.y;
            ulx = java_min(ulx, tmp);
            lrx = java_max(lrx, tmp);
            let tmp = c.x + c.y;
            llx = java_min(llx, tmp);
            urx = java_max(urx, tmp);
        }
        IntOctagon::new(
            d2i(lx.floor()),
            d2i(ly.floor()),
            d2i(rx.ceil()),
            d2i(uy.ceil()),
            d2i(ulx.floor()),
            d2i(lrx.ceil()),
            d2i(llx.floor()),
            d2i(urx.ceil()),
        )
    }

    /// Approximation of the nearest point of this polyline to `from_point`; `None` if
    /// the polyline has no corners.
    pub fn nearest_point_approx(&self, from_point: FloatPoint) -> Option<FloatPoint> {
        let mut min_distance = f64::MAX;
        let mut nearest_point = None;
        let corners = self.corner_approx_arr();
        for c in &corners {
            let curr_distance = c.distance(from_point);
            if curr_distance < min_distance {
                min_distance = curr_distance;
                nearest_point = Some(*c);
            }
        }
        const TOLERANCE: f64 = 1.0;
        let lines = &self.inner.lines;
        for i in 1..lines.len().saturating_sub(1) {
            let projection = from_point.projection_approx(&lines[i]);
            let curr_distance = projection.distance(from_point);
            if curr_distance < min_distance {
                let segment_length = corners[i].distance(corners[i - 1]);
                if projection.distance(corners[i]) + projection.distance(corners[i - 1])
                    < segment_length + TOLERANCE
                {
                    min_distance = curr_distance;
                    nearest_point = Some(projection);
                }
            }
        }
        nearest_point
    }

    /// Approximation of the distance of `from_point` to this polyline; infinite for a
    /// polyline without corners (Java fails with a `NullPointerException`).
    pub fn distance(&self, from_point: FloatPoint) -> f64 {
        self.nearest_point_approx(from_point)
            .map_or(f64::INFINITY, |p| from_point.distance(p))
    }

    /// Combines this polyline with `other` at a common end point. Returns this polyline
    /// unchanged if one of them is empty or they have no common end point.
    pub fn combine(&self, other: &Polyline) -> Polyline {
        let this_lines = &self.inner.lines;
        let other_lines = &other.inner.lines;
        if this_lines.len() < 3 || other_lines.len() < 3 {
            return self.clone();
        }
        let (combine_at_start, combine_other_at_start) =
            if self.first_corner() == other.first_corner() {
                (true, true)
            } else if self.first_corner() == other.last_corner() {
                (true, false)
            } else if self.last_corner() == other.first_corner() {
                (false, true)
            } else if self.last_corner() == other.last_corner() {
                (false, false)
            } else {
                return self.clone();
            };
        let mut new_lines = Vec::with_capacity(this_lines.len() + other_lines.len() - 2);
        if combine_at_start {
            if combine_other_at_start {
                new_lines.extend(other_lines[1..].iter().rev().map(Line::opposite));
            } else {
                new_lines.extend_from_slice(&other_lines[..other_lines.len() - 1]);
            }
            new_lines.extend_from_slice(&this_lines[1..]);
        } else {
            new_lines.extend_from_slice(&this_lines[..this_lines.len() - 1]);
            if combine_other_at_start {
                new_lines.extend_from_slice(&other_lines[1..]);
            } else {
                new_lines.extend(
                    other_lines[..other_lines.len() - 1]
                        .iter()
                        .rev()
                        .map(Line::opposite),
                );
            }
        }
        Polyline::new(&new_lines)
    }

    /// Splits this polyline at the intersection of the line `line_index` with
    /// `end_line`. Returns `None` if `line_index` is out of range, the lines are
    /// parallel, the split point is an end point, or a piece degenerates to a point.
    pub fn split(&self, line_index: usize, end_line: &Line) -> Option<[Polyline; 2]> {
        let lines = &self.inner.lines;
        if line_index < 1 || line_index + 2 > lines.len() {
            // Java logs "Polyline.split: lineIndex out of range".
            return None;
        }
        if lines[line_index].is_parallel(end_line) {
            return None;
        }
        let new_end_corner = lines[line_index].intersection(end_line);
        if (line_index == 1 && new_end_corner == self.first_corner())
            || (line_index + 2 >= lines.len() && new_end_corner == self.last_corner())
        {
            // No split at an end point.
            return None;
        }
        let mut first_piece: Vec<Line> = lines[..=line_index].to_vec();
        if self.corner(line_index - 1) != new_end_corner {
            first_piece.push(*end_line);
        }
        let mut second_piece: Vec<Line> = Vec::with_capacity(lines.len() - line_index + 1);
        if self.corner(line_index) != new_end_corner {
            second_piece.push(*end_line);
        }
        second_piece.extend_from_slice(&lines[line_index..]);
        let result = [Polyline::new(&first_piece), Polyline::new(&second_piece)];
        if result[0].is_point() || result[1].is_point() {
            return None;
        }
        Some(result)
    }

    /// The polyline without the lines `from_no` to `to_no`; unchanged if the range is
    /// invalid.
    pub fn skip_lines(&self, from_no: usize, to_no: usize) -> Polyline {
        let lines = &self.inner.lines;
        if to_no + 1 > lines.len() || from_no > to_no {
            return self.clone();
        }
        let mut new_lines = lines[..from_no].to_vec();
        new_lines.extend_from_slice(&lines[to_no + 1..]);
        Polyline::new(&new_lines)
    }

    /// True if `point` lies on one of the segments of this polyline (only implemented
    /// for integer points).
    pub fn contains(&self, point: &Point) -> bool {
        (1..self.inner.lines.len().saturating_sub(1))
            .any(|i| LineSegment::from_polyline(self, i).contains(point))
    }

    /// The shortest connection from `point` to a segment of this polyline, as a line
    /// segment perpendicular to the segment; `None` if there is no such segment.
    ///
    /// # Panics
    /// If `point` is rational (Java only implements lines through `IntPoint`s).
    pub fn projection_line(&self, point: &Point) -> Option<LineSegment> {
        let from_point = point.to_float();
        let int_point = point.expect_int();
        let mut min_distance = f64::MAX;
        let mut result_line: Option<Line> = None;
        let mut nearest_line: Option<Line> = None;
        let lines = &self.inner.lines;
        for i in 1..lines.len().saturating_sub(1) {
            let projection = from_point.projection_approx(&lines[i]);
            let curr_distance = projection.distance(from_point);
            if curr_distance < min_distance {
                let Some(direction_towards_line) = lines[i].perpendicular_direction(point) else {
                    continue;
                };
                let curr_result_line =
                    Line::from_point_and_direction(int_point, direction_towards_line.expect_int());
                let prev_corner_side = curr_result_line.side_of(&self.corner(i - 1));
                let next_corner_side = curr_result_line.side_of(&self.corner(i));
                if prev_corner_side == next_corner_side && prev_corner_side != Side::Collinear {
                    // The projection point is outside the segment.
                    continue;
                }
                nearest_line = Some(lines[i]);
                min_distance = curr_distance;
                result_line = Some(curr_result_line);
            }
        }
        let nearest_line = nearest_line?;
        let start_line = Line::from_point_and_direction(int_point, nearest_line.int_direction());
        Some(LineSegment::new(start_line, result_line?, nearest_line))
    }

    /// Shortens this polyline to `new_line_count` lines, so that the last segment has
    /// approximately the length `last_segment_length`.
    ///
    /// # Panics
    /// If `new_line_count < 3` or it exceeds the number of lines.
    pub fn shorten(&self, new_line_count: usize, last_segment_length: f64) -> Polyline {
        let lines = &self.inner.lines;
        let last_corner = self.corner_approx(new_line_count - 2);
        let prev_last_corner = self.corner_approx(new_line_count - 3);
        let new_last_corner = prev_last_corner
            .change_length(last_corner, last_segment_length)
            .round();
        if Point::Int(new_last_corner) == self.corner(self.corner_count().saturating_sub(2)) {
            return self.skip_lines(new_line_count - 1, new_line_count - 1);
        }
        let mut new_lines = lines[..new_line_count - 2].to_vec();
        let mut first_line_point = lines[new_line_count - 2].a();
        if first_line_point == new_last_corner {
            first_line_point = lines[new_line_count - 2].b();
        }
        let new_prev_last_line = Line::new(first_line_point, new_last_corner);
        new_lines.push(new_prev_last_line);
        new_lines.push(Line::get_instance(
            new_last_corner,
            new_prev_last_line.int_direction().turn_45_degree(6),
        ));
        Polyline::new(&new_lines)
    }
}

/// Removes lines parallel to their predecessor; empty if fewer than three lines remain.
fn remove_consecutive_parallel_lines(lines: Vec<Line>) -> Vec<Line> {
    if lines.len() < 3 {
        return lines;
    }
    let mut tmp: Vec<Line> = Vec::with_capacity(lines.len());
    tmp.push(lines[0]);
    for line in &lines[1..] {
        if !tmp[tmp.len() - 1].is_parallel(line) {
            tmp.push(*line);
        }
    }
    if tmp.len() == lines.len() {
        return lines;
    }
    if tmp.len() < 3 {
        return Vec::new();
    }
    tmp
}

/// Removes lines that would make consecutive segments overlap; empty if fewer than
/// three lines remain. A literal port of the Java index juggling.
///
/// # Panics
/// In the degenerate case where Java indexes the array at -1.
fn remove_overlaps(lines: Vec<Line>) -> Vec<Line> {
    let len = lines.len();
    if len < 4 {
        return lines;
    }
    let mut tmp: Vec<Line> = vec![lines[0]; len];
    let mut new_length = 0usize;
    if !lines[0].is_equal_or_opposite(&lines[2]) {
        new_length += 1;
    }
    tmp[new_length] = lines[1];
    new_length += 1;
    for i in 2..len - 2 {
        let last = new_length
            .checked_sub(1)
            .expect("Polyline.removeOverlaps: index -1 (ArrayIndexOutOfBoundsException in Java)");
        if tmp[last].is_equal_or_opposite(&lines[i + 1]) {
            // Skip line i and remove line i - 1.
            new_length -= 1;
        } else {
            tmp[new_length] = lines[i];
            new_length += 1;
        }
    }
    tmp[new_length] = lines[len - 2];
    new_length += 1;
    if new_length >= 2 && !lines[len - 1].is_equal_or_opposite(&tmp[new_length - 2]) {
        tmp[new_length] = lines[len - 1];
        new_length += 1;
    }
    if new_length == len {
        return lines;
    }
    if new_length < 3 {
        return Vec::new();
    }
    tmp.truncate(new_length);
    tmp
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shapes::{PolylineShapeOps, TileShapeOps};

    fn ip(x: i32, y: i32) -> Point {
        Point::from(IntPoint::new(x, y))
    }

    fn pl(c: &[(i32, i32)]) -> Polyline {
        let pts: Vec<Point> = c.iter().map(|&(x, y)| ip(x, y)).collect();
        Polyline::from_points(&pts)
    }

    #[test]
    fn corners_of_point_polyline() {
        let p = pl(&[(0, 0), (10, 0), (10, 10), (20, 10)]);
        assert_eq!(p.lines().len(), 5);
        assert_eq!(p.corner_count(), 4);
        assert_eq!(
            p.corners(),
            vec![ip(0, 0), ip(10, 0), ip(10, 10), ip(20, 10)]
        );
        assert_eq!(p.first_corner(), ip(0, 0));
        assert_eq!(p.last_corner(), ip(20, 10));
        assert_eq!(p.corner(99), ip(20, 10));
        assert!(p.is_orthogonal());
        assert!(p.is_multiple_of_45_degree());
        assert!(!p.is_empty());
        assert!(!p.is_point());
        assert_eq!(p.length_approx(), 30.0);
        assert_eq!(p.bounding_box(), IntBox::new(0, 0, 20, 10));
        let o = p.bounding_octagon_range(0, 3);
        assert_eq!((o.left_x, o.right_x, o.bottom_y, o.top_y), (0, 20, 0, 10));
    }

    #[test]
    fn line_constructor_filters_and_orients() {
        let p = pl(&[(0, 0), (10, 0), (10, 10)]);
        // Rebuilding from the lines keeps the polyline.
        let q = Polyline::new(p.lines());
        assert_eq!(q.corners(), p.corners());
        // Parallel consecutive lines are removed.
        let mut lines = p.lines().to_vec();
        lines.insert(2, lines[1].translate(0.0));
        let r = Polyline::new(&lines);
        assert_eq!(r.corners(), p.corners());
        assert!(Polyline::new(&p.lines()[..2]).is_empty());
    }

    #[test]
    fn reverse_and_combine() {
        let p = pl(&[(0, 0), (10, 0), (10, 10)]);
        let r = p.reverse();
        assert_eq!(r.corners(), vec![ip(10, 10), ip(10, 0), ip(0, 0)]);
        let q = pl(&[(10, 10), (20, 10), (20, 30)]);
        let c = p.combine(&q);
        assert_eq!(
            c.corners(),
            vec![ip(0, 0), ip(10, 0), ip(10, 10), ip(20, 10), ip(20, 30)]
        );
        let c2 = q.combine(&p);
        assert_eq!(
            c2.corners(),
            vec![ip(0, 0), ip(10, 0), ip(10, 10), ip(20, 10), ip(20, 30)]
        );
        let d = pl(&[(50, 50), (60, 60)]);
        assert_eq!(p.combine(&d).corners(), p.corners());
    }

    #[test]
    fn split_and_skip() {
        let p = pl(&[(0, 0), (20, 0), (20, 20)]);
        let cut = Line::from_coords(10, -5, 10, 5);
        let [a, b] = p.split(1, &cut).unwrap();
        assert_eq!(a.corners(), vec![ip(0, 0), ip(10, 0)]);
        assert_eq!(b.corners(), vec![ip(10, 0), ip(20, 0), ip(20, 20)]);
        assert!(p.split(0, &cut).is_none());
        assert!(p.split(1, &Line::from_coords(0, 1, 5, 1)).is_none());
        let at_start = Line::from_coords(0, -5, 0, 5);
        assert!(p.split(1, &at_start).is_none());
        assert_eq!(p.skip_lines(5, 1).corners(), p.corners());
    }

    #[test]
    fn contains_and_nearest_point() {
        let p = pl(&[(0, 0), (10, 0), (10, 10)]);
        assert!(p.contains(&ip(5, 0)));
        assert!(p.contains(&ip(10, 7)));
        assert!(!p.contains(&ip(5, 5)));
        let n = p.nearest_point_approx(FloatPoint::new(5.0, 3.0)).unwrap();
        assert_eq!(n, FloatPoint::new(5.0, 0.0));
        assert_eq!(p.distance(FloatPoint::new(13.0, 4.0)), 3.0);
        assert!(
            Polyline::from_points(&[])
                .nearest_point_approx(FloatPoint::new(0.0, 0.0))
                .is_none()
        );
    }

    #[test]
    fn projection_line_is_perpendicular() {
        let p = pl(&[(0, 0), (10, 0), (10, 10)]);
        let seg = p.projection_line(&ip(4, 5)).unwrap();
        assert_eq!(seg.start_point(), ip(4, 5));
        assert_eq!(seg.end_point(), ip(4, 0));
        assert!(p.projection_line(&ip(30, 30)).is_none());
    }

    #[test]
    fn offset_shapes_cover_segments() {
        let p = pl(&[(0, 0), (100, 0), (100, 100)]);
        let shapes = p.offset_shapes(10);
        assert_eq!(shapes.len(), 2);
        assert!(shapes[0].contains(&ip(50, 9)));
        assert!(shapes[0].contains(&ip(50, -10)));
        assert!(!shapes[0].contains(&ip(50, 11)));
        assert!(shapes[1].contains(&ip(109, 50)));
        let bb = shapes[0].bounding_box();
        assert_eq!(bb, IntBox::new(-10, -10, 110, 10));
        assert!(p.offset_shape(10, 0).is_some());
        assert!(p.offset_shape(10, 2).is_none());
        assert_eq!(p.offset_box(5, 0), IntBox::new(-5, -5, 105, 5));
    }

    #[test]
    fn transforms() {
        let p = pl(&[(0, 0), (10, 0), (10, 10)]);
        let t = p.translate_by(&Vector::get_instance(1, 2));
        assert_eq!(t.corners(), vec![ip(1, 2), ip(11, 2), ip(11, 12)]);
        let r = p.turn_90_degree(1, IntPoint::new(0, 0));
        assert_eq!(r.corners(), vec![ip(0, 0), ip(0, 10), ip(-10, 10)]);
        let m = p.mirror_vertical(IntPoint::new(0, 0));
        assert_eq!(m.corners(), vec![ip(0, 0), ip(-10, 0), ip(-10, 10)]);
        let h = p.mirror_horizontal(IntPoint::new(0, 0));
        assert_eq!(h.corners(), vec![ip(0, 0), ip(10, 0), ip(10, -10)]);
        let ra = p.rotate_approx(std::f64::consts::PI, FloatPoint::new(0.0, 0.0));
        assert_eq!(ra.corners(), vec![ip(0, 0), ip(-10, 0), ip(-10, -10)]);
    }

    #[test]
    fn shorten_last_segment() {
        let p = pl(&[(0, 0), (10, 0), (10, 100)]);
        let s = p.shorten(4, 40.0);
        assert_eq!(s.corners(), vec![ip(0, 0), ip(10, 0), ip(10, 40)]);
    }
}

impl Polyline {
    /// [`Polyline::new`] that also says where each output line comes from: for every
    /// line of the result, the index of the input line it is, or `None` where the
    /// constructor turned the line around (Java's `opposite()` creates a new `Line`
    /// object there).
    ///
    /// Java's `PolylineTrace.change` compares `Line` objects by reference, so the
    /// pull-tight optimizer (O01) tracks which lines of its working polyline are the
    /// trace's own. `new_mapped(lines).0` equals `Polyline::new(lines)`.
    pub fn new_mapped(input_lines: &[Line]) -> (Polyline, Vec<Option<usize>>) {
        let indexed: Vec<(Line, usize)> = input_lines.iter().copied().zip(0..).collect();
        let mut filtered = remove_overlaps_mapped(remove_parallel_mapped(indexed));
        if filtered.len() < 3 {
            return (Polyline::from_raw_lines(Vec::new()), Vec::new());
        }
        let mut origins: Vec<Option<usize>> = filtered.iter().map(|(_, i)| Some(*i)).collect();
        let mut float_corners: Vec<Option<FloatPoint>> = vec![None; filtered.len() - 1];
        for i in 1..filtered.len() - 1 {
            let corner = filtered[i].0.intersection_approx(&filtered[i + 1].0);
            float_corners[i] = Some(corner);
            let side_of_line = filtered[i - 1].0.side_of_float(corner);
            if side_of_line != Side::Collinear {
                let d0 = filtered[i - 1].0.direction();
                let d1 = filtered[i].0.direction();
                if d0.side_of(&d1) != side_of_line {
                    filtered[i].0 = filtered[i].0.opposite();
                    origins[i] = None;
                }
            }
        }
        let result = Polyline::from_raw_lines(filtered.into_iter().map(|(l, _)| l).collect());
        for (cell, corner) in result.inner.float_corners.iter().zip(float_corners) {
            if let Some(c) = corner {
                let _ = cell.set(c);
            }
        }
        (result, origins)
    }
}

/// [`remove_consecutive_parallel_lines`] carrying each line's input index.
fn remove_parallel_mapped(lines: Vec<(Line, usize)>) -> Vec<(Line, usize)> {
    if lines.len() < 3 {
        return lines;
    }
    let mut tmp: Vec<(Line, usize)> = Vec::with_capacity(lines.len());
    tmp.push(lines[0]);
    for line in &lines[1..] {
        if !tmp[tmp.len() - 1].0.is_parallel(&line.0) {
            tmp.push(*line);
        }
    }
    if tmp.len() == lines.len() {
        return lines;
    }
    if tmp.len() < 3 {
        return Vec::new();
    }
    tmp
}

/// [`remove_overlaps`] carrying each line's input index.
fn remove_overlaps_mapped(lines: Vec<(Line, usize)>) -> Vec<(Line, usize)> {
    let len = lines.len();
    if len < 4 {
        return lines;
    }
    let mut tmp: Vec<(Line, usize)> = vec![lines[0]; len];
    let mut new_length = 0usize;
    if !lines[0].0.is_equal_or_opposite(&lines[2].0) {
        new_length += 1;
    }
    tmp[new_length] = lines[1];
    new_length += 1;
    for i in 2..len - 2 {
        let last = new_length
            .checked_sub(1)
            .expect("Polyline.removeOverlaps: index -1 (ArrayIndexOutOfBoundsException in Java)");
        if tmp[last].0.is_equal_or_opposite(&lines[i + 1].0) {
            new_length -= 1;
        } else {
            tmp[new_length] = lines[i];
            new_length += 1;
        }
    }
    tmp[new_length] = lines[len - 2];
    new_length += 1;
    if new_length >= 2
        && !lines[len - 1]
            .0
            .is_equal_or_opposite(&tmp[new_length - 2].0)
    {
        tmp[new_length] = lines[len - 1];
        new_length += 1;
    }
    if new_length == len {
        return lines;
    }
    if new_length < 3 {
        return Vec::new();
    }
    tmp.truncate(new_length);
    tmp
}

#[cfg(test)]
mod new_mapped_tests {
    use super::*;
    use crate::jmath::JavaRandom;

    fn same_lines(a: &Polyline, b: &Polyline) -> bool {
        a.lines().len() == b.lines().len()
            && a.lines()
                .iter()
                .zip(b.lines())
                .all(|(x, y)| x.a() == y.a() && x.b() == y.b())
    }

    fn random_lines(random: &mut JavaRandom) -> Vec<Line> {
        let count = 2 + random.next_int(8) as usize;
        let coord = |random: &mut JavaRandom| random.next_int(9) - 4;
        let mut lines = Vec::with_capacity(count);
        while lines.len() < count {
            let (ax, ay, bx, by) = (coord(random), coord(random), coord(random), coord(random));
            if (ax, ay) != (bx, by) {
                lines.push(Line::from_coords(ax * 100, ay * 100, bx * 100, by * 100));
            }
        }
        lines
    }

    /// O01.1: `new_mapped(lines).0` equals `Polyline::new(lines)` on 1,000 seeded
    /// line arrays, and every kept line is its input line or its opposite.
    #[test]
    fn new_mapped_matches_new() {
        let mut random = JavaRandom::new(20261016);
        let (mut dropped, mut flipped) = (0, 0);
        for _ in 0..1000 {
            let lines = random_lines(&mut random);
            let (mapped, origins) = Polyline::new_mapped(&lines);
            assert!(same_lines(&mapped, &Polyline::new(&lines)), "{lines:?}");
            assert_eq!(origins.len(), mapped.lines().len());
            for (line, origin) in mapped.lines().iter().zip(&origins) {
                match origin {
                    Some(i) => assert!(line.a() == lines[*i].a() && line.b() == lines[*i].b()),
                    None => flipped += 1,
                }
            }
            if !mapped.is_empty() && mapped.lines().len() < lines.len() {
                dropped += 1;
            }
        }
        assert!(
            dropped > 0 && flipped > 0,
            "dropped {dropped}, flipped {flipped}"
        );
    }

    /// O01.1: hand-checked maps with a dropped and a flipped line.
    #[test]
    fn new_mapped_hand_cases() {
        // A parallel duplicate (index 2) is dropped.
        let lines = [
            Line::from_coords(0, 0, 0, 1),
            Line::from_coords(0, 0, 1, 0),
            Line::from_coords(0, 0, 2, 0),
            Line::from_coords(5, 0, 5, 1),
        ];
        let (p, origins) = Polyline::new_mapped(&lines);
        assert_eq!(p.lines().len(), 3);
        assert_eq!(origins, vec![Some(0), Some(1), Some(3)]);
        // The middle line points against the polyline's direction and is turned.
        let lines = [
            Line::from_coords(0, 0, 0, 1),
            Line::from_coords(10, 0, 0, 0),
            Line::from_coords(10, 0, 10, 1),
        ];
        let (p, origins) = Polyline::new_mapped(&lines);
        assert_eq!(origins, vec![Some(0), None, Some(2)]);
        assert_eq!(p.lines()[1].a(), IntPoint::new(0, 0));
        assert!(same_lines(&p, &Polyline::new(&lines)));
    }
}
