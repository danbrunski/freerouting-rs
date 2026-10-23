//! K15.41, K15.47: the pieces of a pour's opened fill and their islands,
//! made from what the area's last fill left ([`PieceCache`]).
//!
//! A fill's pieces are a function of its trapezoids, so a trapezoid equal
//! to one of an earlier fill takes that fill's piece and the shape tests
//! between such pieces (K15.41). K15.47 keeps the cache from fill to fill
//! and updates it in place, and starts the islands from the last fill's
//! joins ([`island_numbers_seeded`]): on kit-dev-coldfire-xilinx_5213's
//! route with the pour pair 2 023 fills of about 24 000 pieces each, 3 % of
//! them new, where the island numbering took 16.1 s and the pieces' map
//! 9.4 s; 4.3 s and 4.7 s after. The islands and their numbers are those of
//! [`island_numbers_with`](crate::quality::pour_islands::island_numbers_with)
//! without seeds (checked on every fill in a debug build).

use std::sync::{Arc, OnceLock};

use freerouting_geometry::{IntBox, PolylineShapeOps, Shape, TileShapeOps};

use super::{FillGeometry, FillPiece, PourFillWork};
use crate::drc::Trapezoid;
use crate::quality::pour_fill::trapezoid_tile;
use crate::quality::pour_islands::{Seeds, island_numbers_seeded};

/// K15.41: per area the pieces of its fills by the bits of their
/// trapezoid, and the shape tests between them (by the pieces' ids, in the
/// order asked), for the next fill: a piece is a function of its trapezoid,
/// the radius and the layer, and a test of its two pieces. K15.47: kept
/// and updated in place from fill to fill (pruned to the last fill's pieces
/// when it grows), with the joins of the last fill.
#[derive(Clone, Debug, Default)]
pub(super) struct PieceCache {
    radius: u64,
    layer: i32,
    pieces: std::collections::HashMap<[u64; 6], PieceSlot, FastHash>,
    next_id: u64,
    tests: std::collections::HashMap<(u64, u64), bool, FastHash>,
    /// The fills made with this cache; a slot whose `fill` is this was in
    /// the last one.
    fill: u32,
    /// The joins of the last fill (pairs of its pieces' indices whose
    /// shapes meet, spanning its islands: [`island_numbers_seeded`]).
    unions: Vec<(u32, u32)>,
    /// The work of the fill that built this cache.
    pub(super) work: PourFillWork,
}

/// K15.47: the hasher of [`PieceCache`]'s maps, whose keys are the bits of
/// coordinates and piece ids: a multiply-rotate mix of 8-byte words, fixed
/// in every process ([`crate::util::StableHasher`]'s rule) and several
/// times faster than its SipHash on 48-byte keys (the maps are looked up
/// once per piece of every fill). No result depends on the maps' order.
type FastHash = std::hash::BuildHasherDefault<WordHasher>;

#[derive(Default)]
struct WordHasher(u64);

impl WordHasher {
    fn mix(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

impl std::hash::Hasher for WordHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for chunk in &mut chunks {
            let mut word = [0u8; 8];
            word.copy_from_slice(chunk);
            self.mix(u64::from_le_bytes(word));
        }
        let rest = chunks.remainder();
        if !rest.is_empty() {
            let mut word = [0u8; 8];
            word[..rest.len()].copy_from_slice(rest);
            self.mix(u64::from_le_bytes(word));
        }
    }

    fn write_u64(&mut self, word: u64) {
        self.mix(word);
    }

    fn write_usize(&mut self, word: usize) {
        self.mix(word as u64);
    }
}

/// K15.47: a trapezoid's piece (`None`: no tile), the last fill it was in
/// and its index there.
#[derive(Clone, Debug)]
struct PieceSlot {
    piece: Option<Arc<FillPiece>>,
    fill: u32,
    index: usize,
}

impl PieceCache {
    /// Drops what the last fill (of `trapezoids` trapezoids and `pieces`)
    /// did not use, once the maps hold much more than it.
    fn prune(&mut self, trapezoids: usize, pieces: &[Arc<FillPiece>]) {
        if self.pieces.len() > 2 * trapezoids + 1024 {
            let now = self.fill;
            self.pieces.retain(|_, slot| slot.fill == now);
        }
        if self.tests.len() > 4 * pieces.len() + 4096 {
            let live: std::collections::HashSet<u64, FastHash> =
                pieces.iter().map(|p| p.id).collect();
            self.tests
                .retain(|(a, b), _| live.contains(a) && live.contains(b));
        }
    }
}

