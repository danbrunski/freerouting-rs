//! `BoardHistory` (A08.1) with a fake board: the eviction, restore and rank
//! rules of `autoroute/BoardHistory.java` at `cda540f` (unchanged at
//! `a773a70ec`).
//!
//! The last six tests port Java's `autoroute/BoardHistoryTest` (all six, on
//! `empty_board.dsn` and `Issue159-setonix_2hp-pcb.dsn`): they store B01's
//! `BoardSnapshot`, compare boards by their state hash (B01.5) and score
//! them with `BoardStatistics::router_score` on Java's default settings
//! (B06.2), as Java's `new BoardStatistics(board).getRouterScore(settings)`.
//! The history entry for the snapshot is a minimal one in this file; the
//! pipeline's own is A13.1's. Java loads through `HeadlessBoardManager`;
//! the port loads as the DRC does (`drc::load_design`), which only adds the
//! post-load processing, and the tests only compare a board with itself.

use std::cell::Cell;
use std::path::Path;
use std::sync::OnceLock;

use freerouting::autoroute::history::{
    BoardHistory, HistoryEntry, HistorySource, MAX_HISTORY_SIZE,
};
use freerouting::board::{BoardSnapshot, RoutingBoard};
use freerouting::drc::load_design;
use freerouting::scoring::BoardStatistics;
use freerouting::settings::{ResolveContext, RouterSettings, Source, default_patch, resolve};

/// A fake board: a hash and a score.
#[derive(Clone, Copy, Debug, PartialEq)]
struct FakeBoard {
    hash: u128,
    score: f32,
}

/// What the history stores for a [`FakeBoard`]; counts its restores.
#[derive(Debug)]
struct FakeEntry {
    board: FakeBoard,
}

impl HistoryEntry for FakeEntry {
    type Board = FakeBoard;

    fn state_hash(&self) -> u128 {
        self.board.hash
    }

    fn restore(&self) -> FakeBoard {
        self.board
    }
}

/// A source that counts how often the history asks for its score and its
/// entry.
struct Counting {
    board: FakeBoard,
    scores: Cell<u32>,
    entries: Cell<u32>,
}

impl Counting {
    fn new(hash: u128, score: f32) -> Self {
        Counting {
            board: FakeBoard { hash, score },
            scores: Cell::new(0),
            entries: Cell::new(0),
        }
    }
}

impl HistorySource for Counting {
    type Entry = FakeEntry;

    fn state_hash(&self) -> u128 {
        self.board.hash
    }

    fn router_score(&self) -> f32 {
        self.scores.set(self.scores.get() + 1);
        self.board.score
    }

    fn to_entry(&self) -> FakeEntry {
        self.entries.set(self.entries.get() + 1);
        FakeEntry { board: self.board }
    }
}

impl HistorySource for FakeBoard {
    type Entry = FakeEntry;

    fn state_hash(&self) -> u128 {
        self.hash
    }

    fn router_score(&self) -> f32 {
        self.score
    }

    fn to_entry(&self) -> FakeEntry {
        FakeEntry { board: *self }
    }
}

fn board(hash: u128, score: f32) -> FakeBoard {
    FakeBoard { hash, score }
}

fn history(max_size: usize, boards: &[(u128, f32)]) -> BoardHistory<FakeEntry> {
    let mut history = BoardHistory::with_max_size(max_size);
    for &(hash, score) in boards {
        history.add(&board(hash, score));
    }
    history
}

#[test]
fn default_cap_is_java_max_history_size() {
    assert_eq!(MAX_HISTORY_SIZE, 30);
    assert_eq!(BoardHistory::<FakeEntry>::new().max_size(), 30);
}

#[test]
fn same_hash_is_added_once() {
    let mut history = history(5, &[(1, 10.0)]);
    assert!(!history.add(&board(1, 99.0)));
    assert_eq!(history.len(), 1);
    assert_eq!(history.scores(), vec![10.0]);
}

#[test]
fn tie_with_the_worst_at_capacity_is_dropped() {
    let mut history = history(3, &[(1, 5.0), (2, 3.0), (3, 7.0)]);
    let tie = Counting::new(4, 3.0);
    assert!(!history.add(&tie));
    assert_eq!(history.len(), 3);
    assert!(!history.contains(4));
    assert_eq!(history.scores(), vec![5.0, 3.0, 7.0]);
    assert_eq!(tie.scores.get(), 1, "the score is computed once");
    assert_eq!(tie.entries.get(), 0, "a dropped board is never stored");
}

