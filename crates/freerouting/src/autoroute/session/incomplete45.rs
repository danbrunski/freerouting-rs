//! The new incomplete rooms of the 45 degree neighbour calculation
//! (A02.4).
//!
//! Port of `Sorted45DegreeRoomNeighbours.calculateNewIncompleteRooms`
//! (`:610-788`), the obstacle-room variants (`:254-309`, `:474-608`) and
//! `insertIncompleteRoom` (`:437-472`) at `a773a70ec`. The neighbours are
//! visited in comparator order, which decides the creation order of the
//! incomplete rooms and their doors.
//!
//! Java bug kept for `legacy`: the edge sweep of an obstacle room never
//! advances its compared corner (`:263`), so a full sweep skips side 7 (and,
//! on a box, the left side 6) and keeps sides of length 0.
//! `router.java_quirks.edge_corner_not_advanced = false` advances it (N06.5,
//! `edge_room_sides_switch_both_behaviours`). With statistics on, the edge rooms
//! the other value would add or drop count as `maze_edge_corner_flips` in
//! either arm, and the full sweeps of obstacle rooms without sorted neighbours
//! as `maze_edge_sweeps_without_neighbours`.

use freerouting_geometry::{IntOctagon, PolylineShapeOps, TileShape};

use super::Session;
use super::neighbours45::Calc45;

/// The sides from `from_side` to `to_side` (a bit per side) that
/// `calculateEdgeIncompleteRoomsOfObstacleExpansionRoom` gives an edge room:
/// those whose end corner differs from the compared corner. `java` is
/// `router.java_quirks.edge_corner_not_advanced`.
fn edge_room_sides(room: &IntOctagon, from_side: usize, to_side: usize, java: bool) -> u8 {
    // PARITY(quirk, java_quirks.edge_corner_not_advanced):
    // Sorted45DegreeRoomNeighbours.java:263, the corner compared with each
    // next corner is never advanced, so a side is skipped only when its end
    // corner equals the first side's start: a full sweep (0 to 7) always skips
    // side 7 and keeps the sides of length 0. `false` advances it, so exactly
    // the sides of length 0 are skipped.
    let mut sides = 0u8;
    let mut current_corner = room.int_corner(from_side);
    let mut side = from_side;
    loop {
        let next = (side + 1) % 8;
        if current_corner != room.int_corner(next) {
            sides |= 1 << side;
        }
        if side == to_side {
            return sides;
        }
        if !java {
            current_corner = room.int_corner(next);
        }
        side = next;
    }
}

/// The board octagon `board` cut back to the far side of `room`'s `side`
/// (`:271-293`).
fn edge_room_octagon(board: &IntOctagon, room: &IntOctagon, side: usize) -> IntOctagon {
    let mut o = *board;
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
    o
}

/// `insertIncompleteRoom`'s tests (`:437-472`): the normalized octagon, its
/// intersection with `room` and the door's dimension, if the octagon is
/// 2-dimensional and touches `room` along a line or more.
fn incomplete_room_45(
    room: &IntOctagon,
    octagon: IntOctagon,
) -> Option<(IntOctagon, IntOctagon, i32)> {
    let shape = octagon.normalize();
    if shape.dimension() != 2 {
        return None;
    }
    let contained = room.intersection_octagon(&shape);
    if contained.is_empty() {
        return None;
    }
    let door_dimension = contained.dimension();
    (door_dimension > 0).then_some((shape, contained, door_dimension))
}

