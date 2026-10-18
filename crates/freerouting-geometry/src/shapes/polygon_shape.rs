//! Shapes bounded by a closed polygon.

use std::sync::{Arc, OnceLock};

use crate::float_point::{FloatPoint, java_max, java_min};
use crate::jmath::{JavaRandom, d2i};
use crate::line::Line;
use crate::point::{IntPoint, Point};
use crate::polygon::Polygon;
use crate::side::Side;
use crate::vector::Vector;

use super::{Circle, IntBox, IntOctagon, PolylineShapeOps, Simplex, TileShape, TileShapeOps};

const SEED: i64 = 99;

struct PolygonShapeInner {
    corners: Box<[Point]>,
    bounding_box: OnceLock<IntBox>,
    bounding_octagon: OnceLock<IntOctagon>,
    convex_pieces: OnceLock<Vec<TileShape>>,
}

/// A shape bounded by a closed polygon. The corners are ordered counterclockwise,
/// starting with the corner with the lowest y (then lowest x) coordinate.
///
/// Port of `app.freerouting.geometry.planar.PolygonShape`. Cheap to clone (the corners
/// and caches are shared).
#[derive(Clone)]
pub struct PolygonShape {
    inner: Arc<PolygonShapeInner>,
}

impl std::fmt::Debug for PolygonShape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PolygonShape")
            .field("corners", &self.inner.corners)
            .finish()
    }
}

impl PolygonShape {
    /// Creates the shape from `polygon`, normalizing the orientation and the first
    /// corner.
    pub fn from_polygon(polygon: &Polygon) -> PolygonShape {
        let current_polygon = if polygon.winding_number_after_closing() < 0 {
            polygon.revert_corners()
        } else {
            polygon.clone()
        };
        let current_corners = current_polygon.corner_array();
        let mut last_corner_no = current_corners.len() as isize - 1;
        if last_corner_no > 0 && current_corners[0] == current_corners[last_corner_no as usize] {
            last_corner_no -= 1;
        }
        let mut last_point_collinear = false;
        if last_corner_no >= 2 {
            let l = last_corner_no as usize;
            last_point_collinear = current_corners[l]
                .side_of(&current_corners[l - 1], &current_corners[0])
                == Side::Collinear;
        }
        if last_point_collinear {
            last_corner_no -= 1;
        }
        let mut first_corner_no: isize = 0;
        let mut first_point_collinear = false;
        if last_corner_no - first_corner_no >= 2 {
            first_point_collinear = current_corners[0].side_of(
                &current_corners[1],
                &current_corners[last_corner_no as usize],
            ) == Side::Collinear;
        }
        if first_point_collinear {
            first_corner_no += 1;
        }
        let mut result = Vec::new();
        if last_corner_no >= first_corner_no {
            let first = first_corner_no as usize;
            let last = last_corner_no as usize;
            let mut start_corner_no = first;
            let mut start_corner = current_corners[start_corner_no].to_float();
            for (i, corner) in current_corners
                .iter()
                .enumerate()
                .take(last + 1)
                .skip(first + 1)
            {
                let current_corner = corner.to_float();
                if current_corner.y < start_corner.y
                    || (current_corner.y == start_corner.y && current_corner.x < start_corner.x)
                {
                    start_corner_no = i;
                    start_corner = current_corner;
                }
            }
            result.extend_from_slice(&current_corners[start_corner_no..=last]);
            result.extend_from_slice(&current_corners[first..start_corner_no]);
        }
        PolygonShape::from_normalized_corners(result)
    }

    /// Creates the shape from an array of corner points.
    pub fn new(corners: &[Point]) -> PolygonShape {
        PolygonShape::from_polygon(&Polygon::new(corners))
    }

    fn from_normalized_corners(corners: Vec<Point>) -> PolygonShape {
        PolygonShape {
            inner: Arc::new(PolygonShapeInner {
                corners: corners.into_boxed_slice(),
                bounding_box: OnceLock::new(),
                bounding_octagon: OnceLock::new(),
                convex_pieces: OnceLock::new(),
            }),
        }
    }

    /// The normalized corners.
    pub fn corners(&self) -> &[Point] {
        &self.inner.corners
    }

    fn pieces_or_empty(&self) -> Vec<TileShape> {
        // Java dereferences the (possibly null) result of `splitToConvex` here; a failed
        // split is treated as having no pieces.
        self.split_to_convex().unwrap_or_default()
    }

