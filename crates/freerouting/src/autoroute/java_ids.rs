//! Java's `getId` formulas of the autoroute objects (A01.2).
//!
//! The maze queue breaks ties between equal costs by `door.getId()`
//! (`MazeListElement.java:80-113`), and the drill pages and drills are
//! keyed the same way, so these ids decide routes. Java computes them with
//! `int` arithmetic that overflows silently; every function here uses
//! `wrapping_mul`/`wrapping_add` on `i32` to give the same bits.
//!
//! The inputs are other ids: room ids, item ids, and `Shape.getId()` of tile
//! shapes and points (`freerouting_geometry`'s `get_id`). The id of a
//! complete free-space room is not a formula: it is the engine's counter
//! (A02).

/// `ExpansionDoor.getId` (`ExpansionDoor.java:185-190`): the two room ids
/// combined independently of their order.
pub fn expansion_door_id(first_room_id: i32, second_room_id: i32) -> i32 {
    // PARITY: ExpansionDoor.java:189, `Math.min(id1, id2) * 31 + Math.max(id1, id2)`.
    first_room_id
        .min(second_room_id)
        .wrapping_mul(31)
        .wrapping_add(first_room_id.max(second_room_id))
}

/// `TargetItemExpansionDoor.getId` (`TargetItemExpansionDoor.java:71-74`).
/// `room_id` is the id of the door's room, or 0 when it has none.
pub fn target_door_id(item_id: i32, room_id: i32) -> i32 {
    // PARITY: TargetItemExpansionDoor.java:73, `31 * item.getId() + (room != null ? room.getId() : 0)`.
    item_id.wrapping_mul(31).wrapping_add(room_id)
}

/// `ObstacleExpansionRoom.getId` (`ObstacleExpansionRoom.java:49-51`): the
/// item id shifted left by 10, or-ed with the shape index.
pub fn obstacle_room_id(item_id: i32, index_in_item: i32) -> i32 {
    // PARITY: ObstacleExpansionRoom.java:50, `(item.getId() << 10) | indexInItem`;
    // Java's `<<` keeps the low 32 bits, as `wrapping_shl` does.
    item_id.wrapping_shl(10) | index_in_item
}

/// `IncompleteFreeSpaceExpansionRoom.getId`
/// (`IncompleteFreeSpaceExpansionRoom.java:38-41`). Java throws on a room
/// without a shape; the caller has a shape id or decides what a missing
/// shape means.
pub fn incomplete_room_id(shape_id: i32, layer: i32) -> i32 {
    // PARITY: IncompleteFreeSpaceExpansionRoom.java:40, `31 * getShape().getId() + getLayer()`.
    shape_id.wrapping_mul(31).wrapping_add(layer)
}

/// `ExpansionDrill.getId` (`ExpansionDrill.java:127-130`).
pub fn expansion_drill_id(location_id: i32, first_layer: i32, last_layer: i32) -> i32 {
    // PARITY: ExpansionDrill.java:129, `31 * (31 * location.getId() + firstLayer) + lastLayer`.
    location_id
        .wrapping_mul(31)
        .wrapping_add(first_layer)
        .wrapping_mul(31)
        .wrapping_add(last_layer)
}

/// `DrillPage.getId` (`DrillPage.java:193-196`).
pub fn drill_page_id(shape_id: i32, net_number: i32) -> i32 {
    // PARITY: DrillPage.java:195, `31 * shape.getId() + netNumber`.
    shape_id.wrapping_mul(31).wrapping_add(net_number)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_overflow_like_java_int() {
        // `int` overflow wraps; the probe `java-ids` checks the values against Java.
        assert_eq!(
            expansion_door_id(i32::MAX, 5),
            5i32.wrapping_mul(31).wrapping_add(i32::MAX)
        );
        assert_eq!(expansion_door_id(3, 2), expansion_door_id(2, 3));
        assert_eq!(expansion_door_id(2, 3), 65);
        assert_eq!(target_door_id(i32::MIN, 0), 0i32.wrapping_sub(i32::MIN));
        assert_eq!(obstacle_room_id(1, 3), 1027);
        assert_eq!(obstacle_room_id(0x0020_0000, 0), i32::MIN);
        assert_eq!(obstacle_room_id(0x0040_0000, 7), 7);
        assert_eq!(obstacle_room_id(-1, 5), -1024 | 5);
        assert_eq!(incomplete_room_id(10, 2), 312);
        assert_eq!(expansion_drill_id(1, 0, 3), 964);
        assert_eq!(drill_page_id(-7, 4), -213);
    }
}
