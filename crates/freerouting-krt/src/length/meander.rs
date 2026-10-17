//! Meander bumps (X07.3): the geometry of one trombone bump on a straight
//! run, in exact integer board units.
//!
//! A run is a straight segment from `start` to `end` whose direction is a
//! multiple of 45°; it is measured in steps of its primitive integer
//! direction `g` ((±1, 0), (0, ±1) or (±1, ±1)), so every point
//! `start + t·g` is an exact integer point on the segment. A bump replaces
//! the stretch `[at, at + pitch]` of the run with a U standing `amplitude`
//! steps off it, on the left or right of the direction of travel (left is
//! `g` turned by +90°), the perpendicular `n` having the same step length
//! as `g`:
//! - [`AngleStyle::Ninety`]: up `A·n`, along `p·g`, down `A·n`. Added
//!   length `2·A·|g|`.
//! - [`AngleStyle::FortyFive`]: the four corners cut at 45° with chamfer
//!   `c = p / 6` (integer division): `c(g+n)`, `(A-2c)n`, `c(g+n)`,
//!   `(p-4c)g`, `c(g-n)`, `-(A-2c)n`, `c(g-n)`. Every piece is a multiple of
//!   45° when `g` is, so a bump on a 45° segment stays 45°-legal. Added
//!   length `|g|·(2A - (8 - 4√2)·c)`.
//!
//! The pitch is the distance between a bump's two risers (90°) or between
//! their straight parts (45°: `p - 2c`), so the stage picks `p` from the
//! spacing `2·hw + clearance` the same-net arms must keep (same-net copper
//! has no clearance check). The amplitude is at least that spacing too.
//!
//! Written from the idea of KiCadRoutingTools' trombone meander
//! (`py_router/length_matching.py` `generate_trombone_meander`,
//! `get_bump_segments` at `0486e799`); the bump geometry, its exact integer
//! steps and the corner rule are this module's.

use freerouting::board::structure::AngleRestriction;
use freerouting::geometry::{IntPoint, Point, Polyline};

/// The corner style of a bump.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AngleStyle {
    /// Square corners.
    Ninety,
    /// Corners cut at 45°.
    FortyFive,
}

impl AngleStyle {
    /// 45° where the board's trace angle restriction allows it, else 90°.
    pub fn for_restriction(restriction: AngleRestriction) -> AngleStyle {
        match restriction {
            AngleRestriction::NinetyDegree => AngleStyle::Ninety,
            AngleRestriction::FortyfiveDegree | AngleRestriction::None => AngleStyle::FortyFive,
        }
    }
}

/// The side of a bump, seen along the run's direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Side {
    /// `g` turned by +90°.
    Left,
    /// `g` turned by -90°.
    Right,
}

impl Side {
    /// Left first, then right.
    pub const ORDER: [Side; 2] = [Side::Left, Side::Right];
}

/// A straight run in exact steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run {
    /// The first point.
    pub start: IntPoint,
    /// The last point.
    pub end: IntPoint,
    /// The primitive direction, each coordinate in {-1, 0, 1}.
    pub g: (i32, i32),
    /// The length in steps of `g`.
    pub steps: i64,
}

impl Run {
    /// The run from `start` to `end`, if its direction is a multiple of 90°
    /// (and, unless `restriction` is 90°, of 45°) and it is not a point.
    pub fn new(start: IntPoint, end: IntPoint, restriction: AngleRestriction) -> Option<Run> {
        let dx = i64::from(end.x) - i64::from(start.x);
        let dy = i64::from(end.y) - i64::from(start.y);
        if dx == 0 && dy == 0 {
            return None;
        }
        let axis = dx == 0 || dy == 0;
        let diagonal = dx.abs() == dy.abs();
        if !(axis || diagonal && restriction != AngleRestriction::NinetyDegree) {
            return None;
        }
        Some(Run {
            start,
            end,
            g: (dx.signum() as i32, dy.signum() as i32),
            steps: dx.abs().max(dy.abs()),
        })
    }

    /// The length of one step: 1 or √2.
    pub fn step_length(&self) -> f64 {
        if self.g.0 != 0 && self.g.1 != 0 {
            std::f64::consts::SQRT_2
        } else {
            1.0
        }
    }

    /// The length in board units.
    pub fn length(&self) -> f64 {
        self.steps as f64 * self.step_length()
    }

    /// The perpendicular of `side`, with the step length of `g`.
    pub fn normal(&self, side: Side) -> (i32, i32) {
        let (gx, gy) = self.g;
        match side {
            Side::Left => (-gy, gx),
            Side::Right => (gy, -gx),
        }
    }

    /// Steps covering at least `length` board units.
    pub fn steps_for(&self, length: f64) -> i64 {
        (length / self.step_length()).ceil().max(1.0) as i64
    }
}