    /// True if one of the convex pieces intersects `circle`.
    pub fn intersects_circle(&self, circle: &Circle) -> bool {
        self.pieces_or_empty()
            .iter()
            .any(|p| p.intersects_circle(circle))
    }

    /// True if one of the convex pieces intersects `simplex`.
    pub fn intersects_simplex(&self, simplex: &Simplex) -> bool {
        self.pieces_or_empty()
            .iter()
            .any(|p| p.intersects_simplex(simplex))
    }

    /// True if one of the convex pieces intersects `oct`.
    pub fn intersects_octagon(&self, oct: &IntOctagon) -> bool {
        self.pieces_or_empty()
            .iter()
            .any(|p| p.intersects_octagon(oct))
    }

    /// True if one of the convex pieces intersects `b`.
    pub fn intersects_box(&self, b: &IntBox) -> bool {
        self.pieces_or_empty().iter().any(|p| p.intersects_box(b))
    }

    /// True if a convex piece of this polygon intersects a convex piece of `other`.
    ///
    /// Java recurses forever here (`PolygonShape.intersects(Shape)` calls itself through
    /// `other.intersects(this)`); this port compares the convex pieces.
    pub fn intersects_polygon(&self, other: &PolygonShape) -> bool {
        let other_pieces = other.pieces_or_empty();
        self.pieces_or_empty()
            .iter()
            .any(|p| other_pieces.iter().any(|q| p.intersects_tile(q)))
    }

    /// The shape enlarged by `offset`: itself for 0, otherwise `None` (not implemented
    /// in Java, which returns `null`).
    pub fn enlarge(&self, offset: f64) -> Option<PolygonShape> {
        if offset == 0.0 {
            return Some(self.clone());
        }
        None
    }

    /// Not implemented in Java: always 0.
    pub fn border_distance(&self, _point: FloatPoint) -> f64 {
        0.0
    }

    /// [`PolygonShape::border_distance`] from the centre of gravity (always 0).
    pub fn smallest_radius(&self) -> f64 {
        self.border_distance(self.centre_of_gravity())
    }

    /// True if one of the convex pieces contains `point`.
    pub fn contains_float(&self, point: FloatPoint) -> bool {
        self.pieces_or_empty()
            .iter()
            .any(|p| p.contains_float(point))
    }

    /// True if `point` is inside or on the border.
    pub fn contains(&self, point: &Point) -> bool {
        !self.is_outside(point)
    }

    /// True if `point` is inside but not on the border (the border test is not
    /// implemented in Java and always false).
    pub fn contains_inside(&self, point: &Point) -> bool {
        if self.contains_on_border(point) {
            return false;
        }
        !self.is_outside(point)
    }

    /// True if `point` is outside all convex pieces.
    pub fn is_outside(&self, point: &Point) -> bool {
        self.pieces_or_empty().iter().all(|p| p.is_outside(point))
    }

    /// Not implemented in Java: always false.
    pub fn contains_on_border(&self, _point: &Point) -> bool {
        false
    }

    /// Not implemented in Java: always 0.
    pub fn distance(&self, _point: FloatPoint) -> f64 {
        0.0
    }

    /// The shape translated by `vector`.
    pub fn translate_by(&self, vector: &Vector) -> PolygonShape {
        if vector.is_zero_constant() {
            return self.clone();
        }
        let new_corners: Vec<Point> = self
            .corners()
            .iter()
            .map(|c| c.translate_by(vector))
            .collect();
        PolygonShape::new(&new_corners)
    }

