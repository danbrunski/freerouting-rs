//! Keepout construction (G05.7): Java's `TileConstructionState`,
//! `CircleConstructionState`, `PolygonShapeConstructionState` and
//! `CornerItemConstructionState`.
//!
//! The tool collects the clicked corners in board units. Snapping a corner
//! to the angle restriction, the shape check and the insertion are the
//! steps Java's `complete()` does; they are written here from the engine's
//! primitives (`check_shape`, `generate_snapshot`, `insert_obstacle`)
//! because the editor command that owns them (I03.4,
//! `Command::InsertKeepout`) has not landed. When it does, [`complete`] is
//! the one function to replace by that call; the tool, the rubber band and
//! the tests stay.
//!
//! Order, as in Java: a corner is rounded (`FloatPoint.round`) before it is
//! snapped to the angle of the last corner. A hole (`HoleConstructionState`)
//! needs the editor's remove-and-reinsert command and is not offered yet.

use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::board::structure::{AngleRestriction, FixedState};
use freerouting::geometry::{
    Area, Circle, FloatPoint, IntPoint, Line, Point, PolygonShape, Shape, Side, TileShape,
};

use crate::commands::KeepoutKind;

/// A keepout under construction.
#[derive(Clone, Debug, PartialEq)]
pub struct KeepoutTool {
    pub kind: KeepoutKind,
    corners: Vec<IntPoint>,
}

/// What [`complete`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The keepout was inserted (after a snapshot).
    Inserted(ItemId),
    /// Java's `keepout_cancelled_because_of_overlaps`: too few corners, an
    /// invalid shape (self-intersections), a zero radius, or an overlap.
    Refused,
}

impl KeepoutTool {
    /// Starts at `at` (the right-click location). `kind` is a polygon-like
    /// shape (tile or polygon) or a circle; a hole is not offered.
    pub fn start(kind: KeepoutKind, board: &RoutingBoard, at: FloatPoint) -> KeepoutTool {
        let mut tool = KeepoutTool {
            kind,
            corners: Vec::new(),
        };
        if kind == KeepoutKind::Circle {
            tool.corners.push(at.round());
        } else {
            tool.add_corner(board, at);
        }
        tool
    }

    /// The corners so far (for a circle, its centre).
    pub fn corners(&self) -> &[IntPoint] {
        &self.corners
    }

    /// Java `CornerItemConstructionState.snap`: the line from the last
    /// corner to `point`, snapped to the board's angle restriction.
    pub fn snap(&self, board: &RoutingBoard, point: IntPoint) -> IntPoint {
        let Some(last) = self.corners.last() else {
            return point;
        };
        match restriction(board) {
            AngleRestriction::NinetyDegree => point.orthogonal_projection(*last),
            AngleRestriction::FortyfiveDegree => point.fortyfive_degree_projection(*last),
            AngleRestriction::None => point,
        }
    }

    /// Java `addCorner`: rounds `at`, snaps it and adds it. A tile then
    /// drops the concave corners at its end (`TileConstructionState`).
    pub fn add_corner(&mut self, board: &RoutingBoard, at: FloatPoint) {
        let snapped = self.snap(board, at.round());
        self.corners.push(snapped);
        if self.kind == KeepoutKind::Tile {
            self.remove_concave_corners(board);
        }
    }

    /// Java `addCornerForSnapAngle`: a corner so the closing line fits the
    /// angle restriction.
    fn add_corner_for_snap_angle(&mut self, board: &RoutingBoard) {
        let (Some(first), Some(last)) = (self.corners.first(), self.corners.last()) else {
            return;
        };
        let added = match restriction(board) {
            AngleRestriction::None => None,
            AngleRestriction::NinetyDegree => last.ninety_degree_corner(*first, true),
            AngleRestriction::FortyfiveDegree => last.fortyfive_degree_corner(*first, true),
        };
        if let Some(corner) = added {
            self.corners.push(corner);
        }
    }

    /// Java `TileConstructionState.removeConcaveCorners`: skips concave
    /// corners at the end of the list.
    fn remove_concave_corners(&mut self, board: &RoutingBoard) {
        let mut corners = self.corners.clone();
        let mut new_length = corners.len();
        if new_length < 3 {
            return;
        }
        let mode = restriction(board);
        let last_corner = corners[new_length - 1];
        let mut current_corner = corners[new_length - 2];
        while new_length > 2 {
            let prev_corner = corners[new_length - 3];
            if side_of(last_corner, prev_corner, current_corner) == Side::OnTheLeft {
                break;
            }
            if mode != AngleRestriction::FortyfiveDegree {
                corners[new_length - 2] = last_corner;
            }
            new_length -= 1;
            if mode == AngleRestriction::NinetyDegree {
                // Keeps the lines orthogonal by moving the previous corner.
                let prev_prev = (new_length >= 3).then(|| corners[new_length - 3]);
                corners[new_length - 2] = match prev_prev {
                    Some(p) if p.x == prev_corner.x => IntPoint {
                        x: prev_corner.x,
                        y: last_corner.y,
                    },
                    _ => IntPoint {
                        x: last_corner.x,
                        y: prev_corner.y,
                    },
                };
            }
            current_corner = prev_corner;
        }
        if new_length < corners.len() {
            corners.truncate(new_length);
            self.corners = corners;
        }
    }

