//! The locator driver (A05.2) and its result and errors (A05.4).
//!
//! Port of `FoundConnectionLocator` (`FoundConnectionLocator.java` at
//! `a773a70ec`): the constructor's loop (`:62-183`), `getInstance`
//! (`:185-211`), `calculateStartingPoint` (`:213-223`), `calculateNextTrace`
//! (`:410-493`) and `adjustStartCorner` (`:522-537`). The 45 degree corner
//! rules are in `locate45.rs`.
//!
//! The located connection runs from the destination back to the start: one
//! [`ResultItem`] per layer, each a list of integer corners, split at the
//! drills of the backtrack chain.
//!
//! **Identity, not value.** Java tests two corners by reference
//! (`FoundConnectionLocator.java:415,432`): a new point with the same value
//! still counts as a new corner. The port passes [`Corner`] and never
//! compares values there.

use std::collections::BTreeSet;
use std::fmt;

use freerouting_geometry::{FloatPoint, IntPoint, PolylineShapeOps, TileShape, TileShapeOps};

use super::backtrack::{BacktrackElement, RipupCosts, backtrack};
use super::locate45;
use crate::autoroute::session::search::SearchResult;
use crate::autoroute::session::{ExpandableRef, RoomError, RoomId, Session, TargetDoorId};
use crate::board::items::ItemId;
use crate::board::structure::AngleRestriction;

/// One trace of a located connection (Java
/// `FoundConnectionLocator.ResultItem`): its corners, rounded and without
/// consecutive duplicates, and its layer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResultItem {
    pub corners: Vec<IntPoint>,
    pub layer: i32,
    /// With a width policy (Q17.1): `half_widths[k]` is the trace half width
    /// of the segment from corner `k - 1` to corner `k`, the policy's width
    /// of the room the locator placed corner `k` in (`half_widths[0]` is
    /// unused). Empty without one: every segment has the control's half
    /// width on the layer.
    pub half_widths: Vec<i32>,
}

/// A located connection (the public fields of Java's
/// `FoundConnectionLocator`). Empty `items` is a valid result: Java returns
/// it when the start of the backtrack chain is not a target door
/// (`:103-111`, then `start_item` is `None`) or the destination is neither a
/// target door nor a drill (`:130-134`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Located {
    /// The traces from the destination to the start (`connectionItems`).
    pub items: Vec<ResultItem>,
    /// The item the connection starts at (`startItem`).
    pub start_item: Option<ItemId>,
    /// The item it ends at; `None` at a fanout drill (`targetItem`).
    pub target_item: Option<ItemId>,
    pub start_layer: i32,
    pub target_layer: i32,
    /// The backtrack chain from the destination to the start
    /// (`backtrackArray`).
    pub backtrack: Vec<BacktrackElement>,
}

/// Where the locator stops where Java throws. Each variant's `at` names the
/// Java method with its file and line; Java's `autorouteConnection` catches
/// the exception and returns `FAILED` (`AutorouteEngine.java:179-195`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocateError {
    /// Java's `NullPointerException`: a shape, room, item or point that
    /// Java dereferences is missing.
    NullPointer { at: &'static str },
    /// Java's `ArrayIndexOutOfBoundsException`: a section, layer or chain
    /// index out of range.
    IndexOutOfBounds { at: &'static str },
    /// Java's `ClassCastException`: the chain element Java casts to
    /// `ExpansionDrill` is something else.
    ClassCast { at: &'static str },
    /// Java's endless loop: `calculateNextTraceCorners` warns and returns
    /// corners without advancing, so `calculateNextTrace` calls it again
    /// forever (until an `OutOfMemoryError`, which Java does not catch).
    EndlessLoop { at: &'static str },
    /// An exception of the room graph (a door shape or its sections).
    Room(RoomError),
}

impl LocateError {
    /// The simple name of the Java exception (or error).
    pub fn java_exception(&self) -> &'static str {
        match self {
            LocateError::NullPointer { .. } => "NullPointerException",
            LocateError::IndexOutOfBounds { .. } => "ArrayIndexOutOfBoundsException",
            LocateError::ClassCast { .. } => "ClassCastException",
            LocateError::EndlessLoop { .. } => "OutOfMemoryError",
            LocateError::Room(e) => e.java_exception(),
        }
    }
}

impl fmt::Display for LocateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LocateError::NullPointer { at } => write!(f, "NullPointerException in {at}"),
            LocateError::IndexOutOfBounds { at } => {
                write!(f, "ArrayIndexOutOfBoundsException in {at}")
            }
            LocateError::ClassCast { at } => write!(f, "ClassCastException in {at}"),
            LocateError::EndlessLoop { at } => write!(
                f,
                "endless loop in {at}: Java repeats the call until it runs out of memory"
            ),
            LocateError::Room(e) => write!(f, "{e} (in the locator)"),
        }
    }
}

