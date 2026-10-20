//! The any-angle neighbour calculation of a room (A07.4).
//!
//! Port of the calculation of `SortedRoomNeighbours.java` at `a773a70ec`:
//! `calculate` (`:95-135`), `calculateIncompleteRoomsWithEmptyNeighbours`
//! (`:137-155`), `calculateTargetDoors` (`:157-184`), `calculateNeighbours`
//! (`:186-326`), `tryRemoveEdge` (`:412-500`), `calculateNewIncompleteRooms`
//! (`:503-652`) and `SortedRoomNeighbour` (`:658-799`). Rooms of the plain
//! tree are general convex shapes (simplexes); the neighbours are sorted
//! counterclockwise around the room's border.
//!
//! Orders that decide results:
//! - the tree's overlap order, then a **stable** sort by `getId()` and shape
//!   index (`:203-211`);
//! - the neighbour set: Java's `TreeSet` whose comparator treats corner
//!   distances within `c_dist_tolerance = 1` as equal and then falls back
//!   to other criteria, so it is **not transitive**; a [`JavaTreeSet`] makes
//!   exactly `TreeMap.put`'s comparisons, so the set holds Java's elements
//!   in Java's order (`// PARITY:`);
//! - the neighbour iteration of `calculateNewIncompleteRooms`, which decides
//!   the creation order of the incomplete rooms and their doors, and cuts
//!   the completed room's shape on the way;
//! - the own-net objects, kept in overlap order and given target doors after
//!   the shape is final (`:131-133`).
//!
//! Java's lazily computed `firstCorner`/`lastCorner` depend only on the
//! room's original shape (the `roomShape` field, captured before the cuts),
//! so they are computed when the neighbour is built.
//!
//! `router.java_quirks.any_angle_room_side_index` (N06.16): Java takes the
//! touching sides on the room's own shape, but `tryRemoveEdge` and
//! `calculateNewIncompleteRooms` index its `toSimplex()`. An `IntOctagon` room
//! has 8 border lines, degenerate ones too, and its simplex fewer, so the
//! indexes name different lines; a side past the simplex's last line is never
//! reached by the gap walk (`prevNo` stays below the line count), which then
//! adds incomplete rooms until memory runs out (Java: `OutOfMemoryError` on A07's
//! `Issue026-J2_reference-any` and `Issue143-rpi_splitter-any`). `true` (unset)
//! keeps Java's indexes; `false` takes the sides, corners and the sort on the
//! simplex, so every index is one of the simplex's lines and a gap walk ends
//! within `n + 1` steps. Counted in both arms
//! (`any_angle_room_side_mismatches`). `legacy` keeps Java's indexes; profile
//! `enhanced` takes the simplex's (N06.13, 2026-10-20).
//!
//! One case Java goes on with and the port does not: a point touch on no
//! border line of the room or the neighbour (Java logs
//! "touchingSideNoOfRoom >= 0 expected" and keeps side `-1`, which the
//! shapes then clamp or reject each in their own way) gives
//! [`RoomError::IndexOutOfBounds`] (listed in `docs/DIVERGENCES.md`).

use std::cmp::Ordering;

use freerouting_geometry::{
    Line, Point, PolylineShapeOps, Side, Signum, Simplex, TileShape, TileShapeOps,
};

use super::java_tree_set::JavaTreeSet;
use super::neighbours45::{object_java_id, sign};
use super::rooms::FromRoom;
use super::{Room, RoomError, RoomId, Session};
use crate::board::items::ItemId;
use crate::board::search::rooms::IncompleteRoom;
use crate::board::search::{ObjectEntry, TreeObject};
use crate::run_stats::{self, Counter, record_n06};

const NPE: RoomError = RoomError::NullPointer {
    at: "SortedRoomNeighbours.calculate",
};

/// The divergence for a point touch on no border line.
pub(crate) const NO_TOUCHING_SIDE: RoomError = RoomError::IndexOutOfBounds {
    at: "SortedRoomNeighbours.calculateNeighbours (a point touch on no border line, side -1)",
};

/// `SortedRoomNeighbour.c_dist_tolerance`.
const DIST_TOLERANCE: f64 = 1.0;

