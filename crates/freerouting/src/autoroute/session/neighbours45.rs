//! The 45 degree neighbour calculation of a room (A02.3, A02.4).
//!
//! Port of `Sorted45DegreeRoomNeighbours` (972 lines) and
//! `SortedRoomNeighbours.insertDoorOk` (`SortedRoomNeighbours.java:331-388`),
//! with `CompleteFreeSpaceExpansionRoom.calculateTargetDoors` and
//! `ObstacleExpansionRoom.createOverlapDoor`, at `a773a70ec`.
//!
//! A room is completed by finding the objects of the autoroute tree that
//! touch it, sorting them counterclockwise around its octagon, creating
//! doors to them, and creating new incomplete rooms with doors in the gaps
//! between them. When an edge of the room touches nothing, the edge lines
//! are removed and the room completed again; if it grew, the calculation
//! starts over with a new room id (`calculate`, `:45-79`).
//!
//! Orders that decide results:
//! - the tree's overlap order, then a **stable** sort by `getId()` and shape
//!   index (`:104-112`), so an item and a room with the same id keep tree
//!   order (rooms first);
//! - the neighbour set: Java's `TreeSet` with an exact integer comparator
//!   (`:913-970`); a sorted vector with first-wins insertion holds the same
//!   elements in the same order;
//! - the neighbour iteration of `calculateNewIncompleteRooms`, which decides
//!   the creation order of the incomplete rooms and their doors (in
//!   `session/incomplete45.rs`).

use std::cmp::Ordering;

use freerouting_geometry::{CRIT_INT, IntOctagon, Line, PolylineShapeOps, TileShape, TileShapeOps};

use super::rooms::FromRoom;
use super::{Room, RoomError, RoomId, Session};
use crate::board::items::ItemId;
use crate::board::search::rooms::IncompleteRoom;
use crate::board::search::{ObjectEntry, RoomId as TreeRoomId, TreeObject};

/// A neighbour of the room being completed (Java
/// `Sorted45DegreeRoomNeighbours.SortedRoomNeighbour`).
#[derive(Clone, Debug)]
pub(crate) struct Neighbour45 {
    pub object: TreeObject,
    /// The intersection of the bounding octagon of the neighbour's tree
    /// shape (Java's unused `shape` field) with the room's octagon.
    pub intersection: IntOctagon,
    /// The first and last side of the room's octagon the neighbour touches.
    pub first: usize,
    pub last: usize,
}

/// The state of one neighbour calculation.
pub(crate) struct Calc45 {
    /// The completed room: a new complete free room, or the obstacle room.
    pub completed: RoomId,
    pub from_obstacle: bool,
    /// The layer of the room.
    pub layer: i32,
    /// The bounding octagon of the completed room's shape (`roomShape`).
    pub room_oct: IntOctagon,
    /// `sortedNeighbours`, in comparator order.
    pub sorted: Vec<Neighbour45>,
    /// `edgeInteriorTouchesObstacle`.
    pub edge_touch: [bool; 8],
}

const NPE: RoomError = RoomError::NullPointer {
    at: "Sorted45DegreeRoomNeighbours.calculate",
};

/// The Java `getId` of a tree object: the room's counter id or the item's
/// id.
pub(super) fn object_java_id(object: TreeObject) -> i32 {
    match object {
        TreeObject::Room(id) => id.0,
        TreeObject::Item(id) => id.0,
    }
}

/// `Integer.signum`-style ordering of a Java `int` comparator result.
pub(super) fn sign(value: i32) -> Ordering {
    value.cmp(&0)
}

