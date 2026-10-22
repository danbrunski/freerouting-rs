//! K15.2: `router.java_quirks.placed_shape_convex`, the convex hull of a placed
//! tile shape's rounded corners.
//!
//! `PlacementTransform.shape` (`PlacementTransform.java:54-68`) adds the
//! placement in floating point, rounds each corner and hands the corners to
//! `TileShape.getInstance`, which takes them for a convex polygon. Where short
//! edges round to reflex vertices, the half-planes of the corners' edges give a
//! cut shape (a pad loses corners) or an unbounded one (C65's fabrication outline
//! in bug 1 of `docs/development/pcu-rev2-board-report.md`). `java_corners =
//! true`, the switch unset, keeps that; `false` takes the convex hull of the
//! rounded corners first.

use freerouting_geometry::shapes::{PolygonShape, Shape, Simplex, TileShape};
use freerouting_geometry::{Area, FloatPoint, IntPoint, Line, PlacementTransform, Point};

/// C65's F.Fab circle as the board holds it relative to the footprint: the
/// enlarged simplex of the reader's polyline outline, read from
/// `crates/freerouting/tests/data/k15/c65-nm-position.kicad10.dsn` (each line as
/// its two defining points `ax, ay, bx, by`).
const C65_LINES: [(i32, i32, i32, i32); 77] = [
    (0, -50488, 1, -50488),
    (-1106, -50488, 1105, -50439),
    (3316, -50393, 4415, -50271),
    (7712, -49912, 12048, -49037),
    (12048, -49048, 16290, -47793),
    (16290, -47810, 20404, -46185),
    (20404, -46206, 24359, -44224),
    (24359, -44253, 28123, -41930),
    (28123, -41966, 31666, -39318),
    (31666, -39365, 33314, -37890),
    (71413, 0, 71414, 1),
    (35647, -35744, 37158, -34129),
    (38619, -32514, 41344, -29030),
    (41305, -29030, 43712, -25319),
    (43682, -25319, 45751, -21409),
    (45727, -21409, 47442, -17332),
    (47425, -17332, 48773, -13119),
    (48760, -13119, 49731, -8803),
    (49723, -8803, 50308, -4419),
    (50304, -4419, 50500, 0),
    (50500, 0, 50304, 4419),
    (50308, 4419, 49723, 8803),
    (49731, 8803, 48760, 13119),
    (48773, 13119, 47425, 17332),
    (47442, 17332, 45727, 21409),
    (45751, 21409, 43682, 25319),
    (43712, 25319, 41305, 29030),
    (41344, 29030, 38619, 32514),
    (38669, 32514, 37158, 34129),
    (71413, 0, 71412, 1),
    (34962, 36415, 33314, 37890),
    (31666, 39318, 28123, 41966),
    (28123, 41930, 24359, 44253),
    (24359, 44224, 20404, 46206),
    (20404, 46185, 16290, 47810),
    (16290, 47793, 12048, 49048),
    (12048, 49037, 7712, 49912),
    (7712, 49905, 6613, 50027),
    (3316, 50390, 1105, 50439),
    (0, 50488, -1, 50488),
    (-1106, 50489, -3313, 50342),
    (-5520, 50200, -7705, 49859),
    (-9890, 49527, -14183, 48461),
    (-14183, 48475, -18365, 47034),
    (-18365, 47053, -22403, 45248),
    (-22403, 45273, -26266, 43118),
    (-26266, 43150, -28095, 41906),
    (-29924, 40703, -33347, 37902),
    (-33347, 37955, -34401, 36924),
    (-71379, 0, -71380, -1),
    (-37167, 34163, -40043, 30802),
    (-39999, 30802, -42568, 27201),
    (-42534, 27201, -44773, 23387),
    (-44746, 23387, -45693, 21388),
    (-46620, 19389, -48154, 15240),
    (-48139, 15240, -48429, 14173),
    (-49289, 10972, -50068, 6618),
    (-50062, 6618, -50453, 2212),
    (-50451, 2212, -50451, 2211),
    (-50453, -2212, -50062, -6618),
    (-50068, -6618, -49289, -10972),
    (-49299, -10972, -49009, -12039),
    (-48154, -15240, -46620, -19389),
    (-46640, -19389, -45693, -21388),
    (-44773, -23387, -42534, -27201),
    (-42568, -27201, -39999, -30802),
    (-40043, -30802, -37167, -34163),
    (-71379, 0, -71378, -1),
    (-36509, -34862, -35455, -35893),
    (-33347, -37902, -29924, -40703),
    (-29924, -40662, -28095, -41906),
    (-26266, -43118, -22403, -45273),
    (-22403, -45248, -18365, -47053),
    (-18365, -47034, -14183, -48475),
    (-14183, -48461, -9890, -49527),
    (-9890, -49518, -7705, -49859),
    (-5520, -50195, -3313, -50342),
];

