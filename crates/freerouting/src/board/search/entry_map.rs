//! The tree entries of each item, by item id (K13.9).
//!
//! A search tree looks the entries of an item up for every leaf a query
//! returns (`stored_shape`), about a hundred million times per autorouter pass
//! on CM5_MINIMA_3 with its track arcs as polylines. An ordered map costs a
//! tree descent per lookup; this map keeps the entries of ids below
//! [`DENSE_LIMIT`] in a vector indexed by id, and the rest in an ordered map.
//! Iteration follows [`ItemId`]'s order (highest id first), as the ordered map
//! did, so nothing that iterates sees a change.

use std::collections::BTreeMap;

use crate::board::items::ItemId;

/// Ids at or above this are kept in the ordered map (1 Mi slots).
const DENSE_LIMIT: usize = 1 << 20;

/// A map from item ids to `V`, a vector below [`DENSE_LIMIT`] and an ordered
/// map above it.
#[derive(Clone, Debug)]
pub(crate) struct ItemIdMap<V> {
    dense: Vec<Option<(ItemId, V)>>,
    sparse: BTreeMap<ItemId, V>,
}

impl<V> Default for ItemIdMap<V> {
    fn default() -> Self {
        ItemIdMap {
            dense: Vec::new(),
            sparse: BTreeMap::new(),
        }
    }
}

impl<V> ItemIdMap<V> {
    fn slot(id: ItemId) -> Option<usize> {
        usize::try_from(id.0).ok().filter(|&i| i < DENSE_LIMIT)
    }

    pub(crate) fn get(&self, id: &ItemId) -> Option<&V> {
        match Self::slot(*id) {
            Some(i) => self.dense.get(i)?.as_ref().map(|(_, v)| v),
            None => self.sparse.get(id),
        }
    }

    pub(crate) fn get_mut(&mut self, id: &ItemId) -> Option<&mut V> {
        match Self::slot(*id) {
            Some(i) => self.dense.get_mut(i)?.as_mut().map(|(_, v)| v),
            None => self.sparse.get_mut(id),
        }
    }

    pub(crate) fn contains_key(&self, id: &ItemId) -> bool {
        self.get(id).is_some()
    }

    pub(crate) fn insert(&mut self, id: ItemId, value: V) -> Option<V> {
        match Self::slot(id) {
            Some(i) => {
                if i >= self.dense.len() {
                    self.dense.resize_with(i + 1, || None);
                }
                self.dense[i].replace((id, value)).map(|(_, v)| v)
            }
            None => self.sparse.insert(id, value),
        }
    }

    pub(crate) fn remove(&mut self, id: &ItemId) -> Option<V> {
        match Self::slot(*id) {
            Some(i) => self.dense.get_mut(i)?.take().map(|(_, v)| v),
            None => self.sparse.remove(id),
        }
    }

    /// The entries in [`ItemId`] order: the ids above the vector first (the
    /// highest first), then the vector's from the highest index down, then the
    /// negative ids.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&ItemId, &V)> + '_ {
        let above = self.sparse.iter().take_while(|(id, _)| id.0 >= 0);
        let below = self.sparse.iter().skip_while(|(id, _)| id.0 >= 0);
        above
            .chain(
                self.dense
                    .iter()
                    .rev()
                    .filter_map(|slot| slot.as_ref().map(|(id, v)| (id, v))),
            )
            .chain(below)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iterates_in_item_id_order_across_the_vector_and_the_map() {
        let mut map = ItemIdMap::default();
        let ids = [5, 0, 1 << 21, 3, -1, (1 << 20) + 7, 1 << 19, -40];
        for id in ids {
            assert!(map.insert(ItemId(id), id).is_none());
        }
        assert_eq!(map.insert(ItemId(3), 33), Some(3));
        assert_eq!(map.remove(&ItemId(0)), Some(0));
        assert_eq!(map.remove(&ItemId(0)), None);
        assert!(!map.contains_key(&ItemId(0)));
        *map.get_mut(&ItemId(5)).unwrap() += 50;
        let mut reference: BTreeMap<ItemId, i32> = BTreeMap::new();
        for (id, v) in [(5, 55), (1 << 21, 1 << 21), (3, 33), (-1, -1)] {
            reference.insert(ItemId(id), v);
        }
        reference.insert(ItemId((1 << 20) + 7), (1 << 20) + 7);
        reference.insert(ItemId(1 << 19), 1 << 19);
        reference.insert(ItemId(-40), -40);
        let got: Vec<(ItemId, i32)> = map.iter().map(|(k, v)| (*k, *v)).collect();
        let want: Vec<(ItemId, i32)> = reference.iter().map(|(k, v)| (*k, *v)).collect();
        assert_eq!(got, want);
        assert_eq!(map.get(&ItemId(-1)), Some(&-1));
        assert_eq!(map.get(&ItemId(1 << 21)), Some(&(1 << 21)));
    }
}
