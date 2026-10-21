//! K13.9: the drill-page split skips the cutouts of pieces whose bounding boxes are
//! apart from the hole piece, and a simplex keeps its first enlargement. Both are
//! speed-ups that must give back what the full computation gives.
//!
//! - [`PolylineArea::split_to_convex_counted`] against
//!   [`PolylineArea::split_to_convex_reference`] (every hole piece cut out of every
//!   piece, as Java does): the same pieces, in the same order and representation,
//!   and the same [`SplitReach`] counts, on a page with the trace segments of a
//!   polyline arc as holes (the shape of K13.8's 3000-point arcs) and on seeded
//!   random holes;
//! - [`TileShape::cutout_apart`] against [`TileShape::cutout_counted`] for every kind
//!   pair whose boxes are apart;
//! - [`Simplex::enlarge`] against [`Simplex::enlarge_uncached`], first and repeated
//!   offsets;
//! - [`IntBox::is_apart_from`] at its one-unit margin and at the coordinate limits.

use freerouting_geometry::jmath::JavaRandom;
use freerouting_geometry::{
    IntBox, IntOctagon, IntPoint, Line, Point, PolylineArea, PolylineShape, PolylineShapeOps,
    Simplex, SplitQuirks, SplitReach, TileShape,
};

const QUIRK_SETS: [SplitQuirks; 2] = [
    SplitQuirks::JAVA,
    SplitQuirks {
        simplex_prev_division_line: false,
        octagon_box_cutout_branch3: false,
        cutout_hole_crosses_border: false,
        simplex_octagon_half_corner: false,
    },
];

fn int_in(rnd: &mut JavaRandom, lo: i32, hi: i32) -> i32 {
    lo + rnd.next_int(hi - lo + 1)
}

fn convex(corners: &[IntPoint]) -> TileShape {
    let points: Vec<Point> = corners.iter().copied().map(Point::from).collect();
    TileShape::get_instance_from_points(&points)
}

/// The shape of a straight trace segment from `a` to `b` of half width `w`: a
/// quadrilateral, a simplex unless it is axis-parallel.
fn segment_shape(a: (f64, f64), b: (f64, f64), w: f64) -> TileShape {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = dx.hypot(dy);
    let (nx, ny) = (-dy / len * w, dx / len * w);
    let p = |x: f64, y: f64| IntPoint::new(x.round() as i32, y.round() as i32);
    convex(&[
        p(a.0 + nx, a.1 + ny),
        p(a.0 - nx, a.1 - ny),
        p(b.0 - nx, b.1 - ny),
        p(b.0 + nx, b.1 + ny),
    ])
}

/// A page with the segments of a quarter-circle arc of `segments` pieces as holes,
/// as a board's trace arc laid as a polyline puts them into a drill page.
fn arc_page(segments: usize) -> PolylineArea {
    let page = TileShape::IntBox(IntBox::new(0, 0, 400_000, 400_000));
    let (cx, cy, r) = (50_000.0, 50_000.0, 300_000.0);
    let corner = |i: usize| {
        let a = std::f64::consts::FRAC_PI_2 * i as f64 / segments as f64;
        (cx + r * a.cos(), cy + r * a.sin())
    };
    let mut holes: Vec<PolylineShape> = (0..segments)
        .map(|i| PolylineShape::from(segment_shape(corner(i), corner(i + 1), 1_250.0)))
        .collect();
    // Pads and a via beside the arc, of the other two kinds.
    holes.push(PolylineShape::from(TileShape::IntBox(IntBox::new(
        20_000, 20_000, 60_000, 45_000,
    ))));
    holes.push(PolylineShape::from(TileShape::IntOctagon(
        IntOctagon::new(
            200_000, 150_000, 230_000, 180_000, 30_000, 70_000, 360_000, 400_000,
        )
        .normalize(),
    )));
    PolylineArea::new(PolylineShape::from(page), holes)
}

