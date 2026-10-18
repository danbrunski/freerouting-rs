//! Q17.3 (Q02.5's design): piecewise widen-back with a stepped taper.
//!
//! A neck-down retry ships its connection narrower than the net's class:
//! Java's (`neck_width_um`) along its whole length, `bounded-neck` in the
//! rooms near its pins. [`widen_traces`] runs after the retry inserted the
//! connection and before its pull-tight. For each narrowed trace, in
//! ascending item id (the narrowing ledger's order), it:
//!
//! 1. cuts the trace into pieces of at most [`PIECE_UM`] (0.5 mm), from the
//!    trace's first corner, at exact lattice points of each segment (the
//!    integer points `a + k·(d / gcd(d))` of a segment from `a` along `d`;
//!    on a 90 or 45 degree segment every board unit), so no piece changes
//!    the trace's geometry;
//! 2. tries the class width on each piece with the exact clearance check
//!    (`BasicBoard::check_polyline_trace`, the check of an inserted trace:
//!    its tree shapes against every obstacle, own pins included where the
//!    piece does not end on them), keeping the pieces that pass;
//! 3. with tapers ([`WidenOptions::taper`], the `router.neck_down.widen_back`
//!    switch): drops a wide island between two narrow pieces that is no
//!    longer than two tapers, then replaces the first [`TAPER_UM`] (0.5 mm)
//!    of each wide run at each junction with a narrow piece by 4 steps of
//!    width `wide - (wide - narrow)·k/5`, k = 4..1 from the narrow side
//!    (KRT `_apply_neckdown_widths`); a taper step is narrower than the
//!    wide piece it replaces, on the same centre line, so it fits where the
//!    wide piece did;
//! 4. replaces the trace by one trace per run of equal width (same layer,
//!    nets, clearance class and fixed state), when anything widened.
//!
//! **Zones.** `bounded-neck` (without the switch) widens only the pieces
//! outside its neck zones (a pin's box enlarged by D): the pieces of a
//! segment that meet a zone (to within one lattice step) stay narrow. With
//! the switch every piece is tried.
//!
//! Nothing here narrows or moves copper, and every wide piece passed the
//! check the router's own insertion uses, so the pass adds no clearance
//! violation. The pieces are tried in order along each trace; the result
//! depends only on the board (no hash order, no time).

use freerouting_geometry::{IntPoint, Point, Polyline};

use crate::board::RoutingBoard;
use crate::board::items::ItemId;

/// The longest piece tried at the class width, in micrometres.
pub const PIECE_UM: f64 = 500.0;
/// The length of a taper, in micrometres.
pub const TAPER_UM: f64 = 500.0;
/// The steps of a taper.
pub const TAPER_STEPS: i64 = 4;

/// How a widen-back pass runs.
#[derive(Clone, Debug, PartialEq)]
pub struct WidenOptions {
    /// The longest piece, board units.
    pub piece_len: f64,
    /// Tapers and the island rule (the `widen_back` switch).
    pub taper: bool,
    /// The taper length, board units.
    pub taper_len: f64,
    /// Boxes `[x0, y0, x1, y1]` whose pieces stay narrow (`bounded-neck`'s
    /// neck zones); empty to try every piece.
    pub zones: Vec<[i64; 4]>,
}

impl WidenOptions {
    /// The options on `board`: 0.5 mm pieces and tapers, with or without
    /// the tapers, and the zones that stay narrow.
    pub fn on_board(board: &RoutingBoard, taper: bool, zones: Vec<[i64; 4]>) -> WidenOptions {
        WidenOptions {
            piece_len: um_to_units(board, PIECE_UM),
            taper,
            taper_len: um_to_units(board, TAPER_UM),
            zones,
        }
    }
}

/// `um` micrometres in board units.
pub fn um_to_units(board: &RoutingBoard, um: f64) -> f64 {
    let resolution = board.communication.resolution.max(1);
    crate::board::structure::Unit::scale(
        um * f64::from(resolution),
        crate::board::structure::Unit::Um,
        board.communication.unit,
    )
}