/// The 45° chamfer of a bump of `pitch` steps.
pub fn chamfer(pitch: i64, style: AngleStyle) -> i64 {
    match style {
        AngleStyle::Ninety => 0,
        AngleStyle::FortyFive => pitch / 6,
    }
}

/// The length a bump adds to its run, in board units (module docs).
pub fn added_length(amplitude: i64, pitch: i64, style: AngleStyle, step_length: f64) -> f64 {
    let c = chamfer(pitch, style) as f64;
    step_length * (2.0 * amplitude as f64 - (8.0 - 4.0 * std::f64::consts::SQRT_2) * c)
}

/// The largest amplitude (steps) whose bump adds at most `length` board
/// units, for `pitch` and `style`.
pub fn amplitude_for(length: f64, pitch: i64, style: AngleStyle, step_length: f64) -> i64 {
    let c = chamfer(pitch, style) as f64;
    let a = (length / step_length + (8.0 - 4.0 * std::f64::consts::SQRT_2) * c) / 2.0;
    if a.is_finite() { a.floor() as i64 } else { 0 }
}

/// The corners of a bump at step `at` of `run`, from the point where it
/// leaves the run to the point where it rejoins it, consecutive repeats
/// dropped; `None` if a corner leaves the `i32` range or the bump does not
/// fit (`at + pitch` past the run's end, `amplitude < 2c`).
pub fn bump_points(
    run: &Run,
    at: i64,
    amplitude: i64,
    pitch: i64,
    side: Side,
    style: AngleStyle,
) -> Option<Vec<IntPoint>> {
    let c = chamfer(pitch, style);
    if at < 0 || pitch <= 0 || at + pitch > run.steps || amplitude < 2 * c || amplitude <= 0 {
        return None;
    }
    let (gx, gy) = (i64::from(run.g.0), i64::from(run.g.1));
    let (nx, ny) = run.normal(side);
    let (nx, ny) = (i64::from(nx), i64::from(ny));
    // Moves in (along g, along n) steps.
    let moves: Vec<(i64, i64)> = match style {
        AngleStyle::Ninety => vec![(0, amplitude), (pitch, 0), (0, -amplitude)],
        AngleStyle::FortyFive => vec![
            (c, c),
            (0, amplitude - 2 * c),
            (c, c),
            (pitch - 4 * c, 0),
            (c, -c),
            (0, -(amplitude - 2 * c)),
            (c, -c),
        ],
    };
    let mut x = i64::from(run.start.x) + at * gx;
    let mut y = i64::from(run.start.y) + at * gy;
    let mut out = vec![int_point(x, y)?];
    for (u, v) in moves {
        if u == 0 && v == 0 {
            continue;
        }
        x += u * gx + v * nx;
        y += u * gy + v * ny;
        out.push(int_point(x, y)?);
    }
    Some(out)
}

fn int_point(x: i64, y: i64) -> Option<IntPoint> {
    Some(IntPoint::new(
        i32::try_from(x).ok()?,
        i32::try_from(y).ok()?,
    ))
}

