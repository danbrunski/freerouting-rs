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
//! `Clone` on [`BasicBoard`] and [`RoutingBoard`] is *not* a Java copy: it
//! keeps the search trees, their compensation, the transient state and the
//! revision as they are. It is for tests and the viewer. Routing, history
//! and optimizer code copies boards with [`RoutingBoard::deep_copy`] or a
//! [`BoardSnapshot`].

use std::sync::Arc;

use freerouting_geometry::IntBox;

use crate::autoroute::failure_log::FailureLog;
use crate::board::basic_board::BasicBoard;
use crate::board::items::{Item, ItemId};
use crate::board::routing_board::RoutingBoard;
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
}

impl BoardSnapshot {
    /// A board built from this snapshot the way Java's `readObject` builds
    /// one (see the module docs).
    pub fn restore(&self) -> RoutingBoard {
        let mut board = RoutingBoard::from_basic(BasicBoard::from_snapshot(self));
        *board.failure_log_mut() = self.failure_log.clone();
        board
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