/// A straight part of a trace: `n` lattice steps `step` from `from`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Piece {
    from: (i64, i64),
    step: (i64, i64),
    n: i64,
    half_width: i32,
    /// Tried at the class width.
    candidate: bool,
}

impl Piece {
    fn point(&self, k: i64) -> (i64, i64) {
        (self.from.0 + k * self.step.0, self.from.1 + k * self.step.1)
    }

    fn to(&self) -> (i64, i64) {
        self.point(self.n)
    }

    fn unit(&self) -> f64 {
        (self.step.0 as f64).hypot(self.step.1 as f64)
    }

    fn len(&self) -> f64 {
        self.n as f64 * self.unit()
    }

    /// The part from step `k0` to step `k1`.
    fn part(&self, k0: i64, k1: i64) -> Piece {
        Piece {
            from: self.point(k0),
            n: k1 - k0,
            ..*self
        }
    }
}

fn gcd(a: i64, b: i64) -> i64 {
    let (mut a, mut b) = (a.abs(), b.abs());
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// The steps `k` in `0..=g` with `lo <= a + k·s <= hi`, as `(first, last)`.
fn axis_range(a: i64, s: i64, lo: i64, hi: i64, g: i64) -> Option<(i64, i64)> {
    if s == 0 {
        return (lo <= a && a <= hi).then_some((0, g));
    }
    let (first, last) = if s > 0 {
        (ceil_div(lo - a, s), (hi - a).div_euclid(s))
    } else {
        (ceil_div(hi - a, s), (lo - a).div_euclid(s))
    };
    let (first, last) = (first.max(0), last.min(g));
    (first <= last).then_some((first, last))
}

/// `ceil(a / b)` for `b != 0`.
fn ceil_div(a: i64, b: i64) -> i64 {
    -((-a).div_euclid(b))
}

/// The pieces of the segment from `a` to `b` at half width `half_width`:
/// the parts meeting a zone (widened by one step each way) are single
/// pieces that stay narrow, the rest is cut into candidate pieces of at
/// most `piece_len`.
fn segment_pieces(
    a: (i64, i64),
    b: (i64, i64),
    half_width: i32,
    zones: &[[i64; 4]],
    piece_len: f64,
) -> Vec<Piece> {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let g = gcd(dx, dy);
    if g == 0 {
        return Vec::new();
    }
    let whole = Piece {
        from: a,
        step: (dx / g, dy / g),
        n: g,
        half_width,
        candidate: true,
    };
    let mut near: Vec<(i64, i64)> = zones
        .iter()
        .filter_map(|z| {
            let x = axis_range(a.0, whole.step.0, z[0], z[2], g)?;
            let y = axis_range(a.1, whole.step.1, z[1], z[3], g)?;
            let (first, last) = (x.0.max(y.0), x.1.min(y.1));
            (first <= last).then_some(((first - 1).max(0), (last + 1).min(g)))
        })
        .collect();
    near.sort_unstable();
    let mut pieces = Vec::new();
    let mut k = 0;
    for (first, last) in near {
        if last <= k && first < k {
            continue;
        }
        let first = first.max(k);
        far_pieces(&whole, k, first, piece_len, &mut pieces);
        if last > first {
            pieces.push(Piece {
                candidate: false,
                ..whole.part(first, last)
            });
        }
        k = k.max(last);
    }
    far_pieces(&whole, k, g, piece_len, &mut pieces);
    pieces
}

/// Candidate pieces of at most `piece_len` from step `k0` to `k1`.
fn far_pieces(whole: &Piece, k0: i64, k1: i64, piece_len: f64, out: &mut Vec<Piece>) {
    if k1 <= k0 {
        return;
    }
    let steps = k1 - k0;
    let len = steps as f64 * whole.unit();
    let m = ((len / piece_len.max(1.0)).ceil() as i64).clamp(1, steps);
    let mut prev = k0;
    for j in 1..=m {
        let next = k0 + steps * j / m;
        if next > prev {
            out.push(whole.part(prev, next));
            prev = next;
        }
    }
}

/// The half width of taper step `k` (1..=4, 4 nearest the narrow side).
fn taper_half_width(wide: i32, narrow: i32, k: i64) -> i32 {
    let delta = i64::from(wide) - i64::from(narrow);
    (i64::from(wide) - delta * k / (TAPER_STEPS + 1)) as i32
}

/// Splits the pieces of a run so that a boundary lies at each distance of
/// `cuts` (ascending) from the run's start, at the lattice point at or
/// before it.
fn split_at(run: Vec<Piece>, cuts: &[f64]) -> Vec<Piece> {
    let mut out = Vec::with_capacity(run.len() + cuts.len());
    let mut start = 0.0;
    let mut cut = cuts.iter().copied().peekable();
    for piece in run {
        let len = piece.len();
        let mut k0 = 0;
        while let Some(&c) = cut.peek() {
            if c >= start + len {
                break;
            }
            cut.next();
            let k = (((c - start) / piece.unit()).floor() as i64).clamp(0, piece.n);
            if k > k0 {
                out.push(piece.part(k0, k));
                k0 = k;
            }
        }
        if piece.n > k0 {
            out.push(piece.part(k0, piece.n));
        }
        start += len;
    }
    out
}

/// The tapers of a wide run of half width `wide` next to narrow pieces of
/// half width `narrow` at its start and/or end.
fn taper_run(
    run: Vec<Piece>,
    narrow: i32,
    wide: i32,
    at_start: bool,
    at_end: bool,
    taper_len: f64,
) -> Vec<Piece> {
    let total: f64 = run.iter().map(Piece::len).sum();
    let t = if at_start && at_end {
        taper_len.min(total / 2.0)
    } else {
        taper_len.min(total)
    };
    let quarter = t / TAPER_STEPS as f64;
    let mut cuts = Vec::new();
    for j in 1..=TAPER_STEPS {
        if at_start {
            cuts.push(quarter * j as f64);
        }
        if at_end {
            cuts.push(total - quarter * j as f64);
        }
    }
    cuts.sort_by(f64::total_cmp);
    let mut start = 0.0;
    split_at(run, &cuts)
        .into_iter()
        .map(|mut piece| {
            let len = piece.len();
            let middle = start + len / 2.0;
            start += len;
            let from_start = (at_start && middle < t).then_some(middle);
            let from_end = (at_end && total - middle < t).then_some(total - middle);
            if let Some(d) = from_start.into_iter().chain(from_end).reduce(f64::min) {
                let j = ((d / quarter).floor() as i64).clamp(0, TAPER_STEPS - 1);
                piece.half_width = taper_half_width(wide, narrow, TAPER_STEPS - j);
            }
            piece
        })
        .collect()
}

/// The runs of equal `half_width` (a wide run is a run whose pieces have
/// half width `wide`).
fn runs(pieces: &[Piece]) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let mut start = 0;
    for i in 1..=pieces.len() {
        if i == pieces.len() || pieces[i].half_width != pieces[start].half_width {
            out.push(start..i);
            start = i;
        }
    }
    out
}

