// SPDX-License-Identifier: GPL-3.0-only AND MIT
// SPDX-FileCopyrightText: 2026 drandyhaas (KiCadRoutingTools)
// Portions ported from KiCadRoutingTools py_router/bga_fanout/grid.py
// (analyze_bga_grid, _cluster_positions, staggered_lattice_diagnosis) at 0486e799.

//! The ball array of a BGA in its package frame (X12.1).
//!
//! [`analyze_array`] maps every pin of a component to the package frame
//! with X10.1's [`PartFrame`] and fits the centres to a lattice, as KRT's
//! `analyze_bga_grid` does in the footprint frame:
//! - the distinct x and y positions, merging values closer than
//!   [`CLUSTER_TOLERANCE_UM`] (KRT `_COORD_TOL`: rounding noise of a turned
//!   part), each cluster at its mean (exact integer mean, rounded);
//! - per axis the dominant gap: gaps rounded to [`PITCH_ROUNDING_UM`] (KRT
//!   rounds to 0.01 mm), the most frequent rounded value (ties: the
//!   smallest) if at least half the gaps have it; the pitch is the exact
//!   mean of the gaps with that rounded value;
//! - every centre must lie within the cluster tolerance of a lattice point;
//!   row 0 is the top (largest y in the DSN's y-up frame), column 0 the
//!   left.
//!
//! **Which parts are arrays** (the X12 decision made concrete), checked in
//! this order, each failure a [`Rejection`] the stage reports:
//! at least four pins, all SMD on one layer; a lattice on both axes; every
//! centre on it; not staggered; one pitch on both axes (within the
//! cluster tolerance); at least [`MIN_GRID`] × [`MIN_GRID`] positions; and
//! each of the two outer rings at least half populated (a QFN or QFP whose
//! pins happen to fit a lattice has one ring: DAC2020 bm01's 0.5 mm
//! QFP-100 fits a 33 × 33 lattice).
//!
//! **Staggered** (adapted from KRT's `staggered_lattice_diagnosis`): fewer
//! than half the balls have a ball at an orthogonally adjacent lattice
//! position. KRT asks for a nearest neighbour above 1.4 lattice steps and
//! an occupancy below 35 %; the neighbour count also rejects a checkerboard
//! at 50 % occupancy, whose lattice is not the ball pitch either. A
//! depopulated array keeps its neighbours and passes.
//!
//! Determinism (D2): integer arithmetic on the mapped centres, ordered
//! maps; pins sorted by item id before clustering.

use std::collections::BTreeMap;

use freerouting::board::BasicBoard;
use freerouting::board::items::ItemId;
use freerouting::board::structure::ComponentId;
use freerouting::geometry::{IntBox, IntPoint};
use freerouting::job::overrides::um_to_board_units;

use crate::fanout::frame::{PartFrame, Side};

/// KRT `_COORD_TOL` (1e-3 mm): centres closer than this are one position.
pub const CLUSTER_TOLERANCE_UM: f64 = 1.0;
/// KRT's pitch vote rounds gaps to 0.01 mm.
pub const PITCH_ROUNDING_UM: f64 = 10.0;
/// The smallest grid of an array, positions per side (the X12 decision).
pub const MIN_GRID: u16 = 6;

/// Why a component is not a ball array.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rejection {
    /// Fewer than four pins, or the component is not placed.
    TooFewPins,
    /// A pin spans more than one layer, or the pins are on different
    /// layers.
    NotSmd,
    /// No dominant gap on one axis.
    NoLattice,
    /// A centre is off the lattice.
    OffGrid,
    /// The balls sit on a staggered lattice (module docs).
    Staggered,
    /// The pitches of the two axes differ.
    NotSquare,
    /// Fewer than [`MIN_GRID`] positions on an axis.
    TooSmall,
    /// One of the two outer rings is less than half populated.
    Sparse,
}

impl Rejection {
    /// The name used in the log.
    pub fn name(self) -> &'static str {
        match self {
            Rejection::TooFewPins => "too few pins",
            Rejection::NotSmd => "not SMD on one layer",
            Rejection::NoLattice => "no lattice",
            Rejection::OffGrid => "off grid",
            Rejection::Staggered => "staggered",
            Rejection::NotSquare => "not square",
            Rejection::TooSmall => "too small",
            Rejection::Sparse => "sparse outer rings",
        }
    }
}