/// A neighbour of the room being completed (Java
/// `SortedRoomNeighbours.SortedRoomNeighbour`).
#[derive(Clone, Debug)]
pub(crate) struct NeighbourAny {
    pub object: TreeObject,
    /// `neighbourShape`.
    pub shape: TileShape,
    /// `touchingSideNoOfRoom`.
    pub room_side: usize,
    /// `touchingSideNoOfNeighbourRoom`.
    pub neighbour_side: usize,
    /// `roomTouchIsCorner`.
    pub room_corner: bool,
    /// `neighbourRoomTouchIsCorner`.
    pub neighbour_corner: bool,
    /// `firstCorner()`.
    pub first_corner: Point,
    /// `lastCorner()`.
    pub last_corner: Point,
}

/// The state of one neighbour calculation.
pub(crate) struct CalcAny {
    pub completed: RoomId,
    pub from_obstacle: bool,
    pub layer: i32,
    /// The room's shape at the start (`roomShape`, `fromRoom.getShape()`).
    pub room_shape: TileShape,
    /// The obstacle room's item and shape index, for an obstacle room.
    pub obstacle: Option<(ItemId, usize)>,
    /// `sortedNeighbours`, in set order.
    pub sorted: Vec<NeighbourAny>,
    /// `ownNetObjects`, in overlap order.
    pub own_net: Vec<ObjectEntry>,
}

impl NeighbourAny {
    /// The constructor (`:691-706`) with `firstCorner` (`:758-777`) and
    /// `lastCorner` (`:780-798`).
    fn new(
        object: TreeObject,
        shape: TileShape,
        room: &TileShape,
        (room_side, neighbour_side): (usize, usize),
        (room_corner, neighbour_corner): (bool, bool),
    ) -> NeighbourAny {
        let (first_corner, last_corner) = if room_corner {
            let c = room.corner(room_side);
            (c.clone(), c)
        } else if neighbour_corner {
            let c = shape.corner(neighbour_side);
            (c.clone(), c)
        } else {
            let first = shape.corner(shape.next_no(neighbour_side));
            let prev_line = room.border_line(room.prev_no(room_side));
            let first = if prev_line.side_of(&first) == Side::OnTheRight {
                first
            } else {
                // The corner is outside the door shape.
                room.corner(room_side)
            };
            let last = shape.corner(neighbour_side);
            let next_line = room.border_line(room.next_no(room_side));
            let last = if next_line.side_of(&last) == Side::OnTheRight {
                last
            } else {
                room.corner(room.next_no(room_side))
            };
            (first, last)
        };
        NeighbourAny {
            object,
            shape,
            room_side,
            neighbour_side,
            room_corner,
            neighbour_corner,
            first_corner,
            last_corner,
        }
    }
}

/// `SortedRoomNeighbour.compareTo` (`:712-755`), `room` being the room's
/// original shape. Not transitive: corner distances within the tolerance
/// fall through to the last corners, then to the touching lines'
/// directions.
pub(crate) fn compare_any(room: &TileShape, a: &NeighbourAny, b: &NeighbourAny) -> Ordering {
    let by_side = (a.room_side as i32).wrapping_sub(b.room_side as i32);
    if by_side != 0 {
        return sign(by_side);
    }
    let compare_corner = room.corner_approx(a.room_side);
    let this_distance = a.first_corner.to_float().distance(compare_corner);
    let other_distance = b.first_corner.to_float().distance(compare_corner);
    let mut delta = this_distance - other_distance;
    if delta.abs() <= DIST_TOLERANCE && a.first_corner == b.first_corner {
        let this_distance = a.last_corner.to_float().distance(compare_corner);
        let other_distance = b.last_corner.to_float().distance(compare_corner);
        delta = this_distance - other_distance;
        if delta.abs() <= DIST_TOLERANCE && a.neighbour_corner && b.neighbour_corner {
            // A short touch at a link between two trace lines: the
            // directions of the touching lines decide.
            let mut line_no = a.room_side;
            if a.room_corner {
                line_no = room.prev_no(line_no);
            }
            let compare_dir = room.border_line(line_no).direction().opposite();
            let this_line = a.shape.border_line(a.neighbour_side);
            let other_line = b.shape.border_line(b.neighbour_side);
            delta = f64::from(
                compare_dir.compare_from(&this_line.direction(), &other_line.direction()),
            );
        }
    }
    let mut res = Signum::as_int(delta);
    if res == 0 {
        res = object_java_id(a.object).wrapping_sub(object_java_id(b.object));
    }
    sign(res)
}