impl std::error::Error for LocateError {}

impl From<RoomError> for LocateError {
    fn from(e: RoomError) -> LocateError {
        LocateError::Room(e)
    }
}

/// A corner proposed for the trace, by identity (`// PARITY:`
/// `FoundConnectionLocator.java:415,432`): `Unchanged` is the very point the
/// trace is at; `New` is a new point object, even if its value equals it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Corner {
    Unchanged,
    New(FloatPoint),
}

pub(super) const AT_STARTING_POINT: &str =
    "FoundConnectionLocator.calculateStartingPoint (FoundConnectionLocator.java:216-218)";
pub(super) const AT_CONSTRUCTOR: &str =
    "FoundConnectionLocator.<init> (FoundConnectionLocator.java:113-176)";
const AT_NEXT_TRACE: &str =
    "FoundConnectionLocator.calculateNextTrace (FoundConnectionLocator.java:443-446)";
const AT_ADJUST: &str =
    "FoundConnectionLocator.adjustStartCorner (FoundConnectionLocator.java:526-536)";

/// The state of one location (Java's protected fields of
/// `FoundConnectionLocator`).
pub(super) struct Locator<'a, 'b> {
    pub(super) s: &'a mut Session<'b>,
    pub(super) angle: AngleRestriction,
    pub(super) chain: Vec<BacktrackElement>,
    pub(super) current_from: FloatPoint,
    /// `previousFromPoint`, read by the any-angle locator only.
    pub(super) previous_from: FloatPoint,
    pub(super) current_trace_layer: i32,
    pub(super) from_index: usize,
    pub(super) to_index: usize,
    pub(super) target_index: usize,
    pub(super) target_shape: TileShape,
}

/// `ctrl.compensatedTraceHalfWidth[layer]`, or the width policy's for
/// `room` (Q17.1).
pub(super) fn half_width(
    s: &Session<'_>,
    room: Option<RoomId>,
    layer: i32,
    at: &'static str,
) -> Result<i32, LocateError> {
    let index = usize::try_from(layer)
        .ok()
        .filter(|l| *l < s.control().compensated_trace_half_width.len())
        .ok_or(LocateError::IndexOutOfBounds { at })?;
    Ok(match room {
        Some(room) => s.room_compensated_half_width(room, index),
        None => s.control().compensated_trace_half_width[index],
    })
}

/// The shape of a room; `NullPointer` for an incomplete room on the whole
/// plane.
pub(super) fn room_shape<'s>(
    s: &'s Session<'_>,
    room: RoomId,
    at: &'static str,
) -> Result<&'s TileShape, LocateError> {
    s.room(room).shape().ok_or(LocateError::NullPointer { at })
}

/// `FloatPoint.round().toFloat()`.
pub(super) fn round_to_integer(point: FloatPoint) -> FloatPoint {
    point.round().to_float()
}