    /// Java `removeConcaveCornersAtClose`: drops corners from the end so
    /// that closing the polygon makes no concave corner.
    fn remove_concave_corners_at_close(&mut self, board: &RoutingBoard) {
        self.add_corner_for_snap_angle(board);
        if self.corners.len() < 4 {
            return;
        }
        let mut new_length = self.corners.len();
        let (first, second) = (self.corners[0], self.corners[1]);
        while new_length > 3 {
            let last = self.corners[new_length - 1];
            if side_of(last, second, first) != Side::OnTheLeft {
                break;
            }
            new_length -= 1;
        }
        if new_length != self.corners.len() {
            self.corners.truncate(new_length);
            self.add_corner_for_snap_angle(board);
        }
    }

    /// The shape at completion, or `None` when there is nothing to insert
    /// (too few corners, an invalid polygon, a circle of radius 0).
    /// `radius_point` is where the circle's radius ends (the click).
    pub fn shape(&mut self, board: &RoutingBoard, radius_point: Option<IntPoint>) -> Option<Area> {
        match self.kind {
            KeepoutKind::Circle => {
                let center = *self.corners.first()?;
                let edge = radius_point?;
                let radius = freerouting::geometry::jmath::round_to_i32(
                    center.to_float().distance(edge.to_float()),
                );
                if radius <= 0 {
                    return None;
                }
                let circle = Circle::new(center, radius);
                let shape = match restriction(board) {
                    AngleRestriction::NinetyDegree => Shape::from(circle.bounding_box()),
                    AngleRestriction::FortyfiveDegree => Shape::from(circle.bounding_octagon()),
                    AngleRestriction::None => Shape::from(circle),
                };
                Some(Area::from(shape))
            }
            KeepoutKind::Polygon => {
                self.add_corner_for_snap_angle(board);
                if self.corners.len() < 3 {
                    return None;
                }
                let points: Vec<Point> = self.corners.iter().map(|c| Point::from(*c)).collect();
                Some(Area::from(Shape::from(PolygonShape::new(&points))))
            }
            KeepoutKind::Tile => {
                self.remove_concave_corners_at_close(board);
                let count = self.corners.len();
                if count < 3 {
                    return None;
                }
                let lines: Vec<Line> = (0..count)
                    .map(|i| Line::new(self.corners[i], self.corners[(i + 1) % count]))
                    .collect();
                Some(Area::from(Shape::from(TileShape::get_instance_from_lines(
                    &lines,
                ))))
            }
            KeepoutKind::Hole => None,
        }
    }
}

fn restriction(board: &RoutingBoard) -> AngleRestriction {
    board.basic().rules.trace_angle_restriction()
}

fn side_of(point: IntPoint, p1: IntPoint, p2: IntPoint) -> Side {
    Point::from(point).side_of(&Point::from(p1), &Point::from(p2))
}

/// Completes the keepout on `layer`: checks the shape against the board
/// (clearance class "none", no nets), takes a snapshot and inserts it as an
/// unfixed keepout. Java's `complete()` of the three construction states.
/// See the module docs: I03.4 replaces this body.
pub fn complete(
    tool: &mut KeepoutTool,
    board: &mut RoutingBoard,
    layer: i32,
    radius_point: Option<IntPoint>,
) -> Outcome {
    let Some(area) = tool.shape(board, radius_point) else {
        return Outcome::Refused;
    };
    let class_none = freerouting::rules::BoardRules::clearance_class_none();
    // An area that cannot be split into convex pieces (self-intersections)
    // is refused by `check_shape` as well.
    if area.split_to_convex().is_none() || !board.check_shape(&area, layer, &[], class_none) {
        return Outcome::Refused;
    }
    board.generate_snapshot();
    Outcome::Inserted(board.insert_obstacle(area, layer, class_none, FixedState::Unfixed))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: i32, y: i32) -> IntPoint {
        IntPoint { x, y }
    }

    #[test]
    fn side_of_matches_the_left_turn_rule() {
        // Counterclockwise corners (0,0) (10,0) (10,10): the last is on the
        // left of the line from the first to the second.
        assert_eq!(side_of(p(10, 10), p(0, 0), p(10, 0)), Side::OnTheLeft);
        assert_eq!(side_of(p(10, -10), p(0, 0), p(10, 0)), Side::OnTheRight);
    }
}
