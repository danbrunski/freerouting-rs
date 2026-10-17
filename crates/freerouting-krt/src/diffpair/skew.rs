//! Intra-pair skew (X09.4): the shorter track of a pair gets meander bumps
//! near the end where the mismatch comes from, until the two tracks are
//! within `router.diff_pairs.skew_um` (default [`DEFAULT_SKEW_UM`]).
//!
//! **When.** On the planned tracks with their straight connectors, before
//! the pair's B09 transaction ([`super::legs`]), so the bumps go in with
//! the pair and the exact DRC judges them with it; a pair whose bumps the
//! DRC refuses is refused as a whole and the next attempt runs. Lengths
//! are the tracks' polyline lengths: both nets have the same vias, so the
//! barrels cancel, and a DSN has no pad-to-die lengths.
//!
//! **Where.** The mismatched end is the end whose P and N pin centres are
//! farther apart along its escape direction (the start on a tie). The
//! shorter net's segments are taken from that end inwards; each straight,
//! 45°-legal segment with integer ends takes bumps from one pitch in,
//! every two pitches, while the skew exceeds the tolerance. A bump stands
//! on the side away from the partner (P's side of the centreline for P),
//! so the gap only opens over the bump.
//!
//! **One bump** is X07's ([`crate::length::meander::bump_points`]): pitch
//! from the arm spacing `2·hw + clearance` as X07 picks it, amplitude the
//! largest that adds at most the remaining skew
//! ([`crate::length::meander::amplitude_for`]), at most X07's
//! [`MAX_AMPLITUDE_UM`]. 45° corners where the board allows them; a skew
//! too small for a 45° bump of that pitch gets a square one (90° corners
//! are 45°-legal too). Lengths only grow, so the longer track is never
//! passed by more than one step.
//!
//! Written from the idea of KiCadRoutingTools' intra-pair length matching
//! (`py_router/length_matching.py`, the trombone on the shorter leg, at
//! `0486e799`); the placement rule is this module's.

use freerouting::board::BasicBoard;
use freerouting::geometry::{IntPoint, Line, Polyline};

use super::centreline::{End, PairEnds, net_clearance_class, pad_reach};
use super::model::Pair;
use crate::length::Units;
use crate::length::matching::MAX_AMPLITUDE_UM;
use crate::length::meander::{AngleStyle, Run, Side, added_length, amplitude_for, bump_points};

/// The default skew tolerance (`router.diff_pairs.skew_um`), µm.
pub const DEFAULT_SKEW_UM: f64 = 25.0;

/// The most bumps one pair gets.
const MAX_BUMPS: usize = 64;

/// What the compensation did.
#[derive(Clone, Debug, PartialEq)]
pub struct SkewOutcome {
    /// The lengthened net.
    pub net: i32,
    /// `|len P − len N|` before and after, board units.
    pub before: f64,
    pub after: f64,
    /// The bumps placed.
    pub bumps: usize,
    /// True if the mismatched end is the start.
    pub at_start: bool,
}

/// How far `end`'s P and N pins are apart along its escape direction.
fn stagger(end: &End, other: &End) -> f64 {
    let (ux, uy) = end.escape(other);
    let dx = f64::from(end.p_centre.x) - f64::from(end.n_centre.x);
    let dy = f64::from(end.p_centre.y) - f64::from(end.n_centre.y);
    (dx * ux + dy * uy).abs()
}

fn length(tracks: &[(i32, Polyline, Polyline)], p: bool) -> f64 {
    tracks
        .iter()
        .map(|(_, a, b)| if p { a } else { b }.length_approx())
        .sum()
}