/// C65's placement in that file: (57.251006, -105.242039) mm in board units,
/// on the front, not rotated.
const C65_LOCATION: FloatPoint = FloatPoint {
    x: 572510.06,
    y: -1052420.3900000001,
};

fn c65_outline() -> Shape {
    let lines = C65_LINES
        .iter()
        .map(|&(ax, ay, bx, by)| Line::new(IntPoint::new(ax, ay), IntPoint::new(bx, by)))
        .collect();
    Shape::from(TileShape::Simplex(Simplex::new(lines)))
}

/// A 24-gon pad of radius 2500 units (its corners rounded to the grid),
/// enlarged by 75 units, as the DSN reader builds a polygon pad drawn with an
/// aperture: the enlarged bounding octagon adds edges a few units long.
const PAD_CORNERS: [(i32, i32); 24] = [
    (0, -2500),
    (647, -2415),
    (1250, -2165),
    (1768, -1768),
    (2165, -1250),
    (2415, -647),
    (2500, 0),
    (2415, 647),
    (2165, 1250),
    (1768, 1768),
    (1250, 2165),
    (647, 2415),
    (0, 2500),
    (-647, 2415),
    (-1250, 2165),
    (-1768, 1768),
    (-2165, 1250),
    (-2415, 647),
    (-2500, 0),
    (-2415, -647),
    (-2165, -1250),
    (-1768, -1768),
    (-1250, -2165),
    (-647, -2415),
];

fn polygon_pad() -> Shape {
    let corners: Vec<Point> = PAD_CORNERS
        .iter()
        .map(|&(x, y)| Point::Int(IntPoint::new(x, y)))
        .collect();
    let TileShape::Simplex(pad) = TileShape::get_instance_from_points(&corners) else {
        panic!("a simplex pad");
    };
    Shape::from(TileShape::Simplex(pad.enlarge(75.0)))
}

/// The corners `shape` places to, rounded, as `PlacementTransform::shape` takes
/// them (no pin rotation), without repeats.
fn rounded_corners(t: &PlacementTransform, shape: &Shape, offset: FloatPoint) -> Vec<Point> {
    let mut unique: Vec<Point> = Vec::new();
    for corner in shape.corner_approx_arr() {
        let local = FloatPoint::new(offset.x + corner.x, offset.y + corner.y);
        let p = Point::Int(t.point(local).round());
        if !unique.contains(&p) {
            unique.push(p);
        }
    }
    unique
}

/// True if the rounded corners have a reflex vertex: their convex hull drops a
/// corner.
fn has_reflex_vertex(corners: &[Point]) -> bool {
    let polygon = PolygonShape::new(corners);
    polygon.convex_hull().corners().len() < polygon.corners().len()
}

/// The box of `points`.
fn corner_box(points: &[Point]) -> (i32, i32, i32, i32) {
    let ints: Vec<IntPoint> = points.iter().map(|p| p.expect_int()).collect();
    (
        ints.iter().map(|p| p.x).min().unwrap(),
        ints.iter().map(|p| p.y).min().unwrap(),
        ints.iter().map(|p| p.x).max().unwrap(),
        ints.iter().map(|p| p.y).max().unwrap(),
    )
}

fn same(a: &Shape, b: &Shape) -> bool {
    format!("{a:?}") == format!("{b:?}")
}

