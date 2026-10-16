//! The autorouter's change log (B02.8): which items changed, at Java's hook
//! points and in Java's order.
//!
//! Java invalidates the autorouter's rooms through
//! `RoutingBoard.additionalUpdateAfterChange` (`RoutingBoard.java:96-121`),
//! which does nothing unless an engine maintains its database, which only
//! Java's retained mode does; A00 doesn't port retained mode. So parity needs
//! only the side table ([`ItemSideTable`](crate::board::side_table::ItemSideTable));
//! this log serves a later fast mode. It is off by default and costs one
//! branch per hook when off. The hook points:
//! - insert: after the tree insert (`BoardItemRepository.java:162-165`);
//! - remove: before the tree remove (`:206-207`);
//! - combine: once per successful combine (`PolylineTrace.java:189`);
//! - split: each piece, when there is more than one (`:687-691`);
//! - change: before `saveForUndo` (`:944`);
//! - the fast cutout: the old trace (`ShapeTraceEntries.java:113`; O03 calls
//!   [`BasicBoard::note_autoroute_cutout`]).
//!
//! It also records the items whose autoroute info Java clears: the items
//! undo and redo restore (`BasicBoard.applyUndoRedoSideEffects`), and the
//! `clearDerivedData` points: `reinsertTreeItems` (each item twice, as Java
//! clears it before the reinsertion and again in `insertAllBoardItems`), and
//! the geometry transforms and `assignComponentId`, which call
//! [`BasicBoard::note_autoroute_info_cleared`] (B05 ports them).

use crate::board::basic_board::BasicBoard;
use crate::board::items::ItemId;

/// One record of the autoroute change log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutorouteChange {
    Inserted(ItemId),
    Removed(ItemId),
    Combined(ItemId),
    SplitPiece(ItemId),
    Changed(ItemId),
    CutOut(ItemId),
    /// Java clears the item's autoroute info (`clearAutorouteInfo`,
    /// `clearDerivedData`).
    InfoCleared(ItemId),
}

impl BasicBoard {
    /// Switches the autoroute change log on or off; off drops the records.
    pub fn set_autoroute_change_log(&mut self, on: bool) {
        self.autoroute_log = on.then(Vec::new);
    }

    /// True if the autoroute change log is on.
    pub fn is_autoroute_change_log_on(&self) -> bool {
        self.autoroute_log.is_some()
    }

    /// The records since the last call; the log stays on.
    pub fn take_autoroute_changes(&mut self) -> Vec<AutorouteChange> {
        self.autoroute_log
            .as_mut()
            .map(std::mem::take)
            .unwrap_or_default()
    }

    pub(crate) fn log_autoroute(&mut self, change: AutorouteChange) {
        if let Some(log) = &mut self.autoroute_log {
            log.push(change);
        }
    }

    /// Records that the fast cutout replaced trace `id` (Java
    /// `ShapeTraceEntries.fastCutoutTrace`, `:113`); O03 calls it.
    pub fn note_autoroute_cutout(&mut self, id: ItemId) {
        self.log_autoroute(AutorouteChange::CutOut(id));
    }

    /// Records that Java clears the autoroute info of item `id`
    /// (`clearDerivedData` after a geometry transform or
    /// `assignComponentId`).
    pub fn note_autoroute_info_cleared(&mut self, id: ItemId) {
        self.log_autoroute(AutorouteChange::InfoCleared(id));
    }
}