impl Neighbour45 {
    /// The constructor (`:815-906`): the first and last touching sides of
    /// the room's octagon. `None` if the room may lie inside the
    /// neighbour (Java's `-1`). Marks the edges whose interior the
    /// neighbour touches in `edge_touch`, whether or not the neighbour then
    /// enters the set.
    fn new(
        object: TreeObject,
        intersection: IntOctagon,
        room: &IntOctagon,
        edge_touch: &mut [bool; 8],
    ) -> Option<Neighbour45> {
        let is = &intersection;
        let first = if is.bottom_y == room.bottom_y
            && is.lower_left_diagonal_x > room.lower_left_diagonal_x
        {
            0
        } else if is.lower_right_diagonal_x == room.lower_right_diagonal_x
            && is.bottom_y > room.bottom_y
        {
            1
        } else if is.right_x == room.right_x
            && is.lower_right_diagonal_x < room.lower_right_diagonal_x
        {
            2
        } else if is.upper_right_diagonal_x == room.upper_right_diagonal_x
            && is.right_x < room.right_x
        {
            3
        } else if is.top_y == room.top_y && is.upper_right_diagonal_x < room.upper_right_diagonal_x
        {
            4
        } else if is.upper_left_diagonal_x == room.upper_left_diagonal_x && is.top_y < room.top_y {
            5
        } else if is.left_x == room.left_x && is.upper_left_diagonal_x > room.upper_left_diagonal_x
        {
            6
        } else if is.lower_left_diagonal_x == room.lower_left_diagonal_x && is.left_x > room.left_x
        {
            7
        } else {
            return None;
        };
        let last = if is.lower_left_diagonal_x == room.lower_left_diagonal_x
            && is.bottom_y > room.bottom_y
        {
            7
        } else if is.left_x == room.left_x && is.lower_left_diagonal_x > room.lower_left_diagonal_x
        {
            6
        } else if is.upper_left_diagonal_x == room.upper_left_diagonal_x && is.left_x > room.left_x
        {
            5
        } else if is.top_y == room.top_y && is.upper_left_diagonal_x > room.upper_left_diagonal_x {
            4
        } else if is.upper_right_diagonal_x == room.upper_right_diagonal_x && is.top_y < room.top_y
        {
            3
        } else if is.right_x == room.right_x
            && is.upper_right_diagonal_x < room.upper_right_diagonal_x
        {
            2
        } else if is.lower_right_diagonal_x == room.lower_right_diagonal_x
            && is.right_x < room.right_x
        {
            1
        } else if is.bottom_y == room.bottom_y
            && is.lower_right_diagonal_x < room.lower_right_diagonal_x
        {
            0
        } else {
            return None;
        };
        // PARITY: Sorted45DegreeRoomNeighbours.java:882-905, a side effect
        // of the constructor, so it also happens for a neighbour the set
        // then drops as a duplicate.
        let mut next = first;
        loop {
            let current = next;
            next = (next + 1) % 8;
            if !edge_touch[current] {
                let mut touch_only_at_corner = false;
                if current == first && is.int_corner(current) == room.int_corner(next) {
                    touch_only_at_corner = true;
                }
                if current == last && is.int_corner(next) == room.int_corner(current) {
                    touch_only_at_corner = true;
                }
                if !touch_only_at_corner {
                    edge_touch[current] = true;
                }
            }
            if current == last {
                break;
            }
        }
        Some(Neighbour45 {
            object,
            intersection,
            first,
            last,
        })
    }