/// Bumps the shorter of `tracks`' nets ([module docs](self)); `Ok(None)`
/// when the skew is within `tolerance_um` already. `tracks` are the
/// pair's planned `(layer, P track, N track)` per section, in travel order.
pub fn compensate_skew(
    board: &BasicBoard,
    pair: &Pair,
    ends: &PairEnds,
    tracks: &mut [(i32, Polyline, Polyline)],
    tolerance_um: f64,
) -> Result<Option<SkewOutcome>, String> {
    let units = Units::of(board);
    let tolerance = units.from_um(tolerance_um);
    let (lp, ln) = (length(tracks, true), length(tracks, false));
    let before = (lp - ln).abs();
    if before <= tolerance {
        return Ok(None);
    }
    let lengthen_p = lp < ln;
    let at_start = stagger(&ends.start, &ends.end) >= stagger(&ends.end, &ends.start);
    let p_left = p_is_left(ends);
    // Away from the partner, seen along the direction of travel.
    let outward_left = if lengthen_p { p_left } else { !p_left };
    let restriction = board.rules.trace_angle_restriction();
    let board_style = AngleStyle::for_restriction(restriction);
    let net = if lengthen_p { pair.p } else { pair.n };
    let clearance_class = net_clearance_class(board, net);
    let max_amplitude = units.from_um(MAX_AMPLITUDE_UM);
    let order: Vec<usize> = if at_start {
        (0..tracks.len()).collect()
    } else {
        (0..tracks.len()).rev().collect()
    };
    let (near, far) = if at_start {
        (&ends.start, &ends.end)
    } else {
        (&ends.end, &ends.start)
    };
    let mut remaining = before;
    let mut bumps = 0;
    // The segments next to the mismatched end keep their bumps clear of
    // its pads; the first segment that is no run (a corner) ends that.
    let mut near_end = true;
    for k in order {
        let layer = tracks[k].0;
        let hw = pair.half_width(layer);
        let clearance = board.clearance_value(clearance_class, clearance_class, layer);
        let spacing = 2.0 * f64::from(hw) + f64::from(clearance.max(0));
        let keep_off = pad_reach(board, near, far, layer) + 2.0 * spacing;
        let track = if lengthen_p {
            &tracks[k].1
        } else {
            &tracks[k].2
        };
        let mut lines = track.lines().to_vec();
        let corners = track.corners();
        let segments: Vec<usize> = if at_start {
            (0..corners.len().saturating_sub(1)).collect()
        } else {
            (0..corners.len().saturating_sub(1)).rev().collect()
        };
        // Bumps per segment, in travel order of the polyline; applied from
        // the highest segment down so the line indices stay valid.
        let mut planned: Vec<(usize, Vec<IntPoint>)> = Vec::new();
        for s in segments {
            if remaining <= tolerance || bumps >= MAX_BUMPS {
                break;
            }
            let (Some(a), Some(b)) = (corners[s].as_int(), corners[s + 1].as_int()) else {
                continue;
            };
            // The run goes inwards from the mismatched end.
            let (from, to) = if at_start { (a, b) } else { (b, a) };
            let Some(run) = Run::new(from, to, restriction) else {
                continue;
            };
            let side = if at_start == outward_left {
                Side::Left
            } else {
                Side::Right
            };
            let steps_spacing = run.steps_for(spacing);
            let mut points: Vec<IntPoint> = Vec::new();
            for style in [board_style, AngleStyle::Ninety] {
                let pitch = match style {
                    AngleStyle::Ninety => steps_spacing,
                    AngleStyle::FortyFive => (3 * steps_spacing + 1) / 2,
                };
                let mut position = if near_end {
                    pitch.max(run.steps_for(keep_off))
                } else {
                    pitch
                };
                let mut got = 0.0;
                let mut count = 0;
                while position + 2 * pitch <= run.steps
                    && remaining - got > tolerance
                    && bumps + count < MAX_BUMPS
                {
                    let amp = amplitude_for(remaining - got, pitch, style, run.step_length())
                        .min(run.steps_for(max_amplitude));
                    // `None` when the amplitude is below the 45° chamfers.
                    let Some(bump) = bump_points(&run, position, amp, pitch, side, style) else {
                        break;
                    };
                    got += added_length(amp, pitch, style, run.step_length());
                    points.extend(bump);
                    count += 1;
                    position += 2 * pitch;
                }
                if !points.is_empty() {
                    remaining -= got;
                    bumps += count;
                    break;
                }
            }
            near_end = false;
            if points.is_empty() {
                continue;
            }
            if !at_start {
                points.reverse();
            }
            planned.push((s, points));
        }
        planned.sort_by_key(|(s, _)| std::cmp::Reverse(*s));
        for (s, points) in planned {
            // Segment `s` lies on line `s + 1`.
            let run_line = lines[s + 1];
            let mut insert: Vec<Line> = points.windows(2).map(|w| Line::new(w[0], w[1])).collect();
            insert.push(run_line);
            let at = s + 2;
            lines.splice(at..at, insert);
        }
        let polyline = Polyline::new(&lines);
        if polyline.is_empty() {
            return Err("the skew bumps made an empty track".to_string());
        }
        if lengthen_p {
            tracks[k].1 = polyline;
        } else {
            tracks[k].2 = polyline;
        }
        if remaining <= tolerance || bumps >= MAX_BUMPS {
            break;
        }
    }
    let after = (length(tracks, true) - length(tracks, false)).abs();
    Ok(Some(SkewOutcome {
        net,
        before,
        after,
        bumps,
        at_start,
    }))
}

/// P's side of the direction of travel at the start (the side rule of
/// [`super::legs::crossing`]).
fn p_is_left(ends: &PairEnds) -> bool {
    let m = ends.start.midpoint();
    let (ux, uy) = ends.start.escape(&ends.end);
    let px = f64::from(ends.start.p_centre.x) - f64::from(m.x);
    let py = f64::from(ends.start.p_centre.y) - f64::from(m.y);
    ux * py - uy * px >= 0.0
}
