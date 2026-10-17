//! The copper diff: what of the file's copper the routed board still has
//! (K08.1).
//!
//! The router normalizes and merges traces, so item ids do not survive; the
//! diff works on geometry in board units, exactly, with `i128` products:
//!
//! - an original segment or arc is **kept** when every piece of it (the
//!   segment, or the chords of the arc as the reader flattens it), mapped to
//!   board units the way the board builder maps it, lies on the final trace
//!   segments of the same layer, net and half width (collinear and covered by
//!   their union, so a segment the router split in two is still kept).
//!   A locked original is always kept; so is copper the reader did not put on
//!   the board (no net, not a copper layer). Everything else is **removed**.
//! - an original via is kept when a final via has its position, net, span,
//!   diameter and drill.
//! - the parts of final trace segments that kept originals do not cover, and
//!   the final vias no kept via matches, are **added**, in the board's item
//!   order. An added end that lands on the board image of an original end or
//!   via centre is written at that original's exact nanometres.

use std::collections::{BTreeMap, BTreeSet};

use super::{CopperIndex, CopperKind};
use crate::legacy::Pt;

/// A point in board units.
pub type BPt = (i64, i64);

/// Maps between the file's nanometres and the board's units.
pub trait BoardFrame {
    /// A file point to board units, as the board builder rounds it.
    fn to_board(&self, p: Pt) -> BPt;
    /// A track width in nanometres to the board's half width.
    fn half_width(&self, width_nm: i32) -> i64;
    /// A board point back to file nanometres (exact).
    fn to_nm(&self, b: BPt) -> Pt;
    /// A board length (a half width doubled) in nanometres.
    fn length_nm(&self, units: i64) -> i64;
}

/// A frame of `nm_per_unit` nanometres per board unit, Y negated, rounding
/// half away from zero. Tests use it; the engine maps through its own
/// coordinate transform.
#[derive(Clone, Copy, Debug)]
pub struct GridFrame {
    pub nm_per_unit: i64,
}

fn div_round(v: i64, d: i64) -> i64 {
    let q = v / d;
    let r = v % d;
    if 2 * r.abs() >= d { q + v.signum() } else { q }
}

impl BoardFrame for GridFrame {
    fn to_board(&self, p: Pt) -> BPt {
        (
            div_round(i64::from(p.0), self.nm_per_unit),
            div_round(-i64::from(p.1), self.nm_per_unit),
        )
    }
    fn half_width(&self, width_nm: i32) -> i64 {
        div_round(i64::from(width_nm), 2 * self.nm_per_unit)
    }
    fn to_nm(&self, b: BPt) -> Pt {
        (
            (b.0 * self.nm_per_unit) as i32,
            (-b.1 * self.nm_per_unit) as i32,
        )
    }
    fn length_nm(&self, units: i64) -> i64 {
        units * self.nm_per_unit
    }
}

/// A trace of the routed board.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FinalTrace {
    /// Copper position (0 = `F.Cu`).
    pub layer: usize,
    /// KiCad net code.
    pub net: u32,
    pub half_width: i64,
    pub corners: Vec<BPt>,
}

/// A via of the routed board.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FinalVia {
    pub at: BPt,
    pub net: u32,
    pub top: usize,
    pub bottom: usize,
    pub diameter: i32,
    pub drill: i32,
}

/// A copper item of the routed board.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FinalItem {
    Trace(FinalTrace),
    Via(FinalVia),
}

/// Copper to add to the file, in nanometres.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NewItem {
    Segment {
        start: Pt,
        end: Pt,
        width: i32,
        layer: usize,
        net: u32,
    },
    Via {
        at: Pt,
        diameter: i32,
        drill: i32,
        top: usize,
        bottom: usize,
        net: u32,
        /// `blind`, `buried`, `micro` or empty.
        kind: String,
    },
}

/// The result of [`diff`]: indices into [`CopperIndex::items`] and the new
/// copper.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CopperDiff {
    pub keep: Vec<usize>,
    pub remove: Vec<usize>,
    pub add: Vec<NewItem>,
}

