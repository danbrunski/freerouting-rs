//! A zone fill as fixed wires (K14.4): the arithmetic of the plugin's
//! `fixed_pour_classes` fix-up in `wires` mode.
//!
//! Ported from `kicad-plugin/freerouting_rs/dsn_fixups.py` (last changed in
//! `9651fab`; `fill_rects` and `_wire` are unchanged since the port of
//! canary's `dsn_export.py`, `dbfe7b67`): [`um`], `_crossings`, `_intervals`,
//! `_intersect`, [`fill_rects`], [`SLAB_MM`], [`MIN_MM`] and the geometry of
//! `_wire` ([`wire`]). The DSN text `_wire` formats stays out; the reader
//! builds tree nodes from [`FixedWire::dsn_path_um`].
//!
//! The port is operation for operation in `f64`, in the Python's order, so the
//! same chains give bit-identical rectangles and the same integers after
//! [`um`]:
//! - [`um`] is Python's `round(v * 1000)`, which rounds halves to even
//!   (`f64::round_ties_even`, not `f64::round`).
//! - Python's two-argument `min` and `max` return the first argument unless
//!   the second is strictly smaller or larger ([`py_min`], [`py_max`]); the
//!   `min(xs)` and `max(xs)` of a list keep the first of equal values. That
//!   decides `-0.0` against `0.0`, where `f64::min` need not.
//! - The crossings sort with `partial_cmp`, stably, as Python's `list.sort`
//!   (only `<`, stable): `-0.0` and `0.0` stay in the order they came.
//! - The probes `x + 1e-4` and `x1 - 1e-4`, the half-open test
//!   `xa <= x < xb` in `_crossings`, `hi - lo >= MIN_MM` in `_intersect`, and
//!   the strip merge (each later strip compared with the first strip of its
//!   run, `abs(p - q) < 1e-3` on both ends) are the Python's.
//!
//! # Frame
//!
//! Points are in millimetres in the board's own frame, y down, as
//! `kicad_board._chains` takes them from pcbnew (`pcbnew.ToMM` of the
//! nanometre coordinates: `nm / 1e6`, which equals the `f64` the file's
//! six-decimal text parses to). [`fill_rects`] and [`wire`] work in that
//! frame. The y flip belongs to the DSN: `_wire` writes `{um(x)} {-um(y)}`,
//! the negation applied to the rounded integer. [`FixedWire::dsn_path_um`]
//! gives those integers; the reader's tree (KiCad's DSN frame, `um`, y
//! negated, see [`crate::units::map_pt`]) takes them as `f64`, so it equals the
//! fixed-up export parsed. Negating the integer gives `0`, never `-0.0`.

/// Width of the vertical strips a pour fill is cut into (`SLAB_MM`).
pub const SLAB_MM: f64 = 0.25;

/// Strips and intervals thinner than this are dropped (`MIN_MM`).
pub const MIN_MM: f64 = 0.05;

/// A point `(x, y)` in mm, board frame (y down).
pub type MmPoint = (f64, f64);

/// A rectangle `(x0, y0, x1, y1)` in mm, as `fill_rects` yields it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FillRect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

/// The fixed path `_wire` lays over one rectangle: its two points and its
/// width, in mm before [`um`], board frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FixedWire {
    pub from: MmPoint,
    pub to: MmPoint,
    pub width: f64,
}

/// A [`FixedWire`] after [`um`], board frame (y down).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FixedWireUm {
    pub from: (i64, i64),
    pub to: (i64, i64),
    pub width: i64,
}

/// `um(v)`: `round(v * 1000)`, mm to integer µm, halves to even. Python's
/// `round` raises on NaN and infinity; here they saturate (`as i64`), and a
/// fill never has them.
pub fn um(v: f64) -> i64 {
    (v * 1000.0).round_ties_even() as i64
}