/// The island rule and the tapers on `pieces` (narrow `narrow`, wide
/// `wide`).
fn with_tapers(mut pieces: Vec<Piece>, narrow: i32, wide: i32, taper_len: f64) -> Vec<Piece> {
    // Islands: a wide run between two narrow runs no longer than two tapers.
    let all = runs(&pieces);
    for (i, r) in all.iter().enumerate() {
        let inner = i > 0 && i + 1 < all.len();
        let len: f64 = pieces[r.clone()].iter().map(Piece::len).sum();
        if inner && pieces[r.start].half_width == wide && len <= 2.0 * taper_len {
            for p in &mut pieces[r.clone()] {
                p.half_width = narrow;
            }
        }
    }
    let all = runs(&pieces);
    let mut out = Vec::with_capacity(pieces.len() + 8);
    for (i, r) in all.iter().enumerate() {
        let run = pieces[r.clone()].to_vec();
        if run[0].half_width != wide {
            out.extend(run);
            continue;
        }
        let at_start = i > 0;
        let at_end = i + 1 < all.len();
        out.extend(taper_run(run, narrow, wide, at_start, at_end, taper_len));
    }
    out
}

/// The corners of a run of pieces.
fn run_points(run: &[Piece]) -> Vec<Point> {
    let mut points: Vec<Point> = Vec::with_capacity(run.len() + 1);
    let mut push = |p: (i64, i64)| {
        let p = Point::Int(IntPoint::new(p.0 as i32, p.1 as i32));
        if points.last() != Some(&p) {
            points.push(p);
        }
    };
    push(run[0].from);
    for piece in run {
        push(piece.to());
    }
    points
}

