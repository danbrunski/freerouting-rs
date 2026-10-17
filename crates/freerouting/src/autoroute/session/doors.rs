//! Door geometry and door sections (A02.5).
//!
//! Port of `ExpansionDoor` (`ExpansionDoor.java`), `TargetItemExpansionDoor`
//! and `MazeSearchElement` at `a773a70ec`.
//!
//! A door's shape is the intersection of its two rooms' shapes, recomputed
//! on every call as Java's `getShape` does (`:43-52`): room shapes change
//! during the search (an incomplete room's edge removal, the any-angle
//! corner cuts), and a cache would have to follow every change. P02 may
//! cache it later.
//!
//! The maze search state of a door is one [`SectionState`] per section, in
//! the session's flat section array. `section_segments` allocates the
//! sections; when their count changes, the door gets new ones and the old
//! state is lost, as Java replaces `sectionArr` (`:193-201`).

use freerouting_geometry::jmath::d2i;
use freerouting_geometry::{FloatLine, Point, PolylineShapeOps, Simplex, TileShape, TileShapeOps};

use super::{
    Door, DoorId, ExpandableRef, Room, RoomError, RoomId, Session, TargetDoor, TargetDoorId,
};
use crate::autoroute::java_ids;
use crate::board::items::ItemId;

/// Java `AutorouteEngine.TRACE_WIDTH_TOLERANCE`.
pub const TRACE_WIDTH_TOLERANCE: f64 = 2.0;

/// The adjustment direction of a maze search element (Java
/// `MazeSearchElement.Adjustment`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Adjustment {
    #[default]
    None,
    Right,
    Left,
}

/// The maze search state of one door section (Java `MazeSearchElement`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SectionState {
    /// The section is occupied by the expansion (`isOccupied`).
    pub occupied: bool,
    /// The door and section the expansion came from (`backtrackDoor`,
    /// `sectionNoOfBacktrackDoor`).
    pub backtrack: Option<(ExpandableRef, usize)>,
    pub room_ripped: bool,
    pub adjustment: Adjustment,
    /// The ripup cost paid to enter the door's room.
    pub ripup_cost: i32,
}

impl SectionState {
    /// Java `MazeSearchElement.reset`.
    pub fn reset(&mut self) {
        *self = SectionState::default();
    }
}

