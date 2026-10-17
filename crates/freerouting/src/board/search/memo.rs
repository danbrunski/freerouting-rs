//! The exact-key memo of trace tree shapes (P02.5).
//!
//! A trace's tree shapes are the offset shapes of its segments
//! (`Polyline::offset_shape`, or `offset_box` in a 90 degree tree), each a
//! pure function of the polyline's lines and the offset: the half width
//! plus the tree's clearance compensation for the trace's class on its
//! layer. A board computes them again for the same traces whenever it builds
//! a whole tree: a restore's default tree, each autoroute tree, a tree
//! rebuilt for clearance compensation. The optimizer does that once per
//! candidate (O06.4). Those builds take the shapes from the memo
//! (`ShapeSearchTree::insert_memoized`); single insertions and other shape
//! calculations compute them, because a new or candidate polyline is
//! mostly never seen again and would only crowd the memo (on
//! Issue219-LogicBoard_smt, memoizing every calculation evicted 85 % of
//! its misses).
//!
//! **Exact keys.** The key is the polyline's lines, compared point by point
//! as the integers they are (not by Java's geometric `Line.equals`), the
//! total offset and whether the tree stores boxes. Two keys are equal only
//! when the computation would see the same input, so a hit returns the
//! shapes a computation would return, bit for bit. Nothing is canonicalized:
//! a translated copy of a trace is another key, because the offset shapes
//! are computed in floating point and translation does not commute with
//! their rounding (D6).
//!
//! **Sharing.** A board's trees share its memo, and so do its snapshots and
//! every board restored from them (the optimizer's candidates, the batch
//! loop's history), across threads behind a mutex. The memo only stores
//! results of a pure function, so which thread fills it, and whether a
//! lookup hits, never changes a result (D2).
//!
//! **Size.** Two generations of at most [`ShapeMemo::GENERATION_CAPACITY`]
//! polylines each: lookups search both and move a hit from the old
//! generation to the current one; when the current one is full, the old one
//! is dropped (its entries count as evictions) and the current one becomes
//! the old one. [`MemoCounts`] reports hits, misses and evictions.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hash, Hasher};
use std::sync::{Arc, Mutex};

use freerouting_geometry::{Polyline, TileShape};

/// The tree shapes of one trace, in segment order.
pub(crate) type TraceShapes = Arc<[Option<TileShape>]>;

/// The lookups of a [`ShapeMemo`] so far.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MemoCounts {
    /// Lookups that found the shapes.
    pub hits: u64,
    /// Lookups that computed them.
    pub misses: u64,
    /// Entries dropped with an old generation.
    pub evictions: u64,
}

/// What the shapes are a function of.
struct Key {
    /// The hash of the fields below, computed outside the lock.
    hash: u64,
    /// Bounding boxes (a 90 degree tree) instead of offset shapes.
    boxes: bool,
    offset: i32,
    /// Shared with the trace; its lines never change.
    polyline: Polyline,
}

/// One step of the key hash (the multiply-rotate of rustc's `FxHasher`):
/// cheap, and fixed, so a memo probes the same way in every process.
fn mix(hash: u64, value: i32) -> u64 {
    (hash.rotate_left(5) ^ u64::from(value as u32)).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95)
}

impl Key {
    fn new(polyline: &Polyline, offset: i32, boxes: bool) -> Self {
        let mut hash = mix(u64::from(boxes), offset);
        for line in polyline.lines() {
            let (a, b) = (line.a(), line.b());
            hash = mix(mix(mix(mix(hash, a.x), a.y), b.x), b.y);
        }
        Key {
            hash,
            boxes,
            offset,
            polyline: polyline.clone(),
        }
    }
}

impl Hash for Key {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u64(self.hash);
    }
}

impl PartialEq for Key {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash
            && self.boxes == other.boxes
            && self.offset == other.offset
            && same_lines(&self.polyline, &other.polyline)
    }
}

impl Eq for Key {}

/// The lines of `a` and `b` have the same defining points, in order.
fn same_lines(a: &Polyline, b: &Polyline) -> bool {
    let (a, b) = (a.lines(), b.lines());
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| x.a() == y.a() && x.b() == y.b())
}

/// The hasher of the memo's maps: a [`Key`] hashes to its stored hash.
#[derive(Default)]
struct KeyHasher(u64);

impl Hasher for KeyHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        // Only `Key::hash` writes, through `write_u64`; fold anything else.
        for &byte in bytes {
            self.0 = mix(self.0, i32::from(byte));
        }
    }

    fn write_u64(&mut self, value: u64) {
        self.0 = value;
    }
}

type KeyMap = HashMap<Key, TraceShapes, BuildHasherDefault<KeyHasher>>;

#[derive(Default)]
struct Generations {
    current: KeyMap,
    old: KeyMap,
    counts: MemoCounts,
}

/// The memo of trace tree shapes (see the module docs).
#[derive(Default)]
pub struct ShapeMemo {
    inner: Mutex<Generations>,
}

impl std::fmt::Debug for ShapeMemo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShapeMemo")
            .field("counts", &self.counts())
            .finish()
    }
}

impl ShapeMemo {
    /// The polylines one generation holds.
    pub const GENERATION_CAPACITY: usize = 1 << 14;

    /// A new, empty memo.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Generations> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The lookups so far.
    pub fn counts(&self) -> MemoCounts {
        self.lock().counts
    }

