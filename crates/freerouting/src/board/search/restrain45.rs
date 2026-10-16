//! The restrain step of the 45° room completion (B03.4).
//!
//! Port of `ShapeSearchTree45Degree.restrainShape`, `calcOutsideRestrainedShape`,
//! `calcInsideRestrainedShape`, `obstacleSegmentTouchesInside` and
//! `signedLineDistance` (at `a773a70ec`). All shapes are octagons; the
//! border lines are numbered 0 (bottom) to 7 counterclockwise, as in
//! `IntOctagon.borderLine`.

use freerouting_geometry::{IntOctagon, PolylineShapeOps, Side, TileShape, TileShapeOps};

use super::rooms::IncompleteRoom;

/// Restrains `room` to an octagon that does not intersect the interior of
/// `obstacle` and contains the room's contained shape; if no border line
/// of the obstacle leaves the contained shape outside, cuts along one
/// through it and restrains the rest piece again. The rooms are appended to
/// `result` in Java's order.
pub(super) fn restrain_45(
    room: &IncompleteRoom,
    obstacle: &IntOctagon,
    result: &mut Vec<IncompleteRoom>,
) {
    let Some(contained) = &room.contained else {
        return;
    };
    if contained.is_empty() {
        return;
    }
    // Java: an IntOctagon (`isIntOctagon`, which boxes are too) or a
    // Simplex with a bounding octagon; anything else gives no rooms.
    let to_contain = if contained.is_int_octagon() || matches!(contained, TileShape::Simplex(_)) {
        match contained.bounding_octagon() {
            Some(o) => o,
            None => return,
        }
    } else {
        return;
    };
    let room_shape = match &room.shape {
        Some(shape @ (TileShape::IntOctagon(_) | TileShape::Simplex(_))) => {
            match shape.bounding_octagon() {
                Some(o) => o,
                None => return,
            }
        }
        _ => return,
    };
    let layer = room.layer;

    // PARITY: the border line furthest from the contained shape among those
    // whose segment touches the room's inside; the first one on ties
    // (strictly greater), with the distance tested first.
    let mut cut_line_distance = -1.0;
    let mut restraining_line = None;
    for line_no in 0..8 {
        let distance = signed_line_distance(obstacle, line_no, &to_contain);
        if distance > cut_line_distance
            && obstacle_segment_touches_inside(obstacle, line_no, &room_shape)
        {
            cut_line_distance = distance;
            restraining_line = Some(line_no);
        }
    }
    if cut_line_distance >= 0.0 {
        let line_no = restraining_line.expect("a distance >= 0 has its line");
        let restrained = calc_outside_restrained_shape(obstacle, line_no, &room_shape);
        result.push(IncompleteRoom::new(
            Some(TileShape::IntOctagon(restrained)),
            layer,
            Some(TileShape::IntOctagon(to_contain)),
        ));
        return;
    }

    // No border line leaves all of the contained shape outside: cut along
    // one that runs through it.
    if to_contain.dimension() < 1 {
        return;
    }
    let mut restraining_line = None;
    for line_no in 0..8 {
        if obstacle_segment_touches_inside(obstacle, line_no, &room_shape)
            && to_contain.side_of_line(&obstacle.border_line(line_no)) == Side::Collinear
        {
            restraining_line = Some(line_no);
            break;
        }
    }
    let Some(line_no) = restraining_line else {
        return;
    };
    let restrained = calc_outside_restrained_shape(obstacle, line_no, &room_shape);
    if restrained.dimension() == 2 {
        let new_contained = to_contain.intersection_octagon(&restrained);
        if new_contained.dimension() > 0 {
            result.push(IncompleteRoom::new(
                Some(TileShape::IntOctagon(restrained)),
                layer,
                Some(TileShape::IntOctagon(new_contained)),
            ));
        }
    }
    // PARITY: the outside piece first, then the pieces of the inside rest.
    let rest = calc_inside_restrained_shape(obstacle, line_no, &room_shape);
    if rest.dimension() >= 2 {
        let rest_contained = to_contain.intersection_octagon(&rest);
        if rest_contained.dimension() >= 0 {
            let rest_room = IncompleteRoom::new(
                Some(TileShape::IntOctagon(rest)),
                layer,
                Some(TileShape::IntOctagon(rest_contained)),
            );
            restrain_45(&rest_room, obstacle, result);
        }
    }
}

