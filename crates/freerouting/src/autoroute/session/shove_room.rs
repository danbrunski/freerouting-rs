//! Shoving obstacle trace rooms (A04.7).
//!
//! Port of `MazeTraceShover.checkShoveTraceLine` (`MazeTraceShover.java:
//! 32-318`, with `endPointsMatching`) and `MazeSearchEngine.shoveTraceRoom`
//! (`MazeSearchEngine.java:1152-1227`) at `a773a70ec`.
//!
//! When the search enters the room of an obstacle trace of its own width
//! and clearance class from the outermost section of a door, it asks how
//! far the trace's line can be pushed to one side: first
//! `checkTraceSegment` (B04) for the part nothing unshovable blocks, then
//! the static `TraceShover.check` (O03) for the shove itself. The doors on
//! that side within the shove width become reachable, and their sections
//! are expanded with the adjustment `Left` or `Right` for the link door of a
//! dimension-2 trace room. Java's "left" pass shoves with
//! `shoveToTheLeft = false` and its "right" pass with `true`; the port
//! keeps that.

use freerouting_geometry::{
    FloatLine, FloatPoint, Line, LineSegment, Point, PolylineShapeOps, Side, TileShapeOps,
};

use super::queue::MazeListElement;
use super::search::MazeSearch;
use super::sink::{ExtraCost, SearchSink};
use super::{Adjustment, DoorId, ExpandableRef, Room, RoomError, RoomId};
use crate::board::basic_board::BasicBoard;
use crate::board::items::ItemId;
use crate::shove::trace_check::TraceShover;

/// A door section a shove makes reachable (Java
/// `MazeTraceShover.DoorSection`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct DoorSection {
    pub door: DoorId,
    pub section: usize,
    pub line: FloatLine,
}

/// Java `MazeTraceShover.endPointsMatching` (`:320-342`): the trace and
/// `from_item` meet at an end point, so a shove can continue through a
/// link door.
fn end_points_matching(board: &BasicBoard, trace: ItemId, from_item: ItemId) -> bool {
    if from_item == trace {
        return true;
    }
    let (Some(trace_item), Some(from)) = (board.item(trace), board.item(from_item)) else {
        return false;
    };
    let Some(t) = trace_item.as_trace() else {
        return false;
    };
    if !trace_item.shares_net(from) {
        return false;
    }
    if let Some(drill) = from.drill() {
        let center = Point::Int(drill.center);
        center == t.first_corner()
            || center == t.last_corner()
            // N04: under `inside_pad` an end whose cap overlaps a pin's pad
            // meets it too.
            || board.pad_touches_trace_end(from, &t.first_corner(), t.layer, t.half_width)
            || board.pad_touches_trace_end(from, &t.last_corner(), t.layer, t.half_width)
    } else if let Some(f) = from.as_trace() {
        t.first_corner() == f.first_corner()
            || t.first_corner() == f.last_corner()
            || t.last_corner() == f.first_corner()
            || t.last_corner() == f.last_corner()
    } else {
        false
    }
}

/// The nearer end of `line` seen from `point` (`a` on a tie).
fn nearer_end(line: &FloatLine, point: FloatPoint) -> FloatPoint {
    if line.a.distance_square(point) <= line.b.distance_square(point) {
        line.a
    } else {
        line.b
    }
}

impl<S: SearchSink, C: ExtraCost> MazeSearch<'_, '_, S, C> {
    /// `shoveTraceRoom` (`MazeSearchEngine.java:1152-1227`): false if no
    /// door was expanded, so that the occupation by ripup can be delayed.
    pub(crate) fn shove_trace_room(
        &mut self,
        element: &MazeListElement,
        room: RoomId,
    ) -> Result<bool, RoomError> {
        let count = self.s.section_count(element.door);
        if element.section != 0 && element.section + 1 != count {
            // Inner sections of a door are not shoved.
            return Ok(true);
        }
        let mut result = false;
        // PARITY: MazeSearchEngine.java:1161-1221, the "left" list shoves
        // with shoveToTheLeft = false, the "right" list with true.
        for (skip, to_the_left, link_adjustment) in [
            (Adjustment::Right, false, Adjustment::Left),
            (Adjustment::Left, true, Adjustment::Right),
        ] {
            if element.adjustment == skip {
                continue;
            }
            let mut sections = Vec::new();
            if self.check_shove_trace_line(element, room, to_the_left, &mut sections)? {
                result = true;
            }
            for section in sections {
                let adjustment = if self.s.door(section.door).dimension == 2 {
                    // The link door to the next room.
                    link_adjustment
                } else {
                    Adjustment::None
                };
                self.expand_to_door_section(
                    ExpandableRef::Door(section.door),
                    section.section,
                    Some(section.line),
                    element,
                    0,
                    adjustment,
                )?;
            }
        }
        Ok(result)
    }

