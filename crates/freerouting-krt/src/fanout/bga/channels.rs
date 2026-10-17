//! Channel capacity and layer balancing of a ball array (X12.2).
//!
//! Written from the X12 checklist's design. KiCadRoutingTools'
//! `py_router/bga_fanout` (at `0486e799`) gave the ideas: routing channels
//! between ball rows and columns (`grid.calculate_channels`), the
//! half-pitch budget of an escape (#158 in its README) and spreading the
//! escapes evenly over the layers (`layer_balance.rebalance_layers`); no
//! KRT code is ported here.
//!
//! **Bands.** A ball escapes straight out through its side ([`super::array::BallArray::side`])
//! along the line of its row (sides W, E) or column (N, S): its *band*.
//! The dog-bone vias of the array ([`super::vias`]) sit on the band's two
//! edges, half a pitch to each side of the line, on every layer they span,
//! so a band holds `capacity(pitch, via, w, c)` traces per layer. Each
//! escape takes one *slot* of its band on one layer; the slots of a band
//! are fixed positions across it, numbered from the edge the band's vias
//! sit on (every ball of a band puts its via on the same edge), and the
//! outer rings take the slots nearest that edge. On the pad layer the
//! balls, dog-bones and vias fill the field, so only the outer ring, whose
//! vias lie outside the field, escapes there.
//!
//! **Capacity** (the X12 decision): `floor((pitch − obstacle − c) / (w + c))`
//! traces of width `w` and clearance `c` between two obstacles of width
//! `obstacle` a pitch apart, in integer units: `n` traces need
//! `n·w + (n + 1)·c` of the gap. Here the obstacle is the largest via of the
//! array on the layer.
//!
//! **Assignment** ([`assign`]): the demands in escape order (rings outside
//! in, each clockwise from its top-left); each goes to the allowed layer
//! with a free slot in its band and the most remaining capacity (free slots
//! over all its bands), ties by layer index in the part's own order (the
//! pad layer first). A ring's escapes therefore spread over the layers
//! that have room, the counts per layer differing by at most one unless a
//! band is full; a demand with no free slot gets none.
//!
//! Determinism (D2): integer arithmetic and ordered maps only.

use std::collections::BTreeMap;

use crate::fanout::frame::Side;

/// The traces of width `width` and clearance `clearance` that fit
/// between two obstacles of width `obstacle` whose centres are `pitch`
/// apart (module docs); 0 if none does.
pub fn capacity(pitch: i64, obstacle: i64, width: i64, clearance: i64) -> u32 {
    let room = pitch - obstacle - clearance;
    let per = width + clearance;
    if room <= 0 || per <= 0 {
        return 0;
    }
    u32::try_from(room / per).unwrap_or(u32::MAX)
}

/// One escape layer of an array, in the part's own layer order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EscapeLayer {
    /// The board layer.
    pub layer: i32,
    /// Its traces per band.
    pub capacity: u32,
    /// True for the balls' own layer (only the outer ring escapes there).
    pub pad_layer: bool,
}

/// The band of an escape: its side and the row (W, E) or column (N, S) it
/// runs along.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Band {
    /// The side.
    pub side: Side,
    /// The row or column index.
    pub line: u16,
}

/// One ball that needs an escape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Demand {
    /// Its ring (0 outermost).
    pub ring: u16,
    /// Its band.
    pub band: Band,
    /// The layers (bits by index into the [`EscapeLayer`] list) its net and
    /// via may use.
    pub allowed: u64,
}

/// Where an escape goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    /// The index into the [`EscapeLayer`] list.
    pub layer: usize,
    /// The slot in the band, from the via edge.
    pub slot: u32,
}

/// The slots of `demands` in order (module docs); `bands` is the number of
/// bands of a layer (twice the rows plus twice the columns).
pub fn assign(layers: &[EscapeLayer], bands: u32, demands: &[Demand]) -> Vec<Option<Slot>> {
    let mut used: BTreeMap<(Band, usize), u32> = BTreeMap::new();
    let mut remaining: Vec<u64> = layers
        .iter()
        .map(|l| u64::from(l.capacity) * u64::from(bands))
        .collect();
    demands
        .iter()
        .map(|d| {
            let best = layers
                .iter()
                .enumerate()
                .filter(|(i, l)| {
                    *i < 64
                        && d.allowed & (1u64 << i) != 0
                        && (!l.pad_layer || d.ring == 0)
                        && used.get(&(d.band, *i)).copied().unwrap_or(0) < l.capacity
                })
                .max_by(|a, b| remaining[a.0].cmp(&remaining[b.0]).then(b.0.cmp(&a.0)))
                .map(|(i, _)| i)?;
            let slot = used.entry((d.band, best)).or_insert(0);
            let out = Slot {
                layer: best,
                slot: *slot,
            };
            *slot += 1;
            remaining[best] = remaining[best].saturating_sub(1);
            Some(out)
        })
        .collect()
}

/// The offset of slot `slot` of `n` from the band's line, toward the via
/// edge `sigma` (±1) for the first slots, with slots `pitch` apart:
/// `σ·(n − 1 − 2·slot)·pitch / 2`, rounded toward zero (so a slot never
/// moves outward).
pub fn slot_offset(slot: u32, n: u32, pitch: i64, sigma: i64) -> i64 {
    let k = i64::from(n) - 1 - 2 * i64::from(slot);
    sigma * (k * pitch / 2)
}
