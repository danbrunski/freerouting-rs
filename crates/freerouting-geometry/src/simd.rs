//! Box and octagon overlap tests on bounds stored as lanes (P05).
//!
//! A search tree tests one query shape against many node bounds. [`BoundLanes`] stores a
//! bound as two arrays of four `i32`: the lower and the upper limit in each of the four
//! directions of an octagon (x, y, and the two diagonals). Laid out like this, one
//! overlap test is the same comparison in four lanes, which the compiler turns into a
//! few vector instructions (SSE2 on x86_64, NEON on aarch64) instead of four
//! data-dependent branches.
//!
//! The tests are exact integer comparisons, so they give the same answer as the scalar
//! predicates they replace: [`BoundLanes::octagons_intersect`] is
//! [`IntOctagon::intersects_octagon`] and [`BoundLanes::boxes_intersect`] is
//! [`IntBox::intersects_box`], for every input including the `i32` limits
//! (`tests/simd_lanes.rs` compares them on random and edge inputs).

use crate::shapes::{IntBox, IntOctagon, RegularTileShape};

/// A box or octagon bound as lanes: `lo[i] <= hi[i]` bound the shape in direction `i`.
///
/// For an octagon the directions are x (`left_x`, `right_x`), y (`bottom_y`, `top_y`),
/// the +45 degree diagonal (`lower_left_diagonal_x`, `upper_right_diagonal_x`) and the
/// -45 degree diagonal (`upper_left_diagonal_x`, `lower_right_diagonal_x`). A box fills
/// the x and y lanes and repeats them in the two diagonal lanes, so its test is the same
/// four-lane loop (the repeated lanes ask the same question twice).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(C, align(32))]
pub struct BoundLanes {
    /// The lower limits.
    pub lo: [i32; 4],
    /// The upper limits.
    pub hi: [i32; 4],
}

impl BoundLanes {
    /// The lanes of an octagon.
    #[inline]
    pub const fn from_octagon(o: &IntOctagon) -> BoundLanes {
        BoundLanes {
            lo: [
                o.left_x,
                o.bottom_y,
                o.lower_left_diagonal_x,
                o.upper_left_diagonal_x,
            ],
            hi: [
                o.right_x,
                o.top_y,
                o.upper_right_diagonal_x,
                o.lower_right_diagonal_x,
            ],
        }
    }

    /// The lanes of a box (x and y, repeated in the diagonal lanes).
    #[inline]
    pub const fn from_box(b: &IntBox) -> BoundLanes {
        BoundLanes {
            lo: [b.ll.x, b.ll.y, b.ll.x, b.ll.y],
            hi: [b.ur.x, b.ur.y, b.ur.x, b.ur.y],
        }
    }

    /// The lanes of a regular tile shape.
    #[inline]
    pub const fn from_regular(shape: &RegularTileShape) -> BoundLanes {
        match shape {
            RegularTileShape::IntBox(b) => BoundLanes::from_box(b),
            RegularTileShape::IntOctagon(o) => BoundLanes::from_octagon(o),
        }
    }

    /// [`IntOctagon::intersects_octagon`] of the two octagons: in every lane the larger
    /// lower limit is at most the smaller upper limit, the same comparisons in the same
    /// lanes, evaluated without branches so the loop vectorizes (on SSE2 one
    /// `pcmpgtd` and a blend per `max` and `min`, then one compare and `movmskps`).
    #[inline(always)]
    pub fn octagons_intersect(&self, q: &BoundLanes) -> bool {
        let mut ok = true;
        for i in 0..4 {
            ok &= self.lo[i].max(q.lo[i]) <= self.hi[i].min(q.hi[i]);
        }
        ok
    }

    /// [`IntBox::intersects_box`] of the two boxes: each lower limit is at most the other
    /// box's upper limit. Unlike the octagon test this does not ask whether a box is
    /// empty, as Java's `IntBox.intersects(IntBox)` does not.
    #[inline(always)]
    pub fn boxes_intersect(&self, q: &BoundLanes) -> bool {
        let mut miss = false;
        for i in 0..4 {
            miss |= (self.lo[i] > q.hi[i]) | (q.lo[i] > self.hi[i]);
        }
        !miss
    }
}

/// The bounds of two shapes as lanes, side by side: lanes 0 to 3 are the first
/// shape's [`BoundLanes`], lanes 4 to 7 the second's. A search tree stores its inner
/// nodes' two children like this, so one test answers for both (P05.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(C, align(64))]
pub struct PairLanes {
    /// The lower limits.
    pub lo: [i32; 8],
    /// The upper limits.
    pub hi: [i32; 8],
}

impl PairLanes {
    /// The pair of `first` and `second`.
    #[inline]
    pub fn new(first: &BoundLanes, second: &BoundLanes) -> PairLanes {
        let mut pair = PairLanes {
            lo: [0; 8],
            hi: [0; 8],
        };
        pair.set(0, first);
        pair.set(1, second);
        pair
    }

    /// The pair of `shape` with itself: a query against a pair.
    #[inline]
    pub fn splat(shape: &BoundLanes) -> PairLanes {
        PairLanes::new(shape, shape)
    }

    /// Replaces shape `slot` (0 or 1).
    #[inline]
    pub fn set(&mut self, slot: usize, shape: &BoundLanes) {
        self.lo[4 * slot..4 * slot + 4].copy_from_slice(&shape.lo);
        self.hi[4 * slot..4 * slot + 4].copy_from_slice(&shape.hi);
    }

    /// Shape `slot` (0 or 1).
    #[inline]
    pub fn get(&self, slot: usize) -> BoundLanes {
        let mut shape = BoundLanes {
            lo: [0; 4],
            hi: [0; 4],
        };
        shape.lo.copy_from_slice(&self.lo[4 * slot..4 * slot + 4]);
        shape.hi.copy_from_slice(&self.hi[4 * slot..4 * slot + 4]);
        shape
    }

    /// [`BoundLanes::octagons_intersect`] of both shapes with the query `q` (a
    /// [`splat`](Self::splat)): bit 0 for the first shape, bit 1 for the second.
    #[inline(always)]
    pub fn octagons_intersect(&self, q: &PairLanes) -> u32 {
        let mut ok = [false; 8];
        // Four arrays indexed in step: an index loop is what vectorizes
        // (with `std::array::from_fn` the overlap kernel ran 21 % more instructions).
        #[allow(clippy::needless_range_loop)]
        for i in 0..8 {
            ok[i] = self.lo[i].max(q.lo[i]) <= self.hi[i].min(q.hi[i]);
        }
        u32::from(ok[0] & ok[1] & ok[2] & ok[3]) | (u32::from(ok[4] & ok[5] & ok[6] & ok[7]) << 1)
    }

    /// [`BoundLanes::boxes_intersect`] of both shapes with the query `q` (a
    /// [`splat`](Self::splat)): bit 0 for the first shape, bit 1 for the second.
    #[inline(always)]
    pub fn boxes_intersect(&self, q: &PairLanes) -> u32 {
        let mut ok = [false; 8];
        #[allow(clippy::needless_range_loop)]
        for i in 0..8 {
            ok[i] = (self.lo[i] <= q.hi[i]) & (q.lo[i] <= self.hi[i]);
        }
        u32::from(ok[0] & ok[1] & ok[2] & ok[3]) | (u32::from(ok[4] & ok[5] & ok[6] & ok[7]) << 1)
    }
}
