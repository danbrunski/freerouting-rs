//! The 45 degree corners of a located connection (A05.3).
//!
//! Port of `FoundConnectionLocator45Degree.java` at `a773a70ec`:
//! `roundToInteger` (`:40-42`), `calcHorizontalFirstFromDoor` (`:48-101`),
//! `calculateNextTraceCorners` (`:103-313`) and `calcHorizontalFirstToDoor`
//! (`:319-371`), with the base class's `ninetyDegreeCorner`,
//! `fortyfiveDegreeCorner` and `calculateAdditionalCorner`
//! (`FoundConnectionLocator.java:329-404`). The 90 degree restriction uses
//! this locator too, with orthogonal additional corners.
//!
//! The door sections come from `Session::section_segments` with the
//! compensated trace half width of the trace's layer; it reallocates a
//! door's sections when their count changes, as Java's
//! `ExpansionDoor.getSectionSegments` does (`// PARITY(quirk):`, A02.5).
//! The backtrack chain is read before, so the reallocation changes nothing
//! the locator reads.
//!
//! Every corner this locator returns is a new point object in Java, so
//! every one is [`Corner::New`].

use freerouting_geometry::{FloatPoint, PolylineShapeOps, Signum, TileShape, TileShapeOps};

use super::locate::{Corner, LocateError, Locator, half_width, room_shape, round_to_integer};
use crate::autoroute::session::doors::TRACE_WIDTH_TOLERANCE;
use crate::autoroute::session::{ExpandableRef, Room, Session};
use crate::board::structure::AngleRestriction;

const AT_CORNERS: &str = "FoundConnectionLocator45Degree.calculateNextTraceCorners \
     (FoundConnectionLocator45Degree.java:127-283)";
const AT_FROM_DOOR: &str = "FoundConnectionLocator45Degree.calcHorizontalFirstFromDoor \
     (FoundConnectionLocator45Degree.java:56-59)";
const AT_TO_DOOR: &str = "FoundConnectionLocator45Degree.calcHorizontalFirstToDoor \
     (FoundConnectionLocator45Degree.java:326-329)";
/// The warning branch "ExpansionDoor expected" with corners already found.
pub(super) const AT_NOT_A_DOOR: &str = "FoundConnectionLocator45Degree.calculateNextTraceCorners \
     (FoundConnectionLocator45Degree.java:229-232, \"ExpansionDoor expected\")";
/// The warning branch "lineSections inconsistent" with corners already
/// found.
const AT_SECTIONS: &str = "FoundConnectionLocator45Degree.calculateNextTraceCorners \
     (FoundConnectionLocator45Degree.java:245-249, \"lineSections inconsistent\")";

/// Java `FoundConnectionLocator.ninetyDegreeCorner` (`:329-341`).
fn ninety_degree_corner(from: FloatPoint, to: FloatPoint, horizontal_first: bool) -> FloatPoint {
    if horizontal_first {
        FloatPoint::new(to.x, from.y)
    } else {
        FloatPoint::new(from.x, to.y)
    }
}

/// Java `FoundConnectionLocator.fortyfiveDegreeCorner` (`:343-384`).
fn fortyfive_degree_corner(from: FloatPoint, to: FloatPoint, horizontal_first: bool) -> FloatPoint {
    let abs_dx = (to.x - from.x).abs();
    let abs_dy = (to.y - from.y).abs();
    let (x, y);
    if abs_dx <= abs_dy {
        if horizontal_first {
            x = to.x;
            y = if to.y >= from.y {
                from.y + abs_dx
            } else {
                from.y - abs_dx
            };
        } else {
            x = from.x;
            y = if to.y > from.y {
                to.y - abs_dx
            } else {
                to.y + abs_dx
            };
        }
    } else if horizontal_first {
        y = from.y;
        x = if to.x > from.x {
            to.x - abs_dy
        } else {
            to.x + abs_dy
        };
    } else {
        y = to.y;
        x = if to.x > from.x {
            from.x + abs_dy
        } else {
            from.x - abs_dy
        };
    }
    FloatPoint::new(x, y)
}

/// Java `FoundConnectionLocator.calculateAdditionalCorner` (`:390-404`): a
/// corner such that both lines from `from` over it to `to` keep the angle
/// restriction. Without a restriction Java returns `to` itself (the
/// any-angle locator's adjusted start corner).
pub(super) fn calculate_additional_corner(
    from: FloatPoint,
    to: FloatPoint,
    horizontal_first: bool,
    angle: AngleRestriction,
) -> FloatPoint {
    match angle {
        AngleRestriction::NinetyDegree => ninety_degree_corner(from, to, horizontal_first),
        AngleRestriction::FortyfiveDegree => fortyfive_degree_corner(from, to, horizontal_first),
        AngleRestriction::None => to,
    }
}

