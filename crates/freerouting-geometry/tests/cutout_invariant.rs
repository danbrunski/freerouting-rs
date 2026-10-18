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
//! Two kinds of hole outside the domain break the invariant whatever N06.4's switches
//! are. N06.14 added a switch for each, and
//! [`java_breaks_the_invariant_outside_the_two_rows`] pins one case of each under all 16
//! switch values:
//! - `cutout_hole_crosses_border`: a hole crossing the outer border. Its corners on the
//!   border are not divided, and Java uses their stand-in division line (the hole edge
//!   before the corner) to clip later pieces, which go missing (the areas sum to less);
//! - `simplex_octagon_half_corner`: an octagon whose diagonals meet at a half-integer
//!   point, cut out of a simplex. As a simplex it has a corner that is not an integer
//!   point, `calcDivisionLines` takes such a corner for one on the outer border and
//!   divides nothing there, and pieces overlap. An enlarged simplex has such corners
//!   too.
//!
//! [`all_fixed_cutouts_tile_the_rest_on_the_extended_set`] checks that with all four
//! switches `false` the invariant holds on the extended set: the random cases above,
//! holes centred on the outer border, and enlarged holes
//! ([`TileShape::enlarge`], as the search trees enlarge obstacles by a clearance).
//!
//! One-unit overlaps along an octagon edge of length -1 (an odd-parity octagon after
//! `normalize`) come from rounding to the integer grid and are not counted
//! ([`SLIVER_WIDTH`]). So are the strips, at most one unit wide, along the two division
//! lines through integer points that the `simplex_octagon_half_corner` fix puts on
//! either side of an inexact corner.
//!
//! The cases come from a seeded [`JavaRandom`], so every run checks the same shapes.

use std::collections::BTreeMap;

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
/// unit wide. The pieces beside an inexact corner that the `simplex_octagon_half_corner`
/// fix divides share a strip at most `1 / |dir|` wide, one unit at most.
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
    ..SplitQuirks::JAVA
};

const ONLY_OCTAGON_FIXED: SplitQuirks = SplitQuirks {
    octagon_box_cutout_branch3: false,
    ..SplitQuirks::JAVA
};

const ONLY_BORDER_FIXED: SplitQuirks = SplitQuirks {
    cutout_hole_crosses_border: false,
    ..SplitQuirks::JAVA
};

const ONLY_HALF_CORNER_FIXED: SplitQuirks = SplitQuirks {
    simplex_octagon_half_corner: false,
    ..SplitQuirks::JAVA
};