    /// The smallest surrounding octagon (never `None` for a polygon).
    pub fn polygon_bounding_octagon(&self) -> IntOctagon {
        *self.inner.bounding_octagon.get_or_init(|| {
            let mut lx = f64::from(i32::MAX);
            let mut ly = f64::from(i32::MAX);
            let mut rx = f64::from(i32::MIN);
            let mut uy = f64::from(i32::MIN);
            let mut ulx = f64::from(i32::MAX);
            let mut lrx = f64::from(i32::MIN);
            let mut llx = f64::from(i32::MAX);
            let mut urx = f64::from(i32::MIN);
            for corner in self.corners() {
                let current = corner.to_float();
                lx = java_min(lx, current.x);
                ly = java_min(ly, current.y);
                rx = java_max(rx, current.x);
                uy = java_max(uy, current.y);
                let tmp = current.x - current.y;
                ulx = java_min(ulx, tmp);
                lrx = java_max(lrx, tmp);
                let tmp = current.x + current.y;
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
        })
    }

    /// True if every segment between two points of the shape lies in the shape.
    pub fn is_convex(&self) -> bool {
        let corners = self.corners();
        let n = corners.len();
        if n <= 2 {
            return true;
        }
        let mut prev_point = &corners[n - 1];
        let mut current_point = &corners[0];
        let mut next_point = &corners[1];
        for ind in 0..n {
            if next_point.side_of(prev_point, current_point) == Side::OnTheRight {
                return false;
            }
            prev_point = current_point;
            current_point = next_point;
            next_point = if ind == n - 2 {
                &corners[0]
            } else if ind == n - 1 {
                &corners[1]
            } else {
                &corners[ind + 2]
            };
        }
        // Check that the sum of the interior angles is at most 2 * pi.
        let first_line = Line::from_points(&corners[n - 1], &corners[0]);
        let mut current_line = Line::from_points(&corners[0], &corners[1]);
        let first_direction = first_line.int_direction();
        let mut last_det = first_direction.determinant(&current_line.int_direction());
        for corner in &corners[2..] {
            current_line = Line::new(current_line.b(), corner.expect_int());
            let current_det = first_direction.determinant(&current_line.int_direction());
            if last_det <= 0.0 && current_det > 0.0 {
                return false;
            }
            last_det = current_det;
        }
        true
    }

    /// The convex hull of this polygon shape.
    pub fn convex_hull(&self) -> PolygonShape {
        let corners = self.corners();
        let n = corners.len();
        if n <= 2 {
            return self.clone();
        }
        let mut prev_point = &corners[n - 1];
        let mut current_point = &corners[0];
        for ind in 0..n {
            let next_point = if ind == n - 1 {
                &corners[0]
            } else {
                &corners[ind + 1]
            };
            if next_point.side_of(prev_point, current_point) != Side::OnTheLeft {
                let mut new_corners = corners.to_vec();
                new_corners.remove(ind);
                return PolygonShape::new(&new_corners).convex_hull();
            }
            prev_point = current_point;
            current_point = next_point;
        }
        self.clone()
    }

    /// A bounding convex tile: the shape of the convex hull.
    ///
    /// # Panics
    /// If the shape is empty.
    pub fn bounding_tile(&self) -> TileShape {
        let hull = self.convex_hull();
        let hc = hull.corners();
        let n = hc.len();
        let mut bounding_lines = Vec::with_capacity(n);
        for i in 0..n - 1 {
            bounding_lines.push(Line::from_points(&hc[i], &hc[i + 1]));
        }
        bounding_lines.push(Line::from_points(&hc[n - 1], &hc[0]));
        TileShape::get_instance_from_lines(&bounding_lines)
    }

    /// The area of the shape.
    pub fn area(&self) -> f64 {
        if self.dimension() < 2 {
            return 0.0;
        }
        let corners = self.corners();
        let n = corners.len();
        let mut result = 0.0;
        let mut prev_corner = corners[n - 2].to_float();
        let mut current_corner = corners[n - 1].to_float();
        for corner in corners {
            let next_corner = corner.to_float();
            result += current_corner.x * (next_corner.y - prev_corner.y);
            prev_corner = current_corner;
            current_corner = next_corner;
        }
        0.5 * result.abs()
    }

    /// Approximation of the nearest point of the shape to `from_point`.
    pub fn nearest_point_approx(&self, from_point: FloatPoint) -> Option<FloatPoint> {
        let mut min_dist = f64::MAX;
        let mut result = None;
        for piece in self.pieces_or_empty() {
            if let Some(current_nearest_point) = piece.nearest_point_approx(from_point) {
                let current_distance = current_nearest_point.distance_square(from_point);
                if current_distance < min_dist {
                    min_dist = current_distance;
                    result = Some(current_nearest_point);
                }
            }
        }
        result
    }

    /// The shape turned by `factor` times 90 degrees around `pole`.
    pub fn turn_90_degree(&self, factor: i32, pole: IntPoint) -> PolygonShape {
        let pole = Point::Int(pole);
        let new_corners: Vec<Point> = self
            .corners()
            .iter()
            .map(|c| c.turn_90_degree(factor, &pole))
            .collect();
        PolygonShape::new(&new_corners)
    }

    /// The shape rotated by `angle` around `pole`; not exact.
    pub fn rotate_approx(&self, angle: f64, pole: FloatPoint) -> PolygonShape {
        if angle == 0.0 {
            return self.clone();
        }
        let new_corners: Vec<Point> = self
            .corners()
            .iter()
            .map(|c| Point::Int(c.to_float().rotate(angle, pole).round()))
            .collect();
        PolygonShape::new(&new_corners)
    }

    /// The shape mirrored at the vertical line through `pole`.
    pub fn mirror_vertical(&self, pole: IntPoint) -> PolygonShape {
        let pole = Point::Int(pole);
        let new_corners: Vec<Point> = self
            .corners()
            .iter()
            .map(|c| c.mirror_vertical(&pole))
            .collect();
        PolygonShape::new(&new_corners)
    }

    /// The shape mirrored at the horizontal line through `pole`.
    pub fn mirror_horizontal(&self, pole: IntPoint) -> PolygonShape {
        let pole = Point::Int(pole);
        let new_corners: Vec<Point> = self
            .corners()
            .iter()
            .map(|c| c.mirror_horizontal(&pole))
            .collect();
        PolygonShape::new(&new_corners)
    }

    /// Splits this polygon shape into convex pieces; `None` if the split fails (for
    /// example because of self intersections). Rounded intersection points are used, so
    /// the result is not exact.
    ///
    /// Like Java, a `java.util.Random` with seed 99 picks the start corners, so the
    /// result is reproducible.
    pub fn split_to_convex(&self) -> Option<Vec<TileShape>> {
        if let Some(pieces) = self.inner.convex_pieces.get() {
            return Some(pieces.clone());
        }
        let mut random = JavaRandom::new(SEED);
        let convex_pieces = self.split_to_convex_recu(&mut random)?;
        let result: Vec<TileShape> = convex_pieces
            .iter()
            .map(|p| TileShape::get_instance_from_points(p.corners()))
            .collect();
        Some(self.inner.convex_pieces.get_or_init(|| result).clone())
    }

    fn split_to_convex_recu(&self, random: &mut JavaRandom) -> Option<Vec<PolygonShape>> {
        let corners = self.corners();
        let n = corners.len();
        let mut start_corner_no = random.next_int(n as i32) as usize;
        let mut current_corner = &corners[start_corner_no];
        let mut prev_corner = if start_corner_no != 0 {
            &corners[start_corner_no - 1]
        } else {
            &corners[n - 1]
        };
        let mut concave_corner_no = None;
        for _ in 0..n {
            let next_corner = if start_corner_no < n - 1 {
                &corners[start_corner_no + 1]
            } else {
                &corners[0]
            };
            if next_corner.side_of(prev_corner, current_corner) == Side::OnTheRight {
                concave_corner_no = Some(start_corner_no);
                break;
            }
            prev_corner = current_corner;
            current_corner = next_corner;
            start_corner_no = (start_corner_no + 1) % n;
        }
        let Some(concave_corner_no) = concave_corner_no else {
            return Some(vec![self.clone()]);
        };
        let (corner_no_after_projection, projection) = self.division_point(concave_corner_no)?;

        let mut corner_count = corner_no_after_projection as isize - concave_corner_no as isize;
        if corner_count < 0 {
            corner_count += n as isize;
        }
        corner_count += 1;
        let mut first_arr = Vec::with_capacity(corner_count as usize);
        let mut corner_ind = concave_corner_no;
        for _ in 0..corner_count - 1 {
            first_arr.push(corners[corner_ind].clone());
            corner_ind = (corner_ind + 1) % n;
        }
        first_arr.push(Point::Int(projection.round()));
        let mut corner_count = concave_corner_no as isize - corner_no_after_projection as isize;
        if corner_count < 0 {
            corner_count += n as isize;
        }
        corner_count += 2;
        let mut last_arr = Vec::with_capacity(corner_count as usize);
        last_arr.push(Point::Int(projection.round()));
        let mut corner_ind = corner_no_after_projection;
        for _ in 1..corner_count {
            last_arr.push(corners[corner_ind].clone());
            corner_ind = (corner_ind + 1) % n;
        }
        let last_piece = PolygonShape::new(&last_arr);
        let first_piece = PolygonShape::new(&first_arr);
        let mut result = first_piece.split_to_convex_recu(random)?;
        let c2 = last_piece.split_to_convex_recu(random)?;
        result.extend(c2);
        Some(result)
    }

    /// At the concave corner `concave_corner_no`, a minimal axis parallel division line
    /// dividing the polygon into two. Returns the number of the corner after the
    /// projection and the projection point, or `None` if no projection was found.
    fn division_point(&self, concave_corner_no: usize) -> Option<(usize, FloatPoint)> {
        let corners = self.corners();
        let n = corners.len();
        let concave_corner = corners[concave_corner_no].to_float();
        let before_concave_corner = if concave_corner_no != 0 {
            corners[concave_corner_no - 1].to_float()
        } else {
            corners[n - 1].to_float()
        };
        let after_concave_corner = if concave_corner_no == n - 1 {
            corners[0].to_float()
        } else {
            corners[concave_corner_no + 1].to_float()
        };
        let search_right =
            before_concave_corner.y > concave_corner.y || concave_corner.y > after_concave_corner.y;
        let search_left =
            before_concave_corner.y < concave_corner.y || concave_corner.y < after_concave_corner.y;
        let search_up =
            before_concave_corner.x < concave_corner.x || concave_corner.x < after_concave_corner.x;
        let search_down =
            before_concave_corner.x > concave_corner.x || concave_corner.x > after_concave_corner.x;

        let mut min_projection_dist = f64::from(i32::MAX);
        let mut min_projection = None;
        let mut corner_no_after_min_projection = 0;
        let mut corner_no_after_curr_projection = (concave_corner_no + 2) % n;
        let mut corner_before_curr_projection = if corner_no_after_curr_projection != 0 {
            &corners[corner_no_after_curr_projection - 1]
        } else {
            &corners[n - 1]
        };
        let mut corner_before_projection_approx = corner_before_curr_projection.to_float();
        let loop_end = n.saturating_sub(2);
        for _ in 0..loop_end {
            let corner_after_curr_projection = &corners[corner_no_after_curr_projection];
            let corner_after_projection_approx = corner_after_curr_projection.to_float();
            if corner_before_projection_approx.y != corner_after_projection_approx.y {
                // Try a horizontal division.
                let (min_y, max_y) =
                    if corner_after_projection_approx.y > corner_before_projection_approx.y {
                        (
                            corner_before_projection_approx.y,
                            corner_after_projection_approx.y,
                        )
                    } else {
                        (
                            corner_after_projection_approx.y,
                            corner_before_projection_approx.y,
                        )
                    };
                if concave_corner.y >= min_y && concave_corner.y <= max_y {
                    let current_line = Line::from_points(
                        corner_before_curr_projection,
                        corner_after_curr_projection,
                    );
                    let x_intersection = current_line.function_in_y_value_approx(concave_corner.y);
                    let current_distance = (x_intersection - concave_corner.x).abs();
                    let projection_ok = current_distance < min_projection_dist
                        && ((search_right
                            && x_intersection > concave_corner.x
                            && concave_corner.y <= corner_after_projection_approx.y)
                            || (search_left
                                && x_intersection < concave_corner.x
                                && concave_corner.y >= corner_after_projection_approx.y));
                    if projection_ok {
                        min_projection_dist = current_distance;
                        corner_no_after_min_projection = corner_no_after_curr_projection;
                        min_projection = Some(FloatPoint::new(x_intersection, concave_corner.y));
                    }
                }
            }
            if corner_before_projection_approx.x != corner_after_projection_approx.x {
                // Try a vertical division.
                let (min_x, max_x) =
                    if corner_after_projection_approx.x > corner_before_projection_approx.x {
                        (
                            corner_before_projection_approx.x,
                            corner_after_projection_approx.x,
                        )
                    } else {
                        (
                            corner_after_projection_approx.x,
                            corner_before_projection_approx.x,
                        )
                    };
                if concave_corner.x >= min_x && concave_corner.x <= max_x {
                    let current_line = Line::from_points(
                        corner_before_curr_projection,
                        corner_after_curr_projection,
                    );
                    let y_intersection = current_line.function_value_approx(concave_corner.x);
                    let current_distance = (y_intersection - concave_corner.y).abs();
                    let projection_ok = current_distance < min_projection_dist
                        && ((search_up
                            && y_intersection > concave_corner.y
                            && concave_corner.x >= corner_after_projection_approx.x)
                            || (search_down
                                && y_intersection < concave_corner.y
                                && concave_corner.x <= corner_after_projection_approx.x));
                    if projection_ok {
                        min_projection_dist = current_distance;
                        corner_no_after_min_projection = corner_no_after_curr_projection;
                        min_projection = Some(FloatPoint::new(concave_corner.x, y_intersection));
                    }
                }
            }
            corner_before_curr_projection = corner_after_curr_projection;
            corner_before_projection_approx = corner_after_projection_approx;
            if corner_no_after_curr_projection == n - 1 {
                corner_no_after_curr_projection = 0;
            } else {
                corner_no_after_curr_projection += 1;
            }
        }
        min_projection.map(|p| (corner_no_after_min_projection, p))
    }
}

impl PolylineShapeOps for PolygonShape {
    fn border_line_count(&self) -> usize {
        self.corners().len()
    }

