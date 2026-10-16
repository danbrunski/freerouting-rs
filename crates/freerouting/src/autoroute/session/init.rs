//! The search initialisation (A02.6).
//!
//! Port of `MazeSearchEngine.getInstance` (`:147-161`), `init`
//! (`:991-1125`) and `reduceTraceShapesAtTiePins` (`:167-183`) at
//! `a773a70ec`, without `maintainDatabase` (off unless a system property
//! is set; the port always starts from a fresh session).
//!
//! In Java's order: the tie-pin reductions of the start items, then of the
//! destination items; the destination items join the destination distance
//! (each tree shape's bounding box on its layer); a fanout without
//! destination joins the board box on the first and last layer; the start
//! items get one incomplete room per tree shape, which must contain the
//! item's trace connection shape; the start rooms are completed; and every
//! target door of the completed rooms that leads to a start item enters the
//! maze queue at the centre of gravity of its connection shape inside the
//! room. The stop checks are at Java's four points (`:997, :1031, :1057,
//! :1073`).

use std::collections::BTreeSet;

use freerouting_geometry::{FloatLine, PolylineShapeOps};

use super::queue::{MazeListElement, QueueKey};
use super::{Adjustment, ExpandableRef, RoomError, RoomId, Session};
use crate::board::items::ItemId;

impl Session<'_> {
    /// Java `MazeSearchEngine.getInstance` with `init`: prepares the search
    /// from `start` to `destination` (both in Java's `TreeSet<Item>`
    /// order). False where Java's `init` returns false (no destination, no
    /// start door, or a stop request), and then `getInstance` gives null.
    pub fn init(
        &mut self,
        start: &BTreeSet<ItemId>,
        destination: &BTreeSet<ItemId>,
    ) -> Result<bool, RoomError> {
        self.reduce_trace_shapes_at_tie_pins(start);
        self.reduce_trace_shapes_at_tie_pins(destination);
        if !self.join_destinations(destination) {
            return Ok(false);
        }
        let Some(start_rooms) = self.start_rooms(start) else {
            return Ok(false);
        };
        let mut completed: Vec<RoomId> = Vec::new();
        for room in start_rooms {
            if self.is_stop_requested() {
                return Ok(false);
            }
            completed.extend(self.complete_expansion_room(room)?);
        }
        self.queue_start_doors(&completed)
    }

    /// `reduceTraceShapesAtTiePins` (`:167-183`): at every pin of several
    /// nets in `items`, the traces of other nets that end at it give up
    /// the pin's shape in the autoroute tree.
    fn reduce_trace_shapes_at_tie_pins(&mut self, items: &BTreeSet<ItemId>) {
        for &id in items {
            let Some(item) = self.board.item(id) else {
                continue;
            };
            if !item.is_pin() || item.net_count() <= 1 {
                continue;
            }
            for contact in self.board.normal_contacts(id) {
                let skip = self
                    .board
                    .item(contact)
                    .is_none_or(|c| !c.is_trace() || c.contains_net(self.net));
                if skip {
                    continue;
                }
                self.board
                    .reduce_trace_shape_at_tie_pin(self.tree_class, id, contact);
            }
        }
    }

    /// The destination items (`:995-1022`): start flag off, each tree shape
    /// joins the destination distance. A fanout without destination joins
    /// the board box on the first and last layer. False if there is no
    /// destination or a stop was requested.
    fn join_destinations(&mut self, destination: &BTreeSet<ItemId>) -> bool {
        let mut destination_ok = false;
        for &id in destination {
            if self.is_stop_requested() {
                return false;
            }
            self.item_info_mut(id).start = false;
            let boxes: Vec<_> = {
                let board = &*self.board;
                let Some(item) = board.item(id) else {
                    continue;
                };
                self.tree()
                    .tree_shapes(item, &board.shape_context())
                    .into_iter()
                    .enumerate()
                    .filter_map(|(i, shape)| shape.map(|s| (s.bounding_box(), item.shape_layer(i))))
                    .collect()
            };
            // PARITY: MazeSearchEngine.java:1003-1008, in tree shape order.
            for (bounds, layer) in boxes {
                self.distance.join(&bounds, layer);
            }
            destination_ok = true;
        }
        if !destination_ok && self.control.is_fanout {
            let board_box = self.board.bounding_box();
            self.distance.join(&board_box, 0);
            self.distance
                .join(&board_box, self.control.layer_count as i32 - 1);
            destination_ok = true;
        }
        destination_ok
    }

    /// The start items (`:1035-1051`): start flag on, and for a connectable
    /// item one incomplete room per tree shape on the whole plane that must
    /// contain the shape's trace connection shape. `None` on a stop.
    fn start_rooms(&mut self, start: &BTreeSet<ItemId>) -> Option<Vec<RoomId>> {
        let mut rooms = Vec::new();
        for &id in start {
            if self.is_stop_requested() {
                return None;
            }
            self.item_info_mut(id).start = true;
            let shapes: Vec<_> = {
                let board = &*self.board;
                let Some(item) = board.item(id) else {
                    continue;
                };
                if !item.is_connectable_kind() {
                    continue;
                }
                let tree = self.tree();
                let count = tree.tree_shape_count(item, &board.shape_context());
                (0..count)
                    .map(|i| {
                        (
                            board.trace_connection_shape(tree, id, i),
                            item.shape_layer(i),
                        )
                    })
                    .collect()
            };
            for (contained, layer) in shapes {
                rooms.push(self.add_incomplete(None, layer, contained));
            }
        }
        Some(rooms)
    }

    /// Queues the target doors of the completed start rooms that lead to
    /// start items (`:1066-1110`). True if one was queued.
    fn queue_start_doors(&mut self, completed: &[RoomId]) -> Result<bool, RoomError> {
        let mut start_ok = false;
        for &room in completed {
            let layer = self.room(room).layer();
            let doors = self.room(room).target_doors().to_vec();
            for door in doors {
                if self.is_stop_requested() {
                    return Ok(false);
                }
                if self.is_destination_door(door) {
                    continue;
                }
                let npe = RoomError::NullPointer {
                    at: "MazeSearchEngine.init",
                };
                let target = self.target_door(door);
                let door_room = target.room.ok_or(npe.clone())?;
                let connection = self
                    .board
                    .trace_connection_shape(self.tree(), target.item, target.tree_entry)
                    .ok_or(npe.clone())?;
                let room_shape = self.room(door_room).shape().ok_or(npe)?;
                let center = connection.intersection(room_shape).centre_of_gravity();
                let sorting_value = self.distance.calculate_point(&center, layer);
                let key = QueueKey::new(sorting_value, 0.0, self.target_door_java_id(door)?, 0);
                let element = MazeListElement {
                    door: ExpandableRef::TargetDoor(door),
                    section: 0,
                    backtrack: None,
                    expansion_value: 0.0,
                    sorting_value,
                    next_room: Some(room),
                    shape_entry: FloatLine::new(center, center),
                    room_ripped: false,
                    adjustment: Adjustment::None,
                    already_checked: false,
                    ripup_cost: 0,
                };
                // A04.2: through the fanout filter, as Java's overridden add.
                self.queue_element(key, element);
                start_ok = true;
            }
        }
        Ok(start_ok)
    }
}
