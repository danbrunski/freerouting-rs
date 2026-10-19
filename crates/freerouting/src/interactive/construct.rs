//! Keepout construction and holes (I03.4): Java's
//! `CornerItemConstructionState`, `TileConstructionState`,
//! `PolygonShapeConstructionState`, `CircleConstructionState` and
//! `HoleConstructionState` (`gui/interactive/*.java` at `a773a70ec`).
//!
//! A [`Construction`] collects the clicked corners in board units. Orders
//! kept from Java:
//! - a corner is rounded (`FloatPoint.round`), then snapped to the angle
//!   restriction from the last corner (`snap`: orthogonal projection in 90°
//!   mode, 45° projection in 45° mode);
//! - a tile drops its concave corners after every click
//!   (`removeConcaveCorners`) and at the close
//!   (`removeConcaveCornersAtClose`, which first adds a corner so the
//!   closing line fits the angle restriction, `addCornerForSnapAngle`);
//! - a polygon adds that corner at the close and must split into convex
//!   pieces;
//! - a circle is its bounding box in 90° mode and its bounding octagon in
//!   45° mode; its radius is the distance from the (unrounded) centre to
//!   the pointer, rounded with `Math.round`, and must be positive as a
//!   double (the pointer is the completing click or the last move; Java
//!   reads it at the last repaint, DIVERGENCES "Interactive editor");
//! - every keepout is checked with `check_shape` (no nets, clearance class
//!   "none") on the current layer; only a fitting one takes a snapshot and
//!   is inserted unfixed;
//! - a hole is added to the one keepout, via keepout or conduction area
//!   under the start point on the current layer (not a circle); later
//!   corners count only inside that area. The area is removed and a plain
//!   keepout with the old border and holes plus the new one is inserted on
//!   its layer with its clearance class. Like Java this makes a conduction
//!   area or a via keepout a keepout, and leaves the old item on the board
//!   when its deletion is forbidden. A border or hole that is not a polyline
//!   shape (Java's `ClassCastException`) fails with `adding_hole_failed`.

use freerouting_geometry::{
    Area, Circle, FloatPoint, IntPoint, Line, Point, PolygonShape, PolylineArea, PolylineShape,
    Shape, Side, TileShape,
};

use super::Status;
use super::settings::EditSettings;
use crate::board::items::ItemId;
use crate::board::structure::{AngleRestriction, FixedState};
use crate::board::{ItemFilter, RoutingBoard, SelectableChoice};
use crate::rules::BoardRules;

/// The keepout shapes a construction can make.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeepoutKind {
    Tile,
    Polygon,
    Circle,
}

/// A keepout as the pointer input that builds it: the clicked corners of a
/// tile or polygon (the first one starts it), or a circle's centre and
/// radius. [`insert_keepout`] replays the construction.
#[derive(Clone, Debug, PartialEq)]
pub enum KeepoutShape {
    Tile(Vec<FloatPoint>),
    Polygon(Vec<FloatPoint>),
    Circle { center: FloatPoint, radius: f64 },
}

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    Keepout(KeepoutKind),
    Hole { area: ItemId },
}

/// A keepout or hole under construction.
#[derive(Clone, Debug, PartialEq)]
pub struct Construction {
    kind: Kind,
    corners: Vec<IntPoint>,
    circle_center: FloatPoint,
    circle_radius: f64,
}

/// What a completed construction did.
#[derive(Clone, Debug, PartialEq)]
pub struct ConstructOutcome {
    /// The inserted keepout (after one snapshot).
    pub inserted: Option<ItemId>,
    /// Java's `keepout_successful_completed` /
    /// `keepout_cancelled_because_of_overlaps`, or `adding_hole_completed` /
    /// `adding_hole_failed`.
    pub status: Status,
}

/// The result of a click during a construction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstructClick {
    /// A corner was added or ignored; the construction goes on.
    Continue,
    /// A circle is finished by its click: call [`Construction::complete`].
    Complete,
}

fn restriction(board: &RoutingBoard) -> AngleRestriction {
    board.rules.trace_angle_restriction()
}

fn side_of(point: IntPoint, p1: IntPoint, p2: IntPoint) -> Side {
    Point::from(point).side_of(&Point::from(p1), &Point::from(p2))
}

fn keepout_status(ok: bool) -> Status {
    let key = if ok {
        "keepout_successful_completed"
    } else {
        "keepout_cancelled_because_of_overlaps"
    };
    Status::new(key, [])
}

impl Construction {
    /// Starts a keepout at `at` (Java `startTile`, `startPolygon`,
    /// `startCircle`).
    pub fn keepout(board: &RoutingBoard, kind: KeepoutKind, at: FloatPoint) -> Construction {
        let mut construction = Construction {
            kind: Kind::Keepout(kind),
            corners: Vec::new(),
            circle_center: at,
            circle_radius: 0.0,
        };
        if kind != KeepoutKind::Circle {
            construction.add_corner(board, at);
        }
        construction
    }