    /// The shapes of `polyline` at `offset` (`boxes`: a 90 degree tree's),
    /// from the memo or from `compute`, which must be the pure function of
    /// these three that the memo stands for.
    pub(crate) fn trace_shapes(
        &self,
        polyline: &Polyline,
        offset: i32,
        boxes: bool,
        compute: impl FnOnce() -> Vec<Option<TileShape>>,
    ) -> Vec<Option<TileShape>> {
        let key = Key::new(polyline, offset, boxes);
        {
            let mut generations = self.lock();
            if let Some(shapes) = generations.current.get(&key) {
                let shapes = shapes.to_vec();
                generations.counts.hits += 1;
                return shapes;
            }
            if let Some((key, shapes)) = generations.old.remove_entry(&key) {
                generations.counts.hits += 1;
                let result = shapes.to_vec();
                generations.insert(key, shapes);
                return result;
            }
            generations.counts.misses += 1;
        }
        let shapes = compute();
        let stored: TraceShapes = shapes.as_slice().into();
        self.lock().insert(key, stored);
        shapes
    }
}

impl Generations {
    fn insert(&mut self, key: Key, shapes: TraceShapes) {
        if self.current.len() >= ShapeMemo::GENERATION_CAPACITY && !self.current.contains_key(&key)
        {
            self.counts.evictions += self.old.len() as u64;
            self.old = std::mem::take(&mut self.current);
        }
        self.current.insert(key, shapes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use freerouting_geometry::{IntPoint, Line, Point};

    fn polyline(points: &[(i32, i32)]) -> Polyline {
        let points: Vec<Point> = points
            .iter()
            .map(|&(x, y)| Point::Int(IntPoint::new(x, y)))
            .collect();
        Polyline::from_points(&points)
    }

    fn shapes(polyline: &Polyline, offset: i32) -> Vec<Option<TileShape>> {
        (0..polyline.lines().len() - 2)
            .map(|i| polyline.offset_shape(offset, i))
            .collect()
    }

    /// The shapes as text (`TileShape` has no `PartialEq`; its `Debug`
    /// prints every coordinate).
    fn text(shapes: &[Option<TileShape>]) -> String {
        format!("{shapes:?}")
    }

    #[test]
    fn a_hit_returns_the_computed_shapes() {
        let memo = ShapeMemo::new();
        let trace = polyline(&[(0, 0), (1000, 0), (1000, 700)]);
        let first = memo.trace_shapes(&trace, 50, false, || shapes(&trace, 50));
        // An equal polyline built separately is the same key.
        let copy = polyline(&[(0, 0), (1000, 0), (1000, 700)]);
        let second = memo.trace_shapes(&copy, 50, false, || unreachable!("a hit"));
        assert_eq!(text(&first), text(&second));
        assert_eq!(text(&second), text(&shapes(&copy, 50)));
        assert_eq!(
            memo.counts(),
            MemoCounts {
                hits: 1,
                misses: 1,
                evictions: 0
            }
        );
    }

    #[test]
    fn keys_are_exact() {
        let memo = ShapeMemo::new();
        let trace = polyline(&[(0, 0), (1000, 0), (1000, 700)]);
        memo.trace_shapes(&trace, 50, false, || shapes(&trace, 50));
        // Another offset, the box kind, a translated copy: all misses.
        memo.trace_shapes(&trace, 51, false, || shapes(&trace, 51));
        memo.trace_shapes(&trace, 50, true, || shapes(&trace, 50));
        let moved = polyline(&[(1, 0), (1001, 0), (1001, 700)]);
        let got = memo.trace_shapes(&moved, 50, false, || shapes(&moved, 50));
        assert_eq!(text(&got), text(&shapes(&moved, 50)));
        // The same line through other defining points is another key,
        // though Java's `Line.equals` calls the lines equal.
        let mut lines = trace.lines().to_vec();
        let (a, b) = (lines[1].a(), lines[1].b());
        lines[1] = Line::new(
            IntPoint::new(2 * a.x - b.x, 2 * a.y - b.y),
            IntPoint::new(2 * b.x - a.x, 2 * b.y - a.y),
        );
        let respelled = Polyline::new(&lines);
        assert_eq!(respelled.lines().len(), trace.lines().len());
        memo.trace_shapes(&respelled, 50, false, || shapes(&respelled, 50));
        assert_eq!(memo.counts().misses, 5);
        assert_eq!(memo.counts().hits, 0);
    }

    #[test]
    fn a_full_generation_ages_and_old_hits_move_back() {
        let memo = ShapeMemo::new();
        let n = ShapeMemo::GENERATION_CAPACITY as i32;
        let trace = |i: i32| polyline(&[(0, 0), (1000 + i, 0), (1000 + i, 700)]);
        for i in 0..n {
            let t = trace(i);
            memo.trace_shapes(&t, 50, false, || shapes(&t, 50));
        }
        assert_eq!(memo.counts().evictions, 0);
        // The next miss starts a generation; the first one is old now.
        let t = trace(n);
        memo.trace_shapes(&t, 50, false, || shapes(&t, 50));
        let first = trace(0);
        memo.trace_shapes(&first, 50, false, || unreachable!("an old hit"));
        assert_eq!(memo.counts().hits, 1);
        // Filling the second generation drops the rest of the first.
        for i in n + 1..2 * n {
            let t = trace(i);
            memo.trace_shapes(&t, 50, false, || shapes(&t, 50));
        }
        assert_eq!(memo.counts().evictions, n as u64 - 1);
        memo.trace_shapes(&first, 50, false, || unreachable!("moved back"));
    }
}