impl Session<'_> {
    /// Java `new ExpansionDoor(first, second, dimension)`. The door is not
    /// added to the rooms' lists.
    pub fn new_door(&mut self, first: RoomId, second: RoomId, dimension: i32) -> DoorId {
        let id = DoorId(self.doors.len() as u32);
        self.doors.push(Door {
            rooms: [first, second],
            dimension,
            sections: (0, 0),
        });
        id
    }

    /// Java `new ExpansionDoor(first, second)`: the dimension of the
    /// intersection of the two room shapes.
    pub fn new_door_of_shapes(
        &mut self,
        first: RoomId,
        second: RoomId,
    ) -> Result<DoorId, RoomError> {
        let dimension = self.intersection_of(first, second)?.dimension();
        Ok(self.new_door(first, second, dimension))
    }

    fn room_shape_or_npe(&self, room: RoomId) -> Result<&TileShape, RoomError> {
        self.room(room).shape().ok_or(RoomError::NullPointer {
            at: "ExpansionDoor.getShape",
        })
    }

    fn intersection_of(&self, first: RoomId, second: RoomId) -> Result<TileShape, RoomError> {
        let first = self.room_shape_or_npe(first)?;
        let second = self.room_shape_or_npe(second)?;
        Ok(first.intersection(second))
    }

    /// The shape of `door`: the intersection of its rooms' shapes, computed
    /// now (`ExpansionDoor.getShape`).
    pub fn door_shape(&self, door: DoorId) -> Result<TileShape, RoomError> {
        let [first, second] = self.door(door).rooms;
        self.intersection_of(first, second)
    }

    /// The other room of `door` seen from `room`; `None` if `room` is
    /// neither (`ExpansionDoor.otherRoom(ExpansionRoom)`).
    pub fn other_room(&self, door: DoorId, room: RoomId) -> Option<RoomId> {
        let [first, second] = self.door(door).rooms;
        if room == first {
            Some(second)
        } else if room == second {
            Some(first)
        } else {
            None
        }
    }

    /// [`other_room`](Self::other_room) if that is a complete room
    /// (`ExpansionDoor.otherRoom(CompleteExpansionRoom)`).
    pub fn other_complete_room(&self, door: DoorId, room: RoomId) -> Option<RoomId> {
        self.other_room(door, room)
            .filter(|other| self.room(*other).is_complete())
    }

    /// Java `ExpansionDoor.getId`: `min(a, b) * 31 + max(a, b)` of the two
    /// room ids.
    pub fn door_java_id(&self, door: DoorId) -> Result<i32, RoomError> {
        let [first, second] = self.door(door).rooms;
        Ok(java_ids::expansion_door_id(
            self.room_java_id(first)?,
            self.room_java_id(second)?,
        ))
    }

    /// Java `TargetItemExpansionDoor.getId`: `31 * item id + room id`, 0
    /// for no room.
    pub fn target_door_java_id(&self, door: TargetDoorId) -> Result<i32, RoomError> {
        let door = self.target_door(door);
        let room_id = match door.room {
            Some(room) => self.room_java_id(room)?,
            None => 0,
        };
        Ok(java_ids::target_door_id(door.item.0, room_id))
    }

    /// The Java `getId` of an expandable object.
    pub fn expandable_java_id(&self, object: ExpandableRef) -> Result<i32, RoomError> {
        match object {
            ExpandableRef::Door(door) => self.door_java_id(door),
            ExpandableRef::TargetDoor(door) => self.target_door_java_id(door),
            ExpandableRef::Drill(drill) => Ok(self.drill(drill).java_id()),
            ExpandableRef::DrillPage(page) => self
                .built_drill_pages()
                .map(|pages| pages.page(page).java_id())
                .ok_or(RoomError::NullPointer {
                    at: "DrillPage.getId",
                }),
        }
    }

    /// The sections of `door` (`sectionArr`); empty until
    /// [`section_segments`](Self::section_segments) allocates them.
    pub fn door_sections(&self, door: DoorId) -> &[SectionState] {
        let (start, len) = self.door(door).sections;
        &self.sections[start as usize..(start + len) as usize]
    }

    /// The sections of `door`, mutable.
    pub fn door_sections_mut(&mut self, door: DoorId) -> &mut [SectionState] {
        let (start, len) = self.doors[door.0 as usize].sections;
        &mut self.sections[start as usize..(start + len) as usize]
    }

    /// Java `ExpansionDoor.allocateSections` (`:193-201`).
    pub(crate) fn allocate_sections(&mut self, door: DoorId, count: usize) {
        let (_, len) = self.door(door).sections;
        if len as usize == count {
            return;
        }
        // PARITY(quirk): ExpansionDoor.java:197 replaces the section array
        // when the count differs, so the occupation and backtrack state of
        // the old sections are lost.
        let start = self.sections.len() as u32;
        self.sections
            .resize_with(self.sections.len() + count, SectionState::default);
        self.doors[door.0 as usize].sections = (start, count as u32);
    }

    /// Java `ExpansionDoor.getSectionSegments` (`:105-147`): the door line
    /// shrunk by `offset + TRACE_WIDTH_TOLERANCE` and divided into sections
    /// at most `10 * offset` long. Allocates the sections.
    pub fn section_segments(
        &mut self,
        door: DoorId,
        offset: f64,
    ) -> Result<Vec<FloatLine>, RoomError> {
        let offset = offset + TRACE_WIDTH_TOLERANCE;
        let door_shape = self.door_shape(door)?;
        if door_shape.is_empty() {
            return Ok(Vec::new());
        }
        let dimension = self.door(door).dimension;
        let [first, second] = self.door(door).rooms;
        let both_free = matches!(self.room(first), Room::CompleteFree { .. })
            && matches!(self.room(second), Room::CompleteFree { .. });
        let (door_line, shrinked) = if dimension == 1 {
            let line = door_shape
                .diagonal_corner_segment()
                .ok_or(RoomError::NullPointer {
                    at: "ExpansionDoor.getSectionSegments",
                })?;
            (line, line.shrink_segment(offset))
        } else if dimension == 2 && both_free {
            let Some(line) = self.calc_door_line_segment(door, &door_shape)? else {
                // A complete free room inside the other room.
                return Ok(Vec::new());
            };
            if line.b.distance_square(line.a) < 4.0 * offset * offset {
                // A small door; 2-dimensional small doors are not expanded.
                return Ok(Vec::new());
            }
            (line, line.shrink_segment(offset))
        } else {
            let gravity = door_shape.centre_of_gravity();
            let line = FloatLine::new(gravity, gravity);
            (line, line)
        };
        let max_section_width = 10.0 * offset;
        let count = d2i(door_line.b.distance(door_line.a) / max_section_width).wrapping_add(1);
        let count = usize::try_from(count).map_err(|_| RoomError::IndexOutOfBounds {
            at: "ExpansionDoor.allocateSections",
        })?;
        self.allocate_sections(door, count);
        Ok(shrinked.divide_segment_into_sections(count))
    }

    /// Java `ExpansionDoor.calcDoorLineSegment` (`:149-174`): the segment
    /// between the first two different corners of `door_shape` that are on
    /// the border of both room shapes.
    fn calc_door_line_segment(
        &self,
        door: DoorId,
        door_shape: &TileShape,
    ) -> Result<Option<FloatLine>, RoomError> {
        let [first, second] = self.door(door).rooms;
        let first_shape = self.room_shape_or_npe(first)?;
        let second_shape = self.room_shape_or_npe(second)?;
        let mut first_corner: Option<Point> = None;
        let mut second_corner: Option<Point> = None;
        for i in 0..door_shape.border_line_count() {
            let corner = door_shape.corner(i);
            if !first_shape.contains_inside(&corner) && !second_shape.contains_inside(&corner) {
                match &first_corner {
                    None => first_corner = Some(corner),
                    Some(f) if *f != corner => {
                        second_corner = Some(corner);
                        break;
                    }
                    Some(_) => {}
                }
            }
        }
        Ok(match (first_corner, second_corner) {
            (Some(a), Some(b)) => Some(FloatLine::new(a.to_float(), b.to_float())),
            _ => None,
        })
    }

    /// Java `ExpansionDoor.reset`.
    pub fn reset_door(&mut self, door: DoorId) {
        for section in self.door_sections_mut(door) {
            section.reset();
        }
    }

    /// Java `new TargetItemExpansionDoor(item, treeEntryNo, room, tree)`:
    /// the shape is the item's tree shape intersected with the room's, or
    /// empty without a room. Not added to the room.
    pub(crate) fn new_target_door(
        &mut self,
        item: ItemId,
        tree_entry: usize,
        room: Option<RoomId>,
    ) -> Result<TargetDoorId, RoomError> {
        let shape = match room {
            None => TileShape::Simplex(Simplex::empty()),
            Some(room) => {
                let npe = RoomError::NullPointer {
                    at: "TargetItemExpansionDoor.<init>",
                };
                let board = &*self.board;
                let tree = self.tree();
                let item_ref = board.item(item).ok_or(npe.clone())?;
                let item_shape = tree
                    .tree_shape(item_ref, tree_entry, &board.shape_context())
                    .ok_or(npe.clone())?;
                let room_shape = self.room(room).shape().ok_or(npe)?;
                item_shape.intersection(room_shape)
            }
        };
        let id = TargetDoorId(self.target_doors.len() as u32);
        self.target_doors.push(TargetDoor {
            item,
            tree_entry,
            room,
            shape,
            section: SectionState::default(),
        });
        Ok(id)
    }

    /// Java `TargetItemExpansionDoor.isDestinationDoor`: the item is not in
    /// the start set. Java creates the item's autoroute data when it is
    /// missing, with the start flag off.
    ///
    /// With [`AutorouteControl::destinations_only`](crate::autoroute::control::AutorouteControl::destinations_only)
    /// set, the item must also be in that set.
    pub fn is_destination_door(&self, door: TargetDoorId) -> bool {
        let item = self.target_door(door).item;
        !self.is_start_door(door)
            && self
                .control
                .destinations_only
                .as_ref()
                .is_none_or(|only| only.contains(&item))
    }

    /// True if the item is in the start set. With Java's rule (no
    /// `destinations_only`) this is exactly `!is_destination_door`.
    pub fn is_start_door(&self, door: TargetDoorId) -> bool {
        let item = self.target_door(door).item;
        self.item_info(item).is_some_and(|info| info.start)
    }

    /// True if the search ignores the door: its item is neither a start
    /// item nor one of `destinations_only`. Never true with Java's rule.
    pub fn is_ignored_door(&self, door: TargetDoorId) -> bool {
        !self.is_start_door(door) && !self.is_destination_door(door)
    }

    /// Java `TargetItemExpansionDoor.reset`.
    pub fn reset_target_door(&mut self, door: TargetDoorId) {
        self.target_doors[door.0 as usize].section.reset();
    }

    /// Java `ExpansionRoom.removeDoor(door)`: removes the first occurrence
    /// of `door` from `room`'s doors (`ArrayList.remove(Object)`). False if
    /// the room did not have it.
    pub(crate) fn remove_door_from(&mut self, room: RoomId, door: DoorId) -> bool {
        let doors = self.room_mut(room).doors_mut();
        match doors.iter().position(|d| *d == door) {
            Some(index) => {
                // PARITY: ArrayList.remove(Object) shifts the rest.
                doors.remove(index);
                true
            }
            None => false,
        }
    }

    /// Java `ExpansionRoom.addDoor`: appends `door` to `room`'s doors.
    pub(crate) fn add_door_to(&mut self, room: RoomId, door: DoorId) {
        self.room_mut(room).doors_mut().push(door);
    }

    /// Java `doorExists(other)`: a door of `room` has `other` as one of its
    /// rooms.
    pub fn door_exists(&self, room: RoomId, other: RoomId) -> bool {
        self.room(room)
            .doors()
            .iter()
            .any(|d| self.door(*d).rooms.contains(&other))
    }
}