/// The run as a polyline with one bump at step `at` (X07.3's
/// `bump(segment, at, amplitude, pitch, side, angle_style)`). `None` if the
/// segment is no [`Run`] or the bump does not fit.
pub fn bump(
    segment: (IntPoint, IntPoint),
    at: i64,
    amplitude: i64,
    pitch: i64,
    side: Side,
    style: AngleStyle,
) -> Option<Polyline> {
    let restriction = match style {
        AngleStyle::Ninety => AngleRestriction::NinetyDegree,
        AngleStyle::FortyFive => AngleRestriction::FortyfiveDegree,
    };
    let run = Run::new(segment.0, segment.1, restriction)?;
    let mut points = vec![run.start];
    points.extend(bump_points(&run, at, amplitude, pitch, side, style)?);
    points.push(run.end);
    points.dedup();
    let points: Vec<Point> = points.into_iter().map(Point::from).collect();
    Some(Polyline::from_points(&points))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: i32, y: i32) -> IntPoint {
        IntPoint::new(x, y)
    }

    #[test]
    fn added_length_matches_the_formula() {
        let cases = [
            ((p(0, 0), p(100_000, 0)), AngleStyle::Ninety, 4_500, 4_500),
            (
                (p(0, 0), p(100_000, 0)),
                AngleStyle::FortyFive,
                4_500,
                6_750,
            ),
            (
                (p(0, 0), p(0, -90_000)),
                AngleStyle::FortyFive,
                7_001,
                6_751,
            ),
            (
                (p(0, 0), p(60_000, 60_000)),
                AngleStyle::FortyFive,
                3_000,
                4_800,
            ),
            ((p(0, 0), p(0, 90_000)), AngleStyle::Ninety, 3_001, 3_333),
        ];
        for (segment, style, amplitude, pitch) in cases {
            let run = Run::new(segment.0, segment.1, AngleRestriction::FortyfiveDegree).unwrap();
            for side in Side::ORDER {
                let line = bump(segment, 2 * pitch, amplitude, pitch, side, style).unwrap();
                let added = line.length_approx() - run.length();
                let want = added_length(amplitude, pitch, style, run.step_length());
                assert!(
                    (added - want).abs() < 1e-6,
                    "{segment:?} {style:?} {side:?}: {added} vs {want}"
                );
            }
            // amplitude_for inverts added_length exactly at whole steps.
            let want = added_length(amplitude, pitch, style, run.step_length());
            assert_eq!(
                amplitude_for(want + 1e-9, pitch, style, run.step_length()),
                amplitude
            );
        }
        // 90°: 2·A·|g| on an axis segment, exactly.
        assert_eq!(added_length(10, 7, AngleStyle::Ninety, 1.0), 20.0);
    }

    #[test]
    fn bumps_on_a_45_degree_segment_stay_45_degree_legal() {
        for (a, b) in [
            (p(0, 0), p(80_000, 80_000)),
            (p(5, 7), p(-79_995, 80_007)),
            (p(0, 0), p(100_000, 0)),
        ] {
            for side in Side::ORDER {
                for (amplitude, pitch) in [(4_500, 6_750), (2_250, 6_750), (9_000, 13_500)] {
                    let line = bump(
                        (a, b),
                        10_000,
                        amplitude,
                        pitch,
                        side,
                        AngleStyle::FortyFive,
                    )
                    .unwrap();
                    assert!(line.is_multiple_of_45_degree(), "{a:?} {b:?} {side:?}");
                    let corners = line.corners();
                    assert_eq!(corners.first(), Some(&Point::from(a)));
                    assert_eq!(corners.last(), Some(&Point::from(b)));
                    assert!(corners.iter().all(|c| c.as_int().is_some()));
                }
            }
            // 90° bumps on an axis segment are orthogonal.
            if a.y == b.y {
                let line = bump((a, b), 10_000, 4_500, 4_500, Side::Left, AngleStyle::Ninety);
                assert!(line.unwrap().is_orthogonal());
            }
        }
        // A 90° board takes no diagonal run; any-angle runs are refused.
        assert!(Run::new(p(0, 0), p(10, 10), AngleRestriction::NinetyDegree).is_none());
        assert!(Run::new(p(0, 0), p(10, 7), AngleRestriction::None).is_none());
    }

    #[test]
    fn both_sides_are_tried_left_first() {
        assert_eq!(Side::ORDER, [Side::Left, Side::Right]);
        // Left of +x is +y; right is -y.
        let run = Run::new(p(0, 0), p(100_000, 0), AngleRestriction::FortyfiveDegree).unwrap();
        let left = bump_points(&run, 10_000, 4_500, 4_500, Side::Left, AngleStyle::Ninety).unwrap();
        let right =
            bump_points(&run, 10_000, 4_500, 4_500, Side::Right, AngleStyle::Ninety).unwrap();
        assert_eq!(
            left,
            vec![
                p(10_000, 0),
                p(10_000, 4_500),
                p(14_500, 4_500),
                p(14_500, 0)
            ]
        );
        assert_eq!(
            right,
            vec![
                p(10_000, 0),
                p(10_000, -4_500),
                p(14_500, -4_500),
                p(14_500, 0)
            ]
        );
    }

    #[test]
    fn a_bump_that_does_not_fit_is_refused() {
        let run = Run::new(p(0, 0), p(10_000, 0), AngleRestriction::FortyfiveDegree).unwrap();
        assert!(bump_points(&run, 6_000, 4_500, 4_500, Side::Left, AngleStyle::Ninety).is_none());
        // Below 2c the 45° risers would turn back.
        assert!(bump_points(&run, 0, 1_000, 6_000, Side::Left, AngleStyle::FortyFive).is_none());
        // Coordinates at the i32 limit.
        let edge = Run::new(
            p(i32::MAX - 10_000, 0),
            p(i32::MAX, 0),
            AngleRestriction::FortyfiveDegree,
        )
        .unwrap();
        assert!(bump_points(&edge, 0, 4_500, 4_500, Side::Left, AngleStyle::Ninety).is_some());
        let top = Run::new(
            p(0, i32::MAX - 100),
            p(10_000, i32::MAX - 100),
            AngleRestriction::FortyfiveDegree,
        )
        .unwrap();
        assert!(bump_points(&top, 0, 4_500, 4_500, Side::Left, AngleStyle::Ninety).is_none());
    }
}
