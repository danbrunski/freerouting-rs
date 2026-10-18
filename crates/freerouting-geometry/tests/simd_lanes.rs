//! P05.1/P05.2: the lane tests of `simd::BoundLanes` give the answers of the
//! scalar predicates they replace in the search tree.
//!
//! - `BoundLanes::octagons_intersect` against `IntOctagon::intersects_octagon`;
//! - `BoundLanes::boxes_intersect` against `IntBox::intersects_box`;
//! - `PairLanes::octagons_intersect` and `PairLanes::boxes_intersect` (two
//!   shapes against one query) against the same predicates per shape.
//!
//! One million random pairs of each: on a small grid (touching and empty
//! shapes are frequent), near `CRIT_INT`, at the `i32` limits, and from the
//! constants (`EMPTY`, single points, the whole plane).

use freerouting_geometry::jmath::JavaRandom;
use freerouting_geometry::simd::{BoundLanes, PairLanes};
use freerouting_geometry::{CRIT_INT, IntBox, IntOctagon};

const PAIRS: usize = 1_000_000;

/// A coordinate: mostly on a small grid, sometimes near `CRIT_INT` or at the
/// `i32` limits.
fn coordinate(random: &mut JavaRandom) -> i32 {
    match random.next_int(16) {
        0 => [i32::MIN, i32::MAX, i32::MIN + 1, i32::MAX - 1, 0, -1][random.next_int(6) as usize],
        1 => [CRIT_INT, -CRIT_INT, CRIT_INT - 1, 1 - CRIT_INT][random.next_int(4) as usize],
        2 | 3 => random.next_int(2 * CRIT_INT + 1) - CRIT_INT,
        4 => random.next_int_unbounded(),
        _ => random.next_int(41) - 20,
    }
}

fn random_octagon(random: &mut JavaRandom) -> IntOctagon {
    match random.next_int(40) {
        0 => IntOctagon::EMPTY,
        1 => {
            let (x, y) = (coordinate(random), coordinate(random));
            IntBox::new(x, y, x, y).to_int_octagon()
        }
        2 => IntOctagon::new(
            i32::MIN,
            i32::MIN,
            i32::MAX,
            i32::MAX,
            i32::MIN,
            i32::MAX,
            i32::MIN,
            i32::MAX,
        ),
        _ => IntOctagon::new(
            coordinate(random),
            coordinate(random),
            coordinate(random),
            coordinate(random),
            coordinate(random),
            coordinate(random),
            coordinate(random),
            coordinate(random),
        ),
    }
}

/// An octagon whose bounds are mostly consistent (a box with its corners
/// cut), so that most pairs are decided by a late lane.
fn random_cut_box_octagon(random: &mut JavaRandom) -> IntOctagon {
    let x = random.next_int(31) - 15;
    let y = random.next_int(31) - 15;
    let (rx, ty) = (x + random.next_int(30) - 3, y + random.next_int(30) - 3);
    let mut cut = || random.next_int(12);
    IntOctagon::new(
        x,
        y,
        rx,
        ty,
        x - ty + cut(),
        rx - y - cut(),
        x + y + cut(),
        rx + ty - cut(),
    )
}

fn random_box(random: &mut JavaRandom) -> IntBox {
    match random.next_int(40) {
        0 => IntBox::EMPTY,
        1 => IntBox::new(i32::MIN, i32::MIN, i32::MAX, i32::MAX),
        _ => IntBox::new(
            coordinate(random),
            coordinate(random),
            coordinate(random),
            coordinate(random),
        ),
    }
}

#[test]
fn octagon_lanes_equal_intersects_octagon() {
    let mut random = JavaRandom::new(51);
    let (mut hits, mut misses) = (0usize, 0usize);
    for i in 0..PAIRS {
        let (a, b) = if i % 2 == 0 {
            (random_octagon(&mut random), random_octagon(&mut random))
        } else {
            (
                random_cut_box_octagon(&mut random),
                random_cut_box_octagon(&mut random),
            )
        };
        let expected = a.intersects_octagon(&b);
        let (la, lb) = (BoundLanes::from_octagon(&a), BoundLanes::from_octagon(&b));
        assert_eq!(la.octagons_intersect(&lb), expected, "{a:?} {b:?}");
        assert_eq!(lb.octagons_intersect(&la), expected, "{b:?} {a:?}");
        if expected {
            hits += 1;
        } else {
            misses += 1;
        }
    }
    assert!(
        hits > PAIRS / 20 && misses > PAIRS / 20,
        "{hits} hits, {misses} misses"
    );
}