    /// The comparator (`:913-970`): counterclockwise around the room by the
    /// first touching side and point, then the last, then the object id.
    fn compare(&self, other: &Neighbour45) -> Ordering {
        // PARITY: Sorted45DegreeRoomNeighbours.java:913-970, int arithmetic.
        match self.first.cmp(&other.first) {
            Ordering::Equal => {}
            unequal => return unequal,
        }
        let is1 = &self.intersection;
        let is2 = &other.intersection;
        let mut cmp = match self.first {
            0 => is1.corner_x(0).wrapping_sub(is2.corner_x(0)),
            1 => is1.corner_x(1).wrapping_sub(is2.corner_x(1)),
            2 => is1.corner_y(2).wrapping_sub(is2.corner_y(2)),
            3 => is1.corner_y(3).wrapping_sub(is2.corner_y(3)),
            4 => is2.corner_x(4).wrapping_sub(is1.corner_x(4)),
            5 => is2.corner_x(5).wrapping_sub(is1.corner_x(5)),
            6 => is2.corner_y(6).wrapping_sub(is1.corner_y(6)),
            _ => is2.corner_y(7).wrapping_sub(is1.corner_y(7)),
        };
        if cmp == 0 {
            let this_diff = (self.last + 8 - self.first) % 8;
            let other_diff = (other.last + 8 - other.first) % 8;
            match this_diff.cmp(&other_diff) {
                Ordering::Equal => {}
                unequal => return unequal,
            }
            cmp = match self.last {
                0 => is1.corner_x(1).wrapping_sub(is2.corner_x(1)),
                1 => is1.corner_x(2).wrapping_sub(is2.corner_x(2)),
                2 => is1.corner_y(3).wrapping_sub(is2.corner_y(3)),
                3 => is1.corner_y(4).wrapping_sub(is2.corner_y(4)),
                4 => is2.corner_x(5).wrapping_sub(is1.corner_x(5)),
                5 => is2.corner_x(6).wrapping_sub(is1.corner_x(6)),
                6 => is2.corner_y(7).wrapping_sub(is1.corner_y(7)),
                _ => is2.corner_y(0).wrapping_sub(is1.corner_y(0)),
            };
        }
        if cmp == 0 {
            cmp = object_java_id(self.object).wrapping_sub(object_java_id(other.object));
        }
        sign(cmp)
    }
}

impl Calc45 {
    /// Java `addSortedNeighbour` (`:244-251`).
    fn add_neighbour(&mut self, object: TreeObject, intersection: IntOctagon) {
        let Some(neighbour) =
            Neighbour45::new(object, intersection, &self.room_oct, &mut self.edge_touch)
        else {
            return;
        };
        // PARITY: TreeSet.add keeps the element already there on a tie.
        if let Err(index) = self
            .sorted
            .binary_search_by(|probe| probe.compare(&neighbour))
        {
            self.sorted.insert(index, neighbour);
        }
    }
}

/// Java `removeNotTouchingBorderLines` (`:173-242`): the border lines whose
/// interior touches nothing are moved to the coordinate limits.
fn remove_not_touching_border_lines(room: &IntOctagon, touch: &[bool; 8]) -> IntOctagon {
    let pick = |side: usize, value: i32, limit: i32| if touch[side] { value } else { limit };
    IntOctagon::new(
        pick(6, room.left_x, -CRIT_INT),
        pick(0, room.bottom_y, -CRIT_INT),
        pick(2, room.right_x, CRIT_INT),
        pick(4, room.top_y, CRIT_INT),
        pick(5, room.upper_left_diagonal_x, -CRIT_INT),
        pick(1, room.lower_right_diagonal_x, CRIT_INT),
        pick(7, room.lower_left_diagonal_x, -CRIT_INT),
        pick(3, room.upper_right_diagonal_x, CRIT_INT),
    )
    .normalize()
}