fn random_shape(rnd: &mut JavaRandom, cx: i32, cy: i32, r: i32) -> TileShape {
    match rnd.next_int(3) {
        0 => TileShape::IntBox(IntBox::new(cx - r, cy - r / 2, cx + r / 2, cy + r)),
        1 => TileShape::IntOctagon(
            IntOctagon::new(
                cx - r,
                cy - r,
                cx + r,
                cy + r,
                cx - cy - r,
                cx - cy + r,
                cx + cy - r,
                cx + cy + r,
            )
            .normalize(),
        ),
        _ => {
            let a = rnd.next_double() * std::f64::consts::TAU;
            let p = |t: f64| {
                IntPoint::new(
                    cx + (f64::from(r) * t.cos()).round() as i32,
                    cy + (f64::from(r) * t.sin()).round() as i32,
                )
            };
            convex(&[p(a), p(a + 2.1), p(a + 4.0)])
        }
    }
}

/// A random page with up to 40 random holes, some crossing the border.
fn random_page(rnd: &mut JavaRandom) -> PolylineArea {
    let page = TileShape::IntBox(IntBox::new(0, 0, 200_000, 150_000));
    let n = 1 + rnd.next_int(40);
    let holes = (0..n)
        .map(|_| {
            let cx = int_in(rnd, -5_000, 205_000);
            let cy = int_in(rnd, -5_000, 155_000);
            let r = int_in(rnd, 500, 12_000);
            PolylineShape::from(random_shape(rnd, cx, cy, r))
        })
        .collect();
    PolylineArea::new(PolylineShape::from(page), holes)
}

fn assert_split_matches_reference(area: &PolylineArea, what: &str) {
    for quirks in QUIRK_SETS {
        let mut fast_reach = SplitReach::default();
        let fast = area.split_to_convex_counted(None, quirks, &mut fast_reach);
        let mut reference_reach = SplitReach::default();
        let reference = area.split_to_convex_reference(quirks, &mut reference_reach);
        assert_eq!(
            format!("{fast:?}"),
            format!("{reference:?}"),
            "{what}: pieces differ ({quirks:?})"
        );
        assert_eq!(
            format!("{fast_reach:?}"),
            format!("{reference_reach:?}"),
            "{what}: reach differs ({quirks:?})"
        );
    }
}

#[test]
fn the_split_of_an_arc_page_matches_the_full_cutout() {
    let area = arc_page(300);
    assert_split_matches_reference(&area, "arc page");
    let pieces = area.split_to_convex(None).expect("the page splits");
    assert!(pieces.len() > 300, "{} pieces", pieces.len());
}

/// Issue756-tomu-fpga9 has a hole piece that is a one-line simplex with a zero
/// direction: dimension 2, unbounded, its bounding box the point at `i32::MAX`. Java
/// still cuts the pieces far from it into several, so the split may not skip them.
#[test]
fn a_one_line_simplex_hole_is_cut_out_of_pieces_far_from_it() {
    let p = IntPoint::new(932_346, -592_963);
    let half_plane = TileShape::Simplex(Simplex::new(vec![Line::new(p, p)]));
    assert_eq!(half_plane.dimension(), 2);
    let page = convex(&[
        IntPoint::new(712_500, -797_710),
        IntPoint::new(800_000, -797_710),
        IntPoint::new(800_000, -780_000),
        IntPoint::new(712_500, -790_000),
    ]);
    let area = PolylineArea::new(
        PolylineShape::from(page),
        vec![
            PolylineShape::from(TileShape::IntBox(IntBox::new(
                713_000, -797_000, 714_000, -796_000,
            ))),
            PolylineShape::from(half_plane),
        ],
    );
    assert_split_matches_reference(&area, "one-line simplex hole");
}

#[test]
fn the_split_of_random_pages_matches_the_full_cutout() {
    let mut rnd = JavaRandom::new(0x4B13_0009);
    for case in 0..150 {
        let area = random_page(&mut rnd);
        assert_split_matches_reference(&area, &format!("random page {case}"));
    }
}

