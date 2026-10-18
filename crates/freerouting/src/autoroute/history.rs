//! The boards a routing run can go back to.
//!
//! Port of `autoroute/BoardHistory.java` at `cda540f`. The batch loop adds
//! the board before every pass; when the passes stop improving, it restores
//! the best board it has not restored too often, and at the end it restores
//! the best board when that beats the final one.
//!
//! [`BoardHistory`] is generic over what it stores. The board side is a
//! [`HistorySource`]: it gives the board's state hash and router score and
//! makes the stored [`HistoryEntry`], which restores a board. The pipeline
//! instantiates it with B01's board snapshot (A13.1); the tests use a fake.
//! Java stores the serialized board, its `getHash` string and its router
//! score as a `float`; the port stores the entry, a `u128` state hash and
//! the score as an `f32`, computed once when the board is added.
//!
//! The Java class is thread-safe (synchronized methods and a read-write
//! lock); the port takes `&mut self` instead. `BoardHistoryEntry.java` (a
//! top-level class nobody uses) is not ported.

use std::cmp::Ordering;

/// The default bound on the number of boards (Java `MAX_HISTORY_SIZE`).
pub const MAX_HISTORY_SIZE: usize = 30;

/// A stored board that can be restored.
pub trait HistoryEntry {
    /// The board a restore builds.
    type Board;

    /// The state hash of the stored board (Java `BoardSnapshotManager.getHash`).
    fn state_hash(&self) -> u128;

    /// A new board equal to the stored one (Java `BasicBoard.deserialize`).
    fn restore(&self) -> Self::Board;
}

/// A board that can be added to a [`BoardHistory`].
pub trait HistorySource {
    /// What the history stores for this board.
    type Entry: HistoryEntry;

    /// The board's state hash; equal to the entry's
    /// [`HistoryEntry::state_hash`].
    fn state_hash(&self) -> u128;

    /// The board's router score (Java
    /// `new BoardStatistics(board).getRouterScore(routerSettings)`).
    fn router_score(&self) -> f32;

    /// The entry to store (Java `board.serialize(false)`).
    fn to_entry(&self) -> Self::Entry;
}

/// One stored board (Java's private `BoardHistory.BoardHistoryEntry`).
#[derive(Clone, Debug)]
struct Stored<S> {
    entry: S,
    hash: u128,
    score: f32,
    restore_count: i32,
}

/// The bounded list of boards seen during a routing run (Java
/// `BoardHistory`).
#[derive(Clone, Debug)]
pub struct BoardHistory<S> {
    max_size: usize,
    boards: Vec<Stored<S>>,
}

impl<S> Default for BoardHistory<S> {
    fn default() -> Self {
        BoardHistory::new()
    }
}

impl<S> BoardHistory<S> {
    /// An empty history bounded by [`MAX_HISTORY_SIZE`].
    pub fn new() -> Self {
        BoardHistory::with_max_size(MAX_HISTORY_SIZE)
    }

    /// An empty history bounded by `max_size` boards (Java's package-private
    /// test constructor). A bound of 0 behaves like Java's: the first `add`
    /// reads the worst entry of an empty list, which the port reports as a
    /// panic.
    pub fn with_max_size(max_size: usize) -> Self {
        BoardHistory {
            max_size,
            boards: Vec::new(),
        }
    }

    /// The bound on the number of boards.
    pub fn max_size(&self) -> usize {
        self.max_size
    }

