//! P02.2: the tile-shape intersection predicates that skip building
//! shapes give the results of the paths they replace.
//!
//! - `Simplex::intersects_simplex`, `intersects_octagon_simplex` and
//!   `intersects_box_simplex` test the emptiness of the intersection in a
//!   stack buffer; the reference is `!intersection_simplex(..).is_empty()`
//!   on the built simplexes, as before.
//! - `TileShape::intersects_tile` dispatches directly; the reference is
//!   `intersects(&Shape::from(other))`.
//!
//! Random shapes on a small grid, so that touching, parallel, coincident
//! and empty cases are frequent, and on a wide one near the coordinate
//! limit.

use freerouting_geometry::jmath::JavaRandom;
use freerouting_geometry::{
    IntBox, IntOctagon, IntPoint, Line, PolylineShapeOps, Shape, Simplex, TileShape, TileShapeOps,
};

fn coordinate(random: &mut JavaRandom, range: i32) -> i32 {
    random.next_int(2 * range + 1) - range
}

fn random_box(random: &mut JavaRandom, range: i32) -> IntBox {
    let x = coordinate(random, range);
    let y = coordinate(random, range);
    // Sometimes empty (upper corner below or left of the lower one).
    let w = random.next_int(range) - range / 8;
    let h = random.next_int(range) - range / 8;
    IntBox::new(x, y, x.saturating_add(w), y.saturating_add(h))
}

fn random_octagon(random: &mut JavaRandom, range: i32) -> IntOctagon {
    let b = random_box(random, range);
    let cut = |r: &mut JavaRandom| r.next_int(range / 2 + 1);
    let (lx, by, rx, ty) = (b.ll.x, b.ll.y, b.ur.x, b.ur.y);
    IntOctagon::new(
        lx,
        by,
        rx,
        ty,
        lx.saturating_sub(ty).saturating_add(cut(random)),
        rx.saturating_sub(by).saturating_sub(cut(random)),
        lx.saturating_add(by).saturating_add(cut(random)),
        rx.saturating_add(ty).saturating_sub(cut(random)),
    )
    .normalize()
}

fn random_simplex(random: &mut JavaRandom, range: i32) -> Simplex {
    let count = 3 + random.next_int(4) as usize;
    let mut lines = Vec::with_capacity(count);
    while lines.len() < count {
        let a = IntPoint::new(coordinate(random, range), coordinate(random, range));
        let b = IntPoint::new(coordinate(random, range), coordinate(random, range));
        if a != b {
            lines.push(Line::new(a, b));
        }
    }
    Simplex::get_instance(&lines)
}

fn random_tile(random: &mut JavaRandom, range: i32) -> TileShape {
    match random.next_int(3) {
        0 => TileShape::IntBox(random_box(random, range)),
        1 => TileShape::IntOctagon(random_octagon(random, range)),
        _ => TileShape::Simplex(random_simplex(random, range)),
    }
}

fn reference(a: &Simplex, b: &Simplex) -> bool {
    !a.intersection_simplex(b).is_empty()
}

fn check_ranges(seed: i64, range: i32, rounds: usize) -> (usize, usize) {
    let mut random = JavaRandom::new(seed);
    let mut hits = 0;
    for _ in 0..rounds {
        let s = random_simplex(&mut random, range);
        let t = random_simplex(&mut random, range);
        let o = random_octagon(&mut random, range);
        let b = random_box(&mut random, range);
        let expected = reference(&s, &t);
        assert_eq!(s.intersects_simplex(&t), expected, "{s:?} / {t:?}");
        hits += usize::from(expected);
        let expected = reference(&s, &o.to_simplex());
        assert_eq!(s.intersects_octagon_simplex(&o), expected, "{s:?} / {o:?}");
        let expected = reference(&s, &b.to_simplex());
        assert_eq!(s.intersects_box_simplex(&b), expected, "{s:?} / {b:?}");

        let x = random_tile(&mut random, range);
        let y = random_tile(&mut random, range);
        let expected = x.intersects(&Shape::from(y.clone()));
        assert_eq!(x.intersects_tile(&y), expected, "{x:?} / {y:?}");
    }
    (hits, rounds)
}

#[test]
fn predicates_equal_the_built_intersection_on_a_small_grid() {
    let (hits, rounds) = check_ranges(20261017, 12, 20_000);
    // Both outcomes are exercised.
    assert!(
        hits > rounds / 20 && hits < rounds - rounds / 20,
        "{hits} of {rounds}"
    );
}

#[test]
fn predicates_equal_the_built_intersection_near_the_coordinate_limit() {
    check_ranges(7, 400_000_000, 5_000);
}

#[test]
fn empty_shapes_never_intersect() {
    let empty = Simplex::empty();
    let unit = IntBox::new(0, 0, 10, 10);
    assert!(!empty.intersects_box_simplex(&unit));
    assert!(!unit.to_simplex().intersects_simplex(&empty));
    let empty_box = IntBox::new(10, 10, 0, 0);
    assert!(!unit.to_simplex().intersects_box_simplex(&empty_box));
    let touching = IntBox::new(10, 0, 20, 10);
    assert_eq!(
        unit.to_simplex().intersects_box_simplex(&touching),
        reference(&unit.to_simplex(), &touching.to_simplex())
    );
}
