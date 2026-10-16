//! The room database of a session (A02.1, A02.2).
//!
//! Port of `AutorouteEngine.java:321-700` at `a773a70ec`: the incomplete and
//! complete room lists, room completion, room removal with its new
//! incomplete rooms, the door cascade of `removeAllDoors` and `clear`, plus
//! `ItemAutorouteInfo.getExpansionRoom` (`ItemAutorouteInfo.java:54-82`).
//!
//! The orders that decide results are Java's: the lists append and remove
//! by identity, complete rooms enter the autoroute tree when they are added
//! to the list and leave it in list order on `clear`, and the room id
//! counter advances on every neighbour calculation.

use freerouting_geometry::{PolylineShapeOps, Simplex, TileShape, TileShapeOps};

use super::{CalculationMode, DoorId, ItemAutorouteInfo, Room, RoomError, RoomId, Session, TreeOp};
use crate::board::items::ItemId;
use crate::board::search::rooms::IncompleteRoom;
use crate::board::search::{RoomId as TreeRoomId, TreeObject};

/// Where a neighbour calculation starts: a free room whose shape was just
/// completed (Java's incomplete room handed to `addCompleteRoom`, which is
/// in no list), or an obstacle room of the arena.
#[derive(Clone, Debug)]
pub enum FromRoom {
    Incomplete(IncompleteRoom),
    Obstacle(RoomId),
}

