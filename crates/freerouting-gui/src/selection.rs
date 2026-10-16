//! The selected board items (Java's `InspectedItemState` item list).
//!
//! A `BTreeSet` of [`ItemId`]s, so iteration is stable for lists and tests.
//! The selection is set by the Info lists (Select) and, later, by the
//! inspect tool (G05). The highlight is drawn by [`crate::overlays`].

use std::collections::BTreeSet;

use freerouting::board::items::ItemId;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub items: BTreeSet<ItemId>,
}

impl Selection {
    /// Replaces the selection by `ids`.
    pub fn set(&mut self, ids: impl IntoIterator<Item = ItemId>) {
        self.items = ids.into_iter().collect();
    }

    /// Adds `id`, or removes it when it is selected.
    pub fn toggle(&mut self, id: ItemId) {
        if !self.items.remove(&id) {
            self.items.insert(id);
        }
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn contains(&self, id: ItemId) -> bool {
        self.items.contains(&id)
    }

    /// The selected ids in set order (the engine orders ids highest first).
    pub fn ids(&self) -> Vec<ItemId> {
        self.items.iter().copied().collect()
    }

    /// Drops the ids that are not items of `board` any more (after an undo
    /// or a delete).
    pub fn retain_existing(&mut self, board: &freerouting::board::BasicBoard) {
        self.items.retain(|id| board.item(*id).is_some());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_toggle_clear() {
        let mut s = Selection::default();
        s.set([ItemId(5), ItemId(2), ItemId(5)]);
        assert_eq!(s.len(), 2);
        assert_eq!(s.ids().len(), 2);
        s.toggle(ItemId(2));
        assert!(!s.contains(ItemId(2)));
        s.toggle(ItemId(9));
        assert!(s.contains(ItemId(9)));
        s.clear();
        assert!(s.is_empty());
    }
}
