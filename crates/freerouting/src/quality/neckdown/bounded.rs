//! Q02.3 (in part): the distance bound of the `bounded-neck` policy, as
//! pure integer functions. The width policy inside the maze search that
//! would use them, its stage registration and its insertion with
//! per-segment widths are not built: the A04 search has no per-room width
//! hook yet (its control carries one half width per layer, used by room
//! completion, the door checks, the path locator and the inserter), and
//! the measurement that decides whether the policy stays waits for P04
//! (see the checklist's Q02.3 note).
//!
//! **The rule** (checklist "Decisions"): a room is inside the neck of a pin
//! when its bounding box meets the pin's bounding box enlarged by D on
//! every side (a gap of at most D, so a gap of exactly D is inside). D is
//! the distance from the pin's box to the far side of its component's
//! courtyard (the package outline's bounding box; the pin's own box when
//! the DSN gives no outline), plus one pin pitch of that component. The
//! neck width is `max(2·trace_neckdown_half_width(layer), class minimum on
//! the layer, board minimum track width)`: never below the board's rules
//! (D8). All of it is in integer board units, with the sums in `i64`
//! (exact for any `i32` box).

use freerouting_geometry::{IntBox, IntPoint};

/// `b` enlarged by `d` on every side, in `i64` (exact for any `i32` input).
fn enlarged(b: &IntBox, d: i32) -> [i64; 4] {
    let d = i64::from(d);
    [
        i64::from(b.ll.x) - d,
        i64::from(b.ll.y) - d,
        i64::from(b.ur.x) + d,
        i64::from(b.ur.y) + d,
    ]
}

/// True if `room` meets `pin` enlarged by `d` (touching counts).
pub fn room_within(room: &IntBox, pin: &IntBox, d: i32) -> bool {
    let [x0, y0, x1, y1] = enlarged(pin, d);
    i64::from(room.ur.x) >= x0
        && i64::from(room.ll.x) <= x1
        && i64::from(room.ur.y) >= y0
        && i64::from(room.ll.y) <= y1
}

/// The neck distance D of a pin with box `pin`: the largest distance from
/// a side of `pin` to the same side of `courtyard` (0 without a courtyard
/// or when the pin sticks out), plus `pitch`. Saturates at `i32::MAX`.
pub fn neck_distance(pin: &IntBox, courtyard: Option<&IntBox>, pitch: i32) -> i32 {
    let reach = courtyard.map_or(0, |c| {
        [
            i64::from(pin.ll.x) - i64::from(c.ll.x),
            i64::from(pin.ll.y) - i64::from(c.ll.y),
            i64::from(c.ur.x) - i64::from(pin.ur.x),
            i64::from(c.ur.y) - i64::from(pin.ur.y),
        ]
        .into_iter()
        .max()
        .unwrap_or(0)
        .max(0)
    });
    let d = reach + i64::from(pitch.max(0));
    i32::try_from(d).unwrap_or(i32::MAX)
}

/// The pitch of a pin at `center` among the other pin centres of its
/// component: the smallest Euclidean distance to another centre, rounded
/// up to a board unit (0 for a component with one pin). The squared
/// distance is exact in `i128`; the root is the integer ceiling.
pub fn pitch(center: IntPoint, others: &[IntPoint]) -> i32 {
    let best = others
        .iter()
        .filter(|p| **p != center)
        .map(|p| {
            let dx = i128::from(p.x) - i128::from(center.x);
            let dy = i128::from(p.y) - i128::from(center.y);
            dx * dx + dy * dy
        })
        .min();
    best.map_or(0, |sq| {
        i32::try_from(ceil_sqrt(sq as u128)).unwrap_or(i32::MAX)
    })
}

/// The smallest `r` with `r * r >= n`.
fn ceil_sqrt(n: u128) -> u128 {
    if n == 0 {
        return 0;
    }
    let mut r = (n as f64).sqrt() as u128;
    while r * r < n {
        r += 1;
    }
    while r > 0 && (r - 1) * (r - 1) >= n {
        r -= 1;
    }
    r
}