    /// # Panics
    /// If `no` is out of range (Java returns `null`).
    fn corner(&self, no: usize) -> Point {
        self.corners()[no].clone()
    }

    /// # Panics
    /// If `no` is out of range (Java returns `null`) or a corner is rational.
    fn border_line(&self, no: usize) -> Line {
        let corners = self.corners();
        let next_corner = if no == corners.len() - 1 {
            &corners[0]
        } else {
            &corners[no + 1]
        };
        Line::from_points(&corners[no], next_corner)
    }

    fn corner_is_bounded(&self, _no: usize) -> bool {
        true
    }

    fn is_empty(&self) -> bool {
        self.corners().is_empty()
    }

    fn is_bounded(&self) -> bool {
        true
    }

    fn dimension(&self) -> i32 {
        match self.corners().len() {
            0 => -1,
            1 => 0,
            2 => 1,
            _ => 2,
        }
    }

    fn bounding_box(&self) -> IntBox {
        *self.inner.bounding_box.get_or_init(|| {
            let mut llx = f64::from(i32::MAX);
            let mut lly = f64::from(i32::MAX);
            let mut urx = f64::from(i32::MIN);
            let mut ury = f64::from(i32::MIN);
            for corner in self.corners() {
                let current = corner.to_float();
                llx = java_min(llx, current.x);
                lly = java_min(lly, current.y);
                urx = java_max(urx, current.x);
                ury = java_max(ury, current.y);
            }
            IntBox::from_points(
                IntPoint::new(d2i(llx.floor()), d2i(lly.floor())),
                IntPoint::new(d2i(urx.ceil()), d2i(ury.ceil())),
            )
        })
    }

