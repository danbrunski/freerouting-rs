//! Entry sides: where a shove comes into a shape (O03.1).
//!
//! Port of `board.model.structure.ShapeEntrySide` and `ShapeAndEntrySide`
//! (Java at `a773a70ec`). A [`ShapeEntrySide`] names the border line of a
//! tile shape a shove comes from, and optionally the point on it where the
//! pushing trace crosses that border. [`ShapeAndEntrySide`] is the shape of
//! one segment of a substitute trace, cut so that it has no dog ears at the
//! trace's ends, with the entry side of the cut.
//!
//! Java's `ShapeAndEntrySide` tests `tmpShape != currentShape` after
//! intersecting with a cut half-plane. `Simplex.intersection(Simplex)` always
//! builds a new object (`removeRedundantLines` returns `this` of the new
//! simplex), so that test is always true and only the emptiness test
//! decides; the port tests emptiness only.

use freerouting_geometry::{
    FloatLine, FloatPoint, Line, LineSegment, Point, Polyline, PolylineShapeOps, Side, TileShape,
    TileShapeOps,
};

use crate::board::BasicBoard;
use crate::board::items::Item;
use crate::board::search::ShapeSearchTree;

/// The side of a shape a shove comes from (Java `ShapeEntrySide`).
///
/// `no` is Java's `no`, `None` for -1 (not calculated);
/// `border_intersection` is `null` in Java when `None`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeEntrySide {
    pub no: Option<usize>,
    pub border_intersection: Option<FloatPoint>,
}

impl ShapeEntrySide {
    /// Java `ShapeEntrySide.NOT_CALCULATED`.
    pub const NOT_CALCULATED: ShapeEntrySide = ShapeEntrySide {
        no: None,
        border_intersection: None,
    };

    /// Java `ShapeEntrySide(int no, FloatPoint borderIntersection)`: values
    /// already calculated.
    pub fn from_values(no: Option<usize>, border_intersection: Option<FloatPoint>) -> Self {
        ShapeEntrySide {
            no,
            border_intersection,
        }
    }

    /// Java `ShapeEntrySide(Polyline, int no, TileShape)`: the border line of
    /// `shape` where `polyline` enters it, searched backwards from segment
    /// `no` (expected between 1 and the line count - 2).
    pub fn from_polyline(polyline: &Polyline, no: usize, shape: &TileShape) -> Self {
        for current_no in (1..=no).rev() {
            let current_seg = LineSegment::from_polyline(polyline, current_no);
            let intersections = current_seg.border_intersections(shape);
            if let Some(&first) = intersections.first() {
                let intersection = current_seg
                    .get_line()
                    .intersection_approx(&shape.border_line(first));
                return ShapeEntrySide {
                    no: Some(first),
                    border_intersection: Some(intersection),
                };
            }
        }
        // The first corner of the polyline is inside the shape: the border
        // intersection of lines[1] nearest to the first corner.
        let from_point = polyline.corner_approx(0);
        let check_line = &polyline.lines()[1];
        let mut min_dist = f64::MAX;
        let mut result = ShapeEntrySide::NOT_CALCULATED;
        for i in 0..shape.border_line_count() {
            let current_intersection = check_line.intersection_approx(&shape.border_line(i));
            let current_distance = current_intersection.distance(from_point).abs();
            if current_distance < min_dist {
                result = ShapeEntrySide {
                    no: Some(i),
                    border_intersection: Some(current_intersection),
                };
                min_dist = current_distance;
            }
        }
        result
    }

    /// Java `ShapeEntrySide(Point, TileShape)`: the border side of `shape`
    /// nearest to `from_point` (the shove direction of a drill item).
    ///
    /// Java logs a warning when the projection lies on no border line and
    /// keeps `no = -1`; the port keeps `None`.
    pub fn from_point(from_point: &Point, shape: &TileShape) -> Self {
        // Java throws on an empty shape (no border line); no caller passes one.
        let Some(border_projection) = shape.nearest_border_point(from_point) else {
            return ShapeEntrySide::NOT_CALCULATED;
        };
        ShapeEntrySide {
            no: shape.contains_on_border_line_no(&border_projection),
            border_intersection: Some(border_projection.to_float()),
        }
    }

