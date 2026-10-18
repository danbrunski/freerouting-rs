//! Board snapshots and deep copies with Java's restore semantics (B01.3).
//!
//! Java copies, compares and restores whole boards through serialization:
//! `BasicBoard.serialize/deserialize/clone`, `RoutingBoardUndoFacade.deepCopy`
//! and `BoardHistory`. A [`BoardSnapshot`] is the counterpart of the Java
//! byte array: it holds what Java serializes, which is the board's
//! non-`transient` fields, and no search trees.
//!
//! [`BoardSnapshot::restore`] builds a board the way `BasicBoard.readObject`
//! does (`BasicBoard.java:1508-1520`): a new `SearchTreeManager` with only the
//! default tree (`ShapeSearchTree`, class 0, clearance compensation off), into
//! which every item of the item list is inserted, highest id first, with all
//! its shapes in index order. The transient state starts fresh: no
//! normalize suppression, revision 0, no changed area, no shove-failing
//! obstacle, an empty edge-pin-net cache, an unbuilt routing ledger, the
//! journal off and an empty log. The rebuild order decides the tree shape and
//! so the work counts of every later search (decision D5).
//!
//! A restore drops clearance compensation and every compensated tree, as
//! Java's does, even when the GUI turned compensation on (kept on purpose in
//! the parity phase; B00 open question 3).
//!
//! [`RoutingBoard::deep_copy`] is `snapshot().restore()` followed by
//! [`RoutingBoard::clear_transient_autoroute_state`]. Java's copy also clears
//! the items' autoroute info and finishes the autoroute engine; in the port
//! both live in the autoroute session, outside the board, so there is nothing
//! to clear.
//!
//! **Repeated restores (P02.4).** A restore is a pure function of the
//! snapshot: the same snapshot always builds the same board, tree shape and
//! work-meter count included. So a snapshot that is restored again (the
//! optimizer restores its pass baseline once per candidate, O06.4) keeps a
//! clone of the board its second restore built in a memo shared by the
//! snapshot's clones, and every later restore clones that board instead of
//! rebuilding the default tree. A clone of a freshly restored board is that board: the same
//! items, undo structure, id generator, tree (shape, leaves, entries), meter
//! count and fresh transient state (`Clone` keeps all of them; only the
//! meter's identity is new, as on every board). The first restore builds
//! without filling the memo, so a snapshot restored once (history, strict
//! DRC, `deep_copy`) costs and keeps nothing more than before.
//! [`BoardSnapshot::rebuild`] always builds; `tests/snapshot_memo.rs` pins
//! the two equal.
//!
//! Java's restore cannot share a tree between a fixed base and the wiring:
//! `readObject` inserts every item highest id first, so wiring (newer, higher
//! ids) and pins interleave in one tree whose shape decides the work counts
//! (D5). The memo is what an immutable base buys under that rule.
//!
//! `Clone` on [`BasicBoard`] and [`RoutingBoard`] is *not* a Java copy: it
//! keeps the search trees, their compensation, the transient state and the
//! revision as they are. It is for tests and the viewer. Routing, history
//! and optimizer code copies boards with [`RoutingBoard::deep_copy`] or a
//! [`BoardSnapshot`].

use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use freerouting_geometry::IntBox;

use crate::autoroute::failure_log::FailureLog;
use crate::board::basic_board::BasicBoard;
use crate::board::items::{Item, ItemId};
use crate::board::routing_board::RoutingBoard;
use crate::board::search::ShapeMemo;
use crate::board::state::Communication;
use crate::board::structure::{Components, LayerStructure};
use crate::datastructures::UndoableObjects;
use crate::library::BoardLibrary;
use crate::rules::BoardRules;