    fn bounding_octagon(&self) -> Option<IntOctagon> {
        Some(self.polygon_bounding_octagon())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pts(coords: &[(i32, i32)]) -> Vec<Point> {
        coords
            .iter()
            .map(|&(x, y)| Point::Int(IntPoint::new(x, y)))
            .collect()
    }

    #[test]
    fn normalizes_orientation_and_start_corner() {
        // Clockwise input starting at the top right.
        let shape = PolygonShape::new(&pts(&[(10, 10), (10, 0), (0, 0), (0, 10)]));
        assert_eq!(
            shape.corners(),
            pts(&[(0, 0), (10, 0), (10, 10), (0, 10)]).as_slice()
        );
        assert!(shape.is_convex());
        assert_eq!(shape.dimension(), 2);
        assert_eq!(shape.bounding_box(), IntBox::new(0, 0, 10, 10));
        assert_eq!(shape.area(), 100.0);
    }

    #[test]
    fn l_shape_is_split_into_convex_pieces() {
        let shape = PolygonShape::new(&pts(&[
            (0, 0),
            (20, 0),
            (20, 10),
            (10, 10),
            (10, 20),
            (0, 20),
        ]));
        assert!(!shape.is_convex());
        let pieces = shape.split_to_convex().unwrap();
        assert_eq!(pieces.len(), 2);
        let total: f64 = pieces.iter().map(|p| p.area()).sum();
        assert_eq!(total, 300.0);
        assert_eq!(shape.area(), total);
        assert!(shape.contains(&Point::Int(IntPoint::new(5, 15))));
        assert!(shape.contains(&Point::Int(IntPoint::new(15, 5))));
        assert!(!shape.contains(&Point::Int(IntPoint::new(15, 15))));
        assert!(shape.contains_float(FloatPoint::new(5.0, 5.0)));
        let hull = shape.convex_hull();
        assert_eq!(hull.corners().len(), 5);
        assert!(hull.is_convex());
        assert_eq!(shape.bounding_tile().area(), 350.0);
        assert!(shape.intersects_box(&IntBox::new(12, 2, 13, 3)));
        assert!(!shape.intersects_box(&IntBox::new(12, 12, 13, 13)));
        let other = PolygonShape::new(&pts(&[(15, 15), (30, 15), (30, 30)]));
        assert!(!shape.intersects_polygon(&other));
    }
}
