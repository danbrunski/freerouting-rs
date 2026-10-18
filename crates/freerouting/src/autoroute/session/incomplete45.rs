//! The new incomplete rooms of the 45 degree neighbour calculation
//! (A02.4).
//!
//! Port of `Sorted45DegreeRoomNeighbours.calculateNewIncompleteRooms`
//! (`:610-788`), the obstacle-room variants (`:254-309`, `:474-608`) and
//! `insertIncompleteRoom` (`:437-472`) at `a773a70ec`. The neighbours are
//! visited in comparator order, which decides the creation order of the
//! incomplete rooms and their doors.

use freerouting_geometry::{IntOctagon, PolylineShapeOps, TileShape};

use super::Session;
use super::neighbours45::Calc45;

impl Session<'_> {
    /// The board's bounding octagon.
    fn board_octagon(&self) -> IntOctagon {
        self.board.bounding_box().to_int_octagon()
    }

    /// Java `calculateEdgeIncompleteRoomsOfObstacleExpansionRoom`
    /// (`:254-309`): an incomplete room beyond each side of the obstacle
    /// room from `from_side` to `to_side`.
    pub(super) fn edge_incomplete_rooms_of_obstacle(
        &mut self,
        calc: &Calc45,
        from_side: usize,
        to_side: usize,
    ) {
        let board = self.board_octagon();
        let room = &calc.room_oct;
        // PARITY(quirk, java_quirks.edge_corner_not_advanced):
        // Sorted45DegreeRoomNeighbours.java:263, the corner compared with each
        // next corner is never advanced, so a side is skipped only when its end
        // corner equals the first side's start. `false` advances it, so every
        // side of length 0 is skipped.
        let advance_corner = !self.control.quirks.edge_corner_not_advanced;
        let mut current_corner = room.int_corner(from_side);
        let mut side = from_side;
        loop {
            let next = (side + 1) % 8;
            if current_corner != room.int_corner(next) {
                let mut o = board;
                match side {
                    0 => o.top_y = room.bottom_y,
                    1 => o.upper_left_diagonal_x = room.lower_right_diagonal_x,
                    2 => o.left_x = room.right_x,
                    3 => o.lower_left_diagonal_x = room.upper_right_diagonal_x,
                    4 => o.bottom_y = room.top_y,
                    5 => o.lower_right_diagonal_x = room.upper_left_diagonal_x,
                    6 => o.right_x = room.left_x,
                    _ => o.upper_right_diagonal_x = room.lower_left_diagonal_x,
                }
                self.insert_incomplete_room_45(calc, o);
            }
            if side == to_side {
                break;
            }
            if advance_corner {
                current_corner = room.int_corner(next);
            }
            side = next;
        }
    }

    /// Java `insertIncompleteRoom` (`:437-472`): an incomplete room of the
    /// normalized octagon if it is 2-dimensional and touches the room, with
    /// a door of the touch's dimension.
    fn insert_incomplete_room_45(&mut self, calc: &Calc45, octagon: IntOctagon) {
        let shape = octagon.normalize();
        if shape.dimension() != 2 {
            return;
        }
        let contained = calc.room_oct.intersection_octagon(&shape);
        if contained.is_empty() {
            return;
        }
        let door_dimension = contained.dimension();
        if door_dimension > 0 {
            let new_room = self.add_incomplete(
                Some(TileShape::IntOctagon(shape)),
                calc.layer,
                Some(TileShape::IntOctagon(contained)),
            );
            let door = self.new_door(calc.completed, new_room, door_dimension);
            self.add_door_to(calc.completed, door);
            self.add_door_to(new_room, door);
        }
    }

    /// Java `calculateNewIncompleteRoomsForObstacleExpansionRoom`
    /// (`:474-608`).
    fn new_incomplete_rooms_of_obstacle(&mut self, calc: &Calc45, prev: usize, next: usize) {
        let from_side = calc.sorted[prev].last;
        let to_side = calc.sorted[next].first;
        if from_side == to_side && prev != next {
            return;
        }
        let board = self.board_octagon();
        let room = calc.room_oct;
        let p = calc.sorted[prev].intersection;
        let mut o = board;
        match from_side {
            0 => {
                o.top_y = room.bottom_y;
                o.upper_left_diagonal_x = p.lower_right_diagonal_x;
            }
            1 => {
                o.upper_left_diagonal_x = room.lower_right_diagonal_x;
                o.left_x = p.right_x;
            }
            2 => {
                o.left_x = room.right_x;
                o.lower_left_diagonal_x = p.upper_right_diagonal_x;
            }
            3 => {
                o.lower_left_diagonal_x = room.upper_right_diagonal_x;
                o.bottom_y = p.top_y;
            }
            4 => {
                o.bottom_y = room.top_y;
                o.lower_right_diagonal_x = p.upper_left_diagonal_x;
            }
            5 => {
                o.lower_right_diagonal_x = room.upper_left_diagonal_x;
                o.right_x = p.left_x;
            }
            6 => {
                o.right_x = room.left_x;
                o.upper_right_diagonal_x = p.lower_left_diagonal_x;
            }
            _ => {
                o.upper_right_diagonal_x = room.lower_left_diagonal_x;
                o.top_y = p.bottom_y;
            }
        }
        self.insert_incomplete_room_45(calc, o);

        let n = calc.sorted[next].intersection;
        let mut o = board;
        match to_side {
            0 => {
                o.top_y = room.bottom_y;
                o.upper_right_diagonal_x = n.lower_left_diagonal_x;
            }
            1 => {
                o.upper_left_diagonal_x = room.lower_right_diagonal_x;
                o.top_y = n.bottom_y;
            }
            2 => {
                o.left_x = room.right_x;
                o.upper_left_diagonal_x = n.lower_right_diagonal_x;
            }
            3 => {
                o.lower_left_diagonal_x = room.upper_right_diagonal_x;
                o.left_x = n.right_x;
            }
            4 => {
                o.bottom_y = room.top_y;
                o.lower_left_diagonal_x = n.upper_right_diagonal_x;
            }
            5 => {
                o.lower_right_diagonal_x = room.upper_left_diagonal_x;
                o.bottom_y = n.top_y;
            }
            6 => {
                o.right_x = room.left_x;
                o.lower_right_diagonal_x = n.upper_left_diagonal_x;
            }
            _ => {
                o.upper_right_diagonal_x = room.lower_left_diagonal_x;
                o.right_x = n.left_x;
            }
        }
        self.insert_incomplete_room_45(calc, o);

        // The incomplete rooms on the free sides in between.
        let from = (from_side + 1) % 8;
        if from == to_side {
            return;
        }
        let to = (to_side + 7) % 8;
        self.edge_incomplete_rooms_of_obstacle(calc, from, to);
    }

    /// Java `calculateNewIncompleteRooms` (`:610-788`): an incomplete room
    /// with a door in each gap between consecutive neighbours.
    pub(super) fn new_incomplete_rooms_45(&mut self, calc: &Calc45) {
        let count = calc.sorted.len();
        let mut prev = count - 1;
        if calc.from_obstacle && count == 1 {
            self.new_incomplete_rooms_of_obstacle(calc, prev, prev);
            return;
        }
        let board = self.board_octagon();
        for next in 0..count {
            let p_side = calc.sorted[prev].last;
            let n_side = calc.sorted[next].first;
            let p = calc.sorted[prev].intersection;
            let n = calc.sorted[next].intersection;
            let touch = n.intersection_octagon(&p);
            let insert = if touch.is_empty() {
                true
            } else if touch.dimension() >= 1 {
                false
            } else if p_side == n_side {
                // A point contact at a corner of the room.
                false
            } else {
                p_side != (n_side + 1) % 8
            };
            if insert {
                if calc.from_obstacle && n_side != p_side {
                    self.new_incomplete_rooms_of_obstacle(calc, prev, next);
                } else {
                    let o = gap_octagon(board, n_side, p_side, &p, &n);
                    self.insert_incomplete_room_45(calc, o);
                }
            }
            prev = next;
        }
    }
}