impl CopperDiff {
    /// Nothing removed and nothing added.
    pub fn is_empty(&self) -> bool {
        self.remove.is_empty() && self.add.is_empty()
    }
}

fn cross(a: BPt, b: BPt, c: BPt) -> i128 {
    let (ux, uy) = (i128::from(b.0 - a.0), i128::from(b.1 - a.1));
    let (vx, vy) = (i128::from(c.0 - a.0), i128::from(c.1 - a.1));
    ux * vy - uy * vx
}

fn dot(a: BPt, b: BPt, c: BPt) -> i128 {
    let (ux, uy) = (i128::from(b.0 - a.0), i128::from(b.1 - a.1));
    let (vx, vy) = (i128::from(c.0 - a.0), i128::from(c.1 - a.1));
    ux * vx + uy * vy
}

fn len2(a: BPt, b: BPt) -> i128 {
    dot(a, b, b)
}

/// Whether `p` lies on the closed segment `a`-`b`.
fn on_segment(a: BPt, b: BPt, p: BPt) -> bool {
    if a == b {
        return p == a;
    }
    if cross(a, b, p) != 0 {
        return false;
    }
    let t = dot(a, b, p);
    t >= 0 && t <= len2(a, b)
}

/// The key traces are compared under.
type TraceKey = (usize, u32, i64);

/// The board-unit pieces of an original track (zero-length pieces dropped;
/// a track that is a point is one degenerate piece).
fn pieces(points: &[Pt], frame: &dyn BoardFrame) -> Vec<(BPt, BPt)> {
    let b: Vec<BPt> = points.iter().map(|p| frame.to_board(*p)).collect();
    let mut out: Vec<(BPt, BPt)> = b
        .windows(2)
        .filter(|w| w[0] != w[1])
        .map(|w| (w[0], w[1]))
        .collect();
    if out.is_empty()
        && let Some(&p) = b.first()
    {
        out.push((p, p));
    }
    out
}

