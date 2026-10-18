//! The any-angle corners of a located connection (A07.5).
//!
//! Port of `FoundConnectionLocatorAnyAngle.java` at `a773a70ec`:
//! `calcDoorLeftCorner`/`calcDoorRightCorner` (`:44-62`),
//! `calculateNextTraceCorners` (`:68-345`), `rightTurnNextCorner`,
//! `leftTurnNextCorner`, `rightLeftTangentialPoint` and
//! `leftRightTangentialPoint` (`:347-455`). From the current point, the
//! trace runs straight through as many doors as stay visible, bending
//! around the corner that closes the view at the tangent of a circle of the
//! trace half width; the bend is then checked against the last doors and
//! corrected.
//!
//! **Identity, not value.** Java returns `this.currentFromPoint` itself in
//! two places (a door already passed, and a tangent point that does not
//! exist), and keeps a corner only when it is not that object
//! (`resultCorner != this.currentFromPoint`, `:329`; `calculateNextTrace`,
//! `FoundConnectionLocator.java:432`). The port passes [`Corner::Unchanged`]
//! for it, never a value comparison (`// PARITY:`).

use freerouting_geometry::{FloatLine, FloatPoint, Point, PolylineShapeOps, Side, TileShapeOps};

use super::locate::{Corner, LocateError, Locator, half_width, room_shape};
use crate::autoroute::session::doors::TRACE_WIDTH_TOLERANCE;
use crate::autoroute::session::{ExpandableRef, Session};

/// `FoundConnectionLocatorAnyAngle.cTolerance`.
const TOLERANCE: f64 = 1.0;

const AT_CORNERS: &str = "FoundConnectionLocatorAnyAngle.calculateNextTraceCorners \
     (FoundConnectionLocatorAnyAngle.java:68-345)";
const AT_DOOR_CORNER: &str = "FoundConnectionLocatorAnyAngle.calcDoorLeftCorner \
     (FoundConnectionLocatorAnyAngle.java:44-62)";

/// A corner that may be Java's `null`, `this.currentFromPoint` itself, or a
/// new point.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Found {
    Null,
    From,
    New(FloatPoint),
}

/// `fromPoint.rightTangentialPoint(toPoint, distance)`, null-safe as Java's.
fn right_tangent(from: FloatPoint, to: Option<FloatPoint>, distance: f64) -> Option<FloatPoint> {
    from.right_tangential_point(to?, distance)
}

/// `fromPoint.leftTangentialPoint(toPoint, distance)`, null-safe as Java's.
fn left_tangent(from: FloatPoint, to: Option<FloatPoint>, distance: f64) -> Option<FloatPoint> {
    from.left_tangential_point(to?, distance)
}

/// The left (`left`) or right most corner of the door of `element`, seen
/// from the centre of gravity of the room it is entered from
/// (`calcDoorLeftCorner`, `calcDoorRightCorner`).
fn door_corner(
    s: &Session<'_>,
    door: ExpandableRef,
    next_room: Option<crate::autoroute::session::RoomId>,
    left: bool,
) -> Result<FloatPoint, LocateError> {
    let npe = LocateError::NullPointer { at: AT_DOOR_CORNER };
    // `door.otherRoom(nextRoom)`: only a door between two rooms has one.
    let from_room = match (door, next_room) {
        (ExpandableRef::Door(d), Some(next)) => s.other_room(d, next).ok_or(npe.clone())?,
        _ => return Err(npe),
    };
    let pole = room_shape(s, from_room, AT_DOOR_CORNER)?.centre_of_gravity();
    let shape = s.expandable_shape(door)?;
    let no = if left {
        shape.index_of_left_most_corner(pole)
    } else {
        shape.index_of_right_most_corner(pole)
    };
    Ok(shape.corner_approx(no))
}

impl Locator<'_, '_> {
    fn door_corners(&self, index: usize) -> Result<(FloatPoint, FloatPoint), LocateError> {
        let element = self
            .chain
            .get(index)
            .ok_or(LocateError::IndexOutOfBounds { at: AT_CORNERS })?;
        let left = door_corner(self.s, element.door, element.next_room, true)?;
        let right = door_corner(self.s, element.door, element.next_room, false)?;
        Ok((left, right))
    }

