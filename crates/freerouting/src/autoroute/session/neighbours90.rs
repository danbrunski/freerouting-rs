//! The 90 degree neighbour calculation of a room (A07.2).
//!
//! Port of `SortedOrthogonalRoomNeighbours.java` (727 lines) at
//! `a773a70ec`: `calculate` (`:43-77`),
//! `calculateIncompleteRoomsWithEmptyNeighbours` (`:79-112`),
//! `calculateNeighbours` (`:114-208`), `removeBorderLine` (`:210-221`),
//! `calculateNewIncompleteRooms` (`:223-400`), `insertIncompleteRoom`
//! (`:402-423`), `tryRemoveEdge` (`:425-584`) and the neighbour with its
//! comparator (`:597-725`). Rooms and tree shapes of the 90 degree tree are
//! boxes; Java gives up (returns no room) on any other shape.
//!
//! Orders that decide results:
//! - the tree's overlap order, then a **stable** sort by `getId()` and shape
//!   index (`:131-140`);
//! - the neighbour set, Java's `TreeSet` with an exact integer comparator
//!   and an id tie-break (`:673-725`), kept in a [`JavaTreeSet`]: a
//!   neighbour touching no side (Java's warning, side `-1`) makes the
//!   comparator inconsistent, and the set then still holds what Java's
//!   holds;
//! - the neighbour iteration of `calculateNewIncompleteRooms`, which decides
//!   the creation order of the incomplete rooms and their doors.
//!
//! Java bug kept for `legacy`: the room right of an obstacle room, after a
//! neighbour that ends on side 0, gets `roomShape.ur.x` as both x bounds
//! (`:296-301`), so it has width 0 and is never made.
//! `router.java_quirks.ortho_zero_width_room = false` reaches the board's right
//! edge, as cases 0 and 2 reach theirs (N06.5,
//! `right_of_obstacle_room_switch_both_behaviours`). With statistics on, the
//! rooms the fix would make count as `maze_ortho_zero_width_room_flips` in
//! either arm.

use std::cmp::Ordering;

use freerouting_geometry::{CRIT_INT, IntBox, PolylineShapeOps, TileShape, TileShapeOps};

use super::java_tree_set::JavaTreeSet;
use super::neighbours45::{object_java_id, sign};
use super::rooms::FromRoom;
use super::{Room, RoomError, RoomId, Session};
use crate::board::search::rooms::IncompleteRoom;
use crate::board::search::{ObjectEntry, RoomId as TreeRoomId, TreeObject};

const NPE: RoomError = RoomError::NullPointer {
    at: "SortedOrthogonalRoomNeighbours.calculate",
};

/// A neighbour of the room being completed (Java
/// `SortedOrthogonalRoomNeighbours.SortedRoomNeighbour`).
#[derive(Clone, Debug)]
pub(crate) struct Neighbour90 {
    pub object: TreeObject,
    /// The room's box intersected with the neighbour's box.
    pub intersection: IntBox,
    /// The first and last side of the room's box the neighbour touches;
    /// `-1` where Java warns "case not expected".
    pub first: i32,
    pub last: i32,
}

/// The state of one neighbour calculation.
pub(crate) struct Calc90 {
    pub completed: RoomId,
    pub from_obstacle: bool,
    pub layer: i32,
    /// The box of the completed room (`roomShape`).
    pub room_box: IntBox,
    /// `sortedNeighbours`, in set order.
    pub sorted: Vec<Neighbour90>,
    /// `edgeInteriorTouchesObstacle`.
    pub edge_touch: [bool; 4],
}

type NeighbourSet = JavaTreeSet<Neighbour90, fn(&Neighbour90, &Neighbour90) -> Ordering>;

