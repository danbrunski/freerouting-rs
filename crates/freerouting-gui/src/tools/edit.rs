//! Delete, Fix and Unfix of the selection (G05.4, decision G00 question 6).
//!
//! Java's inspect toolbar has no such buttons at the pin: the edits it
//! lists route to `GuiBoardLegacyEditActions`, whose methods are empty
//! (`docs/DIVERGENCES.md`, "Delete, Fix and Unfix of the selection"). Dan
//! accepted adding these three. The editor command that owns them (I03.6)
//! has not landed, so the engine's primitives are called here, in the order
//! I03.6 specifies; that command replaces these functions.
//!
//! - **Delete**: items whose deletion is forbidden (`is_deletion_forbidden`:
//!   items of components, user-fixed items, power planes) stay and the
//!   status is Java's `some_items_are_fixed_and_could_therefore_not_be_removed`;
//!   the rest are removed after one snapshot. Nothing removable: no
//!   snapshot, no edit.
//! - **Fix** / **Unfix**: the fixed state of every selected item, without a
//!   snapshot. `BasicBoard::set_fixed_state` and `unfix` change the item in
//!   place and do not save it for undo (Java's `Item.setFixedState` does not
//!   either), so these two edits are not undoable until I03.6 gives the
//!   engine an undoable form.

use std::collections::BTreeSet;

use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::board::structure::FixedState;

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

/// Removes the removable items of `ids`.
pub fn delete(board: &mut RoutingBoard, ids: &BTreeSet<ItemId>) -> EditOutcome {
    let (removable, refused): (Vec<ItemId>, Vec<ItemId>) = ids
        .iter()
        .copied()
        .filter(|id| board.basic().item(*id).is_some())
        .partition(|id| {
            board
                .basic()
                .item(*id)
                .is_some_and(|item| !board.basic().is_deletion_forbidden(item))
        });
    if removable.is_empty() {
        return EditOutcome {
            changed: 0,
            refused: refused.len(),
        };
    }
    board.generate_snapshot();
    board.remove_items(removable.iter().copied());
    EditOutcome {
        changed: removable.len(),
        refused: refused.len(),
    }
}

/// Fixes the items of `ids` (user fixed). Items that are fixed already are
/// not counted.
pub fn fix(board: &mut RoutingBoard, ids: &BTreeSet<ItemId>) -> EditOutcome {
    let mut changed = 0;
    for id in ids {
        let Some(item) = board.basic().item(*id) else {
            continue;
        };
        if item.fixed_state() != FixedState::UserFixed {
            board.set_fixed_state(*id, FixedState::UserFixed);
            changed += 1;
        }
    }
    EditOutcome {
        changed,
        refused: 0,
    }
}

/// Unfixes the items of `ids` (Java `Item.unfix`: not the system-fixed
/// ones). Items that are unfixed already are not counted.
pub fn unfix(board: &mut RoutingBoard, ids: &BTreeSet<ItemId>) -> EditOutcome {
    let mut changed = 0;
    for id in ids {
        let Some(item) = board.basic().item(*id) else {
            continue;
        };
        if matches!(
            item.fixed_state(),
            FixedState::UserFixed | FixedState::ShoveFixed
        ) {
            board.unfix(*id);
            changed += 1;
        }
    }
    EditOutcome {
        changed,
        refused: 0,
    }
}