    /// `currentTargetShape.nearestPoint(currentFromPoint.round()).toFloat()`.
    fn nearest_target_point(&self) -> Result<FloatPoint, LocateError> {
        let from = Point::Int(self.current_from.round());
        self.target_shape
            .nearest_point(&from)
            .map(|p| p.to_float())
            .ok_or(LocateError::NullPointer { at: AT_CORNERS })
    }

    /// Java `calculateNextTraceCorners` (`:68-345`).
    pub(super) fn next_trace_corners_any(&mut self) -> Result<Vec<Corner>, LocateError> {
        if self.to_index >= self.target_index {
            if self.to_index == self.target_index {
                let nearest = self.nearest_target_point()?;
                self.to_index += 1;
                return Ok(vec![Corner::New(nearest)]);
            }
            return Ok(Vec::new());
        }
        let room = self
            .to_index
            .checked_sub(1)
            .and_then(|i| self.chain.get(i))
            .and_then(|e| e.next_room);
        let exact = f64::from(half_width(
            self.s,
            room,
            self.current_trace_layer,
            AT_CORNERS,
        )?);
        let max = exact + TRACE_WIDTH_TOLERANCE;
        let middle = exact + TOLERANCE;
        let from = self.current_from;
        let previous = self.previous_from;

        let (left, right) = self.door_corners(self.to_index)?;
        let mut door_left = Some(left);
        let mut door_right = Some(right);
        if from.side_of(left, right) != Side::OnTheRight {
            // The door is already crossed at the current point.
            if from.scalar_product(previous, left) >= 0.0 {
                door_left = None;
            }
            if from.scalar_product(previous, right) >= 0.0 {
                door_right = None;
            }
            if door_left.is_none() && door_right.is_none() {
                self.to_index += 1;
                // PARITY: `result.add(this.currentFromPoint)`, the same object.
                return Ok(vec![Corner::Unchanged]);
            }
        }

        // The longest straight line through the doors from the current point.
        let mut end_of_trace = false;
        let mut left_tangent_point;
        let mut right_tangent_point;
        let mut new_door = self.to_index;
        let mut left_ind = new_door;
        let mut right_ind = new_door;
        let mut current_door = self.to_index + 1;
        let mut result = Found::Null;
        loop {
            left_tangent_point = right_tangent(from, door_left, max);
            if door_left.is_some() && left_tangent_point.is_none() {
                left_tangent_point = door_left;
            }
            right_tangent_point = left_tangent(from, door_right, max);
            if door_right.is_some() && right_tangent_point.is_none() {
                right_tangent_point = door_right;
            }
            if let (Some(lt), Some(rt), Some(dl), Some(dr)) = (
                left_tangent_point,
                right_tangent_point,
                door_left,
                door_right,
            ) && rt.side_of(from, lt) != Side::OnTheRight
            {
                // The gap is too small for a trace of this half width.
                if dl.distance(from) <= dr.distance(from) {
                    new_door = left_ind;
                    result = left_turn_next_corner(from, max, dl, dr);
                } else {
                    new_door = right_ind;
                    result = right_turn_next_corner(from, max, dr, dl);
                }
                break;
            }
            if current_door >= self.target_index {
                end_of_trace = true;
                break;
            }
            let (next_left, next_right) = self.door_corners(current_door)?;
            let mut next_left = Some(next_left);
            let mut next_right = Some(next_right);
            if let (Some(nl), Some(nr)) = (next_left, next_right)
                && from.side_of(nl, nr) != Side::OnTheRight
            {
                // The door may be crossed already at the current point.
                if door_left.is_none() && from.scalar_product(previous, nl) >= 0.0 {
                    next_left = None;
                }
                if door_right.is_none() && from.scalar_product(previous, nr) >= 0.0 {
                    next_right = None;
                }
                if next_left.is_none() && next_right.is_none() {
                    // Should not happen: the previous door was not passed.
                    self.to_index += 1;
                    return Ok(vec![Corner::Unchanged]);
                }
            }
            if let (Some(dl), Some(dr)) = (door_left, door_right) {
                // Both corners are defined (the side tests are only valid
                // then); `next_left` and `next_right` are too.
                let nl = next_left.ok_or(LocateError::NullPointer { at: AT_CORNERS })?;
                let nr = next_right.ok_or(LocateError::NullPointer { at: AT_CORNERS })?;
                if nl.side_of(from, dr) == Side::OnTheRight {
                    // Bend to the right.
                    new_door = right_ind + 1;
                    result = right_turn_next_corner(from, max, dr, nl);
                    break;
                }
                if nr.side_of(from, dl) == Side::OnTheLeft {
                    // Bend to the left.
                    new_door = left_ind + 1;
                    result = left_turn_next_corner(from, max, dl, nr);
                    break;
                }
            }
            if narrows(from, door_right, next_right, max, Side::OnTheRight)? {
                door_right = next_right;
                right_ind = current_door;
            }
            if narrows(from, door_left, next_left, max, Side::OnTheLeft)? {
                door_left = next_left;
                left_ind = current_door;
            }
            current_door += 1;
        }

        if end_of_trace {
            let nearest = self.nearest_target_point()?;
            result = Found::New(nearest);
            if let Some(lt) = left_tangent_point
                && nearest.side_of(from, lt) == Side::OnTheLeft
            {
                // The nearest target point is left of the visible range.
                new_door = left_ind + 1;
                let no = self.target_shape.index_of_right_most_corner(from);
                let target_right = self.target_shape.corner_approx(no);
                let dl = door_left.ok_or(LocateError::NullPointer { at: AT_CORNERS })?;
                if let Some(corner) = right_left_tangential_point(from, target_right, dl, max) {
                    result = Found::New(corner);
                    end_of_trace = false;
                }
            } else if let Some(rt) = right_tangent_point
                && nearest.side_of(from, rt) == Side::OnTheRight
            {
                // The nearest target point is right of the visible range.
                let no = self.target_shape.index_of_left_most_corner(from);
                let target_left = self.target_shape.corner_approx(no);
                new_door = right_ind + 1;
                let dr = door_right.ok_or(LocateError::NullPointer { at: AT_CORNERS })?;
                if let Some(corner) = left_right_tangential_point(from, target_left, dr, max) {
                    result = Found::New(corner);
                    end_of_trace = false;
                }
            }
        }
        if end_of_trace {
            new_door = self.target_index;
        }

        // Check the clearance to the previous doors and correct the corner.
        let (corrected, corrected_door) =
            self.correct_corner(from, result, new_door, max, middle)?;
        if let Some(corrected) = corrected {
            result = Found::New(corrected);
            new_door = corrected_door.max(self.to_index);
        }
        self.to_index = new_door;
        // PARITY: `resultCorner != this.currentFromPoint`
        // (FoundConnectionLocatorAnyAngle.java:329) by reference.
        Ok(match result {
            Found::New(point) => vec![Corner::New(point)],
            Found::Null | Found::From => Vec::new(),
        })
    }