/// What kind of array a [`BallArray`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrayKind {
    /// Every position has a ball.
    Full,
    /// Some positions are empty (a depopulated centre, missing balls).
    Depopulated,
}

/// A component's balls on their lattice, in the package frame.
#[derive(Clone, Debug, PartialEq)]
pub struct BallArray {
    /// The component.
    pub component: ComponentId,
    /// Its name.
    pub name: String,
    /// The part frame.
    pub frame: PartFrame,
    /// Rows (from the top).
    pub rows: u16,
    /// Columns (from the left).
    pub cols: u16,
    /// The pitch, board units.
    pub pitch: i64,
    /// The part-frame centre of position (0, 0), the top-left.
    pub origin: IntPoint,
    /// Whether each position has a ball, row-major.
    pub populated: Vec<bool>,
    /// The pin at each populated `(row, col)`.
    pub ball_of: BTreeMap<(u16, u16), ItemId>,
    /// The board layer of the balls.
    pub pad_layer: i32,
    /// The box of every pad in the part frame (the pin field).
    pub field: IntBox,
    /// The box of the package outline in the part frame, if it has one.
    pub outline: Option<IntBox>,
}

impl BallArray {
    /// Full or depopulated.
    pub fn kind(&self) -> ArrayKind {
        if self.populated.iter().all(|b| *b) {
            ArrayKind::Full
        } else {
            ArrayKind::Depopulated
        }
    }

    /// True if `(row, col)` has a ball.
    pub fn is_populated(&self, row: u16, col: u16) -> bool {
        row < self.rows
            && col < self.cols
            && self.populated[usize::from(row) * usize::from(self.cols) + usize::from(col)]
    }

    /// The part-frame centre of position `(row, col)`, which may lie off
    /// the array (negative or past the last index).
    pub fn centre(&self, row: i64, col: i64) -> IntPoint {
        let x = i64::from(self.origin.x) + col * self.pitch;
        let y = i64::from(self.origin.y) - row * self.pitch;
        IntPoint::new(clamp_i32(x), clamp_i32(y))
    }

    /// The ring of `(row, col)`: 0 for the outer ring.
    pub fn ring(&self, row: u16, col: u16) -> u16 {
        row.min(col)
            .min(self.rows - 1 - row)
            .min(self.cols - 1 - col)
    }

    /// The number of rings.
    pub fn ring_count(&self) -> u16 {
        self.rows.min(self.cols).div_ceil(2)
    }

    /// The side `(row, col)` escapes through: its ring's edges as a
    /// pinwheel, each side owning the corner where it starts clockwise
    /// from the top-left (N the top-left corner, E the top-right, S the
    /// bottom-right, W the bottom-left); a single-ball ring is N.
    pub fn side(&self, row: u16, col: u16) -> Side {
        let k = self.ring(row, col);
        let (last_row, last_col) = (self.rows - 1 - k, self.cols - 1 - k);
        if row == k && col < last_col {
            Side::N
        } else if col == last_col && row < last_row {
            Side::E
        } else if row == last_row && col > k {
            Side::S
        } else if col == k && row > k {
            Side::W
        } else {
            Side::N
        }
    }

    /// Every position of ring `k` clockwise from its top-left corner:
    /// along the top, down the right, back along the bottom, up the left.
    pub fn ring_positions(&self, k: u16) -> Vec<(u16, u16)> {
        let (last_row, last_col) = (self.rows - 1 - k, self.cols - 1 - k);
        let mut out = Vec::new();
        for c in k..=last_col {
            out.push((k, c));
        }
        for r in k + 1..=last_row {
            out.push((r, last_col));
        }
        if last_row > k {
            for c in (k..last_col).rev() {
                out.push((last_row, c));
            }
        }
        if last_col > k {
            for r in (k + 1..last_row).rev() {
                out.push((r, k));
            }
        }
        out
    }

    /// The populated positions in escape order: rings outside in, each
    /// clockwise from its top-left (the X12 order).
    pub fn escape_order(&self) -> Vec<(u16, u16)> {
        (0..self.ring_count())
            .flat_map(|k| self.ring_positions(k))
            .filter(|&(r, c)| self.is_populated(r, c))
            .collect()
    }

    /// The box the escapes leave: the pin field united with the outline.
    pub fn escape_box(&self) -> IntBox {
        match &self.outline {
            Some(o) => self.field.union_box(o),
            None => self.field,
        }
    }
}

