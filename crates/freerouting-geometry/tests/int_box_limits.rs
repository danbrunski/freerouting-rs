//! K15.1: `IntBox::offset` at the `i32` limits, and the box of an unbounded
//! shape.
//!
//! Java's `IntBox.offset` (`IntBox.java:448-456`) adds in `int` arithmetic, so a
//! box that reaches `Integer.MAX_VALUE` wraps to a negative upper bound and the
//! offset box is empty. The port saturates (DIVERGENCES, "freerouting-geometry").
//! The box of an unbounded simplex has its infinite corners at `i32::MIN` /
//! `i32::MAX` (`Simplex::bounding_box` rounds them with `d2i`), which
//! `IntBox::has_infinite_coordinate` tells.

use freerouting_geometry::shapes::{IntBox, PolylineShapeOps, Simplex};
use freerouting_geometry::{IntPoint, Line};

#[test]
fn offset_saturates_at_the_upper_limit() {
    let b = IntBox::new(0, 0, i32::MAX, i32::MAX);
    assert_eq!(
        b.offset(1000.0),
        IntBox::new(-1000, -1000, i32::MAX, i32::MAX)
    );
    assert!(!b.offset(1000.0).is_empty());
    let near = IntBox::new(0, 0, i32::MAX - 10, i32::MAX - 999);
    assert_eq!(
        near.offset(1000.0),
        IntBox::new(-1000, -1000, i32::MAX, i32::MAX)
    );
}

#[test]
fn offset_saturates_at_the_lower_limit() {
    let b = IntBox::new(i32::MIN, i32::MIN + 5, 0, 0);
    assert_eq!(
        b.offset(1000.0),
        IntBox::new(i32::MIN, i32::MIN, 1000, 1000)
    );
    let whole = IntBox::new(i32::MIN, i32::MIN, i32::MAX, i32::MAX);
    assert_eq!(whole.offset(1000.0), whole);
}

#[test]
fn offset_inside_the_limits_is_unchanged() {
    let b = IntBox::new(-5, 3, 7, 11);
    assert_eq!(b.offset(2.0), IntBox::new(-7, 1, 9, 13));
    assert_eq!(b.offset(-2.0), IntBox::new(-3, 5, 5, 9));
    // a negative offset from the limits moves inside them
    let whole = IntBox::new(i32::MIN, i32::MIN, i32::MAX, i32::MAX);
    assert_eq!(
        whole.offset(-1.0),
        IntBox::new(i32::MIN + 1, i32::MIN + 1, i32::MAX - 1, i32::MAX - 1)
    );
    // an empty box is returned as it is, as in Java
    assert_eq!(IntBox::EMPTY.offset(1000.0), IntBox::EMPTY);
}

#[test]
fn infinite_coordinates_are_told() {
    assert!(!IntBox::new(-5, 3, 7, 11).has_infinite_coordinate());
    assert!(!IntBox::EMPTY.has_infinite_coordinate());
    assert!(IntBox::new(0, 0, i32::MAX, 5).has_infinite_coordinate());
    assert!(IntBox::new(0, 0, 5, i32::MAX).has_infinite_coordinate());
    assert!(IntBox::new(i32::MIN, 0, 5, 5).has_infinite_coordinate());
    assert!(IntBox::new(0, i32::MIN, 5, 5).has_infinite_coordinate());
}

#[test]
fn the_box_of_an_unbounded_simplex_has_infinite_coordinates() {
    // the strip -10 <= x <= 0 below y = 10, open downwards: the corner of the
    // two parallel lines is at infinity
    let lines = vec![
        Line::new(IntPoint::new(0, 0), IntPoint::new(0, 1)),
        Line::new(IntPoint::new(0, 10), IntPoint::new(-1, 10)),
        Line::new(IntPoint::new(-10, 10), IntPoint::new(-10, 9)),
    ];
    let simplex = Simplex::new(lines);
    assert!(!simplex.is_bounded());
    let b = simplex.bounding_box();
    assert!(b.has_infinite_coordinate(), "{b:?}");
}