    /// Java `HoleConstructionState.startOk`: the one keepout, via keepout or
    /// conduction area at `at` on the current layer. `Err` carries Java's
    /// status.
    pub fn hole(
        board: &RoutingBoard,
        settings: &EditSettings,
        at: FloatPoint,
    ) -> Result<Construction, Status> {
        let filter = ItemFilter::of(&[
            SelectableChoice::Keepout,
            SelectableChoice::ViaKeepout,
            SelectableChoice::Conduction,
        ]);
        let found =
            board.pick_items_filtered(&Point::Int(at.round()), settings.layer, Some(&filter));
        if found.len() != 1 {
            return Err(Status::new("no_item_found_for_adding_hole", []));
        }
        let area = *found.iter().next().expect("one item");
        Construction::hole_in(board, area, at)
    }

    /// A hole in area item `area` started at `at`, without the pick (for
    /// scripts and probes).
    pub fn hole_in(
        board: &RoutingBoard,
        area: ItemId,
        at: FloatPoint,
    ) -> Result<Construction, Status> {
        let Some(obstacle) = board.item(area).and_then(|i| i.as_area()) else {
            return Err(Status::new("no_obstacle_area_found_for_adding_hole", []));
        };
        if matches!(obstacle.area(), Area::Shape(Shape::Circle(_))) {
            return Err(Status::new("adding_hole_to_circle_not_yet_implemented", []));
        }
        let mut construction = Construction {
            kind: Kind::Hole { area },
            corners: Vec::new(),
            circle_center: at,
            circle_radius: 0.0,
        };
        construction.add_corner(board, at);
        Ok(construction)
    }

    /// The keepout kind, `None` for a hole.
    pub fn keepout_kind(&self) -> Option<KeepoutKind> {
        match self.kind {
            Kind::Keepout(kind) => Some(kind),
            Kind::Hole { .. } => None,
        }
    }

    /// The area a hole is added to.
    pub fn hole_area(&self) -> Option<ItemId> {
        match self.kind {
            Kind::Hole { area } => Some(area),
            Kind::Keepout(_) => None,
        }
    }

    /// The corners so far (empty for a circle).
    pub fn corners(&self) -> &[IntPoint] {
        &self.corners
    }

    /// A circle's centre and current radius.
    pub fn circle(&self) -> (FloatPoint, f64) {
        (self.circle_center, self.circle_radius)
    }

    /// Java `CornerItemConstructionState.snap`.
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

    /// The pointer moved: a circle's radius follows it (Java computes it in
    /// `draw`); returns the snapped pointer position for the rubber band.
    pub fn pointer_moved(&mut self, board: &RoutingBoard, at: FloatPoint) -> IntPoint {
        if self.keepout_kind() == Some(KeepoutKind::Circle) {
            self.circle_radius = self.circle_center.distance(at);
        }
        self.snap(board, at.round())
    }

    /// Java `leftButtonClicked` of the construction states.
    pub fn click(&mut self, board: &RoutingBoard, at: FloatPoint) -> ConstructClick {
        match self.kind {
            Kind::Keepout(KeepoutKind::Circle) => {
                self.circle_radius = self.circle_center.distance(at);
                return ConstructClick::Complete;
            }
            Kind::Keepout(KeepoutKind::Tile) => {
                self.add_corner(board, at);
                self.remove_concave_corners(board);
            }
            Kind::Keepout(KeepoutKind::Polygon) => self.add_corner(board, at),
            Kind::Hole { area } => {
                let inside = board
                    .item(area)
                    .and_then(|i| i.as_area())
                    .is_some_and(|a| a.area().contains_float(at));
                if inside {
                    self.add_corner(board, at);
                }
            }
        }
        ConstructClick::Continue
    }

    /// Java `addCorner`.
    fn add_corner(&mut self, board: &RoutingBoard, at: FloatPoint) {
        let snapped = self.snap(board, at.round());
        self.corners.push(snapped);
    }

    /// Java `addCornerForSnapAngle`.
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

    /// Java `TileConstructionState.removeConcaveCorners`.
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

    /// Java `TileConstructionState.removeConcaveCornersAtClose`.
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

    /// Java `complete()` of the construction state: inserts the keepout on
    /// `layer` (the current layer), or adds the hole.
    pub fn complete(mut self, board: &mut RoutingBoard, layer: i32) -> ConstructOutcome {
        match self.kind.clone() {
            Kind::Hole { area } => self.complete_hole(board, area),
            Kind::Keepout(kind) => {
                let shape = self.keepout_area(board, kind);
                let class = BoardRules::clearance_class_none();
                let fits = shape
                    .as_ref()
                    .is_some_and(|area| board.check_shape(area, layer, &[], class));
                let inserted = match shape {
                    Some(area) if fits => {
                        board.generate_snapshot();
                        Some(board.insert_obstacle(area, layer, class, FixedState::Unfixed))
                    }
                    _ => None,
                };
                ConstructOutcome {
                    inserted,
                    status: keepout_status(inserted.is_some()),
                }
            }
        }
    }

