//! The restrain step of the 90° room completion (B03.5).
//!
//! Port of `ShapeSearchTree90Degree.restrainShape` (at `a773a70ec`). All
//! shapes are boxes. The distances are Java `int` arithmetic.

use freerouting_geometry::{IntBox, PolylineShapeOps, TileShape};

use super::rooms::IncompleteRoom;

/// Restrains `room` to a box that does not intersect the interior of
/// `obstacle` and contains the room's contained shape; if the contained
/// shape intersects the obstacle, splits the room in two at an obstacle
/// edge through it and restrains the second piece again. The rooms are
/// appended to `result` in Java's order.
pub(super) fn restrain_90(
    room: &IncompleteRoom,
    obstacle: &IntBox,
    result: &mut Vec<IncompleteRoom>,
) {
    let Some(contained) = &room.contained else {
        return;
    };
    if contained.is_empty() {
        return;
    }
    let Some(shape) = &room.shape else {
        return;
    };
    let r = shape.bounding_box();
    let c = contained.bounding_box();
    let layer = room.layer;
    let o = obstacle;
    let mut cut_line_distance = 0;
    let mut restrained = None;

    // PARITY: the four edges in Java's order (right, left, lower, upper);
    // a later edge wins only if strictly further from the contained shape.
    if r.ll.x < o.ur.x && r.ur.x > o.ur.x && r.ur.y > o.ll.y && r.ll.y < o.ur.y {
        let distance = c.ll.x.wrapping_sub(o.ur.x);
        if distance > cut_line_distance {
            cut_line_distance = distance;
            restrained = Some(IntBox::new(o.ur.x, r.ll.y, r.ur.x, r.ur.y));
        }
    }
    if r.ll.x < o.ll.x && r.ur.x > o.ll.x && r.ur.y > o.ll.y && r.ll.y < o.ur.y {
        let distance = o.ll.x.wrapping_sub(c.ur.x);
        if distance > cut_line_distance {
            cut_line_distance = distance;
            restrained = Some(IntBox::new(r.ll.x, r.ll.y, o.ll.x, r.ur.y));
        }
    }
    if r.ll.y < o.ll.y && r.ur.y > o.ll.y && r.ur.x > o.ll.x && r.ll.x < o.ur.x {
        let distance = o.ll.y.wrapping_sub(c.ur.y);
        if distance > cut_line_distance {
            cut_line_distance = distance;
            restrained = Some(IntBox::new(r.ll.x, r.ll.y, r.ur.x, o.ll.y));
        }
    }
    if r.ll.y < o.ur.y && r.ur.y > o.ur.y && r.ur.x > o.ll.x && r.ll.x < o.ur.x {
        let distance = c.ll.y.wrapping_sub(o.ur.y);
        if distance > cut_line_distance {
            restrained = Some(IntBox::new(r.ll.x, o.ur.y, r.ur.x, r.ur.y));
        }
    }
    if let Some(restrained) = restrained {
        result.push(IncompleteRoom::new(
            Some(TileShape::IntBox(restrained)),
            layer,
            Some(TileShape::IntBox(c)),
        ));
        return;
    }

    // The contained shape intersects the obstacle: split the room in two.
    let is = c.intersection_box(o);
    if is.is_empty() {
        return;
    }
    let split = if is.ll.x > r.ll.x && is.ll.x == o.ll.x && is.ll.x < r.ur.x {
        Some((
            IntBox::new(r.ll.x, r.ll.y, is.ll.x, r.ur.y),
            IntBox::new(is.ll.x, r.ll.y, r.ur.x, r.ur.y),
        ))
    } else if is.ur.x > r.ll.x && is.ur.x == o.ur.x && is.ur.x < r.ur.x {
        Some((
            IntBox::new(is.ur.x, r.ll.y, r.ur.x, r.ur.y),
            IntBox::new(r.ll.x, r.ll.y, is.ur.x, r.ur.y),
        ))
    } else if is.ll.y > r.ll.y && is.ll.y == o.ll.y && is.ll.y < r.ur.y {
        Some((
            IntBox::new(r.ll.x, r.ll.y, r.ur.x, is.ll.y),
            IntBox::new(r.ll.x, is.ll.y, r.ur.x, r.ur.y),
        ))
    } else if is.ur.y > r.ll.y && is.ur.y == o.ur.y && is.ur.y < r.ur.y {
        Some((
            IntBox::new(r.ll.x, is.ur.y, r.ur.x, r.ur.y),
            IntBox::new(r.ll.x, r.ll.y, r.ur.x, is.ur.y),
        ))
    } else {
        None
    };
    if let Some((first, second)) = split {
        let first_contained = c.intersection_box(&first);
        if first_contained.dimension() > 0 {
            result.push(IncompleteRoom::new(
                Some(TileShape::IntBox(first)),
                layer,
                Some(TileShape::IntBox(first_contained)),
            ));
            let second_room = IncompleteRoom::new(
                Some(TileShape::IntBox(second)),
                layer,
                Some(TileShape::IntBox(c.intersection_box(&second))),
            );
            restrain_90(&second_room, obstacle, result);
        }
    }
}