    /// Adds `board` unless a board with the same hash is stored (Java `add`,
    /// `BoardHistory.java:60-94`).
    ///
    /// When the history is full, the board's score is computed first and
    /// compared with the worst stored score, found by a linear scan that
    /// keeps the first of equal worst scores. A board that does not score
    /// strictly better is dropped without making an entry; otherwise the
    /// worst entry is removed and the board appended. Returns whether the
    /// board was added.
    pub fn add<B>(&mut self, board: &B) -> bool
    where
        B: HistorySource<Entry = S>,
    {
        let hash = board.state_hash();
        if self.contains(hash) {
            return false;
        }
        let score = board.router_score();
        if self.boards.len() >= self.max_size {
            // PARITY: Java scans with `<` from index 0, so the first of equal
            // worst scores is evicted, and drops the newcomer on `<=`.
            let mut worst_index = 0;
            let mut worst_score = self.boards[0].score;
            for (index, stored) in self.boards.iter().enumerate().skip(1) {
                if stored.score < worst_score {
                    worst_score = stored.score;
                    worst_index = index;
                }
            }
            if score <= worst_score {
                return false;
            }
            self.boards.remove(worst_index);
        }
        self.boards.push(Stored {
            entry: board.to_entry(),
            hash,
            score,
            restore_count: 0,
        });
        true
    }

    /// Removes every board (Java `clear`).
    pub fn clear(&mut self) {
        self.boards.clear();
    }

    /// True if a board with this state hash is stored (Java `contains`).
    pub fn contains(&self, hash: u128) -> bool {
        self.boards.iter().any(|stored| stored.hash == hash)
    }

    /// Removes the first board with this state hash (Java `remove`).
    pub fn remove(&mut self, hash: u128) {
        if let Some(index) = self.boards.iter().position(|stored| stored.hash == hash) {
            self.boards.remove(index);
        }
    }

    /// The highest stored score, but at least 0 (Java `getMaxScore`,
    /// `:131-145`): the maximum starts at 0, so a history of negative scores
    /// reports 0.
    pub fn max_score(&self) -> f32 {
        let mut max_score = 0.0f32;
        for stored in &self.boards {
            if stored.score > max_score {
                max_score = stored.score;
            }
        }
        max_score
    }

    /// The number of stored boards (Java `size`).
    pub fn len(&self) -> usize {
        self.boards.len()
    }

    /// True if no board is stored.
    pub fn is_empty(&self) -> bool {
        self.boards.is_empty()
    }

    /// The 1-based position of the board with this hash, `None` if it is not
    /// stored (Java `getRank`, -1). The list is in insertion order until a
    /// restore sorts it by score.
    pub fn rank(&self, hash: u128) -> Option<usize> {
        self.boards
            .iter()
            .position(|stored| stored.hash == hash)
            .map(|index| index + 1)
    }

    /// The stored scores in list order.
    pub fn scores(&self) -> Vec<f32> {
        self.boards.iter().map(|stored| stored.score).collect()
    }

    /// How often each stored board was restored, in list order.
    pub fn restore_counts(&self) -> Vec<i32> {
        self.boards
            .iter()
            .map(|stored| stored.restore_count)
            .collect()
    }
}

impl<S: HistoryEntry> BoardHistory<S> {
    /// Restores the best board restored at most `max_restore_count` times
    /// before (Java `restoreBoard`, `:151-178`); a count `<= 0` means no
    /// limit.
    ///
    /// The list is sorted by score, best first, with a stable sort that
    /// compares like Java's `Float.compare` ([`java_float_compare`]). The
    /// first board whose restore count is `<= max_restore_count` has its
    /// count incremented and is restored. `None` when there is none.
    pub fn restore(&mut self, max_restore_count: i32) -> Option<S::Board> {
        let max_restore_count = if max_restore_count <= 0 {
            i32::MAX
        } else {
            max_restore_count
        };
        // PARITY: `boards.sort((o1, o2) -> Float.compare(o2.score, o1.score))`
        // is a stable sort; `sort_by` is stable too.
        self.boards
            .sort_by(|a, b| java_float_compare(b.score, a.score));
        let stored = self
            .boards
            .iter_mut()
            .find(|stored| stored.restore_count <= max_restore_count)?;
        stored.restore_count += 1;
        Some(stored.entry.restore())
    }