/// The diff of the file's copper against the routed board's items (in the
/// board's item order). `final_items` lists traces and vias only.
pub fn diff(index: &CopperIndex, final_items: &[FinalItem], frame: &dyn BoardFrame) -> CopperDiff {
    // final trace segments by key
    let mut segments: BTreeMap<TraceKey, Vec<(BPt, BPt)>> = BTreeMap::new();
    let mut vias: BTreeSet<&FinalVia> = BTreeSet::new();
    for item in final_items {
        match item {
            FinalItem::Trace(t) => {
                let list = segments.entry((t.layer, t.net, t.half_width)).or_default();
                for w in t.corners.windows(2) {
                    if w[0] != w[1] {
                        list.push((w[0], w[1]));
                    }
                }
            }
            FinalItem::Via(v) => {
                vias.insert(v);
            }
        }
    }

    let mut out = CopperDiff::default();
    // kept pieces by key, for the coverage of final segments
    let mut kept_pieces: BTreeMap<TraceKey, Vec<(BPt, BPt)>> = BTreeMap::new();
    let mut kept_vias: BTreeSet<FinalVia> = BTreeSet::new();
    // exact nanometres of original ends, first in file order wins
    let mut snap: BTreeMap<BPt, Pt> = BTreeMap::new();
    let mut widths: BTreeMap<i64, i32> = BTreeMap::new();
    let mut via_kinds: BTreeMap<(i32, i32, usize, usize), String> = BTreeMap::new();

    for (i, item) in index.items.iter().enumerate() {
        let Some(net) = item.net.filter(|_| item.routable) else {
            out.keep.push(i);
            continue;
        };
        match &item.kind {
            CopperKind::Segment | CopperKind::Arc { .. } => {
                let (Some(layer), Some(width)) = (item.layer, item.width) else {
                    out.keep.push(i);
                    continue;
                };
                let hw = frame.half_width(width);
                widths.entry(hw).or_insert(width);
                for p in [item.start, item.end] {
                    snap.entry(frame.to_board(p)).or_insert(p);
                }
                let key = (layer, net, hw);
                let own = pieces(&item.points, frame);
                let finals = segments.get(&key).map(Vec::as_slice).unwrap_or(&[]);
                let covered = own.iter().all(|&(c, d)| {
                    if c == d {
                        finals.iter().any(|&(a, b)| on_segment(a, b, c))
                    } else {
                        uncovered(c, d, finals).is_empty()
                    }
                });
                if covered || item.locked {
                    out.keep.push(i);
                    kept_pieces.entry(key).or_default().extend(own);
                } else {
                    out.remove.push(i);
                }
            }
            CopperKind::Via { kind } => {
                let (Some((top, bottom)), Some(diameter)) = (item.span, item.width) else {
                    out.keep.push(i);
                    continue;
                };
                let drill = item.drill.unwrap_or(0);
                let at = frame.to_board(item.start);
                snap.entry(at).or_insert(item.start);
                if !kind.is_empty() {
                    via_kinds
                        .entry((diameter, drill, top, bottom))
                        .or_insert_with(|| kind.clone());
                }
                let this = FinalVia {
                    at,
                    net,
                    top,
                    bottom,
                    diameter,
                    drill,
                };
                if item.locked || vias.contains(&this) {
                    out.keep.push(i);
                    kept_vias.insert(this);
                } else {
                    out.remove.push(i);
                }
            }
        }
    }

    let copper = index.layers.len();
    // the reader's merge state after the kept tracks: (net, width, layer) and
    // the end of the last one in file order
    let mut last_track: Option<((u32, i32, usize), Pt)> = out
        .keep
        .iter()
        .rev()
        .map(|&i| &index.items[i])
        .find(|i| i.routable && !matches!(i.kind, CopperKind::Via { .. }))
        .and_then(|i| Some(((i.net?, i.width?, i.layer?), i.end)));
    let to_nm = |b: BPt| snap.get(&b).copied().unwrap_or_else(|| frame.to_nm(b));
    for item in final_items {
        match item {
            FinalItem::Trace(t) => {
                let key = (t.layer, t.net, t.half_width);
                let width = widths
                    .get(&t.half_width)
                    .copied()
                    .unwrap_or_else(|| frame.length_nm(2 * t.half_width) as i32);
                let kept = kept_pieces.get(&key).map(Vec::as_slice).unwrap_or(&[]);
                let mut run: Vec<(Pt, Pt)> = Vec::new();
                for w in t.corners.windows(2) {
                    if w[0] == w[1] {
                        continue;
                    }
                    for (s, e) in uncovered(w[0], w[1], kept) {
                        run.push((to_nm(s), to_nm(e)));
                    }
                }
                let Some(&(first, _)) = run.first() else {
                    continue;
                };
                // The reader (like KiCad's export) merges a track into the
                // previous wire when it starts where that one ended; a trace
                // that would chain onto the previous one is written backwards
                // so that it reads back as its own trace.
                let state = (t.net, width, t.layer);
                if last_track == Some((state, first)) {
                    run = run.into_iter().rev().map(|(s, e)| (e, s)).collect();
                }
                if let Some(&(_, end)) = run.last() {
                    last_track = Some((state, end));
                }
                out.add
                    .extend(run.into_iter().map(|(start, end)| NewItem::Segment {
                        start,
                        end,
                        width,
                        layer: t.layer,
                        net: t.net,
                    }));
            }
            FinalItem::Via(v) => {
                if kept_vias.contains(v) {
                    continue;
                }
                let kind = via_kinds
                    .get(&(v.diameter, v.drill, v.top, v.bottom))
                    .cloned()
                    .unwrap_or_else(|| span_kind(v.top, v.bottom, copper).to_owned());
                out.add.push(NewItem::Via {
                    at: to_nm(v.at),
                    diameter: v.diameter,
                    drill: v.drill,
                    top: v.top,
                    bottom: v.bottom,
                    net: v.net,
                    kind,
                });
            }
        }
    }
    out
}