#[test]
fn strictly_better_evicts_the_first_worst() {
    // Two entries share the worst score 3.0; the first of them goes.
    let mut history = history(4, &[(1, 5.0), (2, 3.0), (3, 7.0), (4, 3.0)]);
    let better = Counting::new(5, 3.5);
    assert!(history.add(&better));
    assert_eq!(history.len(), 4);
    assert!(!history.contains(2));
    assert!(history.contains(4));
    assert_eq!(history.scores(), vec![5.0, 7.0, 3.0, 3.5]);
    assert_eq!(better.scores.get(), 1);
    assert_eq!(better.entries.get(), 1);
}

#[test]
fn below_capacity_adds_regardless_of_score() {
    let mut history = history(3, &[(1, 5.0)]);
    let worse = Counting::new(2, -100.0);
    assert!(history.add(&worse));
    assert_eq!(history.len(), 2);
    assert_eq!(worse.scores.get(), 1);
    assert_eq!(worse.entries.get(), 1);
}

#[test]
fn restore_3_returns_the_same_board_at_most_four_times() {
    let mut history = history(5, &[(1, 5.0), (2, 9.0), (3, 7.0)]);
    for count in 1..=4 {
        assert_eq!(history.restore(3), Some(board(2, 9.0)), "restore {count}");
    }
    // Board 2 has been restored 4 times (> 3): the next best comes next.
    assert_eq!(history.restore(3), Some(board(3, 7.0)));
    assert_eq!(history.restore_counts(), vec![4, 1, 0]);
}

#[test]
fn restore_with_no_eligible_board_is_none() {
    let mut history = history(5, &[(1, 5.0)]);
    assert_eq!(history.restore(1), Some(board(1, 5.0)));
    assert_eq!(history.restore(1), Some(board(1, 5.0)));
    assert_eq!(history.restore(1), None);
    assert_eq!(BoardHistory::<FakeEntry>::new().restore_best(), None);
}

#[test]
fn restore_zero_or_negative_is_unlimited() {
    let mut history = history(5, &[(1, 5.0), (2, 9.0)]);
    for _ in 0..10 {
        assert_eq!(history.restore(0), Some(board(2, 9.0)));
        assert_eq!(history.restore(-4), Some(board(2, 9.0)));
        assert_eq!(history.restore_best(), Some(board(2, 9.0)));
    }
}

#[test]
fn rank_is_insertion_order_until_a_restore_sorts() {
    let mut history = history(5, &[(1, 5.0), (2, 9.0), (3, 7.0), (4, 9.0)]);
    let ranks = |h: &BoardHistory<FakeEntry>| (1..=5).map(|x| h.rank(x)).collect::<Vec<_>>();
    assert_eq!(
        ranks(&history),
        vec![Some(1), Some(2), Some(3), Some(4), None]
    );
    history.restore_best();
    // Stable sort, best first: 2 (9.0) stays before 4 (9.0).
    assert_eq!(
        ranks(&history),
        vec![Some(4), Some(1), Some(3), Some(2), None]
    );
    assert_eq!(history.scores(), vec![9.0, 9.0, 7.0, 5.0]);
}

#[test]
fn max_score_starts_at_zero() {
    let all_negative = history(5, &[(1, -5.0), (2, -1.0)]);
    assert_eq!(all_negative.max_score(), 0.0);
    assert_eq!(BoardHistory::<FakeEntry>::new().max_score(), 0.0);
    assert_eq!(history(5, &[(1, -5.0), (2, 4.5)]).max_score(), 4.5);
}

#[test]
fn contains_remove_clear_len() {
    let mut history = history(5, &[(1, 5.0), (2, 9.0), (3, 7.0)]);
    assert!(history.contains(2));
    history.remove(2);
    assert!(!history.contains(2));
    assert_eq!(history.len(), 2);
    history.remove(42);
    assert_eq!(history.len(), 2);
    assert_eq!(history.rank(3), Some(2));
    history.clear();
    assert!(history.is_empty());
    assert_eq!(history.len(), 0);
}

#[test]
fn nan_scores_sort_first_as_in_java() {
    // Float.compare puts NaN above every number, so a descending sort puts
    // it first, a NaN with the sign bit included.
    let negative_nan = f32::from_bits(0xffc0_0000);
    let mut history = history(5, &[(1, 5.0), (2, negative_nan), (3, f32::INFINITY)]);
    let restored = history.restore_best().unwrap();
    assert_eq!(restored.hash, 2);
    assert_eq!(history.rank(3), Some(2));
    assert_eq!(history.rank(1), Some(3));
}