/// The left and right corners of a one-dimensional door's diagonal corner
/// segment, ordered by x, then y (`:56-67`, `:326-337`).
fn door_corners(
    s: &Session<'_>,
    door: ExpandableRef,
    at: &'static str,
) -> Result<(TileShape, FloatPoint, FloatPoint), LocateError> {
    let shape = s.expandable_shape(door)?;
    let segment = shape
        .diagonal_corner_segment()
        .ok_or(LocateError::NullPointer { at })?;
    let (a, b) = (segment.a, segment.b);
    let (left, right) = if a.x < b.x || a.x == b.x && a.y <= b.y {
        (a, b)
    } else {
        (b, a)
    };
    Ok((shape, left, right))
}

/// The lean of a one-dimensional door (`:68-98`, `:338-368`): about
/// vertical, about horizontal, or about diagonal (`right` for a right
/// diagonal, its left corner below its right one).
enum DoorLean {
    Vertical,
    Horizontal,
    Diagonal { right: bool },
}

fn door_lean(shape: &TileShape, left: FloatPoint, right: FloatPoint) -> DoorLean {
    let bounds = shape.bounding_box();
    let door_dx = right.x - left.x;
    let door_dy = right.y - left.y;
    let abs_door_dy = door_dy.abs();
    let door_max_width = freerouting_geometry::jmath::max_f64(door_dx, abs_door_dy);
    let door_half_max_width = 0.5 * door_max_width;
    if f64::from(bounds.width()) <= door_half_max_width {
        DoorLean::Vertical
    } else if f64::from(bounds.height()) <= door_half_max_width {
        DoorLean::Horizontal
    } else {
        DoorLean::Diagonal {
            right: left.y < right.y,
        }
    }
}

/// `Math.abs(dx) > Math.abs(dy)` with the signs of `dx` and `dy` compared
/// as `Signum.of` does: `(same_sign, |dx| > |dy|, |dx| < |dy|)`.
fn slope(from: FloatPoint, to: FloatPoint) -> (bool, bool, bool) {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let same_sign = Signum::of(dx) == Signum::of(dy);
    (same_sign, dx.abs() > dy.abs(), dx.abs() < dy.abs())
}

/// Java `calcHorizontalFirstFromDoor` (`:48-101`).
fn calc_horizontal_first_from_door(
    s: &Session<'_>,
    from_door: ExpandableRef,
    from: FloatPoint,
    to: FloatPoint,
) -> Result<bool, LocateError> {
    if s.expandable_dimension(from_door) != 1 {
        let bounds = s.expandable_shape(from_door)?.bounding_box();
        return Ok(bounds.height() >= bounds.width());
    }
    let (shape, left, right) = door_corners(s, from_door, AT_FROM_DOOR)?;
    Ok(match door_lean(&shape, left, right) {
        DoorLean::Vertical => true,
        DoorLean::Horizontal => false,
        DoorLean::Diagonal { right } => {
            let (same_sign, dx_greater, dx_smaller) = slope(from, to);
            if right == same_sign {
                dx_greater
            } else {
                dx_smaller
            }
        }
    })
}

/// Java `calcHorizontalFirstToDoor` (`:319-371`).
fn calc_horizontal_first_to_door(
    s: &Session<'_>,
    to_door: ExpandableRef,
    from: FloatPoint,
    to: FloatPoint,
) -> Result<bool, LocateError> {
    if s.expandable_dimension(to_door) != 1 {
        let bounds = s.expandable_shape(to_door)?.bounding_box();
        return Ok(bounds.height() <= bounds.width());
    }
    let (shape, left, right) = door_corners(s, to_door, AT_TO_DOOR)?;
    Ok(match door_lean(&shape, left, right) {
        DoorLean::Vertical => false,
        DoorLean::Horizontal => true,
        DoorLean::Diagonal { right } => {
            let (same_sign, dx_greater, dx_smaller) = slope(from, to);
            if right == same_sign {
                dx_smaller
            } else {
                dx_greater
            }
        }
    })
}