impl Session<'_> {
    /// Java `SortedRoomNeighbours.calculate` (`:95-135`).
    pub(crate) fn calculate_neighbours_any(
        &mut self,
        from: FromRoom,
    ) -> Result<Option<RoomId>, RoomError> {
        let mut from = from;
        loop {
            let room_id = self.generate_room_id();
            let calc = self.sorted_neighbours_any(&from, room_id)?;
            if let FromRoom::Incomplete(room) = &mut from
                && self.try_remove_edge_any(room, &calc)?
            {
                // PARITY: SortedRoomNeighbours.java:112-115, the doors go and
                // the calculation starts over with a new room id.
                self.remove_all_doors(calc.completed);
                continue;
            }
            if calc.sorted.is_empty() {
                if let Some(obstacle) = calc.obstacle {
                    self.empty_neighbour_rooms_any(&calc, obstacle);
                }
            } else {
                let contained = match &from {
                    FromRoom::Incomplete(room) => Some(room.contained.clone()),
                    FromRoom::Obstacle(_) => None,
                };
                self.new_incomplete_rooms_any(&calc, contained)?;
            }
            if !calc.from_obstacle {
                // Java `calculateTargetDoors` (`:157-184`) on the final shape.
                for entry in &calc.own_net {
                    self.calculate_target_doors(calc.completed, entry.object, entry.shape_index)?;
                }
            }
            return Ok(Some(calc.completed));
        }
    }

    /// Java `calculateNeighbours` (`:186-326`).
    fn sorted_neighbours_any(
        &mut self,
        from: &FromRoom,
        room_id: i32,
    ) -> Result<CalcAny, RoomError> {
        let (room_shape, layer, completed, obstacle) = match from {
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
                (shape, room.layer, completed, None)
            }
            FromRoom::Obstacle(room) => match self.room(*room) {
                Room::Obstacle {
                    item,
                    index,
                    shape,
                    layer,
                    ..
                } => (shape.clone(), *layer, *room, Some((*item, *index))),
                _ => return Err(NPE),
            },
        };
        let mut entries: Vec<ObjectEntry> = Vec::new();
        self.tree().overlapping_object_entries(
            &room_shape,
            layer,
            &[],
            &self.board.items,
            &mut entries,
        );
        // PARITY: SortedRoomNeighbours.java:203-211, a stable sort by
        // getId() and then shape index.
        entries.sort_by(|a, b| {
            let by_id = object_java_id(a.object).wrapping_sub(object_java_id(b.object));
            if by_id != 0 {
                sign(by_id)
            } else {
                a.shape_index.cmp(&b.shape_index)
            }
        });
        let simplex_sides =
            side_simplex(&room_shape, self.control.quirks.any_angle_room_side_index);
        let side_shape = simplex_sides.as_ref().unwrap_or(&room_shape);
        let comparator_room = side_shape.clone();
        // PARITY: TreeSet.add with a comparator that is not transitive.
        let mut set = JavaTreeSet::new(move |a: &NeighbourAny, b: &NeighbourAny| {
            compare_any(&comparator_room, a, b)
        });
        let mut own_net = Vec::new();
        for entry in entries {
            let object = entry.object;
            let index = entry.shape_index;
            if obstacle.is_none() && !self.is_trace_obstacle_object(object) {
                // Target doors wait until the room's shape is final.
                own_net.push(entry);
                continue;
            }
            let current_shape = self.object_tree_shape(object, index).ok_or(NPE)?;
            let intersection = room_shape.intersection(&current_shape);
            let dimension = intersection.dimension();
            if dimension > 1 {
                if obstacle.is_some()
                    && let TreeObject::Item(item) = object
                    && self.is_routable(item)
                {
                    let overlap = self.obstacle_room(item, index)?;
                    self.create_overlap_door(completed, overlap)?;
                }
                continue;
            }
            if dimension < 0 {
                continue;
            }
            if dimension == 1 {
                let Some(sides) = side_shape.touching_sides(&current_shape) else {
                    continue;
                };
                set.insert(NeighbourAny::new(
                    object,
                    current_shape,
                    side_shape,
                    sides,
                    (false, false),
                ));
                self.door_to_line_neighbour(completed, object, index, &intersection)?;
            } else {
                let neighbour =
                    point_touch_neighbour(object, current_shape, side_shape, &intersection)?;
                set.insert(neighbour);
            }
        }
        Ok(CalcAny {
            completed,
            from_obstacle: obstacle.is_some(),
            layer,
            room_shape,
            obstacle,
            sorted: set.into_sorted_vec(),
            own_net,
        })
    }

    /// The door to a neighbour touching along a line (`:264-282`).
    fn door_to_line_neighbour(
        &mut self,
        completed: RoomId,
        object: TreeObject,
        index: usize,
        intersection: &TileShape,
    ) -> Result<(), RoomError> {
        let neighbour = match object {
            TreeObject::Room(id) => self.room_of_tree_id(id),
            TreeObject::Item(item) if self.is_routable(item) => self.obstacle_room(item, index)?,
            TreeObject::Item(_) => None,
        };
        if let Some(neighbour) = neighbour
            && self.insert_door_ok(completed, neighbour, intersection)
        {
            let door = self.new_door(completed, neighbour, 1);
            self.add_door_to(neighbour, door);
            self.add_door_to(completed, door);
        }
        Ok(())
    }

    /// Java `calculateIncompleteRoomsWithEmptyNeighbours` (`:137-155`): an
    /// incomplete room beyond each border line of the obstacle room where a
    /// door is allowed.
    fn empty_neighbour_rooms_any(&mut self, calc: &CalcAny, obstacle: (ItemId, usize)) {
        let shape = calc.room_shape.clone();
        for i in 0..shape.border_line_count() {
            let line = shape.border_line(i);
            if !self.insert_door_ok_at_trace(obstacle, Some(&line)) {
                continue;
            }
            let new_shape = TileShape::Simplex(Simplex::get_instance(&[line.opposite()]));
            let contained = shape.intersection(&new_shape);
            let new_room = self.add_incomplete(Some(new_shape), calc.layer, Some(contained));
            let door = self.new_door(calc.completed, new_room, 1);
            self.add_door_to(calc.completed, door);
            self.add_door_to(new_room, door);
        }
    }

    /// Java `tryRemoveEdge` (`:412-500`): the first border line of the room
    /// without a touching neighbour is removed and the room completed again
    /// (ignoring nothing); one result with a larger area replaces `room`'s
    /// shape and contained shape, and the calculation starts over.
    fn try_remove_edge_any(
        &mut self,
        room: &mut IncompleteRoom,
        calc: &CalcAny,
    ) -> Result<bool, RoomError> {
        let room_simplex = room.shape.as_ref().ok_or(NPE)?.to_simplex();
        let room_area = TileShape::Simplex(room_simplex.clone()).area();
        let mut remove: Option<usize> = None;
        let mut prev_edge: Option<usize> = None;
        let mut current_edge = 0;
        for neighbour in &calc.sorted {
            if Some(neighbour.room_side) == prev_edge {
                continue;
            }
            if neighbour.room_side == current_edge {
                prev_edge = Some(current_edge);
                current_edge += 1;
            } else {
                remove = Some(current_edge);
                break;
            }
        }
        if remove.is_none() && current_edge < room_simplex.border_line_count() {
            remove = Some(current_edge);
        }
        let Some(remove) = remove else {
            return Ok(false);
        };
        let enlarged = room_simplex.remove_border_line(remove);
        let enlarged_room = IncompleteRoom::new(
            Some(TileShape::Simplex(enlarged)),
            room.layer,
            room.contained.clone(),
        );
        let new_rooms = self.complete_shape(&enlarged_room, None, None);
        if new_rooms.len() != 1 {
            return Ok(false);
        }
        let new_room = &new_rooms[0];
        if new_room.shape.as_ref().ok_or(NPE)?.area() > room_area {
            room.shape = new_room.shape.clone();
            room.contained = new_room.contained.clone();
            return Ok(true);
        }
        Ok(false)
    }

    /// The completed room's current shape.
    fn completed_shape(&self, room: RoomId) -> Result<TileShape, RoomError> {
        self.room(room).shape().cloned().ok_or(NPE)
    }

    /// Java `calculateNewIncompleteRooms` (`:503-652`): incomplete rooms
    /// with doors in the gaps between consecutive neighbours, walking each
    /// gap backwards along the room's border lines. A free room's shape is
    /// cut by the line between the two neighbours' corners on the way.
    /// `contained` is the incomplete room's contained shape (`None` for an
    /// obstacle room).
    fn new_incomplete_rooms_any(
        &mut self,
        calc: &CalcAny,
        contained: Option<Option<TileShape>>,
    ) -> Result<(), RoomError> {
        let simplex = calc.room_shape.to_simplex();
        let room = TileShape::Simplex(simplex.clone());
        let count = calc.sorted.len();
        let last = count - 1;
        let mut prev = last;
        for next in 0..count {
            let p = &calc.sorted[prev];
            let n = &calc.sorted[next];
            let mut first_side = p.room_side;
            let mut last_side = n.room_side;
            let current_next = room.next_no(first_side);
            // PARITY: `prevNeighbour == this.sortedNeighbours.getLast()` is
            // an identity test, here an index test.
            let at_last = prev == last;
            let check = first_side != last_side || at_last;
            let prev_ends_at_corner = check && p.last_corner == room.corner(current_next);
            let next_starts_at_corner = check && n.first_corner == room.corner(last_side);
            if prev_ends_at_corner {
                first_side = current_next;
            }
            if next_starts_at_corner {
                last_side = room.prev_no(last_side);
            }
            let neighbours_touch = count > 1 && p.last_corner == n.first_corner;
            if !neighbours_touch {
                let mut last_bounding = p.neighbour_side;
                if !(prev_ends_at_corner || p.room_corner) {
                    last_bounding = p.shape.prev_no(last_bounding);
                }
                let mut first_bounding = n.neighbour_side;
                if !(next_starts_at_corner || n.neighbour_corner) {
                    first_bounding = n.shape.next_no(first_bounding);
                }
                let gap = Gap {
                    first_side,
                    last_side,
                    at_last,
                    end_line: p.shape.border_line(last_bounding).opposite(),
                    start_line: n.shape.border_line(first_bounding).opposite(),
                    cut: (p.last_corner.clone(), n.first_corner.clone()),
                };
                self.gap_rooms_any(calc, &room, &gap, contained.as_ref())?;
            }
            prev = next;
        }
        Ok(())
    }

    /// The loop of `calculateNewIncompleteRooms` over one gap
    /// (`:555-648`), from the next neighbour's side back to the previous
    /// neighbour's side.
    fn gap_rooms_any(
        &mut self,
        calc: &CalcAny,
        room: &TileShape,
        gap: &Gap,
        contained: Option<&Option<TileShape>>,
    ) -> Result<(), RoomError> {
        let mut start_line = Some(gap.start_line);
        let mut middle: Option<Line> = None;
        let mut current = gap.last_side;
        let mut first_time = true;
        let mut steps = 0;
        loop {
            // The fix's bound (N06.16): every side is one of the simplex's
            // lines, so the walk reaches `first_side` within one lap, or two
            // when it starts there (`at_last && first_time`).
            steps += 1;
            debug_assert!(
                self.control.quirks.any_angle_room_side_index
                    || steps <= room.border_line_count() + 1,
                "the gap walk ends within n + 1 steps"
            );
            let mut corner_cut_off = false;
            if let Some(contained) = contained
                && current == gap.last_side
                && gap.first_side != gap.last_side
            {
                // A line from the last corner of the previous neighbour to
                // the first corner of the next cuts off the room's corners
                // in the empty space (first pass only).
                let start = gap.cut.0.to_float().round();
                let end = gap.cut.1.to_float().round();
                let cut = Line::new(start, end);
                let half_plane = TileShape::get_instance_half_plane(cut);
                let cut_shape = self
                    .completed_shape(calc.completed)?
                    .intersection(&half_plane);
                if let Room::CompleteFree { shape, .. } = self.room_mut(calc.completed) {
                    *shape = cut_shape;
                }
                let contained = contained.as_ref().ok_or(NPE)?;
                corner_cut_off = contained.side_of_line(&cut) == Side::OnTheLeft;
                if corner_cut_off {
                    middle = Some(cut.opposite());
                }
            }
            let next_side = room.prev_no(current);
            if !corner_cut_off {
                middle = Some(room.border_line(current).opposite());
            }
            let middle_line = middle.ok_or(NPE)?;
            let middle_dir = middle_line.direction();
            let last_time =
                (current == gap.first_side && !(gap.at_last && first_time)) || corner_cut_off;
            let end_line =
                if last_time && gap.end_line.direction().side_of(&middle_dir) == Side::OnTheLeft {
                    Some(gap.end_line)
                } else {
                    // A concave corner between the middle and the last line.
                    None
                };
            if start_line
                .as_ref()
                .is_some_and(|s| middle_dir.side_of(&s.direction()) != Side::OnTheLeft)
            {
                // A concave corner between the first and the middle line.
                start_line = None;
            }
            let mut lines = Vec::with_capacity(3);
            lines.extend(start_line);
            lines.push(middle_line);
            lines.extend(end_line);
            let new_shape = Simplex::get_instance(&lines);
            if !new_shape.is_empty() {
                let new_shape = TileShape::Simplex(new_shape);
                let new_contained = self
                    .completed_shape(calc.completed)?
                    .intersection(&new_shape);
                if !new_contained.is_empty() {
                    let new_room =
                        self.add_incomplete(Some(new_shape), calc.layer, Some(new_contained));
                    let door = self.new_door(calc.completed, new_room, 1);
                    self.add_door_to(calc.completed, door);
                    self.add_door_to(new_room, door);
                }
            }
            if last_time {
                return Ok(());
            }
            current = next_side;
            start_line = None;
            first_time = false;
        }
    }
}