fn clamp_i32(v: i64) -> i32 {
    v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// One pin in the part frame.
struct FramePin {
    id: ItemId,
    centre: IntPoint,
    pad: IntBox,
}

/// The positions of `values` merged where consecutive sorted values are at
/// most `tol` apart, each cluster at its mean (KRT `_cluster_positions`).
fn cluster(values: &[i64], tol: i64) -> Vec<i64> {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let mut out = Vec::new();
    let mut group: Vec<i64> = Vec::new();
    for v in sorted {
        if let Some(&last) = group.last()
            && v - last > tol
        {
            out.push(mean(&group));
            group.clear();
        }
        group.push(v);
    }
    if !group.is_empty() {
        out.push(mean(&group));
    }
    out
}

/// The rounded mean of a non-empty group (half away from zero).
fn mean(group: &[i64]) -> i64 {
    let n = group.len() as i128;
    let sum: i128 = group.iter().map(|&v| i128::from(v)).sum();
    let q = (2 * sum + n * sum.signum()) / (2 * n);
    q as i64
}

/// KRT's dominant pitch of the gaps between cluster positions: gaps
/// rounded to `rounding`, the most frequent rounded value (ties: the
/// smallest) held by at least half the gaps, then the exact mean of the
/// gaps with that value; `None` if there is none or it is at most `tol`.
fn dominant_pitch(positions: &[i64], rounding: i64, tol: i64) -> Option<i64> {
    let gaps: Vec<i64> = positions.windows(2).map(|w| w[1] - w[0]).collect();
    if gaps.is_empty() {
        return None;
    }
    let round = |g: i64| ((g + rounding / 2) / rounding) * rounding;
    let mut counts: BTreeMap<i64, usize> = BTreeMap::new();
    for &g in &gaps {
        *counts.entry(round(g)).or_default() += 1;
    }
    let (&value, &count) = counts
        .iter()
        .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))?;
    if 2 * count < gaps.len() {
        return None;
    }
    let members: Vec<i64> = gaps
        .iter()
        .copied()
        .filter(|&g| round(g) == value)
        .collect();
    let pitch = mean(&members);
    (pitch > tol).then_some(pitch)
}

/// The lattice index of `v` from `start` at `pitch`, if `v` is within
/// `tol` of a lattice point.
fn lattice_index(v: i64, start: i64, pitch: i64, tol: i64) -> Option<i64> {
    let d = v - start;
    let index = if d >= 0 {
        (2 * d + pitch) / (2 * pitch)
    } else {
        -((pitch - 2 * d) / (2 * pitch))
    };
    ((d - index * pitch).abs() <= tol).then_some(index)
}

/// The pins of `component` in its part frame, by item id.
fn frame_pins(board: &BasicBoard, component: ComponentId, frame: &PartFrame) -> Vec<FramePin> {
    let mut pins: Vec<FramePin> = board
        .items()
        .filter(|i| i.is_pin() && i.component_id() == component.0)
        .filter_map(|item| {
            let drill = item.drill()?;
            Some(FramePin {
                id: item.id(),
                centre: frame.to_part(drill.center),
                pad: frame.box_to_part(&drill.bounding_box()),
            })
        })
        .collect();
    pins.sort_by_key(|p| p.id);
    pins
}

/// The single pad layer of `component`'s pins, if every pin is SMD on it.
fn pad_layer(board: &BasicBoard, pins: &[FramePin]) -> Option<i32> {
    let mut layer = None;
    for p in pins {
        let item = board.item(p.id)?;
        if item.first_layer() != item.last_layer() {
            return None;
        }
        match layer {
            None => layer = Some(item.first_layer()),
            Some(l) if l != item.first_layer() => return None,
            Some(_) => {}
        }
    }
    layer
}

/// True if fewer than half the cells have an orthogonal neighbour.
fn is_staggered(cells: &BTreeMap<(i64, i64), ItemId>) -> bool {
    let with_neighbour = cells
        .keys()
        .filter(|&&(r, c)| {
            [(r - 1, c), (r + 1, c), (r, c - 1), (r, c + 1)]
                .iter()
                .any(|n| cells.contains_key(n))
        })
        .count();
    2 * with_neighbour < cells.len()
}