    /// Restores the best board (Java `restoreBestBoard`, `restore(0)`).
    pub fn restore_best(&mut self) -> Option<S::Board> {
        self.restore(0)
    }

    /// The stored entries with their scores, best first in the order
    /// [`restore`](Self::restore) sorts them (a stable sort by Java's
    /// `Float.compare`), without changing the list or a restore count. Not
    /// Java: Q04.3's output guard reads the history this way.
    pub fn best_first(&self) -> Vec<(&S, f32)> {
        let mut stored: Vec<&Stored<S>> = self.boards.iter().collect();
        stored.sort_by(|a, b| java_float_compare(b.score, a.score));
        stored
            .into_iter()
            .map(|stored| (&stored.entry, stored.score))
            .collect()
    }
}

/// Java's `Float.compare(a, b)` as an [`Ordering`].
///
/// `-0.0` is less than `0.0`, and every NaN, whatever its sign and payload,
/// is equal to every other NaN and greater than positive infinity
/// (`Float.compare` compares `floatToIntBits`, which maps all NaNs to
/// `0x7fc00000`). [`f32::total_cmp`] agrees except for NaNs, which it orders
/// by sign and payload, so NaNs are made canonical first.
pub fn java_float_compare(a: f32, b: f32) -> Ordering {
    let canonical = |x: f32| if x.is_nan() { f32::NAN } else { x };
    canonical(a).total_cmp(&canonical(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Float.compare` at `cda540f` for every pair of these values, row by
    /// row, from a probe run with `tools/java-reference/probe.sh`.
    const JAVA_VALUES: [u32; 9] = [
        0x8000_0000, // -0.0
        0x0000_0000, // 0.0
        0x7fc0_0000, // NaN
        0xffc0_0000, // NaN with the sign bit
        0x7f80_0001, // signalling NaN payload
        0x7f80_0000, // +inf
        0xff80_0000, // -inf
        0x3fc0_0000, // 1.5
        0xbfc0_0000, // -1.5
    ];
    const JAVA_COMPARE: [i32; 81] = [
        0, -1, -1, -1, -1, -1, 1, -1, 1, 1, 0, -1, -1, -1, -1, 1, -1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1,
        1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, -1, -1, -1, 0, 1, 1, 1, -1, -1,
        -1, -1, -1, -1, 0, -1, -1, 1, 1, -1, -1, -1, -1, 1, 0, 1, -1, -1, -1, -1, -1, -1, 1, -1, 0,
    ];

    #[test]
    fn java_float_compare_matches_java() {
        for (i, &a) in JAVA_VALUES.iter().enumerate() {
            for (j, &b) in JAVA_VALUES.iter().enumerate() {
                let got = java_float_compare(f32::from_bits(a), f32::from_bits(b)) as i32;
                assert_eq!(
                    got,
                    JAVA_COMPARE[i * 9 + j],
                    "Float.compare({a:#x}, {b:#x})"
                );
            }
        }
    }

    #[test]
    fn total_cmp_agrees_with_java_for_signed_zero_and_positive_nan() {
        // The checklist's check: `total_cmp` alone already gives Java's
        // order for -0.0 and the canonical NaN, but not for a NaN with the
        // sign bit, which x86 arithmetic produces (0.0 / 0.0).
        assert_eq!((-0.0f32).total_cmp(&0.0), Ordering::Less);
        assert_eq!(java_float_compare(-0.0, 0.0), Ordering::Less);
        assert_eq!(f32::NAN.total_cmp(&f32::INFINITY), Ordering::Greater);
        assert_eq!(
            java_float_compare(f32::NAN, f32::INFINITY),
            Ordering::Greater
        );
        let negative_nan = f32::from_bits(0xffc0_0000);
        assert_eq!(negative_nan.total_cmp(&f32::NEG_INFINITY), Ordering::Less);
        assert_eq!(
            java_float_compare(negative_nan, f32::INFINITY),
            Ordering::Greater
        );
    }
}