/// The opened fill grown from the core tiles of `trapezoids` by `radius` on
/// `layer` (module docs), `None` for a core with no tile, with the pieces,
/// shape tests and joins for the next fill. K15.41: a piece and a test that
/// `cache` holds are taken from it. K15.47: `cache` is updated in place,
/// and the islands start from the joins of `last`, the fill `cache` was
/// last updated for ([`island_numbers_seeded`]).
pub(super) fn fill_of_trapezoids(
    trapezoids: &[Trapezoid],
    radius: f64,
    layer: i32,
    cache: PieceCache,
    last: Option<&FillGeometry>,
) -> (Option<Arc<FillGeometry>>, PieceCache) {
    let reuse = cache.radius == radius.to_bits() && cache.layer == layer;
    let mut next = if reuse {
        cache
    } else {
        PieceCache {
            radius: radius.to_bits(),
            layer,
            ..PieceCache::default()
        }
    };
    next.work = PourFillWork::default();
    let last = last.filter(|_| reuse && next.fill > 0);
    let (seen, now) = (next.fill, next.fill + 1);
    next.fill = now;
    let mut pieces: Vec<Arc<FillPiece>> = Vec::with_capacity(trapezoids.len());
    // Per piece its index in the last fill, if it was in it.
    let mut was: Vec<Option<usize>> = Vec::with_capacity(trapezoids.len());
    for t in trapezoids {
        let key = [
            t.x_left, t.x_right, t.left[0], t.left[1], t.right[0], t.right[1],
        ]
        .map(f64::to_bits);
        let index = pieces.len();
        match next.pieces.get_mut(&key) {
            Some(slot) => {
                next.work.pieces_reused += usize::from(slot.piece.is_some());
                if let Some(piece) = &slot.piece {
                    // A trapezoid twice in one fill is new the second time.
                    let earlier = slot.fill == seen && last.is_some();
                    was.push(earlier.then_some(slot.index));
                    pieces.push(piece.clone());
                    slot.index = index;
                }
                slot.fill = now;
            }
            None => {
                let piece = trapezoid_tile(t).map(|core| {
                    let grown = if radius > 0.0 {
                        core.enlarge(radius)
                    } else {
                        core.clone()
                    };
                    let shape = Shape::from(grown.clone());
                    next.next_id += 1;
                    next.work.pieces_built += 1;
                    Arc::new(FillPiece {
                        id: next.next_id,
                        bounds: grown.bounding_box(),
                        shape_bounds: shape.bounding_box(),
                        area: grown.area(),
                        core,
                        grown,
                        shape,
                    })
                });
                if let Some(piece) = &piece {
                    was.push(None);
                    pieces.push(piece.clone());
                }
                next.pieces.insert(
                    key,
                    PieceSlot {
                        piece,
                        fill: now,
                        index,
                    },
                );
            }
        }
    }
    next.prune(trapezoids.len(), &pieces);
    if pieces.is_empty() {
        next.unions.clear();
        return (None, next);
    }
    let seeds = last.map(|last| seeds_from(last, &was, &next.unions));
    next.work.tests_reused += seeds.as_ref().map_or(0, |s| s.unions.len());
    let boxes: Vec<IntBox> = pieces.iter().map(|p| p.shape_bounds).collect();
    let layers = vec![layer; pieces.len()];
    let (work, tests) = (&mut next.work, &mut next.tests);
    let found = island_numbers_seeded(&boxes, &layers, seeds.as_ref(), |i, j| {
        let key = (pieces[i].id, pieces[j].id);
        if let Some(known) = tests.get(&key) {
            work.tests_reused += 1;
            return *known;
        }
        work.tests_run += 1;
        let meets = pieces[i].shape.intersects(&pieces[j].shape);
        tests.insert(key, meets);
        meets
    });
    #[cfg(debug_assertions)]
    if seeds.is_some() {
        let (scratch, _) =
            crate::quality::pour_islands::island_numbers_with(&boxes, &layers, |i, j| {
                pieces[i].shape.intersects(&pieces[j].shape)
            });
        debug_assert_eq!(found.numbers, scratch, "seeded islands of a fill");
    }
    next.unions = found
        .unions
        .iter()
        .map(|&(i, j)| (i as u32, j as u32))
        .collect();
    let island = found.numbers;
    let count = island.iter().copied().max().map_or(0, |m| m + 1);
    let mut size = vec![0.0f64; count];
    for (piece, k) in pieces.iter().zip(&island) {
        size[*k] += piece.area;
    }
    let fill = FillGeometry {
        pieces,
        island,
        size,
        radius,
        grid: OnceLock::new(),
    };
    (Some(Arc::new(fill)), next)
}

/// K15.47: the [`Seeds`] of a fill from `last`, the fill before it: `was`
/// gives per piece now its index in `last`, and `unions` are `last`'s joins
/// (indices in `last`).
fn seeds_from(last: &FillGeometry, was: &[Option<usize>], unions: &[(u32, u32)]) -> Seeds {
    const GONE: usize = usize::MAX;
    let mut now_of = vec![GONE; last.pieces.len()];
    for (i, w) in was.iter().enumerate() {
        if let Some(w) = w
            && let Some(slot) = now_of.get_mut(*w)
        {
            *slot = i;
        }
    }
    let mut lost = vec![false; last.size.len()];
    for (w, now) in now_of.iter().enumerate() {
        if *now == GONE
            && let Some(k) = last.island.get(w)
        {
            lost[*k] = true;
        }
    }
    let earlier = was
        .iter()
        .map(|w| w.and_then(|w| last.island.get(w).copied()))
        .collect();
    let unions = unions
        .iter()
        .filter_map(|&(a, b)| {
            let (a, b) = (*now_of.get(a as usize)?, *now_of.get(b as usize)?);
            (a != GONE && b != GONE).then_some((a, b))
        })
        .collect();
    Seeds {
        unions,
        earlier,
        lost,
    }
}