/// Java `nearestBorderPointsApprox(point, 2)[1].distance(point) >=
/// min_distance` where Java's array has a second entry; Java's array keeps
/// trailing `null`s where the port's list is shorter, and reading one
/// throws.
fn second_border_point_far_enough(
    shape: &TileShape,
    point: FloatPoint,
    min_distance: f64,
) -> Result<Option<bool>, LocateError> {
    let simplex = shape.to_simplex();
    let line_count = simplex.border_line_count();
    let java_len = if line_count <= 1 || simplex.dimension() == 0 {
        line_count.min(1)
    } else {
        line_count.min(2)
    };
    if java_len < 2 {
        return Ok(None);
    }
    let points = simplex.nearest_border_points_approx(point, 2);
    let second = points
        .get(1)
        .ok_or(LocateError::NullPointer { at: AT_CORNERS })?;
    Ok(Some(second.distance(point) >= min_distance))
}

/// Java `calculateNextTraceCorners` (`:103-313`): the next corners of the
/// trace under construction; empty when the trace is complete.
pub(super) fn calculate_next_trace_corners(
    l: &mut Locator<'_, '_>,
) -> Result<Vec<Corner>, LocateError> {
    let mut result = Vec::new();
    if l.to_index > l.target_index {
        return Ok(result);
    }
    let index_error = LocateError::IndexOutOfBounds { at: AT_CORNERS };
    let from_info = *l.chain.get(l.to_index - 1).ok_or(index_error.clone())?;
    let Some(from_room) = from_info.next_room else {
        // Java warns "nextRoom is null" and returns no corners.
        return Ok(result);
    };
    let room = room_shape(l.s, from_room, AT_CORNERS)?.clone();
    let trace_half_width = half_width(l.s, l.current_trace_layer, AT_CORNERS)?;
    // Some tolerance for free space expansion rooms.
    let trace_half_width_add = trace_half_width + TRACE_WIDTH_TOLERANCE as i32;
    let shrink_offset = if matches!(l.s.room(from_room), Room::Obstacle { .. }) {
        trace_half_width
    } else {
        trace_half_width_add
    };
    let mut shrinked = room.offset(-f64::from(shrink_offset));
    if !shrinked.is_empty() {
        // Enter the shrunk room shape by a 45 degree angle first.
        let nearest = shrinked
            .nearest_point_approx(l.current_from)
            .ok_or(LocateError::NullPointer { at: AT_CORNERS })?;
        let horizontal_first =
            calc_horizontal_first_from_door(l.s, from_info.door, l.current_from, nearest)?;
        let nearest = round_to_integer(nearest);
        result.push(Corner::New(calculate_additional_corner(
            l.current_from,
            nearest,
            horizontal_first,
            l.angle,
        )));
        result.push(Corner::New(nearest));
        l.current_from = nearest;
    } else {
        shrinked = room;
    }

    if l.to_index == l.target_index {
        let nearest = l
            .target_shape
            .nearest_point_approx(l.current_from)
            .ok_or(LocateError::NullPointer { at: AT_CORNERS })?;
        let nearest = round_to_integer(nearest);
        let mut add = calculate_additional_corner(l.current_from, nearest, true, l.angle);
        if !shrinked.contains_float(add) {
            add = calculate_additional_corner(l.current_from, nearest, false, l.angle);
        }
        result.push(Corner::New(add));
        result.push(Corner::New(nearest));
        l.to_index += 1;
        return Ok(result);
    }

    let to_info = *l.chain.get(l.to_index).ok_or(index_error)?;
    let ExpandableRef::Door(to_door) = to_info.door else {
        // Java warns "ExpansionDoor expected" and returns what it has.
        return stalled(result, AT_NOT_A_DOOR);
    };
    let nearest_to_door = if l.s.door(to_door).dimension == 2 {
        // Not in free angle routing, where corners are cut off.
        let shrinked_door = l.s.door_shape(to_door)?.shrink(f64::from(shrink_offset));
        shrinked_door
            .nearest_point_approx(l.current_from)
            .ok_or(LocateError::NullPointer { at: AT_CORNERS })?
    } else {
        // PARITY(quirk): getSectionSegments reallocates the door's sections
        // when their count changes (ExpansionDoor.java:193-201).
        let sections = l.s.section_segments(to_door, f64::from(trace_half_width))?;
        let Some(section) = sections.get(to_info.section).copied() else {
            // Java warns "lineSections inconsistent".
            return stalled(result, AT_SECTIONS);
        };
        let add = f64::from(trace_half_width_add);
        let section_len = section.b.distance(section.a);
        let mut point = if section_len <= 2.5 * add {
            section.a.middle_point(section.b)
        } else {
            section
                .shrink_segment(add)
                .nearest_segment_point(l.current_from)
        };
        let mut ok = true;
        if let Some(next_room) = to_info.next_room {
            // With an IntBox or IntOctagon this would not work: their
            // border lines may have length 0.
            let shape = room_shape(l.s, next_room, AT_CORNERS)?;
            if let Some(far) = second_border_point_far_enough(shape, point, add)? {
                ok = far;
            }
        }
        if ok {
            let shape = room_shape(l.s, from_room, AT_CORNERS)?;
            if let Some(far) = second_border_point_far_enough(shape, point, add)? {
                ok = far;
            }
        }
        if !ok {
            // The room may have an acute (45 degree) angle at a corner of
            // the door.
            point = section.a.middle_point(section.b);
        }
        point
    };
    let nearest_to_door = round_to_integer(nearest_to_door);
    let horizontal_first =
        calc_horizontal_first_to_door(l.s, to_info.door, l.current_from, nearest_to_door)?;
    result.push(Corner::New(calculate_additional_corner(
        l.current_from,
        nearest_to_door,
        horizontal_first,
        l.angle,
    )));
    result.push(Corner::New(nearest_to_door));
    l.to_index += 1;
    Ok(result)
}