#[test]
fn cutout_apart_is_the_cutout_of_a_shape_apart() {
    let mut rnd = JavaRandom::new(139);
    let mut checked = 0;
    for case in 0..3_000 {
        let r = int_in(&mut rnd, 1_000, 50_000);
        let outer = random_shape(&mut rnd, 0, 0, r);
        let cx = int_in(&mut rnd, -150_000, 150_000);
        let cy = int_in(&mut rnd, -150_000, 150_000);
        let r = int_in(&mut rnd, 1_000, 50_000);
        let hole = random_shape(&mut rnd, cx, cy, r);
        if hole.dimension() < 2 || !outer.bounding_box().is_apart_from(&hole.bounding_box()) {
            continue;
        }
        checked += 1;
        for quirks in QUIRK_SETS {
            let mut reach = SplitReach::default();
            let full = outer
                .cutout_counted(&hole, quirks, &mut reach)
                .expect("cutout");
            let apart = outer.cutout_apart(&hole);
            assert_eq!(
                format!("{full:?}"),
                format!("{:?}", vec![apart]),
                "case {case}: {outer:?} minus {hole:?}"
            );
            assert_eq!(format!("{reach:?}"), format!("{:?}", SplitReach::default()));
        }
    }
    assert!(checked > 1_000, "{checked} apart pairs");
}

#[test]
fn enlarge_returns_what_it_computes() {
    let mut rnd = JavaRandom::new(1_309);
    for _ in 0..500 {
        let TileShape::Simplex(simplex) = random_shape(&mut rnd, 1_000, -2_000, 20_000) else {
            continue;
        };
        let first = f64::from(int_in(&mut rnd, 1, 5_000));
        let second = first + 0.5;
        for offset in [first, first, second, second, first, 0.0] {
            assert_eq!(
                format!("{:?}", simplex.enlarge(offset)),
                format!("{:?}", simplex.enlarge_uncached(offset)),
                "{simplex:?} by {offset}"
            );
        }
        // A clone shares the kept result.
        let clone = simplex.clone();
        assert_eq!(
            format!("{:?}", clone.enlarge(first)),
            format!("{:?}", simplex.enlarge_uncached(first))
        );
    }
}

#[test]
fn enlarge_keeps_its_first_result() {
    let simplex = match convex(&[
        IntPoint::new(0, 0),
        IntPoint::new(10_000, 1_000),
        IntPoint::new(3_000, 9_000),
    ]) {
        TileShape::Simplex(s) => s,
        other => panic!("not a simplex: {other:?}"),
    };
    let a = simplex.enlarge(250.0);
    let b = simplex.enlarge(250.0);
    // The second call hands out the kept simplex: its storage is the first's.
    assert!(
        std::ptr::eq(a.lines().as_ptr(), b.lines().as_ptr()),
        "the second enlarge computed again"
    );
    let c = simplex.enlarge(300.0);
    let d = simplex.enlarge(300.0);
    assert!(!std::ptr::eq(c.lines().as_ptr(), d.lines().as_ptr()));
    let _: Simplex = c;
}

#[test]
fn boxes_are_apart_beyond_one_unit_each() {
    let a = IntBox::new(0, 0, 10, 10);
    // 10 + 1 < x - 1 from x = 13.
    assert!(!a.is_apart_from(&IntBox::new(12, 0, 20, 10)));
    assert!(a.is_apart_from(&IntBox::new(13, 0, 20, 10)));
    assert!(!a.is_apart_from(&IntBox::new(0, -12, 10, -2)));
    assert!(a.is_apart_from(&IntBox::new(0, -13, 10, -3)));
    assert!(IntBox::new(13, 0, 20, 10).is_apart_from(&a));
    assert!(!a.is_apart_from(&IntBox::EMPTY));
    assert!(!IntBox::EMPTY.is_apart_from(&a));
    // At the limits the sums do not overflow.
    let low = IntBox::new(i32::MIN, i32::MIN, i32::MIN + 1, i32::MIN + 1);
    let high = IntBox::new(i32::MAX - 1, i32::MAX - 1, i32::MAX, i32::MAX);
    assert!(low.is_apart_from(&high));
    assert!(high.is_apart_from(&low));
    let all = IntBox::new(i32::MIN, i32::MIN, i32::MAX, i32::MAX);
    assert!(!all.is_apart_from(&high));
    assert!(!low.is_apart_from(&all));
}
