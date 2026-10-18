//! N06.4 (batch G): the convex pieces of a cutout tile the rest of the outer shape.
//!
//! Over random holes in boxes, octagons and simplexes, the pieces that
//! [`TileShape::cutout_with`] returns must satisfy two invariants:
//! - their areas sum to the outer shape's area minus the hole's;
//! - no two pieces overlap.
//!
//! The domain is the one the two switches of [`SplitQuirks`] are about: holes inside the
//! outer shape whose corners are integer points. There, Java's code breaks the invariant
//! only through `octagon_box_cutout_branch3` (`IntOctagon.java:1206-1223`), and with both
//! switches `false` it holds on every case. `simplex_prev_division_line` changes no piece
//! at all (its fix is inert; see `Simplex::cutout_from_counted`), which
//! [`the_simplex_switch_changes_no_piece`] checks over every case, inside the domain and
//! outside it.
//!
//! Two kinds of hole outside the domain break the invariant whatever the switches are;
//! neither is an inventoried row. [`java_breaks_the_invariant_outside_the_two_rows`] pins
//! one case of each:
//! - a hole crossing the outer border: corners on the border that are not integer points
//!   get no division line, and pieces go missing (the areas sum to less);
//! - an octagon whose diagonals meet at a half-integer point, cut out of a simplex: as a
//!   simplex it has a corner that is not an integer point, `calcDivisionLines` takes such
//!   a corner for one on the outer border and divides nothing there, and pieces overlap.
//!
//! One-unit overlaps along an octagon edge of length -1 (an odd-parity octagon after
//! `normalize`) come from rounding to the integer grid and are not counted
//! ([`SLIVER_WIDTH`]).
//!
//! The cases come from a seeded [`JavaRandom`], so every run checks the same shapes.

use freerouting_geometry::jmath::JavaRandom;
use freerouting_geometry::{
    IntBox, IntOctagon, IntPoint, Line, Point, PolylineShapeOps, Simplex, SplitQuirks, SplitReach,
    TileShape, TileShapeOps,
};

/// The number of random cases per outer/inner kind pair.
const CASES: usize = 600;

/// Coordinates stay well inside `CRIT_INT`, as on boards.
const EXTENT: i32 = 200_000;

/// The width of an overlap that comes from rounding to the integer grid, not from a
/// wrong piece: an octagon whose diagonals meet at a half-integer point keeps an edge
/// of length -1 after `normalize`, and the pieces on both sides of it share a strip one
/// unit wide.
const SLIVER_WIDTH: f64 = 1.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Box,
    Octagon,
    Simplex,
}

const KINDS: [Kind; 3] = [Kind::Box, Kind::Octagon, Kind::Simplex];

const ONLY_SIMPLEX_FIXED: SplitQuirks = SplitQuirks {
    simplex_prev_division_line: false,
    octagon_box_cutout_branch3: true,
};

const ONLY_OCTAGON_FIXED: SplitQuirks = SplitQuirks {
    simplex_prev_division_line: true,
    octagon_box_cutout_branch3: false,
};

/// A violation of the tiling invariant.
#[derive(Debug)]
#[allow(dead_code)] // read through `Debug` in the failure messages
enum Violation {
    /// The pieces' areas do not sum to outer minus inner.
    AreaSum { expected: f64, got: f64 },
    /// Pieces `i` and `j` overlap by `area`.
    Overlap { i: usize, j: usize, area: f64 },
}

fn int_in(rnd: &mut JavaRandom, lo: i32, hi: i32) -> i32 {
    lo + rnd.next_int(hi - lo + 1)
}

fn random_box(rnd: &mut JavaRandom, cx: i32, cy: i32, r: i32) -> IntBox {
    let w = int_in(rnd, r / 4, r);
    let h = int_in(rnd, r / 4, r);
    IntBox::new(cx - w, cy - h, cx + w, cy + h)
}