#[test]
fn signed_zero_scores_sort_positive_first() {
    let mut history = history(5, &[(1, -0.0), (2, 0.0)]);
    assert_eq!(history.restore_best().unwrap().hash, 2);
    assert_eq!(history.rank(1), Some(2));
}

// Java's `autoroute/BoardHistoryTest`.

/// A real board with the settings its history scores it with.
struct RealBoard<'a> {
    board: &'a RoutingBoard,
    settings: &'a RouterSettings,
}

/// The stored board: B01's snapshot and the hash of the board it came from.
struct SnapshotEntry {
    snapshot: BoardSnapshot,
    hash: u128,
}

impl HistoryEntry for SnapshotEntry {
    type Board = RoutingBoard;

    fn state_hash(&self) -> u128 {
        self.hash
    }

    fn restore(&self) -> RoutingBoard {
        self.snapshot.restore()
    }
}

impl HistorySource for RealBoard<'_> {
    type Entry = SnapshotEntry;

    fn state_hash(&self) -> u128 {
        self.board.state_hash()
    }

    fn router_score(&self) -> f32 {
        BoardStatistics::new(self.board).router_score(Some(self.settings))
    }

    fn to_entry(&self) -> SnapshotEntry {
        SnapshotEntry {
            snapshot: self.board.snapshot(),
            hash: self.board.state_hash(),
        }
    }
}

/// Java's `setUp`: the two boards and `SettingsMerger` over
/// `DefaultSettings`.
struct Setup {
    board1: RoutingBoard,
    board2: RoutingBoard,
    settings: RouterSettings,
}

impl Setup {
    fn board1(&self) -> RealBoard<'_> {
        RealBoard {
            board: &self.board1,
            settings: &self.settings,
        }
    }

    fn board2(&self) -> RealBoard<'_> {
        RealBoard {
            board: &self.board2,
            settings: &self.settings,
        }
    }
}

fn load(name: &str) -> RoutingBoard {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    load_design(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn setup() -> &'static Setup {
    static SETUP: OnceLock<Setup> = OnceLock::new();
    SETUP.get_or_init(|| {
        let ctx = ResolveContext { cores: 4 };
        Setup {
            board1: load("empty_board.dsn"),
            board2: load("Issue159-setonix_2hp-pcb.dsn"),
            settings: resolve(&[(Source::Defaults, default_patch(ctx.cores))], &ctx).settings,
        }
    })
}

#[test]
fn add_and_restore_board() {
    let s = setup();
    let mut history = BoardHistory::new();
    history.add(&s.board1());
    let restored = history.restore_best().expect("a restored board");
    assert!(!std::ptr::eq(&restored, &s.board1), "a new board");
    assert_eq!(
        restored.state_hash(),
        s.board1.state_hash(),
        "the restored board equals the original"
    );
}

#[test]
fn restore_best_board_from_multiple() {
    let s = setup();
    let mut history = BoardHistory::new();
    // board2 has unrouted connections, so it scores lower than the empty
    // board1.
    history.add(&s.board1());
    history.add(&s.board2());
    assert!(s.board1().router_score() > s.board2().router_score());
    let best = history.restore_best().expect("a restored board");
    assert_eq!(best.state_hash(), s.board1.state_hash(), "the best board");
}

#[test]
fn contains() {
    let s = setup();
    let mut history = BoardHistory::new();
    history.add(&s.board1());
    assert!(history.contains(s.board1.state_hash()));
    assert!(!history.contains(s.board2.state_hash()));
}

#[test]
fn clear() {
    let s = setup();
    let mut history = BoardHistory::new();
    history.add(&s.board1());
    history.add(&s.board2());
    assert_eq!(history.len(), 2);
    history.clear();
    assert_eq!(history.len(), 0, "empty after clear");
}

#[test]
fn size_cap_never_exceeds_max_history_size() {
    let s = setup();
    let mut history = BoardHistory::with_max_size(MAX_HISTORY_SIZE);
    history.add(&s.board1());
    history.add(&s.board2());
    history.add(&s.board1());
    assert!(history.len() <= MAX_HISTORY_SIZE);
}

#[test]
fn size_cap_evicts_worst_entry() {
    let s = setup();
    // A cap of 1: the better board1 fills it, and the worse board2 does not
    // evict it.
    let mut history = BoardHistory::with_max_size(1);
    history.add(&s.board1());
    assert_eq!(history.len(), 1);
    history.add(&s.board2());
    assert_eq!(history.len(), 1, "the size stays at the cap");
    let best = history.restore_best().expect("the best board is kept");
    assert_eq!(best.state_hash(), s.board1.state_hash(), "board1 is kept");
}