    /// Java `ShapeEntrySide(LineSegment, TileShape, boolean shoveToTheLeft)`:
    /// the side of `shape` at the start of `line_segment`, turned by two
    /// sides to the left (`shove_to_the_left`) or right, with the middle of
    /// that side as border intersection.
    pub fn from_segment(
        line_segment: &LineSegment,
        shape: &TileShape,
        shove_to_the_left: bool,
    ) -> Self {
        let start_corner = line_segment.start_point_approx();
        let end_corner = line_segment.end_point_approx();
        let border_line_count = shape.border_line_count();
        let check_line = line_segment.get_line();
        let first_corner = shape.corner_approx(0);
        let mut prev_side = check_line.side_of_float(first_corner);
        let mut front_side_no = None;
        for i in 1..=border_line_count {
            let next_corner = if i == border_line_count {
                first_corner
            } else {
                shape.corner_approx(i)
            };
            let next_side = check_line.side_of_float(next_corner);
            if prev_side != next_side {
                let current_intersection =
                    shape.border_line(i - 1).intersection_approx(&check_line);
                if current_intersection.distance_square(start_corner)
                    < current_intersection.distance_square(end_corner)
                {
                    front_side_no = Some(i - 1);
                    break;
                }
            }
            prev_side = next_side;
        }
        let front_side_no =
            front_side_no.unwrap_or_else(|| nearest_side(shape, start_corner, border_line_count));
        let no = if shove_to_the_left {
            (front_side_no + 2) % border_line_count
        } else {
            (front_side_no + border_line_count - 2) % border_line_count
        };
        let prev_corner = shape.corner_approx(no);
        let next_corner = shape.corner_approx((no + 1) % border_line_count);
        ShapeEntrySide {
            no: Some(no),
            border_intersection: Some(prev_corner.middle_point(next_corner)),
        }
    }
}

/// The fallback of Java's segment constructor: the side whose perpendicular
/// projection of `start_corner` lies on the side (tolerance 0.01) and is
/// nearest, 0 if none.
fn nearest_side(shape: &TileShape, start_corner: FloatPoint, border_line_count: usize) -> usize {
    let mut min_distance = f64::MAX;
    let mut nearest = 0;
    for i in 0..border_line_count {
        let border = shape.border_line(i);
        let border_line = FloatLine::new(border.a().to_float(), border.b().to_float());
        let projection = border_line.perpendicular_projection(start_corner);
        let side_start = shape.corner_approx(i);
        let side_end = shape.corner_approx((i + 1) % border_line_count);
        if projection.is_contained_in_box(side_start, side_end, 0.01) {
            let distance = start_corner.distance(projection);
            if distance < min_distance {
                min_distance = distance;
                nearest = i;
            }
        }
    }
    nearest
}

/// The shape of one segment of a substitute trace and the side a shove
/// comes from (Java `ShapeAndEntrySide`).
#[derive(Clone, Debug)]
pub struct ShapeAndEntrySide {
    pub shape: TileShape,
    /// `None` where Java's `fromSide` is `null`.
    pub from_side: Option<ShapeEntrySide>,
}

