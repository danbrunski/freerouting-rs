//! The geometry of one routed run (Q15.1): bends, acute corners and jogs.
//!
//! A *run* is the copper of a connection between two layer changes (or the
//! ends of the connection): a chain of straight segments on one layer. The
//! functions here work on plain numbers, so the definitions can be tested
//! without a board.
//!
//! - **Bend.** A vertex where the direction changes by more than
//!   [`BEND_DEGREES`] (10 degrees). 10 degrees is below any corner a router
//!   makes on purpose (22.5 and 45 degrees are the smallest in Freerouting and
//!   KiCad), and above the 5.7 degrees between two chords of an arc that KiCad
//!   or the `.kicad_pcb` reader approximates (0.1 rad), so a smooth arc is not
//!   a row of bends. A turn is the angle between the two segments' directions,
//!   0 for a straight continuation and 180 for a reversal.
//! - **Acute corner.** A bend whose interior angle is below 90 degrees: the
//!   directions' dot product is negative (a turn of more than 90 degrees). A
//!   right angle is not acute.
//! - **Jog.** A segment between two bends that is shorter than
//!   [`JOG_WIDTHS`] (2) times the track width. Segments that continue in
//!   the same direction (a turn of at most 10 degrees) count as one segment,
//!   so a straight line split in two traces is not two jogs. A segment that
//!   ends at a pin or a via is not between two bends.

/// A turn of more than this many degrees is a bend.
pub const BEND_DEGREES: f64 = 10.0;
/// A segment between two bends shorter than this many track widths is a jog.
pub const JOG_WIDTHS: f64 = 2.0;

/// One straight segment of a run, in board units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seg {
    pub a: (f64, f64),
    pub b: (f64, f64),
    /// The full track width of the segment.
    pub width: f64,
}

impl Seg {
    pub fn length(&self) -> f64 {
        (self.b.0 - self.a.0).hypot(self.b.1 - self.a.1)
    }
}

/// What one run contributes to its connection.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RunStats {
    /// Board units.
    pub length: f64,
    pub bends: usize,
    pub acute: usize,
    pub jogs: usize,
}

impl RunStats {
    pub fn add(&mut self, other: &RunStats) {
        self.length += other.length;
        self.bends += other.bends;
        self.acute += other.acute;
        self.jogs += other.jogs;
    }
}

/// The turn between two directions: degrees (0 to 180) and whether the
/// corner is acute (more than 90 degrees).
fn turn(d0: (f64, f64), d1: (f64, f64)) -> (f64, bool) {
    let cross = d0.0 * d1.1 - d0.1 * d1.0;
    let dot = d0.0 * d1.0 + d0.1 * d1.1;
    let degrees = cross.abs().atan2(dot).to_degrees();
    let norm = d0.0.hypot(d0.1) * d1.0.hypot(d1.1);
    (degrees, dot < -1e-9 * norm)
}

/// Measures the segments of one run, in order along the connection.
/// Zero-length segments are ignored.
pub fn measure_run(segments: &[Seg]) -> RunStats {
    let segs: Vec<&Seg> = segments.iter().filter(|s| s.length() > 0.0).collect();
    let mut stats = RunStats {
        length: segs.iter().map(|s| s.length()).sum(),
        ..RunStats::default()
    };
    // The segment group since the last bend, and whether a bend opened it.
    let mut group_length = 0.0;
    let mut group_width: f64 = 0.0;
    let mut opened_by_bend = false;
    for (i, seg) in segs.iter().enumerate() {
        group_length += seg.length();
        group_width = group_width.max(seg.width);
        let Some(next) = segs.get(i + 1) else {
            break;
        };
        let d0 = (seg.b.0 - seg.a.0, seg.b.1 - seg.a.1);
        let d1 = (next.b.0 - next.a.0, next.b.1 - next.a.1);
        let (degrees, acute) = turn(d0, d1);
        if degrees <= BEND_DEGREES {
            continue;
        }
        stats.bends += 1;
        if acute {
            stats.acute += 1;
        }
        if opened_by_bend && group_length < JOG_WIDTHS * group_width {
            stats.jogs += 1;
        }
        group_length = 0.0;
        group_width = 0.0;
        opened_by_bend = true;
    }
    stats
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(a: (f64, f64), b: (f64, f64)) -> Seg {
        Seg { a, b, width: 2.0 }
    }

    #[test]
    fn a_straight_line_in_two_pieces_is_one_segment() {
        let stats = measure_run(&[seg((0.0, 0.0), (10.0, 0.0)), seg((10.0, 0.0), (20.0, 0.0))]);
        assert_eq!((stats.bends, stats.acute, stats.jogs), (0, 0, 0));
        assert_eq!(stats.length, 20.0);
    }

    #[test]
    fn the_bend_threshold_is_ten_degrees() {
        // 5.7 degrees is one chord step of an approximated arc; 20 is a bend
        let step = 0.1_f64;
        let gentle = measure_run(&[
            seg((0.0, 0.0), (100.0, 0.0)),
            seg(
                (100.0, 0.0),
                (100.0 + 100.0 * step.cos(), 100.0 * step.sin()),
            ),
        ]);
        assert_eq!(gentle.bends, 0);
        let k = 20.0_f64.to_radians();
        let sharp = measure_run(&[
            seg((0.0, 0.0), (100.0, 0.0)),
            seg((100.0, 0.0), (100.0 + 100.0 * k.cos(), 100.0 * k.sin())),
        ]);
        assert_eq!((sharp.bends, sharp.acute), (1, 0));
    }

    #[test]
    fn a_right_angle_is_not_acute_but_a_hairpin_is() {
        let right = measure_run(&[seg((0.0, 0.0), (10.0, 0.0)), seg((10.0, 0.0), (10.0, 10.0))]);
        assert_eq!((right.bends, right.acute), (1, 0));
        let hairpin = measure_run(&[seg((0.0, 0.0), (10.0, 0.0)), seg((10.0, 0.0), (0.0, 1.0))]);
        assert_eq!((hairpin.bends, hairpin.acute), (1, 1));
    }

    #[test]
    fn a_jog_needs_a_bend_on_both_sides() {
        // the 3-unit step is shorter than two 2-unit widths... 3 < 4
        let stair = measure_run(&[
            seg((0.0, 0.0), (10.0, 0.0)),
            seg((10.0, 0.0), (10.0, 3.0)),
            seg((10.0, 3.0), (20.0, 3.0)),
        ]);
        assert_eq!((stair.bends, stair.jogs), (2, 1));
        // a short last segment ends at a pad: not between two bends
        let stub = measure_run(&[seg((0.0, 0.0), (10.0, 0.0)), seg((10.0, 0.0), (10.0, 3.0))]);
        assert_eq!((stub.bends, stub.jogs), (1, 0));
        // 4 is two widths: not shorter
        let exact = measure_run(&[
            seg((0.0, 0.0), (10.0, 0.0)),
            seg((10.0, 0.0), (10.0, 4.0)),
            seg((10.0, 4.0), (20.0, 4.0)),
        ]);
        assert_eq!(exact.jogs, 0);
    }

    #[test]
    fn zero_length_segments_and_empty_runs_are_ignored() {
        assert_eq!(measure_run(&[]), RunStats::default());
        let stats = measure_run(&[
            seg((0.0, 0.0), (10.0, 0.0)),
            seg((10.0, 0.0), (10.0, 0.0)),
            seg((10.0, 0.0), (20.0, 0.0)),
        ]);
        assert_eq!((stats.bends, stats.length), (0, 20.0));
    }
}