#[test]
fn c65s_outline_is_bounded_with_the_switch_off() {
    let outline = c65_outline();
    let t = PlacementTransform::new(C65_LOCATION, 0.0, false, false);
    let corners = rounded_corners(&t, &outline, FloatPoint::ZERO);
    assert!(
        has_reflex_vertex(&corners),
        "the rounding makes reflex vertices"
    );

    // Unset (`shape`) is Java's: the box reaches `i32::MAX`.
    let java = t.shape(&outline, FloatPoint::ZERO, 0.0);
    assert!(same(
        &java,
        &t.shape_with(&outline, FloatPoint::ZERO, 0.0, true)
    ));
    let java_box = java.bounding_box();
    assert!(java_box.has_infinite_coordinate(), "{java_box:?}");
    assert_eq!((java_box.ur.x, java_box.ur.y), (i32::MAX, i32::MAX));

    // `false`: the hull of the rounded corners, bounded by them.
    let fixed = t.shape_with(&outline, FloatPoint::ZERO, 0.0, false);
    assert!(fixed.is_bounded());
    let b = fixed.bounding_box();
    assert!(!b.has_infinite_coordinate(), "{b:?}");
    assert_eq!((b.ll.x, b.ll.y, b.ur.x, b.ur.y), corner_box(&corners));
    assert_eq!(
        (b.ll.x, b.ll.y, b.ur.x, b.ur.y),
        (522059, -1102908, 623010, -1001932)
    );
    for corner in &corners {
        assert!(fixed.contains(corner), "{corner:?} outside");
    }
    // A component outline is placed as an area.
    let area = Area::Shape(outline);
    assert!(
        t.area(&area).bounding_box().has_infinite_coordinate(),
        "area, unset"
    );
    let placed = t.area_with(&area, false);
    assert_eq!(placed.bounding_box(), b, "area, switch off");
}

/// The pad's position: one where the rounding gives reflex vertices (found by
/// a search over fractional positions; the pad is convex at an integer one).
const PAD_LOCATION: FloatPoint = FloatPoint {
    x: 100000.0,
    y: -50000.0,
};
const PAD_OFFSET: FloatPoint = FloatPoint { x: 0.0, y: 0.13 };

#[test]
fn a_pad_rounded_to_a_reflex_vertex_keeps_its_hull_with_the_switch_off() {
    let pad = polygon_pad();
    for mirrored in [false, true] {
        let t = PlacementTransform::new(PAD_LOCATION, 0.0, mirrored, false);
        let corners = rounded_corners(&t, &pad, PAD_OFFSET);
        assert!(has_reflex_vertex(&corners), "mirrored {mirrored}");

        // Unset is Java's: the edges' half-planes cut corners off the pad.
        let java = t.shape(&pad, PAD_OFFSET, 0.0);
        assert!(same(&java, &t.shape_with(&pad, PAD_OFFSET, 0.0, true)));
        assert!(java.is_bounded());
        let lost = corners.iter().filter(|c| !java.contains(c)).count();
        assert_eq!(lost, 8, "mirrored {mirrored}: Java cuts 8 corners off");

        // `false`: the convex hull, which keeps every corner.
        let fixed = t.shape_with(&pad, PAD_OFFSET, 0.0, false);
        for corner in &corners {
            assert!(fixed.contains(corner), "mirrored {mirrored}: {corner:?}");
        }
        let hull = PolygonShape::new(&corners).convex_hull();
        let mut want: Vec<IntPoint> = hull.corners().iter().map(|p| p.expect_int()).collect();
        let mut got: Vec<IntPoint> = fixed
            .corner_approx_arr()
            .iter()
            .map(|c| c.round())
            .collect();
        want.sort_by_key(|p| (p.x, p.y));
        got.sort_by_key(|p| (p.x, p.y));
        assert_eq!(got, want, "mirrored {mirrored}: the hull's corners");
    }
}

/// Where the rounded corners stay convex, both values give the same shape.
#[test]
fn convex_placements_are_the_same_under_both_values() {
    let mut convex = 0;
    let mut reflex = 0;
    for shape in [polygon_pad(), c65_outline()] {
        for i in 0..20 {
            for rotation in [0.0, 90.0, 37.0] {
                let location = FloatPoint::new(
                    100000.0 + f64::from(i) * 0.17,
                    -50000.0 + f64::from(i) * 0.13,
                );
                let t = PlacementTransform::new(location, rotation, i % 2 == 1, false);
                let java = t.shape_with(&shape, FloatPoint::ZERO, 0.0, true);
                let fixed = t.shape_with(&shape, FloatPoint::ZERO, 0.0, false);
                let mut corners = Vec::new();
                for corner in shape.corner_approx_arr() {
                    let p = Point::Int(t.point(corner).round());
                    if !corners.contains(&p) {
                        corners.push(p);
                    }
                }
                if has_reflex_vertex(&corners) {
                    reflex += 1;
                    assert!(fixed.is_bounded());
                    for corner in &corners {
                        assert!(fixed.contains(corner));
                    }
                } else {
                    convex += 1;
                    assert!(same(&java, &fixed), "{location:?} {rotation}");
                }
            }
        }
    }
    assert!(convex > 0 && reflex > 0, "convex {convex}, reflex {reflex}");
}