impl Session<'_> {
    /// The board's bounding octagon.
    fn board_octagon(&self) -> IntOctagon {
        self.board.bounding_box().to_int_octagon()
    }

    /// Java `calculateEdgeIncompleteRoomsOfObstacleExpansionRoom`
    /// (`:254-309`): an incomplete room beyond each side of the obstacle
    /// room from `from_side` to `to_side`, in side order.
    ///
    /// With statistics on, the edge rooms that the other value of
    /// `edge_corner_not_advanced` would add or drop count as
    /// `maze_edge_corner_flips`, in either arm (N06.5).
    pub(super) fn edge_incomplete_rooms_of_obstacle(
        &mut self,
        calc: &Calc45,
        from_side: usize,
        to_side: usize,
    ) {
        let board = self.board_octagon();
        let room = calc.room_oct;
        let java = self.control.quirks.edge_corner_not_advanced;
        let sides = edge_room_sides(&room, from_side, to_side, java);
        if crate::run_stats::enabled() {
            let flipped = sides ^ edge_room_sides(&room, from_side, to_side, !java);
            for side in (0..8).filter(|side| flipped & (1 << side) != 0) {
                if incomplete_room_45(&room, edge_room_octagon(&board, &room, side)).is_some() {
                    crate::run_stats::record_n06(crate::run_stats::Counter::MazeEdgeCornerFlips);
                }
            }
        }
        let mut side = from_side;
        loop {
            if sides & (1 << side) != 0 {
                self.insert_incomplete_room_45(calc, edge_room_octagon(&board, &room, side));
            }
            if side == to_side {
                break;
            }
            side = (side + 1) % 8;
        }
    }

    /// Java `insertIncompleteRoom` (`:437-472`): an incomplete room of the
    /// normalized octagon if it is 2-dimensional and touches the room, with
    /// a door of the touch's dimension.
    fn insert_incomplete_room_45(&mut self, calc: &Calc45, octagon: IntOctagon) {
        if let Some((shape, contained, door_dimension)) =
            incomplete_room_45(&calc.room_oct, octagon)
        {
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

#[cfg(test)]
mod tests {
    use freerouting_geometry::IntBox;

    use super::*;

    /// The box (-100, -100)-(100, 100) as an octagon: its four diagonal sides
    /// (1, 3, 5, 7) have length 0.
    const BOX: IntOctagon = IntOctagon::new(-100, -100, 100, 100, -200, 200, -200, 200);
    /// A regular octagon: every side has a length.
    const OCTAGON: IntOctagon = IntOctagon::new(-100, -100, 100, 100, -150, 150, -150, 150);

    /// `edge_corner_not_advanced` (N06.5): Java compares every next corner with
    /// the first side's start, so a full sweep skips side 7, and on a box also
    /// the left side 6 (its end corner is the start corner), while it keeps
    /// the diagonal sides of length 0. `false` skips exactly the sides of
    /// length 0.
    #[test]
    fn edge_room_sides_switch_both_behaviours() {
        assert_eq!(edge_room_sides(&OCTAGON, 0, 7, true), 0b0111_1111);
        assert_eq!(edge_room_sides(&OCTAGON, 0, 7, false), 0b1111_1111);
        assert_eq!(edge_room_sides(&BOX, 0, 7, true), 0b0011_1111);
        assert_eq!(edge_room_sides(&BOX, 0, 7, false), 0b0101_0101);
        // A partial sweep of a regular octagon is the same in both arms.
        assert_eq!(edge_room_sides(&OCTAGON, 2, 4, true), 0b0001_1100);
        assert_eq!(edge_room_sides(&OCTAGON, 2, 4, false), 0b0001_1100);
        // A sweep that wraps past side 7.
        assert_eq!(edge_room_sides(&OCTAGON, 6, 1, true), 0b1100_0011);
        assert_eq!(edge_room_sides(&OCTAGON, 6, 1, false), 0b1100_0011);
    }

    /// Only the box's left room (side 6) is made of the sides the switch flips:
    /// the diagonal sides' rooms touch the box in a corner point, no door.
    #[test]
    fn of_the_flipped_box_sides_only_the_left_room_is_made() {
        let board = IntBox::new(-1000, -1000, 1000, 1000).to_int_octagon();
        let flipped = edge_room_sides(&BOX, 0, 7, true) ^ edge_room_sides(&BOX, 0, 7, false);
        assert_eq!(flipped, 0b0110_1010);
        let made: Vec<usize> = (0..8)
            .filter(|side| flipped & (1 << side) != 0)
            .filter(|&side| {
                incomplete_room_45(&BOX, edge_room_octagon(&board, &BOX, side)).is_some()
            })
            .collect();
        assert_eq!(made, [6]);
        let (shape, contained, door_dimension) =
            incomplete_room_45(&BOX, edge_room_octagon(&board, &BOX, 6)).unwrap();
        assert_eq!(shape.right_x, -100);
        assert_eq!((contained.left_x, contained.right_x), (-100, -100));
        assert_eq!(door_dimension, 1);
    }
}