    /// `MazeTraceShover.checkShoveTraceLine` (`:32-318`): false if the shove
    /// did not succeed and shoving from another door section may do better.
    /// The reachable door sections go to `to_doors`.
    fn check_shove_trace_line(
        &mut self,
        element: &MazeListElement,
        room: RoomId,
        to_the_left: bool,
        to_doors: &mut Vec<DoorSection>,
    ) -> Result<bool, RoomError> {
        let ExpandableRef::Door(from_door) = element.door else {
            return Ok(true);
        };
        let Room::Obstacle {
            item: trace_id,
            index: corner_no,
            layer,
            ..
        } = self.s.room(room)
        else {
            return Ok(true);
        };
        let (trace_id, corner_no, layer) = (*trace_id, *corner_no, *layer);
        let control = self.s.control;
        let layer_index = layer as usize;
        let (polyline, half_width, class) = {
            let Some(item) = self.s.board.item(trace_id) else {
                return Ok(true);
            };
            let Some(trace) = item.as_trace() else {
                return Ok(true);
            };
            (
                trace.polyline.clone(),
                trace.half_width,
                item.clearance_class(),
            )
        };
        // Only traces of the same half width and clearance class are shoved.
        if half_width != control.trace_half_width[layer_index]
            || class != control.trace_clearance_class_index
        {
            return Ok(true);
        }
        let compensated = f64::from(control.compensated_trace_half_width[layer_index]);
        let from_door_shape = self.s.door_shape(from_door)?;
        if from_door_shape.max_width() < 2.0 * compensated {
            return Ok(true);
        }
        let lines = polyline.lines();
        // Stale indices occur when traces change during routing.
        if corner_no + 2 >= lines.len() {
            return Ok(false);
        }
        let dimension = self.s.door(from_door).dimension;
        let npe = |at| RoomError::NullPointer { at };
        let mut segment = if dimension == 2 {
            // Shove from a link door towards the other link door.
            let Some(other) = self.s.other_complete_room(from_door, room) else {
                return Ok(false);
            };
            let Room::Obstacle {
                item: other_item, ..
            } = self.s.room(other)
            else {
                return Ok(false);
            };
            if !end_points_matching(self.s.board, trace_id, *other_item) {
                return Ok(false);
            }
            let door_center = from_door_shape.centre_of_gravity();
            let corner1 = polyline.corner_approx(corner_no);
            let corner2 = polyline.corner_approx(corner_no + 1);
            if corner1.distance_square(corner2) < 1.0 {
                // The segment may be reduced to a point.
                return Ok(false);
            }
            let towards_start =
                door_center.distance_square(corner2) < door_center.distance_square(corner1);
            let segment = LineSegment::from_polyline(&polyline, corner_no + 1);
            if towards_start {
                segment.opposite()
            } else {
                segment
            }
        } else {
            let from_room = self
                .s
                .other_complete_room(from_door, room)
                .ok_or(npe("MazeTraceShover.checkShoveTraceLine"))?;
            let from_point = self.room_shape(from_room)?.centre_of_gravity();
            let shove_trace_line = &lines[corner_no + 1];
            let door_segment = from_door_shape
                .diagonal_corner_segment()
                .ok_or(npe("MazeTraceShover.checkShoveTraceLine"))?;
            let side_of_trace_line = shove_trace_line.side_of_float_tol(door_segment.a, 0.0);
            let polar = from_door_shape
                .polar_line_segment(from_point)
                .ok_or(npe("MazeTraceShover.checkShoveTraceLine"))?;
            let door_line_swapped =
                polar.b.distance_square(door_segment.a) < polar.a.distance_square(door_segment.a);
            let check_distance = compensated + 5.0;
            let check_square = check_distance * check_distance;
            let entry = &element.shape_entry;
            // Shove only from the right most section to the right or from the
            // left most section to the left.
            let section_ok =
                if to_the_left && !door_line_swapped || !to_the_left && door_line_swapped {
                    element.section + 1 == self.s.section_count(element.door)
                        && (entry.a.distance_square(door_segment.b) <= check_square
                            || entry.b.distance_square(door_segment.b) <= check_square)
                } else {
                    element.section == 0
                        && (entry.a.distance_square(door_segment.a) <= check_square
                            || entry.b.distance_square(door_segment.a) <= check_square)
                };
            if !section_ok {
                return Ok(false);
            }
            let shrinked = polar.shrink_segment(compensated);
            let freerouting_geometry::Direction::Int(perpendicular) =
                shove_trace_line.direction().turn_45_degree(2)
            else {
                return Err(npe("MazeTraceShover.checkShoveTraceLine"));
            };
            let forward = side_of_trace_line == Side::OnTheLeft && to_the_left
                || side_of_trace_line != Side::OnTheLeft && !to_the_left;
            let start_point = if to_the_left { shrinked.b } else { shrinked.a };
            let closing = Line::from_point_and_direction(start_point.round(), perpendicular);
            if forward {
                LineSegment::new(closing, lines[corner_no + 1], lines[corner_no + 2])
            } else {
                LineSegment::new(
                    closing,
                    lines[corner_no + 1].opposite(),
                    lines[corner_no].opposite(),
                )
            }
        };
        let trace_half_width = control.trace_half_width[layer_index];
        let nets = [control.net_number];
        let mut shove_width = self.s.board.check_trace_segment_line(
            &segment,
            layer,
            &nets,
            trace_half_width,
            control.trace_clearance_class_index,
            true,
        );
        let max_int = f64::from(i32::MAX);
        let mut shortened = false;
        if shove_width < max_int {
            shove_width -= 1.0;
            if shove_width <= 0.0 {
                return Ok(true);
            }
            segment = segment.change_length_approx(shove_width);
            shortened = true;
        }
        let from_corner = segment.start_point_approx();
        let to_corner = segment.end_point_approx();
        let segment_is_point = from_corner.distance_square(to_corner) < 0.1;
        if !segment_is_point {
            shove_width = TraceShover::new(self.s.board).check_segment_ok_length(
                &segment,
                to_the_left,
                layer,
                &nets,
                trace_half_width,
                control.trace_clearance_class_index,
                control.max_shove_trace_recursion_depth,
                control.max_shove_via_recursion_depth,
            );
            if shove_width <= 0.0 {
                return Ok(true);
            }
        }
        if shortened {
            shove_width =
                freerouting_geometry::jmath::min_f64(shove_width, from_corner.distance(to_corner));
        }
        let shove_line = segment.get_line();
        // A door must lie between the from door and the end of the shove.
        let from_door_compare = if dimension == 2 || segment_is_point {
            f64::MAX
        } else {
            to_corner.distance_square(from_door_shape.corner_approx(0))
        };
        let wanted_side = if to_the_left {
            Side::OnTheLeft
        } else {
            Side::OnTheRight
        };
        for door in self.s.room(room).doors().to_vec() {
            if door == from_door {
                continue;
            }
            let [first, second] = self.s.door(door).rooms;
            if let (Room::Obstacle { item: a, .. }, Room::Obstacle { item: b, .. }) =
                (self.s.room(first), self.s.room(second))
                && a != b
            {
                // There may be topological problems at a trace fork.
                continue;
            }
            let door_shape = self.s.door_shape(door)?;
            if self.s.door(door).dimension == 2 && shove_width >= max_int {
                if door_shape.contains_float(to_corner) {
                    let sections = self.s.section_segments(door, compensated)?;
                    let line = *sections.first().ok_or(RoomError::IndexOutOfBounds {
                        at: "MazeTraceShover.checkShoveTraceLine",
                    })?;
                    to_doors.push(DoorSection {
                        door,
                        section: 0,
                        line,
                    });
                }
            } else if !segment_is_point {
                // The door is 1-dimensional: on the same border line as the
                // from door?
                let Some(door_segment) = door_shape.diagonal_corner_segment() else {
                    continue;
                };
                if shove_line.side_of_float_tol(door_segment.a, 0.0) != wanted_side
                    || shove_line.side_of_float_tol(door_segment.b, 0.0) != wanted_side
                {
                    continue;
                }
                let door_line = door_shape
                    .polar_line_segment(from_corner)
                    .ok_or(npe("MazeTraceShover.checkShoveTraceLine"))?;
                let nearest_corner = nearer_end(&door_line, from_corner);
                if to_corner.distance_square(nearest_corner) >= from_door_compare {
                    // Not in the direction of the shove's end.
                    continue;
                }
                let projection = nearest_corner.projection_approx(&shove_line);
                if projection.distance(from_corner) + compensated <= shove_width {
                    let sections = self.s.section_segments(door, compensated)?;
                    for (i, line) in sections.into_iter().enumerate() {
                        let corner = nearer_end(&line, from_corner);
                        if corner.projection_approx(&shove_line).distance(from_corner)
                            <= shove_width
                        {
                            to_doors.push(DoorSection {
                                door,
                                section: i,
                                line,
                            });
                        }
                    }
                }
            }
        }
        Ok(true)
    }
}