    /// The keepout's area at completion, `None` when Java gives up before
    /// the shape check (too few corners, a polygon without a convex split, a
    /// radius of 0).
    fn keepout_area(&mut self, board: &RoutingBoard, kind: KeepoutKind) -> Option<Area> {
        match kind {
            KeepoutKind::Circle => {
                if self.circle_radius <= 0.0 {
                    return None;
                }
                let center = self.circle_center.round();
                let radius = freerouting_geometry::jmath::round_to_i32(self.circle_radius);
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
                if self.corners.len() <= 2 {
                    return None;
                }
                let polygon = PolygonShape::new(&self.points());
                polygon.split_to_convex()?;
                Some(Area::from(Shape::from(polygon)))
            }
            KeepoutKind::Tile => {
                self.remove_concave_corners_at_close(board);
                let count = self.corners.len();
                if count <= 2 {
                    return None;
                }
                let lines: Vec<Line> = (0..count)
                    .map(|i| Line::new(self.corners[i], self.corners[(i + 1) % count]))
                    .collect();
                let tile = TileShape::get_instance_from_lines(&lines);
                Some(Area::from(Shape::from(tile)))
            }
        }
    }

    fn points(&self) -> Vec<Point> {
        self.corners.iter().map(|c| Point::from(*c)).collect()
    }

    /// Java `HoleConstructionState.complete`.
    fn complete_hole(mut self, board: &mut RoutingBoard, area_id: ItemId) -> ConstructOutcome {
        let failed = ConstructOutcome {
            inserted: None,
            status: Status::new("adding_hole_failed", []),
        };
        let Some(item) = board.item(area_id) else {
            return failed;
        };
        let Some(obstacle) = item.as_area() else {
            return failed;
        };
        let (layer, class) = (obstacle.layer(), item.clearance_class());
        let area = obstacle.area().clone();
        self.add_corner_for_snap_angle(board);
        if self.corners.len() <= 2 {
            return failed;
        }
        // Java casts the border and every hole to `PolylineShape`.
        let Shape::Polyline(border) = area.get_border() else {
            return failed;
        };
        let mut holes: Vec<PolylineShape> = Vec::new();
        for hole in area.get_holes() {
            let Shape::Polyline(hole) = hole else {
                return failed;
            };
            holes.push(hole);
        }
        holes.push(PolylineShape::from(PolygonShape::new(&self.points())));
        let new_area = PolylineArea::new(border, holes);
        // PARITY(quirk, java_quirks.octagon_box_cutout_branch3): the board's split
        // switches (all four), whose cached pieces the insertion below reuses; Java's
        // split when they are `true`.
        if new_area
            .split_to_convex_with(None, board.java_quirks().split())
            .is_none()
        {
            return failed;
        }
        board.generate_snapshot();
        board.remove_item(area_id);
        let inserted =
            board.insert_obstacle(Area::from(new_area), layer, class, FixedState::Unfixed);
        ConstructOutcome {
            inserted: Some(inserted),
            status: Status::new("adding_hole_completed", []),
        }
    }
}

/// Builds `shape` as the pointer would (start, clicks, complete) and inserts
/// it on `layer` (I03.4's `Command::InsertKeepout`).
pub fn insert_keepout(
    board: &mut RoutingBoard,
    shape: &KeepoutShape,
    layer: i32,
) -> ConstructOutcome {
    let construction = match shape {
        KeepoutShape::Circle { center, radius } => {
            let mut c = Construction::keepout(board, KeepoutKind::Circle, *center);
            c.circle_radius = *radius;
            c
        }
        KeepoutShape::Tile(clicks) | KeepoutShape::Polygon(clicks) => {
            let kind = if matches!(shape, KeepoutShape::Tile(_)) {
                KeepoutKind::Tile
            } else {
                KeepoutKind::Polygon
            };
            let Some((&first, rest)) = clicks.split_first() else {
                return ConstructOutcome {
                    inserted: None,
                    status: keepout_status(false),
                };
            };
            let mut c = Construction::keepout(board, kind, first);
            for &at in rest {
                c.click(board, at);
            }
            c
        }
    };
    construction.complete(board, layer)
}

/// Adds the hole clicked as `clicks` (the first one starts it) to area
/// `area` (I03.4's `Command::AddHole`). `Err` is a start refusal.
pub fn add_hole(
    board: &mut RoutingBoard,
    area: ItemId,
    clicks: &[FloatPoint],
) -> Result<ConstructOutcome, Status> {
    let Some((&first, rest)) = clicks.split_first() else {
        return Err(Status::new("no_item_found_for_adding_hole", []));
    };
    let mut construction = Construction::hole_in(board, area, first)?;
    for &at in rest {
        construction.click(board, at);
    }
    let layer = board.item(area).map_or(0, |i| i.first_layer());
    Ok(construction.complete(board, layer))
}