/// One gap of `calculateNewIncompleteRooms` between two neighbours.
struct Gap {
    /// `firstTouchingSideNo`, `lastTouchingSideNo` after the corner moves.
    first_side: usize,
    last_side: usize,
    /// The previous neighbour is the last one of the set.
    at_last: bool,
    /// `endEdgeLine` before the concave check: the previous neighbour's
    /// bounding line, opposite.
    end_line: Line,
    /// `startEdgeLine`: the next neighbour's bounding line, opposite.
    start_line: Line,
    /// The previous neighbour's last corner and the next one's first.
    cut: (Point, Point),
}

/// `router.java_quirks.any_angle_room_side_index` (N06.16): the shape the
/// neighbours' sides index, `None` for the room's own shape (Java, `java`) and
/// the room's simplex under the fix. The calculation is counted in both arms
/// when the two have different border line counts.
// PARITY(quirk, java_quirks.any_angle_room_side_index): Java takes the touching
// sides on `roomShape` (SortedRoomNeighbours.java:189, 253, 288, 296) and indexes
// its `toSimplex()` with them (:419, :511).
pub(crate) fn side_simplex(room_shape: &TileShape, java: bool) -> Option<TileShape> {
    if java && !run_stats::enabled() {
        return None;
    }
    let simplex = room_shape.to_simplex();
    if simplex.border_line_count() != room_shape.border_line_count() {
        record_n06(Counter::AnyAngleRoomSideMismatches);
    }
    (!java).then_some(TileShape::Simplex(simplex))
}