    /// The correction loop (`:296-326`): the corrected corner and its door
    /// index, if a door corner of the last doors is too close to the line
    /// from `from` to `result`.
    fn correct_corner(
        &self,
        from: FloatPoint,
        result: Found,
        new_door: usize,
        max: f64,
        middle: f64,
    ) -> Result<(Option<FloatPoint>, usize), LocateError> {
        // `Math.max(currentToDoorIndex - 5, currentFromDoorIndex + 1)` in
        // `int`: a negative difference loses to the second operand.
        let check_from = if self.to_index >= 5 {
            (self.to_index - 5).max(self.from_index + 1)
        } else {
            self.from_index + 1
        };
        let mut corrected: Option<FloatPoint> = None;
        let mut corrected_door = 0;
        if check_from >= new_door {
            return Ok((None, 0));
        }
        let end = match result {
            Found::New(point) => point,
            Found::From => from,
            // `new FloatLine(currentFromPoint, null).segmentDistance(...)`.
            Found::Null => return Err(LocateError::NullPointer { at: AT_CORNERS }),
        };
        let check_line = FloatLine::new(from, end);
        for i in check_from..new_door {
            let (left, right) = self.door_corners(i)?;
            let distance = check_line.segment_distance(left);
            if distance.abs() < middle
                && let Some(candidate) =
                    right_left_tangential_point(check_line.a, check_line.b, left, max)
                && corrected.is_none_or(|c| candidate.side_of(from, c) == Side::OnTheRight)
            {
                corrected_door = i;
                corrected = Some(candidate);
            }
            let distance = check_line.segment_distance(right);
            if distance.abs() < middle
                && let Some(candidate) =
                    left_right_tangential_point(check_line.a, check_line.b, right, max)
                && corrected.is_none_or(|c| candidate.side_of(from, c) == Side::OnTheLeft)
            {
                corrected_door = i;
                corrected = Some(candidate);
            }
        }
        Ok((corrected, corrected_door))
    }
}

