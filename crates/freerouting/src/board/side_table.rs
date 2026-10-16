//! Per-item scratch data of the autorouter (B02.8).
//!
//! Java keeps the autorouter's per-item data on the item (`ItemAutorouteInfo`,
//! `Via.getAutorouteDrillInfo`) and drops it with `clearAutorouteInfo`. The
//! port keeps it outside the board, in an [`ItemSideTable`] the A series'
//! session owns: dense by item id, with a generation stamp, so dropping all
//! entries (Java's `clearAllItemTemporaryAutorouteData`) is O(1).

use crate::board::items::ItemId;

/// A table of `T` per item id, cleared in O(1).
#[derive(Clone, Debug)]
pub struct ItemSideTable<T> {
    generation: u32,
    slots: Vec<Option<(u32, T)>>,
}

impl<T> Default for ItemSideTable<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> ItemSideTable<T> {
    pub fn new() -> Self {
        ItemSideTable {
            generation: 0,
            slots: Vec::new(),
        }
    }

    fn index(id: ItemId) -> Option<usize> {
        usize::try_from(id.0).ok()
    }

    /// The entry of `id`, if it was set since the last [`clear`](Self::clear).
    pub fn get(&self, id: ItemId) -> Option<&T> {
        match self.slots.get(Self::index(id)?)? {
            Some((generation, value)) if *generation == self.generation => Some(value),
            _ => None,
        }
    }

    /// The entry of `id`, mutable.
    pub fn get_mut(&mut self, id: ItemId) -> Option<&mut T> {
        let generation = self.generation;
        match self.slots.get_mut(Self::index(id)?)? {
            Some((g, value)) if *g == generation => Some(value),
            _ => None,
        }
    }

    pub fn contains(&self, id: ItemId) -> bool {
        self.get(id).is_some()
    }

    /// Sets the entry of `id`. A negative id is ignored.
    pub fn insert(&mut self, id: ItemId, value: T) {
        let Some(index) = Self::index(id) else {
            return;
        };
        if index >= self.slots.len() {
            self.slots.resize_with(index + 1, || None);
        }
        self.slots[index] = Some((self.generation, value));
    }

    /// The entry of `id`, set to `make()` first if there is none.
    pub fn get_or_insert_with(&mut self, id: ItemId, make: impl FnOnce() -> T) -> Option<&mut T> {
        if !self.contains(id) {
            self.insert(id, make());
        }
        self.get_mut(id)
    }

    /// Drops the entry of `id` (Java `Item.clearAutorouteInfo`).
    pub fn remove(&mut self, id: ItemId) -> Option<T> {
        let generation = self.generation;
        let slot = self.slots.get_mut(Self::index(id)?)?;
        match slot.take() {
            Some((g, value)) if g == generation => Some(value),
            _ => None,
        }
    }

    /// Drops every entry in O(1). After 2^32 clears the slots are wiped, so
    /// an old stamp never comes back to life.
    pub fn clear(&mut self) {
        if self.generation == u32::MAX {
            self.slots.clear();
            self.generation = 0;
        } else {
            self.generation += 1;
        }
    }
}