/// A box with up to four 45 degree corners cut off.
fn random_octagon(rnd: &mut JavaRandom, cx: i32, cy: i32, r: i32) -> IntOctagon {
    let b = random_box(rnd, cx, cy, r);
    let (lx, ly, rx, uy) = (b.ll.x, b.ll.y, b.ur.x, b.ur.y);
    let span = (rx - lx).min(uy - ly);
    let mut cut = || {
        if rnd.next_boolean() {
            int_in(rnd, 1, span)
        } else {
            0
        }
    };
    // x - y in [lx - uy, rx - ly], x + y in [lx + ly, rx + uy].
    let ulx = lx - uy + cut();
    let lrx = rx - ly - cut();
    let llx = lx + ly + cut();
    let urx = rx + uy - cut();
    IntOctagon::new(lx, ly, rx, uy, ulx, lrx, llx, urx).normalize()
}

/// A convex polygon with 3 to 6 corners on a circle, counterclockwise.
fn random_simplex(rnd: &mut JavaRandom, cx: i32, cy: i32, r: i32) -> TileShape {
    let n = 3 + rnd.next_int(4) as usize;
    let mut angles: Vec<f64> = (0..n)
        .map(|_| rnd.next_double() * std::f64::consts::TAU)
        .collect();
    angles.sort_by(f64::total_cmp);
    let mut corners: Vec<IntPoint> = Vec::with_capacity(n);
    for a in angles {
        let p = IntPoint::new(
            cx + (f64::from(r) * a.cos()).round() as i32,
            cy + (f64::from(r) * a.sin()).round() as i32,
        );
        if corners.last() != Some(&p) && corners.first() != Some(&p) {
            corners.push(p);
        }
    }
    if corners.len() < 3 {
        return TileShape::IntBox(IntBox::EMPTY);
    }
    let points: Vec<Point> = corners.into_iter().map(Point::from).collect();
    TileShape::get_instance_from_points(&points)
}

fn random_shape(rnd: &mut JavaRandom, kind: Kind, cx: i32, cy: i32, r: i32) -> TileShape {
    match kind {
        Kind::Box => TileShape::IntBox(random_box(rnd, cx, cy, r)),
        Kind::Octagon => TileShape::IntOctagon(random_octagon(rnd, cx, cy, r)),
        Kind::Simplex => random_simplex(rnd, cx, cy, r),
    }
}

/// One random case: an outer shape and a hole near its middle, sometimes crossing its
/// border.
fn random_case(rnd: &mut JavaRandom, outer: Kind, inner: Kind) -> (TileShape, TileShape) {
    let r = int_in(rnd, 2_000, EXTENT / 2);
    let cx = int_in(rnd, -EXTENT / 2, EXTENT / 2);
    let cy = int_in(rnd, -EXTENT / 2, EXTENT / 2);
    let outer_shape = random_shape(rnd, outer, cx, cy, r);
    let ir = int_in(rnd, r / 20, r * 3 / 4);
    let off = r / 2;
    let icx = cx + int_in(rnd, -off, off);
    let icy = cy + int_in(rnd, -off, off);
    let inner_shape = random_shape(rnd, inner, icx, icy, ir);
    (outer_shape, inner_shape)
}

/// Every random case with both shapes 2-dimensional, as `(outer kind, inner kind, outer,
/// hole)`.
fn all_cases() -> Vec<(Kind, Kind, TileShape, TileShape)> {
    let mut cases = Vec::new();
    for (oi, &outer) in KINDS.iter().enumerate() {
        for (ii, &inner) in KINDS.iter().enumerate() {
            let mut rnd = JavaRandom::new(0x4e06_0400 + (oi * 3 + ii) as i64);
            for _ in 0..CASES {
                let (o, h) = random_case(&mut rnd, outer, inner);
                if o.dimension() == 2 && h.dimension() == 2 {
                    cases.push((outer, inner, o, h));
                }
            }
        }
    }
    cases
}

fn area_of(shape: &TileShape) -> f64 {
    if shape.dimension() == 2 {
        shape.area()
    } else {
        0.0
    }
}

/// True if the hole lies in the outer shape and its corners, as a simplex, are integer
/// points: the domain of the two switches.
fn in_domain(outer: &TileShape, hole: &TileShape) -> bool {
    let simplex = hole.to_simplex();
    let exact = (0..simplex.border_line_count()).all(|i| {
        let c = simplex.corner_approx(i);
        c.x == c.x.round() && c.y == c.y.round()
    });
    exact && (area_of(&hole.intersection(outer)) - area_of(hole)).abs() < 1.0
}