/// Java `FoundConnectionLocator.getInstance` (`:185-211`) with the
/// constructor (`:62-183`): backtracks the search result into `ripped` and
/// `costs` (filled even when the caller rejects the result) and locates
/// the traces. The 45 and 90 degree restrictions use the 45 degree
/// locator, no restriction the any-angle locator (A07).
pub fn locate(
    session: &mut Session<'_>,
    result: &SearchResult,
    angle: AngleRestriction,
    ripped: &mut BTreeSet<ItemId>,
    costs: Option<&mut RipupCosts>,
) -> Result<Located, LocateError> {
    let chain = backtrack(session, result, ripped, costs)?;
    let mut located = Located {
        items: Vec::new(),
        start_item: None,
        target_item: None,
        start_layer: 0,
        target_layer: 0,
        backtrack: chain,
    };
    let start = located.backtrack.last().map(|e| e.door);
    let Some(ExpandableRef::TargetDoor(start_door)) = start else {
        // Java warns "ItemExpansionDoor expected for startInfo.door".
        return Ok(located);
    };
    let npe = LocateError::NullPointer { at: AT_CONSTRUCTOR };
    let start_room = session.target_door(start_door).room.ok_or(npe.clone())?;
    located.start_item = Some(session.target_door(start_door).item);
    located.start_layer = session.room(start_room).layer();
    let (destination, section) = result.destination;
    let (current_from, at_fanout_end) = match destination {
        ExpandableRef::TargetDoor(door) => {
            let room = session.target_door(door).room.ok_or(npe)?;
            located.target_item = Some(session.target_door(door).item);
            located.target_layer = session.room(room).layer();
            (calculate_starting_point(session, door)?, false)
        }
        ExpandableRef::Drill(drill) => {
            // Only in a fanout.
            let drill = session.drill(drill);
            located.target_layer = drill.first_layer.wrapping_add(section as i32);
            (drill.location.to_float(), true)
        }
        _ => {
            // Java warns "unexpected type of destinationDoor".
            return Ok(located);
        }
    };
    let mut locator = Locator {
        s: session,
        angle,
        chain: std::mem::take(&mut located.backtrack),
        current_from,
        // FoundConnectionLocator.java:137.
        previous_from: current_from,
        current_trace_layer: located.target_layer,
        from_index: 0,
        to_index: 0,
        target_index: 0,
        target_shape: TileShape::IntBox(freerouting_geometry::IntBox::EMPTY),
    };
    located.items = locator.run(start_door, at_fanout_end)?;
    located.backtrack = locator.chain;
    Ok(located)
}

/// Java `calculateStartingPoint` (`:213-219`): the rounded centre of
/// gravity of the door item's trace connection shape within the door's
/// room.
fn calculate_starting_point(
    s: &Session<'_>,
    door: TargetDoorId,
) -> Result<FloatPoint, LocateError> {
    let npe = LocateError::NullPointer {
        at: AT_STARTING_POINT,
    };
    let target = s.target_door(door);
    let connection = s
        .board
        .trace_connection_shape(s.tree(), target.item, target.tree_entry)
        .ok_or(npe.clone())?;
    let room = target.room.ok_or(npe)?;
    let shape = connection.intersection(room_shape(s, room, AT_STARTING_POINT)?);
    let centre = shape.centre_of_gravity();
    // K15.25 (`router.pour_fill_target`): the end lies in the pour's fill.
    let parts = s.pour_fill_parts(target.item, &shape).unwrap_or_default();
    let end = crate::autoroute::pour_target::end_point(&parts, centre).unwrap_or(centre);
    Ok(round_to_integer(end))
}

impl Locator<'_, '_> {
    /// The constructor's loop (`:139-181`): one trace per layer.
    fn run(
        &mut self,
        start_door: TargetDoorId,
        mut at_fanout_end: bool,
    ) -> Result<Vec<ResultItem>, LocateError> {
        let mut items = Vec::new();
        let mut done = false;
        while !done {
            let mut layer_changed = false;
            if at_fanout_end {
                // Java does not advance currentTargetDoorIndex here.
                layer_changed = true;
            } else {
                self.target_index = self.from_index + 1;
                while self.target_index < self.chain.len() && !layer_changed {
                    if matches!(self.chain[self.target_index].door, ExpandableRef::Drill(_)) {
                        layer_changed = true;
                    } else {
                        self.target_index += 1;
                    }
                }
            }
            if layer_changed {
                // The next trace leads to a via.
                let door = self.chain.get(self.target_index).map(|e| e.door);
                let Some(ExpandableRef::Drill(drill)) = door else {
                    return Err(LocateError::ClassCast { at: AT_CONSTRUCTOR });
                };
                let location = self.s.drill(drill).location.clone();
                self.target_shape = TileShape::IntBox(TileShape::get_instance_point(&location));
            } else {
                // The next trace leads to the final target.
                done = true;
                self.target_index = self.chain.len() - 1;
                self.target_shape = self.final_target_shape(start_door)?;
            }
            self.to_index = self.from_index + 1;
            let trace = self.calculate_next_trace(layer_changed, at_fanout_end)?;
            at_fanout_end = false;
            items.push(trace);
        }
        Ok(items)
    }