#[test]
fn box_lanes_equal_intersects_box() {
    let mut random = JavaRandom::new(52);
    let (mut hits, mut misses) = (0usize, 0usize);
    for _ in 0..PAIRS {
        let (a, b) = (random_box(&mut random), random_box(&mut random));
        let expected = a.intersects_box(&b);
        let (la, lb) = (BoundLanes::from_box(&a), BoundLanes::from_box(&b));
        assert_eq!(la.boxes_intersect(&lb), expected, "{a:?} {b:?}");
        assert_eq!(lb.boxes_intersect(&la), expected, "{b:?} {a:?}");
        if expected {
            hits += 1;
        } else {
            misses += 1;
        }
    }
    assert!(
        hits > PAIRS / 20 && misses > PAIRS / 20,
        "{hits} hits, {misses} misses"
    );
}

#[test]
fn edge_constants() {
    let all = [
        IntOctagon::EMPTY,
        IntBox::new(0, 0, 0, 0).to_int_octagon(),
        IntBox::new(-CRIT_INT, -CRIT_INT, CRIT_INT, CRIT_INT).to_int_octagon(),
        IntOctagon::new(
            i32::MIN,
            i32::MIN,
            i32::MAX,
            i32::MAX,
            i32::MIN,
            i32::MAX,
            i32::MIN,
            i32::MAX,
        ),
        IntOctagon::new(
            i32::MAX,
            i32::MAX,
            i32::MIN,
            i32::MIN,
            i32::MAX,
            i32::MIN,
            i32::MAX,
            i32::MIN,
        ),
    ];
    for a in &all {
        for b in &all {
            let (la, lb) = (BoundLanes::from_octagon(a), BoundLanes::from_octagon(b));
            assert_eq!(
                la.octagons_intersect(&lb),
                a.intersects_octagon(b),
                "{a:?} {b:?}"
            );
        }
    }
    let boxes = [
        IntBox::EMPTY,
        IntBox::new(0, 0, 0, 0),
        IntBox::new(i32::MIN, i32::MIN, i32::MAX, i32::MAX),
        IntBox::new(i32::MAX, i32::MAX, i32::MIN, i32::MIN),
    ];
    for a in &boxes {
        for b in &boxes {
            let (la, lb) = (BoundLanes::from_box(a), BoundLanes::from_box(b));
            assert_eq!(la.boxes_intersect(&lb), a.intersects_box(b), "{a:?} {b:?}");
        }
    }
}

/// The answer bits of a pair against one query, from the scalar predicates.
fn expected_octagon_bits(first: &IntOctagon, second: &IntOctagon, q: &IntOctagon) -> u32 {
    u32::from(first.intersects_octagon(q)) | (u32::from(second.intersects_octagon(q)) << 1)
}

fn expected_box_bits(first: &IntBox, second: &IntBox, q: &IntBox) -> u32 {
    u32::from(first.intersects_box(q)) | (u32::from(second.intersects_box(q)) << 1)
}

#[test]
fn pair_lanes_equal_the_scalar_predicates() {
    let mut random = JavaRandom::new(53);
    let mut seen = [0usize; 4];
    for i in 0..PAIRS {
        let octagon = |r: &mut JavaRandom| {
            if i % 2 == 0 {
                random_octagon(r)
            } else {
                random_cut_box_octagon(r)
            }
        };
        let (a, b, q) = (
            octagon(&mut random),
            octagon(&mut random),
            octagon(&mut random),
        );
        let expected = expected_octagon_bits(&a, &b, &q);
        let pair = PairLanes::new(&BoundLanes::from_octagon(&a), &BoundLanes::from_octagon(&b));
        let query = PairLanes::splat(&BoundLanes::from_octagon(&q));
        assert_eq!(
            pair.octagons_intersect(&query),
            expected,
            "{a:?} {b:?} {q:?}"
        );
        seen[expected as usize] += 1;

        let (a, b, q) = (
            random_box(&mut random),
            random_box(&mut random),
            random_box(&mut random),
        );
        let pair = PairLanes::new(&BoundLanes::from_box(&a), &BoundLanes::from_box(&b));
        let query = PairLanes::splat(&BoundLanes::from_box(&q));
        assert_eq!(
            pair.boxes_intersect(&query),
            expected_box_bits(&a, &b, &q),
            "{a:?} {b:?} {q:?}"
        );
    }
    assert!(
        seen.iter().all(|&n| n > PAIRS / 200),
        "answer bits seen: {seen:?}"
    );
}