/// A warning branch of `calculateNextTraceCorners` that returns without
/// advancing `currentToDoorIndex`: with no corners the trace ends; with
/// corners Java calls it again with the same state forever.
fn stalled(result: Vec<Corner>, at: &'static str) -> Result<Vec<Corner>, LocateError> {
    if result.is_empty() {
        Ok(result)
    } else {
        Err(LocateError::EndlessLoop { at })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> FloatPoint {
        FloatPoint::new(x, y)
    }

    #[test]
    fn ninety_degree_corners_go_horizontal_or_vertical_first() {
        let (from, to) = (p(0.0, 0.0), p(30.0, 10.0));
        let angle = AngleRestriction::NinetyDegree;
        assert_eq!(
            calculate_additional_corner(from, to, true, angle),
            p(30.0, 0.0)
        );
        assert_eq!(
            calculate_additional_corner(from, to, false, angle),
            p(0.0, 10.0)
        );
    }

    #[test]
    fn fortyfive_degree_corners_follow_javas_branches() {
        let angle = AngleRestriction::FortyfiveDegree;
        // |dx| <= |dy|.
        let (from, to) = (p(0.0, 0.0), p(10.0, 30.0));
        assert_eq!(
            calculate_additional_corner(from, to, true, angle),
            p(10.0, 10.0)
        );
        assert_eq!(
            calculate_additional_corner(from, to, false, angle),
            p(0.0, 20.0)
        );
        let to = p(10.0, -30.0);
        assert_eq!(
            calculate_additional_corner(from, to, true, angle),
            p(10.0, -10.0)
        );
        assert_eq!(
            calculate_additional_corner(from, to, false, angle),
            p(0.0, -20.0)
        );
        // |dx| > |dy|.
        let to = p(30.0, 10.0);
        assert_eq!(
            calculate_additional_corner(from, to, true, angle),
            p(20.0, 0.0)
        );
        assert_eq!(
            calculate_additional_corner(from, to, false, angle),
            p(10.0, 10.0)
        );
        let to = p(-30.0, 10.0);
        assert_eq!(
            calculate_additional_corner(from, to, true, angle),
            p(-20.0, 0.0)
        );
        assert_eq!(
            calculate_additional_corner(from, to, false, angle),
            p(-10.0, 10.0)
        );
        // Equal y: `to.y >= from.y` goes up by |dx|, `to.y > from.y` does not.
        let to = p(0.0, 0.0);
        assert_eq!(
            calculate_additional_corner(from, to, true, angle),
            p(0.0, 0.0)
        );
    }

    #[test]
    fn no_restriction_returns_the_target() {
        let (from, to) = (p(0.0, 0.0), p(3.0, 7.0));
        assert_eq!(
            calculate_additional_corner(from, to, true, AngleRestriction::None),
            to
        );
    }

    #[test]
    fn a_warning_branch_without_corners_ends_the_trace_and_with_corners_loops() {
        assert_eq!(stalled(Vec::new(), AT_SECTIONS), Ok(Vec::new()));
        assert_eq!(
            stalled(vec![Corner::New(p(1.0, 2.0))], AT_SECTIONS),
            Err(LocateError::EndlessLoop { at: AT_SECTIONS })
        );
    }
}