/// N06.4's two switches fixed, N06.14's two kept.
const N06_4_FIXED: SplitQuirks = SplitQuirks {
    simplex_prev_division_line: false,
    octagon_box_cutout_branch3: false,
    ..SplitQuirks::JAVA
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
        for java in SplitQuirks::all() {
            if !java.simplex_prev_division_line {
                continue;
            }
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
/// value of N06.4's switches, and N06.14's switch for each repairs it (both behaviours
/// of `cutout_hole_crosses_border` and `simplex_octagon_half_corner`).
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
    for quirks in SplitQuirks::all() {
        let gap = violations(&page, &crossing, quirks).unwrap();
        if quirks.cutout_hole_crosses_border {
            assert!(
                matches!(gap[..], [Violation::AreaSum { expected, got }] if got < expected / 2.0 + 1.0),
                "{quirks:?}: {gap:?}"
            );
        } else {
            assert!(gap.is_empty(), "{quirks:?}: {gap:?}");
        }
        let overlap = violations(&pentagon, &odd, quirks).unwrap();
        if quirks.simplex_octagon_half_corner {
            assert!(
                overlap
                    .iter()
                    .any(|v| matches!(v, Violation::Overlap { area, .. } if *area > 100_000.0)),
                "{quirks:?}: {overlap:?}"
            );
        } else {
            assert!(overlap.is_empty(), "{quirks:?}: {overlap:?}");
        }
    }
}

/// Every case, inside the domain and outside it, with holes crossing the outer
/// border: the invariant's violations under `quirks`, and the cases checked.
fn broken_anywhere(quirks: SplitQuirks) -> (Vec<(Kind, Kind, Vec<Violation>)>, usize) {
    let mut broken = Vec::new();
    let mut checked = 0;
    for (outer_kind, inner_kind, o, h) in extended_cases() {
        if let Some(v) = violations(&o, &h, quirks) {
            checked += 1;
            if !v.is_empty() {
                broken.push((outer_kind, inner_kind, v));
            }
        }
    }
    (broken, checked)
}

/// [`all_cases`], [`crossing_cases`] and [`enlarged_cases`].
fn extended_cases() -> Vec<(Kind, Kind, TileShape, TileShape)> {
    let mut cases = all_cases();
    cases.extend(crossing_cases());
    cases.extend(enlarged_cases());
    cases
}

/// Holes enlarged as the search trees enlarge obstacle shapes by a clearance
/// ([`TileShape::enlarge`]): an enlarged simplex has corners that are not integer
/// points. Half of them cross the outer border.
fn enlarged_cases() -> Vec<(Kind, Kind, TileShape, TileShape)> {
    let mut cases = Vec::new();
    for (oi, &outer) in KINDS.iter().enumerate() {
        for (ii, &inner) in KINDS.iter().enumerate() {
            let mut rnd = JavaRandom::new(0x4e06_1401 + (oi * 3 + ii) as i64);
            for _ in 0..CASES {
                let (o, h) = if rnd.next_boolean() {
                    random_case(&mut rnd, outer, inner)
                } else {
                    let mut crossing = one_crossing_case(&mut rnd, outer, inner);
                    match crossing.take() {
                        Some(pair) => pair,
                        None => continue,
                    }
                };
                let h = h.enlarge(f64::from(int_in(&mut rnd, 10, 3_000)));
                if o.dimension() == 2 && h.dimension() == 2 {
                    cases.push((outer, inner, o, h));
                }
            }
        }
    }
    cases
}

/// One hole whose centre lies on the outer shape's border; `None` if a shape is
/// not 2-dimensional.
fn one_crossing_case(
    rnd: &mut JavaRandom,
    outer: Kind,
    inner: Kind,
) -> Option<(TileShape, TileShape)> {
    let r = int_in(rnd, 2_000, EXTENT / 2);
    let cx = int_in(rnd, -EXTENT / 2, EXTENT / 2);
    let cy = int_in(rnd, -EXTENT / 2, EXTENT / 2);
    let o = random_shape(rnd, outer, cx, cy, r);
    if o.dimension() < 2 {
        return None;
    }
    let n = o.border_line_count() as i32;
    let corner = o.corner_approx(rnd.next_int(n) as usize);
    let next = o.corner_approx(rnd.next_int(n) as usize);
    let t = rnd.next_double();
    let ir = int_in(rnd, r / 20, r / 2);
    let icx = (corner.x + t * (next.x - corner.x)).round() as i32;
    let icy = (corner.y + t * (next.y - corner.y)).round() as i32;
    let h = random_shape(rnd, inner, icx, icy, ir);
    (h.dimension() == 2).then_some((o, h))
}

/// Holes whose centre lies near the outer shape's border, so that most cross it.
fn crossing_cases() -> Vec<(Kind, Kind, TileShape, TileShape)> {
    let mut cases = Vec::new();
    for (oi, &outer) in KINDS.iter().enumerate() {
        for (ii, &inner) in KINDS.iter().enumerate() {
            let mut rnd = JavaRandom::new(0x4e06_1400 + (oi * 3 + ii) as i64);
            for _ in 0..CASES {
                if let Some((o, h)) = one_crossing_case(&mut rnd, outer, inner) {
                    cases.push((outer, inner, o, h));
                }
            }
        }
    }
    cases
}

/// The broken cases by kind pair and violation kind, for the failure messages.
fn tally(broken: &[(Kind, Kind, Vec<Violation>)]) -> BTreeMap<String, usize> {
    let mut by_kind = BTreeMap::new();
    for (o, i, v) in broken {
        let what = if v.iter().any(|v| matches!(v, Violation::Overlap { .. })) {
            "overlap"
        } else {
            "gap"
        };
        *by_kind.entry(format!("{o:?}-{i:?}-{what}")).or_insert(0) += 1;
    }
    by_kind
}

/// N06.14: with all four switches `false` the invariant holds on every case, inside
/// the domain and outside it: holes crossing the border, odd-parity octagons and
/// enlarged simplexes whose corners are not integer points.
#[test]
fn all_fixed_cutouts_tile_the_rest_on_the_extended_set() {
    let (broken, checked) = broken_anywhere(SplitQuirks::FIXED);
    println!(
        "all four switches false: {} of {checked} cutouts broken",
        broken.len()
    );
    assert!(
        checked > 3 * KINDS.len() * KINDS.len() * CASES / 2,
        "{checked}"
    );
    assert!(
        broken.is_empty(),
        "{} of {checked} cutouts break the invariant with every switch false: {:?}; first: {:?}",
        broken.len(),
        tally(&broken),
        broken.first()
    );
}

/// N06.14: outside the domain Java breaks the invariant through all three switches
/// that change pieces. Each N06.14 switch alone repairs the failures of its kind: no
/// gap is left with `cutout_hole_crosses_border` false, no overlap from an inexact
/// corner with `simplex_octagon_half_corner` false. Only the two together with N06.4's
/// branch-3 fix repair every case.
#[test]
fn java_breaks_the_extended_set_through_each_switch() {
    let (java, checked) = broken_anywhere(SplitQuirks::JAVA);
    let java_tally = tally(&java);
    println!(
        "Java: {} of {checked} cutouts broken: {java_tally:?}",
        java.len()
    );
    let gaps = |broken: &[(Kind, Kind, Vec<Violation>)]| {
        broken
            .iter()
            .filter(|(_, _, v)| v.iter().all(|v| matches!(v, Violation::AreaSum { .. })))
            .count()
    };
    let simplex_overlaps = |broken: &[(Kind, Kind, Vec<Violation>)]| {
        broken
            .iter()
            .filter(|(o, i, v)| {
                (*o == Kind::Simplex || *i == Kind::Simplex)
                    && v.iter().any(|v| matches!(v, Violation::Overlap { .. }))
            })
            .count()
    };
    assert!(gaps(&java) > 50, "{java_tally:?}");
    assert!(simplex_overlaps(&java) > 50, "{java_tally:?}");
    let (border_fixed, _) = broken_anywhere(ONLY_BORDER_FIXED);
    println!(
        "cutout_hole_crosses_border false: {:?}",
        tally(&border_fixed)
    );
    assert_eq!(gaps(&border_fixed), 0, "{:?}", tally(&border_fixed));
    let (half_fixed, _) = broken_anywhere(ONLY_HALF_CORNER_FIXED);
    println!(
        "simplex_octagon_half_corner false: {:?}",
        tally(&half_fixed)
    );
    assert_eq!(simplex_overlaps(&half_fixed), 0, "{:?}", tally(&half_fixed));
    assert!(gaps(&half_fixed) > 0);
    let (n06_4_fixed, _) = broken_anywhere(N06_4_FIXED);
    assert!(!n06_4_fixed.is_empty());
}

/// The N06.14 counters count, whatever the switches, where their switch can change a
/// cutout: every cutout that a switch changes has a count. The counts are an upper
/// bound: a clip that the fix drops, or a division it adds, can leave the piece as it
/// was.
#[test]
fn the_n06_14_counters_count_the_changed_cutouts() {
    let (mut border_changed, mut border_counted) = (0, 0);
    let (mut half_changed, mut half_counted) = (0, 0);
    for (_, _, o, h) in extended_cases() {
        let mut java_reach = SplitReach::default();
        let java = o.cutout_counted(&h, SplitQuirks::JAVA, &mut java_reach);
        let mut border_reach = SplitReach::default();
        let border = o.cutout_counted(&h, ONLY_BORDER_FIXED, &mut border_reach);
        let mut half_reach = SplitReach::default();
        let half = o.cutout_counted(&h, ONLY_HALF_CORNER_FIXED, &mut half_reach);
        // The border count does not depend on its own switch, and the half-corner count
        // depends on no switch.
        assert_eq!(java_reach, border_reach, "{o:?} {h:?}");
        assert_eq!(
            java_reach.simplex_octagon_half_corner,
            half_reach.simplex_octagon_half_corner
        );
        let border_differs = describe(&java) != describe(&border);
        let border_reached = java_reach.cutout_hole_crosses_border > 0;
        assert!(!border_differs || border_reached, "{o:?} {h:?}");
        let half_differs = describe(&java) != describe(&half);
        let half_reached = java_reach.simplex_octagon_half_corner > 0;
        assert!(!half_differs || half_reached, "{o:?} {h:?}");
        border_changed += usize::from(border_differs);
        border_counted += usize::from(border_reached);
        half_changed += usize::from(half_differs);
        half_counted += usize::from(half_reached);
    }
    println!(
        "cutout_hole_crosses_border changes {border_changed} cutouts, counted in {border_counted}"
    );
    println!(
        "simplex_octagon_half_corner changes {half_changed} cutouts, counted in {half_counted}"
    );
    assert!(border_changed > 50 && half_changed > 50);
    // Most counted cutouts do change.
    assert!(border_changed * 2 > border_counted && half_changed * 2 > half_counted);
}

/// N06.14's switches change no piece inside N06.4's domain: there every corner of the
/// cut-out part is an integer point, so no corner is undivided by Java's assumption.
#[test]
fn the_n06_14_switches_change_nothing_in_the_domain() {
    let both = SplitQuirks {
        cutout_hole_crosses_border: false,
        simplex_octagon_half_corner: false,
        ..SplitQuirks::JAVA
    };
    let mut checked = 0;
    for (_, _, o, h) in all_cases() {
        if !in_domain(&o, &h) {
            continue;
        }
        let mut reach = SplitReach::default();
        let fixed = o.cutout_counted(&h, both, &mut reach);
        assert_eq!(
            describe(&o.cutout_with(&h, SplitQuirks::JAVA)),
            describe(&fixed)
        );
        assert_eq!(
            (
                reach.cutout_hole_crosses_border,
                reach.simplex_octagon_half_corner
            ),
            (0, 0)
        );
        checked += 1;
    }
    assert!(checked > 1000, "{checked}");
}