/// A neighbour touching the room in one point (`:283-323`).
fn point_touch_neighbour(
    object: TreeObject,
    shape: TileShape,
    room: &TileShape,
    intersection: &TileShape,
) -> Result<NeighbourAny, RoomError> {
    let point = intersection.corner(0);
    let (room_side, room_corner) = match room.equals_corner(&point) {
        Some(corner) => (corner, true),
        None => (
            room.contains_on_border_line_no(&point)
                .ok_or(NO_TOUCHING_SIDE)?,
            false,
        ),
    };
    let (neighbour_side, neighbour_corner) = match shape.equals_corner(&point) {
        // The previous border line makes the incomplete room as big as
        // possible.
        Some(corner) => (shape.prev_no(corner), true),
        None => (
            shape
                .contains_on_border_line_no(&point)
                .ok_or(NO_TOUCHING_SIDE)?,
            false,
        ),
    };
    Ok(NeighbourAny::new(
        object,
        shape,
        room,
        (room_side, neighbour_side),
        (room_corner, neighbour_corner),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use freerouting_geometry::IntBox;

    fn simplex_box(lx: i32, ly: i32, rx: i32, uy: i32) -> TileShape {
        TileShape::Simplex(IntBox::new(lx, ly, rx, uy).to_simplex())
    }

    /// A point touch that lies on no border line of the room (a point
    /// neighbour inside the room) is the listed divergence: Java goes on
    /// with side -1, the port stops with `IndexOutOfBounds`.
    #[test]
    fn a_point_touch_on_no_border_line_is_an_error() {
        let room = simplex_box(0, 0, 100, 100);
        let point = TileShape::IntBox(IntBox::new(50, 50, 50, 50));
        let result =
            point_touch_neighbour(TreeObject::Item(ItemId(1)), point.clone(), &room, &point);
        assert_eq!(result.err(), Some(NO_TOUCHING_SIDE));
    }

    /// `any_angle_room_side_index` (N06.16), the octagon room of the
    /// `Issue026-J2_reference-any` fanout scaled down: the triangle x >= 0,
    /// y >= 0, x + y <= 100 as an `IntOctagon` has 8 border lines and a 3-line
    /// simplex. A neighbour along its diagonal touches octagon side 3 (Java),
    /// past the simplex's last line, which `prev_no` never reaches, so Java's
    /// gap walk never ends; the fix takes simplex side 1, the same line.
    #[test]
    fn room_side_index_switch_both_behaviours() {
        let room = TileShape::IntOctagon(TileShape::get_instance_octagon(
            0, 0, 100, 100, -100, 100, 0, 100,
        ));
        let beyond = TileShape::IntOctagon(TileShape::get_instance_octagon(
            0, 0, 100, 100, -100, 100, 100, 200,
        ));
        let simplex = room.to_simplex();
        assert_eq!(
            (room.border_line_count(), simplex.border_line_count()),
            (8, 3)
        );

        assert!(
            side_simplex(&room, true).is_none(),
            "Java indexes the octagon"
        );
        let (java_side, _) = room.touching_sides(&beyond).expect("a line touch");
        assert_eq!(java_side, 3);
        let walk = TileShape::Simplex(simplex.clone());
        assert!(
            (0..3).all(|side| walk.prev_no(side) != java_side),
            "the gap walk never reaches Java's side"
        );

        let fixed = side_simplex(&room, false).expect("the fix indexes the simplex");
        let (fixed_side, _) = fixed.touching_sides(&beyond).expect("a line touch");
        assert_eq!(fixed_side, 1);
        assert_eq!(fixed.border_line(fixed_side), room.border_line(java_side));

        // A box room has as many lines as its simplex: both arms index alike.
        let square = simplex_box(0, 0, 100, 100);
        let fixed_square = side_simplex(&square, false).expect("the fix's shape");
        assert_eq!(fixed_square.border_line_count(), square.border_line_count());
    }

    /// Neighbours sort by the touching side of the room, then by the
    /// distance of their first corner from the side's start corner.
    #[test]
    fn neighbours_sort_by_side_then_corner_distance() {
        let room = simplex_box(0, 0, 100, 100);
        let mut set =
            JavaTreeSet::new(|a: &NeighbourAny, b: &NeighbourAny| compare_any(&room, a, b));
        let mut add = |id: i32, shape: TileShape| {
            let sides = room.touching_sides(&shape).expect("touching sides");
            set.insert(NeighbourAny::new(
                TreeObject::Item(ItemId(id)),
                shape,
                &room,
                sides,
                (false, false),
            ))
        };
        assert!(add(1, simplex_box(100, 20, 150, 40)));
        assert!(add(2, simplex_box(60, -10, 80, 0)));
        assert!(add(3, simplex_box(10, -10, 30, 0)));
        // Same side and a first corner within the tolerance of 3's: the last
        // corners decide (farther: after 3).
        assert!(add(4, simplex_box(10, -10, 50, 0)));
        let order: Vec<i32> = set.iter().map(|n| object_java_id(n.object)).collect();
        let sides: Vec<usize> = set.iter().map(|n| n.room_side).collect();
        assert_eq!(sides[0], sides[1]);
        assert_eq!(order[..3], [3, 4, 2]);
        assert_eq!(order[3], 1);
    }
}