/// Whether the visible range gets smaller on the right (`side`
/// `OnTheRight`) or left side through the next door's corner
/// (`:257-288`). A missing current corner always narrows.
fn narrows(
    from: FloatPoint,
    door: Option<FloatPoint>,
    next: Option<FloatPoint>,
    max: f64,
    side: Side,
) -> Result<bool, LocateError> {
    let Some(door) = door else {
        return Ok(true);
    };
    let next = next.ok_or(LocateError::NullPointer { at: AT_CORNERS })?;
    if next.side_of(from, door) == side {
        return Ok(false);
    }
    let tangent = if side == Side::OnTheRight {
        from.left_tangential_point(next, max)
    } else {
        from.right_tangential_point(next, max)
    };
    Ok(tangent.is_some_and(|t| FloatLine::new(from, t).segment_distance(door) >= max))
}

/// Java `rightTurnNextCorner` (`:353-371`): `from` itself where a tangent
/// point does not exist.
fn right_turn_next_corner(from: FloatPoint, dist: f64, to: FloatPoint, next: FloatPoint) -> Found {
    let Some(tangent) = from.left_tangential_point(to, dist) else {
        return Found::From;
    };
    let first = FloatLine::new(from, tangent);
    let Some(tangent) = to.right_tangential_point(next, 2.0 * dist + TOLERANCE) else {
        return Found::From;
    };
    let second = FloatLine::new(to, tangent).translate(dist);
    first.intersection(&second).map_or(Found::Null, Found::New)
}

/// Java `leftTurnNextCorner` (`:379-396`).
fn left_turn_next_corner(from: FloatPoint, dist: f64, to: FloatPoint, next: FloatPoint) -> Found {
    let Some(tangent) = from.right_tangential_point(to, dist) else {
        return Found::From;
    };
    let first = FloatLine::new(from, tangent);
    let Some(tangent) = to.left_tangential_point(next, 2.0 * dist + TOLERANCE) else {
        return Found::From;
    };
    let second = FloatLine::new(to, tangent).translate(-dist);
    first.intersection(&second).map_or(Found::Null, Found::New)
}

/// Java `rightLeftTangentialPoint` (`:402-422`).
fn right_left_tangential_point(
    from: FloatPoint,
    to: FloatPoint,
    center: FloatPoint,
    dist: f64,
) -> Option<FloatPoint> {
    let first = FloatLine::new(from, from.right_tangential_point(center, dist)?);
    let second = FloatLine::new(to, to.left_tangential_point(center, dist)?);
    first.intersection(&second)
}

/// Java `leftRightTangentialPoint` (`:428-448`).
fn left_right_tangential_point(
    from: FloatPoint,
    to: FloatPoint,
    center: FloatPoint,
    dist: f64,
) -> Option<FloatPoint> {
    let first = FloatLine::new(from, from.left_tangential_point(center, dist)?);
    let second = FloatLine::new(to, to.right_tangential_point(center, dist)?);
    first.intersection(&second)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A turn whose tangent point does not exist (the start inside the
    /// circle) returns the start point itself, Java's `return fromCorner`.
    #[test]
    fn a_missing_tangent_returns_the_start_itself() {
        let from = FloatPoint::new(0.0, 0.0);
        let to = FloatPoint::new(1.0, 0.0);
        let next = FloatPoint::new(50.0, 50.0);
        assert_eq!(right_turn_next_corner(from, 5.0, to, next), Found::From);
        assert_eq!(left_turn_next_corner(from, 5.0, to, next), Found::From);
    }

    /// A tangent around a corner from far away bends the trace beyond it.
    #[test]
    fn the_tangential_points_go_around_the_center() {
        let from = FloatPoint::new(0.0, 0.0);
        let to = FloatPoint::new(100.0, 0.0);
        let center = FloatPoint::new(50.0, 0.0);
        let p = right_left_tangential_point(from, to, center, 10.0).expect("intersection");
        let q = left_right_tangential_point(from, to, center, 10.0).expect("intersection");
        assert!((p.x - 50.0).abs() < 1e-9 && (q.x - 50.0).abs() < 1e-9);
        assert!(p.y * q.y < 0.0, "on opposite sides: {p:?} {q:?}");
        assert!(p.y.abs() > 10.0);
    }
}