impl Neighbour90 {
    /// The constructor (`:621-666`): marks the edges whose interior the
    /// neighbour touches, then finds the first and last touching sides.
    fn new(
        object: TreeObject,
        intersection: IntBox,
        room: &IntBox,
        edge_touch: &mut [bool; 4],
    ) -> Neighbour90 {
        let is = &intersection;
        // PARITY: SortedOrthogonalRoomNeighbours.java:626-645, a side effect
        // of the constructor, also for a neighbour the set then drops.
        if is.ll.y == room.ll.y && is.ur.x > room.ll.x && is.ll.x < room.ur.x {
            edge_touch[0] = true;
        }
        if is.ur.x == room.ur.x && is.ur.y > room.ll.y && is.ll.y < room.ur.y {
            edge_touch[1] = true;
        }
        if is.ur.y == room.ur.y && is.ur.x > room.ll.x && is.ll.x < room.ur.x {
            edge_touch[2] = true;
        }
        if is.ll.x == room.ll.x && is.ur.y > room.ll.y && is.ll.y < room.ur.y {
            edge_touch[3] = true;
        }
        let first = if is.ll.y == room.ll.y && is.ll.x > room.ll.x {
            0
        } else if is.ur.x == room.ur.x && is.ll.y > room.ll.y {
            1
        } else if is.ur.y == room.ur.y {
            2
        } else if is.ll.x == room.ll.x {
            3
        } else {
            -1
        };
        let last = if is.ll.x == room.ll.x && is.ll.y > room.ll.y {
            3
        } else if is.ur.y == room.ur.y && is.ll.x > room.ll.x {
            2
        } else if is.ur.x == room.ur.x {
            1
        } else if is.ll.y == room.ll.y {
            0
        } else {
            -1
        };
        Neighbour90 {
            object,
            intersection,
            first,
            last,
        }
    }

    /// The comparator (`:673-725`), `int` arithmetic; an out-of-range side
    /// compares equal, as Java's warning branches return 0.
    fn compare(&self, other: &Neighbour90) -> Ordering {
        match self.first.cmp(&other.first) {
            Ordering::Equal => {}
            unequal => return unequal,
        }
        let is1 = &self.intersection;
        let is2 = &other.intersection;
        let mut cmp = match self.first {
            0 => is1.ll.x.wrapping_sub(is2.ll.x),
            1 => is1.ll.y.wrapping_sub(is2.ll.y),
            2 => is2.ur.x.wrapping_sub(is1.ur.x),
            3 => is2.ur.y.wrapping_sub(is1.ur.y),
            _ => return Ordering::Equal,
        };
        if cmp == 0 {
            // Java's `%` of a non-negative value: the sides are -1..=3 and
            // `first` is 0..=3 here.
            let this_diff = (self.last - self.first + 4) % 4;
            let other_diff = (other.last - other.first + 4) % 4;
            match this_diff.cmp(&other_diff) {
                Ordering::Equal => {}
                unequal => return unequal,
            }
            cmp = match self.last {
                0 => is1.ur.x.wrapping_sub(is2.ur.x),
                1 => is1.ur.y.wrapping_sub(is2.ur.y),
                2 => is2.ll.x.wrapping_sub(is1.ll.x),
                3 => is2.ll.y.wrapping_sub(is1.ll.y),
                _ => return Ordering::Equal,
            };
        }
        if cmp == 0 {
            cmp = object_java_id(self.object).wrapping_sub(object_java_id(other.object));
        }
        sign(cmp)
    }
}

/// Java `removeBorderLine` (`:210-221`).
fn remove_border_line(room: &IntBox, edge: usize) -> IntBox {
    match edge {
        0 => IntBox::new(room.ll.x, -CRIT_INT, room.ur.x, room.ur.y),
        1 => IntBox::new(room.ll.x, room.ll.y, CRIT_INT, room.ur.y),
        2 => IntBox::new(room.ll.x, room.ll.y, room.ur.x, CRIT_INT),
        _ => IntBox::new(-CRIT_INT, room.ll.y, room.ur.x, room.ur.y),
    }
}

/// Java `insertIncompleteRoom`'s tests (`:402-423`): the box of `coords`
/// (`[ll.x, ll.y, ur.x, ur.y]`), its intersection with `room` and the door's
/// dimension, if the box is 2-dimensional and touches `room` along a line or more.
fn incomplete_room_90(room: &IntBox, coords: [i32; 4]) -> Option<(IntBox, IntBox, i32)> {
    let [ll_x, ll_y, ur_x, ur_y] = coords;
    let shape = IntBox::new(ll_x, ll_y, ur_x, ur_y);
    if TileShape::IntBox(shape).dimension() != 2 {
        return None;
    }
    let contained = room.intersection_box(&shape);
    if contained.is_empty() {
        return None;
    }
    let door_dimension = TileShape::IntBox(shape.intersection_box(room)).dimension();
    (door_dimension > 0).then_some((shape, contained, door_dimension))
}