fn pieces(outer: &TileShape, hole: &TileShape, quirks: SplitQuirks) -> Option<Vec<TileShape>> {
    let pieces = outer.cutout_with(hole, quirks)?;
    Some(pieces.into_iter().filter(|p| p.dimension() == 2).collect())
}

/// The invariant's violations for `hole` cut out of `outer`; `None` if the cutout fails
/// (Java's `null`).
fn violations(outer: &TileShape, hole: &TileShape, quirks: SplitQuirks) -> Option<Vec<Violation>> {
    let pieces = pieces(outer, hole, quirks)?;
    let outer_area = outer.area();
    // The pieces are bounded by lines through integer points; their corners are
    // rational, so the float areas carry rounding only.
    let tolerance = outer_area * 1e-9 + 1.0;
    let mut found = Vec::new();
    let mut slivers = 0.0;
    for i in 0..pieces.len() {
        for j in i + 1..pieces.len() {
            let overlap = pieces[i].intersection(&pieces[j]);
            if overlap.dimension() < 2 {
                continue;
            }
            let area = overlap.area();
            if overlap.min_width() <= SLIVER_WIDTH {
                slivers += area;
            } else if area > tolerance {
                found.push(Violation::Overlap { i, j, area });
            }
        }
    }
    let got: f64 = pieces.iter().map(TileShapeOps::area).sum();
    let expected = outer_area - area_of(&hole.intersection(outer));
    if (got - expected).abs() > tolerance + slivers {
        found.push(Violation::AreaSum { expected, got });
    }
    Some(found)
}

/// The cases of the domain that break the invariant under `quirks`, as `(outer kind,
/// inner kind, violations)`, and the number of domain cases checked.
fn broken_in_domain(quirks: SplitQuirks) -> (Vec<(Kind, Kind, Vec<Violation>)>, usize) {
    let mut broken = Vec::new();
    let mut checked = 0;
    for (outer_kind, inner_kind, o, h) in all_cases() {
        if !in_domain(&o, &h) {
            continue;
        }
        if let Some(v) = violations(&o, &h, quirks) {
            checked += 1;
            if !v.is_empty() {
                broken.push((outer_kind, inner_kind, v));
            }
        }
    }
    (broken, checked)
}

fn describe(pieces: &Option<Vec<TileShape>>) -> String {
    format!("{pieces:?}")
}

#[test]
fn fixed_cutouts_tile_the_rest_of_the_outer_shape() {
    let (broken, checked) = broken_in_domain(SplitQuirks::FIXED);
    println!(
        "both switches false: {} of {checked} domain cutouts broken",
        broken.len()
    );
    // Every kind pair contributes: about a third of the random holes are in the domain.
    assert!(checked > KINDS.len() * KINDS.len() * CASES / 5, "{checked}");
    assert!(
        broken.is_empty(),
        "{} of {checked} cutouts break the invariant with both switches false; first: {:?}",
        broken.len(),
        broken.first()
    );
}

#[test]
fn java_breaks_the_invariant_only_through_branch_3() {
    let (java, checked) = broken_in_domain(SplitQuirks::JAVA);
    println!("Java: {} of {checked} domain cutouts broken", java.len());
    assert!(
        java.len() * 20 > checked,
        "branch 3 should break many box cutouts: {} of {checked}",
        java.len()
    );
    // Branch 3 lives in an octagon cut out of a box; every broken case is one, and
    // each has an overlap.
    assert!(java.iter().all(|(o, i, v)| *o == Kind::Box
        && *i == Kind::Octagon
        && v.iter().any(|v| matches!(v, Violation::Overlap { .. }))));
    // The branch-3 fix alone repairs every one; the simplex switch alone repairs none.
    let (octagon_fixed, _) = broken_in_domain(ONLY_OCTAGON_FIXED);
    assert!(octagon_fixed.is_empty(), "{:?}", octagon_fixed.first());
    let (simplex_fixed, _) = broken_in_domain(ONLY_SIMPLEX_FIXED);
    assert_eq!(simplex_fixed.len(), java.len());
}