impl Session<'_> {
    /// Java `addIncompleteExpansionRoom`: a new incomplete room, appended
    /// to the list.
    pub fn add_incomplete(
        &mut self,
        shape: Option<TileShape>,
        layer: i32,
        contained: Option<TileShape>,
    ) -> RoomId {
        let room = self.push_room(Room::Incomplete {
            shape,
            contained,
            layer,
            doors: Vec::new(),
        });
        // PARITY: ArrayList.add, AutorouteEngine.java:362.
        self.incomplete.push(room);
        room
    }

    /// Java `getFirstIncompleteExpansionRoom`.
    pub fn first_incomplete(&self) -> Option<RoomId> {
        self.incomplete.first().copied()
    }

    /// Java `removeIncompleteExpansionRoom`: removes the room's doors, then
    /// the room from the list (by identity, the first occurrence).
    pub fn remove_incomplete(&mut self, room: RoomId) {
        self.remove_all_doors(room);
        if let Some(index) = self.incomplete.iter().position(|r| *r == room) {
            self.incomplete.remove(index);
        }
    }

    /// Java `removeAllDoors` (`:627-642`): every door of `room` leaves the
    /// other room's list, and an incomplete other room is removed too (with
    /// its doors). Then `room` has no doors (and a complete free room no
    /// target doors).
    pub fn remove_all_doors(&mut self, room: RoomId) {
        // Java iterates the list itself; nothing in the loop changes it
        // (the other room's door to `room` is gone before it is removed).
        let doors: Vec<DoorId> = self.room(room).doors().to_vec();
        for door in doors {
            let Some(other) = self.other_room(door, room) else {
                continue;
            };
            self.remove_door_from(other, door);
            if matches!(self.room(other), Room::Incomplete { .. }) {
                self.remove_incomplete(other);
            }
        }
        match self.room_mut(room) {
            Room::CompleteFree {
                doors,
                target_doors,
                ..
            } => {
                doors.clear();
                target_doors.clear();
            }
            other => other.doors_mut().clear(),
        }
    }

    /// Java `removeCompleteExpansionRoom` (`:391-430`): each neighbour that
    /// touches the room along a line gets a new incomplete room on that
    /// side (the half plane opposite its touching border line, containing
    /// the touching segment); then the room's doors go, the room leaves the
    /// tree and the list, and the drill pages over its shape are
    /// invalidated.
    pub fn remove_complete(&mut self, room: RoomId) -> Result<(), RoomError> {
        let (room_shape, room_layer) = match self.room(room) {
            Room::CompleteFree { shape, layer, .. } => (shape.clone(), *layer),
            _ => {
                return Err(RoomError::NullPointer {
                    at: "AutorouteEngine.removeCompleteExpansionRoom",
                });
            }
        };
        let doors: Vec<DoorId> = self.room(room).doors().to_vec();
        for door in doors {
            let Some(neighbour) = self.other_room(door, room) else {
                continue;
            };
            self.remove_door_from(neighbour, door);
            let neighbour_shape =
                self.room(neighbour)
                    .shape()
                    .cloned()
                    .ok_or(RoomError::NullPointer {
                        at: "AutorouteEngine.removeCompleteExpansionRoom",
                    })?;
            let intersection = room_shape.intersection(&neighbour_shape);
            if intersection.dimension() == 1 {
                let (_, neighbour_side) = room_shape.touching_sides(&neighbour_shape).ok_or(
                    RoomError::IndexOutOfBounds {
                        at: "AutorouteEngine.removeCompleteExpansionRoom",
                    },
                )?;
                let line = neighbour_shape.border_line(neighbour_side).opposite();
                let shape = TileShape::Simplex(Simplex::get_instance(&[line]));
                let new_room = self.add_incomplete(Some(shape), room_layer, Some(intersection));
                let new_door = self.new_door(neighbour, new_room, 1);
                self.add_door_to(neighbour, new_door);
                self.add_door_to(new_room, new_door);
            }
        }
        self.remove_all_doors(room);
        self.remove_from_tree(room);
        if let Some(index) = self.complete.iter().position(|r| *r == room) {
            self.complete.remove(index);
        }
        self.invalidate_drill_pages(&room_shape);
        Ok(())
    }

    /// Java `invalidateDrillPages`: the drill pages overlapping `shape`
    /// must be recalculated (A03). Nothing to do before the page array is
    /// built: its pages have no drills yet.
    pub fn invalidate_drill_pages(&mut self, shape: &TileShape) {
        if let Some(pages) = self.drill_pages.as_mut() {
            pages.invalidate(shape);
        }
    }

    /// Java `completeExpansionRoom` (`:432-545`): completes the shape of
    /// the incomplete room `room` and returns the new complete rooms. The
    /// first door to a complete free room with a 2-dimensional overlap
    /// gives the room to ignore and its overlap. `room` is removed. The
    /// first 2-dimensional candidate is added as it is; every later one is
    /// completed again first, because the first room may now overlap it.
    ///
    /// Java catches every exception here and returns no rooms, keeping what
    /// was changed before; so does this, but for
    /// [`RoomError::UnsupportedAngle`], which has no Java counterpart.
    pub fn complete_expansion_room(&mut self, room: RoomId) -> Result<Vec<RoomId>, RoomError> {
        match self.try_complete_expansion_room(room) {
            Ok(rooms) => Ok(rooms),
            Err(e @ RoomError::UnsupportedAngle(_)) => Err(e),
            Err(_) => Ok(Vec::new()),
        }
    }

    fn try_complete_expansion_room(&mut self, room: RoomId) -> Result<Vec<RoomId>, RoomError> {
        let (shape, contained, layer) = match self.room(room) {
            Room::Incomplete {
                shape,
                contained,
                layer,
                ..
            } => (shape.clone(), contained.clone(), *layer),
            _ => {
                return Err(RoomError::NullPointer {
                    at: "AutorouteEngine.completeExpansionRoom",
                });
            }
        };
        let mut from_door_shape: Option<TileShape> = None;
        let mut ignore: Option<TreeObject> = None;
        for &door in self.room(room).doors() {
            let Some(other) = self.other_room(door, room) else {
                continue;
            };
            if let Room::CompleteFree { id, .. } = self.room(other)
                && self.door(door).dimension == 2
            {
                from_door_shape = Some(self.door_shape(door)?);
                ignore = Some(TreeObject::Room(TreeRoomId(*id)));
                break;
            }
        }
        let start = IncompleteRoom::new(shape, layer, contained);
        let candidates = self.complete_shape(&start, ignore, from_door_shape.as_ref());
        self.remove_incomplete(room);
        let mut result = Vec::new();
        let mut first = true;
        for candidate in candidates {
            if shape_dimension(&candidate.shape) != 2 {
                continue;
            }
            if first {
                first = false;
                if let Some(completed) = self.add_complete_room(candidate)? {
                    result.push(completed);
                }
            } else {
                // The first completed room may intersect this candidate now.
                let again = self.complete_shape(&candidate, ignore, from_door_shape.as_ref());
                for candidate in again {
                    if let Some(completed) = self.add_complete_room(candidate)? {
                        result.push(completed);
                    }
                }
            }
        }
        Ok(result)
    }

    /// `autorouteSearchTree.completeShape(room, netNumber, ignore,
    /// ignoreShape)` on the session's tree.
    pub(crate) fn complete_shape(
        &mut self,
        room: &IncompleteRoom,
        ignore: Option<TreeObject>,
        ignore_shape: Option<&TileShape>,
    ) -> Vec<IncompleteRoom> {
        self.board.complete_shape(
            self.tree_class,
            room,
            self.net,
            ignore,
            ignore_shape,
            &mut self.scratch,
        )
    }

    /// Java `addCompleteRoom` (`:547-569`): calculates the doors of the
    /// completed shape and, if the room is 2-dimensional, appends it to the
    /// list and inserts it into the tree.
    fn add_complete_room(&mut self, room: IncompleteRoom) -> Result<Option<RoomId>, RoomError> {
        let Some(completed) = self.calculate_doors(FromRoom::Incomplete(room))? else {
            return Ok(None);
        };
        let Room::CompleteFree {
            id, shape, layer, ..
        } = self.room(completed)
        else {
            return Ok(None);
        };
        if shape.dimension() != 2 {
            return Ok(None);
        }
        let (id, shape, layer) = (*id, shape.clone(), *layer);
        // PARITY: list order, then the tree insert (AutorouteEngine.java:556-557).
        self.complete.push(completed);
        self.by_java_id.insert(id, completed);
        self.board
            .insert_room(self.tree_class, TreeRoomId(id), shape, layer);
        self.tree_ops.push(TreeOp::Insert(id));
        Ok(Some(completed))
    }

    /// Java `calculateDoors` (`SortedRoomNeighbours.complete`): the
    /// neighbour calculation of the tree kind. Returns the completed room:
    /// a new complete free room, or the obstacle room itself.
    pub(crate) fn calculate_doors(&mut self, from: FromRoom) -> Result<Option<RoomId>, RoomError> {
        match self.mode {
            CalculationMode::Degree45 => self.calculate_neighbours_45(from),
            mode => Err(RoomError::UnsupportedAngle(mode)),
        }
    }

    /// Java `completeNeighbourRooms` (`:591-620`): completes the incomplete
    /// neighbours of `room`, restarting the door iteration after each
    /// completion, and calculates the doors of neighbouring obstacle rooms
    /// once.
    pub fn complete_neighbour_rooms(&mut self, room: RoomId) -> Result<(), RoomError> {
        let mut index = 0;
        while let Some(&door) = self.room(room).doors().get(index) {
            index += 1;
            let Some(neighbour) = self.other_room(door, room) else {
                continue;
            };
            match self.room(neighbour) {
                Room::Incomplete { .. } => {
                    self.complete_expansion_room(neighbour)?;
                    // PARITY: AutorouteEngine.java:608 restarts the iteration.
                    index = 0;
                }
                Room::Obstacle {
                    doors_calculated: false,
                    ..
                } => {
                    self.calculate_doors(FromRoom::Obstacle(neighbour))?;
                    if let Room::Obstacle {
                        doors_calculated, ..
                    } = self.room_mut(neighbour)
                    {
                        *doors_calculated = true;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Removes a complete free room from the tree
    /// (`CompleteFreeSpaceExpansionRoom.removeFromTree`).
    fn remove_from_tree(&mut self, room: RoomId) {
        if let Room::CompleteFree { id, .. } = self.room(room) {
            let id = *id;
            self.board.remove_room(self.tree_class, TreeRoomId(id));
            self.tree_ops.push(TreeOp::Remove(id));
        }
    }

    /// Java `clear` (`:321-332`): every complete room leaves the tree, in
    /// list order; both lists are dropped, the counter is reset and the
    /// items' autoroute data are cleared. The arenas are emptied too.
    pub fn clear(&mut self) {
        let complete = std::mem::take(&mut self.complete);
        for room in complete {
            // PARITY: list order, AutorouteEngine.java:323-325.
            self.remove_from_tree(room);
        }
        self.incomplete.clear();
        self.room_count = 0;
        self.item_info.clear();
        self.rooms.clear();
        self.doors.clear();
        self.target_doors.clear();
        self.sections.clear();
        self.by_java_id.clear();
        self.queue.clear();
        self.clear_drills();
    }

    /// Java `ItemAutorouteInfo.getExpansionRoom(index, tree)`
    /// (`ItemAutorouteInfo.java:54-82`): the obstacle room of tree shape
    /// `index` of `item`, created on first use. The per-item array follows
    /// the item's current tree shape count, keeping the rooms it has. `None`
    /// for an index out of range (Java warns and returns null).
    pub fn obstacle_room(
        &mut self,
        item: ItemId,
        index: usize,
    ) -> Result<Option<RoomId>, RoomError> {
        let npe = RoomError::NullPointer {
            at: "ItemAutorouteInfo.getExpansionRoom",
        };
        let (count, shape, layer) = {
            let board = &*self.board;
            let tree = self.tree();
            let item_ref = board.item(item).ok_or(npe.clone())?;
            let ctx = board.shape_context();
            let count = tree.tree_shape_count(item_ref, &ctx);
            let shape = tree.tree_shape(item_ref, index, &ctx);
            (count, shape, item_ref.shape_layer(index))
        };
        let info: &mut ItemAutorouteInfo = self.item_info_mut(item);
        let rooms = info.obstacle_rooms.get_or_insert_with(Vec::new);
        // PARITY: a changed count resizes the array and keeps the rooms
        // below the new count (ItemAutorouteInfo.java:57-66).
        rooms.resize(count, None);
        if index >= count {
            return Ok(None);
        }
        if let Some(room) = rooms[index] {
            return Ok(Some(room));
        }
        let shape = shape.ok_or(npe)?;
        let room = self.push_room(Room::Obstacle {
            item,
            index,
            shape,
            layer,
            doors: Vec::new(),
            doors_calculated: false,
        });
        if let Some(rooms) = self.item_info_mut(item).obstacle_rooms.as_mut() {
            rooms[index] = Some(room);
        }
        Ok(Some(room))
    }
}

/// The dimension of an optional shape; -1 for none.
pub(crate) fn shape_dimension(shape: &Option<TileShape>) -> i32 {
    shape.as_ref().map_or(-1, |s| s.dimension())
}