/// What Java serializes of a board: its non-`transient` fields.
///
/// The item list keeps its whole undo structure, the components their undo
/// list, and `communication` the id generator, so a restored board undoes,
/// redoes and numbers new items exactly as the original would. Java's
/// `RoutingBoard.failureLog` is serialized too, so the failure log comes
/// back with a restore.
#[derive(Clone, Debug)]
pub struct BoardSnapshot {
    pub(crate) items: UndoableObjects<ItemId, Item>,
    pub(crate) components: Arc<Components>,
    pub(crate) rules: Arc<BoardRules>,
    pub(crate) library: Arc<BoardLibrary>,
    pub(crate) layer_structure: Arc<LayerStructure>,
    pub(crate) communication: Communication,
    pub(crate) bounding_box: IntBox,
    pub(crate) max_trace_half_width: i32,
    pub(crate) min_trace_half_width: i32,
    pub(crate) pre_existing_clearance_violations_count: usize,
    /// `RoutingBoard.failureLog` (non-transient in Java).
    pub(crate) failure_log: FailureLog,
    /// The board built by the second restore (see the module docs), shared
    /// by the snapshot's clones. Nothing may change the fields above once a
    /// snapshot exists; they are only read.
    pub(crate) memo: Arc<RestoreMemo>,
    /// The board's trace-shape memo (P02.5), handed to every restored
    /// board.
    pub(crate) shape_memo: Arc<ShapeMemo>,
    /// The room order and work unit of the board's trees (P04), which a
    /// restored board keeps.
    pub(crate) search_config: crate::board::search::SearchConfig,
}

/// The restore memo of a [`BoardSnapshot`] (P02.4).
#[derive(Default)]
pub(crate) struct RestoreMemo {
    /// How many restores were asked for.
    restores: AtomicUsize,
    /// A clone of the board the second restore built, cloned by every later
    /// one.
    board: OnceLock<RoutingBoard>,
}

impl fmt::Debug for RestoreMemo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RestoreMemo")
            .field("restores", &self.restores.load(Ordering::Relaxed))
            .field("memoized", &self.board.get().is_some())
            .finish()
    }
}

impl BoardSnapshot {
    /// A board built from this snapshot the way Java's `readObject` builds
    /// one (see the module docs).
    ///
    /// From the third restore of a snapshot (or of its clones) on, this
    /// clones the board the second one built (P02.4, module docs).
    pub fn restore(&self) -> RoutingBoard {
        if self.memo.restores.fetch_add(1, Ordering::Relaxed) == 0 {
            return self.rebuild();
        }
        // The memo keeps a clone, which holds no spare capacity; the restore
        // that fills it returns the board it built.
        let mut built = None;
        let memoized = self.memo.board.get_or_init(|| {
            let board = self.rebuild();
            let compact = board.clone();
            built = Some(board);
            compact
        });
        built.unwrap_or_else(|| memoized.clone())
    }

    /// [`restore`](Self::restore) without the memo: always builds the board
    /// from the item list.
    pub fn rebuild(&self) -> RoutingBoard {
        let mut board = RoutingBoard::from_basic(BasicBoard::from_snapshot(self));
        *board.failure_log_mut() = self.failure_log.clone();
        board
    }

    /// Whether a later [`restore`](Self::restore) clones a memoized board.
    pub fn is_memoized(&self) -> bool {
        self.memo.board.get().is_some()
    }

    /// The number of items in the snapshot.
    pub fn item_count(&self) -> usize {
        self.items.len()
    }
}

impl RoutingBoard {
    /// The board's serialized state (Java `serialize(false)`), without
    /// search trees.
    pub fn snapshot(&self) -> BoardSnapshot {
        let mut snapshot = self.basic().snapshot();
        snapshot.failure_log = self.failure_log().clone();
        snapshot
    }

    /// A copy of the board with Java's deep-copy semantics
    /// (`RoutingBoardUndoFacade.deepCopy`): `snapshot().restore()`, then
    /// [`clear_transient_autoroute_state`](Self::clear_transient_autoroute_state).
    pub fn deep_copy(&self) -> RoutingBoard {
        let mut copy = self.snapshot().restore();
        copy.clear_transient_autoroute_state();
        copy
    }
}