#[test]
fn the_simplex_switch_changes_no_piece() {
    let mut simplex_cutouts = 0;
    for (outer_kind, inner_kind, o, h) in all_cases() {
        for octagon in [true, false] {
            let java = SplitQuirks {
                simplex_prev_division_line: true,
                octagon_box_cutout_branch3: octagon,
            };
            let fixed = SplitQuirks {
                simplex_prev_division_line: false,
                ..java
            };
            let mut reach = SplitReach::default();
            let fixed_pieces = o.cutout_counted(&h, fixed, &mut reach);
            assert_eq!(
                describe(&o.cutout_with(&h, java)),
                describe(&fixed_pieces),
                "{outer_kind:?} minus {inner_kind:?}: {o:?} {h:?}"
            );
            assert_eq!(reach.simplex_prev_division_line, 0);
        }
        if outer_kind == Kind::Simplex || inner_kind == Kind::Simplex {
            simplex_cutouts += 1;
        }
    }
    println!("{simplex_cutouts} cutouts with a simplex, identical under both values");
    assert!(simplex_cutouts > 2 * CASES);
}

/// The branch-3 counter counts exactly the cutouts whose pieces the switch changes.
#[test]
fn the_branch_3_counter_counts_the_changed_cutouts() {
    let (mut changed, mut counted) = (0, 0);
    for (_, _, o, h) in all_cases() {
        let mut java_reach = SplitReach::default();
        let mut fixed_reach = SplitReach::default();
        let java = o.cutout_counted(&h, SplitQuirks::JAVA, &mut java_reach);
        let fixed = o.cutout_counted(&h, ONLY_OCTAGON_FIXED, &mut fixed_reach);
        // The count looks at Java's state and the fix side by side: both arms agree.
        assert_eq!(java_reach, fixed_reach);
        let differs = describe(&java) != describe(&fixed);
        assert_eq!(
            differs,
            java_reach.octagon_box_cutout_branch3 > 0,
            "{o:?} {h:?}"
        );
        changed += usize::from(differs);
        counted += java_reach.octagon_box_cutout_branch3;
    }
    println!("branch 3 changes {changed} cutouts, counted {counted}");
    assert!(changed > 0);
    assert_eq!(counted, changed as u64, "one branch 3 per cutout");
}

fn line(ax: i32, ay: i32, bx: i32, by: i32) -> Line {
    Line::new(IntPoint::new(ax, ay), IntPoint::new(bx, by))
}

/// Two random cases outside the domain, pinned: they break the invariant under every
/// value of the switches. Candidates for new inventory rows, not N06.4's bugs.
#[test]
fn java_breaks_the_invariant_outside_the_two_rows() {
    // A triangle crossing the bottom and left sides of a box: half the rest is missing.
    let page = TileShape::IntBox(IntBox::new(-77823, -69103, 64463, 7809));
    let crossing = TileShape::Simplex(Simplex::new(vec![
        line(-44154, -109768, 10312, -28116),
        line(10312, -28116, -82325, -30905),
        line(-82325, -30905, -44154, -109768),
    ]));
    // An odd-parity octagon (its right edge has length -1, the corner (12761.5, -2288.5))
    // inside a pentagon.
    let pentagon = TileShape::Simplex(Simplex::new(vec![
        line(18566, -24528, 11681, 25069),
        line(11681, 25069, -24538, 23198),
        line(-24538, 23198, -37153, 807),
        line(-37153, 807, -35807, -12313),
        line(-35807, -12313, 18566, -24528),
    ]));
    let odd = TileShape::IntOctagon(
        IntOctagon::new(8306, -2880, 12762, -1432, 10511, 15050, 5426, 10473).normalize(),
    );
    assert!(!in_domain(&page, &crossing));
    assert!(!in_domain(&pentagon, &odd));
    let (clipped, whole) = (area_of(&odd.intersection(&pentagon)), area_of(&odd));
    assert!((clipped - whole).abs() < 1.0, "inside: {clipped} {whole}");
    for a in [false, true] {
        for b in [false, true] {
            let quirks = SplitQuirks {
                simplex_prev_division_line: a,
                octagon_box_cutout_branch3: b,
            };
            let gap = violations(&page, &crossing, quirks).unwrap();
            assert!(
                matches!(gap[..], [Violation::AreaSum { expected, got }] if got < expected / 2.0 + 1.0),
                "{gap:?}"
            );
            let overlap = violations(&pentagon, &odd, quirks).unwrap();
            assert!(
                overlap
                    .iter()
                    .any(|v| matches!(v, Violation::Overlap { area, .. } if *area > 100_000.0)),
                "{overlap:?}"
            );
        }
    }
}