/// Python's `min(a, b)`: `a` unless `b < a`.
fn py_min(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

/// Python's `max(a, b)`: `a` unless `b > a`.
fn py_max(a: f64, b: f64) -> f64 {
    if b > a { b } else { a }
}

/// `_crossings`: the y of every edge crossing the vertical line at `x`,
/// sorted. An edge counts when `xa <= x < xb` or `xb <= x < xa`.
fn crossings(chains: &[&[MmPoint]], x: f64) -> Vec<f64> {
    let mut ys = Vec::new();
    for pts in chains {
        // zip(pts, pts[1:] + pts[:1])
        for (k, &(xa, ya)) in pts.iter().enumerate() {
            let (xb, yb) = pts[(k + 1) % pts.len()];
            if (xa <= x && x < xb) || (xb <= x && x < xa) {
                ys.push(ya + (x - xa) * (yb - ya) / (xb - xa));
            }
        }
    }
    ys.sort_by(|a, b| a.partial_cmp(b).expect("a fill crossing is never NaN"));
    ys
}

/// `_intervals`: the crossings paired up, even-odd; an odd last one is left
/// out (`range(0, len(ys) - 1, 2)`).
fn intervals(chains: &[&[MmPoint]], x: f64) -> Vec<(f64, f64)> {
    crossings(chains, x)
        .chunks_exact(2)
        .map(|p| (p[0], p[1]))
        .collect()
}

/// `_intersect`: the pairwise overlaps of two interval lists, `a` outer,
/// kept when at least [`MIN_MM`] tall.
fn intersect(a: &[(f64, f64)], b: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for &(a0, a1) in a {
        for &(b0, b1) in b {
            let (lo, hi) = (py_max(a0, b0), py_min(a1, b1));
            if hi - lo >= MIN_MM {
                out.push((lo, hi));
            }
        }
    }
    out
}

/// A strip `x0..x1` and the y intervals inside the fill across it.
struct Strip {
    x0: f64,
    x1: f64,
    inside: Vec<(f64, f64)>,
}

/// The vertical strips of `fill_rects`'s first loop.
fn strips(chains: &[&[MmPoint]]) -> Vec<Strip> {
    let mut xs = chains.iter().flat_map(|pts| pts.iter().map(|p| p.0));
    let first = xs.next().expect("chains are not empty");
    // min(xs), max(xs): the first of equal values
    let (mut x, x_end) = xs.fold((first, first), |(lo, hi), v| {
        (if v < lo { v } else { lo }, if v > hi { v } else { hi })
    });
    let mut out = Vec::new();
    while x < x_end - MIN_MM {
        let x1 = py_min(x + SLAB_MM, x_end);
        let inside = intersect(&intervals(chains, x + 1e-4), &intervals(chains, x1 - 1e-4));
        out.push(Strip { x0: x, x1, inside });
        x = x1;
    }
    out
}

/// Whether strip `b`'s intervals are strip `a`'s, within `1e-3` mm on each end.
fn same_intervals(a: &[(f64, f64)], b: &[(f64, f64)]) -> bool {
    b.len() == a.len()
        && b.iter()
            .zip(a)
            .all(|(p, q)| (p.0 - q.0).abs() < 1e-3 && (p.1 - q.1).abs() < 1e-3)
}

/// `fill_rects(chains)`: rectangles inside a polygon set (outlines and holes,
/// even-odd), from vertical strips of [`SLAB_MM`], neighbouring strips with
/// the same intervals merged. Empty chains are skipped. The order is the
/// generator's: by strip run, then by interval.
pub fn fill_rects(chains: &[Vec<MmPoint>]) -> Vec<FillRect> {
    let chains: Vec<&[MmPoint]> = chains
        .iter()
        .filter(|c| !c.is_empty())
        .map(Vec::as_slice)
        .collect();
    if chains.is_empty() {
        return Vec::new();
    }
    let strips = strips(&chains);
    let mut out = Vec::new();
    let mut i = 0;
    while i < strips.len() {
        let Strip { x0, x1, inside } = &strips[i];
        let mut x1 = *x1;
        let mut j = i + 1;
        while j < strips.len() && same_intervals(inside, &strips[j].inside) {
            x1 = strips[j].x1;
            j += 1;
        }
        out.extend(inside.iter().map(|&(y0, y1)| FillRect {
            x0: *x0,
            y0,
            x1,
            y1,
        }));
        i = j;
    }
    out
}

/// The geometry of `_wire(layer, net, rect, nl)`: a path along the
/// rectangle's long side, as wide as its short side; a square, or one whose
/// sides differ by less than 1 µm, gets a path at least 0.001 mm long.
pub fn wire(rect: FillRect) -> FixedWire {
    let FillRect { x0, y0, x1, y1 } = rect;
    let (w, h) = (x1 - x0, y1 - y0);
    if w >= h {
        let ya = (y0 + y1) / 2.0;
        FixedWire {
            from: (x0 + h / 2.0, ya),
            to: (py_max(x1 - h / 2.0, x0 + h / 2.0 + 0.001), ya),
            width: h,
        }
    } else {
        let xa = (x0 + x1) / 2.0;
        FixedWire {
            from: (xa, y0 + w / 2.0),
            to: (xa, py_max(y1 - w / 2.0, y0 + w / 2.0 + 0.001)),
            width: w,
        }
    }
}

impl FixedWire {
    /// The wire after [`um`], in the board frame.
    pub fn to_um(&self) -> FixedWireUm {
        FixedWireUm {
            from: (um(self.from.0), um(self.from.1)),
            to: (um(self.to.0), um(self.to.1)),
            width: um(self.width),
        }
    }

    /// The integers of the `(path <layer> ...)` `_wire` writes, in order:
    /// `um(width)`, then `um(x) -um(y)` for each point (the DSN frame).
    pub fn dsn_path_um(&self) -> [i64; 5] {
        let u = self.to_um();
        [u.width, u.from.0, -u.from.1, u.to.0, -u.to.1]
    }
}