    /// The start item's connection shape within the start door's room,
    /// shrunk by the trace half width when it is an area and the shrunk
    /// shape is not empty (`:164-175`).
    fn final_target_shape(&self, start_door: TargetDoorId) -> Result<TileShape, LocateError> {
        let npe = LocateError::NullPointer { at: AT_CONSTRUCTOR };
        let door = self.s.target_door(start_door);
        let connection = self
            .s
            .board
            .trace_connection_shape(self.s.tree(), door.item, door.tree_entry)
            .ok_or(npe.clone())?;
        let room = door.room.ok_or(npe)?;
        let mut shape = connection.intersection(room_shape(self.s, room, AT_CONSTRUCTOR)?);
        // K15.25 (`router.pour_fill_target`): a start pour is reached in its
        // fill, in the part nearest the previous corner.
        let parts = self
            .s
            .pour_fill_parts(door.item, &shape)
            .unwrap_or_default();
        if let Some(part) = crate::autoroute::pour_target::nearest_part(&parts, self.current_from) {
            shape = part.clone();
        }
        if shape.dimension() >= 2 {
            // The target is a conduction area: shrink it by the trace half
            // width for a safe connection.
            let layer = self.s.room(room).layer();
            let half_width = f64::from(half_width(self.s, Some(room), layer, AT_CONSTRUCTOR)?);
            let shrinked = shape.offset(-half_width);
            if !shrinked.is_empty() {
                return Ok(shrinked);
            }
        }
        Ok(shape)
    }

    /// Java `calculateNextTrace` (`:410-493`).
    fn calculate_next_trace(
        &mut self,
        layer_changed: bool,
        at_fanout_end: bool,
    ) -> Result<ResultItem, LocateError> {
        let mut corners = vec![self.current_from];
        // Q17.1: the half width of the room of each corner, with a policy.
        let with_widths = self.s.width_policy().is_some();
        let mut widths: Vec<i32> = Vec::new();
        if with_widths {
            widths.push(i32::MAX);
        }
        if !at_fanout_end {
            // PARITY: FoundConnectionLocator.java:415 compares the adjusted
            // corner with currentFromPoint by reference.
            if let Corner::New(adjusted) = self.adjust_start_corner()? {
                let add = locate45::calculate_additional_corner(
                    self.current_from,
                    adjusted,
                    true,
                    self.angle,
                );
                corners.push(add);
                corners.push(adjusted);
                if with_widths {
                    let width = self.room_trace_half_width(self.from_index);
                    widths.extend([width, width]);
                }
                self.previous_from = self.current_from;
                self.current_from = adjusted;
            }
        }
        loop {
            let width = if with_widths {
                self.room_trace_half_width(self.to_index.wrapping_sub(1))
            } else {
                0
            };
            let next = match self.angle {
                AngleRestriction::None => self.next_trace_corners_any()?,
                _ => locate45::calculate_next_trace_corners(self)?,
            };
            if next.is_empty() {
                break;
            }
            for corner in next {
                // PARITY: FoundConnectionLocator.java:432 compares each
                // corner with the previous one by reference.
                if let Corner::New(point) = corner {
                    corners.push(point);
                    if with_widths {
                        widths.push(width);
                    }
                    self.previous_from = self.current_from;
                    self.current_from = point;
                }
            }
        }
        let mut next_layer = self.current_trace_layer;
        if layer_changed {
            self.from_index = self.target_index + 1;
            let element = self
                .chain
                .get(self.from_index)
                .ok_or(LocateError::IndexOutOfBounds { at: AT_NEXT_TRACE })?;
            if let Some(room) = element.next_room {
                next_layer = self.s.room(room).layer();
            }
        }
        let mut rounded: Vec<IntPoint> = Vec::with_capacity(corners.len());
        let mut half_widths: Vec<i32> = Vec::with_capacity(widths.len());
        for (i, corner) in corners.into_iter().enumerate() {
            let point = corner.round();
            if rounded.last() != Some(&point) {
                rounded.push(point);
                if with_widths {
                    half_widths.push(widths[i]);
                }
            } else if let Some(last) = half_widths.last_mut() {
                // A segment rounded away: its corner keeps the narrower.
                *last = (*last).min(widths[i]);
            }
        }
        let item = ResultItem {
            corners: rounded,
            layer: self.current_trace_layer,
            half_widths,
        };
        self.current_trace_layer = next_layer;
        Ok(item)
    }