impl Session<'_> {
    /// Java `Sorted45DegreeRoomNeighbours.calculate` (`:45-79`). Returns
    /// the completed room.
    pub(crate) fn calculate_neighbours_45(
        &mut self,
        from: FromRoom,
    ) -> Result<Option<RoomId>, RoomError> {
        let mut from = from;
        loop {
            let room_id = self.generate_room_id();
            let calc = self.sorted_neighbours_45(&from, room_id)?;
            if let FromRoom::Incomplete(room) = &mut from
                && self.try_remove_edge_line(room, &calc)?
            {
                // PARITY: Sorted45DegreeRoomNeighbours.java:63-66, the doors
                // go and the calculation starts over with a new room id.
                self.remove_all_doors(calc.completed);
                continue;
            }
            if calc.sorted.is_empty() {
                if calc.from_obstacle {
                    // N06.5 reachability: the full edge sweep that
                    // `edge_corner_not_advanced` cuts short.
                    crate::run_stats::record_n06(
                        crate::run_stats::Counter::MazeEdgeSweepsWithoutNeighbours,
                    );
                    self.edge_incomplete_rooms_of_obstacle(&calc, 0, 7);
                }
            } else {
                self.new_incomplete_rooms_45(&calc);
            }
            return Ok(Some(calc.completed));
        }
    }

    /// The tree shape of a tree object (`getTreeShape`): an item's stored
    /// shape or a room's shape.
    pub(super) fn object_tree_shape(&self, object: TreeObject, index: usize) -> Option<TileShape> {
        let tree = self.tree();
        match object {
            TreeObject::Room(id) => tree.room(id).map(|r| r.shape.clone()),
            TreeObject::Item(id) => {
                let item = self.board.item(id)?;
                tree.tree_shape(item, index, &self.board.shape_context())
            }
        }
    }

    /// `isTraceObstacle(netNumber)` of a tree object; rooms always are.
    pub(super) fn is_trace_obstacle_object(&self, object: TreeObject) -> bool {
        match object {
            TreeObject::Room(_) => true,
            TreeObject::Item(id) => self
                .board
                .item(id)
                .is_some_and(|item| self.board.is_trace_obstacle(item, self.net)),
        }
    }

    pub(super) fn is_routable(&self, item: ItemId) -> bool {
        self.board.item(item).is_some_and(|i| i.is_routable())
    }

    /// Java `calculateNeighbours` (`:85-171`).
    fn sorted_neighbours_45(&mut self, from: &FromRoom, room_id: i32) -> Result<Calc45, RoomError> {
        let (room_shape, layer, completed, from_obstacle) = match from {
            FromRoom::Incomplete(room) => {
                let shape = room.shape.clone().ok_or(NPE)?;
                let completed = self.push_room(Room::CompleteFree {
                    id: room_id,
                    shape: shape.clone(),
                    layer: room.layer,
                    doors: Vec::new(),
                    target_doors: Vec::new(),
                    net_dependent: false,
                });
                (shape, room.layer, completed, false)
            }
            FromRoom::Obstacle(room) => {
                let r = self.room(*room);
                (r.shape().cloned().ok_or(NPE)?, r.layer(), *room, true)
            }
        };
        let room_oct = room_shape.bounding_octagon().ok_or(NPE)?;
        let mut calc = Calc45 {
            completed,
            from_obstacle,
            layer,
            room_oct,
            sorted: Vec::new(),
            edge_touch: [false; 8],
        };
        let mut entries: Vec<ObjectEntry> = Vec::new();
        self.tree().overlapping_object_entries(
            &room_shape,
            layer,
            &[],
            &self.board.items,
            &mut entries,
        );
        // PARITY: Sorted45DegreeRoomNeighbours.java:104-112, a stable sort
        // by getId() and then shape index.
        entries.sort_by(|a, b| {
            let by_id = object_java_id(a.object).wrapping_sub(object_java_id(b.object));
            if by_id != 0 {
                sign(by_id)
            } else {
                a.shape_index.cmp(&b.shape_index)
            }
        });
        for entry in entries {
            self.neighbour_entry_45(&mut calc, entry)?;
        }
        Ok(calc)
    }

    /// One overlapping tree entry of `calculateNeighbours` (`:117-169`).
    fn neighbour_entry_45(
        &mut self,
        calc: &mut Calc45,
        entry: ObjectEntry,
    ) -> Result<(), RoomError> {
        let object = entry.object;
        let index = entry.shape_index;
        if !calc.from_obstacle && !self.is_trace_obstacle_object(object) {
            return self.calculate_target_doors(calc.completed, object, index);
        }
        let current_shape = self.object_tree_shape(object, index).ok_or(NPE)?;
        let current_oct = current_shape.bounding_octagon().ok_or(NPE)?;
        let intersection = calc.room_oct.intersection_octagon(&current_oct);
        let dimension = intersection.dimension();
        if dimension > 1 && calc.from_obstacle {
            if let TreeObject::Item(item) = object
                && self.is_routable(item)
            {
                let overlap = self.obstacle_room(item, index)?;
                self.create_overlap_door(calc.completed, overlap)?;
            }
            return Ok(());
        }
        if dimension < 0 {
            // A corner of two diagonal lines with non-integer coordinates.
            return Ok(());
        }
        calc.add_neighbour(object, intersection);
        if dimension > 0 {
            let neighbour = match object {
                TreeObject::Room(id) => self.room_of_tree_id(id),
                TreeObject::Item(item) if self.is_routable(item) => {
                    self.obstacle_room(item, index)?
                }
                TreeObject::Item(_) => None,
            };
            if let Some(neighbour) = neighbour
                && self.insert_door_ok(
                    calc.completed,
                    neighbour,
                    &TileShape::IntOctagon(intersection),
                )
            {
                let door = self.new_door_of_shapes(calc.completed, neighbour)?;
                self.add_door_to(neighbour, door);
                self.add_door_to(calc.completed, door);
            }
        }
        Ok(())
    }

    /// Java `CompleteFreeSpaceExpansionRoom.calculateTargetDoors`
    /// (`CompleteFreeSpaceExpansionRoom.java:132-152`): the room overlaps an
    /// object of its own net, so it depends on the net; a connectable item
    /// of the net whose connection shape meets the room gets a target door.
    pub(super) fn calculate_target_doors(
        &mut self,
        room: RoomId,
        object: TreeObject,
        index: usize,
    ) -> Result<(), RoomError> {
        let Room::CompleteFree {
            net_dependent,
            shape,
            ..
        } = self.room_mut(room)
        else {
            return Err(NPE);
        };
        *net_dependent = true;
        let room_shape = shape.clone();
        let TreeObject::Item(item) = object else {
            return Ok(());
        };
        let Some(item_ref) = self.board.item(item) else {
            return Ok(());
        };
        if !item_ref.is_connectable_kind() || !item_ref.contains_net(self.net) {
            return Ok(());
        }
        let connection = self.board.trace_connection_shape(self.tree(), item, index);
        if let Some(connection) = connection
            && room_shape.intersects_tile(&connection)
        {
            let door = self.new_target_door(item, index, Some(room))?;
            if let Room::CompleteFree { target_doors, .. } = self.room_mut(room) {
                target_doors.push(door);
            }
        }
        Ok(())
    }

    /// Java `ObstacleExpansionRoom.createOverlapDoor` (`:77-102`): a
    /// 2-dimensional door between two overlapping obstacle rooms of
    /// routable items of the same net (of one trace only between
    /// consecutive segments). `other` missing is Java's null.
    pub(super) fn create_overlap_door(
        &mut self,
        this: RoomId,
        other: Option<RoomId>,
    ) -> Result<bool, RoomError> {
        let other = other.ok_or(RoomError::NullPointer {
            at: "ObstacleExpansionRoom.createOverlapDoor",
        })?;
        if self.door_exists(this, other) {
            return Ok(false);
        }
        let (
            Room::Obstacle {
                item: this_item,
                index: this_index,
                ..
            },
            Room::Obstacle {
                item: other_item,
                index: other_index,
                ..
            },
        ) = (self.room(this), self.room(other))
        else {
            return Err(RoomError::NullPointer {
                at: "ObstacleExpansionRoom.createOverlapDoor",
            });
        };
        let (this_item, this_index, other_item, other_index) =
            (*this_item, *this_index, *other_item, *other_index);
        let (Some(a), Some(b)) = (self.board.item(this_item), self.board.item(other_item)) else {
            return Ok(false);
        };
        if !(a.is_routable() && b.is_routable()) {
            return Ok(false);
        }
        if !a.shares_net(b) {
            return Ok(false);
        }
        if this_item == other_item {
            if !a.is_trace() {
                return Ok(false);
            }
            // Doors only between consecutive trace segments.
            if this_index != other_index + 1 && this_index + 1 != other_index {
                return Ok(false);
            }
        }
        let door = self.new_door(this, other, 2);
        self.add_door_to(this, door);
        self.add_door_to(other, door);
        Ok(true)
    }

    /// Java `SortedRoomNeighbours.insertDoorOk(room1, room2, doorShape)`
    /// (`:331-367`); `door_shape` has dimension 1.
    pub(crate) fn insert_door_ok(
        &self,
        room1: RoomId,
        room2: RoomId,
        door_shape: &TileShape,
    ) -> bool {
        if self.door_exists(room1, room2) {
            return false;
        }
        let item_of = |room: RoomId| match self.room(room) {
            Room::Obstacle { item, index, .. } => Some((*item, *index)),
            _ => None,
        };
        let (obstacle1, obstacle2) = (item_of(room1), item_of(room2));
        match (obstacle1, obstacle2) {
            (Some((a, _)), Some((b, _))) => {
                // Overlap doors only between items of one net.
                return match (self.board.item(a), self.board.item(b)) {
                    (Some(a), Some(b)) => a.shares_net(b),
                    _ => false,
                };
            }
            (None, None) => return true,
            _ => {}
        }
        // 1-dimensional doors of trace rooms only parallel to the trace.
        let mut door_line: Option<Line> = None;
        let mut prev_corner = door_shape.corner(0);
        for i in 1..door_shape.border_line_count() {
            let corner = door_shape.corner(i);
            if corner != prev_corner {
                door_line = Some(door_shape.border_line(i - 1));
                break;
            }
            prev_corner = corner;
        }
        if let Some(obstacle) = obstacle1
            && !self.insert_door_ok_at_trace(obstacle, door_line.as_ref())
        {
            return false;
        }
        if let Some(obstacle) = obstacle2 {
            return self.insert_door_ok_at_trace(obstacle, door_line.as_ref());
        }
        true
    }

    /// Java `SortedRoomNeighbours.insertDoorOk(room, doorLine)`
    /// (`:374-388`): at the first and last segment of a trace only doors
    /// parallel to the segment.
    pub(super) fn insert_door_ok_at_trace(
        &self,
        (item, index): (ItemId, usize),
        door_line: Option<&Line>,
    ) -> bool {
        let Some(door_line) = door_line else {
            return false;
        };
        let Some(trace) = self.board.item(item).and_then(|i| i.as_trace()) else {
            return true;
        };
        let last = trace.tile_shape_count() as i64 - 1;
        if index == 0 || index as i64 == last {
            let lines = trace.polyline.lines();
            return lines
                .get(index + 1)
                .is_some_and(|line| line.is_parallel(door_line));
        }
        true
    }

    /// Java `tryRemoveEdgeLine` (`:315-430`): when an edge of the room
    /// longer than one unit touches no neighbour, the room without the
    /// untouched edge lines is completed again, ignoring the largest
    /// 2-dimensional door to a complete free room. If that gives one room
    /// with a larger area, it replaces `room`'s shape and contained shape,
    /// and the calculation starts over.
    fn try_remove_edge_line(
        &mut self,
        room: &mut IncompleteRoom,
        calc: &Calc45,
    ) -> Result<bool, RoomError> {
        let Some(TileShape::IntOctagon(room_oct)) = &room.shape else {
            return Ok(false);
        };
        let room_oct = *room_oct;
        let room_area = room_oct.area();
        let try_remove = (0..8).any(|i| {
            !calc.edge_touch[i]
                && calc
                    .room_oct
                    .corner_approx(i)
                    .distance_square(calc.room_oct.corner_approx((i + 1) % 8))
                    > 1.0
        });
        if !try_remove {
            return Ok(false);
        }
        let enlarged = remove_not_touching_border_lines(&room_oct, &calc.edge_touch);
        let mut ignore_shape: Option<TileShape> = None;
        let mut ignore: Option<TreeObject> = None;
        let mut max_door_area = 0.0;
        for &door in self.room(calc.completed).doors() {
            if self.door(door).dimension != 2 {
                continue;
            }
            if let Some(other) = self.other_complete_room(door, calc.completed)
                && let Room::CompleteFree { id, .. } = self.room(other)
            {
                let door_shape = self.door_shape(door)?;
                let area = door_shape.area();
                if area > max_door_area {
                    max_door_area = area;
                    ignore_shape = Some(door_shape);
                    ignore = Some(TreeObject::Room(TreeRoomId(*id)));
                }
            }
        }
        let enlarged_room = IncompleteRoom::new(
            Some(TileShape::IntOctagon(enlarged)),
            room.layer,
            room.contained.clone(),
        );
        let new_rooms = self.complete_shape(&enlarged_room, ignore, ignore_shape.as_ref());
        if new_rooms.len() == 1 {
            let new_room = &new_rooms[0];
            let new_area = new_room.shape.as_ref().ok_or(NPE)?.area();
            if new_area > room_area {
                room.shape = new_room.shape.clone();
                room.contained = new_room.contained.clone();
                return Ok(true);
            }
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oct(lx: i32, ly: i32, rx: i32, uy: i32) -> IntOctagon {
        freerouting_geometry::IntBox::new(lx, ly, rx, uy).to_int_octagon()
    }

    /// A duplicate neighbour (equal under the comparator) is dropped from
    /// the set, but its constructor has marked the edges it touches
    /// (`Sorted45DegreeRoomNeighbours.java:882-905`).
    #[test]
    fn a_duplicate_neighbour_still_sets_the_edge_flags() {
        let room = oct(0, 0, 100, 100);
        let mut calc = Calc45 {
            completed: RoomId(0),
            from_obstacle: false,
            layer: 0,
            room_oct: room,
            sorted: Vec::new(),
            edge_touch: [false; 8],
        };
        // A neighbour below the room touching the bottom side from x = 10
        // to x = 40.
        let below = oct(10, -50, 40, 0);
        let touch = room.intersection_octagon(&below);
        calc.add_neighbour(TreeObject::Item(ItemId(7)), touch);
        assert_eq!(calc.sorted.len(), 1);
        assert_eq!(
            calc.edge_touch,
            [true, false, false, false, false, false, false, false]
        );
        // Clear the flags, then add an equal neighbour (same intersection,
        // same id): the set keeps one element, the flag is set again.
        calc.edge_touch = [false; 8];
        calc.add_neighbour(TreeObject::Item(ItemId(7)), touch);
        assert_eq!(calc.sorted.len(), 1, "the duplicate is not inserted");
        assert!(
            calc.edge_touch[0],
            "the duplicate's constructor set the flag"
        );
    }

    #[test]
    fn neighbours_sort_counterclockwise_by_touching_side_and_point() {
        let room = oct(0, 0, 100, 100);
        let mut calc = Calc45 {
            completed: RoomId(0),
            from_obstacle: false,
            layer: 0,
            room_oct: room,
            sorted: Vec::new(),
            edge_touch: [false; 8],
        };
        let mut add = |id: i32, shape: IntOctagon| {
            let touch = room.intersection_octagon(&shape);
            calc.add_neighbour(TreeObject::Item(ItemId(id)), touch);
        };
        add(1, oct(100, 20, 150, 40)); // right side (2)
        add(2, oct(60, -10, 80, 0)); // bottom side (0), further right
        add(3, oct(10, -10, 30, 0)); // bottom side (0)
        add(4, oct(-10, 50, 0, 70)); // left side (6)
        let order: Vec<i32> = calc
            .sorted
            .iter()
            .map(|n| object_java_id(n.object))
            .collect();
        assert_eq!(order, [3, 2, 1, 4]);
        assert_eq!(
            calc.sorted.iter().map(|n| n.first).collect::<Vec<_>>(),
            [0, 0, 2, 6]
        );
    }

    #[test]
    fn untouched_border_lines_move_to_the_limits() {
        let room = oct(0, 0, 100, 100);
        let mut touch = [false; 8];
        touch[0] = true;
        let enlarged = remove_not_touching_border_lines(&room, &touch);
        assert_eq!(enlarged.bottom_y, 0);
        assert_eq!(enlarged.top_y, CRIT_INT);
        assert_eq!(enlarged.left_x, -CRIT_INT);
    }
}