/// True if border line segment `line_no` of `obstacle` intersects the
/// inside of `room` (Java `obstacleSegmentTouchesInside`).
pub(super) fn obstacle_segment_touches_inside(
    obstacle: &IntOctagon,
    line_no: usize,
    room: &IntOctagon,
) -> bool {
    let corner_x = obstacle.corner_x(line_no);
    let corner_y = obstacle.corner_y(line_no);
    let mut border_line = line_no;
    for _ in 0..5 {
        if room.side_of_border_line(corner_x, corner_y, border_line) != Side::OnTheLeft {
            return false;
        }
        border_line = (border_line + 1) % 8;
    }
    let next_line = (line_no + 1) % 8;
    let next_x = obstacle.corner_x(next_line);
    let next_y = obstacle.corner_y(next_line);
    border_line = (line_no + 5) % 8;
    for _ in 0..3 {
        if room.side_of_border_line(next_x, next_y, border_line) != Side::OnTheLeft {
            return false;
        }
        border_line = (border_line + 1) % 8;
    }
    true
}

/// The signed distance of border line `line_no` of `obstacle` from
/// `contained` (Java `signedLineDistance`). The differences are Java `int`
/// arithmetic; the diagonal ones are scaled by 0.5 instead of `1 / sqrt(2)`
/// to prefer orthogonal lines slightly (D6: the same operations in the same
/// order).
pub(super) fn signed_line_distance(
    obstacle: &IntOctagon,
    line_no: usize,
    contained: &IntOctagon,
) -> f64 {
    match line_no {
        0 => f64::from(obstacle.bottom_y.wrapping_sub(contained.top_y)),
        2 => f64::from(contained.left_x.wrapping_sub(obstacle.right_x)),
        4 => f64::from(contained.bottom_y.wrapping_sub(obstacle.top_y)),
        6 => f64::from(obstacle.left_x.wrapping_sub(contained.right_x)),
        1 => {
            0.5 * f64::from(
                contained
                    .upper_left_diagonal_x
                    .wrapping_sub(obstacle.lower_right_diagonal_x),
            )
        }
        3 => {
            0.5 * f64::from(
                contained
                    .lower_left_diagonal_x
                    .wrapping_sub(obstacle.upper_right_diagonal_x),
            )
        }
        5 => {
            0.5 * f64::from(
                obstacle
                    .upper_left_diagonal_x
                    .wrapping_sub(contained.lower_right_diagonal_x),
            )
        }
        7 => {
            0.5 * f64::from(
                obstacle
                    .lower_left_diagonal_x
                    .wrapping_sub(contained.upper_right_diagonal_x),
            )
        }
        _ => 0.0,
    }
}

/// `room` intersected with the half plane outside border line `line_no` of
/// `obstacle`, normalized (Java `calcOutsideRestrainedShape`).
pub(super) fn calc_outside_restrained_shape(
    obstacle: &IntOctagon,
    line_no: usize,
    room: &IntOctagon,
) -> IntOctagon {
    let mut r = *room;
    match line_no {
        0 => r.top_y = obstacle.bottom_y,
        2 => r.left_x = obstacle.right_x,
        4 => r.bottom_y = obstacle.top_y,
        6 => r.right_x = obstacle.left_x,
        1 => r.upper_left_diagonal_x = obstacle.lower_right_diagonal_x,
        3 => r.lower_left_diagonal_x = obstacle.upper_right_diagonal_x,
        5 => r.lower_right_diagonal_x = obstacle.upper_left_diagonal_x,
        7 => r.upper_right_diagonal_x = obstacle.lower_left_diagonal_x,
        _ => {}
    }
    r.normalize()
}

/// `room` intersected with the half plane inside border line `line_no` of
/// `obstacle`, normalized (Java `calcInsideRestrainedShape`).
pub(super) fn calc_inside_restrained_shape(
    obstacle: &IntOctagon,
    line_no: usize,
    room: &IntOctagon,
) -> IntOctagon {
    let mut r = *room;
    match line_no {
        0 => r.bottom_y = obstacle.bottom_y,
        2 => r.right_x = obstacle.right_x,
        4 => r.top_y = obstacle.top_y,
        6 => r.left_x = obstacle.left_x,
        1 => r.lower_right_diagonal_x = obstacle.lower_right_diagonal_x,
        3 => r.upper_right_diagonal_x = obstacle.upper_right_diagonal_x,
        5 => r.upper_left_diagonal_x = obstacle.upper_left_diagonal_x,
        7 => r.lower_left_diagonal_x = obstacle.lower_left_diagonal_x,
        _ => {}
    }
    r.normalize()
}