impl ShapeAndEntrySide {
    /// Java `ShapeAndEntrySide(trace, index, orthogonal, inShoveCheck)` for
    /// trace `trace` (on the board or a temporary substitute piece) and its
    /// segment `index`. In 90 degree mode (`orthogonal`) the shape is the
    /// bounding box of the default tree's shape; otherwise the tree shape is
    /// cut at the trace's first and last line where they are within the
    /// compensated half width, and the cut gives the entry side. Without a
    /// cut, the side comes from [`ShapeEntrySide::from_polyline`] unless
    /// `in_shove_check`.
    ///
    /// `None` if the trace has no tree shape `index` (Java throws).
    pub fn new(
        board: &BasicBoard,
        trace: &Item,
        index: usize,
        orthogonal: bool,
        in_shove_check: bool,
    ) -> Option<Self> {
        let tree = board.search_trees().default_tree();
        let polyline = &trace.as_trace()?.polyline;
        let mut current_shape = tree.tree_shape(trace, index, &board.shape_context())?;
        let mut from_side = None;
        if orthogonal {
            current_shape = TileShape::IntBox(current_shape.bounding_box());
        } else {
            // Prevent dog ears at the start and the end of the substitute trace.
            current_shape = TileShape::Simplex(current_shape.to_simplex());
            let half_width = compensated_half_width_of(board, tree, trace);
            let end_cutline = cutline_at_end(index, polyline, half_width);
            let mut cut_off_at_end = false;
            if let Some(line) = &end_cutline {
                let cut = current_shape.intersection(&TileShape::get_instance_half_plane(*line));
                if !cut.is_empty() {
                    current_shape = TileShape::Simplex(cut.to_simplex());
                    cut_off_at_end = true;
                }
            }
            let start_cutline = cutline_at_start(index, polyline, half_width);
            let mut cut_off_at_start = false;
            if let Some(line) = &start_cutline {
                let cut = current_shape.intersection(&TileShape::get_instance_half_plane(*line));
                if !cut.is_empty() {
                    current_shape = TileShape::Simplex(cut.to_simplex());
                    cut_off_at_start = true;
                }
            }
            let mut side = None;
            if cut_off_at_start {
                let line = start_cutline.expect("start cut line");
                side = current_shape.border_line_index(&line).map(|i| (i, line));
            }
            if side.is_none() && cut_off_at_end {
                let line = end_cutline.expect("end cut line");
                side = current_shape.border_line_index(&line).map(|i| (i, line));
            }
            if let Some((side_index, line)) = side {
                let border_intersection =
                    line.intersection_approx(&current_shape.border_line(side_index));
                from_side = Some(ShapeEntrySide::from_values(
                    Some(side_index),
                    Some(border_intersection),
                ));
            }
        }
        if from_side.is_none() && !in_shove_check {
            // In a shove check this may give an undesired stack level > 1 in
            // ShapeTraceEntries.
            from_side = Some(ShapeEntrySide::from_polyline(
                polyline,
                index,
                &current_shape,
            ));
        }
        Some(ShapeAndEntrySide {
            shape: current_shape,
            from_side,
        })
    }
}

/// Java `Trace.getCompensatedHalfWidth(tree)` for a trace item that may not
/// be on the board.
pub(crate) fn compensated_half_width_of(
    board: &BasicBoard,
    tree: &ShapeSearchTree,
    trace: &Item,
) -> i32 {
    let Some(t) = trace.as_trace() else {
        return 0;
    };
    t.half_width
        + tree.clearance_compensation_value(
            &board.rules.clearance_matrix,
            trace.clearance_class(),
            t.layer,
        )
}

/// The cut line at the trace's end: its last line, turned so that the
/// third-last corner is on its right.
fn cutline_at_end(index: usize, lines: &Polyline, half_width: i32) -> Option<Line> {
    let n = lines.lines().len();
    if index == n - 3
        || lines
            .corner_approx(n - 2)
            .distance(lines.corner_approx(index + 1))
            < f64::from(half_width)
    {
        let current_line = lines.lines()[n - 1];
        let is = lines.corner_approx(n - 3);
        return Some(oriented_cutline(current_line, is));
    }
    None
}

/// The cut line at the trace's start: its first line, turned so that the
/// second corner is on its right.
fn cutline_at_start(index: usize, lines: &Polyline, half_width: i32) -> Option<Line> {
    if index == 0
        || lines.corner_approx(0).distance(lines.corner_approx(index)) < f64::from(half_width)
    {
        let current_line = lines.lines()[0];
        let is = lines.corner_approx(1);
        return Some(oriented_cutline(current_line, is));
    }
    None
}

fn oriented_cutline(line: Line, point: FloatPoint) -> Line {
    if line.side_of_float(point) == Side::OnTheLeft {
        line.opposite()
    } else {
        line
    }
}
