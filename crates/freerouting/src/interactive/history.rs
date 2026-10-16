//! Undo and redo (I03.5): Java's `GuiBoardHistoryController.undo/redo`
//! (`gui/workspace/controllers/GuiBoardHistoryController.java` at
//! `a773a70ec`).
//!
//! The editor calls these only in a menu mode (Java's `canChangeHistory`:
//! the current state is a `MenuState`); elsewhere the command is ignored
//! without a message. The snapshots are the ones the sessions take where
//! Java takes them (I00, "Snapshots where Java takes them").

use std::collections::BTreeSet;

use super::Status;
use crate::board::RoutingBoard;

/// What an undo or redo did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryStep {
    /// False if there was nothing to undo or redo.
    pub done: bool,
    /// The nets of the restored and cancelled items (Java's ratsnest
    /// updates), ascending.
    pub changed_nets: BTreeSet<i32>,
    /// `undo` / `no_more_undo_possible`, `redo` / `no_more_redo_possible`.
    pub status: Status,
}

/// Java `GuiBoardHistoryController.undo`.
pub fn undo(board: &mut RoutingBoard) -> HistoryStep {
    let mut changed_nets = BTreeSet::new();
    let done = board.undo(Some(&mut changed_nets));
    let key = if done {
        "undo"
    } else {
        "no_more_undo_possible"
    };
    HistoryStep {
        done,
        changed_nets,
        status: Status::new(key, []),
    }
}

/// Java `GuiBoardHistoryController.redo`.
pub fn redo(board: &mut RoutingBoard) -> HistoryStep {
    let mut changed_nets = BTreeSet::new();
    let done = board.redo(Some(&mut changed_nets));
    let key = if done {
        "redo"
    } else {
        "no_more_redo_possible"
    };
    HistoryStep {
        done,
        changed_nets,
        status: Status::new(key, []),
    }
}