/// The octagon of the incomplete room between neighbours with
/// intersections `p` (last side `p_side`) and `n` (first side `n_side`)
/// (`Sorted45DegreeRoomNeighbours.java:648-781`).
fn gap_octagon(
    board: IntOctagon,
    n_side: usize,
    p_side: usize,
    p: &IntOctagon,
    n: &IntOctagon,
) -> IntOctagon {
    let mut o = board;
    match n_side {
        0 => {
            if p.lower_left_diagonal_x < n.lower_left_diagonal_x {
                o.upper_right_diagonal_x = n.lower_left_diagonal_x;
                o.top_y = p.bottom_y;
                if p_side == 0 {
                    o.upper_left_diagonal_x = p.lower_right_diagonal_x;
                }
            } else if p.lower_left_diagonal_x > n.lower_left_diagonal_x {
                o.right_x = n.left_x;
                o.upper_right_diagonal_x = p.lower_left_diagonal_x;
            } else {
                o.upper_right_diagonal_x = n.lower_left_diagonal_x;
            }
        }
        1 => {
            if p.bottom_y < n.bottom_y {
                o.top_y = n.bottom_y;
                o.upper_left_diagonal_x = p.lower_right_diagonal_x;
                if p_side == 1 {
                    o.left_x = p.right_x;
                }
            } else if p.bottom_y > n.bottom_y {
                o.top_y = p.bottom_y;
                o.upper_right_diagonal_x = n.lower_left_diagonal_x;
            } else {
                o.top_y = n.bottom_y;
            }
        }
        2 => {
            if p.lower_right_diagonal_x > n.lower_right_diagonal_x {
                o.upper_left_diagonal_x = n.lower_right_diagonal_x;
                o.left_x = p.right_x;
                if p_side == 2 {
                    o.lower_left_diagonal_x = p.upper_right_diagonal_x;
                }
            } else if p.lower_right_diagonal_x < n.lower_right_diagonal_x {
                o.top_y = n.bottom_y;
                o.upper_left_diagonal_x = p.lower_right_diagonal_x;
            } else {
                o.upper_left_diagonal_x = n.lower_right_diagonal_x;
            }
        }
        3 => {
            if p.right_x > n.right_x {
                o.left_x = n.right_x;
                o.lower_left_diagonal_x = p.upper_right_diagonal_x;
                if p_side == 3 {
                    o.bottom_y = p.top_y;
                }
            } else if p.right_x < n.right_x {
                o.left_x = p.right_x;
                o.upper_left_diagonal_x = n.lower_right_diagonal_x;
            } else {
                o.left_x = n.right_x;
            }
        }
        4 => {
            if p.upper_right_diagonal_x > n.upper_right_diagonal_x {
                o.lower_left_diagonal_x = n.upper_right_diagonal_x;
                o.bottom_y = p.top_y;
                if p_side == 4 {
                    o.lower_right_diagonal_x = p.upper_left_diagonal_x;
                }
            } else if p.upper_right_diagonal_x < n.upper_right_diagonal_x {
                o.left_x = n.right_x;
                o.lower_left_diagonal_x = p.upper_right_diagonal_x;
            } else {
                o.lower_left_diagonal_x = n.upper_right_diagonal_x;
            }
        }
        5 => {
            if p.top_y > n.top_y {
                o.bottom_y = n.top_y;
                o.lower_right_diagonal_x = p.upper_left_diagonal_x;
                if p_side == 5 {
                    o.right_x = p.left_x;
                }
            } else if p.top_y < n.top_y {
                o.bottom_y = p.top_y;
                o.lower_left_diagonal_x = n.upper_right_diagonal_x;
            } else {
                o.bottom_y = n.top_y;
            }
        }
        6 => {
            if p.upper_left_diagonal_x < n.upper_left_diagonal_x {
                o.lower_right_diagonal_x = n.upper_left_diagonal_x;
                o.right_x = p.left_x;
                if p_side == 6 {
                    o.upper_right_diagonal_x = p.lower_left_diagonal_x;
                }
            } else if p.upper_left_diagonal_x > n.upper_left_diagonal_x {
                o.bottom_y = n.top_y;
                o.lower_right_diagonal_x = p.upper_left_diagonal_x;
            } else {
                o.lower_right_diagonal_x = n.upper_left_diagonal_x;
            }
        }
        _ => {
            if p.left_x < n.left_x {
                o.right_x = n.left_x;
                o.upper_right_diagonal_x = p.lower_left_diagonal_x;
                if p_side == 7 {
                    o.top_y = p.bottom_y;
                }
            } else if p.left_x > n.left_x {
                o.right_x = p.left_x;
                o.lower_right_diagonal_x = n.upper_left_diagonal_x;
            } else {
                o.right_x = n.left_x;
            }
        }
    }
    o
}