/// The neck width on a layer: `max(2·pin_neck_half_width, class_min,
/// board_min)`, board units. Saturates at `i32::MAX`.
pub fn neck_width(pin_neck_half_width: i32, class_min: i32, board_min: i32) -> i32 {
    pin_neck_half_width
        .saturating_mul(2)
        .max(class_min)
        .max(board_min)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bx(x0: i32, y0: i32, x1: i32, y1: i32) -> IntBox {
        IntBox::new(x0, y0, x1, y1)
    }

    #[test]
    fn ceil_sqrt_is_exact_at_squares_and_between() {
        assert_eq!(ceil_sqrt(0), 0);
        assert_eq!(ceil_sqrt(1), 1);
        assert_eq!(ceil_sqrt(24), 5);
        assert_eq!(ceil_sqrt(25), 5);
        assert_eq!(ceil_sqrt(26), 6);
        let big = u128::from(u32::MAX) * u128::from(u32::MAX);
        assert_eq!(ceil_sqrt(big), u128::from(u32::MAX));
        assert_eq!(ceil_sqrt(big + 1), u128::from(u32::MAX) + 1);
    }

    #[test]
    fn pitch_is_the_nearest_other_centre() {
        let c = IntPoint::new(0, 0);
        let others = [c, IntPoint::new(5000, 0), IntPoint::new(3000, 4000)];
        assert_eq!(pitch(c, &others), 5000);
        assert_eq!(pitch(c, &[c]), 0);
        assert_eq!(pitch(c, &[IntPoint::new(1, 1)]), 2);
    }

    #[test]
    fn neck_distance_reaches_the_far_courtyard_side_plus_a_pitch() {
        let pin = bx(-125, -350, 125, 350);
        let courtyard = bx(-2500, -2500, 2500, 3000);
        assert_eq!(neck_distance(&pin, Some(&courtyard), 5000), 2650 + 5000);
        assert_eq!(neck_distance(&pin, None, 5000), 5000);
        let small = bx(-10, -10, 10, 10);
        assert_eq!(neck_distance(&pin, Some(&small), 0), 0);
    }

    #[test]
    fn neck_width_never_goes_below_the_rules() {
        assert_eq!(neck_width(1249, 2000, 1500), 2498);
        assert_eq!(neck_width(500, 2000, 1500), 2000);
        assert_eq!(neck_width(500, 1000, 1500), 1500);
        assert_eq!(neck_width(i32::MAX, 0, 0), i32::MAX);
    }

    /// The room predicate at gaps of D − 1, D and D + 1 units, on each of
    /// the four sides of the pin and diagonally.
    #[test]
    fn room_predicate_at_d_minus_one_d_and_d_plus_one() {
        let pin = bx(-125, -350, 125, 350);
        let d = neck_distance(&pin, Some(&bx(-2500, -2500, 2500, 2500)), 500);
        assert_eq!(d, 2375 + 500);
        for (gap, inside) in [(d - 1, true), (d, true), (d + 1, false)] {
            let right = bx(125 + gap, -10, 125 + gap + 300, 10);
            let left = bx(-125 - gap - 300, -10, -125 - gap, 10);
            let above = bx(-10, 350 + gap, 10, 350 + gap + 300);
            let below = bx(-10, -350 - gap - 300, 10, -350 - gap);
            let corner = bx(125 + gap, 350 + gap, 125 + gap + 1, 350 + gap + 1);
            for room in [right, left, above, below, corner] {
                assert_eq!(room_within(&room, &pin, d), inside, "gap {gap}: {room:?}");
            }
        }
        // A room covering the pin is always inside.
        assert!(room_within(&bx(-1000, -1000, 1000, 1000), &pin, 0));
    }

    #[test]
    fn room_predicate_is_exact_at_the_coordinate_limits() {
        let pin = bx(i32::MAX - 10, i32::MAX - 10, i32::MAX, i32::MAX);
        let room = bx(i32::MIN, i32::MIN, i32::MIN + 1, i32::MIN + 1);
        assert!(!room_within(&room, &pin, i32::MAX));
        let gap = i64::from(i32::MAX - 10) - i64::from(i32::MIN + 1);
        assert!(gap > i64::from(i32::MAX), "the gap exceeds any i32 D");
        let near = bx(-1, -1, 0, 0);
        assert!(room_within(&near, &pin, i32::MAX));
        assert!(!room_within(&near, &pin, i32::MAX - 11));
        assert!(room_within(&near, &pin, i32::MAX - 10));
    }

    #[test]
    fn neck_distance_saturates_at_the_coordinate_limits() {
        let pin = bx(i32::MAX - 1, i32::MAX - 1, i32::MAX, i32::MAX);
        let courtyard = bx(i32::MIN, i32::MIN, i32::MAX, i32::MAX);
        assert_eq!(neck_distance(&pin, Some(&courtyard), i32::MAX), i32::MAX);
    }
}