    /// The trace half width (not compensated) of the room chain element
    /// `index` enters, on the current trace layer (Q17.1): the width a
    /// segment placed in that room is inserted at. The control's without a
    /// room.
    fn room_trace_half_width(&self, index: usize) -> i32 {
        let layer = usize::try_from(self.current_trace_layer).unwrap_or(0);
        let layer = layer.min(self.s.control().trace_half_width.len().saturating_sub(1));
        match self.chain.get(index).and_then(|e| e.next_room) {
            Some(room) => self.s.room_half_width(room, layer),
            None => self.s.control().trace_half_width[layer],
        }
    }

    /// Java `adjustStartCorner` (`:522-537`): the start corner moved into
    /// the start room shrunk by the trace half width, if it is outside.
    fn adjust_start_corner(&self) -> Result<Corner, LocateError> {
        let element = self
            .chain
            .get(self.from_index)
            .ok_or(LocateError::IndexOutOfBounds { at: AT_ADJUST })?;
        let Some(room) = element.next_room else {
            return Ok(Corner::Unchanged);
        };
        let half_width = f64::from(half_width(
            self.s,
            Some(room),
            self.current_trace_layer,
            AT_ADJUST,
        )?);
        let shrinked = room_shape(self.s, room, AT_ADJUST)?.offset(-half_width);
        if shrinked.is_empty() || shrinked.contains_float(self.current_from) {
            return Ok(Corner::Unchanged);
        }
        let nearest = shrinked
            .nearest_point_approx(self.current_from)
            .ok_or(LocateError::NullPointer { at: AT_ADJUST })?;
        Ok(Corner::New(round_to_integer(nearest)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each error names its Java exception, and its `Display` the Java
    /// class and line.
    #[test]
    fn each_error_maps_to_its_java_exception_and_names_the_java_site() {
        let cases = [
            (
                LocateError::NullPointer {
                    at: AT_STARTING_POINT,
                },
                "NullPointerException",
                "FoundConnectionLocator.java:216",
            ),
            (
                LocateError::IndexOutOfBounds { at: AT_NEXT_TRACE },
                "ArrayIndexOutOfBoundsException",
                "FoundConnectionLocator.java:443",
            ),
            (
                LocateError::ClassCast { at: AT_CONSTRUCTOR },
                "ClassCastException",
                "FoundConnectionLocator.java:113",
            ),
            (
                LocateError::EndlessLoop {
                    at: locate45::AT_NOT_A_DOOR,
                },
                "OutOfMemoryError",
                "FoundConnectionLocator45Degree.java:229",
            ),
            (
                LocateError::Room(RoomError::NullPointer {
                    at: "ExpansionDoor.getShape",
                }),
                "NullPointerException",
                "ExpansionDoor.getShape",
            ),
        ];
        for (error, exception, site) in cases {
            assert_eq!(error.java_exception(), exception, "{error:?}");
            let text = error.to_string();
            assert!(text.contains(site), "{text} names {site}");
        }
    }
}
