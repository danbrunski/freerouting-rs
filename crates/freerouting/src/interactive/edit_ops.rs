//! Delete and fix/unfix of the selection (I03.6, decision 1 of I00, accepted
//! by Dan 2026-10-16).
//!
//! Java's GUI has these edits disabled at `a773a70ec`: the inspect toolbar
//! has no buttons for them and `GuiBoardLegacyEditActions` makes the menu
//! entries no-ops. They are enabled here by decision (DIVERGENCES,
//! "Interactive editor").
//!
//! - [`delete_selection`]: one snapshot; items whose deletion is forbidden
//!   (items of components, user-fixed items, power planes) stay and the
//!   status is Java's `some_items_are_fixed_and_could_therefore_not_be_removed`;
//!   the rest go through `remove_items_and_pull_tight` with the tidy width
//!   and accuracy when push is on, else `remove_items` (the pattern of
//!   `RouteState.cancel`). Nothing removable: no snapshot, no change.
//! - [`fix_selection`]: one snapshot, then every item that changes is saved
//!   for undo before its fixed state is set, so one undo restores the
//!   states. Java's `Item.setFixedState` and `unfix` change the item without
//!   saving it, so the board's own [`set_fixed_state`] and [`unfix`] are not
//!   undoable; this is the undoable form. Fix makes an item user fixed;
//!   unfix makes it unfixed unless it is system fixed (Java `Item.unfix`).
//!
//! [`set_fixed_state`]: crate::board::BasicBoard::set_fixed_state
//! [`unfix`]: crate::board::BasicBoard::unfix

use std::collections::BTreeSet;

use super::Status;
use super::settings::EditSettings;
use crate::board::items::ItemId;
use crate::board::structure::FixedState;
use crate::board::{BasicBoard, RoutingBoard};

/// What an edit of the selection did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EditOutcome {
    /// Items removed or changed.
    pub changed: usize,
    /// Items kept because they are fixed (delete only).
    pub refused: usize,
    /// `some_items_are_fixed_and_could_therefore_not_be_removed` when
    /// `refused > 0`.
    pub status: Option<Status>,
}

/// Removes the removable items of `selection` after one snapshot.
pub fn delete_selection(
    board: &mut RoutingBoard,
    settings: &EditSettings,
    selection: &BTreeSet<ItemId>,
) -> EditOutcome {
    let (removable, refused): (Vec<ItemId>, Vec<ItemId>) = selection
        .iter()
        .copied()
        .filter(|id| board.item(*id).is_some())
        .partition(|id| {
            board
                .item(*id)
                .is_some_and(|item| !board.is_deletion_forbidden(item) && !item.is_user_fixed())
        });
    let status = (!refused.is_empty()).then(|| {
        Status::new(
            "some_items_are_fixed_and_could_therefore_not_be_removed",
            [],
        )
    });
    if !removable.is_empty() {
        board.generate_snapshot();
        if settings.push_enabled {
            board.remove_items_and_pull_tight(
                &removable,
                settings.trace_tidy_width,
                settings.pull_tight_accuracy,
            );
        } else {
            board.remove_items(removable.iter().copied());
        }
    }
    EditOutcome {
        changed: removable.len(),
        refused: refused.len(),
        status,
    }
}

/// Fixes (`fixed`) or unfixes the items of `selection` after one snapshot,
/// undoably. Items already in the target state are not counted, and with
/// none to change no snapshot is taken.
pub fn fix_selection(
    board: &mut RoutingBoard,
    selection: &BTreeSet<ItemId>,
    fixed: bool,
) -> EditOutcome {
    let target = |state: FixedState| {
        if fixed {
            FixedState::UserFixed
        } else if state == FixedState::SystemFixed {
            state
        } else {
            FixedState::Unfixed
        }
    };
    let changing: Vec<ItemId> = selection
        .iter()
        .copied()
        .filter(|id| {
            board
                .item(*id)
                .is_some_and(|item| target(item.fixed_state()) != item.fixed_state())
        })
        .collect();
    if !changing.is_empty() {
        board.generate_snapshot();
        for &id in &changing {
            let state = board
                .item(id)
                .map_or(FixedState::Unfixed, |i| target(i.fixed_state()));
            set_fixed_state_undoable(board, id, state);
        }
    }
    EditOutcome {
        changed: changing.len(),
        refused: 0,
        status: None,
    }
}

/// Sets the fixed state of item `id`, saving it for undo first.
pub fn set_fixed_state_undoable(board: &mut BasicBoard, id: ItemId, state: FixedState) {
    board.items.save_for_undo(&id);
    board.set_fixed_state(id, state);
}
