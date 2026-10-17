//! The batch loop's board history entries (A13.1): A08's [`BoardHistory`]
//! over B01's [`BoardSnapshot`].
//!
//! Java's `BoardHistory` stores `board.serialize(false)`, the board's
//! `getHash` string and its router score. Here a [`HistorySnapshot`] stores
//! the snapshot and the board's [`state_hash`](crate::board::BasicBoard::state_hash);
//! a restore is [`BoardSnapshot::restore`], never a `Clone`, so it brings
//! back what Java's serialized board holds (the items with their ids and
//! undo structure, the id generator, the failure log) and resets the
//! transient state as `BasicBoard.readObject` does.
//!
//! [`ScoredBoard`] is the [`HistorySource`]: the board with the statistics
//! the loop already computed for it, so adding it to the history computes
//! neither the statistics nor the score again (A13's "statistics once per
//! pass"). The score is Java's `float` router score of the same statistics
//! (`new BoardStatistics(board).getRouterScore(routerSettings)`), so the
//! history's comparisons are Java's.

use crate::autoroute::history::{BoardHistory, HistoryEntry, HistorySource};
use crate::board::BoardSnapshot;
use crate::board::routing_board::RoutingBoard;
use crate::scoring::BoardStatistics;
use crate::settings::RouterSettings;

/// The history of a routing run: B01 snapshots.
pub type SnapshotHistory = BoardHistory<HistorySnapshot>;

/// One stored board: B01's snapshot and the state hash of the board it was
/// taken from.
#[derive(Clone, Debug)]
pub struct HistorySnapshot {
    snapshot: BoardSnapshot,
    hash: u128,
}

impl HistorySnapshot {
    /// The snapshot of `board`, whose state hash is `hash`.
    pub fn new(board: &RoutingBoard, hash: u128) -> Self {
        HistorySnapshot {
            snapshot: board.snapshot(),
            hash,
        }
    }

    /// The stored snapshot.
    pub fn snapshot(&self) -> &BoardSnapshot {
        &self.snapshot
    }
}

impl HistoryEntry for HistorySnapshot {
    type Board = RoutingBoard;

    fn state_hash(&self) -> u128 {
        self.hash
    }

    /// B01's restore (Java `BasicBoard.deserialize`).
    fn restore(&self) -> RoutingBoard {
        self.snapshot.restore()
    }
}

/// What the loop knows about a board state: its statistics, computed once,
/// and the figures read from them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BoardFigures {
    /// The router score (`getRouterScore(routerSettings)`), in `f32`.
    pub score: f32,
    /// Incomplete connections (`connections.incompleteCount`).
    pub incomplete: usize,
    /// Clearance violations (`clearanceViolations.totalCount`).
    pub violations: usize,
}

impl BoardFigures {
    /// The figures of `board` (one `BoardStatistics`).
    pub fn of(board: &RoutingBoard, settings: &RouterSettings) -> Self {
        BoardFigures::from_statistics(&BoardStatistics::new(board), settings)
    }

    /// The figures read from `stats`.
    pub fn from_statistics(stats: &BoardStatistics, settings: &RouterSettings) -> Self {
        BoardFigures {
            score: stats.router_score(Some(settings)),
            incomplete: stats.connections.incomplete_count,
            violations: stats.clearance_violations.total_count,
        }
    }
}

/// A board to add to the history, with its hash and score already known.
#[derive(Clone, Copy, Debug)]
pub struct ScoredBoard<'a> {
    /// The board.
    pub board: &'a RoutingBoard,
    /// Its state hash.
    pub hash: u128,
    /// Its router score.
    pub score: f32,
}

impl HistorySource for ScoredBoard<'_> {
    type Entry = HistorySnapshot;

    fn state_hash(&self) -> u128 {
        self.hash
    }

    fn router_score(&self) -> f32 {
        self.score
    }

    fn to_entry(&self) -> HistorySnapshot {
        HistorySnapshot::new(self.board, self.hash)
    }
}