/// KiCad's via type of a span: through, `blind` (one outer layer) or
/// `buried` (inner layers only).
pub fn span_kind(top: usize, bottom: usize, copper: usize) -> &'static str {
    let last = copper.saturating_sub(1);
    if top == 0 && bottom == last {
        ""
    } else if top == 0 || bottom == last {
        "blind"
    } else {
        "buried"
    }
}

/// The parts of `a`-`b` that the `kept` pieces collinear with it do not
/// cover, from `a` towards `b`. The ends of each part are `a`, `b` or ends of
/// kept pieces, so they are exact board points.
fn uncovered(a: BPt, b: BPt, kept: &[(BPt, BPt)]) -> Vec<(BPt, BPt)> {
    let l2 = len2(a, b);
    let param = |p: BPt| dot(a, b, p);
    // covered intervals of the parameter, with the points at their ends
    let mut intervals: Vec<((i128, BPt), (i128, BPt))> = Vec::new();
    for &(c, d) in kept {
        if c == d || cross(a, b, c) != 0 || cross(a, b, d) != 0 {
            continue;
        }
        let (tc, td) = (param(c), param(d));
        let (lo, hi) = if tc <= td {
            ((tc, c), (td, d))
        } else {
            ((td, d), (tc, c))
        };
        if hi.0 <= 0 || lo.0 >= l2 {
            continue;
        }
        let lo = if lo.0 < 0 { (0, a) } else { lo };
        let hi = if hi.0 > l2 { (l2, b) } else { hi };
        intervals.push((lo, hi));
    }
    intervals.sort_by_key(|(lo, hi)| (lo.0, hi.0));
    let mut out = Vec::new();
    let mut at: (i128, BPt) = (0, a);
    for (lo, hi) in intervals {
        if lo.0 > at.0 {
            out.push((at.1, lo.1));
        }
        if hi.0 > at.0 {
            at = hi;
        }
    }
    if at.0 < l2 {
        out.push((at.1, b));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uncovered_parts() {
        let (a, b) = ((0, 0), (10, 0));
        assert_eq!(uncovered(a, b, &[]), vec![(a, b)]);
        assert!(uncovered(a, b, &[((10, 0), (0, 0))]).is_empty());
        assert_eq!(
            uncovered(a, b, &[((2, 0), (4, 0)), ((3, 0), (6, 0))]),
            vec![(a, (2, 0)), ((6, 0), b)]
        );
        // not collinear: ignored
        assert_eq!(uncovered(a, b, &[((0, 1), (10, 1))]), vec![(a, b)]);
        // overhanging both ends
        assert!(uncovered(a, b, &[((-5, 0), (15, 0))]).is_empty());
    }

    #[test]
    fn on_segment_is_exact_at_the_limits() {
        let m = i64::from(i32::MAX);
        let (a, b) = ((-m, -m), (m, m));
        assert!(on_segment(a, b, (0, 0)));
        assert!(on_segment(a, b, (m - 1, m - 1)));
        assert!(!on_segment(a, b, (m - 1, m)));
        assert!(!on_segment(a, (m - 1, m - 1), (m, m)));
    }

    #[test]
    fn grid_frame_rounds_half_away() {
        let f = GridFrame { nm_per_unit: 100 };
        assert_eq!(f.to_board((150, -150)), (2, 2));
        assert_eq!(f.to_board((-149, 149)), (-1, -1));
        assert_eq!(f.to_nm((2, 2)), (200, -200));
        assert_eq!(f.half_width(250_000), 1250);
        assert_eq!(f.half_width(250), 1);
    }

    #[test]
    fn span_kinds() {
        assert_eq!(span_kind(0, 3, 4), "");
        assert_eq!(span_kind(0, 1, 4), "blind");
        assert_eq!(span_kind(2, 3, 4), "blind");
        assert_eq!(span_kind(1, 2, 4), "buried");
    }
}