/// What [`widen_traces`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WidenReport {
    /// The traces replaced, ascending id.
    pub replaced: Vec<ItemId>,
    /// The traces inserted in their place, in insertion order.
    pub inserted: Vec<ItemId>,
}

/// Widens the traces `ids` (ascending id) towards the half width
/// `target(layer)` where the exact clearance check lets it (see the module
/// docs).
pub fn widen_traces(
    board: &mut RoutingBoard,
    ids: &[ItemId],
    target: &dyn Fn(i32) -> i32,
    options: &WidenOptions,
) -> WidenReport {
    let mut report = WidenReport::default();
    for &id in ids {
        if let Some(inserted) = widen_trace(board, id, target, options) {
            report.replaced.push(id);
            report.inserted.extend(inserted);
        }
    }
    report
}

/// Widens one trace; the traces that replace it, or `None` if it stays.
fn widen_trace(
    board: &mut RoutingBoard,
    id: ItemId,
    target: &dyn Fn(i32) -> i32,
    options: &WidenOptions,
) -> Option<Vec<ItemId>> {
    let item = board.item(id)?;
    let trace = item.as_trace()?;
    let (layer, narrow) = (trace.layer, trace.half_width);
    let wide = target(layer);
    if narrow >= wide {
        return None;
    }
    let nets = item.net_numbers().to_vec();
    let (class, fixed) = (item.clearance_class(), item.fixed_state());
    let corners: Vec<(i64, i64)> = trace
        .polyline
        .corners()
        .iter()
        .map(|c| c.as_int().map(|p| (i64::from(p.x), i64::from(p.y))))
        .collect::<Option<_>>()?;
    let mut pieces: Vec<Piece> = corners
        .windows(2)
        .flat_map(|w| segment_pieces(w[0], w[1], narrow, &options.zones, options.piece_len))
        .collect();
    let mut widened = false;
    for piece in pieces.iter_mut().filter(|p| p.candidate) {
        let run = [*piece];
        let points = run_points(&run);
        let polyline = Polyline::from_points(&points);
        if board.check_polyline_trace(&polyline, layer, wide, &nets, class) {
            piece.half_width = wide;
            widened = true;
        }
    }
    if !widened {
        return None;
    }
    if options.taper {
        pieces = with_tapers(pieces, narrow, wide, options.taper_len);
    }
    board.remove_item(id);
    let mut inserted = Vec::new();
    for r in runs(&pieces) {
        let points = run_points(&pieces[r.clone()]);
        if points.len() < 2 {
            continue;
        }
        let polyline = Polyline::from_points(&points);
        let half_width = pieces[r.start].half_width;
        if let Some(new) =
            board.insert_trace_without_cleaning(polyline, layer, half_width, &nets, class, fixed)
        {
            inserted.push(new);
        }
    }
    Some(inserted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn total(pieces: &[Piece]) -> i64 {
        pieces
            .iter()
            .map(|p| p.n * p.step.0.abs().max(p.step.1.abs()))
            .sum()
    }

    #[test]
    fn pieces_cut_a_segment_at_lattice_points_without_gaps() {
        let pieces = segment_pieces((0, 0), (10_000, 0), 100, &[], 3000.0);
        assert_eq!(pieces.len(), 4);
        assert_eq!(total(&pieces), 10_000);
        for w in pieces.windows(2) {
            assert_eq!(w[0].to(), w[1].from);
        }
        assert!(pieces.iter().all(|p| p.candidate && p.len() <= 3000.0));
        // A 45 degree segment: every unit is a lattice point.
        let diagonal = segment_pieces((0, 0), (-700, 700), 100, &[], 300.0);
        assert_eq!(diagonal.last().map(Piece::to), Some((-700, 700)));
        // A segment with no inner lattice point is one piece.
        let odd = segment_pieces((0, 0), (7, 3), 100, &[], 1.0);
        assert_eq!(odd.len(), 1);
    }

    #[test]
    fn the_part_meeting_a_zone_stays_narrow() {
        let zone = [2000, -50, 3000, 50];
        let pieces = segment_pieces((0, 0), (10_000, 0), 100, &[zone], 500.0);
        let narrow: Vec<&Piece> = pieces.iter().filter(|p| !p.candidate).collect();
        assert_eq!(narrow.len(), 1);
        assert_eq!((narrow[0].from, narrow[0].to()), ((1999, 0), (3001, 0)));
        assert_eq!(total(&pieces), 10_000);
        // A zone the segment misses changes nothing.
        let missed = segment_pieces((0, 0), (10_000, 0), 100, &[[0, 100, 10, 200]], 500.0);
        assert!(missed.iter().all(|p| p.candidate));
    }

    #[test]
    fn axis_ranges_are_exact_for_negative_steps() {
        assert_eq!(axis_range(10, -1, 3, 5, 10), Some((5, 7)));
        assert_eq!(axis_range(10, 1, 3, 5, 10), None);
        assert_eq!(axis_range(0, 3, 4, 9, 10), Some((2, 3)));
        assert_eq!(axis_range(4, 0, 4, 9, 10), Some((0, 10)));
    }

    #[test]
    fn a_taper_steps_down_towards_the_narrow_side() {
        assert_eq!(taper_half_width(7500, 2000, 4), 3100);
        assert_eq!(taper_half_width(7500, 2000, 1), 6400);
        let narrow = |n: i64, x: i64| Piece {
            from: (x, 0),
            step: (1, 0),
            n,
            half_width: 2000,
            candidate: false,
        };
        let wide = |n: i64, x: i64| Piece {
            half_width: 7500,
            ..narrow(n, x)
        };
        let pieces = vec![narrow(1000, 0), wide(10_000, 1000), narrow(1000, 11_000)];
        let tapered = with_tapers(pieces, 2000, 7500, 4000.0);
        let widths: Vec<(i64, i32)> = tapered.iter().map(|p| (p.n, p.half_width)).collect();
        assert_eq!(
            widths,
            [
                (1000, 2000),
                (1000, 3100),
                (1000, 4200),
                (1000, 5300),
                (1000, 6400),
                (2000, 7500),
                (1000, 6400),
                (1000, 5300),
                (1000, 4200),
                (1000, 3100),
                (1000, 2000),
            ]
        );
    }

    #[test]
    fn a_wide_island_no_longer_than_two_tapers_goes_back_to_narrow() {
        let piece = |n: i64, x: i64, half_width: i32| Piece {
            from: (x, 0),
            step: (1, 0),
            n,
            half_width,
            candidate: true,
        };
        let pieces = vec![
            piece(100, 0, 20),
            piece(1000, 100, 75),
            piece(100, 1100, 20),
        ];
        let out = with_tapers(pieces, 20, 75, 500.0);
        assert!(out.iter().all(|p| p.half_width == 20));
        let pieces = vec![
            piece(100, 0, 20),
            piece(1001, 100, 75),
            piece(100, 1101, 20),
        ];
        let out = with_tapers(pieces, 20, 75, 500.0);
        assert!(out.iter().any(|p| p.half_width == 75));
    }
}