/// The ball array of `component`, or why it is none (module docs).
pub fn analyze_array(board: &BasicBoard, component: ComponentId) -> Result<BallArray, Rejection> {
    let frame = PartFrame::of_component(board, component).ok_or(Rejection::TooFewPins)?;
    let pins = frame_pins(board, component, &frame);
    if pins.len() < 4 {
        return Err(Rejection::TooFewPins);
    }
    let layer = pad_layer(board, &pins).ok_or(Rejection::NotSmd)?;
    let tol = i64::from(um_to_board_units(board, CLUSTER_TOLERANCE_UM)).max(1);
    let rounding = i64::from(um_to_board_units(board, PITCH_ROUNDING_UM)).max(1);
    let xs: Vec<i64> = pins.iter().map(|p| i64::from(p.centre.x)).collect();
    let ys: Vec<i64> = pins.iter().map(|p| i64::from(p.centre.y)).collect();
    let (cx, cy) = (cluster(&xs, tol), cluster(&ys, tol));
    let px = dominant_pitch(&cx, rounding, tol).ok_or(Rejection::NoLattice)?;
    let py = dominant_pitch(&cy, rounding, tol).ok_or(Rejection::NoLattice)?;
    let (x0, y0) = (cx[0], cy[cy.len() - 1]);
    let mut cells: BTreeMap<(i64, i64), ItemId> = BTreeMap::new();
    for p in &pins {
        let col = lattice_index(i64::from(p.centre.x), x0, px, tol).ok_or(Rejection::OffGrid)?;
        let row = lattice_index(y0, i64::from(p.centre.y), py, tol).ok_or(Rejection::OffGrid)?;
        if cells.insert((row, col), p.id).is_some() {
            return Err(Rejection::OffGrid);
        }
    }
    if is_staggered(&cells) {
        return Err(Rejection::Staggered);
    }
    if (px - py).abs() > tol {
        return Err(Rejection::NotSquare);
    }
    let rows = cells.keys().map(|k| k.0).max().unwrap_or(0) + 1;
    let cols = cells.keys().map(|k| k.1).max().unwrap_or(0) + 1;
    let (Ok(rows), Ok(cols)) = (u16::try_from(rows), u16::try_from(cols)) else {
        return Err(Rejection::TooSmall);
    };
    if rows < MIN_GRID || cols < MIN_GRID {
        return Err(Rejection::TooSmall);
    }
    let mut populated = vec![false; usize::from(rows) * usize::from(cols)];
    let mut ball_of = BTreeMap::new();
    for (&(r, c), &id) in &cells {
        let (r, c) = (r as u16, c as u16);
        populated[usize::from(r) * usize::from(cols) + usize::from(c)] = true;
        ball_of.insert((r, c), id);
    }
    let c = board
        .components
        .get(component)
        .ok_or(Rejection::TooFewPins)?;
    let field = pins[1..]
        .iter()
        .fold(pins[0].pad, |acc, p| acc.union_box(&p.pad));
    let outline = board
        .library
        .packages
        .get(c.package())
        .and_then(|p| p.outline.as_ref())
        .and_then(|shapes| {
            shapes
                .iter()
                .flatten()
                .map(|s| s.bounding_box())
                .reduce(|a, b| a.union_box(&b))
        });
    let array = BallArray {
        component,
        name: c.name.clone(),
        frame,
        rows,
        cols,
        pitch: (px + py) / 2,
        origin: IntPoint::new(clamp_i32(x0), clamp_i32(y0)),
        populated,
        ball_of,
        pad_layer: layer,
        field,
        outline,
    };
    for ring in 0..2 {
        let positions = array.ring_positions(ring);
        let balls = positions
            .iter()
            .filter(|&&(r, c)| array.is_populated(r, c))
            .count();
        if 2 * balls < positions.len() {
            return Err(Rejection::Sparse);
        }
    }
    Ok(array)
}

/// Every component of `board` by name, with its array or why it is none.
pub fn arrays_of(board: &BasicBoard) -> Vec<(ComponentId, Result<BallArray, Rejection>)> {
    let mut ids: Vec<(String, ComponentId)> = board
        .components
        .iter()
        .map(|c| (c.name.clone(), c.id))
        .collect();
    ids.sort();
    ids.into_iter()
        .map(|(_, id)| (id, analyze_array(board, id)))
        .collect()
}