/// The room right of obstacle room `room`, from its bottom up to the next
/// neighbour's intersection `next` (case 1 of `calculateNewIncompleteRooms`
/// after a neighbour ending on side 0, 2 or 3). `java` is
/// `router.java_quirks.ortho_zero_width_room`:
/// `SortedOrthogonalRoomNeighbours.java:296-301` passes `roomShape.ur.x` as both
/// x bounds, a box of width 0 that is never inserted; `false` reaches the board
/// box `board`'s right edge, as cases 0 and 2 reach theirs.
fn ortho_right_of_obstacle_room(
    room: &IntBox,
    board: &IntBox,
    next: &IntBox,
    java: bool,
) -> [i32; 4] {
    // PARITY(quirk, java_quirks.ortho_zero_width_room): `room.ur.x` twice.
    let right_x = if java { room.ur.x } else { board.ur.x };
    [room.ur.x, room.ll.y, right_x, next.ll.y]
}

impl Session<'_> {
    /// Java `SortedOrthogonalRoomNeighbours.calculate` (`:43-77`). `None`
    /// where Java returns null (a shape that is not a box).
    pub(crate) fn calculate_neighbours_90(
        &mut self,
        from: FromRoom,
    ) -> Result<Option<RoomId>, RoomError> {
        let mut from = from;
        loop {
            let room_id = self.generate_room_id();
            let Some(calc) = self.sorted_neighbours_90(&from, room_id)? else {
                return Ok(None);
            };
            if let FromRoom::Incomplete(room) = &mut from
                && self.try_remove_edge_90(room, &calc)?
            {
                // PARITY: SortedOrthogonalRoomNeighbours.java:59-62, the
                // doors go and the calculation starts over with a new id.
                self.remove_all_doors(calc.completed);
                continue;
            }
            if calc.sorted.is_empty() {
                if calc.from_obstacle {
                    self.empty_neighbour_rooms_90(&calc);
                }
            } else {
                self.new_incomplete_rooms_90(&calc);
            }
            return Ok(Some(calc.completed));
        }
    }

    /// Java `calculateNeighbours` (`:114-208`).
    fn sorted_neighbours_90(
        &mut self,
        from: &FromRoom,
        room_id: i32,
    ) -> Result<Option<Calc90>, RoomError> {
        let (room_box, layer, completed, from_obstacle) = match from {
            FromRoom::Incomplete(room) => {
                let Some(TileShape::IntBox(room_box)) = room.shape else {
                    // Java warns "IntBox expected for roomShape".
                    return Ok(None);
                };
                let completed = self.push_room(Room::CompleteFree {
                    id: room_id,
                    shape: TileShape::IntBox(room_box),
                    layer: room.layer,
                    doors: Vec::new(),
                    target_doors: Vec::new(),
                    net_dependent: false,
                });
                (room_box, room.layer, completed, false)
            }
            FromRoom::Obstacle(room) => {
                let r = self.room(*room);
                let Some(TileShape::IntBox(room_box)) = r.shape() else {
                    return Ok(None);
                };
                (*room_box, r.layer(), *room, true)
            }
        };
        let room_shape = TileShape::IntBox(room_box);
        let mut entries: Vec<ObjectEntry> = Vec::new();
        self.tree().overlapping_object_entries(
            &room_shape,
            layer,
            &[],
            &self.board.items,
            &mut entries,
        );
        // PARITY: SortedOrthogonalRoomNeighbours.java:131-140, a stable sort
        // by getId() and then shape index.
        entries.sort_by(|a, b| {
            let by_id = object_java_id(a.object).wrapping_sub(object_java_id(b.object));
            if by_id != 0 {
                sign(by_id)
            } else {
                a.shape_index.cmp(&b.shape_index)
            }
        });
        let mut set: NeighbourSet = JavaTreeSet::new(Neighbour90::compare);
        let mut edge_touch = [false; 4];
        for entry in entries {
            let ok = self.neighbour_entry_90(
                completed,
                from_obstacle,
                &room_box,
                entry,
                &mut set,
                &mut edge_touch,
            )?;
            if !ok {
                return Ok(None);
            }
        }
        Ok(Some(Calc90 {
            completed,
            from_obstacle,
            layer,
            room_box,
            sorted: set.into_sorted_vec(),
            edge_touch,
        }))
    }

    /// One overlapping tree entry of `calculateNeighbours` (`:144-206`);
    /// false where Java returns null.
    fn neighbour_entry_90(
        &mut self,
        completed: RoomId,
        from_obstacle: bool,
        room_box: &IntBox,
        entry: ObjectEntry,
        set: &mut NeighbourSet,
        edge_touch: &mut [bool; 4],
    ) -> Result<bool, RoomError> {
        let object = entry.object;
        let index = entry.shape_index;
        if !from_obstacle && !self.is_trace_obstacle_object(object) {
            self.calculate_target_doors(completed, object, index)?;
            return Ok(true);
        }
        let Some(TileShape::IntBox(current_box)) = self.object_tree_shape(object, index) else {
            // Java warns "IntBox expected for currentShape" and returns null.
            return Ok(false);
        };
        let intersection = room_box.intersection_box(&current_box);
        let dimension = TileShape::IntBox(intersection).dimension();
        if dimension > 1 && from_obstacle {
            if let TreeObject::Item(item) = object
                && self.is_routable(item)
            {
                let overlap = self.obstacle_room(item, index)?;
                self.create_overlap_door(completed, overlap)?;
            }
            return Ok(true);
        }
        if dimension < 0 {
            return Ok(true);
        }
        set.insert(Neighbour90::new(object, intersection, room_box, edge_touch));
        if dimension > 0 {
            let neighbour = match object {
                TreeObject::Room(id) => self.room_of_tree_id(id),
                TreeObject::Item(item) if self.is_routable(item) => {
                    self.obstacle_room(item, index)?
                }
                TreeObject::Item(_) => None,
            };
            if let Some(neighbour) = neighbour
                && self.insert_door_ok(completed, neighbour, &TileShape::IntBox(intersection))
            {
                let door = self.new_door_of_shapes(completed, neighbour)?;
                self.add_door_to(neighbour, door);
                self.add_door_to(completed, door);
            }
        }
        Ok(true)
    }

    /// Java `calculateIncompleteRoomsWithEmptyNeighbours` (`:79-112`): an
    /// incomplete room beyond each side of the obstacle room.
    fn empty_neighbour_rooms_90(&mut self, calc: &Calc90) {
        let b = self.board.bounding_box();
        let r = calc.room_box;
        for side in 0..4 {
            let new_box = match side {
                0 => IntBox::new(b.ll.x, b.ll.y, b.ur.x, r.ll.y),
                1 => IntBox::new(r.ur.x, b.ll.y, b.ur.x, b.ur.y),
                2 => IntBox::new(b.ll.x, r.ur.y, b.ur.x, b.ur.y),
                _ => IntBox::new(b.ll.x, b.ll.y, r.ll.x, b.ur.y),
            };
            let contained = r.intersection_box(&new_box);
            let new_room = self.add_incomplete(
                Some(TileShape::IntBox(new_box)),
                calc.layer,
                Some(TileShape::IntBox(contained)),
            );
            let door = self.new_door(calc.completed, new_room, 1);
            self.add_door_to(calc.completed, door);
            self.add_door_to(new_room, door);
        }
    }

    /// Java `insertIncompleteRoom` (`:402-423`).
    fn insert_incomplete_room_90(&mut self, calc: &Calc90, coords: [i32; 4]) {
        if let Some((shape, contained, door_dimension)) = incomplete_room_90(&calc.room_box, coords)
        {
            let new_room = self.add_incomplete(
                Some(TileShape::IntBox(shape)),
                calc.layer,
                Some(TileShape::IntBox(contained)),
            );
            let door = self.new_door(calc.completed, new_room, door_dimension);
            self.add_door_to(calc.completed, door);
            self.add_door_to(new_room, door);
        }
    }

    /// Java `calculateNewIncompleteRooms` (`:223-400`): an incomplete room
    /// with a door in each gap between consecutive neighbours.
    fn new_incomplete_rooms_90(&mut self, calc: &Calc90) {
        let b = self.board.bounding_box();
        let room = calc.room_box;
        let obstacle = calc.from_obstacle;
        let mut prev = calc.sorted.len() - 1;
        for next in 0..calc.sorted.len() {
            let p = &calc.sorted[prev];
            let n = &calc.sorted[next];
            let (pi, ni) = (p.intersection, n.intersection);
            let mut rooms: Vec<[i32; 4]> = Vec::new();
            if !ni.intersects_box(&pi) {
                match n.first {
                    0 => {
                        if p.last == 0 {
                            if pi.ur.x < ni.ll.x {
                                rooms.push([pi.ur.x, b.ll.y, ni.ll.x, room.ll.y]);
                            }
                        } else if pi.ll.y > room.ll.y || ni.ll.x > room.ll.x {
                            if obstacle {
                                if p.last == 3 {
                                    rooms.push([b.ll.x, room.ll.y, room.ll.x, pi.ll.y]);
                                }
                                rooms.push([room.ll.x, b.ll.y, ni.ll.x, room.ll.y]);
                            } else {
                                rooms.push([b.ll.x, b.ll.y, ni.ll.x, pi.ll.y]);
                            }
                        }
                    }
                    1 => {
                        if p.last == 1 {
                            if pi.ur.y < ni.ll.y {
                                rooms.push([room.ur.x, pi.ur.y, b.ur.x, ni.ll.y]);
                            }
                        } else if pi.ur.x < room.ur.x || ni.ll.y > room.ll.y {
                            if obstacle {
                                if p.last == 0 {
                                    rooms.push([pi.ur.x, b.ll.y, room.ur.x, room.ll.y]);
                                }
                                // PARITY(quirk, java_quirks.ortho_zero_width_room):
                                // SortedOrthogonalRoomNeighbours.java:296-301, a box of
                                // width 0 (room.ur.x twice), never inserted; `false`
                                // reaches the board's right edge, as cases 0 and 2 do
                                // (`ortho_right_of_obstacle_room`).
                                let fixed = [room.ur.x, room.ll.y, b.ur.x, ni.ll.y];
                                if crate::run_stats::enabled()
                                    && incomplete_room_90(&room, fixed).is_some()
                                {
                                    // Counted in either arm: Java's box has width 0
                                    // and is never inserted, the fixed one would be.
                                    crate::run_stats::record_n06(
                                        crate::run_stats::Counter::MazeOrthoZeroWidthRoomFlips,
                                    );
                                }
                                rooms.push(ortho_right_of_obstacle_room(
                                    &room,
                                    &b,
                                    &ni,
                                    self.control.quirks.ortho_zero_width_room,
                                ));
                            } else {
                                rooms.push([pi.ur.x, b.ll.y, b.ur.x, ni.ll.y]);
                            }
                        }
                    }
                    2 => {
                        if p.last == 2 {
                            if pi.ll.x > ni.ur.x {
                                rooms.push([ni.ur.x, room.ur.y, pi.ll.x, b.ur.y]);
                            }
                        } else if pi.ur.y < room.ur.y || ni.ur.x < room.ur.x {
                            if obstacle {
                                if p.last == 1 {
                                    rooms.push([room.ur.x, pi.ur.y, b.ur.x, room.ur.y]);
                                }
                                rooms.push([ni.ur.x, room.ur.y, room.ur.x, b.ur.y]);
                            } else {
                                rooms.push([ni.ur.x, pi.ur.y, b.ur.x, b.ur.y]);
                            }
                        }
                    }
                    3 => {
                        if p.last == 3 {
                            if pi.ll.y > ni.ur.y {
                                rooms.push([b.ll.x, ni.ur.y, room.ll.x, pi.ll.y]);
                            }
                        } else if ni.ur.y < room.ur.y || pi.ll.x > room.ll.x {
                            if obstacle {
                                if p.last == 2 {
                                    rooms.push([room.ll.x, room.ur.y, pi.ll.x, b.ur.y]);
                                }
                                rooms.push([b.ll.x, ni.ur.y, room.ll.x, room.ur.y]);
                            } else {
                                rooms.push([b.ll.x, ni.ur.y, pi.ll.x, b.ur.y]);
                            }
                        }
                    }
                    // Java warns "illegal touching side".
                    _ => {}
                }
            }
            for coords in rooms {
                self.insert_incomplete_room_90(calc, coords);
            }
            prev = next;
        }
    }

    /// Java `tryRemoveEdge` (`:425-584`): when an edge of the room touches
    /// no neighbour, the room without that edge line is completed again,
    /// ignoring the largest 2-dimensional door to a complete free room. If
    /// that gives one room with a larger area, it replaces `room`'s shape
    /// and contained shape, and the calculation starts over.
    fn try_remove_edge_90(
        &mut self,
        room: &mut IncompleteRoom,
        calc: &Calc90,
    ) -> Result<bool, RoomError> {
        let Some(TileShape::IntBox(room_box)) = &room.shape else {
            return Ok(false);
        };
        let room_box = *room_box;
        let room_area = TileShape::IntBox(room_box).area();
        let Some(edge) = (0..4).find(|&i| !calc.edge_touch[i]) else {
            return Ok(false);
        };
        let enlarged = remove_border_line(&room_box, edge);
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
            Some(TileShape::IntBox(enlarged)),
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
    use crate::board::items::ItemId;

    fn neighbour(id: i32, room: &IntBox, shape: IntBox, touch: &mut [bool; 4]) -> Neighbour90 {
        Neighbour90::new(
            TreeObject::Item(ItemId(id)),
            room.intersection_box(&shape),
            room,
            touch,
        )
    }

    /// `ortho_zero_width_room` (N06.5): the room right of an obstacle room, up
    /// to the next neighbour. Java's box has width 0 and is never made; `false`
    /// reaches the board's right edge and touches the room along its right side.
    #[test]
    fn right_of_obstacle_room_switch_both_behaviours() {
        let room = IntBox::new(100, 100, 200, 200);
        let board = IntBox::new(0, 0, 1000, 1000);
        let next = IntBox::new(200, 160, 200, 180);
        let java = ortho_right_of_obstacle_room(&room, &board, &next, true);
        assert_eq!(java, [200, 100, 200, 160]);
        assert!(incomplete_room_90(&room, java).is_none());
        let fixed = ortho_right_of_obstacle_room(&room, &board, &next, false);
        assert_eq!(fixed, [200, 100, 1000, 160]);
        let (shape, contained, door_dimension) =
            incomplete_room_90(&room, fixed).expect("the fixed room is made");
        assert_eq!(shape, IntBox::new(200, 100, 1000, 160));
        assert_eq!(contained, IntBox::new(200, 100, 200, 160));
        assert_eq!(door_dimension, 1);
        // A next neighbour at the room's bottom leaves no room in either arm.
        let low = IntBox::new(200, 100, 200, 120);
        for java in [true, false] {
            let coords = ortho_right_of_obstacle_room(&room, &board, &low, java);
            assert!(incomplete_room_90(&room, coords).is_none(), "{java}");
        }
    }

    #[test]
    fn neighbours_sort_counterclockwise_by_side_and_point() {
        let room = IntBox::new(0, 0, 100, 100);
        let mut touch = [false; 4];
        let mut set: NeighbourSet = JavaTreeSet::new(Neighbour90::compare);
        set.insert(neighbour(
            1,
            &room,
            IntBox::new(100, 20, 150, 40),
            &mut touch,
        ));
        set.insert(neighbour(2, &room, IntBox::new(60, -10, 80, 0), &mut touch));
        set.insert(neighbour(3, &room, IntBox::new(10, -10, 30, 0), &mut touch));
        set.insert(neighbour(4, &room, IntBox::new(-10, 50, 0, 70), &mut touch));
        let order: Vec<i32> = set.iter().map(|n| object_java_id(n.object)).collect();
        assert_eq!(order, [3, 2, 1, 4]);
        assert_eq!(touch, [true, true, false, true]);
    }

    /// A neighbour that only touches a corner gets no side for the start
    /// (Java warns), and compares equal to another such neighbour.
    #[test]
    fn a_corner_touch_has_no_first_side_and_compares_equal() {
        let room = IntBox::new(0, 0, 100, 100);
        let mut touch = [false; 4];
        let a = neighbour(1, &room, IntBox::new(-10, -10, 0, 0), &mut touch);
        assert_eq!((a.first, a.last), (3, 0));
        let inside = Neighbour90::new(
            TreeObject::Item(ItemId(2)),
            IntBox::new(10, 10, 20, 20),
            &room,
            &mut touch,
        );
        assert_eq!((inside.first, inside.last), (-1, -1));
        let other = Neighbour90 {
            object: TreeObject::Item(ItemId(3)),
            ..inside.clone()
        };
        assert_eq!(inside.compare(&other), Ordering::Equal);
        assert_eq!(touch, [false; 4]);
    }

    #[test]
    fn a_removed_border_line_moves_to_the_limit() {
        let room = IntBox::new(1, 2, 3, 4);
        assert_eq!(
            remove_border_line(&room, 0),
            IntBox::new(1, -CRIT_INT, 3, 4)
        );
        assert_eq!(remove_border_line(&room, 1), IntBox::new(1, 2, CRIT_INT, 4));
        assert_eq!(remove_border_line(&room, 2), IntBox::new(1, 2, 3, CRIT_INT));
        assert_eq!(
            remove_border_line(&room, 3),
            IntBox::new(-CRIT_INT, 2, 3, 4)
        );
    }
}
