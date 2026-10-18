//! Delete, Fix and Unfix of the selection (G05.4, decision G00 question 6).
//!
//! Java's inspect toolbar has no such buttons at the pin: the edits it
//! lists route to `GuiBoardLegacyEditActions`, whose methods are empty
//! (`docs/DIVERGENCES.md`, "Delete, Fix and Unfix of the selection"). The owner
//! accepted adding these three. The engine owns them (I03.6,
//! `freerouting::interactive::{delete_selection, fix_selection}`); these
//! functions only map their outcome for the GUI.
//!
//! - **Delete**: items whose deletion is forbidden (`is_deletion_forbidden`:
//!   items of components, user-fixed items, power planes) stay and the
//!   status is Java's `some_items_are_fixed_and_could_therefore_not_be_removed`;
//!   the rest are removed after one snapshot. Nothing removable: no
//!   snapshot, no edit. The GUI has no push setting yet, so the removal is
//!   the plain one (the engine's push-off path); G09 passes the user's
//!   settings.
//! - **Fix** / **Unfix**: the fixed state of every selected item after one
//!   snapshot, undoably (the engine saves each item for undo first).

use std::collections::BTreeSet;

use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::interactive::{EditSettings, delete_selection, fix_selection};

/// What an edit of the selection did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EditOutcome {
    /// Items removed or changed.
    pub changed: usize,
    /// Items that could not be removed because they are fixed.
    pub refused: usize,
}

impl EditOutcome {
    /// True if the board changed.
    pub fn edited(&self) -> bool {
        self.changed > 0
    }
}

impl From<freerouting::interactive::EditOutcome> for EditOutcome {
    fn from(outcome: freerouting::interactive::EditOutcome) -> Self {
        EditOutcome {
            changed: outcome.changed,
            refused: outcome.refused,
        }
    }
}

/// Removes the removable items of `ids`.
pub fn delete(board: &mut RoutingBoard, ids: &BTreeSet<ItemId>) -> EditOutcome {
    let settings = EditSettings {
        push_enabled: false,
        ..EditSettings::for_board(board)
    };
    delete_selection(board, &settings, ids).into()
}

/// Fixes the items of `ids` (user fixed). Items that are fixed already are
/// not counted.
pub fn fix(board: &mut RoutingBoard, ids: &BTreeSet<ItemId>) -> EditOutcome {
    fix_selection(board, ids, true).into()
}

/// Unfixes the items of `ids` (Java `Item.unfix`: not the system-fixed
/// ones). Items that are unfixed already are not counted.
pub fn unfix(board: &mut RoutingBoard, ids: &BTreeSet<ItemId>) -> EditOutcome {
    fix_selection(board, ids, false).into()
}
